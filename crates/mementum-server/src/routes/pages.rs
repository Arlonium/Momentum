//! Routes des pages : arbre et corbeille d'un espace, création, lecture, renommage et
//! déplacement, corbeille, restauration, suppression définitive, lecture et écriture en MMD.
//!
//! **Droits.** Chaque route demande une session, puis passe par `access.rs` avant de lire ou
//! d'écrire quoi que ce soit. Les refus viennent toujours dans cet ordre :
//! 1. `401 unauthenticated` sans session ;
//! 2. `404 not_found` pour une page que la personne ne voit pas : inconnue, mal formée, d'un
//!    espace où elle n'a aucun rôle, ou qu'elle ne peut pas lire. La réponse est la même dans
//!    tous ces cas, et elle part avant que le corps de la requête soit lu ;
//! 3. `403 forbidden` si son niveau ne permet pas l'action ;
//! 4. `409 trashed` si l'action écrit dans une page à la corbeille.
//!
//! Une page nommée dans le corps (`parentId`, `afterId`) que la personne ne voit pas reçoit
//! l'erreur d'un id inconnu (`422 invalid_parent`, `422 invalid_after`).
//!
//! | Route | Il faut |
//! |---|---|
//! | `GET /api/v1/spaces/{id}/tree`, `…/trash` | un rôle dans l'espace |
//! | `POST /api/v1/pages` | à la racine : être membre de l'espace ; sous une page : `CreateChild` sur elle |
//! | `GET /api/v1/pages/{id}`, `GET …/mmd` | `Read` |
//! | `PATCH /api/v1/pages/{id}` | titre : `Rename` ; place sous le même parent : `Reorder` ; autre parent : `Move` sur la page et `CreateChild` sur le nouveau parent (à la racine : être membre) |
//! | `POST …/trash`, `POST …/restore` | `Trash`, `Restore` |
//! | `DELETE /api/v1/pages/{id}` | `Delete`, et que la page soit à la corbeille |
//! | `PUT …/mmd` | `Edit` |
//!
//! **Positions** (Rulings R13 et R23). Le store place un nœud `after: None` en tête de ses
//! frères ; les routes :
//! - `POST` sans `afterId` : en fin des frères (`Sibling::Last` : le store trouve le dernier
//!   frère dans sa transaction, la route ne lit pas l'espace) ; `"afterId": null` : en tête ;
//!   un id : après lui ;
//! - `PATCH` sans `parentId` ni `afterId` : pas de déplacement ; `parentId` seul (`null` =
//!   racine) : en fin des enfants du nouveau parent ; `afterId` seul : réordonnancement chez
//!   le parent actuel (`null` = en tête) ; les deux : on suit les deux. Une page déjà à la
//!   place demandée n'est pas écrite.
//!
//! **Connexions de synchro.** Mettre à la corbeille, restaurer et changer de parent changent
//! ce que chacun peut faire de la page et de ses sous-pages : après le commit, les connexions
//! de synchro du sous-arbre sont fermées (`rights_changed`), et se rouvrent en relisant leurs
//! droits. Une suppression définitive arrête les acteurs des pages supprimées (`Hub::forget`).
//! L'écriture et cette suite vont au bout même si le client coupe la connexion.
//!
//! **Réponse d'une écriture.** Créer une page et la changer de parent changent les droits de
//! l'espace. La réponse n'est pas relue pour autant : elle se compose de ce que l'écriture a
//! rendu et des accès lus avant elle (ceux du parent, les lignes de la page). Relire pourrait
//! répondre `404` à une écriture réussie, si les droits bougent entre-temps, et le client
//! réessaierait.

use std::cmp::Reverse;

use axum::Json;
use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use mementum_acl::{Action, Grant, Level, NodeAccess, Role, SpaceAccess, Subject};
use mementum_crdt::PageDoc;
use mementum_md::{Block, Content, Props, RenderOpts, parse, render};
use mementum_schema::PropSpec;
use mementum_store::{NewNode, Node, NodeKind, Place, Sibling, StoreError};
use uuid::Uuid;

use crate::access::{at_least, permit};
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::routes::spaces::space_id;
use crate::types::{CreatePage, ErrorCode, PageNode, RootSection, Tree, UpdatePage};
use crate::{AppState, to_completion};

