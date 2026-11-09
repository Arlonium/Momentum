//! Fixtures exportées du vrai BlockNote 0.57 (Task 8, `apps/web/e2e/fidelity.spec.ts`
//! avec `EXPORT_FIXTURES=1`) : l'état Yjs d'un navigateur qui a chargé
//! `apps/web/src/editor/fixtures/<nom>.input.json`.
//! - Relu par le serveur, il donne exactement le MMD attendu, écrit à la main
//!   d'après les règles du §6.2.
//! - Ce MMD, écrit par le serveur, redonne le même arbre Yjs que BlockNote :
//!   mêmes nœuds, mêmes attributs typés (largeurs de colonnes comprises),
//!   mêmes plages de texte.

mod common;

use common::dump;
use mementum_crdt::PageDoc;
use mementum_md::{RenderOpts, parse, render};

const ALL_BLOCKS_YJS: &[u8] = include_bytes!("../../../spec/mmd/fixtures/yjs/all-blocks.bin");
const ALL_BLOCKS_MMD: &str = include_str!("../../../spec/mmd/fixtures/all-blocks.mmd");
const NON_DEFAULT_YJS: &[u8] = include_bytes!("../../../spec/mmd/fixtures/yjs/non-default.bin");
const NON_DEFAULT_MMD: &str = include_str!("../../../spec/mmd/fixtures/non-default.mmd");

/// Le MMD de la page dont `state` est l'état Yjs complet.
fn mmd_of(state: &[u8]) -> String {
    let page = PageDoc::from_updates([state]).expect("fixture lisible");
    render(&page.read_blocks(), RenderOpts::default()).markdown
}

/// Le MMD attendu, écrit par le serveur dans une page neuve, donne l'arbre
/// Yjs de BlockNote. Les ids des blocs diffèrent (le MMD n'en porte pas,
/// sauf celui du bloc `unknown`) : ils sont omis des deux côtés. Les autres
/// `id` (celui d'une mention) sont comparés.
fn assert_writes_like_blocknote(mmd: &str, blocknote: &[u8]) {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(mmd).expect("MMD attendu lisible"))
        .expect("écriture");
    let no_ids = |tag: &str, key: &str, _: &yrs::Any| {
        key == "id" && matches!(tag, "blockContainer" | "columnList" | "column")
    };
    let theirs = dump(blocknote, &no_ids);
    assert!(theirs.len() > 50, "fixture lue en entier");
    assert_eq!(dump(&page.encode_state(), &no_ids), theirs);
}

#[test]
fn reads_blocknote_fixture_all_blocks() {
    assert_eq!(mmd_of(ALL_BLOCKS_YJS), ALL_BLOCKS_MMD);
}

#[test]
fn reads_blocknote_fixture_non_default() {
    assert_eq!(mmd_of(NON_DEFAULT_YJS), NON_DEFAULT_MMD);
}

#[test]
fn writes_like_blocknote_all_blocks() {
    assert_writes_like_blocknote(ALL_BLOCKS_MMD, ALL_BLOCKS_YJS);
}

#[test]
fn writes_like_blocknote_non_default() {
    assert_writes_like_blocknote(NON_DEFAULT_MMD, NON_DEFAULT_YJS);
}
