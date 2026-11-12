//! Mises à jour Yjs échangées avec les clients : diff d'après un vecteur
//! d'état, et entrées illisibles.

use mementum_crdt::{CrdtError, PageDoc};
use mementum_md::{block_text, parse};

#[test]
fn diff_brings_a_peer_up_to_date() {
    let (page, created) = PageDoc::new_page();
    let peer = PageDoc::from_updates([created.as_slice()]).unwrap();
    page.write_blocks(&parse("Un\n\nDeux\n").unwrap()).unwrap();
    let id = page.read_blocks()[0].id.clone();
    page.replace_text(&id, 0, 2, "Premier").unwrap();

    let diff = page.encode_diff(&peer.state_vector()).unwrap();
    peer.apply_update(&diff).unwrap();
    assert_eq!(peer.read_blocks(), page.read_blocks());
    assert_eq!(peer.revision(), page.revision());
    assert_eq!(peer.state_vector(), page.state_vector());
}

#[test]
fn unreadable_input_is_a_decode_error() {
    let (page, _) = PageDoc::new_page();
    let before = page.read_blocks();
    assert!(matches!(
        page.apply_update(&[0xff, 0xff, 0xff]),
        Err(CrdtError::Decode(_))
    ));
    assert!(matches!(
        page.encode_diff(&[0x80; 8]),
        Err(CrdtError::Decode(_))
    ));
    assert!(matches!(
        PageDoc::from_updates([&[0xff, 0xff][..]]),
        Err(CrdtError::Decode(_))
    ));
    assert_eq!(page.read_blocks(), before);
}

/// `varUint` lib0.
fn var(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Mise à jour sans bloc dont le jeu de suppressions vise `client`, de
/// `clock` sur `len` (Ruling R25).
fn deletion(client: u64, clock: u64, len: u64) -> Vec<u8> {
    let mut update = vec![0, 1];
    var(client, &mut update);
    update.push(1);
    var(clock, &mut update);
    var(len, &mut update);
    update
}

#[test]
fn hostile_delete_set_is_a_decode_error() {
    let (page, _) = PageDoc::new_page();
    let before = page.read_blocks();
    // yrs ne les refuse qu'en paniquant en debug (`debug_assert!`, addition
    // qui déborde) : la garde les lit avant lui.
    for (what, update) in [
        ("client hors de 53 bits", deletion(1 << 60, 0, 1)),
        ("client juste au-delà", deletion(1 << 53, 0, 1)),
        ("plage qui déborde", deletion(7, u64::from(u32::MAX), 2)),
    ] {
        assert!(
            matches!(page.apply_update(&update), Err(CrdtError::Decode(_))),
            "{what}"
        );
    }
    assert_eq!(page.read_blocks(), before);
    // Le plus grand client admis passe.
    assert_eq!(page.apply_update(&deletion((1 << 53) - 1, 0, 1)), Ok(()));
}

/// Lit un entier variable comme lib0, dans le navigateur : la valeur entière,
/// sans la couper à 32 bits.
fn read_var(bytes: &[u8], at: &mut usize) -> u128 {
    let (mut value, mut shift) = (0u128, 0u32);
    loop {
        let byte = bytes[*at];
        *at += 1;
        value |= u128::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return value;
        }
        shift += 7;
    }
}

/// Le client, l'horloge et la longueur de la seule plage qu'une mise à jour
/// sans bloc supprime, lus comme un navigateur, jusqu'au dernier octet.
fn read_deletion(update: &[u8]) -> (u128, u128, u128) {
    let mut at = 0;
    assert_eq!(read_var(update, &mut at), 0, "aucun bloc");
    assert_eq!(read_var(update, &mut at), 1, "un client");
    let client = read_var(update, &mut at);
    assert_eq!(read_var(update, &mut at), 1, "une plage");
    let clock = read_var(update, &mut at);
    let len = read_var(update, &mut at);
    assert_eq!(at, update.len(), "des octets après les suppressions");
    (client, clock, len)
}

/// yrs et Yjs ne lisent pas les mêmes octets pareil : yrs laisse tomber les
/// bits d'un entier au-delà de 32, Yjs lit l'entier en entier. Une horloge
/// écrite avec 2^32 de plus désigne donc, pour la page, le texte à supprimer,
/// et pour un navigateur, une plage qu'il ne connaît pas : il la garde en
/// attente, et ne supprime rien. La page rend ce qu'elle a lu, réencodé : c'est
/// cela qui s'écrit et se relaie, jamais les octets reçus (Ruling R130).
#[test]
fn client_update_is_returned_as_the_page_read_it() {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(
        &parse(
            "Bonjour
",
        )
        .unwrap(),
    )
    .unwrap();
    let id = page.read_blocks()[0].id.clone();
    // La suppression honnête de « Bonjour », faite ailleurs : elle dit le
    // client et la plage du texte.
    let peer = PageDoc::from_updates([page.encode_state().as_slice()]).unwrap();
    let honest = peer.replace_text(&id, 0, 7, "").unwrap();
    let (client, clock, len) = read_deletion(&honest);

    let forged = deletion(client as u64, clock as u64 + (1 << 32), len as u64);
    assert_eq!(read_deletion(&forged), (client, clock + (1 << 32), len));
    let read = page.apply_client_update(&forged).expect("lisible par yrs");
    assert_eq!(
        block_text(&page.read_blocks()[0]),
        "",
        "la page a supprimé le texte"
    );
    assert_eq!(
        read_deletion(&read),
        (client, clock, len),
        "un navigateur lit ce que la page a supprimé"
    );
    assert_eq!(read, honest);
}

/// Ce qu'un client honnête envoie ressort avec le même effet : une page qui
/// applique ce que la première a rendu est la même page.
#[test]
fn honest_client_update_is_returned_with_the_same_effect() {
    let (page, created) = PageDoc::new_page();
    let client = PageDoc::from_updates([created.as_slice()]).unwrap();
    let mirror = PageDoc::from_updates([created.as_slice()]).unwrap();
    let written = client
        .write_blocks(
            &parse(
                "# Titre

Un **gras**, un [lien](https://exemple.fr).
",
            )
            .unwrap(),
        )
        .unwrap();
    let id = client.read_blocks()[1].id.clone();
    let edited = client.replace_text(&id, 0, 2, "Deux").unwrap();

    for update in [written, edited] {
        let read = page
            .apply_client_update(&update)
            .expect("mise à jour honnête");
        mirror
            .apply_update(&read)
            .expect("réencodée, elle se relit");
    }
    assert_eq!(page.read_blocks(), client.read_blocks());
    assert_eq!(mirror.read_blocks(), page.read_blocks());
    assert_eq!(mirror.revision(), page.revision());
    assert_eq!(mirror.state_vector(), page.state_vector());
}
