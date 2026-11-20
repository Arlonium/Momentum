//! Route des rétroliens d'une page (lot 4, Task S6 ; plan du lot, « La requête et les droits ») :
//! les pages qui mènent à une page, par un lien de page, une mention de page, ou un lien du texte
//! vers son adresse, telles que l'indexeur les a projetées dans `page_links`.
//!
//! | Route | Il faut | Réponse |
//! |---|---|---|
//! | `GET /api/v1/pages/{id}/backlinks` | `read` (à la corbeille aussi) | [`Backlinks`] : les pages qui y mènent, que le compte voit, de l'espace de la page, hors corbeille, dans l'ordre de leurs identifiants, deux cents au plus ; jamais avant 75 ms ([`BACKLINKS_FLOOR`]) |
//!
//! **Refus**, dans l'ordre des pages : `401` sans session ; `409 account_changed` pour un autre
//! compte nommé ; `404 not_found` pour une page que la personne ne voit pas, exactement la réponse
//! d'une page qui n'existe pas (identifiant mal formé compris). Rien d'autre : toute page qu'on
//! voit se lit, donc jamais `403` ; la route n'écrit rien, donc jamais `409 trashed`.
//!
//! **Une page qu'on ne voit pas ne se devine ni ne se compte** (Review Focus 1 du plan). Le store
//! rend les sources sans droits ni corbeille (`page_links.target_id` n'a pas de clé étrangère, et
//! tout compte peut écrire, dans une page à lui, des liens vers l'identifiant d'une page d'un
//! autre). La route en garde ce que `access.rs` montre au compte, hors corbeille, **avant** de
//! compter les deux cents qu'elle rend : borner d'abord laisserait trois cents sources non vues
//! cacher les vraies (`unseen_sources_never_hide_seen_ones`). La réponse ne dit ni combien de
//! sources ont été lues, ni s'il en reste : elle est la même quelles que soient les sources non
//! vues, **dans ses octets** (`the_answer_is_the_same_whatever_unseen_sources_exist`, jusqu'à la
//! borne du parcours, `the_walk_stops_at_its_bound`) **et dans son temps, tant que le calcul tient
//! sous le plancher** (Ruling R1022) : lire une source non vue coûte quelques microsecondes
//! (2,7 µs, mesure du Step 3), si bien que la durée d'une lecture comptait les sources qu'on ne voit
//! pas ; la réponse d'une lecture qui passe ne part donc jamais avant [`BACKLINKS_FLOOR`] (150 ms)
//! après le début du traitement (`an_answer_never_leaves_before_the_floor`). Un refus (`401`,
//! `409`, `404`) ne lit aucune source et ne l'attend pas. **Le plancher et la borne du
//! parcours sont arrêtés ensemble** (Task X2, Ruling R1167) : au pire, sur le poste de mesure,
//! dix mille sources non vues se lisent en 24 ms au p95, et deux cents sources vues dont le bloc
//! qui mène fait 64 Kio (qu'un compte peut poser lui-même, pour pousser la lecture près du
//! plancher) en 62 ms : 86 ms ensemble, sous le plancher jusque sur un poste 1,7 fois plus lent.
//! **Limite** : sur un poste plus lent encore, une telle lecture dépasse le plancher, et sa durée
//! redit alors le nombre de sources lues.
//!
//! **Le parcours est borné** : les sources se lisent par lots de [`BATCH`], dans l'ordre de leurs
//! identifiants, chaque lot trié par droits, jusqu'à [`SHOWN`] sources vues ou [`WALKED`] sources
//! lues. Au-delà de la borne, des sources vues peuvent manquer sans que rien le dise (limite L-j du
//! plan, qu'on n'atteint qu'en créant dix mille pages dans l'espace de la page : le store ne lit
//! pas les liens d'un autre espace, R1015). Chaque source coûte une entrée de l'index, quel que
//! soit le nombre de ses liens (R1016), et l'extrait est coupé par la base : une lecture au pire
//! coûte ce que mesure `measure_backlinks_at_the_walk_bound` (R1019), sans débit propre.
//!
//! Le titre d'une source n'est pas dans la réponse : le client le lit dans l'arbre de l'espace,
//! qui montre les mêmes pages. L'extrait est le texte indexé du premier bloc qui mène à la page,
//! dans l'ordre des identifiants de bloc (R963) ; `blockId` est `null` pour une clé de
//! remplacement (`mementum_search::is_stand_in`), qui ne nomme pas un bloc qu'on rejoint.

