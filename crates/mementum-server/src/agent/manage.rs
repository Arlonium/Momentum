//! Les gestes d'un agent sur une page (lot 5, Task S7) : l'outil `manage_pages` (M2) appelle les
//! fonctions de ce module, qui mènent les routes de page elles-mêmes (`PATCH /api/v1/pages/{id}`,
//! `…/trash`, `…/restore`, `…/duplicate`) : leurs droits, leurs refus et leur journal. Un jeton n'y
//! a rien de plus qu'une session restreinte à lui (Ruling R1218 : un déplacement ne se fait que
//! parmi les sœurs, `Move` étant interdit à tout jeton ; la corbeille demande l'option).

use axum::Json;
use axum::extract::{Path, State};
use uuid::Uuid;

use crate::AppState;
use crate::auth::{AuthUser, LiveCaller};
use crate::error::ApiError;
use crate::types::{PageNode, UpdatePage};

/// Renomme la page (`PATCH`, `title`).
pub(crate) async fn rename(
    state: &AppState,
    user: &AuthUser,
    page: Uuid,
    title: String,
) -> Result<PageNode, ApiError> {
    patch(
        state,
        user,
        page,
        UpdatePage {
            title: Some(title),
            parent_id: None,
            after_id: None,
        },
    )
    .await
}

/// Range la page après la sœur `after` (`None` : en tête), sous le même parent (`PATCH`,
/// `afterId`). `after` est l'identifiant tel que l'agent l'a donné : c'est la route qui le juge,
/// après les refus de la page (un identifiant qu'elle ne lit pas, ou d'une page qui n'est pas une
/// sœur : `422 invalid_after`).
pub(crate) async fn reorder(
    state: &AppState,
    user: &AuthUser,
    page: Uuid,
    after: Option<String>,
) -> Result<PageNode, ApiError> {
    patch(
        state,
        user,
        page,
        UpdatePage {
            title: None,
            parent_id: None,
            after_id: Some(after),
        },
    )
    .await
}

async fn patch(
    state: &AppState,
    user: &AuthUser,
    page: Uuid,
    body: UpdatePage,
) -> Result<PageNode, ApiError> {
    let Json(node) = crate::routes::pages::update(
        State(state.clone()),
        user.clone(),
        Ok(Path(page.to_string())),
        Ok(Json(body)),
    )
    .await?;
    Ok(node)
}

/// Met la page à la corbeille (`POST …/trash`).
pub(crate) async fn trash(
    state: &AppState,
    user: &AuthUser,
    page: Uuid,
) -> Result<PageNode, ApiError> {
    let Json(node) = crate::routes::pages::trash(
        State(state.clone()),
        user.clone(),
        Ok(Path(page.to_string())),
    )
    .await?;
    Ok(node)
}

/// Sort la page de la corbeille (`POST …/restore`).
pub(crate) async fn restore(
    state: &AppState,
    user: &AuthUser,
    page: Uuid,
) -> Result<PageNode, ApiError> {
    let Json(node) = crate::routes::pages::restore(
        State(state.clone()),
        user.clone(),
        Ok(Path(page.to_string())),
    )
    .await?;
    Ok(node)
}

/// Duplique la page (`POST …/duplicate`) : la racine de la copie.
pub(crate) async fn duplicate(
    state: &AppState,
    caller: &LiveCaller,
    page: Uuid,
) -> Result<PageNode, ApiError> {
    let (_, Json(node)) = crate::routes::duplicate::duplicate(
        State(state.clone()),
        caller.clone(),
        Ok(Path(page.to_string())),
    )
    .await?;
    Ok(node)
}
