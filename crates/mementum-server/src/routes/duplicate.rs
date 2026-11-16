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
//! à la corbeille (elle se lit, mais ne se copie pas) ; `422 subtree_too_large` au-delà de
//! [`MAX_DUPLICATED_PAGES`] pages, ou de [`MAX_DUPLICATED_BYTES`] octets de documents.
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
use mementum_md::{Block, PropValue};
use mementum_store::{CopiedAttachment, CopiedPage, NewCopy, Node, Sibling, StoreError};
use mementum_sync::SyncError;
use uuid::Uuid;

use crate::access::{at_least, permit, permit_level};
use crate::auth::{AuthUser, SessionUser};
use crate::error::ApiError;
use crate::routes::pages::{MAX_TITLE_CHARS, page_id, page_node};
use crate::types::PageNode;
use crate::{AppState, now_ms, to_completion};

/// Pages d'une copie, racine comprise (Ruling R252) : au-delà, `422 subtree_too_large`, et l'on
/// duplique par morceaux.
pub const MAX_DUPLICATED_PAGES: usize = 100;

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
        let destination = destination(&space, &access)?;
        if access.in_trash {
            return Err(ApiError::trashed());
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
fn destination(space: &SpaceAccess, access: &NodeAccess) -> Result<Destination, ApiError> {
    if let Some(parent) = access.visible_parent
        && let Some(above) = space.node(parent)
        && permit_level(above, Action::CreateChild).is_ok()
    {
        return Ok(Destination::Beside {
            parent,
            access: *above,
        });
    }
    let role = at_least(space, Role::Member)?;
    Ok(Destination::Private { role })
}

/// Les pages du sous-arbre de `root` que `space` montre, hors corbeille, la racine d'abord,
/// chaque page après sa page parente, les sœurs dans l'ordre de leurs positions. `nodes` est
/// tout l'espace, trié par `(parent, position)` (`Store::list_nodes`). On ne descend que par les
/// pages gardées : une page que la personne ne voit pas, ou à la corbeille, sort avec ses
/// sous-pages. S'arrête dès que la liste dépasse `limit` : l'appelant refuse.
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
            if child.id != root && space.node(child.id).is_some_and(|access| !access.in_trash) {
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

/// L'attache que nomme `url`, si c'est exactement l'adresse d'un fichier de la page `page` :
/// `/api/v1/pages/<page>/files/<attache>`, sans rien avant ni après.
fn file_of(url: &str, page: Uuid) -> Option<Uuid> {
    let (of, file) = url.strip_prefix("/api/v1/pages/")?.split_once("/files/")?;
    (Uuid::parse_str(of).ok()? == page).then_some(())?;
    Uuid::parse_str(file).ok()
}

/// L'adresse de fichier d'un bloc, s'il en porte une.
fn file_url(block: &Block) -> Option<&str> {
    if !FILE_BLOCKS.contains(&block.kind.as_str()) {
        return None;
    }
    match block.props.get("url") {
        Some(PropValue::Str(url)) => Some(url),
        _ => None,
    }
}

/// Les attaches de la page `page` que nomment ses blocs, chacune une fois, à toute profondeur.
fn named_files(blocks: &[Block], page: Uuid) -> Vec<Uuid> {
    let mut found = Vec::new();
    let mut known = HashSet::new();
    let mut stack = vec![blocks];
    while let Some(level) = stack.pop() {
        for block in level {
            if let Some(file) = file_url(block).and_then(|url| file_of(url, page))
                && known.insert(file)
            {
                found.push(file);
            }
            stack.push(&block.children);
        }
    }
    found
}

/// Le document neuf de la copie de la page `from`, d'identifiant `to` : les blocs de l'original,
/// **tous sans identifiant** (l'écriture en tire de neufs, à chaque profondeur), les adresses des
/// fichiers de `from` que `files` recopie récrites vers leur attache neuve. Toute autre adresse
/// reste telle quelle : externe, d'une autre page, ou d'un fichier que la page n'a pas.
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
            let copy = file_url(block)
                .and_then(|url| file_of(url, from))
                .and_then(|file| files.get(&file));
            if let Some(copy) = copy {
                let url = format!("/api/v1/pages/{to}/files/{copy}");
                block.props.insert("url".to_string(), PropValue::Str(url));
            }
            stack.push(&mut block.children);
        }
    }
    let page = PageDoc::new_page().0;
    page.write_blocks(&blocks)?;
    Ok(page.encode_state())
}

