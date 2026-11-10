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
//! Un changement de rôle et un retrait se décident sur les rôles lus par la route, que le store
//! revérifie dans la transaction qui écrit (`Expected`) : s'ils ont changé entre-temps, rien
//! n'est écrit, et la route relit et redécide (deux reprises au plus, puis `409
//! roles_changed`). Ils ferment ensuite les connexions de synchro de l'espace
//! (`rights_changed`). Écriture, journal et fermeture vont dans une tâche à part : ils vont au
//! bout même si le client coupe la connexion.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mementum_acl::Role;
use mementum_store::{Expected, StoreError};
use uuid::Uuid;

use crate::access::require;
use crate::auth::AuthUser;
use crate::error::{ApiError, internal};
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
    // Sous `admin`, `403` avant de regarder la personne visée.
    let caller = state.role_at_least(&user, space_id, Role::Admin).await?;
    let Json(body) = body?;
    let target = target.ok_or_else(ApiError::member_not_found)?;
    let roles = Roles {
        caller,
        target: role_in(&state, space_id, target).await?,
    };
    let wanted = Role::from(body.role);
    let writer = state.clone();
    to_completion(
        async move { change_role(&writer, &user, space_id, target, wanted, roles).await },
    )
    .await?;
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
    // Sous `admin`, `403` avant de regarder la personne visée : un invité, qui ne voit pas les
    // membres, n'apprend pas ainsi qui l'est.
    if target != Some(user.id) {
        require(caller, Role::Admin)?;
    }
    let target = target.ok_or_else(ApiError::member_not_found)?;
    let roles = Roles {
        caller,
        target: role_in(&state, space_id, target).await?,
    };
    let writer = state.clone();
    to_completion(async move { remove(&writer, &user, space_id, target, roles).await }).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Reprises d'une décision quand les rôles ont changé entre leur lecture et l'écriture.
const ROLE_RETRIES: usize = 2;

/// Les rôles sur lesquels une route de membres décide : celui de l'appelant, celui de la
/// personne visée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Roles {
    caller: Role,
    target: Role,
}

impl Roles {
    /// Ce que le store doit retrouver dans sa transaction pour écrire.
    fn expected(self, actor: &AuthUser) -> Expected {
        Expected {
            actor: actor.id,
            actor_role: self.caller,
            target_role: self.target,
        }
    }
}

/// Relit les rôles de l'appelant (`404` s'il n'en a plus) et de la personne visée (`404` si
/// elle n'en a plus). Comme à la première lecture, hors départ volontaire (`leaving`),
/// l'appelant passé sous `admin` reçoit `403` avant que la personne visée soit regardée.
async fn read_roles(
    state: &AppState,
    user: &AuthUser,
    space_id: Uuid,
    target: Uuid,
    leaving: bool,
) -> Result<Roles, ApiError> {
    let caller = state.role_at_least(user, space_id, Role::Guest).await?;
    if !leaving {
        require(caller, Role::Admin)?;
    }
    Ok(Roles {
        caller,
        target: role_in(state, space_id, target).await?,
    })
}

/// Passe `target` au rôle `wanted`, décidé sur `roles`. Le store revérifie ces rôles dans la
/// transaction qui écrit : s'ils ont changé entre-temps (`Stale`), ou si la personne est
/// partie, la route les relit et redécide, [`ROLE_RETRIES`] fois au plus, puis `409
/// roles_changed`. Le journal porte le rôle d'avant lu dans cette transaction.
async fn change_role(
    state: &AppState,
    user: &AuthUser,
    space_id: Uuid,
    target: Uuid,
    wanted: Role,
    mut roles: Roles,
) -> Result<(), ApiError> {
    let mut retries = 0;
    loop {
        may_change_role(roles.caller, roles.target, wanted)?;
        if roles.target == wanted {
            return Ok(());
        }
        match state
            .store
            .set_role(space_id, target, wanted, roles.expected(user))
            .await
        {
            Ok(before) => {
                tracing::info!(
                    space_id = %space_id,
                    user_id = %user.id,
                    member_id = %target,
                    from = before.as_str(),
                    to = wanted.as_str(),
                    "rôle changé"
                );
                state.rights_changed(space_id, None).await;
                return Ok(());
            }
            Err(StoreError::Stale | StoreError::NotFound) if retries < ROLE_RETRIES => {
                retries += 1;
                log_reprise(space_id, target, retries);
                roles = read_roles(state, user, space_id, target, false).await?;
            }
            Err(e) => return Err(member_error(e)),
        }
    }
}

