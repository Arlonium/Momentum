//! La lecture d'une page (`PageDoc::read_blocks`, puis le rendu en MMD de `GET /mmd`) croît
//! comme son document, quoi qu'un client y ait écrit (Rulings R381 et R385). La méthode et ce qui
//! la garde de passer à tort sont dans `mementum-md/tests/common/linear.rs`.

#[path = "../../mementum-md/tests/common/linear.rs"]
mod linear;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use linear::{Answer, Form, Limits, grows_like_input};
use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{MAX_COLOR_BYTES, MAX_HREF_BYTES, RenderOpts, render};
use yrs::types::{Attrs, Delta};
use yrs::{
    Any, Doc, ReadTxn, Text, Transact, Xml, XmlElementPrelim, XmlElementRef, XmlFragment,
    XmlTextPrelim,
};

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

/// Une marque `{key: value}` de BlockNote.
fn mark(key: &str, value: &str) -> Any {
    Any::Map(Arc::new(HashMap::from([(
        key.to_string(),
        Any::from(value),
    )])))
}

/// Un bloc de la page fait d'un élément `tag` (`paragraph`, ou une balise inconnue : un nœud
/// étranger) dont le texte compte `runs` lettres, toutes sous les marques `marked`, que le document
/// ne porte qu'une fois, et la marque `toggled` une lettre sur deux.
///
/// Écrit en temps linéaire : chaque lettre est un élément à elle (insérée en tête, de la dernière à
/// la première : yrs ne les fond pas), les marques communes sont posées sur tout le texte d'un coup,
/// puis l'autre en une passe (`apply_delta`). Un texte écrit d'un tenant, puis marqué une lettre
/// sur deux, faisait couper à yrs l'élément à chaque lettre, en recopiant la suite (1,7 s pour
/// 80 000 lettres, en release) ; écrire plage par plage reparcourrait le texte à chaque plage, et
/// répéterait les marques.
fn marked_runs(tag: &str, runs: usize, marked: Attrs, toggled: &str) -> PageDoc {
    let toggle = || {
        let mut attrs = Attrs::new();
        attrs.insert(toggled.into(), Any::Map(Arc::new(HashMap::new())));
        Some(Box::new(attrs))
    };
    page_from(|doc| {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let group = fragment.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
        let container = group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "bloc");
        let element = container.push_back(&mut txn, XmlElementPrelim::empty(tag));
        let text = element.push_back(&mut txn, XmlTextPrelim::new(""));
        for i in (0..runs).rev() {
            text.insert(&mut txn, 0, if i % 2 == 0 { "a" } else { "b" });
        }
        text.format(&mut txn, 0, runs as u32, marked);
        let formats: Vec<Delta<Any>> = (0..runs)
            .map(|i| Delta::Retain(1, if i % 2 == 1 { toggle() } else { None }))
            .collect();
        text.apply_delta(&mut txn, formats);
    })
}

/// Un paragraphe de `runs` plages d'une lettre, en italique une sur deux, toutes sous la marque
/// `name` (`textColor`, `link`…) de valeur `value`.
fn alternating_runs(runs: usize, name: &str, key: &str, value: &str) -> PageDoc {
    let mut marked = Attrs::new();
    marked.insert(name.into(), mark(key, value));
    marked_runs("paragraph", runs, marked, "italic")
}

fn size(page: &PageDoc) -> usize {
    page.encode_state().len()
}

/// Ce que lit `GET /mmd` : les blocs de la page, rendus en MMD.
fn read(page: &PageDoc) -> String {
    render(&page.read_blocks(), RenderOpts::default()).markdown
}

/// Les bornes absolues de la lecture d'une page (Ruling R433), par octet de son état Yjs : 64
/// octets de pic (au plus 22 relevés, pour un nœud étranger), et un MMD de 8 octets (au plus 2,7,
/// pour des plages alternées sous une couleur de 100 octets, que le MMD récrit à chaque plage).
const LIMITS: Limits = Limits {
    peak_per_byte: 64,
    answer: Answer::PerByte(8),
};

