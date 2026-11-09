//! API HTTP du serveur. Les requêtes passent par `app()` en mémoire
//! (`tower::ServiceExt::oneshot`) ; la WebSocket, par un écouteur sur un port
//! libre de 127.0.0.1 (jamais 8080, Ruling R24).
//!
//! Les journaux vérifiés sont ceux du fil du test : les tests qui en lisent
//! tournent en `current_thread` (défaut de `#[tokio::test]`), routes et
//! acteurs compris.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{Method, Request, StatusCode, header};
use mementum_schema::SCHEMA_VERSION;
use mementum_server::{Config, Info, PageNode, Tree};
use mementum_store::Store;
use mementum_sync::testkit::Client;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use uuid::Uuid;

/// Délai maximal d'une attente de convergence.
const WAIT: Duration = Duration::from_secs(10);

struct TestApp {
    _dir: TempDir,
    cfg: Config,
    router: Router,
}

struct Reply {
    status: StatusCode,
    content_type: Option<String>,
    body: Bytes,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!(
                "réponse {} illisible en JSON ({e}) : {:?}",
                self.status,
                String::from_utf8_lossy(&self.body)
            )
        })
    }

    fn text(&self) -> String {
        String::from_utf8(self.body.to_vec()).expect("une réponse en UTF-8")
    }

    /// Vérifie le statut et la forme d'une erreur : `{ "error": code,
    /// "message": texte non vide }`, en JSON.
    fn assert_error(&self, status: StatusCode, code: &str) -> Value {
        assert_eq!(self.status, status, "réponse : {}", self.text());
        assert_eq!(
            self.content_type.as_deref(),
            Some("application/json"),
            "une erreur est en JSON"
        );
        let body = self.json();
        assert_eq!(body["error"], code, "réponse : {body}");
        assert!(
            body["message"].as_str().is_some_and(|m| !m.is_empty()),
            "message d'erreur attendu : {body}"
        );
        body
    }
}

impl TestApp {
    async fn new() -> TestApp {
        TestApp::with_bind("127.0.0.1:0").await
    }

    /// `app()` ne lie pas l'adresse : `bind` ne règle que les contrôles.
    async fn with_bind(bind: &str) -> TestApp {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            bind: bind.parse().unwrap(),
            // Un dossier qui n'existe pas encore : `app` le crée.
            data_dir: dir.path().join("donnees"),
        };
        let router = mementum_server::app(&cfg).await.unwrap();
        TestApp {
            _dir: dir,
            cfg,
            router,
        }
    }

    /// Envoie la requête avec `Host: localhost` si elle n'en a pas (comme un
    /// navigateur, ou le mandataire de Vite).
    async fn send(&self, mut req: Request<Body>) -> Reply {
        if !req.headers().contains_key(header::HOST) {
            req.headers_mut()
                .insert(header::HOST, "localhost".parse().unwrap());
        }
        self.send_raw(req).await
    }

    /// Envoie la requête telle quelle.
    async fn send_raw(&self, req: Request<Body>) -> Reply {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let content_type = res
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string());
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        Reply {
            status,
            content_type,
            body,
        }
    }

    async fn get(&self, uri: &str) -> Reply {
        self.send(Request::get(uri).body(Body::empty()).unwrap())
            .await
    }

    async fn json(&self, method: Method, uri: &str, body: Value) -> Reply {
        self.send(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }

    async fn post(&self, body: Value) -> Reply {
        self.json(Method::POST, "/api/v1/pages", body).await
    }

    async fn create(&self, body: Value) -> PageNode {
        let reply = self.post(body).await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text());
        serde_json::from_slice(&reply.body).unwrap()
    }

    async fn patch(&self, id: &str, body: Value) -> Reply {
        self.json(Method::PATCH, &format!("/api/v1/pages/{id}"), body)
            .await
    }

    async fn patched(&self, id: &str, body: Value) -> PageNode {
        let reply = self.patch(id, body).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text());
        serde_json::from_slice(&reply.body).unwrap()
    }

    async fn get_mmd(&self, id: &str) -> Reply {
        self.get(&format!("/api/v1/pages/{id}/mmd")).await
    }

    async fn put_mmd(&self, id: &str, mmd: impl Into<Body>) -> Reply {
        self.send(
            Request::put(format!("/api/v1/pages/{id}/mmd"))
                .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
                .body(mmd.into())
                .unwrap(),
        )
        .await
    }

    async fn tree(&self) -> Tree {
        let reply = self.get("/api/v1/tree").await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text());
        serde_json::from_slice(&reply.body).unwrap()
    }

    async fn node(&self, id: &str) -> PageNode {
        self.tree()
            .await
            .nodes
            .into_iter()
            .find(|n| n.id == id)
            .unwrap_or_else(|| panic!("page {id} absente de l'arbre"))
    }

    /// Titres des enfants de `parent` (`None` = racine), dans l'ordre des
    /// positions.
    async fn children(&self, parent: Option<&str>) -> Vec<String> {
        let mut nodes: Vec<PageNode> = self
            .tree()
            .await
            .nodes
            .into_iter()
            .filter(|n| n.parent_id.as_deref() == parent)
            .collect();
        nodes.sort_by(|a, b| a.position.cmp(&b.position));
        nodes.into_iter().map(|n| n.title).collect()
    }

    /// Sert l'application sur un port libre de 127.0.0.1.
    async fn listen(&self) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = self.router.clone();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        addr
    }

    fn db_path(&self) -> PathBuf {
        self.cfg.db_path()
    }

    async fn node_titled(&self, title: &str) -> PageNode {
        self.tree()
            .await
            .nodes
            .into_iter()
            .find(|n| n.title == title)
            .unwrap_or_else(|| panic!("aucune page « {title} »"))
    }
}

