//! `POST /api/v1/pages/{id}/duplicate` : dupliquer une page et ses sous-pages (Ruling R252).
//!
//! La copie ne partage rien avec l'original : des pages neuves, des documents neufs où **aucun
//! bloc ne garde son identifiant** (un bloc recollé d'une page à l'autre avec l'`id` de l'original
//! serait défait par le dédoublonnage), des attaches à elle pour les fichiers que ses blocs
//! montrent, dont les adresses sont récrites. Elle ne reprend ni les commentaires, ni les
//! versions, ni les favoris, ni les accès nommés de l'original. Ne sont copiées que les
//! sous-pages **que `access.rs` montre à la personne**, hors corbeille.
//!
//! | Cas | Où va la copie |
//! |---|---|
//! | elle voit la page parente et peut y créer (`CreateChild`) | sous la page parente, juste après la page, sans accès propre : elle en hérite |
//! | sinon, pour un membre de l'espace | à la racine de l'espace, en dernier, avec `(user:<elle>, full)` : dans son « Privé » |
//! | sinon (une invitée) | `403 forbidden` |
//!
//! **Refus**, dans l'ordre : `401` ; `404 not_found` pour une page qu'elle ne voit pas ; `403
//! forbidden` pour une invitée qui n'a nulle part où poser la copie ; `409 trashed` pour une page
//! à la corbeille (elle se lit, mais ne se copie pas) ; `422 invalid_request` pour un dossier ;
//! `429 duplication_busy`, avec `Retry-After`, si le compte duplique déjà ou a dépensé son budget
//! de la minute ; `422 subtree_too_large` au-delà de [`MAX_DUPLICATED_PAGES`] pages, ou de
//! [`MAX_DUPLICATED_BYTES`] octets de documents.
//!
//! **Débit** (`duplications.rs`, Rulings R605 à R607). Un compte ne mène qu'une duplication à la
//! fois, et ne bâtit pas plus de 64 Mio de documents par minute, écrits ou non : au-delà, `429`,
//! tout de suite, sans file d'attente. Une duplication ne tient donc jamais plus d'une place de
//! calcul, et toutes les copies s'écrivent dans une même voie de la file d'écriture : une frappe
//! n'attend jamais plus d'une copie, que la borne de poids tient courte.
//!
//! **Déroulé** (Ruling R89 : le contrôle qui compte vient après tout ce qui attend).
//! 1. Premier contrôle : les refus, et les pages à copier.
//! 2. Page par page : ses blocs, lus par son acteur, qui n'est occupé que le temps de cette
//!    lecture ; les attaches que ses blocs nomment ; puis son document neuf, bâti hors des fils du
//!    serveur, au tour du compte (`mmd_reads`, Ruling R436). Les blocs d'une page sont lâchés avant
//!    de lire la suivante : la requête ne garde que les documents bâtis, sous la borne de poids.
//! 3. Second contrôle, **juste avant d'écrire**, dans une tâche que l'abandon de la requête
//!    n'arrête pas : la session, la page (encore lue, hors corbeille), la destination, rejouée
//!    (c'est celle-ci qui est écrite), et chaque sous-page copiée (encore vue, hors corbeille,
//!    sous la même page ; sinon elle sort de la copie, avec ses propres sous-pages).
//! 4. L'écriture, en une transaction du store, qui relit elle-même la page et la destination au
//!    nom de la personne (Ruling R590) : si elles ont bougé pendant que l'écriture attendait son
//!    tour, rien n'est écrit, et la route reprend au second contrôle, [`DUPLICATE_RETRIES`] fois
//!    au plus, puis répond `409 page_moved`.
//!
//! Une page dupliquée est une page **neuve** : elle est écrite avec son nœud, comme `POST
//! /api/v1/pages`, sans passer par un acteur. Elle naît sans version : son contenu de départ
//! n'est dans aucune (l'original le porte).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mementum_acl::{Action, Grant, Level, NodeAccess, Role, SpaceAccess, Subject};
use mementum_crdt::{CrdtError, PageDoc};
use mementum_md::{Block, Content, Inline, PropValue};
use mementum_store::{CopiedAttachment, CopiedPage, NewCopy, Node, NodeKind, Sibling, StoreError};
use mementum_sync::SyncError;
use uuid::Uuid;

use crate::access::{permit, permit_level, permit_root_page};
use crate::auth::{AuthUser, LiveCaller};
use crate::duplications::Busy;
use crate::error::ApiError;
use crate::routes::pages::{MAX_TITLE_CHARS, page_id, page_node};
use crate::types::{ErrorCode, PageNode};
use crate::{AppState, now_ms, to_completion};

/// Pages d'une copie, racine comprise (Ruling R252) : au-delà, `422 subtree_too_large`, et l'on
/// duplique par morceaux.
pub const MAX_DUPLICATED_PAGES: usize = 100;

/// Octets au plus, tous documents confondus, d'une copie (Rulings R592, R608) : au-delà, `422
/// subtree_too_large`. La copie s'écrit en une transaction, et l'écriture de SQLite est unique
/// pour l'instance : pendant elle, chaque frappe de chaque page attend. Mesuré en release : 6 ms
/// par mébioctet sur le poste de mesure. À 16 Mio, la frappe qui tombe pendant la transaction
/// l'attend une centaine de millisecondes, sous son budget de 150 ms ; à 64 Mio, la première
/// borne, elle attendait 0,25 à 0,47 s, et quatre à dix fois plus sur un disque lent. C'est le
/// p95 de la frappe qui tient, pas son maximum : sous une rafale de copies de plusieurs comptes,
/// la frappe qui tombe pendant une copie de 15 Mio a attendu 160 à 199 ms (re-relecture).
pub const MAX_DUPLICATED_BYTES: usize = 16 << 20;

/// Ce qui suit le titre de la page dans celui de sa copie.
const COPY_SUFFIX: &str = " (copie)";

/// Tentatives d'écriture d'une copie dont la page ou la destination bouge (Ruling R590).
const DUPLICATE_RETRIES: usize = 3;

/// Les types de blocs qui portent l'adresse d'un fichier dans leur prop `url` (Ruling R248).
const FILE_BLOCKS: [&str; 4] = ["image", "video", "audio", "file"];

/// Où va la copie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Destination {
    /// Sous la page parente, juste après la page : la copie hérite de `access`, les accès de la
    /// personne sur la page parente.
    Beside { parent: Uuid, access: NodeAccess },
    /// À la racine de l'espace, en dernier, dans le « Privé » de la personne, de rôle `role`.
    Private { role: Role },
}

/// Ce que le contrôle a lu : la page, le calcul des accès de son espace, et la destination.
struct Checked {
    node: Node,
    space: Arc<SpaceAccess>,
    destination: Destination,
}

/// Une page à copier, telle que la personne la voit : `parent` est le rang de sa page parente
/// dans la liste (`None` pour la racine).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    id: Uuid,
    parent: Option<usize>,
    title: String,
    position: String,
}

/// Une page de la copie, bâtie : son original, son identifiant neuf, son document, ses attaches.
struct Built {
    source: Uuid,
    id: Uuid,
    doc: Vec<u8>,
    attachments: Vec<CopiedAttachment>,
}

/// `422 subtree_too_large` : trop de pages.
fn too_many_pages() -> ApiError {
    ApiError::subtree_too_large(format!(
        "cette page et ses sous-pages font plus de {MAX_DUPLICATED_PAGES} pages : duplique-les \
         par morceaux, une sous-page à la fois"
    ))
}

/// `422 subtree_too_large` : trop d'octets, au-delà de `max`.
fn too_heavy(max: usize) -> ApiError {
    let bound = match max >> 20 {
        0 => format!("{max} octets"),
        mib => format!("{mib} Mio"),
    };
    ApiError::subtree_too_large(format!(
        "le contenu de cette page et de ses sous-pages pèse plus de {bound} : duplique-les par \
         morceaux, une sous-page à la fois"
    ))
}

/// `429 duplication_busy` : pourquoi le compte ne duplique pas maintenant, et quand revenir.
fn busy(why: Busy) -> ApiError {
    match why {
        Busy::Running => ApiError::duplication_busy(
            "tu as déjà une duplication en cours : attends qu'elle finisse avant d'en demander \
             une autre ; rien n'a été copié",
            1,
        ),
        Busy::Spent { retry_after } => ApiError::duplication_busy(
            format!(
                "tu as dupliqué beaucoup de contenu en peu de temps : réessaie dans \
                 {retry_after} s ; rien n'a été copié"
            ),
            retry_after,
        ),
    }
}

