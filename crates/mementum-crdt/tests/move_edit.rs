//! Édition concurrente d'un bloc déplacé (Task 13b, Rulings R221 et R371 à R379).
//!
//! Yjs n'a pas de déplacement : un client écrit un bloc déplacé comme sa suppression et sa
//! recréation au même `id` (correctif de y-prosemirror, Ruling R220 ; Tab, Maj+Tab et
//! `PageDoc::move_block` l'écrivaient déjà ainsi). Ce qu'un autre a tapé dans l'original sans
//! que l'auteur du déplacement le voie part avec l'élément supprimé. Quand le déplacement arrive
//! au serveur **après** la frappe, `PageDoc::apply_client_update` le reporte sur la copie
//! (`Applied::carried`, module `moves`). Quand il arrive **avant**, la frappe reste perdue : la
//! reconnaître demanderait de lire les éléments supprimés de l'original, ce que yrs n'offre pas
//! (Ruling R222, limite affirmée plus bas).
//!
//! La plupart des tests rejouent les octets qu'écrit un vrai éditeur BlockNote 0.57 corrigé
//! (`crates/mementum-sync/tests/fixtures/moves/<ordre>/<scène>/`, écrits par
//! `apps/web/src/editor/blockMoves.test.ts`) : la page de départ, la mise à jour de Mona (qui
//! déplace) et celle de Tom (qui tape), chacune faite depuis la page de départ sans rien voir de
//! l'autre, **dans les deux ordres des identifiants de client** (`mona-first` : Mona 2 et Tom 3 ;
//! `tom-first` : l'inverse), qui départagent deux insertions au même endroit. Le serveur est ici
//! un `PageDoc` qui fait ce que fait l'acteur de `mementum-sync` (appliquer, dédoublonner), et
//! Mona et Tom deux `PageDoc` qui reçoivent ce qu'il diffuse : chaque page se lit chez les trois.

use std::path::PathBuf;

use mementum_crdt::PageDoc;
use mementum_md::{Block, Content, Inline, RenderOpts, block_text, parse, render};

/// L'affirmation d'une limite connue, partagée avec les tests de `mementum-sync`.
#[path = "../../mementum-sync/tests/common/limit.rs"]
mod limit;

/// Les deux ordres des identifiants de client des octets du navigateur.
const ORDERS: [&str; 2] = ["mona-first", "tom-first"];

/// Ce que Tom tape dans la plupart des scènes.
const TYPED: &str = " de Tom";

/// Un fichier des octets du navigateur.
fn fixture(order: &str, scene: &str, file: &str) -> Vec<u8> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "mementum-sync",
        "tests",
        "fixtures",
        "moves",
        order,
        scene,
        file,
    ]
    .iter()
    .collect();
    std::fs::read(&path).unwrap_or_else(|e| panic!("{} : {e}", path.display()))
}

/// Qui envoie.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Who {
    Mona,
    Tom,
}

/// Le déplacement arrive après la frappe : l'ordre que la Task 13b règle.
const MOVE_AFTER_EDIT: [Who; 2] = [Who::Tom, Who::Mona];
/// Le déplacement arrive avant la frappe : l'ordre de la limite R222.
const MOVE_BEFORE_EDIT: [Who; 2] = [Who::Mona, Who::Tom];

/// Une scène jouée : les trois documents, et les reports qu'a écrits le serveur.
struct Played {
    server: PageDoc,
    mona: PageDoc,
    tom: PageDoc,
    carried: Vec<Vec<u8>>,
}

impl Played {
    /// La même ligne chez les trois, ou un échec qui dit qui diffère.
    fn line(&self) -> String {
        let server = line(&self.server.read_blocks());
        assert_eq!(line(&self.mona.read_blocks()), server, "Mona et le serveur");
        assert_eq!(line(&self.tom.read_blocks()), server, "Tom et le serveur");
        server
    }
}

