//! Route de recherche dans un espace (lot 4, Task S5 ; plan du lot, « La requête et les droits »).
//!
//! | Route | Il faut | Réponse |
//! |---|---|---|
//! | `POST /api/v1/spaces/{id}/search`, corps [`SearchRequest`] | un rôle dans l'espace, invité compris | [`SearchResults`] : les pages que le compte voit, hors corbeille, vingt au plus ; jamais avant 50 ms ([`SEARCH_FLOOR`]) pour une recherche calculée |
//!
//! C'est un `POST` qui n'écrit rien : la recherche est dans le corps, pour n'être dans aucune
//! adresse, donc dans aucun journal d'accès (Ruling R917). Elle n'est jamais journalisée ici, à
//! aucun niveau, ni recopiée dans un message de refus.
//!
//! **Refus**, dans l'ordre : `401` sans session ; `409 account_changed` pour un autre compte
//! nommé ; `404 not_found` sans rôle dans l'espace, la réponse d'un espace inconnu, **que le corps
//! soit lisible ou non** ; puis le corps : `413` au-delà de 4 Kio ([`MAX_SEARCH_BODY_BYTES`]),
//! `400` ou `422 invalid_json` ; `422 invalid_query` pour une recherche de plus de 200 caractères
//! ou de 8 mots, ou qui demande au-delà de ses cent premières pages ; `429 search_busy`
//! (`Retry-After: 1`) pour une recherche qui attendait et qu'une plus récente du compte a
//! remplacée. Le corps, petit et borné, est reçu avant tout refus, celui de la session compris
//! (un refus rendu sur un corps non lu se perd chez un client qui envoie sans attendre, R505),
//! mais il n'est **jugé** qu'après les droits. Rien de l'index n'est lu avant le `404`.
//!
//! **Les droits** (Review Focus 1 et 2 du plan). L'index plein texte ne porte ni espace, ni droit,
//! ni corbeille : il rend toute page de l'ensemble qu'on lui donne. Cet ensemble est bâti ici, à
//! chaque recherche, de ce qu'`access.rs` montre au compte dans **cet** espace, hors corbeille
//! ([`compute`]) : un droit retiré, une page fermée ou jetée valent à la recherche suivante, sans
//! attendre l'indexeur ; les droits dont il sort sont relus une fois la place du débit prise, si
//! bien qu'ils valent aussi pour une recherche qui attendait. Chaque page rendue est revérifiée (un filet : une page hors de l'ensemble
//! ferait une ligne `error` et serait jetée), puis reçoit son titre, lu dans la base à l'instant
//! (R904), et son fil d'Ariane, fait des seuls ancêtres que le compte voit (`visible_parent`,
//! R903). `indexing` ne dit en retard que des pages de l'ensemble. La réponse ne porte aucun total
//! (R905).
//!
//! **Le débit** (`searches.rs`, R910) : une recherche admise et une en attente par compte, quatre
//! calculs de front pour l'instance. Tout ce qui croît avec l'espace (bâtir l'ensemble, lire
//! l'index, remonter les fils d'Ariane) tourne sous les deux places, sur le fil bloquant, jamais
//! sur un fil du moteur ni avant le débit. Un calcul lancé va au bout, même si la requête est
//! abandonnée, et garde ses places jusque-là : la recherche est menée dans une tâche à elle
//! ([`answered`]).
//!
//! **Le plancher** (R932) : la réponse d'une recherche calculée ne part jamais avant
//! [`SEARCH_FLOOR`] après la prise de ses places, que l'index ait lu une entrée ou cent mille (lire
//! une entrée d'une page que le compte ne voit pas coûte, et ce coût se lirait dans le temps de
//! réponse). L'attente se fait sur le moteur, sans fil bloquant ; **les deux places, celle du
//! compte et celle de l'instance, ne sont rendues qu'à la fin de cette attente**, requête
//! abandonnée ou non (R1054) : sans cela, l'heure où commence la recherche suivante du compte,
//! ou celle d'un cinquième compte qui attendait une place de l'instance, dirait la durée du
//! calcul. L'instance mène donc au plus quatre recherches calculées par plancher, quatre-vingts
//! par seconde. Une recherche sans aucun mot, un refus et un `429` ne lisent pas l'index et
//! n'attendent pas.
//! **Ce qui reste** (limite L-b du plan) : un calcul plus long que le plancher. Sa durée se lit
//! alors dans le temps de réponse, et dans l'attente de qui vient derrière. Et le titre d'une
//! page trouvée est lu à l'instant de la réponse sans que le droit soit rejugé : une page fermée
//! au compte pendant le calcul est encore rendue, avec le titre qu'elle porte à cet instant (la
//! fenêtre est la durée du calcul).
//!
//! **Ce que la route ne borne pas** : la durée d'un calcul lancé. Rien ne l'interrompt (le moteur
//! n'offre pas d'arrêt à une recherche en cours) ; elle est bornée par la forme de la recherche
//! (huit mots, un seul début de mot, les plafonds de vérification de l'index) et par le débit, et
//! tient l'index pendant ce temps (une réouverture ou une reconstruction l'attend, R1006).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use mementum_acl::{NodeAccess, SpaceAccess};
use mementum_search::{Hit, IndexError, PageSet, Query, is_stand_in, read_query};
use tokio::time::Instant;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{ApiError, internal};
use crate::indexer::IndexerHandle;
use crate::routes::spaces::space_id;
use crate::searches::{Searches, Seat, Turn};
use crate::types::{SearchAncestor, SearchHit, SearchRequest, SearchResults};

