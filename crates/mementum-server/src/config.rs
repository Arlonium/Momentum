//! Configuration du serveur : adresse d'écoute, dossier des données,
//! inscriptions, adresse publique, intervalle des versions et taille d'un
//! fichier envoyé.
//!
//! | Réglage | Variable | Option de `mementum serve` | Défaut |
//! |---|---|---|---|
//! | adresse d'écoute | `MEMENTUM_BIND` | `--bind` | `127.0.0.1:8080` |
//! | dossier des données | `MEMENTUM_DATA_DIR` | `--data-dir` | `./data` |
//! | inscriptions | `MEMENTUM_SIGNUP` | `--signup` | `invite` |
//! | adresse publique | `MEMENTUM_PUBLIC_URL` | `--public-url` | aucune |
//! | intervalle des versions d'une page, en secondes (1 au moins) | `MEMENTUM_VERSION_EVERY_SECS` | `--version-every-secs` | `600` |
//! | taille la plus grande d'un fichier envoyé, en Mio (de 1 à 4 096) | `MEMENTUM_MAX_FILE_MB` | `--max-file-mb` | `100` |
//! | calme attendu sur une page avant de l'indexer, en millisecondes (de 0 à 15 000) | `MEMENTUM_INDEX_AFTER_MS` | aucune | `2000` |
//! | appels par minute d'un jeton d'API (de 1 à 100 000) | `MEMENTUM_TOKEN_CALLS_PER_MIN` | aucune | `120` |
//! | appels par minute de tous les jetons d'un compte (de 1 à 100 000) | `MEMENTUM_ACCOUNT_TOKEN_CALLS_PER_MIN` | aucune | `240` |
//!
//! Une option de la ligne de commande l'emporte sur sa variable, qui
//! l'emporte sur le défaut. Cinq derniers réglages n'ont ni variable ni
//! option : l'intervalle des pings du canal de l'espace
//! ([`Config::events_ping`]) et le délai d'un envoi de fichier qui stagne
//! ([`Config::upload_idle`]), que seuls les tests raccourcissent, et les
//! réglages d'ouverture de la base ([`Config::store`]), le poids le plus
//! grand d'une copie de pages ([`Config::max_duplicated_bytes`]) et le plus
//! long délai avant d'indexer une page écrite sans relâche
//! ([`Config::index_at_most`]), que seuls les tests changent.
//!
//! Le serveur n'écoute jamais par défaut sur une autre interface que la
//! boucle locale, et les inscriptions sont par défaut sur invitation (Ruling
//! R50) : le premier compte, lui, est toujours permis.

use std::ffi::{OsStr, OsString};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use axum::http::Uri;
use mementum_store::StoreOptions;
use mementum_sync::VERSION_EVERY;

use crate::events::EVENTS_PING;
pub use crate::types::SignupMode;

/// Adresse d'écoute par défaut : la boucle locale seulement.
pub const DEFAULT_BIND: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

/// Dossier des données par défaut, relatif au dossier courant.
pub const DEFAULT_DATA_DIR: &str = "./data";

/// Nom du fichier SQLite, dans le dossier des données.
pub const DB_FILE: &str = "mementum.db";

/// Variable d'environnement de l'adresse d'écoute.
pub const BIND_VAR: &str = "MEMENTUM_BIND";

/// Variable d'environnement du dossier des données.
pub const DATA_DIR_VAR: &str = "MEMENTUM_DATA_DIR";

/// Variable d'environnement des inscriptions : `open` ou `invite`.
pub const SIGNUP_VAR: &str = "MEMENTUM_SIGNUP";

/// Variable d'environnement de l'adresse publique de l'instance.
pub const PUBLIC_URL_VAR: &str = "MEMENTUM_PUBLIC_URL";

/// Variable d'environnement de l'intervalle des versions d'une page, en secondes.
pub const VERSION_EVERY_VAR: &str = "MEMENTUM_VERSION_EVERY_SECS";

/// Option de `mementum serve` de l'intervalle des versions, telle que les messages la nomment.
const VERSION_EVERY_OPTION: &str = "--version-every-secs";

/// Variable d'environnement de la borne d'un fichier envoyé, en Mio.
pub const MAX_FILE_MB_VAR: &str = "MEMENTUM_MAX_FILE_MB";

/// Option de `mementum serve` de la borne d'un fichier, telle que les messages la nomment.
const MAX_FILE_MB_OPTION: &str = "--max-file-mb";

