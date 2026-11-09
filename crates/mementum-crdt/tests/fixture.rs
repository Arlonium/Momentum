//! Lecture du document Yjs exporté par BlockNote 0.57 pendant la phase 0,
//! et écriture dans le même format.

mod common;

use common::dump;
use mementum_crdt::PageDoc;
use mementum_md::{RenderOpts, render};
use yrs::Any;

const YJS: &[u8] = include_bytes!("../../../spec/mmd/fixtures/yjs/p0-all-blocks.bin");
const MMD: &str = include_str!("../../../spec/mmd/fixtures/p0-all-blocks.mmd");

#[test]
fn reads_p0_blocknote_fixture() {
    let page = PageDoc::from_updates([YJS]).expect("fixture lisible");
    let markdown = render(&page.read_blocks(), RenderOpts::default()).markdown;
    assert_eq!(markdown, MMD);
}

#[test]
fn writes_like_blocknote() {
    let blocks = PageDoc::from_updates([YJS]).unwrap().read_blocks();
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&blocks).unwrap();

    // Même arbre, mêmes attributs typés, mêmes plages de texte que BlockNote.
    // Seule différence admise : la prop `foreign` du bloc `unknown`, ajoutée
    // au registre après l'export de la fixture (Ruling R4).
    let ours = dump(&page.encode_state(), &|tag, key, value| {
        tag == "unknown" && key == "foreign" && *value == Any::Bool(false)
    });
    let blocknote = dump(YJS, &|_, _, _| false);
    assert!(blocknote.len() > 100, "fixture lue en entier");
    assert_eq!(ours, blocknote);
}
