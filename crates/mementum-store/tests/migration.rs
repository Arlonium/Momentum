//! Migration `0002` : une base du lot 1 garde ses pages, une base neuve n'a pas d'espace.

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use common::{int, side_pool, texts};
use mementum_acl::{Grant, Level, Subject};
use mementum_store::{NodeKind, Store, StoreError};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

/// Une base telle que le code du lot 1 la produit : les options de connexion de `Store::open`,
/// et la seule migration `0001_init.sql`, lancée par sqlx (qui écrit donc `_sqlx_migrations`,
/// somme de contrôle comprise).
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

/// Une page du lot 1, écrite en SQL.
async fn lot1_page(
    pool: &SqlitePool,
    (org, vault): (Uuid, Uuid),
    parent: Option<Uuid>,
    title: &str,
    position: &str,
    at: i64,
) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO nodes (id, org_id, vault_id, parent_id, kind, title, position, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, 'page', ?5, ?6, ?7, ?8)",
    )
    .bind(id.to_string())
    .bind(org.to_string())
    .bind(vault.to_string())
    .bind(parent.map(|p| p.to_string()))
    .bind(title)
    .bind(position)
    .bind(at)
    .bind(at + 5)
    .execute(pool)
    .await
    .unwrap();
    id
}

/// Les tables que vise chaque clé étrangère de `table`.
async fn foreign_tables(db: &Path, table: &str) -> BTreeSet<String> {
    let pool = side_pool(db).await;
    let targets = sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA foreign_key_list({table})"
    )))
    .fetch_all(&pool)
    .await
    .unwrap()
    .iter()
    .map(|r| r.get::<String, _>("table"))
    .collect();
    pool.close().await;
    targets
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

/// `sql` est-il refusé par une clé étrangère ?
async fn refused_by_foreign_key(db: &Path, sql: &str) -> bool {
    let pool = side_pool(db).await;
    let result = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&pool)
        .await;
    pool.close().await;
    matches!(result, Err(sqlx::Error::Database(e)) if e.is_foreign_key_violation())
}

