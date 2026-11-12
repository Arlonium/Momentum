//! Copie de la base avant toute migration en attente : faite par SQLite (`VACUUM INTO`) à côté de
//! la base, jamais écrasée, et sans elle rien ne migre.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{LOT2, database_with, int, texts};
use mementum_store::{Backup, Store, backup_path};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

/// Le nombre de migrations du store : une base migrée les porte toutes.
const MIGRATIONS: i64 = 3;

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

/// `db` suivi de `suffix` (`-wal`, `-shm`, `.partiel`).
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

/// Les tables du lot 1 (`orgs`, `vaults`) et du lot 2 (`spaces`) que `pool` porte.
async fn lot_tables(pool: &SqlitePool) -> Vec<String> {
    sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('orgs', 'vaults', 'spaces') \
         ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .iter()
    .map(|r| r.get(0))
    .collect()
}

/// La copie `backup`, recopiée sans rien d'autre dans un dossier neuf (ni `-wal` ni `-shm`), et
/// ouverte en lecture seule par sqlx, sans le `Store`. Le dossier vit autant que la connexion.
async fn open_alone(backup: &Path) -> (tempfile::TempDir, SqlitePool) {
    let elsewhere = tempfile::tempdir().unwrap();
    let alone = elsewhere.path().join("copie.db");
    std::fs::copy(backup, &alone).unwrap();
    let pool = read_only(&alone).await;
    (elsewhere, pool)
}

/// Les copies datées de `db` d'avant la migration `0002`, rangées à côté d'elle :
/// `<base>.avant-0002.<AAAAMMJJ>T<HHMMSS>Z.bak`, ou `…Z-<n>.bak`.
fn dated_copies(db: &Path) -> Vec<PathBuf> {
    let dir = db.parent().unwrap();
    let prefix = format!("{}.avant-0002.", db.file_name().unwrap().to_str().unwrap());
    entries(dir)
        .into_iter()
        .filter(|name| {
            let Some(rest) = name
                .strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(".bak"))
            else {
                return false;
            };
            let (stamp, n) = rest.split_once('-').unwrap_or((rest, "2"));
            !n.is_empty()
                && n.bytes().all(|c| c.is_ascii_digit())
                && stamp.len() == 16
                && stamp.bytes().enumerate().all(|(i, c)| match i {
                    8 => c == b'T',
                    15 => c == b'Z',
                    _ => c.is_ascii_digit(),
                })
        })
        .map(|name| dir.join(name))
        .collect()
}

/// Une copie de la base par `VACUUM INTO`, comme celle du store.
async fn vacuum_into(pool: &SqlitePool, target: &Path) {
    sqlx::query("VACUUM INTO ?")
        .bind(target.to_str().unwrap())
        .execute(pool)
        .await
        .unwrap();
}

/// L'hypothèse de la comparaison des copies : deux `VACUUM INTO` d'une même base donnent les mêmes
/// octets, que la base ait encore des pages dans son `-wal` ou non, des pages libres ou non, et
/// quelle que soit la connexion. Une copie identique déjà là est donc gardée, sans rien de plus.
#[tokio::test]
async fn same_database_gives_the_same_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let pool = lot1_database(&db).await;
    let vault = lot1_vault(&pool).await;
    for i in 0..200 {
        lot1_page(&pool, vault, &format!("Page {i:03}"), &format!("a{i:03}")).await;
    }
    sqlx::query("DELETE FROM nodes WHERE title LIKE 'Page %5'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(std::fs::metadata(sibling(&db, "-wal")).unwrap().len() > 0);

    // Une première copie quand les pages sont encore dans le `-wal`, une deuxième par une autre
    // connexion, une troisième après fermeture (le `-wal` est alors reversé dans la base).
    let copies = tempfile::tempdir().unwrap();
    let (first, second, third) = (
        copies.path().join("1.db"),
        copies.path().join("2.db"),
        copies.path().join("3.db"),
    );
    vacuum_into(&pool, &first).await;
    let other = common::side_pool(&db).await;
    vacuum_into(&other, &second).await;
    other.close().await;
    pool.close().await;
    assert!(!sibling(&db, "-wal").exists());
    let reopened = common::side_pool(&db).await;
    vacuum_into(&reopened, &third).await;
    reopened.close().await;
    let bytes = std::fs::read(&first).unwrap();
    assert_eq!(std::fs::read(&second).unwrap(), bytes);
    assert_eq!(std::fs::read(&third).unwrap(), bytes);

    // Posée au nom de la copie, elle est gardée : la copie fraîche, identique, ne laisse rien.
    let backup = backup_path(&db, 2);
    std::fs::copy(&first, &backup).unwrap();
    let store = Store::open(&db).await.unwrap();
    assert_eq!(
        store.backup(),
        Some(&Backup {
            path: backup.clone(),
            bytes: bytes.len() as u64,
            kept: true,
            without_hard_link: false,
        })
    );
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await,
        MIGRATIONS
    );
    store.close().await;
    assert_eq!(std::fs::read(&backup).unwrap(), bytes);
    assert_eq!(
        entries(dir.path()),
        BTreeSet::from([
            "mementum.db".to_string(),
            "mementum.db.avant-0002.bak".to_string()
        ])
    );
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
            without_hard_link: false,
        })
    );
    // Puis les migrations ont tourné.
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(version AS TEXT) FROM _sqlx_migrations ORDER BY version"
        )
        .await,
        [
            Some("1".to_string()),
            Some("2".to_string()),
            Some("3".to_string())
        ]
    );
    store.close().await;
    pool.close().await;
    assert!(!sibling(&backup, "-wal").exists());
    assert!(!sibling(&backup, "-shm").exists());
    assert!(!sibling(&backup, ".partiel").exists());

    // La copie se lit seule. C'est la base du lot 1, avec ses trois pages.
    let (_elsewhere, copy) = open_alone(&backup).await;
    // `VACUUM INTO` écrit une base en mode de journal classique : elle n'a jamais besoin de `-shm`,
    // même sur un support en lecture seule.
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&copy)
        .await
        .unwrap();
    assert_eq!(mode, "delete");
    assert_eq!(lot_tables(&copy).await, ["orgs", "vaults"]);
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