/// La borne d'un fichier envoyé par défaut, en Mio (Ruling R247).
pub const DEFAULT_MAX_FILE_MB: u64 = 100;

/// La plus haute borne qu'on peut donner à un fichier, en Mio (Ruling R247).
pub const MAX_MAX_FILE_MB: u64 = 4096;

/// Variable d'environnement du calme attendu sur une page avant de l'indexer, en millisecondes
/// (Ruling R906). Sans option de ligne de commande : elle sert aux suites de bout en bout, qui ne
/// peuvent pas attendre deux secondes après chaque frappe.
pub const INDEX_AFTER_MS_VAR: &str = "MEMENTUM_INDEX_AFTER_MS";

/// Le plus long calme qu'on peut demander avant d'indexer une page, en millisecondes : le plafond
/// d'une page écrite sans relâche ([`INDEX_AT_MOST`](crate::INDEX_AT_MOST)). Au-delà, le calme
/// ne serait jamais attendu, le plafond passant toujours avant lui.
pub const MAX_INDEX_AFTER_MS: u64 = 15_000;

/// Variable d'environnement des appels par minute d'un jeton d'API (lot 5, Ruling R1241).
pub const TOKEN_CALLS_VAR: &str = "MEMENTUM_TOKEN_CALLS_PER_MIN";

/// Variable d'environnement des appels par minute de tous les jetons d'un compte (lot 5, Ruling
/// R1241).
pub const ACCOUNT_TOKEN_CALLS_VAR: &str = "MEMENTUM_ACCOUNT_TOKEN_CALLS_PER_MIN";

/// Le plus grand débit d'appels par minute qu'on peut régler, par jeton comme par compte.
pub const MAX_CALLS_PER_MIN: u32 = 100_000;

/// Le délai au-delà duquel un envoi qui ne reçoit plus rien est abandonné (Ruling R247).
pub const UPLOAD_IDLE: Duration = Duration::from_secs(30);

/// Ce qu'on attend d'une adresse publique, pour les messages d'erreur.
const PUBLIC_URL_HINT: &str = "attendu une adresse comme https://notes.exemple.fr";

/// Configuration du serveur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Adresse d'écoute. Un port 0 laisse le système en choisir un libre :
    /// `serve` écrit l'adresse réellement liée (Ruling R24).
    pub bind: SocketAddr,
    /// Dossier des données, créé au besoin. La base SQLite y est
    /// [`DB_FILE`].
    pub data_dir: PathBuf,
    /// Qui peut créer un compte. Le premier compte de l'instance est permis
    /// dans les deux cas.
    pub signup: SignupMode,
    /// Adresse sous laquelle les gens atteignent l'instance, par exemple
    /// `https://notes.exemple.fr` (derrière un mandataire). Seule son origine
    /// compte ([`public_origin`]) : en `https`, le cookie de session porte
    /// `Secure` ; et cette origine est admise sur les écritures et sur la
    /// synchronisation (Ruling R51).
    pub public_url: Option<String>,
    /// Intervalle des pings du canal de l'espace (`/events/{spaceId}`), [`EVENTS_PING`] par
    /// défaut. Ni variable ni option : seuls les tests le raccourcissent, pour ne pas attendre
    /// 20 s.
    pub events_ping: Duration,
    /// Intervalle des versions d'une page (Ruling R135) : une version est coupée avant
    /// l'écriture qui suit de ce délai la première écriture non versionnée. Dix minutes par
    /// défaut ([`VERSION_EVERY`]), une seconde au moins.
    pub version_every: Duration,
    /// Octets au plus par fichier envoyé (Ruling R247) : [`DEFAULT_MAX_FILE_MB`] Mio par défaut,
    /// de 1 à [`MAX_MAX_FILE_MB`] Mio.
    pub max_file_bytes: u64,
    /// Un envoi de fichier qui ne reçoit aucun octet pendant ce délai est abandonné, sa
    /// connexion fermée sans réponse (Ruling R247) : [`UPLOAD_IDLE`] par défaut. Ni variable ni
    /// option : seuls les tests le raccourcissent.
    pub upload_idle: Duration,
    /// Les réglages d'ouverture de la base ([`StoreOptions`] : délai d'attente sur un verrou,
    /// disque lent simulé), les défauts du store par défaut. Ni variable ni option : seuls les
    /// tests les changent.
    pub store: StoreOptions,
    /// Octets au plus, tous documents confondus, d'une page dupliquée avec ses sous-pages
    /// (Ruling R592) : [`MAX_DUPLICATED_BYTES`](crate::MAX_DUPLICATED_BYTES) par défaut. Ni
    /// variable ni option : seuls les tests l'abaissent, pour ne pas bâtir des dizaines de
    /// mégaoctets de pages.
    pub max_duplicated_bytes: usize,
    /// Le calme attendu sur une page, après sa dernière écriture, avant de l'indexer (Ruling
    /// R906) : [`INDEX_AFTER`](crate::INDEX_AFTER) par défaut, deux secondes.
    pub index_after: Duration,
    /// Le plus long délai entre la première écriture non indexée d'une page et son indexation,
    /// même écrite sans relâche (Ruling R906) : [`INDEX_AT_MOST`](crate::INDEX_AT_MOST) par
    /// défaut, quinze secondes. Ni variable ni option : seuls les tests le changent.
    pub index_at_most: Duration,
    /// Appels par minute glissante d'un jeton d'API (Ruling R1241), 120 par défaut.
    pub token_calls_per_min: u32,
    /// Appels par minute glissante de tous les jetons d'un compte (Ruling R1241), 240 par défaut.
    pub account_token_calls_per_min: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind: DEFAULT_BIND,
            data_dir: PathBuf::from(DEFAULT_DATA_DIR),
            signup: SignupMode::default(),
            public_url: None,
            events_ping: EVENTS_PING,
            version_every: VERSION_EVERY,
            max_file_bytes: DEFAULT_MAX_FILE_MB << 20,
            upload_idle: UPLOAD_IDLE,
            store: StoreOptions::default(),
            max_duplicated_bytes: crate::MAX_DUPLICATED_BYTES,
            index_after: crate::INDEX_AFTER,
            index_at_most: crate::INDEX_AT_MOST,
            token_calls_per_min: crate::agent::calls::TOKEN_CALLS_PER_MIN,
            account_token_calls_per_min: crate::agent::calls::ACCOUNT_TOKEN_CALLS_PER_MIN,
        }
    }
}

