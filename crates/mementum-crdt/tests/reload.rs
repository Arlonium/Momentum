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
//!
//! Et (Ruling R536) : une page dont le journal porte le report d'un bloc recréé (Task 13b : un
//! client déplace un bloc sans avoir vu ce qu'un autre vient d'y taper, et le serveur reporte la
//! frappe dans la copie) se relit comme la page vivante, et le report passe les mêmes gardes.

use mementum_crdt::{FRAGMENT, PageDoc};
use proptest::prelude::*;
use yrs::branch::{Branch, BranchID};
use yrs::types::Attrs;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, ClientID, Doc, OffsetKind, Options, Out, ReadTxn, StateVector, Text, Transact, Update,
    Xml, XmlElementPrelim, XmlFragment, XmlOut, XmlTextPrelim, XmlTextRef,
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
    /// Les reports d'un bloc recréé écrits au journal (Task 13b).
    carried: usize,
}

impl Served {
    fn new() -> Served {
        let (page, created) = PageDoc::new_page();
        Served {
            page,
            journal: vec![created],
            carried: 0,
        }
    }

    fn receive(&mut self, update: &[u8]) -> bool {
        match self.page.apply_client_update(update) {
            Ok(applied) => {
                self.journal.push(applied.read);
                self.carried += usize::from(applied.carried.is_some());
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

/// Ce qu'écrit un client qui déplace le premier bloc, comme y-prosemirror (Ruling R220) : il le
/// recrée au même `id` à la fin de la racine, avec le texte qu'il en voit (marques comprises), et
/// supprime l'original. Une mise à jour vide si la page n'a plus de premier bloc à `id` dont le
/// premier enfant est un élément, ou si son texte porte un contenu fait à la main.
fn moved(doc: &Doc) -> Vec<u8> {
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = doc.transact_mut();
    let found = (|| {
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            return None;
        };
        let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
            return None;
        };
        let Some(Out::Any(Any::String(id))) = container.get_attribute(&txn, "id") else {
            return None;
        };
        let Some(XmlOut::Element(content)) = container.get(&txn, 0) else {
            return None;
        };
        let runs = match content.get(&txn, 0) {
            Some(XmlOut::Text(text)) => {
                let mut shown = 0;
                let mut runs = Vec::new();
                for run in text.diff(&txn, yrs::types::text::YChange::identity) {
                    let Out::Any(Any::String(s)) = run.insert else {
                        return None;
                    };
                    shown += s.encode_utf16().count() as u32;
                    let attrs: Attrs = run.attributes.map(|a| *a).unwrap_or_default();
                    runs.push((s.to_string(), attrs));
                }
                if shown != text.len(&txn) {
                    return None;
                }
                runs
            }
            None => Vec::new(),
            Some(_) => return None,
        };
        Some((group, id.to_string(), content.tag().to_string(), runs))
    })();
    if let Some((group, id, tag, runs)) = found {
        let len = group.len(&txn);
        let copy = group.insert(&mut txn, len, XmlElementPrelim::empty("blockContainer"));
        copy.insert_attribute(&mut txn, "id", id);
        let content = copy.push_back(&mut txn, XmlElementPrelim::empty(tag.as_str()));
        let text = content.push_back(&mut txn, XmlTextPrelim::new(""));
        let mut at = 0;
        for (run, attrs) in runs {
            text.insert_with_attributes(&mut txn, at, &run, attrs);
            at += run.encode_utf16().count() as u32;
        }
        group.remove_range(&mut txn, 0, 1);
    }
    txn.encode_update_v1()
}

/// Ruling R536 : Olga tape en gras dans un bloc que Mona déplace sans avoir vu la frappe. Le
/// serveur reporte la frappe dans la copie de Mona ; le report, au journal avec la mise à jour,
/// passe les gardes, et la page rechargée est la page vivante.
#[test]
fn a_page_with_a_carried_report_reloads_as_live() {
    let mut served = Served::new();
    let clients = [client(10, &served), client(20, &served)];
    assert!(served.receive(&honest(&clients[0], |txn, t| t.insert(txn, 0, "abc"))));
    for doc in &clients {
        catch_up(doc, &served);
    }
    let typed = honest(&clients[0], |txn, t| {
        let mut attrs = Attrs::new();
        attrs.insert("bold".into(), Any::Map(Default::default()));
        t.insert_with_attributes(txn, 1, "XY", attrs);
    });
    assert!(served.receive(&typed));
    assert!(
        served.receive(&moved(&clients[1])),
        "le déplacement de Mona"
    );
    assert_eq!(served.carried, 1, "un report");
    assert_eq!(texts(&served.page), ["aXYbc"]);
    served.assert_reload_is_live("un report au journal");
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
    /// L'autre client tape `typed` à la position `at` du premier bloc ; `who`, qui ne l'a pas vu,
    /// déplace ce bloc (Ruling R536 : le serveur reporte la frappe dans la copie).
    Move {
        who: usize,
        at: usize,
        typed: String,
    },
    /// Une mise à jour faite à la main : `item` à l'horloge `clock` (ramenée sous le plancher si
    /// `below`) d'un client de la page, à la place `place` (la racine, en tête d'un type de la page,
    /// ou après un élément connu), et une suppression d'un élément connu.
    Hostile {
        item: Option<HostileItem>,
        delete: Option<(usize, u32)>,
    },
    /// Un client de passage pose l'attribut `ATTRS[attr]` sur le premier bloc (`on_container`)
    /// ou sur son contenu ; si `raced`, un autre client de passage, parti du même état, a posé
    /// juste avant une valeur honnête sous la même clé : la valeur a alors pour origine une
    /// entrée qui n'est plus l'entrée courante (garde des attributs, Rulings R689 à R691).
    ///
    /// `stored` : la mise à jour n'est pas reçue d'un client mais déjà au journal, écrite avant
    /// la garde (Ruling R690) ; la page est rechargée.
    Attr {
        attr: usize,
        on_container: bool,
        raced: bool,
        stored: bool,
    },
}

/// Les attributs posés par `Op::Attr` : du type du registre, ou non (un objet, une liste,
/// `null`, un nombre pour une chaîne).
fn attrs() -> Vec<(&'static str, Any)> {
    let object = Any::Map(std::sync::Arc::new(std::collections::HashMap::from([(
        "toString".to_string(),
        Any::Number(yrs::any::Number::Int(1)),
    )])));
    vec![
        ("backgroundColor", Any::from("red")),
        ("backgroundColor", object.clone()),
        ("textAlignment", Any::Null),
        ("textAlignment", Any::from("center")),
        ("textColor", Any::Array(vec![Any::from("a")].into())),
        ("id", Any::Number(yrs::any::Number::Int(7))),
        ("id", object.clone()),
        ("mienne", object),
        ("mienne", Any::from("x")),
    ]
}

/// Ce qu'écrit un client de passage, qui a reçu la page : `key = value` sur le premier bloc ou
/// sur son contenu (vide s'il n'y est plus).
fn attribute(doc: &Doc, on_container: bool, key: &str, value: Any) -> Vec<u8> {
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = doc.transact_mut();
    if let Some(XmlOut::Element(group)) = fragment.get(&txn, 0)
        && let Some(XmlOut::Element(container)) = group.get(&txn, 0)
    {
        if on_container {
            container.insert_attribute(&mut txn, key, value);
        } else if let Some(XmlOut::Element(content)) = container.get(&txn, 0) {
            content.insert_attribute(&mut txn, key, value);
        }
    }
    txn.encode_update_v1()
}

/// Aucun élément de la page ne porte un attribut qu'aucun client du schéma n'écrit : un objet,
/// une liste, des octets, `null` (les seules valeurs fautives que ces tests posent).
fn assert_attrs_are_sound(page: &PageDoc, what: &str) {
    let doc = page.yrs_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    for node in fragment.successors(&txn) {
        let XmlOut::Element(element) = node else {
            continue;
        };
        for (key, value) in element.attributes(&txn) {
            assert!(
                !matches!(
                    value,
                    Out::Any(Any::Map(_) | Any::Array(_) | Any::Buffer(_) | Any::Null)
                ),
                "{what} : « {} » porte {key} = {value:?}",
                element.tag()
            );
        }
    }
}

/// L'élément d'une mise à jour faite à la main : le client, l'horloge, vrai pour la ramener sous
/// le plancher, l'élément (parmi `ITEMS`) et sa place (le genre, puis le type ou l'élément visé).
type HostileItem = (usize, u32, bool, usize, (usize, usize));

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
        (0..2usize, any::<usize>(), "[a-z]{1,3}").prop_map(|(who, at, typed)| Op::Move {
            who,
            at,
            typed
        }),
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
        (
            0..attrs().len(),
            any::<bool>(),
            any::<bool>(),
            any::<bool>()
        )
            .prop_map(|(attr, on_container, raced, stored)| Op::Attr {
                attr,
                on_container,
                raced,
                stored,
            }),
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
    /// la page vivante, quelles que soient les mises à jour faites à la main qui s'y mêlent ; des
    /// déplacements faits sans avoir vu la dernière frappe y mêlent des reports (R536).
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
                Op::Move { who, at, typed } => {
                    let typing = honest(&clients[1 - *who], |txn, t| {
                        let at = bounded(*at, t.len(txn) as usize);
                        t.insert(txn, at, typed);
                    });
                    served.receive(&typing);
                    served.assert_reload_is_live(&format!("étape {step} ({op:?}), la frappe"));
                    moved(&clients[*who])
                }
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
                Op::Attr { attr, on_container, raced, stored } => {
                    let (key, value) = attrs().swap_remove(*attr);
                    // Deux clients de passage partis du même état : le second ne voit pas ce
                    // que le premier écrit.
                    let passing = [
                        client(1_000 + 2 * step as u64, &served),
                        client(1_001 + 2 * step as u64, &served),
                    ];
                    if *raced {
                        let honest = if key == "id" { "autre" } else { "blue" };
                        let first = attribute(&passing[0], *on_container, key, Any::from(honest));
                        served.receive(&first);
                        served.assert_reload_is_live(&format!("étape {step} ({op:?}), la course"));
                    }
                    let update = attribute(&passing[1], *on_container, key, value);
                    if *stored {
                        served.journal.push(update);
                        served.page = served.reloaded();
                        vec![0, 0]
                    } else {
                        update
                    }
                }
            };
            served.receive(&update);
            served.assert_reload_is_live(&format!("étape {step} ({op:?})"));
            assert_attrs_are_sound(&served.page, &format!("étape {step} ({op:?})"));
        }
    }
}

