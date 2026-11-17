//! Un élément à cheval sur ce que la page connaît, tel que le vrai Yjs l'écrit (Ruling R784 ;
//! constat C1 de la re-relecture 4 de la garde des attributs).
//!
//! L'onglet écrit ; la mise à jour part sur son ancienne connexion et y reste. Il écrit encore,
//! hors ligne, et Yjs fond les deux écritures en un élément. Sa nouvelle connexion reçoit l'étape
//! 1 du serveur, qui ne connaît pas encore la première écriture ; elle arrive ; puis l'étape 2 de
//! l'onglet, dont un élément part sous ce que le serveur connaît maintenant.
//!
//! Les scènes sont celles de `apps/web/src/editor/astride.test.ts`, écrites par le vrai Yjs dans
//! `tests/fixtures/yjs_updates/astride.json` : la première écriture (`first`), l'étape 2 à cheval
//! (`step2`), ce que Yjs écrirait pour un destinataire qui connaît la première (`cut`), et l'état
//! qu'un vrai Yjs qui connaît la première obtient en appliquant l'étape 2 à cheval (`state`). La
//! page doit obtenir cet état, et relayer la coupe, octet pour octet.

use std::collections::BTreeMap;

use mementum_crdt::{FRAGMENT, PageDoc};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, OffsetKind, Options, ReadTxn, StateVector, Transact, Update};

struct Scene {
    first: Vec<u8>,
    step2: Vec<u8>,
    cut: Vec<u8>,
    state: Vec<u8>,
}

fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/yjs_updates")
}

fn bytes(value: &serde_json::Value) -> Vec<u8> {
    serde_json::from_value(value.clone()).expect("des octets")
}

fn scenes() -> (Vec<u8>, BTreeMap<String, Scene>) {
    let base: Vec<u8> =
        serde_json::from_str(&std::fs::read_to_string(dir().join("base.json")).unwrap()).unwrap();
    let read: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(dir().join("astride.json")).unwrap())
            .unwrap();
    let scenes: BTreeMap<String, Scene> = read
        .into_iter()
        .map(|(name, scene)| {
            let scene = Scene {
                first: bytes(&scene["first"]),
                step2: bytes(&scene["step2"]),
                cut: bytes(&scene["cut"]),
                state: bytes(&scene["state"]),
            };
            (name, scene)
        })
        .collect();
    assert!(scenes.len() >= 7, "les scènes de Vitest");
    (base, scenes)
}

fn state_vector(page: &PageDoc) -> StateVector {
    StateVector::decode_v1(&page.state_vector()).unwrap()
}

/// Ce que la page porte est ce que le vrai Yjs a obtenu (`state`), et un yrs neuf le relit, en
/// entier comme depuis chaque état passé.
fn assert_reads_as_yjs(page: &PageDoc, scene: &Scene, stages: &[Vec<u8>], what: &str) {
    let yjs = PageDoc::from_updates([scene.state.as_slice()])
        .unwrap_or_else(|e| panic!("{what} : l'état de Yjs ne se relit pas : {e}"));
    assert_eq!(page.read_blocks(), yjs.read_blocks(), "{what} : les blocs");
    assert_eq!(
        state_vector(page),
        state_vector(&yjs),
        "{what} : le vecteur d'état"
    );
    assert!(!page.has_pending_deletions(), "{what}");
    let whole = PageDoc::from_updates([page.encode_state().as_slice()])
        .unwrap_or_else(|e| panic!("{what} : l'état de la page ne se relit pas : {e}"));
    assert_eq!(whole.read_blocks(), yjs.read_blocks(), "{what} : relue");
    for (n, stage) in stages.iter().enumerate() {
        let doc = Doc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        doc.get_or_insert_xml_fragment(FRAGMENT);
        doc.transact_mut()
            .apply_update(Update::decode_v1(stage).unwrap())
            .unwrap();
        let sv = doc.transact().state_vector().encode_v1();
        let diff = page.encode_diff(&sv).unwrap();
        doc.transact_mut()
            .apply_update(Update::decode_v1(&diff).unwrap())
            .unwrap();
        let txn = doc.transact();
        assert!(
            txn.store().pending_update().is_none(),
            "{what} : l'étape 2 depuis l'état {n} laisse des éléments en attente"
        );
        assert_eq!(
            txn.state_vector(),
            state_vector(page),
            "{what} : l'étape 2 depuis l'état {n}"
        );
    }
}

