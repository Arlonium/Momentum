//! Linéarité de l'index plein texte (Rulings R380, R433, R920), et ses mesures (`#[ignore]`, en
//! release, une à la fois).
//!
//! L'aide de `mementum-md/tests/common/linear.rs` compte les allocations du fil qui lit. Ici,
//! c'est un fil de l'écrivain du moteur qui indexe, et un autre qui fusionne : l'allocateur de ce
//! binaire compte donc pour tout le processus, et tous ses tests passent un à la fois ([`ALONE`]).

#[path = "../../mementum-store/tests/common/tmp.rs"]
mod tmp;

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use mementum_search::{Options, PageSet, Query, SearchIndex, read_query};
use tmp::TestDir;
use uuid::Uuid;

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);

fn record(grown: isize, requested: usize) {
    let now = LIVE.fetch_add(grown, Ordering::Relaxed) + grown;
    PEAK.fetch_max(now, Ordering::Relaxed);
    TOTAL.fetch_add(requested, Ordering::Relaxed);
}

// SAFETY : chaque appel est transmis tel quel à l'allocateur du système ; le compte se fait dans
// des compteurs atomiques, sans allocation.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record(layout.size() as isize, layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        record(-(layout.size() as isize), 0);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(ptr, layout, new_size) };
        if !moved.is_null() {
            record(new_size as isize - layout.size() as isize, new_size);
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Un test à la fois : le compte vaut pour tout le processus.
static ALONE: Mutex<()> = Mutex::new(());

fn alone() -> std::sync::MutexGuard<'static, ()> {
    ALONE.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Copy)]
struct Cost {
    allocated: usize,
    peak: usize,
    time: Duration,
}

/// Ce que coûte un travail, tous fils compris : octets alloués, pointe du tas au-dessus de son
/// niveau de départ, temps.
fn cost<R>(work: impl FnOnce() -> R) -> (R, Cost) {
    let live = LIVE.load(Ordering::Relaxed);
    PEAK.store(live, Ordering::Relaxed);
    let total = TOTAL.load(Ordering::Relaxed);
    let start = Instant::now();
    let result = work();
    let cost = Cost {
        time: start.elapsed(),
        allocated: TOTAL.load(Ordering::Relaxed) - total,
        peak: (PEAK.load(Ordering::Relaxed) - live).max(0) as usize,
    };
    (result, cost)
}

/// Rapport des tailles mesurées, et rapport admis des mesures : linéaire, environ huit ;
/// quadratique, soixante-quatre.
const SCALE: usize = 8;
const BOUND: f64 = 16.0;
/// La petite mesure de temps ne compte jamais pour moins (Ruling R433).
const BASE_TIME: Duration = Duration::from_millis(20);

fn ratio(large: f64, small: f64) -> f64 {
    large / small.max(1.0)
}

fn page(n: usize) -> Uuid {
    Uuid::from_u128(0x0193_b2c4_5d6e_7f80_9a1b_0000_0000_0000 + n as u128)
}

fn pages(of: std::ops::Range<usize>) -> PageSet {
    of.map(page).collect()
}

fn query(raw: &str) -> Query {
    read_query(raw).expect("une requête sous les bornes")
}

// ---------------------------------------------------------------------------------------------
// Des textes.

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Un bloc hostile de 64 Kio (Rulings R920, R928) : des mots de `len` lettres tirées au hasard,
/// donc tous différents, dès leurs premières lettres (il n'y a que 676 débuts de deux lettres :
/// un mot donne en moyenne un peu moins de huit termes neufs aux débuts, pas neuf).
fn hostile(block: usize, len: usize) -> String {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ ((block as u64 + 1) << 32));
    let mut text = String::with_capacity(64 << 10);
    while text.len() + len < (64 << 10) {
        for _ in 0..len {
            text.push((b'a' + rng.below(26) as u8) as char);
        }
        text.push(' ');
    }
    text
}

const VOCABULARY: usize = 60_000;

/// Un mot d'un vocabulaire où quelques mots reviennent très souvent : le mot de rang zéro est
/// dans presque tous les blocs.
fn word(rng: &mut Rng, out: &mut String) {
    // La puissance quatrième d'un tirage uniforme : les petits rangs sont tirés bien plus
    // souvent (le rang zéro, six fois sur cent).
    let u = rng.below(1 << 20) as f64 / (1u64 << 20) as f64;
    let rank = (u * u * u * u * VOCABULARY as f64) as usize;
    write_word(rank, out);
}