#[tokio::test]
async fn migration_keeps_lot1_pages() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");

    // Une base du lot 1 : une organisation, un coffre, trois pages dont une sous-page, un
    // instantané et deux mises à jour.
    let pool = lot1_database(&db).await;
    let (org, vault) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query(
        "INSERT INTO orgs (id, slug, name, created_at) VALUES (?, 'default', 'default', 1000)",
    )
    .bind(org.to_string())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO vaults (id, org_id, name, kind, created_at) VALUES (?, ?, 'Mon coffre', 'personal', 1001)",
    )
    .bind(vault.to_string())
    .bind(org.to_string())
    .execute(&pool)
    .await
    .unwrap();
    // La sous-page est plus ancienne que son parent (créée à la racine, puis déplacée) : la
    // copie doit accepter un enfant lu avant son parent.
    let c = lot1_page(&pool, (org, vault), None, "Sous-page", "a0V", 4000).await;
    let a = lot1_page(&pool, (org, vault), None, "Accueil", "a0", 2000).await;
    let b = lot1_page(&pool, (org, vault), None, "  Brouillon ", "a1", 3000).await;
    sqlx::query("UPDATE nodes SET parent_id = ? WHERE id = ?")
        .bind(a.to_string())
        .bind(c.to_string())
        .execute(&pool)
        .await
        .unwrap();
    // Des octets qui ne sont pas de l'UTF-8 : la copie doit être exacte.
    let snapshot: &[u8] = &[0x00, 0xff, 0x01, 0xfe, 0x80, 0x0a, 0x0d];
    let update_a: &[u8] = &[0xc3, 0x28, 0x00, 0x00, 0x7f];
    let update_c: &[u8] = &[0xde, 0xad, 0xbe, 0xef];
    sqlx::query(
        "INSERT INTO doc_snapshots (node_id, seq, state_blob, created_at) VALUES (?, 2, ?, 2100)",
    )
    .bind(a.to_string())
    .bind(snapshot)
    .execute(&pool)
    .await
    .unwrap();
    for (node, seq, blob) in [(a, 3, update_a), (c, 1, update_c)] {
        sqlx::query(
            "INSERT INTO doc_updates (node_id, seq, update_blob, created_at) VALUES (?, ?, ?, 2200)",
        )
        .bind(node.to_string())
        .bind(seq)
        .bind(blob)
        .execute(&pool)
        .await
        .unwrap();
    }
    pool.close().await;

    // Le code du lot 2 ouvre cette base : `0001` est reconnue (même somme de contrôle), `0002`
    // est appliquée.
    let store = Store::open(&db).await.unwrap();
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(version AS TEXT) FROM _sqlx_migrations WHERE success ORDER BY version"
        )
        .await,
        [Some("1".to_string()), Some("2".to_string())]
    );

    // Le coffre est devenu un espace de même id, sans membre.
    assert_eq!(int(&db, "SELECT COUNT(*) FROM spaces").await, 1);
    let space = store.get_space(vault).await.unwrap().expect("espace");
    assert_eq!(space.id, vault);
    assert_eq!(space.name, "Mon espace");
    assert_eq!(space.rev, 0);
    assert_eq!(space.created_at, 1001);
    assert_eq!(int(&db, "SELECT COUNT(*) FROM members").await, 0);
    assert_eq!(
        int(
            &db,
            "SELECT COUNT(*) FROM spaces WHERE created_by IS NOT NULL"
        )
        .await,
        0
    );

    // Les trois nœuds, avec leur id, leur parent, leur place et leur titre.
    let nodes = store.list_nodes(vault).await.unwrap();
    assert_eq!(nodes.len(), 3);
    for (id, parent, title, position, at) in [
        (a, None, "Accueil", "a0", 2000),
        (b, None, "  Brouillon ", "a1", 3000),
        (c, Some(a), "Sous-page", "a0V", 4000),
    ] {
        let node = store.get_node(id).await.unwrap().expect("nœud");
        assert!(nodes.contains(&node));
        assert_eq!(node.space_id, vault);
        assert_eq!(node.parent_id, parent, "{title}");
        assert_eq!(node.kind, NodeKind::Page);
        assert_eq!(node.title, title);
        assert_eq!(node.position, position);
        assert_eq!(node.created_at, at);
        assert_eq!(node.updated_at, at + 5);
        assert_eq!(node.created_by, None);
        assert_eq!(node.updated_by, None);
        assert_eq!(node.trashed_at, None);
    }

    // Les documents, octet pour octet.
    let doc = store.load_doc(a).await.unwrap();
    assert_eq!(doc.snapshot, Some((2, snapshot.to_vec())));
    assert_eq!(doc.updates, vec![(3, update_a.to_vec())]);
    let doc = store.load_doc(c).await.unwrap();
    assert_eq!(doc.snapshot, None);
    assert_eq!(doc.updates, vec![(1, update_c.to_vec())]);
    let doc = store.load_doc(b).await.unwrap();
    assert!(doc.snapshot.is_none() && doc.updates.is_empty());
    assert_eq!(
        int(
            &db,
            "SELECT COUNT(*) FROM doc_updates WHERE created_at = 2200 AND actor_id IS NULL"
        )
        .await,
        2
    );
    assert_eq!(
        int(
            &db,
            "SELECT COUNT(*) FROM doc_snapshots WHERE created_at = 2100"
        )
        .await,
        1
    );

    // Les pages de racine vont à tous les membres ; la sous-page hérite.
    let members_full = Grant {
        subject: Subject::Members,
        level: Level::Full,
    };
    assert_eq!(store.node_grants(a).await.unwrap(), [members_full]);
    assert_eq!(store.node_grants(b).await.unwrap(), [members_full]);
    assert_eq!(store.node_grants(c).await.unwrap(), []);
    let mut granted: Vec<Uuid> = store
        .space_grants(vault)
        .await
        .unwrap()
        .into_iter()
        .map(|(node, grant)| {
            assert_eq!(grant, members_full);
            node
        })
        .collect();
    granted.sort();
    assert_eq!(granted, [a, b]);

    // Plus d'organisations ni de coffres, aucune table de travail oubliée.
    let tables: Vec<String> = texts(
        &db,
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .await
    .into_iter()
    .flatten()
    .collect();
    assert_eq!(
        tables,
        [
            "_sqlx_migrations",
            "acl",
            "doc_snapshots",
            "doc_updates",
            "instance_settings",
            "invitations",
            "members",
            "nodes",
            "sessions",
            "spaces",
            "users",
        ]
    );
    // Les index : celui de l'arbre, et ceux des clés que suit la suppression d'un nœud ; l'index
    // de passage de la migration est parti avec l'ancienne table.
    let indexes: Vec<String> = texts(
        &db,
        "SELECT name FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .await
    .into_iter()
    .flatten()
    .collect();
    assert_eq!(
        indexes,
        [
            "invitations_node_key",
            "nodes_by_parent",
            "nodes_parent_key"
        ]
    );
    for (index, columns) in [
        ("invitations_node_key", &["node_id"][..]),
        ("nodes_by_parent", &["space_id", "parent_id", "position"]),
        ("nodes_parent_key", &["parent_id"]),
    ] {
        let found: Vec<String> = texts(
            &db,
            &format!("SELECT name FROM pragma_index_info('{index}') ORDER BY seqno"),
        )
        .await
        .into_iter()
        .flatten()
        .collect();
        assert_eq!(found, columns, "{index}");
    }

    // Les clés étrangères tiennent, et celles des tables recréées visent les nouvelles tables.
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM pragma_foreign_key_check").await,
        0
    );
    assert_eq!(
        foreign_tables(&db, "nodes").await,
        set(&["nodes", "spaces", "users"])
    );
    assert_eq!(
        foreign_tables(&db, "doc_updates").await,
        set(&["nodes", "users"])
    );
    assert_eq!(foreign_tables(&db, "doc_snapshots").await, set(&["nodes"]));
    assert_eq!(foreign_tables(&db, "acl").await, set(&["nodes"]));
    assert_eq!(
        foreign_tables(&db, "invitations").await,
        set(&["nodes", "spaces", "users"])
    );
    assert_eq!(
        foreign_tables(&db, "members").await,
        set(&["spaces", "users"])
    );
    for faulty in [
        "INSERT INTO nodes (id, space_id, kind, title, position, created_at, updated_at) \
         VALUES ('n', 'espace-inconnu', 'page', 'x', 'a0', 1, 1)",
        "INSERT INTO nodes (id, space_id, parent_id, kind, title, position, created_at, updated_at) \
         SELECT 'n', id, 'parent-inconnu', 'page', 'x', 'a0', 1, 1 FROM spaces",
        "INSERT INTO doc_updates (node_id, seq, update_blob, created_at) VALUES ('nœud-inconnu', 1, x'00', 1)",
        "INSERT INTO doc_snapshots (node_id, seq, state_blob, created_at) VALUES ('nœud-inconnu', 1, x'00', 1)",
        "INSERT INTO acl (node_id, subject, level) VALUES ('nœud-inconnu', 'members', 'read')",
    ] {
        assert!(refused_by_foreign_key(&db, faulty).await, "{faulty}");
    }

    // Et l'on continue d'écrire dans ce qui a été migré : une page après la dernière racine, une
    // mise à jour à la suite du journal.
    let d = store
        .create_node(common::page(&space, None, Some(b), "Neuve"))
        .await
        .unwrap();
    assert!(d.position.as_str() > "a1");
    assert_eq!(store.append_updates(a, &[b"suite"], None).await.unwrap(), 4);
    assert!(matches!(
        store.append_updates(Uuid::now_v7(), &[b"x"], None).await,
        Err(StoreError::NotFound)
    ));
    store.close().await;

    // Une seconde ouverture ne migre plus rien.
    let store = Store::open(&db).await.unwrap();
    assert_eq!(store.list_nodes(vault).await.unwrap().len(), 4);
    assert_eq!(
        store.load_doc(a).await.unwrap().snapshot,
        Some((2, snapshot.to_vec()))
    );
    store.close().await;
}

