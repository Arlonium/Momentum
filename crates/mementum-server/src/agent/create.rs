//! Des pages créées en lot avec leur contenu (lot 5, Task S7 ; Rulings R1235, R1243, R590) : la
//! route `POST /api/v1/spaces/{id}/pages/batch` et l'outil `create_pages` (M2) appellent
//! [`create_pages`].
//!
//! Comme une duplication (R1243) : le débit des duplications du compte (une à la fois, 64 Mio de
//! documents bâtis par minute, sur la part de l'appel, R1242), les documents bâtis hors des fils
//! du serveur (`mmd_reads.run`), écrits ensemble dans la voie des copies, en une transaction,
//! toutes les pages ou aucune. Trois contrôles : avant de lire le corps ; juste avant d'écrire,
//! une fois les documents bâtis (l'appel relu, la destination rejugée) ; puis dans la transaction
//! (le droit de créer là, le jeton vivant, Rulings R590, R1356). Une écriture par jeton ne pose
//! aucun média à adresse web (R1235) : la page neuve n'en porte aucun d'avance.

use std::sync::Arc;

use axum::http::StatusCode;
use mementum_acl::{Action, Grant, Level, NodeAccess, Subject};
use mementum_crdt::PageDoc;
use mementum_md::Block;
use mementum_store::{NewPageDoc, NewPages, StoreError};
use uuid::Uuid;

use crate::access::{permit, permit_root_page};
use crate::auth::{AuthUser, LiveCaller};
use crate::duplications::Busy;
use crate::error::ApiError;
use crate::routes::pages::{page_node, read_mmd};
use crate::types::{CreatePages, ErrorCode, PageNode, RootSection};
use crate::{AppState, now_ms, to_completion};

/// Pages d'un lot, au plus (`mementum_mcp::tools::MAX_PAGES`).
pub const MAX_BATCH_PAGES: usize = mementum_mcp::tools::MAX_PAGES;

/// Reprises quand ce sur quoi le lot a décidé ne tient plus à son tour d'écriture (R590) ;
/// au-delà, `409 page_moved`.
const BATCH_RETRIES: usize = 3;

/// Où vont les pages, et ce que leur auteur pourra en faire.
struct Destination {
    parent: Option<Uuid>,
    grants: Vec<Grant>,
    access: NodeAccess,
}

