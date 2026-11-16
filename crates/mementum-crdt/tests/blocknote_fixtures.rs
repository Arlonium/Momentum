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
use yrs::updates::decoder::Decode;

const ALL_BLOCKS_YJS: &[u8] = include_bytes!("../../../spec/mmd/fixtures/yjs/all-blocks.bin");
const ALL_BLOCKS_MMD: &str = include_str!("../../../spec/mmd/fixtures/all-blocks.mmd");
const NON_DEFAULT_YJS: &[u8] = include_bytes!("../../../spec/mmd/fixtures/yjs/non-default.bin");
const NON_DEFAULT_MMD: &str = include_str!("../../../spec/mmd/fixtures/non-default.mmd");
const RICH_BLOCKS_YJS: &[u8] = include_bytes!("../../../spec/mmd/fixtures/yjs/rich-blocks.bin");
const RICH_BLOCKS_MMD: &str = include_str!("../../../spec/mmd/fixtures/rich-blocks.mmd");

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

/// Task X2 : les blocs riches que posent les écrans de la partie B (callouts de chaque couleur
/// et de chaque icône proposées, équations de plusieurs lignes, à ligne `$$` et vide, formules en
/// ligne dont une que `$…$` ne porte pas, signet, table des matières, lien de page, mentions,
/// fichiers de l'instance aux noms à guillemets et à accents).
#[test]
fn reads_blocknote_fixture_rich_blocks() {
    assert_eq!(mmd_of(RICH_BLOCKS_YJS), RICH_BLOCKS_MMD);
}

#[test]
fn writes_like_blocknote_all_blocks() {
    assert_writes_like_blocknote(ALL_BLOCKS_MMD, ALL_BLOCKS_YJS);
}

#[test]
fn writes_like_blocknote_non_default() {
    assert_writes_like_blocknote(NON_DEFAULT_MMD, NON_DEFAULT_YJS);
}

#[test]
fn writes_like_blocknote_rich_blocks() {
    assert_writes_like_blocknote(RICH_BLOCKS_MMD, RICH_BLOCKS_YJS);
}

/// Ce que la page donne à un client est ce qu'elle a intégré, encodé par
/// yrs (Rulings R130, R491). Pour ce qu'écrit un vrai navigateur, cela ne doit
/// rien changer : l'état complet d'une page de BlockNote, rendu par la page
/// qui l'a lu, redonne le même arbre, attribut par attribut. Renvoyé par un
/// client, cet état n'apporte rien : la page n'en écrit ni n'en relaie aucun
/// élément (avant R491, elle en réencodait tout, et le journal l'aurait gardé).
#[test]
fn blocknote_state_is_returned_with_the_same_tree() {
    let everything = |_: &str, _: &str, _: &yrs::Any| false;
    for (what, state) in [
        ("all-blocks", ALL_BLOCKS_YJS),
        ("non-default", NON_DEFAULT_YJS),
        ("rich-blocks", RICH_BLOCKS_YJS),
    ] {
        let page = PageDoc::from_updates([state]).expect("fixture lisible");
        let returned = page.encode_state();
        let theirs = dump(state, &everything);
        assert!(theirs.len() > 50, "{what} : fixture lue en entier");
        assert_eq!(dump(&returned, &everything), theirs, "{what}");
        assert_eq!(mmd_of(&returned), mmd_of(state), "{what}");
        let read = page
            .apply_client_update(state)
            .expect("un état déjà connu")
            .read;
        let read = yrs::Update::decode_v1(&read).unwrap();
        assert!(read.insertions(true).is_empty(), "{what} : rien de neuf");
        assert!(read.delete_set().is_empty(), "{what} : rien de supprimé");
    }
}