impl Config {
    /// Les défauts, remplacés par les variables `MEMENTUM_*` qui sont posées
    /// (une variable vide compte comme absente).
    pub fn from_env() -> anyhow::Result<Config> {
        Config::from_args(None, None, None, None, None, None)
    }

    /// Les options de la ligne de commande d'abord, puis les variables
    /// d'environnement, puis les défauts. Une variable n'est lue que si
    /// l'option manque : un `MEMENTUM_BIND` invalide n'empêche pas un
    /// `--bind` valide. `version_every_secs` et `max_file_mb` sont les textes
    /// des options `--version-every-secs` et `--max-file-mb`, relus ici comme
    /// leurs variables, pour que les deux soient refusés de la même façon.
    pub fn from_args(
        bind: Option<SocketAddr>,
        data_dir: Option<PathBuf>,
        signup: Option<SignupMode>,
        public_url: Option<String>,
        version_every_secs: Option<String>,
        max_file_mb: Option<String>,
    ) -> anyhow::Result<Config> {
        Config::resolve(
            bind,
            data_dir,
            signup,
            public_url,
            version_every_secs,
            max_file_mb,
            |name| std::env::var_os(name),
        )
    }

    /// Comme [`Config::from_args`], avec `var` pour lire les variables.
    pub fn resolve(
        bind: Option<SocketAddr>,
        data_dir: Option<PathBuf>,
        signup: Option<SignupMode>,
        public_url: Option<String>,
        version_every_secs: Option<String>,
        max_file_mb: Option<String>,
        var: impl Fn(&str) -> Option<OsString>,
    ) -> anyhow::Result<Config> {
        let bind = match bind {
            Some(bind) => bind,
            None => match set(&var, BIND_VAR) {
                Some(raw) => parse_bind(&raw)?,
                None => DEFAULT_BIND,
            },
        };
        let signup = match signup {
            Some(signup) => signup,
            None => match set(&var, SIGNUP_VAR) {
                Some(raw) => parse_signup(&raw)?,
                None => SignupMode::default(),
            },
        };
        let public_url = match public_url {
            Some(url) => Some(url),
            None => match set(&var, PUBLIC_URL_VAR) {
                Some(raw) => Some(raw.into_string().map_err(|raw| {
                    anyhow::anyhow!(
                        "{PUBLIC_URL_VAR} invalide (« {} ») : {PUBLIC_URL_HINT}",
                        raw.to_string_lossy()
                    )
                })?),
                None => None,
            },
        };
        let public_url = public_url.map(|url| url.trim().to_string());
        if let Some(url) = &public_url {
            // Refusée au démarrage, pas à la première requête.
            public_origin(url)?;
        }
        let version_every = match version_every_secs {
            Some(raw) => parse_version_every(OsStr::new(&raw), VERSION_EVERY_OPTION)?,
            None => match set(&var, VERSION_EVERY_VAR) {
                Some(raw) => parse_version_every(&raw, VERSION_EVERY_VAR)?,
                None => VERSION_EVERY,
            },
        };
        let max_file_mb = match max_file_mb {
            Some(raw) => parse_max_file_mb(OsStr::new(&raw), MAX_FILE_MB_OPTION)?,
            None => match set(&var, MAX_FILE_MB_VAR) {
                Some(raw) => parse_max_file_mb(&raw, MAX_FILE_MB_VAR)?,
                None => DEFAULT_MAX_FILE_MB,
            },
        };
        let index_after = match set(&var, INDEX_AFTER_MS_VAR) {
            Some(raw) => parse_index_after(&raw)?,
            None => crate::INDEX_AFTER,
        };
        let token_calls_per_min = match set(&var, TOKEN_CALLS_VAR) {
            Some(raw) => parse_calls_per_min(&raw, TOKEN_CALLS_VAR)?,
            None => crate::agent::calls::TOKEN_CALLS_PER_MIN,
        };
        let account_token_calls_per_min = match set(&var, ACCOUNT_TOKEN_CALLS_VAR) {
            Some(raw) => parse_calls_per_min(&raw, ACCOUNT_TOKEN_CALLS_VAR)?,
            None => crate::agent::calls::ACCOUNT_TOKEN_CALLS_PER_MIN,
        };
        Ok(Config {
            bind,
            data_dir: resolve_data_dir(data_dir, &var),
            signup,
            public_url,
            events_ping: EVENTS_PING,
            version_every,
            max_file_bytes: max_file_mb << 20,
            upload_idle: UPLOAD_IDLE,
            store: StoreOptions::default(),
            max_duplicated_bytes: crate::MAX_DUPLICATED_BYTES,
            index_after,
            index_at_most: crate::INDEX_AT_MOST,
            token_calls_per_min,
            account_token_calls_per_min,
        })
    }

