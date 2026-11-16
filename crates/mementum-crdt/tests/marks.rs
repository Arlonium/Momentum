//! Les marques de texte qu'une page garde (Ruling R431) : celles du schéma des blocs (`bold`,
//! `italic`, `underline`, `strike`, `code`, `textColor`, `backgroundColor`, `link`), chacune d'une
//! valeur bornée. Une mise à jour de client qui en pose une autre est refusée ; relue du stockage
//! (une base d'avant cette règle), elle est lue supprimée : le serveur la retire de son document,
//! et les clients la retirent du leur à la synchro suivante.
//!
//! Le constat (C1 de la re-relecture du tour 3 de la piste S) : un paragraphe de C lettres sous
//! K marques inconnues, le gras une lettre sur deux, coûtait K × C à chaque lecture de la page
//! (`diff` de yrs recopie toutes les marques en vigueur à chaque plage) : 701 Ko envoyés, 3,1 s
//! d'acteur et 5 Gio pour une lecture par un lecteur.

use std::collections::HashMap;
use std::sync::Arc;

use mementum_crdt::{CrdtError, FRAGMENT, PageDoc};
use mementum_md::{Content, Inline, MAX_COLOR_BYTES, MAX_HREF_BYTES};
use yrs::types::Attrs;
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, OffsetKind, Options, ReadTxn, Text, Transact, Update, XmlFragment, XmlOut,
    XmlTextPrelim, XmlTextRef,
};

/// Un client qui a reçu la page `root` (une page neuve) : son document.
fn replica(root: &[u8]) -> Doc {
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(root).unwrap())
        .unwrap();
    doc
}

/// Le paragraphe de la page neuve.
fn paragraph<T: ReadTxn>(txn: &T) -> yrs::XmlElementRef {
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    let mut node = fragment.get(txn, 0);
    for _ in 0..2 {
        node = match node {
            Some(XmlOut::Element(e)) => e.get(txn, 0),
            other => panic!("élément attendu, trouvé {other:?}"),
        };
    }
    match node {
        Some(XmlOut::Element(p)) => p,
        other => panic!("paragraphe attendu, trouvé {other:?}"),
    }
}

/// La mise à jour qu'un client écrirait : « abc » dans le paragraphe de la page neuve, sous la
/// marque `key` de valeur `value`.
fn marked(root: &[u8], key: &str, value: Any) -> (Doc, Vec<u8>) {
    let doc = replica(root);
    let update = {
        let mut txn = doc.transact_mut();
        let p = paragraph(&txn);
        let text = p.push_back(&mut txn, XmlTextPrelim::new(""));
        let mut attrs = Attrs::new();
        attrs.insert(key.into(), value);
        text.insert_with_attributes(&mut txn, 0, "abc", attrs);
        txn.encode_update_v1()
    };
    (doc, update)
}

fn map(key: &str, value: &str) -> Any {
    Any::Map(Arc::new(HashMap::from([(
        key.to_string(),
        Any::from(value),
    )])))
}

fn empty() -> Any {
    Any::Map(Arc::new(HashMap::new()))
}

/// Les marques de chaque plage du texte du paragraphe, lues comme un navigateur les lirait.
fn runs_of(doc: &Doc) -> Vec<(String, Vec<String>)> {
    let txn = doc.transact();
    let p = paragraph(&txn);
    let Some(XmlOut::Text(text)) = p.get(&txn, 0) else {
        panic!("texte attendu");
    };
    let text: XmlTextRef = text;
    text.diff(&txn, YChange::identity)
        .into_iter()
        .map(|d| {
            let mut keys: Vec<String> = d
                .attributes
                .iter()
                .flat_map(|a| a.keys())
                .map(|k| k.to_string())
                .collect();
            keys.sort();
            (format!("{:?}", d.insert), keys)
        })
        .collect()
}

#[test]
fn a_client_mark_outside_the_schema_is_refused() {
    let (page, root) = PageDoc::new_page();
    let (_, update) = marked(&root, "k0", Any::Bool(true));
    let refused = page.apply_client_update(&update);
    assert!(
        matches!(&refused, Err(CrdtError::Structure(why)) if why.contains("k0")),
        "{refused:?}"
    );
}

