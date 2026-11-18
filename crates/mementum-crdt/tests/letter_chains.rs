//! Des lettres envoyées une à une (Ruling R867). yrs fond à la validation les éléments qui se suivent,
//! du dernier au premier, et chaque élément garde sa copie de ce qui a été fondu jusqu'à la fin de
//! la suite (`ClientBlockList::squash_left`) : 80 000 lettres (543 Ko) lui demandent 4,5 Gio. Un
//! client n'en envoie pas : Yjs fond ses lettres avant de les écrire. La page refuse d'un client ce
//! qui ferait recopier trop de texte, et relit tout ce que son journal porte déjà.

use mementum_crdt::{FRAGMENT, MAX_COPIED_UNITS, PageDoc};
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

/// Ce que yrs recopie pour `count` éléments de `letters` lettres en une suite : le deuxième une fois,
/// le troisième deux fois…
fn copied(count: u64, letters: u64) -> u64 {
    letters * count * (count - 1) / 2
}

/// Le plus grand nombre d'éléments de `letters` lettres que la borne admet en une suite.
fn most(letters: u64) -> u32 {
    let mut count = 2;
    while copied(count + 1, letters) <= MAX_COPIED_UNITS {
        count += 1;
    }
    count as u32
}

fn refused_before_being_applied(page: &PageDoc, update: &[u8]) {
    let before = page.revision();
    let Err(refused) = page.try_client_update(update) else {
        panic!("acceptée");
    };
    assert!(refused.intact, "{}", refused.error);
    assert_eq!(page.revision(), before);
}

#[test]
fn a_chain_of_letters_is_taken_up_to_the_bound_and_refused_beyond() {
    let count = most(1);
    let page_one = page();
    page_one
        .try_client_update(&chain(count, 1))
        .expect("la borne admet ce qu'elle dit");
    assert_eq!(text(&page_one).len(), count as usize);
    refused_before_being_applied(&page(), &chain(count + 1, 1));
}

/// Le coût est celui du texte recopié, pas du nombre d'éléments.
#[test]
fn the_bound_counts_the_letters_that_are_copied() {
    let count = most(100);
    assert!(count < most(1) / 9);
    page()
        .try_client_update(&chain(count, 100))
        .expect("la borne admet ce qu'elle dit");
    refused_before_being_applied(&page(), &chain(count + 1, 100));
}

/// Un seul élément, si long soit-il, n'est recopié par personne.
#[test]
fn one_long_item_copies_nothing() {
    let page = page();
    page.try_client_update(&chain(1, 200_000)).expect("accepté");
    assert_eq!(text(&page).len(), 200_000);
}

/// La borne vaut pour toute la mise à jour : plusieurs suites ne la contournent pas.
#[test]
fn the_bound_holds_for_the_whole_update() {
    let half = most(1) * 3 / 4;
    let mut update = vec![1];
    structs(&mut update, 2 * half, 1, Some(half));
    update.push(0);
    refused_before_being_applied(&page(), &update);
}

/// Une lettre qui ne prolonge pas la précédente commence une autre suite : deux suites coûtent
/// moins qu'une seule de leur longueur réunie.
#[test]
fn a_letter_that_does_not_follow_the_one_before_starts_another_run() {
    let half = most(1) * 2 / 3;
    assert!(copied(u64::from(2 * half), 1) > MAX_COPIED_UNITS);
    assert!(2 * copied(u64::from(half), 1) <= MAX_COPIED_UNITS);
    let mut update = vec![1];
    structs(&mut update, 2 * half, 1, Some(half));
    update.push(0);
    page().try_client_update(&update).expect("accepté");
}

/// Un élément posé entre deux lettres les sépare pour de bon : yrs ne les fond pas. C'est ce que
/// laisse derrière elle une mise en forme faite hors ligne sur des lettres qu'on vient de taper.
#[test]
fn letters_kept_apart_by_what_is_written_between_them_are_not_copied() {
    let count = most(1) + 1_000;
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
#[test]
fn an_item_that_names_one_neighbour_keeps_nothing_apart() {
    let count = most(1) + 1_000;
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
    refused_before_being_applied(&page(), &update);
}

/// Une lettre effacée dans la même mise à jour ne se fond pas avec ses voisines (le ramasse-miettes
/// retire son texte avant) : un onglet qui garde de quoi annuler écrit de telles suites.
#[test]
fn letters_deleted_one_in_two_are_not_copied() {
    // Une lettre sur deux, ou la dernière de chaque élément de deux lettres.
    for letters in [1u32, 2] {
        let count = most(u64::from(letters)) + 1_000;
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
    let count = most(1) + 1;
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
