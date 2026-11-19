//! FTS5 de SQLite, sur le même corpus : le repli de la décision L1.
//!
//! `libsqlite3-sys` est épinglé à la version du serveur (0.37.0, celle du `Cargo.lock` de la
//! racine), compilé par la même fonction `bundled`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{Connection, params};

use crate::corpus::{self, Rng, Vocab};
use crate::{Args, mb, ms, stats};

/// FTS5 est-il dans le SQLite que sqlx compile pour le serveur ? Par une requête, avec sqlx
/// et les mêmes fonctions que le `Cargo.toml` de la racine.
pub fn check() {
    use sqlx::Connection;
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    rt.block_on(async {
        let mut c = sqlx::sqlite::SqliteConnection::connect("sqlite::memory:").await.expect("connexion");
        let version: String = sqlx::query_scalar("SELECT sqlite_version()").fetch_one(&mut c).await.unwrap();
        let options: Vec<String> = sqlx::query_scalar("SELECT compile_options FROM pragma_compile_options")
            .fetch_all(&mut c)
            .await
            .unwrap();
        sqlx::query("CREATE VIRTUAL TABLE t USING fts5(x, tokenize='unicode61 remove_diacritics 2')")
            .execute(&mut c)
            .await
            .expect("FTS5 absent du SQLite de sqlx");
        sqlx::query("INSERT INTO t(x) VALUES ('Un élève de l''école'), ('autre chose')").execute(&mut c).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM t WHERE t MATCH 'eleve'").fetch_one(&mut c).await.unwrap();
        println!(
            "FTS5 par sqlx : SQLite {version} ; options de compilation qui nomment FTS : {:?} ; table fts5 créée ; « eleve » trouve {n} ligne (attendu 1)",
            options.iter().filter(|o| o.contains("FTS")).collect::<Vec<_>>()
        );
        assert_eq!(n, 1);
    });
}

fn connect(path: &Path) -> Connection {
    let c = Connection::open(path).expect("ouvrir la base");
    // Les réglages du store : WAL, `synchronous=FULL`.
    c.pragma_update(None, "journal_mode", "WAL").unwrap();
    c.pragma_update(None, "synchronous", "FULL").unwrap();
    // Par défaut le cache de SQLite (2 Mo par connexion), comme le serveur ; `MEMENTUM_BENCH_CACHE_MB`
    // pour voir ce qu'un plus grand cache change.
    if let Ok(v) = std::env::var("MEMENTUM_BENCH_CACHE_MB") {
        let kib: i64 = v.parse::<i64>().expect("MEMENTUM_BENCH_CACHE_MB") * 1024;
        c.pragma_update(None, "cache_size", -kib).unwrap();
    }
    c
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn checkpoint(c: &Connection) {
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
}

fn block_uuid(page: u32, block: u32) -> String {
    if block == 0 {
        return String::new();
    }
    let mut r = Rng(((page as u64) << 32) | block as u64);
    let (a, b) = (r.next(), r.next());
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", a >> 32, (a >> 16) & 0xffff, a & 0xffff, b >> 48, b & 0xffff_ffff_ffff)
}

const BI: &str = "CREATE TABLE bi (
    id INTEGER PRIMARY KEY,
    node_id TEXT NOT NULL,
    block_id TEXT NOT NULL,
    type TEXT NOT NULL,
    text TEXT NOT NULL,
    hash BLOB NOT NULL,
    UNIQUE (node_id, block_id)
)";

fn fts_ddl(prefix: Option<&str>) -> String {
    let p = prefix.map(|p| format!(", prefix='{p}'")).unwrap_or_default();
    format!(
        "CREATE VIRTUAL TABLE fts USING fts5(text, content='bi', content_rowid='id', tokenize='unicode61 remove_diacritics 2'{p})"
    )
}

