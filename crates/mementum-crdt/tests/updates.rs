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
