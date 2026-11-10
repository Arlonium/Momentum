//! Serveur HTTP de Mementum : comptes et sessions, API REST des pages, MMD et
//! WebSocket de synchronisation.
//!
//! | Route | Session | Réponse |
//! |---|---|---|
//! | `GET /health` | non | `ok` |
//! | `GET /api/v1/info` | non | [`Info`] : versions, `signup` (`open` ou `invite`), `setup` (aucun compte n'existe encore) |
//! | `POST /api/v1/auth/signup` | non | `201`, [`Me`], cookie de session ; `403 signup_closed` ; `404 invalid_invitation` ; `409 email_taken` ; `422 invalid_email`, `invalid_name`, `invalid_password` |
//! | `POST /api/v1/auth/login` | non | [`Me`], cookie de session ; `401 invalid_credentials` ; `429 too_many_attempts` (avec `Retry-After`) |
//! | `POST /api/v1/auth/logout` | non | `204`, session supprimée, cookie effacé |
//! | `POST /api/v1/auth/password` | oui | `204`, les autres sessions du compte sont supprimées ; `401 invalid_credentials` ; `422 invalid_password` ; `429 too_many_attempts` |
//! | `GET /api/v1/me` | oui | [`Me`] : le compte et ses espaces |
//! | `POST /api/v1/spaces` | oui | `201`, `SpaceInfo` : l'espace créé, dont le compte est `owner` ; `422 invalid_name` |
//! | `PATCH /api/v1/spaces/{id}` | `admin` | `SpaceInfo` ; `422 invalid_name` |
//! | `GET /api/v1/spaces/{id}/members` | `member` | `Member[]` |
//! | `PATCH /api/v1/spaces/{id}/members/{userId}` | `admin` (entre `admin` et `member`) ou `owner` | `Member` ; `409 last_owner` |
//! | `DELETE /api/v1/spaces/{id}/members/{userId}` | `admin` (jamais un `owner`), ou soi-même | `204` ; `409 last_owner` |
//! | `POST /api/v1/spaces/{id}/invitations` | `admin` | `201`, `InvitationCreated` (le jeton, montré cette fois seulement) ; `422 invalid_email` |
//! | `GET /api/v1/spaces/{id}/invitations` | `admin` | `PendingInvitation[]` : celles qui attendent, sans jeton (une invitation de page, seulement si l'on peut partager la page) |
//! | `DELETE /api/v1/invitations/{id}` | `admin` de l'espace de l'invitation (de page : qui peut aussi partager la page) | `204` ; `404` pour une invitation inconnue, échue, acceptée, ou d'un espace où l'on n'est rien |
//! | `POST /api/v1/invitations/lookup` | non | `InvitationInfo` ; `404 invalid_invitation` |
//! | `POST /api/v1/invitations/accept` | oui | `Accepted` ; `404 invalid_invitation` |
//! | `GET /api/v1/tree` | pas encore | [`Tree`] : les pages de l'espace par défaut (`vaultId` porte son id) |
//! | `POST /api/v1/pages` | pas encore | `201`, [`PageNode`] : le nœud **et** son document |
//! | `PATCH /api/v1/pages/{id}` | pas encore | [`PageNode`] ; `409 cycle` ; `404` |
//! | `GET /api/v1/pages/{id}/mmd` | pas encore | `text/markdown; charset=utf-8` ; `404` sans rien créer |
//! | `PUT /api/v1/pages/{id}/mmd` | pas encore | `204` ; `422 invalid_mmd` (avec `line`) ; `422 invalid_blocks` : prop hors registre sur un bloc ou une cellule (Ruling R41), ou blocs que BlockNote refuserait |
//! | `GET /sync/{id}?schema=N` | pas encore | WebSocket y-websocket ; `404` si la page n'existe pas |
//!
//! Toute erreur est en JSON : `{ "error": <code>, "message": <texte> }`
//! ([`types::ErrorBody`]). Une route qui demande une session répond `401
//! unauthenticated` sans elle. La base SQLite est le fichier
//! [`config::DB_FILE`] du dossier des données.
//!
//! **Droits** (`access.rs`, le seul chemin par lequel le serveur les calcule) :
//! la colonne « Session » donne le rôle minimal dans l'espace. Qui n'y a aucun
//! rôle reçoit `404 not_found`, la réponse d'un espace qui n'existe pas, admin
//! de l'instance compris ; en dessous du rôle demandé, `403 forbidden`.
//!
//! **Comptes** (`auth.rs`, `routes/auth.rs`) : comptes locaux, mot de passe
//! haché en Argon2id, session par cookie. Le premier compte créé administre
//! l'instance ; ensuite, on s'inscrit librement (`MEMENTUM_SIGNUP=open`) ou
//! avec le jeton d'une invitation (`invite`, le défaut). Un mot de passe
//! oublié se règle par `mementum user set-password`, faute de SMTP.
//!
//! **Ce que les comptes ne protègent pas encore.** Les routes de pages et
//! `/sync` (« pas encore » ci-dessus) répondent toujours sans session, comme
//! au lot 1, jusqu'aux pages sous droits : quiconque atteint le serveur lit
//! et modifie les pages. Elles servent l'espace par défaut, demandé au store
//! à chaque appel (Ruling R55) : le plus ancien espace, donc celui du premier
//! compte. D'ici là, n'écouter que sur la boucle locale (le défaut,
//! `127.0.0.1:8080`).
//!
//! **Garde-fous réseau** (`guard.rs`) : sur la boucle locale, l'en-tête `Host`
//! doit la nommer (DNS rebinding, Ruling R31) ; partout, une requête qui
//! écrit, comme une connexion `/sync`, est refusée si elle porte l'`Origin`
//! d'un autre site.

