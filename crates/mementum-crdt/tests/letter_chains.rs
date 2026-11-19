//! Des lettres envoyées une à une (Rulings R874, R876). yrs fond à la validation les éléments qui
//! se suivent. Celui de crates.io le faisait du dernier au premier, chaque élément gardant sa copie
//! de ce qui était fondu jusqu'à la fin de la suite (`ClientBlockList::squash_left`) : 80 000
//! lettres (543 Ko) lui demandaient 4,5 Gio. Le serveur compile un yrs corrigé (`vendor/yrs`), et
//! la page ne refuse plus rien pour cela : la borne des tours 8 et 9 (`MAX_COPIED_UNITS`, Rulings
//! R867, R869 à R871) est partie.
//!
//! Les formes d'ici sont celles qui trompaient son compte : ce qui sépare ou non deux lettres
//! (un élément connu, à cheval, en attente, supprimé, une entrée de map, une plage ramassée, la
//! frappe concurrente d'un autre client). Chacune reste sous un pic de mémoire que le yrs
//! d'origine dépassait dès qu'il fondait ses lettres, quel que soit le verdict de la page ; les
//! plafonds par octet reçu, à deux tailles, sont dans `squash_memory.rs`.

#[allow(dead_code)]
#[path = "../../mementum-md/tests/common/linear.rs"]
mod linear;

use mementum_crdt::{FRAGMENT, PageDoc};
use yrs::updates::decoder::Decode;
use yrs::{
    ClientID, Doc, GetString, OffsetKind, Options, ReadTxn, StateVector, Text, Transact, Update,
    XmlFragment, XmlOut, XmlTextPrelim, XmlTextRef,
};

const AUTHOR: u64 = 10;

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

fn replica(id: u64, state: &[u8]) -> Doc {
    let doc = Doc::with_options(Options {
        client_id: ClientID::new(id),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(state).unwrap())
        .unwrap();
    doc
}

fn text_of<T: ReadTxn>(txn: &T) -> XmlTextRef {
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    let Some(XmlOut::Element(group)) = fragment.get(txn, 0) else {
        panic!("le groupe");
    };
    let Some(XmlOut::Element(container)) = group.get(txn, 0) else {
        panic!("le bloc");
    };
    let Some(XmlOut::Element(paragraph)) = container.get(txn, 0) else {
        panic!("le paragraphe");
    };
    let Some(XmlOut::Text(text)) = paragraph.get(txn, 0) else {
        panic!("le texte");
    };
    text
}

/// Une page dont le premier paragraphe porte un texte vide, écrit par `AUTHOR` à son horloge 0.
fn page() -> PageDoc {
    let (page, created) = PageDoc::new_page();
    let tab = replica(AUTHOR, &created);
    let update = {
        let fragment = tab.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = tab.transact_mut();
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            panic!("le groupe");
        };
        let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
            panic!("le bloc");
        };
        let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
            panic!("le paragraphe");
        };
        paragraph.insert(&mut txn, 0, XmlTextPrelim::new(""));
        txn.encode_update_v1()
    };
    page.try_client_update(&update).expect("le texte");
    page
}

fn text(page: &PageDoc) -> String {
    let txn = page.yrs_doc().transact();
    text_of(&txn).get_string(&txn)
}

/// Les éléments d'`AUTHOR`, de `letters` lettres chacun, à partir de l'horloge 1 : chacun a pour
/// origine la dernière unité du précédent, sauf celui de rang `split`, qui commence une autre suite.
fn structs(body: &mut Vec<u8>, count: u32, letters: u32, split: Option<u32>) {
    var(body, u64::from(count));
    var(body, AUTHOR);
    var(body, 1);
    for k in 0..count {
        if k == 0 {
            body.extend_from_slice(&[0x04, 0]);
            var(body, AUTHOR);
            var(body, 0);
        } else if split == Some(k) {
            // Sans l'unité d'avant pour origine : une autre suite commence.
            body.push(0x84);
            var(body, AUTHOR);
            var(body, u64::from(k * letters - 1));
        } else {
            body.push(0x84);
            var(body, AUTHOR);
            var(body, u64::from(k * letters));
        }
        var(body, u64::from(letters));
        body.extend(std::iter::repeat_n(b'x', letters as usize));
    }
}

