//! Bench des bases : `cargo run --release -p spike-db -- bench --rows 100000`

use std::time::Instant;

use serde_json::json;
use spike_db::{build_dataset, group_counts, open, query, rollup_sum, Dir, Filter, Op, Query};

fn pct(mut v: Vec<f64>, p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p).round() as usize]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let rows: usize = args.iter().position(|a| a == "--rows").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let dir = std::env::temp_dir().join(format!("spike-db-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let pool = open(&dir.join("bench.sqlite")).await?;
    let t = Instant::now();
    build_dataset(&pool, rows, 42).await?;
    println!("jeu de données : {rows} lignes en {:.1} s\n", t.elapsed().as_secs_f64());

    let first = Query { source: "tasks".into(), filter: None, sort: vec![("due".into(), Dir::Asc)], limit: 200, after: None };
    let filter = Query { filter: Some(Filter::Cond { prop: "status".into(), op: Op::Eq, value: json!("En cours") }), sort: vec![], ..first.clone() };
    let filter_sort = Query {
        filter: Some(Filter::And(vec![
            Filter::Or(vec![
                Filter::Cond { prop: "status".into(), op: Op::Eq, value: json!("En cours") },
                Filter::Cond { prop: "status".into(), op: Op::Eq, value: json!("À faire") },
            ]),
            Filter::Cond { prop: "priority".into(), op: Op::Gt, value: json!(2) },
        ])),
        sort: vec![("priority".into(), Dir::Desc)],
        ..first.clone()
    };

    println!("| Opération | p50 (ms) | p95 (ms) |\n|---|---|---|");
    macro_rules! bench {
        ($name:expr, $body:expr) => {{
            let mut v = Vec::new();
            for _ in 0..20 {
                let t = Instant::now();
                $body;
                v.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            println!("| {} | {:.1} | {:.1} |", $name, pct(v.clone(), 0.5), pct(v, 0.95));
        }};
    }
    bench!("Première fenêtre (200 lignes triées par échéance)", query(&pool, &first).await?);
    bench!("Filtre statut = En cours", query(&pool, &filter).await?);
    bench!("Filtre ET/OU + tri par priorité", query(&pool, &filter_sort).await?);
    bench!("Groupes par statut", group_counts(&pool, "tasks", "status").await?);
    bench!("Rollup : somme des estimations par projet", rollup_sum(&pool, "project", "estimate").await?);
    let mut n = rows;
    bench!("Insertion d'une ligne", {
        n += 1;
        sqlx::query("INSERT INTO rows (id, source, props, created_at) VALUES (?, 'tasks', ?, 0)")
            .bind(format!("t{n:06}"))
            .bind(json!({ "title": "Nouvelle", "status": "À faire", "priority": 3, "due": "2026-06-01" }).to_string())
            .execute(&pool)
            .await?
    });
    bench!("Modification d'une cellule", {
        sqlx::query("UPDATE rows SET props = json_set(props, '$.status', 'Terminé') WHERE id = 't000123'").execute(&pool).await?
    });

    // Variante de repli (spec §5.5) : une vraie table SQL par source, une colonne typée par propriété.
    for sql in [
        "CREATE INDEX IF NOT EXISTS rel_from ON relations(from_row, prop)",
        "CREATE TABLE tasks_typed (id TEXT PRIMARY KEY, title TEXT, status TEXT, assignee TEXT, due TEXT, priority INTEGER, done INTEGER, estimate REAL, url TEXT, notes TEXT, created_at INTEGER, tags TEXT, project TEXT)",
        "INSERT INTO tasks_typed SELECT r.id, json_extract(props,'$.title'), json_extract(props,'$.status'), json_extract(props,'$.assignee'), json_extract(props,'$.due'), json_extract(props,'$.priority'), json_extract(props,'$.done'), json_extract(props,'$.estimate'), json_extract(props,'$.url'), json_extract(props,'$.notes'), json_extract(props,'$.created_at'), json_extract(props,'$.tags'), rel.to_row FROM rows r LEFT JOIN relations rel ON rel.from_row = r.id AND rel.prop = 'project' WHERE r.source = 'tasks'",
        "CREATE INDEX t_status ON tasks_typed(status)",
        "CREATE INDEX t_due ON tasks_typed(due)",
        "CREATE INDEX t_priority ON tasks_typed(priority)",
        "CREATE INDEX t_project ON tasks_typed(project)",
        "ANALYZE",
    ] {
        sqlx::query(sql).execute(&pool).await?;
    }
    println!("\nVariante : table typée par source\n\n| Opération | p50 (ms) | p95 (ms) |\n|---|---|---|");
    bench!("Première fenêtre (200 lignes triées par échéance)", sqlx::query("SELECT * FROM tasks_typed ORDER BY due, id LIMIT 201").fetch_all(&pool).await?);
    bench!("Filtre statut = En cours", sqlx::query("SELECT * FROM tasks_typed WHERE status = 'En cours' ORDER BY id LIMIT 201").fetch_all(&pool).await?);
    bench!(
        "Filtre ET/OU + tri par priorité",
        sqlx::query("SELECT * FROM tasks_typed WHERE (status = 'En cours' OR status = 'À faire') AND priority > 2 ORDER BY priority DESC, id LIMIT 201").fetch_all(&pool).await?
    );
    bench!("Groupes par statut", sqlx::query("SELECT status, COUNT(*) FROM tasks_typed GROUP BY status").fetch_all(&pool).await?);
    bench!("Rollup : somme des estimations par projet", sqlx::query("SELECT project, SUM(estimate) FROM tasks_typed GROUP BY project").fetch_all(&pool).await?);

    pool.close().await;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
