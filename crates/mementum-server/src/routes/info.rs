//! `GET /health` et `GET /api/v1/info`.

use axum::Json;
use axum::extract::State;
use mementum_schema::SCHEMA_VERSION;

use crate::AppState;
use crate::error::ApiError;
use crate::types::Info;

/// `GET /health` : `ok` dès que le serveur répond.
pub(crate) async fn health() -> &'static str {
    "ok"
}

/// `GET /api/v1/info` : version du serveur et de son schéma de blocs, et ce
/// qu'il faut savoir avant d'avoir une session : qui peut s'inscrire, et si
/// l'instance attend encore son premier compte.
pub(crate) async fn info(State(state): State<AppState>) -> Result<Json<Info>, ApiError> {
    Ok(Json(Info {
        version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: SCHEMA_VERSION,
        signup: state.signup,
        setup: state.store.count_users().await? == 0,
    }))
}