#[test]
fn a_client_mark_of_the_schema_passes_up_to_its_bound() {
    let accepted = [
        ("bold", empty()),
        ("italic", empty()),
        ("underline", empty()),
        ("strike", empty()),
        ("code", empty()),
        ("bold", Any::Bool(true)),
        (
            "textColor",
            map("stringValue", &"c".repeat(MAX_COLOR_BYTES)),
        ),
        (
            "backgroundColor",
            map("stringValue", &"c".repeat(MAX_COLOR_BYTES)),
        ),
        ("link", map("href", &"u".repeat(MAX_HREF_BYTES))),
    ];
    for (key, value) in accepted {
        let (page, root) = PageDoc::new_page();
        let (_, update) = marked(&root, key, value.clone());
        page.apply_client_update(&update)
            .unwrap_or_else(|e| panic!("{key} = {value:?} : {e}"));
        let blocks = page.read_blocks();
        let Content::Inline(content) = &blocks[0].content else {
            panic!("contenu en ligne attendu");
        };
        assert_eq!(content.len(), 1, "{key} : {content:?}");
    }

    let refused = [
        ("bold", map("x", &"y".repeat(40))),
        (
            "textColor",
            map("stringValue", &"c".repeat(MAX_COLOR_BYTES + 1)),
        ),
        (
            "backgroundColor",
            map("stringValue", &"c".repeat(MAX_COLOR_BYTES + 1)),
        ),
        ("textColor", Any::from("c".repeat(MAX_COLOR_BYTES + 1))),
        ("link", map("href", &"u".repeat(MAX_HREF_BYTES + 1))),
        ("link", Any::from("u".repeat(MAX_HREF_BYTES + 1))),
    ];
    for (key, value) in refused {
        let (page, root) = PageDoc::new_page();
        let (_, update) = marked(&root, key, value);
        let refused = page.apply_client_update(&update);
        assert!(
            matches!(&refused, Err(CrdtError::Structure(why)) if why.contains(key)),
            "{key} : {refused:?}"
        );
    }
}

/// Relue du stockage, une marque que la garde refuserait est lue supprimée : le texte reste, la
/// marque n'est plus ni dans ce que lit la page, ni dans ce qu'elle donne aux clients, et un
/// client qui la portait la retire à la synchro.
#[test]
fn a_stored_mark_outside_the_schema_is_read_deleted() {
    let cases = [
        ("k0", Any::Bool(true)),
        ("link", map("href", &"u".repeat(MAX_HREF_BYTES + 1))),
        (
            "textColor",
            map("stringValue", &"c".repeat(MAX_COLOR_BYTES + 1)),
        ),
    ];
    for (key, value) in cases {
        let (_, root) = PageDoc::new_page();
        let (client, update) = marked(&root, key, value);
        assert_eq!(
            runs_of(&client),
            [("Any(String(\"abc\"))".to_string(), vec![key.to_string()])],
            "le client porte la marque"
        );
        let page = PageDoc::from_updates([root.as_slice(), update.as_slice()])
            .unwrap_or_else(|e| panic!("{key} : la page ne se charge pas : {e}"));
        let blocks = page.read_blocks();
        assert_eq!(
            blocks[0].content,
            Content::Inline(vec![Inline::Text {
                text: "abc".into(),
                styles: Default::default(),
            }]),
            "{key}"
        );
        // Ce que la page donne à un client neuf ne porte plus la marque.
        let fresh = replica(&page.encode_state());
        assert_eq!(
            runs_of(&fresh),
            [("Any(String(\"abc\"))".to_string(), vec![])],
            "{key} : client neuf"
        );
        // Le client qui la portait la retire en recevant ce qui lui manque.
        let state = client.transact().state_vector();
        let missing = page
            .encode_diff(&yrs::updates::encoder::Encode::encode_v1(&state))
            .unwrap();
        client
            .transact_mut()
            .apply_update(Update::decode_v1(&missing).unwrap())
            .unwrap();
        assert_eq!(
            runs_of(&client),
            [("Any(String(\"abc\"))".to_string(), vec![])],
            "{key} : client qui la portait"
        );
        // Rien n'est laissé en attente.
        assert!(!page.has_pending_deletions(), "{key}");
    }
}

/// Une marque du schéma, relue du stockage à sa borne, reste.
#[test]
fn a_stored_mark_of_the_schema_stays() {
    let (_, root) = PageDoc::new_page();
    let (_, update) = marked(&root, "link", map("href", &"u".repeat(MAX_HREF_BYTES)));
    let page = PageDoc::from_updates([root.as_slice(), update.as_slice()]).unwrap();
    let fresh = replica(&page.encode_state());
    assert_eq!(
        runs_of(&fresh),
        [("Any(String(\"abc\"))".to_string(), vec!["link".to_string()])]
    );
}