    /// Chemin de la base SQLite.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join(DB_FILE)
    }
}

/// Le dossier des données : l'option, sinon `MEMENTUM_DATA_DIR`, sinon
/// `./data`. Pour les commandes qui n'ont besoin que de lui (`mementum user
/// set-password`) : une autre variable, même invalide, ne les arrête pas.
pub fn data_dir(option: Option<PathBuf>) -> PathBuf {
    resolve_data_dir(option, &|name: &str| std::env::var_os(name))
}

fn resolve_data_dir(option: Option<PathBuf>, var: &impl Fn(&str) -> Option<OsString>) -> PathBuf {
    option
        .or_else(|| set(var, DATA_DIR_VAR).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR))
}

/// La variable `name`, si elle est posée. Vide, ou blanche quand elle est en
/// UTF-8 : comme absente.
fn set(var: &impl Fn(&str) -> Option<OsString>, name: &str) -> Option<OsString> {
    var(name).filter(|v| !v.is_empty() && v.to_str().is_none_or(|s| !s.trim().is_empty()))
}

/// L'origine d'une adresse publique, telle qu'un navigateur l'écrit dans
/// l'en-tête `Origin` : `schéma://hôte[:port]`, en minuscules, sans le port
/// par défaut du schéma. Le chemin, s'il y en a un, ne compte pas. Erreur si
/// l'adresse n'est pas en `http` ou `https`, n'a pas d'hôte, ou porte des
/// identifiants (`utilisateur@hôte`).
pub fn public_origin(url: &str) -> anyhow::Result<String> {
    let invalid = || anyhow::anyhow!("adresse publique invalide (« {url} ») : {PUBLIC_URL_HINT}");
    let uri: Uri = url.trim().parse().map_err(|_| invalid())?;
    let scheme = match uri.scheme_str() {
        Some(scheme) if scheme.eq_ignore_ascii_case("http") => "http",
        Some(scheme) if scheme.eq_ignore_ascii_case("https") => "https",
        _ => return Err(invalid()),
    };
    let authority = uri.authority().ok_or_else(invalid)?;
    let host = authority.host().to_ascii_lowercase();
    if host.is_empty() || authority.as_str().contains('@') {
        return Err(invalid());
    }
    let default_port = if scheme == "https" { 443 } else { 80 };
    Ok(match authority.port_u16() {
        Some(port) if port != default_port => format!("{scheme}://{host}:{port}"),
        _ => format!("{scheme}://{host}"),
    })
}