/// Ajoute au poids `total` d'une copie les `doc` octets d'un document de plus ; refuse dès que
/// le poids dépasse `max` (un poids égal à la borne passe).
fn weigh(total: &mut usize, doc: usize, max: usize) -> Result<(), ApiError> {
    *total = total.saturating_add(doc);
    if *total > max {
        return Err(too_heavy(max));
    }
    Ok(())
}

impl AppState {
    /// Le contrôle d'une duplication, dans l'ordre des refus : `404` pour une page que `user`
    /// ne voit pas, `403` s'il n'a nulle part où poser la copie, `409 trashed` pour une page à
    /// la corbeille. Mené deux fois : avant de lire quoi que ce soit, puis juste avant d'écrire.
    async fn duplication(&self, user: &AuthUser, id: Uuid) -> Result<Checked, ApiError> {
        let (node, access, space) = self
            .seen_node(user, id)
            .await?
            .ok_or_else(ApiError::page_not_found)?;
        permit(&access, Action::Read)?;
        let destination = destination(&space, &access, user)?;
        if access.in_trash {
            return Err(ApiError::trashed());
        }
        // Aucune route ne crée encore de dossier ; le jour où il y en a, il ne se duplique pas
        // comme une page, dont il n'a pas le document.
        if node.kind != NodeKind::Page {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::InvalidRequest,
                "seule une page se duplique : ceci est un dossier",
            ));
        }
        Ok(Checked {
            node,
            space,
            destination,
        })
    }
}

/// Où va la copie d'une page d'accès `access` (voir le module). La corbeille n'y entre pas :
/// l'appelant la refuse ensuite, pour la page comme pour sa page parente.
fn destination(
    space: &SpaceAccess,
    access: &NodeAccess,
    user: &AuthUser,
) -> Result<Destination, ApiError> {
    if let Some(parent) = access.visible_parent
        && let Some(above) = space.node(parent)
        && permit_level(above, Action::CreateChild).is_ok()
    {
        return Ok(Destination::Beside {
            parent,
            access: *above,
        });
    }
    // À la racine : ce qu'il faut pour y créer une page, niveau d'un jeton compris (R1218).
    let role = permit_root_page(space, user)?;
    Ok(Destination::Private { role })
}

/// Les pages du sous-arbre de `root` que `space` montre, hors corbeille, la racine d'abord,
/// chaque page après sa page parente, les sœurs dans l'ordre de leurs positions. `nodes` est
/// tout l'espace, trié par `(parent, position)` (`Store::list_nodes`). On ne descend que par les
/// pages gardées : une page que la personne ne voit pas, ou à la corbeille, sort avec ses
/// sous-pages ; de même ce qui n'est pas une page (un dossier), qui n'a pas de document. S'arrête dès que la liste dépasse `limit` : l'appelant refuse.
fn seen_subtree(space: &SpaceAccess, nodes: &[Node], root: Uuid, limit: usize) -> Vec<Seen> {
    let mut children: HashMap<Uuid, Vec<&Node>> = HashMap::new();
    let mut top = None;
    for node in nodes {
        if node.id == root {
            top = Some(node);
        }
        if let Some(parent) = node.parent_id {
            children.entry(parent).or_default().push(node);
        }
    }
    let Some(top) = top else {
        return Vec::new();
    };
    let seen = |node: &Node, parent: Option<usize>| Seen {
        id: node.id,
        parent,
        title: node.title.clone(),
        position: node.position.clone(),
    };
    let mut out = vec![seen(top, None)];
    let mut at = 0;
    while at < out.len() && out.len() <= limit {
        for child in children.get(&out[at].id).into_iter().flatten() {
            // La racine ne revient pas : un nœud n'a qu'un parent, et une page prise dans un
            // cycle n'est vue par personne.
            if child.id != root
                && child.kind == NodeKind::Page
                && space.node(child.id).is_some_and(|access| !access.in_trash)
            {
                out.push(seen(child, Some(at)));
            }
        }
        at += 1;
    }
    out
}

/// Le titre de la copie : celui de la page, suivi de « (copie) », coupé pour tenir dans
/// [`MAX_TITLE_CHARS`].
fn copy_title(title: &str) -> String {
    let room = MAX_TITLE_CHARS - COPY_SUFFIX.chars().count();
    let kept: String = title.chars().take(room).collect();
    format!("{}{COPY_SUFFIX}", kept.trim_end())
}

/// La page et l'attache que nomme `url`, si c'est exactement l'adresse d'un fichier, sous la
/// forme que l'éditeur montre (Ruling R754) : `/api/v1/pages/<page>/files/<attache>`, sans rien
/// avant ni après, chaque identifiant à tirets, en chiffres hexadécimaux sans casse. C'est la
/// règle de `filePath` du client (`apps/web/src/editor/files.ts`) : les deux lisent la table de
/// cas `spec/files/addresses.json`. Les routes lisent plus large (`Uuid::parse_str` : sans
/// tirets, entre accolades, `urn:uuid:`) ; une adresse sous une de ces formes, que l'éditeur ne
/// montre pas, n'est pas celle d'un fichier pour la copie.
pub(crate) fn file_address(url: &str) -> Option<(Uuid, Uuid)> {
    let (page, file) = url.strip_prefix("/api/v1/pages/")?.split_once("/files/")?;
    Some((hyphenated(page)?, hyphenated(file)?))
}

/// L'identifiant `id`, s'il est écrit à tirets (8-4-4-4-12 chiffres hexadécimaux, sans casse) :
/// de 36 octets, `Uuid::parse_str` ne lit que cette forme ; la table de cas le vérifie.
fn hyphenated(id: &str) -> Option<Uuid> {
    (id.len() == 36).then(|| Uuid::parse_str(id).ok())?
}

/// L'attache que nomme `url`, si c'est exactement l'adresse d'un fichier de la page `page`
/// ([`file_address`]).
fn file_of(url: &str, page: Uuid) -> Option<Uuid> {
    let (of, file) = file_address(url)?;
    (of == page).then_some(file)
}

/// Les adresses des liens de ce contenu en ligne.
fn link_addresses(inlines: &mut [Inline], visit: &mut dyn FnMut(&mut String)) {
    for inline in inlines {
        if let Inline::Link { href, children } = inline {
            visit(href);
            link_addresses(children, visit);
        }
    }
}

/// Passe `visit` sur chaque adresse que portent ces blocs, à toute profondeur, dans l'ordre du
/// document : la prop `url` d'un bloc de fichier ([`FILE_BLOCKS`]), et l'adresse de chaque lien
/// d'un texte ou d'une cellule de tableau (Ruling R609). Rien d'autre : ni la prop `url` d'un
/// signet, ni une mention.
fn each_address(blocks: &mut [Block], visit: &mut dyn FnMut(&mut String)) {
    let mut stack = vec![blocks];
    while let Some(level) = stack.pop() {
        for block in level.iter_mut() {
            if FILE_BLOCKS.contains(&block.kind.as_str())
                && let Some(PropValue::Str(url)) = block.props.get_mut("url")
            {
                visit(url);
            }
            match &mut block.content {
                Content::Inline(inlines) => link_addresses(inlines, visit),
                Content::Table(table) => {
                    for cell in table.rows.iter_mut().flatten() {
                        link_addresses(&mut cell.content, visit);
                    }
                }
                Content::None => {}
            }
            stack.push(&mut block.children);
        }
    }
}

/// Les attaches de la page `page` que nomment ses blocs, chacune une fois.
fn named_files(blocks: &mut [Block], page: Uuid) -> Vec<Uuid> {
    let mut found = Vec::new();
    let mut known = HashSet::new();
    each_address(blocks, &mut |address| {
        if let Some(file) = file_of(address, page)
            && known.insert(file)
        {
            found.push(file);
        }
    });
    found
}

/// Le bloc est-il un bloc de fichier ([`FILE_BLOCKS`]) sans adresse ? Il ne nomme alors aucun
/// fichier : c'est celui qu'un envoi en route a posé (Ruling R685).
fn file_block_without_address(block: &Block) -> bool {
    FILE_BLOCKS.contains(&block.kind.as_str())
        && !matches!(block.props.get("url"), Some(PropValue::Str(url)) if !url.is_empty())
}