/// Une base du lot 2 : la copie porte le nom de la migration `0003`, et c'est la base du lot 2,
/// sans la version des espaces ni les tables de la collaboration.
#[tokio::test]
async fn lot2_database_is_backed_up_before_0003() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let pool = database_with(&db, LOT2).await;
    let space = Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO spaces (id, name, rev, created_at) VALUES (?, 'Équipe', 3, 1000)")
        .bind(&space)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO nodes (id, space_id, kind, title, position, created_at, updated_at) \
         VALUES (?, ?, 'page', 'Accueil', 'a0', 2000, 2000)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(&space)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let store = Store::open(&db).await.unwrap();
    let backup = backup_path(&db, 3);
    assert_eq!(backup, dir.path().join("mementum.db.avant-0003.bak"));
    assert_eq!(
        store.backup(),
        Some(&Backup {
            path: backup.clone(),
            bytes: std::fs::metadata(&backup).unwrap().len(),
            kept: false,
            without_hard_link: false,
        })
    );
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await,
        MIGRATIONS
    );
    store.close().await;
    assert_eq!(
        entries(dir.path()),
        BTreeSet::from([
            "mementum.db".to_string(),
            "mementum.db.avant-0003.bak".to_string()
        ])
    );

    let (_elsewhere, copy) = open_alone(&backup).await;
    assert_eq!(lot_tables(&copy).await, ["spaces"]);
    assert_eq!(titles(&copy).await, ["Accueil"]);
    let versions: Vec<i64> = sqlx::query("SELECT version FROM _sqlx_migrations ORDER BY version")
        .fetch_all(&copy)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(versions, [1, 2]);
    let lot3: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM pragma_table_info('spaces') WHERE name = 'lsn') \
              + (SELECT COUNT(*) FROM sqlite_master WHERE name = 'comment_threads')",
    )
    .fetch_one(&copy)
    .await
    .unwrap();
    assert_eq!(lot3, 0);
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

    // Un fichier porte déjà le nom de la copie, et ce n'est pas la base d'aujourd'hui.
    let backup = backup_path(&db, 2);
    let before = b"copie laissee par une montee de version precedente".to_vec();
    std::fs::write(&backup, &before).unwrap();

    // Il n'est pas écrasé : la copie fraîche prend un nom daté, à côté.
    let store = Store::open(&db).await.unwrap();
    assert_eq!(std::fs::read(&backup).unwrap(), before);
    let dated = dated_copies(&db);
    assert_eq!(dated.len(), 1, "{dated:?}");
    assert_eq!(
        store.backup(),
        Some(&Backup {
            path: dated[0].clone(),
            bytes: std::fs::metadata(&dated[0]).unwrap().len(),
            kept: false,
            without_hard_link: false,
        })
    );
    // L'ouverture a continué : la base est migrée.
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await,
        MIGRATIONS
    );
    store.close().await;
    assert_eq!(entries(dir.path()).len(), 3);
    assert!(!sibling(&backup, ".partiel").exists());

    let (_elsewhere, copy) = open_alone(&dated[0]).await;
    assert_eq!(lot_tables(&copy).await, ["orgs", "vaults"]);
    assert_eq!(titles(&copy).await, ["Accueil"]);
    copy.close().await;
}

