//! Authentification : mots de passe, jetons, session par cookie, frein des
//! connexions. Les routes sont dans `routes/auth.rs`.
//!
//! - **Mots de passe** : Argon2id aux réglages par défaut de la crate
//!   (`Argon2::default()` : 19 Mio, 2 passes, 1 fil), écrits au format PHC.
//!   Le calcul se fait hors des fils des requêtes (`spawn_blocking`), et
//!   [`MAX_CONCURRENT_HASHES`] à la fois au plus. Un mot de passe n'est jamais
//!   journalisé ni renvoyé ; les types qui en portent un n'ont pas de `Debug`.
//! - **Jetons** de session et d'invitation : 32 octets du générateur du
//!   système, donnés une fois en base64url ; la base ne garde que leur
//!   SHA-256 ([`new_token`], [`token_hash`]).
//! - **Session** : le cookie [`SESSION_COOKIE`] (`HttpOnly`, `SameSite=Lax`,
//!   `Path=/`, 30 jours, `Secure` quand l'adresse publique est en `https`)
//!   porte le jeton. [`AuthUser`] en tire le compte, sinon `401
//!   unauthenticated`.
//! - **Frein des connexions** ([`LoginThrottle`]) : après
//!   [`LOGIN_MAX_FAILURES`] échecs pour une même adresse en
//!   [`LOGIN_WINDOW_MS`], tout est refusé jusqu'à la fin de la fenêtre. Le
//!   compteur est en mémoire : il repart de zéro au redémarrage.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use anyhow::Context;
use argon2::Argon2;
use argon2::password_hash::{self, PasswordHasher, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cookie::time::Duration;
use cookie::{Cookie, SameSite};
use mementum_store::{Backup, Store, User};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::error::ApiError;
use crate::types::ErrorCode;
use crate::{AppState, config, now_ms};

/// Nom du cookie de session.
pub const SESSION_COOKIE: &str = "mementum_session";

/// Durée d'une session, en millisecondes : 30 jours. L'échéance glisse avec
/// l'usage (`Store::session_user`).
pub const SESSION_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Longueur minimale d'un mot de passe, en caractères Unicode.
pub const PASSWORD_MIN_CHARS: usize = 10;

/// Longueur maximale d'un mot de passe, en caractères Unicode.
pub const PASSWORD_MAX_CHARS: usize = 200;

/// Longueur maximale du nom d'un compte, en caractères Unicode.
pub const MAX_NAME_CHARS: usize = 100;

/// Longueur maximale d'une adresse e-mail, en caractères (RFC 5321).
pub const MAX_EMAIL_CHARS: usize = 254;

/// Échecs de connexion permis pour une même adresse dans une fenêtre.
pub const LOGIN_MAX_FAILURES: u32 = 10;

/// Durée de la fenêtre du frein des connexions, en millisecondes : 15
/// minutes, à partir du premier échec compté.
pub const LOGIN_WINDOW_MS: i64 = 15 * 60 * 1000;

/// Calculs Argon2 menés de front, au plus : chacun prend 19 Mio et un cœur.
/// Les suivants attendent leur tour, pour qu'une rafale de connexions ne
/// prenne ni toute la mémoire ni tous les fils bloquants.
pub const MAX_CONCURRENT_HASHES: usize = 4;

/// Adresses suivies à la fois par le frein des connexions, au plus : la
/// mémoire du compteur est bornée (moins de 1 Mo), même sous une rafale
/// d'adresses toutes différentes.
const LOGIN_TRACKED_MAX: usize = 10_000;

/// Octets d'un jeton.
const TOKEN_BYTES: usize = 32;

/// Caractères d'un jeton écrit en base64url sans remplissage.
const TOKEN_CHARS: usize = 43;

/// Hachage vérifié quand il n'y a pas de hachage à vérifier (adresse
/// inconnue, compte sans mot de passe), pour que la réponse coûte la même
/// vérification Argon2 qu'un mot de passe faux. C'est le hachage, aux
/// réglages de `Argon2::default()`, de 32 octets aléatoires que personne n'a
/// gardés (le test `dummy_hash_costs_a_real_verification` vérifie les
/// réglages) ; et le résultat de cette vérification n'ouvre jamais rien.
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$JRXJwnKKEb49hIMZKPYmgw$Qzd9lAJI/sFCOOE5OGi1oKGQSQFISt8fJd3elw7Ucls";

static HASHING: Semaphore = Semaphore::const_new(MAX_CONCURRENT_HASHES);

/// Mène un calcul Argon2 sur un fil bloquant, quand vient son tour.
async fn argon2_task<T: Send + 'static>(
    work: impl FnOnce() -> password_hash::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    // Le sémaphore n'est jamais fermé. Le tour est rendu quand le calcul
    // finit, même si la requête qui l'attendait a été abandonnée.
    let turn = HASHING.acquire().await.context("sémaphore Argon2 fermé")?;
    tokio::task::spawn_blocking(move || {
        let _turn = turn;
        work()
    })
    .await
    .context("calcul Argon2 interrompu")?
    .map_err(|e| anyhow::anyhow!("calcul Argon2 : {e}"))
}