/// Titre d'une page sans titre.
pub const UNTITLED: &str = "Sans titre";

/// Longueur maximale d'un titre, en caractères Unicode (et non en octets ni
/// en unités UTF-16).
pub const MAX_TITLE_CHARS: usize = 500;

/// Type de contenu du MMD renvoyé.
const MARKDOWN: &str = "text/markdown; charset=utf-8";

/// Reprises d'un `PATCH` quand la page a changé de parent entre la lecture de ses droits et
/// l'écriture : la route relit et redécide. Au-delà, `503 unavailable`.
const MOVE_RETRIES: usize = 3;

/// `GET /api/v1/spaces/{id}/tree` : les pages de l'espace que la personne voit, hors
/// corbeille, chacune avec son niveau et sa section.
pub(crate) async fn tree(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Tree>, ApiError> {
    let space_id = space_id(path)?;
    let access = state.space_access(&user, space_id).await?;
    let nodes = state.store.list_nodes(space_id).await?;
    let mut nodes: Vec<PageNode> = nodes
        .iter()
        .filter_map(|node| {
            let access = access.node(node.id).filter(|access| !access.in_trash)?;
            Some(page_node(node, access, None))
        })
        .collect();
    // Une page dont le parent n'est pas visible est rendue à la racine : le tri se fait sur
    // le parent rendu, pas sur celui du store.
    nodes.sort_by(|a, b| {
        (&a.parent_id, &a.position, &a.id).cmp(&(&b.parent_id, &b.position, &b.id))
    });
    Ok(Json(Tree {
        space_id: space_id.to_string(),
        nodes,
    }))
}

/// `GET /api/v1/spaces/{id}/trash` : les pages mises à la corbeille (pas leurs descendants,
/// qui n'y sont que par héritage) que la personne peut restaurer, la plus récente d'abord.
pub(crate) async fn trash_list(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Vec<PageNode>>, ApiError> {
    let space_id = space_id(path)?;
    let access = state.space_access(&user, space_id).await?;
    let nodes = state.store.list_nodes(space_id).await?;
    let mut trashed: Vec<(i64, PageNode)> = nodes
        .iter()
        .filter_map(|node| {
            let at = node.trashed_at?;
            let access = access
                .node(node.id)
                .filter(|access| access.level.allows(Action::Restore))?;
            Some((at, page_node(node, access, Some(at))))
        })
        .collect();
    trashed.sort_by(|(at_a, a), (at_b, b)| (Reverse(at_a), &a.id).cmp(&(Reverse(at_b), &b.id)));
    Ok(Json(trashed.into_iter().map(|(_, node)| node).collect()))
}

/// `POST /api/v1/pages` : crée une page, à la racine d'un espace (`spaceId`) ou sous une page
/// (`parentId`). Le nœud, ses accès et son document sont écrits en une transaction.
///
/// À la racine, la page reçoit les accès de sa section : `(members, full)` dans « Espace »,
/// `(user:<auteur>, full)` dans « Privé » (le défaut). Une sous-page n'en reçoit aucun : elle
/// hérite de sa page parente.
pub(crate) async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    body: Result<Json<CreatePage>, JsonRejection>,
) -> Result<(StatusCode, Json<PageNode>), ApiError> {
    let Json(body) = body?;
    let title = title(body.title.as_deref())?;
    // Un espace mal nommé n'existe pas, comme dans un chemin.
    let named_space = body
        .space_id
        .as_deref()
        .map(|raw| Uuid::parse_str(raw).map_err(|_| ApiError::space_not_found()))
        .transpose()?;
    // Où va la page, les accès qu'elle reçoit, et ce que son auteur pourra en faire.
    let (space_id, space, parent, grants, access) = match body.parent_id.as_deref() {
        Some(raw) => {
            if body.section.is_some() {
                return Err(unexpected_body(
                    "« section » ne se donne qu'à la racine : une sous-page hérite des accès de \
                     sa page parente (retirer « section », ou « parentId »)",
                ));
            }
            if let Some(space_id) = named_space {
                state.space_access(&user, space_id).await?;
            }
            let id = Uuid::parse_str(raw).map_err(|_| ApiError::invalid_parent())?;
            let (parent, access, space) = state
                .seen_node(&user, id)
                .await?
                .filter(|(parent, ..)| named_space.is_none_or(|space| space == parent.space_id))
                .ok_or_else(ApiError::invalid_parent)?;
            permit(&access, Action::CreateChild)?;
            // Sans ligne propre, la sous-page a les accès de sa page parente.
            let inherited = NodeAccess {
                visible_parent: Some(parent.id),
                ..access
            };
            (
                parent.space_id,
                space,
                Some(parent.id),
                Vec::new(),
                inherited,
            )
        }
        None => {
            let space_id = named_space.ok_or_else(|| {
                unexpected_body(
                    "champ manquant : « spaceId » (l'espace où créer la page, obligatoire sans \
                     « parentId »)",
                )
            })?;
            let space = state.space_access(&user, space_id).await?;
            let role = at_least(&space, Role::Member)?;
            let subject = match body.section.unwrap_or_default() {
                RootSection::Space => Subject::Members,
                RootSection::Private => Subject::User(user.id),
            };
            let grant = Grant {
                subject,
                level: Level::Full,
            };
            let access = AppState::own_access(&[grant], &user, role)
                .ok_or_else(|| ApiError::internal(&"page neuve sans accès pour son auteur"))?;
            (space_id, space, None, vec![grant], access)
        }
    };
    let after = match &body.after_id {
        None => Some(Sibling::Last),
        Some(None) => None,
        Some(Some(raw)) => {
            let (sibling, _) = seen_in(&space, raw).ok_or_else(ApiError::invalid_after)?;
            Some(Sibling::Node(sibling))
        }
    };
    // Le document que l'acteur de la page écrirait à son premier chargement : écrit avec le
    // nœud, il n'y a plus d'instant où la page existe sans lui.
    let doc = PageDoc::new_page().0.encode_state();
    let new = NewNode {
        space_id,
        parent_id: parent,
        after,
        kind: NodeKind::Page,
        title,
        created_by: Some(user.id),
        grants,
        doc: Some(doc),
    };
    match state.store.create_node(new).await {
        Ok(node) => Ok((StatusCode::CREATED, Json(page_node(&node, &access, None)))),
        Err(error) => Err(placement_error(&state, error, user.id, space_id, None, parent).await),
    }
}

/// `GET /api/v1/pages/{id}` : la page, telle que la personne la voit.
pub(crate) async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<PageNode>, ApiError> {
    let id = page_id(path)?;
    let (node, access) = state.node_access(&user, id, Action::Read).await?;
    let trashed_at = trashed_since(&state, &node, &access).await?;
    Ok(Json(page_node(&node, &access, trashed_at)))
}

/// `PATCH /api/v1/pages/{id}` : renomme, déplace, ou les deux, en une seule écriture. Tout
/// est vérifié avant d'écrire, et un refus ne change rien, pas même le titre. Une page déjà à
/// la place demandée n'est pas écrite.
///
/// Qui passe une page à la racine y a toujours l'accès complet : sa ligne `(user:<lui>, full)`
/// est posée, ou relevée si elle était plus faible, dans la transaction du déplacement (Ruling
/// R82). Sans cela, une page qui ne tenait son accès complet que de ses ancêtres n'aurait
/// plus personne pour la gérer.
///
/// La place est décidée sur le parent lu avec les droits. Si la page change de parent avant
/// l'écriture, le store refuse (`Stale`) : la route relit et redécide, [`MOVE_RETRIES`] fois
/// au plus. Un changement de place autorisé par `edit` ne ramène donc jamais une page sous un
/// parent qu'elle a quitté entre-temps.
pub(crate) async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<UpdatePage>, JsonRejection>,
) -> Result<Json<PageNode>, ApiError> {
    let id = page_id(path)?;
    // La page d'abord : qui ne la voit pas reçoit `404` avant que le corps soit lu.
    let mut seen = state
        .seen_node(&user, id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    let Json(body) = body?;
    let title = body.title.as_deref().map(|t| title(Some(t))).transpose()?;
    let mut retries = 0;
    loop {
        let (node, access, space) = seen;
        if title.is_some() {
            permit(&access, Action::Rename)?;
        }
        let moving = planned_move(&state, &user, &node, &access, &space, &body).await?;
        if title.is_none() && moving.is_none() {
            let trashed_at = trashed_since(&state, &node, &access).await?;
            return Ok(Json(page_node(&node, &access, trashed_at)));
        }
        let (place, root_grant, changes_parent) = match &moving {
            Some(moving) => (Some(moving.place), moving.root_grant, moving.changes_parent),
            None => (None, None, false),
        };
        let writer = state.clone();
        let (space_id, by, new_title) = (node.space_id, user.id, title.clone());
        let written = to_completion(async move {
            let written = writer
                .store
                .update_node(id, new_title.as_deref(), place, Some(by), root_grant)
                .await;
            if written.is_ok() && changes_parent {
                writer.subtree_rights_changed(space_id, id).await;
            }
            Ok(written)
        })
        .await?;
        match written {
            // Ni un titre ni une place sous le même parent ne changent les accès, et une
            // page à la corbeille n'arrive pas ici.
            Ok(updated) => {
                let access = moving.map_or(access, |moving| moving.access);
                return Ok(Json(page_node(&updated, &access, None)));
            }
            Err(StoreError::Stale) if retries < MOVE_RETRIES => {
                retries += 1;
                tracing::debug!(
                    node_id = %id,
                    retries,
                    "la page a changé de parent avant l'écriture : la route relit et redécide"
                );
                seen = state
                    .seen_node(&user, id)
                    .await?
                    .ok_or_else(ApiError::page_not_found)?;
            }
            Err(StoreError::Stale) => {
                return Err(ApiError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    ErrorCode::Unavailable,
                    "la page change de place en ce moment : rien n'a été modifié ; réessayer",
                ));
            }
            Err(error) => {
                let parent = place.and_then(|place| place.parent_id);
                return Err(placement_error(&state, error, by, space_id, Some(id), parent).await);
            }
        }
    }
}

