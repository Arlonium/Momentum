//! Ce qu'un agent sait de son compte et de ses espaces (lot 5, Task S7 ; Ruling R1247) :
//! `GET /api/v1/me` et `GET …/spaces/{id}/members` par un jeton, et ce que l'outil
//! `workspace_info` (M2) assemble.
//!
//! **Jamais une adresse e-mail** : une adresse rendue à un agent sort de l'instance au premier
//! appel d'un modèle hébergé. `/me` par jeton rend le compte (son nom), les espaces de la portée du
//! jeton, et le jeton ; la liste des membres, leurs noms et leurs rôles. Une session garde ses
//! réponses d'avant (`Me`, `Member`), que l'application lit.

use axum::Json;
use axum::response::{IntoResponse, Response};
use mementum_acl::Role;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{ApiError, internal};
use crate::types::{self, AgentMe, AgentUser, MemberName, SearchAncestor, SpaceInfo};

/// `GET /api/v1/me` par un jeton : le compte sans son adresse, les espaces que la portée du jeton
/// ouvre (avec le rôle restreint au jeton : `guest` pour une portée de pages, au plus `member`
/// sinon), et le jeton. `None` pour une session : la route rend alors `Me`.
pub(crate) async fn me(state: &AppState, user: &AuthUser) -> Result<Option<AgentMe>, ApiError> {
    let Some(grant) = user.token() else {
        return Ok(None);
    };
    let mut spaces = Vec::new();
    for (space, _) in state.store.spaces_of(user.id).await.map_err(internal)? {
        if !grant.opens(space.id) {
            continue;
        }
        let Some(access) = state.find_space_access(user, space.id).await? else {
            continue;
        };
        if let Some(role) = access.role() {
            spaces.push(SpaceInfo {
                id: space.id.to_string(),
                name: space.name,
                role: role.into(),
            });
        }
    }
    let token = crate::routes::tokens::token_info(state, user, grant.token_id)
        .await?
        // Le jeton a été lu vivant par la porte : parti depuis, c'est un jeton mort.
        .ok_or_else(ApiError::invalid_token)?;
    Ok(Some(AgentMe {
        user: AgentUser {
            id: user.id.to_string(),
            name: user.name.clone(),
        },
        spaces,
        token,
    }))
}

/// `GET /api/v1/spaces/{id}/members` : un membre de l'espace au moins (un invité, ou un jeton à
/// portée de pages : `403`). Par une session, `Member[]` ; par un jeton, `MemberName[]`, sans
/// adresse (Ruling R1247).
pub(crate) async fn members(
    state: &AppState,
    user: &AuthUser,
    space_id: Uuid,
) -> Result<Response, ApiError> {
    state.role_at_least(user, space_id, Role::Member).await?;
    let members = state.store.members(space_id).await.map_err(internal)?;
    if user.token().is_none() {
        let full: Vec<types::Member> = members
            .into_iter()
            .map(|m| types::Member {
                user_id: m.user_id.to_string(),
                name: m.name,
                email: m.email,
                role: m.role.into(),
            })
            .collect();
        return Ok(Json(full).into_response());
    }
    Ok(Json(named(members)).into_response())
}

/// Les membres de l'espace, tels qu'un agent les lit (l'outil `workspace_info`, M2) : noms et
/// rôles, jamais les adresses ; un membre de l'espace au moins.
#[allow(dead_code)]
pub(crate) async fn member_names(
    state: &AppState,
    user: &AuthUser,
    space_id: Uuid,
) -> Result<Vec<MemberName>, ApiError> {
    state.role_at_least(user, space_id, Role::Member).await?;
    Ok(named(
        state.store.members(space_id).await.map_err(internal)?,
    ))
}

fn named(members: Vec<mementum_store::Member>) -> Vec<MemberName> {
    members
        .into_iter()
        .map(|m| MemberName {
            user_id: m.user_id.to_string(),
            name: m.name,
            role: m.role.into(),
        })
        .collect()
}

/// Le plan d'un espace tel que l'appel le voit (l'outil `fetch` d'un espace, et `workspace_info`
/// d'un espace nommé) : les pages hors corbeille, dans l'ordre de l'arbre, chacune avec sa
/// profondeur, `max` au plus ; vrai s'il y en a d'autres.
#[allow(dead_code)]
pub(crate) async fn space_plan(
    state: &AppState,
    user: &AuthUser,
    space_id: Uuid,
    max: usize,
) -> Result<(Vec<(usize, SearchAncestor)>, bool), ApiError> {
    let tree = crate::routes::pages::tree(
        axum::extract::State(state.clone()),
        user.clone(),
        Ok(axum::extract::Path(space_id.to_string())),
    )
    .await?
    .0;
    // L'arbre rendu est trié par `(parentId, position)` : le plan le parcourt en profondeur.
    let mut by_parent: std::collections::HashMap<Option<String>, Vec<&types::PageNode>> =
        std::collections::HashMap::new();
    for node in &tree.nodes {
        by_parent
            .entry(node.parent_id.clone())
            .or_default()
            .push(node);
    }
    let mut out = Vec::new();
    let mut stack: Vec<(usize, &types::PageNode)> = by_parent
        .get(&None)
        .map(|roots| roots.iter().rev().map(|n| (0, *n)).collect())
        .unwrap_or_default();
    let mut more = false;
    while let Some((depth, node)) = stack.pop() {
        if out.len() == max {
            more = true;
            break;
        }
        out.push((
            depth,
            SearchAncestor {
                id: node.id.clone(),
                title: node.title.clone(),
            },
        ));
        if let Some(children) = by_parent.get(&Some(node.id.clone())) {
            stack.extend(children.iter().rev().map(|c| (depth + 1, *c)));
        }
    }
    Ok((out, more))
}

/// L'espace que nomme `name` (son identifiant, ou son nom, sans casse) parmi ceux que la portée de
/// l'appel ouvre : `None` pour un nom inconnu ou hors de la portée (la même réponse) ;
/// `Err(ids)` pour deux espaces de ce nom (l'outil dit `ambiguous_space`, avec leurs
/// identifiants).
#[allow(dead_code)]
pub(crate) async fn resolve_space(
    state: &AppState,
    user: &AuthUser,
    name: &str,
) -> Result<Result<Option<Uuid>, Vec<Uuid>>, ApiError> {
    let wanted = name.trim();
    let mut found = Vec::new();
    for (space, _) in state.store.spaces_of(user.id).await.map_err(internal)? {
        let named = space.id.to_string() == wanted.to_lowercase()
            || space.name.to_lowercase() == wanted.to_lowercase();
        if named && state.find_space_access(user, space.id).await?.is_some() {
            found.push(space.id);
        }
    }
    Ok(match found.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(*one)),
        _ => Err(found),
    })
}
