//! Routes des favoris (Ruling R254) : un compte marque des pages, et retrouve la liste de
//! celles qu'il a marquées dans un espace. Un favori est à son auteur seul : personne d'autre
//! ne le lit, et il ne change rien à la page.
//!
//! | Route | Il faut | Réponse |
//! |---|---|---|
//! | `GET /api/v1/spaces/{id}/favorites` | un rôle dans l'espace | [`Favorites`] : ses pages favorites qu'il voit, hors corbeille, dans l'ordre où il les a ajoutées |
//! | `PUT`, `DELETE /api/v1/pages/{id}/favorite` | `Read` (à la corbeille aussi) | `204` ; déjà dans cet état : `204`, rien n'est écrit |
//!
//! **Refus**, dans l'ordre des pages : `401` sans session ; `404 not_found` pour une page que la
//! personne ne voit pas, exactement la réponse d'une page qui n'existe pas, et pour un espace où
//! elle n'a aucun rôle, celle d'un espace qui n'existe pas. Rien d'autre : toute page qu'on voit
//! se lit, donc jamais `403` ; un favori n'écrit pas dans la page, donc jamais `409 trashed`.
//!
//! **La liste ne nomme que ce que la personne voit.** Le store rend ses favoris tels qu'ils
//! sont en base, pages devenues invisibles et pages à la corbeille comprises (Ruling R268) : il
//! ne juge ni les droits ni la corbeille. La liste n'en garde que ce que `access.rs` lui montre,
//! hors corbeille, comme l'arbre. Sans ce tri, elle dirait à quelqu'un à qui l'on a retiré une
//! page qu'elle existe encore : un favori part avec sa page, et celui d'une page supprimée
//! quitterait la liste quand celui d'une page cachée y resterait. Le favori, lui, n'est pas
//! retiré : l'accès rendu, ou la page restaurée, il revient à sa place. De même pour qui n'a
//! plus de rôle dans l'espace, et y revient (Ruling R280).
//!
//! **Un favori n'avance ni `spaces.rev` ni `spaces.lsn`**, et rien n'est annoncé sur le canal de
//! l'espace (Ruling R254) : il ne change ni les droits ni la barre latérale des autres. L'onglet
//! qui le pose relit sa liste ; les autres onglets du compte, à leur prochaine lecture de
//! l'arbre.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mementum_acl::Action;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::routes::pages::page_id;
use crate::routes::spaces::space_id;
use crate::types::Favorites;
use crate::{AppState, now_ms};

/// `GET /api/v1/spaces/{id}/favorites` : les pages favorites du compte dans l'espace, celles
/// qu'il voit, hors corbeille, dans l'ordre où il les a ajoutées.
pub(crate) async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Favorites>, ApiError> {
    let space_id = space_id(path)?;
    let access = state.space_access(&user, space_id).await?;
    let favorites = state.store.favorites(user.id, space_id).await?;
    let pages = favorites
        .into_iter()
        .filter(|page| access.node(*page).is_some_and(|access| !access.in_trash))
        .map(|page| page.to_string())
        .collect();
    Ok(Json(Favorites { pages }))
}

/// `PUT /api/v1/pages/{id}/favorite` : la page devient un favori du compte. Elle l'est déjà :
/// rien n'est écrit, et elle garde sa place dans la liste.
pub(crate) async fn add(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    set(&state, &user, page_id(path)?, true).await
}

/// `DELETE /api/v1/pages/{id}/favorite` : la page n'est plus un favori du compte. Elle ne
/// l'était pas : rien n'est écrit.
pub(crate) async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    set(&state, &user, page_id(path)?, false).await
}

/// Pose (`on`) ou retire le favori de `user` sur la page `id`, qu'il doit voir.
///
/// Lire suffit, et la corbeille n'y change rien (`Action::Read` n'écrit pas dans la page) : on
/// retire ainsi le favori d'une page jetée. Une page supprimée entre le contrôle et l'écriture
/// rend `NotFound` : la réponse d'une page introuvable, par la conversion générique.
async fn set(
    state: &AppState,
    user: &AuthUser,
    id: Uuid,
    on: bool,
) -> Result<StatusCode, ApiError> {
    state.node_access(user, id, Action::Read).await?;
    state.store.set_favorite(user.id, id, on, now_ms()).await?;
    Ok(StatusCode::NO_CONTENT)
}
