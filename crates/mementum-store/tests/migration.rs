//! Migrations `0002` à `0004` : une base du lot 1 garde ses pages, une base du lot 2 gagne les
//! tables de la collaboration sans rien perdre, une base de la partie A du lot 3 gagne celles des
//! fichiers et des favoris sans rien perdre, une base neuve n'a pas d'espace.

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use common::{LOT2, LOT3A, database_with, int, side_pool, texts};
use mementum_acl::{Grant, Level, Subject};
use mementum_store::{NodeKind, Store, StoreError, backup_path};
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

/// Les tables d'une base à jour, par nom.
const ALL_TABLES: [&str; 19] = [
    "_sqlx_migrations",
    "acl",
    "attachments",
    "block_moves",
    "comment_reactions",
    "comment_threads",
    "comments",
    "doc_snapshots",
    "doc_updates",
    "doc_versions",
    "favorites",
    "files",
    "instance_settings",
    "invitations",
    "members",
    "nodes",
    "sessions",
    "spaces",
    "users",
];

/// Les tables des fichiers et des favoris, que la migration `0004` crée (Ruling R243).
const FILES_AND_FAVORITES_TABLES: [&str; 3] = ["files", "attachments", "favorites"];

/// Les tables de la collaboration, que la migration `0003` crée et que les tâches suivantes du
/// lot remplissent.
const COLLABORATION_TABLES: [&str; 5] = [
    "comment_threads",
    "comments",
    "comment_reactions",
    "doc_versions",
    "block_moves",
];

/// Les noms des tables de la base, hors tables internes de SQLite.
async fn table_names(db: &Path) -> Vec<String> {
    texts(
        db,
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .await
    .into_iter()
    .flatten()
    .collect()
}

/// Les colonnes de `table`, dans l'ordre du schéma.
async fn columns(db: &Path, table: &str) -> Vec<String> {
    texts(
        db,
        &format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid"),
    )
    .await
    .into_iter()
    .flatten()
    .collect()
}

/// Les lignes de `table`, chacune écrite par `quote()` de SQLite sur ces colonnes (octets d'un
/// blob compris, `NULL` distinct du texte), triées : de quoi comparer une table avant et après
/// une migration, valeur pour valeur.
async fn rows(db: &Path, table: &str, columns: &[String]) -> Vec<String> {
    let cells = columns
        .iter()
        .map(|c| format!("quote(\"{c}\")"))
        .collect::<Vec<_>>()
        .join(" || '|' || ");
    let mut rows: Vec<String> = texts(db, &format!("SELECT {cells} FROM {table}"))
        .await
        .into_iter()
        .flatten()
        .collect();
    rows.sort();
    rows
}

/// Exécute `sql` sur `pool`, ses valeurs liées dans l'ordre.
async fn exec(pool: &SqlitePool, sql: &str, values: &[&str]) {
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()));
    for value in values {
        query = query.bind(*value);
    }
    query.execute(pool).await.unwrap();
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

    // Le code d'aujourd'hui ouvre cette base : `0001` est reconnue (même somme de contrôle),
    // `0002` à `0004` sont appliquées.
    let store = Store::open(&db).await.unwrap();
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(version AS TEXT) FROM _sqlx_migrations WHERE success ORDER BY version"
        )
        .await,
        [
            Some("1".to_string()),
            Some("2".to_string()),
            Some("3".to_string()),
            Some("4".to_string())
        ]
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
    assert_eq!(tables, ALL_TABLES);
    // Les index : celui de l'arbre, ceux des clés que suit la suppression d'un nœud, ceux des
    // tables de la collaboration, et ceux des fichiers et des favoris ; l'index de passage de la
    // migration `0002` est parti avec l'ancienne table.
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
            "attachments_by_file",
            "attachments_by_node",
            "comment_threads_by_node",
            "comments_by_thread",
            "doc_versions_by_node",
            "favorites_by_node",
            "invitations_node_key",
            "nodes_by_parent",
            "nodes_parent_key"
        ]
    );
    for (index, columns) in [
        ("attachments_by_file", &["file_hash"][..]),
        ("attachments_by_node", &["node_id", "created_at"]),
        ("comment_threads_by_node", &["node_id", "created_at"]),
        (
            "comments_by_thread",
            &["thread_id", "created_at", "author_id", "(expression)", "id"],
        ),
        ("doc_versions_by_node", &["node_id", "seq", "created_at"]),
        ("favorites_by_node", &["node_id"]),
        ("invitations_node_key", &["node_id"]),
        ("nodes_by_parent", &["space_id", "parent_id", "position"]),
        ("nodes_parent_key", &["parent_id"]),
    ] {
        let found: Vec<String> = texts(
            &db,
            &format!(
                "SELECT coalesce(name, '(expression)') FROM pragma_index_info('{index}') \
                 ORDER BY seqno"
            ),
        )
        .await
        .into_iter()
        .flatten()
        .collect();
        assert_eq!(found, columns, "{index}");
    }
    // L'expression de `comments_by_thread`, que le plafond du texte d'une page reprend telle
    // quelle pour lire sa somme dans l'index (Ruling R350).
    let sql = texts(
        &db,
        "SELECT sql FROM sqlite_master WHERE name = 'comments_by_thread'",
    )
    .await;
    assert_eq!(
        sql,
        [Some(
            "CREATE INDEX comments_by_thread ON comments \
             (thread_id, created_at, author_id, octet_length(body), id)"
                .to_string()
        )]
    );

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

