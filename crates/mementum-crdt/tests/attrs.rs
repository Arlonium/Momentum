//! La garde des attributs de bloc (Rulings R689 à R701, module `attrs`).
//!
//! Le constat (relecture de la Task C8, sondes `V7` et `V9`) : Yjs garde toute valeur JSON dans
//! un attribut d'élément, y-prosemirror la passe telle quelle au nœud, et BlockNote écrit
//! `String(valeur)` de chaque prop dans un attribut `data-…`. Un compte qui peut modifier une
//! page, avec un client à lui, y écrit `backgroundColor = {"toString": 1}` : l'éditeur lève
//! « Cannot convert object to primitive value », et la page ne s'ouvre plus chez personne.
//!
//! Une mise à jour de client qui pose sur un élément que BlockNote connaît un attribut qui n'est
//! pas du type que dit le registre est refusée, avant d'être appliquée quand la mise à jour dit
//! de quel attribut il s'agit ; relue du stockage (une base d'avant cette règle), l'attribut est
//! retiré ; ce que le serveur écrit passe par le même jugement.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use common::dump;
use mementum_crdt::{CrdtError, FRAGMENT, PageDoc, Refused};
use mementum_md::{Block, Content, PropValue, Props, parse};
use yrs::any::Number;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, ClientID, Doc, OffsetKind, Options, Out, ReadTxn, Transact, TransactionMut, Update, Xml,
    XmlElementPrelim, XmlElementRef, XmlFragment, XmlOut,
};

