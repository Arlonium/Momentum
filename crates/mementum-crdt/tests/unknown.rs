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
    fresh.write_blocks(&parsed).unwrap();
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

/// Page construite à la main : `build` reçoit la racine `blockGroup`.
fn page_from(build: impl FnOnce(&mut yrs::TransactionMut, &XmlElementRef)) -> PageDoc {
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

/// Réécrit les blocs d'une page dans une page neuve, document brut renvoyé.
fn rewritten(page: &PageDoc) -> (PageDoc, Doc) {
    let (fresh, _) = PageDoc::new_page();
    fresh.write_blocks(&page.read_blocks()).unwrap();
    let raw = decode(&fresh);
    (fresh, raw)
}

fn foreign_block(original_type: &str) -> mementum_md::Block {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD
        .encode(r#"{"attrs":{"a":"1"},"content":[],"children":[]}"#);
    mementum_md::Block {
        id: "f".into(),
        kind: "unknown".into(),
        props: mementum_md::Props::from([
            (
                "originalType".to_string(),
                PropValue::Str(original_type.into()),
            ),
            ("data".to_string(), PropValue::Str(data)),
            ("foreign".to_string(), PropValue::Bool(true)),
        ]),
        content: Content::None,
        children: vec![],
    }
}

/// Ruling R20 : un nom de nœud de BlockNote, ou un nom invalide, ne se
/// recrée pas ; il s'écrit en bloc `unknown` de BlockNote.
#[test]
fn blocknote_names_are_never_recreated() {
    for name in [
        "tableRow",
        "mention",
        "hardBreak",
        "doc",
        "paragraph",
        "a b",
        "",
        "x>y",
    ] {
        let (page, _) = PageDoc::new_page();
        page.write_blocks(&[foreign_block(name)]).unwrap();
        let raw = decode(&page);
        let txn = raw.transact();
        let container = find_element(&txn, "blockContainer").unwrap();
        let Some(XmlOut::Element(node)) = container.get(&txn, 0) else {
            panic!("nœud de contenu attendu")
        };
        assert_eq!(node.tag().as_ref(), "unknown", "« {name} » recréé");
    }
    // Un nom étranger valide, lui, se recrée.
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&[foreign_block("futureWidget")]).unwrap();
    let raw = decode(&page);
    assert!(find_element(&raw.transact(), "futureWidget").is_some());
}

/// Ruling R20 : ce que la lecture garde d'un nœud étranger profond, la
/// réécriture le recrée (bornes du JSON alignées sur `serde_json`).
#[test]
fn deep_foreign_node_is_recreated_up_to_the_kept_depth() {
    let page = page_from(|txn, root| {
        let container = root.push_back(txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(txn, "id", "f");
        let mut e = container.push_back(txn, XmlElementPrelim::empty("futureWidget"));
        for _ in 0..70 {
            e = e.push_back(txn, XmlElementPrelim::empty("inner"));
        }
        e.push_back(txn, XmlTextPrelim::new("x"));
    });
    let (_, raw) = rewritten(&page);
    let txn = raw.transact();
    let widget = find_element(&txn, "futureWidget").expect("futureWidget recréé");
    let mut depth = 0;
    let mut current = widget;
    while let Some(XmlOut::Element(child)) = current.get(&txn, 0) {
        depth += 1;
        current = child;
    }
    assert_eq!(depth, 48, "profondeur gardée dans data");
}

/// M3 : un nœud étranger hors `blockContainer` porte `"unwrapped": true`, se
/// recrée hors enveloppe, et son id passe par le contrôle des doublons.
#[test]
fn unwrapped_foreign_node_keeps_its_place_and_a_unique_id() {
    let page = page_from(|txn, root| {
        for _ in 0..2 {
            let list = root.push_back(txn, XmlElementPrelim::empty("futureList"));
            list.insert_attribute(txn, "id", "fl");
            let item = list.push_back(txn, XmlElementPrelim::empty("futureItem"));
            item.push_back(txn, XmlTextPrelim::new("x"));
        }
    });
    let blocks = page.read_blocks();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].id, "fl");
    let Some(PropValue::Str(data)) = blocks[0].props.get("data") else {
        panic!("data attendu")
    };
    use base64::Engine;
    let json = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap(),
    )
    .unwrap();
    assert!(json.contains(r#""unwrapped":true"#), "{json}");

    let (fresh, raw) = rewritten(&page);
    let txn = raw.transact();
    let root = match txn.get_xml_fragment(FRAGMENT).unwrap().get(&txn, 0) {
        Some(XmlOut::Element(root)) => root,
        other => panic!("racine attendue, trouvé {other:?}"),
    };
    let lists: Vec<String> = root
        .children(&txn)
        .map(|c| match c {
            XmlOut::Element(e) => {
                assert_eq!(e.tag().as_ref(), "futureList", "recréé hors enveloppe");
                match e.get_attribute(&txn, "id") {
                    Some(Out::Any(Any::String(id))) => id.to_string(),
                    other => panic!("id attendu, trouvé {other:?}"),
                }
            }
            other => panic!("élément attendu, trouvé {other:?}"),
        })
        .collect();
    assert_eq!(lists.len(), 2);
    assert_eq!(lists[0], "fl");
    assert_ne!(lists[1], "fl", "le second reçoit un nouvel id");
    assert_eq!(fresh.read_blocks().len(), 2);
}

/// M3 : un objet incorporé dans le texte d'un nœud étranger revient tel quel.
#[test]
fn foreign_text_embed_passes_through() {
    let embed = Any::Map(std::sync::Arc::new(HashMap::from([(
        "k".to_string(),
        Any::Number(Number::Int(3)),
    )])));
    let expected = embed.clone();
    let page = page_from(move |txn, root| {
        let container = root.push_back(txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(txn, "id", "f");
        let widget = container.push_back(txn, XmlElementPrelim::empty("futureWidget"));
        let text = widget.push_back(txn, XmlTextPrelim::new("ab"));
        text.insert_embed(txn, 1, embed);
    });
    let (_, raw) = rewritten(&page);
    let txn = raw.transact();
    let widget = find_element(&txn, "futureWidget").unwrap();
    let Some(XmlOut::Text(text)) = widget.get(&txn, 0) else {
        panic!("texte attendu")
    };
    let inserts: Vec<Out> = text
        .diff(&txn, YChange::identity)
        .into_iter()
        .map(|d| d.insert)
        .collect();
    assert_eq!(
        inserts,
        [
            Out::Any(Any::from("a")),
            Out::Any(expected),
            Out::Any(Any::from("b")),
        ]
    );
}