/// Une base du lot 2, écrite en SQL comme le code du lot 2 l'écrit : deux comptes et une session,
/// deux espaces et leurs membres, des pages (une sous-page, une à la corbeille), des accès, deux
/// invitations (une en attente, une acceptée), un instantané et des mises à jour aux octets qui ne
/// sont pas de l'UTF-8. Rend les deux espaces.
async fn lot2_data(pool: &SqlitePool) -> (Uuid, Uuid) {
    let (olga, bob) = (Uuid::now_v7().to_string(), Uuid::now_v7().to_string());
    let (team, own) = (Uuid::now_v7(), Uuid::now_v7());
    let (team_s, own_s) = (team.to_string(), own.to_string());
    let [a, b, c, d] = [(); 4].map(|_| Uuid::now_v7().to_string());
    exec(
        pool,
        "INSERT INTO users (id, email, name, password_hash, instance_admin, created_at) \
         VALUES (?1, 'olga@exemple.test', 'Olga', 'h1', 1, 1000), \
                (?2, 'bob@exemple.test', 'Bob', NULL, 0, 1001)",
        &[&olga, &bob],
    )
    .await;
    exec(
        pool,
        "INSERT INTO sessions (token_hash, user_id, created_at, expires_at) \
         VALUES (x'0102', ?1, 1002, 9000000000000)",
        &[&olga],
    )
    .await;
    exec(
        pool,
        "INSERT INTO spaces (id, name, rev, created_by, created_at) \
         VALUES (?1, 'Équipe', 7, ?3, 1003), (?2, 'Perso', 2, ?3, 1004)",
        &[&team_s, &own_s, &olga],
    )
    .await;
    exec(
        pool,
        "INSERT INTO members (space_id, user_id, role, created_at) \
         VALUES (?1, ?3, 'owner', 1003), (?1, ?4, 'member', 1005), (?2, ?3, 'owner', 1004)",
        &[&team_s, &own_s, &olga, &bob],
    )
    .await;
    exec(
        pool,
        "INSERT INTO nodes (id, space_id, parent_id, kind, title, position, created_by, \
                            created_at, updated_by, updated_at, trashed_at, trashed_by) \
         VALUES (?1, ?5, NULL, 'page', 'Accueil', 'a0', ?7, 2000, ?8, 2100, NULL, NULL), \
                (?2, ?5, ?1, 'folder', ' Dossier ', 'a0', ?8, 2001, ?8, 2001, NULL, NULL), \
                (?3, ?5, NULL, 'page', 'Jetée', 'a1', NULL, 2002, NULL, 2002, 2500, ?7), \
                (?4, ?6, NULL, 'page', 'À moi', 'a0', ?7, 2003, ?7, 2003, NULL, NULL)",
        &[&a, &b, &c, &d, &team_s, &own_s, &olga, &bob],
    )
    .await;
    exec(
        pool,
        "INSERT INTO acl (node_id, subject, level) \
         VALUES (?1, 'members', 'full'), (?2, 'user:' || ?5, 'edit'), \
                (?3, 'user:' || ?6, 'full'), (?4, 'user:' || ?6, 'full')",
        &[&a, &b, &c, &d, &bob, &olga],
    )
    .await;
    exec(
        pool,
        "INSERT INTO invitations (id, space_id, node_id, email, grant_role, grant_level, \
                                  token_hash, created_by, created_at, expires_at, \
                                  accepted_at, accepted_by) \
         VALUES (?1, ?3, NULL, 'carl@exemple.test', 'member', NULL, x'aa', ?5, 3000, \
                 9000000000000, NULL, NULL), \
                (?2, ?3, ?4, 'bob@exemple.test', NULL, 'edit', x'bb', ?5, 3001, \
                 9000000000000, 3002, ?6)",
        &[
            &Uuid::now_v7().to_string(),
            &Uuid::now_v7().to_string(),
            &team_s,
            &b,
            &olga,
            &bob,
        ],
    )
    .await;
    exec(
        pool,
        "INSERT INTO doc_snapshots (node_id, seq, state_blob, created_at) \
         VALUES (?1, 2, x'00ff01fe800a0d', 2200)",
        &[&a],
    )
    .await;
    exec(
        pool,
        "INSERT INTO doc_updates (node_id, seq, update_blob, created_at, actor_id) \
         VALUES (?1, 3, x'c32800007f', 2300, ?3), (?2, 1, x'deadbeef', 2301, NULL)",
        &[&a, &b, &bob],
    )
    .await;
    exec(
        pool,
        "INSERT INTO instance_settings (key, value) VALUES ('réglage', 'valeur')",
        &[],
    )
    .await;
    (team, own)
}

