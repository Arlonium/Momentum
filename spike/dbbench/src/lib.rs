//! Bases de données Mementum sur SQLite : lignes en JSON typé + index d'expression + table d'arêtes.

use std::path::Path;
use std::str::FromStr;

use anyhow::{bail, Result};
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};

// ---------------------------------------------------------------- aléatoire déterministe

/// Petit générateur xorshift64* : jeux de données reproductibles sans dépendance.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

// Le SQL dynamique est marqué `AssertSqlSafe` : noms de propriétés validés par `check_prop`,
// toutes les valeurs passent en paramètres liés.

// ---------------------------------------------------------------- schéma et données

pub async fn open(path: &Path) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);
    let pool = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await?;
    for sql in [
        "CREATE TABLE IF NOT EXISTS rows (id TEXT PRIMARY KEY, source TEXT NOT NULL, props TEXT NOT NULL, created_at INTEGER NOT NULL)",
        "CREATE TABLE IF NOT EXISTS relations (from_row TEXT NOT NULL, prop TEXT NOT NULL, to_row TEXT NOT NULL)",
        "CREATE INDEX IF NOT EXISTS idx_rel ON relations(prop, to_row)",
        "CREATE INDEX IF NOT EXISTS idx_status ON rows(source, json_extract(props, '$.status'))",
        "CREATE INDEX IF NOT EXISTS idx_due ON rows(source, json_extract(props, '$.due'))",
        "CREATE INDEX IF NOT EXISTS idx_assignee ON rows(source, json_extract(props, '$.assignee'))",
        "CREATE INDEX IF NOT EXISTS idx_priority ON rows(source, json_extract(props, '$.priority'))",
    ] {
        sqlx::query(sql).execute(&pool).await?;
    }
    Ok(pool)
}

const STATUSES: [&str; 3] = ["À faire", "En cours", "Terminé"];

fn task_props(i: usize, rng: &mut Rng) -> Value {
    let mut tags: Vec<String> = Vec::new();
    for _ in 0..1 + rng.below(3) {
        let t = format!("tag{}", rng.below(20));
        if !tags.contains(&t) {
            tags.push(t);
        }
    }
    let notes = match rng.below(8) {
        0 | 1 => "",
        2 => "urgent : à traiter",
        _ => "note ordinaire",
    };
    json!({
        "title": format!("Tâche {i}"),
        "status": STATUSES[rng.below(3)],
        "assignee": format!("Personne {:02}", rng.below(50)),
        "due": format!("2026-{:02}-{:02}", 1 + rng.below(12), 1 + rng.below(28)),
        "priority": 1 + rng.below(5),
        "tags": tags,
        "done": rng.below(2) == 0,
        "estimate": rng.below(400) as f64 / 10.0 + 0.5,
        "url": format!("https://ex.dev/t/{i}"),
        "notes": notes,
        "created_at": 1_700_000_000 + i,
    })
}

/// `rows` tâches (12 propriétés, relation `project` vers 1 000 projets), insérées par lots.
pub async fn build_dataset(pool: &SqlitePool, rows: usize, seed: u64) -> Result<()> {
    let mut rng = Rng::new(seed);
    let mut tx = pool.begin().await?;
    for chunk in (0..1_000usize).collect::<Vec<_>>().chunks(500) {
        let sql = format!("INSERT INTO rows (id, source, props, created_at) VALUES {}", vec!["(?, 'projects', ?, 0)"; chunk.len()].join(", "));
        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
        for p in chunk {
            q = q.bind(format!("p{p}")).bind(json!({ "title": format!("Projet {p}") }).to_string());
        }
        q.execute(&mut *tx).await?;
    }
    for chunk in (0..rows).collect::<Vec<_>>().chunks(500) {
        let sql = format!("INSERT INTO rows (id, source, props, created_at) VALUES {}", vec!["(?, 'tasks', ?, ?)"; chunk.len()].join(", "));
        let rel_sql = format!("INSERT INTO relations (from_row, prop, to_row) VALUES {}", vec!["(?, 'project', ?)"; chunk.len()].join(", "));
        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
        let mut r = sqlx::query(sqlx::AssertSqlSafe(rel_sql.as_str()));
        for &i in chunk {
            let id = format!("t{i:06}");
            q = q.bind(id.clone()).bind(task_props(i, &mut rng).to_string()).bind(i as i64);
            r = r.bind(id).bind(format!("p{}", rng.below(1_000)));
        }
        q.execute(&mut *tx).await?;
        r.execute(&mut *tx).await?;
    }
    tx.commit().await?;
    sqlx::query("ANALYZE").execute(pool).await?;
    Ok(())
}

// ---------------------------------------------------------------- filtres

#[derive(Debug, Clone)]
pub enum Op {
    Eq,
    Neq,
    Contains,
    Lt,
    Gt,
    Before,
    After,
    IsEmpty,
}