fn chain(count: u32, letters: u32) -> Vec<u8> {
    let mut update = vec![1];
    structs(&mut update, count, letters, None);
    update.push(0);
    update
}

/// De quoi demander 287 Mio au yrs d'origine, en lettres seules.
const MANY: u32 = 20_000;

/// Une suite de lettres est prise, si longue soit-elle, en lettres seules comme en éléments de cent
/// lettres.
#[test]
fn a_chain_of_letters_is_taken_whatever_its_length() {
    for (count, letters) in [(MANY, 1), (MANY / 10, 100)] {
        let page = page();
        accepted_within_the_peak(&page, &chain(count, letters));
        assert_eq!(text(&page).len(), (count * letters) as usize);
    }
}

/// Un seul élément, si long soit-il, n'est recopié par personne.
#[test]
fn one_long_item_copies_nothing() {
    let page = page();
    page.try_client_update(&chain(1, 200_000)).expect("accepté");
    assert_eq!(text(&page).len(), 200_000);
}

/// Une lettre qui ne prolonge pas la précédente commence une autre suite.
#[test]
fn a_letter_that_does_not_follow_the_one_before_starts_another_run() {
    let half = MANY / 2;
    let mut update = vec![1];
    structs(&mut update, 2 * half, 1, Some(half));
    update.push(0);
    let page = page();
    accepted_within_the_peak(&page, &update);
    assert_eq!(text(&page).len(), 2 * half as usize);
}

/// Un élément posé entre deux lettres les sépare pour de bon : yrs ne les fond pas. C'est ce que
/// laisse derrière elle une mise en forme faite hors ligne sur des lettres qu'on vient de taper.
#[test]
fn letters_kept_apart_by_what_is_written_between_them_are_not_copied() {
    let count = MANY;
    let mut update = vec![2];
    var(&mut update, u64::from(count - 1));
    var(&mut update, 20);
    var(&mut update, 0);
    for k in 1..count {
        update.push(0xC4);
        var(&mut update, AUTHOR);
        var(&mut update, u64::from(k));
        var(&mut update, AUTHOR);
        var(&mut update, u64::from(k + 1));
        update.extend_from_slice(&[1, b'y']);
    }
    structs(&mut update, count, 1, None);
    update.push(0);
    let page = page();
    page.try_client_update(&update).expect("accepté");
    assert_eq!(text(&page).len(), 2 * count as usize - 1);
}

/// Un voisin seulement nommé ne sépare rien : posé après la suite, il laisse les lettres se fondre.
/// (D'un client plus grand : c'est la frappe concurrente de quelqu'un qui n'a pas vu la suite.)
#[test]
fn an_item_that_names_one_neighbour_keeps_nothing_apart() {
    let count = MANY;
    let mut update = vec![2];
    var(&mut update, u64::from(count - 1));
    var(&mut update, 900);
    var(&mut update, 0);
    for k in 1..count {
        update.push(0x84);
        var(&mut update, AUTHOR);
        var(&mut update, u64::from(k));
        update.extend_from_slice(&[1, b'y']);
    }
    structs(&mut update, count, 1, None);
    update.push(0);
    let page = page();
    accepted_within_the_peak(&page, &update);
    assert_eq!(text(&page).len(), 2 * count as usize - 1);
}

/// Une lettre effacée dans la même mise à jour ne se fond pas avec ses voisines (le ramasse-miettes
/// retire son texte avant) : un onglet qui garde de quoi annuler écrit de telles suites.
#[test]
fn letters_deleted_one_in_two_are_not_copied() {
    // Une lettre sur deux, ou la dernière de chaque élément de deux lettres.
    for letters in [1u32, 2] {
        let count = MANY / letters;
        let mut update = vec![1];
        structs(&mut update, count, letters, None);
        let ranges: Vec<u32> = (0..count)
            .step_by(if letters == 1 { 2 } else { 1 })
            .map(|k| (k + 1) * letters)
            .collect();
        var(&mut update, 1);
        var(&mut update, AUTHOR);
        var(&mut update, ranges.len() as u64);
        for clock in &ranges {
            var(&mut update, u64::from(*clock));
            var(&mut update, 1);
        }
        let page = page();
        page.try_client_update(&update).expect("accepté");
        assert_eq!(text(&page).len(), (count * letters) as usize - ranges.len());
    }
}