/// Ce que fait l'acteur d'une mise à jour d'un client : l'appliquer, puis dédoublonner ; ce
/// qu'il lit est relayé aux autres, le report et le correctif vont à tous.
fn receive(played: &mut Played, who: Who, update: &[u8]) {
    let applied = played
        .server
        .apply_client_update(update)
        .expect("mise à jour acceptée");
    let fix = played.server.dedupe_blocks();
    let other = match who {
        Who::Mona => &played.tom,
        Who::Tom => &played.mona,
    };
    other.apply_update(&applied.read).expect("relayée");
    for extra in applied.carried.iter().chain(fix.iter()) {
        played.mona.apply_update(extra).expect("chez Mona");
        played.tom.apply_update(extra).expect("chez Tom");
    }
    played.carried.extend(applied.carried);
}

/// Joue la scène `scene` des octets du navigateur, dans l'ordre d'identifiants `order`, les
/// mises à jour arrivant au serveur dans l'ordre `arrival`.
fn play(order: &str, scene: &str, arrival: [Who; 2]) -> Played {
    let base = fixture(order, scene, "base.bin");
    let mona = fixture(order, scene, "mona.bin");
    let tom = fixture(order, scene, "tom.bin");
    play_updates(&base, &mona, &tom, &arrival)
}

fn play_updates(base: &[u8], mona: &[u8], tom: &[u8], arrival: &[Who]) -> Played {
    let mut played = Played {
        server: PageDoc::from_updates([base]).unwrap(),
        mona: PageDoc::from_updates([base, mona]).unwrap(),
        tom: PageDoc::from_updates([base, tom]).unwrap(),
        carried: Vec::new(),
    };
    for who in arrival {
        let update = match who {
            Who::Mona => mona,
            Who::Tom => tom,
        };
        receive(&mut played, *who, update);
    }
    played
}