/// Un client qui a reçu l'état `state`, sous l'identifiant `client`.
fn replica_as(client: u64, state: &[u8]) -> Doc {
    let doc = Doc::with_options(Options {
        client_id: ClientID::new(client),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(state).unwrap())
        .unwrap();
    doc
}

fn replica(state: &[u8]) -> Doc {
    replica_as(4242, state)
}

/// La racine `blockGroup` de la page.
fn root<T: ReadTxn>(txn: &T) -> XmlElementRef {
    match txn.get_xml_fragment(FRAGMENT).unwrap().get(txn, 0) {
        Some(XmlOut::Element(root)) => root,
        other => panic!("racine attendue, trouvé {other:?}"),
    }
}

fn child<T: ReadTxn>(txn: &T, parent: &XmlElementRef, index: u32) -> XmlElementRef {
    match parent.get(txn, index) {
        Some(XmlOut::Element(e)) => e,
        other => panic!("élément attendu, trouvé {other:?}"),
    }
}

/// Le `blockContainer` de rang `index` au premier niveau.
fn container<T: ReadTxn>(txn: &T, index: u32) -> XmlElementRef {
    child(txn, &root(txn), index)
}

/// Le nœud de contenu du bloc de rang `index` au premier niveau.
fn content<T: ReadTxn>(txn: &T, index: u32) -> XmlElementRef {
    child(txn, &container(txn, index), 0)
}

/// Ce qu'un client écrit : la mise à jour de `write`, faite dans son document.
fn written(doc: &Doc, write: impl FnOnce(&mut TransactionMut)) -> Vec<u8> {
    let mut txn = doc.transact_mut();
    write(&mut txn);
    txn.encode_update_v1()
}

/// `{"toString": 1}` : la valeur de la sonde `V9`.
fn object() -> Any {
    Any::Map(Arc::new(HashMap::from([(
        "toString".to_string(),
        Any::Number(Number::Int(1)),
    )])))
}

fn int(i: i64) -> Any {
    Any::Number(Number::Int(i))
}

fn float(f: f64) -> Any {
    Any::Number(Number::Float(f))
}

fn list(values: Vec<Any>) -> Any {
    Any::Array(values.into())
}

/// La valeur de l'attribut `key` du contenu du bloc `index`, dans l'état `state`, comme un
/// navigateur la lirait.
fn attr_of(state: &[u8], index: u32, key: &str) -> Option<Any> {
    let doc = replica(state);
    let txn = doc.transact();
    match content(&txn, index).get_attribute(&txn, key) {
        Some(Out::Any(any)) => Some(any),
        Some(other) => panic!("valeur attendue, trouvé {other:?}"),
        None => None,
    }
}

/// Un bloc neuf en fin de page : `blockContainer[id] > <tag>`, et son nœud de contenu.
fn push_block(txn: &mut TransactionMut, id: &str, tag: &str) -> XmlElementRef {
    let container = root(txn).push_back(txn, XmlElementPrelim::empty("blockContainer"));
    container.insert_attribute(txn, "id", id);
    container.push_back(txn, XmlElementPrelim::empty(tag))
}

/// Une cellule d'un tableau neuf en fin de page.
fn push_cell(txn: &mut TransactionMut) -> XmlElementRef {
    let table = push_block(txn, "t", "table");
    let row = table.push_back(txn, XmlElementPrelim::empty("tableRow"));
    let cell = row.push_back(txn, XmlElementPrelim::empty("tableCell"));
    cell.push_back(txn, XmlElementPrelim::empty("tableParagraph"));
    cell
}

fn refused_before(result: &Result<mementum_crdt::Applied, Refused>, naming: &str) -> bool {
    matches!(
        result,
        Err(Refused { error: CrdtError::Structure(why), intact: true }) if why.contains(naming)
    )
}

/// Le constat lui-même : `backgroundColor = {"toString": 1}` écrit par-dessus la valeur d'un bloc
/// déjà là (l'entrée ne nomme ni son élément ni sa clé : elle a pour origine l'entrée qu'elle
/// écrase). Refusé avant d'être appliqué (Ruling R492) : la page n'a pas changé.
#[test]
fn an_attribute_of_another_type_written_over_a_block_is_refused_before_it_is_applied() {
    let cases: Vec<(&str, Any)> = vec![
        ("backgroundColor", object()),
        ("backgroundColor", Any::Null),
        ("backgroundColor", int(7)),
        ("backgroundColor", Any::Bool(true)),
        ("textColor", list(vec![Any::from("red")])),
        ("textAlignment", Any::Buffer(vec![1u8, 2].into())),
        ("textAlignment", float(f64::NAN)),
    ];
    for (key, value) in cases {
        let (page, root_update) = PageDoc::new_page();
        let before = page.encode_state();
        let client = replica(&root_update);
        let update = written(&client, |txn| {
            content(txn, 0).insert_attribute(txn, key, value.clone());
        });
        let result = page.try_client_update(&update);
        assert!(
            refused_before(&result, key),
            "{key} = {value:?} : {result:?}"
        );
        assert!(
            page.encode_state() == before,
            "{key} = {value:?} : la page a changé"
        );
        assert_eq!(
            attr_of(&page.encode_state(), 0, key),
            attr_of(&before, 0, key)
        );
    }
}

/// La première valeur d'un attribut nomme son élément et sa clé : sur un bloc de la page comme
/// sur un bloc que la même mise à jour insère.
#[test]
fn an_attribute_of_another_type_named_by_the_update_is_refused_before_it_is_applied() {
    // Sur un élément de la page.
    for (key, value, naming) in [
        ("inconnu", object(), "hors du registre"),
        ("inconnu", Any::Null, "hors du registre"),
        ("inconnu", list(vec![int(1)]), "hors du registre"),
        ("level", object(), "level"),
    ] {
        let (page, root_update) = PageDoc::new_page();
        let client = replica(&root_update);
        let update = written(&client, |txn| {
            content(txn, 0).insert_attribute(txn, key, value.clone());
        });
        let result = page.try_client_update(&update);
        assert!(
            refused_before(&result, naming),
            "{key} = {value:?} : {result:?}"
        );
    }
    // L'identifiant d'un bloc de la page.
    let (page, root_update) = PageDoc::new_page();
    let client = replica(&root_update);
    let update = written(&client, |txn| {
        container(txn, 0).insert_attribute(txn, "id", int(5));
    });
    assert!(refused_before(&page.try_client_update(&update), "id"));

    // Sur un bloc neuf.
    let blocks: Vec<(&str, &str, Any)> = vec![
        ("heading", "level", object()),
        ("heading", "level", Any::from("1 2")),
        ("heading", "level", float(1e21)),
        ("heading", "level", float(2.5)),
        ("heading", "isToggleable", Any::from("true")),
        ("checkListItem", "checked", int(1)),
        ("image", "url", object()),
        ("image", "previewWidth", Any::from("320")),
        ("image", "previewWidth", float(f64::INFINITY)),
        ("image", "showPreview", Any::Null),
        ("numberedListItem", "start", float(0.5)),
        ("codeBlock", "language", int(3)),
        ("callout", "icon", list(vec![])),
        ("unknown", "foreign", Any::from("true")),
    ];
    for (tag, key, value) in blocks {
        let (page, root_update) = PageDoc::new_page();
        let before = page.encode_state();
        let client = replica(&root_update);
        let update = written(&client, |txn| {
            push_block(txn, "neuf", tag).insert_attribute(txn, key, value.clone());
        });
        let result = page.try_client_update(&update);
        assert!(
            refused_before(&result, key),
            "{tag}.{key} = {value:?} : {result:?}"
        );
        assert!(page.encode_state() == before);
    }
    // L'identifiant d'un bloc neuf, et une valeur écrasée dans la même mise à jour.
    let (page, root_update) = PageDoc::new_page();
    let client = replica(&root_update);
    let update = written(&client, |txn| {
        let heading = push_block(txn, "neuf", "heading");
        heading.insert_attribute(txn, "level", int(2));
        heading.insert_attribute(txn, "level", object());
    });
    assert!(refused_before(&page.try_client_update(&update), "level"));
    let update = written(&client, |txn| {
        let container = root(txn).push_back(txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(txn, "id", object());
    });
    assert!(refused_before(&page.try_client_update(&update), "id"));
}

/// Les fusions et les largeurs d'une cellule : prosemirror-tables alloue une case par colonne
/// couverte, additionne les `colspan` et joint les largeurs.
#[test]
fn cell_spans_and_widths_are_bounded() {
    let refused: Vec<(&str, Any)> = vec![
        ("colspan", int(101)),
        ("colspan", int(1_000_000_000)),
        ("colspan", int(0)),
        ("colspan", int(-1)),
        ("colspan", Any::from("2")),
        ("colspan", float(1.5)),
        ("rowspan", int(0)),
        ("rowspan", Any::from("abc")),
        ("rowspan", object()),
        ("colwidth", list(vec![Any::from("120")])),
        ("colwidth", list(vec![object()])),
        ("colwidth", list(vec![float(f64::INFINITY)])),
        ("colwidth", list(vec![int(1); 101])),
        ("colwidth", int(120)),
        ("colwidth", object()),
        ("backgroundColor", list(vec![int(120)])),
    ];
    for (key, value) in refused {
        let (page, root_update) = PageDoc::new_page();
        let client = replica(&root_update);
        let update = written(&client, |txn| {
            push_cell(txn).insert_attribute(txn, key, value.clone());
        });
        let result = page.try_client_update(&update);
        assert!(
            refused_before(&result, key),
            "{key} = {value:?} : {result:?}"
        );
    }
    let accepted: Vec<(&str, Any)> = vec![
        ("colspan", int(1)),
        ("colspan", int(100)),
        ("colspan", float(2.0)),
        ("rowspan", int(250_000)),
        ("colwidth", list(vec![int(120)])),
        ("colwidth", list(vec![int(120), Any::Undefined])),
        ("colwidth", list(vec![float(120.5), Any::Null])),
        ("colwidth", list(vec![int(1); 100])),
        ("colwidth", Any::Undefined),
    ];
    for (key, value) in accepted {
        let (page, root_update) = PageDoc::new_page();
        let client = replica(&root_update);
        let update = written(&client, |txn| {
            push_cell(txn).insert_attribute(txn, key, value.clone());
        });
        page.try_client_update(&update)
            .unwrap_or_else(|e| panic!("{key} = {value:?} : {e:?}"));
    }
}

/// Ce qu'un client du schéma écrit passe : chaque type, `undefined` pour une prop sans valeur,
/// une prop hors du registre que le MMD recopie.
#[test]
fn attributes_of_the_registry_types_pass() {
    let (page, root_update) = PageDoc::new_page();
    let client = replica(&root_update);
    let accepted: Vec<(&str, &str, Any)> = vec![
        ("heading", "level", int(3)),
        ("heading", "level", float(3.0)),
        ("heading", "isToggleable", Any::Bool(true)),
        ("heading", "backgroundColor", Any::from("red")),
        ("checkListItem", "checked", Any::Bool(true)),
        ("numberedListItem", "start", Any::Undefined),
        ("numberedListItem", "start", int(12)),
        ("image", "previewWidth", Any::Undefined),
        ("image", "previewWidth", int(320)),
        ("image", "previewWidth", float(320.5)),
        ("image", "url", Any::from("u".repeat(100_000))),
        ("paragraph", "mienne", Any::from("x")),
        ("paragraph", "mienne", int(7)),
        ("paragraph", "mienne", float(0.5)),
        ("paragraph", "mienne", Any::Bool(false)),
        ("unknown", "data", Any::from("e30=")),
    ];
    for (n, (tag, key, value)) in accepted.into_iter().enumerate() {
        let update = written(&client, |txn| {
            push_block(txn, &format!("b{n}"), tag).insert_attribute(txn, key, value.clone());
        });
        page.try_client_update(&update)
            .unwrap_or_else(|e| panic!("{tag}.{key} = {value:?} : {e:?}"));
        assert_eq!(
            attr_of(&page.encode_state(), n as u32 + 1, key),
            Some(value),
            "{tag}.{key}"
        );
    }
    // Par-dessus une valeur de la page, et par-dessus elle encore.
    for (key, value) in [
        ("backgroundColor", Any::from("red")),
        ("backgroundColor", Any::from("blue")),
        ("textAlignment", Any::Undefined),
    ] {
        let update = written(&client, |txn| {
            content(txn, 0).insert_attribute(txn, key, value.clone());
        });
        page.try_client_update(&update)
            .unwrap_or_else(|e| panic!("{key} = {value:?} : {e:?}"));
        assert_eq!(attr_of(&page.encode_state(), 0, key), Some(value));
    }
}

/// Un nœud d'un autre nom (un bloc d'un schéma plus récent, Ruling R4) n'est pas jugé : ses
/// attributs passent tels quels, et son aller-retour par le MMD les garde.
#[test]
fn a_foreign_element_keeps_any_attribute() {
    let (page, root_update) = PageDoc::new_page();
    let client = replica(&root_update);
    let update = written(&client, |txn| {
        let node = push_block(txn, "ailleurs", "schemaPlusRecent");
        node.insert_attribute(txn, "reglages", object());
        node.insert_attribute(txn, "liste", list(vec![Any::from("a"), object()]));
    });
    page.try_client_update(&update).expect("nœud étranger");
    let reloaded = PageDoc::from_updates([page.encode_state().as_slice()]).unwrap();
    let blocks = reloaded.read_blocks();
    assert_eq!(blocks[1].kind, "unknown");
    // Récrit par le serveur, le nœud retrouve ses attributs.
    let (rewritten, _) = PageDoc::new_page();
    rewritten.write_blocks(&blocks).unwrap();
    let state = rewritten.encode_state();
    assert_eq!(attr_of(&state, 1, "reglages"), Some(object()));
    assert_eq!(
        attr_of(&state, 1, "liste"),
        Some(list(vec![Any::from("a"), object()]))
    );
}

/// Deux clients écrasent la même valeur en même temps : le second arrivé a pour origine une
/// entrée qui n'est plus l'entrée courante, et la garde ne sait pas, avant d'appliquer, de quel
/// attribut il s'agit. Jugé dans la page (Ruling R691) : s'il l'emporte, la mise à jour est
/// refusée après l'application (la page est à recharger) ; s'il perd, sa valeur n'est lue de
/// personne. Dans aucun des deux ordres d'identifiants la valeur fautive n'est servie.
#[test]
fn a_value_written_over_an_older_entry_is_judged_in_the_page() {
    let mut refused = 0;
    for (honest, hostile) in [(10, 20), (20, 10)] {
        let (page, root_update) = PageDoc::new_page();
        let (a, b) = (
            replica_as(honest, &root_update),
            replica_as(hostile, &root_update),
        );
        let red = written(&a, |txn| {
            content(txn, 0).insert_attribute(txn, "backgroundColor", "red");
        });
        let bad = written(&b, |txn| {
            content(txn, 0).insert_attribute(txn, "backgroundColor", object());
        });
        page.try_client_update(&red).expect("valeur honnête");
        match page.try_client_update(&bad) {
            Ok(_) => assert_eq!(
                attr_of(&page.encode_state(), 0, "backgroundColor"),
                Some(Any::from("red")),
                "la valeur fautive a perdu"
            ),
            Err(Refused { error, intact }) => {
                assert!(!intact, "jugée après l'application");
                assert!(
                    matches!(&error, CrdtError::Structure(why) if why.contains("backgroundColor")),
                    "{error:?}"
                );
                refused += 1;
            }
        }
    }
    assert_eq!(
        refused, 1,
        "la valeur fautive l'emporte dans un des deux ordres"
    );
}

/// Une course honnête : deux clients changent la même prop en même temps, dans les deux ordres
/// d'identifiants. Rien n'est refusé.
#[test]
fn two_honest_clients_changing_the_same_attribute_both_pass() {
    for (first, second) in [(10, 20), (20, 10)] {
        let (page, root_update) = PageDoc::new_page();
        let red = written(&replica_as(first, &root_update), |txn| {
            content(txn, 0).insert_attribute(txn, "backgroundColor", "red");
        });
        let blue = written(&replica_as(second, &root_update), |txn| {
            content(txn, 0).insert_attribute(txn, "backgroundColor", "blue");
        });
        page.try_client_update(&red).expect("le premier");
        page.try_client_update(&blue).expect("le second");
        let kept = attr_of(&page.encode_state(), 0, "backgroundColor");
        assert!(
            kept == Some(Any::from("red")) || kept == Some(Any::from("blue")),
            "{kept:?}"
        );
    }
}

/// La mise à jour honnête qui écrase `backgroundColor` par « x », d'un client d'identifiant 5 et
/// d'horloge nulle, récrite à la main pour porter un autre contenu que la valeur `Any` : un
/// objet incorporé (`ContentEmbed`), `{"toString": 1}`, que Yjs rend tel quel comme valeur de
/// l'attribut.
fn embed_over_the_background(root_update: &[u8]) -> Vec<u8> {
    let client = replica_as(5, root_update);
    let honest = written(&client, |txn| {
        content(txn, 0).insert_attribute(txn, "backgroundColor", "x");
    });
    // `1` client, `1` bloc, client 5, horloge 0, puis l'élément : contenu `Any` (8), une origine
    // (0x80), et la marque d'une entrée de map (0x20), que l'élément forgé ne porte même pas.
    assert_eq!(honest[..5], [1, 1, 5, 0, 0xA8], "{honest:?}");
    let value = [1, 119, 1, b'x'];
    let at = honest
        .windows(value.len())
        .position(|w| w == value)
        .expect("la valeur");
    let json = br#"{"toString":1}"#;
    let mut forged = honest[..at].to_vec();
    forged[4] = 0x85;
    forged.push(json.len() as u8);
    forged.extend_from_slice(json);
    forged.extend_from_slice(&honest[at + value.len()..]);
    forged
}

/// Une valeur qui n'est pas une valeur `Any` (un objet incorporé), posée par l'origine d'une
/// entrée de la page : rien ne la distingue, avant d'appliquer, d'un contenu de texte. Jugée dans
/// la page : refusée après l'application (Ruling R691).
#[test]
fn an_embedded_object_written_over_an_attribute_is_refused_in_the_page() {
    let (page, root_update) = PageDoc::new_page();
    let result = page.try_client_update(&embed_over_the_background(&root_update));
    assert!(
        matches!(
            &result,
            Err(Refused { error: CrdtError::Structure(why), intact: false })
                if why.contains("backgroundColor")
        ),
        "{result:?}"
    );
}

/// Les blocs d'une page dont le journal porte `updates`, et son état.
fn loaded(updates: &[&[u8]]) -> (PageDoc, Vec<u8>) {
    let page = PageDoc::from_updates(updates.iter().copied()).expect("journal lisible");
    let state = page.encode_state();
    (page, state)
}

/// Relue du stockage (une base d'avant la garde), la valeur fautive est retirée dans la même
/// lecture : la page ne la sert à personne, un client qui l'avait la perd à la synchro, et la
/// relecture la retire chaque fois de la même façon.
#[test]
fn a_stored_attribute_of_another_type_is_removed_when_the_journal_is_read() {
    let (_, root_update) = PageDoc::new_page();
    let hostile = replica(&root_update);
    let over = written(&hostile, |txn| {
        content(txn, 0).insert_attribute(txn, "backgroundColor", object());
        content(txn, 0).insert_attribute(txn, "textColor", "red");
    });
    let fresh = written(&hostile, |txn| {
        let heading = push_block(txn, "h", "heading");
        heading.insert_attribute(txn, "level", object());
        heading.insert_attribute(txn, "textAlignment", "center");
        let cell = push_cell(txn);
        cell.insert_attribute(txn, "colspan", int(1_000_000_000));
        cell.insert_attribute(txn, "colwidth", list(vec![object()]));
        cell.insert_attribute(txn, "rowspan", int(2));
    });
    let embedded = embed_over_the_background(&root_update);
    for journal in [
        vec![root_update.as_slice(), &over, &fresh],
        vec![root_update.as_slice(), &fresh, &over],
        vec![root_update.as_slice(), &embedded],
    ] {
        let (page, state) = loaded(&journal);
        assert_eq!(attr_of(&state, 0, "backgroundColor"), None, "retirée");
        if journal.len() == 3 {
            assert_eq!(attr_of(&state, 0, "textColor"), Some(Any::from("red")));
            assert_eq!(attr_of(&state, 1, "level"), None);
            assert_eq!(
                attr_of(&state, 1, "textAlignment"),
                Some(Any::from("center"))
            );
            let doc = replica(&state);
            let txn = doc.transact();
            let cell = child(&txn, &child(&txn, &content(&txn, 2), 0), 0);
            assert!(cell.get_attribute(&txn, "colspan").is_none());
            assert!(cell.get_attribute(&txn, "colwidth").is_none());
            assert!(matches!(
                cell.get_attribute(&txn, "rowspan"),
                Some(Out::Any(Any::Number(Number::Int(2))))
            ));
            drop(txn);
            // Le titre se lit à son niveau par défaut.
            let blocks = page.read_blocks();
            assert_eq!(blocks[1].kind, "heading");
            assert!(!blocks[1].props.contains_key("level"));
        }
        // La relecture est la même chaque fois.
        let (again, _) = loaded(&journal);
        assert_eq!(again.read_blocks(), page.read_blocks());
        // Le client qui avait la valeur la perd en recevant ce qui lui manque.
        let had = replica(&root_update);
        for update in &journal[1..] {
            had.transact_mut()
                .apply_update(Update::decode_v1(update).unwrap())
                .unwrap();
        }
        let missing = page
            .encode_diff(&yrs::updates::encoder::Encode::encode_v1(
                &had.transact().state_vector(),
            ))
            .unwrap();
        had.transact_mut()
            .apply_update(Update::decode_v1(&missing).unwrap())
            .unwrap();
        let txn = had.transact();
        assert!(
            content(&txn, 0)
                .get_attribute(&txn, "backgroundColor")
                .is_none(),
            "le client garde la valeur fautive"
        );
    }
}

/// Un instantané (tout l'état d'une page, en une entrée) qui porte des valeurs fautives : elles
/// sont retirées, et rien d'autre.
#[test]
fn a_stored_snapshot_loses_only_its_unsound_attributes() {
    let (_, root_update) = PageDoc::new_page();
    let hostile = replica(&root_update);
    written(&hostile, |txn| {
        content(txn, 0).insert_attribute(txn, "backgroundColor", object());
        let image = push_block(txn, "i", "image");
        image.insert_attribute(txn, "url", "/a.png");
        image.insert_attribute(txn, "previewWidth", Any::from("large"));
        container(txn, 1).insert_attribute(txn, "id", int(9));
    });
    let snapshot = hostile
        .transact()
        .encode_state_as_update_v1(&yrs::StateVector::default());
    let (_, state) = loaded(&[&snapshot]);
    let skip = |_: &str, _: &str, _: &Any| false;
    let kept = dump(&state, &skip);
    let theirs: Vec<String> = dump(&snapshot, &skip);
    assert_eq!(kept.len(), theirs.len(), "aucun nœud retiré");
    assert_eq!(attr_of(&state, 0, "backgroundColor"), None);
    assert_eq!(attr_of(&state, 1, "previewWidth"), None);
    assert_eq!(attr_of(&state, 1, "url"), Some(Any::from("/a.png")));
    let doc = replica(&state);
    let txn = doc.transact();
    assert!(container(&txn, 1).get_attribute(&txn, "id").is_none());
}

/// Ce que le serveur écrit passe par le même jugement (Ruling R692) : une prop du registre d'un
/// autre type que le sien s'écrit à son défaut.
#[test]
fn the_server_writes_a_registry_prop_of_another_type_at_its_default() {
    let mut blocks = parse("# Titre\n").unwrap();
    blocks[0]
        .props
        .insert("level".into(), PropValue::Str("1 2".into()));
    blocks[0]
        .props
        .insert("isToggleable".into(), PropValue::Int(1));
    blocks[0]
        .props
        .insert("textAlignment".into(), PropValue::Str("center".into()));
    let image = Block {
        id: String::new(),
        kind: "image".into(),
        props: Props::from([
            ("previewWidth".to_string(), PropValue::Float(f64::NAN)),
            ("url".to_string(), PropValue::Bool(true)),
        ]),
        content: Content::None,
        children: vec![],
    };
    blocks.push(image);
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&blocks).unwrap();
    let state = page.encode_state();
    assert_eq!(attr_of(&state, 0, "level"), Some(int(1)));
    assert_eq!(attr_of(&state, 0, "isToggleable"), Some(Any::Bool(false)));
    assert_eq!(
        attr_of(&state, 0, "textAlignment"),
        Some(Any::from("center"))
    );
    assert_eq!(attr_of(&state, 1, "previewWidth"), Some(Any::Undefined));
    assert_eq!(attr_of(&state, 1, "url"), Some(Any::from("")));
}

/// La recréation d'un nœud étranger (`data` d'un bloc `unknown`, que `PUT /mmd` reçoit d'un
/// agent) n'écrit pas, sur un élément que BlockNote connaît, un attribut que la garde refuserait
/// d'un client (Ruling R692) ; les autres sont écrits.
#[test]
fn a_recreated_foreign_node_does_not_write_an_unsound_attribute() {
    let data = serde_json::json!({
        "attrs": { "reglages": { "toString": 1 } },
        "content": [{
            "tag": "paragraph",
            "attrs": {
                "backgroundColor": { "toString": 1 },
                "textColor": "red",
                "textAlignment": null,
            },
            "content": [],
        }],
    });
    let block = Block {
        id: "etranger".into(),
        kind: "unknown".into(),
        props: Props::from([
            (
                "originalType".to_string(),
                PropValue::Str("schemaPlusRecent".into()),
            ),
            (
                "data".to_string(),
                PropValue::Str(base64::engine::general_purpose::STANDARD.encode(data.to_string())),
            ),
            ("foreign".to_string(), PropValue::Bool(true)),
        ]),
        content: Content::None,
        children: vec![],
    };
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&[block]).unwrap();
    let state = page.encode_state();
    assert_eq!(attr_of(&state, 0, "reglages"), Some(object()), "le nœud");
    let doc = replica(&state);
    let txn = doc.transact();
    let inner = child(&txn, &content(&txn, 0), 0);
    assert_eq!(inner.tag().as_ref(), "paragraph");
    assert!(inner.get_attribute(&txn, "backgroundColor").is_none());
    assert!(inner.get_attribute(&txn, "textAlignment").is_none());
    assert!(matches!(
        inner.get_attribute(&txn, "textColor"),
        Some(Out::Any(Any::String(s))) if s.as_ref() == "red"
    ));
    drop(txn);
    // Ce que le serveur a écrit, un client l'enverrait sans refus.
    let empty = PageDoc::from_updates(std::iter::empty::<&[u8]>()).unwrap();
    assert_eq!(empty.attribute_refusal(&state).unwrap(), None);
}

