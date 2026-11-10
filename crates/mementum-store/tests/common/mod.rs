//! Outils des tests d'intégration : base temporaire, espace de départ, et SQL direct par une
//! connexion annexe (comptes de test, pannes simulées, lectures de contrôle).
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use mementum_store::{NewNode, NodeKind, Space, Store};
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;
use uuid::Uuid;

/// Une base neuve dans un dossier temporaire, avec un espace sans membre.
pub struct Fixture {
    pub dir: TempDir,
    pub db: PathBuf,
    pub store: Store,
    pub space: Space,
}

pub async fn fresh() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sous-dossier").join("m.db");
    let store = Store::open(&db).await.unwrap();
    let space = store.create_space("Essais", None).await.unwrap();
    Fixture {
        dir,
        db,
        store,
        space,
    }
}

/// Une page sans auteur, sans accès posé et sans document.
pub fn page(space: &Space, parent: Option<Uuid>, after: Option<Uuid>, title: &str) -> NewNode {
    NewNode {
        space_id: space.id,
        parent_id: parent,
        after,
        kind: NodeKind::Page,
        title: title.to_string(),
        created_by: None,
        grants: Vec::new(),
        doc: None,
    }
}

/// Une connexion annexe sur la base (clés étrangères vérifiées, comme celles du store).
pub async fn side_pool(db: &Path) -> SqlitePool {
    SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap()
}

/// Exécute `sql` par une connexion annexe (déclencheur de panne…).
pub async fn exec_sql(db: &Path, sql: &str) {
    let pool = side_pool(db).await;
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

/// L'entier que rend `sql` (un `SELECT COUNT(*) …`, une colonne entière d'une ligne).
pub async fn int(db: &Path, sql: &str) -> i64 {
    let pool = side_pool(db).await;
    let n = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    pool.close().await;
    n
}

/// La première colonne, en texte, de chaque ligne que rend `sql`.
pub async fn texts(db: &Path, sql: &str) -> Vec<Option<String>> {
    let pool = side_pool(db).await;
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    pool.close().await;
    rows
}

/// Crée un compte par du SQL direct : l'API des comptes n'est pas celle de ces tests.
pub async fn new_user(db: &Path) -> Uuid {
    let id = Uuid::now_v7();
    let pool = side_pool(db).await;
    sqlx::query(
        "INSERT INTO users (id, email, name, password_hash, created_at) \
         VALUES (?1, ?2, 'Compte de test', 'pas-un-hachage', 1)",
    )
    .bind(id.to_string())
    .bind(format!("{id}@exemple.test"))
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    id
}

/// Pose une panne : toute écriture `event` (`INSERT`, `UPDATE`…) sur `table` échoue, pour les
/// lignes que retient `when` (une condition SQL sur `NEW`, ou `1`).
pub async fn break_table(db: &Path, event: &str, table: &str, when: &str) {
    exec_sql(
        db,
        &format!(
            "CREATE TRIGGER panne BEFORE {event} ON {table} WHEN {when} \
             BEGIN SELECT RAISE(ABORT, 'panne simulée'); END"
        ),
    )
    .await;
}

/// Retire la panne posée par [`break_table`].
pub async fn repair(db: &Path) {
    exec_sql(db, "DROP TRIGGER panne").await;
}
