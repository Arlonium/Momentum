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

/// `(p50, p95, la pire)` en millisecondes.
fn percentiles(mut times: Vec<Duration>) -> (f64, f64, f64) {
    times.sort();
    let at = |rank: usize| times[rank].as_secs_f64() * 1e3;
    (
        at(times.len() / 2),
        at((times.len() * 95).div_ceil(100) - 1),
        at(times.len() - 1),
    )
}

/// Les octets d'un dossier, sous-dossiers compris.
fn dir_bytes(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_bytes(&entry.path()),
            _ => std::fs::metadata(entry.path()).map_or(0, |meta| meta.len()),
        })
        .sum()
}

/// Les octets de la base : son fichier et son journal.
fn database_bytes(db: &std::path::Path) -> u64 {
    ["", "-wal"]
        .iter()
        .map(|suffix| {
            let mut name = db.file_name().unwrap().to_os_string();
            name.push(suffix);
            std::fs::metadata(db.with_file_name(name)).map_or(0, |meta| meta.len())
        })
        .sum()
}

/// La mémoire de travail de ce processus, en octets.
fn working_set() -> u64 {
    let pid = sysinfo::Pid::from_u32(std::process::id());
    let mut system = sysinfo::System::new().expect("sysinfo");
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).map_or(0, |process| process.memory())
}

/// Les mesures de ce binaire passent une à la fois (Ruling R341).
static ALONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release : cargo test --release -p mementum-server --test search_measure \
            measure_search_on_10000_pages -- --ignored --nocapture"]