/// Tout ce que BlockNote écrit passe (les états exportés d'un vrai navigateur, Task 8), comme
/// mise à jour de client et comme entrée du journal : rien n'est refusé, rien n'est retiré.
#[test]
fn nothing_the_editor_writes_is_refused_or_removed() {
    let fixtures: [(&str, &[u8]); 4] = [
        (
            "all-blocks",
            include_bytes!("../../../spec/mmd/fixtures/yjs/all-blocks.bin"),
        ),
        (
            "non-default",
            include_bytes!("../../../spec/mmd/fixtures/yjs/non-default.bin"),
        ),
        (
            "rich-blocks",
            include_bytes!("../../../spec/mmd/fixtures/yjs/rich-blocks.bin"),
        ),
        (
            "p0-all-blocks",
            include_bytes!("../../../spec/mmd/fixtures/yjs/p0-all-blocks.bin"),
        ),
    ];
    let skip = |_: &str, _: &str, _: &Any| false;
    for (name, state) in fixtures {
        let empty = PageDoc::from_updates(std::iter::empty::<&[u8]>()).unwrap();
        assert_eq!(empty.attribute_refusal(state).unwrap(), None, "{name}");
        let (_, kept) = loaded(&[state]);
        let theirs = dump(state, &skip);
        assert!(theirs.len() > 20, "{name} : fixture lue en entier");
        assert_eq!(dump(&kept, &skip), theirs, "{name}");
    }
}