use std::time::Duration;

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use mementum_acl::{Action, SpaceAccess};
use tokio::time::Instant;
use uuid::Uuid;

use crate::AppState;
use crate::access::permit;
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::routes::pages::page_id;
use crate::types::{Backlink, Backlinks};

/// Sources vues rendues au plus.
pub(crate) const SHOWN: usize = 200;

/// Sources lues au plus, vues ou non : au-delà, le parcours s'arrête (limite L-j).
pub(crate) const WALKED: usize = 10_000;

/// Sources lues par lot, chaque lot trié par droits avant le suivant.
pub(crate) const BATCH: usize = 500;

/// Caractères (points de code) de l'extrait, coupé par la base.
pub(crate) const EXCERPT_CHARS: usize = 240;

/// Les bornes d'une lecture : celles de la route ([`BOUNDS`]), plus basses dans les tests.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    shown: usize,
    walked: usize,
    batch: usize,
}

const BOUNDS: Bounds = Bounds {
    shown: SHOWN,
    walked: WALKED,
    batch: BATCH,
};

/// Le temps de réponse plancher d'une lecture qui passe (Ruling R1022) : sa réponse ne part
/// jamais avant ce délai, compté du début du traitement, que le parcours ait été long ou court.
pub(crate) const BACKLINKS_FLOOR: Duration = Duration::from_millis(150);

/// `GET /api/v1/pages/{id}/backlinks` : les pages qui mènent à la page, que le compte voit.
pub(crate) async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Backlinks>, ApiError> {
    let started = Instant::now();
    let id = page_id(path)?;
    Ok(Json(answered(&state, &user, id, BOUNDS, started).await?))
}

/// [`backlinks`], dont la réponse ne part pas avant [`BACKLINKS_FLOOR`] après `started` ; un
/// refus part tout de suite : il ne lit aucune source.
async fn answered(
    state: &AppState,
    user: &AuthUser,
    id: Uuid,
    bounds: Bounds,
    started: Instant,
) -> Result<Backlinks, ApiError> {
    let found = backlinks(state, user, id, bounds).await?;
    #[cfg(test)]
    tests::computed(id);
    // Sur le moteur, sans fil bloquant : une attente, pas un calcul.
    tokio::time::sleep_until(started + BACKLINKS_FLOOR).await;
    Ok(found)
}

