//! Budget du §5.10 côté serveur : la part du serveur dans « Ouvrir une page
//! jamais vue (réseau local) < 300 ms ». Charger depuis SQLite le `PageDoc`
//! d'une page de 10 000 blocs, comme le fait l'acteur d'une page qui démarre,
//! prend moins de 300 ms au p95 sur 20 chargements.
//!
//! Ignoré par défaut, car la mesure n'a de sens qu'en `--release` :
//! `cargo test --release -p mementum-sync -- --ignored` (job `budgets` de la
//! CI). C'est le seul test ignoré de la crate.

mod common;

use std::time::{Duration, Instant};

use common::*;
use mementum_store::Store;
use mementum_sync::SyncConfig;

/// Blocs de la page.
const BLOCKS: usize = 10_000;

/// Chargements mesurés.
const LOADS: usize = 20;

/// Part serveur de « Ouvrir une page jamais vue (réseau local) < 300 ms ».
const BUDGET: Duration = Duration::from_millis(300);

/// Centile `q` (entre 0 et 1), au plus proche rang : 19ᵉ valeur sur 20 pour le p95.
fn percentile(values: &[Duration], q: f64) -> Duration {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn load_10000_block_page_from_store_under_300ms() {
    let cfg = SyncConfig::default();
    let f = Fixture::new(cfg).await;
    let mmd = (1..=BLOCKS)
        .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    f.write_mmd(&mmd).await;
    let ids = f.ids().await;
    assert_eq!(ids.len(), BLOCKS);

    // Pire cas du journal : `snapshot_every - 1` mises à jour depuis
    // l'instantané (la suivante déclenche une compaction). L'écriture MMD en
    // est une ; un client tape les autres, une frappe par mise à jour.
    let edits = cfg.snapshot_every - 2;
    let client = f.client().await;
    for _ in 0..edits {
        client.type_text(&ids[0], "x");
    }
    // Le serveur traite les messages d'une connexion dans l'ordre : son étape 2
    // arrive quand toutes les frappes sont écrites.
    let before = client.step2_count();
    client.resync();
    assert!(
        client
            .wait_until(Duration::from_secs(120), |c| c.step2_count() > before)
            .await,
        "le serveur n'a pas traité les frappes"
    );
    drop(client);
    let journal = f.store.load_doc(f.page).await.unwrap().updates.len() as u64;
    assert_eq!(journal, cfg.snapshot_every - 1, "journal du pire cas");

    let mut times = Vec::with_capacity(LOADS);
    for _ in 0..LOADS {
        // Une connexion neuve à chaque fois : le cache de pages de SQLite est
        // vide, comme pour l'acteur d'une page que personne n'a ouverte.
        let store = Store::open(&f.db).await.unwrap();
        let start = Instant::now();
        // Ce que fait l'acteur à son démarrage (`actor::load`) : l'instantané
        // et le journal relus, puis appliqués dans l'ordre des `seq`.
        let page = stored_page(&store, f.page).await;
        let elapsed = start.elapsed();
        assert_eq!(page.read_blocks().len(), BLOCKS);
        drop(page);
        store.close().await;
        times.push(elapsed);
    }

    let p50 = percentile(&times, 0.5);
    let p95 = percentile(&times, 0.95);
    println!(
        "MESURE chargement serveur d'une page de {BLOCKS} blocs (+ {journal} mises à jour) : \
         p50 {p50:?}, p95 {p95:?}, max {:?} ({LOADS} chargements)",
        times.iter().max().unwrap()
    );
    assert!(
        p95 < BUDGET,
        "chargement de la page de {BLOCKS} blocs : p95 {p95:?} (budget {BUDGET:?}), mesures {times:?}"
    );
}