/// `POST /api/v1/pages/{id}/duplicate` : `201`, la racine de la copie, telle que la personne la
/// voit (voir le module).
pub(crate) async fn duplicate(
    State(state): State<AppState>,
    caller: SessionUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<(StatusCode, Json<PageNode>), ApiError> {
    let SessionUser { user, session } = caller;
    let id = page_id(path)?;
    // Premier contrôle : les refus, avant de lire quoi que ce soit d'une page.
    let first = state.duplication(&user, id).await?;
    let space_id = first.node.space_id;
    let nodes = state.store.list_nodes(space_id).await?;
    let sources = seen_subtree(&first.space, &nodes, id, MAX_DUPLICATED_PAGES);
    drop(nodes);
    if sources.len() > MAX_DUPLICATED_PAGES {
        return Err(too_many_pages());
    }

    // Page par page : lire, puis bâtir, sans garder les blocs d'une page pendant la suivante.
    let mut built = Vec::with_capacity(sources.len());
    for source in &sources {
        let blocks = match state.hub.read_blocks(source.id).await {
            Ok(blocks) => blocks,
            // Une sous-page supprimée depuis le contrôle : le second contrôle la sort de la copie.
            Err(SyncError::NotFound) if source.id != id => continue,
            Err(error) => return Err(error.into()),
        };
        let mut attachments = Vec::new();
        let mut files = HashMap::new();
        for file in named_files(&blocks, source.id) {
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
            .run(user.id, move || copied_doc(blocks, from, to, &files))
            .await?
            .map_err(SyncError::Crdt)?;
        built.push(Built {
            source: from,
            id: to,
            doc,
            attachments,
        });
    }

    let user_id = user.id;
    let (root, access, pages) = to_completion(async move {
        let mut built = built;
        for _ in 0..DUPLICATE_RETRIES {
            // Second contrôle, juste avant d'écrire : la session, la page, la destination.
            state.session_still_live(session).await?;
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
            let dir = tempfile::tempdir().unwrap();
            let state = state(&dir).await;
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

    /// Ruling R590 : une copie attend son tour d'écriture derrière les écritures que son compte a
    /// en file ; ce qui change pendant cette attente, après le second contrôle de la route, est vu
    /// par la transaction, qui n'écrit rien, et la route redécide. Le test tient un tour de la
    /// file, y range une écriture de Mona (un favori), puis sa duplication, qui a passé son second
    /// contrôle, puis, par une autre voie, le changement ; le tour rendu, la file sert le favori,
    /// le changement, puis la copie. Page mise à la corbeille : `409 trashed`, aucune page créée ;
    /// droit de créer sous la page parente retiré : la copie va dans le « Privé » de Mona.
    /// Témoin : sans changement, la copie se pose sous la page parente.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn what_changes_while_a_copy_waits_its_turn_is_seen_by_its_transaction() {
        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Change {
            Nothing,
            Trashed,
            NoMoreCreation,
        }
        let bench = Bench::new().await;
        let store = bench.state.store.clone();
        let queued = |n: usize| {
            let store = store.clone();
            async move {
                wait_until("les écritures sont en file", || store.queued_writes() == n).await;
            }
        };
        for change in [Change::Nothing, Change::Trashed, Change::NoMoreCreation] {
            let what = format!("{change:?}");
            let parent = bench.page(None, "Projet", &[members(Level::Edit)]).await;
            let page = bench.page(Some(parent), "Notes", &[]).await;
            let before = bench.nodes().await;

            let held = store.hold_writes().await;
            let favorite = Request::put(format!("/api/v1/pages/{page}/favorite"))
                .header(header::HOST, "localhost")
                .header(header::COOKIE, &bench.mona.1)
                .body(Body::empty())
                .unwrap();
            let router = bench.router.clone();
            let favorite = tokio::spawn(async move { router.oneshot(favorite).await.unwrap() });
            queued(1).await;
            let reply = bench.duplicate(&bench.mona.1, page);
            // La copie est en file : son second contrôle est passé.
            queued(2).await;
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
                    }
                })
            };
            if change != Change::Nothing {
                queued(3).await;
            }
            drop(held);
            changed.await.unwrap();
            assert_eq!(favorite.await.unwrap().status(), StatusCode::NO_CONTENT);
            let (status, body) = reply.await.unwrap();
            let text = String::from_utf8_lossy(&body).to_string();
            match change {
                Change::Trashed => {
                    assert_eq!(status, StatusCode::CONFLICT, "{what} : {text}");
                    assert_eq!(error_of(&body), "trashed", "{what}");
                    assert_eq!(bench.nodes().await, before, "{what} : aucune page créée");
                }
                Change::Nothing | Change::NoMoreCreation => {
                    assert_eq!(status, StatusCode::CREATED, "{what} : {text}");
                    assert_eq!(bench.nodes().await, before + 1, "{what}");
                    let copy: Value = serde_json::from_slice(&body).unwrap();
                    let id = Uuid::parse_str(copy["id"].as_str().unwrap()).unwrap();
                    let stored = store.get_node(id).await.unwrap().unwrap();
                    let own = store.node_grants(id).await.unwrap();
                    if change == Change::Nothing {
                        assert_eq!(stored.parent_id, Some(parent), "{what}");
                        assert!(own.is_empty(), "{what}");
                    } else {
                        assert_eq!(stored.parent_id, None, "{what} : à la racine");
                        assert_eq!(own, [named(bench.mona.0, Level::Full)], "{what}");
                        assert_eq!(copy["section"], "private", "{what}");
                    }
                }
            }
        }
        bench.finish().await;
    }
}