/// L'affirmation d'une limite connue, partagée avec les tests de `mementum-sync`. Le module est
/// déclaré ici, au niveau du fichier, et pas dans `known_limits` : un `#[path]` dans un module en
/// ligne se lit depuis `tests/known_limits/`, un dossier qui n'existe pas, que Windows traverse et
/// que Linux refuse.
#[path = "../../mementum-sync/tests/common/limit.rs"]
mod limit;

/// Les limites connues (Ruling R340) : ce que le test dit voulu n'est pas tenu, et il l'affirme.
/// Le jour où la perte est levée, il échoue : le sortir de ce module, et lui faire affirmer la page
/// voulue.
mod known_limits {
    use super::*;

    use super::limit::known_limit;

    /// L'alignement du premier paragraphe, tel que la page le lit.
    fn alignment(page: &PageDoc) -> Option<String> {
        page.read_blocks()[0]
            .props
            .get("textAlignment")
            .map(|v| format!("{v:?}"))
    }

    /// Ruling R539 (mineur de la re-relecture du tour 7) : une entrée du journal écrite avant
    /// R496, qui récrit un attribut d'un élément (une entrée de map placée par son origine, la
    /// valeur d'avant) et pose aussi une chaîne en tête de la racine. À la relecture, R498 supprime
    /// le contenu sans type que l'entrée a intégré hors d'un texte : la chaîne, et avec elle la
    /// valeur neuve de l'attribut, une valeur `Any` dont le parent est l'élément.
    ///
    /// Voulu : la chaîne seule part, l'alignement reste « center ». Aujourd'hui : il part aussi.
    /// Seule une entrée hostile d'avant le tour 7 le permet : la garde refuse la chaîne d'un client.
    #[test]
    fn a_stored_untyped_child_takes_an_attribute_overwrite_with_it() {
        let mut served = Served::new();
        let olga = client(10, &served);
        assert!(served.receive(&honest(&olga, |txn, t| t.insert(txn, 0, "abc"))));
        let first_paragraph = |doc: &Doc| {
            let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
            let txn = doc.transact();
            let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
                panic!("la racine");
            };
            let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
                panic!("le bloc");
            };
            let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
                panic!("le paragraphe");
            };
            paragraph
        };
        let aligned = |doc: &Doc, value: &str| {
            let paragraph = first_paragraph(doc);
            let mut txn = doc.transact_mut();
            paragraph.insert_attribute(&mut txn, "textAlignment", value.to_string());
            txn.encode_update_v1()
        };
        catch_up(&olga, &served);
        assert!(served.receive(&aligned(&olga, "right")));
        let str_of = |v: &str| Some(format!("{:?}", mementum_md::PropValue::Str(v.to_string())));
        assert_eq!(alignment(&served.page), str_of("right"), "avant l'entrée");
        let hostile = client(4242, &served);
        let overwrite = aligned(&hostile, "center");
        // Seule, la valeur neuve reste : R498 ne joue que si une liste ne se lit plus en entier.
        let alone = PageDoc::from_updates(
            served
                .journal
                .iter()
                .map(Vec::as_slice)
                .chain([overwrite.as_slice()]),
        )
        .unwrap();
        assert_eq!(alignment(&alone), str_of("center"), "la valeur neuve seule");
        let entry = yrs::merge_updates_v1([
            overwrite.as_slice(),
            hand_made(Some((4242, 1, Place::Root, Item::Text("x"))), &[]).as_slice(),
        ])
        .unwrap();
        served.journal.push(entry);
        let reloaded = served.reloaded();
        assert_eq!(texts(&reloaded), ["abc"], "la chaîne partie");
        let seen = alignment(&reloaded);
        let wanted = str_of("center");
        let unmet: Vec<String> = (seen != wanted)
            .then(|| format!("alignement relu : {seen:?}"))
            .into_iter()
            .collect();
        known_limit(
            "R539 : la relecture d'une entrée d'avant R496 supprime toute valeur sans type \
             intégrée hors d'un texte, dont la valeur neuve d'un attribut placée par son origine ; \
             le lever demande de ne juger que les enfants d'une liste (pas les entrées de map)",
            &unmet,
            &["alignement relu : None".to_string()],
        );
    }
}