/// Ruling R490, pour les attributs : seul ce que la mise à jour intègre est jugé. Un client qui
/// a gardé dans son document la valeur fautive d'avant la garde la renvoie, à une horloge que la
/// page connaît : yrs l'ignore, et la mise à jour n'est pas refusée pour elle (sinon ce client
/// serait refusé à chaque envoi).
#[test]
fn an_attribute_the_page_already_knows_is_not_judged_again() {
    let (_, root_update) = PageDoc::new_page();
    let hostile = replica(&root_update);
    let over = written(&hostile, |txn| {
        content(txn, 0).insert_attribute(txn, "backgroundColor", object());
    });
    let fresh = written(&hostile, |txn| {
        push_block(txn, "h", "heading").insert_attribute(txn, "level", object());
    });
    let (page, state) = loaded(&[&root_update, &over, &fresh]);
    assert_eq!(attr_of(&state, 0, "backgroundColor"), None);
    for known in [&over, &fresh] {
        page.try_client_update(known)
            .unwrap_or_else(|e| panic!("renvoi d'une entrée connue : {e:?}"));
    }
    assert!(page.encode_state() == state, "rien n'a changé");
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

    /// Ruling R700 : ce que le serveur écrit d'un arbre de blocs quelconque (`PUT /mmd`, une
    /// restauration, une duplication ; nœuds étrangers compris), un client l'enverrait sans
    /// refus de la garde des attributs, et la relecture du journal n'en retire rien.
    #[test]
    fn whatever_the_server_writes_passes_the_attribute_guard(tree in mementum_md::arb::tree()) {
        let (page, _) = PageDoc::new_page();
        if page.write_blocks(&tree).is_err() {
            return Ok(());
        }
        let state = page.encode_state();
        let empty = PageDoc::from_updates(std::iter::empty::<&[u8]>()).unwrap();
        proptest::prop_assert_eq!(empty.attribute_refusal(&state).unwrap(), None);
        let reloaded = PageDoc::from_updates([state.as_slice()]).unwrap();
        proptest::prop_assert_eq!(reloaded.read_blocks(), page.read_blocks());
        proptest::prop_assert_eq!(reloaded.revision(), page.revision());
    }
}