async fn measure_search_on_10000_pages() {
    let _alone = ALONE.lock().await;
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
    // L'auteur d'une page sur deux, pour le filtre « auteur » (lot 5, Task S7, Ruling R1248).
    let author = team.olga.id;
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
            if i % 2 == 0 {
                author.to_string()
            } else {
                String::new()
            },
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
    let empty_database = database_bytes(&cfg.db_path());
    let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", cfg.db_path().display()))
        .await
        .unwrap();
    let mut tx = side.begin().await.unwrap();
    for chunk in nodes.chunks(200) {
        let rows = vec!["(?, ?, ?, 'page', ?, ?, NULLIF(?, ''), 0, 0)"; chunk.len()].join(", ");
        let sql = format!(
            "INSERT INTO nodes (id, space_id, parent_id, kind, title, position, created_by, \
             created_at, updated_at) VALUES {rows}"
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
    // La date du contenu de chaque page (`page_edits`, R1246) : la page i est datée de i.
    for (i, chunk) in states.chunks(200).enumerate() {
        let rows = vec!["(?, ?, NULL, 0)"; chunk.len()].join(", ");
        let sql = format!(
            "INSERT INTO page_edits (node_id, edited_at, edited_by, edited_seq) VALUES {rows}"
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for (j, value) in chunk.iter().flatten().enumerate() {
            query = query.bind(value.clone()).bind((i * 200 + j) as i64);
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
    // La part de `blocks_index` dans la base (L18) : la table et sa clé primaire.
    let table_bytes: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(pgsize), 0) FROM dbstat WHERE name LIKE '%blocks_index%'",
    )
    .fetch_one(&side)
    .await
    .unwrap_or(-1);
    side.close().await;
    let full_database = database_bytes(&cfg.db_path());
    let data_dir = cfg.data_dir.clone();
    // Ce que le test tenait pour écrire la base ne compte pas dans la pointe du tas.
    drop((nodes, lines, states));
    let before_rebuild = LIVE.load(Ordering::Relaxed);
    PEAK.store(before_rebuild, Ordering::Relaxed);

    // L'index plein texte retiré : le démarrage le reconstruit depuis `blocks_index`.
    std::fs::remove_dir_all(cfg.data_dir.join("search")).unwrap();
    let rebuilt = Instant::now();
    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    let rebuilt = rebuilt.elapsed();
    let rebuilt_peak = PEAK.load(Ordering::Relaxed).saturating_sub(before_rebuild);
    let index_bytes = dir_bytes(&data_dir.join("search"));

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
            let (p50, p95, worst) = percentiles(times);
            said.push(format!(
                "{who}, {name} : p50 {p50:.2} ms, p95 {p95:.2} ms, la pire {worst:.2} ms, pointe \
                 {:.2} Mio, {found} pages",
                peak as f64 / (1 << 20) as f64
            ));
        }
    }

    // Quatre calculs de front (le plus que l'instance en mène, R910), chacun sur son fil : la
    // recherche de huit mots, et celle d'un mot rare.
    let eight = "commun alpha beta gamma delta epsilon zeta eta";
    for (name, words) in [("huit mots", eight), ("un mot rare", rare.as_str())] {
        let query = read_query(words).unwrap();
        let (times, peak) = peak_during(|| {
            std::thread::scope(|scope| {
                let threads: Vec<_> = (0..4)
                    .map(|_| {
                        scope.spawn(|| {
                            let mut times = Vec::with_capacity(ROUNDS);
                            for _ in 0..ROUNDS {
                                let started = Instant::now();
                                t.indexer.search(&query, &everything, 0, 20).unwrap();
                                times.push(started.elapsed());
                            }
                            times
                        })
                    })
                    .collect();
                threads
                    .into_iter()
                    .flat_map(|thread| thread.join().unwrap())
                    .collect::<Vec<_>>()
            })
        });
        let (p50, p95, worst) = percentiles(times);
        said.push(format!(
            "membre, {name}, quatre calculs de front : p50 {p50:.2} ms, p95 {p95:.2} ms, la pire \
             {worst:.2} ms, pointe des quatre {:.1} Mio",
            peak as f64 / (1 << 20) as f64
        ));
    }

    // La route entière, plancher compris.
    let mona = t.browser();
    mona.log_in("mona@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let uri = format!("/api/v1/spaces/{space}/search");
    let mut route_p95 = Vec::new();
    for (name, words) in [
        ("un mot rare", rare.as_str()),
        ("un mot très courant", "commun"),
        ("huit mots", eight),
    ] {
        let mut times = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let started = Instant::now();
            let reply = mona.post(&uri, json!({ "q": words })).await;
            times.push(started.elapsed());
            reply.assert_status(StatusCode::OK);
            assert!(!reply.json()["results"].as_array().unwrap().is_empty());
        }
        let (p50, p95, worst) = percentiles(times);
        route_p95.push((name, p95));
        said.push(format!(
            "la route, membre, {name}, plancher compris : p50 {p50:.1} ms, p95 {p95:.1} ms, la \
             pire {worst:.1} ms"
        ));
    }

    // Les filtres de la recherche (lot 5, Task S7, Ruling R1248) : ils réduisent l'ensemble des
    // pages permises, d'après `nodes` et `page_edits`, avant l'index. Par la route, plancher
    // compris : à 100 000 pages (`MEMENTUM_MEASURE_PAGES=100000`), la mesure que demande le plan
    // (`measure_search_filter_on_100000_pages`).
    let half = (pages / 2) as i64;
    for (name, body) in [
        (
            "filtre auteur",
            json!({ "q": "commun", "createdBy": author.to_string() }),
        ),
        ("filtre date", json!({ "q": "commun", "editedAfter": half })),
        (
            "filtre auteur et date",
            json!({ "q": "commun", "createdBy": author.to_string(), "editedBefore": half }),
        ),
    ] {
        let mut times = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let started = Instant::now();
            let reply = mona.post(&uri, body.clone()).await;
            times.push(started.elapsed());
            reply.assert_status(StatusCode::OK);
            assert!(
                !reply.json()["results"].as_array().unwrap().is_empty(),
                "{name}"
            );
        }
        let (p50, p95, worst) = percentiles(times);
        said.push(format!(
            "la route, membre, {name}, plancher compris : p50 {p50:.1} ms, p95 {p95:.1} ms, la \
             pire {worst:.1} ms"
        ));
    }
    let filter = Instant::now();
    let passing = t
        .store
        .pages_matching(space, Some(author), Some(half), None)
        .await
        .unwrap()
        .len();
    said.push(format!(
        "la lecture des pages qui passent les filtres auteur et date, seule : {:.1} ms, {passing} \
         pages",
        filter.elapsed().as_secs_f64() * 1000.0
    ));

    // Pendant que tout l'index est récrit : chaque page repasse par l'outbox, l'indexeur enchaîne
    // les passages (R1029), le moteur valide et fusionne. La recherche lit l'index entier pendant
    // ce temps.
    if scale("MEMENTUM_MEASURE_REFEED", 1) == 1 {
        let refeeding = Instant::now();
        let mut after = None;
        loop {
            after = t.store.outbox_everything(after, 1_000).await.unwrap();
            if after.is_none() {
                break;
            }
        }
        let settling = tokio::spawn({
            let indexer = t.indexer.clone();
            async move { indexer.settled().await }
        });
        let queries = [
            ("un mot rare", read_query(&rare).unwrap()),
            ("huit mots", read_query(eight).unwrap()),
        ];
        let mut times = [Vec::new(), Vec::new()];
        let mut route = Vec::new();
        let (mut peak_set, mut rounds) = (0, 0usize);
        while !settling.is_finished() {
            for (at, (_, query)) in queries.iter().enumerate() {
                let started = Instant::now();
                let found = t.indexer.search(query, &everything, 0, 20);
                // L'index lâché le temps d'une réouverture ne rend rien : ce tour n'est pas compté.
                if found.is_ok_and(|found| !found.pages.is_empty()) {
                    times[at].push(started.elapsed());
                }
            }
            let started = Instant::now();
            let reply = mona.post(&uri, json!({ "q": eight })).await;
            reply.assert_status(StatusCode::OK);
            route.push(started.elapsed());
            rounds += 1;
            if rounds % 20 == 0 {
                peak_set = peak_set.max(working_set());
            }
        }
        settling.await.unwrap();
        let refed = refeeding.elapsed();
        for ((name, _), times) in queries.iter().zip(times) {
            let count = times.len();
            let (p50, p95, worst) = percentiles(times);
            said.push(format!(
                "pendant que tout l'index est récrit ({:.0} s), membre, {name} : {count} \
                 recherches, p50 {p50:.2} ms, p95 {p95:.2} ms, la pire {worst:.2} ms",
                refed.as_secs_f64()
            ));
        }
        let (p50, p95, worst) = percentiles(route);
        said.push(format!(
            "pendant que tout l'index est récrit, la route, huit mots : p50 {p50:.1} ms, p95 \
             {p95:.1} ms, la pire {worst:.1} ms ; mémoire de travail du processus {:.0} Mo au plus",
            peak_set as f64 / 1e6
        ));
    }

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
        let (p50, p95, _) = percentiles(times);
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
        let (p50, p95, _) = percentiles(times);
        said.push(format!(
            "« en retard ? » demandé à l'indexeur pour {name}, {ROUNDS} fois de suite : p50 \
             {p50:.3} ms, p95 {p95:.3} ms, la plus longue {worst:.2} ms, {} lectures de la base",
            t.indexer.behind_reads() - reads
        ));
    }

    println!(
        "MESURE measure_search_on_10000_pages : {pages} pages de {blocks} blocs ({:.1} Mio de \
         texte), index reconstruit en {:.1} s (pointe du tas du processus {:.0} Mio, index de \
         {:.1} Mio) ; la base passe de {:.1} à {:.1} Mio, dont `blocks_index` {:.1} Mio ;\n  {}",
        text_bytes as f64 / (1 << 20) as f64,
        rebuilt.as_secs_f64(),
        rebuilt_peak as f64 / (1 << 20) as f64,
        index_bytes as f64 / (1 << 20) as f64,
        empty_database as f64 / (1 << 20) as f64,
        full_database as f64 / (1 << 20) as f64,
        table_bytes as f64 / (1 << 20) as f64,
        said.join(" ;\n  ")
    );
    // Lot 4, Task X2 (Ruling R1163) : « une recherche répond en moins de 100 ms au p95 » (§5.10),
    // par la route entière, à l'échelle de référence (10 000 pages, 100 000 blocs) ; à une autre
    // échelle, la ligne `MESURE` dit ce qu'il en est, sans rien affirmer.
    if pages == 10_000 && blocks == 10 {
        for (name, p95) in route_p95 {
            assert!(
                p95 < 100.0,
                "la route, {name} : p95 {p95:.1} ms (budget 100 ms)"
            );
        }
    }
}

