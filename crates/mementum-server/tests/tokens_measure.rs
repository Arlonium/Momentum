//! Mesure de l'identité par jeton (lot 5, Task S5, Rulings R1217, R1573, R1575) : la lecture d'un
//! jeton, la restriction seule, les routes qui restreignent tout l'espace par jeton et par
//! session, et le `404` d'une page hors de la portée contre celui d'une page inconnue, à 10 000 et
//! 100 000 pages et sur une chaîne profonde. Une mesure, pas un budget : `#[ignore]`, en release,
//! une à la fois (`cargo test --release -p mementum-server --test tokens_measure -- --ignored
//! --nocapture`) ; elle écrit des lignes `MESURE`.

mod common;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::*;
use mementum_acl::{Barred, Grant, Level, Role, SpaceAccess, Subject, TreeNode};
use mementum_server::SignupMode;
use mementum_store::{TokenLevel, TokenScope};
use uuid::Uuid;

static ALONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn percentiles(mut times: Vec<Duration>) -> (f64, f64) {
    times.sort();
    let at = |q: f64| {
        let rank = ((times.len() as f64 * q).ceil() as usize).clamp(1, times.len()) - 1;
        times[rank].as_secs_f64() * 1e3
    };
    (at(0.5), at(0.95))
}

fn timed<T>(rounds: usize, mut work: impl FnMut() -> T) -> (f64, f64) {
    let mut times = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let started = Instant::now();
        std::hint::black_box(work());
        times.push(started.elapsed());
    }
    percentiles(times)
}

/// La restriction seule, en mémoire : un espace de `pages` pages sous des racines ouvertes aux
/// membres (chaque page sous l'une des deux cents plus récentes, comme le générateur de S0), et
/// une chaîne de `pages` pages imbriquées.
#[test]
#[ignore]
fn measure_narrowing_alone() {
    for pages in [10_000usize, 100_000] {
        let user = Uuid::from_u128(1);
        let ids: Vec<Uuid> = (0..pages)
            .map(|i| Uuid::from_u128(10 + i as u128))
            .collect();
        for shape in ["large", "chaîne"] {
            let nodes: Vec<TreeNode> = ids
                .iter()
                .enumerate()
                .map(|(i, &id)| TreeNode {
                    id,
                    parent_id: match (shape, i) {
                        (_, 0) => None,
                        ("large", i) if i < 200 => None,
                        ("large", i) => Some(ids[i - 1 - (i * 7919) % 200]),
                        (_, i) => Some(ids[i - 1]),
                    },
                    trashed: false,
                })
                .collect();
            let grants: Vec<(Uuid, Grant)> = nodes
                .iter()
                .filter(|n| n.parent_id.is_none())
                .map(|n| {
                    let grant = Grant {
                        subject: Subject::Members,
                        level: Level::Edit,
                    };
                    (n.id, grant)
                })
                .collect();
            let (compute_p50, compute_p95) = timed(10, || {
                SpaceAccess::compute(&nodes, &grants, user, Some(Role::Member))
            });
            let account = SpaceAccess::compute(&nodes, &grants, user, Some(Role::Member));
            let deepest = *ids.last().unwrap();
            let roots_in = [ids[0]];
            let mut roots_out = [Uuid::from_u128(u128::MAX)];
            roots_out.sort();
            let barred = Barred::NONE;
            let (all_p50, all_p95) = timed(30, || account.narrowed(Level::Edit, barred, None));
            let (pages_p50, pages_p95) = timed(30, || {
                account.narrowed(Level::Edit, barred, Some(&roots_in))
            });
            let (in_p50, in_p95) = timed(2_000, || {
                account.narrowed_node(deepest, Level::Edit, barred, Some(&roots_in))
            });
            let (out_p50, out_p95) = timed(2_000, || {
                account.narrowed_node(deepest, Level::Edit, barred, Some(&roots_out))
            });
            let (unknown_p50, unknown_p95) = timed(2_000, || {
                account.narrowed_node(Uuid::from_u128(5), Level::Edit, barred, Some(&roots_in))
            });
            println!(
                "MESURE restriction {shape} {pages} pages : calcul du compte {compute_p50:.3} / \
                 {compute_p95:.3} ms ; tout l'espace sans racines {all_p50:.3} / {all_p95:.3} ms, \
                 avec une racine {pages_p50:.3} / {pages_p95:.3} ms ; la page la plus profonde \
                 jugée seule, dans la portée {in_p50:.4} / {in_p95:.4} ms, hors d'elle \
                 {out_p50:.4} / {out_p95:.4} ms, une page inconnue {unknown_p50:.4} / \
                 {unknown_p95:.4} ms (p50 / p95)"
            );
        }
    }
}

/// Les routes, dans le serveur : la lecture d'un jeton, l'arbre par jeton et par session, une page
/// lue par jeton et par session, le `404` d'une page hors de la portée contre celui d'une page
/// inconnue.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn measure_token_lookup_and_narrowing() {
    let _alone = ALONE.lock().await;
    for pages in [10_000usize, 100_000] {
        for shape in ["large", "chaîne"] {
            if shape == "chaîne" && pages > 10_000 {
                continue;
            }
            measure_routes(pages, shape).await;
        }
    }
}