/// Ruling R701 : l'état d'une version de l'historique, coupée avant la garde, est servi sans les
/// attributs que la garde refuse ; un état sain est servi tel quel, octet pour octet.
#[test]
fn a_kept_state_is_served_without_its_unsound_attributes() {
    let (page, root_update) = PageDoc::new_page();
    page.write_blocks(&parse("# Titre\n\nTexte\n").unwrap())
        .unwrap();
    let sound = page.encode_state();
    let served = PageDoc::served_state(&sound).expect("un état sain");
    assert_eq!(loaded(&[&served]).0.read_blocks(), page.read_blocks());
    assert_eq!(loaded(&[&served]).0.revision(), page.revision());

    let hostile = replica(&root_update);
    written(&hostile, |txn| {
        content(txn, 0).insert_attribute(txn, "backgroundColor", "red");
        content(txn, 0).insert_attribute(txn, "backgroundColor", object());
        push_block(txn, "h", "heading").insert_attribute(txn, "level", Any::from("1 2"));
    });
    let kept = hostile
        .transact()
        .encode_state_as_update_v1(&yrs::StateVector::default());
    let served = PageDoc::served_state(&kept).expect("un état lisible");
    assert!(served != kept, "corrigé");
    assert_eq!(attr_of(&served, 0, "backgroundColor"), None);
    assert_eq!(attr_of(&served, 1, "level"), None);
    assert_eq!(
        attr_of(&served, 0, "textColor"),
        attr_of(&kept, 0, "textColor")
    );
    // Un état illisible n'est pas servi.
    assert_eq!(PageDoc::served_state(&[9, 9, 9]), None);
}