/// Ce que le journal porte déjà se relit : une page écrite ne cesse pas de se charger.
#[test]
fn what_the_journal_already_holds_is_read_again() {
    let (written, created) = PageDoc::new_page();
    let tab = replica(AUTHOR, &created);
    let first = {
        let fragment = tab.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = tab.transact_mut();
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            panic!("le groupe");
        };
        let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
            panic!("le bloc");
        };
        let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
            panic!("le paragraphe");
        };
        paragraph.insert(&mut txn, 0, XmlTextPrelim::new(""));
        txn.encode_update_v1()
    };
    drop(written);
    let count = MANY;
    let page = PageDoc::from_updates([
        created.as_slice(),
        first.as_slice(),
        chain(count, 1).as_slice(),
    ])
    .expect("la page se charge");
    assert_eq!(text(&page).len(), count as usize);
}

/// Un long texte tapé lettre à lettre, puis l'étape 2 d'un onglet resté hors ligne : une frappe par
/// message, et une étape 2 d'un seul élément.
#[test]
fn a_long_text_typed_letter_by_letter_passes_online_and_offline() {
    let online = page();
    let offline = page();
    let tab = replica(77, &online.encode_state());
    let seen = StateVector::decode_v1(&online.state_vector()).unwrap();
    let typed = 20_000u32;
    for at in 0..typed {
        let update = {
            let mut txn = tab.transact_mut();
            let text = text_of(&txn);
            text.insert(&mut txn, at, "a");
            txn.encode_update_v1()
        };
        online.try_client_update(&update).expect("une frappe");
    }
    assert_eq!(text(&online).len(), typed as usize);
    let step2 = tab.transact().encode_diff_v1(&seen);
    let replayed = replica(78, &offline.encode_state());
    replayed
        .transact_mut()
        .apply_update(Update::decode_v1(&step2).unwrap())
        .unwrap();
    let again = replayed
        .transact()
        .encode_diff_v1(&StateVector::decode_v1(&offline.state_vector()).unwrap());
    offline.try_client_update(&again).expect("l'étape 2");
    assert_eq!(text(&offline).len(), typed as usize);
}

// Ce qui trompait le compte de la borne (Rulings R867, R869 à R871). Chaque forme porte assez de
// lettres pour que le yrs d'origine demande 287 Mio dès qu'il les fond toutes : le plafond de
// mémoire tient désormais que la page les accepte ou les refuse pour une autre raison.

type Id = (u64, u32);

/// Les lettres d'une forme.
const LETTERS: u32 = 20_000;
/// Ce qu'une mise à jour peut demander, acceptée ou refusée.
const PEAK: usize = 64 << 20;

/// Un élément de `letters` lettres, entre `origin` et `right`.
fn element(origin: Id, right: Option<Id>, letters: u32) -> Vec<u8> {
    let mut out = vec![if right.is_some() { 0xC4 } else { 0x84 }];
    for id in std::iter::once(origin).chain(right) {
        var(&mut out, id.0);
        var(&mut out, u64::from(id.1));
    }
    var(&mut out, u64::from(letters));
    out.extend(std::iter::repeat_n(b'x', letters as usize));
    out
}

/// Un élément de `letters` lettres, en tête du texte d'`AUTHOR`.
fn head(letters: u32) -> Vec<u8> {
    let mut out = vec![0x04, 0];
    var(&mut out, AUTHOR);
    var(&mut out, 0);
    var(&mut out, u64::from(letters));
    out.extend(std::iter::repeat_n(b'x', letters as usize));
    out
}

