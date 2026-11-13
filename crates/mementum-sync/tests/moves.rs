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
//!   et les réécrit avec `EXPORT_FIXTURES=1`. Depuis le correctif de y-prosemirror (Rulings R220
//!   et R223), le bloc glissé y est supprimé et recréé au même `id`, et les autres ne sont pas
//!   touchés ; avant lui, les éléments entre l'ancienne place et la nouvelle étaient récrits sur
//!   place. Pas bâtis avec yrs : une insertion placée après un caractère supprimé change la
//!   fusion, et rien ne dit que yrs la place comme Yjs.
//!
//! Chaque test dit ce qui est voulu : chaque bloc une seule fois, X juste sous Z, le texte de Tom
//! dans le bloc où il l'a tapé et nulle part ailleurs, chez Mona, chez Tom, dans le document de
//! l'acteur et dans le journal relu. Chacun vérifie d'abord, par le même chemin, que le
//! déplacement seul et la frappe seule font ce qu'ils disent.
//!
//! Ceux du module `known_limits` affirment une **limite connue** (Ruling R340,
//! `common/limit.rs`) : ce qui est voulu n'est pas tenu aujourd'hui. Ils sont exécutés à chaque
//! passage, et passent tant que la perte dite, mot pour mot, se voit aux quatre endroits ; ils
//! échouent le jour où elle est levée (la Task 13b sort alors le test du module et lui fait
//! affirmer la page voulue), et pour toute autre issue : une précondition manquée, une page qui
//! ne converge pas, une autre perte. Aucun n'est `#[ignore]`, marque réservée aux mesures :
//! ignorés, ils échouaient par construction dans le job `budgets` de la CI, qui lance tout ce qui
//! l'est.
//!
//! Ce qu'ils montrent est au registre du lot 3, sous « Task 12 », puis sous « La CI remise au
//! vert ».

mod common;

use common::limit::known_limit;
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

/// Une page telle qu'on la compare : `(id, texte)` de chaque bloc de premier niveau, dans l'ordre.
type Page = Vec<(String, String)>;

/// Les quatre endroits où la page est relue, dans l'ordre de [`Scene::play`].
const PLACES: [&str; 4] = [
    "chez Mona",
    "chez Tom",
    "dans le document de l'acteur",
    "dans le journal relu",
];

/// Pourquoi la frappe se perd quand le déplacement est une copie au même id puis la suppression
/// de l'original (Tab, Maj+Tab, les enfants d'un bloc glissé) : elle est faite dans l'original,
/// qui part avec elle.
const LOST_WITH_THE_ORIGINAL: &str = "R39 : la frappe faite dans un bloc qu'un autre copie puis \
     supprime part avec l'original ; à lever par la Task 13b";

/// Pourquoi la frappe se perd dans un bloc glissé par un navigateur : depuis le Ruling R220, il
/// est supprimé et recréé au même id, et la frappe est faite dans l'élément supprimé.
const LOST_WITH_THE_DRAGGED_BLOCK: &str = "R39 et R220 : la frappe faite dans un bloc qu'un autre \
     glisse part avec l'élément supprimé ; à lever par la Task 13b";

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
    /// Mona, Tom, l'acteur et le journal relu ([`PLACES`]).
    async fn play(&self, steps: &[Step]) -> Vec<(&'static str, Page)> {
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
        let seen = [
            self.mona.blocks(),
            self.tom.blocks(),
            self.f.hub.read_blocks(self.f.page).await.unwrap(),
            stored.read_blocks(),
        ];
        PLACES
            .into_iter()
            .zip(seen.iter().map(|blocks| summary(blocks)))
            .collect()
    }

    /// `(id, texte)` attendus, dans l'ordre de la page.
    fn expected(&self, wanted: [(&String, &str); 3]) -> Page {
        wanted
            .into_iter()
            .map(|(id, text)| (id.clone(), text.to_string()))
            .collect()
    }
}

