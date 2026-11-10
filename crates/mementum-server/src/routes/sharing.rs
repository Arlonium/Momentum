//! Routes du partage d'une page : qui y a accès et à quel niveau, l'accès général (la ligne
//! `members`), l'accès d'une personne, les invitations sur la page.
//!
//! | Route | Réponse |
//! |---|---|
//! | `GET /api/v1/pages/{id}/access` | [`PageAccess`] |
//! | `PUT /api/v1/pages/{id}/access/general` ([`SetGeneral`]) | [`PageAccess`] ; `level: null` = « seulement les invités » : la ligne `members` de la page est retirée |
//! | `PUT /api/v1/pages/{id}/access/users/{userId}` ([`SetLevel`]) | [`PageAccess`] ; `404` si le compte n'a aucun rôle dans l'espace |
//! | `DELETE /api/v1/pages/{id}/access/users/{userId}` | [`PageAccess`] ; `404` si la page n'a pas de ligne propre pour ce compte |
//! | `POST /api/v1/pages/{id}/invitations` ([`InviteToPage`]) | `201`, `InvitationCreated` |
//!
//! **Droits.** Toutes demandent `Share`, donc `full` sur la page, par `access.rs`, avant que le
//! corps soit lu : `404` pour qui ne la voit pas, la réponse d'une page qui n'existe pas ;
//! `403 forbidden` sous `full` ; `409 trashed` pour un changement sur une page à la corbeille,
//! dont les accès se lisent encore. `GET …/access` porte le nom et l'adresse des comptes : il
//! n'est rendu qu'à qui a `full` ; après un changement qui lui ôte `full` (on peut se retirer
//! soi-même), la réponse est `204`, sans corps.
//!
//! **Règles.**
//! - on donne `read`, `comment`, `edit` ou `full`, à un compte qui a un rôle dans l'espace,
//!   membre ou invité. Un compte sans rôle répond comme un compte inconnu ;
//! - après tout changement, la page garde quelqu'un en `full`, par ses lignes ou par celles
//!   d'un ancêtre (Ruling R70 : une ligne `members` compte, les admins y ont `full` ; une ligne
//!   restée à quelqu'un sans rôle, non) ; sinon `409 last_full_access`, et rien n'est écrit.
//!   C'est le store qui le vérifie, dans la transaction de l'écriture
//!   (`Store::change_access`), sur la même notion que le départ d'un membre ;
//! - retirer son dernier accès à un invité le fait sortir de l'espace (même transaction) ;
//! - **décision et écriture tiennent ensemble** : la route lit la révision de l'espace
//!   (`spaces.rev`), puis décide (le droit de l'appelant, le rôle de la personne visée, la
//!   ligne à retirer) sur ce qu'elle lit ensuite ; le store n'écrit que si l'espace est encore
//!   à cette révision. Sinon la route relit et redécide, [`ACCESS_RETRIES`] fois au plus, puis
//!   `409 access_changed`. Un appelant qui perd `full` entre sa lecture et son écriture
//!   n'écrit donc rien ;
//! - après le commit, les connexions de synchro de la page et de ses sous-pages sont fermées
//!   (`rights_changed`), et la page sort de l'arbre de qui l'a perdue à l'appel suivant (le
//!   cache des accès suit `spaces.rev`). Écriture et fermeture vont dans une tâche à part :
//!   elles vont au bout même si le client coupe la connexion. Un changement qui ne change rien
//!   n'écrit rien et ne ferme rien ;
//! - une invitation sur une page ne cherche aucun compte : la réponse est la même, que
//!   l'adresse en ait un ou non (Ruling R47). Elle ne vaut que tant que son auteur peut
//!   partager la page (Ruling R69, `invitations::valid_invitation`). Ce n'est pas un droit : la
//!   créer ne ferme aucune connexion.

use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mementum_acl::{Action, Level, NodeAccess, Subject};
use mementum_store::{AccessChange, AccessWritten, InvitationGrant, Node, StoreError};
use uuid::Uuid;

use crate::access::permit;
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::routes::invitations::{self, pending_invitation};
use crate::routes::pages::page_id;
use crate::types::{
    AccessEntry, InvitationCreated, InviteToPage, PageAccess, SetGeneral, SetLevel,
};
use crate::{AppState, to_completion};

/// Reprises d'un changement d'accès quand l'espace a bougé entre la décision et l'écriture.
const ACCESS_RETRIES: usize = 2;

