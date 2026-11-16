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

/// Ruling R350 : ce que coûte `GET /api/v1/pages/{id}/comments` sur une page pleine, tant que la
/// lecture n'est pas paginée. 10 000 messages de 419 octets (4 Mio de texte, le plafond), en 20
/// fils de 500, écrits par un compte au nom de 100 caractères ; puis la même page au pire cas
/// accepté (Ruling R357) : 20 000 réactions, la part de dix comptes au nom de 100 caractères
/// (8 emojis sur chacun de 250 messages). Les fils sont écrits en SQL, en une transaction. Une
/// ligne pour la durée de 20 lectures, une pour la taille de la réponse, dans chaque cas.
#[tokio::test]
#[ignore = "mesure, à lancer en --release avec --nocapture"]
async fn measure_comments_of_a_full_page() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Pleine" })).await.id;
    let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", t.db_path().display()))
        .await
        .unwrap();
    let long_name = "N".repeat(100);
    sqlx::query("UPDATE users SET name = ? WHERE id = ?")
        .bind(&long_name)
        .bind(alice.id.to_string())
        .execute(&side)
        .await
        .unwrap();
    let body = "x".repeat(419);
    let mut tx = side.begin().await.unwrap();
    let mut at = 1_000_000_i64;
    let mut comments = Vec::new();
    for _ in 0..20 {
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
        for _ in 0..500 {
            at += 1;
            let comment = uuid::Uuid::now_v7().to_string();
            sqlx::query(
                "INSERT INTO comments (id, thread_id, author_id, body, created_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&comment)
            .bind(&thread)
            .bind(alice.id.to_string())
            .bind(&body)
            .bind(at)
            .execute(&mut *tx)
            .await
            .unwrap();
            comments.push(comment);
        }
    }
    tx.commit().await.unwrap();

    let uri = format!("/api/v1/pages/{page}/comments");
    let read = async |what: &str| {
        let mut times = Vec::new();
        let mut bytes = 0;
        for _ in 0..20 {
            let start = Instant::now();
            let reply = alice.browser.get(&uri).await;
            times.push(start.elapsed());
            reply.assert_status(StatusCode::OK);
            bytes = reply.body.len();
        }
        report(&format!("GET …/comments, page pleine, {what}"), times);
        println!(
            "MESURE taille de la réponse, {what} : {bytes} octets ({:.1} Mio)",
            bytes as f64 / (1024.0 * 1024.0)
        );
    };
    read("sans réaction").await;

    let mut tx = side.begin().await.unwrap();
    for user in 0..10usize {
        let id = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO users (id, email, name, password_hash, created_at) \
             VALUES (?, ?, ?, 'pas-un-hachage', 1)",
        )
        .bind(&id)
        .bind(format!("reaction-{user}@exemple.test"))
        .bind(format!("{user}{}", "R".repeat(99)))
        .execute(&mut *tx)
        .await
        .unwrap();
        for comment in &comments[user * 250..(user + 1) * 250] {
            for i in 0..8u32 {
                sqlx::query(
                    "INSERT INTO comment_reactions (comment_id, user_id, emoji, created_at) \
                     VALUES (?, ?, ?, ?)",
                )
                .bind(comment)
                .bind(&id)
                .bind(char::from_u32(0x1F600 + i).unwrap().to_string())
                .bind(at)
                .execute(&mut *tx)
                .await
                .unwrap();
            }
        }
    }
    tx.commit().await.unwrap();
    side.close().await;
    read("20 000 réactions de dix comptes").await;
}

/// Un morceau du corps de l'envoi mesuré : statique, il n'est jamais alloué.
static UPLOAD_CHUNK: [u8; 64 * 1024] = [42; 64 * 1024];

/// Un corps sans longueur, fait de `left` fois [`UPLOAD_CHUNK`], produit au fil de la lecture.
struct GeneratedBody {
    left: usize,
}

impl http_body::Body for GeneratedBody {
    type Data = axum::body::Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<axum::body::Bytes>, Self::Error>>> {
        if self.left == 0 {
            return std::task::Poll::Ready(None);
        }
        self.left -= 1;
        let chunk = axum::body::Bytes::from_static(&UPLOAD_CHUNK);
        std::task::Poll::Ready(Some(Ok(http_body::Frame::data(chunk))))
    }
}

/// Le budget d'une frappe vue chez un autre (conception §5.10), au p95.
const TYPING_BUDGET: Duration = Duration::from_millis(150);