fn write_word(rank: usize, out: &mut String) {
    const SYLLABLES: [&str; 16] = [
        "ba", "con", "dé", "fi", "gra", "lou", "men", "nor", "pré", "qui", "ré", "si", "tan",
        "ver", "zo", "é",
    ];
    let mut rest = rank + 16;
    while rest > 0 {
        out.push_str(SYLLABLES[rest % 16]);
        rest /= 16;
    }
}

/// Un texte de `words` mots du vocabulaire.
fn prose(rng: &mut Rng, words: usize) -> String {
    let mut text = String::new();
    for i in 0..words {
        if i > 0 {
            text.push(' ');
        }
        word(rng, &mut text);
    }
    text
}

/// Un espace de `pages` pages de `blocks` blocs d'une douzaine de mots, sous des titres de
/// trois à cinq mots ; rend les octets de texte reçus et ceux des titres recopiés.
fn corpus(index: &SearchIndex, pages: usize, blocks: usize) -> (u64, u64) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut received, mut titles) = (0, 0);
    for n in 0..pages {
        let title = prose(&mut rng, 3 + (n % 3));
        index.put_entry(page(n), None, &title, "").expect("écrire");
        received += title.len() as u64;
        titles += title.len() as u64;
        for b in 0..blocks {
            let text = prose(&mut rng, 8 + (b % 9));
            let block = Uuid::from_u128(rng.next() as u128 | ((n as u128) << 64)).to_string();
            index
                .put_entry(page(n), Some(&block), &title, &text)
                .expect("écrire");
            received += text.len() as u64;
            titles += title.len() as u64;
        }
        if n % 500 == 499 {
            index.commit("jeton").expect("valider");
        }
    }
    index.commit("jeton").expect("valider");
    (received, titles)
}

fn terms_written(index: &SearchIndex) -> u64 {
    [
        "title",
        "text",
        "title_starts",
        "text_starts",
        "node",
        "entry",
    ]
    .iter()
    .map(|field| index.terms_of(field).expect("compter"))
    .sum()
}

// ---------------------------------------------------------------------------------------------
// Linéarité.

/// Écrit `blocks` blocs hostiles dans un index neuf et valide : les octets reçus, les termes
/// écrits, le coût.
fn write_hostile(blocks: usize, len: usize) -> (usize, u64, Cost) {
    let dir = TestDir::new();
    let index = SearchIndex::open(dir.path()).expect("ouvrir").index;
    let texts: Vec<String> = (0..blocks).map(|block| hostile(block, len)).collect();
    let received = texts.iter().map(String::len).sum();
    let ((), cost) = cost(|| {
        for (n, text) in texts.iter().enumerate() {
            index
                .put_entry(page(n / 16), Some(&format!("b{n}")), "Notes", text)
                .expect("écrire");
        }
        index.commit("jeton").expect("valider");
    });
    let terms = terms_written(&index);
    (received, terms, cost)
}