/// `count` éléments d'`AUTHOR` à partir de l'horloge 1, de `letters` lettres, chacun à la suite du
/// précédent.
fn links(count: u32, letters: u32) -> Vec<Vec<u8>> {
    (0..count)
        .map(|k| match k {
            0 => head(letters),
            _ => element((AUTHOR, k * letters), None, letters),
        })
        .collect()
}

/// Les deux unités de la jointure `k` (de 1 à `count - 1`) de [`links`].
fn seam(k: u32, letters: u32) -> (Id, Id) {
    ((AUTHOR, k * letters), (AUTHOR, k * letters + 1))
}

/// Une mise à jour : par client, sa première horloge et ses éléments ; puis ce qu'elle supprime.
fn written(clients: &[(u64, u32, Vec<Vec<u8>>)], deleted: &[(u64, Vec<(u32, u32)>)]) -> Vec<u8> {
    let mut out = Vec::new();
    var(&mut out, clients.len() as u64);
    for (client, clock, items) in clients {
        var(&mut out, items.len() as u64);
        var(&mut out, *client);
        var(&mut out, u64::from(*clock));
        out.extend(items.iter().flatten());
    }
    var(&mut out, deleted.len() as u64);
    for (client, ranges) in deleted {
        var(&mut out, *client);
        var(&mut out, ranges.len() as u64);
        for (clock, len) in ranges {
            var(&mut out, u64::from(*clock));
            var(&mut out, u64::from(*len));
        }
    }
    out
}

/// La mise à jour, et le pic de mémoire qu'elle a demandé.
fn judged(page: &PageDoc, update: &[u8]) -> (Result<(), bool>, usize) {
    let answer = std::cell::Cell::new(Ok(()));
    let cost = linear::once(|| {
        answer.set(
            page.try_client_update(update)
                .map(|_| ())
                .map_err(|r| r.intact),
        );
    });
    (answer.get(), cost.peak)
}

/// Ce que la borne refusait. Le verdict n'est plus affirmé : acceptée, ou refusée pour autre chose.
fn within_the_peak(page: &PageDoc, update: &[u8]) {
    let (_, peak) = judged(page, update);
    assert!(peak < PEAK, "pic de {peak} octets");
}

fn accepted_within_the_peak(page: &PageDoc, update: &[u8]) {
    let (answer, peak) = judged(page, update);
    assert!(peak < PEAK, "pic de {peak} octets");
    assert_eq!(answer, Ok(()), "acceptée");
}

/// Un élément que la page connaît déjà, yrs l'ignore : il ne sépare rien. Constat C1 de la
/// re-relecture 10 : 80 000 lettres passaient ainsi la borne, pour 4,6 Gio. En lettres seules comme
/// en éléments de plusieurs lettres.
#[test]
fn items_the_page_already_has_separate_nothing() {
    for (count, letters) in [(LETTERS, 1), (LETTERS / 3, 10)] {
        let page = page();
        let known = written(&[(30, 0, vec![head(count)])], &[]);
        page.try_client_update(&known).expect("le client 30 écrit");
        let shields = (1..count)
            .map(|k| {
                let (a, b) = seam(k, letters);
                element(a, Some(b), 1)
            })
            .collect();
        let update = written(&[(30, 0, shields), (AUTHOR, 1, links(count, letters))], &[]);
        within_the_peak(&page, &update);
    }
}

/// Un élément à cheval sur ce que la page connaît : seule sa part neuve est intégrée, à la suite de
/// sa part connue, loin des deux lettres qu'il nomme.
#[test]
fn items_astride_what_the_page_knows_separate_nothing() {
    let count = MANY / 2;
    let page = page();
    let clients: Vec<(u64, u32, Vec<Vec<u8>>)> = (1..count)
        .map(|k| (u64::from(1_000 + k), 0, vec![head(1)]))
        .collect();
    page.try_client_update(&written(&clients, &[]))
        .expect("les clients écrivent");
    let mut sent: Vec<(u64, u32, Vec<Vec<u8>>)> = (1..count)
        .map(|k| {
            let (a, b) = seam(k, 1);
            (u64::from(1_000 + k), 0, vec![element(a, Some(b), 2)])
        })
        .collect();
    sent.push((AUTHOR, 1, links(count, 1)));
    within_the_peak(&page, &written(&sent, &[]));
}

