//! Aides des tests du serveur, partagées par `api.rs`, `auth.rs` et les
//! suivants.
//!
//! Les requêtes passent par `app()` en mémoire (`tower::ServiceExt::oneshot`) ;
//! la WebSocket, par un écouteur sur un port libre de 127.0.0.1 (jamais 8080,
//! Ruling R24).
//!
//! Deux façons de lire les journaux :
//! - [`Logs`] : ceux du fil du test. Les tests qui en lisent tournent en
//!   `current_thread` (défaut de `#[tokio::test]`), routes et acteurs compris ;
//! - [`all_logs`] : ceux de tous les fils du processus de test, au niveau le
//!   plus bavard (fils de SQLite et des calculs bloquants compris).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use mementum_server::{Config, PageNode, Tree};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

/// Délai maximal d'une attente de convergence.
pub const WAIT: Duration = Duration::from_secs(10);

/// Nom du cookie de session (contrat de l'API).
pub const SESSION_COOKIE: &str = "mementum_session";

/// Mot de passe des comptes de test : une constante manifestement fausse.
pub const PASSWORD: &str = "mot-de-passe-de-test";

pub struct TestApp {
    _dir: TempDir,
    pub cfg: Config,
    pub router: Router,
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub content_type: Option<String>,
    pub body: Bytes,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| {
            panic!(
                "réponse {} illisible en JSON ({e}) : {:?}",
                self.status,
                String::from_utf8_lossy(&self.body)
            )
        })
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.body.to_vec()).expect("une réponse en UTF-8")
    }

    /// Vérifie le statut et la forme d'une erreur : `{ "error": code,
    /// "message": texte non vide }`, en JSON.
    pub fn assert_error(&self, status: StatusCode, code: &str) -> Value {
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

    /// Vérifie le statut d'une réponse qui n'est pas une erreur.
    pub fn assert_status(&self, status: StatusCode) -> &Reply {
        assert_eq!(self.status, status, "réponse : {}", self.text());
        self
    }

    /// Les en-têtes `Set-Cookie` de la réponse, telles quelles.
    pub fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("Set-Cookie lisible").to_string())
            .collect()
    }

    /// L'en-tête `Set-Cookie` du cookie de session : une seule, sinon le
    /// test échoue.
    pub fn session_set_cookie(&self) -> String {
        let mut cookies: Vec<String> = self
            .set_cookies()
            .into_iter()
            .filter(|c| c.starts_with(&format!("{SESSION_COOKIE}=")))
            .collect();
        assert_eq!(
            cookies.len(),
            1,
            "un Set-Cookie de session attendu : {:?}",
            self.set_cookies()
        );
        cookies.remove(0)
    }
}

impl TestApp {
    pub async fn new() -> TestApp {
        TestApp::with_config(|_| {}).await
    }

    /// `app()` ne lie pas l'adresse : `bind` ne règle que les contrôles.
    pub async fn with_bind(bind: &str) -> TestApp {
        TestApp::with_config(|cfg| cfg.bind = bind.parse().unwrap()).await
    }

