//! Schéma des colonnes de BlockNote (Ruling R18) : `move_block`,
//! `insert_blocks_after` et `dedupe_blocks` ne le cassent jamais, et la page
//! reste lisible en MMD après chaque opération.

use mementum_crdt::{CrdtError, FRAGMENT, PageDoc};
use mementum_md::{Block, RenderOpts, parse, render};
use yrs::updates::decoder::Decode;
use yrs::{
    Doc, OffsetKind, Options, Transact, TransactionMut, Update, Xml, XmlElementPrelim,
    XmlElementRef, XmlFragment, XmlOut, XmlTextPrelim,
};

const TWO_LISTS: &str = "<columns>\n<column>\nA1\n</column>\n<column>\nA2\n</column>\n</columns>\n\n<columns>\n<column>\nB1\n</column>\n<column>\nB2\n</column>\n</columns>\n\nP\n";

fn page(mmd: &str) -> PageDoc {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(mmd).unwrap()).unwrap();
    page
}

fn raw_doc() -> Doc {
    Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    })
}

/// Page construite à la main : `build` reçoit la racine `blockGroup`.
fn page_from(build: impl FnOnce(&mut TransactionMut, &XmlElementRef)) -> PageDoc {
    let doc = raw_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = {
        let mut txn = doc.transact_mut();
        let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        build(&mut txn, &root);
        txn.encode_update_v1()
    };
    PageDoc::from_updates([update.as_slice()]).unwrap()
}

fn paragraph(
    txn: &mut TransactionMut,
    parent: &XmlElementRef,
    id: &str,
    text: &str,
) -> XmlElementRef {
    let container = parent.push_back(txn, XmlElementPrelim::empty("blockContainer"));
    container.insert_attribute(txn, "id", id);
    let p = container.push_back(txn, XmlElementPrelim::empty("paragraph"));
    p.push_back(txn, XmlTextPrelim::new(text));
    container
}

fn column(txn: &mut TransactionMut, list: &XmlElementRef, id: &str) -> XmlElementRef {
    let column = list.push_back(txn, XmlElementPrelim::empty("column"));
    column.insert_attribute(txn, "id", id);
    column
}

/// La page respecte le schéma de BlockNote et se relit en MMD.
fn assert_sound(page: &PageDoc) {
    fn schema(blocks: &[Block], in_list: bool) {
        for b in blocks {
            assert_eq!(b.kind == "column", in_list, "colonne mal placée : {b:?}");
            if b.kind == "columnList" {
                assert!(b.children.len() >= 2, "liste de moins de 2 colonnes");
            }
            if b.kind == "column" {
                assert!(!b.children.is_empty(), "colonne vide");
                assert!(b.children.iter().all(|c| c.kind != "columnList"));
            }
            schema(&b.children, b.kind == "columnList");
        }
    }
    let blocks = page.read_blocks();
    schema(&blocks, false);
    let markdown = render(&blocks, RenderOpts::default()).markdown;
    parse(&markdown).unwrap_or_else(|e| panic!("{e}\n{markdown}"));
    // Ni colonne vide, ni liste d'une colonne, ni `blockGroup` vide dans Yjs.
    let doc = raw_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(&page.encode_state()).unwrap())
        .unwrap();
    let txn = doc.transact();
    for node in fragment.successors(&txn) {
        if let XmlOut::Element(e) = node {
            let n = e.children(&txn).count();
            match e.tag().as_ref() {
                "blockGroup" | "column" => assert!(n > 0, "{} vide", e.tag()),
                "columnList" => assert!(n >= 2, "liste de {n} colonne"),
                _ => {}
            }
        }
    }
}

fn ids(blocks: &[Block], out: &mut Vec<String>) {
    for b in blocks {
        out.push(b.id.clone());
        ids(&b.children, out);
    }
}

#[test]
fn probe_cases_are_refused() {
    let page = page(TWO_LISTS);
    let blocks = page.read_blocks();
    let (a, b, p) = (&blocks[0], &blocks[1], &blocks[2]);
    let refused = |r: Result<Vec<u8>, CrdtError>| matches!(r, Err(CrdtError::Structure(_)));

    // Une colonne déplacée vers l'autre liste laisserait la sienne à une colonne.
    assert!(refused(
        page.move_block(&a.children[0].id, Some(&b.children[0].id))
    ));
    // Le seul bloc d'une colonne, déplacé ailleurs, la viderait.
    assert!(refused(
        page.move_block(&a.children[1].children[0].id, Some(&p.id))
    ));
    // Une liste de colonnes insérée directement dans une colonne.
    let list =
        parse("<columns>\n<column>\nX\n</column>\n<column>\nY\n</column>\n</columns>\n").unwrap();
    assert!(refused(page.insert_blocks_after(
        Some(&b.children[1].children[0].id),
        &list
    )));
    // Des colonnes mal formées insérées dans la page.
    let one = vec![Block {
        children: vec![list[0].children[0].clone()],
        ..list[0].clone()
    }];
    assert!(refused(page.insert_blocks_after(Some(&p.id), &one)));
    assert!(refused(
        page.insert_blocks_after(Some(&p.id), &list[0].children)
    ));
    assert!(refused(page.write_blocks(&one)));

    assert_eq!(page.read_blocks(), blocks, "rien n'a changé");
    assert_sound(&page);
}

