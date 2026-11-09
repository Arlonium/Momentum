//! Outils des tests d'intégration : base temporaire, page créée, hub et serveur de test.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mementum_crdt::PageDoc;
use mementum_md::{Block, block_text, parse};
use mementum_store::{NewNode, NodeKind, Store};
use mementum_sync::{Hub, SyncConfig};
use tempfile::TempDir;
use uuid::Uuid;

pub use mementum_sync::testkit::*;

/// Délai maximal d'une attente de convergence.
pub const WAIT: Duration = Duration::from_secs(10);

/// Une base dans un dossier temporaire, une page créée (avec son document),
/// un hub et un serveur de test.
pub struct Fixture {
    pub dir: TempDir,
    pub db: PathBuf,
    pub store: Store,
    pub hub: Arc<Hub>,
    pub server: TestServer,
    pub page: Uuid,
}

impl Fixture {
    pub async fn new(cfg: SyncConfig) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("mementum.db");
        let store = Store::open(&db).await.unwrap();
        let page = new_page(&store).await;
        let hub = Hub::new(store.clone(), cfg);
        hub.create_page_doc(page).await.unwrap();
        let server = TestServer::start(hub.clone()).await;
        Fixture {
            dir,
            db,
            store,
            hub,
            server,
            page,
        }
    }

    /// URL de la page pour un client du schéma courant.
    pub fn url(&self) -> String {
        self.server.url(self.page, mementum_schema::SCHEMA_VERSION)
    }

    /// Un client connecté et synchronisé.
    pub async fn client(&self) -> Client {
        Client::connect_synced(&self.url()).await
    }

    /// Écrit `mmd` dans la page par le hub (écriture serveur).
    pub async fn write_mmd(&self, mmd: &str) {
        self.hub
            .write_blocks(self.page, parse(mmd).unwrap())
            .await
            .unwrap();
    }

    /// Ids des blocs de premier niveau, lus par le hub.
    pub async fn ids(&self) -> Vec<String> {
        self.hub
            .read_blocks(self.page)
            .await
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect()
    }
}

/// Crée un nœud de page (sans document) dans le coffre par défaut.
pub async fn new_page(store: &Store) -> Uuid {
    let space = store.default_space().await.unwrap();
    store
        .create_node(NewNode {
            vault_id: space.vault_id,
            parent_id: None,
            after: None,
            kind: NodeKind::Page,
            title: "Page".into(),
        })
        .await
        .unwrap()
        .id
}

/// Le document tel que le store le rebâtit : l'instantané, puis les mises à
/// jour dans l'ordre des `seq`.
pub async fn stored_page(store: &Store, page: Uuid) -> PageDoc {
    let state = store.load_doc(page).await.unwrap();
    let updates = state
        .snapshot
        .iter()
        .map(|(_, s)| s.as_slice())
        .chain(state.updates.iter().map(|(_, u)| u.as_slice()));
    PageDoc::from_updates(updates).unwrap()
}

/// Texte de chaque bloc de premier niveau, une ligne par bloc.
pub fn text(blocks: &[Block]) -> String {
    blocks.iter().map(block_text).collect::<Vec<_>>().join("\n")
}

/// Nombre de lignes de chaque table de la base.
pub async fn row_counts(db: &std::path::Path) -> Vec<(String, i64)> {
    use sqlx::Row;
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    let tables: Vec<String> = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .map(|r| r.get(0))
    .collect();
    let mut counts = Vec::new();
    for table in tables {
        let n: i64 = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM \"{table}\""
        )))
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
        counts.push((table, n));
    }
    pool.close().await;
    counts
}
