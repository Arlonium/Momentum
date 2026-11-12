//! Édition faite pendant le déplacement d'un bloc (limite R39 du lot 1, que la Task 13 lève,
//! Ruling R138), vue par l'acteur d'une page. Mona déplace le bloc X juste sous le bloc Z ; Tom
//! tape « de Tom », chacun depuis le même état. Chaque mise à jour part par la connexion de son
//! auteur, telle qu'un client qui revient en ligne l'envoie, et l'acteur la traite avant la
//! suivante, dans les deux ordres d'arrivée.
//!
//! Deux formes de déplacement :
//!
//! - **une copie au même id, puis la suppression de l'original**, bâtie par `PageDoc::move_block`
//!   (avec `PageDoc::replace_text` pour la frappe, comme `mementum-crdt/tests/move_edit.rs`).
//!   C'est ce qu'écrit un navigateur pour Tab, pour Maj+Tab, et pour les enfants d'un bloc glissé
//!   (scénarios S9, S10 et S3 de `apps/web/src/editor/blockMoves.test.ts`). Ce que ces deux tests
//!   ne reproduisent pas, c'est le geste : le déplacement d'un bloc au même niveau, qu'un
//!   navigateur écrit autrement ;
//! - **le glisser tel qu'un navigateur l'écrit** : les octets de `tests/fixtures/moves/`, écrits
//!   par y-prosemirror sous un vrai éditeur BlockNote 0.57, aux identifiants de client fixes, par
//!   la suite Vitest `apps/web/src/editor/blockMoves.test.ts`, qui les compare à chaque exécution
//!   et les réécrit avec `EXPORT_FIXTURES=1`. Les éléments entre l'ancienne place et la nouvelle
//!   y sont récrits sur place (leurs `id` et leurs textes changent d'élément), sans copie ni
//!   suppression d'élément. Pas bâtis avec yrs : une insertion placée après un caractère
//!   supprimé change la fusion, et rien ne dit que yrs la place comme Yjs.
//!
//! Ces tests disent ce qui est voulu, pas ce qui se passe aujourd'hui : chaque bloc une seule
//! fois, X juste sous Z, le texte de Tom dans le bloc où il l'a tapé et nulle part ailleurs, chez
//! Mona, chez Tom, dans le document de l'acteur et dans le journal relu. Chacun vérifie d'abord,
//! par le même chemin, que le déplacement seul et la frappe seule font ce qu'ils disent.
//!
//! Ce qu'ils montrent aujourd'hui est au registre du lot 3, sous « Task 12 ».

mod common;

use common::*;
use mementum_md::{Block, block_text};
use mementum_store::{NewNode, NodeKind, Store};
use mementum_sync::{Hub, SyncConfig};

/// La page de départ des tests bâtis avec yrs.
const START: &str = "Bloc X\n\nBloc Y\n\nBloc Z\n";

/// Ce que Tom tape.
const TYPED: &str = " de Tom";

/// La page de départ des octets du navigateur : « Bloc X, Bloc Y, Bloc Z », ids `x`, `y`, `z`.
const BROWSER_BASE: &[u8] = include_bytes!("fixtures/moves/base.bin");
/// Mona glisse X sous Z, dans un navigateur.
const BROWSER_MOVE: &[u8] = include_bytes!("fixtures/moves/mona-drags-x-under-z.bin");
/// Tom tape « de Tom » à la fin de X, dans un navigateur.
const BROWSER_TYPED_IN_X: &[u8] = include_bytes!("fixtures/moves/tom-types-in-x.bin");
/// Tom tape « de Tom » à la fin de Y, dans un navigateur.
const BROWSER_TYPED_IN_Y: &[u8] = include_bytes!("fixtures/moves/tom-types-in-y.bin");

/// Une mise à jour du scénario.
#[derive(Clone, Copy)]
enum Step {
    /// Mona déplace X juste sous Z.
    Move,
    /// Tom tape « de Tom ».
    Edit,
}