#[test]
fn allowed_column_moves() {
    let page = page(
        "<columns>\n<column>\nA1\n</column>\n<column>\nA2\n</column>\n<column>\nA3\n</column>\n</columns>\n\n<columns>\n<column>\nB1\n</column>\n<column>\nB2\n</column>\n</columns>\n\nP\n",
    );
    let blocks = page.read_blocks();
    // Dans sa liste, ou vers une autre quand la sienne garde deux colonnes.
    page.move_block(&blocks[0].children[0].id, Some(&blocks[0].children[2].id))
        .expect("dans sa liste");
    assert_sound(&page);
    page.move_block(&blocks[0].children[1].id, Some(&blocks[1].children[1].id))
        .expect("vers l'autre liste");
    assert_sound(&page);
    let after = page.read_blocks();
    assert_eq!(after[0].children.len(), 2);
    assert_eq!(after[1].children.len(), 3);
    // Un bloc d'une colonne qui en garde un autre : permis.
    page.insert_blocks_after(
        Some(&after[0].children[0].children[0].id),
        &parse("C\n").unwrap(),
    )
    .unwrap();
    page.move_block(&after[0].children[0].children[0].id, Some(&after[2].id))
        .expect("la colonne garde un bloc");
    assert_sound(&page);
}

#[test]
fn moving_out_the_last_child_removes_the_empty_group() {
    let page = page("- A\n  - B\n\nC\n");
    let blocks = page.read_blocks();
    page.move_block(&blocks[0].children[0].id, Some(&blocks[1].id))
        .unwrap();
    assert_eq!(
        render(&page.read_blocks(), RenderOpts::default()).markdown,
        "- A\n\nC\n\n- B\n"
    );
    assert_sound(&page);
}

#[test]
fn concurrent_column_moves_then_dedupe() {
    let base = page(
        "<columns>\n<column>\nC1\n</column>\n<column>\nC2\n</column>\n<column>\nC3\n</column>\n</columns>\n\n<columns>\n<column>\nM1\n</column>\n<column>\nM2\n</column>\n</columns>\n",
    );
    let state = base.encode_state();
    let a = PageDoc::from_updates([state.as_slice()]).unwrap();
    let b = PageDoc::from_updates([state.as_slice()]).unwrap();
    let blocks = a.read_blocks();
    let (c1, m1, m2) = (
        &blocks[0].children[0].id,
        &blocks[1].children[0].id,
        &blocks[1].children[1].id,
    );
    let from_a = a.move_block(c1, Some(m1)).unwrap();
    let from_b = b.move_block(c1, Some(m2)).unwrap();
    a.apply_update(&from_b).unwrap();
    b.apply_update(&from_a).unwrap();

    let fix = a.dedupe_blocks().expect("une colonne en double");
    b.apply_update(&fix).unwrap();
    assert_eq!(a.read_blocks(), b.read_blocks());
    let after = a.read_blocks();
    assert_eq!(after[0].children.len(), 2);
    assert_eq!(after[1].children.len(), 3);
    let mut all = Vec::new();
    ids(&after, &mut all);
    assert_eq!(all.iter().filter(|id| *id == c1).count(), 1);
    assert_sound(&a);
    assert_eq!(a.dedupe_blocks(), None);
}

#[test]
fn dedupe_repairs_without_moving_content() {
    let page = page_from(|txn, root| {
        // Liste de deux colonnes de même id : supprimer la seconde laisserait
        // la liste à une colonne, elle reçoit un nouvel id ; son bloc en double
        // part, et la colonne vidée reçoit un paragraphe vide.
        let list = root.push_back(txn, XmlElementPrelim::empty("columnList"));
        list.insert_attribute(txn, "id", "L");
        for _ in 0..2 {
            let c = column(txn, &list, "C");
            paragraph(txn, &c, "P", "dans la colonne");
        }
        // Liste dont une colonne ne contient qu'un doublon : vidée, elle
        // reçoit un paragraphe vide.
        let list = root.push_back(txn, XmlElementPrelim::empty("columnList"));
        list.insert_attribute(txn, "id", "M");
        let c = column(txn, &list, "M1");
        paragraph(txn, &c, "Q", "premier");
        let c = column(txn, &list, "M2");
        paragraph(txn, &c, "Q", "doublon");
        // Un enfant en double : le `blockGroup` vidé est retiré.
        paragraph(txn, root, "X", "original");
        let parent = paragraph(txn, root, "A", "parent");
        let group = parent.push_back(txn, XmlElementPrelim::empty("blockGroup"));
        paragraph(txn, &group, "X", "doublon");
    });

    page.dedupe_blocks().expect("des doublons");
    assert_sound(&page);
    let blocks = page.read_blocks();
    let mut all = Vec::new();
    ids(&blocks, &mut all);
    let mut unique = all.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), all.len(), "ids uniques : {all:?}");
    assert_eq!(blocks[0].children.len(), 2, "la liste garde deux colonnes");
    assert_eq!(blocks[0].children[0].id, "C");
    assert_eq!(blocks[1].children.len(), 2);
    let text = render(&blocks, RenderOpts::default()).markdown;
    for kept in ["dans la colonne", "premier", "original", "parent"] {
        assert!(text.contains(kept), "« {kept} » perdu :\n{text}");
    }
    assert!(!text.contains("doublon"), "{text}");
    assert_eq!(page.dedupe_blocks(), None);
}