#[test]
fn put_entry_grows_like_hostile_text() {
    let _alone = alone();
    let mut failures = Vec::new();
    for (what, len) in [("mots de 40 octets", 40), ("mots de 10 octets", 10)] {
        let (small_bytes, small_terms, small) = write_hostile(4, len);
        let (large_bytes, large_terms, large) = write_hostile(4 * SCALE, len);
        println!(
            "{what} : {small_bytes} puis {large_bytes} octets ; {small_terms} puis {large_terms} \
             termes ; {small:?} puis {large:?}"
        );
        assert_eq!(large_bytes, SCALE * small_bytes);
        for (measure, value) in [
            (
                "octets alloués",
                ratio(large.allocated as f64, small.allocated as f64),
            ),
            (
                "termes écrits",
                ratio(large_terms as f64, small_terms as f64),
            ),
            (
                "temps",
                ratio(
                    large.time.as_nanos() as f64,
                    small.time.max(BASE_TIME).as_nanos() as f64,
                ),
            ),
        ] {
            if value > BOUND {
                failures.push(format!("{what} : {measure} multiplié par {value:.1}"));
            }
        }
        // Les bornes absolues (Ruling R433) : la pointe du tas par octet reçu, et jamais plus
        // de termes que d'octets reçus (neuf débuts et le mot, pour onze octets au pire).
        if large.peak > 256 * large_bytes {
            failures.push(format!(
                "{what} : pointe de {} octets pour {large_bytes} octets reçus",
                large.peak
            ));
        }
        if large_terms as usize > large_bytes {
            failures.push(format!(
                "{what} : {large_terms} termes pour {large_bytes} octets reçus"
            ));
        }
        // Le témoin : les débuts sont bien écrits, de l'ordre de huit termes neufs par mot avec
        // le mot lui-même.
        let words = (large_bytes / (len + 1)) as u64;
        assert!(
            large_terms >= 7 * words,
            "{large_terms} termes, {words} mots"
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Un index de `entries` entrées, quatre par page, qui portent toutes les mêmes mots.
fn uniform(entries: usize) -> (TestDir, SearchIndex) {
    let dir = TestDir::new();
    let index = SearchIndex::open(dir.path()).expect("ouvrir").index;
    for n in 0..entries {
        index
            .put_entry(
                page(n / 4),
                Some(&format!("b{}", n % 4)),
                "Notes communes",
                "agir internationalement, sans relâche",
            )
            .expect("écrire");
    }
    index.commit("jeton").expect("valider");
    (dir, index)
}

#[test]
fn search_grows_like_the_index() {
    let _alone = alone();
    const BASE: usize = 4_000;
    let (_small_dir, small_index) = uniform(BASE);
    let (_large_dir, large_index) = uniform(SCALE * BASE);
    let mut failures = Vec::new();
    for (what, raw, allowed) in [
        (
            "un mot de toutes les entrées, toutes permises",
            "agir",
            true,
        ),
        ("un mot du titre et un début de mot", "communes inter", true),
        (
            "un préfixe long partagé par toutes les entrées",
            "sans internationalem",
            true,
        ),
        ("aucune entrée permise", "agir internationalem", false),
    ] {
        let run = |index: &SearchIndex, entries: usize| {
            let allowed = if allowed {
                pages(0..entries / 4)
            } else {
                pages(entries..entries + entries / 4)
            };
            let query = query(raw);
            // Le meilleur temps de trois ; les octets de la première.
            let (found, mut first) =
                cost(|| index.search(&query, &allowed, 0, 20).expect("chercher"));
            for _ in 0..2 {
                let (_, again) = cost(|| index.search(&query, &allowed, 0, 20).expect("chercher"));
                first.time = first.time.min(again.time);
            }
            (found, first)
        };
        let (found, small) = run(&small_index, BASE);
        let (_, large) = run(&large_index, SCALE * BASE);
        println!("{what} : {small:?} puis {large:?}");
        // Le témoin : la recherche trouve (ou, sans page permise, ne trouve rien).
        assert_eq!(found.pages.len(), if allowed { 20 } else { 0 }, "{what}");
        for (measure, value) in [
            (
                "octets alloués",
                ratio(large.allocated as f64, small.allocated as f64),
            ),
            ("pointe du tas", ratio(large.peak as f64, small.peak as f64)),
            (
                "temps",
                ratio(
                    large.time.as_nanos() as f64,
                    small.time.max(BASE_TIME).as_nanos() as f64,
                ),
            ),
        ] {
            if value > BOUND {
                failures.push(format!("{what} : {measure} multiplié par {value:.1}"));
            }
        }
        // La borne absolue : la pointe du tas par entrée de l'index.
        if large.peak > 256 * SCALE * BASE {
            failures.push(format!("{what} : pointe de {} octets", large.peak));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// ---------------------------------------------------------------------------------------------
// Mesures (`#[ignore]`, en release : `cargo test --release -p mementum-search --test
// linear_index -- --ignored --nocapture`). Leurs lignes `MESURE` vont au registre.

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1_000_000.0
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
        let path = entry.path();
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_dir() => total += dir_size(&path),
            Ok(meta) => total += meta.len(),
            Err(_) => {}
        }
    }
    total
}

/// (mémoire de travail, mémoire privée) du processus, d'après sysinfo (Ruling R924 : la seconde
/// n'est la mémoire privée que sous Windows).
fn process_memory() -> (u64, u64) {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let pid = Pid::from_u32(std::process::id());
    let mut system = System::new().expect("sysinfo");
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    let process = system.process(pid).expect("ce processus");
    (process.memory(), process.virtual_memory())
}

/// Relève la pointe de mémoire de travail du processus tant qu'il vit.
struct PeakWatch {
    stop: std::sync::Arc<AtomicBool>,
    peak: std::sync::Arc<AtomicU64>,
    thread: std::thread::JoinHandle<()>,
}

impl PeakWatch {
    fn start() -> PeakWatch {
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let peak = std::sync::Arc::new(AtomicU64::new(0));
        let (stopped, seen) = (stop.clone(), peak.clone());
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                seen.fetch_max(process_memory().0, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        PeakWatch { stop, peak, thread }
    }

    fn finish(self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.join().expect("le fil de relevé");
        self.peak.load(Ordering::Relaxed)
    }
}

fn print_usage(what: &str, index: &SearchIndex) -> u64 {
    let mut stored = 0;
    for (field, bytes) in index.usage().expect("tailles") {
        println!("MESURE {what} : {field} {:.2} Mo", mb(bytes));
        if field == "(gardés)" {
            stored = bytes;
        }
    }
    stored
}

const MEASURED_PAGES: usize = 5_000;
const MEASURED_BLOCKS: usize = 40;

#[test]
#[ignore = "mesure : en release, seule"]
fn measure_the_index_by_field() {
    let _alone = alone();
    let mut stored = Vec::new();
    for (what, keep_title, keep_text) in [
        ("titre et texte gardés", true, true),
        ("texte gardé seul", false, true),
        ("rien de gardé (repli de L18)", false, false),
    ] {
        let dir = TestDir::new();
        let options = Options {
            keep_title,
            keep_text,
            ..Options::default()
        };
        let index = SearchIndex::open_with(dir.path(), options)
            .expect("ouvrir")
            .index;
        let start = Instant::now();
        let (received, titles) = corpus(&index, MEASURED_PAGES, MEASURED_BLOCKS);
        let written = start.elapsed();
        let index = index.wait_for_merges().expect("fusions");
        println!(
            "MESURE {what} : {MEASURED_PAGES} pages, {} entrées, {:.1} Mo de texte reçu, {:.1} \
             Mo de titres recopiés ; écrit en {:.1} s, {} segments, dossier {:.1} Mo",
            MEASURED_PAGES * (MEASURED_BLOCKS + 1),
            mb(received),
            mb(titles),
            written.as_secs_f64(),
            index.segments(),
            mb(dir_size(dir.path())),
        );
        stored.push((print_usage(what, &index), titles));
    }
    let title = stored[0].0.saturating_sub(stored[1].0);
    println!(
        "MESURE titre gardé sur chaque entrée (R929) : {:.2} Mo, pour {:.2} Mo de titres bruts \
         recopiés ; textes gardés {:.2} Mo",
        mb(title),
        mb(stored[0].1),
        mb(stored[1].0),
    );
}

fn percentile(times: &mut [Duration], p: f64) -> Duration {
    times.sort();
    times[((times.len() as f64 * p).ceil() as usize).clamp(1, times.len()) - 1]
}

#[test]
#[ignore = "mesure : en release, seule"]
fn measure_a_common_last_word() {
    let _alone = alone();
    const PAGES: usize = 10_000;
    const BLOCKS: usize = 20;
    let dir = TestDir::new();
    let index = SearchIndex::open(dir.path()).expect("ouvrir").index;
    corpus(&index, PAGES, BLOCKS);
    let index = index.wait_for_merges().expect("fusions");
    let allowed = pages(0..PAGES);
    let ten = pages(0..10);
    // Le mot le plus courant du vocabulaire, et un mot rare.
    let (mut common, mut rare) = (String::new(), String::new());
    write_word(0, &mut common);
    write_word(40_000, &mut rare);
    let whole = |word: &str| Query {
        terms: vec![query(word).prefix.expect("un mot")],
        prefix: None,
    };
    let start: String = common.chars().take(2).collect();
    for (what, asked) in [
        ("mot très courant, entier seulement", whole(&common)),
        (
            "mot très courant, en dernier mot (entier ou début, R930)",
            query(&common),
        ),
        ("début de deux lettres très courant", query(&start)),
        (
            "mot rare puis mot très courant en dernier",
            query(&format!("{rare} {common}")),
        ),
        ("mot rare, en dernier mot", query(&rare)),
    ] {
        for (who, set) in [
            ("10 000 pages permises", &allowed),
            ("10 pages permises", &ten),
        ] {
            let mut times = Vec::new();
            let mut found = 0;
            for _ in 0..60 {
                let begun = Instant::now();
                let (hits, trace) = index
                    .search_traced(&asked, set, 0, 20, None)
                    .expect("chercher");
                times.push(begun.elapsed());
                found = trace.collected;
                assert!(hits.pages.len() <= 20);
            }
            println!(
                "MESURE dernier mot, {} entrées, {what}, {who} : {found} entrées lues ; p50 \
                 {:.2} ms, p95 {:.2} ms",
                PAGES * (BLOCKS + 1),
                percentile(&mut times, 0.5).as_secs_f64() * 1e3,
                percentile(&mut times, 0.95).as_secs_f64() * 1e3,
            );
        }
    }
}

#[test]
#[ignore = "mesure : en release, seule"]
fn measure_the_heap_of_hostile_blocks() {
    let _alone = alone();
    const BLOCKS: usize = 300;
    for len in [40, 10] {
        for max_docs in [None, Some(20)] {
            let what = format!(
                "{BLOCKS} blocs de 64 Kio de mots de {len} octets, {}",
                match max_docs {
                    None => "fusions par défaut".to_string(),
                    Some(max) => format!("aucun segment de plus de {max} entrées fusionné"),
                }
            );
            let dir = TestDir::new();
            let options = Options {
                max_docs_before_merge: max_docs,
                ..Options::default()
            };
            let before = process_memory();
            let watch = PeakWatch::start();
            let mut received = 0;
            let ((index, written), cost) = cost(|| {
                let index = SearchIndex::open_with(dir.path(), options)
                    .expect("ouvrir")
                    .index;
                let start = Instant::now();
                for block in 0..BLOCKS {
                    let text = hostile(block, len);
                    received += text.len() as u64;
                    index
                        .put_entry(page(block / 30), Some(&format!("b{block}")), "Notes", &text)
                        .expect("écrire");
                    // Par petits lots, comme l'indexeur : c'est ce qui fait fusionner.
                    if block % 2 == 1 {
                        index.commit("jeton").expect("valider");
                    }
                }
                let written = start.elapsed();
                (index.wait_for_merges().expect("fusions"), written)
            });
            let peak = watch.finish();
            println!(
                "MESURE {what} : {:.1} Mo reçus, écrits en {:.1} s ({:.2} Mo/s), fusions finies \
                 à {:.1} s ; pointe du tas {:.0} Mo, {:.0} Mo alloués ; mémoire de travail : \
                 {:.0} Mo avant, pointe {:.0} Mo ; {} segments, dossier {:.1} Mo ({:.2} fois le \
                 texte reçu)",
                mb(received),
                written.as_secs_f64(),
                mb(received) / written.as_secs_f64(),
                cost.time.as_secs_f64(),
                mb(cost.peak as u64),
                mb(cost.allocated as u64),
                mb(before.0),
                mb(peak),
                index.segments(),
                mb(dir_size(dir.path())),
                dir_size(dir.path()) as f64 / received as f64,
            );
            print_usage(&what, &index);
        }
    }
}

#[test]
#[ignore = "mesure : en release, seule"]
fn measure_a_resting_writer_of_50_mb() {
    let _alone = alone();
    for buffer in [15_000_000, 50_000_000] {
        let dir = TestDir::new();
        let live = LIVE.load(Ordering::Relaxed);
        let before = process_memory();
        let options = Options {
            buffer_bytes: buffer,
            ..Options::default()
        };
        let index = SearchIndex::open_with(dir.path(), options)
            .expect("ouvrir")
            .index;
        let opened = LIVE.load(Ordering::Relaxed) - live;
        let watch = PeakWatch::start();
        let ((), cost) = cost(|| {
            corpus(&index, 2_500, MEASURED_BLOCKS);
        });
        let index = index.wait_for_merges().expect("fusions");
        index.commit("jeton").expect("valider");
        let peak = watch.finish();
        std::thread::sleep(Duration::from_secs(2));
        let resting = LIVE.load(Ordering::Relaxed) - live;
        let after = process_memory();
        println!(
            "MESURE écrivain de {} Mo au repos : tas {:.2} Mo à l'ouverture, {:.2} Mo 2 s après \
             la dernière validation (pointe pendant l'écriture de {} entrées : {:.0} Mo) ; \
             mémoire de travail {:.0} Mo avant, {:.0} Mo au repos, pointe {:.0} Mo ; mémoire \
             privée {:.0} Mo avant, {:.0} Mo au repos",
            buffer / 1_000_000,
            mb(opened.max(0) as u64),
            mb(resting.max(0) as u64),
            2_500 * (MEASURED_BLOCKS + 1),
            mb(cost.peak as u64),
            mb(before.0),
            mb(after.0),
            mb(peak),
            mb(before.1),
            mb(after.1),
        );
    }
}