/// La borne juge la mise à jour telle que yrs la lira : des origines droites que la règle V retire
/// font une seule suite de lettres qui, reçues, n'en faisaient pas.
#[test]
fn letters_whose_right_origins_the_rules_remove_are_one_chain() {
    let page = page();
    for update in [
        written(&[(AUTHOR, 1, vec![head(4)])], &[]),
        written(
            &[(40, 0, vec![element((AUTHOR, 2), Some((AUTHOR, 3)), 2)])],
            &[],
        ),
        written(&[(41, 0, vec![element((40, 0), Some((40, 1)), 1)])], &[]),
    ] {
        page.try_client_update(&update).expect("le décor");
    }
    let rights = [(40, 1), (41, 0)];
    let items: Vec<Vec<u8>> = (0..LETTERS)
        .map(|k| {
            let origin = if k == 0 { (AUTHOR, 4) } else { (77, k - 1) };
            element(origin, Some(rights[k as usize % 2]), 1)
        })
        .collect();
    within_the_peak(&page, &written(&[(77, 0, items)], &[]));
}

/// Un élément supprimé par la mise à jour qui le pose reste à sa place, entre ses deux voisins.
#[test]
fn an_item_deleted_by_its_own_update_still_keeps_two_letters_apart() {
    let shields: Vec<Vec<u8>> = (1..LETTERS)
        .map(|k| {
            let (a, b) = seam(k, 1);
            element(a, Some(b), 1)
        })
        .collect();
    let update = written(
        &[(30, 0, shields), (AUTHOR, 1, links(LETTERS, 1))],
        &[(30, vec![(0, LETTERS - 1)])],
    );
    let page = page();
    accepted_within_the_peak(&page, &update);
    assert_eq!(text(&page).len(), LETTERS as usize);
}

/// Des lettres supprimées par la mise à jour qui les pose : le ramasse-miettes retire leur texte
/// avant que yrs fonde.
#[test]
fn letters_deleted_by_their_own_update_cost_no_copy() {
    let ranges: Vec<(u32, u32)> = (1..=LETTERS).step_by(2).map(|clock| (clock, 1)).collect();
    let update = written(&[(AUTHOR, 1, links(LETTERS, 1))], &[(AUTHOR, ranges)]);
    let page = page();
    accepted_within_the_peak(&page, &update);
    assert_eq!(text(&page).len(), LETTERS as usize / 2);
}

/// Une plage ramassée ne se pose nulle part.
#[test]
fn collected_ranges_separate_nothing() {
    let ranges: Vec<Vec<u8>> = (1..LETTERS).map(|_| vec![0, 1]).collect();
    let update = written(&[(30, 0, ranges), (AUTHOR, 1, links(LETTERS, 1))], &[]);
    within_the_peak(&page(), &update);
}

/// Un trou entre deux lettres : rien ne se fond par-dessus, et un client n'en laisse pas.
#[test]
fn a_hole_between_letters_is_refused() {
    let mut items = links(LETTERS / 2, 1);
    items.push(vec![10, 1]);
    items.extend((0..LETTERS / 2).map(|k| element((AUTHOR, LETTERS / 2 + 1 + k), None, 1)));
    within_the_peak(&page(), &written(&[(AUTHOR, 1, items)], &[]));
}

/// Une entrée de map n'est pas un élément de la liste : ni posée entre les deux lettres, ni voisine
/// de la seconde.
#[test]
fn map_entries_separate_nothing() {
    for exact in [true, false] {
        let entries: Vec<Vec<u8>> = (1..LETTERS)
            .map(|k| {
                let (a, b) = seam(k, 1);
                let mut out = vec![if exact { 0xE8 } else { 0xA8 }];
                for id in std::iter::once(a).chain(exact.then_some(b)) {
                    var(&mut out, id.0);
                    var(&mut out, u64::from(id.1));
                }
                out.extend_from_slice(&[1, 119, 1, b'v']);
                out
            })
            .collect();
        let update = written(&[(5, 0, entries), (AUTHOR, 1, links(LETTERS, 1))], &[]);
        within_the_peak(&page(), &update);
    }
}

