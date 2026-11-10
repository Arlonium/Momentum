//! Outils des tests d'intégration : base temporaire, page créée, hub et serveur de test.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{Block, block_text, parse};
use mementum_schema::SCHEMA_VERSION;
use mementum_store::{NewNode, NodeKind, Store};
use mementum_sync::{ConnInfo, Hub, SyncConfig};
use sqlx::Row;
use tempfile::TempDir;
use uuid::Uuid;
use yrs::updates::decoder::Decode;
use yrs::{Doc, Transact, Update, XmlElementRef, XmlFragment, XmlOut};

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

    /// URL de la page pour un client du schéma courant, qui a le droit
    /// d'écrire, sans compte.
    pub fn url(&self) -> String {
        self.server.url(self.page, SCHEMA_VERSION)
    }

    /// URL de la page pour une connexion `conn`.
    pub fn url_for(&self, conn: ConnInfo) -> String {
        self.server.url_for(self.page, conn)
    }

    /// Un client connecté et synchronisé.
    pub async fn client(&self) -> Client {
        Client::connect_synced(&self.url()).await
    }

    /// Écrit `mmd` dans la page par le hub (écriture serveur, sans compte).
    pub async fn write_mmd(&self, mmd: &str) {
        self.hub
            .write_blocks(self.page, parse(mmd).unwrap(), None)
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

/// Une connexion du schéma courant, qui a le droit d'écrire, sans compte :
/// celle de [`Fixture::url`].
pub fn writing_conn() -> ConnInfo {
    ConnInfo {
        client_schema: SCHEMA_VERSION,
        can_write: true,
        actor: None,
    }
}

/// Crée un nœud de page (sans document ni accès posé) dans l'espace par défaut.
pub async fn new_page(store: &Store) -> Uuid {
    let space = store.default_space().await.unwrap();
    store
        .create_node(NewNode {
            space_id: space.id,
            parent_id: None,
            after: None,
            kind: NodeKind::Page,
            title: "Page".into(),
            created_by: None,
            grants: Vec::new(),
            doc: None,
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

/// Attend que le serveur ait traité tout ce que `client` a envoyé (il traite
/// les messages d'une connexion dans l'ordre, et répond ensuite par une étape 2).
pub async fn processed(client: &Client) {
    let before = client.step2_count();
    client.resync();
    assert!(
        client.wait_until(WAIT, |c| c.step2_count() > before).await,
        "le serveur n'a pas traité les mises à jour du client"
    );
}

/// Les pages qui ont un acteur vivant, triées.
pub async fn live(hub: &Hub) -> Vec<Uuid> {
    let mut pages = hub.live_pages().await;
    pages.sort();
    pages
}

/// Attend que les pages qui ont un acteur vivant soient exactement `expected`
/// (triées), [`WAIT`] au plus. Renvoie le dernier constat.
pub async fn live_becomes(hub: &Hub, expected: &[Uuid]) -> bool {
    let deadline = tokio::time::Instant::now() + WAIT;
    while live(hub).await != expected {
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    true
}

/// Une connexion `conn` est en lecture seule : elle reçoit tout, ses mises à
/// jour ne sont ni écrites ni diffusées, et elle reste ouverte.
pub async fn assert_read_only(conn: ConnInfo) {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Bonjour\n").await;
    let id = f.ids().await[0].clone();
    let other = Client::connect_synced(&f.url_for(conn)).await;
    let observer = f.client().await;
    let writer = f.client().await;
    assert_eq!(text(&other.blocks()), "Bonjour", "il reçoit tout");
    let rows = row_counts(&f.db).await;

    other.type_text(&id, " ZZZ");
    processed(&other).await;
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "Bonjour");
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "Bonjour"
    );

    // Les autres écrivent : il le reçoit. L'observateur, qui reçoit les
    // diffusions dans l'ordre, ne voit jamais sa mise à jour.
    writer.type_text(&id, "!");
    assert!(
        observer
            .wait_until(WAIT, |c| text(&c.blocks()) == "Bonjour!")
            .await
    );
    assert!(
        other
            .wait_until(WAIT, |c| text(&c.blocks()).contains('!'))
            .await,
        "il reçoit les mises à jour des autres"
    );
    assert!(
        text(&other.blocks()).contains("ZZZ"),
        "sa copie garde son texte"
    );
    assert!(!text(&observer.blocks()).contains("ZZZ"));
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "Bonjour!");
    assert!(!other.is_closed());
}

/// Une connexion annexe sur la base.
async fn side_pool(db: &Path) -> sqlx::SqlitePool {
    sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap()
}

/// Crée un compte par du SQL direct : l'API des comptes n'est pas celle de
/// ces tests.
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

/// Le compte (`actor_id`) de chaque mise à jour au journal de la page, dans
/// l'ordre des `seq`.
pub async fn actors(db: &Path, page: Uuid) -> Vec<Option<Uuid>> {
    let pool = side_pool(db).await;
    let actors = sqlx::query("SELECT actor_id FROM doc_updates WHERE node_id = ? ORDER BY seq")
        .bind(page.to_string())
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|row| {
            row.get::<Option<String>, _>(0)
                .map(|id| Uuid::parse_str(&id).unwrap())
        })
        .collect();
    pool.close().await;
    actors
}

/// Nombre de lignes de chaque table de la base.
pub async fn row_counts(db: &Path) -> Vec<(String, i64)> {
    let pool = side_pool(db).await;
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

/// Exécute `sql` sur la base par une connexion annexe (trigger de test…).
pub async fn exec_sql(db: &Path, sql: &str) {
    let pool = side_pool(db).await;
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

/// Un document yrs à part, à l'état du client : pour préparer des mises à
/// jour sans les envoyer.
pub fn detached(client: &Client) -> Doc {
    let doc = new_doc();
    let state = client.page().unwrap().encode_state();
    doc.transact_mut()
        .apply_update(Update::decode_v1(&state).unwrap())
        .unwrap();
    doc
}

/// Le nœud de contenu (le paragraphe) du premier bloc.
pub fn first_content(doc: &Doc) -> XmlElementRef {
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    let child = |e: &XmlElementRef| match e.get(&txn, 0) {
        Some(XmlOut::Element(e)) => e,
        _ => panic!("élément attendu"),
    };
    let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
        panic!("pas de racine blockGroup");
    };
    child(&child(&group))
}
