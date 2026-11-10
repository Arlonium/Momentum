//! Routes des invitations : inviter dans un espace, lister et révoquer celles qui attendent,
//! consulter un lien sans session, l'accepter.
//!
//! C'est le jeton qui fait foi, jamais l'adresse e-mail, qui n'est pas vérifiée (Ruling R47).
//! Le jeton (32 octets aléatoires en base64url) n'est montré qu'une fois, dans la réponse qui
//! crée l'invitation ; le serveur n'en garde que le SHA-256, et ne le journalise jamais. Une
//! invitation dure [`INVITATION_TTL_MS`] et sert une fois. Un jeton inconnu, mal formé, échu,
//! révoqué ou déjà accepté reçoit toujours la même réponse, `404 invalid_invitation`.
//!
//! [`valid_invitation`] et [`accept_invitation`] sont les seuls chemins par lesquels le serveur
//! consulte et accepte une invitation par son jeton, inscription comprise
//! (`routes/auth.rs`) ; l'acceptation passe par la consultation. Un contrôle de plus sur les
//! invitations (l'auteur d'une invitation de page a-t-il encore `full` sur la page, Ruling
//! R69) n'a donc qu'un endroit où s'ajouter : [`valid_invitation`].
//!
//! Une invitation de page n'est listée, et ne se révoque ici, que pour qui peut partager la
//! page : un admin ne voit pas les pages privées des autres, ni donc qui y est invité.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mementum_acl::{Action, Role, SpaceAccess};
use mementum_store::{Invitation, InvitationGrant, NewInvitation, StoreError};
use uuid::Uuid;

use crate::access::at_least;
use crate::auth::{self, AuthUser};
use crate::error::ApiError;
use crate::routes::spaces::space_id;
use crate::types::{
    Accepted, InvitationCreated, InvitationInfo, InvitationToken, InviteToSpace, PendingInvitation,
};
use crate::{AppState, now_ms};

/// Durée d'une invitation, en millisecondes : 7 jours.
pub(crate) const INVITATION_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// `POST /api/v1/spaces/{id}/invitations` : `201`, l'id de l'invitation et son jeton, montré
/// cette fois seulement (`admin`).
pub(crate) async fn invite_to_space(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<InviteToSpace>, JsonRejection>,
) -> Result<(StatusCode, Json<InvitationCreated>), ApiError> {
    let space_id = space_id(path)?;
    state.role_at_least(&user, space_id, Role::Admin).await?;
    let Json(body) = body?;
    let email = auth::normalize_email(&body.email);
    if !auth::email_is_valid(&email) {
        return Err(ApiError::invalid_email());
    }
    let role = Role::from(body.role);
    let (token, token_hash) = auth::new_token();
    let invitation = state
        .store
        .create_invitation(NewInvitation {
            space_id,
            grant: InvitationGrant::Space(role),
            email,
            token_hash,
            created_by: user.id,
            expires_at: now_ms() + INVITATION_TTL_MS,
        })
        .await
        .map_err(|e| match e {
            // L'espace, ou le compte, a disparu depuis la lecture des droits.
            StoreError::NotFound => ApiError::space_not_found(),
            e => internal(e),
        })?;
    tracing::info!(
        invitation_id = %invitation.id,
        space_id = %space_id,
        user_id = %user.id,
        role = role.as_str(),
        "invitation créée"
    );
    Ok((
        StatusCode::CREATED,
        Json(InvitationCreated {
            id: invitation.id.to_string(),
            token,
        }),
    ))
}

