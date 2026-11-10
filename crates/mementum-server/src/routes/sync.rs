//! `GET /sync/{id}?schema=N` : la WebSocket de synchronisation d'une page,
//! au protocole de y-websocket, servie par [`Hub::serve_connection`].
//!
//! Une en-tête `Origin` d'une autre page que celles de la boucle locale, du
//! serveur ou de son adresse publique donne `403 bad_origin`, avant tout
//! (Rulings R31 et R51 : une WebSocket échappe à CORS). L'existence de la page
//! est vérifiée ensuite, **avant** la poignée de main : une page inconnue
//! donne `404`, sans acteur ni écriture.
//!
//! `schema` est la version de schéma du client. Absente ou illisible, elle
//! vaut 0 : un client qui ne la donne pas est traité comme d'un autre schéma,
//! en lecture seule (il reçoit tout, ses mises à jour sont ignorées), et le
//! serveur le journalise.
//!
//! [`Hub::serve_connection`]: mementum_sync::Hub::serve_connection

use std::collections::HashMap;

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use mementum_sync::ConnInfo;
use mementum_sync::ws::WsBytes;

use crate::AppState;
use crate::error::ApiError;
use crate::guard;
use crate::routes::pages::page_id;
use crate::types::ErrorCode;

pub(crate) async fn sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    path: Result<Path<String>, PathRejection>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    if !guard::origin_allowed(
        guard::request_origin(&headers),
        guard::request_host(&headers, &uri),
        state.public_origin.as_deref(),
    ) {
        return Err(guard::bad_origin());
    }
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
        .and_then(|Query(q)| q.get("schema").and_then(|s| s.trim().parse::<u32>().ok()));
    let schema = match schema {
        Some(schema) => schema,
        None => {
            tracing::warn!(
                node_id = %id,
                "client sans version de schéma (paramètre schema absent ou illisible) : \
                 connexion en lecture seule"
            );
            0
        }
    };
    // Ruling R27 : axum refuse un message trop gros avant de le garder en
    // entier ; `serve_connection` contrôle la même borne.
    let max = state.hub.config().max_message_bytes;
    let hub = state.hub.clone();
    // En attendant les droits (Task 10b du lot 2) : tout le monde écrit, sans
    // compte.
    let conn = ConnInfo {
        client_schema: schema,
        can_write: true,
        actor: None,
    };
    Ok(upgrade
        .max_message_size(max)
        .max_frame_size(max)
        .on_failed_upgrade(move |error| {
            tracing::warn!(node_id = %id, %error, "poignée de main WebSocket manquée");
        })
        .on_upgrade(move |socket| async move {
            if let Err(error) = hub.serve_connection(id, conn, WsBytes(socket)).await {
                tracing::warn!(node_id = %id, %error, "connexion de synchronisation refusée");
            }
        }))
}
