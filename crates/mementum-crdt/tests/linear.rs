//! La lecture d'une page (`PageDoc::read_blocks`, puis le rendu en MMD de `GET /mmd`) croît
//! comme son document, quoi qu'un client y ait écrit (Rulings R381 et R385). La méthode et ce qui
//! la garde de passer à tort sont dans `mementum-md/tests/common/linear.rs`.

#[path = "../../mementum-md/tests/common/linear.rs"]
mod linear;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use linear::{Form, grows_like_input};
use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{MAX_COLOR_BYTES, RenderOpts, render};
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

/// Un paragraphe de `runs` plages d'une lettre, en italique une sur deux, toutes sous la marque
/// `name` (`textColor`, `link`…) de valeur `value`, que le document ne porte qu'une fois : le
/// texte est écrit d'un tenant sous la marque, puis l'italique posé en une passe (`apply_delta`).
/// Écrire plage par plage reparcourrait le texte à chaque plage, et répéterait la marque.
fn alternating_runs(runs: usize, name: &str, key: &str, value: &str) -> PageDoc {
    let mut marked = Attrs::new();
    marked.insert(name.into(), mark(key, value));
    let italic = || {
        let mut attrs = Attrs::new();
        attrs.insert("italic".into(), Any::Map(Arc::new(HashMap::new())));
        Some(Box::new(attrs))
    };
    page_from(|doc| {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let group = fragment.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
        let container = group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "bloc");
        let paragraph = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        let text = paragraph.push_back(&mut txn, XmlTextPrelim::new(""));
        let letters = "ab".repeat(runs / 2) + if runs % 2 == 1 { "a" } else { "" };
        text.apply_delta(
            &mut txn,
            [Delta::Inserted(Any::from(letters), Some(Box::new(marked)))],
        );
        let formats: Vec<Delta<Any>> = (0..runs)
            .map(|i| Delta::Retain(1, if i % 2 == 1 { italic() } else { None }))
            .collect();
        text.apply_delta(&mut txn, formats);
    })
}

fn size(page: &PageDoc) -> usize {
    page.encode_state().len()
}

/// Ce que lit `GET /mmd` : les blocs de la page, rendus en MMD.
fn read(page: &PageDoc) -> String {
    render(&page.read_blocks(), RenderOpts::default()).markdown
}

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
    ];
    let failures = grows_like_input(&forms, size, read);
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