    /// Une application sur un dossier de données neuf, avec la configuration
    /// par défaut (boucle locale, inscriptions sur invitation) retouchée par
    /// `adjust`.
    pub async fn with_config(adjust: impl FnOnce(&mut Config)) -> TestApp {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            // Un dossier qui n'existe pas encore : `app` le crée.
            data_dir: dir.path().join("donnees"),
            ..Config::default()
        };
        adjust(&mut cfg);
        let router = mementum_server::app(&cfg).await.unwrap();
        TestApp {
            _dir: dir,
            cfg,
            router,
        }
    }

    /// Une application démarrée sur une base du lot 1 ([`lot1_database`]),
    /// que son ouverture migre. Rend aussi ce que la base contenait.
    pub async fn on_lot1_database() -> (TestApp, Lot1) {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            data_dir: dir.path().join("donnees"),
            ..Config::default()
        };
        std::fs::create_dir_all(&cfg.data_dir).unwrap();
        let lot1 = lot1_database(&cfg.db_path()).await;
        let router = mementum_server::app(&cfg).await.unwrap();
        let app = TestApp {
            _dir: dir,
            cfg,
            router,
        };
        (app, lot1)
    }

    /// Envoie la requête avec `Host: localhost` si elle n'en a pas (comme un
    /// navigateur, ou le mandataire de Vite).
    pub async fn send(&self, mut req: Request<Body>) -> Reply {
        if !req.headers().contains_key(header::HOST) {
            req.headers_mut()
                .insert(header::HOST, "localhost".parse().unwrap());
        }
        self.send_raw(req).await
    }

    /// Envoie la requête telle quelle.
    pub async fn send_raw(&self, req: Request<Body>) -> Reply {
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string());
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        Reply {
            status,
            headers,
            content_type,
            body,
        }
    }

    pub async fn get(&self, uri: &str) -> Reply {
        self.send(Request::get(uri).body(Body::empty()).unwrap())
            .await
    }

    pub async fn json(&self, method: Method, uri: &str, body: Value) -> Reply {
        self.send(json_request(method, uri, &body)).await
    }

    pub async fn post(&self, body: Value) -> Reply {
        self.json(Method::POST, "/api/v1/pages", body).await
    }

    pub async fn create(&self, body: Value) -> PageNode {
        let reply = self.post(body).await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text());
        serde_json::from_slice(&reply.body).unwrap()
    }

    pub async fn patch(&self, id: &str, body: Value) -> Reply {
        self.json(Method::PATCH, &format!("/api/v1/pages/{id}"), body)
            .await
    }

    pub async fn patched(&self, id: &str, body: Value) -> PageNode {
        let reply = self.patch(id, body).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text());
        serde_json::from_slice(&reply.body).unwrap()
    }

    pub async fn get_mmd(&self, id: &str) -> Reply {
        self.get(&format!("/api/v1/pages/{id}/mmd")).await
    }

    pub async fn put_mmd(&self, id: &str, mmd: impl Into<Body>) -> Reply {
        self.send(
            Request::put(format!("/api/v1/pages/{id}/mmd"))
                .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
                .body(mmd.into())
                .unwrap(),
        )
        .await
    }

    pub async fn tree(&self) -> Tree {
        let reply = self.get("/api/v1/tree").await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text());
        serde_json::from_slice(&reply.body).unwrap()
    }

    pub async fn node(&self, id: &str) -> PageNode {
        self.tree()
            .await
            .nodes
            .into_iter()
            .find(|n| n.id == id)
            .unwrap_or_else(|| panic!("page {id} absente de l'arbre"))
    }

    /// Titres des enfants de `parent` (`None` = racine), dans l'ordre des
    /// positions.
    pub async fn children(&self, parent: Option<&str>) -> Vec<String> {
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
    pub async fn listen(&self) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = self.router.clone();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        addr
    }

    pub fn db_path(&self) -> PathBuf {
        self.cfg.db_path()
    }

    pub async fn node_titled(&self, title: &str) -> PageNode {
        self.tree()
            .await
            .nodes
            .into_iter()
            .find(|n| n.title == title)
            .unwrap_or_else(|| panic!("aucune page « {title} »"))
    }

    /// Un navigateur sans session.
    pub fn browser(&self) -> Browser<'_> {
        Browser {
            app: self,
            session: Mutex::new(None),
        }
    }

    /// Crée un compte (mot de passe [`PASSWORD`]) et rend le navigateur qui
    /// porte sa session, avec le `Me` de la réponse.
    pub async fn signed_up(&self, email: &str, name: &str) -> (Browser<'_>, Value) {
        let browser = self.browser();
        let reply = browser.sign_up(email, name, PASSWORD, None).await;
        reply.assert_status(StatusCode::CREATED);
        assert!(browser.session().is_some(), "cookie de session attendu");
        (browser, reply.json())
    }
}

/// Ce que contient la base écrite par [`lot1_database`] : un coffre, une page
/// « Racine » et sa sous-page « Sous-page ».
pub struct Lot1 {
    pub vault: uuid::Uuid,
    pub root: uuid::Uuid,
    pub child: uuid::Uuid,
}

/// Écrit dans `db` une base telle que le code du lot 1 la laisse, comme dans
/// les tests du store : les options de connexion de `Store::open`, la seule
/// migration `0001_init.sql` lancée par sqlx, puis une organisation, un coffre
/// et deux pages écrits en SQL. Les connexions sont fermées au retour.
pub async fn lot1_database(db: &Path) -> Lot1 {
    use sqlx::sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
    };
    let migrations = tempfile::tempdir().unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mementum-store/migrations/0001_init.sql"),
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
    sqlx::migrate::Migrator::new(migrations.path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();

    let org = uuid::Uuid::now_v7();
    let lot1 = Lot1 {
        vault: uuid::Uuid::now_v7(),
        root: uuid::Uuid::now_v7(),
        child: uuid::Uuid::now_v7(),
    };
    sqlx::query(
        "INSERT INTO orgs (id, slug, name, created_at) VALUES (?, 'default', 'default', 1000)",
    )
    .bind(org.to_string())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO vaults (id, org_id, name, kind, created_at) \
         VALUES (?, ?, 'Mon coffre', 'personal', 1001)",
    )
    .bind(lot1.vault.to_string())
    .bind(org.to_string())
    .execute(&pool)
    .await
    .unwrap();
    for (id, parent, title) in [
        (lot1.root, None, "Racine"),
        (lot1.child, Some(lot1.root), "Sous-page"),
    ] {
        sqlx::query(
            "INSERT INTO nodes \
                 (id, org_id, vault_id, parent_id, kind, title, position, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 'page', ?, 'a0', 2000, 2000)",
        )
        .bind(id.to_string())
        .bind(org.to_string())
        .bind(lot1.vault.to_string())
        .bind(parent.map(|p| p.to_string()))
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    }
    pool.close().await;
    lot1
}

/// Une requête au corps JSON.
pub fn json_request(method: Method, uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Un navigateur de test : il renvoie le cookie de session qu'une réponse lui
/// a donné, et l'oublie quand une réponse l'efface.
pub struct Browser<'a> {
    app: &'a TestApp,
    /// La valeur du cookie de session, s'il en porte un.
    session: Mutex<Option<String>>,
}

