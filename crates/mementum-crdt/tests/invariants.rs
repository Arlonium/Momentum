//! Ruling R10 : un document écrit par un client buggé ou malveillant se lit
//! en un arbre que `render` écrit en MMD relisible par `parse`.

use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{Block, Content, MAX_DEPTH, PropValue, RenderOpts, block_text, parse, render};
use yrs::any::Number;
use yrs::{
    Any, Doc, OffsetKind, Options, Transact, TransactionMut, Xml, XmlElementPrelim, XmlElementRef,
    XmlFragment, XmlTextPrelim,
};

/// Page construite à la main : `build` reçoit la racine `blockGroup`.
fn page_from(build: impl FnOnce(&mut TransactionMut, &XmlElementRef)) -> PageDoc {
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = {
        let mut txn = doc.transact_mut();
        let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        build(&mut txn, &root);
        txn.encode_update_v1()
    };
    PageDoc::from_updates([update.as_slice()]).expect("mise à jour lisible")
}

/// `blockContainer[id] > tag[attrs]`, avec un texte s'il est donné.
fn block(
    txn: &mut TransactionMut,
    parent: &XmlElementRef,
    id: &str,
    tag: &str,
    attrs: &[(&str, Any)],
    text: Option<&str>,
) -> (XmlElementRef, XmlElementRef) {
    let container = parent.push_back(txn, XmlElementPrelim::empty("blockContainer"));
    container.insert_attribute(txn, "id", id);
    let node = container.push_back(txn, XmlElementPrelim::empty(tag));
    for (k, v) in attrs {
        node.insert_attribute(txn, *k, v.clone());
    }
    if let Some(t) = text {
        node.push_back(txn, XmlTextPrelim::new(t));
    }
    (container, node)
}

/// `tableCell[colwidth] > tableParagraph > texte`.
fn cell(txn: &mut TransactionMut, row: &XmlElementRef, width: Option<Any>, text: &str) {
    let cell = row.push_back(txn, XmlElementPrelim::empty("tableCell"));
    if let Some(w) = width {
        cell.insert_attribute(txn, "colwidth", Any::Array(vec![w].into()));
    }
    let paragraph = cell.push_back(txn, XmlElementPrelim::empty("tableParagraph"));
    paragraph.push_back(txn, XmlTextPrelim::new(text));
}

fn without_ids(mut blocks: Vec<Block>) -> Vec<Block> {
    fn clear(blocks: &mut [Block]) {
        for b in blocks {
            b.id.clear();
            clear(&mut b.children);
        }
    }
    clear(&mut blocks);
    blocks
}

/// `render` écrit un MMD que `parse` relit en le même arbre.
fn assert_renders_readable(blocks: &[Block]) {
    let markdown = render(blocks, RenderOpts::default()).markdown;
    let parsed = parse(&markdown).unwrap_or_else(|e| panic!("{e}\nMMD :\n{markdown}"));
    assert_eq!(parsed, without_ids(blocks.to_vec()), "MMD :\n{markdown}");
}

fn depth(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|b| 1 + depth(&b.children))
        .max()
        .unwrap_or(0)
}

fn texts(blocks: &[Block], out: &mut Vec<String>) {
    for b in blocks {
        out.push(block_text(b));
        texts(&b.children, out);
    }
}

