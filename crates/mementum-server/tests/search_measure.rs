//! Mesures de la recherche dans le serveur (lot 4, Task S5, Step 3) : seules dans leur binaire,
//! qui compte les octets alloués par tout le processus (une recherche tourne sur le fil du test,
//! mais le moteur a ses fils).
//!
//! En release, à la demande :
//! `cargo test --release -p mementum-server --test search_measure -- --ignored --nocapture`.
//! L'échelle se règle par `MEMENTUM_MEASURE_PAGES` (10 000 par défaut) et
//! `MEMENTUM_MEASURE_BLOCKS` (blocs par page, 10 par défaut).

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use common::{PASSWORD, Team, TestApp};
use mementum_acl::{Level, Subject};
use mementum_search::{PageSet, read_query};
use mementum_server::SignupMode;
use serde_json::json;
use uuid::Uuid;

/// L'allocateur du système, qui compte les octets tenus par tout le processus et leur pointe.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grown(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

// SAFETY : tout est délégué à l'allocateur du système ; seuls des compteurs s'y ajoutent.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grown(layout.size());
        // SAFETY : le contrat de l'appelant est celui de `System.alloc`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY : le contrat de l'appelant est celui de `System.dealloc`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        grown(new_size);
        // SAFETY : le contrat de l'appelant est celui de `System.realloc`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// La pointe des octets tenus pendant `work`, au-dessus de ce qui l'était à son début.
fn peak_during<T>(work: impl FnOnce() -> T) -> (T, usize) {
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let done = work();
    (done, PEAK.load(Ordering::Relaxed).saturating_sub(before))
}