/// Le document neuf de la copie de la page `from`, d'identifiant `to` : les blocs de l'original,
/// **tous sans identifiant** (l'écriture en tire de neufs, à chaque profondeur), les adresses des
/// fichiers de `from` que `files` recopie récrites vers leur attache neuve, sur un bloc de fichier
/// comme dans un lien. Toute autre adresse reste telle quelle : externe, d'une autre page, ou
/// d'un fichier que la page n'a pas. Un bloc de fichier sans adresse perd sa prop `name` (Ruling
/// R753) : pendant un envoi, le bloc de l'original porte le nom du fichier sur le poste, que
/// l'envoi remplace, ou retire s'il échoue, dans l'original seul.
fn copied_doc(
    mut blocks: Vec<Block>,
    from: Uuid,
    to: Uuid,
    files: &HashMap<Uuid, Uuid>,
) -> Result<Vec<u8>, CrdtError> {
    let mut stack = vec![blocks.as_mut_slice()];
    while let Some(level) = stack.pop() {
        for block in level.iter_mut() {
            block.id.clear();
            if file_block_without_address(block) {
                block.props.remove("name");
            }
            stack.push(&mut block.children);
        }
    }
    each_address(&mut blocks, &mut |address| {
        if let Some(copy) = file_of(address, from).and_then(|file| files.get(&file)) {
            *address = format!("/api/v1/pages/{to}/files/{copy}");
        }
    });
    let page = PageDoc::new_page().0;
    page.write_blocks(&blocks)?;
    let state = page.encode_state();
    // Ruling R833 : le document d'une copie est la seule copie de ce qu'elle porte ; il est relu
    // comme le chargement le relira, avant d'être écrit (la règle de R816). Ici, hors des fils du
    // serveur (`mmd_reads.run`), au prix d'un chargement de plus par page copiée.
    PageDoc::check_state(&state)?;
    Ok(state)
}

