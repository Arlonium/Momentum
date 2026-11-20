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

/// Ce que le système compte au processus : sa mémoire de travail (RSS sous Linux), et ce qu'il
/// tient en propre (Ruling R924) : `RssAnon` de `/proc/<pid>/status` sous Linux, la mémoire
/// privée sous Windows (`virtual_memory()` de sysinfo y est `PrivateUsage` ; sous Linux elle est
/// la taille virtuelle, qui ne juge rien). Ailleurs, la seconde est inconnue.
#[derive(Clone, Copy, Debug, Default)]
struct Memory {
    rss: u64,
    own: Option<u64>,
}

fn memory_of(pid: u32) -> Memory {
    let pid = Pid::from_u32(pid);
    let mut system = System::new().expect("sysinfo");
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    let Some(process) = system.process(pid) else {
        return Memory::default();
    };
    let own = if cfg!(windows) {
        Some(process.virtual_memory())
    } else if cfg!(target_os = "linux") {
        std::fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()
            .and_then(|status| {
                let line = status.lines().find(|line| line.starts_with("RssAnon:"))?;
                let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
                Some(kib * 1024)
            })
    } else {
        None
    };
    Memory {
        rss: process.memory(),
        own,
    }
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1e6
}

fn own_mb(memory: Memory) -> String {
    memory
        .own
        .map_or("inconnue".to_string(), |own| format!("{:.1}", mb(own)))
}

