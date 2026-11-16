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

/// Tom efface « oc » dans X et tape à sa fin pendant que Mona glisse X. La copie de Mona porte
/// encore « oc » : deux caractères de plus que ce que le serveur a de vivant, que Mona n'a pas
/// tapés. Le serveur ne sait pas les distinguer d'un effacement de Mona retapé ailleurs : il
/// s'abstient (Ruling R450). La suppression de Tom n'est pas reportée (« oc » revient), sa frappe
/// part avec l'original, et rien n'est retiré par un report.
#[test]
fn concurrent_delete_is_not_carried_and_the_server_abstains() {
    controls(
        "concurrent-delete",
        YZX,
        "x « Bl X de Tom » | y « Bloc Y » | z « Bloc Z »",
    );
    for (order, played) in both_orders("concurrent-delete", MOVE_AFTER_EDIT) {
        assert!(played.carried.is_empty(), "{order}");
        assert_eq!(played.line(), YZX, "{order}");
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

/// Tom tape « d », que Mona reçoit, puis « e » juste après, qu'elle ne voit pas avant de
/// déplacer X : sur le serveur, les deux lettres de Tom, aux horloges qui se suivent, tiennent
/// dans un seul élément de yrs. Seul « e » est reporté, à sa place (Ruling R371 : les blocs de
/// yrs sont d'abord découpés aux bornes du jeu de suppressions).
#[test]
fn characters_seen_and_unseen_in_one_element_are_told_apart() {
    let (state, [x, _, z]) = yrs_page();
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let d = tom.replace_text(&x, 6, 0, " d").unwrap();
    let e = tom.replace_text(&x, 8, 0, "e").unwrap();
    let mona = PageDoc::from_updates([state.as_slice(), d.as_slice()]).unwrap();
    let before = PageDoc::from_updates([state.as_slice(), d.as_slice()]).unwrap();
    mona.move_block(&x, Some(&z)).unwrap();
    let moved = mona.encode_diff(&before.state_vector()).unwrap();
    let both = PageDoc::from_updates([state.as_slice(), d.as_slice(), e.as_slice()]).unwrap();
    let played = play_updates(&both.encode_state(), &moved, &e, &[Who::Mona]);
    assert_eq!(played.carried.len(), 1, "un report");
    let blocks = played.server.read_blocks();
    assert_eq!(blocks[2].id, x);
    assert_eq!(block_text(&blocks[2]), "Bloc X de");
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

/// Des mises à jour faites à la main, avec yrs : ce qu'un client peut envoyer sans être un
/// éditeur BlockNote (Task 13b, tour de correction 1).
mod raw {
    use std::collections::BTreeMap;

    use mementum_crdt::FRAGMENT;
    use yrs::branch::{Branch, BranchID};
    use yrs::updates::decoder::Decode;
    use yrs::{
        Doc, GetString, ID, OffsetKind, Options, ReadTxn, StateVector, Text, Transact,
        TransactionMut, Update, Xml, XmlElementPrelim, XmlElementRef, XmlFragment, XmlFragmentRef,
        XmlOut, XmlTextPrelim,
    };

    /// Un document de client, parti de `state`.
    pub fn doc(state: &[u8]) -> Doc {
        let doc = Doc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        doc.transact_mut()
            .apply_update(Update::decode_v1(state).unwrap())
            .unwrap();
        doc
    }

    pub fn element(out: Option<XmlOut>) -> XmlElementRef {
        match out {
            Some(XmlOut::Element(e)) => e,
            other => panic!("pas un élément : {other:?}"),
        }
    }

    /// L'identifiant de l'élément Yjs d'un type.
    pub fn id_of<B: AsRef<Branch>>(node: &B) -> ID {
        match node.as_ref().id() {
            BranchID::Nested(id) => id,
            BranchID::Root(_) => panic!("racine"),
        }
    }

    /// Le fragment de la page, pris **avant** toute transaction (avec la feature `sync` de yrs,
    /// `get_or_insert_*` attend le verrou qu'une transaction ouverte détient).
    pub fn fragment(doc: &Doc) -> XmlFragmentRef {
        doc.get_or_insert_xml_fragment(FRAGMENT)
    }

    /// La racine `blockGroup` de la page.
    pub fn root<T: ReadTxn>(fragment: &XmlFragmentRef, txn: &T) -> XmlElementRef {
        element(fragment.get(txn, 0))
    }

    /// Le `blockContainer` de premier niveau dont l'attribut `id` vaut `id`, et son index.
    pub fn top_block<T: ReadTxn>(
        fragment: &XmlFragmentRef,
        txn: &T,
        id: &str,
    ) -> (XmlElementRef, u32) {
        let root = root(fragment, txn);
        (0..root.len(txn))
            .map(|i| (element(root.get(txn, i)), i))
            .find(|(c, _)| {
                c.get_attribute(txn, "id")
                    .map(|v| v.to_string(txn))
                    .as_deref()
                    == Some(id)
            })
            .expect("bloc de premier niveau")
    }

    /// Écrit un `blockContainer` d'`id` donné, avec un nœud de contenu `tag` et son texte, à
    /// l'index `at` de `parent`.
    pub fn container(
        txn: &mut TransactionMut,
        parent: &XmlElementRef,
        at: u32,
        id: &str,
        tag: &str,
        text: &str,
    ) -> XmlElementRef {
        let c = parent.insert(txn, at, XmlElementPrelim::empty("blockContainer"));
        c.insert_attribute(txn, "id", id.to_string());
        let content = c.push_back(txn, XmlElementPrelim::empty(tag));
        if !text.is_empty() {
            let t = content.push_back(txn, XmlTextPrelim::new(""));
            t.insert(txn, 0, text);
        }
        c
    }

    /// Une mise à jour v1 sans bloc, qui ne porte que ces suppressions `(client, horloge,
    /// longueur)`.
    pub fn deletions(ranges: &[ID]) -> Vec<u8> {
        let mut by_client: BTreeMap<u64, Vec<u32>> = BTreeMap::new();
        for id in ranges {
            by_client.entry(id.client.get()).or_default().push(id.clock);
        }
        let mut out = Vec::new();
        var(&mut out, 0);
        var(&mut out, by_client.len() as u64);
        for (client, mut clocks) in by_client {
            clocks.sort_unstable();
            var(&mut out, client);
            var(&mut out, clocks.len() as u64);
            for clock in clocks {
                var(&mut out, u64::from(clock));
                var(&mut out, 1);
            }
        }
        out
    }

    fn var(out: &mut Vec<u8>, mut n: u64) {
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
    }

    /// Le texte et le nombre de sous-blocs du `blockContainer` vivant d'identifiant `id`, où qu'il
    /// soit dans la page (même hors des groupes que lit `read_blocks`).
    pub fn block_summary(state: &[u8], id: &str) -> Option<(String, usize)> {
        let doc = doc(state);
        let fragment = fragment(&doc);
        let txn = doc.transact();
        let container = fragment.successors(&txn).find_map(|node| match node {
            XmlOut::Element(e)
                if e.tag().as_ref() == "blockContainer"
                    && e.get_attribute(&txn, "id")
                        .map(|v| v.to_string(&txn))
                        .as_deref()
                        == Some(id) =>
            {
                Some(e)
            }
            _ => None,
        })?;
        let mut text = String::new();
        let mut children = 0;
        for child in container.children(&txn) {
            let XmlOut::Element(e) = child else { continue };
            if e.tag().as_ref() == "blockGroup" {
                children = e.len(&txn) as usize;
            } else if let Some(XmlOut::Text(t)) = e.get(&txn, 0) {
                text = t.get_string(&txn);
            }
        }
        Some((text, children))
    }

    /// Ce que `doc` a écrit depuis `before`, et ces suppressions de plus, en une mise à jour.
    pub fn update_with(doc: &Doc, before: &StateVector, deleted: &[ID]) -> Vec<u8> {
        let inserted = doc.transact().encode_diff_v1(before);
        if deleted.is_empty() {
            return inserted;
        }
        yrs::merge_updates_v1([inserted.as_slice(), deletions(deleted).as_slice()]).unwrap()
    }
}

/// Un paragraphe et ses sous-blocs, sans identifiant.
fn para(text: &str, children: Vec<Block>) -> Block {
    Block {
        id: String::new(),
        kind: "paragraph".to_string(),
        props: Default::default(),
        content: Content::Inline(if text.is_empty() {
            Vec::new()
        } else {
            vec![Inline::Text {
                text: text.to_string(),
                styles: Default::default(),
            }]
        }),
        children,
    }
}

/// La page `blocks`, son état et les ids de ses blocs de premier niveau.
fn page_of(blocks: &[Block]) -> (Vec<u8>, Vec<String>) {
    let (base, _) = PageDoc::new_page();
    base.write_blocks(blocks).unwrap();
    let ids = base.read_blocks().into_iter().map(|b| b.id).collect();
    (base.encode_state(), ids)
}

/// La page relue depuis `updates` (le journal), puis depuis l'état complet du serveur
/// (l'instantané d'une compaction) : les deux se relisent.
fn assert_reloads(server: &PageDoc, journal: &[&[u8]]) {
    let reloaded = PageDoc::from_updates(journal.iter().copied());
    assert!(
        reloaded.is_ok(),
        "le journal ne se relit plus : {:?}",
        reloaded.err()
    );
    let snapshot = server.encode_state();
    let reopened = PageDoc::from_updates([snapshot.as_slice()]);
    assert!(
        reopened.is_ok(),
        "l'instantané ne se relit plus : {:?}",
        reopened.err()
    );
}

/// Ce qu'un client supprime de l'original en recréant X.
enum Deleted {
    /// Les premiers éléments de l'original, dans cet ordre : son `blockContainer`, son nœud de
    /// contenu, son texte (l'élément du texte, pas ses caractères).
    Parts(usize),
    /// Tout l'original, comme le fait un éditeur.
    All,
}

/// Une copie de X déposée tout au fond d'une chaîne de groupes, son nœud de contenu à la
/// profondeur `content_depth`, avec le texte `text`, par un client qui supprime de l'original
/// ce que dit `deleted`. Rend la mise à jour.
fn deep_copy(state: &[u8], x: &str, content_depth: usize, text: &str, deleted: Deleted) -> Vec<u8> {
    use yrs::{ReadTxn, Transact, XmlElementPrelim, XmlFragment, XmlOut};
    let doc = raw::doc(state);
    let frag = raw::fragment(&doc);
    let (parts, index) = {
        let txn = doc.transact();
        let (c, index) = raw::top_block(&frag, &txn, x);
        let content = raw::element(c.get(&txn, 0));
        let mut parts = vec![raw::id_of(&c), raw::id_of(&content)];
        if let Some(XmlOut::Text(t)) = content.get(&txn, 0) {
            parts.push(raw::id_of(&t));
        }
        (parts, index)
    };
    let before = doc.transact().state_vector();
    {
        let mut txn = doc.transact_mut();
        let root = raw::root(&frag, &txn);
        let mut parent = root.clone();
        // La racine est à la profondeur 1, le conteneur de la copie à `content_depth - 1`.
        for _ in 2..content_depth - 1 {
            parent = parent.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        }
        raw::container(&mut txn, &parent, 0, x, "paragraph", text);
        if matches!(deleted, Deleted::All) {
            root.remove_range(&mut txn, index, 1);
        }
    }
    match deleted {
        Deleted::Parts(n) => raw::update_with(&doc, &before, &parts[..n.min(parts.len())]),
        Deleted::All => raw::update_with(&doc, &before, &[]),
    }
}

/// C1 : un client recrée X tout au fond de la page, le nœud de contenu de la copie à la
/// profondeur `MAX_YJS_DEPTH` et sans texte, et ne supprime de l'original que son élément et
/// son nœud de contenu (tout le texte est « non vu »). Le serveur n'écrit aucun texte au-delà de
/// la borne : la page se relit, journal et instantané.
#[test]
fn carried_text_is_never_written_beyond_the_depth_bound() {
    let (state, ids) = page_of(&[para("Bloc X", vec![]), para("Bloc Y", vec![])]);
    let update = deep_copy(
        &state,
        &ids[0],
        mementum_crdt::MAX_YJS_DEPTH,
        "",
        Deleted::Parts(2),
    );
    let server = PageDoc::from_updates([state.as_slice()]).unwrap();
    let applied = server
        .apply_client_update(&update)
        .expect("acceptée par la garde");
    let mut journal = vec![state.as_slice(), applied.read.as_slice()];
    journal.extend(applied.carried.as_deref());
    assert_reloads(&server, &journal);
}

/// C1 (Ruling R452) : une copie de X posée ailleurs que dans un groupe de blocs ou une colonne
/// (ici dans le paragraphe de Y), où BlockNote n'en pose jamais, ne reçoit pas de report.
#[test]
fn a_copy_outside_a_group_or_a_column_receives_nothing() {
    let (state, ids) = page_of(&[para("Bloc X", vec![]), para("Bloc Y", vec![])]);
    let x = &ids[0];
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let typed = tom.replace_text(x, 6, 0, " de Tom").unwrap();
    let doc = raw::doc(&state);
    let frag = raw::fragment(&doc);
    let before = {
        use yrs::{ReadTxn, Transact};
        doc.transact().state_vector()
    };
    {
        use yrs::{Transact, XmlFragment};
        let mut txn = doc.transact_mut();
        let (y, _) = raw::top_block(&frag, &txn, &ids[1]);
        let paragraph = raw::element(y.get(&txn, 0));
        let len = paragraph.len(&txn);
        raw::container(&mut txn, &paragraph, len, x, "paragraph", "Bloc X");
        raw::root(&frag, &txn).remove_range(&mut txn, 0, 1);
    }
    let moved = raw::update_with(&doc, &before, &[]);
    let server = PageDoc::from_updates([state.as_slice()]).unwrap();
    server.apply_client_update(&typed).unwrap();
    let applied = server.apply_client_update(&moved).unwrap();
    assert!(applied.carried.is_none());
}

/// I3 (M13) : la copie au fond de la page a la place de reporter son texte, pas celle d'un
/// sous-bloc qu'un autre a mis sous l'original : le texte est reporté, le sous-bloc ne l'est pas,
/// et la page se relit.
#[test]
fn a_sub_block_too_deep_is_left_out_but_the_text_is_carried() {
    let (state, ids) = page_of(&[para("Bloc X", vec![]), para("Bloc Y", vec![])]);
    let x = &ids[0];
    // Tom tape à la fin de X et lui met un sous-bloc.
    let doc = raw::doc(&state);
    let frag = raw::fragment(&doc);
    let before = {
        use yrs::{ReadTxn, Transact};
        doc.transact().state_vector()
    };
    {
        use yrs::{Text, Transact, XmlElementPrelim, XmlFragment, XmlOut};
        let mut txn = doc.transact_mut();
        let (c, _) = raw::top_block(&frag, &txn, x);
        let content = raw::element(c.get(&txn, 0));
        let Some(XmlOut::Text(text)) = content.get(&txn, 0) else {
            panic!("le texte de X");
        };
        text.insert(&mut txn, 6, " de Tom");
        let group = c.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        raw::container(&mut txn, &group, 0, "enfant", "paragraph", "Enfant de Tom");
    }
    let typed = raw::update_with(&doc, &before, &[]);
    // Le nœud de contenu de la copie à la profondeur MAX - 2 : son texte, déjà là, peut recevoir
    // le report ; un groupe de sous-blocs sous la copie (MAX - 2), un sous-bloc (MAX - 1), son
    // paragraphe (MAX), son texte (MAX + 1) : trop profond.
    let moved = deep_copy(
        &state,
        x,
        mementum_crdt::MAX_YJS_DEPTH - 2,
        "Bloc X",
        Deleted::All,
    );
    let played = play_updates(&state, &moved, &typed, &[Who::Tom, Who::Mona]);
    let summary = raw::block_summary(&played.server.encode_state(), x);
    assert_eq!(summary, Some(("Bloc X de Tom".to_string(), 0)));
    played.line();
    assert_reloads(&played.server, &[played.server.encode_state().as_slice()]);
}

/// Un bloc à `n` sous-blocs, et une mise à jour de quelques dizaines d'octets : une copie vide
/// au même id, qui ne supprime de l'original que son élément et son paragraphe (le groupe de
/// sous-blocs est « non vu »).
fn empty_copy_of_a_parent(n: usize) -> (PageDoc, Vec<u8>, String) {
    let children: Vec<Block> = (0..n)
        .map(|i| {
            para(
                &if n <= 65 {
                    format!("{i}")
                } else {
                    format!("Sous-bloc numéro {i} avec un peu de texte")
                },
                vec![],
            )
        })
        .collect();
    let (state, ids) = page_of(&[para("Parent", children), para("Autre", vec![])]);
    let x = ids[0].clone();
    let doc = raw::doc(&state);
    let frag = raw::fragment(&doc);
    let (original, content) = {
        use yrs::{Transact, XmlFragment};
        let txn = doc.transact();
        let (c, _) = raw::top_block(&frag, &txn, &x);
        (raw::id_of(&c), raw::id_of(&raw::element(c.get(&txn, 0))))
    };
    let before = {
        use yrs::{ReadTxn, Transact};
        doc.transact().state_vector()
    };
    {
        use yrs::{Transact, XmlFragment};
        let mut txn = doc.transact_mut();
        let root = raw::root(&frag, &txn);
        let len = root.len(&txn);
        raw::container(&mut txn, &root, len, &x, "paragraph", "");
    }
    let update = raw::update_with(&doc, &before, &[original, content]);
    (
        PageDoc::from_updates([state.as_slice()]).unwrap(),
        update,
        x,
    )
}

/// C2 : des sous-blocs « non vus » d'un coup, au-delà de la borne du report. 65 sous-blocs de
/// quelques octets (la borne des 64 sous-blocs, sous celle des 64 Kio), puis 1 000 (la sonde de la
/// relecture : 280 Ko reportés pour 136 o reçus). Le serveur n'écrit rien de plus que la mise à
/// jour, aucun sous-bloc recopié, et dit pourquoi.
#[test]
fn a_carry_beyond_the_bound_is_withheld() {
    for n in [65, 1_000] {
        let (server, update, x) = empty_copy_of_a_parent(n);
        let applied = server.apply_client_update(&update).expect("acceptée");
        assert!(
            applied.carried.is_none(),
            "{n} : {} o reportés",
            applied.carried.map_or(0, |c| c.len())
        );
        assert!(
            applied.withheld.is_some(),
            "{n} : la raison, pour le journal"
        );
        let blocks = server.read_blocks();
        let copy = find(&blocks, &x).expect("la copie");
        assert!(copy.children.is_empty(), "{n}");
    }
}

/// Le témoin de C2 : 64 sous-blocs non vus, juste à la borne, sont recopiés.
#[test]
fn a_carry_at_the_bound_is_written() {
    let (server, update, x) = empty_copy_of_a_parent(64);
    let applied = server.apply_client_update(&update).expect("acceptée");
    assert!(applied.carried.is_some());
    let blocks = server.read_blocks();
    assert_eq!(find(&blocks, &x).expect("la copie").children.len(), 64);
}

/// C2, en octets : un texte non vu de 80 Kio n'est pas reporté ; de 32 Kio, il l'est.
#[test]
fn a_carried_text_beyond_64_kib_is_withheld() {
    for (size, carried) in [(32 * 1024, true), (80 * 1024, false)] {
        let (state, ids) = page_of(&[para("Bloc X", vec![]), para("Bloc Y", vec![])]);
        let x = &ids[0];
        let typist = PageDoc::from_updates([state.as_slice()]).unwrap();
        let typed = typist.replace_text(x, 6, 0, &"t".repeat(size)).unwrap();
        let mona = PageDoc::from_updates([state.as_slice()]).unwrap();
        let moved = mona.move_block(x, Some(&ids[1])).unwrap();
        let played = play_updates(&state, &moved, &typed, &[Who::Tom, Who::Mona]);
        assert_eq!(!played.carried.is_empty(), carried, "{size} o");
        let text = block_text(find(&played.server.read_blocks(), x).unwrap());
        assert_eq!(text.len(), if carried { 6 + size } else { 6 }, "{size} o");
    }
}

/// I1 : Mona corrige « chatt » en « chat » et ajoute « est » avant « le chien », hors ligne, puis
/// déplace X ; Tom tape « , pas » juste après « et ». La copie de Mona n'est pas le texte vu
/// avec des insertions seulement (un caractère vu a été effacé) : le serveur s'abstient, plutôt
/// que de mettre « , pas » après « est » (sonde de la relecture : « le chat et est, pas le
/// chien »).
#[test]
fn a_typo_fixed_then_a_word_added_by_the_mover_carries_nothing() {
    let (state, ids) = page_of(&[para("le chatt et le chien", vec![]), para("Bloc Y", vec![])]);
    let x = &ids[0];
    let mona = PageDoc::from_updates([state.as_slice()]).unwrap();
    let before = PageDoc::from_updates([state.as_slice()]).unwrap();
    mona.replace_text(x, 7, 1, "").unwrap();
    mona.replace_text(x, 11, 0, "est ").unwrap();
    mona.move_block(x, Some(&ids[1])).unwrap();
    let moved = mona.encode_diff(&before.state_vector()).unwrap();
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let typed = tom.replace_text(x, 11, 0, ", pas").unwrap();
    let played = play_updates(&state, &moved, &typed, &[Who::Tom, Who::Mona]);
    assert!(played.carried.is_empty());
    let text = block_text(find(&played.server.read_blocks(), x).unwrap());
    assert_eq!(text, "le chat et est le chien");
    played.line();
}

/// I1 : un effacement de Mona n'importe où dans le bloc (ici au début, la frappe de Tom à la
/// fin) fait s'abstenir le serveur.
#[test]
fn a_deletion_by_the_mover_anywhere_in_the_block_carries_nothing() {
    let (state, [x, _, z]) = yrs_page();
    let mona = PageDoc::from_updates([state.as_slice()]).unwrap();
    let before = PageDoc::from_updates([state.as_slice()]).unwrap();
    mona.replace_text(&x, 0, 1, "").unwrap();
    mona.move_block(&x, Some(&z)).unwrap();
    let moved = mona.encode_diff(&before.state_vector()).unwrap();
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let typed = tom.replace_text(&x, 6, 0, " de Tom").unwrap();
    let played = play_updates(&state, &moved, &typed, &[Who::Tom, Who::Mona]);
    assert!(played.carried.is_empty());
    assert_eq!(block_text(&played.server.read_blocks()[2]), "loc X");
}

/// I2 : Tom tape dans X ; Mona puis Léa déplacent X, sans rien voir des autres. La frappe est
/// reportée dans la copie de Mona ; le dédoublonnage garde cette copie, quelle que soit la place
/// des deux exemplaires.
#[test]
fn two_moves_after_an_edit_keep_the_edit() {
    let (state, ids) = page_of(&[
        para("Bloc W", vec![]),
        para("Bloc X", vec![]),
        para("Bloc Y", vec![]),
        para("Bloc Z", vec![]),
    ]);
    let (w, x, z) = (&ids[0], &ids[1], &ids[3]);
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let typed = tom.replace_text(x, 6, 0, " de Tom").unwrap();
    let mut lost = Vec::new();
    for (mona_after, lea_after) in [
        (Some(z), Some(w)),
        (Some(w), Some(z)),
        (None, Some(z)),
        (Some(z), None),
    ] {
        let fresh = PageDoc::from_updates([state.as_slice()]).unwrap();
        let mona = PageDoc::from_updates([state.as_slice()]).unwrap();
        mona.move_block(x, mona_after.map(String::as_str)).unwrap();
        let moved_m = mona.encode_diff(&fresh.state_vector()).unwrap();
        let lea = PageDoc::from_updates([state.as_slice()]).unwrap();
        lea.move_block(x, lea_after.map(String::as_str)).unwrap();
        let moved_l = lea.encode_diff(&fresh.state_vector()).unwrap();
        let server = PageDoc::from_updates([state.as_slice()]).unwrap();
        for update in [&typed, &moved_m, &moved_l] {
            server.apply_client_update(update).unwrap();
            server.dedupe_blocks();
        }
        let blocks = server.read_blocks();
        let xs: Vec<String> = blocks
            .iter()
            .filter(|b| &b.id == x)
            .map(block_text)
            .collect();
        if xs != ["Bloc X de Tom"] {
            lost.push(format!("{mona_after:?}/{lea_after:?} : {}", line(&blocks)));
        }
    }
    assert!(lost.is_empty(), "{lost:?}");
}

/// I3 (M15) : X a deux sous-blocs vus, x1 et x2 ; Tom en ajoute un entre eux pendant que Mona
/// déplace X. Le sous-bloc de Tom revient après la copie de x1, avant x2 (Ruling R374).
#[test]
fn an_unseen_sub_block_goes_after_the_seen_one_before_it() {
    let (state, ids) = page_of(&[
        para("Bloc X", vec![para("Un", vec![]), para("Deux", vec![])]),
        para("Bloc Y", vec![]),
    ]);
    let x = &ids[0];
    let base = PageDoc::from_updates([state.as_slice()]).unwrap();
    let children: Vec<String> = base.read_blocks()[0]
        .children
        .iter()
        .map(|b| b.id.clone())
        .collect();
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let typed = tom
        .insert_blocks_after(Some(&children[0]), &[para("De Tom", vec![])])
        .unwrap();
    let mona = PageDoc::from_updates([state.as_slice()]).unwrap();
    let moved = mona.move_block(x, Some(&ids[1])).unwrap();
    let played = play_updates(&state, &moved, &typed, &[Who::Tom, Who::Mona]);
    let blocks = played.server.read_blocks();
    let texts: Vec<String> = find(&blocks, x)
        .unwrap()
        .children
        .iter()
        .map(block_text)
        .collect();
    assert_eq!(texts, ["Un", "De Tom", "Deux"]);
    played.line();
}

/// Un double de X (même id), posé à la fin de la page par une mise à jour relue du stockage,
/// sans dédoublonnage : rend la page avec le double.
fn with_a_double_of_x(state: &[u8], x: &str) -> Vec<u8> {
    use yrs::{ReadTxn, Transact, XmlFragment};
    let doc = raw::doc(state);
    let frag = raw::fragment(&doc);
    {
        let mut txn = doc.transact_mut();
        let root = raw::root(&frag, &txn);
        let len = root.len(&txn);
        raw::container(&mut txn, &root, len, x, "paragraph", "Leurre");
    }
    doc.transact()
        .encode_state_as_update_v1(&yrs::StateVector::default())
}

/// I3 (M7) : deux originaux vivants de X, que la mise à jour de Mona supprime tous deux en
/// recréant X : la correspondance n'est pas sûre, rien n'est reporté.
#[test]
fn two_originals_of_one_id_carry_nothing() {
    let (state, ids) = page_of(&[para("Bloc X", vec![]), para("Bloc Y", vec![])]);
    let x = &ids[0];
    let state = with_a_double_of_x(&state, x);
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let typed = tom.replace_text(x, 6, 0, " de Tom").unwrap();
    let doc = raw::doc(&state);
    let frag = raw::fragment(&doc);
    let before = {
        use yrs::{ReadTxn, Transact};
        doc.transact().state_vector()
    };
    {
        use yrs::{Transact, XmlFragment};
        let mut txn = doc.transact_mut();
        let root = raw::root(&frag, &txn);
        // Les deux exemplaires de X sont aux index 0 et 2 ; la copie va à la fin.
        let len = root.len(&txn);
        raw::container(&mut txn, &root, len, x, "paragraph", "Bloc X");
        root.remove_range(&mut txn, 2, 1);
        root.remove_range(&mut txn, 0, 1);
    }
    let moved = raw::update_with(&doc, &before, &[]);
    // La page telle qu'un serveur la charge, avec la frappe de Tom : aucun dédoublonnage n'est
    // passé entre-temps, et la mise à jour de Mona est la première qui arrive.
    let loaded = PageDoc::from_updates([state.as_slice(), typed.as_slice()]).unwrap();
    let server = PageDoc::from_updates([loaded.encode_state().as_slice()]).unwrap();
    let applied = server.apply_client_update(&moved).unwrap();
    assert!(applied.carried.is_none());
    server.dedupe_blocks();
    assert!(!line(&server.read_blocks()).contains("Tom"));
}

/// I3 (M8, M9) : Ruling R372. Un jeu de suppressions de plus de 65 536 horloges (une étape 2, qui
/// porte toutes les suppressions de son client, ici 70 000 lettres effacées dans Y) fait chercher
/// l'original par un parcours de la page, et un seul ; un double de X que la mise à jour ne
/// supprime pas n'en est pas un (l'original est celui que le jeu couvre), et la frappe de Tom est
/// reportée. Un déplacement ordinaire ne parcourt pas la page.
#[test]
fn originals_are_found_by_a_page_walk_beyond_65536_deleted_clocks() {
    let (state, ids) = page_of(&[
        para("Bloc X", vec![]),
        para("Bloc Y", vec![]),
        para("Bloc Z", vec![]),
    ]);
    let (x, y, z) = (&ids[0], &ids[1], &ids[2]);
    // Quelqu'un tape 70 000 lettres dans Y, puis les efface.
    let page = PageDoc::from_updates([state.as_slice()]).unwrap();
    page.replace_text(y, 6, 0, &"a".repeat(70_000)).unwrap();
    page.replace_text(y, 6, 70_000, "").unwrap();
    // Un double de X, à la fin de la page, que le serveur n'a pas encore dédoublonné.
    let state = with_a_double_of_x(&page.encode_state(), x);
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let typed = tom.replace_text(x, 6, 0, " de Tom").unwrap();
    let mona = PageDoc::from_updates([state.as_slice()]).unwrap();
    mona.move_block(x, Some(z)).unwrap();
    // Mona revient en ligne : son étape 2 porte tout son jeu de suppressions.
    let moved = mona.encode_state();
    let server = PageDoc::from_updates([state.as_slice(), typed.as_slice()]).unwrap();
    let applied = server.apply_client_update(&moved).unwrap();
    assert_eq!(server.original_walks(), 1, "un parcours de la page");
    assert!(applied.carried.is_some(), "la frappe de Tom est reportée");
    let blocks = server.read_blocks();
    let copy = blocks
        .iter()
        .filter(|b| &b.id == x)
        .find(|b| block_text(b) != "Leurre");
    assert_eq!(copy.map(block_text).as_deref(), Some("Bloc X de Tom"));

    // Le même déplacement, sans les 70 000 horloges : pas de parcours.
    let (state, ids) = page_of(&[
        para("Bloc X", vec![]),
        para("Bloc Y", vec![]),
        para("Bloc Z", vec![]),
    ]);
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let typed = tom.replace_text(&ids[0], 6, 0, " de Tom").unwrap();
    let mona = PageDoc::from_updates([state.as_slice()]).unwrap();
    let moved = mona.move_block(&ids[0], Some(&ids[2])).unwrap();
    let server = PageDoc::from_updates([state.as_slice(), typed.as_slice()]).unwrap();
    assert!(
        server
            .apply_client_update(&moved)
            .unwrap()
            .carried
            .is_some()
    );
    assert_eq!(server.original_walks(), 0, "aucun parcours");
}

/// I3 (M10) : X est vide ; Tom remplace son paragraphe par un autre, où il tape, pendant que
/// Mona déplace X. Le nœud de contenu de l'original n'est pas celui que Mona avait vu : rien
/// n'est reporté (Ruling R377).
#[test]
fn a_content_node_replaced_by_someone_else_is_left_alone() {
    let (state, ids) = page_of(&[para("", vec![]), para("Bloc Y", vec![])]);
    let x = &ids[0];
    let doc = raw::doc(&state);
    let frag = raw::fragment(&doc);
    let before = {
        use yrs::{ReadTxn, Transact};
        doc.transact().state_vector()
    };
    {
        use yrs::{Text, Transact, XmlElementPrelim, XmlFragment, XmlTextPrelim};
        let mut txn = doc.transact_mut();
        let (c, _) = raw::top_block(&frag, &txn, x);
        c.remove_range(&mut txn, 0, 1);
        let p = c.insert(&mut txn, 0, XmlElementPrelim::empty("paragraph"));
        let t = p.push_back(&mut txn, XmlTextPrelim::new(""));
        t.insert(&mut txn, 0, "Tom");
    }
    let typed = raw::update_with(&doc, &before, &[]);
    let mona = PageDoc::from_updates([state.as_slice()]).unwrap();
    let moved = mona.move_block(x, Some(&ids[1])).unwrap();
    let played = play_updates(&state, &moved, &typed, &[Who::Tom, Who::Mona]);
    assert!(played.carried.is_empty());
    assert_eq!(
        block_text(find(&played.server.read_blocks(), x).unwrap()),
        ""
    );
}

/// I3 (M12) : Mona change X en titre et le déplace, hors ligne ; Tom tape à la fin de X. Le nœud
/// de contenu de la copie n'a plus la balise de l'original : rien n'est reporté (Ruling R377).
#[test]
fn a_type_changed_by_the_mover_is_left_alone() {
    let (state, ids) = page_of(&[para("Bloc X", vec![]), para("Bloc Y", vec![])]);
    let x = &ids[0];
    let tom = PageDoc::from_updates([state.as_slice()]).unwrap();
    let typed = tom.replace_text(x, 6, 0, " de Tom").unwrap();
    let doc = raw::doc(&state);
    let frag = raw::fragment(&doc);
    let before = {
        use yrs::{ReadTxn, Transact};
        doc.transact().state_vector()
    };
    {
        use yrs::{Transact, XmlFragment};
        let mut txn = doc.transact_mut();
        let root = raw::root(&frag, &txn);
        let len = root.len(&txn);
        raw::container(&mut txn, &root, len, x, "heading", "Bloc X");
        root.remove_range(&mut txn, 0, 1);
    }
    let moved = raw::update_with(&doc, &before, &[]);
    let played = play_updates(&state, &moved, &typed, &[Who::Tom, Who::Mona]);
    assert!(played.carried.is_empty());
    let blocks = played.server.read_blocks();
    let copy = find(&blocks, x).unwrap();
    assert_eq!(
        (copy.kind.as_str(), block_text(copy).as_str()),
        ("heading", "Bloc X")
    );
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