/// Les deux ordres d'arrivée, nommés pour les messages.
const ORDERS: [(&str, [Step; 2]); 2] = [
    ("le déplacement arrive d'abord", [Step::Move, Step::Edit]),
    ("la frappe arrive d'abord", [Step::Edit, Step::Move]),
];

/// Ids des blocs X, Y et Z.
struct Ids {
    x: String,
    y: String,
    z: String,
}

/// Une page « Bloc X, Bloc Y, Bloc Z », Mona et Tom connectés, et leurs deux mises à jour,
/// faites depuis le même état, que personne n'a encore envoyées.
struct Scene {
    f: Fixture,
    mona: Client,
    tom: Client,
    ids: Ids,
    moved: Vec<u8>,
    edited: Vec<u8>,
}

impl Scene {
    /// Les mises à jour bâties avec yrs : `move_block` (copie et suppression) et `replace_text`
    /// à la fin de X.
    async fn built() -> Scene {
        let f = Fixture::new(SyncConfig::default()).await;
        f.write_mmd(START).await;
        let read = f.ids().await;
        let ids = Ids {
            x: read[0].clone(),
            y: read[1].clone(),
            z: read[2].clone(),
        };
        let mona = f.client().await;
        let tom = f.client().await;
        let moved = mona
            .page()
            .unwrap()
            .move_block(&ids.x, Some(&ids.z))
            .unwrap();
        let edited = tom
            .page()
            .unwrap()
            .replace_text(&ids.x, 6, 0, TYPED)
            .unwrap();
        Scene {
            f,
            mona,
            tom,
            ids,
            moved,
            edited,
        }
    }

    /// Les octets du navigateur : la page de départ, écrite telle quelle comme document de la
    /// page, le glisser de Mona, et la frappe de Tom `typed`.
    async fn browser(typed: &[u8]) -> Scene {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("mementum.db");
        let store = Store::open(&db).await.unwrap();
        let space = store.create_space("Essais", None).await.unwrap();
        let page = store
            .create_node(NewNode {
                space_id: space.id,
                parent_id: None,
                after: None,
                kind: NodeKind::Page,
                title: "Page".into(),
                created_by: None,
                grants: Vec::new(),
                doc: Some(BROWSER_BASE.to_vec()),
            })
            .await
            .unwrap()
            .id;
        let hub = Hub::new(store.clone(), SyncConfig::default());
        let server = TestServer::start(hub.clone()).await;
        let f = Fixture {
            dir,
            db,
            store,
            hub,
            server,
            page,
        };
        let mona = f.client().await;
        let tom = f.client().await;
        let ids = Ids {
            x: "x".into(),
            y: "y".into(),
            z: "z".into(),
        };
        Scene {
            f,
            mona,
            tom,
            ids,
            moved: BROWSER_MOVE.to_vec(),
            edited: typed.to_vec(),
        }
    }

    /// Envoie `steps` dans l'ordre, chacune par la connexion de son auteur, telle qu'elle a été
    /// faite : c'est ce qu'envoie un client qui revient en ligne (sa réponse à l'étape 1 du
    /// serveur ne porte que ce que le serveur n'a pas). L'acteur traite chacune avant la
    /// suivante ; son auteur reçoit en retour l'état du serveur, comme à une reconnexion. Attend
    /// ensuite que Mona et Tom aient tout ce que le serveur a écrit, puis rend ce que voient
    /// Mona, Tom, l'acteur et le journal relu.
    async fn play(&self, steps: &[Step]) -> Vec<(&'static str, Vec<Block>)> {
        for step in steps {
            let (author, update) = match step {
                Step::Move => (&self.mona, &self.moved),
                Step::Edit => (&self.tom, &self.edited),
            };
            author.send_update(update);
            processed(author).await;
        }
        let stored = stored_page(&self.f.store, self.f.page).await;
        let revision = stored.revision();
        for (who, client) in [("Mona", &self.mona), ("Tom", &self.tom)] {
            assert!(
                client
                    .wait_until(WAIT, |c| c.page().is_ok_and(|p| p.revision() == revision))
                    .await,
                "{who} n'a pas reçu tout ce que le serveur a écrit"
            );
        }
        vec![
            ("chez Mona", self.mona.blocks()),
            ("chez Tom", self.tom.blocks()),
            (
                "dans le document de l'acteur",
                self.f.hub.read_blocks(self.f.page).await.unwrap(),
            ),
            ("dans le journal relu", stored.read_blocks()),
        ]
    }