/// Journaux `tracing` émis sur le fil courant tant que la valeur vit.
struct Logs {
    buffer: Arc<Mutex<Vec<u8>>>,
    _guard: tracing::subscriber::DefaultGuard,
}

struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Logs {
    fn capture() -> Logs {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || LogWriter(writer.clone()))
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        Logs {
            buffer,
            _guard: tracing::subscriber::set_default(subscriber),
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.buffer.lock().unwrap_or_else(PoisonError::into_inner))
            .into_owned()
    }
}

/// Exécute du SQL sur la base, par une connexion à part (pannes simulées).
async fn exec_sql(db: &Path, sql: &str) {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

/// Poignée de main WebSocket avec une en-tête `Origin` facultative :
/// `Ok` si elle aboutit, sinon le statut HTTP de la réponse.
async fn handshake(url: &str, origin: Option<&str>) -> Result<(), u16> {
    use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
    let mut request = url.into_client_request().unwrap();
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert(header::ORIGIN, origin.parse().unwrap());
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok(_) => Ok(()),
        Err(tungstenite::Error::Http(response)) => Err(response.status().as_u16()),
        Err(e) => panic!("poignée de main : {e}"),
    }
}

fn titles(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/mmd/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} : {e}", path.display()))
}

#[tokio::test]
async fn health_and_info() {
    let t = TestApp::new().await;
    assert!(t.cfg.data_dir.is_dir(), "le dossier des données est créé");
    assert!(t.db_path().starts_with(&t.cfg.data_dir));

    let health = t.get("/health").await;
    assert_eq!(health.status, StatusCode::OK);
    assert_eq!(health.text(), "ok");

    let reply = t.get("/api/v1/info").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.json(),
        json!({ "version": env!("CARGO_PKG_VERSION"), "schemaVersion": SCHEMA_VERSION })
    );
    let info: Info = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(info.schema_version, SCHEMA_VERSION);
}

