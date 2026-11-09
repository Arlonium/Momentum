//! Serveur HTTP de Mementum : API REST des pages, MMD et WebSocket de
//! synchronisation.
//!
//! | Route | Réponse |
//! |---|---|
//! | `GET /health` | `ok` |
//! | `GET /api/v1/info` | [`Info`] |
//! | `GET /api/v1/tree` | [`Tree`] : les pages du coffre par défaut |
//! | `POST /api/v1/pages` | `201`, [`PageNode`] : le nœud **et** son document |
//! | `PATCH /api/v1/pages/{id}` | [`PageNode`] ; `409 cycle` ; `404` |
//! | `GET /api/v1/pages/{id}/mmd` | `text/markdown; charset=utf-8` ; `404` sans rien créer |
//! | `PUT /api/v1/pages/{id}/mmd` | `204` ; `422 invalid_mmd` (avec `line`) ; `422 invalid_blocks` : prop hors registre sur un bloc ou une cellule (Ruling R41), ou blocs que BlockNote refuserait |
//! | `GET /sync/{id}?schema=N` | WebSocket y-websocket ; `404` si la page n'existe pas |
//!
//! Toute erreur est en JSON : `{ "error": <code>, "message": <texte> }`
//! ([`types::ErrorBody`]). La base SQLite est le fichier
//! [`config::DB_FILE`] du dossier des données.
//!
//! Le lot 1 n'a pas d'authentification : par défaut, le serveur n'écoute que
//! sur `127.0.0.1:8080`, et il refuse le DNS rebinding et le détournement de
//! WebSocket (`guard.rs`, Ruling R31).

pub mod config;
mod error;
mod guard;
pub mod types;

mod routes {
    pub(crate) mod info;
    pub(crate) mod pages;
    pub(crate) mod sync;
}

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, patch, post};
use mementum_store::{Node, Store};
use mementum_sync::{Hub, SyncConfig};
use tokio::net::TcpListener;
use uuid::Uuid;

pub use config::Config;
pub use error::ApiError;
pub use routes::pages::{MAX_TITLE_CHARS, UNTITLED};
pub use types::{Info, PageNode, Tree};

use routes::{info, pages, sync};
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
    /// Le coffre par défaut, seul coffre du lot 1.
    vault_id: Uuid,
    /// L'en-tête `Host` doit nommer la boucle locale : vrai quand le serveur
    /// n'écoute que sur elle (Ruling R31).
    check_host: bool,
}

impl AppState {
    /// La page `id` de ce coffre, sinon `404`.
    async fn page(&self, id: Uuid) -> Result<Node, ApiError> {
        match self.store.get_node(id).await? {
            Some(node) if node.vault_id == self.vault_id => Ok(node),
            _ => Err(ApiError::page_not_found()),
        }
    }
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
    let space = store.default_space().await.context("coffre par défaut")?;
    let hub = Hub::new(store.clone(), SyncConfig::default());
    let check_host = cfg.bind.ip().is_loopback();
    if !check_host {
        tracing::warn!(
            bind = %cfg.bind,
            "écoute hors de la boucle locale : sans authentification au lot 1, quiconque atteint              cette adresse lit et modifie les pages ; contrôle de l'en-tête Host désactivé"
        );
    }
    let state = AppState {
        store: store.clone(),
        hub,
        vault_id: space.vault_id,
        check_host,
    };
    Ok((router(state), store))
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(info::health))
        .route("/api/v1/info", get(info::info))
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
        // Avant toute route, fallbacks compris (Ruling R31).
        .layer(middleware::from_fn_with_state(
            state.clone(),
            guard::check_host,
        ))
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
