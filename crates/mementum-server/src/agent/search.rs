//! Chercher pour un agent (lot 5, Task S7 ; Rulings R1248, R917) : l'outil `search` (M2) appelle
//! [`search`], qui mène la route `POST /api/v1/spaces/{id}/search` elle-même, filtres compris
//! (`createdBy`, `editedAfter`, `editedBefore`, qui ne font que réduire l'ensemble des pages que
//! le compte voit, avant l'index). Un outil ne fait rien que la route ne fait pas.

use axum::Json;
use axum::extract::{Path, State};
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::types::{SearchRequest, SearchResults};

/// La recherche `request` dans l'espace `space_id`, comme la route : le débit, le plancher, les
/// droits (restreints au jeton), les filtres.
pub(crate) async fn search(
    state: &AppState,
    user: &AuthUser,
    space_id: Uuid,
    request: SearchRequest,
) -> Result<SearchResults, ApiError> {
    let Json(found) = crate::routes::search::search(
        State(state.clone()),
        Ok(user.clone()),
        Ok(Path(space_id.to_string())),
        Ok(Json(request)),
    )
    .await?;
    Ok(found)
}
