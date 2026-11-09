//! Bench yrs contre Loro sur une page de 10 000 blocs :
//! `cargo run --release -p spike-crdt-bench` (écrit aussi `crdtbench/results/rust.csv`).

use std::time::Instant;

use spike_crdt_bench::{l, y, Rng};

const N: usize = 10_000;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn rss_mb() -> f64 {
    let mut sys = sysinfo::System::new().unwrap();
    let pid = sysinfo::Pid::from_u32(std::process::id());
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map(|p| p.memory() as f64 / 1_048_576.0).unwrap_or(f64::NAN)
}

fn ms<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let t = Instant::now();
    let r = f();
    (r, t.elapsed().as_secs_f64() * 1000.0)
}

fn main() {
    let moves_a: Vec<usize> = (0..50).collect();
    let moves_b: Vec<usize> = (25..75).collect();
    let mut rows: Vec<(String, String, String)> = Vec::new();
    let mut add = |w: &str, yv: String, lv: String| rows.push((w.to_string(), yv, lv));

    // W1 — construction
    let w1y = median((0..5).map(|_| ms(|| y::built(N)).1).collect());
    let w1l = median((0..5).map(|_| ms(|| l::built(N)).1).collect());
    add("W1 construire 10 000 blocs (ms)", format!("{w1y:.1}"), format!("{w1l:.1}"));

    // W2 — taille de l'instantané
    let (ydoc, ldoc) = (y::built(N), l::built(N));
    let (ys, ls) = (y::snapshot(&ydoc), l::snapshot(&ldoc));
    add("W2 instantané (Ko)", format!("{}", ys.len() / 1024), format!("{}", ls.len() / 1024));

    // W3 — chargement de l'instantané
    let w3y = median((0..5).map(|_| ms(|| y::load(&ys, 9)).1).collect());
    let w3l = median((0..5).map(|_| ms(|| l::load(&ls, 9)).1).collect());
    add("W3 charger l'instantané (ms)", format!("{w3y:.1}"), format!("{w3l:.1}"));
    // Loro charge paresseusement : on mesure aussi « charger puis lire les 10 000 blocs ».
    let w3by = median((0..5).map(|_| ms(|| y::block_texts(&y::load(&ys, 9)).len()).1).collect());
    let w3bl = median((0..5).map(|_| ms(|| l::block_texts(&l::load(&ls, 9)).len()).1).collect());
    add("W3b charger puis lire tous les blocs (ms)", format!("{w3by:.1}"), format!("{w3bl:.1}"));

    // W4 — mémoire après chargement (écart de RSS du processus)
    drop((ydoc, ldoc));
    let before = rss_mb();
    let held_y = y::load(&ys, 10);
    let mid = rss_mb();
    let held_l = l::load(&ls, 10);
    let after = rss_mb();
    add("W4 mémoire après chargement (Mo, écart RSS)", format!("{:.1}", mid - before), format!("{:.1}", after - mid));
    drop((held_y, held_l));

    // W5 — 2 pairs × 1 000 éditions concurrentes + 50 déplacements chacun, puis fusion
    let (ya, yb) = (y::load(&ys, 1), y::load(&ys, 2));
    y::edit_random(&ya, &mut Rng::new(1), 1_000, &moves_a);
    y::edit_random(&yb, &mut Rng::new(2), 1_000, &moves_b);
    let ((), w5y) = ms(|| y::sync(&ya, &yb));
    let yconv = y::block_texts(&ya) == y::block_texts(&yb);
    let ydup = y::block_texts(&ya).len() - N;
    let (la, lb) = (l::load(&ls, 1), l::load(&ls, 2));
    l::edit_random(&la, &mut Rng::new(1), 1_000, &moves_a);
    l::edit_random(&lb, &mut Rng::new(2), 1_000, &moves_b);
    let ((), w5l) = ms(|| l::sync(&la, &lb));
    let lconv = l::block_texts(&la) == l::block_texts(&lb);
    let ldup = l::block_texts(&la).len() as isize - N as isize;
    add(
        "W5 fusion de 2 × 1 000 éditions + 50 déplacements (ms)",
        format!("{w5y:.1} (convergé : {yconv}, blocs en double : {ydup})"),
        format!("{w5l:.1} (convergé : {lconv}, blocs en double : {ldup})"),
    );

    // W6 — taille de l'état après 10 000 éditions de texte (historique compris)
    let y6 = y::load(&ys, 3);
    y::edit_random(&y6, &mut Rng::new(3), 10_000, &[]);
    let l6 = l::load(&ls, 3);
    l::edit_random(&l6, &mut Rng::new(3), 10_000, &[]);
    add("W6 état après 10 000 éditions (Ko)", format!("{}", y::snapshot(&y6).len() / 1024), format!("{}", l::snapshot(&l6).len() / 1024));

    println!("| Charge | yrs 0.28 | Loro 1.17 |\n|---|---|---|");
    let mut csv = String::from("charge;yrs;loro\n");
    for (w, a, b) in &rows {
        println!("| {w} | {a} | {b} |");
        csv.push_str(&format!("{w};{a};{b}\n"));
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("results");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("rust.csv"), csv).unwrap();
}