/// Hache un mot de passe en Argon2id, avec un sel aléatoire : rend la chaîne
/// PHC (`$argon2id$v=19$m=19456,t=2,p=1$<sel>$<hachage>`) à garder en base.
pub async fn hash_password(password: String) -> anyhow::Result<String> {
    argon2_task(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hash| hash.to_string())
    })
    .await
}

/// Le mot de passe est-il celui de ce hachage PHC ? Sans hachage (adresse
/// inconnue, compte sans mot de passe) : faux, après la même vérification
/// Argon2 que pour un mot de passe faux.
pub async fn verify_password(password: String, hash: Option<String>) -> anyhow::Result<bool> {
    argon2_task(move || verify(password.as_bytes(), hash.as_deref(), DUMMY_HASH)).await
}

/// Vérifie `password` contre `hash` ; sans hachage, contre `dummy`, et le
/// résultat est faux quoi que dise la vérification : même le mot de passe du
/// hachage factice n'ouvre rien.
fn verify(password: &[u8], hash: Option<&str>, dummy: &str) -> password_hash::Result<bool> {
    match Argon2::default().verify_password(password, hash.unwrap_or(dummy)) {
        Ok(()) => Ok(hash.is_some()),
        Err(password_hash::Error::PasswordInvalid) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Un jeton neuf : 32 octets aléatoires. Rend le jeton écrit en base64url
/// (43 caractères, sans remplissage), à donner une seule fois à son porteur,
/// et son SHA-256, seul gardé en base.
///
/// Panique si le générateur aléatoire du système ne répond pas : il n'y a
/// alors aucun jeton sûr à donner.
pub fn new_token() -> (String, [u8; 32]) {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).expect("générateur aléatoire du système indisponible");
    (URL_SAFE_NO_PAD.encode(bytes), Sha256::digest(bytes).into())
}

/// Le SHA-256 sous lequel la base garde ce jeton ; `None` si le texte n'est
/// pas un jeton (32 octets en base64url, sans remplissage, sous leur seule
/// écriture canonique).
pub fn token_hash(token: &str) -> Option<[u8; 32]> {
    if token.len() != TOKEN_CHARS {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(token).ok()?;
    (bytes.len() == TOKEN_BYTES).then(|| Sha256::digest(&bytes).into())
}

/// La forme sous laquelle une adresse e-mail est gardée, cherchée et
/// comptée : rognée et en minuscules, comme dans le store.
pub(crate) fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// `email` (déjà rognée) a-t-elle la forme d'une adresse ? Une seule `@`,
/// quelque chose de chaque côté, ni blanc ni caractère de contrôle, 254
/// caractères au plus. Rien de plus : sans SMTP, l'adresse n'est jamais
/// vérifiée, et elle ne donne aucun droit (Ruling R47).
pub(crate) fn email_is_valid(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && email.chars().count() <= MAX_EMAIL_CHARS
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Le nom d'un compte, rogné : 1 à [`MAX_NAME_CHARS`] caractères, sinon
/// `422 invalid_name`.
pub(crate) fn account_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidName,
            format!("le nom a 1 à {MAX_NAME_CHARS} caractères"),
        ));
    }
    Ok(name.to_string())
}