/// Un bloc de 64 Kio de mots de dix lettres tous différents, d'un bloc à l'autre aussi : le pire
/// cas du champ des débuts de mot (neuf débuts pour onze octets, Ruling R928).
fn hostile_block(rank: usize) -> String {
    const WORDS: usize = (64 << 10) / 11;
    let mut text = String::with_capacity(WORDS * 11);
    for word in 0..WORDS {
        // Le rang brouillé : sans cela les mots partageraient leurs premières lettres, donc
        // leurs débuts.
        let mut n =
            ((rank * WORDS + word) as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) % 26u64.pow(10);
        let mut letters = [b'a'; 10];
        for letter in letters.iter_mut().rev() {
            *letter = b'a' + (n % 26) as u8;
            n /= 26;
        }
        text.push_str(std::str::from_utf8(&letters).unwrap());
        text.push(' ');
    }
    text.pop();
    text
}

/// Lot 4, Task X2 (risque ouvert RO-1, Ruling R928) : ce que coûte **au serveur** l'indexation de
/// pages de mots courts tous différents : `MEMENTUM_MEASURE_HOSTILE_MB` Mio de texte (20 par
/// défaut), en pages de seize blocs de 64 Kio. Les lignes sont écrites d'un coup dans la base,
/// serveur arrêté ; l'index plein texte, retiré, est reconstruit par l'indexeur (passages dos à
/// dos, validations, fusions). Sont relevés : la durée, la pointe du tas du processus (tout ce
/// que le serveur alloue), la pointe de sa mémoire de travail (fichiers de segments projetés
/// compris), et la taille de l'index, jusqu'à la fin des fusions.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release : cargo test --release -p mementum-server --test search_measure \
            measure_indexing_hostile_pages -- --ignored --nocapture"]