/// `MEMENTUM_BIND`, avec une erreur en français (sans la cause de std).
fn parse_bind(raw: &OsStr) -> anyhow::Result<SocketAddr> {
    raw.to_str()
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{BIND_VAR} invalide (« {} ») : attendu une adresse comme 127.0.0.1:8080",
                raw.to_string_lossy()
            )
        })
}

/// L'intervalle des versions, lu dans la variable ou l'option `name` : un nombre entier de
/// secondes, espaces de bord admises, une au moins.
fn parse_version_every(raw: &OsStr, name: &str) -> anyhow::Result<Duration> {
    raw.to_str()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|secs| *secs >= 1)
        .map(Duration::from_secs)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{name} invalide (« {} ») : attendu un nombre entier de secondes, 1 au moins \
                 (600 par défaut : une version toutes les dix minutes de frappe)",
                raw.to_string_lossy()
            )
        })
}

/// `MEMENTUM_INDEX_AFTER_MS` : un nombre entier de millisecondes, espaces de bord admises, de 0 à
/// [`MAX_INDEX_AFTER_MS`].
fn parse_index_after(raw: &OsStr) -> anyhow::Result<Duration> {
    raw.to_str()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|ms| *ms <= MAX_INDEX_AFTER_MS)
        .map(Duration::from_millis)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{INDEX_AFTER_MS_VAR} invalide (« {} ») : attendu un nombre entier de \
                 millisecondes, de 0 à {MAX_INDEX_AFTER_MS} (2000 par défaut : une page est \
                 indexée deux secondes après sa dernière écriture)",
                raw.to_string_lossy()
            )
        })
}

/// Un débit d'appels par minute, lu dans la variable `name` : un nombre entier, espaces de bord
/// admises, de 1 à [`MAX_CALLS_PER_MIN`].
fn parse_calls_per_min(raw: &OsStr, name: &str) -> anyhow::Result<u32> {
    raw.to_str()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|calls| (1..=MAX_CALLS_PER_MIN).contains(calls))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{name} invalide (« {} ») : attendu un nombre entier d'appels par minute, de 1 \
                 à {MAX_CALLS_PER_MIN}",
                raw.to_string_lossy()
            )
        })
}

/// La borne d'un fichier, lue dans la variable ou l'option `name` : un nombre entier de Mio,
/// espaces de bord admises, de 1 à [`MAX_MAX_FILE_MB`].
fn parse_max_file_mb(raw: &OsStr, name: &str) -> anyhow::Result<u64> {
    raw.to_str()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|mb| (1..=MAX_MAX_FILE_MB).contains(mb))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{name} invalide (« {} ») : attendu un nombre entier de Mio, de 1 à \
                 {MAX_MAX_FILE_MB} ({DEFAULT_MAX_FILE_MB} par défaut : la taille la plus grande \
                 d'un fichier envoyé dans une page)",
                raw.to_string_lossy()
            )
        })
}