fn scale(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// Quatre lettres pour un rang : un mot propre à une page, sans chiffre (un chiffre couperait
/// peut-être le mot).
fn letters(mut rank: usize) -> String {
    let mut word = [b'a'; 4];
    for letter in word.iter_mut().rev() {
        *letter = b'a' + (rank % 26) as u8;
        rank /= 26;
    }
    String::from_utf8(word.to_vec()).unwrap()
}

/// `(p50, p95)` en millisecondes.
fn percentiles(mut times: Vec<Duration>) -> (f64, f64) {
    times.sort();
    let at = |rank: usize| times[rank].as_secs_f64() * 1e3;
    (
        at(times.len() / 2),
        at((times.len() * 95).div_ceil(100) - 1),
    )
}

/// Mesure (Task S5, Step 3) : une recherche sur un espace de `MEMENTUM_MEASURE_PAGES` pages de
/// `MEMENTUM_MEASURE_BLOCKS` blocs, pour un membre qui voit tout et pour une invitée qui voit dix
/// pages ; un, trois et huit mots, un mot très courant seul (R930), un début de mot de plus de
/// dix caractères que toutes les entrées portent (R920 : temps et pointe de mémoire). Le calcul
/// de l'index est mesuré à part du plancher ; la route entière l'est une fois. Puis ce que
/// `behind` lit dans la base quand toutes les pages sont à l'outbox (une reconstruction), pour
/// leur espace et pour un autre (R996).
///
/// Les pages sont écrites d'un coup dans la base, serveur arrêté (`nodes`, `blocks_index`,
/// `search_state`), et l'index plein texte, retiré, est reconstruit au démarrage depuis
/// `blocks_index` (R1001).
#[tokio::test]
#[ignore = "mesure, en release : cargo test --release -p mementum-server --test search_measure \
            -- --ignored --nocapture"]
async fn measure_search_on_10000_pages() {
    const ROUNDS: usize = 40;
    let pages = scale("MEMENTUM_MEASURE_PAGES", 10_000);
    let blocks = scale("MEMENTUM_MEASURE_BLOCKS", 10);

    let t = TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await;
    let team = Team::new(&t).await;
    let root = team
        .page(None, "Racine", &[(Subject::Members, Level::Read)])
        .await;
    let invited = team
        .page(
            None,
            "Invitée",
            &[
                (Subject::Members, Level::Read),
                (Subject::User(team.gina.id), Level::Read),
            ],
        )
        .await;
    let (space, other_space) = (team.space, team.zoe.space);
    t.indexer.settled().await;
    team.store.close().await;
    drop(team);
    let (dir, cfg) = t.stop().await;

    // Les pages, d'un coup : les dix premières sous la page de l'invitée.
    let ids: Vec<Uuid> = (0..pages).map(|i| Uuid::from_u128(1 + i as u128)).collect();
    let (mut nodes, mut lines, mut states) = (Vec::new(), Vec::new(), Vec::new());
    let mut text_bytes = 0;
    for (i, id) in ids.iter().enumerate() {
        let parent = if i < 10 { invited } else { root };
        nodes.push(vec![
            id.to_string(),
            space.to_string(),
            parent.to_string(),
            format!("Page {}", letters(i)),
            id.to_string(),
        ]);
        states.push(vec![id.to_string()]);
        for j in 0..blocks {
            let text = format!(
                "commun mot{} terme{} interminable{} alpha beta gamma delta epsilon zeta eta",
                letters(i),
                letters(j),
                letters(i)
            );
            text_bytes += text.len();
            lines.push(vec![id.to_string(), format!("b{j}"), text]);
        }
    }
    let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", cfg.db_path().display()))
        .await
        .unwrap();
    let mut tx = side.begin().await.unwrap();
    for chunk in nodes.chunks(200) {
        let rows = vec!["(?, ?, ?, 'page', ?, ?, 0, 0)"; chunk.len()].join(", ");
        let sql = format!(
            "INSERT INTO nodes (id, space_id, parent_id, kind, title, position, created_at, \
             updated_at) VALUES {rows}"
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for value in chunk.iter().flatten() {
            query = query.bind(value.clone());
        }
        query.execute(&mut *tx).await.unwrap();
    }
    for chunk in lines.chunks(200) {
        let rows = vec!["(?, ?, 'paragraph', ?, x'00')"; chunk.len()].join(", ");
        let sql =
            format!("INSERT INTO blocks_index (node_id, block_id, type, text, hash) VALUES {rows}");
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for value in chunk.iter().flatten() {
            query = query.bind(value.clone());
        }
        query.execute(&mut *tx).await.unwrap();
    }
    for chunk in states.chunks(200) {
        let rows = vec!["(?, 0)"; chunk.len()].join(", ");
        let sql = format!("INSERT INTO search_state (node_id, indexed_seq) VALUES {rows}");
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for value in chunk.iter().flatten() {
            query = query.bind(value.clone());
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

    // L'index plein texte retiré : le démarrage le reconstruit depuis `blocks_index`.
    std::fs::remove_dir_all(cfg.data_dir.join("search")).unwrap();
    let rebuilt = Instant::now();
    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    let rebuilt = rebuilt.elapsed();

    let everything: PageSet = ids.iter().copied().chain([root, invited]).collect();
    let ten: PageSet = ids[..10].iter().copied().chain([invited]).collect();
    let rare = format!("mot{}", letters(pages / 2));
    let rare_seen = format!("mot{}", letters(3));
    let queries = [
        ("un mot rare", rare.clone(), rare_seen.clone()),
        (
            "trois mots",
            format!("commun termeaaaa {rare}"),
            format!("commun termeaaaa {rare_seen}"),
        ),
        (
            "huit mots que toutes les entrées portent",
            "commun alpha beta gamma delta epsilon zeta eta".to_string(),
            "commun alpha beta gamma delta epsilon zeta eta".to_string(),
        ),
        (
            "un mot très courant seul",
            "commun".to_string(),
            "commun".to_string(),
        ),
        (
            "un début de mot de treize caractères",
            "interminablea".to_string(),
            "interminablea".to_string(),
        ),
    ];
    let mut said = Vec::new();
    for (who, allowed) in [("membre", &everything), ("invitée", &ten)] {
        for (name, for_all, for_ten) in &queries {
            let words = if who == "membre" { for_all } else { for_ten };
            let query = read_query(words).unwrap();
            let mut times = Vec::with_capacity(ROUNDS);
            let mut found = 0;
            for _ in 0..ROUNDS {
                let started = Instant::now();
                let pages = t.indexer.search(&query, allowed, 0, 20).unwrap().pages;
                times.push(started.elapsed());
                found = pages.len();
            }
            assert!(found > 0, "{who}, {name} : rien de trouvé");
            let (_, peak) = peak_during(|| t.indexer.search(&query, allowed, 0, 20).unwrap());
            let (p50, p95) = percentiles(times);
            said.push(format!(
                "{who}, {name} : p50 {p50:.2} ms, p95 {p95:.2} ms, pointe {:.2} Mio, {found} pages",
                peak as f64 / (1 << 20) as f64
            ));
        }
    }

    // La route entière, plancher compris.
    let mona = t.browser();
    mona.log_in("mona@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let uri = format!("/api/v1/spaces/{space}/search");
    let mut times = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let started = Instant::now();
        let reply = mona.post(&uri, json!({ "q": "commun" })).await;
        times.push(started.elapsed());
        reply.assert_status(StatusCode::OK);
        assert_eq!(reply.json()["results"].as_array().unwrap().len(), 20);
    }
    let (p50, p95) = percentiles(times);
    said.push(format!(
        "la route, membre, un mot très courant, plancher compris : p50 {p50:.1} ms, p95 {p95:.1} ms"
    ));

    // `behind`, toutes les pages à l'outbox : ce que chaque recherche lit pendant une
    // reconstruction.
    t.indexer.freeze();
    let mut after = None;
    loop {
        after = t.store.outbox_everything(after, 1_000).await.unwrap();
        if after.is_none() {
            break;
        }
    }
    for (name, of) in [("leur espace", space), ("un autre espace", other_space)] {
        let mut times = Vec::with_capacity(ROUNDS);
        let mut late = 0;
        for _ in 0..ROUNDS {
            let started = Instant::now();
            late = t.store.behind_in_space(of, 1_000).await.unwrap().len();
            times.push(started.elapsed());
        }
        let (p50, p95) = percentiles(times);
        said.push(format!(
            "pages en retard lues pour {name}, toute l'instance à l'outbox : p50 {p50:.2} ms, \
             p95 {p95:.2} ms, {late} rendues"
        ));
    }

    // Ce qu'une recherche en paie, par l'indexeur (R1055) : il n'est plus au repos (une page
    // vient d'être créée par l'application, qu'il ne peut pas indexer, figé), et garde une
    // seconde ce que la base a rendu.
    mona.post(
        "/api/v1/pages",
        json!({ "spaceId": space.to_string(), "title": "Neuve" }),
    )
    .await
    .assert_status(StatusCode::CREATED);
    while !t.indexer.behind(space, |_| true).await.unwrap() {
        tokio::task::yield_now().await;
    }
    for (name, of) in [("leur espace", space), ("un autre espace", other_space)] {
        let reads = t.indexer.behind_reads();
        let mut times = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let started = Instant::now();
            t.indexer.behind(of, |_| false).await.unwrap();
            times.push(started.elapsed());
        }
        let worst = times.iter().max().unwrap().as_secs_f64() * 1e3;
        let (p50, p95) = percentiles(times);
        said.push(format!(
            "« en retard ? » demandé à l'indexeur pour {name}, {ROUNDS} fois de suite : p50 \
             {p50:.3} ms, p95 {p95:.3} ms, la plus longue {worst:.2} ms, {} lectures de la base",
            t.indexer.behind_reads() - reads
        ));
    }

    println!(
        "MESURE measure_search_on_10000_pages : {pages} pages de {blocks} blocs ({:.1} Mio de \
         texte), index reconstruit en {:.1} s ;\n  {}",
        text_bytes as f64 / (1 << 20) as f64,
        rebuilt.as_secs_f64(),
        said.join(" ;\n  ")
    );
}