/// L'ouverture échoue, et c'est la migration qui échoue, sur la panne posée par le test : pas
/// la copie, ni autre chose.
async fn migration_fails(db: &Path, attempt: &str) {
    let error = Store::open(db)
        .await
        .err()
        .unwrap_or_else(|| panic!("{attempt} : la migration doit échouer"));
    assert!(
        error.to_string().contains("panne simulée"),
        "{attempt} : {error}"
    );
}

/// La migration échoue deux fois sur une base qui n'a pas bougé entre-temps : une seule copie.
#[tokio::test]
async fn unchanged_database_is_not_copied_twice() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    lot1_with_a_page(&db).await;
    // Un déclencheur refuse que la migration soit notée dans `_sqlx_migrations` : sqlx annule
    // alors toute sa transaction, migration comprise.
    common::break_table(&db, "INSERT", "_sqlx_migrations", "1").await;

    let backup = backup_path(&db, 2);
    let mut first = None;
    for attempt in 1..=2 {
        let error = Store::open(&db)
            .await
            .err()
            .expect("la migration doit échouer");
        assert!(
            error.to_string().contains("panne simulée"),
            "tentative {attempt} : {error}"
        );
        assert_eq!(int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await, 1);
        let bytes = std::fs::read(&backup).unwrap();
        assert_eq!(*first.get_or_insert_with(|| bytes.clone()), bytes);
        assert_eq!(
            entries(dir.path()),
            BTreeSet::from([
                "mementum.db".to_string(),
                "mementum.db.avant-0002.bak".to_string()
            ]),
            "tentative {attempt}"
        );
    }
}

/// Un redémarrage automatique sur une migration qui échoue : une copie sans date existe, la base a
/// changé depuis, puis trois tentatives de suite échouent. La première écrit une copie datée ; les
/// suivantes la trouvent identique. Deux copies en tout, pas quatre.
#[tokio::test]
async fn retry_loop_does_not_pile_up_copies() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let pool = lot1_database(&db).await;
    let vault = lot1_vault(&pool).await;
    lot1_page(&pool, vault, "Accueil", "a0").await;
    pool.close().await;
    common::break_table(&db, "INSERT", "_sqlx_migrations", "1").await;
    migration_fails(&db, "première tentative").await;
    assert!(backup_path(&db, 2).is_file());

    let old = common::side_pool(&db).await;
    lot1_page(&old, vault, "Écrite entre-temps", "a1").await;
    old.close().await;

    let mut dated_bytes = None;
    for attempt in 1..=3 {
        migration_fails(&db, &format!("tentative {attempt}")).await;
        let dated = dated_copies(&db);
        assert_eq!(dated.len(), 1, "tentative {attempt} : {dated:?}");
        let bytes = std::fs::read(&dated[0]).unwrap();
        assert_eq!(*dated_bytes.get_or_insert_with(|| bytes.clone()), bytes);
        assert_eq!(entries(dir.path()).len(), 3, "tentative {attempt}");
    }
}

/// La migration échoue, on revient à l'ancienne version et l'on écrit, puis la version corrigée
/// arrive : la copie d'avant la première tentative ne porte pas ce qui a été écrit depuis.
#[tokio::test]
async fn changed_database_gets_a_fresh_dated_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let pool = lot1_database(&db).await;
    let vault = lot1_vault(&pool).await;
    lot1_page(&pool, vault, "Accueil", "a0").await;
    pool.close().await;

    // Première tentative : la copie est faite, la migration échoue.
    common::break_table(&db, "INSERT", "_sqlx_migrations", "1").await;
    migration_fails(&db, "première tentative").await;
    let backup = backup_path(&db, 2);
    let first = std::fs::read(&backup).unwrap();

    // L'ancienne version écrit une page, avec le schéma du lot 1.
    let old = common::side_pool(&db).await;
    lot1_page(&old, vault, "Écrite entre-temps", "a1").await;
    old.close().await;

    // La version corrigée : une seconde copie, datée, porte la page ; la première est intacte.
    common::repair(&db).await;
    let store = Store::open(&db).await.unwrap();
    let dated = dated_copies(&db);
    assert_eq!(dated.len(), 1, "{dated:?}");
    assert_eq!(
        store.backup(),
        Some(&Backup {
            path: dated[0].clone(),
            bytes: std::fs::metadata(&dated[0]).unwrap().len(),
            kept: false,
            without_hard_link: false,
        })
    );
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await,
        MIGRATIONS
    );
    store.close().await;
    assert_eq!(std::fs::read(&backup).unwrap(), first);
    assert_eq!(entries(dir.path()).len(), 3);

    let (_elsewhere, copy) = open_alone(&dated[0]).await;
    assert_eq!(lot_tables(&copy).await, ["orgs", "vaults"]);
    assert_eq!(titles(&copy).await, ["Accueil", "Écrite entre-temps"]);
    copy.close().await;
    let (_elsewhere, copy) = open_alone(&backup).await;
    assert_eq!(titles(&copy).await, ["Accueil"]);
    copy.close().await;
}