/// Écrit un entier variable de lib0.
fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// La recherche de l'entrée de la page qu'une valeur écrase (`entry_in_page`, Ruling R697) suit
/// une origine qu'un client choisit : un élément supprimé, ramassé avec son bloc, le milieu
/// d'une chaîne, une horloge que la page n'a pas. Rien ne panique, et la page se lit toujours.
#[test]
fn a_value_placed_after_any_item_of_the_page_breaks_nothing() {
    let (mut page, root_update) = PageDoc::new_page();
    let mut journal = vec![root_update.clone()];
    // Un client d'identifiant 31 écrit un bloc de texte, puis le retire : ses éléments sont
    // supprimés, et ceux de dedans ramassés.
    let writer = replica_as(31, &root_update);
    let wrote = written(&writer, |txn| {
        let paragraph = push_block(txn, "p", "paragraph");
        paragraph.insert_attribute(txn, "backgroundColor", "red");
        let text = paragraph.push_back(txn, yrs::XmlTextPrelim::new(""));
        yrs::Text::insert(&text, txn, 0, "abcdef");
    });
    journal.push(page.try_client_update(&wrote).expect("le bloc").read);
    let clocks = writer.transact().state_vector().get(&ClientID::new(31));
    let removed = written(&writer, |txn| {
        root(txn).remove_range(txn, 1, 1);
    });
    journal.push(page.try_client_update(&removed).expect("le retrait").read);
    let before = page.read_blocks();
    for (n, clock) in (0..clocks + 3).enumerate() {
        // Un client, un élément à l'horloge 0 : une valeur `Any` (8), une origine (0x80), la
        // marque d'une entrée de map (0x20) ; la valeur : l'objet `{"toString": 1}`.
        let mut update = vec![1, 1];
        var(&mut update, 900 + n as u64);
        update.extend_from_slice(&[0, 0xA8]);
        var(&mut update, 31);
        var(&mut update, u64::from(clock));
        update.extend_from_slice(&[1, 118, 1, 8]);
        update.extend_from_slice(b"toString");
        update.extend_from_slice(&[125, 1, 0]);
        match page.try_client_update(&update) {
            Ok(applied) => journal.push(applied.read),
            Err(Refused { intact: true, .. }) => {}
            // Refusée après l'application : la page est rechargée de son journal.
            Err(_) => page = PageDoc::from_updates(journal.iter().map(Vec::as_slice)).unwrap(),
        }
        assert_eq!(page.read_blocks(), before, "origine à l'horloge {clock}");
    }
    let state = page.encode_state();
    let doc = replica(&state);
    let txn = doc.transact();
    for node in txn.get_xml_fragment(FRAGMENT).unwrap().successors(&txn) {
        if let XmlOut::Element(element) = node {
            for (key, value) in element.attributes(&txn) {
                assert!(
                    !matches!(value, Out::Any(Any::Map(_))),
                    "« {} » porte {key} = {value:?}",
                    element.tag()
                );
            }
        }
    }
}

// ------------------------------------------------------------------ tour de correction 1
//
// La règle (Ruling R702) : la garde ne juge jamais une valeur telle que yrs la lit quand Yjs la
// lit autrement. Seule une valeur `Any` est une valeur d'attribut ; ce que Yjs lit de chaque
// autre contenu est tenu, avec le vrai Yjs, par `apps/web/src/editor/attrContents.test.ts`.

