//! Erreurs de l'API : toujours en JSON, `{ "error": <code>, "message": <texte
//! en français> }` (plus `line` pour un MMD illisible).

use axum::Json;
use axum::extract::rejection::{BytesRejection, JsonRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mementum_crdt::CrdtError;
use mementum_md::MmdError;
use mementum_store::StoreError;
use mementum_sync::SyncError;

use crate::types::{ErrorBody, ErrorCode};

/// Une réponse d'erreur de l'API.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: ErrorBody,
}

impl ApiError {
    pub fn new(status: StatusCode, error: ErrorCode, message: impl Into<String>) -> ApiError {
        ApiError {
            status,
            body: ErrorBody {
                error,
                message: message.into(),
                line: None,
            },
        }
    }

    /// `404 not_found` : la page n'existe pas.
    pub fn page_not_found() -> ApiError {
        ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::NotFound,
            "page introuvable",
        )
    }

    /// `422 invalid_after`.
    pub fn invalid_after() -> ApiError {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidAfter,
            "afterId ne désigne pas une page sœur à l'endroit visé (null : en tête)",
        )
    }

    /// `422 invalid_parent`.
    pub fn invalid_parent() -> ApiError {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidParent,
            "parentId ne désigne pas une page de ce coffre (null : la racine)",
        )
    }

    /// `500 internal` : le détail va dans le journal, pas dans la réponse.
    pub fn internal(error: &dyn std::fmt::Display) -> ApiError {
        tracing::error!(%error, "erreur interne");
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::Internal,
            "erreur interne du serveur",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

impl From<StoreError> for ApiError {
    /// Les cas où `NotFound` vise autre chose que la page (parent, `afterId`)
    /// sont traités par les routes avant cette conversion.
    fn from(e: StoreError) -> ApiError {
        match e {
            StoreError::NotFound => ApiError::page_not_found(),
            StoreError::Cycle => ApiError::new(
                StatusCode::CONFLICT,
                ErrorCode::Cycle,
                "une page ne peut pas aller sous elle-même ni sous une de ses descendantes",
            ),
            e @ StoreError::Sqlx(_) => ApiError::internal(&e),
        }
    }
}

impl From<SyncError> for ApiError {
    fn from(e: SyncError) -> ApiError {
        match e {
            SyncError::NotFound => ApiError::page_not_found(),
            SyncError::Store(e) => e.into(),
            // Ruling R24 : une écriture que BlockNote refuserait.
            SyncError::Crdt(e @ (CrdtError::Structure(_) | CrdtError::TooDeep)) => ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::InvalidBlocks,
                format!("blocs refusés : {e}"),
            ),
            SyncError::Crdt(e) => ApiError::internal(&e),
            // Ruling R26 : l'acteur de la page ne répond plus.
            SyncError::Closed => ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::Unavailable,
                "la page ne peut pas être ouverte pour l'instant ; réessayer",
            ),
        }
    }
}

impl From<MmdError> for ApiError {
    fn from(e: MmdError) -> ApiError {
        let mut error = ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidMmd,
            e.message,
        );
        error.body.line = Some(e.line);
        error
    }
}

impl From<JsonRejection> for ApiError {
    fn from(e: JsonRejection) -> ApiError {
        let message = match &e {
            JsonRejection::MissingJsonContentType(_) => {
                "le corps doit être du JSON (Content-Type: application/json)".to_string()
            }
            JsonRejection::JsonSyntaxError(_) => format!("JSON illisible ({})", detail(&e)),
            JsonRejection::JsonDataError(_) => format!("corps inattendu ({})", detail(&e)),
            _ => format!("corps illisible ({})", detail(&e)),
        };
        rejection(e.status(), ErrorCode::InvalidJson, message)
    }
}

impl From<BytesRejection> for ApiError {
    fn from(e: BytesRejection) -> ApiError {
        let message = format!("corps illisible ({})", detail(&e));
        rejection(e.status(), ErrorCode::InvalidRequest, message)
    }
}

/// Le message de la cause la plus profonde (celui de serde, par exemple) :
/// les enveloppes d'axum n'ajoutent qu'un préambule en anglais.
fn detail(e: &(dyn std::error::Error + 'static)) -> String {
    let mut deepest = e;
    while let Some(source) = deepest.source() {
        deepest = source;
    }
    deepest.to_string()
}

/// Erreur d'un extracteur d'axum ; un corps trop gros garde son statut 413.
fn rejection(status: StatusCode, code: ErrorCode, message: String) -> ApiError {
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        return ApiError::new(
            status,
            ErrorCode::PayloadTooLarge,
            format!(
                "corps de requête trop gros : {} octets au plus",
                crate::MAX_BODY_BYTES
            ),
        );
    }
    ApiError::new(status, code, message)
}
