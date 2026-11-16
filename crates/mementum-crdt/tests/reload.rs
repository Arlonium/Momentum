//! La page rechargée de son journal est la page vivante (Rulings R490, R491), quoi qu'un client
//! hostile y ait envoyé : des éléments sous le plancher de la page (une horloge qu'elle connaît
//! déjà, d'un client quelconque), des marques hors du schéma, des suppressions.
//!
//! Le constat (N1 de la re-relecture du tour 5 de la piste S) : une mise à jour de 35 octets
//! posait une marque `x` au client d'Olga, à une horloge que la page connaissait (la lettre « B »
//! de son « Bonjour »), avec une suppression pour être écrite. La garde sautait ce qui est sous le
//! plancher, yrs l'ignorait, mais le réencodage de R130 (fait avant l'application) la gardait au
//! journal ; au rechargement, la suppression des marques hors du schéma (R431) retirait
//! l'identifiant, c'est-à-dire la lettre d'Olga : la page servie divergeait de son onglet.
//!
//! Et (Rulings R496 à R498, constat M1 de la re-relecture du tour 6) : un contenu qui n'est pas un
//! nœud XML (une chaîne, une valeur `Any`, un objet incorporé) parmi les enfants d'un élément ou de
//! la racine. `children` de yrs s'arrête au premier enfant qui n'est pas un nœud XML : 25 octets en
//! tête de la racine, et le serveur lisait la page vide ; un agent qui la relisait et la récrivait
//! l'effaçait. La page lue a toujours autant d'enfants que de longueur.

use mementum_crdt::{FRAGMENT, PageDoc};
use proptest::prelude::*;
use yrs::branch::{Branch, BranchID};
use yrs::types::Attrs;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, ClientID, Doc, OffsetKind, Options, ReadTxn, StateVector, Text, Transact, Update,
    XmlFragment, XmlOut, XmlTextPrelim, XmlTextRef,
};

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