/// Une page en une ligne : `id « texte »` par bloc, le type s'il n'est pas un paragraphe, les
/// sous-blocs entre crochets (la forme de `line` dans `apps/web/src/test/editors.ts`).
fn line(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(|b| {
            let kind = if b.kind == "paragraph" {
                String::new()
            } else {
                format!(" ({})", b.kind)
            };
            let mut out = format!("{}{kind} « {} »", b.id, block_text(b));
            if !b.children.is_empty() {
                out.push_str(&format!(" [{}]", line(&b.children)));
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Le bloc `id`, à toute profondeur.
fn find<'a>(blocks: &'a [Block], id: &str) -> Option<&'a Block> {
    blocks
        .iter()
        .find_map(|b| (b.id == id).then_some(b).or_else(|| find(&b.children, id)))
}

/// Les deux ordres, chacun avec ce qu'il donne.
fn both_orders(scene: &str, arrival: [Who; 2]) -> Vec<(&'static str, Played)> {
    ORDERS
        .into_iter()
        .map(|order| (order, play(order, scene, arrival)))
        .collect()
}

/// Chaque geste seul fait ce que dit la scène : un échec ne vient pas d'octets mal faits.
fn controls(scene: &str, mona_alone: &str, tom_alone: &str) {
    for order in ORDERS {
        let base = fixture(order, scene, "base.bin");
        let mona = fixture(order, scene, "mona.bin");
        let tom = fixture(order, scene, "tom.bin");
        let alone = play_updates(&base, &mona, &tom, &[Who::Mona]);
        assert!(
            alone.carried.is_empty(),
            "{order} : Mona seule, rien à reporter"
        );
        assert_eq!(
            line(&alone.server.read_blocks()),
            mona_alone,
            "{order} : Mona seule"
        );
        let alone = play_updates(&base, &mona, &tom, &[Who::Tom]);
        assert_eq!(
            line(&alone.server.read_blocks()),
            tom_alone,
            "{order} : Tom seul"
        );
    }
}

/// `render` écrit un MMD que `parse` relit en le même arbre (aux identifiants près).
fn assert_reads_as_mmd(blocks: &[Block]) {
    fn without_ids(blocks: &mut [Block]) {
        for b in blocks {
            b.id.clear();
            without_ids(&mut b.children);
        }
    }
    let markdown = render(blocks, RenderOpts::default()).markdown;
    let parsed = parse(&markdown).unwrap_or_else(|e| panic!("{e}\nMMD :\n{markdown}"));
    let mut expected = blocks.to_vec();
    without_ids(&mut expected);
    assert_eq!(parsed, expected, "MMD :\n{markdown}");
}

const XYZ: &str = "x « Bloc X » | y « Bloc Y » | z « Bloc Z »";
const YZX: &str = "y « Bloc Y » | z « Bloc Z » | x « Bloc X »";

/// Le texte tapé dans le bloc glissé, à la fin (S1), au début, au milieu et à la fin à la fois,
/// dans un bloc descendu au clavier (S7), dans des textes sans rien en commun (S8), et quand le
/// glisser passe par-dessus un titre (S2) : il est sur la copie, à sa place.
#[test]
fn text_typed_before_the_move_arrives_is_carried() {
    controls(
        "s1",
        YZX,
        "x « Bloc X de Tom » | y « Bloc Y » | z « Bloc Z »",
    );
    let cases = [
        ("s1", "y « Bloc Y » | z « Bloc Z » | x « Bloc X de Tom »"),
        (
            "start-middle-end",
            "y « Bloc Y » | z « Bloc Z » | x « Début Bl[milieu]oc X de Tom »",
        ),
        ("s7", "y « Bloc Y » | x « Bloc X de Tom » | z « Bloc Z »"),
        (
            "s8",
            "y « Rappeler Marc » | z « Arroser les plantes » | x « Acheter du pain de Tom »",
        ),
        (
            "s2",
            "y (heading) « Titre Y » | z « Bloc Z » | x « Bloc X de Tom »",
        ),
    ];
    for (scene, wanted) in cases {
        for (order, played) in both_orders(scene, MOVE_AFTER_EDIT) {
            assert_eq!(played.line(), wanted, "{scene}, {order}");
            assert_eq!(played.carried.len(), 1, "{scene}, {order} : un report");
            assert_reads_as_mmd(&played.server.read_blocks());
        }
    }
}

/// « de Tom », tapé en gras, garde son gras sur la copie ; le reste du bloc n'en prend pas.
#[test]
fn carried_text_keeps_its_marks() {
    for (order, played) in both_orders("bold", MOVE_AFTER_EDIT) {
        assert_eq!(
            played.line(),
            "y « Bloc Y » | z « Bloc Z » | x « Bloc X de Tom »"
        );
        for page in [&played.server, &played.mona, &played.tom] {
            let blocks = page.read_blocks();
            let x = find(&blocks, "x").unwrap();
            let Content::Inline(inline) = &x.content else {
                panic!("{order} : contenu en ligne");
            };
            let runs: Vec<(String, bool)> = inline
                .iter()
                .map(|i| match i {
                    Inline::Text { text, styles } => (text.clone(), styles.bold),
                    other => panic!("{order} : {other:?}"),
                })
                .collect();
            assert_eq!(
                runs,
                vec![("Bloc X".to_string(), false), (TYPED.to_string(), true)],
                "{order}"
            );
        }
    }
}

/// Un sous-bloc que Tom met sous X pendant que Mona glisse X : il suit X.
#[test]
fn child_block_added_under_the_original_follows_it() {
    controls(
        "child-added",
        YZX,
        "x « Bloc X » [x1 « Enfant de Tom »] | y « Bloc Y » | z « Bloc Z »",
    );
    for (order, played) in both_orders("child-added", MOVE_AFTER_EDIT) {
        assert_eq!(
            played.line(),
            "y « Bloc Y » | z « Bloc Z » | x « Bloc X » [x1 « Enfant de Tom »]",
            "{order}"
        );
        assert_reads_as_mmd(&played.server.read_blocks());
    }
}

/// La frappe dans le sous-bloc d'un bloc glissé : le sous-bloc est recréé avec son parent, et
/// la frappe le suit (S3).
#[test]
fn typing_in_a_child_of_a_moved_block_is_carried() {
    controls(
        "s3",
        "y « Bloc Y » | z « Bloc Z » | x « Bloc X » [x1 « Enfant X1 »]",
        "x « Bloc X » [x1 « Enfant X1 de Tom »] | y « Bloc Y » | z « Bloc Z »",
    );
    for (order, played) in both_orders("s3", MOVE_AFTER_EDIT) {
        assert_eq!(
            played.line(),
            "y « Bloc Y » | z « Bloc Z » | x « Bloc X » [x1 « Enfant X1 de Tom »]",
            "{order}"
        );
    }
}

/// Tab (S9) et Maj+Tab (S10) : la frappe suit le bloc à son nouveau niveau.
#[test]
fn indent_and_outdent_carry_what_was_typed() {
    let cases = [
        ("s9", "w « Bloc W » [x « Bloc X de Tom »] | y « Bloc Y »"),
        ("s10", "w « Bloc W » | x « Bloc X de Tom » | y « Bloc Y »"),
    ];
    for (scene, wanted) in cases {
        for (order, played) in both_orders(scene, MOVE_AFTER_EDIT) {
            assert_eq!(played.line(), wanted, "{scene}, {order}");
        }
    }
}

/// Mona tape « de Mona » dans X puis le glisse, hors ligne ; Tom tape « Tom : » au début de X.
/// Le texte de Mona n'est qu'une fois dans la copie. La place de « Tom : » y est sûre (le
/// premier caractère vu est le premier de la copie) : il est reporté (Ruling R373).
#[test]
fn mover_own_offline_edits_are_not_doubled() {
    controls(
        "mover-typed-too",
        "y « Bloc Y » | z « Bloc Z » | x « Bloc X de Mona »",
        "x « Tom : Bloc X » | y « Bloc Y » | z « Bloc Z »",
    );
    for (order, played) in both_orders("mover-typed-too", MOVE_AFTER_EDIT) {
        assert_eq!(
            played.line(),
            "y « Bloc Y » | z « Bloc Z » | x « Tom : Bloc X de Mona »",
            "{order}"
        );
    }
}

/// Le contenu en ligne de X porte une mention : plusieurs textes. Le serveur s'abstient, et la
/// page est celle que donne le glisser seul.
#[test]
fn inline_content_with_several_texts_is_left_alone() {
    for (order, played) in both_orders("several-texts", MOVE_AFTER_EDIT) {
        let mona_alone = play(order, "several-texts", [Who::Mona, Who::Mona]);
        assert!(played.carried.is_empty(), "{order} : rien de reporté");
        assert_eq!(
            played.line(),
            line(&mona_alone.server.read_blocks()),
            "{order}"
        );
        assert!(!played.line().contains("Tom"), "{order}");
    }
}

/// Le même glisser reçu deux fois (un onglet qui revient le renvoie) : reporté une fois.
#[test]
fn the_same_move_received_twice_carries_once() {
    for order in ORDERS {
        let played = play(order, "s1", MOVE_AFTER_EDIT);
        let mut played = played;
        let mona = fixture(order, "s1", "mona.bin");
        receive(&mut played, Who::Mona, &mona);
        assert_eq!(played.carried.len(), 1, "{order} : un seul report");
        assert_eq!(
            played.line(),
            "y « Bloc Y » | z « Bloc Z » | x « Bloc X de Tom »",
            "{order}"
        );
    }
}

/// Tom efface « oc » dans X et tape à sa fin pendant que Mona glisse X. La suppression n'est
/// pas reportée (Mona avait vu « oc » en place : sa copie le porte), et rien de ce que Tom a
/// tapé n'est retiré.
#[test]
fn concurrent_delete_is_not_carried_and_removes_nothing_typed() {
    controls(
        "concurrent-delete",
        YZX,
        "x « Bl X de Tom » | y « Bloc Y » | z « Bloc Z »",
    );
    for (order, played) in both_orders("concurrent-delete", MOVE_AFTER_EDIT) {
        assert_eq!(
            played.line(),
            "y « Bloc Y » | z « Bloc Z » | x « Bloc X de Tom »",
            "{order}"
        );
    }
}

/// Deux glissers du même bloc, sans frappe (S6) : le dédoublonnage n'en garde qu'un, au même
/// identifiant, et rien n'est reporté.
#[test]
fn two_moves_of_the_same_block_leave_one() {
    for arrival in [MOVE_AFTER_EDIT, MOVE_BEFORE_EDIT] {
        for (order, played) in both_orders("s6", arrival) {
            assert!(played.carried.is_empty(), "{order}, {arrival:?}");
            let shown = played.line();
            assert!(
                shown == YZX || shown == "y « Bloc Y » | x « Bloc X » | z « Bloc Z »",
                "{order}, {arrival:?} : {shown}"
            );
        }
    }
}

/// Une frappe dans un bloc que personne ne déplace (S4) : rien à reporter, la page est juste.
#[test]
fn typing_in_a_block_nobody_moves_carries_nothing() {
    for arrival in [MOVE_AFTER_EDIT, MOVE_BEFORE_EDIT] {
        for (order, played) in both_orders("s4", arrival) {
            assert!(played.carried.is_empty(), "{order}");
            assert_eq!(
                played.line(),
                "y « Bloc Y de Tom » | z « Bloc Z » | x « Bloc X »",
                "{order}"
            );
        }
    }
}

/// Bâti avec yrs : la page « Bloc X, Bloc Y, Bloc Z », son état, et les ids des trois blocs.
fn yrs_page() -> (Vec<u8>, [String; 3]) {
    let (base, _) = PageDoc::new_page();
    base.write_blocks(&parse("Bloc X\n\nBloc Y\n\nBloc Z\n").unwrap())
        .unwrap();
    let ids: Vec<String> = base.read_blocks().into_iter().map(|b| b.id).collect();
    (
        base.encode_state(),
        [ids[0].clone(), ids[1].clone(), ids[2].clone()],
    )
}

/// Mona, hors ligne, tape « X » juste avant le « X » de X, puis le déplace sous Z ; Tom tape
/// « ! » juste avant ce même « X ». Dans la copie, « Bloc XX », le « X » vu peut être l'un ou
/// l'autre : la place de « ! » n'est pas sûre, et rien n'est reporté (Ruling R373).
#[test]
fn a_place_that_is_not_sure_is_left_alone() {
    let (state, [x, _, z]) = yrs_page();
    let mona = PageDoc::from_updates([state.as_slice()]).unwrap();
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let before = PageDoc::from_updates([state.as_slice()]).unwrap();
    mona.replace_text(&x, 5, 0, "X").unwrap();
    mona.move_block(&x, Some(&z)).unwrap();
    let moved = mona.encode_diff(&before.state_vector()).unwrap();
    let typed = tom.replace_text(&x, 5, 0, "!").unwrap();
    let played = play_updates(&state, &moved, &typed, &[Who::Tom, Who::Mona]);
    assert!(played.carried.is_empty());
    let blocks = played.server.read_blocks();
    assert_eq!(block_text(&blocks[2]), "Bloc XX");
    assert_eq!(blocks[2].id, x);
}

/// Bâti avec yrs (`move_block` écrit une copie au même id puis supprime l'original) : la frappe
/// arrivée avant le déplacement est sur la copie.
#[test]
fn yrs_move_arriving_after_the_edit_keeps_the_edit() {
    let (state, [x, y, z]) = yrs_page();
    let a = PageDoc::from_updates([state.as_slice()]).unwrap();
    let b = PageDoc::from_updates([state.as_slice()]).unwrap();
    let moved = a.move_block(&x, Some(&z)).unwrap();
    let edited = b.replace_text(&x, 6, 0, " modifié par B").unwrap();
    let played = play_updates(&state, &moved, &edited, &[Who::Tom, Who::Mona]);
    let page: Vec<(String, String)> = played
        .server
        .read_blocks()
        .iter()
        .map(|b| (b.id.clone(), block_text(b)))
        .collect();
    assert_eq!(
        page,
        vec![
            (y, "Bloc Y".to_string()),
            (z, "Bloc Z".to_string()),
            (x, "Bloc X modifié par B".to_string()),
        ]
    );
    played.line();
}

/// Les limites connues (Ruling R340) : ce que le test dit voulu n'est pas tenu, et il
/// l'affirme. Le jour où la perte est levée, il échoue : le sortir de ce module, et lui faire
/// affirmer la page voulue.
mod known_limits {
    use super::limit::known_limit;
    use super::*;

    /// Voulu : X sous Z avec la frappe de Tom, quand le déplacement arrive avant elle. Aujourd'hui
    /// (Ruling R222) : la frappe, faite dans l'original sans avoir vu le déplacement, part avec
    /// lui, chez les trois. Octets du navigateur (S1) dans les deux ordres d'identifiants, et
    /// déplacement bâti avec yrs.
    #[test]
    fn edit_arriving_after_the_move_is_carried() {
        let wanted = "y « Bloc Y » | z « Bloc Z » | x « Bloc X de Tom »";
        let (mut missing, mut loss) = (Vec::new(), Vec::new());
        for (order, played) in both_orders("s1", MOVE_BEFORE_EDIT) {
            let seen = played.line();
            if seen != wanted {
                missing.push(format!("{order} : vu {seen}, voulu {wanted}"));
            }
            loss.push(format!("{order} : vu {YZX}, voulu {wanted}"));
        }

        let (state, [x, _, z]) = yrs_page();
        let a = PageDoc::from_updates([state.as_slice()]).unwrap();
        let b = PageDoc::from_updates([state.as_slice()]).unwrap();
        let moved = a.move_block(&x, Some(&z)).unwrap();
        let edited = b.replace_text(&x, 6, 0, " de Tom").unwrap();
        let played = play_updates(&state, &moved, &edited, &[Who::Mona, Who::Tom]);
        let text = block_text(&played.server.read_blocks()[2]);
        if text != "Bloc X de Tom" {
            missing.push(format!("yrs : vu {text:?}"));
        }
        loss.push("yrs : vu \"Bloc X\"".to_string());
        known_limit(
            "R222 : une frappe faite sans avoir vu le déplacement, arrivée après lui, part avec \
             l'original ; la reconnaître demande de lire les éléments supprimés de l'original",
            &missing,
            &loss,
        );
    }
}

/// La page de départ des octets, pour les témoins.
#[test]
fn the_fixtures_start_from_the_same_page() {
    for order in ORDERS {
        let base = fixture(order, "s1", "base.bin");
        assert_eq!(
            line(
                &PageDoc::from_updates([base.as_slice()])
                    .unwrap()
                    .read_blocks()
            ),
            XYZ
        );
    }
}

/// L'essai au hasard de la Task 13b : des insertions de texte par deux clients (Tom et Léa),
/// puis des déplacements par un troisième (Mona, `move_block` : copie au même id et
/// suppression), faits sans rien voir des insertions et arrivés après elles.
mod at_random {
    use proptest::prelude::*;

    use super::*;

    /// Un bloc de la page de départ : un paragraphe, et peut-être un sous-bloc.
    fn paragraph(text: &str, children: Vec<Block>) -> Block {
        Block {
            id: String::new(),
            kind: "paragraph".to_string(),
            props: Default::default(),
            content: Content::Inline(vec![Inline::Text {
                text: text.to_string(),
                styles: Default::default(),
            }]),
            children,
        }
    }

    /// Les ids de la page, dans l'ordre du document, et le texte de chacun.
    fn flat(blocks: &[Block], out: &mut Vec<(String, String)>) {
        for b in blocks {
            out.push((b.id.clone(), block_text(b)));
            flat(&b.children, out);
        }
    }

    /// Les textes de toute la page.
    fn all_texts(blocks: &[Block]) -> String {
        let mut out = Vec::new();
        flat(blocks, &mut out);
        out.into_iter()
            .map(|(_, t)| t)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Une insertion : le bloc (rang dans l'ordre du document), la place (fraction du texte de
    /// départ, en millièmes).
    type Insert = (usize, u32);

    fn page_strategy() -> impl Strategy<Value = Vec<bool>> {
        // Un bloc de premier niveau par élément ; vrai : il a un sous-bloc.
        proptest::collection::vec(any::<bool>(), 3..6)
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

        /// Chaque texte inséré est dans la page exactement une fois, dans le bloc où il a été
        /// tapé, chez les quatre ; la page se lit en MMD.
        #[test]
        fn moves_and_earlier_edits_lose_no_insert(
            shape in page_strategy(),
            tom in proptest::collection::vec((0usize..12, 0u32..=1000), 0..5),
            lea in proptest::collection::vec((0usize..12, 0u32..=1000), 0..5),
            moves in proptest::collection::vec((0usize..12, proptest::option::of(0usize..12)), 1..4),
        ) {
            let blocks: Vec<Block> = shape
                .iter()
                .enumerate()
                .map(|(i, child)| {
                    let children = if *child {
                        vec![paragraph(&format!("Enfant {i}"), Vec::new())]
                    } else {
                        Vec::new()
                    };
                    paragraph(&format!("Bloc numéro {i}"), children)
                })
                .collect();
            let (base, _) = PageDoc::new_page();
            base.write_blocks(&blocks).unwrap();
            let state = base.encode_state();
            let mut ids = Vec::new();
            flat(&base.read_blocks(), &mut ids);

            // Chaque client tape au plus une fois par bloc, à une place du texte de départ.
            let typing = |who: &str, inserts: &[Insert]| -> (Vec<u8>, Vec<(String, String)>) {
                let doc = PageDoc::from_updates([state.as_slice()]).unwrap();
                let mut done: Vec<(String, String)> = Vec::new();
                let mut updates = Vec::new();
                for (k, (block, place)) in inserts.iter().enumerate() {
                    let (id, text) = &ids[block % ids.len()];
                    if done.iter().any(|(i, _)| i == id) {
                        continue;
                    }
                    let len = text.chars().count();
                    let at = (len * *place as usize) / 1000;
                    let token = format!("Q{who}{k}Q");
                    updates.push(doc.replace_text(id, at, 0, &token).unwrap());
                    done.push((id.clone(), token));
                }
                let base = PageDoc::from_updates([state.as_slice()]).unwrap();
                let update = doc.encode_diff(&base.state_vector()).unwrap();
                (update, done)
            };
            let (tom_update, tom_tokens) = typing("T", &tom);
            let (lea_update, lea_tokens) = typing("L", &lea);

            let mona = PageDoc::from_updates([state.as_slice()]).unwrap();
            for (block, after) in &moves {
                let id = &ids[block % ids.len()].0;
                let after = after.map(|a| ids[a % ids.len()].0.clone());
                // Un déplacement refusé (dans son propre sous-arbre, ou sur lui-même) n'écrit rien.
                let _ = mona.move_block(id, after.as_deref());
            }
            let fresh = PageDoc::from_updates([state.as_slice()]).unwrap();
            let mona_update = mona.encode_diff(&fresh.state_vector()).unwrap();

            let server = PageDoc::from_updates([state.as_slice()]).unwrap();
            let clients = [
                PageDoc::from_updates([state.as_slice(), mona_update.as_slice()]).unwrap(),
                PageDoc::from_updates([state.as_slice(), tom_update.as_slice()]).unwrap(),
                PageDoc::from_updates([state.as_slice(), lea_update.as_slice()]).unwrap(),
            ];
            // Tom et Léa d'abord, Mona ensuite.
            for (author, update) in [(1usize, &tom_update), (2, &lea_update), (0, &mona_update)] {
                let applied = server.apply_client_update(update).unwrap();
                let fix = server.dedupe_blocks();
                for (k, client) in clients.iter().enumerate() {
                    if k != author {
                        client.apply_update(&applied.read).unwrap();
                    }
                    for extra in applied.carried.iter().chain(fix.iter()) {
                        client.apply_update(extra).unwrap();
                    }
                }
            }

            let page = server.read_blocks();
            let shown = line(&page);
            for client in &clients {
                prop_assert_eq!(line(&client.read_blocks()), shown.clone());
            }
            let texts = all_texts(&page);
            for (id, token) in tom_tokens.iter().chain(lea_tokens.iter()) {
                prop_assert_eq!(texts.matches(token.as_str()).count(), 1, "{} : {}", token, shown);
                let block = find(&page, id);
                prop_assert!(
                    block.is_some_and(|b| block_text(b).contains(token.as_str())),
                    "{} pas dans {} : {}", token, id, shown
                );
            }
            assert_reads_as_mmd(&page);
        }
    }
}