mod access;
pub mod auth;
pub mod config;
mod error;
mod guard;
pub mod types;

mod routes {
    pub(crate) mod auth;
    pub(crate) mod info;
    pub(crate) mod invitations;
    pub(crate) mod pages;
    pub(crate) mod spaces;
    pub(crate) mod sync;
}

use std::future::Future;
use std::io::Write;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{delete, get, patch, post};
use mementum_acl::AccessCache;
use mementum_store::{Node, Store};
use mementum_sync::{Hub, SyncConfig};
use tokio::net::TcpListener;
use uuid::Uuid;

pub use auth::set_password;
pub use config::Config;
pub use error::ApiError;
// La copie d'avant migration, que `set_password` signale à qui l'appelle.
pub use mementum_store::Backup;
pub use routes::pages::{MAX_TITLE_CHARS, UNTITLED};
pub use types::{Info, Me, PageNode, SignupMode, Tree};

use auth::LoginThrottle;
use routes::{info, invitations, pages, spaces, sync};
use types::ErrorCode;

/// Taille maximale d'un corps de requête (celle d'axum par défaut, posée
/// explicitement) : 2 Mio. `parse` est déjà bornée en profondeur.
pub const MAX_BODY_BYTES: usize = 2 << 20;

/// Début de la ligne écrite sur la sortie standard une fois le serveur à
/// l'écoute, suivi de l'adresse réellement liée (Ruling R24).
pub const LISTENING: &str = "mementum écoute sur http://";

/// État partagé des routes.
#[derive(Clone)]
pub(crate) struct AppState {
    store: Store,
    hub: Arc<Hub>,
    /// L'en-tête `Host` doit nommer la boucle locale : vrai quand le serveur
    /// n'écoute que sur elle (Ruling R31).
    check_host: bool,
    /// Qui peut créer un compte.
    signup: SignupMode,
    /// Le cookie de session porte `Secure` : l'adresse publique est en
    /// `https`.
    secure_cookies: bool,
    /// L'origine de l'adresse publique (`https://notes.exemple.fr`), admise
    /// sur les écritures et sur `/sync` (Ruling R51).
    public_origin: Option<Arc<str>>,
    /// Le frein des connexions, en mémoire.
    logins: Arc<LoginThrottle>,
    /// Une inscription à la fois (voir `routes::auth::signup`).
    signups: Arc<tokio::sync::Mutex<()>>,
    /// Les accès déjà calculés, par personne et par espace (`access.rs`).
    access: Arc<AccessCache>,
}

impl AppState {
    /// L'espace que servent les routes de pages du lot 1, demandé à chaque
    /// appel (Ruling R55) : le plus ancien, créé sans membre s'il n'y en a
    /// aucun. Transitoire, jusqu'aux pages sous droits.
    async fn default_space(&self) -> Result<Uuid, ApiError> {
        Ok(self.store.default_space().await?.id)
    }