/// Ruling R381 : une couleur ou une adresse de lien longue, sur des plages alternées, était
/// recopiée à chaque plage lue (et le MMD la récrivait à chaque plage) : le produit de sa
/// longueur par leur nombre, en mémoire pour la couleur, en temps pour l'adresse.
#[test]
fn reading_a_page_grows_like_its_document() {
    let long_color = |n: usize| alternating_runs(n, "textColor", "stringValue", &"c".repeat(4 * n));
    let long_background =
        |n: usize| alternating_runs(n, "backgroundColor", "stringValue", &"c".repeat(4 * n));
    let allowed_color =
        |n: usize| alternating_runs(n, "textColor", "stringValue", &"c".repeat(MAX_COLOR_BYTES));
    let long_link = |n: usize| alternating_runs(n, "link", "href", &"u".repeat(4 * n));
    let allowed_link = |n: usize| alternating_runs(n, "link", "href", &"u".repeat(MAX_HREF_BYTES));
    // Depuis le Ruling R431, une couleur de plus de 100 octets ou une adresse de plus de 16 Kio,
    // relue du journal, est supprimée : les formes longues disent que cela garde la lecture
    // linéaire ; celles à la borne, que la lecture d'une marque gardée l'est aussi.
    let forms = [
        Form {
            what: "couleur longue sur des plages alternées",
            build: &long_color,
        },
        Form {
            what: "surlignage long sur des plages alternées",
            build: &long_background,
        },
        Form {
            what: "plages alternées sous une couleur permise",
            build: &allowed_color,
        },
        Form {
            what: "adresse de lien longue sur des plages alternées",
            build: &long_link,
        },
        Form {
            what: "adresse de lien à la borne sur des plages alternées",
            build: &allowed_link,
        },
    ];
    let failures = grows_like_input(&forms, size, read, String::len, LIMITS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Un paragraphe de `runs` lettres sous `keys` marques hors du schéma (`k0`, `k1`…), posées une
/// fois sur tout le texte, le gras une lettre sur deux : la mise à jour qu'un client aurait pu
/// écrire avant le Ruling R431, que la page relit de son journal.
fn unknown_marks(keys: usize, runs: usize) -> PageDoc {
    let mut attrs = Attrs::new();
    for k in 0..keys {
        attrs.insert(format!("k{k}").into(), Any::Bool(true));
    }
    marked_runs("paragraph", runs, attrs, "bold")
}

/// Un nœud étranger (balise inconnue dans un `blockContainer`) dont le texte de `runs` lettres
/// porte la marque `name` de valeur `value` sur tout le texte, le gras une lettre sur deux.
fn foreign_marked(runs: usize, name: &str, value: Any) -> PageDoc {
    let mut marked = Attrs::new();
    marked.insert(name.into(), value);
    marked_runs("widget", runs, marked, "bold")
}

/// Constat critique C1 de la re-relecture du tour 4 (Rulings R431, R432) : des marques hors du
/// schéma, nombreuses, sur des plages alternées, coûtaient leur nombre multiplié par celui des
/// plages à chaque lecture (701 Ko envoyés, 5 Gio pour une lecture) ; dans un nœud étranger, une
/// marque longue était récrite en JSON à chaque plage (188 Ko envoyés, 86 Mo de MMD). Relues du
/// journal, les marques hors du schéma sont supprimées (R431) ; une marque du schéma n'est écrite
/// dans le JSON d'un nœud étranger qu'à la plage où elle change (R432).
#[test]
fn reading_marks_grows_like_the_document() {
    let unknown = |n: usize| unknown_marks(n, n);
    let foreign_unknown = |n: usize| foreign_marked(n, "note", Any::from("n".repeat(4 * n)));
    let foreign_link =
        |n: usize| foreign_marked(n, "link", mark("href", &"u".repeat(MAX_HREF_BYTES)));
    let forms = [
        Form {
            what: "marques hors du schéma, nombreuses, sur des plages alternées",
            build: &unknown,
        },
        Form {
            what: "nœud étranger, marque hors du schéma longue sur des plages alternées",
            build: &foreign_unknown,
        },
        Form {
            what: "nœud étranger, adresse à la borne sur des plages alternées",
            build: &foreign_link,
        },
    ];
    let failures = grows_like_input(&forms, size, read, String::len, LIMITS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Ruling R381 : une couleur plus longue que la borne est lue absente, comme `parse` la refuse ;
/// à la borne, elle est lue. Le MMD rendu se relit.
#[test]
fn a_color_longer_than_the_bound_is_read_absent() {
    for (len, kept) in [(MAX_COLOR_BYTES, true), (MAX_COLOR_BYTES + 1, false)] {
        let page = alternating_runs(4, "textColor", "stringValue", &"c".repeat(len));
        let mmd = read(&page);
        assert_eq!(mmd.contains("<span color="), kept, "{len} octets : {mmd}");
        mementum_md::parse(&mmd).unwrap_or_else(|e| panic!("{mmd} : {e}"));
    }
}

/// Des listes de colonnes d'une seule colonne (que la lecture défait), imbriquées sur `depth`
/// niveaux, un bloc par niveau.
fn nested_single_columns(depth: usize) -> PageDoc {
    page_from(|doc| {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let mut group: XmlElementRef =
            fragment.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
        for i in 0..depth {
            let list = group.push_back(&mut txn, XmlElementPrelim::empty("columnList"));
            let column = list.push_back(&mut txn, XmlElementPrelim::empty("column"));
            let container = column.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
            container.insert_attribute(&mut txn, "id", format!("b{i}"));
            let paragraph = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
            paragraph.push_back(&mut txn, XmlTextPrelim::new("x"));
            group = container.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        }
    })
}

/// Ruling R385 : une liste de colonnes défaite était lue deux fois, pour la juger puis défaite ;
/// imbriquées, chaque niveau coûtait le double du précédent (vingt niveaux : 2,2 s en release ;
/// cinquante tiennent sous la borne de profondeur du document, soit des jours d'acteur). Au plus
/// profond, la lecture et la recherche d'un bloc tiennent dans une seconde ; la lecture se fait
/// sur un fil à part, pour que le test échoue au bout du délai au lieu de durer des heures.
#[test]
fn nested_invalid_column_lists_are_read_once() {
    let depth = 50;
    let page = nested_single_columns(depth);
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || {
        let start = Instant::now();
        let blocks = page.read_blocks();
        let read = start.elapsed();
        let start = Instant::now();
        let found = page.has_block(&format!("b{}", depth - 1)) && !page.has_block("absent");
        let _ = sent.send((blocks, read, found, start.elapsed()));
    });
    let (blocks, read, found, search) = received
        .recv_timeout(Duration::from_secs(10))
        .expect("la lecture de 50 listes défaites imbriquées dure plus de dix secondes");
    assert!(
        found,
        "le bloc le plus profond se trouve, un bloc absent non"
    );
    // Chaque liste défaite laisse son bloc à sa place : un bloc par niveau, chacun enfant du
    // précédent ; au premier niveau, à côté du paragraphe vide de la page neuve.
    assert_eq!(blocks.len(), 2, "le paragraphe de la page neuve, et b0");
    let first = blocks.iter().position(|b| b.id == "b0").expect("b0");
    let mut level = &blocks[first..=first];
    for i in 0..depth {
        assert_eq!(level.len(), 1, "niveau {i}");
        assert_eq!(level[0].id, format!("b{i}"));
        level = &level[0].children;
    }
    assert!(
        read < Duration::from_secs(1) && search < Duration::from_secs(1),
        "lecture {read:?}, recherche {search:?}"
    );
}

/// `depth` listes de colonnes imbriquées sans bloc entre elles, chacune de deux colonnes : la
/// première porte la liste suivante, la seconde est vide (la liste est défaite). Sous la dernière,
/// `width` colonnes vides puis un bloc.
fn wrapped_columns(depth: usize, width: usize) -> PageDoc {
    page_from(|doc| {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let group = fragment.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
        let mut parent: XmlElementRef = group;
        for _ in 0..depth {
            let list = parent.push_back(&mut txn, XmlElementPrelim::empty("columnList"));
            let first = list.push_back(&mut txn, XmlElementPrelim::empty("column"));
            list.push_back(&mut txn, XmlElementPrelim::empty("column"));
            parent = first;
        }
        for _ in 0..width {
            parent.push_back(&mut txn, XmlElementPrelim::empty("column"));
        }
        let container = parent.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "fond");
        let paragraph = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        paragraph.push_back(&mut txn, XmlTextPrelim::new("x"));
    })
}

/// Ruling R385 : une colonne n'est examinée qu'une fois par lecture, quelle que soit la
/// profondeur des listes défaites qui la contiennent. Sans cela, chaque liste défaite examinait de
/// nouveau tout ce qu'elle contient : cinquante niveaux au-dessus de 5 000 colonnes vides, un
/// quart de million d'éléments parcourus. Les octets alloués (une liste d'enfants par élément parcouru) le
/// disent sans dépendre du poste : avec cinquante niveaux, guère plus qu'avec un.
#[test]
fn wrapped_columns_are_examined_once() {
    let _alone = linear::ALONE.lock().unwrap_or_else(|e| e.into_inner());
    let width = 5_000;
    let (shallow, deep) = (wrapped_columns(1, width), wrapped_columns(50, width));
    for (what, read) in [
        (
            "lecture",
            &(|page: &PageDoc| drop(page.read_blocks())) as &dyn Fn(&PageDoc),
        ),
        ("recherche", &|page: &PageDoc| {
            assert!(page.has_block("fond"));
        }),
    ] {
        let (one, fifty) = (
            linear::once(|| read(&shallow)),
            linear::once(|| read(&deep)),
        );
        assert!(
            fifty.allocated <= 4 * one.allocated,
            "{what} : {} octets alloués sous un niveau, {} sous cinquante",
            one.allocated,
            fifty.allocated
        );
    }
}

// ------------------------------------------------------------ garde des attributs (R689)

/// Écrit un entier variable de lib0.
fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// `{"toString": 1}`.
fn object() -> Any {
    Any::Map(Arc::new(HashMap::from([(
        "toString".to_string(),
        Any::Number(yrs::any::Number::Int(1)),
    )])))
}

/// Un client d'identifiant fixé, qui a reçu `state`.
fn client_of(id: u64, state: &[u8]) -> Doc {
    use yrs::updates::decoder::Decode;
    let doc = Doc::with_options(yrs::Options {
        client_id: yrs::ClientID::new(id),
        offset_kind: yrs::OffsetKind::Utf16,
        ..yrs::Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(yrs::Update::decode_v1(state).unwrap())
        .unwrap();
    doc
}

/// Le paragraphe d'une page neuve.
fn first_paragraph<T: ReadTxn>(txn: &T) -> XmlElementRef {
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    let mut element = match fragment.get(txn, 0) {
        Some(yrs::XmlOut::Element(e)) => e,
        _ => panic!("la racine"),
    };
    for _ in 0..2 {
        element = match element.get(txn, 0) {
            Some(yrs::XmlOut::Element(e)) => e,
            _ => panic!("un élément"),
        };
    }
    element
}

/// Une page et la mise à jour qu'un client lui envoie, ou que son journal porte.
struct Sent {
    state: Vec<u8>,
    update: Vec<u8>,
}

/// `n` blocs neufs, chacun avec un attribut d'un autre type que le sien, une liste de largeurs
/// démesurée et une valeur imbriquée.
fn many_unsound_blocks(n: usize) -> Sent {
    let (_, state) = PageDoc::new_page();
    let doc = client_of(7, &state);
    let mut txn = doc.transact_mut();
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    let Some(yrs::XmlOut::Element(root)) = fragment.get(&txn, 0) else {
        panic!("la racine");
    };
    let nested = (0..8).fold(object(), |inner, _| Any::Array(vec![inner].into()));
    for i in 0..n {
        let container = root.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", format!("b{i}"));
        let heading = container.push_back(&mut txn, XmlElementPrelim::empty("heading"));
        heading.insert_attribute(&mut txn, "level", object());
        heading.insert_attribute(&mut txn, "backgroundColor", nested.clone());
        heading.insert_attribute(&mut txn, "textAlignment", "center");
        heading.insert_attribute(
            &mut txn,
            "colwidth",
            Any::Array(vec![Any::Number(yrs::any::Number::Int(120)); 200].into()),
        );
    }
    let update = txn.encode_update_v1();
    Sent { state, update }
}

/// Un attribut du paragraphe écrasé `n` fois dans la même mise à jour : une chaîne d'origines.
fn one_attribute_overwritten(n: usize) -> Sent {
    let (_, state) = PageDoc::new_page();
    let doc = client_of(7, &state);
    let mut txn = doc.transact_mut();
    let paragraph = first_paragraph(&txn);
    for i in 0..n {
        let value = if i % 2 == 0 {
            object()
        } else {
            Any::from("red")
        };
        paragraph.insert_attribute(&mut txn, "backgroundColor", value);
    }
    let update = txn.encode_update_v1();
    Sent { state, update }
}

/// Une page dont le paragraphe porte `n` lettres, chacune un élément Yjs, et une mise à jour
/// faite à la main de `n` valeurs `Any`, chacune placée par l'origine d'une de ces lettres : la
/// garde cherche, pour chacune, si son origine est une entrée de la page (la recherche suit les
/// voisins de l'origine ; sans plafond, `n` recherches de `n` pas).
fn values_placed_after_page_items(n: usize) -> Sent {
    let (_, root) = PageDoc::new_page();
    let doc = client_of(9, &root);
    let first = {
        let mut txn = doc.transact_mut();
        let paragraph = first_paragraph(&txn);
        let text = paragraph.push_back(&mut txn, XmlTextPrelim::new(""));
        let first = txn.state_vector().get(&yrs::ClientID::new(9));
        for _ in 0..n {
            text.insert(&mut txn, 0, "a");
        }
        first
    };
    let state = doc
        .transact()
        .encode_state_as_update_v1(&yrs::StateVector::default());
    // Un client, `n` éléments à partir de l'horloge 0 : contenu `Any` (8) et une origine (0x80).
    let mut update = Vec::new();
    var(&mut update, 1);
    var(&mut update, n as u64);
    var(&mut update, 11);
    var(&mut update, 0);
    for i in 0..n {
        update.push(0x88);
        var(&mut update, 9);
        var(&mut update, u64::from(first) + i as u64);
        var(&mut update, 1);
        update.extend_from_slice(&[119, 1, b'z']);
    }
    update.push(0);
    Sent { state, update }
}

/// Garde des attributs (Rulings R689 à R691) : juger une mise à jour, d'un client comme relue du
/// journal, croît comme elle et comme la page. Trois formes : des blocs nombreux aux attributs
/// fautifs, un attribut écrasé de nombreuses fois, et des valeurs placées par des origines de la
/// page (le plafond des recherches, `MAX_PAGE_LOOKUPS`).
#[test]
fn judging_attributes_grows_like_the_update() {
    let forms = [
        Form {
            what: "blocs nombreux, chacun aux attributs fautifs",
            build: &many_unsound_blocks,
        },
        Form {
            what: "un attribut écrasé de nombreuses fois",
            build: &one_attribute_overwritten,
        },
        Form {
            what: "valeurs placées par des origines de la page",
            build: &values_placed_after_page_items,
        },
    ];
    let size = |sent: &Sent| sent.state.len() + sent.update.len();
    // D'un client (refusée, ou acceptée : des valeurs dans un texte), puis relue du journal.
    let judge = |sent: &Sent| {
        let from_client = {
            let page = PageDoc::from_updates([sent.state.as_slice()]).unwrap();
            page.try_client_update(&sent.update).is_ok()
        };
        let stored =
            PageDoc::from_updates([sent.state.as_slice(), sent.update.as_slice()]).unwrap();
        (from_client, stored.encode_state().len())
    };
    let limits = Limits {
        peak_per_byte: 64,
        answer: Answer::PerByte(8),
    };
    let failures = grows_like_input(&forms, size, judge, |_| 0, limits);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