/// Ce que la page relaie (`read`) porte les mêmes blocs, octet pour octet, que la coupe de Yjs
/// (`cut`). Seuls leurs jeux de suppressions peuvent différer : Yjs met dans une étape 2 toutes
/// les suppressions de l'onglet, la page celles que la mise à jour a faites.
fn assert_same_structs(read: &[u8], cut: &[u8], what: &str) {
    // Les blocs d'une mise à jour : ses octets, moins ceux de son jeu de suppressions (réencodé,
    // il a la même longueur).
    fn structs(update: &[u8]) -> &[u8] {
        let deletions = Update::decode_v1(update).unwrap().delete_set().encode_v1();
        &update[..update.len() - deletions.len()]
    }
    assert_eq!(
        structs(read),
        structs(cut),
        "{what} : les blocs que relaie la page ne sont pas ceux de la coupe de Yjs"
    );
}

/// La course : la première écriture arrive avant l'étape 2 calculée sans elle. La page coupe
/// l'élément à cheval comme Yjs l'aurait écrit pour elle, relaie cette coupe, et porte ce que le
/// vrai Yjs porte ; son journal relu donne la même page.
#[test]
fn an_honest_step_2_after_a_late_write_is_cut_as_yjs_cuts_it() {
    let (base, scenes) = scenes();
    for (name, scene) in &scenes {
        let page = PageDoc::from_updates([base.as_slice()]).unwrap();
        let first = page
            .apply_client_update(&scene.first)
            .unwrap_or_else(|e| panic!("{name} : la première écriture : {e}"));
        let after_first = page.encode_state();
        let applied = page.try_client_update(&scene.step2).unwrap_or_else(|r| {
            panic!(
                "{name} : l'étape 2 du vrai Yjs {:?} est refusée (intacte : {}) : {}",
                scene.step2, r.intact, r.error
            )
        });
        assert_same_structs(&applied.read, &scene.cut, name);
        assert!(applied.carried.is_none() && applied.withheld.is_none());
        let stages = [base.clone(), after_first];
        assert_reads_as_yjs(&page, scene, &stages, name);
        let journal = [base.as_slice(), &first.read, &applied.read];
        let reloaded = PageDoc::from_updates(journal).unwrap();
        assert_reads_as_yjs(&reloaded, scene, &stages, &format!("{name}, rechargée"));
        assert_eq!(reloaded.revision(), page.revision(), "{name}");
        // Renvoyée (l'onglet se reconnecte encore) : rien de neuf, rien de refusé.
        let again = page
            .apply_client_update(&scene.step2)
            .unwrap_or_else(|e| panic!("{name} : l'étape 2 renvoyée : {e}"));
        assert_eq!(again.read, [0, 0], "{name} : renvoyée");
        assert_reads_as_yjs(&page, scene, &stages, &format!("{name}, renvoyée"));
    }
}

/// Sans la course (l'étape 2 d'abord, la première écriture ensuite), tout passe aussi.
#[test]
fn without_the_race_everything_passes() {
    let (base, scenes) = scenes();
    for (name, scene) in &scenes {
        let page = PageDoc::from_updates([base.as_slice()]).unwrap();
        page.apply_client_update(&scene.step2)
            .unwrap_or_else(|e| panic!("{name} : l'étape 2 : {e}"));
        let late = page
            .apply_client_update(&scene.first)
            .unwrap_or_else(|e| panic!("{name} : l'écriture en retard : {e}"));
        assert_eq!(late.read, [0, 0], "{name}");
        assert_reads_as_yjs(&page, scene, std::slice::from_ref(&base), name);
    }
}

/// Au journal (un journal d'avant R491 gardait la mise à jour lue, part connue comprise) :
/// l'entrée à cheval est coupée de même, et la page se charge avec tout ce qui a été écrit.
#[test]
fn an_old_journal_that_holds_one_is_read_as_yjs_reads_it() {
    let (base, scenes) = scenes();
    for (name, scene) in &scenes {
        let journal = [base.as_slice(), &scene.first, &scene.step2];
        let page = PageDoc::from_updates(journal)
            .unwrap_or_else(|e| panic!("{name} : le journal ne se relit pas : {e}"));
        assert_reads_as_yjs(&page, scene, std::slice::from_ref(&base), name);
    }
}