    /// La page `id` de l'espace par défaut, sinon `404`.
    async fn page(&self, id: Uuid) -> Result<Node, ApiError> {
        let space_id = self.default_space().await?;
        match self.store.get_node(id).await? {
            Some(node) if node.space_id == space_id => Ok(node),
            _ => Err(ApiError::page_not_found()),
        }
    }
}

/// Millisecondes Unix.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Mène `work` au bout dans sa propre tâche, même si la requête est
/// abandonnée en route (client parti) : pour une suite d'écritures qui ne doit
/// pas s'arrêter au milieu, ou une écriture de droits suivie de la fermeture
/// des connexions qui les avaient lus.
pub(crate) async fn to_completion<T: Send + 'static>(
    work: impl Future<Output = Result<T, ApiError>> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::spawn(work)
        .await
        .map_err(|e| ApiError::internal(&e))?
}

/// Ce qu'il faut signaler, en avertissement, d'une copie d'avant migration
/// posée sans lien physique (`Backup::without_hard_link`) ; `None` si elle a
/// pris son nom de façon atomique, ou si une copie identique a été gardée.
/// Le serveur le journalise ; `mementum user set-password` l'écrit sur la
/// sortie d'erreur.
pub fn backup_warning(backup: &Backup) -> Option<&'static str> {
    backup.without_hard_link.then_some(
        "la copie d'avant migration a pris son nom par un renommage, parce que ce système de \
         fichiers ne permet pas les liens physiques : rien n'a été écrasé, mais deux processus \
         qui ouvriraient la même base au même instant pourraient viser le même nom ; ne lancer \
         qu'un processus à la fois sur ces données pendant une mise à jour",
    )
}

/// L'application : ouvre (et crée au besoin) le dossier des données et la
/// base, puis monte les routes.
pub async fn app(cfg: &Config) -> anyhow::Result<Router> {
    Ok(build(cfg).await?.0)
}

/// Lance le serveur sur `cfg.bind`, écrit sur la sortie standard la ligne
/// `mementum écoute sur http://<adresse liée>`, puis sert jusqu'à Ctrl-C
/// (ou `SIGTERM`).
pub async fn serve(cfg: Config) -> anyhow::Result<()> {
    let (router, store) = build(&cfg).await?;
    let listener = TcpListener::bind(cfg.bind)
        .await
        .with_context(|| format!("impossible d'écouter sur {}", cfg.bind))?;
    let addr = listener.local_addr()?;
    // Une sortie standard fermée n'arrête pas le serveur : l'adresse est
    // aussi dans le journal « serveur prêt ».
    let announced = {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{LISTENING}{addr}").and_then(|()| stdout.flush())
    };
    if let Err(error) = announced {
        tracing::warn!(%error, "ligne d'écoute impossible à écrire sur la sortie standard ; le serveur continue");
    }
    tracing::info!(%addr, data_dir = %cfg.data_dir.display(), "serveur prêt");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serveur HTTP")?;
    store.close().await;
    Ok(())
}

