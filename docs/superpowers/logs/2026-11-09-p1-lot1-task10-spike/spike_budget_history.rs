//! SPIKE jetable (Task 10), non commité : budget de chargement après des
//! réécritures complètes de la page (PUT /mmd), avec ou sans `skip_gc`.
mod common;

use std::time::{Duration, Instant};

use common::*;
use mementum_store::Store;
use mementum_sync::SyncConfig;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spike"]
async fn spike_load_after_rewrites() {
    let gc = std::env::var_os("SPIKE_GC").is_some();
    let f = Fixture::new(SyncConfig::default()).await;
    let mmd = (1..=10_000)
        .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    for rewrites in 0..=3 {
        f.write_mmd(&mmd).await;
        let mut times = Vec::new();
        let mut bytes = 0;
        for _ in 0..20 {
            let store = Store::open(&f.db).await.unwrap();
            let start = Instant::now();
            let page = stored_page(&store, f.page).await;
            times.push(start.elapsed());
            bytes = page.encode_state().len();
            drop(page);
            store.close().await;
        }
        times.sort();
        let p95: Duration = times[19];
        println!(
            "MESURE gc={gc} réécritures={rewrites} : état {} Ko, p50 {:?}, p95 {p95:?}",
            bytes / 1024,
            times[10]
        );
    }
}