/// Le texte de la règle des mots de passe.
fn password_rule() -> String {
    format!("le mot de passe a {PASSWORD_MIN_CHARS} à {PASSWORD_MAX_CHARS} caractères")
}

/// Un mot de passe à enregistrer : [`PASSWORD_MIN_CHARS`] à
/// [`PASSWORD_MAX_CHARS`] caractères, espaces comprises (il n'est pas
/// rogné), sinon `422 invalid_password`.
pub(crate) fn check_password(password: &str) -> Result<(), ApiError> {
    let chars = password.chars().count();
    if (PASSWORD_MIN_CHARS..=PASSWORD_MAX_CHARS).contains(&chars) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidPassword,
            password_rule(),
        ))
    }
}

/// L'en-tête `Set-Cookie` qui pose le cookie de session.
pub(crate) fn session_cookie(token: &str, secure: bool) -> HeaderValue {
    set_cookie(token, Duration::milliseconds(SESSION_TTL_MS), secure)
}

/// L'en-tête `Set-Cookie` qui efface le cookie de session : la même
/// étiquette, vide et échue.
pub(crate) fn removal_cookie(secure: bool) -> HeaderValue {
    set_cookie("", Duration::ZERO, secure)
}

fn set_cookie(value: &str, max_age: Duration, secure: bool) -> HeaderValue {
    let mut cookie = Cookie::build((SESSION_COOKIE, value))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(max_age);
    if secure {
        cookie = cookie.secure(true);
    }
    HeaderValue::from_str(&cookie.build().to_string())
        .expect("un jeton en base64url s'écrit dans une en-tête")
}

/// Le SHA-256 du jeton que porte le cookie de session de la requête : celui
/// du premier cookie de ce nom. `None` s'il n'y en a pas, ou si sa valeur
/// n'est pas un jeton.
pub(crate) fn session_hash(headers: &HeaderMap) -> Option<[u8; 32]> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(Cookie::split_parse)
        .filter_map(Result::ok)
        .find(|cookie| cookie.name() == SESSION_COOKIE)
        .and_then(|cookie| token_hash(cookie.value()))
}

/// Le compte de la session : cookie → session → compte, sinon `401
/// unauthenticated`. À demander en argument d'une route qui exige une
/// session ; la base est relue à chaque appel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub instance_admin: bool,
}

impl From<User> for AuthUser {
    fn from(user: User) -> AuthUser {
        AuthUser {
            id: user.id,
            email: user.email,
            name: user.name,
            instance_admin: user.instance_admin,
        }
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<AuthUser, ApiError> {
        let hash = session_hash(&parts.headers).ok_or_else(ApiError::unauthenticated)?;
        state
            .store
            .session_user(hash, now_ms(), SESSION_TTL_MS)
            .await
            .map_err(|e| ApiError::internal(&e))?
            .map(AuthUser::from)
            .ok_or_else(ApiError::unauthenticated)
    }
}

/// Une tentative refusée par le frein des connexions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Throttled {
    /// Secondes à attendre : la fin de la fenêtre.
    pub(crate) retry_after: u64,
    /// C'est le premier refus de cette fenêtre (à journaliser une fois).
    pub(crate) first: bool,
}

/// La fenêtre d'une adresse.
struct Window {
    /// Millisecondes Unix de la première tentative comptée.
    started: i64,
    /// Tentatives comptées depuis, sans réussite.
    failures: u32,
    /// Une tentative a déjà été refusée dans cette fenêtre.
    refused: bool,
}