impl Browser<'_> {
    /// La valeur du cookie de session porté, s'il y en a un.
    pub fn session(&self) -> Option<String> {
        self.session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Pose (ou retire) le cookie de session à la main : cookie rejoué,
    /// forgé, d'un autre navigateur.
    pub fn set_session(&self, value: Option<&str>) {
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = value.map(str::to_string);
    }

    /// Envoie la requête avec le cookie de session porté, puis suit les
    /// `Set-Cookie` de la réponse comme un navigateur : `Max-Age=0` ou une
    /// valeur vide effacent le cookie.
    pub async fn send(&self, mut req: Request<Body>) -> Reply {
        if let Some(value) = self.session() {
            req.headers_mut().insert(
                header::COOKIE,
                format!("{SESSION_COOKIE}={value}").parse().unwrap(),
            );
        }
        let reply = self.app.send(req).await;
        for raw in reply.set_cookies() {
            let cookie = cookie::Cookie::parse(raw.clone())
                .unwrap_or_else(|e| panic!("Set-Cookie illisible ({e}) : {raw}"));
            if cookie.name() != SESSION_COOKIE {
                continue;
            }
            let erased = cookie.value().is_empty() || cookie.max_age().is_some_and(|d| d.is_zero());
            self.set_session((!erased).then(|| cookie.value()));
        }
        reply
    }

    pub async fn get(&self, uri: &str) -> Reply {
        self.send(Request::get(uri).body(Body::empty()).unwrap())
            .await
    }

    /// `POST` sans corps.
    pub async fn post_empty(&self, uri: &str) -> Reply {
        self.send(Request::post(uri).body(Body::empty()).unwrap())
            .await
    }

    pub async fn post(&self, uri: &str, body: Value) -> Reply {
        self.send(json_request(Method::POST, uri, &body)).await
    }

    pub async fn patch(&self, uri: &str, body: Value) -> Reply {
        self.send(json_request(Method::PATCH, uri, &body)).await
    }

    /// `DELETE` sans corps.
    pub async fn delete(&self, uri: &str) -> Reply {
        self.send(Request::delete(uri).body(Body::empty()).unwrap())
            .await
    }

    pub async fn sign_up(
        &self,
        email: &str,
        name: &str,
        password: &str,
        invitation: Option<&str>,
    ) -> Reply {
        let mut body = json!({ "email": email, "name": name, "password": password });
        if let Some(token) = invitation {
            body["invitation"] = json!(token);
        }
        self.post("/api/v1/auth/signup", body).await
    }

    pub async fn log_in(&self, email: &str, password: &str) -> Reply {
        self.post(
            "/api/v1/auth/login",
            json!({ "email": email, "password": password }),
        )
        .await
    }

    pub async fn log_out(&self) -> Reply {
        self.post_empty("/api/v1/auth/logout").await
    }

    pub async fn change_password(&self, current: &str, new: &str) -> Reply {
        self.post(
            "/api/v1/auth/password",
            json!({ "current": current, "new": new }),
        )
        .await
    }

    pub async fn me(&self) -> Reply {
        self.get("/api/v1/me").await
    }
}

/// Journaux `tracing` émis sur le fil courant tant que la valeur vit.
pub struct Logs {
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
    pub fn capture() -> Logs {
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

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.buffer.lock().unwrap_or_else(PoisonError::into_inner))
            .into_owned()
    }
}

/// Les journaux de tout le processus de test : ceux de chaque fil qui n'a pas
/// son propre [`Logs`] (fils des autres tests, de SQLite et des calculs
/// bloquants compris), au niveau `TRACE`. L'abonné est posé au premier appel
/// et vit jusqu'à la fin du processus ; `text` rend tout ce qui a été écrit
/// depuis.
pub struct AllLogs {
    buffer: Arc<Mutex<Vec<u8>>>,
}

pub fn all_logs() -> &'static AllLogs {
    static LOGS: OnceLock<AllLogs> = OnceLock::new();
    LOGS.get_or_init(|| {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || LogWriter(writer.clone()))
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("un seul abonné global par processus de test");
        AllLogs { buffer }
    })
}

impl AllLogs {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.buffer.lock().unwrap_or_else(PoisonError::into_inner))
            .into_owned()
    }
}

/// Exécute du SQL sur la base, par une connexion à part (pannes simulées,
/// échéances avancées).
pub async fn exec_sql(db: &Path, sql: &str) {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

/// La première colonne de la première ligne d'une requête, par une connexion
/// à part.
pub async fn query_i64(db: &Path, sql: &str) -> i64 {
    use sqlx::Row;
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    let value = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    pool.close().await;
    value
}

/// Poignée de main WebSocket avec une en-tête `Origin` facultative :
/// `Ok` si elle aboutit, sinon le statut HTTP de la réponse.
pub async fn handshake(url: &str, origin: Option<&str>) -> Result<(), u16> {
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

/// Un message d'erreur en français, sans la cause de serde ou de std recopiée.
pub fn assert_french(body: &Value, expected: &[&str]) {
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