/// Sans migration en attente, l'ouverture ne touche à aucun fichier de copie, même temporaire.
#[tokio::test]
async fn partial_is_left_alone_when_nothing_is_pending() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    lot1_with_a_page(&db).await;
    Store::open(&db).await.unwrap().close().await;

    let partial = sibling(&backup_path(&db, 2), ".partiel");
    std::fs::write(&partial, b"reste d'une copie").unwrap();
    let store = Store::open(&db).await.unwrap();
    assert_eq!(store.backup(), None);
    store.close().await;
    assert_eq!(std::fs::read(&partial).unwrap(), b"reste d'une copie");
}

/// Le chemin de la copie passe à SQLite en paramètre lié : une apostrophe ou des accents n'y
/// changent rien.
#[tokio::test]
async fn backup_in_a_folder_with_quote_and_accents() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("l'été").join("mementum.db");
    std::fs::create_dir(db.parent().unwrap()).unwrap();
    lot1_with_a_page(&db).await;

    let store = Store::open(&db).await.unwrap();
    let backup = dir.path().join("l'été").join("mementum.db.avant-0002.bak");
    assert_eq!(store.backup().map(|b| &b.path), Some(&backup));
    store.close().await;

    let (_elsewhere, copy) = open_alone(&backup).await;
    assert_eq!(titles(&copy).await, ["Accueil"]);
    copy.close().await;
}

#[tokio::test]
async fn interrupted_backup_is_redone() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let pool = lot1_database(&db).await;
    let vault = lot1_vault(&pool).await;
    lot1_page(&pool, vault, "Accueil", "a0").await;
    lot1_page(&pool, vault, "Notes", "a1").await;
    pool.close().await;

    // Une coupure pendant `VACUUM INTO` a laissé la copie à moitié écrite, sous son nom temporaire :
    // la première page d'une base SQLite, sans la suite.
    let backup = backup_path(&db, 2);
    let partial = sibling(&backup, ".partiel");
    std::fs::write(&partial, &std::fs::read(&db).unwrap()[..4096]).unwrap();

    let store = Store::open(&db).await.unwrap();
    let bytes = std::fs::metadata(&backup).unwrap().len();
    assert_eq!(
        store.backup(),
        Some(&Backup {
            path: backup.clone(),
            bytes,
            kept: false,
            without_hard_link: false,
        })
    );
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await,
        MIGRATIONS
    );
    store.close().await;
    assert!(!partial.exists());
    assert_eq!(
        entries(dir.path()),
        BTreeSet::from([
            "mementum.db".to_string(),
            "mementum.db.avant-0002.bak".to_string()
        ])
    );

    // La copie refaite se lit seule, avec toutes les pages.
    let (_elsewhere, copy) = open_alone(&backup).await;
    assert_eq!(lot_tables(&copy).await, ["orgs", "vaults"]);
    assert_eq!(titles(&copy).await, ["Accueil", "Notes"]);
    copy.close().await;
}

#[tokio::test]
async fn failed_backup_stops_before_migrating() {
    // La copie est rendue impossible par un dossier : au nom de la copie (elle s'écrit, mais ne peut
    // pas prendre son nom), puis au nom temporaire (ce n'est pas le reste d'une copie interrompue,
    // rien ne le supprime).
    for obstacle in [
        "mementum.db.avant-0002.bak",
        "mementum.db.avant-0002.bak.partiel",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("mementum.db");
        lot1_with_a_page(&db).await;
        let blocked = dir.path().join(obstacle);
        std::fs::create_dir(&blocked).unwrap();

        let error = Store::open(&db)
            .await
            .err()
            .expect("l'ouverture doit échouer");
        assert!(
            error
                .to_string()
                .contains(&format!("{} avant la migration", blocked.display())),
            "{obstacle} : l'erreur nomme l'obstacle : {error}"
        );

        // La base est restée au lot 1, et rien n'a été laissé à côté.
        assert_eq!(
            int(
                &db,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('orgs', 'vaults')"
            )
            .await,
            2,
            "{obstacle}"
        );
        assert_eq!(
            int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await,
            1,
            "{obstacle}"
        );
        assert_eq!(
            int(&db, "SELECT COUNT(*) FROM nodes").await,
            1,
            "{obstacle}"
        );
        assert!(blocked.is_dir(), "{obstacle}");
        assert!(entries(&blocked).is_empty(), "{obstacle}");
        assert_eq!(
            entries(dir.path()),
            BTreeSet::from(["mementum.db".to_string(), obstacle.to_string()]),
            "{obstacle}"
        );
    }
}