/// Review Focus 4, pour ce que le test ci-dessus ne porte pas : une base du lot 1 à deux
/// coffres, dont une page est à la corbeille avec sa sous-page. Chaque coffre devient son
/// espace, avec ses pages et elles seules ; la corbeille est gardée telle quelle (sa date, sans
/// auteur : le lot 1 n'en avait pas), et une page de racine à la corbeille reçoit l'accès des
/// membres comme les autres : restaurée, elle est à eux.
#[tokio::test]
async fn migration_keeps_every_vault_and_the_trash() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let pool = lot1_database(&db).await;
    let org = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO orgs (id, slug, name, created_at) VALUES (?, 'default', 'default', 1000)",
    )
    .bind(org.to_string())
    .execute(&pool)
    .await
    .unwrap();
    let (home, work) = (Uuid::now_v7(), Uuid::now_v7());
    for (vault, name, kind, at) in [
        (home, "Mon coffre", "personal", 1001),
        (work, "Travail", "shared", 1002),
    ] {
        sqlx::query(
            "INSERT INTO vaults (id, org_id, name, kind, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(vault.to_string())
        .bind(org.to_string())
        .bind(name)
        .bind(kind)
        .bind(at)
        .execute(&pool)
        .await
        .unwrap();
    }
    let kept = lot1_page(&pool, (org, home), None, "Gardée", "a0", 2000).await;
    let binned = lot1_page(&pool, (org, home), None, "Jetée", "a1", 3000).await;
    let under = lot1_page(
        &pool,
        (org, home),
        Some(binned),
        "Sous la jetée",
        "a0",
        4000,
    )
    .await;
    let at_work = lot1_page(&pool, (org, work), None, "Au travail", "a0", 5000).await;
    sqlx::query("UPDATE nodes SET trashed_at = 3500 WHERE id = ?")
        .bind(binned.to_string())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let store = Store::open(&db).await.unwrap();
    // Deux espaces, de même id que les coffres, sans membre.
    assert_eq!(int(&db, "SELECT COUNT(*) FROM spaces").await, 2);
    assert_eq!(int(&db, "SELECT COUNT(*) FROM members").await, 0);
    for (vault, created_at) in [(home, 1001), (work, 1002)] {
        let space = store.get_space(vault).await.unwrap().expect("espace");
        assert_eq!((space.rev, space.created_at), (0, created_at));
    }
    // Chacun a ses pages, et elles seules.
    let ids = async |space: Uuid| {
        let mut ids: Vec<Uuid> = store
            .list_nodes(space)
            .await
            .unwrap()
            .iter()
            .map(|n| n.id)
            .collect();
        ids.sort();
        ids
    };
    let mut at_home = vec![kept, binned, under];
    at_home.sort();
    assert_eq!(ids(home).await, at_home);
    assert_eq!(ids(work).await, [at_work]);

    // La corbeille : la page y est toujours, depuis la même date ; sa sous-page y est par
    // héritage, sans date à elle ; les autres n'y sont pas.
    let node = async |id: Uuid| store.get_node(id).await.unwrap().expect("nœud");
    assert_eq!(node(binned).await.trashed_at, Some(3500));
    assert_eq!(node(under).await.parent_id, Some(binned));
    for id in [under, kept, at_work] {
        assert_eq!(node(id).await.trashed_at, None);
    }
    assert_eq!(store.trashed_since(under).await.unwrap(), Some(3500));
    assert_eq!(store.trashed_since(kept).await.unwrap(), None);
    assert_eq!(
        int(
            &db,
            "SELECT COUNT(*) FROM nodes WHERE trashed_by IS NOT NULL"
        )
        .await,
        0
    );
    // Les pages de racine vont aux membres, celle de la corbeille comprise ; la sous-page
    // hérite.
    let members_full = Grant {
        subject: Subject::Members,
        level: Level::Full,
    };
    for root in [kept, binned, at_work] {
        assert_eq!(store.node_grants(root).await.unwrap(), [members_full]);
    }
    assert_eq!(store.node_grants(under).await.unwrap(), []);
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM pragma_foreign_key_check").await,
        0
    );
    // Elle se restaure, et se supprime, comme une page jetée depuis.
    let restored = store.restore_node(binned).await.unwrap();
    assert_eq!(restored.trashed_at, None);
    assert_eq!(store.trashed_since(under).await.unwrap(), None);
    store.close().await;
}

#[tokio::test]
async fn fresh_database_has_no_space() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let store = Store::open(&db).await.unwrap();
    assert_eq!(int(&db, "SELECT COUNT(*) FROM spaces").await, 0);
    assert_eq!(
        int(
            &db,
            "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('orgs', 'vaults') OR name LIKE '%\\_new' ESCAPE '\\'"
        )
        .await,
        0
    );
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM pragma_foreign_key_check").await,
        0
    );
    store.close().await;
}

/// Un compte peut ne pas avoir de mot de passe (connexion par OIDC, prévue plus tard) : SQLite
/// ne lève pas un `NOT NULL` sans reconstruire la table et tout ce qui la vise.
#[tokio::test]
async fn account_without_password_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let store = Store::open(&db).await.unwrap();
    common::exec_sql(
        &db,
        "INSERT INTO users (id, email, name, password_hash, created_at) \
         VALUES ('u', 'sans-mot-de-passe@exemple.test', 'Sans mot de passe', NULL, 1)",
    )
    .await;
    assert_eq!(
        int(
            &db,
            "SELECT COUNT(*) FROM users WHERE password_hash IS NULL"
        )
        .await,
        1
    );
    store.close().await;
}
