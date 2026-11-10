//! Erreurs de l'API : toujours en JSON, `{ "error": <code>, "message": <texte
//! en français> }` (plus `line` pour un MMD illisible, et l'en-tête
//! `Retry-After` pour un `429`).

use axum::Json;
use axum::extract::rejection::{BytesRejection, JsonRejection};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use mementum_crdt::CrdtError;
use mementum_md::MmdError;
use mementum_store::{ShareRefusal, StoreError};
use mementum_sync::SyncError;

use crate::types::{ErrorBody, ErrorCode};

/// Une réponse d'erreur de l'API.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: ErrorBody,
    /// Secondes de l'en-tête `Retry-After` (`429 too_many_attempts`).
    retry_after: Option<u64>,
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
            retry_after: None,
        }
    }

    /// `404 not_found` : la page n'existe pas, **ou** la personne ne la voit pas. Les deux
    /// cas reçoivent exactement cette réponse, comme un id mal formé : qui ne peut pas lire
    /// une page n'apprend rien d'elle, pas même qu'elle existe.
    pub fn page_not_found() -> ApiError {
        ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::NotFound,
            "page introuvable",
        )
    }

    /// `422 invalid_after` : la même réponse pour un id inconnu, mal formé, d'une page qui
    /// n'est pas sœur à l'endroit visé, ou d'une page que la personne ne voit pas.
    pub fn invalid_after() -> ApiError {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidAfter,
            "afterId ne désigne pas une page sœur à l'endroit visé (null : en tête)",
        )
    }

    /// `422 invalid_parent` : la même réponse pour un id inconnu, mal formé, d'un autre
    /// espace, ou d'une page que la personne ne voit pas.
    pub fn invalid_parent() -> ApiError {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidParent,
            "parentId ne désigne pas une page de cet espace (null : la racine)",
        )
    }

    /// `409 trashed` : une écriture dans une page à la corbeille.
    pub fn trashed() -> ApiError {
        ApiError::new(
            StatusCode::CONFLICT,
            ErrorCode::Trashed,
            "la page est à la corbeille : elle se lit, mais ne s'écrit plus ; la restaurer \
             d'abord",
        )
    }

    /// `409 parent_trashed`.
    pub fn parent_trashed() -> ApiError {
        ApiError::new(
            StatusCode::CONFLICT,
            ErrorCode::ParentTrashed,
            "une page parente est à la corbeille : c'est elle qu'il faut restaurer",
        )
    }

    /// `409 not_trashed`.
    pub fn not_trashed() -> ApiError {
        ApiError::new(
            StatusCode::CONFLICT,
            ErrorCode::NotTrashed,
            "la page n'est pas à la corbeille : on ne supprime définitivement que ce qui y est",
        )
    }

    /// `401 unauthenticated` : pas de session.
    pub fn unauthenticated() -> ApiError {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            ErrorCode::Unauthenticated,
            "session absente ou échue : se connecter",
        )
    }

    /// `401 invalid_credentials` : la même réponse pour une adresse inconnue,
    /// un mot de passe faux et un compte sans mot de passe.
    pub fn invalid_credentials() -> ApiError {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            ErrorCode::InvalidCredentials,
            "adresse e-mail ou mot de passe invalide",
        )
    }

    /// `429 too_many_attempts`, avec `Retry-After: <secondes>` : trop de connexions ratées
    /// pour une adresse.
    pub fn too_many_attempts(retry_after: u64) -> ApiError {
        ApiError::throttled("trop de tentatives pour cette adresse", retry_after)
    }

    /// Le `429 too_many_attempts` du changement de mot de passe, avec son `Retry-After`. Le
    /// code de la connexion, mais pas son message : ici, personne n'a tapé d'adresse.
    pub fn too_many_wrong_passwords(retry_after: u64) -> ApiError {
        ApiError::throttled("trop de mots de passe actuels incorrects", retry_after)
    }

    /// `429 too_many_attempts` pour `what`, à réessayer dans `retry_after` secondes.
    fn throttled(what: &str, retry_after: u64) -> ApiError {
        let mut error = ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::TooManyAttempts,
            format!("{what} : réessayer dans {} min", retry_after.div_ceil(60)),
        );
        error.retry_after = Some(retry_after);
        error
    }

    /// `403 signup_closed`.
    pub fn signup_closed() -> ApiError {
        ApiError::new(
            StatusCode::FORBIDDEN,
            ErrorCode::SignupClosed,
            "les inscriptions sont sur invitation : il faut le lien d'une invitation pour créer \
             un compte",
        )
    }

    /// `409 email_taken`.
    pub fn email_taken() -> ApiError {
        ApiError::new(
            StatusCode::CONFLICT,
            ErrorCode::EmailTaken,
            "un compte porte déjà cette adresse e-mail",
        )
    }

    /// `422 invalid_email`.
    pub fn invalid_email() -> ApiError {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidEmail,
            "adresse e-mail invalide : une adresse comme nom@exemple.fr, sans espace, de 254 \
             caractères au plus",
        )
    }

    /// `404 invalid_invitation` : la même réponse pour un jeton inconnu,
    /// échu, révoqué ou déjà accepté.
    pub fn invalid_invitation() -> ApiError {
        ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::InvalidInvitation,
            "invitation introuvable : le lien est faux, échu, révoqué, ou il a déjà servi",
        )
    }

    /// `404 not_found` : l'espace n'existe pas, **ou** la personne n'y a aucun
    /// rôle. Les deux cas reçoivent exactement cette réponse : qui n'est pas
    /// dans un espace n'apprend rien de lui, pas même qu'il existe.
    pub fn space_not_found() -> ApiError {
        ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::NotFound,
            "espace introuvable",
        )
    }

    /// `404 not_found` : la personne visée n'a pas de rôle dans l'espace (à qui
    /// voit l'espace).
    pub fn member_not_found() -> ApiError {
        ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::NotFound,
            "cette personne n'est pas dans l'espace",
        )
    }

    /// `404 not_found` : l'invitation n'existe pas, est échue ou acceptée, **ou**
    /// elle est d'un espace (d'une page) où la personne ne voit rien. Même
    /// réponse dans tous ces cas.
    pub fn invitation_not_found() -> ApiError {
        ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::NotFound,
            "invitation introuvable",
        )
    }

    /// `403 forbidden` : une action au-dessus de son rôle dans un espace qu'on
    /// voit, ou de son niveau sur une page qu'on voit.
    pub fn forbidden(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::FORBIDDEN, ErrorCode::Forbidden, message)
    }

    /// `409 last_owner`.
    pub fn last_owner() -> ApiError {
        ApiError::new(
            StatusCode::CONFLICT,
            ErrorCode::LastOwner,
            "un espace garde au moins un propriétaire : en nommer un autre d'abord",
        )
    }

    /// `409 roles_changed` : les rôles ont bougé à chaque tentative d'écriture.
    pub fn roles_changed() -> ApiError {
        ApiError::new(
            StatusCode::CONFLICT,
            ErrorCode::RolesChanged,
            "les rôles viennent de changer : rien n'a été modifié ; réessayer",
        )
    }

    /// `409 page_moved` (Ruling R91) : à chaque tentative d'écriture d'un `PATCH`, la page
    /// avait changé de parent, ou la personne n'avait plus ce qu'il faut pour la passer à la
    /// racine. Ce n'est pas une panne du serveur : rien n'est écrit, et réessayer suffit.
    pub fn page_moved() -> ApiError {
        ApiError::new(
            StatusCode::CONFLICT,
            ErrorCode::PageMoved,
            "la page change de place en ce moment : rien n'a été modifié ; réessayer",
        )
    }

    /// `409 last_full_access` : la page n'aurait plus personne en accès complet (Ruling R70).
    pub fn last_full_access() -> ApiError {
        ApiError::new(
            StatusCode::CONFLICT,
            ErrorCode::LastFullAccess,
            "la page n'aurait plus personne en accès complet : donner d'abord l'accès complet à \
             quelqu'un d'autre ; rien n'a été modifié",
        )
    }

    /// `409 inherited_access` : l'accès général demandé (retiré, ou plus bas) est sous celui
    /// qu'une page parente donne déjà aux membres.
    pub fn inherited_access() -> ApiError {
        ApiError::new(
            StatusCode::CONFLICT,
            ErrorCode::InheritedAccess,
            "l'accès général vient d'une page parente : il ne se retire ni ne se baisse ici, mais \
             sur elle ; rien n'a été modifié",
        )
    }

    /// `403 forbidden` (Ruling R85) : un invité ne pose ni ne retire l'accès général d'une
    /// page, même en accès complet.
    pub fn guest_general_access() -> ApiError {
        ApiError::forbidden(
            "un invité ne change pas l'accès général d'une page (ce qu'en voient les membres de \
             l'espace) : c'est aux membres de le faire",
        )
    }

    /// `403 forbidden` : il faut l'accès complet sur la page. (Avant le corps de la requête,
    /// `access.rs` donne aussi le niveau qu'on a ; ce refus-ci vient de la transaction.)
    fn below_full() -> ApiError {
        ApiError::forbidden("niveau insuffisant sur cette page : il faut « accès complet »")
    }

    /// Un changement d'accès refusé par le store, dans les statuts des routes du partage.
    fn share_refused(why: ShareRefusal) -> ApiError {
        match why {
            ShareRefusal::Unseen => ApiError::page_not_found(),
            ShareRefusal::BelowFull => ApiError::below_full(),
            ShareRefusal::GuestGeneral => ApiError::guest_general_access(),
            ShareRefusal::Trashed => ApiError::trashed(),
            ShareRefusal::NotInSpace => ApiError::member_not_found(),
            ShareRefusal::NoOwnLine => ApiError::access_not_found(),
            ShareRefusal::InheritedAccess => ApiError::inherited_access(),
        }
    }

    /// `404 not_found` : la page n'a pas d'accès propre pour ce compte (il n'en a aucun, ou
    /// seulement par une page parente), ou le compte est inconnu. Même réponse dans ces cas.
    pub fn access_not_found() -> ApiError {
        ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::NotFound,
            "ce compte n'a pas d'accès propre sur cette page : un accès hérité se change sur la \
             page qui le porte",
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

/// Une erreur, du store le plus souvent, qui ne désigne pour l'appelant ni page ni espace :
/// `500 internal`, le détail au journal. Pour les routes qui ne passent pas par
/// `From<StoreError>`, qui lirait un `NotFound` comme une page introuvable.
pub(crate) fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(&error)
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.body)).into_response();
        if let Some(seconds) = self.retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}