/// Les migrations `0003` et `0004` sur une base du lot 2 : tout ce qui y était reste à
/// l'identique, valeur pour valeur ; chaque espace gagne sa version, à 0 ; les cinq tables de la
/// collaboration et les trois des fichiers et des favoris existent, vides, et les clés étrangères
/// des premières visent ce qu'il faut (celles des secondes : `lot3a_database_gains_files_and_favorites`).
#[tokio::test]
async fn lot2_database_gains_the_collaboration_tables() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let pool = database_with(&db, LOT2).await;
    let (team, own) = lot2_data(&pool).await;
    pool.close().await;

    // Ce que porte la base du lot 2, table par table, colonne par colonne.
    let lot2_tables = table_names(&db).await;
    let mut before = Vec::new();
    for table in &lot2_tables {
        let columns = columns(&db, table).await;
        let rows = rows(&db, table, &columns).await;
        before.push((table.clone(), columns, rows));
    }

    let store = Store::open(&db).await.unwrap();
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(version AS TEXT) FROM _sqlx_migrations WHERE success ORDER BY version"
        )
        .await,
        [
            Some("1".to_string()),
            Some("2".to_string()),
            Some("3".to_string()),
            Some("4".to_string())
        ]
    );

    // Rien n'a changé dans ce qui était là, `_sqlx_migrations` mise à part (elle gagne les lignes
    // de `0003` et `0004`) ; `spaces` gagne une colonne, et ses autres colonnes n'ont pas bougé.
    for (table, columns_before, rows_before) in &before {
        if table == "_sqlx_migrations" {
            continue;
        }
        let mut columns_after = columns(&db, table).await;
        if table == "spaces" {
            assert_eq!(columns_after.pop().as_deref(), Some("lsn"));
        }
        assert_eq!(&columns_after, columns_before, "{table}");
        assert_eq!(
            &rows(&db, table, columns_before).await,
            rows_before,
            "{table}"
        );
    }
    assert!(
        before
            .iter()
            .any(|(table, _, rows)| table == "acl" && rows.len() == 4)
    );

    // La version de chaque espace part de 0 ; `rev` garde la sienne.
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM spaces WHERE lsn != 0").await,
        0
    );
    for (space, rev) in [(team, 7), (own, 2)] {
        assert_eq!(store.space_lsn(space).await.unwrap(), Some(0));
        assert_eq!(store.space_rev(space).await.unwrap(), Some(rev));
    }
    assert_eq!(store.space_lsn(Uuid::now_v7()).await.unwrap(), None);

    // Les cinq tables de la collaboration et les trois des fichiers et des favoris, vides, et
    // rien d'autre de neuf.
    assert_eq!(table_names(&db).await, ALL_TABLES);
    for table in COLLABORATION_TABLES
        .into_iter()
        .chain(FILES_AND_FAVORITES_TABLES)
    {
        assert!(!lot2_tables.iter().any(|t| t == table), "{table}");
        assert_eq!(
            int(&db, &format!("SELECT COUNT(*) FROM {table}")).await,
            0,
            "{table}"
        );
    }
    assert_eq!(
        foreign_tables(&db, "comment_threads").await,
        set(&["nodes", "users"])
    );
    assert_eq!(
        foreign_tables(&db, "comments").await,
        set(&["comment_threads", "users"])
    );
    assert_eq!(
        foreign_tables(&db, "comment_reactions").await,
        set(&["comments", "users"])
    );
    assert_eq!(foreign_tables(&db, "doc_versions").await, set(&["nodes"]));
    assert_eq!(foreign_tables(&db, "block_moves").await, set(&["nodes"]));
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM pragma_foreign_key_check").await,
        0
    );
    store.close().await;

    // Une seconde ouverture ne migre plus rien.
    let store = Store::open(&db).await.unwrap();
    assert_eq!(store.backup(), None);
    assert_eq!(int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await, 4);
    store.close().await;
}