/// Ce que yrs laisse en attente n'est pas posé (Ruling R871). Il fond les lettres qu'il a intégrées
/// avant que la mise à jour soit dite incomplète : le yrs d'origine y demandait 291 Mio pour 20 000
/// lettres, payés avant le refus.
#[test]
fn items_yrs_leaves_waiting_separate_nothing() {
    let unknown = (99, 1_000);
    let mut no_parent = vec![0x04, 0];
    var(&mut no_parent, unknown.0);
    var(&mut no_parent, u64::from(unknown.1));
    no_parent.extend_from_slice(&[1, b'x']);
    let waiting = [
        (element(unknown, None, 1), None),
        (element((AUTHOR, 1), Some(unknown), 1), None),
        (no_parent, None),
        (element((5, 0), None, 1), None),
        (element((5, LETTERS + 500), None, 1), None),
        (element((6, 0), None, 1), Some(element((5, 0), None, 1))),
        (
            element((6, 500), None, 1),
            Some(element((AUTHOR, 1), None, 1)),
        ),
        (
            element((6, 0), Some(unknown), 1),
            Some(element((AUTHOR, 1), None, 1)),
        ),
    ];
    for exact in [true, false] {
        for (first, other) in &waiting {
            let mut shields = vec![first.clone()];
            shields.extend((1..LETTERS).map(|k| {
                let (a, b) = seam(k, 1);
                element(a, exact.then_some(b), 1)
            }));
            let mut clients = vec![(5, 0, shields), (AUTHOR, 1, links(LETTERS, 1))];
            clients.extend(other.iter().map(|item| (6, 0, vec![item.clone()])));
            within_the_peak(&page(), &written(&clients, &[]));
        }
    }
}

/// Ce dont un séparateur dépend peut venir plus loin dans le message.
#[test]
fn a_separator_may_depend_on_what_comes_later_in_the_update() {
    let mut shields = vec![element((AUTHOR, LETTERS), None, 1)];
    shields.extend((1..LETTERS).map(|k| {
        let (a, b) = seam(k, 1);
        element(a, Some(b), 1)
    }));
    let update = written(&[(5, 0, shields), (AUTHOR, 1, links(LETTERS, 1))], &[]);
    accepted_within_the_peak(&page(), &update);
}

/// yrs ignore ce que la page connaît déjà : ce dont cela dépendait n'est attendu de personne.
#[test]
fn what_the_page_already_has_waits_for_nothing() {
    let page = page();
    let known = written(&[(5, 0, vec![head(1)])], &[]);
    page.try_client_update(&known).expect("le client 5 écrit");
    let mut shields = vec![element((99, 1_000), None, 1)];
    shields.extend((1..LETTERS).map(|k| {
        let (a, b) = seam(k, 1);
        element(a, Some(b), 1)
    }));
    let update = written(&[(5, 0, shields), (AUTHOR, 1, links(LETTERS, 1))], &[]);
    accepted_within_the_peak(&page, &update);
}

/// Deux clients peuvent s'attendre tour à tour : chacun n'avance que jusqu'à ce qu'on attend de lui.
#[test]
fn two_clients_may_wait_for_each_other_in_turn() {
    let mut shields = vec![
        element((AUTHOR, LETTERS), None, 1),
        element((6, 0), None, 1),
    ];
    shields.extend((1..LETTERS).map(|k| {
        let (a, b) = seam(k, 1);
        element(a, Some(b), 1)
    }));
    let other = vec![element((5, 0), None, 1), element((5, 1), None, 1)];
    let update = written(
        &[
            (5, 0, shields),
            (6, 0, other),
            (AUTHOR, 1, links(LETTERS, 1)),
        ],
        &[],
    );
    accepted_within_the_peak(&page(), &update);
}

