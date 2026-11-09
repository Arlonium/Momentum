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
        .route("/pages/{page}/mmd", axum::routing::put(put_mmd))
        .with_state(rooms)
}

/// Remplace le contenu d'une page par un document MMD (import, tests).
async fn put_mmd(
    axum::extract::State(rooms): axum::extract::State<Rooms>,
    axum::extract::Path(page): axum::extract::Path<String>,
    body: String,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let blocks = match spike_mmd::parse(&body) {
        Ok(b) => b,
        Err(e) => return (StatusCode::UNPROCESSABLE_ENTITY, format!("ligne {} : {}", e.line, e.message)).into_response(),
    };
    let result = async {
        let room = rooms.get_or_load(&page).await?;
        room.transact("import", |txn, frag| spike_mmd::ydoc::write_blocks(txn, frag, &blocks)).await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
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
