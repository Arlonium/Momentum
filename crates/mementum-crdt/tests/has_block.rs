//! Dire si un bloc existe sans relire la page (constat S7 de la relecture finale, Ruling R206) :
//! `PageDoc::has_block` répond comme une recherche dans `read_blocks`, à toute profondeur, sans
//! bâtir la liste des blocs (ni leur texte, ni leurs props).

use std::time::{Duration, Instant};

use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{Block, arb, parse};
use proptest::prelude::*;
use yrs::{Doc, ReadTxn, Transact, Xml, XmlElementPrelim, XmlFragment, XmlTextPrelim};

/// Les identifiants des blocs lus, à toute profondeur.
fn read_ids(blocks: &[Block]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<&Block> = blocks.iter().collect();
    while let Some(block) = stack.pop() {
        out.push(block.id.clone());
        stack.extend(block.children.iter());
    }
    out
}

/// `has_block` dit, pour chaque identifiant lu et pour d'autres, ce que dit `read_blocks`.
fn assert_agrees(page: &PageDoc, others: &[&str]) {
    let ids = read_ids(&page.read_blocks());
    for id in &ids {
        assert!(page.has_block(id), "bloc lu, mais introuvable : {id}");
    }
    for other in others {
        assert_eq!(
            page.has_block(other),
            ids.iter().any(|id| id == other),
            "« {other} » parmi {ids:?}"
        );
    }
    assert!(!page.has_block(""), "un identifiant vide n'est pas un bloc");
}

/// Une page dont le fragment porte `build` (des éléments posés à la main, comme un client
/// pourrait les écrire), relue par un `PageDoc`.
fn page_from(build: impl FnOnce(&Doc)) -> PageDoc {
    let doc = Doc::new();
    build(&doc);
    let state = doc
        .transact()
        .encode_state_as_update_v1(&yrs::StateVector::default());
    let (page, _) = PageDoc::new_page();
    page.apply_update(&state).unwrap();
    page
}

#[test]
fn has_block_agrees_with_read_blocks_on_every_form() {
    for (what, mmd) in [
        (
            "toutes les formes",
            include_str!("../../../spec/mmd/fixtures/all-blocks.mmd"),
        ),
        (
            "cas limites",
            include_str!("../../../spec/mmd/fixtures/p1-edge-cases.mmd"),
        ),
    ] {
        let (page, _) = PageDoc::new_page();
        page.write_blocks(&parse(mmd).unwrap())
            .unwrap_or_else(|e| panic!("{what} : {e}"));
        assert_agrees(&page, &["inconnu", "pg_spec01"]);
    }
}

/// Des structures que BlockNote n'écrit pas, mais qu'un client peut écrire : une liste de
/// colonnes d'une seule colonne (défaite à la lecture : ses blocs prennent sa place, la liste et
/// la colonne ne sont pas des blocs), une colonne hors de toute liste, un nœud étranger hors
/// d'un `blockContainer`, un `blockContainer` hors de tout `blockGroup` (pas lu).
#[test]
fn has_block_agrees_with_read_blocks_on_malformed_structures() {
    let page = page_from(|doc| {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let group = fragment.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
        let list = group.insert(&mut txn, 0, XmlElementPrelim::empty("columnList"));
        list.insert_attribute(&mut txn, "id", "liste-seule");
        let column = list.insert(&mut txn, 0, XmlElementPrelim::empty("column"));
        column.insert_attribute(&mut txn, "id", "colonne-seule");
        let container = column.insert(&mut txn, 0, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "dans-la-colonne");
        let paragraph = container.insert(&mut txn, 0, XmlElementPrelim::empty("paragraph"));
        paragraph.insert(&mut txn, 0, XmlTextPrelim::new("Texte"));
        let stray = group.insert(&mut txn, 1, XmlElementPrelim::empty("column"));
        stray.insert_attribute(&mut txn, "id", "colonne-perdue");
        let inside = stray.insert(&mut txn, 0, XmlElementPrelim::empty("blockContainer"));
        inside.insert_attribute(&mut txn, "id", "sous-la-colonne-perdue");
        let foreign = group.insert(&mut txn, 2, XmlElementPrelim::empty("inconnu"));
        foreign.insert_attribute(&mut txn, "id", "etranger");
        let outside = fragment.insert(&mut txn, 1, XmlElementPrelim::empty("blockContainer"));
        outside.insert_attribute(&mut txn, "id", "hors-du-groupe");
    });
    assert_agrees(
        &page,
        &[
            "liste-seule",
            "colonne-seule",
            "dans-la-colonne",
            "colonne-perdue",
            "sous-la-colonne-perdue",
            "etranger",
            "hors-du-groupe",
        ],
    );
}

