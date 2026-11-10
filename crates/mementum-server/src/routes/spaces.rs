//! Routes des espaces et de leurs membres : créer, renommer, lister les membres, changer un
//! rôle, retirer quelqu'un ou partir.
//!
//! Les droits passent tous par `access.rs`. Qui n'a aucun rôle dans l'espace reçoit `404`,
//! comme pour un espace qui n'existe pas, avant que le corps de la requête soit lu : un corps
//! invalide ne lui apprend rien non plus. Tableau des rôles (« Modèle des droits ») :
//!
//! | | owner | admin | member | guest |
//! |---|---|---|---|---|
//! | voir les membres | oui | oui | oui | non |
//! | renommer l'espace | oui | oui | non | non |
//! | retirer quelqu'un | oui | oui, sauf un propriétaire | non | non |
//! | changer un rôle | tous | entre `admin` et `member` | non | non |
//! | partir | oui, sauf le dernier propriétaire | oui | oui | oui |
//!
//! Un changement de rôle et un retrait ferment les connexions de synchro de l'espace
//! (`rights_changed`), dans la tâche qui écrit : la suite va au bout même si le client coupe la
//! connexion.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mementum_acl::Role;
use mementum_store::StoreError;
use uuid::Uuid;

use crate::access::require;
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::types::{self, CreateSpace, ErrorCode, SpaceInfo, UpdateMember, UpdateSpace};
use crate::{AppState, to_completion};

/// Longueur maximale du nom d'un espace, en caractères Unicode.
pub(crate) const MAX_SPACE_NAME_CHARS: usize = 100;

/// `POST /api/v1/spaces` : `201`, l'espace, dont le compte est propriétaire.
pub(crate) async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    body: Result<Json<CreateSpace>, JsonRejection>,
) -> Result<(StatusCode, Json<SpaceInfo>), ApiError> {
    let Json(body) = body?;
    let name = space_name(&body.name)?;
    let space = match state.store.create_space(&name, Some(user.id)).await {
        Ok(space) => space,
        // Le compte a disparu depuis la lecture de sa session.
        Err(StoreError::NotFound) => return Err(ApiError::unauthenticated()),
        Err(e) => return Err(internal(e)),
    };
    tracing::info!(space_id = %space.id, user_id = %user.id, "espace créé");
    Ok((
        StatusCode::CREATED,
        Json(SpaceInfo {
            id: space.id.to_string(),
            name: space.name,
            role: Role::Owner.into(),
        }),
    ))
}

/// `PATCH /api/v1/spaces/{id}` : renomme l'espace (`admin`).
pub(crate) async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<UpdateSpace>, JsonRejection>,
) -> Result<Json<SpaceInfo>, ApiError> {
    let space_id = space_id(path)?;
    let role = state.role_at_least(&user, space_id, Role::Admin).await?;
    let Json(body) = body?;
    let name = space_name(&body.name)?;
    let space = match state.store.rename_space(space_id, &name).await {
        Ok(space) => space,
        Err(StoreError::NotFound) => return Err(ApiError::space_not_found()),
        Err(e) => return Err(internal(e)),
    };
    Ok(Json(SpaceInfo {
        id: space.id.to_string(),
        name: space.name,
        role: role.into(),
    }))
}