/// Le temps de réponse plancher d'une recherche calculée (Ruling R932) : valeur de départ, à
/// arrêter en X2 au-dessus du pire p95 mesuré.
pub const SEARCH_FLOOR: Duration = Duration::from_millis(50);

/// Pages rendues par réponse (Ruling R911).
pub const SEARCH_PAGE_SIZE: usize = 20;

/// Pages qu'une recherche rend en tout, de réponse en réponse (Ruling R911) : cinq réponses.
pub const MAX_SEARCH_PAGES: usize = 100;

/// Octets au plus du corps d'une recherche : 200 caractères de quatre octets au plus, échappés
/// en JSON, tiennent dessous.
pub const MAX_SEARCH_BODY_BYTES: usize = 4096;

/// Ancêtres nommés au plus dans le fil d'Ariane d'une page trouvée : les plus proches d'elle.
pub(crate) const MAX_PATH: usize = 32;

/// `POST /api/v1/spaces/{id}/search` : les pages de l'espace que le compte voit et qui portent
/// les mots cherchés.
pub(crate) async fn search(
    State(state): State<AppState>,
    caller: Result<AuthUser, ApiError>,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<SearchRequest>, JsonRejection>,
) -> Result<Json<SearchResults>, ApiError> {
    let user = caller?;
    let space_id = space_id(path)?;
    // Les droits d'abord : sans rôle, `404`, quoi que porte le corps. Ce premier contrôle ne
    // sert qu'aux refus : l'ensemble permis est bâti des droits relus une fois la place prise.
    state.space_access(&user, space_id).await?;
    let Json(request) = body?;
    let query =
        read_query(&request.q).map_err(|refusal| ApiError::invalid_query(refusal.to_string()))?;
    let after = usize::try_from(request.after.unwrap_or(0)).unwrap_or(usize::MAX);
    if after >= MAX_SEARCH_PAGES {
        return Err(ApiError::invalid_query(
            "Une recherche ne rend que ses cent premières pages : précise-la pour trouver les \
             suivantes.",
        ));
    }
    let filter = Filter::of(&request)?;
    if query.is_empty() {
        // Rien à chercher : ni place, ni calcul, ni plancher.
        return Ok(Json(SearchResults::default()));
    }
    let (seat, turn) = state
        .searches
        .enter(user.party())
        .await
        .map_err(|_| ApiError::search_busy())?;
    let started = Instant::now();
    // Second contrôle (R89), sous la garde : la recherche a pu attendre sa place, et un droit
    // retiré pendant ce temps vaut pour elle. Lu dans le cache des accès, par `rev`. Un refus ici
    // (le compte sorti de l'espace) n'a rien lu de l'index : il part sans plancher.
    let access = state.space_access(&user, space_id).await?;
    // Dans une tâche à elle : abandonnée, la requête laisse le calcul finir, et la place du
    // compte tenue jusqu'au plancher.
    let search = Search {
        state,
        account: user.id,
        space_id,
        access,
        query,
        after,
        filter,
    };
    let found = tokio::spawn(answered(search, seat, turn, started))
        .await
        .map_err(internal)??;
    Ok(Json(found))
}