/// La mise à jour honnête qui écrase `key` du premier bloc par « x » (client 5, horloge 0),
/// récrite à la main : l'octet d'en-tête `info`, le contenu `body` à la place de la valeur, et,
/// avec `left`, cet élément pour origine et l'entrée écrasée pour origine droite.
fn forged(
    root_update: &[u8],
    key: &str,
    info: u8,
    left: Option<(u64, u32)>,
    body: &[u8],
) -> Vec<u8> {
    let client = replica_as(5, root_update);
    let honest = written(&client, |txn| {
        content(txn, 0).insert_attribute(txn, key, "x");
    });
    assert_eq!(honest[..5], [1, 1, 5, 0, 0xA8], "{honest:?}");
    let value = [1, 119, 1, b'x'];
    let at = honest
        .windows(value.len())
        .position(|w| w == value)
        .expect("la valeur");
    let mut out = vec![1, 1, 5, 0, info];
    if let Some((client, clock)) = left {
        var(&mut out, client);
        var(&mut out, u64::from(clock));
    }
    out.extend_from_slice(&honest[5..at]);
    out.extend_from_slice(body);
    out.extend_from_slice(&honest[at + value.len()..]);
    out
}

/// L'identifiant Yjs d'un élément XML.
fn id_of(element: &XmlElementRef) -> (u64, u32) {
    match AsRef::<yrs::branch::Branch>::as_ref(element).id() {
        yrs::branch::BranchID::Nested(id) => (id.client.get(), id.clock),
        other => panic!("un élément imbriqué, trouvé {other:?}"),
    }
}

const JSON_OBJECT: &[u8] = br#"{"toString":1}"#;

/// Un contenu JSON : `len - 1`, puis la chaîne.
fn json_body() -> Vec<u8> {
    [&[0, JSON_OBJECT.len() as u8][..], JSON_OBJECT].concat()
}

/// C1 de la relecture : un contenu JSON (`ContentJSON`) posé par l'origine de l'entrée courante.
/// yrs en rend la chaîne (bonne pour une prop de texte), Yjs `JSON.parse` de la chaîne : un
/// objet, et la page ne s'ouvre plus. Refusé d'un client où qu'il soit, avant d'être appliqué.
#[test]
fn json_content_is_refused_from_a_client_wherever_it_is() {
    let (page, root_update) = PageDoc::new_page();
    let before = page.encode_state();
    for key in ["backgroundColor", "textColor", "textAlignment"] {
        // Par l'origine de l'entrée courante (0x82), puis avec la marque d'entrée de map (0xA2).
        for info in [0x82, 0xA2] {
            let update = forged(&root_update, key, info, None, &json_body());
            let result = page.try_client_update(&update);
            assert!(
                refused_before(&result, "JSON"),
                "{key} {info:#x} : {result:?}"
            );
            assert!(page.encode_state() == before);
        }
    }
    // En enfant de la racine du fragment, nommée : refusé aussi.
    let mut child = vec![1, 1, 6, 0, 0x02, 1];
    var(&mut child, FRAGMENT.len() as u64);
    child.extend_from_slice(FRAGMENT.as_bytes());
    child.extend_from_slice(&json_body());
    child.push(0);
    assert!(page.try_client_update(&child).is_err());
    assert!(page.encode_state() == before);
}

/// Les contenus qui ne sont pas une valeur `Any`, posés par l'origine de l'entrée courante d'un
/// attribut : une chaîne (yrs lit « red », Yjs « d » : m1 de la relecture), des octets, un objet
/// incorporé. Rien ne les distingue, dans les octets, d'un contenu de texte : refusés dans la
/// page, après l'application.
#[test]
fn a_content_that_is_not_a_value_is_refused_as_an_attribute() {
    let cases: [(&str, u8, Vec<u8>); 3] = [
        ("une chaîne", 0x84, vec![3, b'r', b'e', b'd']),
        ("des octets", 0x83, vec![2, 1, 2]),
        (
            "un objet incorporé",
            0x85,
            [&[JSON_OBJECT.len() as u8][..], JSON_OBJECT].concat(),
        ),
    ];
    for (what, info, body) in cases {
        let (page, root_update) = PageDoc::new_page();
        let update = forged(&root_update, "backgroundColor", info, None, &body);
        let result = page.try_client_update(&update);
        assert!(
            matches!(
                &result,
                Err(Refused {
                    error: CrdtError::Structure(_),
                    intact: false
                })
            ),
            "{what} : {result:?}"
        );
    }
}

/// Relus du journal (une base d'avant la garde), ou dans l'état d'une version : l'attribut que
/// porte un tel contenu est retiré, JSON compris, et il ne revient pas à la relecture suivante.
#[test]
fn a_stored_content_that_is_not_a_value_loses_its_attribute() {
    let (_, root_update) = PageDoc::new_page();
    let cases: [(u8, Vec<u8>); 4] = [
        (0x82, json_body()),
        (0x84, vec![3, b'r', b'e', b'd']),
        (0x83, vec![2, 1, 2]),
        (0x85, [&[JSON_OBJECT.len() as u8][..], JSON_OBJECT].concat()),
    ];
    for (info, body) in cases {
        let update = forged(&root_update, "backgroundColor", info, None, &body);
        let (page, state) = loaded(&[&root_update, &update]);
        assert_eq!(attr_of(&state, 0, "backgroundColor"), None, "{info:#x}");
        assert!(
            !state.windows(8).any(|w| w == b"toString"),
            "{info:#x} : l'état servi porte encore l'objet"
        );
        let (again, _) = loaded(&[&root_update, &update]);
        assert_eq!(again.revision(), page.revision());
        // L'état d'une version (R701) : tout le document en une mise à jour.
        let kept = {
            let raw = replica(&root_update);
            raw.transact_mut()
                .apply_update(Update::decode_v1(&update).unwrap())
                .unwrap();
            raw.transact()
                .encode_state_as_update_v1(&yrs::StateVector::default())
        };
        // yrs écrit un contenu JSON sous une forme qu'il ne relit pas : l'état n'est pas servi.
        let Some(served) = PageDoc::served_state(&kept) else {
            assert_eq!(info, 0x82, "seul un état à contenu JSON est illisible");
            continue;
        };
        assert_ne!(info, 0x82, "un état à contenu JSON est servi");
        assert!(served != kept, "{info:#x} : servi tel quel");
        assert_eq!(attr_of(&served, 0, "backgroundColor"), None, "{info:#x}");
        assert!(!served.windows(8).any(|w| w == b"toString"), "{info:#x}");
    }
}