/// Un changement de place décidé par `PATCH`, droits vérifiés.
struct Move {
    place: Place,
    /// L'accès à poser si la page arrive à la racine (Ruling R82).
    root_grant: Option<Grant>,
    /// La page change de parent : les droits de son sous-arbre changent.
    changes_parent: bool,
    /// Les accès de la personne à la page une fois le déplacement écrit.
    access: NodeAccess,
}

/// La place que demande `body` pour la page, si la personne a le droit de l'y mettre ; `None`
/// s'il ne demande aucune place. Sous le même parent, il faut `Reorder` ; sous un autre, `Move`
/// sur la page et `CreateChild` sur le nouveau parent, ou être membre pour la racine.
async fn planned_move(
    state: &AppState,
    user: &AuthUser,
    node: &Node,
    access: &NodeAccess,
    space: &SpaceAccess,
    body: &UpdatePage,
) -> Result<Option<Move>, ApiError> {
    if body.parent_id.is_none() && body.after_id.is_none() {
        return Ok(None);
    }
    // Un parent nommé doit être visible, même si c'est déjà celui de la page.
    let named_parent = match &body.parent_id {
        Some(Some(raw)) => Some(seen_in(space, raw).ok_or_else(ApiError::invalid_parent)?),
        Some(None) | None => None,
    };
    let parent = match &body.parent_id {
        None => node.parent_id,
        Some(_) => named_parent.map(|(parent, _)| parent),
    };
    // La place parmi les enfants de ce parent, lue après les droits : un frère que la
    // personne ne voit pas est refusé comme un id inconnu.
    let place = || {
        let after = match &body.after_id {
            None => Some(Sibling::Last),
            Some(None) => None,
            Some(Some(raw)) => {
                let (sibling, _) = seen_in(space, raw).ok_or_else(ApiError::invalid_after)?;
                Some(Sibling::Node(sibling))
            }
        };
        Ok::<_, ApiError>(Place {
            parent_id: parent,
            after,
            from: Some(node.parent_id),
        })
    };
    if parent == node.parent_id {
        permit(access, Action::Reorder)?;
        return Ok(Some(Move {
            place: place()?,
            root_grant: None,
            changes_parent: false,
            access: *access,
        }));
    }
    permit(access, Action::Move)?;
    // Ce que la page devient pour la personne : ce que lui donnent les lignes posées sur la
    // page, et ce qu'elle hérite du nouveau parent.
    let (root_grant, access) = match named_parent {
        Some((parent, under)) => {
            permit(&under, Action::CreateChild)?;
            let role = at_least(space, Role::Guest)?;
            let own = state.store.node_grants(node.id).await?;
            let alone = AppState::own_access(&own, user, role);
            let access = NodeAccess {
                level: alone.map_or(under.level, |alone| alone.level.max(under.level)),
                section: under.section,
                in_trash: false,
                visible_parent: Some(parent),
            };
            (None, access)
        }
        // Pas de parent nommé, et un autre parent que le sien : la racine. Il faut ce qu'il
        // faut pour y créer une page, et la personne y aura l'accès complet (Ruling R82).
        None => {
            let role = at_least(space, Role::Member)?;
            let grant = Grant {
                subject: Subject::User(user.id),
                level: Level::Full,
            };
            let mut own = state.store.node_grants(node.id).await?;
            own.retain(|line| line.subject != grant.subject);
            own.push(grant);
            let access = AppState::own_access(&own, user, role).ok_or_else(|| {
                ApiError::internal(&"page à la racine sans accès pour qui la déplace")
            })?;
            (Some(grant), access)
        }
    };
    Ok(Some(Move {
        place: place()?,
        root_grant,
        changes_parent: true,
        access,
    }))
}