/// `POST /api/v1/spaces/{id}/pages/batch`, et l'outil `create_pages` : `201` et les pages, toutes
/// ou aucune. `401`, puis `404` sans rôle dans l'espace (ou hors de la portée du jeton) ont été
/// dits par l'appelant avant le corps ; puis, dans l'ordre du code (défaut I6 de la relecture
/// finale) : la destination (`422 invalid_parent`, `403` sous le niveau ou pour une page à la
/// racine d'un invité ou d'un jeton « Lecture ») ; la forme sans rien bâtir (`422 invalid_request`
/// pour `section` avec une page parente, `too_many_pages`, `title_too_long` avec `index`) ; le
/// débit (`429 duplication_busy`) ; puis, page par page, en bâtissant les documents, un MMD
/// (`invalid_mmd` avec `index` et `line`, `invalid_blocks`, `too_many_blocks`,
/// `too_long_to_write`, un média du web par jeton : `external_media`) et le poids de tout le lot
/// (`subtree_too_large`) ; enfin, à l'écriture, `409 page_moved` si la destination a bougé trois
/// fois (R590), `401` si l'appel ne vaut plus.
pub(crate) async fn create_pages(
    state: &AppState,
    caller: &LiveCaller,
    space_id: Uuid,
    request: CreatePages,
) -> Result<Vec<PageNode>, ApiError> {
    let user = &caller.user;
    let parent = match request.parent_id.as_deref() {
        None => None,
        Some(raw) => Some(Uuid::parse_str(raw).map_err(|_| ApiError::invalid_parent())?),
    };
    let section = request.section.unwrap_or_default();
    // La destination d'abord (`403` sous le niveau), puis la forme du corps (`422`) : l'ordre des
    // refus (tour de correction de S7).
    destination(state, user, space_id, parent, section).await?;
    if parent.is_some() && request.section.is_some() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidRequest,
            "« section » ne se donne qu'à la racine : une sous-page hérite des accès de sa page \
             parente (retirer « section », ou « parentId »)",
        ));
    }
    if request.pages.is_empty() || request.pages.len() > MAX_BATCH_PAGES {
        return Err(ApiError::too_many_pages(MAX_BATCH_PAGES));
    }
    let mut titles = Vec::with_capacity(request.pages.len());
    for (i, page) in request.pages.iter().enumerate() {
        titles.push(
            crate::routes::pages::title(page.title.as_deref()).map_err(|e| e.with("index", i))?,
        );
    }
    // Ruling R1243 : comme une duplication, sur la part de l'appel (R1242).
    let running = Arc::new(state.duplications.begin(user.party()).map_err(busy)?);
    let forbid_media = user.token().is_some();
    let mut built = Vec::with_capacity(request.pages.len());
    let mut weight = 0usize;
    for (i, (page, title)) in request.pages.into_iter().zip(titles).enumerate() {
        let markdown = page.markdown.unwrap_or_default();
        let doc = state
            .mmd_reads
            .run(user.party(), {
                let running = Arc::clone(&running);
                move || {
                    let doc = built_doc(&markdown, forbid_media).map_err(|e| e.with("index", i))?;
                    running.built(doc.len());
                    Ok::<_, ApiError>(doc)
                }
            })
            .await??;
        weight += doc.len();
        if weight > state.max_duplicated_bytes {
            return Err(ApiError::subtree_too_large(format!(
                "ces pages pèsent plus de {} Mio une fois écrites : crée-les en plusieurs appels ; \
                 rien n'a été créé",
                state.max_duplicated_bytes >> 20
            )));
        }
        built.push(NewPageDoc {
            id: Uuid::now_v7(),
            title,
            doc,
        });
    }

    let state = state.clone();
    let LiveCaller { user, proof } = caller.clone();
    let token = user.token().map(|grant| grant.token_id);
    // La portée d'un jeton de pages, relue dans la transaction (tour de correction de S7).
    let token_roots = user
        .token()
        .and_then(|grant| grant.roots().map(<[Uuid]>::to_vec));
    to_completion(async move {
        // La place du compte est rendue quand cette tâche finit, client parti ou non.
        let _running = running;
        let mut pages = built;
        for _ in 0..BATCH_RETRIES {
            // Second contrôle, juste avant d'écrire : l'appel, puis la destination.
            state.caller_still_live(proof).await?;
            let now = destination(&state, &user, space_id, parent, section).await?;
            let new = NewPages {
                space_id,
                parent_id: now.parent,
                grants: now.grants,
                created_by: user.id,
                token,
                token_roots: token_roots.clone(),
                pages,
            };
            match state.store.create_pages(&new, now_ms()).await {
                Ok(nodes) => {
                    return Ok(nodes
                        .iter()
                        .map(|node| page_node(node, &now.access, None))
                        .collect());
                }
                // Le droit de créer là a bougé pendant l'attente du tour d'écriture : rien n'est
                // écrit, on relit et on redécide.
                Err(StoreError::Stale | StoreError::NotFound) => pages = new.pages,
                Err(error) => return Err(error.into()),
            }
        }
        Err(ApiError::page_moved())
    })
    .await
}