async fn measure_routes(pages: usize, shape: &str) {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.token_calls_per_min = 100_000;
        cfg.account_token_calls_per_min = 100_000;
    })
    .await;
    let team = Team::new(&t).await;
    let root = team
        .page(None, "Racine", &[(Subject::Members, Level::Edit)])
        .await;
    let aside = team
        .page(None, "À côté", &[(Subject::Members, Level::Edit)])
        .await;
    let space = team.space;
    t.indexer.settled().await;
    team.store.close().await;
    let mona = team.mona.id;
    let mona_cookie = team.mona.browser.cookie().unwrap();
    drop(team);
    let (dir, cfg) = t.stop().await;

    // Les pages, d'un coup, par une connexion à côté, serveur arrêté.
    let ids: Vec<Uuid> = (0..pages).map(|i| Uuid::from_u128(1 + i as u128)).collect();
    let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", cfg.db_path().display()))
        .await
        .unwrap();
    let mut tx = side.begin().await.unwrap();
    for (start, chunk) in ids.chunks(200).enumerate() {
        let rows = vec!["(?, ?, ?, 'page', 'Page', ?, 0, 0)"; chunk.len()].join(", ");
        let sql = format!(
            "INSERT INTO nodes (id, space_id, parent_id, kind, title, position, created_at, \
             updated_at) VALUES {rows}"
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for (offset, id) in chunk.iter().enumerate() {
            let i = start * 200 + offset;
            let parent = match shape {
                "chaîne" if i == 0 => root,
                "chaîne" => ids[i - 1],
                _ if i < 200 => root,
                _ => ids[i - 1 - (i * 7919) % 200],
            };
            query = query
                .bind(id.to_string())
                .bind(space.to_string())
                .bind(parent.to_string())
                .bind(format!("{i:08}"));
        }
        query.execute(&mut *tx).await.unwrap();
    }
    sqlx::query("UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = ?")
        .bind(space.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    side.close().await;
    let t = TestApp::on(dir, cfg).await;
    let (all, _) = api_token(&t.store, mona, TokenLevel::Read, TokenScope::All, false).await;
    let (scoped, _) = api_token(
        &t.store,
        mona,
        TokenLevel::Read,
        TokenScope::Pages {
            space,
            pages: vec![root],
        },
        false,
    )
    .await;
    let (elsewhere, _) = api_token(
        &t.store,
        mona,
        TokenLevel::Read,
        TokenScope::Pages {
            space,
            pages: vec![aside],
        },
        false,
    )
    .await;
    let deepest = *ids.last().unwrap();
    let by_session = |uri: String| {
        let mut request = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        request
            .headers_mut()
            .insert(axum::http::header::COOKIE, mona_cookie.parse().unwrap());
        request
    };
    let by_token = |uri: String, token: &str| {
        let request = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        with_bearer(request, token)
    };
    let tree = format!("/api/v1/spaces/{space}/tree");
    let page = format!("/api/v1/pages/{deepest}");
    let unknown = format!("/api/v1/pages/{}", Uuid::now_v7());
    measure(
        &t,
        shape,
        pages,
        "arbre par session",
        StatusCode::OK,
        &|| by_session(tree.clone()),
    )
    .await;
    measure(
        &t,
        shape,
        pages,
        "arbre par jeton (tous les espaces)",
        StatusCode::OK,
        &|| by_token(tree.clone(), &all),
    )
    .await;
    measure(
        &t,
        shape,
        pages,
        "arbre par jeton (une racine)",
        StatusCode::OK,
        &|| by_token(tree.clone(), &scoped),
    )
    .await;
    measure(
        &t,
        shape,
        pages,
        "page la plus profonde par session",
        StatusCode::OK,
        &|| by_session(page.clone()),
    )
    .await;
    measure(
        &t,
        shape,
        pages,
        "page la plus profonde par jeton, dans la portée",
        StatusCode::OK,
        &|| by_token(page.clone(), &scoped),
    )
    .await;
    measure(
        &t,
        shape,
        pages,
        "page la plus profonde par jeton, hors de la portée",
        StatusCode::NOT_FOUND,
        &|| by_token(page.clone(), &elsewhere),
    )
    .await;
    measure(
        &t,
        shape,
        pages,
        "page inconnue par jeton",
        StatusCode::NOT_FOUND,
        &|| by_token(unknown.clone(), &elsewhere),
    )
    .await;
    measure(
        &t,
        shape,
        pages,
        "page inconnue par session",
        StatusCode::NOT_FOUND,
        &|| by_session(unknown.clone()),
    )
    .await;
    // La lecture d'un jeton seule : le hachage, le jeton et son compte.
    let hash_of =
        |token: &str| mementum_server::auth::token_hash(token.trim_start_matches("mmt_")).unwrap();
    let hash = hash_of(&scoped);
    let mut times = Vec::new();
    for _ in 0..200 {
        let started = Instant::now();
        let token = t.store.token_by_hash(hash, 0).await.unwrap().unwrap();
        t.store.get_user(token.user_id).await.unwrap().unwrap();
        times.push(started.elapsed());
    }
    let (p50, p95) = percentiles(times);
    println!(
        "MESURE jetons {shape} {pages} pages, lecture d'un jeton (token_by_hash, get_user) : \
         {p50:.3} / {p95:.3} ms (p50 / p95)"
    );
    t.stop().await;
}

/// Une route mesurée : une fois pour remplir les caches, puis `ROUNDS` fois.
async fn measure(
    t: &TestApp,
    shape: &str,
    pages: usize,
    name: &str,
    expect: StatusCode,
    make: impl Fn() -> Request<Body>,
) {
    const ROUNDS: usize = 40;
    assert_eq!(t.send(make()).await.status, expect, "{name}");
    let mut times = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let started = Instant::now();
        let reply = t.send(make()).await;
        times.push(started.elapsed());
        assert_eq!(reply.status, expect, "{name}");
    }
    let (p50, p95) = percentiles(times);
    println!("MESURE jetons {shape} {pages} pages, {name} : {p50:.3} / {p95:.3} ms (p50 / p95)");
}