/// Une recherche admise : ce qu'il faut pour la mener sans la requête.
struct Search {
    state: AppState,
    account: Uuid,
    space_id: Uuid,
    access: Arc<SpaceAccess>,
    query: Query,
    after: usize,
    filter: Option<Filter>,
}

/// Les filtres d'une recherche (lot 5, Ruling R1248) : créée par ce compte, contenu daté depuis,
/// ou avant. Ils réduisent l'ensemble des pages permises, avant l'index, jamais ne l'élargissent.
#[derive(Debug, Clone, Copy)]
struct Filter {
    created_by: Option<Uuid>,
    edited_after: Option<i64>,
    edited_before: Option<i64>,
}

impl Filter {
    /// Les filtres du corps, lus : `422 invalid_filter` pour un compte qui ne se lit pas, ou des
    /// dates qu'aucune page ne peut passer. Aucun filtre : `None`.
    fn of(request: &SearchRequest) -> Result<Option<Filter>, ApiError> {
        let created_by = match request.created_by.as_deref() {
            None => None,
            Some(raw) => Some(
                Uuid::try_parse(raw)
                    .ok()
                    .filter(|_| raw.len() == 36)
                    .ok_or_else(|| {
                        ApiError::invalid_filter(
                            "« createdBy » : l'identifiant d'un compte (un membre, tel que la \
                             liste des membres le donne)",
                        )
                    })?,
            ),
        };
        if let (Some(after), Some(before)) = (request.edited_after, request.edited_before)
            && after >= before
        {
            return Err(ApiError::invalid_filter(
                "« editedAfter » doit venir avant « editedBefore » : aucune page ne passerait",
            ));
        }
        Ok((created_by.is_some()
            || request.edited_after.is_some()
            || request.edited_before.is_some())
        .then_some(Filter {
            created_by,
            edited_after: request.edited_after,
            edited_before: request.edited_before,
        }))
    }
}

/// Mène la recherche, puis attend le plancher ; la place du compte n'est rendue qu'ensuite, que
/// le calcul ait réussi ou non.
async fn answered(
    search: Search,
    seat: Seat,
    turn: Turn,
    started: Instant,
) -> Result<SearchResults, ApiError> {
    let searches = search.state.searches.clone();
    // La place de l'instance est tenue à deux : par le calcul, sur son fil, jusqu'à sa fin ; par
    // cette tâche, jusqu'au plancher. Elle est rendue au plus tard des deux (Ruling R1054).
    let turn = Arc::new(turn);
    let found = results(search, turn.clone()).await;
    searches.answer_ready();
    // Sur le moteur, sans fil bloquant : une attente, pas un calcul.
    tokio::time::sleep_until(started + SEARCH_FLOOR).await;
    drop((seat, turn));
    found
}

/// Le calcul, sur le fil bloquant, sous la place de l'instance ; puis les titres et le retard,
/// lus dans la base.
async fn results(search: Search, turn: Arc<Turn>) -> Result<SearchResults, ApiError> {
    let Search {
        state,
        account,
        space_id,
        access,
        query,
        after,
        filter,
    } = search;
    // Les pages qui passent les filtres, lues dans la base sous les places : rien de ce qui en
    // sort n'est jamais rendu sans être aussi vu par le compte (`allowed_pages`).
    let passing = match filter {
        None => None,
        Some(filter) => Some(
            state
                .store
                .pages_matching(
                    space_id,
                    filter.created_by,
                    filter.edited_after,
                    filter.edited_before,
                )
                .await
                .map_err(internal)?
                .into_iter()
                .collect::<HashSet<Uuid>>(),
        ),
    };
    let (searches, indexer) = (state.searches.clone(), state.indexer.clone());
    let computed = tokio::task::spawn_blocking(move || {
        // Un calcul lancé va au bout, même si sa tâche est partie : il garde la place jusque-là.
        let _turn = turn;
        let computed = compute(
            &searches,
            &indexer,
            &access,
            account,
            &query,
            after,
            passing.as_ref(),
        );
        searches.finished();
        computed
    })
    .await
    .map_err(internal)?
    // L'erreur de l'index porte des chemins du serveur : au journal, jamais dans la réponse.
    .map_err(internal)?;

    let mut named: Vec<Uuid> = computed
        .hits
        .iter()
        .map(|(hit, _)| hit.node_id)
        .chain(
            computed
                .hits
                .iter()
                .flat_map(|(_, path)| path.iter().copied()),
        )
        .collect();
    named.sort_unstable();
    named.dedup();
    let titles = state.store.node_titles(&named).await.map_err(internal)?;
    // Après la recherche : l'état lu vaut au moins pour ce qui vient d'être cherché (R1013).
    let allowed = computed.allowed;
    let indexing = state
        .indexer
        .behind(space_id, |page| allowed.contains(&page))
        .await
        .map_err(internal)?;

    let results = computed
        .hits
        .into_iter()
        .filter_map(|(hit, path)| {
            // Une page supprimée depuis le calcul n'a plus de titre : elle n'est pas rendue.
            let title = titles.get(&hit.node_id)?.clone();
            let path = path
                .into_iter()
                .filter_map(|id| {
                    Some(SearchAncestor {
                        id: id.to_string(),
                        title: titles.get(&id)?.clone(),
                    })
                })
                .collect();
            Some(SearchHit {
                page_id: hit.node_id.to_string(),
                title,
                path,
                // Une clé de remplacement ne nomme pas un bloc qu'on rejoint.
                block_id: hit.block_id.filter(|key| !is_stand_in(key)),
                excerpt: hit.excerpt,
                marks: hit.marks,
            })
        })
        .collect();
    Ok(SearchResults {
        results,
        more: computed.more,
        indexing,
    })
}