/// Le frein des connexions : après [`LOGIN_MAX_FAILURES`] échecs pour une
/// même adresse (rognée, en minuscules) dans une fenêtre de
/// [`LOGIN_WINDOW_MS`], toute tentative pour cette adresse est refusée
/// jusqu'à la fin de la fenêtre, bon mot de passe compris. Une réussite
/// remet le compteur à zéro.
///
/// Une tentative est comptée comme un échec dès qu'elle commence, et
/// décomptée si elle réussit : des tentatives simultanées ne dépassent pas le
/// seuil. Le compteur ne garde pas les adresses, seulement leur SHA-256, et
/// il suit [`LOGIN_TRACKED_MAX`] adresses au plus : au-delà, la plus ancienne
/// fenêtre est oubliée.
#[derive(Default)]
pub(crate) struct LoginThrottle {
    windows: Mutex<HashMap<[u8; 32], Window>>,
}

impl LoginThrottle {
    /// Compte une tentative pour `email` à `now` (millisecondes Unix), ou la
    /// refuse si l'adresse a épuisé ses échecs.
    pub(crate) fn attempt(&self, email: &str, now: i64) -> Result<(), Throttled> {
        let key = throttle_key(email);
        let mut windows = self.windows.lock().unwrap_or_else(PoisonError::into_inner);
        if !windows.contains_key(&key) && windows.len() >= LOGIN_TRACKED_MAX {
            make_room(&mut windows, now);
        }
        let window = windows.entry(key).or_insert(Window {
            started: now,
            failures: 0,
            refused: false,
        });
        if now - window.started >= LOGIN_WINDOW_MS {
            *window = Window {
                started: now,
                failures: 0,
                refused: false,
            };
        }
        if window.failures >= LOGIN_MAX_FAILURES {
            let first = !window.refused;
            window.refused = true;
            let left_ms = (window.started + LOGIN_WINDOW_MS - now).clamp(1, LOGIN_WINDOW_MS);
            return Err(Throttled {
                retry_after: (left_ms as u64).div_ceil(1000),
                first,
            });
        }
        window.failures += 1;
        Ok(())
    }

    /// La tentative a réussi : le compteur de `email` repart de zéro.
    pub(crate) fn succeeded(&self, email: &str) {
        self.windows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&throttle_key(email));
    }
}

/// La clé d'une adresse dans le frein : le SHA-256 de sa forme normalisée,
/// celle sous laquelle le compte est cherché.
fn throttle_key(email: &str) -> [u8; 32] {
    Sha256::digest(normalize_email(email).as_bytes()).into()
}

/// Fait de la place dans un frein plein : les fenêtres finies partent ; s'il
/// n'y en a pas, la plus ancienne.
fn make_room(windows: &mut HashMap<[u8; 32], Window>, now: i64) {
    windows.retain(|_, window| now - window.started < LOGIN_WINDOW_MS);
    if windows.len() >= LOGIN_TRACKED_MAX {
        let oldest = windows
            .iter()
            .min_by_key(|(_, window)| window.started)
            .map(|(key, _)| *key);
        if let Some(key) = oldest {
            windows.remove(&key);
        }
    }
}

/// `mementum user set-password` : donne un nouveau mot de passe au compte de
/// cette adresse et ferme toutes ses sessions. C'est l'issue d'un mot de
/// passe oublié, faute de SMTP (Ruling R47) ; qui peut lancer la commande a
/// déjà accès au dossier des données.
///
/// La base est celle du dossier des données, ouverte comme le fait le
/// serveur : la commande peut tourner pendant qu'il tourne. Rien n'est créé
/// si la base n'existe pas. Le mot de passe n'est ni affiché ni journalisé.
/// Rend l'adresse du compte, telle que la base la garde.
///
/// Ouvrir la base la migre si une migration est en attente : la commande
/// peut être la première à ouvrir une base d'une version plus ancienne. Une
/// copie de la base est alors écrite d'abord (`Store::open`), et `on_backup`
/// la reçoit, que la suite réussisse ou non.
pub async fn set_password(
    data_dir: &Path,
    email: &str,
    password: &str,
    on_backup: impl FnOnce(&Backup),
) -> anyhow::Result<String> {
    let chars = password.chars().count();
    if !(PASSWORD_MIN_CHARS..=PASSWORD_MAX_CHARS).contains(&chars) {
        anyhow::bail!("{} ; celui-ci en a {chars}", password_rule());
    }
    let db = data_dir.join(config::DB_FILE);
    if !db.is_file() {
        anyhow::bail!(
            "aucune base dans le dossier des données {} (attendu : {})",
            data_dir.display(),
            db.display()
        );
    }
    let store = Store::open(&db)
        .await
        .with_context(|| format!("impossible d'ouvrir la base {}", db.display()))?;
    if let Some(backup) = store.backup() {
        on_backup(backup);
    }
    let changed = async {
        let user = store
            .user_by_email(email)
            .await
            .context("lecture du compte")?
            .with_context(|| format!("aucun compte pour l'adresse {}", normalize_email(email)))?;
        let hash = hash_password(password.to_string()).await?;
        store
            .set_password(user.id, &hash)
            .await
            .context("écriture du mot de passe")?;
        Ok(user.email)
    }
    .await;
    store.close().await;
    changed
}

