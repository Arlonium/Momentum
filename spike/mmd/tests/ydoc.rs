//! Task 4 — MMD ⇄ Yjs au format BlockNote, côté serveur (yrs).

use spike_mmd::ydoc::{read_blocks, write_blocks, FRAGMENT};
use spike_mmd::{parse, render, BlockKind, RenderOpts};
use yrs::updates::decoder::Decode;
use yrs::{Doc, OffsetKind, Options, Transact, Update};

const MMD: &str = include_str!("../../fixtures/mmd/all-blocks.mmd");
const BLOCKNOTE_YJS: &[u8] = include_bytes!("../../fixtures/yjs/all-blocks.bin");

fn new_doc() -> Doc {
    Doc::with_options(Options { offset_kind: OffsetKind::Utf16, ..Default::default() })
}

#[test]
fn reads_blocknote_fixture() {
    let doc = new_doc();
    let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut().apply_update(Update::decode_v1(BLOCKNOTE_YJS).unwrap()).unwrap();
    let blocks = read_blocks(&doc.transact(), &frag);
    assert_eq!(render(&blocks, RenderOpts { anchors: false }).markdown, MMD);
}

#[test]
fn write_then_read_is_identity() {
    let expected = parse(MMD).unwrap();
    let doc = new_doc();
    let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
    write_blocks(&mut doc.transact_mut(), &frag, &expected);
    let back = read_blocks(&doc.transact(), &frag);
    assert_eq!(back, expected);
}

#[test]
fn write_replaces_previous_content() {
    let doc = new_doc();
    let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
    write_blocks(&mut doc.transact_mut(), &frag, &parse("Ancien contenu\n").unwrap());
    let fresh = parse("Nouveau contenu\n").unwrap();
    write_blocks(&mut doc.transact_mut(), &frag, &fresh);
    assert_eq!(read_blocks(&doc.transact(), &frag), fresh);
}

#[test]
fn unknown_block_survives_yjs() {
    let doc = new_doc();
    let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
    write_blocks(&mut doc.transact_mut(), &frag, &parse(MMD).unwrap());
    let back = read_blocks(&doc.transact(), &frag);
    let u = back.last().unwrap();
    assert_eq!(u.id, "b_unknown01");
    assert_eq!(u.kind, BlockKind::Unknown { type_name: "future_widget".into(), data: "eyJ4IjoxfQ==".into() });
}
