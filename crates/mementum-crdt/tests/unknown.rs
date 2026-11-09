//! Nœud Yjs de type inconnu du serveur (Ruling R4), et page neuve.

use std::collections::HashMap;

use mementum_crdt::{FRAGMENT, META, PageDoc};
use mementum_md::{Content, PropValue, RenderOpts, parse, render};
use mementum_schema::SCHEMA_VERSION;
use yrs::any::Number;
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, Map, OffsetKind, Options, Out, ReadTxn, Text, Transact, Update, Xml,
    XmlElementPrelim, XmlElementRef, XmlFragment, XmlOut, XmlTextPrelim,
};

fn raw_doc() -> Doc {
    Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    })
}

/// Document brut décodé depuis l'état d'une page.
fn decode(page: &PageDoc) -> Doc {
    let doc = raw_doc();
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.get_or_insert_map(META);
    doc.transact_mut()
        .apply_update(Update::decode_v1(&page.encode_state()).unwrap())
        .unwrap();
    doc
}

fn find_element<T: ReadTxn>(txn: &T, tag: &str) -> Option<XmlElementRef> {
    let fragment = txn.get_xml_fragment(FRAGMENT)?;
    fragment.successors(txn).find_map(|n| match n {
        XmlOut::Element(e) if e.tag().as_ref() == tag => Some(e),
        _ => None,
    })
}

fn plain_text<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> String {
    element
        .children(txn)
        .filter_map(|c| match c {
            XmlOut::Text(t) => Some(t),
            _ => None,
        })
        .flat_map(|t| t.diff(txn, YChange::identity))
        .map(|d| match d.insert {
            Out::Any(Any::String(s)) => s.to_string(),
            other => panic!("texte attendu, trouvé {other:?}"),
        })
        .collect()
}

#[test]
fn unknown_node_passes_through() {
    // blockGroup > blockContainer[id] > futureWidget[a="1", n=2] > « x »
    let doc = raw_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = {
        let mut txn = doc.transact_mut();
        let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        let container = root.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "b_fw");
        let widget = container.push_back(&mut txn, XmlElementPrelim::empty("futureWidget"));
        widget.insert_attribute(&mut txn, "a", "1");
        widget.insert_attribute(&mut txn, "n", Any::Number(Number::Int(2)));
        widget.push_back(&mut txn, XmlTextPrelim::new("x"));
        txn.encode_update_v1()
    };

    let page = PageDoc::from_updates([update.as_slice()]).unwrap();
    let blocks = page.read_blocks();
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    assert_eq!(block.kind, "unknown");
    assert_eq!(block.id, "b_fw");
    assert_eq!(block.content, Content::None);
    assert_eq!(
        block.props.get("originalType"),
        Some(&PropValue::Str("futureWidget".into()))
    );
    assert_eq!(block.props.get("foreign"), Some(&PropValue::Bool(true)));
    assert!(matches!(block.props.get("data"), Some(PropValue::Str(d)) if !d.is_empty()));

    // Le MMD porte le nœud tel quel.
    let markdown = render(&blocks, RenderOpts::default()).markdown;
    assert!(markdown.starts_with("<block type=\"futureWidget\" id=\"b_fw\" data=\""));
    let parsed = parse(&markdown).expect("MMD relisible");
    assert_eq!(parsed, blocks);

    // Réécrit dans une page neuve, il redonne l'élément d'origine.
    let (fresh, _) = PageDoc::new_page();
    fresh.write_blocks(&parsed);
    assert_eq!(fresh.read_blocks(), blocks);
    let raw = decode(&fresh);
    let txn = raw.transact();
    let widget = find_element(&txn, "futureWidget").expect("futureWidget recréé");
    let attrs: HashMap<String, Any> = widget
        .attributes(&txn)
        .map(|(k, v)| match v {
            Out::Any(a) => (k.to_string(), a),
            other => panic!("attribut simple attendu, trouvé {other:?}"),
        })
        .collect();
    assert_eq!(
        attrs,
        HashMap::from([
            ("a".to_string(), Any::from("1")),
            ("n".to_string(), Any::Number(Number::Int(2))),
        ])
    );
    assert_eq!(plain_text(&txn, &widget), "x");
    let parent = match widget.parent() {
        Some(XmlOut::Element(p)) => p,
        other => panic!("blockContainer attendu, trouvé {other:?}"),
    };
    assert_eq!(parent.tag().as_ref(), "blockContainer");
}

#[test]
fn new_page_has_root_and_schema() {
    let (page, update) = PageDoc::new_page();
    let again = PageDoc::from_updates([update.as_slice()]).unwrap();
    for p in [&page, &again] {
        let blocks = p.read_blocks();
        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(b.kind, "paragraph");
        assert_eq!(b.content, Content::Inline(vec![]));
        assert!(b.props.is_empty() && b.children.is_empty());
        assert!(uuid::Uuid::parse_str(&b.id).is_ok_and(|u| u.get_version_num() == 7));
        assert_eq!(p.schema_version(), Some(SCHEMA_VERSION));
    }

    // Racine BlockNote : blockGroup > blockContainer > paragraph, et rien d'autre.
    let raw = decode(&page);
    let txn = raw.transact();
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    assert_eq!(fragment.len(&txn), 1);
    let Some(XmlOut::Element(root)) = fragment.get(&txn, 0) else {
        panic!("racine attendue")
    };
    assert_eq!(root.tag().as_ref(), "blockGroup");
    assert_eq!(root.len(&txn), 1);
    let Some(XmlOut::Element(container)) = root.get(&txn, 0) else {
        panic!("blockContainer attendu")
    };
    assert_eq!(container.tag().as_ref(), "blockContainer");
    let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
        panic!("paragraph attendu")
    };
    assert_eq!(paragraph.tag().as_ref(), "paragraph");
    let meta = txn.get_map(META).unwrap();
    assert_eq!(
        meta.get(&txn, "schemaVersion"),
        Some(Out::Any(Any::Number(Number::Int(i64::from(
            SCHEMA_VERSION
        )))))
    );
}