#[cfg(test)]
mod tests {
    use argon2::password_hash::phc::PasswordHash;

    use super::*;

    /// Les valeurs fixées par le plan du lot 2.
    #[test]
    fn constants_are_those_of_the_plan() {
        assert_eq!(SESSION_COOKIE, "mementum_session");
        assert_eq!(SESSION_TTL_MS, 2_592_000_000);
        assert_eq!((PASSWORD_MIN_CHARS, PASSWORD_MAX_CHARS), (10, 200));
        assert_eq!(MAX_NAME_CHARS, 100);
        assert_eq!(LOGIN_MAX_FAILURES, 10);
        assert_eq!(LOGIN_WINDOW_MS, 900_000);
    }

    #[tokio::test]
    async fn password_roundtrip() {
        let hash = hash_password("mot-de-passe-de-test".to_string())
            .await
            .unwrap();
        // Argon2id, réglages par défaut de la crate, sel et hachage.
        assert!(
            hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{hash}"
        );
        let verify = async |password: &str, hash: Option<&str>| {
            verify_password(password.to_string(), hash.map(str::to_string))
                .await
                .unwrap()
        };
        assert!(verify("mot-de-passe-de-test", Some(&hash)).await);
        assert!(!verify("mot-de-passe-de-tesT", Some(&hash)).await);
        assert!(!verify("", Some(&hash)).await);
        // Un sel par hachage : le même mot de passe ne donne pas deux fois
        // la même chaîne.
        let again = hash_password("mot-de-passe-de-test".to_string())
            .await
            .unwrap();
        assert_ne!(again, hash);
        assert!(verify("mot-de-passe-de-test", Some(&again)).await);
        // Sans hachage : toujours faux, même pour qui connaîtrait le mot de
        // passe du hachage factice.
        assert!(!verify("mot-de-passe-de-test", None).await);
        assert!(!verify("", None).await);
        let password = b"mot-de-passe-de-test";
        assert_eq!(super::verify(password, Some(&hash), &again), Ok(true));
        assert_eq!(super::verify(password, None, &hash), Ok(false));
        // Un hachage illisible est une erreur, pas un mot de passe faux.
        let unreadable = verify_password("x".to_string(), Some("pas un hachage".to_string()));
        assert!(unreadable.await.is_err());
    }

    /// Le hachage factice a la forme d'un hachage neuf : la vérification
    /// d'une adresse inconnue coûte celle d'un mot de passe faux.
    #[tokio::test]
    async fn dummy_hash_costs_a_real_verification() {
        let fresh = hash_password("mot-de-passe-de-test".to_string())
            .await
            .unwrap();
        let fresh = PasswordHash::new(&fresh).unwrap();
        let dummy = PasswordHash::new(DUMMY_HASH).expect("hachage factice lisible");
        assert_eq!(dummy.algorithm, fresh.algorithm);
        assert_eq!(dummy.version, fresh.version);
        assert_eq!(dummy.params, fresh.params);
        let len = |hash: &PasswordHash| {
            (
                hash.salt.as_ref().map(|salt| salt.len()),
                hash.hash.as_ref().map(|output| output.len()),
            )
        };
        assert_eq!(len(&dummy), len(&fresh));
        assert_eq!(len(&dummy), (Some(16), Some(32)));
    }

