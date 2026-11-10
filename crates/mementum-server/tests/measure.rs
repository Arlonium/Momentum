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

/// Créations par série.
const SERIES: usize = 200;

/// Le centile `percent` de durées triées, en millisecondes.
fn centile(sorted: &[Duration], percent: usize) -> f64 {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank - 1].as_secs_f64() * 1000.0
}

/// Affiche la série, et rend sa médiane en millisecondes.
fn report(what: &str, mut times: Vec<Duration>) -> f64 {
    times.sort();
    let median = centile(&times, 50);
    println!(
        "MESURE {what} : {} requêtes, p50 {median:.2} ms, p95 {:.2} ms, max {:.2} ms",
        times.len(),
        centile(&times, 95),
        centile(&times, 100),
    );
    median
}

/// Report du lot 1 (« création d'une page par l'API à 110 ms », serveur en debug) : la durée de
/// `POST /api/v1/pages`, une requête après l'autre. Le nœud, ses accès et son document sont
/// écrits en une transaction.
///
/// Des séries de 200 créations : à la racine d'un espace vide ; en sous-pages d'une même
/// page ; puis à la racine du même espace quand il porte 500, 1 000, 1 500 et 2 000 pages. La
/// ligne « pente » dit ce qu'une page de plus dans l'espace ajoute à une création (chaque
/// création change la révision de l'espace, et le calcul des accès est refait à la requête
/// suivante).
///
/// Puis 200 renommages d'une page de cet espace de 2 200 pages : un titre ne change pas la
/// révision (Ruling R83), sa durée ne dépend donc pas de la taille de l'espace.
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
    for i in 0..SERIES {
        let (elapsed, id) = create(json!({ "title": format!("Page {i}") })).await;
        times.push(elapsed);
        parent = id;
    }
    let empty = report("à la racine d'un espace vide", times);

    let mut times = Vec::new();
    for i in 0..SERIES {
        let body = json!({ "title": format!("Sous-page {i}"), "parentId": parent });
        times.push(create(body).await.0);
    }
    report("en sous-pages d'une même page", times);

    // L'espace porte 400 pages. Une série commence quand il en porte `pages`.
    let mut count = 2 * SERIES;
    let mut full = 0.0;
    for pages in [500, 1000, 1500, 2000] {
        while count < pages {
            create(json!({ "title": format!("Page {count}") })).await;
            count += 1;
        }
        let mut times = Vec::new();
        for _ in 0..SERIES {
            times.push(create(json!({ "title": format!("Page {count}") })).await.0);
            count += 1;
        }
        full = report(&format!("à la racine d'un espace de {pages} pages"), times);
    }
    // Les séries sont centrées sur 100 et sur 2 100 pages.
    println!(
        "MESURE pente : {:.1} µs par page de l'espace",
        (full - empty) * 1000.0 / 2000.0
    );

    let mut times = Vec::new();
    for i in 0..SERIES {
        let start = Instant::now();
        let reply = alice
            .patch(&parent, json!({ "title": format!("Titre {i}") }))
            .await;
        times.push(start.elapsed());
        reply.assert_status(StatusCode::OK);
    }
    report("renommer une page d'un espace de 2 200 pages", times);
}