#[test]
fn ill_typed_props_and_empty_table() {
    let nan = || Any::Number(Number::Float(f64::NAN));
    let page = page_from(|txn, root| {
        // `level` en chaîne : converti sans perte ; illisible : défaut.
        block(
            txn,
            root,
            "h2",
            "heading",
            &[("level", Any::from("2"))],
            Some("Deux"),
        );
        block(
            txn,
            root,
            "hx",
            "heading",
            &[("level", Any::from("abc"))],
            Some("Un"),
        );
        // Largeurs NaN : défaut (absente) ; largeur en chaîne : convertie.
        block(
            txn,
            root,
            "img",
            "image",
            &[
                ("url", Any::from("https://x.dev/a.png")),
                ("previewWidth", nan()),
            ],
            None,
        );
        let list = root.push_back(txn, XmlElementPrelim::empty("columnList"));
        list.insert_attribute(txn, "id", "cols");
        for (id, width) in [("c1", nan()), ("c2", Any::from("2"))] {
            let column = list.push_back(txn, XmlElementPrelim::empty("column"));
            column.insert_attribute(txn, "id", id);
            column.insert_attribute(txn, "width", width);
            block(txn, &column, &format!("{id}p"), "paragraph", &[], Some(id));
        }
        // Tableau sans ligne.
        block(txn, root, "t0", "table", &[], None);
        // Tableau dont la première ligne n'a pas de cellule.
        let (_, table) = block(txn, root, "t1", "table", &[], None);
        table.push_back(txn, XmlElementPrelim::empty("tableRow"));
        let row = table.push_back(txn, XmlElementPrelim::empty("tableRow"));
        cell(txn, &row, None, "cellule");
        // Largeurs NaN et hors des entiers sûrs (que `parse` ne relirait pas) : automatiques.
        let (_, table) = block(txn, root, "t2", "table", &[], None);
        let row = table.push_back(txn, XmlElementPrelim::empty("tableRow"));
        for width in [
            nan(),
            Any::Number(Number::Float(1e21)),
            Any::Number(Number::Int(120)),
        ] {
            cell(txn, &row, Some(width), "w");
        }
        // Props hors registre dont le nom ne s'écrit pas en MMD : ignorées.
        block(
            txn,
            root,
            "q",
            "quote",
            &[
                ("bg", Any::from("red")),
                ("a b", Any::from("x")),
                ("fooBar", Any::from(true)),
            ],
            Some("Citation"),
        );
    });

    let blocks = page.read_blocks();
    assert_renders_readable(&blocks);

    let kinds: Vec<&str> = blocks.iter().map(|b| b.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "heading",
            "heading",
            "image",
            "columnList",
            "table",
            "table",
            "table",
            "quote"
        ]
    );
    assert_eq!(blocks[0].props.get("level"), Some(&PropValue::Int(2)));
    assert_eq!(blocks[1].props.get("level"), None);
    assert_eq!(blocks[2].props.get("previewWidth"), None);
    let columns = &blocks[3].children;
    assert_eq!(columns[0].props.get("width"), None);
    assert_eq!(columns[1].props.get("width"), Some(&PropValue::Float(2.0)));
    let Content::Table(empty) = &blocks[4].content else {
        panic!("tableau attendu")
    };
    assert_eq!(empty.rows.len(), 1);
    assert_eq!(empty.rows[0].len(), 1);
    assert!(empty.rows[0][0].content.is_empty());
    let Content::Table(t1) = &blocks[5].content else {
        panic!("tableau attendu")
    };
    assert_eq!(t1.rows.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1]);
    assert_eq!(t1.column_widths, [None]);
    let Content::Table(t2) = &blocks[6].content else {
        panic!("tableau attendu")
    };
    assert_eq!(t2.column_widths, [None, None, Some(120.0)]);
    assert_eq!(
        blocks[7].props.keys().collect::<Vec<_>>(),
        ["fooBar"],
        "seule la prop au nom valide reste"
    );
}

#[test]
fn deep_nesting_is_bounded() {
    const LEVELS: usize = 150;
    let page = page_from(|txn, root| {
        let mut group = root.clone();
        for i in 1..=LEVELS {
            let (container, _) = block(
                txn,
                &group,
                &format!("n{i}"),
                "paragraph",
                &[],
                Some(&format!("niveau {i}")),
            );
            if i < LEVELS {
                group = container.push_back(txn, XmlElementPrelim::empty("blockGroup"));
            }
        }
    });

    let blocks = page.read_blocks();
    assert_eq!(depth(&blocks), MAX_DEPTH);
    let mut all = Vec::new();
    texts(&blocks, &mut all);
    let expected: Vec<String> = (1..=LEVELS).map(|i| format!("niveau {i}")).collect();
    assert_eq!(all, expected, "aucun bloc perdu, ordre du document gardé");
    assert_renders_readable(&blocks);
}