fn scale(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// Quatre lettres pour un rang : un mot propre à une page, sans chiffre.
fn letters(mut rank: usize) -> String {
    let mut word = [b'a'; 4];
    for letter in word.iter_mut().rev() {
        *letter = b'a' + (rank % 26) as u8;
        rank /= 26;
    }
    String::from_utf8(word.to_vec()).unwrap()
}

/// Une recherche du compte, par la route : `(statut, corps lu)`.
fn search(server: &Server, cookie: &str, space: &str, words: &str) -> (u16, serde_json::Value) {
    let body = serde_json::json!({ "q": words }).to_string();
    let reply = server.request(
        "POST",
        &format!("/api/v1/spaces/{space}/search"),
        Some(&body),
        &[("Cookie", cookie)],
    );
    let read = serde_json::from_str(&reply.body).unwrap_or(serde_json::Value::Null);
    (reply.status, read)
}

/// Lot 4, Task X2 (Rulings R924, R1160) : le démarrage et la mémoire du binaire **avec un index
/// plein texte à jour** de `MEMENTUM_MEASURE_PAGES` pages (10 000 par défaut) de
/// `MEMENTUM_MEASURE_BLOCKS` blocs (10), l'écrivain et le lecteur tenus.
///
/// Les pages sont écrites d'un coup dans la base, serveur arrêté ; l'index, retiré, est
/// reconstruit par le serveur (la pointe de cette reconstruction est relevée, sans budget), puis
/// le serveur est tué. Sont jugés ensuite : le démarrage jusqu'à `/health` (meilleur de trois,
/// < 1 s), la mémoire de travail 2 s après le lancement (meilleur de trois, < 150 Mo : la mesure
/// existante, sur ce dossier), et, après 500 recherches, la mémoire que le processus tient en
/// propre (< 150 Mo). La mémoire de travail d'après les recherches compte les fichiers de l'index
/// projetés en mémoire : elle est dite, sans budget (limite L-r).
#[test]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
fn cold_start_and_memory_with_an_index_of_10000_pages() {
    use sqlx::{Connection, SqliteConnection};
    release_only();
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    const SEARCHES: usize = 500;
    let pages = scale("MEMENTUM_MEASURE_PAGES", 10_000);
    let blocks = scale("MEMENTUM_MEASURE_BLOCKS", 10);
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");

    // Un compte, son espace, une page à lui sous laquelle vont toutes les autres.
    let server = Server::start(&data);
    let account =
        r#"{"email":"alice@exemple.fr","name":"Alice","password":"un mot de passe assez long"}"#;
    let signup = server.request("POST", "/api/v1/auth/signup", Some(account), &[]);
    assert_eq!(signup.status, 201, "{}", signup.body);
    let cookie = signup.session_cookie().expect("cookie de session");
    let read: serde_json::Value = serde_json::from_str(&signup.body).unwrap();
    let space = read["spaces"][0]["id"].as_str().unwrap().to_string();
    let body = serde_json::json!({ "spaceId": space, "title": "Racine" }).to_string();
    let reply = server.request("POST", "/api/v1/pages", Some(&body), &[("Cookie", &cookie)]);
    assert_eq!(reply.status, 201, "{}", reply.body);
    let read: serde_json::Value = serde_json::from_str(&reply.body).unwrap();
    let root = read["id"].as_str().unwrap().to_string();
    // La page créée est indexée avant l'arrêt : sa ligne d'état ne doit pas rester à -1.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (status, found) = search(&server, &cookie, &space, "racine");
        assert_eq!(status, 200, "{found}");
        if found["indexing"] == false && found["results"].as_array().is_some_and(|r| r.len() == 1) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "la page créée n'est jamais trouvée"
        );
        thread::sleep(Duration::from_millis(100));
    }
    server.kill();

    // Les pages, d'un coup, serveur arrêté ; puis l'index retiré, que le démarrage reconstruit
    // depuis `blocks_index` (Ruling R1001).
    let mut text_bytes = 0usize;
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let url = format!("sqlite:{}", data.join("mementum.db").display());
        let mut conn = SqliteConnection::connect(&url).await.unwrap();
        let mut tx = conn.begin().await.unwrap();
        let ids: Vec<String> = (0..pages)
            .map(|i| uuid::Uuid::from_u128(1 + i as u128).to_string())
            .collect();
        for (chunk_rank, chunk) in ids.chunks(200).enumerate() {
            let rows = vec!["(?, ?, ?, 'page', ?, ?, 0, 0)"; chunk.len()].join(", ");
            let sql = format!(
                "INSERT INTO nodes (id, space_id, parent_id, kind, title, position, created_at, \
                 updated_at) VALUES {rows}"
            );
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
            for (i, id) in chunk.iter().enumerate() {
                query = query
                    .bind(id.clone())
                    .bind(space.clone())
                    .bind(root.clone())
                    .bind(format!("Page {}", letters(chunk_rank * 200 + i)))
                    .bind(id.clone());
            }
            query.execute(&mut *tx).await.unwrap();
            let rows = vec!["(?, 0)"; chunk.len()].join(", ");
            let sql = format!("INSERT INTO search_state (node_id, indexed_seq) VALUES {rows}");
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
            for id in chunk {
                query = query.bind(id.clone());
            }
            query.execute(&mut *tx).await.unwrap();
        }
        let mut lines = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            for j in 0..blocks {
                let text = format!(
                    "commun mot{} terme{} alpha beta gamma delta epsilon zeta eta",
                    letters(i),
                    letters(j)
                );
                text_bytes += text.len();
                lines.push((id.clone(), format!("b{j}"), text));
            }
        }
        for chunk in lines.chunks(200) {
            let rows = vec!["(?, ?, 'paragraph', ?, x'00')"; chunk.len()].join(", ");
            let sql = format!(
                "INSERT INTO blocks_index (node_id, block_id, type, text, hash) VALUES {rows}"
            );
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
            for (id, block, text) in chunk {
                query = query
                    .bind(id.clone())
                    .bind(block.clone())
                    .bind(text.clone());
            }
            query.execute(&mut *tx).await.unwrap();
        }
        sqlx::query("UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = ?")
            .bind(space.clone())
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        conn.close().await.unwrap();
    });
    std::fs::remove_dir_all(data.join("search")).unwrap();

    // La reconstruction : sa durée, et la pointe de mémoire du processus pendant elle.
    let last_word = format!("mot{}", letters(pages - 1));
    let rebuilding = Instant::now();
    let server = Server::start(&data);
    let mut peak = Memory::default();
    let deadline = Instant::now() + Duration::from_secs(1_800);
    loop {
        let now = memory_of(server.pid());
        peak.rss = peak.rss.max(now.rss);
        peak.own = peak.own.max(now.own);
        let (status, found) = search(&server, &cookie, &space, &last_word);
        if status == 200
            && found["indexing"] == false
            && found["results"].as_array().is_some_and(|r| !r.is_empty())
        {
            break;
        }
        assert!(Instant::now() < deadline, "l'index n'est jamais à jour");
        thread::sleep(Duration::from_millis(100));
    }
    let rebuilt = rebuilding.elapsed();
    // Deux secondes de repos : la dernière clôture est passée.
    thread::sleep(Duration::from_secs(2));
    let after_rebuild = memory_of(server.pid());
    server.kill();
    let index_bytes: u64 = std::fs::read_dir(data.join("search"))
        .unwrap()
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum();
    println!(
        "MESURE index de {pages} pages de {blocks} blocs ({:.1} Mo de texte, index de {:.1} Mo) \
         reconstruit par le serveur en {rebuilt:?} : pointe de la mémoire de travail {:.1} Mo, de \
         la mémoire en propre {} Mo ; deux secondes après, {:.1} Mo et {} Mo",
        mb(text_bytes as u64),
        mb(index_bytes),
        mb(peak.rss),
        own_mb(peak),
        mb(after_rebuild.rss),
        own_mb(after_rebuild),
    );

    // Le démarrage et le repos, sur ce dossier.
    let (mut times, mut idle) = (Vec::new(), Vec::new());
    for _ in 0..LAUNCHES {
        let start = Instant::now();
        let server = Server::start(&data);
        let (status, body) = server.http("GET", "/health", None);
        let elapsed = start.elapsed();
        assert_eq!((status, body.as_str()), (200, "ok"));
        times.push(elapsed);
        thread::sleep(Duration::from_secs(2).saturating_sub(start.elapsed()));
        idle.push(memory_of(server.pid()));
        // L'index est celui de la base : rien n'est reconstruit, la recherche répond à jour.
        let (status, found) = search(&server, &cookie, &space, &last_word);
        assert_eq!(status, 200, "{found}");
        assert_eq!(found["indexing"], false, "l'index a été reconstruit");
        assert_eq!(found["results"].as_array().map(Vec::len), Some(1));
        server.kill();
    }
    let best = *times.iter().min().unwrap();
    let best_idle = idle.iter().map(|memory| memory.rss).min().unwrap();
    println!(
        "MESURE démarrage avec l'index jusqu'à /health : {times:?}, meilleur {best:?} ; au repos \
         (2 s après le lancement), mémoire de travail {:?} Mo, en propre {:?} Mo",
        idle.iter()
            .map(|m| format!("{:.1}", mb(m.rss)))
            .collect::<Vec<_>>(),
        idle.iter().map(|m| own_mb(*m)).collect::<Vec<_>>(),
    );

    // Après usage : 500 recherches, d'un mot rare, d'un mot courant, de trois mots.
    let server = Server::start(&data);
    let searching = Instant::now();
    for i in 0..SEARCHES {
        let words = match i % 3 {
            0 => format!("mot{}", letters(i * 7919 % pages)),
            1 => "commun".to_string(),
            _ => format!("commun terme{} alpha", letters(i % blocks)),
        };
        let (status, found) = search(&server, &cookie, &space, &words);
        assert_eq!(status, 200, "{found}");
        assert!(found["results"].as_array().is_some_and(|r| !r.is_empty()));
    }
    let used = memory_of(server.pid());
    println!(
        "MESURE après {SEARCHES} recherches ({:?}) : mémoire de travail {:.1} Mo (fichiers de \
         l'index projetés compris), en propre {} Mo",
        searching.elapsed(),
        mb(used.rss),
        own_mb(used),
    );
    server.kill();

    assert!(
        best < COLD_START,
        "démarrage avec l'index : {best:?} au mieux (budget {COLD_START:?}), mesures {times:?}"
    );
    assert!(
        best_idle < IDLE_RSS,
        "mémoire de travail au repos avec l'index : {best_idle} octets au mieux (budget {IDLE_RSS})"
    );
    for (when, memory) in [
        ("après les recherches", used),
        ("après la reconstruction", after_rebuild),
    ] {
        if let Some(own) = memory.own {
            assert!(
                own < IDLE_RSS,
                "mémoire tenue en propre {when} : {own} octets (budget {IDLE_RSS})"
            );
        }
    }
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
