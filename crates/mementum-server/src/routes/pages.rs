//! Routes des pages : arbre, création, renommage et déplacement, lecture et
//! écriture en MMD.
//!
//! Positions (Rulings R13 et R23). Le store place un nœud `after: None` en
//! tête de ses frères ; les routes :
//! - `POST` sans `afterId` : en fin des frères (la route passe le dernier
//!   frère au store) ; `"afterId": null` : en tête ; un id : après lui ;
//! - `PATCH` sans `parentId` ni `afterId` : pas de déplacement ; `parentId`
//!   seul (`null` = racine) : en fin des enfants du nouveau parent ; `afterId`
//!   seul : réordonnancement chez le parent actuel (`null` = en tête) ; les
//!   deux : on suit les deux.

use axum::Json;
use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use mementum_acl::{Grant, Level, Subject};
use mementum_md::{Block, Content, Props, RenderOpts, parse, render};
use mementum_schema::PropSpec;
use mementum_store::{NewNode, Node, NodeKind, StoreError};
use uuid::Uuid;

use crate::AppState;
use crate::error::ApiError;
use crate::types::{CreatePage, ErrorCode, PageNode, Tree, UpdatePage};

/// Titre d'une page sans titre.
pub const UNTITLED: &str = "Sans titre";

/// Longueur maximale d'un titre, en caractères Unicode (et non en octets ni
/// en unités UTF-16).
pub const MAX_TITLE_CHARS: usize = 500;

/// Type de contenu du MMD renvoyé.
const MARKDOWN: &str = "text/markdown; charset=utf-8";

/// `GET /api/v1/tree`.
pub(crate) async fn tree(State(state): State<AppState>) -> Result<Json<Tree>, ApiError> {
    let space_id = state.default_space().await?;
    let nodes = state.store.list_nodes(space_id).await?;
    Ok(Json(Tree {
        // Le type de l'API ne change qu'avec les pages sous droits : d'ici là, `vaultId` porte
        // l'id de l'espace.
        vault_id: space_id.to_string(),
        nodes: nodes.iter().map(page_node).collect(),
    }))
}

/// `POST /api/v1/pages` : crée le nœud, puis son document.
pub(crate) async fn create(
    State(state): State<AppState>,
    body: Result<Json<CreatePage>, JsonRejection>,
) -> Result<(StatusCode, Json<PageNode>), ApiError> {
    let Json(body) = body?;
    let title = title(body.title.as_deref())?;
    let space_id = state.default_space().await?;
    let parent = match body.parent_id {
        None => None,
        Some(id) => Some(parent(&state, space_id, &id).await?),
    };
    let after = match body.after_id {
        None => last_child(&state, space_id, parent).await?,
        Some(None) => None,
        Some(Some(id)) => Some(after_id(&id)?),
    };
    // Sans compte (routes du lot 1) : pas d'auteur ; une page de racine va à tous les membres
    // de l'espace, une sous-page hérite de son parent.
    let grants = match parent {
        None => vec![Grant {
            subject: Subject::Members,
            level: Level::Full,
        }],
        Some(_) => Vec::new(),
    };
    let node = state
        .store
        .create_node(NewNode {
            space_id,
            parent_id: parent,
            after,
            kind: NodeKind::Page,
            title,
            created_by: None,
            grants,
            // Le document est écrit après le nœud, par le hub (Ruling R32).
            doc: None,
        })
        .await
        .map_err(placement_error)?;
    // Ruling R32 : le nœud existe déjà, et son acteur lui écrira son document
    // au premier chargement (R28). Une erreur ici n'est que journalisée : un
    // 5xx pousserait le client à réessayer, et à créer un doublon.
    if let Err(error) = state.hub.create_page_doc(node.id).await {
        tracing::warn!(
            node_id = %node.id,
            %error,
            "document de la page neuve non écrit ; il le sera à sa première ouverture"
        );
    }
    Ok((StatusCode::CREATED, Json(page_node(&node))))
}