/// Retire `target` de l'espace (ou l'en fait partir, si c'est l'appelant), décidé sur
/// `roles`, avec les mêmes reprises que [`change_role`].
async fn remove(
    state: &AppState,
    user: &AuthUser,
    space_id: Uuid,
    target: Uuid,
    mut roles: Roles,
) -> Result<(), ApiError> {
    let leaving = target == user.id;
    let mut retries = 0;
    loop {
        may_remove(roles, leaving)?;
        match state
            .store
            .remove_member(space_id, target, roles.expected(user))
            .await
        {
            Ok(()) => {
                if leaving {
                    tracing::info!(
                        space_id = %space_id,
                        user_id = %user.id,
                        role = roles.target.as_str(),
                        "membre parti de l'espace"
                    );
                } else {
                    tracing::info!(
                        space_id = %space_id,
                        user_id = %user.id,
                        member_id = %target,
                        role = roles.target.as_str(),
                        "membre retiré de l'espace"
                    );
                }
                state.rights_changed(space_id, None).await;
                return Ok(());
            }
            Err(StoreError::Stale | StoreError::NotFound) if retries < ROLE_RETRIES => {
                retries += 1;
                log_reprise(space_id, target, retries);
                roles = read_roles(state, user, space_id, target, leaving).await?;
            }
            Err(e) => return Err(member_error(e)),
        }
    }
}

