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
                "le corps doit être du JSON (en-tête Content-Type: application/json)".to_string()
            }
            JsonRejection::BytesRejection(_) => "corps de requête illisible".to_string(),
            _ => json_message(&e),
        };
        rejection(e.status(), ErrorCode::InvalidJson, message)
    }
}

impl From<BytesRejection> for ApiError {
    fn from(e: BytesRejection) -> ApiError {
        rejection(
            e.status(),
            ErrorCode::InvalidRequest,
            "corps de requête illisible".to_string(),
        )
    }
}

/// Erreur de serde telle qu'axum la garde (le chemin du champ en plus).
type PathError = serde_path_to_error::Error<serde_json::Error>;

/// Message en français d'un JSON refusé, d'après l'erreur de serde trouvée
/// dans la chaîne des causes (le texte anglais de serde n'est pas recopié).
fn json_message(e: &(dyn std::error::Error + 'static)) -> String {
    let mut cause = Some(e);
    while let Some(error) = cause {
        if let Some(error) = error.downcast_ref::<PathError>() {
            return describe_json(error.inner(), Some(error.path().to_string()));
        }
        if let Some(error) = error.downcast_ref::<serde_json::Error>() {
            return describe_json(error, None);
        }
        cause = error.source();
    }
    "corps JSON illisible".to_string()
}

fn describe_json(error: &serde_json::Error, path: Option<String>) -> String {
    use serde_json::error::Category;
    if !matches!(error.classify(), Category::Data) {
        return format!(
            "JSON illisible (ligne {}, colonne {})",
            error.line(),
            error.column()
        );
    }
    // Les tournures de serde sont stables (`serde::de::Error`) : on en tire
    // le nom du champ en cause.
    let text = error.to_string();
    let named = |prefix: &str| {
        text.strip_prefix(prefix)
            .and_then(|rest| rest.split_once('`'))
            .map(|(name, _)| name.to_string())
    };
    let field = path.filter(|p| !p.is_empty() && p != ".");
    if let Some(name) = named("unknown field `") {
        format!("champ inconnu : « {name} »")
    } else if let Some(name) = named("duplicate field `") {
        format!("champ en double : « {name} »")
    } else if let Some(name) = named("missing field `") {
        format!("champ manquant : « {name} »")
    } else if text.starts_with("invalid type") {
        match field {
            Some(field) => format!("type invalide pour « {field} »"),
            None => "le corps doit être un objet JSON".to_string(),
        }
    } else if text.starts_with("invalid value") {
        match field {
            Some(field) => format!("valeur invalide pour « {field} »"),
            None => "valeur invalide".to_string(),
        }
    } else {
        "corps JSON inattendu".to_string()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `SyncError::Closed` ne s'obtient pas de bout en bout sans tricher :
    /// un acteur qui ne peut pas charger sa page répond l'erreur réelle au
    /// premier demandeur, et `Closed` ne vient qu'après 8 demandes lâchées de
    /// suite (course entre demandes simultanées, ou panique de l'acteur). On
    /// vérifie donc sa traduction (Ruling R26).
    #[test]
    fn closed_is_503_unavailable() {
        let error = ApiError::from(SyncError::Closed);
        assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error.body.error, ErrorCode::Unavailable);
        assert!(error.body.message.contains("réessayer"));
        assert_eq!(error.body.line, None);
    }

    #[test]
    fn crdt_refusals_are_422_invalid_blocks() {
        let error = ApiError::from(SyncError::Crdt(CrdtError::TooDeep));
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error.body.error, ErrorCode::InvalidBlocks);
        let error = ApiError::from(SyncError::Crdt(CrdtError::Structure("x".into())));
        assert_eq!(error.body.error, ErrorCode::InvalidBlocks);
        // Les autres erreurs du document sont internes.
        let error = ApiError::from(SyncError::Crdt(CrdtError::Decode("x".into())));
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.body.message, "erreur interne du serveur");
    }
}