/// Ce qu'une base de la partie A du lot 3 porte en plus d'une base du lot 2, écrit en SQL comme
/// le code de la partie A l'écrit : la version d'un espace, un fil de commentaires résolu sur un
/// bloc, avec un message modifié et une réaction, et une version de page aux octets qui ne sont
/// pas de l'UTF-8. Rien dans `block_moves`, que la Task 13b retire de `0003`.
async fn lot3a_data(pool: &SqlitePool, space: Uuid) {
    let space = space.to_string();
    let olga: String = sqlx::query_scalar("SELECT id FROM users WHERE email = 'olga@exemple.test'")
        .fetch_one(pool)
        .await
        .unwrap();
    let page: String = sqlx::query_scalar("SELECT id FROM nodes WHERE title = 'Accueil'")
        .fetch_one(pool)
        .await
        .unwrap();
    let (thread, comment) = (Uuid::now_v7().to_string(), Uuid::now_v7().to_string());
    exec(pool, "UPDATE spaces SET lsn = 12 WHERE id = ?1", &[&space]).await;
    exec(
        pool,
        "INSERT INTO comment_threads (id, node_id, block_id, created_by, created_at, \
                                      resolved_at, resolved_by) \
         VALUES (?1, ?2, 'bloc-1', ?3, 4000, 4100, ?3)",
        &[&thread, &page, &olga],
    )
    .await;
    exec(
        pool,
        "INSERT INTO comments (id, thread_id, author_id, body, created_at, edited_at) \
         VALUES (?1, ?2, ?3, 'Relu, c''est bon', 4001, 4002)",
        &[&comment, &thread, &olga],
    )
    .await;
    exec(
        pool,
        "INSERT INTO comment_reactions (comment_id, user_id, emoji, created_at) \
         VALUES (?1, ?2, '👍', 4003)",
        &[&comment, &olga],
    )
    .await;
    exec(
        pool,
        "INSERT INTO doc_versions \
         (id, node_id, seq, covers_seq, reason, authors, created_at, state_blob) \
         VALUES (?1, ?2, 3, 3, 'before_restore', json_array(?3), 4200, x'00ff01fe800a0d')",
        &[&Uuid::now_v7().to_string(), &page, &olga],
    )
    .await;
}