#[derive(Debug, Clone)]
pub enum Filter {
    And(Vec<Filter>),
    Or(Vec<Filter>),
    Cond { prop: String, op: Op, value: Value },
}

fn check_prop(p: &str) -> Result<()> {
    if p.is_empty() || !p.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
        bail!("nom de propriété invalide : {p}");
    }
    Ok(())
}

fn compile(f: &Filter, sql: &mut String, binds: &mut Vec<Value>) -> Result<()> {
    match f {
        Filter::And(v) | Filter::Or(v) if v.is_empty() => sql.push_str(if matches!(f, Filter::And(_)) { "1" } else { "0" }),
        Filter::And(v) | Filter::Or(v) => {
            let joiner = if matches!(f, Filter::And(_)) { " AND " } else { " OR " };
            sql.push('(');
            for (i, c) in v.iter().enumerate() {
                if i > 0 {
                    sql.push_str(joiner);
                }
                compile(c, sql, binds)?;
            }
            sql.push(')');
        }
        Filter::Cond { prop, op, value } => {
            check_prop(prop)?;
            let e = format!("json_extract(props, '$.{prop}')");
            let fragment = match op {
                Op::Eq => format!("{e} = ?"),
                Op::Neq => format!("({e} IS NULL OR {e} != ?)"),
                Op::Lt | Op::Before => format!("{e} < ?"),
                Op::Gt | Op::After => format!("{e} > ?"),
                Op::Contains => {
                    binds.push(value.clone());
                    format!("(CASE json_type(props, '$.{prop}') WHEN 'array' THEN EXISTS (SELECT 1 FROM json_each(props, '$.{prop}') WHERE value = ?) ELSE instr({e}, ?) > 0 END)")
                }
                Op::IsEmpty => {
                    sql.push_str(&format!("({e} IS NULL OR {e} = '' OR (json_type(props, '$.{prop}') = 'array' AND json_array_length(props, '$.{prop}') = 0))"));
                    return Ok(());
                }
            };
            binds.push(value.clone());
            sql.push_str(&fragment);
        }
    }
    Ok(())
}

/// Évaluation de référence en Rust (même sémantique que le SQL), pour les tests différentiels.
pub fn eval_filter(f: &Filter, props: &Value) -> bool {
    match f {
        Filter::And(v) => v.iter().all(|c| eval_filter(c, props)),
        Filter::Or(v) => v.iter().any(|c| eval_filter(c, props)),
        Filter::Cond { prop, op, value } => {
            let got = props.get(prop.as_str());
            let eq = || match (got, value) {
                (Some(Value::Bool(a)), Value::Bool(b)) => a == b,
                (Some(Value::Number(a)), Value::Number(b)) => a.as_f64() == b.as_f64(),
                (Some(Value::String(a)), Value::String(b)) => a == b,
                _ => false,
            };
            let cmp = || match (got, value) {
                (Some(Value::Number(a)), Value::Number(b)) => a.as_f64().partial_cmp(&b.as_f64()),
                (Some(Value::String(a)), Value::String(b)) => Some(a.as_str().cmp(b.as_str())),
                _ => None,
            };
            match op {
                Op::Eq => eq(),
                Op::Neq => !eq(),
                Op::Lt | Op::Before => cmp() == Some(std::cmp::Ordering::Less),
                Op::Gt | Op::After => cmp() == Some(std::cmp::Ordering::Greater),
                Op::Contains => match (got, value) {
                    (Some(Value::Array(items)), v) => items.contains(v),
                    (Some(Value::String(s)), Value::String(v)) => s.contains(v.as_str()),
                    _ => false,
                },
                Op::IsEmpty => match got {
                    None | Some(Value::Null) => true,
                    Some(Value::String(s)) => s.is_empty(),
                    Some(Value::Array(a)) => a.is_empty(),
                    _ => false,
                },
            }
        }
    }
}

// ---------------------------------------------------------------- requêtes

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dir {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub struct Cursor {
    pub values: Vec<Value>,
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub source: String,
    pub filter: Option<Filter>,
    pub sort: Vec<(String, Dir)>,
    pub limit: usize,
    pub after: Option<Cursor>,
}

#[derive(Debug, Clone)]
pub struct RowOut {
    pub id: String,
    pub props: Value,
}

#[derive(Debug)]
pub struct PageOfRows {
    pub rows: Vec<RowOut>,
    pub next: Option<Cursor>,
}

fn bind_all<'q>(mut q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>, binds: &'q [Value]) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    for v in binds {
        q = match v {
            Value::String(s) => q.bind(s.as_str()),
            Value::Bool(b) => q.bind(*b as i64),
            Value::Number(n) if n.is_i64() => q.bind(n.as_i64().unwrap()),
            Value::Number(n) => q.bind(n.as_f64().unwrap()),
            _ => q.bind(Option::<String>::None),
        };
    }
    q
}

