//! Mesures, pas des tests : elles n'affirment rien, elles affichent des lignes `MESURE`. À
//! lancer à la demande, sur un serveur compilé en `--release` :
//!
//! `cargo test --release -p mementum-server --test measure -- --ignored --nocapture`
//!
//! Les requêtes passent par l'application en mémoire (`common/mod.rs`) : tout le chemin d'une
//! requête (garde-fous, session, droits, écriture durable dans SQLite), sans la couche TCP.

mod common;

use std::time::{Duration, Instant};

use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

/// Le centile `percent` de durées triées, en millisecondes.
fn centile(sorted: &[Duration], percent: usize) -> f64 {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank - 1].as_secs_f64() * 1000.0
}

fn report(what: &str, mut times: Vec<Duration>) {
    times.sort();
    println!(
        "MESURE {what} : {} créations, p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        times.len(),
        centile(&times, 50),
        centile(&times, 95),
        centile(&times, 100),
    );
}

/// Report du lot 1 (« création d'une page par l'API à 110 ms », serveur en debug) : la durée de
/// `POST /api/v1/pages`, une requête après l'autre. Le nœud, ses accès et son document sont
/// écrits en une transaction ; la réponse relit les droits de l'auteur, que la création vient de
/// changer.
///
/// Trois séries de 200 créations : à la racine d'un espace vide ; en sous-pages d'une même
/// page ; à la racine du même espace une fois qu'il porte 2 000 pages (le calcul des accès,
/// refait après chaque écriture, relit tout l'espace).
#[tokio::test]
#[ignore = "mesure, à lancer en --release avec --nocapture"]
async fn measure_page_creation() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let create = async |body: serde_json::Value| {
        let start = Instant::now();
        let reply = alice.post(body).await;
        let elapsed = start.elapsed();
        reply.assert_status(StatusCode::CREATED);
        (
            elapsed,
            reply.json()["id"].as_str().expect("id").to_string(),
        )
    };

    let mut times = Vec::new();
    let mut parent = String::new();
    for i in 0..200 {
        let (elapsed, id) = create(json!({ "title": format!("Page {i}") })).await;
        times.push(elapsed);
        parent = id;
    }
    report("à la racine d'un espace vide", times);

    let mut times = Vec::new();
    for i in 0..200 {
        let body = json!({ "title": format!("Sous-page {i}"), "parentId": parent });
        times.push(create(body).await.0);
    }
    report("en sous-pages d'une même page", times);

    for i in 400..2000 {
        create(json!({ "title": format!("Page {i}"), "section": "space" })).await;
    }
    let mut times = Vec::new();
    for i in 2000..2200 {
        times.push(create(json!({ "title": format!("Page {i}") })).await.0);
    }
    report("à la racine d'un espace de 2 000 pages", times);
}