/// Où vont les pages : sous `parent`, qu'il faut voir (sinon `422 invalid_parent`, comme une page
/// inconnue) et où il faut pouvoir créer ; ou à la racine de l'espace, dans `section`, ce qui
/// demande d'être membre (et, par jeton, le niveau « Modification », `permit_root_page`).
async fn destination(
    state: &AppState,
    user: &AuthUser,
    space_id: Uuid,
    parent: Option<Uuid>,
    section: RootSection,
) -> Result<Destination, ApiError> {
    let space = state.space_access(user, space_id).await?;
    match parent {
        Some(parent) => {
            let access = space
                .node(parent)
                .copied()
                .ok_or_else(ApiError::invalid_parent)?;
            permit(&access, Action::CreateChild)?;
            Ok(Destination {
                parent: Some(parent),
                grants: Vec::new(),
                // Sans ligne propre, une sous-page a les accès de sa page parente.
                access: NodeAccess {
                    visible_parent: Some(parent),
                    ..access
                },
            })
        }
        None => {
            let role = permit_root_page(&space, user)?;
            let grant = Grant {
                subject: match section {
                    RootSection::Space => Subject::Members,
                    RootSection::Private => Subject::User(user.id),
                },
                level: Level::Full,
            };
            let access = AppState::own_access(&[grant], user, role)
                .ok_or_else(|| ApiError::internal(&"page neuve sans accès pour son auteur"))?;
            Ok(Destination {
                parent: None,
                grants: vec![grant],
                access,
            })
        }
    }
}

/// Le document d'une page neuve, de son MMD : lu et vérifié comme celui de `PUT /mmd`, sans média
/// du web pour un jeton (R1235), écrit dans une page vide, puis relu comme le chargement le relira
/// (la règle de R816 : il est la seule copie de ce qu'il porte).
fn built_doc(markdown: &str, forbid_media: bool) -> Result<Vec<u8>, ApiError> {
    let blocks = if markdown.trim().is_empty() {
        Vec::new()
    } else if super::fetch_header(markdown) {
        // L'en-tête de `fetch` recopié avec le contenu (Ruling R1550) : nommé, sans citation.
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidMmd,
            super::FETCH_HEADER_REFUSAL,
        ));
    } else {
        read_mmd(super::agent_mmd(markdown).as_bytes())?
    };
    if forbid_media && let Some(url) = external_media(&blocks) {
        return Err(ApiError::external_media(format!(
            "une page créée par un jeton d'API ne porte pas d'image, de vidéo ni de son dont \
             l'adresse « {} » est sur le web : un agent ne pose que des médias envoyés à \
             l'instance ; un lien ([texte](adresse)) passe ; rien n'a été créé",
            mementum_md::excerpt(url)
        ))
        .quoting());
    }
    let page = PageDoc::new_page().0;
    if !blocks.is_empty() {
        page.write_blocks(&blocks)
            .map_err(|e| ApiError::from(mementum_sync::SyncError::Crdt(e)))?;
    }
    let state = page.encode_state();
    PageDoc::check_state(&state).map_err(|e| ApiError::internal(&e))?;
    Ok(state)
}

/// L'adresse du premier bloc `image`, `video` ou `audio` qui n'est pas un fichier de l'instance
/// (R1235 : une page neuve ne porte aucun média d'avance), à toute profondeur.
pub(crate) fn external_media(blocks: &[Block]) -> Option<&str> {
    blocks.iter().find_map(|b| {
        let url = match (b.kind.as_str(), b.props.get("url")) {
            ("image" | "video" | "audio", Some(mementum_md::PropValue::Str(url))) => Some(url),
            _ => None,
        };
        url.filter(|url| !url.is_empty() && crate::routes::duplicate::file_address(url).is_none())
            .map(String::as_str)
            .or_else(|| external_media(&b.children))
    })
}

/// `429 duplication_busy` : pourquoi le compte ne crée pas maintenant, et quand revenir.
fn busy(busy: Busy) -> ApiError {
    match busy {
        Busy::Running => ApiError::duplication_busy(
            "une duplication ou une création de pages de ce compte est déjà en cours : attends sa \
             fin, puis recommence",
            1,
        ),
        Busy::Spent { retry_after } => ApiError::duplication_busy(
            format!(
                "ce compte a beaucoup dupliqué ou créé de pages dans la dernière minute : \
                 recommence dans {retry_after} s"
            ),
            retry_after,
        ),
    }
}
