//! Routes de l'historique d'une page (Ruling R135) : les versions que l'acteur de la page a
//! coupées (l'état Yjs complet de la page à un instant, avec les comptes qui l'ont modifiée
//! depuis la version d'avant), l'état gardé de chacune, et la restauration.
//!
//! | Route | Il faut | Réponse |
//! |---|---|---|
//! | `GET /api/v1/pages/{id}/versions` | `edit` (à la corbeille aussi) | [`Version`]`[]`, la plus récente d'abord ; cent au plus |
//! | `GET …/versions/{versionId}/state` | `edit` (à la corbeille aussi) | `application/octet-stream` : l'état Yjs (v1) tel qu'il a été gardé |
//! | `POST …/versions/{versionId}/restore` | `edit` | `204` ; `409 trashed` |
//!
//! **Refus**, dans l'ordre des commentaires (Rulings R168 et R186, écrit en tête de
//! `access.rs`) : `401` sans session ; `404 not_found` pour une page que la personne ne voit pas ;
//! `403 forbidden` sous le niveau ; le `404 not_found` d'une version qui n'est pas de cette page,
//! qui n'existe pas, ou dont l'identifiant est mal formé, **exactement la réponse d'une page
//! introuvable** ; enfin `409 trashed` pour une restauration dans une page à la corbeille. Qui ne
//! voit pas la page n'apprend pas qu'une version existe, et qui la voit n'apprend rien des
//! versions d'une autre.
//!
//! **Restaurer** suit l'ordre de `PUT /mmd` (Ruling R89) : premier contrôle, puis le verrou des
//! écritures du serveur sur la page (Ruling R200, `page_writes.rs`), chargement de la page,
//! second contrôle des droits, puis la commande à l'acteur, qui coupe la version
//! `before_restore` et écrit les blocs de la version sans rien laisser passer entre les deux
//! (`Hub::restore_version`). L'écriture est celle d'un client ou de `PUT /mmd` : journal,
//! diffusion aux connexions, qui reçoivent aussi la trame `{"type":"versions"}` ; elle va au
//! bout même si le client coupe la connexion. Restaurer ce que la page a déjà comme contenu ne
//! fait rien, et répond `204`. Un état gardé que le serveur ne relit plus (base abîmée) répond
//! `500 internal`, sans toucher à la page.
//!
//! L'historique se lit à partir de « Peut modifier » (Ruling R203 ; le plan disait « Peut
//! lire ») : quelqu'un à qui l'on partage une page en lecture ne lit pas ce qu'elle portait
//! avant le partage.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use mementum_acl::Action;
use mementum_sync::display_name;
use uuid::Uuid;

use crate::access::{permit, permit_level};
use crate::auth::{AuthUser, SessionUser};
use crate::error::ApiError;
use crate::routes::pages::page_id;
use crate::types::{Author, Version};
use crate::{AppState, to_completion};

/// Le type de contenu de l'état d'une version : des octets Yjs.
const OCTET_STREAM: &str = "application/octet-stream";

/// `GET /api/v1/pages/{id}/versions` : les versions de la page, la plus récente d'abord.
pub(crate) async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Vec<Version>>, ApiError> {
    let id = page_id(path)?;
    may_read_history(&state, &user, id).await?;
    let versions = state.store.list_versions(id).await?;
    Ok(Json(versions.iter().map(version).collect()))
}

/// `GET /api/v1/pages/{id}/versions/{versionId}/state` : l'état gardé de la version, tel quel.
pub(crate) async fn state(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let (id, raw) = version_path(path)?;
    may_read_history(&state, &user, id).await?;
    let version = version_id(&raw)?;
    let kept = state
        .store
        .get_version(id, version)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    Ok(([(header::CONTENT_TYPE, OCTET_STREAM)], kept).into_response())
}

/// `POST /api/v1/pages/{id}/versions/{versionId}/restore` : écrit les blocs de la version
/// par-dessus le contenu de la page, au nom du compte de la session.
pub(crate) async fn restore(
    State(state): State<AppState>,
    caller: SessionUser,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let SessionUser { user, session } = caller;
    let (id, raw) = version_path(path)?;
    // Premier contrôle : les refus, avant de charger la page. La version est cherchée entre le
    // niveau et la corbeille (Ruling R186).
    let (_, access, _) = state
        .seen_node(&user, id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    permit_level(&access, Action::Edit)?;
    let version = version_id(&raw)?;
    if !state.store.has_version(id, version).await? {
        return Err(ApiError::page_not_found());
    }
    permit(&access, Action::Edit)?;
    // Ruling R200 : une écriture du serveur à la fois par page. Le verrou se prend après le
    // premier contrôle, et ce qui suit va au bout, verrou tenu, même si le client coupe : la
    // requête suivante ne relit ses droits qu'une fois celle-ci écrite.
    let write = state.page_writes.lock(id).await;
    to_completion(async move {
        let _write = write;
        // La page chargée, les droits sont relus : c'est ce second contrôle qui autorise
        // l'écriture (Ruling R89). Entre lui et la restauration, il ne reste que l'envoi
        // d'une commande à l'acteur déjà chargé.
        state.hub.load_page(id).await?;
        // Ruling R209 : la session de la requête, relue sous le verrou. Une déconnexion ou un
        // changement de mot de passe arrête la file, comme un droit retiré.
        state.session_still_live(session).await?;
        state.node_access(&user, id, Action::Edit).await?;
        Ok(state
            .hub
            .restore_version(id, version, Some(user.id))
            .await?)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lire l'historique demande « Peut modifier » (Ruling R203) : `404` pour une page qu'on ne voit
/// pas, `403` en dessous du niveau ; la corbeille n'y change rien (`permit_level`), si bien que
/// qui peut modifier la page lit encore son historique à la corbeille. Un lecteur ne lit pas ce
/// que la page portait avant qu'on la lui partage : un texte effacé (un mot de passe collé par
/// erreur) reste dans les versions, et rien ne permet d'en retirer une.
async fn may_read_history(state: &AppState, user: &AuthUser, id: Uuid) -> Result<(), ApiError> {
    let (_, access, _) = state
        .seen_node(user, id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    permit_level(&access, Action::Edit)
}

/// La page et l'identifiant (encore à lire) de la version d'un chemin `…/versions/{versionId}…`.
/// Un chemin illisible : la page n'existe pas.
fn version_path(
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<(Uuid, String), ApiError> {
    let Path((page, version)) = path.map_err(|_| ApiError::page_not_found())?;
    let page = Uuid::parse_str(&page).map_err(|_| ApiError::page_not_found())?;
    Ok((page, version))
}

/// L'identifiant d'une version : mal formé, c'est une version qui n'est pas de cette page, et
/// la réponse est celle d'une page introuvable.
fn version_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::page_not_found())
}

fn version(version: &mementum_store::Version) -> Version {
    Version {
        id: version.id.to_string(),
        created_at: version.created_at,
        reason: version.reason.into(),
        authors: version
            .authors
            .iter()
            .map(|author| Author {
                id: author.id.to_string(),
                name: display_name(&author.name),
            })
            .collect(),
    }
}
