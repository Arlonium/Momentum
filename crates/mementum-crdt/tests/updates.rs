//! Mises à jour Yjs échangées avec les clients : diff d'après un vecteur
//! d'état, et entrées illisibles.

use mementum_crdt::{CrdtError, PageDoc};
use mementum_md::parse;

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