/// Au-delà de la profondeur du MMD, la lecture aplatit les descendants : ils restent des blocs.
#[test]
fn has_block_agrees_with_read_blocks_beyond_the_depth() {
    let depth = mementum_md::MAX_DEPTH + 5;
    let page = page_from(|doc| {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let mut group = fragment.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
        for level in 0..depth {
            let container = group.insert(&mut txn, 0, XmlElementPrelim::empty("blockContainer"));
            container.insert_attribute(&mut txn, "id", format!("niveau-{level}"));
            let item = container.insert(&mut txn, 0, XmlElementPrelim::empty("bulletListItem"));
            item.insert(&mut txn, 0, XmlTextPrelim::new(format!("Niveau {level}")));
            group = container.insert(&mut txn, 1, XmlElementPrelim::empty("blockGroup"));
        }
    });
    let deepest = format!("niveau-{}", depth - 1);
    assert!(read_ids(&page.read_blocks()).contains(&deepest));
    assert_agrees(&page, &[&deepest, "niveau-0", "niveau-absent"]);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn has_block_agrees_with_read_blocks(t in arb::tree()) {
        let (page, _) = PageDoc::new_page();
        page.write_blocks(&t).unwrap();
        assert_agrees(&page, &["absent", "01890000-0000-7000-8000-000000000000"]);
    }
}

/// `has_block` ne bâtit pas la liste des blocs : sur une page de 2 000 blocs, il coûte bien
/// moins qu'une lecture des blocs. Le meilleur de dix essais de chacun, en alternance : une
/// recherche qui relirait la page coûterait autant qu'elle, et le test échouerait ; la borne (un
/// cinquième) ne peut le faire passer à tort, seulement échouer sur un poste très chargé.
#[test]
fn has_block_costs_far_less_than_reading_the_blocks() {
    let mmd: String = (0..2_000)
        .map(|i| format!("Paragraphe {i}, avec du **gras** et un [lien](https://exemple.fr)."))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(&mmd).unwrap()).unwrap();
    let last = page.read_blocks().last().unwrap().id.clone();
    let (mut lookup, mut read) = (Duration::MAX, Duration::MAX);
    for _ in 0..10 {
        let start = Instant::now();
        assert!(page.has_block(&last));
        lookup = lookup.min(start.elapsed());
        let start = Instant::now();
        assert!(read_ids(&page.read_blocks()).contains(&last));
        read = read.min(start.elapsed());
    }
    assert!(
        lookup * 5 < read,
        "recherche {lookup:?}, lecture des blocs {read:?}"
    );
}

/// Mesure, à lancer en release : chercher un bloc dans une page de 10 000 blocs (le dernier, le
/// pire cas, et un absent), contre la lecture des blocs que faisait la route des commentaires.
#[test]
#[ignore = "mesure, à lancer en --release"]
fn measure_has_block_of_10000_blocks() {
    let mmd: String = (0..10_000)
        .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(&mmd).unwrap()).unwrap();
    let last = page.read_blocks().last().unwrap().id.clone();
    let measure = |work: &dyn Fn()| {
        let mut times: Vec<Duration> = (0..20)
            .map(|_| {
                let start = Instant::now();
                work();
                start.elapsed()
            })
            .collect();
        times.sort();
        (times[10], times[19])
    };
    let (found, found_max) = measure(&|| assert!(page.has_block(&last)));
    let (absent, absent_max) = measure(&|| assert!(!page.has_block("absent")));
    let (read, read_max) = measure(&|| assert!(read_ids(&page.read_blocks()).contains(&last)));
    println!(
        "MESURE bloc cherché dans une page de 10 000 blocs : le dernier p50 {found:?} (max \
         {found_max:?}), un absent p50 {absent:?} (max {absent_max:?}) ; lecture des blocs p50 \
         {read:?} (max {read_max:?})"
    );
}