/// Fenêtre de lignes filtrées et triées, paginée par curseur (valeurs de tri + id).
pub async fn query(pool: &SqlitePool, q: &Query) -> Result<PageOfRows> {
    let mut sql = String::from("SELECT id, props FROM rows WHERE source = ?");
    let mut binds: Vec<Value> = vec![Value::String(q.source.clone())];
    if let Some(f) = &q.filter {
        sql.push_str(" AND ");
        compile(f, &mut sql, &mut binds)?;
    }
    let keys: Vec<(String, Dir)> = q.sort.clone();
    for (p, _) in &keys {
        check_prop(p)?;
    }
    let key_expr = |p: &str| format!("json_extract(props, '$.{p}')");
    if let Some(c) = &q.after {
        // (k1, …, kn, id) > (v1, …, vn, last_id) dans l'ordre lexicographique, sens par clé.
        let mut ors = Vec::new();
        for i in 0..=keys.len() {
            let mut ands = Vec::new();
            for j in 0..i {
                ands.push(format!("{} = ?", key_expr(&keys[j].0)));
                binds.push(c.values[j].clone());
            }
            if i < keys.len() {
                let op = if keys[i].1 == Dir::Asc { ">" } else { "<" };
                ands.push(format!("{} {op} ?", key_expr(&keys[i].0)));
                binds.push(c.values[i].clone());
            } else {
                ands.push("id > ?".into());
                binds.push(Value::String(c.id.clone()));
            }
            ors.push(format!("({})", ands.join(" AND ")));
        }
        sql.push_str(&format!(" AND ({})", ors.join(" OR ")));
    }
    let mut order: Vec<String> = keys.iter().map(|(p, d)| format!("{} {}", key_expr(p), if *d == Dir::Asc { "ASC" } else { "DESC" })).collect();
    order.push("id ASC".into());
    sql.push_str(&format!(" ORDER BY {} LIMIT {}", order.join(", "), q.limit + 1));

    let rows = bind_all(sqlx::query(sqlx::AssertSqlSafe(sql.as_str())), &binds).fetch_all(pool).await?;
    let mut out: Vec<RowOut> = rows
        .iter()
        .map(|r| {
            let props: String = r.get("props");
            RowOut { id: r.get("id"), props: serde_json::from_str(&props).unwrap_or(Value::Null) }
        })
        .collect();
    let next = if out.len() > q.limit {
        out.truncate(q.limit);
        out.last().map(|last| Cursor {
            values: keys.iter().map(|(p, _)| last.props.get(p.as_str()).cloned().unwrap_or(Value::Null)).collect(),
            id: last.id.clone(),
        })
    } else {
        None
    };
    Ok(PageOfRows { rows: out, next })
}

/// Nombre de lignes par valeur d'une propriété (vue groupée).
pub async fn group_counts(pool: &SqlitePool, source: &str, prop: &str) -> Result<Vec<(String, i64)>> {
    check_prop(prop)?;
    // Même expression que l'index (sans CAST), sinon SQLite relit le JSON de chaque ligne.
    let sql = format!("SELECT json_extract(props, '$.{prop}') AS k, COUNT(*) AS n FROM rows WHERE source = ? GROUP BY json_extract(props, '$.{prop}')");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(source).fetch_all(pool).await?;
    let key = |r: &sqlx::sqlite::SqliteRow| -> String {
        r.try_get::<Option<String>, _>("k")
            .ok()
            .flatten()
            .or_else(|| r.try_get::<Option<i64>, _>("k").ok().flatten().map(|v| v.to_string()))
            .or_else(|| r.try_get::<Option<f64>, _>("k").ok().flatten().map(|v| v.to_string()))
            .unwrap_or_default()
    };
    let mut out: Vec<(String, i64)> = rows.iter().map(|r| (key(r), r.get("n"))).collect();
    out.sort();
    Ok(out)
}

/// Rollup « somme » : pour chaque cible de la relation, somme d'une propriété numérique des lignes liées.
pub async fn rollup_sum(pool: &SqlitePool, prop_relation: &str, prop_value: &str) -> Result<Vec<(String, f64)>> {
    check_prop(prop_relation)?;
    check_prop(prop_value)?;
    let sql = format!(
        "SELECT r.to_row AS k, SUM(json_extract(t.props, '$.{prop_value}')) AS s FROM relations r JOIN rows t ON t.id = r.from_row WHERE r.prop = ? GROUP BY r.to_row"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str())).bind(prop_relation).fetch_all(pool).await?;
    Ok(rows.iter().map(|r| (r.get("k"), r.get::<Option<f64>, _>("s").unwrap_or(0.0))).collect())
}