/// `POST /api/v1/pages/{id}/trash` : met la page à la corbeille, et ses sous-pages avec elle
/// (par héritage).
pub(crate) async fn trash(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<PageNode>, ApiError> {
    let id = page_id(path)?;
    let (_, access) = state.node_access(&user, id, Action::Trash).await?;
    let writer = state.clone();
    let by = user.id;
    let trashed = to_completion(async move {
        let trashed = writer.store.trash_node(id, by).await?;
        writer.subtree_rights_changed(trashed.space_id, id).await;
        Ok(trashed)
    })
    .await?;
    Ok(Json(page_node(&trashed, &access, trashed.trashed_at)))
}

/// `POST /api/v1/pages/{id}/restore` : sort la page de la corbeille, et ses sous-pages avec
/// elle ; `409 parent_trashed` si un de ses ancêtres y est (c'est lui qu'on restaure). Sans
/// effet sur une page qui n'y est pas.
pub(crate) async fn restore(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<PageNode>, ApiError> {
    let id = page_id(path)?;
    let (node, access) = state.node_access(&user, id, Action::Restore).await?;
    if !access.in_trash {
        return Ok(Json(page_node(&node, &access, None)));
    }
    let writer = state.clone();
    let restored = to_completion(async move {
        let restored = writer.store.restore_node(id).await?;
        writer.subtree_rights_changed(restored.space_id, id).await;
        Ok(restored)
    })
    .await?;
    // Restaurée : aucun de ses ancêtres n'est à la corbeille non plus.
    Ok(Json(page_node(&restored, &access, None)))
}

/// `DELETE /api/v1/pages/{id}` : supprime définitivement la page et tout son sous-arbre, avec
/// leurs documents, leurs accès et leurs invitations ; `409 not_trashed` si la page n'est pas à
/// la corbeille (elle-même, ou par un ancêtre).
pub(crate) async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let id = page_id(path)?;
    let (node, access) = state.node_access(&user, id, Action::Delete).await?;
    if !access.in_trash {
        return Err(ApiError::not_trashed());
    }
    let writer = state.clone();
    let deleted = to_completion(async move {
        let deleted = writer.store.delete_node(id).await?;
        // Après la suppression en base : qui revient ne trouve plus la page.
        writer.hub.forget(&deleted).await;
        Ok(deleted.len())
    })
    .await?;
    tracing::info!(
        node_id = %id,
        space_id = %node.space_id,
        user_id = %user.id,
        pages = deleted,
        "page supprimée définitivement, avec son sous-arbre"
    );
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/pages/{id}/mmd` : le MMD canonique de la page, sans ancres. Une page à la
/// corbeille se lit encore.
pub(crate) async fn get_mmd(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Response, ApiError> {
    let id = page_id(path)?;
    state.node_access(&user, id, Action::Read).await?;
    let blocks = state.hub.read_blocks(id).await?;
    let mmd = render(&blocks, RenderOpts::default()).markdown;
    Ok(([(header::CONTENT_TYPE, MARKDOWN)], mmd).into_response())
}

/// `PUT /api/v1/pages/{id}/mmd` : remplace le contenu de la page, au nom du compte de la
/// session. L'écriture est persistée, puis diffusée aux connexions ouvertes. Une prop hors
/// registre est refusée (`422 invalid_blocks`, Ruling R41).
pub(crate) async fn put_mmd(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Result<StatusCode, ApiError> {
    let id = page_id(path)?;
    state.node_access(&user, id, Action::Edit).await?;
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
    state.hub.write_blocks(id, blocks, Some(user.id)).await?;
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

/// La page que nomme `raw` dans le corps d'une requête, si la personne la voit dans cet
/// espace, avec ses accès dessus. `None` pour un id mal formé, inconnu, d'un autre espace ou
/// d'une page qu'elle ne voit pas : l'appelant répond la même erreur dans tous ces cas.
fn seen_in(space: &SpaceAccess, raw: &str) -> Option<(Uuid, NodeAccess)> {
    let id = Uuid::parse_str(raw).ok()?;
    Some((id, *space.node(id)?))
}

/// `422 invalid_json` : un corps lisible, mais qui ne dit pas ce qu'il faut.
fn unexpected_body(message: &str) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::InvalidJson,
        message,
    )
}

/// Erreur du store à la création (`page` : `None`) ou au déplacement d'une page. `NotFound`
/// y a plusieurs causes, que la route a toutes écartées avant d'écrire, sauf une : `afterId`
/// n'est pas un frère à l'endroit visé. Ce qui a pu disparaître entre ses contrôles et
/// l'écriture est donc relu ici (le compte, la page, l'espace, le parent) ; s'il ne manque
/// rien, c'est `afterId`.
async fn placement_error(
    state: &AppState,
    error: StoreError,
    user: Uuid,
    space_id: Uuid,
    page: Option<Uuid>,
    parent: Option<Uuid>,
) -> ApiError {
    if !matches!(error, StoreError::NotFound) {
        return error.into();
    }
    let gone = async {
        let store = &state.store;
        if store.get_user(user).await?.is_none() {
            return Ok(Some(ApiError::unauthenticated()));
        }
        if let Some(page) = page
            && store.get_node(page).await?.is_none()
        {
            return Ok(Some(ApiError::page_not_found()));
        }
        if store.space_rev(space_id).await?.is_none() {
            return Ok(Some(ApiError::space_not_found()));
        }
        if let Some(parent) = parent
            && store
                .get_node(parent)
                .await?
                .is_none_or(|parent| parent.space_id != space_id)
        {
            return Ok(Some(ApiError::invalid_parent()));
        }
        Ok::<_, StoreError>(None)
    };
    match gone.await {
        Ok(Some(error)) => error,
        Ok(None) => ApiError::invalid_after(),
        Err(error) => error.into(),
    }
}

/// Depuis quand la page est à la corbeille : sa propre date, sinon celle de son plus proche
/// ancêtre qui y est ; `None` hors corbeille.
async fn trashed_since(
    state: &AppState,
    node: &Node,
    access: &NodeAccess,
) -> Result<Option<i64>, ApiError> {
    if node.trashed_at.is_some() || !access.in_trash {
        return Ok(node.trashed_at);
    }
    // Par héritage : le store remonte de parent en parent, sans lire l'espace.
    Ok(state.store.trashed_since(node.id).await?)
}

/// La page telle que la voit la personne de `access` : son niveau, sa section, et son parent
/// seulement si elle le voit.
fn page_node(node: &Node, access: &NodeAccess, trashed_at: Option<i64>) -> PageNode {
    PageNode {
        id: node.id.to_string(),
        space_id: node.space_id.to_string(),
        parent_id: access.visible_parent.map(|p| p.to_string()),
        kind: match node.kind {
            NodeKind::Page => "page",
            NodeKind::Folder => "folder",
        }
        .to_string(),
        title: node.title.clone(),
        position: node.position.clone(),
        level: access.level.into(),
        section: access.section.into(),
        updated_at: node.updated_at,
        trashed_at,
    }
}