pub fn build(args: &Args) {
    let dir = PathBuf::from(args.pos(0));
    let scale = args.scale();
    let prefix = args.value("--prefix");
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    let vocab = Vocab::new(60_000);

    // 1. La table du plan, telle quelle (`WITHOUT ROWID`), pour sa taille (décision L18).
    let plan_path = dir.join("plan.db");
    let c = connect(&plan_path);
    c.execute_batch(
        "CREATE TABLE blocks_index (
            node_id TEXT NOT NULL, block_id TEXT NOT NULL, type TEXT NOT NULL, text TEXT NOT NULL, hash BLOB NOT NULL,
            PRIMARY KEY (node_id, block_id)) WITHOUT ROWID;",
    )
    .unwrap();
    let (mut n, mut bytes) = (0u64, 0u64);
    let start = Instant::now();
    c.execute_batch("BEGIN").unwrap();
    corpus::stream(&scale, &vocab, |e| {
        if e.block == 0 {
            return; // le titre est dans `nodes`
        }
        bytes += e.text.len() as u64;
        n += 1;
        let hash = corpus::page_id(e.page ^ e.block).to_le_bytes();
        c.prepare_cached("INSERT INTO blocks_index VALUES (?1, ?2, 'paragraph', ?3, ?4)")
            .unwrap()
            .execute(params![uuid_of(e.page), block_uuid(e.page, e.block), e.text, &hash[..]])
            .unwrap();
        if n % 20_000 == 0 {
            c.execute_batch("COMMIT; BEGIN").unwrap();
        }
    });
    c.execute_batch("COMMIT").unwrap();
    checkpoint(&c);
    println!(
        "MESURE blocks_index (table du plan, WITHOUT ROWID) : {n} blocs, {:.1} Mo de texte, écrits en {:.1} s ; fichier {:.1} Mo, soit {:.2} fois le texte",
        mb(bytes),
        start.elapsed().as_secs_f64(),
        mb(file_size(&plan_path)),
        file_size(&plan_path) as f64 / bytes as f64
    );
    drop(c);

    // 2. La même table avec un `rowid` (une table à contenu externe de FTS5 en demande un),
    //    les titres en lignes à part (`block_id = ''`), puis l'index FTS5.
    let path = dir.join("fts.db");
    let c = connect(&path);
    c.execute_batch(BI).unwrap();
    let (mut n, mut bytes) = (0u64, 0u64);
    let start = Instant::now();
    c.execute_batch("BEGIN").unwrap();
    corpus::stream(&scale, &vocab, |e| {
        let text = if e.block == 0 { e.title } else { e.text };
        bytes += text.len() as u64;
        n += 1;
        let hash = corpus::page_id(e.page ^ e.block).to_le_bytes();
        c.prepare_cached("INSERT INTO bi (node_id, block_id, type, text, hash) VALUES (?1, ?2, 'paragraph', ?3, ?4)")
            .unwrap()
            .execute(params![uuid_of(e.page), block_uuid(e.page, e.block), text, &hash[..]])
            .unwrap();
        if n % 20_000 == 0 {
            c.execute_batch("COMMIT; BEGIN").unwrap();
        }
    });
    c.execute_batch("COMMIT").unwrap();
    checkpoint(&c);
    let table_time = start.elapsed();
    let table_size = file_size(&path);
    c.execute_batch(&fts_ddl(prefix)).unwrap();
    let start = Instant::now();
    // Par tranches, comme le ferait un indexeur : jamais une transaction de toute la base.
    let max: i64 = c.query_row("SELECT max(id) FROM bi", [], |r| r.get(0)).unwrap();
    let mut from = 0i64;
    let mut slices = Vec::new();
    while from < max {
        let t = Instant::now();
        c.execute("INSERT INTO fts (rowid, text) SELECT id, text FROM bi WHERE id > ?1 AND id <= ?2", params![from, from + 20_000]).unwrap();
        slices.push(t.elapsed());
        from += 20_000;
    }
    checkpoint(&c);
    let fts_time = start.elapsed();
    let s = stats(&mut slices);
    println!(
        "MESURE FTS5 construction préfixes={prefix:?} : {n} entrées, {:.1} Mo de texte ; table seule {:.1} Mo en {:.1} s ; \
         index FTS5 {:.1} Mo de plus en {:.1} s ({:.0} entrées/s ; une tranche de 20 000 lignes : p50 {:.0} ms, max {:.0} ms) ; \
         fichier {:.1} Mo, soit {:.2} fois le texte",
        mb(bytes),
        mb(table_size),
        table_time.as_secs_f64(),
        mb(file_size(&path) - table_size),
        fts_time.as_secs_f64(),
        n as f64 / fts_time.as_secs_f64(),
        ms(s.p50),
        ms(s.max),
        mb(file_size(&path)),
        file_size(&path) as f64 / bytes as f64
    );
}