/// `GET /api/v1/spaces/{id}/invitations` : les invitations de l'espace qui attendent encore,
/// sans leur jeton (`admin`). Celles des pages que la personne ne peut pas partager n'y sont
/// pas.
pub(crate) async fn space_invitations(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Vec<PendingInvitation>>, ApiError> {
    let space_id = space_id(path)?;
    let access = state.space_access(&user, space_id).await?;
    at_least(&access, Role::Admin)?;
    let pending = state
        .store
        .pending_invitations(space_id)
        .await
        .map_err(internal)?;
    Ok(Json(
        pending
            .into_iter()
            .filter(|i| may_manage(&access, i))
            .map(pending_invitation)
            .collect(),
    ))
}

/// `DELETE /api/v1/invitations/{id}` : `204`, l'invitation est supprimée, par qui peut la
/// lister (`admin` de son espace ; pour une invitation de page, qui peut aussi partager la
/// page). Pour tout autre, et pour une invitation inconnue, échue ou acceptée, la même
/// réponse, `404` : qui ne peut pas la lister n'apprend pas qu'elle existe.
pub(crate) async fn revoke(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let id = path
        .ok()
        .and_then(|Path(id)| Uuid::parse_str(&id).ok())
        .ok_or_else(ApiError::invitation_not_found)?;
    let invitation = state
        .store
        .get_invitation(id, now_ms())
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::invitation_not_found)?;
    let access = state
        .find_space_access(&user, invitation.space_id)
        .await?
        .ok_or_else(ApiError::invitation_not_found)?;
    if at_least(&access, Role::Admin).is_err() || !may_manage(&access, &invitation) {
        return Err(ApiError::invitation_not_found());
    }
    match state.store.revoke_invitation(id).await {
        Ok(()) => {}
        // Acceptée ou révoquée entre-temps.
        Err(StoreError::NotFound) => return Err(ApiError::invitation_not_found()),
        Err(e) => return Err(internal(e)),
    }
    tracing::info!(
        invitation_id = %id,
        space_id = %invitation.space_id,
        user_id = %user.id,
        "invitation révoquée"
    );
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/invitations/lookup`, sans session : ce que dit le lien (l'espace, la page,
/// qui invite, à quelle adresse), sans rien accepter.
pub(crate) async fn lookup(
    State(state): State<AppState>,
    body: Result<Json<InvitationToken>, JsonRejection>,
) -> Result<Json<InvitationInfo>, ApiError> {
    let Json(body) = body?;
    let hash = auth::token_hash(&body.token).ok_or_else(ApiError::invalid_invitation)?;
    let invitation = valid_invitation(&state, hash)
        .await?
        .ok_or_else(ApiError::invalid_invitation)?;
    // L'invitation vient d'être lue : son espace, sa page et son auteur existent (clés
    // étrangères). Disparus depuis, elle ne vaut plus.
    let space = state
        .store
        .get_space(invitation.space_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::invalid_invitation)?;
    let page_title = match invitation.grant {
        InvitationGrant::Space(_) => None,
        InvitationGrant::Page { node_id, .. } => Some(
            state
                .store
                .get_node(node_id)
                .await
                .map_err(internal)?
                .ok_or_else(ApiError::invalid_invitation)?
                .title,
        ),
    };
    let invited_by = state
        .store
        .get_user(invitation.created_by)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::invalid_invitation)?
        .name;
    Ok(Json(InvitationInfo {
        space_name: space.name,
        page_title,
        invited_by,
        email: invitation.email,
    }))
}

/// `POST /api/v1/invitations/accept` : le compte de la session reçoit ce que donne
/// l'invitation (sans que rien de ce qu'il avait ne baisse), puis elle ne sert plus.
pub(crate) async fn accept(
    State(state): State<AppState>,
    user: AuthUser,
    body: Result<Json<InvitationToken>, JsonRejection>,
) -> Result<Json<Accepted>, ApiError> {
    let Json(body) = body?;
    let hash = auth::token_hash(&body.token).ok_or_else(ApiError::invalid_invitation)?;
    let invitation = accept_invitation(&state, hash, user.id)
        .await?
        .ok_or_else(ApiError::invalid_invitation)?;
    Ok(Json(Accepted {
        space_id: invitation.space_id.to_string(),
        page_id: match invitation.grant {
            InvitationGrant::Space(_) => None,
            InvitationGrant::Page { node_id, .. } => Some(node_id.to_string()),
        },
    }))
}

/// L'invitation de ce jeton (son SHA-256), si elle vaut encore : `None` si elle est inconnue,
/// échue, révoquée, déjà acceptée, ou si son auteur n'a plus le rôle qu'elle demande (le store
/// le vérifie, Ruling R69).
///
/// Le seul chemin par lequel le serveur consulte une invitation par son jeton : la
/// consultation d'un lien, l'inscription et [`accept_invitation`].
pub(crate) async fn valid_invitation(
    state: &AppState,
    token_hash: [u8; 32],
) -> Result<Option<Invitation>, ApiError> {
    state
        .store
        .invitation_by_token(token_hash, now_ms())
        .await
        .map_err(internal)
}

/// Accepte l'invitation de ce jeton pour le compte `user_id` : `None` si elle ne vaut plus
/// (voir [`valid_invitation`], par où elle passe d'abord), ou si elle a cessé de valoir à
/// l'instant de l'écriture, que le store revérifie dans sa transaction.
///
/// Le seul chemin par lequel le serveur accepte une invitation : `POST
/// /api/v1/invitations/accept` et l'inscription.
pub(crate) async fn accept_invitation(
    state: &AppState,
    token_hash: [u8; 32],
    user_id: Uuid,
) -> Result<Option<Invitation>, ApiError> {
    if valid_invitation(state, token_hash).await?.is_none() {
        return Ok(None);
    }
    match state
        .store
        .accept_invitation(token_hash, user_id, now_ms())
        .await
    {
        Ok(invitation) => {
            tracing::info!(
                invitation_id = %invitation.id,
                space_id = %invitation.space_id,
                user_id = %user_id,
                "invitation acceptée"
            );
            Ok(Some(invitation))
        }
        Err(StoreError::NotFound) => Ok(None),
        Err(e) => Err(internal(e)),
    }
}

/// La personne de `access` peut-elle voir et révoquer cette invitation ? Une invitation
/// d'espace : oui (la route vérifie le rôle). Une invitation de page : si elle peut partager
/// la page.
fn may_manage(access: &SpaceAccess, invitation: &Invitation) -> bool {
    match invitation.grant {
        InvitationGrant::Space(_) => true,
        InvitationGrant::Page { node_id, .. } => access.can(Action::Share, node_id),
    }
}

fn pending_invitation(invitation: Invitation) -> PendingInvitation {
    let (role, level, page_id) = match invitation.grant {
        InvitationGrant::Space(role) => (Some(role.into()), None, None),
        InvitationGrant::Page { node_id, level } => {
            (None, Some(level.into()), Some(node_id.to_string()))
        }
    };
    PendingInvitation {
        id: invitation.id.to_string(),
        email: invitation.email,
        role,
        level,
        page_id,
        expires_at: invitation.expires_at,
    }
}

/// Une erreur du store qui ne désigne ici aucune page : `500 internal`.
fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(&error)
}
