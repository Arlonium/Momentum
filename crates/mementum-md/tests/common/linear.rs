//! Tests de linéarité (Rulings R380 à R382, R433) : un allocateur qui compte, fil par fil, ce
//! qu'alloue une lecture, et la comparaison d'une lecture à la taille de base et à huit fois cette
//! taille.
//!
//! Partagé par les tests de `mementum-md` et de `mementum-crdt` (inclus par son chemin). Une
//! lecture linéaire fait environ huit fois plus de travail à huit fois la taille ; une lecture
//! quadratique, soixante-quatre fois. Trois mesures, chacune bornée à [`BOUND`] :
//! - les octets alloués pendant la lecture, et le pic de mémoire vive : comptés par l'allocateur
//!   du binaire de test, pour le seul fil qui lit, ils ne dépendent ni du poste ni de sa charge ;
//! - le temps, le meilleur de trois lectures, pour un travail qui ne passerait pas par
//!   l'allocateur ; la plus petite des deux mesures n'y compte jamais pour moins de
//!   [`BASE_TIME`] : une lecture de base de quelques microsecondes ne mesure que le bruit du poste
//!   (Ruling R433).
//!
//! Aucune de ces mesures ne peut passer à tort : la taille de base est choisie pour que sa lecture
//! prenne au moins [`BASE_TIME`] et que l'entrée pèse au moins [`BASE_SIZE`] (ou que la lecture
//! atteigne [`MAX_BASE_PEAK`] de mémoire), si bien que les frais fixes n'y cachent rien : une
//! forme quadratique y dépasserait la borne d'au moins quatre fois. Le temps peut échouer à tort
//! sur un poste dont la charge change d'un coup pendant une mesure ; les deux mesures
//! d'allocation, elles, ne bougent pas d'un passage à l'autre.
//!
//! Une croissance linéaire ne dit pas tout (Ruling R433) : une lecture qui prendrait mille fois
//! son entrée en mémoire grandirait comme elle. Deux bornes absolues s'y ajoutent, à la grande
//! taille ([`Limits`]) : le pic de mémoire par octet d'entrée, et la taille de la réponse (le MMD
//! rendu, ou le message d'erreur que le serveur renverrait).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Allocateur qui compte, pour le fil qui alloue, les octets demandés et le pic de mémoire vive.
pub struct Counting;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
    static TOTAL: Cell<usize> = const { Cell::new(0) };
}

fn record(grown: isize, requested: usize) {
    let _ = LIVE.try_with(|live| {
        let now = live.get() + grown;
        live.set(now);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
    });
    let _ = TOTAL.try_with(|total| total.set(total.get() + requested));
}

// SAFETY : chaque appel est transmis tel quel à l'allocateur du système ; le compte se fait dans
// des cellules propres au fil, sans allocation (initialisation constante, sans destructeur).
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

/// Les mesures passent une à la fois (libtest lance les tests de front, et le temps de l'un
/// compterait dans celui de l'autre).
pub static ALONE: Mutex<()> = Mutex::new(());

/// Temps de lecture minimal de la taille de base.
pub const BASE_TIME: Duration = Duration::from_millis(20);
/// Taille minimale de l'entrée de base (octets pour un MMD, octets d'état Yjs pour une page).
pub const BASE_SIZE: usize = 32 << 10;
/// Taille maximale de l'entrée de base : la grande en fait huit fois plus.
pub const MAX_BASE_SIZE: usize = 512 << 10;
/// Pic de mémoire au-delà duquel la taille de base n'est plus doublée : une forme quadratique en
/// mémoire en prendrait soixante-quatre fois plus à la grande taille (512 Mio), et le test doit
/// pouvoir échouer sans épuiser le poste.
pub const MAX_BASE_PEAK: usize = 8 << 20;
/// Au-delà de ce temps, une lecture n'est pas répétée pour en garder la meilleure.
const LONG_READ: Duration = Duration::from_secs(2);
/// Rapport des tailles mesurées.
pub const SCALE: usize = 8;
/// Rapport des mesures admis : linéaire, environ [`SCALE`] ; quadratique, son carré.
pub const BOUND: f64 = 16.0;

/// Ce que coûte une lecture.
#[derive(Debug, Clone, Copy)]
pub struct Cost {
    pub allocated: usize,
    pub peak: usize,
    pub time: Duration,
    /// La taille de sa réponse (voir [`grows_like_input`]).
    pub answer: usize,
}

/// Une lecture, son résultat gardé jusqu'au relevé : octets alloués, pic et temps.
pub fn once<R>(read: impl FnOnce() -> R) -> Cost {
    measure(read, |_| 0)
}

/// Comme [`once`], avec la taille de la réponse, relevée avant que le résultat soit lâché.
fn measure<R>(read: impl FnOnce() -> R, answer: impl Fn(&R) -> usize) -> Cost {
    let live = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(live));
    let total = TOTAL.with(Cell::get);
    let start = Instant::now();
    let result = read();
    let time = start.elapsed();
    let allocated = TOTAL.with(Cell::get) - total;
    let peak = (PEAK.with(Cell::get) - live).max(0) as usize;
    let answer = answer(&result);
    drop(result);
    Cost {
        allocated,
        peak,
        time,
        answer,
    }
}