/// Ce que le calcul rend : les pages trouvées, chacune avec ses ancêtres vus (du plus haut au
/// plus proche), s'il en reste, et l'ensemble dont elles sortent.
struct Computed {
    hits: Vec<(Hit, Vec<Uuid>)>,
    more: bool,
    allowed: PageSet,
}

/// Le calcul d'une recherche, d'un seul tenant sur son fil : l'ensemble des pages permises, bâti
/// à chaque fois de ce que le compte voit de l'espace, hors corbeille (Rulings R921, R927) ; la
/// lecture de l'index, qui ne rend que des pages de cet ensemble ; le filet ; les fils d'Ariane.
fn compute(
    searches: &Searches,
    indexer: &IndexerHandle,
    access: &SpaceAccess,
    account: Uuid,
    query: &Query,
    after: usize,
    passing: Option<&HashSet<Uuid>>,
) -> Result<Computed, IndexError> {
    searches.computing(account);
    let allowed = allowed_pages(access, passing);
    searches.built();
    let limit = SEARCH_PAGE_SIZE.min(MAX_SEARCH_PAGES - after);
    let found = indexer.search(query, &allowed, after, limit)?;
    // `more` est celui de l'index, avant le filet : une page jetée par lui ne le change pas.
    let more = found.more && after + limit < MAX_SEARCH_PAGES;
    let mut hits = Vec::with_capacity(found.pages.len());
    for hit in found.pages {
        match access.node(hit.node_id) {
            Some(seen) if !seen.in_trash => {
                let path = ancestors(access, seen);
                hits.push((hit, path));
            }
            _ => tracing::error!(
                page = %hit.node_id,
                "l'index a rendu une page hors de l'ensemble permis : elle est jetée"
            ),
        }
    }
    Ok(Computed {
        hits,
        more,
        allowed,
    })
}

/// Les pages qu'une recherche peut rendre : ce que le compte voit de l'espace, **hors
/// corbeille** (la page ou un de ses ancêtres jetés). `access` est celui d'un seul espace : rien
/// d'un autre n'y entre. Les dossiers vus y sont aussi ; l'index n'en porte pas. Avec des
/// filtres (`passing`, lot 5, R1248), seulement celles qui les passent : un filtre ne fait que
/// réduire cet ensemble.
fn allowed_pages(access: &SpaceAccess, passing: Option<&HashSet<Uuid>>) -> PageSet {
    access
        .visible()
        .filter(|(page, seen)| !seen.in_trash && passing.is_none_or(|p| p.contains(page)))
        .map(|(page, _)| page)
        .collect()
}