/// Les blocs de premier niveau : `(id, texte)`, dans l'ordre de la page.
fn summary(blocks: &[Block]) -> Page {
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
    for (who, page) in s.play(&[Step::Move]).await {
        assert_eq!(page, wanted, "déplacement seul, {who}");
    }

    let s = scene().await;
    let Ids { x, y, z } = &s.ids;
    let mut wanted = s.expected([(x, "Bloc X"), (y, "Bloc Y"), (z, "Bloc Z")]);
    let at = wanted
        .iter()
        .position(|(id, _)| id == typed(&s.ids))
        .unwrap();
    wanted[at].1.push_str(TYPED);
    for (who, page) in s.play(&[Step::Edit]).await {
        assert_eq!(page, wanted, "frappe seule, {who}");
    }
}

/// La page `page` vue aux quatre endroits : ce que [`Scene::play`] rendrait si elle y était
/// partout. Sert à dire la perte d'une limite connue avec les mots de [`check_wanted`].
fn everywhere(page: &Page) -> Vec<(&'static str, Page)> {
    PLACES
        .into_iter()
        .map(|place| (place, page.clone()))
        .collect()
}

/// Ce que la Task 13 doit rendre vrai, partout (`seen`), après `order` : la page `wanted`, Tom
/// ayant tapé dans le bloc `typed`. Une ligne par exigence qui manque, dans `failures`.
fn check_wanted(
    s: &Scene,
    order: &str,
    seen: &[(&str, Page)],
    wanted: &[(String, String)],
    typed: &str,
    failures: &mut Vec<String>,
) {
    let Ids { x, z, .. } = &s.ids;
    for (where_, page) in seen {
        let who = format!("{order}, {where_}");
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
        if page.as_slice() != wanted {
            failures.push(format!(
                "{who} : la page n'est pas celle voulue (vu : {page:?}, voulu : {wanted:?})"
            ));
        }
    }
}

/// Le glisser de X sous Z, et la frappe de Tom dans Y, que personne ne déplace, tels qu'un
/// navigateur les écrit, dans les deux ordres d'arrivée : exactement
/// `[y « Bloc Y de Tom », z « Bloc Z », x « Bloc X »]`. Une limite jusqu'au correctif de
/// y-prosemirror (Ruling R220) : le glisser récrivait Y sur place, et le texte de Tom passait
/// dans Z. Il ne touche plus aux blocs qu'il croise.
#[tokio::test]
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

/// Les limites connues (Ruling R340) : ce que chaque test dit voulu n'est pas tenu, et il
/// l'affirme. Il passe tant que la perte dite se voit aux quatre endroits, mot pour mot, et
/// échoue le jour où elle est levée : le sortir alors de ce module, et remplacer `known_limit`
/// par `assert!(failures.is_empty(), …)`.
mod known_limits {
    use super::*;

    /// Le déplacement arrive d'abord, la frappe de Tom ensuite : Tom écrivait hors ligne, et
    /// revient après que Mona a déplacé le bloc. Copie et suppression, bâties avec yrs.
    ///
    /// Voulu : `[y « Bloc Y », z « Bloc Z », x « Bloc X de Tom »]`. Aujourd'hui, le texte de Tom
    /// n'est nulle part : X est juste sous Z, sans lui.
    #[tokio::test]
    async fn edit_arriving_after_the_move_survives() {
        controls(Scene::built, |ids| &ids.x).await;
        let s = Scene::built().await;
        let Ids { x, y, z } = &s.ids;
        let wanted = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X de Tom")]);
        let lost = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X")]);
        let seen = s.play(&[Step::Move, Step::Edit]).await;
        let (mut failures, mut loss) = (Vec::new(), Vec::new());
        check_wanted(&s, ORDERS[0].0, &seen, &wanted, x, &mut failures);
        check_wanted(&s, ORDERS[0].0, &everywhere(&lost), &wanted, x, &mut loss);
        known_limit(LOST_WITH_THE_ORIGINAL, &failures, &loss);
    }

