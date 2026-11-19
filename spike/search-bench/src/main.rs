//! Banc jetable de la Task S0 du lot 4 (recherche) : Tantivy contre FTS5 de SQLite.
//! Rien d'ici n'entre dans le produit. Voir `README.md` pour les commandes.

mod corpus;
mod fts;
mod tv;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Compte les octets alloués par le processus (tas de Rust : ni les fichiers projetés en
/// mémoire, ni le tas de SQLite, qui passe par `malloc`).
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                let live = LIVE.fetch_add(new_size - layout.size(), Ordering::Relaxed) + new_size - layout.size();
                PEAK.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

pub fn heap_live() -> usize {
    LIVE.load(Ordering::Relaxed)
}
/// Remet la pointe au niveau courant, et rend ce niveau.
pub fn heap_reset_peak() -> usize {
    let live = LIVE.load(Ordering::Relaxed);
    PEAK.store(live, Ordering::Relaxed);
    live
}
pub fn heap_peak() -> usize {
    PEAK.load(Ordering::Relaxed)
}

pub fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1_000_000.0
}

/// (mémoire de travail, mémoire privée) du processus, en octets, d'après sysinfo : la première
/// est ce que lit la mesure du budget dans `mementum-cli`.
pub fn process_memory() -> (u64, u64) {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let pid = Pid::from_u32(std::process::id());
    let mut system = System::new().expect("sysinfo");
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    let p = system.process(pid).expect("ce processus");
    (p.memory(), p.virtual_memory())
}

pub fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(read) = std::fs::read_dir(path) {
        for e in read.flatten() {
            let p = e.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(m) = std::fs::metadata(&p) {
                total += m.len();
            }
        }
    }
    total
}

pub struct Stats {
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
    pub min: Duration,
    pub mean: Duration,
    pub sd: Duration,
}

pub fn stats(samples: &mut [Duration]) -> Stats {
    samples.sort();
    let n = samples.len();
    let at = |q: f64| samples[(((n as f64) * q).ceil() as usize).clamp(1, n) - 1];
    let mean = samples.iter().sum::<Duration>().as_secs_f64() / n as f64;
    let var = samples.iter().map(|d| (d.as_secs_f64() - mean).powi(2)).sum::<f64>() / n as f64;
    Stats {
        p50: at(0.5),
        p95: at(0.95),
        max: samples[n - 1],
        min: samples[0],
        mean: Duration::from_secs_f64(mean),
        sd: Duration::from_secs_f64(var.sqrt()),
    }
}

pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

pub struct Args(Vec<String>);

impl Args {
    pub fn flag(&self, name: &str) -> bool {
        self.0.iter().any(|a| a == name)
    }
    pub fn value(&self, name: &str) -> Option<&str> {
        self.0.iter().position(|a| a == name).and_then(|i| self.0.get(i + 1)).map(String::as_str)
    }
    pub fn scale(&self) -> corpus::Scale {
        corpus::Scale::of(self.value("--scale").map(|v| v.parse().expect("--scale")).unwrap_or(1.0))
    }
    pub fn pos(&self, i: usize) -> &str {
        self.0.get(i).map(String::as_str).unwrap_or_else(|| panic!("argument {i} manquant"))
    }
}

fn main() {
    let mut all: Vec<String> = std::env::args().skip(1).collect();
    if all.is_empty() {
        eprintln!("voir README.md");
        std::process::exit(2);
    }
    let cmd = all.remove(0);
    let args = Args(all);
    tv::set_budget(&args);
    match cmd.as_str() {
        "build" => tv::build(&args),
        "sizes" => tv::sizes(&args),
        "open" => tv::open(&args),
        "rest" => tv::rest(&args),
        "query" => tv::query(&args),
        "commits" => tv::commits(&args),
        "r916" => tv::r916(&args),
        "payload" => tv::payload(&args),
        "payload-child" => tv::payload_child(&args),
        "tokens" => tv::tokens(),
        "hostile" => tv::hostile(&args),
        "fts-check" => fts::check(),
        "fts-build" => fts::build(&args),
        "fts-query" => fts::query(&args),
        "fts-write" => fts::write(&args),
        "fts-r916" => fts::r916(&args),
        "fts-hostile" => fts::hostile(&args),
        other => panic!("commande inconnue : {other}"),
    }
}