/// Les ancêtres que le compte voit d'une page, du plus haut au plus proche : la chaîne de
/// `visible_parent`, qui s'arrête au premier parent non vu ; les [`MAX_PATH`] plus proches.
pub(crate) fn ancestors(access: &SpaceAccess, page: &NodeAccess) -> Vec<Uuid> {
    let mut path = Vec::new();
    let mut seen = page;
    while path.len() < MAX_PATH
        && let Some(parent) = seen.visible_parent
        && let Some(above) = access.node(parent)
    {
        path.push(parent);
        seen = above;
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use std::time::Instant as RealInstant;

    use mementum_acl::{Grant, Level, Role, Subject, TreeNode};

    use super::*;

    fn node(id: u128, parent: Option<u128>, trashed: bool) -> TreeNode {
        TreeNode {
            id: Uuid::from_u128(id),
            parent_id: parent.map(Uuid::from_u128),
            trashed,
        }
    }

    /// Ce qu'un membre voit de ces pages, toutes sous la page 1, que les membres lisent.
    fn member_access(nodes: &[TreeNode]) -> SpaceAccess {
        let lines = [(
            Uuid::from_u128(1),
            Grant {
                subject: Subject::Members,
                level: Level::Read,
            },
        )];
        SpaceAccess::compute(
            nodes,
            &lines,
            Uuid::from_u128(u128::MAX),
            Some(Role::Member),
        )
    }

    /// L'ensemble permis laisse dehors une page jetée et ce qui est sous elle, bien que le compte
    /// les voie (la corbeille se lit) ; il garde le reste.
    #[test]
    fn the_allowed_set_leaves_out_the_trash_and_what_is_under_it() {
        let nodes = [
            node(1, None, false),
            node(2, Some(1), false),
            node(3, Some(1), true),
            node(4, Some(3), false),
        ];
        let access = member_access(&nodes);
        assert_eq!(access.visible().count(), 4, "la corbeille se voit");
        let allowed = allowed_pages(&access, None);
        let kept: Vec<u128> = (1..=4)
            .filter(|id| allowed.contains(&Uuid::from_u128(*id)))
            .collect();
        assert_eq!(kept, [1, 2]);
        assert_eq!(allowed.len(), 2);
    }

    /// Le fil d'Ariane va du plus haut au plus proche, et ne garde que les [`MAX_PATH`] ancêtres
    /// les plus proches d'une page très profonde.
    #[test]
    fn the_breadcrumb_keeps_the_nearest_ancestors() {
        let depth = MAX_PATH as u128 + 5;
        let nodes: Vec<TreeNode> = (1..=depth)
            .map(|id| node(id, (id > 1).then(|| id - 1), false))
            .collect();
        let access = member_access(&nodes);
        let path = |id: u128| ancestors(&access, access.node(Uuid::from_u128(id)).unwrap());
        assert!(path(1).is_empty());
        assert_eq!(path(3), [Uuid::from_u128(1), Uuid::from_u128(2)]);
        let deep = path(depth);
        assert_eq!(deep.len(), MAX_PATH);
        assert_eq!(deep.last(), Some(&Uuid::from_u128(depth - 1)));
        assert_eq!(
            deep.first(),
            Some(&Uuid::from_u128(depth - MAX_PATH as u128))
        );
    }

    /// Mesure (Task S5, Step 3 ; release, `--ignored`) : bâtir l'ensemble des pages permises, à
    /// chaque recherche, pour un compte qui voit 10 000 puis 100 000 pages (Ruling R927 : un
    /// cache par `(espace, compte, rev)` ne s'écrit que si cette part passe 10 ms au p95). Une
    /// page sur dix est à la corbeille.
    #[test]
    #[ignore = "mesure, en release : cargo test --release -p mementum-server --lib \
                measure_the_allowed_set -- --ignored --nocapture"]
    fn measure_the_allowed_set() {
        const ROUNDS: usize = 200;
        let mut said = Vec::new();
        for pages in [10_000u128, 100_000] {
            let mut nodes = vec![node(1, None, false)];
            nodes.extend((0..pages).map(|i| node(2 + i, Some(1), i % 10 == 0)));
            let access = member_access(&nodes);
            let mut times = Vec::with_capacity(ROUNDS);
            for _ in 0..ROUNDS {
                let started = RealInstant::now();
                let allowed = allowed_pages(&access, None);
                times.push(started.elapsed());
                assert_eq!(allowed.len() as u128, 1 + pages - pages / 10);
            }
            times.sort();
            said.push(format!(
                "{pages} pages : p50 {:.2} ms, p95 {:.2} ms",
                times[ROUNDS / 2].as_secs_f64() * 1e3,
                times[(ROUNDS * 95).div_ceil(100) - 1].as_secs_f64() * 1e3
            ));
        }
        println!("MESURE measure_the_allowed_set : {}", said.join(" ; "));
    }
}
