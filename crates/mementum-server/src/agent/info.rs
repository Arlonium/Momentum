//! Ce qu'un agent sait de son compte et de ses espaces (lot 5, Task S7 ; Ruling R1247) :
//! `GET /api/v1/me` et `GET …/spaces/{id}/members` par un jeton, et ce que les outils MCP (Task
//! M2) assemblent : `workspace_info`, le nom d'un espace et l'espace implicite d'un jeton qui n'en
//! ouvre qu'un, le plan d'un espace (`fetch`), l'espace de la page sous laquelle on crée.
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
    let spaces = opened_spaces(state, user).await?;
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

/// Les espaces que l'appel ouvre, avec son rôle, dans l'ordre du compte : pour une session, ceux
/// où le compte a un rôle ; pour un jeton, ceux de sa portée, le rôle restreint au jeton (`guest`
/// pour une portée de pages, au plus `member` sinon, Ruling R1217). Un espace hors de la portée n'y
/// est pas : pour un outil, il n'existe pas.
pub(crate) async fn opened_spaces(
    state: &AppState,
    user: &AuthUser,
) -> Result<Vec<SpaceInfo>, ApiError> {
    let mut spaces = Vec::new();
    for (space, _) in state.store.spaces_of(user.id).await.map_err(internal)? {
        if user.token().is_some_and(|grant| !grant.opens(space.id)) {
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
    Ok(spaces)
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

/// Les membres de l'espace, tels qu'un agent les lit (l'outil `workspace_info`, et le filtre
/// `created_by` de `search`, M2) : noms et rôles, jamais les adresses ; un membre de l'espace au
/// moins (`403` sinon).
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
/// profondeur (0 à la racine de ce que l'appel voit), jusqu'à `max_depth` et `max` lignes au
/// plus ; vrai s'il y en a d'autres. Mené par la route de l'arbre (`GET …/tree`).
pub(crate) async fn space_plan(
    state: &AppState,
    user: &AuthUser,
    space_id: Uuid,
    max: usize,
    max_depth: usize,
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
            if depth < max_depth {
                stack.extend(children.iter().rev().map(|c| (depth + 1, *c)));
            } else {
                more = true;
            }
        }
    }
    Ok((out, more))
}

/// L'espace que nomme `name` (son identifiant, ou son nom, sans casse ni espaces de bord) parmi
/// ceux que l'appel ouvre ([`opened_spaces`]) : `None` pour un nom inconnu **ou hors de la
/// portée** (la même réponse : un outil ne dit pas qu'un espace existe hors d'elle) ; `Err(ids)`
/// pour plusieurs espaces de ce nom (l'outil dit `ambiguous_space`, avec leurs identifiants).
pub(crate) async fn resolve_space(
    state: &AppState,
    user: &AuthUser,
    name: &str,
) -> Result<Result<Option<SpaceInfo>, Vec<String>>, ApiError> {
    let wanted = name.trim().to_lowercase();
    let mut found: Vec<SpaceInfo> = opened_spaces(state, user)
        .await?
        .into_iter()
        .filter(|space| space.id == wanted || space.name.trim().to_lowercase() == wanted)
        .collect();
    Ok(match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err(found.into_iter().map(|space| space.id).collect()),
    })
}

/// L'espace d'une page que l'appel voit, corbeille comprise (l'outil `create_pages` sous une page
/// qu'il désigne seule) ; `None` pour une page qu'il ne voit pas, ou qui n'existe pas (la même
/// réponse).
pub(crate) async fn page_space(
    state: &AppState,
    user: &AuthUser,
    page: Uuid,
) -> Result<Option<Uuid>, ApiError> {
    Ok(state
        .seen_node(user, page)
        .await?
        .map(|(node, _, _)| node.space_id))
}