/// Task S4a (Ruling R247 : le dépôt s'écrit en `async`, sans appel bloquant sur les fils qui
/// relaient les frappes) : pendant l'envoi d'un fichier de 100 Mio dans une page (la borne par
/// défaut), un client tape dans une autre page, et chaque frappe est chronométrée jusqu'à ce
/// qu'un autre client de cette page l'ait reçue ; p95 sous 150 ms. La durée de l'envoi est
/// consignée. L'envoi passe par l'application en mémoire, la synchro par une vraie connexion.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure, à lancer en --release avec --nocapture"]
async fn measure_keystrokes_during_a_100_mib_upload() {
    use axum::body::Body;
    use axum::http::{Request, header};
    use common::sync::text;
    use common::{sync_url, synced_as, upload_uri};
    use tower::ServiceExt;

    let t = TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await;
    let addr = t.listen().await;
    let alice = t.account("Alice").await;
    let typed = alice.create(json!({ "title": "Frappes" })).await.id;
    alice
        .put_mmd(&typed, "Texte\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let url = sync_url(addr, &typed);
    let (author, other) = (
        synced_as(&alice.browser, &url).await,
        synced_as(&alice.browser, &url).await,
    );
    let block = author.blocks()[0].id.clone();
    let target = alice.create(json!({ "title": "Fichier" })).await.id;
    let target = uuid::Uuid::parse_str(&target).unwrap();
    let chunks = (100 << 20) / UPLOAD_CHUNK.len();
    let request = Request::post(upload_uri(target, "gros.bin"))
        .header(header::HOST, "localhost")
        .header(header::COOKIE, alice.browser.cookie().unwrap())
        .body(Body::new(GeneratedBody { left: chunks }))
        .unwrap();
    let router = t.router.clone();
    let upload = tokio::spawn(async move {
        let start = Instant::now();
        let response = router.oneshot(request).await.unwrap();
        (response.status(), start.elapsed())
    });

    let mut times = Vec::new();
    while !upload.is_finished() {
        let start = Instant::now();
        author.type_text(&block, "x");
        let expected = text(&author);
        assert!(
            other
                .wait_until(Duration::from_secs(60), |c| text(c) == expected)
                .await,
            "la frappe n'est pas arrivée"
        );
        times.push(start.elapsed());
    }
    let (status, took) = upload.await.unwrap();
    assert_eq!(status, StatusCode::CREATED);
    println!(
        "MESURE envoi d'un fichier de 100 Mio : {:.0} ms",
        took.as_secs_f64() * 1000.0
    );
    assert!(!times.is_empty(), "aucune frappe pendant l'envoi");
    let mut sorted = times.clone();
    sorted.sort();
    let p95 = sorted[(sorted.len() * 95).div_ceil(100).max(1) - 1];
    report(
        "frappe vue par un autre client pendant l'envoi de 100 Mio",
        times,
    );
    assert!(
        p95 < TYPING_BUDGET,
        "frappe pendant l'envoi : p95 {p95:?} (budget {TYPING_BUDGET:?})"
    );
}

/// Le MMD d'une page de `blocks` paragraphes, chacun avec deux marques.
fn paragraphs(blocks: usize) -> String {
    (0..blocks)
        .map(|i| format!("Bloc {i} : du texte **en gras** et *en italique*, pour peser.\n"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Task S6 (Ruling R252) : dupliquer une page de 10 000 blocs, puis 100 pages de 1 000 blocs, puis
/// 10 pages de 10 000. Pour chaque cas : la durée de la requête ; le poids des documents copiés ;
/// la durée de la transaction d'écriture seule (la même copie, récrite par le store, hors de la
/// route) ; et, pendant la requête, les frappes d'un client dans une autre page, chacune
/// chronométrée jusqu'à ce qu'un autre client de cette page l'ait reçue. L'écriture de SQLite est
/// unique pour l'instance : pendant la transaction, le journal de chaque page ouverte attend.
///
/// Ces cas passent sous la borne de poids ([`mementum_server::MAX_DUPLICATED_BYTES`]) ou la
/// dépassent : la ligne dit lequel, et la mesure de la transaction seule vaut dans les deux cas.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure, à lancer en --release avec --nocapture"]
async fn measure_page_duplication() {
    use axum::body::Body;
    use axum::http::{Request, header};
    use common::sync::text;
    use common::{sync_url, synced_as};
    use mementum_store::{CopiedPage, NewCopy};
    use tower::ServiceExt;

    let t = TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await;
    let addr = t.listen().await;
    let alice = t.account("Alice").await;
    let store = Store::open(&t.db_path()).await.unwrap();
    let typed = alice.create(json!({ "title": "Frappes" })).await.id;
    alice
        .put_mmd(&typed, "Texte\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let url = sync_url(addr, &typed);
    let (author, other) = (
        synced_as(&alice.browser, &url).await,
        synced_as(&alice.browser, &url).await,
    );
    let block = author.blocks()[0].id.clone();

    // Le dernier cas dépasse la borne de poids : la route le refuse, et seule sa transaction est
    // mesurée.
    for (pages, blocks) in [
        (1usize, 10_000usize),
        (100, 1_000),
        (10, 10_000),
        (14, 10_000),
        (100, 10_000),
    ] {
        let refused = pages * blocks > 150_000;
        let what = format!("{pages} pages de {blocks} blocs");
        let content = paragraphs(blocks);
        let root = alice.create(json!({ "title": what })).await.id;
        let mut all = vec![root.clone()];
        // Refusée, la copie n'a pas besoin de cent pages pleines : vingt suffisent à peser trop.
        for i in 1..if refused { 20 } else { pages } {
            let body = json!({ "title": format!("Sous-page {i}"), "parentId": root });
            all.push(alice.create(body).await.id);
        }
        for page in &all {
            alice
                .put_mmd(page, content.clone())
                .await
                .assert_status(StatusCode::NO_CONTENT);
        }

        let request = Request::post(format!("/api/v1/pages/{root}/duplicate"))
            .header(header::HOST, "localhost")
            .header(header::COOKIE, alice.browser.cookie().unwrap())
            .body(Body::empty())
            .unwrap();
        let router = t.router.clone();
        let duplication = tokio::spawn(async move {
            let start = Instant::now();
            let response = router.oneshot(request).await.unwrap();
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (status, body, start.elapsed())
        });
        let mut times = Vec::new();
        while !duplication.is_finished() {
            let start = Instant::now();
            author.type_text(&block, "x");
            let expected = text(&author);
            assert!(
                other
                    .wait_until(Duration::from_secs(60), |c| text(c) == expected)
                    .await,
                "la frappe n'est pas arrivée"
            );
            times.push(start.elapsed());
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let (status, body, took) = duplication.await.unwrap();
        println!(
            "MESURE duplication de {what} : {status}, requête {:.0} ms",
            took.as_secs_f64() * 1000.0
        );
        assert_eq!(status == StatusCode::CREATED, !refused, "{what}");
        if !refused {
            let mut sorted = times.clone();
            sorted.sort();
            let p95 = sorted[(sorted.len() * 95).div_ceil(100).max(1) - 1];
            assert!(
                p95 < TYPING_BUDGET,
                "frappe pendant la duplication de {what} : p95 {p95:?} (budget {TYPING_BUDGET:?})"
            );
        }
        report(
            &format!("frappe vue par un autre pendant la duplication de {what}"),
            times,
        );

        // La transaction seule : les documents d'une copie, récrits par le store.
        let docs: Vec<Vec<u8>> = if status == StatusCode::CREATED {
            let copy: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let copy = uuid::Uuid::parse_str(copy["id"].as_str().unwrap()).unwrap();
            let nodes = store.list_nodes(alice.space).await.unwrap();
            let mut docs = Vec::new();
            for node in nodes
                .iter()
                .filter(|n| n.id == copy || n.parent_id == Some(copy))
            {
                let state = store.load_doc(node.id).await.unwrap();
                docs.push(state.snapshot.expect("l'instantané de la copie").1);
            }
            docs
        } else {
            // Refusée pour son poids : des documents du même poids, bâtis ici.
            println!("        refus : {}", String::from_utf8_lossy(&body));
            let blocks = mementum_md::parse(&content).unwrap();
            let page = mementum_crdt::PageDoc::new_page().0;
            page.write_blocks(&blocks).unwrap();
            vec![page.encode_state(); pages]
        };
        assert_eq!(docs.len(), pages);
        let bytes: usize = docs.iter().map(Vec::len).sum();
        let copy = NewCopy {
            space_id: alice.space,
            source: uuid::Uuid::parse_str(&root).unwrap(),
            parent_id: None,
            after: Some(Sibling::Last),
            grants: vec![Grant {
                subject: Subject::User(alice.id),
                level: Level::Full,
            }],
            created_by: alice.id,
            pages: docs
                .into_iter()
                .enumerate()
                .map(|(i, doc)| CopiedPage {
                    id: uuid::Uuid::now_v7(),
                    parent: (i > 0).then_some(0),
                    title: format!("Copie {i}"),
                    position: (i > 0).then(|| format!("a{i:04}")),
                    doc,
                    attachments: Vec::new(),
                })
                .collect(),
        };
        let mut transactions = Vec::new();
        for _ in 0..if refused { 1 } else { 3 } {
            let start = Instant::now();
            store
                .duplicate_pages(&copy_with_fresh_ids(&copy), 1)
                .await
                .unwrap();
            transactions.push(start.elapsed());
        }
        println!(
            "MESURE transaction de la copie de {what} : {:.1} Mio de documents, {:?}",
            bytes as f64 / (1 << 20) as f64,
            transactions
        );
    }
    store.close().await;
}

/// La même copie, sous des identifiants de pages neufs : pour l'écrire une fois de plus.
fn copy_with_fresh_ids(copy: &mementum_store::NewCopy) -> mementum_store::NewCopy {
    let mut again = copy.clone();
    for page in &mut again.pages {
        page.id = uuid::Uuid::now_v7();
    }
    again
}