fn string(out: &mut Vec<u8>, s: &str) {
    var(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

/// Ce qu'un élément fait à la main porte.
#[derive(Debug, Clone)]
enum Item {
    /// Une marque de texte (`ContentFormat`) : sa clé, sa valeur en JSON.
    Mark(&'static str, &'static str),
    /// Un texte (`ContentString`).
    Text(&'static str),
    /// Une valeur `Any` (`ContentAny`, une chaîne).
    Any(&'static str),
    /// Un objet incorporé (`ContentEmbed`, en JSON).
    Embed(&'static str),
}

/// Où un élément fait à la main s'insère.
#[derive(Debug, Clone, Copy)]
enum Place {
    /// En tête de la racine de la page.
    Root,
    /// En tête du type dont l'élément porte cet identifiant.
    In(u64, u32),
    /// Après l'élément de cet identifiant (son origine), dans le même parent.
    After(u64, u32),
}

/// Une mise à jour faite à la main : un élément `item`, au client et à l'horloge donnés, à la
/// place `place`, et des suppressions `(client, horloge, longueur)`.
fn hand_made(at: Option<(u64, u32, Place, Item)>, deletions: &[(u64, u32, u32)]) -> Vec<u8> {
    let mut u = Vec::new();
    match at {
        Some((client, clock, place, item)) => {
            var(&mut u, 1);
            var(&mut u, 1);
            var(&mut u, client);
            var(&mut u, u64::from(clock));
            let kind: u8 = match item {
                Item::Mark(..) => 6,
                Item::Text(_) => 4,
                Item::Any(_) => 8,
                Item::Embed(_) => 5,
            };
            match place {
                Place::Root => {
                    u.push(kind);
                    var(&mut u, 1);
                    string(&mut u, FRAGMENT);
                }
                Place::In(c, k) => {
                    u.push(kind);
                    var(&mut u, 0);
                    var(&mut u, c);
                    var(&mut u, u64::from(k));
                }
                Place::After(c, k) => {
                    u.push(0x80 | kind);
                    var(&mut u, c);
                    var(&mut u, u64::from(k));
                }
            }
            match item {
                Item::Mark(key, value) => {
                    string(&mut u, key);
                    string(&mut u, value);
                }
                Item::Text(text) => string(&mut u, text),
                Item::Any(text) => {
                    var(&mut u, 1);
                    u.push(119);
                    string(&mut u, text);
                }
                Item::Embed(json) => string(&mut u, json),
            }
        }
        None => var(&mut u, 0),
    }
    var(&mut u, deletions.len() as u64);
    for &(client, clock, len) in deletions {
        var(&mut u, client);
        var(&mut u, 1);
        var(&mut u, u64::from(clock));
        var(&mut u, u64::from(len));
    }
    u
}

/// Le texte de chaque bloc de la page.
fn texts(page: &PageDoc) -> Vec<String> {
    page.read_blocks()
        .iter()
        .map(mementum_md::block_text)
        .collect()
}

/// Une page servie et son journal, comme les tient l'acteur : chaque mise à jour acceptée y est
/// écrite telle que la page la rend, suivie du report d'un bloc recréé (Task 13b) et du correctif
/// du dédoublonnage ; une mise à jour refusée fait recharger la page du journal.
struct Served {
    page: PageDoc,
    journal: Vec<Vec<u8>>,
}

impl Served {
    fn new() -> Served {
        let (page, created) = PageDoc::new_page();
        Served {
            page,
            journal: vec![created],
        }
    }

    fn receive(&mut self, update: &[u8]) -> bool {
        match self.page.apply_client_update(update) {
            Ok(applied) => {
                self.journal.push(applied.read);
                self.journal.extend(applied.carried);
                self.journal.extend(self.page.dedupe_blocks());
                true
            }
            Err(_) => {
                self.page = self.reloaded();
                false
            }
        }
    }

    fn reloaded(&self) -> PageDoc {
        PageDoc::from_updates(self.journal.iter().map(Vec::as_slice)).expect("le journal se relit")
    }

    /// La page rechargée de son journal est la page vivante : même révision (vecteur d'état et
    /// suppressions), mêmes blocs, mêmes plages et mêmes marques.
    fn assert_reload_is_live(&self, what: &str) {
        let reloaded = self.reloaded();
        assert_eq!(
            reloaded.read_blocks(),
            self.page.read_blocks(),
            "{what} : blocs"
        );
        assert_eq!(
            reloaded.revision(),
            self.page.revision(),
            "{what} : révision"
        );
        assert_eq!(
            runs(&reloaded),
            runs(&self.page),
            "{what} : plages et marques"
        );
        assert_children_are_their_length(&self.page, what);
    }
}

/// Les identifiants des éléments et des textes de la page (leur élément Yjs), dans l'ordre du
/// parcours.
fn type_ids(page: &PageDoc) -> Vec<(u64, u32)> {
    let doc = page.yrs_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    fragment
        .successors(&txn)
        .filter_map(|node| {
            let branch: &Branch = match &node {
                XmlOut::Element(e) => e.as_ref(),
                XmlOut::Text(t) => t.as_ref(),
                XmlOut::Fragment(_) => return None,
            };
            match branch.id() {
                BranchID::Nested(id) => Some((id.client.get(), id.clock)),
                BranchID::Root(_) => None,
            }
        })
        .collect()
}

/// Ruling R497 : la page lue a autant d'enfants que de longueur, à la racine et dans chaque
/// élément (le parcours de `successors` ne s'arrête pas à un enfant qui n'est pas un nœud XML,
/// `children` si).
fn assert_children_are_their_length(page: &PageDoc, what: &str) {
    let doc = page.yrs_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    assert_eq!(
        fragment.children(&txn).count() as u32,
        fragment.len(&txn),
        "{what} : la racine"
    );
    for node in fragment.successors(&txn) {
        if let XmlOut::Element(e) = node {
            assert_eq!(
                e.children(&txn).count() as u32,
                e.len(&txn),
                "{what} : un élément « {} »",
                e.tag()
            );
        }
    }
}

/// Les plages de tous les textes d'une page, avec leurs marques (relues par un document neuf).
fn runs(page: &PageDoc) -> Vec<String> {
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(&page.encode_state()).unwrap())
        .unwrap();
    let txn = doc.transact();
    let mut out = Vec::new();
    for node in fragment.successors(&txn) {
        if let XmlOut::Text(text) = node {
            for run in text.diff(&txn, yrs::types::text::YChange::identity) {
                let mut marks: Vec<String> = run
                    .attributes
                    .iter()
                    .flat_map(|a| a.iter())
                    .map(|(k, v)| format!("{k}={v:?}"))
                    .collect();
                marks.sort();
                out.push(format!("{:?} {marks:?}", run.insert));
            }
        }
    }
    out
}

/// Un client honnête : un document de la page, à l'identifiant donné.
fn client(id: u64, served: &Served) -> Doc {
    let doc = Doc::with_options(Options {
        client_id: ClientID::new(id),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    catch_up(&doc, served);
    doc
}

/// Le client reçoit ce qui lui manque de la page.
fn catch_up(doc: &Doc, served: &Served) {
    let state = doc.transact().state_vector().encode_v1();
    let missing = served.page.encode_diff(&state).unwrap();
    doc.transact_mut()
        .apply_update(Update::decode_v1(&missing).unwrap())
        .unwrap();
}

/// Le texte du premier paragraphe, créé au besoin ; `None` si une suppression faite à la main a
/// emporté la racine, le bloc ou le paragraphe.
fn paragraph_text(
    txn: &mut yrs::TransactionMut,
    fragment: &yrs::XmlFragmentRef,
) -> Option<XmlTextRef> {
    let Some(XmlOut::Element(group)) = fragment.get(txn, 0) else {
        return None;
    };
    let Some(XmlOut::Element(container)) = group.get(txn, 0) else {
        return None;
    };
    let Some(XmlOut::Element(paragraph)) = container.get(txn, 0) else {
        return None;
    };
    Some(match paragraph.get(txn, 0) {
        Some(XmlOut::Text(text)) => text,
        _ => paragraph.insert(txn, 0, XmlTextPrelim::new("")),
    })
}

/// Ce qu'un client honnête écrit : sa mise à jour (vide si le paragraphe n'est plus là, ou si
/// son texte porte un contenu fait à la main que yrs ne sait pas retirer d'un texte, une valeur
/// `Any`).
fn honest(doc: &Doc, edit: impl FnOnce(&mut yrs::TransactionMut, &XmlTextRef)) -> Vec<u8> {
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = doc.transact_mut();
    if let Some(text) = paragraph_text(&mut txn, &fragment) {
        // `diff` ne montre pas une valeur `Any` d'un texte, que sa longueur compte.
        let mut shown = 0;
        let plain = text
            .diff(&txn, yrs::types::text::YChange::identity)
            .iter()
            .all(|run| match &run.insert {
                yrs::Out::Any(Any::String(s)) => {
                    shown += s.encode_utf16().count() as u32;
                    true
                }
                _ => false,
            })
            && shown == text.len(&txn);
        if plain {
            edit(&mut txn, &text);
        }
    }
    txn.encode_update_v1()
}

/// Le constat N1, tel que la sonde du relecteur le montre.
#[test]
fn a_mark_at_a_known_clock_deletes_nothing_on_reload() {
    let mut served = Served::new();
    let olga = client(1234, &served);
    let typed = honest(&olga, |txn, text| text.insert(txn, 0, "Bonjour"));
    assert!(served.receive(&typed));
    assert_eq!(texts(&served.page), ["Bonjour"]);
    // L'XmlText d'Olga est son horloge 0, « Bonjour » ses horloges 1 à 7 : une marque « x » à
    // l'horloge 1 (la lettre « B »), et la suppression de « r » (horloge 7).
    let attack = hand_made(
        Some((1234, 1, Place::Root, Item::Mark("x", "true"))),
        &[(1234, 7, 1)],
    );
    assert!(
        served.receive(&attack),
        "la garde laisse passer ce qui est sous le plancher"
    );
    assert_eq!(texts(&served.page), ["Bonjou"]);
    served.assert_reload_is_live("une marque à une horloge connue");
}

/// Ruling R491 : ce que rend `apply_client_update`, que l'acteur écrit au journal et relaie, ne
/// porte que ce que la page a intégré : ni la marque à une horloge connue (yrs l'ignore), ni rien
/// d'autre que la suppression.
#[test]
fn the_journal_keeps_only_what_the_page_integrated() {
    let mut served = Served::new();
    let olga = client(1234, &served);
    assert!(served.receive(&honest(&olga, |txn, text| text.insert(txn, 0, "Bonjour"))));
    let attack = hand_made(
        Some((1234, 1, Place::Root, Item::Mark("x", "true"))),
        &[(1234, 7, 1)],
    );
    let read = served.page.apply_client_update(&attack).unwrap().read;
    let decoded = Update::decode_v1(&read).unwrap();
    assert!(decoded.insertions(true).is_empty(), "aucun élément");
    let deleted: Vec<(u64, u32, u32)> = decoded
        .delete_set()
        .iter()
        .flat_map(|(c, r)| r.iter().map(move |r| (c.get(), r.start, r.end)))
        .collect();
    assert_eq!(deleted, [(1234, 7, 8)]);
}

/// Ruling R490 : une entrée déjà au journal (écrite avant R491) qui porte une marque hors du schéma
/// à une horloge connue ne supprime rien à la relecture.
#[test]
fn a_stored_mark_at_a_known_clock_deletes_nothing() {
    let mut served = Served::new();
    let olga = client(1234, &served);
    assert!(served.receive(&honest(&olga, |txn, text| text.insert(txn, 0, "Bonjour"))));
    // L'entrée telle que l'écrivait le réencodage d'avant R491 : la marque, et la suppression.
    let legacy = hand_made(
        Some((1234, 1, Place::Root, Item::Mark("x", "true"))),
        &[(1234, 7, 1)],
    );
    let mut journal = served.journal.clone();
    journal.push(legacy);
    let reloaded = PageDoc::from_updates(journal.iter().map(Vec::as_slice)).unwrap();
    assert_eq!(texts(&reloaded), ["Bonjou"]);
}

/// La page « Un / Deux / Trois », écrite par le serveur, et son journal.
fn served_three() -> Served {
    let mut served = Served::new();
    let blocks = mementum_md::parse("Un\n\nDeux\n\nTrois\n").unwrap();
    let written = served.page.write_blocks(&blocks).unwrap();
    served.journal.push(written);
    served
}

/// Les identifiants du `blockGroup` racine, du premier `blockContainer`, de son paragraphe et du
/// texte de celui-ci.
fn first_path(page: &PageDoc) -> [(u64, u32); 4] {
    let ids = type_ids(page);
    [ids[0], ids[1], ids[2], ids[3]]
}

/// Rulings R496, R497 (constat M1) : un contenu qui n'est pas un nœud XML, posé parmi les enfants
/// de la racine ou d'un élément, est refusé, que son parent soit nommé (la racine, un élément par
/// son identifiant) ou donné par son origine (un voisin) ; la page lue reste entière. Dans un
/// texte, il est du texte, et passe.
#[test]
fn untyped_content_among_the_children_of_an_element_is_refused() {
    let items = [Item::Text("x"), Item::Any("x"), Item::Embed("{}")];
    // Où le poser, d'après les identifiants de la page ; et vrai si la garde le juge avant
    // d'appliquer (le parent est nommé), faux s'il l'est après (le parent vient de l'origine).
    type Where = fn([(u64, u32); 4]) -> (Place, bool);
    let places: [(&str, Where); 6] = [
        ("la racine", |_| (Place::Root, true)),
        ("le blockGroup", |[g, ..]| (Place::In(g.0, g.1), true)),
        ("un blockContainer", |[_, c, ..]| {
            (Place::In(c.0, c.1), true)
        }),
        ("un paragraphe", |[_, _, p, _]| (Place::In(p.0, p.1), true)),
        ("après un blockContainer", |[_, c, ..]| {
            (Place::After(c.0, c.1), false)
        }),
        ("après un paragraphe", |[_, _, p, _]| {
            (Place::After(p.0, p.1), false)
        }),
    ];
    for item in items {
        for (what, place_of) in places {
            let mut served = served_three();
            let (place, before) = place_of(first_path(&served.page));
            let attack = hand_made(Some((4242, 0, place, item.clone())), &[]);
            let refused = served
                .page
                .try_client_update(&attack)
                .expect_err(&format!("{item:?} dans {what} : accepté"));
            assert!(
                refused.error.to_string().contains("n'est pas un nœud XML"),
                "{item:?} dans {what} : {refused:?}"
            );
            assert_eq!(refused.intact, before, "{item:?} dans {what} : {refused:?}");
            // Comme l'acteur : un refus après l'application fait recharger la page.
            if !refused.intact {
                served.page = served.reloaded();
            }
            assert_eq!(
                texts(&served.page),
                ["Un", "Deux", "Trois"],
                "{item:?} dans {what}"
            );
            served.assert_reload_is_live(&format!("{item:?} dans {what}"));
        }
    }
    // Dans un texte, une chaîne est du texte : après la lettre « U » (son origine).
    let mut served = served_three();
    let [.., text] = first_path(&served.page);
    let typed = hand_made(
        Some((4242, 0, Place::After(text.0, text.1 + 1), Item::Text("x"))),
        &[],
    );
    assert!(served.receive(&typed));
    assert_eq!(texts(&served.page), ["Uxn", "Deux", "Trois"]);
    served.assert_reload_is_live("une chaîne dans un texte");
}

/// Ruling R498 : une entrée déjà au journal (écrite avant R496) qui pose un tel contenu parmi les
/// enfants d'un élément ou de la racine en est retirée à la relecture ; une entrée qui le pose à
/// une horloge que la page connaît déjà ne retire rien (R490).
#[test]
fn a_stored_untyped_child_is_deleted_on_reload() {
    for place_of in [
        (|_: &PageDoc| Place::Root) as fn(&PageDoc) -> Place,
        |page| {
            let [group, ..] = first_path(page);
            Place::In(group.0, group.1)
        },
        |page| {
            let [_, container, ..] = first_path(page);
            Place::After(container.0, container.1)
        },
    ] {
        let served = served_three();
        let place = place_of(&served.page);
        let mut journal = served.journal.clone();
        journal.push(hand_made(Some((4242, 0, place, Item::Text("x"))), &[]));
        let reloaded = PageDoc::from_updates(journal.iter().map(Vec::as_slice)).unwrap();
        assert_eq!(texts(&reloaded), ["Un", "Deux", "Trois"], "{place:?}");
        assert_children_are_their_length(&reloaded, &format!("{place:?}"));
        // Sous le plancher : rien n'est retiré (la lettre « U » est au client du serveur).
        let [.., text] = first_path(&served.page);
        let mut journal = served.journal.clone();
        journal.push(hand_made(
            Some((text.0, text.1 + 1, Place::Root, Item::Text("x"))),
            &[],
        ));
        let reloaded = PageDoc::from_updates(journal.iter().map(Vec::as_slice)).unwrap();
        assert_eq!(
            texts(&reloaded),
            ["Un", "Deux", "Trois"],
            "{place:?}, sous le plancher"
        );
    }
}

/// Une opération d'un test de propriétés : celle d'un client honnête, ou une mise à jour faite à la
/// main.
#[derive(Debug, Clone)]
enum Op {
    /// Le client `who` tape `text` à la position `at` (ramenée à la longueur du texte).
    Type { who: usize, at: usize, text: String },
    /// Le client `who` supprime `len` caractères à partir de `at`.
    Delete { who: usize, at: usize, len: usize },
    /// Le client `who` met en gras `len` caractères à partir de `at`.
    Bold { who: usize, at: usize, len: usize },
    /// Une mise à jour faite à la main : `item` à l'horloge `clock` (ramenée sous le plancher si
    /// `below`) d'un client de la page, à la place `place` (la racine, en tête d'un type de la page,
    /// ou après un élément connu), et une suppression d'un élément connu.
    Hostile {
        item: Option<(usize, u32, bool, usize, (usize, usize))>,
        delete: Option<(usize, u32)>,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..2usize, any::<usize>(), "[a-zé]{1,4}").prop_map(|(who, at, text)| Op::Type {
            who,
            at,
            text
        }),
        (0..2usize, any::<usize>(), 1..3usize).prop_map(|(who, at, len)| Op::Delete {
            who,
            at,
            len
        }),
        (0..2usize, any::<usize>(), 1..4usize).prop_map(|(who, at, len)| Op::Bold { who, at, len }),
        (
            proptest::option::of((
                any::<usize>(),
                any::<u32>(),
                any::<bool>(),
                0..ITEMS.len(),
                (0..3usize, any::<usize>())
            )),
            proptest::option::of((any::<usize>(), any::<u32>()))
        )
            .prop_map(|(item, delete)| Op::Hostile { item, delete }),
    ]
}

/// Les éléments faits à la main : une marque hors du schéma, une marque du schéma, une marque trop
/// longue, un texte, une valeur `Any`, un objet incorporé.
const ITEMS: [Item; 6] = [
    Item::Mark("x", "true"),
    Item::Mark("bold", "{}"),
    Item::Mark(
        "bold",
        "{\"x\":\"yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy\"}",
    ),
    Item::Text("z"),
    Item::Any("z"),
    Item::Embed("{}"),
];

fn bounded(at: usize, len: usize) -> u32 {
    if len == 0 { 0 } else { (at % (len + 1)) as u32 }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Ruling R490, R491 : après chaque mise à jour acceptée, la page rechargée de son journal est
    /// la page vivante, quelles que soient les mises à jour faites à la main qui s'y mêlent.
    #[test]
    fn a_reloaded_page_is_the_live_page(ops in prop::collection::vec(op(), 1..24)) {
        let mut served = Served::new();
        let clients = [client(10, &served), client(20, &served)];
        let start = honest(&clients[0], |txn, text| text.insert(txn, 0, "abc"));
        prop_assert!(served.receive(&start));
        for (step, op) in ops.iter().enumerate() {
            for doc in &clients {
                catch_up(doc, &served);
            }
            let update = match op {
                Op::Type { who, at, text } => honest(&clients[*who], |txn, t| {
                    let at = bounded(*at, t.len(txn) as usize);
                    t.insert(txn, at, text);
                }),
                Op::Delete { who, at, len } => honest(&clients[*who], |txn, t| {
                    let total = t.len(txn);
                    let at = bounded(*at, total as usize);
                    let len = (*len as u32).min(total - at);
                    if len > 0 {
                        t.remove_range(txn, at, len);
                    }
                }),
                Op::Bold { who, at, len } => honest(&clients[*who], |txn, t| {
                    let total = t.len(txn);
                    let at = bounded(*at, total as usize);
                    let len = (*len as u32).min(total - at);
                    if len > 0 {
                        let mut attrs = Attrs::new();
                        attrs.insert("bold".into(), Any::Map(Default::default()));
                        t.format(txn, at, len, attrs);
                    }
                }),
                Op::Hostile { item, delete } => {
                    // Les clients de la page et leur horloge.
                    let state = StateVector::decode_v1(&served.page.state_vector()).unwrap();
                    let known: Vec<(u64, u32)> =
                        state.iter().map(|(c, k)| (c.get(), *k)).filter(|(_, k)| *k > 0).collect();
                    let pick = |n: usize| known[n % known.len()];
                    let types = type_ids(&served.page);
                    let item = item.as_ref().map(|(who, clock, below, kind, (place, at))| {
                        let (client, floor) = pick(*who);
                        let clock = if *below { clock % floor } else { floor };
                        let place = match place {
                            1 if !types.is_empty() => {
                                let (c, k) = types[at % types.len()];
                                Place::In(c, k)
                            }
                            0 | 1 => Place::Root,
                            _ => {
                                let (c, f) = pick(*at);
                                Place::After(c, (*at as u32) % f)
                            }
                        };
                        (client, clock, place, ITEMS[*kind].clone())
                    });
                    let deletions: Vec<(u64, u32, u32)> = delete
                        .iter()
                        .map(|(who, clock)| {
                            let (client, floor) = pick(*who);
                            (client, clock % floor, 1)
                        })
                        .collect();
                    hand_made(item, &deletions)
                }
            };
            served.receive(&update);
            served.assert_reload_is_live(&format!("étape {step} ({op:?})"));
        }
    }
}