    /// `(id, texte)` attendus, dans l'ordre de la page.
    fn expected(&self, wanted: [(&String, &str); 3]) -> Vec<(String, String)> {
        wanted
            .into_iter()
            .map(|(id, text)| (id.clone(), text.to_string()))
            .collect()
    }
}

/// Les blocs de premier niveau : `(id, texte)`, dans l'ordre de la page.
fn summary(blocks: &[Block]) -> Vec<(String, String)> {
    blocks
        .iter()
        .map(|b| (b.id.clone(), block_text(b)))
        .collect()
}

/// Le déplacement seul, puis la frappe seule, chacun sur une page à lui et par le chemin du
/// scénario (`scene`) : chacun fait ce qu'il dit, partout. Un échec du scénario ne vient donc ni
/// d'une mise à jour mal faite ni d'une page qui n'a pas convergé. `typed` : le bloc où Tom tape.
async fn controls<F, Fut>(scene: F, typed: impl Fn(&Ids) -> &String)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Scene>,
{
    let s = scene().await;
    let Ids { x, y, z } = &s.ids;
    let wanted = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X")]);
    for (who, blocks) in s.play(&[Step::Move]).await {
        assert_eq!(summary(&blocks), wanted, "déplacement seul, {who}");
    }

    let s = scene().await;
    let Ids { x, y, z } = &s.ids;
    let mut wanted = s.expected([(x, "Bloc X"), (y, "Bloc Y"), (z, "Bloc Z")]);
    let at = wanted
        .iter()
        .position(|(id, _)| id == typed(&s.ids))
        .unwrap();
    wanted[at].1.push_str(TYPED);
    for (who, blocks) in s.play(&[Step::Edit]).await {
        assert_eq!(summary(&blocks), wanted, "frappe seule, {who}");
    }
}

/// Ce que la Task 13 doit rendre vrai, partout (`seen`), après `order` : la page `wanted`, Tom
/// ayant tapé dans le bloc `typed`. Une ligne par exigence qui manque, dans `failures`.
fn check_wanted(
    s: &Scene,
    order: &str,
    seen: &[(&str, Vec<Block>)],
    wanted: &[(String, String)],
    typed: &str,
    failures: &mut Vec<String>,
) {
    let Ids { x, z, .. } = &s.ids;
    for (where_, blocks) in seen {
        let who = format!("{order}, {where_}");
        let page = summary(blocks);
        let texts: Vec<&str> = page.iter().map(|(_, text)| text.as_str()).collect();
        for (id, text) in wanted {
            let found: Vec<&str> = page
                .iter()
                .filter(|(i, _)| i == id)
                .map(|(_, t)| t.as_str())
                .collect();
            if found.len() != 1 {
                failures.push(format!(
                    "{who} : le bloc {id} est dans la page {} fois, pas une (vu : {texts:?})",
                    found.len()
                ));
            } else if found[0] != text {
                let what = if id == typed {
                    "le texte de Tom n'y est pas"
                } else {
                    "son texte a changé"
                };
                failures.push(format!(
                    "{who} : le bloc {id} porte {:?}, pas {text:?} ({what})",
                    found[0]
                ));
            }
        }
        let above = page
            .iter()
            .position(|(id, _)| id == x)
            .and_then(|at| at.checked_sub(1))
            .map(|at| &page[at].0);
        if above != Some(z) {
            failures.push(format!("{who} : X n'est pas juste sous Z (vu : {texts:?})"));
        }
        let elsewhere: Vec<&str> = page
            .iter()
            .filter(|(id, text)| id != typed && text.contains(TYPED.trim()))
            .map(|(_, text)| text.as_str())
            .collect();
        if !elsewhere.is_empty() {
            failures.push(format!(
                "{who} : le texte de Tom est dans un autre bloc : {elsewhere:?}"
            ));
        }
        if page != wanted {
            failures.push(format!(
                "{who} : la page n'est pas celle voulue (vu : {page:?}, voulu : {wanted:?})"
            ));
        }
    }
}