/// Les rétroliens de la page `id` pour `user`, sous `bounds`.
async fn backlinks(
    state: &AppState,
    user: &AuthUser,
    id: Uuid,
    bounds: Bounds,
) -> Result<Backlinks, ApiError> {
    let (node, access, space) = state
        .seen_node(user, id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    // Lire ne se refuse pas à qui voit la page, à la corbeille comprise.
    permit(&access, Action::Read)?;
    let sources = seen_sources(state, &space, id, node.space_id, bounds).await?;
    let found = state
        .store
        .backlink_excerpts(id, node.space_id, &sources, EXCERPT_CHARS)
        .await?;
    let pages = found
        .into_iter()
        .map(|link| Backlink {
            page_id: link.node_id.to_string(),
            block_id: (!mementum_search::is_stand_in(&link.block_id)).then_some(link.block_id),
            excerpt: link.excerpt,
        })
        .collect();
    Ok(Backlinks { pages })
}

/// Les sources de `target` (de l'espace `space_id`) que `access` montre, hors corbeille, dans
/// l'ordre de leurs identifiants : lues par lots, chaque lot trié **avant** d'être compté, jusqu'à
/// `bounds.shown` sources gardées ou `bounds.walked` sources lues.
async fn seen_sources(
    state: &AppState,
    access: &SpaceAccess,
    target: Uuid,
    space_id: Uuid,
    bounds: Bounds,
) -> Result<Vec<Uuid>, ApiError> {
    let mut kept = Vec::new();
    let (mut walked, mut after) = (0, None);
    while walked < bounds.walked && kept.len() < bounds.shown {
        let limit = bounds.batch.min(bounds.walked - walked);
        let batch = state
            .store
            .backlink_sources(target, space_id, after, limit)
            .await?;
        walked += batch.len();
        kept.extend(
            batch
                .iter()
                .copied()
                .filter(|source| access.node(*source).is_some_and(|seen| !seen.in_trash)),
        );
        match batch.last() {
            Some(last) if batch.len() == limit => after = Some(*last),
            _ => break,
        }
    }
    kept.truncate(bounds.shown);
    Ok(kept)
}

/// L'horloge de tokio arrêtée (Ruling R179) : l'aide des tests de `mementum-sync`. Déclarée ici, et
/// pas dans le module de tests : le chemin d'un module déclaré dans un module en ligne part d'un
/// dossier `backlinks/tests/` qui n'existe pas, ce que Linux refuse.
#[cfg(test)]
#[path = "../../../mementum-sync/tests/common/clock.rs"]
mod clock;

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Instant;

    use mementum_acl::{Grant, Level, Role, Subject};
    use mementum_store::{IndexedBlock, NewNode, NodeKind};

    use super::*;
    use crate::access::tests::{account, state};

    /// Les cibles dont une lecture a fini son calcul et attend son plancher ([`answered`]) : un
    /// témoin sans minutage. Par cible, les tests du module tournant ensemble.
    static COMPUTED: Mutex<Vec<Uuid>> = Mutex::new(Vec::new());

    /// Noté par [`answered`], une fois le calcul fini, avant le plancher.
    pub(super) fn computed(target: Uuid) {
        COMPUTED.lock().unwrap().push(target);
    }

    fn was_computed(target: Uuid) -> bool {
        COMPUTED.lock().unwrap().contains(&target)
    }

    /// Attend `ready` en temps réel (dix secondes au plus), en cédant la main : sous l'horloge
    /// arrêtée, aucun délai de tokio ne finit. La borne peut faire échouer le test à tort sur un
    /// poste très chargé, jamais le faire passer.
    async fn until(what: &str, ready: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready() {
            assert!(Instant::now() < deadline, "jamais vu : {what}");
            tokio::task::yield_now().await;
        }
    }

    /// Cède la main assez pour que toute tâche prête ait tourné.
    async fn settle() {
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
    }

    /// Un espace où Olga (propriétaire) écrit et Mona (membre) lit, et une page « Cible » que
    /// les membres lisent.
    struct Bench {
        dir: tempfile::TempDir,
        state: AppState,
        olga: AuthUser,
        mona: AuthUser,
        space: Uuid,
        target: Uuid,
    }

    impl Bench {
        async fn new() -> Bench {
            let dir = tempfile::tempdir().unwrap();
            let state = state(&dir).await;
            let olga = account(&state, "olga").await;
            let mona = account(&state, "mona").await;
            let space = state
                .store
                .create_space("Équipe", Some(olga.id))
                .await
                .unwrap()
                .id;
            state
                .store
                .add_member(space, mona.id, Role::Member)
                .await
                .unwrap();
            let mut bench = Bench {
                dir,
                state,
                olga,
                mona,
                space,
                target: Uuid::nil(),
            };
            bench.target = bench.page(true).await;
            bench
        }

        /// Une page neuve de l'espace, à la racine : que les membres lisent (`seen`), ou privée
        /// d'Olga.
        async fn page(&self, seen: bool) -> Uuid {
            let subject = if seen {
                Subject::Members
            } else {
                Subject::User(self.olga.id)
            };
            self.state
                .store
                .create_node(NewNode {
                    space_id: self.space,
                    parent_id: None,
                    after: None,
                    kind: NodeKind::Page,
                    title: "Page".to_string(),
                    created_by: Some(self.olga.id),
                    grants: vec![Grant {
                        subject,
                        level: if seen { Level::Read } else { Level::Full },
                    }],
                    doc: None,
                })
                .await
                .unwrap()
                .id
        }

        /// `n` pages neuves, vues ou non.
        async fn pages(&self, n: usize, seen: bool) -> Vec<Uuid> {
            let mut pages = Vec::with_capacity(n);
            for _ in 0..n {
                pages.push(self.page(seen).await);
            }
            pages
        }

        /// La projection de `source` : ces blocs, `(clé, texte)`, chacun menant à la cible.
        async fn link(&self, source: Uuid, blocks: &[(&str, &str)]) {
            let targets = [self.target];
            let rows: Vec<IndexedBlock<'_>> = blocks
                .iter()
                .map(|(block_id, text)| IndexedBlock {
                    block_id,
                    kind: "paragraph",
                    text,
                    hash: text.as_bytes(),
                    links: &targets,
                })
                .collect();
            self.state
                .store
                .index_page(source, 1, &rows, false)
                .await
                .unwrap();
        }

        /// Chacune de ces pages mène à la cible par un bloc.
        async fn link_all(&self, sources: &[Uuid]) {
            for &source in sources {
                self.link(source, &[("b", "vers la cible")]).await;
            }
        }

        /// Ce que Mona lit des rétroliens de la cible, sous `bounds`.
        async fn read(&self, bounds: Bounds) -> Backlinks {
            backlinks(&self.state, &self.mona, self.target, bounds)
                .await
                .unwrap()
        }

        /// Les seules pages de ce que Mona lit.
        async fn sources(&self, bounds: Bounds) -> Vec<Uuid> {
            self.read(bounds)
                .await
                .pages
                .into_iter()
                .map(|page| page.page_id.parse().unwrap())
                .collect()
        }

        /// Ferme la base avant le dossier (Ruling R344).
        async fn finish(self) {
            self.state.store.close().await;
            drop(self.dir);
        }
    }

    /// Ruling R1022 : la réponse d'une lecture qui passe ne part pas avant le plancher, compté du
    /// début du traitement, que le parcours ait été long ou court : une page qu'une source mène
    /// et une page sans aucune source, lues au même instant sous l'horloge arrêtée, ont fini leur
    /// calcul (le témoin [`computed`]) et ne répondent ni l'une ni l'autre une milliseconde avant
    /// le plancher ; elles répondent toutes deux au plancher.
    #[tokio::test]
    async fn an_answer_never_leaves_before_the_floor() {
        let bench = Bench::new().await;
        let alone = bench.page(true).await;
        let source = bench.page(true).await;
        bench.link_all(&[source]).await;
        let clock = super::clock::StoppedClock::start();
        let started = tokio::time::Instant::now();
        let reads: Vec<_> = [bench.target, alone]
            .into_iter()
            .map(|target| {
                let (state, mona) = (bench.state.clone(), bench.mona.clone());
                tokio::spawn(async move {
                    answered(&state, &mona, target, BOUNDS, started)
                        .await
                        .unwrap()
                })
            })
            .collect();
        until("les deux calculs finis", || {
            was_computed(bench.target) && was_computed(alone)
        })
        .await;
        settle().await;
        assert!(reads.iter().all(|read| !read.is_finished()), "avant tout");
        tokio::time::advance(BACKLINKS_FLOOR - Duration::from_millis(1)).await;
        settle().await;
        assert!(
            reads.iter().all(|read| !read.is_finished()),
            "une milliseconde avant le plancher"
        );
        // Le minuteur de tokio arrondit une échéance à la milliseconde suivante : `started` porte
        // une fraction de milliseconde (vu en écrivant ce test : une seule de plus ne suffisait
        // pas).
        tokio::time::advance(Duration::from_millis(2)).await;
        until("les deux réponses au plancher", || {
            reads.iter().all(|read| read.is_finished())
        })
        .await;
        let mut found = Vec::new();
        for read in reads {
            found.push(read.await.unwrap().pages.len());
        }
        assert_eq!(found, [1, 0]);
        assert_eq!(BACKLINKS_FLOOR, Duration::from_millis(150));
        drop(clock);
        bench.finish().await;
    }

    /// Un refus ne lit aucune source, et part sans attendre le plancher : sous l'horloge arrêtée,
    /// le `404` d'une page inconnue et celui d'une page que Mona ne voit pas arrivent sans que
    /// l'horloge bouge.
    #[tokio::test]
    async fn a_refusal_does_not_wait_for_the_floor() {
        let bench = Bench::new().await;
        let hidden = bench.page(false).await;
        let clock = super::clock::StoppedClock::start();
        let started = tokio::time::Instant::now();
        for target in [Uuid::now_v7(), hidden] {
            let (state, mona) = (bench.state.clone(), bench.mona.clone());
            let read =
                tokio::spawn(async move { answered(&state, &mona, target, BOUNDS, started).await });
            until("le refus", || read.is_finished()).await;
            let refused = read.await.unwrap().expect_err("un refus");
            let response = axum::response::IntoResponse::into_response(refused);
            assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
        }
        assert_eq!(
            tokio::time::Instant::now(),
            started,
            "l'horloge n'a pas bougé"
        );
        drop(clock);
        bench.finish().await;
    }

    /// Le cœur de la route (Review Focus 1, 4) : trois cents sources que Mona ne voit pas, aux
    /// identifiants avant ceux de trois qu'elle voit, ne cachent pas ces trois-là. C'est ce qui
    /// distingue « trier par droits, puis borner » de « borner, puis trier » : les deux cents
    /// premières sources lues sont toutes des pages qu'elle ne voit pas.
    #[tokio::test]
    async fn unseen_sources_never_hide_seen_ones() {
        let bench = Bench::new().await;
        let unseen = bench.pages(300, false).await;
        let seen = bench.pages(3, true).await;
        assert!(unseen.iter().all(|u| seen.iter().all(|s| u < s)));
        bench.link_all(&unseen).await;
        bench.link_all(&seen).await;
        assert_eq!(bench.sources(BOUNDS).await, seen);
        bench.finish().await;
    }

    /// La réponse est la même, octet pour octet, quelles que soient les sources non vues : trois
    /// sources vues seules, puis avec deux cent cinquante non vues aux identifiants avant, entre et
    /// après les leurs (jusqu'à la borne du parcours).
    #[tokio::test]
    async fn the_answer_is_the_same_whatever_unseen_sources_exist() {
        let bench = Bench::new().await;
        let mut unseen = bench.pages(100, false).await;
        let first = bench.page(true).await;
        unseen.extend(bench.pages(50, false).await);
        let second = bench.page(true).await;
        let third = bench.page(true).await;
        unseen.extend(bench.pages(100, false).await);
        bench.link_all(&[first, second, third]).await;
        let alone = serde_json::to_vec(&bench.read(BOUNDS).await).unwrap();
        assert_eq!(bench.sources(BOUNDS).await, [first, second, third]);

        bench.link_all(&unseen).await;
        let crowded = serde_json::to_vec(&bench.read(BOUNDS).await).unwrap();
        assert_eq!(
            String::from_utf8(crowded).unwrap(),
            String::from_utf8(alone).unwrap()
        );
        bench.finish().await;
    }

    /// Deux cents sources vues au plus, les premières par identifiant. Témoin : la deux cent
    /// unième, que Mona voit, n'y est pas.
    #[tokio::test]
    async fn two_hundred_seen_sources_at_most() {
        let bench = Bench::new().await;
        let seen = bench.pages(201, true).await;
        bench.link_all(&seen).await;
        let sources = bench.sources(BOUNDS).await;
        assert_eq!(sources.len(), SHOWN);
        assert_eq!(sources, seen[..SHOWN]);
        assert_eq!(SHOWN, 200);
        bench.finish().await;
    }

    /// Le parcours s'arrête à sa borne (réglée bas, par lots de trois) : dix sources non vues,
    /// puis une vue, et une borne de dix sources lues : la vue n'est pas lue, et rien ne le dit,
    /// la réponse étant, octet pour octet, celle d'une page sans aucune source (la route ne pose
    /// aucun en-tête à elle : ceux de l'API sont les mêmes pour toutes). Témoin : à onze, elle
    /// l'est. Les lots se suivent sans répéter ni sauter une source.
    #[tokio::test]
    async fn the_walk_stops_at_its_bound() {
        let bench = Bench::new().await;
        let alone = bench.page(true).await;
        let unseen = bench.pages(10, false).await;
        let seen = bench.page(true).await;
        bench.link_all(&unseen).await;
        bench.link_all(&[seen]).await;
        let bounded = |walked| Bounds {
            shown: SHOWN,
            walked,
            batch: 3,
        };
        let stopped = serde_json::to_vec(&bench.read(bounded(10)).await).unwrap();
        let nothing = backlinks(&bench.state, &bench.mona, alone, bounded(10))
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8(stopped).unwrap(),
            String::from_utf8(serde_json::to_vec(&nothing).unwrap()).unwrap()
        );
        assert_eq!(nothing.pages, []);
        assert_eq!(bench.sources(bounded(11)).await, [seen]);
        assert_eq!(bench.sources(bounded(WALKED)).await, [seen]);
        assert_eq!((WALKED, BATCH), (10_000, 500));
        bench.finish().await;
    }

    /// Par lots de deux, des sources vues et non vues mêlées : chaque source vue une fois, dans
    /// l'ordre, comme sous les bornes de la route. Et la borne des sources vues coupe au milieu
    /// d'un lot.
    #[tokio::test]
    async fn batches_neither_repeat_nor_skip_a_source() {
        let bench = Bench::new().await;
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(bench.page(true).await);
            let unseen = bench.page(false).await;
            bench.link_all(&[unseen]).await;
        }
        bench.link_all(&seen).await;
        let pairs = Bounds {
            shown: SHOWN,
            walked: WALKED,
            batch: 2,
        };
        assert_eq!(bench.sources(pairs).await, seen);
        assert_eq!(bench.sources(BOUNDS).await, seen);
        let three = Bounds { shown: 3, ..pairs };
        assert_eq!(bench.sources(three).await, seen[..3]);
        bench.finish().await;
    }

    /// L'extrait est coupé par la base, à 240 caractères : un bloc de 64 Kio de « é » rend 240
    /// « é », pas son texte entier ; un texte court, tel quel.
    #[tokio::test]
    async fn the_excerpt_is_bounded_by_the_database() {
        let bench = Bench::new().await;
        let long = bench.page(true).await;
        let short = bench.page(true).await;
        let text = "é".repeat(32 * 1024);
        bench.link(long, &[("b", &text)]).await;
        bench.link(short, &[("b", "court")]).await;
        let pages = bench.read(BOUNDS).await.pages;
        let excerpts: Vec<&str> = pages.iter().map(|p| p.excerpt.as_str()).collect();
        assert_eq!(excerpts, ["é".repeat(EXCERPT_CHARS).as_str(), "court"]);
        assert_eq!(EXCERPT_CHARS, 240);
        bench.finish().await;
    }

    /// Une clé de remplacement (`#<rang>`) ne nomme pas un bloc qu'on rejoint : `blockId` est
    /// `null`. Une clé écrite par un client, guillemets et caractère de contrôle compris, est
    /// rendue telle quelle, échappée par JSON.
    #[tokio::test]
    async fn a_stand_in_key_is_not_a_block_to_reach() {
        let bench = Bench::new().await;
        let stand_in = bench.page(true).await;
        let odd = bench.page(true).await;
        bench.link(stand_in, &[("#2", "sans identifiant")]).await;
        bench.link(odd, &[("a\"<\u{1}", "étrange")]).await;
        let pages = bench.read(BOUNDS).await.pages;
        let blocks: Vec<Option<&str>> = pages.iter().map(|p| p.block_id.as_deref()).collect();
        assert_eq!(blocks, [None, Some("a\"<\u{1}")]);
        let json = serde_json::to_string(&pages[1]).unwrap();
        assert!(json.contains(r#""blockId":"a\"<\u0001""#), "{json}");
        bench.finish().await;
    }

    /// Mesure (Task S6, Step 3 ; release, `--ignored`) : une lecture au pire, la borne du
    /// parcours atteinte (dix mille sources que Mona ne voit pas, aucune rendue), puis deux
    /// cents sources vues (leurs extraits de 240 caractères). Elle dit si la route demande un
    /// débit à elle : un en faut un si elle coûte plus que la lecture des commentaires d'une page
    /// pleine (p50 356 ms, p95 378 ms, registre du lot 3). Les pages sont écrites d'un coup par une
    /// connexion à côté (des milliers de transactions du store prendraient des minutes) ; le calcul
    /// des accès de l'espace (10 402 pages) est fait une fois avant les tours mesurés, et dit à
    /// part.
    #[tokio::test]
    #[ignore = "mesure, en release : cargo test --release -p mementum-server --lib \
                measure_backlinks_at_the_walk_bound -- --ignored --nocapture"]
    async fn measure_backlinks_at_the_walk_bound() {
        const ROUNDS: usize = 40;
        let bench = Bench::new().await;
        // Les sources non vues sont sous une page privée d'Olga ; les vues, sous une page que
        // les membres lisent. Leurs identifiants passent tous avant ceux du store.
        let hidden = bench.page(false).await;
        let open = bench.page(true).await;
        let busy = bench.page(true).await;
        let unseen: Vec<Uuid> = (1..=WALKED as u128).map(Uuid::from_u128).collect();
        let seen: Vec<Uuid> = (1..=SHOWN as u128)
            .map(|i| Uuid::from_u128((1 << 64) + i))
            .collect();
        // Le bloc qui mène d'une source vue : 4 Ko, ou `MEMENTUM_MEASURE_BLOCK_BYTES` octets
        // (65 536 : la borne d'un bloc, Task X2).
        let seen_bytes = std::env::var("MEMENTUM_MEASURE_BLOCK_BYTES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(4_000);
        let (short, long) = ("é".repeat(2_000), "é".repeat(seen_bytes / 2));
        let db = bench.dir.path().join("m.db");
        let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
            .await
            .unwrap();
        let mut tx = side.begin().await.unwrap();
        let space = bench.space.to_string();
        for (sources, parent, target, text) in [
            (&unseen, hidden, bench.target, &short),
            (&seen, open, busy, &long),
        ] {
            for chunk in sources.chunks(200) {
                for (table, row, width) in [
                    (
                        "nodes (id, space_id, parent_id, kind, title, position, created_at, \
                         updated_at)",
                        "(?, ?, ?, 'page', 'Source', ?, 0, 0)",
                        4,
                    ),
                    (
                        "blocks_index (node_id, block_id, type, text, hash)",
                        "(?, 'b', 'paragraph', ?, x'00')",
                        2,
                    ),
                    (
                        "page_links (node_id, block_id, target_id, space_id)",
                        "(?, 'b', ?, ?)",
                        3,
                    ),
                ] {
                    let rows = vec![row; chunk.len()].join(", ");
                    let sql = format!("INSERT INTO {table} VALUES {rows}");
                    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
                    for source in chunk {
                        let id = source.to_string();
                        query = match width {
                            4 => query
                                .bind(id.clone())
                                .bind(space.clone())
                                .bind(parent.to_string())
                                .bind(id),
                            2 => query.bind(id).bind(text.clone()),
                            _ => query.bind(id).bind(target.to_string()).bind(space.clone()),
                        };
                    }
                    query.execute(&mut *tx).await.unwrap();
                }
            }
        }
        sqlx::query("UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = ?")
            .bind(&space)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        side.close().await;

        let started = Instant::now();
        assert_eq!(bench.read(BOUNDS).await.pages, []);
        let first = started.elapsed();
        let mut rounds = Vec::new();
        for (name, target, expected) in [
            ("sources non vues, à la borne du parcours", bench.target, 0),
            ("200 sources vues", busy, SHOWN),
        ] {
            let mut times = Vec::with_capacity(ROUNDS);
            for _ in 0..ROUNDS {
                let started = Instant::now();
                let found = backlinks(&bench.state, &bench.mona, target, BOUNDS)
                    .await
                    .unwrap();
                times.push(started.elapsed());
                assert_eq!(found.pages.len(), expected);
            }
            times.sort();
            let p50 = times[ROUNDS / 2];
            let p95 = times[(ROUNDS * 95).div_ceil(100) - 1];
            rounds.push(format!(
                "{name} : p50 {:.1} ms, p95 {:.1} ms",
                p50.as_secs_f64() * 1e3,
                p95.as_secs_f64() * 1e3
            ));
        }
        println!(
            "MESURE measure_backlinks_at_the_walk_bound ({WALKED} sources non vues, bloc d'une \
             source vue de {seen_bytes} octets) : {} ; première lecture (accès de l'espace \
             calculés) {:.1} ms",
            rounds.join(" ; "),
            first.as_secs_f64() * 1e3
        );
        bench.finish().await;
    }
}