/// `PATCH /api/v1/pages/{id}` : déplacement (s'il est demandé), puis
/// renommage. Tout est vérifié avant d'écrire : un refus ne change rien.
pub(crate) async fn update(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<UpdatePage>, JsonRejection>,
) -> Result<Json<PageNode>, ApiError> {
    let id = page_id(path)?;
    let mut node = state.page(id).await?;
    let Json(body) = body?;
    let title = body.title.as_deref().map(|t| title(Some(t))).transpose()?;
    if body.parent_id.is_some() || body.after_id.is_some() {
        // L'espace de la page : celui que `state.page` vient de vérifier.
        let space_id = node.space_id;
        let parent = match body.parent_id {
            None => node.parent_id,
            Some(None) => None,
            Some(Some(p)) => Some(parent(&state, space_id, &p).await?),
        };
        let after = match body.after_id {
            // Si la page est déjà la dernière, le store la laisse en place.
            None => last_child(&state, space_id, parent).await?,
            Some(None) => None,
            Some(Some(a)) => Some(after_id(&a)?),
        };
        node = state
            .store
            .move_node(id, parent, after, None)
            .await
            .map_err(placement_error)?;
    }
    if let Some(title) = title {
        node = state.store.rename_node(id, &title, None).await?;
    }
    Ok(Json(page_node(&node)))
}

/// `GET /api/v1/pages/{id}/mmd` : le MMD canonique de la page, sans ancres.
/// Une page inconnue donne `404` sans rien créer.
pub(crate) async fn get_mmd(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Response, ApiError> {
    let id = page_id(path)?;
    state.page(id).await?;
    let blocks = state.hub.read_blocks(id).await?;
    let mmd = render(&blocks, RenderOpts::default()).markdown;
    Ok(([(header::CONTENT_TYPE, MARKDOWN)], mmd).into_response())
}

/// `PUT /api/v1/pages/{id}/mmd` : remplace le contenu de la page. L'écriture
/// est persistée, puis diffusée aux connexions ouvertes. Une prop hors
/// registre est refusée (`422 invalid_blocks`, Ruling R41).
pub(crate) async fn put_mmd(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Result<StatusCode, ApiError> {
    let id = page_id(path)?;
    state.page(id).await?;
    let body = body?;
    let text = std::str::from_utf8(&body).map_err(|e| {
        let line = body[..e.valid_up_to()]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        ApiError::from(mementum_md::MmdError {
            line,
            message: "le texte n'est pas de l'UTF-8 valide".to_string(),
        })
    })?;
    let mut blocks = parse(text)?;
    registered_props_only(&blocks)?;
    unknown_not_foreign(&mut blocks);
    state.hub.write_blocks(id, blocks).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Ruling R41 : au lot 1, `PUT /mmd` n'écrit que les props du registre
/// (`mementum-schema`), sur un bloc comme sur une cellule de tableau. Le format
/// MMD, lui, lit et rend toujours le sac de props (`GET /mmd` rend telle
/// quelle une prop inconnue déjà présente dans Yjs). Mais écrite en attribut
/// Yjs, une prop que le schéma du client ne déclare pas est retirée du
/// document partagé par y-prosemirror dès qu'un navigateur modifie le bloc,
/// chez tout le monde et sans trace : mieux vaut un refus qu'un `204` suivi
/// d'une perte. À lever quand un porteur durable existera (un attribut
/// déclaré au schéma du client).
///
/// Les blocs sont parcourus dans l'ordre du document : l'erreur nomme la
/// première prop refusée. Pas de numéro de ligne, que l'arbre lu ne garde pas.
fn registered_props_only(blocks: &[Block]) -> Result<(), ApiError> {
    let mut stack = vec![blocks.iter()];
    while let Some(level) = stack.last_mut() {
        let Some(block) = level.next() else {
            stack.pop();
            continue;
        };
        // `parse` ne rend que des types du registre (`unknown` compris).
        let known = mementum_schema::block(&block.kind).map_or(&[][..], |spec| spec.props);
        if let Some(name) = unregistered(&block.props, known) {
            return Err(unregistered_prop(
                name,
                format!("un bloc « {} »", block.kind),
                "ce type de bloc",
                known,
            ));
        }
        if let Content::Table(table) = &block.content {
            let known = mementum_schema::cell_props();
            for cell in table.rows.iter().flatten() {
                if let Some(name) = unregistered(&cell.props, known) {
                    return Err(unregistered_prop(
                        name,
                        format!("une cellule d'un bloc « {} »", block.kind),
                        "une cellule",
                        known,
                    ));
                }
            }
        }
        stack.push(block.children.iter());
    }
    Ok(())
}

/// La première prop de `props` (par nom) que le registre ne déclare pas.
/// Après `parse`, une prop du registre porte son nom BlockNote, et une prop
/// inconnue le nom écrit dans le MMD.
fn unregistered<'a>(props: &'a Props, known: &[PropSpec]) -> Option<&'a str> {
    props
        .keys()
        .map(String::as_str)
        .find(|name| !known.iter().any(|p| p.name == *name))
}

/// `422 invalid_blocks` : la prop, où elle est, et ce qu'on peut écrire à la
/// place (les props du registre, sous leur nom MMD).
fn unregistered_prop(name: &str, place: String, target: &str, known: &[PropSpec]) -> ApiError {
    let allowed = if known.is_empty() {
        format!("{target} n'a aucune prop au registre")
    } else {
        let names: Vec<String> = known.iter().map(|p| format!("« {} »", p.mmd)).collect();
        format!("props du registre pour {target} : {}", names.join(", "))
    };
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::InvalidBlocks,
        format!(
            "prop « {name} » hors registre sur {place} : la retirer ({allowed}). PUT /mmd ne \
             l'écrit pas, car un navigateur l'effacerait du document dès qu'il modifie le bloc"
        ),
    )
}