/// Le déplacement arrive d'abord, la frappe de Tom ensuite : Tom écrivait hors ligne, et revient
/// après que Mona a déplacé le bloc. Copie et suppression, bâties avec yrs.
#[tokio::test]
#[ignore = "perte connue R39, levée par la Task 13"]
async fn edit_arriving_after_the_move_survives() {
    controls(Scene::built, |ids| &ids.x).await;
    let s = Scene::built().await;
    let Ids { x, y, z } = &s.ids;
    let wanted = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X de Tom")]);
    let seen = s.play(&[Step::Move, Step::Edit]).await;
    let mut failures = Vec::new();
    check_wanted(&s, ORDERS[0].0, &seen, &wanted, x, &mut failures);
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// La frappe de Tom arrive d'abord, le déplacement ensuite : Mona déplaçait hors ligne, et
/// revient après que Tom a écrit dans le bloc. Copie et suppression, bâties avec yrs.
#[tokio::test]
#[ignore = "perte connue R39, levée par la Task 13"]
async fn move_arriving_after_the_edit_keeps_the_edit() {
    controls(Scene::built, |ids| &ids.x).await;
    let s = Scene::built().await;
    let Ids { x, y, z } = &s.ids;
    let wanted = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X de Tom")]);
    let seen = s.play(&[Step::Edit, Step::Move]).await;
    let mut failures = Vec::new();
    check_wanted(&s, ORDERS[1].0, &seen, &wanted, x, &mut failures);
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Le glisser de X sous Z et la frappe de Tom dans X, tels qu'un navigateur les écrit, dans les
/// deux ordres d'arrivée : exactement `[y « Bloc Y », z « Bloc Z », x « Bloc X de Tom »]`.
#[tokio::test]
#[ignore = "perte connue R39, levée par la Task 13"]
async fn browser_move_keeps_the_text_typed_in_the_moved_block() {
    let scene = || Scene::browser(BROWSER_TYPED_IN_X);
    controls(scene, |ids| &ids.x).await;
    let mut failures = Vec::new();
    for (order, steps) in ORDERS {
        let s = scene().await;
        let Ids { x, y, z } = &s.ids;
        let wanted = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X de Tom")]);
        let seen = s.play(&steps).await;
        check_wanted(&s, order, &seen, &wanted, x, &mut failures);
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Le glisser de X sous Z, et la frappe de Tom dans Y, que personne ne déplace mais que le
/// glisser récrit, tels qu'un navigateur les écrit, dans les deux ordres d'arrivée : exactement
/// `[y « Bloc Y de Tom », z « Bloc Z », x « Bloc X »]`.
#[tokio::test]
#[ignore = "perte connue R39, levée par la Task 13"]
async fn browser_move_keeps_the_text_typed_in_a_block_it_shifts() {
    let scene = || Scene::browser(BROWSER_TYPED_IN_Y);
    controls(scene, |ids| &ids.y).await;
    let mut failures = Vec::new();
    for (order, steps) in ORDERS {
        let s = scene().await;
        let Ids { x, y, z } = &s.ids;
        let wanted = s.expected([(y, "Bloc Y de Tom"), (z, "Bloc Z"), (x, "Bloc X")]);
        let seen = s.play(&steps).await;
        check_wanted(&s, order, &seen, &wanted, y, &mut failures);
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