#[tokio::test]
async fn pages_crud() {
    let t = TestApp::new().await;
    let tree = t.tree().await;
    assert!(tree.nodes.is_empty());
    Uuid::parse_str(&tree.vault_id).expect("vaultId est un UUID");
    let raw = t.get("/api/v1/tree").await.json();
    assert!(raw.get("vaultId").is_some() && raw.get("nodes").is_some());

    // Créer : sans `afterId`, en fin des frères (Ruling R23).
    let a = t.create(json!({ "title": "A" })).await;
    assert_eq!(a.kind, "page");
    assert_eq!(a.parent_id, None);
    assert_eq!(a.title, "A");
    assert!(a.updated_at > 0);
    Uuid::parse_str(&a.id).expect("id en UUID");
    let b = t.create(json!({ "title": "B" })).await;
    assert_eq!(t.children(None).await, titles(&["A", "B"]));
    // `"afterId": null` : en tête ; un id : après lui.
    let c = t.create(json!({ "title": "C", "afterId": null })).await;
    assert_eq!(t.children(None).await, titles(&["C", "A", "B"]));
    let d = t.create(json!({ "title": "D", "afterId": a.id })).await;
    assert_eq!(t.children(None).await, titles(&["C", "A", "D", "B"]));
    // La réponse JSON a la forme attendue (camelCase).
    let raw = t.post(json!({ "title": "brut" })).await.json();
    for key in ["id", "parentId", "kind", "title", "position", "updatedAt"] {
        assert!(raw.get(key).is_some(), "{key} absent de {raw}");
    }

    // Le document est créé avec le nœud : une page neuve se lit en MMD.
    let mmd = t.get_mmd(&a.id).await;
    assert_eq!(mmd.status, StatusCode::OK);

    // Renommer : la place ne change pas.
    let a2 = t.patched(&a.id, json!({ "title": "A2" })).await;
    assert_eq!(a2.title, "A2");
    assert_eq!(a2.position, a.position);
    assert_eq!(a2.parent_id, None);
    assert!(a2.updated_at >= a.updated_at);
    assert_eq!(t.node(&a.id).await.title, "A2");

    // `parentId` seul : en fin des enfants du nouveau parent.
    let e = t.create(json!({ "title": "E", "parentId": a.id })).await;
    assert_eq!(e.parent_id.as_deref(), Some(a.id.as_str()));
    let moved = t.patched(&b.id, json!({ "parentId": a.id })).await;
    assert_eq!(moved.parent_id.as_deref(), Some(a.id.as_str()));
    assert_eq!(t.children(Some(&a.id)).await, titles(&["E", "B"]));
    assert_eq!(t.children(None).await, titles(&["C", "A2", "D", "brut"]));

    // `afterId` seul : réordonnancement chez le parent actuel.
    t.patched(&b.id, json!({ "afterId": null })).await;
    assert_eq!(t.children(Some(&a.id)).await, titles(&["B", "E"]));
    t.patched(&b.id, json!({ "afterId": e.id })).await;
    assert_eq!(t.children(Some(&a.id)).await, titles(&["E", "B"]));

    // `"parentId": null` : à la racine, en fin.
    let root = t.patched(&e.id, json!({ "parentId": null })).await;
    assert_eq!(root.parent_id, None);
    assert_eq!(
        t.children(None).await,
        titles(&["C", "A2", "D", "brut", "E"])
    );
    assert_eq!(t.children(Some(&a.id)).await, titles(&["B"]));

    // Les deux : on suit les deux.
    t.patched(&e.id, json!({ "parentId": a.id, "afterId": null }))
        .await;
    assert_eq!(t.children(Some(&a.id)).await, titles(&["E", "B"]));
    t.patched(&c.id, json!({ "parentId": a.id, "afterId": e.id }))
        .await;
    assert_eq!(t.children(Some(&a.id)).await, titles(&["E", "C", "B"]));

    // Ni `parentId` ni `afterId` : pas de déplacement.
    let before = t.node(&e.id).await;
    let same = t.patched(&e.id, json!({})).await;
    assert_eq!(
        (&same.parent_id, &same.position),
        (&before.parent_id, &before.position)
    );
    let renamed = t.patched(&e.id, json!({ "title": "E2" })).await;
    assert_eq!(
        (&renamed.parent_id, &renamed.position),
        (&before.parent_id, &before.position)
    );

    // Un `afterId` qui n'est pas un frère : 422, rien ne bouge.
    let snapshot = t.tree().await.nodes;
    t.patch(&e.id, json!({ "afterId": d.id }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_after");
    t.patch(&e.id, json!({ "parentId": null, "afterId": b.id }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_after");
    t.patch(&e.id, json!({ "afterId": "pas-un-uuid" }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_after");
    t.post(json!({ "title": "X", "afterId": e.id }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_after");
    // Un parent inconnu : 422 aussi.
    let unknown = Uuid::now_v7().to_string();
    t.post(json!({ "title": "X", "parentId": unknown }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_parent");
    t.patch(&e.id, json!({ "parentId": unknown }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_parent");
    assert_eq!(t.tree().await.nodes, snapshot, "aucun refus n'écrit");

    // Une page inconnue : 404.
    t.patch(&unknown, json!({ "title": "X" }))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    t.patch("pas-un-uuid", json!({ "title": "X" }))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(t.tree().await.nodes, snapshot);
}

#[tokio::test]
async fn move_cycle_is_409() {
    let t = TestApp::new().await;
    let a = t.create(json!({ "title": "A" })).await;
    let b = t.create(json!({ "title": "B", "parentId": a.id })).await;
    let c = t.create(json!({ "title": "C", "parentId": b.id })).await;
    let before = t.tree().await.nodes;

    // Sous sa petite-fille, sous sa fille, sous elle-même : refusé.
    for parent in [&c.id, &b.id, &a.id] {
        let body = t
            .patch(&a.id, json!({ "parentId": parent }))
            .await
            .assert_error(StatusCode::CONFLICT, "cycle");
        assert!(body.get("line").is_none());
    }
    // Le refus n'écrit rien, pas même le titre envoyé avec.
    t.patch(&a.id, json!({ "title": "Renommée", "parentId": c.id }))
        .await
        .assert_error(StatusCode::CONFLICT, "cycle");
    assert_eq!(t.tree().await.nodes, before);

    // Le déplacement inverse est permis.
    let moved = t.patched(&c.id, json!({ "parentId": null })).await;
    assert_eq!(moved.parent_id, None);
}

#[tokio::test]
async fn mmd_get_put() {
    let t = TestApp::new().await;
    let page = t.create(json!({ "title": "Spécification" })).await;
    let mmd = fixture("p0-all-blocks.mmd");

    let put = t.put_mmd(&page.id, mmd.clone()).await;
    assert_eq!(put.status, StatusCode::NO_CONTENT, "{}", put.text());
    assert!(put.body.is_empty());

    let got = t.get_mmd(&page.id).await;
    assert_eq!(got.status, StatusCode::OK);
    assert_eq!(
        got.content_type.as_deref(),
        Some("text/markdown; charset=utf-8")
    );
    assert_eq!(got.text(), mmd, "le MMD relu est exactement celui écrit");

    // Une seconde écriture remplace la première.
    let put = t.put_mmd(&page.id, "Bonjour Café 🚀\n").await;
    assert_eq!(put.status, StatusCode::NO_CONTENT);
    assert_eq!(t.get_mmd(&page.id).await.text(), "Bonjour Café 🚀\n");
}

#[tokio::test]
async fn invalid_mmd_is_422() {
    let t = TestApp::new().await;
    let page = t.create(json!({})).await;
    assert_eq!(
        t.put_mmd(&page.id, "Avant\n").await.status,
        StatusCode::NO_CONTENT
    );

    let body = t
        .put_mmd(&page.id, "* x\n")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_mmd");
    assert_eq!(body["line"], 1, "{body}");

    let body = t
        .put_mmd(&page.id, "Un\n\n## Deux\n\n##### Trop profond\n")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_mmd");
    assert_eq!(body["line"], 5, "{body}");

    // Un corps qui n'est pas de l'UTF-8 : la ligne de l'octet fautif.
    let body = t
        .put_mmd(&page.id, Body::from(b"Un\n\nDeux \xff\n".to_vec()))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_mmd");
    assert_eq!(body["line"], 3, "{body}");

    // Rien n'a été écrit.
    assert_eq!(t.get_mmd(&page.id).await.text(), "Avant\n");
}

#[tokio::test]
async fn invalid_blocks_is_422() {
    use base64::Engine;
    let t = TestApp::new().await;
    let page = t.create(json!({})).await;
    let before = t.get_mmd(&page.id).await.text();
    // Lisible en MMD (moins de `MAX_DEPTH` niveaux), mais trop profond pour le
    // document : un nœud étranger de 60 niveaux sous 78 blocs imbriqués
    // dépasse `MAX_YJS_DEPTH` (`CrdtError::TooDeep`).
    let mut node = json!({ "tag": "inner" });
    for _ in 1..60 {
        node = json!({ "tag": "inner", "content": [node] });
    }
    let data =
        base64::engine::general_purpose::STANDARD.encode(json!({ "content": [node] }).to_string());
    let mut mmd = String::new();
    for level in 0..78 {
        mmd.push_str(&format!(
            "{}Niveau {level}
",
            "  ".repeat(level)
        ));
    }
    mmd.push_str(&format!(
        "{}<block type=\"futureWidget\" id=\"f\" data=\"{data}\" foreign=true/>
",
        "  ".repeat(78)
    ));
    mementum_md::parse(&mmd).expect("MMD lisible");
    let body = t
        .put_mmd(&page.id, mmd)
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_blocks");
    assert!(body.get("line").is_none());
    assert!(
        body["message"].as_str().unwrap().contains("profond"),
        "{body}"
    );
    // Rien n'a été écrit.
    assert_eq!(t.get_mmd(&page.id).await.text(), before);
}

#[tokio::test]
async fn unknown_page_is_404_and_not_created() {
    let t = TestApp::new().await;
    let addr = t.listen().await;
    let id = Uuid::now_v7();

    t.get_mmd(&id.to_string())
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    t.put_mmd(&id.to_string(), "Texte\n")
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    t.get_mmd("pas-un-uuid")
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");

    // `/sync` : 404 avant toute poignée de main WebSocket.
    t.get(&format!("/sync/{id}?schema={SCHEMA_VERSION}"))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    let url = format!("ws://{addr}/sync/{id}?schema={SCHEMA_VERSION}");
    let refused = Client::connect(&url)
        .await
        .err()
        .expect("connexion refusée");
    assert!(refused.contains("404"), "{refused}");
    let refused = Client::connect(&format!("ws://{addr}/sync/pas-un-uuid?schema=1"))
        .await
        .err()
        .expect("connexion refusée");
    assert!(refused.contains("404"), "{refused}");

    // Rien n'a été créé : ni nœud, ni document.
    assert!(t.tree().await.nodes.is_empty());
    let store = Store::open(&t.db_path()).await.unwrap();
    assert!(store.get_node(id).await.unwrap().is_none());
    let doc = store.load_doc(id).await.unwrap();
    assert!(doc.snapshot.is_none() && doc.updates.is_empty());
    store.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_route_serves_existing_page() {
    let t = TestApp::new().await;
    let addr = t.listen().await;
    let page = t.create(json!({ "title": "En direct" })).await;
    let url = format!("ws://{addr}/sync/{}?schema={SCHEMA_VERSION}", page.id);
    let client = Client::connect_synced(&url).await;
    assert_eq!(client.blocks().len(), 1, "la racine écrite par le serveur");
    // Sans poignée de main WebSocket : une erreur JSON, pas un 404.
    t.get(&format!("/sync/{}?schema={SCHEMA_VERSION}", page.id))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_request");

    // Une écriture MMD est diffusée aux connexions ouvertes.
    assert_eq!(
        t.put_mmd(&page.id, "Bonjour\n").await.status,
        StatusCode::NO_CONTENT
    );
    assert!(
        client
            .wait_until(WAIT, |c| {
                c.blocks().first().map(mementum_md::block_text).as_deref() == Some("Bonjour")
            })
            .await,
        "le client n'a pas reçu l'écriture"
    );

    // Une frappe du client se relit en MMD.
    // L'étape 2 qui répond à `resync` vient après le traitement de la frappe.
    let block = client.blocks()[0].id.clone();
    let start = client.step2_count();
    client.type_text(&block, " à tous");
    client.resync();
    assert!(client.wait_until(WAIT, |c| c.step2_count() > start).await);
    assert_eq!(t.get_mmd(&page.id).await.text(), "Bonjour à tous\n");
}

#[tokio::test]
async fn title_rules() {
    let t = TestApp::new().await;
    // Titre absent, nul ou blanc : « Sans titre ».
    assert_eq!(t.create(json!({})).await.title, "Sans titre");
    assert_eq!(t.create(json!({ "title": null })).await.title, "Sans titre");
    assert_eq!(
        t.create(json!({ "title": " \t\n " })).await.title,
        "Sans titre"
    );
    // Espaces de bord retirées.
    assert_eq!(
        t.create(json!({ "title": "  Bonjour  " })).await.title,
        "Bonjour"
    );
    // 500 caractères (et non 500 octets ni 500 unités UTF-16) : accepté.
    let max = "é".repeat(500);
    assert_eq!(t.create(json!({ "title": max })).await.title, max);
    assert_eq!(
        t.create(json!({ "title": format!("  {max}  ") }))
            .await
            .title,
        max
    );
    let rockets = "🚀".repeat(500);
    assert_eq!(t.create(json!({ "title": rockets })).await.title, rockets);

    // 501 caractères : refusé, rien n'est créé.
    let count = t.tree().await.nodes.len();
    t.post(json!({ "title": "é".repeat(501) }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "title_too_long");
    assert_eq!(t.tree().await.nodes.len(), count);

    // Les mêmes règles au renommage.
    let page = t.create(json!({ "title": "Page" })).await;
    let other = t.create(json!({ "title": "Autre" })).await;
    assert_eq!(
        t.patched(&page.id, json!({ "title": "   " })).await.title,
        "Sans titre"
    );
    assert_eq!(
        t.patched(&page.id, json!({ "title": " Nouveau " }))
            .await
            .title,
        "Nouveau"
    );
    let before = t.node(&page.id).await;
    t.patch(&page.id, json!({ "title": "a".repeat(501) }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "title_too_long");
    // Refusé avant tout déplacement demandé avec.
    t.patch(
        &page.id,
        json!({ "title": "a".repeat(501), "parentId": other.id }),
    )
    .await
    .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "title_too_long");
    assert_eq!(t.node(&page.id).await, before);
    // `"title": null` au renommage : le titre ne change pas.
    assert_eq!(
        t.patched(&page.id, json!({ "title": null })).await.title,
        "Nouveau"
    );
}

/// Un message d'erreur en français, sans la cause de serde ou de std recopiée.
fn assert_french(body: &Value, expected: &[&str]) {
    let message = body["message"].as_str().unwrap();
    for part in expected {
        assert!(message.contains(part), "« {part} » absent de « {message} »");
    }
    // Tournures de serde, d'axum et de std.
    for english in [
        "expected",
        "invalid type",
        "invalid value",
        "unknown field",
        "missing field",
        "Failed",
        "at line",
        "EOF",
        "syntax",
    ] {
        assert!(
            !message.contains(english),
            "message pas en français : « {message} »"
        );
    }
}

#[tokio::test]
async fn errors_are_json() {
    let t = TestApp::new().await;
    let post_raw = |body: &'static str| {
        Request::post("/api/v1/pages")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    // JSON illisible : la ligne et la colonne.
    let body = t
        .send(post_raw("{\n  \"title\": "))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_json");
    assert_french(&body, &["JSON illisible", "ligne 2"]);
    let body = t
        .send(post_raw("{} {}"))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_json");
    assert_french(&body, &["JSON illisible"]);
    // Champ inconnu, nommé.
    let body = t
        .post(json!({ "parentID": null }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    assert_french(&body, &["champ inconnu", "parentID"]);
    // Type invalide, avec le champ.
    let body = t
        .post(json!({ "title": 3 }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    assert_french(&body, &["type invalide", "title"]);
    let body = t
        .patch(&Uuid::now_v7().to_string(), json!({}))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    assert_french(&body, &["page introuvable"]);
    // Un corps qui n'est pas un objet. (Un tableau, lui, passe : serde lit
    // une structure depuis une séquence, champs par défaut.)
    let body = t
        .send(post_raw("3"))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    assert_french(&body, &["objet JSON"]);
    // Type de contenu absent.
    let body = t
        .send(
            Request::post("/api/v1/pages")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .assert_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid_json");
    assert_french(&body, &["Content-Type"]);
    // Route et méthode inconnues.
    t.get("/api/v1/inconnue")
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    t.send(Request::delete("/api/v1/tree").body(Body::empty()).unwrap())
        .await
        .assert_error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    assert!(t.tree().await.nodes.is_empty());
}

#[tokio::test]
async fn payload_too_large_is_413() {
    let t = TestApp::new().await;
    let page = t.create(json!({})).await;
    let before = t.get_mmd(&page.id).await.text();
    // Plus de 2 Mio : refusé avant toute lecture du MMD.
    let mmd = "a".repeat(mementum_server::MAX_BODY_BYTES + 1);
    let body = t
        .put_mmd(&page.id, mmd)
        .await
        .assert_error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");
    assert_french(&body, &["trop gros", "2097152"]);
    let big = json!({ "title": "a".repeat(mementum_server::MAX_BODY_BYTES) });
    t.post(big)
        .await
        .assert_error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");
    assert_eq!(t.get_mmd(&page.id).await.text(), before);
    assert_eq!(t.tree().await.nodes.len(), 1);
}

/// Ruling R23 : les combinaisons de `parentId` et `afterId`.
#[tokio::test]
async fn positions_combinations() {
    let t = TestApp::new().await;
    let r1 = t.create(json!({ "title": "R1" })).await;
    // `POST` avec `"parentId": null` : à la racine, en fin.
    let r2 = t.create(json!({ "title": "R2", "parentId": null })).await;
    assert_eq!(r2.parent_id, None);
    assert_eq!(t.children(None).await, titles(&["R1", "R2"]));

    // Sous un parent qui a déjà des enfants, sans `afterId` : en fin.
    let c1 = t.create(json!({ "title": "C1", "parentId": r1.id })).await;
    t.create(json!({ "title": "C2", "parentId": r1.id })).await;
    assert_eq!(t.children(Some(&r1.id)).await, titles(&["C1", "C2"]));
    // `parentId` avec `"afterId": null` : en tête du parent.
    t.create(json!({ "title": "C0", "parentId": r1.id, "afterId": null }))
        .await;
    assert_eq!(t.children(Some(&r1.id)).await, titles(&["C0", "C1", "C2"]));
    // `parentId` avec un id comme `afterId` : après lui.
    t.create(json!({ "title": "C1b", "parentId": r1.id, "afterId": c1.id }))
        .await;
    assert_eq!(
        t.children(Some(&r1.id)).await,
        titles(&["C0", "C1", "C1b", "C2"])
    );

    // `PATCH { "parentId": null, "afterId": null }` : à la racine, en tête.
    let moved = t
        .patched(&c1.id, json!({ "parentId": null, "afterId": null }))
        .await;
    assert_eq!(moved.parent_id, None);
    assert_eq!(t.children(None).await, titles(&["C1", "R1", "R2"]));
    // `PATCH { "parentId": null, "afterId": <frère à la racine> }`.
    t.patched(&c1.id, json!({ "parentId": null, "afterId": r1.id }))
        .await;
    assert_eq!(t.children(None).await, titles(&["R1", "C1", "R2"]));
    assert_eq!(t.children(Some(&r1.id)).await, titles(&["C0", "C1b", "C2"]));
    // Depuis un sous-niveau, après un frère de la racine.
    let c2 = t.node_titled("C2").await;
    t.patched(&c2.id, json!({ "parentId": null, "afterId": r2.id }))
        .await;
    assert_eq!(t.children(None).await, titles(&["R1", "C1", "R2", "C2"]));
}

/// Ruling R32 : si le document ne peut pas être écrit après le nœud, la page
/// est créée quand même (201) et le serveur le journalise ; son acteur lui
/// écrit son document au premier chargement (R28).
#[tokio::test]
async fn page_doc_failure_still_creates_page() {
    let t = TestApp::new().await;
    exec_sql(
        &t.db_path(),
        "CREATE TRIGGER panne BEFORE INSERT ON doc_snapshots \
         BEGIN SELECT RAISE(ABORT, 'panne simulée'); END",
    )
    .await;
    let logs = Logs::capture();
    let page = t.create(json!({ "title": "Sans document" })).await;
    assert_eq!(page.title, "Sans document");
    let text = logs.text();
    assert!(
        text.contains("WARN") && text.contains("document") && text.contains(&page.id),
        "avertissement attendu : {text}"
    );
    assert_eq!(t.node(&page.id).await.title, "Sans document");

    // La panne passée, la page s'ouvre avec la racine écrite par le serveur.
    exec_sql(&t.db_path(), "DROP TRIGGER panne").await;
    let got = t.get_mmd(&page.id).await;
    assert_eq!(got.status, StatusCode::OK, "{}", got.text());
    assert_eq!(got.text(), "<p/>\n");
}

/// Ruling R31 : sur la boucle locale, l'en-tête `Host` doit nommer la boucle
/// locale (DNS rebinding).
#[tokio::test]
async fn host_is_checked_on_loopback() {
    let t = TestApp::new().await;
    let request = |uri: &str, host: Option<&str>| {
        let mut builder = Request::get(uri);
        if let Some(host) = host {
            builder = builder.header(header::HOST, host);
        }
        builder.body(Body::empty()).unwrap()
    };
    for host in [
        "localhost",
        "localhost:5173",
        "LocalHost:8080",
        "127.0.0.1",
        "127.0.0.1:8080",
        "[::1]",
        "[::1]:8080",
    ] {
        let reply = t.send_raw(request("/health", Some(host))).await;
        assert_eq!(reply.status, StatusCode::OK, "Host: {host}");
    }
    for host in [
        "evil.example",
        "evil.example:8080",
        "localhost.evil.example",
        "127.0.0.1.evil.example:8080",
        "127.0.0.2",
        "[::2]:8080",
        "::1",
        "localhost:http",
        "",
    ] {
        for uri in ["/health", "/api/v1/tree", "/api/v1/inconnue"] {
            let body = t
                .send_raw(request(uri, Some(host)))
                .await
                .assert_error(StatusCode::MISDIRECTED_REQUEST, "bad_host");
            assert_french(&body, &["Host"]);
        }
    }
    // Sans en-tête `Host` : refusé aussi.
    t.send_raw(request("/health", None))
        .await
        .assert_error(StatusCode::MISDIRECTED_REQUEST, "bad_host");
    // Une écriture refusée n'écrit rien.
    t.send_raw(
        Request::post("/api/v1/pages")
            .header(header::HOST, "evil.example")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await
    .assert_error(StatusCode::MISDIRECTED_REQUEST, "bad_host");
    assert!(t.tree().await.nodes.is_empty());
}

/// Ruling R31 : hors de la boucle locale, le contrôle de `Host` est désactivé
/// et le démarrage le journalise ; l'`Origin` de `/sync` doit alors être
/// locale ou celle du serveur lui-même.
#[tokio::test]
async fn non_loopback_bind_warns_and_skips_host_check() {
    let logs = Logs::capture();
    let t = TestApp::with_bind("0.0.0.0:0").await;
    let text = logs.text();
    assert!(
        text.contains("WARN") && text.contains("boucle locale") && text.contains("Host"),
        "avertissement attendu : {text}"
    );
    let reply = t
        .send_raw(
            Request::get("/health")
                .header(header::HOST, "192.168.1.10:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);

    let page = t.create(json!({})).await;
    let sync = |origin: &str| {
        Request::get(format!("/sync/{}?schema={SCHEMA_VERSION}", page.id))
            .header(header::HOST, "192.168.1.10:8080")
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .unwrap()
    };
    // Même origine que le serveur : l'Origin passe (puis, sans poignée de
    // main en mémoire, la requête n'est pas une WebSocket).
    t.send_raw(sync("http://192.168.1.10:8080"))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_request");
    t.send_raw(sync("http://localhost:5173"))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_request");
    t.send_raw(sync("http://evil.example"))
        .await
        .assert_error(StatusCode::FORBIDDEN, "bad_origin");
}

/// Ruling R31 : une `Origin` présente doit nommer la boucle locale ; sans
/// `Origin` (client hors navigateur), la connexion passe.
#[tokio::test]
async fn sync_origin_is_checked() {
    let t = TestApp::new().await;
    let addr = t.listen().await;
    let page = t.create(json!({})).await;
    let url = format!("ws://{addr}/sync/{}?schema={SCHEMA_VERSION}", page.id);

    assert_eq!(handshake(&url, None).await, Ok(()));
    assert_eq!(handshake(&url, Some("http://localhost:5174")).await, Ok(()));
    assert_eq!(handshake(&url, Some("http://127.0.0.1:5173")).await, Ok(()));
    assert_eq!(handshake(&url, Some("https://[::1]")).await, Ok(()));
    for origin in [
        "http://evil.example",
        "http://localhost.evil.example:5173",
        "null",
        "file://",
    ] {
        assert_eq!(handshake(&url, Some(origin)).await, Err(403), "{origin}");
    }
    let client = Client::connect_synced(&url).await;
    assert_eq!(client.blocks().len(), 1);

    // La réponse 403 est l'erreur JSON habituelle.
    let body = t
        .send(
            Request::get(format!("/sync/{}?schema={SCHEMA_VERSION}", page.id))
                .header(header::ORIGIN, "http://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .assert_error(StatusCode::FORBIDDEN, "bad_origin");
    assert_french(&body, &["Origin"]);
}

/// Un client qui ne donne pas (ou mal) son schéma est en lecture seule, et le
/// serveur le journalise.
#[tokio::test]
async fn sync_without_schema_is_logged_read_only() {
    let t = TestApp::new().await;
    let addr = t.listen().await;
    let page = t.create(json!({})).await;
    for query in ["", "?schema=abc"] {
        let logs = Logs::capture();
        let url = format!("ws://{addr}/sync/{}{query}", page.id);
        let client = Client::connect_synced(&url).await;
        let text = logs.text();
        assert!(
            text.contains("lecture seule") && text.contains(&page.id),
            "journal attendu pour « {query} » : {text}"
        );
        drop(client);
    }
    // Avec le bon schéma, rien de tel.
    let logs = Logs::capture();
    let client = Client::connect_synced(&format!(
        "ws://{addr}/sync/{}?schema={SCHEMA_VERSION}",
        page.id
    ))
    .await;
    assert!(!logs.text().contains("lecture seule"), "{}", logs.text());
    drop(client);
}

#[test]
fn bindings_are_committed() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/protocol/src/generated");
    let expected: BTreeMap<String, String> =
        mementum_server::types::bindings().into_iter().collect();
    if std::env::var("UPDATE_BINDINGS").as_deref() == Ok("1") {
        std::fs::create_dir_all(&dir).unwrap();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !expected.contains_key(&name) {
                std::fs::remove_file(&path).unwrap();
            }
        }
        for (name, content) in &expected {
            std::fs::write(dir.join(name), content).unwrap();
        }
    }
    let committed: BTreeMap<String, String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| {
            panic!(
                "{} illisible ({e}) : lancer UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed",
                dir.display()
            )
        })
        .map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            (name, std::fs::read_to_string(&path).unwrap())
        })
        .collect();
    assert_eq!(
        committed, expected,
        "packages/protocol/src/generated périmé : lancer UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed"
    );
    for content in expected.values() {
        assert!(!content.contains('\r'), "fichiers générés en LF");
    }
}