/// `GET /api/v1/spaces/{id}/members` : les personnes qui ont un rôle dans l'espace, invités
/// compris, dans l'ordre de leur arrivée (`member`).
pub(crate) async fn members(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Vec<types::Member>>, ApiError> {
    let space_id = space_id(path)?;
    state.role_at_least(&user, space_id, Role::Member).await?;
    let members = state.store.members(space_id).await.map_err(internal)?;
    Ok(Json(members.into_iter().map(member).collect()))
}

/// `PATCH /api/v1/spaces/{id}/members/{userId}` : change le rôle d'un membre (règles en tête
/// du fichier) ; `409 last_owner` si l'espace perdrait son dernier propriétaire.
pub(crate) async fn update_member(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<UpdateMember>, JsonRejection>,
) -> Result<Json<types::Member>, ApiError> {
    let (space_id, target) = member_path(path)?;
    let caller = state.role_at_least(&user, space_id, Role::Admin).await?;
    let Json(body) = body?;
    let target = target.ok_or_else(ApiError::member_not_found)?;
    let current = role_in(&state, space_id, target).await?;
    let wanted = Role::from(body.role);
    may_change_role(caller, current, wanted)?;
    if current != wanted {
        let writer = state.clone();
        to_completion(async move {
            writer
                .store
                .set_role(space_id, target, wanted)
                .await
                .map_err(member_error)?;
            writer.rights_changed(space_id, None).await;
            Ok(())
        })
        .await?;
        tracing::info!(
            space_id = %space_id,
            user_id = %user.id,
            member_id = %target,
            from = current.as_str(),
            to = wanted.as_str(),
            "rôle changé"
        );
    }
    let members = state.store.members(space_id).await.map_err(internal)?;
    members
        .into_iter()
        .find(|m| m.user_id == target)
        .map(|m| Json(member(m)))
        .ok_or_else(ApiError::member_not_found)
}

/// `DELETE /api/v1/spaces/{id}/members/{userId}` : retire quelqu'un de l'espace (`admin`, et
/// jamais un propriétaire s'il n'en est pas un), ou en part (soi-même, quel que soit son rôle) ;
/// `409 last_owner` pour le dernier propriétaire. Ses pages privées restent, invisibles ; ses
/// invitations en attente partent (Ruling R69) ; une page partagée ne reste pas sans personne
/// en accès complet (Ruling R70).
pub(crate) async fn remove_member(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let (space_id, target) = member_path(path)?;
    let caller = state.role_at_least(&user, space_id, Role::Guest).await?;
    let leaving = target == Some(user.id);
    if !leaving {
        require(caller, Role::Admin)?;
    }
    let target = target.ok_or_else(ApiError::member_not_found)?;
    let current = role_in(&state, space_id, target).await?;
    if !leaving && caller == Role::Admin && current == Role::Owner {
        return Err(ApiError::forbidden(
            "un admin ne retire pas un propriétaire de l'espace",
        ));
    }
    let writer = state.clone();
    to_completion(async move {
        writer
            .store
            .remove_member(space_id, target)
            .await
            .map_err(member_error)?;
        writer.rights_changed(space_id, None).await;
        Ok(())
    })
    .await?;
    if leaving {
        tracing::info!(space_id = %space_id, user_id = %user.id, "membre parti de l'espace");
    } else {
        tracing::info!(
            space_id = %space_id,
            user_id = %user.id,
            member_id = %target,
            "membre retiré de l'espace"
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

/// L'id d'un espace dans le chemin : illisible, l'espace n'existe pas.
pub(crate) fn space_id(path: Result<Path<String>, PathRejection>) -> Result<Uuid, ApiError> {
    path.ok()
        .and_then(|Path(id)| Uuid::parse_str(&id).ok())
        .ok_or_else(ApiError::space_not_found)
}

/// L'espace et la personne visée d'un chemin `…/spaces/{id}/members/{userId}`. Un espace
/// illisible n'existe pas (`404`) ; une personne illisible (`None`) n'est pas dans l'espace,
/// mais on ne le dit qu'à qui voit l'espace.
fn member_path(
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<(Uuid, Option<Uuid>), ApiError> {
    let Ok(Path((space, user))) = path else {
        return Err(ApiError::space_not_found());
    };
    let space = Uuid::parse_str(&space).map_err(|_| ApiError::space_not_found())?;
    Ok((space, Uuid::parse_str(&user).ok()))
}

/// Le rôle de `user_id` dans l'espace, sinon `404` (à qui voit l'espace).
async fn role_in(state: &AppState, space_id: Uuid, user_id: Uuid) -> Result<Role, ApiError> {
    state
        .store
        .role_in(space_id, user_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::member_not_found)
}

/// `caller` peut-il faire passer un membre de `current` à `wanted` ? Un propriétaire, oui ; un
/// admin, seulement entre `admin` et `member`, donc jamais pour un propriétaire.
fn may_change_role(caller: Role, current: Role, wanted: Role) -> Result<(), ApiError> {
    let between_admin_and_member = |role| matches!(role, Role::Admin | Role::Member);
    match caller {
        Role::Owner => Ok(()),
        Role::Admin if current == Role::Owner => Err(ApiError::forbidden(
            "un admin ne change pas le rôle d'un propriétaire de l'espace",
        )),
        Role::Admin if between_admin_and_member(current) && between_admin_and_member(wanted) => {
            Ok(())
        }
        _ => Err(ApiError::forbidden(
            "un admin ne fait passer un rôle qu'entre admin et member ; le reste revient aux \
             propriétaires",
        )),
    }
}

/// Règles du nom d'un espace : rogné, 1 à [`MAX_SPACE_NAME_CHARS`] caractères, sinon `422
/// invalid_name`.
fn space_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > MAX_SPACE_NAME_CHARS {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidName,
            format!("le nom d'un espace a 1 à {MAX_SPACE_NAME_CHARS} caractères"),
        ));
    }
    Ok(name.to_string())
}

fn member(m: mementum_store::Member) -> types::Member {
    types::Member {
        user_id: m.user_id.to_string(),
        name: m.name,
        email: m.email,
        role: m.role.into(),
    }
}

/// Erreur d'un changement de rôle ou d'un retrait : la personne a pu partir entre-temps.
fn member_error(error: StoreError) -> ApiError {
    match error {
        StoreError::NotFound => ApiError::member_not_found(),
        StoreError::LastOwner => ApiError::last_owner(),
        e => internal(e),
    }
}

/// Une erreur du store qui ne désigne ici aucune page : `500 internal`.
fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(&error)
}
