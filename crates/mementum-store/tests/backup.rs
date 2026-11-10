//! Copie de la base avant toute migration en attente : faite par SQLite (`VACUUM INTO`) à côté de
//! la base, jamais écrasée, et sans elle rien ne migre.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{int, texts};
use mementum_store::{Backup, Store, backup_path};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

/// Une base telle que le code du lot 1 la produit (comme dans `tests/migration.rs`) : les options
/// de connexion de `Store::open`, et la seule migration `0001_init.sql`, lancée par sqlx.
async fn lot1_database(db: &Path) -> SqlitePool {
    let migrations = tempfile::tempdir().unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations/0001_init.sql"),
        migrations.path().join("0001_init.sql"),
    )
    .unwrap();
    let options = SqliteConnectOptions::new()
        .filename(db)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .foreign_keys(true)
        .busy_timeout(Duration::from_millis(5000));
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .unwrap();
    Migrator::new(migrations.path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    pool
}

/// L'organisation et le coffre d'une base du lot 1, écrits en SQL.
async fn lot1_vault(pool: &SqlitePool) -> (Uuid, Uuid) {
    let (org, vault) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query(
        "INSERT INTO orgs (id, slug, name, created_at) VALUES (?, 'default', 'default', 1000)",
    )
    .bind(org.to_string())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO vaults (id, org_id, name, kind, created_at) VALUES (?, ?, 'Mon coffre', 'personal', 1001)",
    )
    .bind(vault.to_string())
    .bind(org.to_string())
    .execute(pool)
    .await
    .unwrap();
    (org, vault)
}

/// Une page de racine du lot 1, écrite en SQL.
async fn lot1_page(pool: &SqlitePool, (org, vault): (Uuid, Uuid), title: &str, position: &str) {
    sqlx::query(
        "INSERT INTO nodes (id, org_id, vault_id, kind, title, position, created_at, updated_at) \
         VALUES (?1, ?2, ?3, 'page', ?4, ?5, 2000, 2000)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(org.to_string())
    .bind(vault.to_string())
    .bind(title)
    .bind(position)
    .execute(pool)
    .await
    .unwrap();
}

/// Une base du lot 1 avec une page, connexions fermées.
async fn lot1_with_a_page(db: &Path) {
    let pool = lot1_database(db).await;
    let vault = lot1_vault(&pool).await;
    lot1_page(&pool, vault, "Accueil", "a0").await;
    pool.close().await;
}

/// Les noms des entrées de `dir`.
fn entries(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

/// `db` suivi de `suffix` (`-wal`, `-shm`).
fn sibling(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Une connexion en lecture seule sur `db`, sans rien changer à son mode de journal.
async fn read_only(db: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .connect_with(SqliteConnectOptions::new().filename(db).read_only(true))
        .await
        .unwrap()
}

/// Les titres des pages de `pool`, dans l'ordre alphabétique.
async fn titles(pool: &SqlitePool) -> Vec<String> {
    sqlx::query("SELECT title FROM nodes ORDER BY title")
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect()
}

#[tokio::test]
async fn pending_migration_backs_up_first() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");

    // Une base du lot 1 : deux pages reversées dans le fichier principal, puis une troisième,
    // écrite juste avant, qui n'est encore que dans le `-wal`.
    let pool = lot1_database(&db).await;
    let vault = lot1_vault(&pool).await;
    lot1_page(&pool, vault, "Accueil", "a0").await;
    lot1_page(&pool, vault, "Notes", "a1").await;
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&pool)
        .await
        .unwrap();
    lot1_page(&pool, vault, "Écrite juste avant", "a2").await;
    assert!(std::fs::metadata(sibling(&db, "-wal")).unwrap().len() > 0);
    // Le fichier principal, lu seul, n'a pas cette page : elle n'est que dans le `-wal`.
    let main_only = tempfile::tempdir().unwrap();
    std::fs::copy(&db, main_only.path().join("principal.db")).unwrap();
    let main = read_only(&main_only.path().join("principal.db")).await;
    assert_eq!(titles(&main).await, ["Accueil", "Notes"]);
    main.close().await;

    // La connexion du lot 1 reste ouverte : la fermer reverserait le `-wal` dans la base.
    let store = Store::open(&db).await.unwrap();
    let backup = backup_path(&db, 2);
    assert_eq!(backup, dir.path().join("mementum.db.avant-0002.bak"));
    assert!(backup.is_file());
    let bytes = std::fs::metadata(&backup).unwrap().len();
    assert!(bytes > 0);
    assert_eq!(
        store.backup(),
        Some(&Backup {
            path: backup.clone(),
            bytes,
            kept: false,
        })
    );
    // Puis la migration a tourné.
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(version AS TEXT) FROM _sqlx_migrations ORDER BY version"
        )
        .await,
        [Some("1".to_string()), Some("2".to_string())]
    );
    store.close().await;
    pool.close().await;
    assert!(!sibling(&backup, "-wal").exists());
    assert!(!sibling(&backup, "-shm").exists());

    // La copie se lit seule : recopiée sans rien d'autre dans un autre dossier (ni `-wal` ni
    // `-shm`), ouverte en lecture seule par sqlx, sans le `Store`. C'est la base du lot 1, avec ses
    // trois pages.
    let elsewhere = tempfile::tempdir().unwrap();
    let alone = elsewhere.path().join("copie.db");
    std::fs::copy(&backup, &alone).unwrap();
    let copy = read_only(&alone).await;
    // `VACUUM INTO` écrit une base en mode de journal classique : elle n'a jamais besoin de `-shm`,
    // même sur un support en lecture seule.
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&copy)
        .await
        .unwrap();
    assert_eq!(mode, "delete");
    let tables: Vec<String> = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('orgs', 'vaults', 'spaces') \
         ORDER BY name",
    )
    .fetch_all(&copy)
    .await
    .unwrap()
    .iter()
    .map(|r| r.get(0))
    .collect();
    assert_eq!(tables, ["orgs", "vaults"]);
    assert_eq!(
        titles(&copy).await,
        ["Accueil", "Notes", "Écrite juste avant"]
    );
    let versions: Vec<i64> = sqlx::query("SELECT version FROM _sqlx_migrations")
        .fetch_all(&copy)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(versions, [1]);
    copy.close().await;
}

