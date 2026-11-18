//! Aucune mise à jour n'est refusée pour le nombre d'unités de la page qu'elle nomme (Ruling
//! R853) : ce qu'est une unité se lit dans les suites de la garde, sans plafond.

use mementum_crdt::{FRAGMENT, PageDoc};
use yrs::updates::decoder::Decode;
use yrs::{
    Doc, GetString, OffsetKind, Options, ReadTxn, StateVector, Text, Transact, Update, XmlFragment,
    XmlOut, XmlTextPrelim, XmlTextRef,
};

type Id = (u64, u32);

const TAB: u64 = 50;

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

fn replica(id: u64, state: &[u8]) -> Doc {
    let doc = Doc::with_options(Options {
        client_id: yrs::ClientID::new(id),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(state).unwrap())
        .unwrap();
    doc
}

fn paragraph_of(txn: &impl ReadTxn) -> yrs::XmlElementRef {
    let fragment = txn.get_xml_fragment(FRAGMENT).expect("le fragment");
    let Some(XmlOut::Element(group)) = fragment.get(txn, 0) else {
        panic!("pas de groupe de blocs");
    };
    let Some(XmlOut::Element(container)) = group.get(txn, 0) else {
        panic!("pas de bloc");
    };
    let Some(XmlOut::Element(paragraph)) = container.get(txn, 0) else {
        panic!("pas de paragraphe");
    };
    paragraph
}

fn text_of(txn: &impl ReadTxn) -> XmlTextRef {
    match paragraph_of(txn).get(txn, 0) {
        Some(XmlOut::Text(text)) => text,
        _ => panic!("pas de texte"),
    }
}

/// Le texte du premier paragraphe, tel que la page le porte.
fn page_text(page: &PageDoc) -> String {
    let txn = page.yrs_doc().transact();
    text_of(&txn).get_string(&txn)
}

/// Au-delà de 16 384 unités de la page nommées par une seule mise à jour (l'ancien plafond des
/// règles G et P2).
const MANY: usize = 20_000;

/// Une mise à jour faite de `pairs.len()` lettres de `client`, chacune entre ses deux voisins.
fn letters_between(client: u64, pairs: &[(Id, Id)]) -> Vec<u8> {
    let mut u = vec![1];
    var(&mut u, pairs.len() as u64);
    var(&mut u, client);
    u.push(0);
    for (origin, right) in pairs {
        u.push(0xC4);
        for id in [origin, right] {
            var(&mut u, id.0);
            var(&mut u, u64::from(id.1));
        }
        u.extend_from_slice(&[1, b'x']);
    }
    u.push(0);
    u
}

/// `update` est accepté d'un client et relu du journal, et la page porte `expected`.
fn accepted_on_both_paths(state: &[u8], update: &[u8], expected: &str) {
    let page = PageDoc::from_updates([state]).unwrap();
    let applied = page.try_client_update(update);
    assert!(
        applied.is_ok(),
        "d'un client : {:?}",
        applied.err().map(|r| r.error)
    );
    assert_eq!(page_text(&page).len(), expected.len(), "d'un client");
    assert!(page_text(&page) == expected, "d'un client : un autre texte");
    let stored = PageDoc::from_updates([state, update]);
    let stored = stored.unwrap_or_else(|e| panic!("au journal : {e:?}"));
    assert!(
        page_text(&stored) == expected,
        "au journal : un autre texte"
    );
    let reread = PageDoc::from_updates([page.encode_state().as_slice()]).unwrap();
    assert!(
        page_text(&reread) == expected,
        "relue de son état : un autre texte"
    );
}

/// Des lettres écrites une à une par un client (chacune une suite à elle) : une mise à jour qui en
/// nomme plus de 16 384 passe, d'un client comme à la relecture du journal.
#[test]
fn an_update_naming_more_units_than_the_old_bound_passes_on_a_page_a_client_wrote() {
    let (page, created) = PageDoc::new_page();
    let tab = replica(TAB, &created);
    let first = {
        let mut txn = tab.transact_mut();
        let paragraph = paragraph_of(&txn);
        let text = paragraph.insert(&mut txn, 0, XmlTextPrelim::new(""));
        let first = txn.state_vector().get(&yrs::ClientID::new(TAB));
        for _ in 0..MANY {
            text.insert(&mut txn, 0, "a");
        }
        first
    };
    let state = tab
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    page.apply_update(&state).unwrap();
    // Les lettres sont insérées en tête : chacune a la précédente à sa droite.
    let pairs: Vec<(Id, Id)> = (0..MANY as u32 - 1)
        .rev()
        .map(|i| ((TAB, first + i + 1), (TAB, first + i)))
        .collect();
    let update = letters_between(901, &pairs);
    let expected = format!("{}a", "ax".repeat(MANY - 1));
    accepted_on_both_paths(&page.encode_state(), &update, &expected);
}

/// De même sur un texte que le serveur a écrit : ses unités ne sont connues de la garde qu'à la
/// première qui est nommée.
#[test]
fn an_update_naming_more_units_than_the_old_bound_passes_on_a_page_the_server_wrote() {
    let (page, _) = PageDoc::new_page();
    let written = "a".repeat(MANY);
    page.write_blocks(&mementum_md::parse(&format!("{written}\n")).unwrap())
        .unwrap();
    let state = page.encode_state();
    let tab = replica(TAB, &state);
    let before = tab.transact().state_vector();
    {
        let text = text_of(&tab.transact());
        let mut txn = tab.transact_mut();
        // De la fin vers le début : yrs ne reparcourt pas les lettres déjà coupées.
        for at in (1..MANY as u32).rev() {
            text.insert(&mut txn, at, "x");
        }
    }
    let update = tab.transact().encode_diff_v1(&before);
    let expected = format!("{}a", "ax".repeat(MANY - 1));
    accepted_on_both_paths(&state, &update, &expected);
}

/// Ce que la garde lit dans yrs pour une écriture du serveur peut chevaucher ce qu'elle sait déjà :
/// une mise à jour à cheval sur le plancher du client du serveur n'y pose que sa part neuve, que yrs
/// fond ensuite avec l'unité d'avant. La garde garde ce que yrs lui rend et qu'elle ne savait pas.
#[test]
fn what_yrs_merged_across_what_the_guard_knows_is_read_for_the_rest() {
    let (page, _) = PageDoc::new_page();
    let state = StateVector::decode_v1(&page.state_vector()).unwrap();
    let (server, known) = state
        .iter()
        .next()
        .map(|(client, clock)| (client.get(), *clock))
        .unwrap();
    let mut update = vec![1, 1];
    var(&mut update, server);
    var(&mut update, u64::from(known - 1));
    update.push(0x88);
    var(&mut update, server);
    var(&mut update, u64::from(known - 2));
    update.extend_from_slice(&[2, 119, 1, b'r', 119, 1, b'b']);
    update.push(0);
    page.apply_update(&update).expect("une entrée du journal");
    assert_eq!(page.misjudged_units(), None);
}

/// Une plage ramassée à cheval sur ce que la page connaît ne ramasse que sa part neuve : le reste
/// est toujours ce que le serveur y a écrit.
#[test]
fn a_collected_range_across_what_the_page_knows_collects_only_what_is_new() {
    let (page, _) = PageDoc::new_page();
    let state = StateVector::decode_v1(&page.state_vector()).unwrap();
    let (server, known) = state
        .iter()
        .next()
        .map(|(client, clock)| (client.get(), *clock))
        .unwrap();
    let mut update = vec![1, 1];
    var(&mut update, server);
    var(&mut update, u64::from(known - 1));
    update.extend_from_slice(&[0, 2, 0]);
    page.apply_update(&update).expect("une entrée du journal");
    assert_eq!(page.misjudged_units(), None);
}