/// I1 de la relecture : un élément dont l'origine est un enfant (le bloc) et l'origine droite une
/// entrée de map. yrs recopie la clé du voisin de droite : un attribut ; Yjs prend celle de
/// gauche : un enfant sans type du groupe, et y-prosemirror lève chez les autres. Une valeur
/// `Any` qui porte une origine droite est refusée avant d'être appliquée ; un autre contenu, dans
/// la page ; relu du journal, l'élément est supprimé (chez yrs comme chez Yjs, par son
/// identifiant).
#[test]
fn an_item_between_a_child_and_a_map_entry_is_refused() {
    let (page, root_update) = PageDoc::new_page();
    let before = page.encode_state();
    let block = {
        let doc = replica(&root_update);
        let txn = doc.transact();
        id_of(&container(&txn, 0))
    };
    // Une valeur `Any` (8), une origine (0x80), une origine droite (0x40).
    let any = forged(
        &root_update,
        "backgroundColor",
        0xC8,
        Some(block),
        &[1, 119, 3, b'r', b'e', b'd'],
    );
    let result = page.try_client_update(&any);
    assert!(refused_before(&result, "origine droite"), "{result:?}");
    assert!(page.encode_state() == before);
    // Une chaîne (4) : jugée dans la page.
    let string = forged(
        &root_update,
        "backgroundColor",
        0xC4,
        Some(block),
        &[3, b'r', b'e', b'd'],
    );
    let result = page.try_client_update(&string);
    assert!(
        matches!(&result, Err(Refused { intact: false, .. })),
        "{result:?}"
    );
    // Relus du journal : la page se lit comme avant eux, et ne garde ni l'attribut ni l'enfant.
    for update in [&any, &string] {
        // (Le bloc de gauche, lui, est parti à l'intégration : yrs le tient pour écrasé.)
        let (_, state) = loaded(&[&root_update, update]);
        let doc = replica(&state);
        let txn = doc.transact();
        assert_eq!(
            root(&txn).attributes(&txn).count(),
            0,
            "un attribut du groupe"
        );
        assert_eq!(
            root(&txn).children(&txn).count() as u32,
            root(&txn).len(&txn),
            "un enfant sans type"
        );
        assert!(
            !state.windows(3).any(|w| w == b"red"),
            "la valeur est servie"
        );
    }
}

/// I2 de la relecture : une valeur `Any` placée par l'origine d'une lettre d'un texte. Aucun
/// client n'en écrit ; la chercher suit les voisins de son origine (tout le texte) : la mise à
/// jour est refusée à la première, avant d'être appliquée.
#[test]
fn a_value_placed_after_a_letter_is_refused_before_it_is_applied() {
    let (page, root_update) = PageDoc::new_page();
    let writer = replica_as(31, &root_update);
    let first = writer.transact().state_vector().get(&ClientID::new(31));
    let typed = written(&writer, |txn| {
        let text = content(txn, 0).push_back(txn, yrs::XmlTextPrelim::new(""));
        yrs::Text::insert(&text, txn, 0, "abcdef");
    });
    page.try_client_update(&typed).expect("le texte");
    let before = page.encode_state();
    // Le texte est à l'horloge `first`, ses lettres à la suivante.
    let mut update = vec![1, 2, 7, 0];
    for letter in [first + 1, first + 3] {
        update.push(0x88);
        var(&mut update, 31);
        var(&mut update, u64::from(letter));
        update.extend_from_slice(&[1, 119, 1, b'z']);
    }
    update.push(0);
    let result = page.try_client_update(&update);
    assert!(refused_before(&result, "contenu de texte"), "{result:?}");
    assert!(page.encode_state() == before);
}

/// C2 de la relecture : BlockNote 0.57 écrit `colwidth = [NaN, NaN, NaN]` pour toute cellule
/// fusionnée collée sans largeurs. `NaN` est une largeur absente (Ruling R705) : accepté d'un
/// client, gardé à la relecture avec la largeur réelle voisine, lu « sans largeur ».
#[test]
fn a_width_that_is_not_a_number_is_an_absent_width() {
    let nan = || float(f64::NAN);
    let (page, root_update) = PageDoc::new_page();
    let client = replica(&root_update);
    let update = written(&client, |txn| {
        let cell = push_cell(txn);
        cell.insert_attribute(txn, "colspan", int(3));
        cell.insert_attribute(txn, "colwidth", list(vec![nan(), int(120), nan()]));
    });
    page.try_client_update(&update)
        .expect("le collage d'une cellule fusionnée");
    let widths = |state: &[u8]| {
        let doc = replica(state);
        let txn = doc.transact();
        let cell = child(&txn, &child(&txn, &content(&txn, 1), 0), 0);
        match cell.get_attribute(&txn, "colwidth") {
            Some(Out::Any(Any::Array(values))) => values
                .iter()
                .map(|v| match v {
                    Any::Number(Number::Float(f)) if f.is_nan() => "NaN".to_string(),
                    other => format!("{other:?}"),
                })
                .collect::<Vec<_>>(),
            other => panic!("une liste attendue, trouvé {other:?}"),
        }
    };
    let state = page.encode_state();
    assert_eq!(widths(&state), ["NaN", "Number(Int(120))", "NaN"]);
    // Relue du journal : rien n'est retiré.
    let (reloaded, again) = loaded(&[&state]);
    assert_eq!(widths(&again), ["NaN", "Number(Int(120))", "NaN"]);
    let Content::Table(table) = &reloaded.read_blocks()[1].content else {
        panic!("un tableau");
    };
    assert_eq!(table.column_widths, [None, Some(120.0), None]);
    // Une largeur infinie reste refusée.
    let update = written(&client, |txn| {
        let cell = child(txn, &child(txn, &content(txn, 1), 0), 0);
        cell.insert_attribute(txn, "colwidth", list(vec![float(f64::INFINITY)]));
    });
    assert!(refused_before(&page.try_client_update(&update), "colwidth"));
}