/// Ruling R33 : un bloc `unknown` écrit par MMD n'est jamais recréé en élément
/// étranger (`foreign = true`), que y-prosemirror supprimerait chez tous dès
/// qu'un navigateur ouvre la page. Il devient le bloc custom `unknown` de
/// BlockNote, affiché et conservé, `originalType` et `data` gardés. Au lot 1,
/// un élément étranger ne vient que du MMD : un client d'un schéma plus récent
/// est en lecture seule.
fn unknown_not_foreign(blocks: &mut [Block]) {
    let mut stack = vec![blocks];
    while let Some(level) = stack.pop() {
        for block in level.iter_mut() {
            if block.kind == "unknown" {
                block.props.remove("foreign");
            }
            stack.push(&mut block.children);
        }
    }
}

/// L'id d'une page dans le chemin : illisible, la page n'existe pas.
pub(crate) fn page_id(path: Result<Path<String>, PathRejection>) -> Result<Uuid, ApiError> {
    path.ok()
        .and_then(|Path(id)| Uuid::parse_str(&id).ok())
        .ok_or_else(ApiError::page_not_found)
}

/// Règles du titre (Review Focus 5) : espaces de bord retirées ; vide ou
/// absent : « Sans titre » ; plus de 500 caractères : `422 title_too_long`.
pub fn title(raw: Option<&str>) -> Result<String, ApiError> {
    let title = raw.unwrap_or("").trim();
    if title.is_empty() {
        return Ok(UNTITLED.to_string());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::TitleTooLong,
            format!("un titre a {MAX_TITLE_CHARS} caractères au plus"),
        ));
    }
    Ok(title.to_string())
}

/// Le parent visé : une page de l'espace, sinon `422 invalid_parent`.
async fn parent(state: &AppState, space_id: Uuid, raw: &str) -> Result<Uuid, ApiError> {
    let id = Uuid::parse_str(raw).map_err(|_| ApiError::invalid_parent())?;
    match state.store.get_node(id).await? {
        Some(node) if node.space_id == space_id => Ok(id),
        _ => Err(ApiError::invalid_parent()),
    }
}

fn after_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::invalid_after())
}

/// Le dernier enfant de `parent` (`None` = racine), pour placer une page en
/// fin de liste.
async fn last_child(
    state: &AppState,
    space_id: Uuid,
    parent: Option<Uuid>,
) -> Result<Option<Uuid>, ApiError> {
    let nodes = state.store.list_nodes(space_id).await?;
    Ok(nodes
        .into_iter()
        .filter(|n| n.parent_id == parent)
        .max_by(|a, b| a.position.cmp(&b.position))
        .map(|n| n.id))
}

/// Erreur d'une création ou d'un déplacement. La page et le parent ont été
/// vérifiés avant : un `NotFound` du store vient de `afterId`, qui n'est pas
/// un enfant du parent visé.
fn placement_error(e: StoreError) -> ApiError {
    match e {
        StoreError::NotFound => ApiError::invalid_after(),
        e => e.into(),
    }
}

fn page_node(node: &Node) -> PageNode {
    PageNode {
        id: node.id.to_string(),
        parent_id: node.parent_id.map(|p| p.to_string()),
        kind: match node.kind {
            NodeKind::Page => "page",
            NodeKind::Folder => "folder",
        }
        .to_string(),
        title: node.title.clone(),
        position: node.position.clone(),
        updated_at: node.updated_at,
    }
}