/// Toute une suite supprimée par la mise à jour qui la pose.
#[test]
fn a_chain_deleted_whole_by_its_own_update_costs_no_copy() {
    let update = written(
        &[(AUTHOR, 1, links(LETTERS, 1))],
        &[(AUTHOR, vec![(1, LETTERS)])],
    );
    let page = page();
    accepted_within_the_peak(&page, &update);
    assert_eq!(text(&page), "");
}

/// Plusieurs clients dans un message, chacun sa suite.
#[test]
fn several_clients_in_one_message() {
    let count = MANY / 2;
    let second: Vec<Vec<u8>> = (0..count)
        .map(|k| match k {
            0 => element((AUTHOR, count), None, 1),
            _ => element((11, k - 1), None, 1),
        })
        .collect();
    let update = written(&[(AUTHOR, 1, links(count, 1)), (11, 0, second)], &[]);
    within_the_peak(&page(), &update);
}

/// Deux clients qui se relaient ne se fondent pas : yrs ne fond que les lettres d'un même client.
#[test]
fn clients_taking_turns_are_not_copied() {
    let turn = |other: u64, first: u32| -> Vec<Vec<u8>> {
        (first..LETTERS)
            .step_by(2)
            .map(|k| match k {
                0 => element((AUTHOR, 1), None, 1),
                _ => element((other, (k - 1) / 2), None, 1),
            })
            .collect()
    };
    let update = written(&[(20, 0, turn(21, 0)), (21, 0, turn(20, 1))], &[]);
    let page = page();
    page.try_client_update(&chain(1, 1))
        .expect("la première lettre");
    accepted_within_the_peak(&page, &update);
}

/// Plusieurs messages : chacun ne paie que ce qu'il fond.
#[test]
fn several_messages_cost_what_each_one_costs() {
    let page = page();
    let size = 4_096;
    let mut journal = vec![page.encode_state()];
    let cost = linear::once(|| {
        for message in 0..16 {
            let items: Vec<Vec<u8>> = (0..size)
                .map(|k| match message * size + k {
                    0 => head(1),
                    clock => element((AUTHOR, clock), None, 1),
                })
                .collect();
            let update = written(&[(AUTHOR, 1 + message * size, items)], &[]);
            let applied = page.try_client_update(&update).expect("un message");
            journal.push(applied.read);
        }
    });
    assert!(cost.peak < PEAK, "pic de {} octets", cost.peak);
    assert_eq!(text(&page).len(), 16 * size as usize);
    // Relu de son journal, chaque message reste une transaction.
    let reload = linear::once(|| {
        let again =
            PageDoc::from_updates(journal.iter().map(Vec::as_slice)).expect("le journal se relit");
        assert_eq!(text(&again).len(), 16 * size as usize);
    });
    assert!(reload.peak < PEAK, "pic de {} octets", reload.peak);
}

/// Les lettres concurrentes d'un client plus petit (Ruling R870). yrs range deux éléments de mêmes
/// voisins par leur client, le plus petit d'abord : celui-ci se pose entre la lettre et celle qui
/// la prolonge. C'est ce qu'écrivent deux personnes qui tapent à la fin d'un même paragraphe.
fn concurrent(client: u64, count: u32, right: Option<Id>) -> Vec<u8> {
    let others: Vec<Vec<u8>> = (1..count)
        .map(|k| element(seam(k, 1).0, right, 1))
        .collect();
    written(&[(client, 0, others), (AUTHOR, 1, links(count, 1))], &[])
}

#[test]
fn a_concurrent_letter_of_a_smaller_client_keeps_two_letters_apart() {
    let page = page();
    accepted_within_the_peak(&page, &concurrent(5, LETTERS, None));
    assert_eq!(text(&page).len(), 2 * LETTERS as usize - 1);
}

#[test]
fn a_concurrent_letter_of_a_larger_client_comes_after_and_separates_nothing() {
    within_the_peak(&page(), &concurrent(900, LETTERS, None));
}

#[test]
fn a_concurrent_letter_the_page_already_has_separates_nothing() {
    let page = page();
    let known = written(&[(5, 0, vec![head(LETTERS)])], &[]);
    page.try_client_update(&known).expect("le client 5 écrit");
    within_the_peak(&page, &concurrent(5, LETTERS, None));
}