/// `GET /api/v1/pages/{id}/access` : qui a accès à la page, et à quel niveau (`full`). Une
/// page à la corbeille se lit encore : ses accès aussi.
pub(crate) async fn access(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<PageAccess>, ApiError> {
    let id = page_id(path)?;
    let (node, access, _) = state
        .seen_node(&user, id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    // Lire n'écrit pas : la corbeille ne compte pas ici, seul le niveau.
    let reading = NodeAccess {
        in_trash: false,
        ..access
    };
    permit(&reading, Action::Share)?;
    Ok(Json(page_access(&state, &node).await?))
}

/// `PUT /api/v1/pages/{id}/access/general` : pose la ligne `members` de la page à ce niveau,
/// ou la retire (`null`).
pub(crate) async fn set_general(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<SetGeneral>, JsonRejection>,
) -> Result<Response, ApiError> {
    let id = page_id(path)?;
    // La page d'abord : `404` ou `403` avant que le corps soit lu.
    state.node_access(&user, id, Action::Share).await?;
    let Json(body) = body?;
    let change = AccessChange::Members(body.level.map(Level::from));
    change_and_answer(state, user, id, change).await
}

/// `PUT /api/v1/pages/{id}/access/users/{userId}` : donne ce niveau sur la page au compte, qui
/// doit avoir un rôle dans l'espace (sinon `404`, comme pour un compte inconnu).
pub(crate) async fn set_user(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<SetLevel>, JsonRejection>,
) -> Result<Response, ApiError> {
    let (id, target) = user_path(path)?;
    state.node_access(&user, id, Action::Share).await?;
    let Json(body) = body?;
    let target = target.ok_or_else(ApiError::member_not_found)?;
    let change = AccessChange::User(target, Some(body.level.into()));
    change_and_answer(state, user, id, change).await
}

/// `DELETE /api/v1/pages/{id}/access/users/{userId}` : retire la ligne du compte sur la page
/// (`404` s'il n'en a pas : un accès hérité se retire sur la page qui le porte). Un invité qui
/// n'a plus aucune ligne dans l'espace en sort.
pub(crate) async fn remove_user(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let (id, target) = user_path(path)?;
    state.node_access(&user, id, Action::Share).await?;
    let target = target.ok_or_else(ApiError::access_not_found)?;
    change_and_answer(state, user, id, AccessChange::User(target, None)).await
}

/// `POST /api/v1/pages/{id}/invitations` : `201`, l'id de l'invitation et son jeton, montré
/// cette fois seulement. La réponse ne dit pas si l'adresse a un compte.
pub(crate) async fn invite(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<InviteToPage>, JsonRejection>,
) -> Result<(StatusCode, Json<InvitationCreated>), ApiError> {
    let id = page_id(path)?;
    let (node, _) = state.node_access(&user, id, Action::Share).await?;
    let Json(body) = body?;
    let grant = InvitationGrant::Page {
        node_id: id,
        level: body.level.into(),
    };
    invitations::create(&state, &user, node.space_id, grant, &body.email).await
}

/// La page et le compte visé d'un chemin `…/pages/{id}/access/users/{userId}`. Une page
/// illisible n'existe pas (`404`) ; un compte illisible (`None`) n'existe pas non plus, mais on
/// ne le dit qu'à qui peut partager la page.
fn user_path(
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<(Uuid, Option<Uuid>), ApiError> {
    let Ok(Path((page, user))) = path else {
        return Err(ApiError::page_not_found());
    };
    let page = Uuid::parse_str(&page).map_err(|_| ApiError::page_not_found())?;
    Ok((page, Uuid::parse_str(&user).ok()))
}

/// Écrit `change` (décidé, puis écrit dans une tâche à part), puis répond : les accès de la
/// page si l'appelant y a encore `full`, sinon `204`.
async fn change_and_answer(
    state: AppState,
    user: AuthUser,
    id: Uuid,
    change: AccessChange,
) -> Result<Response, ApiError> {
    let writer = state.clone();
    let by = user.clone();
    to_completion(async move {
        let decided = decide(&writer, &by, id, change).await?;
        write(&writer, &by, id, change, decided).await
    })
    .await?;
    match state.seen_node(&user, id).await? {
        Some((node, access, _)) if access.level.allows(Action::Share) => {
            Ok(Json(page_access(&state, &node).await?).into_response())
        }
        // Le changement est fait, mais l'appelant n'a plus `full` sur la page (il s'est retiré
        // son accès, ou l'a baissé) : il n'en voit plus les accès.
        _ => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

/// Une décision prise par [`decide`] : l'espace de la page, et la révision sur laquelle elle a
/// été prise.
#[derive(Debug, Clone, Copy)]
struct Decided {
    space_id: Uuid,
    rev: i64,
}

/// Décide `change` sur la page `id` pour `user`. La révision de l'espace est lue d'abord, et
/// tout ce qui décide (le droit de `user` de partager la page, ce que vise le changement) est
/// lu à elle ou après : le store n'écrira que si l'espace y est encore.
async fn decide(
    state: &AppState,
    user: &AuthUser,
    id: Uuid,
    change: AccessChange,
) -> Result<Decided, ApiError> {
    let space_id = state
        .store
        .get_node(id)
        .await?
        .ok_or_else(ApiError::page_not_found)?
        .space_id;
    let rev = state
        .store
        .space_rev(space_id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    state.node_access(user, id, Action::Share).await?;
    match change {
        AccessChange::Members(_) => {}
        // Un accès ne se donne qu'à quelqu'un de l'espace ; un compte sans rôle répond comme un
        // compte inconnu.
        AccessChange::User(target, Some(_)) => {
            if state.store.role_in(space_id, target).await?.is_none() {
                return Err(ApiError::member_not_found());
            }
        }
        // Seule une ligne posée sur la page se retire ici.
        AccessChange::User(target, None) => {
            let own = state.store.node_grants(id).await?;
            if !own.iter().any(|g| g.subject == Subject::User(target)) {
                return Err(ApiError::access_not_found());
            }
        }
    }
    Ok(Decided { space_id, rev })
}

/// Écrit `change`, décidé par [`decide`]. Si l'espace a bougé depuis la décision, le store
/// n'écrit rien (`Stale`) : elle est reprise sur ce qui est relu, [`ACCESS_RETRIES`] fois au
/// plus, puis `409 access_changed`. Après le commit, les connexions de synchro du sous-arbre de
/// la page sont fermées.
async fn write(
    state: &AppState,
    user: &AuthUser,
    id: Uuid,
    change: AccessChange,
    mut decided: Decided,
) -> Result<AccessWritten, ApiError> {
    let mut retries = 0;
    loop {
        match state.store.change_access(id, change, decided.rev).await {
            Ok(written) => {
                if written.changed {
                    log_change(user, id, decided.space_id, change, written);
                    state.subtree_rights_changed(decided.space_id, id).await;
                }
                return Ok(written);
            }
            Err(StoreError::Stale) if retries < ACCESS_RETRIES => {
                retries += 1;
                decided = decide(state, user, id, change).await?;
            }
            Err(StoreError::Stale) => return Err(ApiError::access_changed()),
            Err(e) => return Err(e.into()),
        }
    }
}

/// Journal d'un changement écrit : qui, sur quelle page, pour qui, à quel niveau ; jamais
/// d'adresse.
fn log_change(
    user: &AuthUser,
    id: Uuid,
    space_id: Uuid,
    change: AccessChange,
    written: AccessWritten,
) {
    let level = |level: Option<Level>| level.map_or("aucun", Level::as_str);
    match change {
        AccessChange::Members(to) => tracing::info!(
            node_id = %id,
            space_id = %space_id,
            user_id = %user.id,
            level = level(to),
            "accès général d'une page changé"
        ),
        AccessChange::User(target, to) => tracing::info!(
            node_id = %id,
            space_id = %space_id,
            user_id = %user.id,
            target_id = %target,
            level = level(to),
            left_space = written.left_space,
            "accès d'une personne à une page changé"
        ),
    }
}

/// Les accès de la page `node` : la ligne `members` la plus forte de la page et de ses
/// ancêtres, les comptes nommés sur eux (une fois chacun, au niveau effectif), et les
/// invitations sur la page qui attendent encore. Seuls les comptes qui ont un rôle dans
/// l'espace y figurent : une ligne restée à quelqu'un qui n'en a plus ne donne rien.
async fn page_access(state: &AppState, node: &Node) -> Result<PageAccess, ApiError> {
    let space_id = node.space_id;
    let nodes = state.store.list_nodes(space_id).await?;
    let parents: HashMap<Uuid, Option<Uuid>> = nodes.iter().map(|n| (n.id, n.parent_id)).collect();
    // La page et ses ancêtres ; chacun une fois, la remontée s'arrête même si des parents
    // tournaient en rond.
    let mut chain = HashSet::from([node.id]);
    let mut parent = node.parent_id;
    while let Some(id) = parent.filter(|id| chain.insert(*id)) {
        parent = parents.get(&id).copied().flatten();
    }
    let mut general: Option<Level> = None;
    let mut own: HashMap<Uuid, Level> = HashMap::new();
    let mut above: HashMap<Uuid, Level> = HashMap::new();
    for (on, grant) in state.store.space_grants(space_id).await? {
        if !chain.contains(&on) {
            continue;
        }
        match grant.subject {
            Subject::Members => general = general.max(Some(grant.level)),
            Subject::User(user) if on == node.id => {
                own.insert(user, grant.level);
            }
            Subject::User(user) => {
                let level = above.entry(user).or_insert(grant.level);
                *level = (*level).max(grant.level);
            }
        }
    }
    let entries = state
        .store
        .members(space_id)
        .await?
        .into_iter()
        .filter_map(|member| {
            let (level, inherited) = match (own.get(&member.user_id), above.get(&member.user_id)) {
                (None, None) => return None,
                (Some(&own), Some(&above)) if above > own => (above, true),
                (Some(&own), _) => (own, false),
                (None, Some(&above)) => (above, true),
            };
            Some(AccessEntry {
                user_id: member.user_id.to_string(),
                name: member.name,
                email: member.email,
                level: level.into(),
                inherited,
            })
        })
        .collect();
    let invitations = state
        .store
        .pending_invitations(space_id)
        .await?
        .into_iter()
        .filter(|i| matches!(i.grant, InvitationGrant::Page { node_id, .. } if node_id == node.id))
        .map(pending_invitation)
        .collect();
    Ok(PageAccess {
        general: general.map(Into::into),
        entries,
        invitations,
    })
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use mementum_acl::{Grant, Role};
    use mementum_store::{NewNode, NodeKind};

    use super::*;
    use crate::access::tests::{account, state};

    /// Mona a décidé de donner `full` à Tom alors qu'elle avait `full` ; Olga la baisse en
    /// lecture avant que Mona écrive. Le store refuse l'écriture décidée sur l'état d'avant, la
    /// route relit et redécide : `403`, et Tom n'a rien reçu.
    #[tokio::test]
    async fn caller_who_loses_full_before_writing_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let mona = account(&state, "mona").await;
        let tom = account(&state, "tom").await;
        let space = state
            .store
            .create_space("Équipe", Some(olga.id))
            .await
            .unwrap();
        for who in [&mona, &tom] {
            state
                .store
                .add_member(space.id, who.id, Role::Member)
                .await
                .unwrap();
        }
        let line = |who: &AuthUser, level| Grant {
            subject: Subject::User(who.id),
            level,
        };
        let page = state
            .store
            .create_node(NewNode {
                space_id: space.id,
                parent_id: None,
                after: None,
                kind: NodeKind::Page,
                title: "P".to_string(),
                created_by: Some(olga.id),
                grants: vec![line(&olga, Level::Full), line(&mona, Level::Full)],
                doc: None,
            })
            .await
            .unwrap();
        let give_tom = AccessChange::User(tom.id, Some(Level::Full));
        // Ce que la route de Mona a décidé avant d'écrire.
        let decided = decide(&state, &mona, page.id, give_tom).await.unwrap();
        // Olga baisse Mona entre-temps.
        let lower_mona = AccessChange::User(mona.id, Some(Level::Read));
        let olga_decided = decide(&state, &olga, page.id, lower_mona).await.unwrap();
        write(&state, &olga, page.id, lower_mona, olga_decided)
            .await
            .unwrap();

        let error = write(&state, &mona, page.id, give_tom, decided)
            .await
            .unwrap_err();
        assert_eq!(error.into_response().status(), StatusCode::FORBIDDEN);
        let grants = state.store.node_grants(page.id).await.unwrap();
        assert!(
            !grants.iter().any(|g| g.subject == Subject::User(tom.id)),
            "{grants:?}"
        );

        // Décidée sur l'état juste, la même écriture passe du premier coup.
        let decided = decide(&state, &olga, page.id, give_tom).await.unwrap();
        let written = write(&state, &olga, page.id, give_tom, decided)
            .await
            .unwrap();
        assert!(written.changed);
    }
}