impl From<StoreError> for ApiError {
    /// Les cas où `NotFound` vise autre chose que la page (parent, `afterId`)
    /// sont traités par les routes avant cette conversion.
    ///
    /// `Stale` n'a pas de traduction ici : il sert aux rôles, au parent d'une
    /// page et au passage à la racine, et chaque route qui l'attend le traduit
    /// elle-même, après ses reprises (`409 roles_changed` dans
    /// `routes/spaces.rs`, `409 page_moved` dans `routes/pages.rs`). S'il
    /// arrive jusqu'ici, une route a oublié de le faire : `500`, et le journal
    /// le dit, plutôt qu'un « les rôles viennent de changer » pour un
    /// déplacement.
    fn from(e: StoreError) -> ApiError {
        match e {
            StoreError::NotFound => ApiError::page_not_found(),
            StoreError::Cycle => ApiError::new(
                StatusCode::CONFLICT,
                ErrorCode::Cycle,
                "une page ne peut pas aller sous elle-même ni sous une de ses descendantes",
            ),
            StoreError::EmailTaken => ApiError::email_taken(),
            StoreError::LastOwner => ApiError::last_owner(),
            StoreError::Stale => ApiError::internal(
                &"StoreError::Stale est arrivé à la conversion générique : la route qui a \
                  appelé le store devait le traduire elle-même (relire et redécider, puis \
                  son propre `409`)",
            ),
            StoreError::LastFullAccess => ApiError::last_full_access(),
            StoreError::Refused(why) => ApiError::share_refused(why),
            StoreError::ParentTrashed => ApiError::parent_trashed(),
            StoreError::NotTrashed => ApiError::not_trashed(),
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
    } else if text.starts_with("invalid value") || text.starts_with("unknown variant") {
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
    use std::io::Write;
    use std::sync::{Arc, Mutex, PoisonError};

    use super::*;

    /// Ce que `work` journalise sur ce fil, avec ce qu'il rend.
    ///
    /// Un second abonné, inerte, est inscrit le temps de la lecture. Seul inscrit du
    /// processus, celui de ce fil ne recevrait rien d'un point de journal que le fil d'un autre
    /// test, sans abonné, atteint le premier : `tracing` décide alors de l'intérêt du point
    /// d'après ce fil-là (`tracing-core`, `Rebuilder::JustOne`), et le tient pour éteint.
    fn logged<T>(work: impl FnOnce() -> T) -> (T, String) {
        struct Buffer(Arc<Mutex<Vec<u8>>>);
        impl Write for Buffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let _never_alone = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || Buffer(writer.clone()))
            .with_ansi(false)
            .finish();
        let value = tracing::subscriber::with_default(subscriber, work);
        let journal = buffer.lock().unwrap_or_else(PoisonError::into_inner);
        (value, String::from_utf8_lossy(&journal).into_owned())
    }

    /// `Stale` n'a pas de traduction générique : il dit « ce sur quoi l'appelant a décidé a
    /// changé », pour les rôles, pour le parent d'une page et pour un passage à la racine, et
    /// chaque route qui l'attend le traduit elle-même (`409 roles_changed`, `409 page_moved`).
    /// Arrivé ici, c'est qu'une route l'a oublié : une erreur interne, et le journal le dit.
    /// Avant, il devenait « les rôles viennent de changer », même pour un déplacement.
    #[test]
    fn stale_has_no_generic_translation() {
        let (error, journal) = logged(|| ApiError::from(StoreError::Stale));
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.body.error, ErrorCode::Internal);
        assert_eq!(error.body.message, "erreur interne du serveur");
        assert!(
            journal.contains("ERROR") && journal.contains("StoreError::Stale"),
            "une ligne `error` qui nomme la conversion oubliée : {journal}"
        );
        // Rien de la cause dans la réponse.
        assert!(!error.body.message.contains("Stale"));
    }

    /// Les autres refus du store gardent leur traduction.
    #[test]
    fn other_store_refusals_keep_their_translation() {
        let cases = [
            (
                StoreError::NotFound,
                StatusCode::NOT_FOUND,
                ErrorCode::NotFound,
            ),
            (StoreError::Cycle, StatusCode::CONFLICT, ErrorCode::Cycle),
            (
                StoreError::LastOwner,
                StatusCode::CONFLICT,
                ErrorCode::LastOwner,
            ),
            (
                StoreError::LastFullAccess,
                StatusCode::CONFLICT,
                ErrorCode::LastFullAccess,
            ),
            (
                StoreError::ParentTrashed,
                StatusCode::CONFLICT,
                ErrorCode::ParentTrashed,
            ),
            (
                StoreError::NotTrashed,
                StatusCode::CONFLICT,
                ErrorCode::NotTrashed,
            ),
        ];
        for (refusal, status, code) in cases {
            let what = refusal.to_string();
            let (error, journal) = logged(|| ApiError::from(refusal));
            assert_eq!((error.status, error.body.error), (status, code), "{what}");
            assert_eq!(journal, "", "{what} : rien au journal");
        }
    }

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
