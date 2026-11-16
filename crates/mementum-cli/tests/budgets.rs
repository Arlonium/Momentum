//! Budgets du §5.10 pour le binaire : « Démarrage à froid du binaire < 1 s »
//! et « Mémoire du serveur au repos < 150 Mo ».
//!
//! Chaque mesure garde le meilleur de 3 lancements : l'antivirus analyse un
//! exécutable neuf au premier lancement. Les deux tests ne tournent pas en
//! même temps, pour ne pas fausser les mesures.
//!
//! Le démarrage mesuré est donc **à chaud** : un processus neuf sur un
//! dossier de données neuf (base créée et migrée), mais un exécutable que le
//! système a déjà en cache. La mesure est optimiste sur le cache ; un vrai
//! démarrage à froid, après un redémarrage de la machine, n'est pas mesuré.
//!
//! Le démarrage est une mesure (Ruling R486) : ignorée par défaut, à lancer en
//! `--release`, sur le binaire que les gens lancent (job `budgets` de la CI :
//! `cargo test --release -p mementum-sync -p mementum-cli -- --ignored`). En
//! debug, à chaque passage, il mesurait la charge du coureur : 1,43 s au mieux
//! sur la CI sous Linux, 1,49 s sur un poste chargé, 67 à 92 ms au calme. La
//! mémoire au repos reste un test exécuté : elle ne dépend pas de la charge.

mod common;

use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use common::Server;
use sysinfo::{Pid, ProcessesToUpdate, System};

/// Lancements par mesure.
const LAUNCHES: usize = 3;

/// « Démarrage à froid du binaire < 1 s » : le budget du §5.10. Le test le
/// compare à un démarrage à chaud (voir l'en-tête).
const COLD_START: Duration = Duration::from_secs(1);

/// « Mémoire du serveur au repos < 150 Mo » (mégaoctets : 10⁶ octets).
const IDLE_RSS: u64 = 150 * 1_000_000;

/// Les mesures se font l'une après l'autre.
static SERIAL: Mutex<()> = Mutex::new(());

/// Une mesure du démarrage faite en debug ne dit rien du binaire que les gens lancent.
fn release_only() {
    // Un `assert!` sur `cfg!` est refusé par clippy (`assertions_on_constants`).
    if cfg!(debug_assertions) {
        panic!(
            "mesure sans valeur en debug : lancer `cargo test --release -p mementum-cli -- --ignored`"
        );
    }
}

#[test]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
fn cold_start_under_1s() {
    release_only();
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut times = Vec::new();
    for _ in 0..LAUNCHES {
        // Un dossier neuf : la base est créée et migrée à chaque lancement.
        let dir = tempfile::tempdir().unwrap();
        let start = Instant::now();
        let server = Server::start(&dir.path().join("donnees"));
        let (status, body) = server.http("GET", "/health", None);
        let elapsed = start.elapsed();
        assert_eq!((status, body.as_str()), (200, "ok"));
        times.push(elapsed);
    }
    let best = *times.iter().min().unwrap();
    println!("démarrage à chaud jusqu'à /health : {times:?}, meilleur {best:?}");
    assert!(
        best < COLD_START,
        "démarrage à chaud : {best:?} au mieux (budget {COLD_START:?}), mesures {times:?}"
    );
}

#[test]
fn idle_rss_under_150mb() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut samples = Vec::new();
    for _ in 0..LAUNCHES {
        let dir = tempfile::tempdir().unwrap();
        let start = Instant::now();
        let server = Server::start(&dir.path().join("donnees"));
        // La mesure se lit 2 s après le lancement, serveur au repos.
        thread::sleep(Duration::from_secs(2).saturating_sub(start.elapsed()));
        let pid = Pid::from_u32(server.pid());
        let mut system = System::new().expect("sysinfo");
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        let rss = system
            .process(pid)
            .expect("le processus du serveur")
            .memory();
        assert!(rss > 0, "RSS illisible pour le processus {pid}");
        samples.push(rss);
    }
    let best = *samples.iter().min().unwrap();
    println!(
        "RSS au repos (2 s après le lancement) : {:?} Mo, meilleur {:.1} Mo",
        samples
            .iter()
            .map(|b| format!("{:.1}", *b as f64 / 1e6))
            .collect::<Vec<_>>(),
        best as f64 / 1e6
    );
    assert!(
        best < IDLE_RSS,
        "RSS au repos : {best} octets au mieux (budget {IDLE_RSS})"
    );
}
