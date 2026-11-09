//! Task 7 — convergence après éditions concurrentes et déplacements de blocs (yrs et Loro).

use std::collections::HashMap;

use spike_crdt_bench::{l, y, Rng, Workload};

const N: usize = 1_000;

fn workload() -> Workload {
    // Les pairs déplacent des blocs qui se chevauchent : 25 blocs sont déplacés par les deux à la fois.
    Workload { edits: 300, moves_a: (0..50).collect(), moves_b: (25..75).collect() }
}

fn duplicates(blocks: &[(String, String)]) -> usize {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for (id, _) in blocks {
        *seen.entry(id).or_default() += 1;
    }
    seen.values().filter(|&&n| n > 1).count()
}

fn distinct(blocks: &[(String, String)]) -> usize {
    blocks.iter().map(|(id, _)| id.as_str()).collect::<std::collections::HashSet<_>>().len()
}

#[test]
fn yrs_converges() {
    let snap = y::snapshot(&y::built(N));
    let (a, b) = (y::load(&snap, 1), y::load(&snap, 2));
    let w = workload();
    y::edit_random(&a, &mut Rng::new(1), w.edits, &w.moves_a);
    y::edit_random(&b, &mut Rng::new(2), w.edits, &w.moves_b);
    y::sync(&a, &b);
    let (ba, bb) = (y::block_texts(&a), y::block_texts(&b));
    assert_eq!(ba, bb, "les deux pairs doivent converger");
    assert_eq!(distinct(&ba), N, "aucun bloc ne doit être perdu");
    println!("yrs : {} blocs, {} ids en double après déplacements concurrents", ba.len(), duplicates(&ba));
}

/// Avec yrs, un déplacement = suppression + réinsertion : deux pairs qui déplacent le même bloc le dupliquent.
#[test]
fn yrs_concurrent_moves_duplicate_blocks() {
    let snap = y::snapshot(&y::built(N));
    let (a, b) = (y::load(&snap, 1), y::load(&snap, 2));
    let w = workload();
    y::edit_random(&a, &mut Rng::new(1), 0, &w.moves_a);
    y::edit_random(&b, &mut Rng::new(2), 0, &w.moves_b);
    y::sync(&a, &b);
    assert_eq!(duplicates(&y::block_texts(&a)), 25, "les 25 blocs déplacés des deux côtés sont dupliqués");
}

#[test]
fn loro_converges_without_duplicates() {
    let snap = l::snapshot(&l::built(N));
    let (a, b) = (l::load(&snap, 1), l::load(&snap, 2));
    let w = workload();
    l::edit_random(&a, &mut Rng::new(1), w.edits, &w.moves_a);
    l::edit_random(&b, &mut Rng::new(2), w.edits, &w.moves_b);
    l::sync(&a, &b);
    let (ba, bb) = (l::block_texts(&a), l::block_texts(&b));
    assert_eq!(ba, bb, "les deux pairs doivent converger");
    assert_eq!(ba.len(), N, "ni bloc perdu ni bloc en trop");
    assert_eq!(duplicates(&ba), 0, "un déplacement concurrent ne doit pas dupliquer");
}
