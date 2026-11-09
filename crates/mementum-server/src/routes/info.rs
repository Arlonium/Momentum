//! `GET /health` et `GET /api/v1/info`.

use axum::Json;
use mementum_schema::SCHEMA_VERSION;

use crate::types::Info;

/// `GET /health` : `ok` dès que le serveur répond.
pub(crate) async fn health() -> &'static str {
    "ok"
}

/// `GET /api/v1/info` : version du serveur et de son schéma de blocs.
pub(crate) async fn info() -> Json<Info> {
    Json(Info {
        version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: SCHEMA_VERSION,
    })
}
