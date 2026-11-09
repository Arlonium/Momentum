//! Task 6 — bases de données : filtres (différentiel SQL ⇄ Rust), pagination, budgets sur 100 000 lignes.

use std::collections::HashSet;
use std::time::Instant;

use serde_json::{json, Value};
use spike_db::{build_dataset, eval_filter, group_counts, open, query, Dir, Filter, Op, Query, Rng};

async fn dataset(rows: usize) -> (tempfile::TempDir, sqlx::SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = open(&dir.path().join("db.sqlite")).await.unwrap();
    build_dataset(&pool, rows, 42).await.unwrap();
    (dir, pool)
}

/// Condition aléatoire cohérente avec le type de la propriété.
fn random_cond(rng: &mut Rng) -> Filter {
    let statuses = ["À faire", "En cours", "Terminé"];
    match rng.below(7) {
        0 => Filter::Cond { prop: "status".into(), op: if rng.below(2) == 0 { Op::Eq } else { Op::Neq }, value: json!(statuses[rng.below(3)]) },
        1 => Filter::Cond { prop: "priority".into(), op: [Op::Eq, Op::Lt, Op::Gt][rng.below(3)].clone(), value: json!(1 + rng.below(5)) },
        2 => Filter::Cond { prop: "due".into(), op: if rng.below(2) == 0 { Op::Before } else { Op::After }, value: json!(format!("2026-{:02}-15", 1 + rng.below(12))) },
        3 => Filter::Cond { prop: "tags".into(), op: Op::Contains, value: json!(format!("tag{}", rng.below(20))) },
        4 => Filter::Cond { prop: "done".into(), op: Op::Eq, value: json!(rng.below(2) == 0) },
        5 => Filter::Cond { prop: "assignee".into(), op: Op::Eq, value: json!(format!("Personne {:02}", rng.below(50))) },
        _ => Filter::Cond { prop: "notes".into(), op: if rng.below(2) == 0 { Op::IsEmpty } else { Op::Contains }, value: json!("urgent") },
    }
}

/// Filtre aléatoire imbriqué sur 3 niveaux au plus.
fn random_filter(rng: &mut Rng, depth: usize) -> Filter {
    if depth == 3 || rng.below(3) == 0 {
        return random_cond(rng);
    }
    let n = 1 + rng.below(3);
    let children = (0..n).map(|_| random_filter(rng, depth + 1)).collect();
    if rng.below(2) == 0 { Filter::And(children) } else { Filter::Or(children) }
}

#[tokio::test]
async fn filter_matches_reference() {
    let (_d, pool) = dataset(2_000).await;
    let all = query(&pool, &Query { source: "tasks".into(), filter: None, sort: vec![], limit: 10_000, after: None }).await.unwrap();
    assert_eq!(all.rows.len(), 2_000);
    let mut rng = Rng::new(7);
    for i in 0..200 {
        let f = random_filter(&mut rng, 1);
        let got: HashSet<String> = query(&pool, &Query { source: "tasks".into(), filter: Some(f.clone()), sort: vec![], limit: 10_000, after: None })
            .await
            .unwrap()
            .rows
            .into_iter()
            .map(|r| r.id)
            .collect();
        let want: HashSet<String> = all.rows.iter().filter(|r| eval_filter(&f, &r.props)).map(|r| r.id.clone()).collect();
        assert_eq!(got, want, "filtre n° {i} : {f:?}");
    }
}

#[tokio::test]
async fn cursor_pagination_is_stable() {
    let (_d, pool) = dataset(2_000).await;
    let mut seen = HashSet::new();
    let mut after = None;
    loop {
        let page = query(&pool, &Query { source: "tasks".into(), filter: None, sort: vec![("priority".into(), Dir::Desc), ("due".into(), Dir::Asc)], limit: 200, after: after.clone() })
            .await
            .unwrap();
        for r in &page.rows {
            assert!(seen.insert(r.id.clone()), "ligne vue deux fois : {}", r.id);
        }
        match page.next {
            Some(c) => after = Some(c),
            None => break,
        }
    }
    assert_eq!(seen.len(), 2_000);
}

fn p95(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * 0.95).round() as usize]
}

/// Budgets de la spec (§5.10) — à lancer sur la machine cible :
/// `cargo test --release -p spike-db -- --ignored budgets_100k --nocapture`
#[tokio::test]
#[ignore]
async fn budgets_100k() {
    let (_d, pool) = dataset(100_000).await;
    let first = Query { source: "tasks".into(), filter: None, sort: vec![("due".into(), Dir::Asc)], limit: 200, after: None };
    let filtered = Query {
        source: "tasks".into(),
        filter: Some(Filter::And(vec![
            Filter::Or(vec![
                Filter::Cond { prop: "status".into(), op: Op::Eq, value: json!("En cours") },
                Filter::Cond { prop: "status".into(), op: Op::Eq, value: json!("À faire") },
            ]),
            Filter::Cond { prop: "priority".into(), op: Op::Gt, value: json!(2) },
        ])),
        sort: vec![("priority".into(), Dir::Desc)],
        limit: 200,
        after: None,
    };
    let mut t_first = Vec::new();
    let mut t_filter = Vec::new();
    let mut t_group = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        query(&pool, &first).await.unwrap();
        t_first.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        query(&pool, &filtered).await.unwrap();
        t_filter.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        group_counts(&pool, "tasks", "status").await.unwrap();
        t_group.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let (a, b, c) = (p95(t_first), p95(t_filter), p95(t_group));
    println!("MESURE 100k : première fenêtre p95 {a:.1} ms, filtre ET/OU + tri p95 {b:.1} ms, groupes p95 {c:.1} ms");
    assert!(a < 500.0, "première fenêtre {a:.1} ms");
    assert!(b < 300.0, "filtre + tri {b:.1} ms");
    assert!(c < 300.0, "groupes {c:.1} ms");
}

#[test]
fn eval_handles_missing_props() {
    let props: Value = json!({ "status": "En cours" });
    assert!(eval_filter(&Filter::Cond { prop: "notes".into(), op: Op::IsEmpty, value: Value::Null }, &props));
    assert!(eval_filter(&Filter::Cond { prop: "priority".into(), op: Op::Neq, value: json!(3) }, &props));
    assert!(!eval_filter(&Filter::Cond { prop: "priority".into(), op: Op::Gt, value: json!(3) }, &props));
}