async fn build(cfg: &Config) -> anyhow::Result<(Router, Store)> {
    // Avant de toucher au disque : une adresse publique invalide arrête tout.
    let public_origin = cfg
        .public_url
        .as_deref()
        .map(config::public_origin)
        .transpose()?;
    std::fs::create_dir_all(&cfg.data_dir).with_context(|| {
        format!(
            "impossible de créer le dossier des données {}",
            cfg.data_dir.display()
        )
    })?;
    let db = cfg.db_path();
    let store = Store::open(&db)
        .await
        .with_context(|| format!("impossible d'ouvrir la base {}", db.display()))?;
    // Le store n'a pas de journal : c'est ici qu'on dit qu'une migration a
    // tourné, et où est la base d'avant (Rulings R67 et R72).
    if let Some(backup) = store.backup() {
        tracing::info!(
            path = %backup.path.display(),
            bytes = backup.bytes,
            kept = backup.kept,
            "base migrée ; la copie d'avant migration est à côté d'elle (kept : une copie \
             identique, d'une tentative précédente, y était déjà)"
        );
        if let Some(warning) = backup_warning(backup) {
            tracing::warn!(path = %backup.path.display(), "{warning}");
        }
    }
    let expired = store
        .purge_sessions(now_ms())
        .await
        .context("purge des sessions échues")?;
    if expired > 0 {
        tracing::info!(sessions = expired, "sessions échues supprimées");
    }
    let hub = Hub::new(store.clone(), SyncConfig::default());
    let check_host = cfg.bind.ip().is_loopback();
    if !check_host {
        // Transitoire : à récrire quand les routes de pages demanderont une
        // session (pages sous droits).
        tracing::warn!(
            bind = %cfg.bind,
            "écoute hors de la boucle locale : le contrôle de l'en-tête Host est désactivé, et \
             ce sont les comptes qui protègent l'accès ; mais les routes de pages et la \
             synchronisation répondent encore sans session : d'ici aux pages sous droits, \
             quiconque atteint cette adresse lit et modifie les pages"
        );
    }
    if store.count_users().await.context("lecture des comptes")? == 0 {
        tracing::warn!(
            "aucun compte n'existe : le premier compte créé sera admin de l'instance, quel que \
             soit le réglage des inscriptions ; le créer avant d'exposer le serveur"
        );
    }
    let state = AppState {
        store: store.clone(),
        hub,
        check_host,
        signup: cfg.signup,
        secure_cookies: public_origin
            .as_deref()
            .is_some_and(|origin| origin.starts_with("https://")),
        public_origin: public_origin.map(Arc::from),
        logins: Arc::new(LoginThrottle::default()),
        signups: Arc::new(tokio::sync::Mutex::new(())),
        access: Arc::new(AccessCache::new(access::ACCESS_CACHE_NODES)),
    };
    Ok((router(state), store))
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(info::health))
        .route("/api/v1/info", get(info::info))
        .route("/api/v1/auth/signup", post(routes::auth::signup))
        .route("/api/v1/auth/login", post(routes::auth::login))
        .route("/api/v1/auth/logout", post(routes::auth::logout))
        .route("/api/v1/auth/password", post(routes::auth::change_password))
        .route("/api/v1/me", get(routes::auth::me))
        .route("/api/v1/spaces", post(spaces::create))
        .route("/api/v1/spaces/{id}", patch(spaces::update))
        .route("/api/v1/spaces/{id}/members", get(spaces::members))
        .route(
            "/api/v1/spaces/{id}/members/{user_id}",
            patch(spaces::update_member).delete(spaces::remove_member),
        )
        .route(
            "/api/v1/spaces/{id}/invitations",
            get(invitations::space_invitations).post(invitations::invite_to_space),
        )
        .route("/api/v1/invitations/{id}", delete(invitations::revoke))
        .route("/api/v1/invitations/lookup", post(invitations::lookup))
        .route("/api/v1/invitations/accept", post(invitations::accept))
        .route("/api/v1/tree", get(pages::tree))
        .route("/api/v1/pages", post(pages::create))
        .route("/api/v1/pages/{id}", patch(pages::update))
        .route(
            "/api/v1/pages/{id}/mmd",
            get(pages::get_mmd).put(pages::put_mmd),
        )
        .route("/sync/{id}", get(sync::sync))
        .fallback(async || {
            ApiError::new(StatusCode::NOT_FOUND, ErrorCode::NotFound, "route inconnue")
        })
        .method_not_allowed_fallback(async || {
            ApiError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                ErrorCode::MethodNotAllowed,
                "méthode non permise sur cette route",
            )
        })
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        // Avant toute route, fallbacks compris (Rulings R31 et R51).
        .layer(middleware::from_fn_with_state(state.clone(), guard::check))
        .with_state(state)
}

/// Ctrl-C, ou `SIGTERM` (arrêt d'un conteneur) sous Unix. Un signal qui ne
/// peut pas être surveillé n'arrête jamais le serveur.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(%error, "Ctrl-C non surveillé");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "SIGTERM non surveillé");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("arrêt demandé");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une copie posée sans lien physique se signale ; une copie posée de
    /// façon atomique, ou gardée, non. (Le cas ne se provoque pas sur le
    /// disque d'un test : il demande un système de fichiers sans liens
    /// physiques.)
    #[test]
    fn backup_without_hard_link_is_a_warning() {
        let backup = |kept, without_hard_link| Backup {
            path: "mementum.db.avant-0002.bak".into(),
            bytes: 4096,
            kept,
            without_hard_link,
        };
        assert_eq!(backup_warning(&backup(false, false)), None);
        assert_eq!(backup_warning(&backup(true, false)), None);
        let warning = backup_warning(&backup(false, true)).expect("un avertissement");
        for part in ["renommage", "liens physiques", "rien n'a été écrasé"] {
            assert!(warning.contains(part), "« {part} » absent de : {warning}");
        }
    }
}