/// Journal d'une reprise : le store a refusé l'écriture décidée sur les rôles lus (ils ont
/// changé, ou la personne visée est partie), la route les relit et redécide.
fn log_reprise(space_id: Uuid, target: Uuid, retries: usize) {
    tracing::debug!(
        space_id = %space_id,
        member_id = %target,
        retries,
        "les rôles lus ne tenaient plus à l'écriture : la route les relit et redécide"
    );
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
/// admin, seulement entre `admin` et `member`, donc jamais pour un propriétaire ; personne
/// d'autre.
fn may_change_role(caller: Role, current: Role, wanted: Role) -> Result<(), ApiError> {
    require(caller, Role::Admin)?;
    let between_admin_and_member = |role| matches!(role, Role::Admin | Role::Member);
    match caller {
        Role::Owner => Ok(()),
        _ if current == Role::Owner => Err(ApiError::forbidden(
            "un admin ne change pas le rôle d'un propriétaire de l'espace",
        )),
        _ if between_admin_and_member(current) && between_admin_and_member(wanted) => Ok(()),
        _ => Err(ApiError::forbidden(
            "un admin ne fait passer un rôle qu'entre admin et membre ; le reste revient aux \
             propriétaires",
        )),
    }
}

/// L'appelant peut-il retirer la personne visée ? Pour partir, toujours (le dernier
/// propriétaire, c'est le store qui le refuse) ; sinon, il faut être `admin`, et `owner` pour
/// retirer un propriétaire.
fn may_remove(roles: Roles, leaving: bool) -> Result<(), ApiError> {
    if leaving {
        return Ok(());
    }
    require(roles.caller, Role::Admin)?;
    if roles.caller != Role::Owner && roles.target == Role::Owner {
        return Err(ApiError::forbidden(
            "un admin ne retire pas un propriétaire de l'espace",
        ));
    }
    Ok(())
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

/// Erreur d'un changement de rôle ou d'un retrait, une fois les reprises épuisées.
fn member_error(error: StoreError) -> ApiError {
    match error {
        StoreError::NotFound => ApiError::member_not_found(),
        StoreError::LastOwner => ApiError::last_owner(),
        // Les rôles ont bougé à chaque reprise.
        StoreError::Stale => ApiError::roles_changed(),
        e => internal(e),
    }
}

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;
    use mementum_store::Space;

    use super::*;
    use crate::access::tests::{account, state};

    /// Olga (propriétaire), Adam (admin) et Mona (admin) dans un espace.
    async fn team(state: &AppState) -> (AuthUser, AuthUser, AuthUser, Space) {
        let olga = account(state, "olga").await;
        let adam = account(state, "adam").await;
        let mona = account(state, "mona").await;
        let space = state
            .store
            .create_space("Équipe", Some(olga.id))
            .await
            .unwrap();
        for who in [&adam, &mona] {
            state
                .store
                .add_member(space.id, who.id, Role::Admin)
                .await
                .unwrap();
        }
        (olga, adam, mona, space)
    }

    fn status(result: Result<(), ApiError>) -> StatusCode {
        result.unwrap_err().into_response().status()
    }

    async fn role(state: &AppState, space: &Space, who: &AuthUser) -> Option<Role> {
        state.store.role_in(space.id, who.id).await.unwrap()
    }

    /// Adam (admin) a lu Mona admin ; Olga la fait propriétaire avant qu'il écrive. Le store
    /// refuse l'écriture décidée sur ce qu'Adam avait lu, la route relit et redécide : `403`,
    /// un admin ne touche pas un propriétaire. Rien n'a changé.
    #[tokio::test]
    async fn admin_cannot_demote_someone_just_made_owner() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let (olga, adam, mona, space) = team(&state).await;
        // Ce que la route d'Adam a lu avant d'écrire.
        let read = Roles {
            caller: Role::Admin,
            target: Role::Admin,
        };
        let promoted = Roles {
            caller: Role::Owner,
            target: Role::Admin,
        };
        change_role(&state, &olga, space.id, mona.id, Role::Owner, promoted)
            .await
            .unwrap();
        let rev = state.store.space_rev(space.id).await.unwrap();

        let result = change_role(&state, &adam, space.id, mona.id, Role::Member, read).await;
        assert_eq!(status(result), StatusCode::FORBIDDEN);
        // Ni la rétrograder, ni la retirer.
        let result = remove(&state, &adam, space.id, mona.id, read).await;
        assert_eq!(status(result), StatusCode::FORBIDDEN);
        assert_eq!(role(&state, &space, &mona).await, Some(Role::Owner));
        assert_eq!(state.store.space_rev(space.id).await.unwrap(), rev);

        // Un admin rétrogradé entre sa lecture et son écriture n'agit plus : la décision
        // reprise sur son rôle relu (`member`) est un refus.
        let demoted = Roles {
            caller: Role::Owner,
            target: Role::Admin,
        };
        change_role(&state, &olga, space.id, adam.id, Role::Member, demoted)
            .await
            .unwrap();
        let mona_admin = Roles {
            caller: Role::Owner,
            target: Role::Owner,
        };
        change_role(&state, &olga, space.id, mona.id, Role::Admin, mona_admin)
            .await
            .unwrap();
        let read = Roles {
            caller: Role::Admin,
            target: Role::Admin,
        };
        let result = change_role(&state, &adam, space.id, mona.id, Role::Member, read).await;
        assert_eq!(status(result), StatusCode::FORBIDDEN);
        let result = remove(&state, &adam, space.id, mona.id, read).await;
        assert_eq!(status(result), StatusCode::FORBIDDEN);
        assert_eq!(role(&state, &space, &mona).await, Some(Role::Admin));

        // Sur des rôles justes, la même fonction écrit du premier coup.
        let read = Roles {
            caller: Role::Owner,
            target: Role::Admin,
        };
        change_role(&state, &olga, space.id, mona.id, Role::Member, read)
            .await
            .unwrap();
        assert_eq!(role(&state, &space, &mona).await, Some(Role::Member));
    }

    /// Sur la reprise après un refus du store, l'appelant est contrôlé avant la personne visée,
    /// comme à la première lecture : Adam, passé invité pendant que Mona quittait l'espace,
    /// reçoit `403`, pas le `404` d'une personne qui n'est plus là.
    #[tokio::test]
    async fn demoted_caller_is_refused_before_the_target_is_looked_up() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let (olga, adam, mona, space) = team(&state).await;
        // Ce que les routes d'Adam ont lu : lui admin, Mona admin.
        let read = Roles {
            caller: Role::Admin,
            target: Role::Admin,
        };
        let by_olga = Roles {
            caller: Role::Owner,
            target: Role::Admin,
        };
        change_role(&state, &olga, space.id, adam.id, Role::Guest, by_olga)
            .await
            .unwrap();
        remove(&state, &olga, space.id, mona.id, by_olga)
            .await
            .unwrap();

        let result = remove(&state, &adam, space.id, mona.id, read).await;
        assert_eq!(status(result), StatusCode::FORBIDDEN);
        let result = change_role(&state, &adam, space.id, mona.id, Role::Member, read).await;
        assert_eq!(status(result), StatusCode::FORBIDDEN);
    }
}