/// Le coût d'une lecture : octets alloués et pic de la première, meilleur temps de trois (une
/// seule si elle dépasse [`LONG_READ`]). Pour les sondes des relecteurs.
#[allow(dead_code)]
pub fn cost<R>(read: impl Fn() -> R) -> Cost {
    cost_answering(read, |_| 0)
}

/// Comme [`cost`], avec la taille de la réponse de la première lecture.
fn cost_answering<R>(read: impl Fn() -> R, answer: impl Fn(&R) -> usize) -> Cost {
    let mut cost = measure(&read, &answer);
    for _ in 0..2 {
        if cost.time >= LONG_READ {
            break;
        }
        cost.time = cost.time.min(once(&read).time);
    }
    cost
}

/// `large / small`, la petite mesure (octets, nanosecondes) jamais sous un.
fn ratio(large: f64, small: f64) -> f64 {
    large / small.max(1.0)
}

/// La borne de la taille d'une réponse, à la grande taille (Ruling R433). Chaque binaire de test
/// qui inclut ce fichier n'en prend qu'une sorte.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum Answer {
    /// Au plus tant d'octets, quelle que soit l'entrée (un message d'erreur).
    Absolute(usize),
    /// Au plus tant d'octets par octet d'entrée (le MMD d'une page).
    PerByte(usize),
}

/// Les bornes absolues d'une lecture, à la grande taille (Ruling R433).
#[derive(Clone, Copy)]
pub struct Limits {
    /// Pic de mémoire par octet d'entrée.
    pub peak_per_byte: usize,
    /// Taille de la réponse.
    pub answer: Answer,
}

/// Une forme d'entrée : de taille proportionnelle à `n`.
pub struct Form<'a, T> {
    pub what: &'a str,
    pub build: &'a dyn Fn(usize) -> T,
}

/// Vérifie, pour chaque forme, que la lecture croît comme l'entrée, et qu'à la grande taille elle
/// tient dans les bornes absolues `limits` ; `size` donne la taille d'une entrée, `read` la lit,
/// `answer` donne la taille de la réponse que le serveur ferait de sa lecture. Rend la liste des
/// écarts (vide si tout va).
pub fn grows_like_input<T, R>(
    forms: &[Form<'_, T>],
    size: impl Fn(&T) -> usize,
    read: impl Fn(&T) -> R,
    answer: impl Fn(&R) -> usize,
    limits: Limits,
) -> Vec<String> {
    let _alone = ALONE.lock().unwrap_or_else(|e| e.into_inner());
    let mut failures = Vec::new();
    for form in forms {
        // La taille de base : voir le module.
        let mut n = 16;
        let small_input = loop {
            let input = (form.build)(n);
            if size(&input) >= MAX_BASE_SIZE {
                break input;
            }
            let first = once(|| read(&input));
            if first.peak >= MAX_BASE_PEAK || (size(&input) >= BASE_SIZE && first.time >= BASE_TIME)
            {
                break input;
            }
            n *= 2;
        };
        let large_input = (form.build)(SCALE * n);
        let small = cost_answering(|| read(&small_input), &answer);
        let large = cost_answering(|| read(&large_input), &answer);
        let sizes = (size(&small_input), size(&large_input));
        println!(
            "{} : {} puis {} ; {small:?} puis {large:?} ; pic {:.1} octets par octet d'entrée",
            form.what,
            sizes.0,
            sizes.1,
            large.peak as f64 / sizes.1.max(1) as f64
        );
        for (measure, value) in [
            (
                "octets alloués",
                ratio(large.allocated as f64, small.allocated as f64),
            ),
            (
                "pic de mémoire",
                ratio(large.peak as f64, small.peak as f64),
            ),
            // En nanosecondes : le rapport ne prend jamais une mesure sous un pour un. La petite
            // mesure ne compte jamais pour moins de `BASE_TIME` (Ruling R433).
            (
                "temps",
                ratio(
                    large.time.as_nanos() as f64,
                    small.time.max(BASE_TIME).as_nanos() as f64,
                ),
            ),
        ] {
            if value > BOUND {
                failures.push(format!(
                    "{} : {measure} multiplié par {value:.1} pour une entrée {SCALE} fois plus \
                     grande ({} puis {} ; {small:?} puis {large:?})",
                    form.what, sizes.0, sizes.1
                ));
            }
        }
        // Les bornes absolues, à la grande taille (Ruling R433).
        if large.peak > limits.peak_per_byte * sizes.1 {
            failures.push(format!(
                "{} : pic de mémoire de {} octets pour {} octets d'entrée, au-delà de {} par octet",
                form.what, large.peak, sizes.1, limits.peak_per_byte
            ));
        }
        let (answer_bound, unit) = match limits.answer {
            Answer::Absolute(bytes) => (bytes, "en tout"),
            Answer::PerByte(bytes) => (bytes * sizes.1, "par octet d'entrée"),
        };
        if large.answer > answer_bound {
            failures.push(format!(
                "{} : réponse de {} octets pour {} octets d'entrée, au-delà de la borne ({} {unit})",
                form.what,
                large.answer,
                sizes.1,
                match limits.answer {
                    Answer::Absolute(bytes) | Answer::PerByte(bytes) => bytes,
                }
            ));
        }
    }
    failures
}