    #[test]
    fn tokens() {
        let (token, hash) = new_token();
        assert_eq!(token.len(), 43);
        assert!(
            token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        assert_eq!(token_hash(&token), Some(hash));
        // Le SHA-256 des 32 octets, pas du texte.
        let bytes = URL_SAFE_NO_PAD.decode(&token).unwrap();
        assert_eq!(bytes.len(), 32);
        let expected: [u8; 32] = Sha256::digest(&bytes).into();
        assert_eq!(hash, expected);
        // Deux jetons ne se ressemblent pas.
        let (other, other_hash) = new_token();
        assert_ne!(other, token);
        assert_ne!(other_hash, hash);

        // Ce qui n'est pas un jeton.
        let zeros = "A".repeat(43);
        assert!(token_hash(&zeros).is_some());
        for bad in [
            "",
            "pas-un-jeton",
            &"A".repeat(42),
            &"A".repeat(44),
            // Avec remplissage, ou dans l'autre alphabet.
            &format!("{}=", "A".repeat(43)),
            &format!("{}+", "A".repeat(42)),
            &format!("{}/", "A".repeat(42)),
            // Une autre écriture des mêmes octets (bits de fin non nuls).
            &format!("{}B", "A".repeat(42)),
            &format!("{} ", "A".repeat(42)),
            &"é".repeat(21),
        ] {
            assert_eq!(token_hash(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn emails() {
        assert_eq!(normalize_email("  Alice@Exemple.FR \n"), "alice@exemple.fr");
        for ok in [
            "a@b",
            "alice@exemple.fr",
            "alice+notes@sous.exemple.fr",
            "élodie@exemple.fr",
            &format!("{}@exemple.fr", "a".repeat(243)),
        ] {
            assert!(email_is_valid(ok), "{ok}");
        }
        for bad in [
            "",
            "alice",
            "@exemple.fr",
            "alice@",
            "alice@@exemple.fr",
            "alice@exemple@fr",
            "alice smith@exemple.fr",
            "alice@exemple.fr\n",
            "alice@exem\u{0}ple.fr",
            &format!("{}@exemple.fr", "a".repeat(244)),
        ] {
            assert!(!email_is_valid(bad), "{bad:?}");
        }
    }

    #[test]
    fn cookies() {
        let (token, hash) = new_token();
        let set = session_cookie(&token, false);
        assert_eq!(
            set.to_str().unwrap(),
            format!("mementum_session={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age=2592000")
        );
        let secure = session_cookie(&token, true);
        assert_eq!(
            secure.to_str().unwrap(),
            format!(
                "mementum_session={token}; HttpOnly; SameSite=Lax; Secure; Path=/; Max-Age=2592000"
            )
        );
        assert_eq!(
            removal_cookie(false).to_str().unwrap(),
            "mementum_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"
        );

        let headers = |values: &[&str]| {
            let mut headers = HeaderMap::new();
            for value in values {
                headers.append(header::COOKIE, value.parse().unwrap());
            }
            headers
        };
        let session = format!("mementum_session={token}");
        assert_eq!(session_hash(&headers(&[&session])), Some(hash));
        assert_eq!(
            session_hash(&headers(&[&format!("a=1; {session}; b=2")])),
            Some(hash)
        );
        assert_eq!(session_hash(&headers(&["a=1", &session])), Some(hash));
        // Pas de cookie, un autre nom, une valeur qui n'est pas un jeton.
        assert_eq!(session_hash(&headers(&[])), None);
        assert_eq!(session_hash(&headers(&[&format!("session={token}")])), None);
        assert_eq!(
            session_hash(&headers(&[&format!("mementum_session_2={token}")])),
            None
        );
        assert_eq!(session_hash(&headers(&["mementum_session=abc"])), None);
        // Le premier cookie de ce nom fait foi.
        assert_eq!(
            session_hash(&headers(&[&format!("mementum_session=abc; {session}")])),
            None
        );
    }

    #[test]
    fn throttle_blocks_until_the_window_ends() {
        let throttle = LoginThrottle::default();
        let start = 1_000_000;
        // Dix tentatives passent, étalées dans la fenêtre.
        for attempt in 0..10 {
            assert_eq!(
                throttle.attempt("alice@exemple.fr", start + attempt * 1000),
                Ok(()),
                "essai {attempt}"
            );
        }
        // La onzième est refusée, jusqu'à la fin de la fenêtre ouverte par
        // la première.
        assert_eq!(
            throttle.attempt("alice@exemple.fr", start + 10_000),
            Err(Throttled {
                retry_after: 890,
                first: true
            })
        );
        // La même adresse, écrite autrement.
        assert_eq!(
            throttle.attempt("  ALICE@Exemple.fr ", start + 10_500),
            Err(Throttled {
                retry_after: 890,
                first: false
            })
        );
        assert_eq!(
            throttle.attempt("alice@exemple.fr", start + LOGIN_WINDOW_MS - 1),
            Err(Throttled {
                retry_after: 1,
                first: false
            })
        );
        // Une autre adresse n'est pas touchée.
        assert_eq!(throttle.attempt("bob@exemple.fr", start + 10_000), Ok(()));
        // La fenêtre finie, le compteur repart : dix tentatives de nouveau.
        for attempt in 0..10 {
            assert_eq!(
                throttle.attempt("alice@exemple.fr", start + LOGIN_WINDOW_MS + attempt),
                Ok(()),
                "essai {attempt} de la fenêtre suivante"
            );
        }
        assert_eq!(
            throttle.attempt("alice@exemple.fr", start + LOGIN_WINDOW_MS + 10),
            Err(Throttled {
                retry_after: 900,
                first: true
            })
        );
    }

    #[test]
    fn throttle_resets_on_success() {
        let throttle = LoginThrottle::default();
        for round in 0..3 {
            for attempt in 0..10 {
                assert_eq!(
                    throttle.attempt("alice@exemple.fr", attempt),
                    Ok(()),
                    "tour {round}, essai {attempt}"
                );
            }
            throttle.succeeded(" Alice@exemple.FR ");
        }
        // Une réussite d'une autre adresse ne décompte rien.
        for attempt in 0..10 {
            assert_eq!(throttle.attempt("alice@exemple.fr", attempt), Ok(()));
        }
        throttle.succeeded("bob@exemple.fr");
        assert!(throttle.attempt("alice@exemple.fr", 10).is_err());
    }

    /// Le frein ne grossit pas sans fin : plein, il oublie les fenêtres
    /// finies, sinon la plus ancienne.
    #[test]
    fn throttle_memory_is_bounded() {
        let throttle = LoginThrottle::default();
        let len = |throttle: &LoginThrottle| throttle.windows.lock().unwrap().len();
        for i in 0..LOGIN_TRACKED_MAX as i64 {
            assert_eq!(throttle.attempt(&format!("{i}@exemple.fr"), i), Ok(()));
        }
        assert_eq!(len(&throttle), LOGIN_TRACKED_MAX);
        // Aucune fenêtre n'est finie : la plus ancienne part.
        let now = LOGIN_TRACKED_MAX as i64;
        assert_eq!(throttle.attempt("une-de-plus@exemple.fr", now), Ok(()));
        assert_eq!(len(&throttle), LOGIN_TRACKED_MAX);
        let windows = throttle.windows.lock().unwrap();
        assert!(!windows.contains_key(&throttle_key("0@exemple.fr")));
        assert!(windows.contains_key(&throttle_key("1@exemple.fr")));
        drop(windows);
        // Une adresse déjà suivie ne chasse personne.
        assert_eq!(throttle.attempt("1@exemple.fr", now), Ok(()));
        assert_eq!(len(&throttle), LOGIN_TRACKED_MAX);
        // Toutes les fenêtres finies partent d'un coup.
        let later = now + LOGIN_WINDOW_MS;
        assert_eq!(throttle.attempt("plus-tard@exemple.fr", later), Ok(()));
        assert_eq!(len(&throttle), 1);
    }
}
