//! La lecture d'une page (`PageDoc::read_blocks`, puis le rendu en MMD de `GET /mmd`) croît
//! comme son document, quoi qu'un client y ait écrit (Ruling R381). La méthode et ce qui
//! la garde de passer à tort sont dans `mementum-md/tests/common/linear.rs`.

#[path = "../../mementum-md/tests/common/linear.rs"]
mod linear;

use std::collections::HashMap;
use std::sync::Arc;

use linear::{Form, grows_like_input};
use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{MAX_COLOR_BYTES, RenderOpts, render};
use yrs::types::{Attrs, Delta};
use yrs::{Any, Doc, ReadTxn, Text, Transact, Xml, XmlElementPrelim, XmlFragment, XmlTextPrelim};

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
