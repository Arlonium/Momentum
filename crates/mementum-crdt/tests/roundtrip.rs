//! Aller-retour arbre → Yjs (format BlockNote) → arbre sur des arbres
//! aléatoires (`arb::tree`), et forme Yjs d'un saut de ligne.

use std::collections::BTreeSet;

use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{Block, Content, Inline, Styles, arb, normalize};
use proptest::prelude::*;
use yrs::any::Number;
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, OffsetKind, Options, Out, Text, Transact, Update, Xml, XmlElementPrelim, XmlFragment,
    XmlOut,
};

/// Ce que Yjs peut distinguer d'un arbre canonique :
/// - une page vide n'existe pas pour BlockNote : `write_blocks(&[])` écrit
///   un paragraphe vide, comme `new_page` ;
/// - deux liens voisins de même `href` ne font qu'un (une marque Yjs, comme
///   une marque ProseMirror, ne sépare pas deux plages de même valeur).
fn yjs_view(t: &[Block]) -> Vec<Block> {
    if t.is_empty() {
        return vec![Block {
            id: String::new(),
            kind: "paragraph".into(),
            props: Default::default(),
            content: Content::Inline(vec![]),
            children: vec![],
        }];
    }
    normalize(t.iter().cloned().map(merge_block_links).collect())
}

fn merge_block_links(mut b: Block) -> Block {
    b.content = match b.content {
        Content::Inline(c) => Content::Inline(merge_links(c)),
        Content::Table(mut t) => {
            for cell in t.rows.iter_mut().flatten() {
                cell.content = merge_links(std::mem::take(&mut cell.content));
            }
            Content::Table(t)
        }
        Content::None => Content::None,
    };
    b.children = b.children.into_iter().map(merge_block_links).collect();
    b
}

fn merge_links(content: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for inline in content {
        if let (
            Some(Inline::Link {
                href: prev,
                children: kept,
            }),
            Inline::Link { href, children },
        ) = (out.last_mut(), &inline)
            && prev == href
        {
            kept.extend(children.iter().cloned());
            continue;
        }
        out.push(inline);
    }
    out
}

/// Vide les ids de l'arbre et les renvoie dans l'ordre du document.
fn take_ids(blocks: &mut [Block], out: &mut Vec<String>) {
    for b in blocks {
        out.push(std::mem::take(&mut b.id));
        take_ids(&mut b.children, out);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn prop_roundtrip_through_yjs(t in arb::tree()) {
        let (page, _) = PageDoc::new_page();
        page.write_blocks(&t);
        let back = page.read_blocks();

        // Les mêmes blocs, une fois la page encodée puis relue.
        let state = page.encode_state();
        let decoded = PageDoc::from_updates([state.as_slice()]).expect("état relisible");
        prop_assert_eq!(&decoded.read_blocks(), &back);

        let mut expected = yjs_view(&normalize(t.clone()));
        let mut got = back.clone();
        let (mut want_ids, mut got_ids) = (Vec::new(), Vec::new());
        take_ids(&mut expected, &mut want_ids);
        take_ids(&mut got, &mut got_ids);
        prop_assert_eq!(&got, &expected);

        // Un id donné est gardé à sa première occurrence (ordre du document) ;
        // un id vide ou répété est généré (UUIDv7).
        let mut seen = BTreeSet::new();
        for (want, got) in want_ids.iter().zip(&got_ids) {
            if !want.is_empty() && seen.insert(want.as_str()) {
                prop_assert_eq!(got, want);
            } else {
                let uuid = uuid::Uuid::parse_str(got);
                prop_assert!(
                    uuid.is_ok_and(|u| u.get_version_num() == 7),
                    "id généré attendu, lu « {} »",
                    got
                );
            }
        }
    }
}

/// Page construite à la main : un tableau d'une ligne de deux cellules
/// `tag`, avec les attributs `table_attrs` sur `table`.
fn one_row_table(tag: &str, table_attrs: &[(&str, i64)]) -> PageDoc {
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = {
        let mut txn = doc.transact_mut();
        let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        let container = root.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "t");
        let table = container.push_back(&mut txn, XmlElementPrelim::empty("table"));
        for (k, v) in table_attrs {
            table.insert_attribute(&mut txn, *k, Any::Number(Number::Int(*v)));
        }
        let row = table.push_back(&mut txn, XmlElementPrelim::empty("tableRow"));
        for _ in 0..2 {
            let cell = row.push_back(&mut txn, XmlElementPrelim::empty(tag));
            cell.push_back(&mut txn, XmlElementPrelim::empty("tableParagraph"));
        }
        txn.encode_update_v1()
    };
    PageDoc::from_updates([update.as_slice()]).unwrap()
}

