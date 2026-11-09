//! `GET /sync/{id}?schema=N` : la WebSocket de synchronisation d'une page,
//! au protocole de y-websocket, servie par [`Hub::serve_connection`].
//!
//! L'existence de la page est vérifiée **avant** la poignée de main : une
//! page inconnue donne `404`, sans acteur ni écriture.
//!
//! `schema` est la version de schéma du client. Absente ou illisible, elle
//! vaut 0 : un client qui ne la donne pas est traité comme d'un autre schéma,
//! en lecture seule (il reçoit tout, ses mises à jour sont ignorées).
//!
//! [`Hub::serve_connection`]: mementum_sync::Hub::serve_connection

use std::collections::HashMap;

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use mementum_sync::ws::WsBytes;

use crate::AppState;
use crate::error::ApiError;
use crate::routes::pages::page_id;
use crate::types::ErrorCode;

pub(crate) async fn sync(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    let id = page_id(path)?;
    state.page(id).await?;
    let upgrade = upgrade.map_err(|e| {
        ApiError::new(
            e.status(),
            ErrorCode::InvalidRequest,
            "connexion WebSocket attendue (en-têtes Upgrade: websocket)",
        )
    })?;
    let schema = query
        .ok()
        .and_then(|Query(q)| q.get("schema").and_then(|s| s.trim().parse::<u32>().ok()))
        .unwrap_or(0);
    // Ruling R27 : axum refuse un message trop gros avant de le garder en
    // entier ; `serve_connection` contrôle la même borne.
    let max = state.hub.config().max_message_bytes;
    let hub = state.hub.clone();
    Ok(upgrade
        .max_message_size(max)
        .max_frame_size(max)
        .on_failed_upgrade(move |error| {
            tracing::warn!(node_id = %id, %error, "poignée de main WebSocket manquée");
        })
        .on_upgrade(move |socket| async move {
            if let Err(error) = hub.serve_connection(id, schema, WsBytes(socket)).await {
                tracing::warn!(node_id = %id, %error, "connexion de synchronisation refusée");
            }
        }))
}