#[tokio::test]
async fn fresh_database_is_not_backed_up() {
    let dir = tempfile::tempdir().unwrap();

    // Aucun fichier.
    let db = dir.path().join("neuve.db");
    let store = Store::open(&db).await.unwrap();
    assert_eq!(store.backup(), None);
    store.close().await;
    assert!(!backup_path(&db, 1).exists());

    // Un fichier vide : aucune table `_sqlx_migrations`.
    let empty = dir.path().join("vide.db");
    std::fs::write(&empty, b"").unwrap();
    let store = Store::open(&empty).await.unwrap();
    assert_eq!(store.backup(), None);
    store.close().await;
    assert!(!backup_path(&empty, 1).exists());

    assert_eq!(
        entries(dir.path()),
        BTreeSet::from(["neuve.db".to_string(), "vide.db".to_string()])
    );
}

#[tokio::test]
async fn up_to_date_database_is_not_backed_up() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    lot1_with_a_page(&db).await;

    // La première ouverture copie et migre.
    let store = Store::open(&db).await.unwrap();
    assert!(store.backup().is_some());
    store.close().await;
    let after_first = entries(dir.path());
    assert_eq!(
        after_first,
        BTreeSet::from([
            "mementum.db".to_string(),
            "mementum.db.avant-0002.bak".to_string()
        ])
    );

    // La seconde trouve une base à jour : aucun fichier de plus.
    let store = Store::open(&db).await.unwrap();
    assert_eq!(store.backup(), None);
    store.close().await;
    assert_eq!(entries(dir.path()), after_first);
}

#[tokio::test]
async fn existing_backup_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    lot1_with_a_page(&db).await;

    // Une montée de version interrompue a laissé sa copie : c'est elle qui porte l'état d'avant.
    let backup = backup_path(&db, 2);
    let before = b"copie laissee par une montee de version interrompue".to_vec();
    std::fs::write(&backup, &before).unwrap();

    let store = Store::open(&db).await.unwrap();
    assert_eq!(std::fs::read(&backup).unwrap(), before);
    assert_eq!(
        store.backup(),
        Some(&Backup {
            path: backup,
            bytes: before.len() as u64,
            kept: true,
        })
    );
    // L'ouverture a continué : la base est migrée.
    assert_eq!(int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await, 2);
    store.close().await;
}

#[tokio::test]
async fn failed_backup_stops_before_migrating() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    lot1_with_a_page(&db).await;

    // Un dossier porte le nom de la copie : SQLite ne peut pas l'écrire.
    let backup = backup_path(&db, 2);
    std::fs::create_dir(&backup).unwrap();

    let error = Store::open(&db)
        .await
        .err()
        .expect("l'ouverture doit échouer");
    assert!(
        error.to_string().contains("mementum.db.avant-0002.bak"),
        "l'erreur nomme la copie : {error}"
    );

    // La base est restée au lot 1.
    assert_eq!(
        int(
            &db,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('orgs', 'vaults')"
        )
        .await,
        2
    );
    assert_eq!(int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await, 1);
    assert_eq!(int(&db, "SELECT COUNT(*) FROM nodes").await, 1);
    assert!(backup.is_dir());
    assert!(entries(&backup).is_empty());
}
