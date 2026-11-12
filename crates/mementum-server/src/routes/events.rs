//! `GET /events/{spaceId}` : le canal de l'espace (Ruling R133), une WebSocket qui ne porte que
//! la version de l'espace. Ce qui s'y passe une fois ouverte est dans `events.rs`.
//!
//! **Qui entre.** Les refus viennent **avant** la poignée de main, dans l'ordre de `/sync` et
//! par ses contrôles (`routes/sync.rs`) :
//! 1. `403 bad_origin` pour une en-tête `Origin` d'un autre site ;
//! 2. `401 unauthenticated` sans session ;
//! 3. `404 not_found` pour un espace où la personne n'a aucun rôle : inconnu, mal nommé, ou dont
//!    elle n'est ni membre ni invitée, admin de l'instance compris. La réponse est celle de toute
//!    route d'un espace introuvable (Ruling R152), la même dans tous ces cas ; pour qui n'a aucun
//!    rôle, l'espace n'est pas lu (Ruling R99) ;
//! 4. `400 invalid_request` sans demande de WebSocket.
//!
//! Un invité ouvre le canal comme un membre : il n'y apprend que des numéros.
//!
//! Comme pour `/sync`, ce contrôle ne sert qu'aux refus : rien de ce qu'il lit n'entre dans la
//! connexion, qui revérifie la session et le rôle avant sa première trame. Les en-têtes sont
//! ceux de `/sync`, pas ceux de l'API (`Cache-Control: no-store` ne concerne que `/api/…`).

use axum::extract::rejection::PathRejection;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use mementum_acl::Role;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::events::{self, MAX_INCOMING_BYTES};
use crate::routes::spaces::space_id;
use crate::routes::sync::{Caller, caller, websocket};

pub(crate) async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    user: Result<AuthUser, ApiError>,
    path: Result<Path<String>, PathRejection>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    let Caller { user, session } = caller(&state, &headers, &uri, user)?;
    let space = space_id(path)?;
    state.role_at_least(&user, space, Role::Guest).await?;
    let upgrade = websocket(upgrade)?;
    Ok(upgrade
        .max_message_size(MAX_INCOMING_BYTES)
        .max_frame_size(MAX_INCOMING_BYTES)
        .on_failed_upgrade(move |error| {
            tracing::warn!(space_id = %space, %error, "poignée de main WebSocket manquée (canal de l'espace)");
        })
        .on_upgrade(move |socket| async move {
            events::serve(state, user, session, space, socket).await;
        }))
}
