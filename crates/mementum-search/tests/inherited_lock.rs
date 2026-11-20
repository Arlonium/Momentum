//! Ruling R1200, la scène d'origine (le rouge de la CI sous Linux dans `reindex.rs`) : le verrou
//! de l'écrivain est un `flock`, qui appartient à la description de fichier ouverte ; un processus
//! fils qu'un autre fil lance en garde une copie de son `fork` à son `exec`, et le verrou que
//! l'ouverture vient de rendre vit encore quand le moteur le reprend.
//!
//! Ce test est **seul dans son binaire** : il lance des processus en boucle, et tout autre test du
//! même binaire qui rouvre un index juste après l'avoir fermé (le premier verrou refuse
//! sur-le-champ, à dessein) recevait `Busy` à tort (vu sur la CI sous Linux, dans
//! `tests/index.rs`, `the_token_of_the_last_commit_is_read_back`). Ne tourne que sous Unix : sous
//! Windows, le verrou n'est pas hérité.

#![cfg(unix)]

#[path = "../../mementum-store/tests/common/tmp.rs"]
mod tmp;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mementum_search::SearchIndex;
use tmp::TestDir;

/// Chaque ouverture se fait dans un dossier neuf : seule la fenêtre de R1200 est éprouvée, pas
/// celle d'une réouverture qui suit une fermeture, où le premier verrou refuse sur-le-champ.
#[test]
fn opening_survives_a_writer_lock_inherited_by_a_child_process() {
    let dir = TestDir::new();
    let stop = Arc::new(AtomicBool::new(false));
    let spawner = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut spawned = 0_u32;
            while !stop.load(Ordering::Relaxed) {
                std::process::Command::new("true")
                    .status()
                    .expect("lancer « true »");
                spawned += 1;
            }
            spawned
        })
    };
    let mut refused = None;
    for n in 0..2_000 {
        let at = dir.path().join(format!("index-{n}"));
        match SearchIndex::open(&at) {
            Ok(opened) => {
                opened.index.close().expect("fermer");
                std::fs::remove_dir_all(&at).expect("retirer le dossier");
            }
            Err(error) => {
                refused = Some((n, error));
                break;
            }
        }
    }
    stop.store(true, Ordering::Relaxed);
    let spawned = spawner.join().expect("le fil qui lance des processus");
    assert!(spawned > 0, "aucun processus lancé pendant les ouvertures");
    if let Some((n, error)) = refused {
        panic!("ouverture {n} refusée ({spawned} processus lancés) : {error:?}");
    }
}