/// La migration `0004` sur une base de la partie A du lot 3 (Ruling R243) : tout ce qui y était
/// reste à l'identique, valeur pour valeur, fils de commentaires et versions compris ; les trois
/// tables des fichiers et des favoris existent, vides, et leurs clés étrangères visent ce qu'il
/// faut ; la copie d'avant migration porte le nom de `0004` ; une seconde ouverture ne migre plus
/// rien.
#[tokio::test]
async fn lot3a_database_gains_files_and_favorites() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let pool = database_with(&db, LOT3A).await;
    let (team, _own) = lot2_data(&pool).await;
    lot3a_data(&pool, team).await;
    pool.close().await;

    // Ce que porte la base de la partie A, table par table, colonne par colonne.
    let lot3a_tables = table_names(&db).await;
    let mut before = Vec::new();
    for table in &lot3a_tables {
        let columns = columns(&db, table).await;
        let rows = rows(&db, table, &columns).await;
        before.push((table.clone(), columns, rows));
    }
    for (table, count) in [
        ("comment_threads", 1),
        ("comments", 1),
        ("comment_reactions", 1),
        ("doc_versions", 1),
    ] {
        assert!(
            before
                .iter()
                .any(|(t, _, rows)| t == table && rows.len() == count),
            "{table}"
        );
    }

    let store = Store::open(&db).await.unwrap();
    assert_eq!(
        texts(
            &db,
            "SELECT CAST(version AS TEXT) FROM _sqlx_migrations WHERE success ORDER BY version"
        )
        .await,
        [
            Some("1".to_string()),
            Some("2".to_string()),
            Some("3".to_string()),
            Some("4".to_string())
        ]
    );
    // La copie d'avant migration : la base de la partie A, sous le nom de `0004`.
    let backup = store.backup().expect("une copie avant la migration");
    assert_eq!(backup.path, backup_path(&db, 4));
    assert_eq!(backup.path, dir.path().join("mementum.db.avant-0004.bak"));
    assert!(backup.path.is_file());

    // Rien n'a changé dans ce qui était là, `_sqlx_migrations` mise à part (elle gagne la ligne
    // de `0004`) : ni une colonne, ni une valeur.
    for (table, columns_before, rows_before) in &before {
        if table == "_sqlx_migrations" {
            continue;
        }
        assert_eq!(&columns(&db, table).await, columns_before, "{table}");
        assert_eq!(
            &rows(&db, table, columns_before).await,
            rows_before,
            "{table}"
        );
    }
    assert_eq!(store.space_lsn(team).await.unwrap(), Some(12));
    assert_eq!(store.space_rev(team).await.unwrap(), Some(7));

    // Les trois tables neuves, vides, et rien d'autre de neuf.
    assert_eq!(table_names(&db).await, ALL_TABLES);
    for table in FILES_AND_FAVORITES_TABLES {
        assert!(!lot3a_tables.iter().any(|t| t == table), "{table}");
        assert_eq!(
            int(&db, &format!("SELECT COUNT(*) FROM {table}")).await,
            0,
            "{table}"
        );
    }
    assert_eq!(foreign_tables(&db, "files").await, set(&[]));
    assert_eq!(
        foreign_tables(&db, "attachments").await,
        set(&["files", "nodes", "users"])
    );
    assert_eq!(
        foreign_tables(&db, "favorites").await,
        set(&["nodes", "users"])
    );
    // Chaque clé étrangère est vérifiée : un contenu connu, puis une attache ou un favori qui
    // nomme une page, un contenu ou un compte inconnu.
    common::exec_sql(
        &db,
        "INSERT INTO files (hash, size, mime, created_at) \
         VALUES (zeroblob(32), 0, 'application/octet-stream', 5000)",
    )
    .await;
    for faulty in [
        "INSERT INTO attachments (id, node_id, file_hash, name, created_by, created_at) \
         SELECT 'a', 'nœud-inconnu', zeroblob(32), 'x', id, 1 FROM users LIMIT 1",
        "INSERT INTO attachments (id, node_id, file_hash, name, created_by, created_at) \
         SELECT 'a', n.id, x'01', 'x', n.created_by, 1 FROM nodes n \
         WHERE n.created_by IS NOT NULL LIMIT 1",
        "INSERT INTO attachments (id, node_id, file_hash, name, created_by, created_at) \
         SELECT 'a', id, zeroblob(32), 'x', 'compte-inconnu', 1 FROM nodes LIMIT 1",
        "INSERT INTO favorites (user_id, node_id, created_at) \
         SELECT id, 'nœud-inconnu', 1 FROM users LIMIT 1",
        "INSERT INTO favorites (user_id, node_id, created_at) \
         SELECT 'compte-inconnu', id, 1 FROM nodes LIMIT 1",
    ] {
        assert!(refused_by_foreign_key(&db, faulty).await, "{faulty}");
    }
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM pragma_foreign_key_check").await,
        0
    );
    store.close().await;

    // Une seconde ouverture ne migre plus rien.
    let store = Store::open(&db).await.unwrap();
    assert_eq!(store.backup(), None);
    assert_eq!(int(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await, 4);
    store.close().await;
}