fn headers(page: &PageDoc) -> (u32, u32) {
    match &page.read_blocks()[0].content {
        Content::Table(t) => (t.header_rows, t.header_cols),
        other => panic!("tableau attendu, trouvé {other:?}"),
    }
}

#[test]
fn table_headers_through_yjs() {
    // Écrit par BlockNote : un tableau tout en en-têtes vaut (lignes, 0) (Ruling R19).
    assert_eq!(headers(&one_row_table("tableHeader", &[])), (1, 0));
    // Indices `headerRows` / `headerCols` qui concordent avec les cellules : gardés.
    let hinted = [("headerRows", 1), ("headerCols", 0)];
    assert_eq!(headers(&one_row_table("tableHeader", &hinted)), (1, 0));
    // Indices périmés (les cellules ont changé depuis) : ignorés.
    assert_eq!(headers(&one_row_table("tableCell", &hinted)), (0, 0));

    // Écrit par le serveur : les en-têtes reviennent tels quels.
    let (page, _) = PageDoc::new_page();
    let table = mementum_md::parse("| a | b |\n| --- | --- |\n").unwrap();
    page.write_blocks(&table);
    assert_eq!(headers(&page), (1, 0));
}

#[test]
fn hard_break_is_a_yjs_element() {
    let text = |s: &str| Inline::Text {
        text: s.into(),
        styles: Styles::default(),
    };
    let paragraph = Block {
        id: String::new(),
        kind: "paragraph".into(),
        props: Default::default(),
        content: Content::Inline(vec![text("a"), Inline::HardBreak, text("b")]),
        children: vec![],
    };
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&[paragraph]);

    // Lecture du document brut, dans un Doc à part.
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let state = page.encode_state();
    doc.transact_mut()
        .apply_update(Update::decode_v1(&state).unwrap())
        .unwrap();
    let txn = doc.transact();

    let element = |node: Option<XmlOut>| match node {
        Some(XmlOut::Element(e)) => e,
        other => panic!("élément attendu, trouvé {other:?}"),
    };
    let group = element(fragment.get(&txn, 0));
    let container = element(group.get(&txn, 0));
    let node = element(container.get(&txn, 0));
    assert_eq!(node.tag().as_ref(), "paragraph");

    let children: Vec<XmlOut> = node.children(&txn).collect();
    assert_eq!(children.len(), 3, "XmlText, hardBreak, XmlText attendus");
    let plain = |node: &XmlOut| match node {
        XmlOut::Text(t) => t
            .diff(&txn, YChange::identity)
            .into_iter()
            .map(|d| match d.insert {
                Out::Any(Any::String(s)) => s.to_string(),
                other => panic!("texte attendu, trouvé {other:?}"),
            })
            .collect::<String>(),
        other => panic!("XmlText attendu, trouvé {other:?}"),
    };
    assert_eq!(plain(&children[0]), "a");
    match &children[1] {
        XmlOut::Element(e) => assert_eq!(e.tag().as_ref(), "hardBreak"),
        other => panic!("élément hardBreak attendu, trouvé {other:?}"),
    }
    assert_eq!(plain(&children[2]), "b");
}