/// `POST /api/v1/pages/{id}/duplicate` : `201`, la racine de la copie, telle que la personne la
/// voit (voir le module).
pub(crate) async fn duplicate(
    State(state): State<AppState>,
    caller: LiveCaller,
    path: Result<Path<String>, PathRejection>,
) -> Result<(StatusCode, Json<PageNode>), ApiError> {
    let LiveCaller { user, proof } = caller;
    let id = page_id(path)?;
    // Premier contrôle : les refus, avant de lire quoi que ce soit d'une page.
    let first = state.duplication(&user, id).await?;
    // Rulings R605, R606 : une duplication à la fois par compte, sous son budget de la minute ;
    // sinon refusée tout de suite. La place est tenue jusqu'à la fin de l'écriture, et ce qui
    // est bâti d'ici là est compté au budget, écrit ou non.
    // La garde est partagée avec chaque calcul (constat N1) : une requête abandonnée pendant une
    // construction garde sa place, et compte le document, jusqu'à la fin de ce calcul.
    let running = Arc::new(state.duplications.begin(user.party()).map_err(busy)?);
    let space_id = first.node.space_id;
    let nodes = state.store.list_nodes(space_id).await?;
    let sources = seen_subtree(&first.space, &nodes, id, MAX_DUPLICATED_PAGES);
    drop(nodes);
    if sources.len() > MAX_DUPLICATED_PAGES {
        return Err(too_many_pages());
    }

    // Page par page : lire, puis bâtir, sans garder les blocs d'une page pendant la suivante.
    let mut built = Vec::with_capacity(sources.len());
    let mut weight = 0;
    for source in &sources {
        let mut blocks = match state.hub.read_blocks(source.id).await {
            Ok(blocks) => blocks,
            // Une sous-page supprimée depuis le contrôle : le second contrôle la sort de la copie.
            Err(SyncError::NotFound) if source.id != id => continue,
            Err(error) => return Err(error.into()),
        };
        let mut attachments = Vec::new();
        let mut files = HashMap::new();
        for file in named_files(&mut blocks, source.id) {
            // Un fichier que la page n'a pas : son adresse reste telle quelle.
            if let Some(attachment) = state.store.attachment(source.id, file).await? {
                let copy = Uuid::now_v7();
                files.insert(file, copy);
                attachments.push(CopiedAttachment {
                    id: copy,
                    hash: attachment.hash,
                    name: attachment.name,
                });
            }
        }
        let (from, to) = (source.id, Uuid::now_v7());
        let doc = state
            .mmd_reads
            .run(user.id, {
                let running = Arc::clone(&running);
                move || {
                    let doc = copied_doc(blocks, from, to, &files);
                    if let Ok(doc) = &doc {
                        running.built(doc.len());
                    }
                    doc
                }
            })
            .await?
            .map_err(SyncError::Crdt)?;
        // Ruling R592 : refusée dès que le poids passe la borne, sans lire les pages suivantes.
        weigh(&mut weight, doc.len(), state.max_duplicated_bytes)?;
        built.push(Built {
            source: from,
            id: to,
            doc,
            attachments,
        });
    }

    let user_id = user.id;
    let (root, access, pages) = to_completion(async move {
        // La place du compte est rendue quand cette tâche finit, client parti ou non.
        let _running = running;
        let mut built = built;
        for _ in 0..DUPLICATE_RETRIES {
            // Second contrôle, juste avant d'écrire : la session (ou le jeton), la page, la
            // destination.
            state.caller_still_live(proof).await?;
            let now = state.duplication(&user, id).await?;
            let (parent_id, after, grants, access) = match now.destination {
                Destination::Beside { parent, access } => {
                    // Sans ligne propre, la copie a les accès de sa page parente.
                    let inherited = NodeAccess {
                        visible_parent: Some(parent),
                        ..access
                    };
                    let after = Some(Sibling::Node(id));
                    (Some(parent), after, Vec::new(), inherited)
                }
                Destination::Private { role } => {
                    let grant = Grant {
                        subject: Subject::User(user.id),
                        level: Level::Full,
                    };
                    let access = AppState::own_access(&[grant], &user, role).ok_or_else(|| {
                        ApiError::internal(&"copie privée sans accès pour son auteur")
                    })?;
                    (None, Some(Sibling::Last), vec![grant], access)
                }
            };
            // Les sous-pages : celles qui sont encore vues, hors corbeille, sous la même page.
            let nodes = state.store.list_nodes(space_id).await?;
            let still = seen_subtree(&now.space, &nodes, id, usize::MAX);
            drop(nodes);
            let pages = copied_pages(&mut built, &still);
            if pages.is_empty() {
                // La page a disparu entre le contrôle et la lecture de l'arbre : le contrôle
                // suivant le dira.
                continue;
            }
            let copy = NewCopy {
                space_id,
                source: id,
                parent_id,
                after,
                grants,
                created_by: user.id,
                pages,
            };
            match state.store.duplicate_pages(&copy, now_ms()).await {
                Ok(root) => return Ok((root, access, copy.pages.len())),
                // La page ou la destination a bougé pendant l'attente du tour d'écriture : rien
                // n'est écrit, on relit et on redécide.
                Err(StoreError::Stale | StoreError::NotFound) => {
                    built = unbuilt(copy.pages, built);
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(ApiError::page_moved())
    })
    .await?;
    tracing::info!(
        node_id = %id,
        copy_id = %root.id,
        space_id = %space_id,
        user_id = %user_id,
        pages,
        "page dupliquée, avec ses sous-pages"
    );
    Ok((StatusCode::CREATED, Json(page_node(&root, &access, None))))
}

/// Les pages de la copie, d'après ce que le second contrôle voit encore (`still`, la racine en
/// tête) : une page bâtie n'est gardée que si son original est encore là, sous la même page
/// parente, elle-même gardée. Les titres et les positions sont ceux de cette seconde lecture.
/// Les documents quittent `built` pour la copie ([`unbuilt`] les y remet).
fn copied_pages(built: &mut [Built], still: &[Seen]) -> Vec<CopiedPage> {
    let by_source: HashMap<Uuid, usize> = built
        .iter()
        .enumerate()
        .map(|(i, page)| (page.source, i))
        .collect();
    // Le rang, dans la copie, de chaque page de `still` qui y entre.
    let mut rank: Vec<Option<usize>> = Vec::with_capacity(still.len());
    let mut pages = Vec::new();
    for (i, seen) in still.iter().enumerate() {
        let parent = match seen.parent {
            None => Some(None),
            Some(above) => rank[above].map(Some),
        };
        let kept = parent.zip(by_source.get(&seen.id));
        rank.push(kept.map(|_| pages.len()));
        let Some((parent, &at)) = kept else {
            continue;
        };
        let page = &mut built[at];
        pages.push(CopiedPage {
            id: page.id,
            parent,
            title: match i {
                0 => copy_title(&seen.title),
                _ => seen.title.clone(),
            },
            position: seen.parent.map(|_| seen.position.clone()),
            doc: std::mem::take(&mut page.doc),
            attachments: std::mem::take(&mut page.attachments),
        });
    }
    pages
}

/// Rend à `built` les documents et les attaches qu'une copie refusée avait pris.
fn unbuilt(pages: Vec<CopiedPage>, mut built: Vec<Built>) -> Vec<Built> {
    let mut taken: HashMap<Uuid, CopiedPage> = pages.into_iter().map(|p| (p.id, p)).collect();
    for page in &mut built {
        if let Some(copy) = taken.remove(&page.id) {
            page.doc = copy.doc;
            page.attachments = copy.attachments;
        }
    }
    built
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use axum::Router;
    use axum::body::{Body, Bytes};
    use axum::http::{Request, header};
    use mementum_store::{NewNode, NodeKind};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;
    use crate::access::tests::state;
    use crate::inputs_cache::tests::wait_until;
    use crate::mmd_reads::MAX_MMD_READS;

    /// Un serveur sur un état neuf, monté par le vrai routeur : l'espace d'Olga, propriétaire,
    /// où Mona est membre et Gina invitée.
    struct Bench {
        state: AppState,
        router: Router,
        space: Uuid,
        olga: (Uuid, String),
        mona: (Uuid, String),
        gina: (Uuid, String),
        _dir: tempfile::TempDir,
    }

    impl Bench {
        async fn new() -> Bench {
            Bench::with(|_| {}).await
        }

        /// Comme [`Bench::new`], sur un état retouché par `adjust` avant que le routeur soit
        /// monté.
        async fn with(adjust: impl FnOnce(&mut AppState)) -> Bench {
            let dir = tempfile::tempdir().unwrap();
            let mut state = state(&dir).await;
            adjust(&mut state);
            let router = crate::router(state.clone());
            let mut bench = Bench {
                state,
                router,
                space: Uuid::nil(),
                olga: (Uuid::nil(), String::new()),
                mona: (Uuid::nil(), String::new()),
                gina: (Uuid::nil(), String::new()),
                _dir: dir,
            };
            let (olga, space) = bench.sign_up("Olga").await;
            let (mona, _) = bench.sign_up("Mona").await;
            let (gina, _) = bench.sign_up("Gina").await;
            let store = &bench.state.store;
            store.add_member(space, mona.0, Role::Member).await.unwrap();
            store.add_member(space, gina.0, Role::Guest).await.unwrap();
            (bench.space, bench.olga, bench.mona, bench.gina) = (space, olga, mona, gina);
            bench
        }

        /// Inscrit un compte : son identifiant et son cookie de session, et son espace.
        async fn sign_up(&self, name: &str) -> ((Uuid, String), Uuid) {
            let body = json!({
                "email": format!("{}@exemple.fr", name.to_lowercase()),
                "name": name,
                "password": "mot-de-passe-de-test",
            });
            let request = Request::post("/api/v1/auth/signup")
                .header(header::HOST, "localhost")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            let response = self.router.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
            let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
            let cookie = cookie.split(';').next().unwrap().to_string();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let me: Value = serde_json::from_slice(&body).unwrap();
            let id = |value: &Value| Uuid::parse_str(value.as_str().unwrap()).unwrap();
            ((id(&me["user"]["id"]), cookie), id(&me["spaces"][0]["id"]))
        }

        /// Une page de l'espace d'Olga, créée par elle, avec ces accès.
        async fn page(&self, parent: Option<Uuid>, title: &str, grants: &[Grant]) -> Uuid {
            self.state
                .store
                .create_node(NewNode {
                    space_id: self.space,
                    parent_id: parent,
                    after: None,
                    kind: NodeKind::Page,
                    title: title.to_string(),
                    created_by: Some(self.olga.0),
                    grants: grants.to_vec(),
                    doc: Some(PageDoc::new_page().0.encode_state()),
                })
                .await
                .unwrap()
                .id
        }

        /// La duplication de `page` par le compte de ce cookie, menée dans une tâche à elle.
        fn duplicate(
            &self,
            cookie: &str,
            page: Uuid,
        ) -> tokio::task::JoinHandle<(StatusCode, Bytes)> {
            let request = Request::post(format!("/api/v1/pages/{page}/duplicate"))
                .header(header::HOST, "localhost")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap();
            let router = self.router.clone();
            tokio::spawn(async move {
                let response = router.oneshot(request).await.unwrap();
                let status = response.status();
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                (status, body)
            })
        }

        /// Prend toutes les places de la file des calculs, au nom d'Olga : la construction d'un
        /// document par un autre compte attend, jusqu'à ce que la garde rendue soit lâchée.
        async fn hold_the_builds(&self) -> Vec<mpsc::Sender<()>> {
            let mut held = Vec::new();
            for _ in 0..MAX_MMD_READS {
                let (release, wait) = mpsc::channel::<()>();
                let reads = self.state.mmd_reads.clone();
                let olga = self.olga.0;
                tokio::spawn(async move {
                    // Rendue quand l'émetteur est lâché.
                    let _ = reads.run(olga, move || wait.recv()).await;
                });
                held.push(release);
            }
            let reads = self.state.mmd_reads.clone();
            wait_until("les places de calcul sont prises", || {
                reads.counts() == (MAX_MMD_READS, 0)
            })
            .await;
            held
        }

        /// Attend que `n` calculs attendent leur place : autant de duplications ont fini de lire
        /// une page et attendent de bâtir son document.
        async fn builds_waiting(&self, n: usize) {
            let reads = self.state.mmd_reads.clone();
            wait_until("la duplication attend de bâtir", || reads.counts().1 == n).await;
        }

        async fn nodes(&self) -> usize {
            self.state.store.list_nodes(self.space).await.unwrap().len()
        }

        /// Ferme la base avant de lâcher le dossier (Ruling R344).
        async fn finish(self) {
            self.state.store.close().await;
        }
    }

    fn members(level: Level) -> Grant {
        Grant {
            subject: Subject::Members,
            level,
        }
    }

    fn named(user: Uuid, level: Level) -> Grant {
        Grant {
            subject: Subject::User(user),
            level,
        }
    }

    fn error_of(body: &Bytes) -> String {
        let body: Value = serde_json::from_slice(body).unwrap();
        body["error"].as_str().unwrap_or_default().to_string()
    }

    /// Ruling R89 : les droits sont contrôlés de nouveau juste avant d'écrire, une fois les pages
    /// lues **et leurs documents bâtis**. Le test prend toutes les places de la file des calculs :
    /// la duplication lit la page, puis attend de bâtir son document ; le droit change alors, et
    /// les places sont rendues. Accès retiré : `404`, aucune page créée ; page mise à la
    /// corbeille : `409 trashed` ; session fermée : `401` ; droit de créer sous la page parente
    /// retiré, pour un membre : la copie va dans son « Privé » ; pour une invitée : `403`.
    /// Témoin : sans changement, `201`, sous la page parente.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn duplicate_checks_rights_again_just_before_writing() {
        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Change {
            Nothing,
            AccessRemoved,
            Trashed,
            ParentTrashed,
            SessionClosed,
            NoMoreCreation,
        }
        use Change::*;
        let bench = Bench::new().await;
        let store = bench.state.store.clone();
        let cases = [
            (Nothing, false, StatusCode::CREATED, ""),
            (AccessRemoved, false, StatusCode::NOT_FOUND, "not_found"),
            (Trashed, false, StatusCode::CONFLICT, "trashed"),
            (ParentTrashed, false, StatusCode::CONFLICT, "trashed"),
            (NoMoreCreation, false, StatusCode::CREATED, ""),
            (Nothing, true, StatusCode::CREATED, ""),
            (NoMoreCreation, true, StatusCode::FORBIDDEN, "forbidden"),
            // En dernier : la session de Mona n'en revient pas.
            (
                SessionClosed,
                false,
                StatusCode::UNAUTHORIZED,
                "unauthenticated",
            ),
        ];
        for (change, guest, status, error) in cases {
            let what = format!("{change:?}, invitée : {guest}");
            let (who, cookie) = if guest { &bench.gina } else { &bench.mona };
            let grants = [members(Level::Edit), named(bench.gina.0, Level::Edit)];
            let parent = bench.page(None, "Projet", &grants).await;
            let page = bench.page(Some(parent), "Notes", &[]).await;
            let before = bench.nodes().await;

            let held = bench.hold_the_builds().await;
            let reply = bench.duplicate(cookie, page);
            // La page est lue : la duplication attend de bâtir son document.
            bench.builds_waiting(1).await;
            match change {
                Nothing => {}
                AccessRemoved => {
                    store.remove_grant(parent, Subject::Members).await.unwrap();
                }
                Trashed => {
                    store.trash_node(page, bench.olga.0).await.unwrap();
                }
                ParentTrashed => {
                    store.trash_node(parent, bench.olga.0).await.unwrap();
                }
                SessionClosed => {
                    let sessions = "DELETE FROM sessions WHERE user_id = ?";
                    let url = format!("sqlite:{}", bench._dir.path().join("m.db").display());
                    let mut side = <sqlx::SqliteConnection as sqlx::Connection>::connect(&url)
                        .await
                        .unwrap();
                    sqlx::query(sessions)
                        .bind(who.to_string())
                        .execute(&mut side)
                        .await
                        .unwrap();
                    sqlx::Connection::close(side).await.unwrap();
                }
                NoMoreCreation => {
                    for grant in [members(Level::Read), named(bench.gina.0, Level::Read)] {
                        store.set_grant(parent, grant).await.unwrap();
                    }
                }
            }
            drop(held);
            let (got, body) = reply.await.unwrap();
            assert_eq!(got, status, "{what} : {}", String::from_utf8_lossy(&body));
            if status != StatusCode::CREATED {
                assert_eq!(error_of(&body), error, "{what}");
                assert_eq!(bench.nodes().await, before, "{what} : aucune page créée");
                continue;
            }
            assert_eq!(bench.nodes().await, before + 1, "{what}");
            let copy: Value = serde_json::from_slice(&body).unwrap();
            let id = Uuid::parse_str(copy["id"].as_str().unwrap()).unwrap();
            let stored = store.get_node(id).await.unwrap().unwrap();
            let own = store.node_grants(id).await.unwrap();
            if change == NoMoreCreation {
                assert_eq!(stored.parent_id, None, "{what} : à la racine");
                assert_eq!(own, [named(*who, Level::Full)], "{what} : privée");
                assert_eq!(copy["parentId"], Value::Null, "{what}");
                assert_eq!(copy["section"], "private", "{what}");
            } else {
                assert_eq!(
                    stored.parent_id,
                    Some(parent),
                    "{what} : sous la page parente"
                );
                assert!(own.is_empty(), "{what}");
                assert_eq!(copy["parentId"], parent.to_string(), "{what}");
            }
        }
        bench.finish().await;
    }

    /// Une sous-page mise à la corbeille, ou sortie du sous-arbre, pendant la construction des
    /// documents n'est pas dans la copie, ni ses propres sous-pages ; une sous-page restée en
    /// place y est, avec le titre qu'elle a au moment d'écrire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_subpage_gone_while_the_documents_are_built_stays_out_of_the_copy() {
        let bench = Bench::new().await;
        let store = bench.state.store.clone();
        let root = bench.page(None, "Projet", &[members(Level::Full)]).await;
        let elsewhere = bench.page(None, "Ailleurs", &[members(Level::Full)]).await;
        let kept = bench.page(Some(root), "Gardée", &[]).await;
        let trashed = bench.page(Some(root), "Jetée", &[]).await;
        let under_trashed = bench.page(Some(trashed), "Sous la jetée", &[]).await;
        let moved = bench.page(Some(root), "Déplacée", &[]).await;
        let before = bench.nodes().await;

        let held = bench.hold_the_builds().await;
        let reply = bench.duplicate(&bench.mona.1, root);
        bench.builds_waiting(1).await;
        store.trash_node(trashed, bench.olga.0).await.unwrap();
        store
            .move_node(moved, Some(elsewhere), None, None)
            .await
            .unwrap();
        store.rename_node(kept, "Renommée", None).await.unwrap();
        drop(held);
        let (status, body) = reply.await.unwrap();
        assert_eq!(
            status,
            StatusCode::CREATED,
            "{}",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(
            bench.nodes().await,
            before + 2,
            "la racine et la page gardée"
        );
        let copy: Value = serde_json::from_slice(&body).unwrap();
        let copy = Uuid::parse_str(copy["id"].as_str().unwrap()).unwrap();
        let all = store.list_nodes(bench.space).await.unwrap();
        let under: Vec<&str> = all
            .iter()
            .filter(|node| node.parent_id == Some(copy))
            .map(|node| node.title.as_str())
            .collect();
        assert_eq!(under, ["Renommée"]);
        let _ = under_trashed;
        bench.finish().await;
    }

    fn node(id: u128, parent: Option<u128>, position: &str) -> Node {
        Node {
            id: Uuid::from_u128(id),
            space_id: Uuid::nil(),
            parent_id: parent.map(Uuid::from_u128),
            kind: NodeKind::Page,
            title: format!("Page {id}"),
            position: position.to_string(),
            created_by: None,
            created_at: 0,
            updated_by: None,
            updated_at: 0,
            trashed_at: None,
        }
    }

    /// Les accès d'un propriétaire sur ces nœuds, tous ouverts aux membres par leur racine.
    fn all_seen(nodes: &[Node], trashed: &[u128]) -> SpaceAccess {
        let tree: Vec<_> = nodes
            .iter()
            .map(|node| mementum_acl::TreeNode {
                id: node.id,
                parent_id: node.parent_id,
                trashed: trashed.contains(&node.id.as_u128()),
            })
            .collect();
        let lines: Vec<_> = nodes
            .iter()
            .filter(|node| node.parent_id.is_none())
            .map(|node| (node.id, members(Level::Full)))
            .collect();
        SpaceAccess::compute(&tree, &lines, Uuid::from_u128(99), Some(Role::Owner))
    }

    /// Le sous-arbre vu : la racine d'abord, chaque page après sa page parente, les sœurs dans
    /// l'ordre des positions ; rien de ce qui est à la corbeille, ni d'un autre sous-arbre ; et la
    /// liste s'arrête dès qu'elle dépasse la borne.
    #[test]
    fn seen_subtree_lists_parents_first_and_stops_past_the_limit() {
        // `list_nodes` trie par (parent, position).
        let nodes = [
            node(1, None, "a0"),
            node(9, None, "a1"),
            node(2, Some(1), "a0"),
            node(3, Some(1), "a1"),
            node(4, Some(1), "a2"),
            node(5, Some(2), "a0"),
            node(6, Some(3), "a0"),
            node(7, Some(4), "a0"),
            node(8, Some(9), "a0"),
        ];
        let space = all_seen(&nodes, &[4]);
        let ids = |seen: &[Seen]| -> Vec<(u128, Option<usize>)> {
            seen.iter().map(|s| (s.id.as_u128(), s.parent)).collect()
        };
        let whole = seen_subtree(&space, &nodes, Uuid::from_u128(1), usize::MAX);
        assert_eq!(
            ids(&whole),
            [
                (1, None),
                (2, Some(0)),
                (3, Some(0)),
                (5, Some(1)),
                (6, Some(2))
            ]
        );
        assert_eq!(whole[1].position, "a0");
        assert_eq!(whole[1].title, "Page 2");
        let part = seen_subtree(&space, &nodes, Uuid::from_u128(3), usize::MAX);
        assert_eq!(ids(&part), [(3, None), (6, Some(0))]);
        assert!(seen_subtree(&space, &nodes, Uuid::from_u128(77), usize::MAX).is_empty());
        // La borne : la liste la dépasse, sans aller au bout.
        let cut = seen_subtree(&space, &nodes, Uuid::from_u128(1), 2);
        assert_eq!(cut.len(), 3);
    }

    /// Une adresse n'est celle d'un fichier de la page que si elle l'est exactement.
    #[test]
    fn only_the_exact_address_of_a_file_of_the_page_is_one() {
        let (page, file) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let exact = format!("/api/v1/pages/{page}/files/{file}");
        assert_eq!(file_of(&exact, page), Some(file));
        for other in [
            format!("/api/v1/pages/{}/files/{file}", Uuid::from_u128(3)),
            format!("https://exemple.fr/api/v1/pages/{page}/files/{file}"),
            format!("{exact}?x=1"),
            format!("{exact}/"),
            format!(" {exact}"),
            format!("/api/v1/pages/{page}/files/"),
            format!("/api/v1/pages/{page}/versions/{file}"),
            String::new(),
        ] {
            assert_eq!(file_of(&other, page), None, "{other}");
        }
    }

    /// Ruling R754 : la règle de l'adresse d'un fichier est celle de l'éditeur, cas par cas.
    /// `file_address` et `filePath` (`apps/web/src/editor/files.ts`) lisent la même table de cas,
    /// `spec/files/addresses.json` ; le test du client est `fileAddresses.test.ts`.
    #[test]
    fn file_address_follows_the_shared_cases() {
        let table: Value =
            serde_json::from_str(include_str!("../../../../spec/files/addresses.json")).unwrap();
        let cases = table["cases"].as_array().unwrap();
        assert!(cases.len() > 30, "la table est lue");
        for case in cases {
            let why = case["why"].as_str().unwrap();
            let address = case["address"].as_str().unwrap();
            let read = file_address(address).map_or(
                Value::Null,
                |(page, file)| json!({ "page": page.to_string(), "file": file.to_string() }),
            );
            assert_eq!(read, case["names"], "{why} : {address:?}");
        }
    }

    /// Ruling R753 : un bloc de fichier (image, vidéo, son, fichier) sans adresse en est un dont
    /// la prop `url` manque, est vide (un bloc pas encore normalisé) ou n'est pas du texte ; une
    /// adresse quelconque, même une espace, en est une. Un autre bloc ne l'est jamais, même sans
    /// `url` (un signet vide, un bloc étranger qui porterait un `name`).
    #[test]
    fn only_a_file_block_without_address_loses_its_name() {
        let block = |kind: &str, url: Option<PropValue>| Block {
            id: String::new(),
            kind: kind.to_string(),
            props: url
                .map(|url| ("url".to_string(), url))
                .into_iter()
                .collect(),
            content: Content::None,
            children: Vec::new(),
        };
        let text = |url: &str| Some(PropValue::Str(url.to_string()));
        for kind in FILE_BLOCKS {
            for url in [None, text(""), Some(PropValue::Int(1))] {
                assert!(
                    file_block_without_address(&block(kind, url.clone())),
                    "{kind} {url:?}"
                );
            }
            for url in [
                text(" "),
                text("https://exemple.fr/a.png"),
                text("photo.png"),
            ] {
                assert!(
                    !file_block_without_address(&block(kind, url.clone())),
                    "{kind} {url:?}"
                );
            }
        }
        for kind in ["bookmark", "paragraph", "unknown", "pageLink"] {
            assert!(!file_block_without_address(&block(kind, None)), "{kind}");
        }
    }

    /// La borne de poids, à la borne : un poids égal passe, un octet de plus est refusé (`422
    /// subtree_too_large`, qui dit la borne et quoi faire) ; le poids s'ajoute d'un document à
    /// l'autre, sans déborder.
    #[tokio::test]
    async fn weight_is_refused_one_byte_past_the_bound() {
        let mut total = 0;
        assert!(weigh(&mut total, 60, 100).is_ok());
        assert!(weigh(&mut total, 40, 100).is_ok());
        assert_eq!(total, 100);
        let refused = weigh(&mut total, 1, 100).expect_err("un octet de trop");
        let mut huge = usize::MAX;
        assert!(weigh(&mut huge, 5, 100).is_err());
        let response = axum::response::IntoResponse::into_response(refused);
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(error_of(&body), "subtree_too_large");
        let body: Value = serde_json::from_slice(&body).unwrap();
        let message = body["message"].as_str().unwrap();
        assert!(message.contains("plus de 100 octets"), "{message}");
        assert!(message.contains("duplique-les par morceaux"), "{message}");
        // La borne livrée se dit en mébioctets.
        let response = axum::response::IntoResponse::into_response(too_heavy(MAX_DUPLICATED_BYTES));
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let message = body["message"].as_str().unwrap();
        assert!(message.contains("plus de 16 Mio"), "{message}");
    }

    /// Le titre d'une copie : suivi de « (copie) », coupé au caractère, jamais au milieu d'un
    /// caractère de plusieurs octets, sans espace laissée avant la parenthèse.
    #[test]
    fn copy_title_fits_the_title_bound() {
        assert_eq!(copy_title("Notes"), "Notes (copie)");
        let long = "é".repeat(MAX_TITLE_CHARS);
        let cut = copy_title(&long);
        assert_eq!(cut.chars().count(), MAX_TITLE_CHARS);
        assert!(cut.ends_with("é (copie)"));
        let spaced = format!("{}{}", "a".repeat(MAX_TITLE_CHARS - 10), " ".repeat(10));
        let cut = copy_title(&spaced);
        assert!(cut.ends_with("a (copie)"), "{cut}");
        assert!(cut.chars().count() <= MAX_TITLE_CHARS);
    }

    impl Bench {
        /// Une requête du compte de ce cookie : son statut, ses en-têtes et son corps.
        async fn send(
            &self,
            cookie: &str,
            request: axum::http::request::Builder,
            body: Vec<u8>,
        ) -> (StatusCode, axum::http::HeaderMap, Bytes) {
            let request = request
                .header(header::HOST, "localhost")
                .header(header::COOKIE, cookie)
                .body(Body::from(body))
                .unwrap();
            // Bornée : une demande qui attendrait à tort (une place de calcul, un tour d'écriture
            // que le test tient) fait échouer le test, au lieu de le pendre.
            let answer = self.router.clone().oneshot(request);
            let response = tokio::time::timeout(std::time::Duration::from_secs(10), answer)
                .await
                .expect("une réponse en moins de dix secondes")
                .unwrap();
            let (status, headers) = (response.status(), response.headers().clone());
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (status, headers, body)
        }

        /// Une page sous `parent`, avec une sous-page, qui montre un fichier à elle par une image
        /// et par un lien : `(page, sous-page, octets du fichier)`.
        async fn illustrated(&self, parent: Uuid) -> (Uuid, Uuid, &'static [u8]) {
            const FILE: &[u8] = b"le contenu du fichier";
            let page = self.page(Some(parent), "Notes", &[]).await;
            let child = self.page(Some(page), "Sous-page", &[]).await;
            let cookie = &self.olga.1;
            let upload = Request::post(format!("/api/v1/pages/{page}/files?name=photo.bin"));
            let (status, _, body) = self.send(cookie, upload, FILE.to_vec()).await;
            assert_eq!(status, StatusCode::CREATED);
            let file: Value = serde_json::from_slice(&body).unwrap();
            let url = file["url"].as_str().unwrap();
            let mmd = format!("# Titre\n\n![photo]({url})\n\nVoir [le fichier]({url}).\n");
            for (id, content) in [(page, mmd), (child, "Dans la sous-page.\n".to_string())] {
                let put = Request::put(format!("/api/v1/pages/{id}/mmd"));
                let (status, _, body) = self.send(cookie, put, content.into_bytes()).await;
                assert_eq!(
                    status,
                    StatusCode::NO_CONTENT,
                    "{}",
                    String::from_utf8_lossy(&body)
                );
            }
            (page, child, FILE)
        }

        /// Les blocs de la page, sans identifiants, l'adresse de chacun de ses propres fichiers
        /// remplacée par « fichier » : ce qu'une copie doit avoir de son original. Avec ces
        /// adresses, dans l'ordre.
        async fn content(&self, page: Uuid) -> (Vec<Block>, Vec<String>) {
            let mut blocks = self.state.hub.read_blocks(page).await.unwrap();
            let mut stack = vec![blocks.as_mut_slice()];
            while let Some(level) = stack.pop() {
                for block in level.iter_mut() {
                    block.id.clear();
                    stack.push(&mut block.children);
                }
            }
            let mut files = Vec::new();
            each_address(&mut blocks, &mut |address| {
                if file_of(address, page).is_some() {
                    files.push(std::mem::replace(address, "fichier".to_string()));
                }
            });
            (blocks, files)
        }

        /// La copie `copy` de la page de [`Bench::illustrated`] est entière : son contenu et celui
        /// de sa sous-page, et son fichier, attaché à elle, que ses deux adresses servent.
        async fn assert_whole_copy(&self, copy: Uuid, of: (Uuid, Uuid, &'static [u8]), what: &str) {
            let (page, child, file) = of;
            let (original, _) = self.content(page).await;
            let (copied, files) = self.content(copy).await;
            assert!(original.len() >= 3, "{what} : précondition");
            assert_eq!(copied, original, "{what} : le contenu de la copie");
            assert_eq!(files.len(), 2, "{what} : l'image et le lien : {files:?}");
            assert_eq!(files[0], files[1], "{what} : une seule attache");
            let (status, _, body) = self
                .send(&self.mona.1, Request::get(&files[0]), Vec::new())
                .await;
            assert_eq!(status, StatusCode::OK, "{what} : le fichier de la copie");
            assert_eq!(body.as_ref(), file, "{what}");
            let all = self.state.store.list_nodes(self.space).await.unwrap();
            let under: Vec<Uuid> = all
                .iter()
                .filter(|node| node.parent_id == Some(copy))
                .map(|node| node.id)
                .collect();
            assert_eq!(under.len(), 1, "{what} : la sous-page");
            assert_eq!(
                self.content(under[0]).await.0,
                self.content(child).await.0,
                "{what} : le contenu de la sous-page"
            );
        }

        /// Nœuds, instantanés et attaches de la base.
        async fn written(&self) -> (i64, i64, i64) {
            let url = format!("sqlite:{}", self._dir.path().join("m.db").display());
            let mut side = <sqlx::SqliteConnection as sqlx::Connection>::connect(&url)
                .await
                .unwrap();
            let mut counts = [0i64; 3];
            for (i, table) in ["nodes", "doc_snapshots", "attachments"].iter().enumerate() {
                let sql = format!("SELECT COUNT(*) FROM {table}");
                counts[i] = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                    .fetch_one(&mut side)
                    .await
                    .unwrap();
            }
            sqlx::Connection::close(side).await.unwrap();
            (counts[0], counts[1], counts[2])
        }
    }

    fn copy_id(body: &Bytes) -> Uuid {
        let copy: Value = serde_json::from_slice(body).unwrap();
        Uuid::parse_str(copy["id"].as_str().unwrap()).unwrap()
    }

    /// Ruling R590 : une copie attend son tour d'écriture ; ce qui change pendant cette attente,
    /// après le second contrôle de la route, est vu par la transaction, qui n'écrit rien, et la
    /// route redécide. Le test tient un tour de la file, y range le changement, puis la
    /// duplication, dont le second contrôle lit donc encore l'état d'avant ; le tour rendu, la
    /// file sert le changement, puis la copie. Page mise à la corbeille : `409 trashed`, rien
    /// d'écrit ; droit de créer sous la page parente retiré (`Stale`) : la copie va dans le
    /// « Privé » de Mona ; page passée sous une autre page (`NotFound` : elle n'est plus une sœur
    /// à l'endroit visé) : la copie se pose à côté d'elle, là où elle est. Après une reprise, la
    /// copie est entière : contenu, sous-page et fichier (constats I1 et I2 de la relecture).
    /// Témoin : sans changement, la copie se pose sous la page parente.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn what_changes_while_a_copy_waits_its_turn_is_seen_by_its_transaction() {
        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Change {
            Nothing,
            Trashed,
            NoMoreCreation,
            Moved,
        }
        let bench = Bench::new().await;
        let store = bench.state.store.clone();
        let queued = |n: usize| {
            let store = store.clone();
            async move {
                wait_until("les écritures sont en file", || store.queued_writes() == n).await;
            }
        };
        for change in [
            Change::Nothing,
            Change::Trashed,
            Change::NoMoreCreation,
            Change::Moved,
        ] {
            let what = format!("{change:?}");
            let parent = bench.page(None, "Projet", &[members(Level::Edit)]).await;
            let elsewhere = bench.page(None, "Ailleurs", &[members(Level::Edit)]).await;
            let source = bench.illustrated(parent).await;
            let page = source.0;
            let before = bench.written().await;

            let held = store.hold_writes().await;
            let changed = {
                let (store, olga) = (store.clone(), bench.olga.0);
                tokio::spawn(async move {
                    match change {
                        Change::Nothing => {}
                        Change::Trashed => {
                            store.trash_node(page, olga).await.unwrap();
                        }
                        Change::NoMoreCreation => {
                            store.set_grant(parent, members(Level::Read)).await.unwrap();
                        }
                        Change::Moved => {
                            store
                                .move_node(page, Some(elsewhere), None, None)
                                .await
                                .unwrap();
                        }
                    }
                })
            };
            let waiting = usize::from(change != Change::Nothing);
            queued(waiting).await;
            let reply = bench.duplicate(&bench.mona.1, page);
            // La copie est en file derrière le changement : son second contrôle est passé.
            queued(waiting + 1).await;
            drop(held);
            changed.await.unwrap();
            let (status, body) = reply.await.unwrap();
            let text = String::from_utf8_lossy(&body).to_string();
            if change == Change::Trashed {
                assert_eq!(status, StatusCode::CONFLICT, "{what} : {text}");
                assert_eq!(error_of(&body), "trashed", "{what}");
                assert_eq!(bench.written().await, before, "{what} : rien d'écrit");
                continue;
            }
            assert_eq!(status, StatusCode::CREATED, "{what} : {text}");
            let after = bench.written().await;
            assert_eq!(
                after,
                (before.0 + 2, before.1 + 2, before.2 + 1),
                "{what} : deux pages, une attache"
            );
            let copy = copy_id(&body);
            let stored = store.get_node(copy).await.unwrap().unwrap();
            let own = store.node_grants(copy).await.unwrap();
            match change {
                Change::NoMoreCreation => {
                    assert_eq!(stored.parent_id, None, "{what} : à la racine");
                    assert_eq!(own, [named(bench.mona.0, Level::Full)], "{what}");
                }
                Change::Moved => {
                    assert_eq!(stored.parent_id, Some(elsewhere), "{what}");
                    assert!(own.is_empty(), "{what}");
                    let moved = store.get_node(page).await.unwrap().unwrap();
                    assert!(moved.position < stored.position, "{what} : après la page");
                }
                _ => {
                    assert_eq!(stored.parent_id, Some(parent), "{what}");
                    assert!(own.is_empty(), "{what}");
                }
            }
            bench.assert_whole_copy(copy, source, &what).await;
        }
        bench.finish().await;
    }

    /// Constat I2 de la relecture : une copie que le store refuse à son tour d'écriture est
    /// reprise, et la troisième tentative l'écrit entière, documents, sous-page et fichier
    /// compris ; refusée trois fois, elle répond `409 page_moved`, et rien n'est écrit. Le store
    /// refuse ici par son crochet de test (`NotFound`, ce que dit une page qui vient de changer de
    /// place, ou un contenu parti).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_copy_refused_at_its_turn_is_tried_three_times() {
        let bench = Bench::new().await;
        let store = bench.state.store.clone();
        let parent = bench.page(None, "Projet", &[members(Level::Edit)]).await;
        let source = bench.illustrated(parent).await;
        let before = bench.written().await;

        store.refuse_next_copies(DUPLICATE_RETRIES);
        let (status, body) = bench.duplicate(&bench.mona.1, source.0).await.unwrap();
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{}",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(error_of(&body), "page_moved");
        assert_eq!(bench.written().await, before, "rien d'écrit");

        store.refuse_next_copies(DUPLICATE_RETRIES - 1);
        let (status, body) = bench.duplicate(&bench.mona.1, source.0).await.unwrap();
        assert_eq!(
            status,
            StatusCode::CREATED,
            "{}",
            String::from_utf8_lossy(&body)
        );
        let after = bench.written().await;
        assert_eq!(after, (before.0 + 2, before.1 + 2, before.2 + 1));
        bench
            .assert_whole_copy(copy_id(&body), source, "refusée deux fois")
            .await;
        // Le crochet est épuisé : la suivante passe du premier coup.
        let (status, _) = bench.duplicate(&bench.mona.1, source.0).await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        bench.finish().await;
    }

    /// Ruling R605 : une duplication à la fois par compte. Pendant que la première de Mona attend
    /// de bâtir, sa seconde est refusée tout de suite (`429 duplication_busy`, `Retry-After: 1`,
    /// un message qui tutoie), sans rien écrire ni attendre ; celle d'un autre compte est admise.
    /// La première finie, Mona duplique de nouveau.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_second_duplication_of_an_account_is_refused_at_once() {
        let bench = Bench::new().await;
        let grants = [members(Level::Edit), named(bench.gina.0, Level::Edit)];
        let parent = bench.page(None, "Projet", &grants).await;
        let page = bench.page(Some(parent), "Notes", &[]).await;
        let other = bench.page(Some(parent), "Autres notes", &[]).await;
        let before = bench.nodes().await;

        let held = bench.hold_the_builds().await;
        let first = bench.duplicate(&bench.mona.1, page);
        bench.builds_waiting(1).await;
        // La même page, puis une autre : c'est le compte qui est occupé, pas la page.
        for target in [page, other] {
            let post = Request::post(format!("/api/v1/pages/{target}/duplicate"));
            let (status, headers, body) = bench.send(&bench.mona.1, post, Vec::new()).await;
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(error_of(&body), "duplication_busy");
            assert_eq!(headers[header::RETRY_AFTER], "1");
            let body: Value = serde_json::from_slice(&body).unwrap();
            let message = body["message"].as_str().unwrap();
            assert!(
                message.contains("tu as déjà une duplication en cours"),
                "{message}"
            );
        }
        // Les refus de la page passent avant celui du débit : une page qu'elle ne voit pas.
        let unseen = Request::post(format!("/api/v1/pages/{}/duplicate", Uuid::now_v7()));
        let (status, _, _) = bench.send(&bench.mona.1, unseen, Vec::new()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        // Un autre compte n'attend pas Mona.
        let second = bench.duplicate(&bench.gina.1, page);
        bench.builds_waiting(2).await;
        assert_eq!(bench.nodes().await, before, "rien d'écrit encore");
        drop(held);
        assert_eq!(first.await.unwrap().0, StatusCode::CREATED);
        assert_eq!(second.await.unwrap().0, StatusCode::CREATED);
        assert_eq!(bench.nodes().await, before + 2);
        // La place est rendue.
        let (status, _) = bench.duplicate(&bench.mona.1, page).await.unwrap();
        assert_eq!(status, StatusCode::CREATED);

        // Elle est tenue jusqu'à la fin de l'écriture : pendant que la copie attend son tour dans
        // la file, documents bâtis, le compte est encore occupé.
        let store = bench.state.store.clone();
        let held = store.hold_writes().await;
        let writing = bench.duplicate(&bench.mona.1, page);
        wait_until("la copie attend son tour d'écriture", || {
            store.queued_writes() == 1
        })
        .await;
        let post = Request::post(format!("/api/v1/pages/{other}/duplicate"));
        let (status, _, body) = bench.send(&bench.mona.1, post, Vec::new()).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error_of(&body), "duplication_busy");
        drop(held);
        assert_eq!(writing.await.unwrap().0, StatusCode::CREATED);
        bench.finish().await;
    }

    /// Ruling R606 : le budget d'un compte. Ici, dix octets par minute : la première duplication
    /// de Mona est admise et bâtit plus ; la suivante est refusée (`429 duplication_busy`), avec
    /// dans `Retry-After` et dans le message le délai au bout duquel la première sort de la
    /// fenêtre, et rien n'est écrit. Le budget d'un autre compte est intact. Une copie refusée
    /// pour son poids a coûté ce qu'elle a bâti : elle compte aussi.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_account_past_its_budget_is_told_when_to_come_back() {
        use crate::duplications::Duplications;
        let minute = std::time::Duration::from_secs(60);
        let bench = Bench::with(|state| {
            state.duplications = Arc::new(Duplications::new(10, minute));
        })
        .await;
        let grants = [members(Level::Edit), named(bench.gina.0, Level::Edit)];
        let parent = bench.page(None, "Projet", &grants).await;
        let page = bench.page(Some(parent), "Notes", &[]).await;
        let refused = async |cookie: &str, what: &str| {
            let before = bench.nodes().await;
            let post = Request::post(format!("/api/v1/pages/{page}/duplicate"));
            let (status, headers, body) = bench.send(cookie, post, Vec::new()).await;
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{what}");
            assert_eq!(error_of(&body), "duplication_busy", "{what}");
            let wait: u64 = headers[header::RETRY_AFTER]
                .to_str()
                .unwrap()
                .parse()
                .unwrap();
            assert!((55..=60).contains(&wait), "{what} : {wait} s");
            let body: Value = serde_json::from_slice(&body).unwrap();
            let message = body["message"].as_str().unwrap();
            assert!(
                message.contains(&format!("réessaie dans {wait} s")),
                "{what} : {message}"
            );
            assert_eq!(bench.nodes().await, before, "{what} : rien d'écrit");
        };
        let (status, _) = bench.duplicate(&bench.mona.1, page).await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "sous le budget, admise");
        refused(&bench.mona.1, "budget dépensé").await;
        refused(&bench.mona.1, "un refus ne rend rien").await;
        let (status, _) = bench.duplicate(&bench.gina.1, page).await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "le budget d'un autre compte");
        bench.finish().await;

        // Une copie refusée pour son poids compte ce qu'elle a bâti.
        let bench = Bench::with(|state| {
            state.duplications = Arc::new(Duplications::new(10, minute));
            state.max_duplicated_bytes = 1;
        })
        .await;
        let parent = bench.page(None, "Projet", &[members(Level::Edit)]).await;
        let page = bench.page(Some(parent), "Notes", &[]).await;
        let (status, body) = bench.duplicate(&bench.mona.1, page).await.unwrap();
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error_of(&body), "subtree_too_large");
        let (status, body) = bench.duplicate(&bench.mona.1, page).await.unwrap();
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "bâtie, donc comptée");
        assert_eq!(error_of(&body), "duplication_busy");
        bench.finish().await;
    }

    /// Constat M2 de la relecture : seule une page se duplique. Un dossier répond `422
    /// invalid_request`, sans rien écrire ; un dossier d'un sous-arbre n'entre pas dans la copie,
    /// ni les pages qu'il porte.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_folder_is_not_duplicated() {
        let bench = Bench::new().await;
        let store = bench.state.store.clone();
        let root = bench.page(None, "Projet", &[members(Level::Full)]).await;
        let kept = bench.page(Some(root), "Page", &[]).await;
        let folder = store
            .create_node(NewNode {
                space_id: bench.space,
                parent_id: Some(root),
                after: None,
                kind: NodeKind::Folder,
                title: "Dossier".to_string(),
                created_by: Some(bench.olga.0),
                grants: Vec::new(),
                doc: None,
            })
            .await
            .unwrap()
            .id;
        let _under = bench.page(Some(folder), "Dans le dossier", &[]).await;
        let before = bench.nodes().await;

        let (status, body) = bench.duplicate(&bench.mona.1, folder).await.unwrap();
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error_of(&body), "invalid_request");
        assert_eq!(bench.nodes().await, before, "rien d'écrit");

        let (status, body) = bench.duplicate(&bench.mona.1, root).await.unwrap();
        assert_eq!(
            status,
            StatusCode::CREATED,
            "{}",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(bench.nodes().await, before + 2, "la racine et sa page");
        let copy = copy_id(&body);
        let all = store.list_nodes(bench.space).await.unwrap();
        let under: Vec<_> = all
            .iter()
            .filter(|node| node.parent_id == Some(copy))
            .map(|node| (node.title.as_str(), node.kind))
            .collect();
        assert_eq!(under, [("Page", NodeKind::Page)]);
        let _ = kept;
        bench.finish().await;
    }

    /// Constat N1 de la re-relecture : une duplication abandonnée pendant la construction d'un
    /// document garde sa place, et compte ce document, tant que le calcul n'a pas fini. Le calcul
    /// va au bout sur son fil, client parti ou non : sans cela, une boucle de requêtes
    /// abandonnées tenait les places de calcul sans rien dépenser. Ici, une page assez grosse
    /// pour que sa construction dure ; la requête est abandonnée pendant elle ; la demande
    /// suivante du compte est refusée (`429`) ; et, le calcul fini, le budget (dix octets par
    /// minute) a compté le document : la suivante est refusée pour la minute.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_abandoned_duplication_keeps_its_place_until_its_build_ends() {
        use crate::duplications::Duplications;
        let bench = Bench::with(|state| {
            let minute = std::time::Duration::from_secs(60);
            state.duplications = Arc::new(Duplications::new(10, minute));
        })
        .await;
        // Le document est écrit avec le nœud : un `PUT /mmd` de cette taille durerait.
        let blocks: Vec<Block> = mementum_md::parse(
            &(0..4_000)
                .map(|i| format!("Bloc {i} : du texte **en gras**, pour durer.\n"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let doc = PageDoc::new_page().0;
        doc.write_blocks(&blocks).unwrap();
        let page = bench
            .state
            .store
            .create_node(NewNode {
                space_id: bench.space,
                parent_id: None,
                after: None,
                kind: NodeKind::Page,
                title: "Grosse".to_string(),
                created_by: Some(bench.olga.0),
                grants: vec![members(Level::Full)],
                doc: Some(doc.encode_state()),
            })
            .await
            .unwrap()
            .id;
        let before = bench.nodes().await;
        let reads = bench.state.mmd_reads.clone();

        let abandoned = bench.duplicate(&bench.mona.1, page);
        wait_until("le document se bâtit", || reads.counts().0 == 1).await;
        // Le client part pendant la construction.
        abandoned.abort();
        let _ = abandoned.await;
        let refused = async |what: &str| {
            let post = Request::post(format!("/api/v1/pages/{page}/duplicate"));
            let (status, _, body) = bench.send(&bench.mona.1, post, Vec::new()).await;
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{what}");
            assert_eq!(error_of(&body), "duplication_busy", "{what}");
            let body: Value = serde_json::from_slice(&body).unwrap();
            body["message"].as_str().unwrap().to_string()
        };
        refused("pendant le calcul abandonné").await;
        wait_until("le calcul abandonné va au bout", || reads.counts().0 == 0).await;
        let message = refused("le calcul fini").await;
        assert!(
            message.contains("réessaie dans"),
            "le document bâti est compté au budget : {message}"
        );
        assert_eq!(bench.nodes().await, before, "rien d'écrit");
        bench.finish().await;
    }
}
