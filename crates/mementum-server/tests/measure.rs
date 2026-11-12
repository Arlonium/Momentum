//! Mesures, pas des tests : elles n'affirment rien, elles affichent des lignes `MESURE`. À
//! lancer à la demande, sur un serveur compilé en `--release`, une mesure à la fois (ensemble,
//! elles se gêneraient) :
//!
//! `cargo test --release -p mementum-server --test measure -- --ignored --nocapture --test-threads=1`
//!
//! Les requêtes passent par l'application en mémoire (`common/mod.rs`) : tout le chemin d'une
//! requête (garde-fous, session, droits, écriture durable dans SQLite), sans la couche TCP.

mod common;

use std::time::{Duration, Instant};

use axum::http::StatusCode;
use common::TestApp;
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_server::SignupMode;
use mementum_store::{NewNode, NodeKind, Sibling, Store};
use serde_json::json;

/// Créations par série.
const SERIES: usize = 200;

/// Pages de l'espace où se mesure la première lecture après une écriture.
const PAGES: usize = 2000;

/// Écritures suivies de leurs lectures, dans cette mesure.
const ROUNDS: usize = 100;

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

/// Fils d'une page chargée de commentaires, et messages par fil : dix mille messages.
const THREADS: usize = 1_000;
const MESSAGES_PER_THREAD: usize = 10;

/// Report de la Task 8 du lot 3 (aucun plafond de fils ni de messages par page avant le lot 7) :
/// ce que coûte `GET /api/v1/pages/{id}/comments` sur une page de 1 000 fils de 10 messages,
/// chaque message de 120 caractères avec une réaction. Les fils sont écrits en SQL, en une
/// transaction : ce n'est pas leur écriture qu'on mesure. Une ligne pour la durée de 20
/// lectures, une pour la taille de la réponse.
#[tokio::test]
#[ignore = "mesure, à lancer en --release avec --nocapture"]
async fn measure_comments_of_a_busy_page() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Très commentée" })).await.id;
    let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", t.db_path().display()))
        .await
        .unwrap();
    let mut tx = side.begin().await.unwrap();
    let body = "Un message d'une longueur ordinaire, comme on en écrit dans la marge d'une page, \
                avec un peu de texte.";
    let mut at = 1_000_000_i64;
    for _ in 0..THREADS {
        let thread = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO comment_threads (id, node_id, block_id, created_by, created_at) \
             VALUES (?, ?, NULL, ?, ?)",
        )
        .bind(&thread)
        .bind(&page)
        .bind(alice.id.to_string())
        .bind(at)
        .execute(&mut *tx)
        .await
        .unwrap();
        for _ in 0..MESSAGES_PER_THREAD {
            at += 1;
            let comment = uuid::Uuid::now_v7().to_string();
            sqlx::query(
                "INSERT INTO comments (id, thread_id, author_id, body, created_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&comment)
            .bind(&thread)
            .bind(alice.id.to_string())
            .bind(body)
            .bind(at)
            .execute(&mut *tx)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO comment_reactions (comment_id, user_id, emoji, created_at) \
                 VALUES (?, ?, '👍', ?)",
            )
            .bind(&comment)
            .bind(alice.id.to_string())
            .bind(at)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
    }
    tx.commit().await.unwrap();
    side.close().await;

    let uri = format!("/api/v1/pages/{page}/comments");
    let mut times = Vec::new();
    let mut bytes = 0;
    for _ in 0..20 {
        let start = Instant::now();
        let reply = alice.browser.get(&uri).await;
        times.push(start.elapsed());
        reply.assert_status(StatusCode::OK);
        bytes = reply.body.len();
    }
    let what = format!(
        "GET …/comments, {THREADS} fils de {MESSAGES_PER_THREAD} messages, une réaction chacun"
    );
    report(&what, times);
    println!(
        "MESURE taille de la réponse : {bytes} octets ({:.1} Mio)",
        bytes as f64 / (1024.0 * 1024.0)
    );
}

/// Ce qu'une écriture dans un espace coûte à ceux qui le lisent ensuite. Dans un espace de
/// 2 000 pages, Alice crée une page (la révision de l'espace avance), puis cinq autres membres
/// lisent chacun cette page (`GET /api/v1/pages/{id}`), l'un après l'autre : c'est leur
/// première requête depuis l'écriture, celle où leurs droits sont recalculés. Une ligne par
/// personne, dans l'ordre où elles lisent ; puis la somme des cinq ; puis, pour comparer, la
/// même lecture refaite sans écriture entre-temps (droits déjà calculés).
#[tokio::test]
#[ignore = "mesure, à lancer en --release avec --nocapture"]
async fn measure_first_read_after_a_write() {
    let t = TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await;
    let alice = t.account("Alice").await;
    let mut readers = Vec::new();
    for name in ["Basile", "Chloe", "David", "Emma", "Farid"] {
        readers.push(t.account(name).await);
    }
    // Les rôles et les pages de départ sont posés par le store : ce ne sont pas eux qu'on
    // mesure.
    let store = Store::open(&t.db_path()).await.unwrap();
    for reader in &readers {
        store
            .add_member(alice.space, reader.id, Role::Member)
            .await
            .unwrap();
    }
    for i in 0..PAGES {
        store
            .create_node(NewNode {
                space_id: alice.space,
                parent_id: None,
                after: Some(Sibling::Last),
                kind: NodeKind::Page,
                title: format!("Page {i}"),
                created_by: Some(alice.id),
                grants: vec![Grant {
                    subject: Subject::Members,
                    level: Level::Full,
                }],
                doc: None,
            })
            .await
            .unwrap();
    }
    store.close().await;

    let read = async |reader: &common::Account<'_>, id: &str| {
        let start = Instant::now();
        let reply = reader.browser.get(&format!("/api/v1/pages/{id}")).await;
        let elapsed = start.elapsed();
        reply.assert_status(StatusCode::OK);
        elapsed
    };
    let mut first: Vec<Vec<Duration>> = vec![Vec::new(); readers.len()];
    let mut all = Vec::new();
    let mut again = Vec::new();
    for i in 0..ROUNDS {
        // Dans la section « Espace » : les cinq autres la voient.
        let body = json!({ "title": format!("Neuve {i}"), "section": "space" });
        let id = alice.create(body).await.id;
        let mut sum = Duration::ZERO;
        for (rank, reader) in readers.iter().enumerate() {
            let elapsed = read(reader, &id).await;
            first[rank].push(elapsed);
            sum += elapsed;
        }
        all.push(sum);
        for reader in &readers {
            again.push(read(reader, &id).await);
        }
    }
    let what = format!("première lecture après une écriture, espace de {PAGES} pages");
    for (rank, times) in first.into_iter().enumerate() {
        report(&format!("{what}, personne {}", rank + 1), times);
    }
    report(&format!("{what}, somme des cinq personnes"), all);
    report(
        &format!("lecture suivante, sans écriture entre-temps, espace de {PAGES} pages"),
        again,
    );
}
