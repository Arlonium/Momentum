//! Serveur du spike Mementum (phase 0) : synchro Yjs + persistance SQLite.

pub mod rooms;
pub mod store;
pub mod ws;

use std::path::PathBuf;

use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;

use rooms::Rooms;
use store::Store;

pub fn app(rooms: Rooms) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/sync/{page}", get(ws::sync_handler))
        .with_state(rooms)
}

/// Lance le serveur sur `listener` avec la base SQLite `db`.
pub async fn serve(listener: TcpListener, db: PathBuf) -> anyhow::Result<()> {
    let store = Store::open(&db).await?;
    let rooms = Rooms::new(store);
    axum::serve(listener, app(rooms)).await?;
    Ok(())
}

/// Chemin de la base : `SPIKE_DB`, sinon `data/spike.db` (relatif au dossier `spike/`).
pub fn db_path() -> PathBuf {
    std::env::var("SPIKE_DB").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("data/spike.db"))
}