fn uuid_of(page: u32) -> String {
    let h = corpus::page_hex(page);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn match_of(terms: &[&str], prefix: Option<&str>) -> String {
    let mut m = String::new();
    for t in terms {
        m.push_str(&format!("\"{t}\" "));
    }
    if let Some(p) = prefix {
        m.push_str(&format!("\"{p}\"*"));
    }
    m.trim_end().to_string()
}

fn fill_allowed(c: &Connection, name: &str, pages: impl Iterator<Item = u32>) {
    c.execute_batch(&format!("CREATE TEMP TABLE {name} (node_id TEXT PRIMARY KEY) WITHOUT ROWID; BEGIN")).unwrap();
    for p in pages {
        c.prepare_cached(&format!("INSERT INTO {name} VALUES (?1)")).unwrap().execute(params![uuid_of(p)]).unwrap();
    }
    c.execute_batch("COMMIT").unwrap();
}

/// La meilleure ligne de chaque page permise (note `bm25` de FTS5), vingt pages, leur extrait.
fn grouped(c: &Connection, allowed: &str, m: &str, ranked: bool) -> (usize, usize) {
    let note = if ranked { "min(f.rank)" } else { "min(b.id)" };
    let sql = format!(
        "SELECT b.node_id, b.id, {note} AS r FROM fts f JOIN bi b ON b.id = f.rowid JOIN {allowed} a ON a.node_id = b.node_id \
         WHERE fts MATCH ?1 GROUP BY b.node_id ORDER BY r LIMIT 20"
    );
    let mut st = c.prepare_cached(&sql).unwrap();
    let ids: Vec<i64> = st.query_map(params![m], |r| r.get(1)).unwrap().map(Result::unwrap).collect();
    let mut chars = 0;
    for id in &ids {
        let x: String = c
            .prepare_cached("SELECT substr(text, 1, 240) FROM bi WHERE id = ?1")
            .unwrap()
            .query_row(params![id], |r| r.get(0))
            .unwrap();
        chars += x.len();
    }
    (ids.len(), chars)
}

fn count_matches(c: &Connection, m: &str) -> i64 {
    c.prepare_cached("SELECT count(*) FROM fts WHERE fts MATCH ?1").unwrap().query_row(params![m], |r| r.get(0)).unwrap()
}

fn timed(iters: usize, mut run: impl FnMut()) -> crate::Stats {
    for _ in 0..2 {
        run();
    }
    let mut samples = Vec::new();
    for _ in 0..iters {
        let t = Instant::now();
        run();
        samples.push(t.elapsed());
        if samples.len() >= 5 && samples.iter().sum::<Duration>() > Duration::from_secs(20) {
            break;
        }
    }
    stats(&mut samples)
}

pub fn query(args: &Args) {
    let path = PathBuf::from(args.pos(0)).join("fts.db");
    let scale = args.scale();
    let iters: usize = args.value("--iters").map(|v| v.parse().unwrap()).unwrap_or(100);
    let c = connect(&path);
    let vocab = Vocab::new(60_000);
    let w = |rank: usize| vocab.words[rank].as_str();
    fill_allowed(&c, "a10", 1..=10);
    fill_allowed(&c, "a10k", 0..scale.big);
    fill_allowed(&c, "a100k", 0..scale.pages_a());
    let eight: Vec<&str> = (0..8).map(w).collect();
    let asks: Vec<(&str, String)> = vec![
        ("1 terme rare (rang 20000)", match_of(&[w(20000)], None)),
        ("1 terme moyen (rang 300)", match_of(&[w(300)], None)),
        ("1 terme très courant (rang 0)", match_of(&[w(0)], None)),
        ("titre « projet » (lignes de titre seulement)", match_of(&["projet"], None)),
        ("3 termes (rangs 5, 20, 60)", match_of(&[w(5), w(20), w(60)], None)),
        ("3 termes dont un préfixe « pr »", match_of(&[w(5), w(20)], Some("pr"))),
        ("8 termes (rangs 0 à 7)", match_of(&eight, None)),
        ("8 termes dont un préfixe « pro »", match_of(&eight[..7], Some("pro"))),
        ("préfixe seul « pr »", match_of(&[], Some("pr"))),
    ];
    for (name, m) in &asks {
        let n = count_matches(&c, m);
        for (fname, table) in [("10 pages", "a10"), ("10 000 pages", "a10k"), ("100 000 pages", "a100k")] {
            let mut found = 0;
            let s = timed(iters, || found = grouped(&c, table, m, true).0);
            println!(
                "MESURE FTS5 requête | {name} | {fname} | groupé par page, note bm25 | {found} pages, {n} lignes touchées | p50 {:.2} ms | p95 {:.2} ms | max {:.2} ms",
                ms(s.p50),
                ms(s.p95),
                ms(s.max)
            );
        }
        let s = timed(iters, || {
            grouped(&c, "a10k", m, false);
        });
        println!(
            "MESURE FTS5 requête | {name} | 10 000 pages | groupé par page, sans note | {n} lignes touchées | p50 {:.2} ms | p95 {:.2} ms | max {:.2} ms",
            ms(s.p50),
            ms(s.p95),
            ms(s.max)
        );
    }
    // Le canal par le temps (question d).
    let n = 2000;
    let (zero, cent) = (match_of(&["canalzero"], None), match_of(&["canalcent"], None));
    let (mut tz, mut tc) = (Vec::new(), Vec::new());
    for _ in 0..n {
        let t = Instant::now();
        grouped(&c, "a10k", &zero, true);
        tz.push(t.elapsed());
        let t = Instant::now();
        grouped(&c, "a10k", &cent, true);
        tc.push(t.elapsed());
        if tc.iter().sum::<Duration>() > Duration::from_secs(60) {
            break;
        }
    }
    let (sz, sc) = (stats(&mut tz), stats(&mut tc));
    println!(
        "MESURE FTS5 canal | 10 000 pages | 0 ligne non permise : p50 {:.3} ms, écart type {:.3} ms | 100 000 non permises : p50 {:.3} ms, écart type {:.3} ms | écart des p50 {:.3} ms",
        ms(sz.p50),
        ms(sz.sd),
        ms(sc.p50),
        ms(sc.sd),
        ms(sc.p50) - ms(sz.p50)
    );
}

/// Ce que FTS5 coûterait à la frappe des autres : la durée d'une transaction d'index dans le
/// tour d'écriture de SQLite (`BEGIN IMMEDIATE` … `COMMIT`, `synchronous=FULL`), avec et sans
/// l'index plein texte.
pub fn write(args: &Args) {
    let path = PathBuf::from(args.pos(0)).join("fts.db");
    let c = connect(&path);
    let vocab = Vocab::new(60_000);
    // Une graine neuve à chaque passage : récrire le même texte ne changerait aucune page.
    let mut rng = Rng(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64);
    let max: i64 = c.query_row("SELECT max(id) FROM bi", [], |r| r.get(0)).unwrap();
    let before = file_size(&path);
    let sentence = |rng: &mut Rng| {
        let mut t = String::new();
        for _ in 0..22 {
            t.push_str(vocab.pick(rng));
            t.push(' ');
        }
        t
    };
    for (n, reps) in [(1usize, 40), (100, 20), (1000, 10), (10_000, 3)] {
        let (mut with, mut without) = (Vec::new(), Vec::new());
        for rep in 0..reps * 2 {
            let fts = rep % 2 == 0;
            // Des blocs voisins (une même page, comme une réindexation), tirés au hasard.
            let first = 1 + rng.below((max - n as i64 - 1) as u64) as i64;
            let fresh: Vec<String> = (0..n).map(|_| sentence(&mut rng)).collect();
            let t = Instant::now();
            c.execute_batch("BEGIN IMMEDIATE").unwrap();
            for (i, new) in fresh.iter().enumerate() {
                let id = first + i as i64;
                if fts {
                    let old: String =
                        c.prepare_cached("SELECT text FROM bi WHERE id = ?1").unwrap().query_row(params![id], |r| r.get(0)).unwrap();
                    c.prepare_cached("INSERT INTO fts (fts, rowid, text) VALUES ('delete', ?1, ?2)").unwrap().execute(params![id, old]).unwrap();
                }
                c.prepare_cached("UPDATE bi SET text = ?2, hash = ?3 WHERE id = ?1")
                    .unwrap()
                    .execute(params![id, new, &(id as u128).to_le_bytes()[..]])
                    .unwrap();
                if fts {
                    c.prepare_cached("INSERT INTO fts (rowid, text) VALUES (?1, ?2)").unwrap().execute(params![id, new]).unwrap();
                }
            }
            c.execute_batch("COMMIT").unwrap();
            if fts { with.push(t.elapsed()) } else { without.push(t.elapsed()) }
        }
        let (sw, so) = (stats(&mut with), stats(&mut without));
        println!(
            "MESURE FTS5 écriture | {n} blocs dans une transaction | avec FTS5 : p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms | \
             blocks_index seul : p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms | {reps} passages chacun",
            ms(sw.p50),
            ms(sw.p95),
            ms(sw.max),
            ms(so.p50),
            ms(so.p95),
            ms(so.max)
        );
    }
    checkpoint(&c);
    println!("MESURE FTS5 écriture : fichier {:.1} Mo -> {:.1} Mo", mb(before), mb(file_size(&path)));
}

/// R916 chez FTS5 : sa note `bm25` lit le nombre de lignes, la longueur moyenne et le nombre
/// de lignes qui portent le terme, toute la table comprise.
pub fn r916(args: &Args) {
    let root = PathBuf::from(args.pos(0));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    let (base, variants) = crate::tv::dosed();
    let make = |name: &str, rows: &[crate::tv::Row]| -> Connection {
        let c = connect(&root.join(name));
        c.execute_batch(BI).unwrap();
        c.execute_batch(&fts_ddl(None)).unwrap();
        c.execute_batch("BEGIN").unwrap();
        for (_, page, block, text) in rows {
            c.execute(
                "INSERT INTO bi (node_id, block_id, type, text, hash) VALUES (?1, ?2, 'paragraph', ?3, x'00')",
                params![uuid_of(*page), block_uuid(*page, *block), text],
            )
            .unwrap();
        }
        c.execute_batch("INSERT INTO fts (rowid, text) SELECT id, text FROM bi; COMMIT").unwrap();
        fill_allowed(&c, "ok", 1..40);
        c
    };
    let run = |c: &Connection, m: &str| -> Vec<String> {
        let mut st = c
            .prepare(
                "SELECT b.node_id, min(f.rank) AS r FROM fts f JOIN bi b ON b.id = f.rowid JOIN ok a ON a.node_id = b.node_id \
                 WHERE fts MATCH ?1 GROUP BY b.node_id ORDER BY r, b.node_id",
            )
            .unwrap();
        st.query_map(params![m], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    let x = make("x.db", &base);
    let mut differs = 0;
    let mut total = 0;
    for (i, (vname, rows, gone)) in variants.iter().enumerate() {
        if !gone.is_empty() {
            continue; // FTS5 supprime pour de bon : pas d'entrée « supprimée, pas encore fusionnée »
        }
        let y = make(&format!("y{i}.db"), rows);
        for (qname, m) in [("dosea", "\"dosea\""), ("doseb", "\"doseb\""), ("dosea doseb", "\"dosea\" \"doseb\""), ("dos*", "\"dos\"*")] {
            let same = run(&x, m) == run(&y, m);
            total += 1;
            if !same {
                differs += 1;
            }
            println!("R916 FTS5 | {vname} | « {qname} » | note bm25 de FTS5 : même ordre = {same}");
        }
    }
    println!("R916 FTS5 verdict : la note bm25 de FTS5 change l'ordre des pages permises dans {differs} cas sur {total}");
}

/// La forme hostile chez FTS5.
pub fn hostile(args: &Args) {
    let dir = PathBuf::from(args.pos(0));
    let blocks: u32 = args.value("--blocks").map(|v| v.parse().unwrap()).unwrap_or(1000);
    let prefix = args.value("--prefix");
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hostile.db");
    let c = connect(&path);
    c.execute_batch(BI).unwrap();
    c.execute_batch(&fts_ddl(prefix)).unwrap();
    let mut rng = Rng(5);
    let mut bytes = 0u64;
    let mut text = String::with_capacity(65_536);
    let start = Instant::now();
    let mut worst = Duration::ZERO;
    for b in 0..blocks {
        text.clear();
        while text.len() + 41 <= 65_536 {
            for _ in 0..40 {
                text.push((b'a' + rng.below(26) as u8) as char);
            }
            text.push(' ');
        }
        bytes += text.len() as u64;
        let t = Instant::now();
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        c.execute(
            "INSERT INTO bi (node_id, block_id, type, text, hash) VALUES (?1, ?2, 'paragraph', ?3, x'00')",
            params![uuid_of(b / 128), block_uuid(b / 128, 1 + b % 128), text],
        )
        .unwrap();
        let id = c.last_insert_rowid();
        c.execute("INSERT INTO fts (rowid, text) VALUES (?1, ?2)", params![id, text]).unwrap();
        c.execute_batch("COMMIT").unwrap();
        worst = worst.max(t.elapsed());
    }
    checkpoint(&c);
    println!(
        "MESURE FTS5 hostile préfixes={prefix:?} : {blocks} blocs de 64 Kio, {:.1} Mo reçus, écrits en {:.1} s (un bloc par transaction, la pire {:.0} ms) ; \
         fichier {:.1} Mo, soit {:.2} fois les octets reçus (le texte y est une fois, dans la table)",
        mb(bytes),
        start.elapsed().as_secs_f64(),
        ms(worst),
        mb(file_size(&path)),
        file_size(&path) as f64 / bytes as f64
    );
}