/// Supprimer une page supprime, avec elle et son sous-arbre, leurs fils de commentaires (et leurs
/// messages et réactions), leurs versions, leurs déplacements de blocs, leurs attaches et les
/// favoris qui les nomment, par les clés étrangères (`ON DELETE CASCADE`) ; ceux d'une autre page
/// restent. Les contenus (`files`) restent tous : c'est au ménage de les retirer (Ruling R245).
#[tokio::test]
async fn deleting_a_page_deletes_its_threads_versions_and_moves() {
    let f = common::fresh().await;
    let author = common::new_user(&f.db).await.to_string();
    let parent = f
        .store
        .create_node(common::page(&f.space, None, None, "Parent"))
        .await
        .unwrap();
    let child = f
        .store
        .create_node(common::page(&f.space, Some(parent.id), None, "Enfant"))
        .await
        .unwrap();
    let other = f
        .store
        .create_node(common::page(&f.space, None, Some(parent.id), "Autre"))
        .await
        .unwrap();
    let pool = side_pool(&f.db).await;
    for (n, node) in [parent.id, child.id, other.id].iter().enumerate() {
        let node = node.to_string();
        let (thread, comment) = (Uuid::now_v7().to_string(), Uuid::now_v7().to_string());
        exec(
            &pool,
            "INSERT INTO comment_threads (id, node_id, block_id, created_by, created_at, \
                                          resolved_at, resolved_by) \
             VALUES (?1, ?2, 'bloc', ?3, 1, 2, ?3)",
            &[&thread, &node, &author],
        )
        .await;
        exec(
            &pool,
            "INSERT INTO comments (id, thread_id, author_id, body, created_at, edited_at) \
             VALUES (?1, ?2, ?3, 'Bonjour', 1, NULL)",
            &[&comment, &thread, &author],
        )
        .await;
        exec(
            &pool,
            "INSERT INTO comment_reactions (comment_id, user_id, emoji, created_at) \
             VALUES (?1, ?2, '👍', 1)",
            &[&comment, &author],
        )
        .await;
        exec(
            &pool,
            "INSERT INTO doc_versions \
             (id, node_id, seq, covers_seq, state_blob, reason, authors, created_at) \
             VALUES (?1, ?2, 3, 3, x'00ff', 'periodic', '[]', 1)",
            &[&Uuid::now_v7().to_string(), &node],
        )
        .await;
        exec(
            &pool,
            "INSERT INTO block_moves (node_id, seq, record, created_at) VALUES (?1, ?2, x'01', 1)",
            &[&node, &(n + 1).to_string()],
        )
        .await;
        // Un contenu par page, attaché à elle, et la page en favori.
        let hash = format!("{:064x}", n + 1);
        exec(
            &pool,
            "INSERT INTO files (hash, size, mime, created_at) \
             VALUES (unhex(?1), 1, 'application/octet-stream', 1)",
            &[&hash],
        )
        .await;
        exec(
            &pool,
            "INSERT INTO attachments (id, node_id, file_hash, name, created_by, created_at) \
             VALUES (?1, ?2, unhex(?3), 'image.png', ?4, 1)",
            &[&Uuid::now_v7().to_string(), &node, &hash, &author],
        )
        .await;
        exec(
            &pool,
            "INSERT INTO favorites (user_id, node_id, created_at) VALUES (?1, ?2, 1)",
            &[&author, &node],
        )
        .await;
    }
    pool.close().await;
    for table in COLLABORATION_TABLES
        .into_iter()
        .chain(FILES_AND_FAVORITES_TABLES)
    {
        assert_eq!(
            int(&f.db, &format!("SELECT COUNT(*) FROM {table}")).await,
            3,
            "{table}"
        );
    }

    f.store
        .trash_node(parent.id, author.parse().unwrap())
        .await
        .unwrap();
    let mut deleted = f.store.delete_node(parent.id).await.unwrap();
    deleted.sort();
    let mut expected = vec![parent.id, child.id];
    expected.sort();
    assert_eq!(deleted, expected);

    // Il ne reste que ce qui était à l'autre page.
    let other = other.id.to_string();
    for (table, of_other) in [
        (
            "comment_threads",
            format!("SELECT COUNT(*) FROM comment_threads WHERE node_id = '{other}'"),
        ),
        (
            "comments",
            format!(
                "SELECT COUNT(*) FROM comments c JOIN comment_threads t ON t.id = c.thread_id \
                 WHERE t.node_id = '{other}'"
            ),
        ),
        (
            "comment_reactions",
            format!(
                "SELECT COUNT(*) FROM comment_reactions r JOIN comments c ON c.id = r.comment_id \
                 JOIN comment_threads t ON t.id = c.thread_id WHERE t.node_id = '{other}'"
            ),
        ),
        (
            "doc_versions",
            format!("SELECT COUNT(*) FROM doc_versions WHERE node_id = '{other}'"),
        ),
        (
            "block_moves",
            format!("SELECT COUNT(*) FROM block_moves WHERE node_id = '{other}'"),
        ),
        (
            "attachments",
            format!("SELECT COUNT(*) FROM attachments WHERE node_id = '{other}'"),
        ),
        (
            "favorites",
            format!("SELECT COUNT(*) FROM favorites WHERE node_id = '{other}'"),
        ),
    ] {
        assert_eq!(
            int(&f.db, &format!("SELECT COUNT(*) FROM {table}")).await,
            1,
            "{table}"
        );
        assert_eq!(int(&f.db, &of_other).await, 1, "{table}");
    }
    // Les contenus restent, ceux des pages supprimées compris : c'est au ménage.
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM files").await, 3);
    assert_eq!(
        int(&f.db, "SELECT COUNT(*) FROM pragma_foreign_key_check").await,
        0
    );
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