    /// La frappe de Tom arrive d'abord, le déplacement ensuite : Mona déplaçait hors ligne, et
    /// revient après que Tom a écrit dans le bloc. Copie et suppression, bâties avec yrs.
    ///
    /// Voulu : `[y « Bloc Y », z « Bloc Z », x « Bloc X de Tom »]`. Aujourd'hui, le texte de Tom
    /// n'est nulle part : X est juste sous Z, sans lui.
    #[tokio::test]
    async fn move_arriving_after_the_edit_keeps_the_edit() {
        controls(Scene::built, |ids| &ids.x).await;
        let s = Scene::built().await;
        let Ids { x, y, z } = &s.ids;
        let wanted = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X de Tom")]);
        let lost = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X")]);
        let seen = s.play(&[Step::Edit, Step::Move]).await;
        let (mut failures, mut loss) = (Vec::new(), Vec::new());
        check_wanted(&s, ORDERS[1].0, &seen, &wanted, x, &mut failures);
        check_wanted(&s, ORDERS[1].0, &everywhere(&lost), &wanted, x, &mut loss);
        known_limit(LOST_WITH_THE_ORIGINAL, &failures, &loss);
    }

    /// Le glisser de X sous Z et la frappe de Tom dans X, tels qu'un navigateur les écrit, dans
    /// les deux ordres d'arrivée.
    ///
    /// Voulu : exactement `[y « Bloc Y », z « Bloc Z », x « Bloc X de Tom »]`. Aujourd'hui, dans
    /// les deux ordres, le texte de Tom n'est nulle part : X est juste sous Z, sans lui (avant le
    /// Ruling R220, il passait dans Y).
    #[tokio::test]
    async fn browser_move_keeps_the_text_typed_in_the_moved_block() {
        let scene = || Scene::browser(BROWSER_TYPED_IN_X);
        controls(scene, |ids| &ids.x).await;
        let (mut failures, mut loss) = (Vec::new(), Vec::new());
        for (order, steps) in ORDERS {
            let s = scene().await;
            let Ids { x, y, z } = &s.ids;
            let wanted = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X de Tom")]);
            let lost = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X")]);
            let seen = s.play(&steps).await;
            check_wanted(&s, order, &seen, &wanted, x, &mut failures);
            check_wanted(&s, order, &everywhere(&lost), &wanted, x, &mut loss);
        }
        known_limit(LOST_WITH_THE_DRAGGED_BLOCK, &failures, &loss);
    }
}

/// La cible elle-même : seule la perte dite, mot pour mot, passe pour une limite connue.
mod the_limit_itself {
    use super::known_limit;

    fn lines(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    /// La perte dite de ces essais : le texte de Tom manque, chez Mona et chez Tom.
    fn loss() -> Vec<String> {
        lines(&[
            "chez Mona : le texte de Tom n'y est pas",
            "chez Tom : le texte de Tom n'y est pas",
        ])
    }

    #[test]
    fn the_stated_loss_is_the_limit() {
        known_limit("essai", &loss(), &loss());
    }

    #[test]
    #[should_panic(expected = "limite levée (essai)")]
    fn a_lifted_limit_fails() {
        known_limit("essai", &[], &loss());
    }

    #[test]
    #[should_panic(expected = "pas la perte dite (essai)")]
    fn another_loss_fails() {
        let unmet = lines(&[
            "chez Mona : le texte de Tom est dans un autre bloc",
            "chez Tom : le texte de Tom est dans un autre bloc",
        ]);
        known_limit("essai", &unmet, &loss());
    }

    /// Une perte levée à moitié (chez Mona, pas chez Tom) n'est plus la perte dite.
    #[test]
    #[should_panic(expected = "pas la perte dite (essai)")]
    fn a_part_of_the_loss_fails() {
        known_limit("essai", &loss()[1..], &loss());
    }

    /// Les mêmes manques dans un autre ordre : ce n'est pas mot pour mot.
    #[test]
    #[should_panic(expected = "pas la perte dite (essai)")]
    fn the_loss_in_another_order_fails() {
        let unmet: Vec<String> = loss().into_iter().rev().collect();
        known_limit("essai", &unmet, &loss());
    }

    /// Un test dont la perte dite est vide (sa page perdue vaut sa page voulue) n'affirme rien.
    #[test]
    #[should_panic(expected = "limite sans perte dite (essai)")]
    fn a_limit_without_a_loss_fails() {
        known_limit("essai", &loss(), &[]);
    }
}
