//! Routes des agents (lot 5, Task S7) : lire une page en tranches, l'écrire par opérations,
//! créer des pages en lot. Ouvertes aux jetons d'API (`auth::TOKEN_ROUTES`) comme aux sessions ;
//! chacune mène la fonction d'`agent/` que l'outil MCP de la même opération appelle (M2).
//!
//! | Route | Il faut | Réponse |
//! |---|---|---|
//! | `GET /api/v1/pages/{id}/document?anchors=block&from=&since=&limit=` | `read` (à la corbeille aussi) | `PageDocument` : métadonnées, révision, une tranche du MMD (24 Kio par défaut, de 1 024 à 49 152 octets) ; avec `since`, `changedSince` et `changed` ; `422 stale_anchor` pour un `from` qui ne désigne pas un bloc ; `400 invalid_request` pour une adresse qui ne se lit pas |
//! | `POST /api/v1/pages/{id}/operations`, corps `PageOperations` | `edit` | `UpdateResult` ; `409 trashed` ; `422 invalid_request`, `mode_unavailable`, `revision_required`, `too_many_operations`, `operation_failed` (avec `index`, `reason`, `hint`, `candidates`, `blocks`), `external_media`, `too_many_blocks`, `too_long_to_write`, `invalid_markdown` par `operation_failed` ; `409 revision_conflict` (avec `revision` et `mmd`), `page_in_use`, `version_refused` ; `429 page_busy` |
//! | `POST /api/v1/spaces/{id}/pages/batch`, corps `CreatePages` | racine : membre (et « Modification » pour un jeton), dans les deux sections ; sous une page : `edit` | `201`, `PageNode[]`, toutes ou aucune ; `422 too_many_pages`, `invalid_parent`, `invalid_mmd` (avec `index`, `line`), `external_media` (par jeton), `too_many_blocks`, `too_long_to_write` ; `429 duplication_busy` |
//!
//! **Ordre des refus** : `401` (session ou jeton), le compte attendu, puis `404` d'une page ou d'un
//! espace qu'on ne voit pas (hors de la portée d'un jeton compris), `403` sous le niveau, `409
//! trashed`, puis l'adresse ou le corps. Le corps est reçu avant tout refus (un refus rendu avant
//! de lire un corps se perd chez un client qui l'envoie d'un coup, R505), mais n'est jugé qu'après
//! les droits.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mementum_acl::Action;
use serde::Deserialize;

use crate::AppState;
use crate::agent::fetch::{DEFAULT_LIMIT, Fetch, MAX_LIMIT, MIN_LIMIT};
use crate::auth::{AuthUser, LiveCaller};
use crate::error::ApiError;
use crate::routes::pages::page_id;
use crate::routes::spaces::space_id;
use crate::types::{CreatePages, ErrorCode, PageDocument, PageNode, PageOperations};

/// L'adresse de `GET …/document`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentQuery {
    /// `block` : chaque bloc porte son alias en ancre.
    anchors: Option<String>,
    from: Option<String>,
    since: Option<String>,
    limit: Option<usize>,
}

/// `400 invalid_request` : l'adresse ne se lit pas.
fn unreadable(message: &str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, ErrorCode::InvalidRequest, message)
}

/// Ce que l'adresse demande, lu et borné.
pub(crate) fn fetch_of(
    query: Result<Query<DocumentQuery>, QueryRejection>,
) -> Result<Fetch, ApiError> {
    let Query(query) = query.map_err(|_| {
        unreadable(
            "adresse illisible : seuls « anchors=block », « from », « since » et « limit » se \
             donnent",
        )
    })?;
    let anchors = match query.anchors.as_deref() {
        None => false,
        Some("block") => true,
        Some(_) => return Err(unreadable("« anchors » ne vaut que « block »")),
    };
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(MIN_LIMIT..=MAX_LIMIT).contains(&limit) {
        return Err(unreadable(
            "« limit » : de 1 024 à 49 152 octets (24 576 par défaut)",
        ));
    }
    if query.since.is_some() && query.from.is_none() {
        return Err(unreadable(
            "« since » ne sert qu'avec « from » : la révision de la première tranche, pour \
             reprendre la lecture",
        ));
    }
    let bounded = |value: &Option<String>| {
        value
            .as_ref()
            .is_none_or(|v| !v.is_empty() && v.len() <= 256)
    };
    if !bounded(&query.from) || !bounded(&query.since) {
        return Err(unreadable("« from » et « since » : de 1 à 256 caractères"));
    }
    Ok(Fetch {
        anchors,
        from: query.from,
        since: query.since,
        limit,
    })
}

/// `GET /api/v1/pages/{id}/document` (voir le module).
pub(crate) async fn document(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    query: Result<Query<DocumentQuery>, QueryRejection>,
) -> Result<Json<PageDocument>, ApiError> {
    let id = page_id(path)?;
    // Les refus d'abord : une page qu'on ne voit pas, puis l'adresse.
    state.node_access(&user, id, Action::Read).await?;
    let fetch = fetch_of(query)?;
    Ok(Json(
        crate::agent::fetch::document(&state, &user, id, fetch).await?,
    ))
}

/// `POST /api/v1/pages/{id}/operations` (voir le module).
pub(crate) async fn operations(
    State(state): State<AppState>,
    caller: LiveCaller,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<PageOperations>, JsonRejection>,
) -> Result<Response, ApiError> {
    let id = page_id(path)?;
    // Premier contrôle : les refus, avant que le corps soit jugé.
    state.node_access(&caller.user, id, Action::Edit).await?;
    let Json(request) = body?;
    let updated = crate::agent::update::update_page(&state, &caller, id, request).await?;
    let mut response = Json(updated.result).into_response();
    response.extensions_mut().insert(updated.details);
    Ok(response)
}

/// `POST /api/v1/spaces/{id}/pages/batch` (voir le module).
pub(crate) async fn pages_batch(
    State(state): State<AppState>,
    caller: LiveCaller,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<CreatePages>, JsonRejection>,
) -> Result<(StatusCode, Json<Vec<PageNode>>), ApiError> {
    let space_id = space_id(path)?;
    // Sans rôle dans l'espace (ou hors de la portée du jeton) : `404`, quoi que porte le corps.
    state.space_access(&caller.user, space_id).await?;
    let Json(request) = body?;
    let pages = crate::agent::create::create_pages(&state, &caller, space_id, request).await?;
    Ok((StatusCode::CREATED, Json(pages)))
}