async fn measure_indexing_hostile_pages() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64};
    let _alone = ALONE.lock().await;
    const BLOCKS: usize = 16;
    let megabytes = scale("MEMENTUM_MEASURE_HOSTILE_MB", 20);
    let pages = megabytes;

    let t = TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await;
    let team = Team::new(&t).await;
    let root = team
        .page(None, "Racine", &[(Subject::Members, Level::Read)])
        .await;
    let space = team.space;
    t.indexer.settled().await;
    team.store.close().await;
    drop(team);
    let (dir, cfg) = t.stop().await;

    let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", cfg.db_path().display()))
        .await
        .unwrap();
    let mut tx = side.begin().await.unwrap();
    let mut text_bytes = 0;
    for page in 0..pages {
        let id = Uuid::from_u128(1 + page as u128).to_string();
        sqlx::query(
            "INSERT INTO nodes (id, space_id, parent_id, kind, title, position, created_at, \
             updated_at) VALUES (?, ?, ?, 'page', 'Hostile', ?, 0, 0)",
        )
        .bind(&id)
        .bind(space.to_string())
        .bind(root.to_string())
        .bind(&id)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("INSERT INTO search_state (node_id, indexed_seq) VALUES (?, 0)")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .unwrap();
        for block in 0..BLOCKS {
            let text = hostile_block(page * BLOCKS + block);
            text_bytes += text.len();
            sqlx::query(
                "INSERT INTO blocks_index (node_id, block_id, type, text, hash) VALUES (?, ?, \
                 'paragraph', ?, x'00')",
            )
            .bind(&id)
            .bind(format!("b{block}"))
            .bind(text)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
    }
    sqlx::query("UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = ?")
        .bind(space.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    side.close().await;
    let data_dir = cfg.data_dir.clone();
    std::fs::remove_dir_all(data_dir.join("search")).unwrap();

    // Un fil relève la mémoire de travail et la taille de l'index pendant tout le travail.
    let (stop, peak_set, peak_dir) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    );
    let set_before = working_set();
    let watch = std::thread::spawn({
        let (stop, peak_set, peak_dir) = (stop.clone(), peak_set.clone(), peak_dir.clone());
        let search = data_dir.join("search");
        move || {
            while !stop.load(Ordering::Relaxed) {
                peak_set.fetch_max(working_set(), Ordering::Relaxed);
                peak_dir.fetch_max(dir_bytes(&search), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    });
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let started = Instant::now();
    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    let indexed = started.elapsed();
    let peak_indexing = PEAK.load(Ordering::Relaxed).saturating_sub(before);
    let set_indexing = peak_set.load(Ordering::Relaxed);
    // Les fusions continuent en fond : la taille du dossier ne bouge plus quand elles ont fini.
    let mut last = (dir_bytes(&data_dir.join("search")), Instant::now());
    while last.1.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let now = dir_bytes(&data_dir.join("search"));
        if now != last.0 {
            last = (now, Instant::now());
        }
    }
    let merged = started.elapsed() - Duration::from_secs(5);
    stop.store(true, Ordering::Relaxed);
    watch.join().unwrap();
    let mib = |bytes: u64| bytes as f64 / (1 << 20) as f64;
    println!(
        "MESURE measure_indexing_hostile_pages : {pages} pages de {BLOCKS} blocs de 64 Kio de \
         mots de dix lettres tous différents ({:.1} Mio de texte) indexées par le serveur en \
         {:.1} s ({:.2} Mio/s), fusions finies à {:.1} s ; pointe du tas {:.0} Mio pendant \
         l'indexation, {:.0} Mio jusqu'à la fin des fusions ; mémoire de travail {:.0} Mio \
         avant, pointe {:.0} Mio pendant l'indexation, {:.0} Mio jusqu'à la fin ; index {:.1} Mio \
         à la fin ({:.2} fois le texte), {:.1} Mio au plus",
        mib(text_bytes as u64),
        indexed.as_secs_f64(),
        mib(text_bytes as u64) / indexed.as_secs_f64(),
        merged.as_secs_f64(),
        mib(peak_indexing as u64),
        mib(PEAK.load(Ordering::Relaxed).saturating_sub(before) as u64),
        mib(set_before),
        mib(set_indexing),
        mib(peak_set.load(Ordering::Relaxed)),
        mib(last.0),
        last.0 as f64 / text_bytes as f64,
        mib(peak_dir.load(Ordering::Relaxed)),
    );
}