/// `MEMENTUM_SIGNUP`.
fn parse_signup(raw: &OsStr) -> anyhow::Result<SignupMode> {
    raw.to_str()
        .and_then(|s| SignupMode::parse(s.trim()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{SIGNUP_VAR} invalide (« {} ») : attendu open ou invite",
                raw.to_string_lossy()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn defaults_listen_on_loopback_only() {
        let cfg = Config::resolve(None, None, None, None, None, None, vars(&[])).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.bind.to_string(), "127.0.0.1:8080");
        assert!(cfg.bind.ip().is_loopback());
        assert_eq!(cfg.data_dir, PathBuf::from("./data"));
        assert_eq!(cfg.db_path(), PathBuf::from("./data").join("mementum.db"));
        // Ruling R50 : sur invitation, sauf réglage contraire.
        assert_eq!(cfg.signup, SignupMode::Invite);
        assert_eq!(cfg.public_url, None);
    }

    #[test]
    fn variables_replace_defaults() {
        let cfg = Config::resolve(
            None,
            None,
            None,
            None,
            None,
            None,
            vars(&[
                ("MEMENTUM_BIND", "0.0.0.0:9000"),
                ("MEMENTUM_DATA_DIR", "/srv/mementum"),
                ("MEMENTUM_SIGNUP", " open "),
                ("MEMENTUM_PUBLIC_URL", " https://notes.exemple.fr/ "),
            ]),
        )
        .unwrap();
        assert_eq!(cfg.bind.to_string(), "0.0.0.0:9000");
        assert_eq!(cfg.data_dir, PathBuf::from("/srv/mementum"));
        assert_eq!(cfg.signup, SignupMode::Open);
        assert_eq!(cfg.public_url.as_deref(), Some("https://notes.exemple.fr/"));

        // Vides : comme absentes.
        let empty = vars(&[
            ("MEMENTUM_BIND", " "),
            ("MEMENTUM_DATA_DIR", ""),
            ("MEMENTUM_SIGNUP", ""),
            ("MEMENTUM_PUBLIC_URL", "  "),
        ]);
        assert_eq!(
            Config::resolve(None, None, None, None, None, None, empty).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn options_win_over_variables() {
        let env = vars(&[
            ("MEMENTUM_BIND", "nimporte"),
            ("MEMENTUM_DATA_DIR", "/srv/ignore"),
            ("MEMENTUM_SIGNUP", "nimporte"),
            ("MEMENTUM_PUBLIC_URL", "nimporte"),
        ]);
        let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
        // Une variable invalide n'empêche pas une option valide.
        let cfg = Config::resolve(
            Some(bind),
            Some(PathBuf::from("ici")),
            Some(SignupMode::Open),
            Some("http://192.168.1.10:8080".to_string()),
            None,
            None,
            env,
        )
        .unwrap();
        assert_eq!(cfg.bind, bind);
        assert_eq!(cfg.data_dir, PathBuf::from("ici"));
        assert_eq!(cfg.signup, SignupMode::Open);
        assert_eq!(cfg.public_url.as_deref(), Some("http://192.168.1.10:8080"));
    }

    #[test]
    fn invalid_bind_is_an_error_in_french() {
        let env = vars(&[("MEMENTUM_BIND", "localhost")]);
        let err = Config::resolve(None, None, None, None, None, None, env).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("MEMENTUM_BIND invalide"), "{message}");
        assert!(message.contains("localhost"), "{message}");
        assert!(!message.contains("invalid socket address"), "{message}");
    }

    #[test]
    fn invalid_signup_is_an_error_in_french() {
        for raw in ["ouvert", "OPEN", "open,invite"] {
            let pairs = [("MEMENTUM_SIGNUP", raw)];
            let err =
                Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap_err();
            let message = format!("{err:#}");
            assert!(message.contains("MEMENTUM_SIGNUP invalide"), "{message}");
            assert!(message.contains(raw), "{message}");
            assert!(message.contains("open ou invite"), "{message}");
        }
    }

    #[test]
    fn invalid_public_url_is_refused_at_startup() {
        for raw in [
            "notes.exemple.fr",
            "ftp://notes.exemple.fr",
            "https://",
            "https://alex:secret@notes.exemple.fr",
            "/chemin",
        ] {
            let pairs = [("MEMENTUM_PUBLIC_URL", raw)];
            let err =
                Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap_err();
            let message = format!("{err:#}");
            assert!(message.contains("adresse publique invalide"), "{message}");
            assert!(message.contains("https://notes.exemple.fr"), "{message}");
            // La même adresse en option est refusée de même.
            let option = Some(raw.to_string());
            assert!(Config::resolve(None, None, None, option, None, None, vars(&[])).is_err());
        }
    }

    /// L'origine est celle qu'écrit un navigateur : minuscules, sans chemin,
    /// sans le port par défaut du schéma.
    #[test]
    fn public_origin_is_what_a_browser_sends() {
        for (url, origin) in [
            ("https://notes.exemple.fr", "https://notes.exemple.fr"),
            ("https://notes.exemple.fr/", "https://notes.exemple.fr"),
            (
                "HTTPS://Notes.Exemple.FR:443/app",
                "https://notes.exemple.fr",
            ),
            (
                "https://notes.exemple.fr:8443",
                "https://notes.exemple.fr:8443",
            ),
            ("http://192.168.1.10:8080", "http://192.168.1.10:8080"),
            ("http://exemple.fr:80", "http://exemple.fr"),
            ("http://exemple.fr:443", "http://exemple.fr:443"),
            ("http://[::1]:8080/", "http://[::1]:8080"),
        ] {
            assert_eq!(public_origin(url).unwrap(), origin, "{url}");
        }
    }

    /// L'intervalle des versions : dix minutes par défaut ; sinon l'option, puis la variable,
    /// en secondes entières (espaces de bord admises), une au moins.
    #[test]
    fn version_interval_comes_from_the_option_then_the_variable() {
        let none = Config::resolve(None, None, None, None, None, None, vars(&[])).unwrap();
        assert_eq!(none.version_every, Duration::from_secs(600));
        let pairs = [("MEMENTUM_VERSION_EVERY_SECS", " 30 ")];
        let variable = Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap();
        assert_eq!(variable.version_every, Duration::from_secs(30));
        // Vide : comme absente.
        let pairs = [("MEMENTUM_VERSION_EVERY_SECS", " ")];
        let empty = Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap();
        assert_eq!(empty.version_every, Duration::from_secs(600));
        // L'option l'emporte, et une variable invalide ne l'empêche pas.
        let pairs = [("MEMENTUM_VERSION_EVERY_SECS", "nimporte")];
        let option = Some("1".to_string());
        let cfg = Config::resolve(None, None, None, None, option, None, vars(&pairs)).unwrap();
        assert_eq!(cfg.version_every, Duration::from_secs(1));
    }

    /// Le calme attendu avant d'indexer une page (Ruling R906) : deux secondes par défaut, quinze
    /// au plus sous une écriture sans relâche ; la variable le règle en millisecondes, et une
    /// valeur qui n'en est pas une arrête le démarrage en la nommant.
    #[test]
    fn index_delay_comes_from_its_variable() {
        let none = Config::resolve(None, None, None, None, None, None, vars(&[])).unwrap();
        assert_eq!(none.index_after, Duration::from_secs(2));
        assert_eq!(none.index_at_most, Duration::from_secs(15));
        let pairs = [("MEMENTUM_INDEX_AFTER_MS", " 50 ")];
        let variable = Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap();
        assert_eq!(variable.index_after, Duration::from_millis(50));
        assert_eq!(variable.index_at_most, Duration::from_secs(15));
        // Au-delà du plafond d'une page écrite sans relâche, le calme ne serait jamais attendu.
        let pairs = [("MEMENTUM_INDEX_AFTER_MS", "15000")];
        let most = Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap();
        assert_eq!(most.index_after, most.index_at_most);
        for raw in ["bientôt", "-1", "15001", "1.5"] {
            let pairs = [("MEMENTUM_INDEX_AFTER_MS", raw)];
            let error = Config::resolve(None, None, None, None, None, None, vars(&pairs))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("MEMENTUM_INDEX_AFTER_MS") && error.contains(raw),
                "{error}"
            );
        }
    }

    /// Le débit des jetons d'API (Ruling R1241) : 120 appels par minute par jeton, 240 par compte,
    /// par défaut ; chaque variable règle le sien, et une valeur qui n'en est pas une arrête le
    /// démarrage en la nommant.
    #[test]
    fn token_rates_come_from_their_variables() {
        let none = Config::resolve(None, None, None, None, None, None, vars(&[])).unwrap();
        assert_eq!(
            (none.token_calls_per_min, none.account_token_calls_per_min),
            (120, 240)
        );
        let default = Config::default();
        assert_eq!(
            (
                default.token_calls_per_min,
                default.account_token_calls_per_min
            ),
            (120, 240)
        );
        let pairs = [
            ("MEMENTUM_TOKEN_CALLS_PER_MIN", " 3 "),
            ("MEMENTUM_ACCOUNT_TOKEN_CALLS_PER_MIN", "5"),
        ];
        let set = Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap();
        assert_eq!(
            (set.token_calls_per_min, set.account_token_calls_per_min),
            (3, 5)
        );
        for name in [
            "MEMENTUM_TOKEN_CALLS_PER_MIN",
            "MEMENTUM_ACCOUNT_TOKEN_CALLS_PER_MIN",
        ] {
            for raw in ["beaucoup", "0", "-1", "100001", "2.5"] {
                let pairs = [(name, raw)];
                let error = Config::resolve(None, None, None, None, None, None, vars(&pairs))
                    .unwrap_err()
                    .to_string();
                assert!(error.contains(name) && error.contains(raw), "{error}");
            }
        }
    }

    /// Une valeur qui n'est pas un nombre entier de secondes, une au moins, arrête le démarrage
    /// en nommant le réglage et la valeur, en français.
    #[test]
    fn invalid_version_interval_is_refused_naming_it() {
        for raw in ["0", "-5", "dix", "1.5", "600s", "99999999999999999999999"] {
            let pairs = [("MEMENTUM_VERSION_EVERY_SECS", raw)];
            let err =
                Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap_err();
            let message = format!("{err:#}");
            assert!(
                message.contains("MEMENTUM_VERSION_EVERY_SECS invalide"),
                "{message}"
            );
            assert!(message.contains(raw), "{message}");
            assert!(message.contains("1 au moins"), "{message}");
            let option = Some(raw.to_string());
            let err = Config::resolve(None, None, None, None, option, None, vars(&[])).unwrap_err();
            let message = format!("{err:#}");
            assert!(
                message.contains("--version-every-secs invalide"),
                "{message}"
            );
            assert!(message.contains(raw), "{message}");
        }
    }

    /// La borne d'un fichier : 100 Mio par défaut ; sinon l'option, puis la variable, en Mio
    /// entiers (espaces de bord admises), de 1 à 4 096. Le délai d'un envoi qui stagne n'a ni
    /// variable ni option : 30 s.
    #[test]
    fn file_bound_comes_from_the_option_then_the_variable() {
        let none = Config::resolve(None, None, None, None, None, None, vars(&[])).unwrap();
        assert_eq!(none.max_file_bytes, 100 << 20);
        assert_eq!(none.upload_idle, Duration::from_secs(30));
        let pairs = [("MEMENTUM_MAX_FILE_MB", " 1 ")];
        let variable = Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap();
        assert_eq!(variable.max_file_bytes, 1 << 20);
        let pairs = [("MEMENTUM_MAX_FILE_MB", "4096")];
        let most = Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap();
        assert_eq!(most.max_file_bytes, 4096 << 20);
        // Vide : comme absente.
        let pairs = [("MEMENTUM_MAX_FILE_MB", " ")];
        let empty = Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap();
        assert_eq!(empty.max_file_bytes, 100 << 20);
        // L'option l'emporte, et une variable invalide ne l'empêche pas.
        let pairs = [("MEMENTUM_MAX_FILE_MB", "nimporte")];
        let option = Some("2".to_string());
        let cfg = Config::resolve(None, None, None, None, None, option, vars(&pairs)).unwrap();
        assert_eq!(cfg.max_file_bytes, 2 << 20);
    }

    /// Une borne qui n'est pas un nombre entier de Mio, de 1 à 4 096, arrête le démarrage en
    /// nommant le réglage et la valeur, en français.
    #[test]
    fn invalid_file_bound_is_refused_naming_it() {
        for raw in [
            "0",
            "4097",
            "nimporte",
            "-1",
            "1.5",
            "100M",
            "99999999999999999999999",
        ] {
            let pairs = [("MEMENTUM_MAX_FILE_MB", raw)];
            let err =
                Config::resolve(None, None, None, None, None, None, vars(&pairs)).unwrap_err();
            let message = format!("{err:#}");
            assert!(
                message.contains(&format!("MEMENTUM_MAX_FILE_MB invalide (« {raw} »)")),
                "{message}"
            );
            assert!(message.contains("de 1 à 4096"), "{message}");
            let option = Some(raw.to_string());
            let err = Config::resolve(None, None, None, None, None, option, vars(&[])).unwrap_err();
            let message = format!("{err:#}");
            assert!(
                message.contains(&format!("--max-file-mb invalide (« {raw} »)")),
                "{message}"
            );
        }
    }

    /// `MEMENTUM_DATA_DIR` se lit tel quel, même hors UTF-8.
    #[test]
    fn data_dir_is_read_without_loss() {
        #[cfg(windows)]
        let raw = {
            use std::os::windows::ffi::OsStringExt;
            // « d », une moitié de paire UTF-16 seule, « x ».
            OsString::from_wide(&[0x64, 0xD800, 0x78])
        };
        #[cfg(unix)]
        let raw = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![b'd', 0xff, b'x'])
        };
        assert!(raw.to_str().is_none(), "pas de l'UTF-8");
        let expected = PathBuf::from(&raw);
        let cfg = Config::resolve(None, None, None, None, None, None, move |name| {
            (name == DATA_DIR_VAR).then(|| raw.clone())
        })
        .unwrap();
        assert_eq!(cfg.data_dir, expected);
    }
}
