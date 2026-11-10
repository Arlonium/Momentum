//! Aides des tests du serveur, partagées par `api.rs`, `auth.rs` et les
//! suivants.
//!
//! Les requêtes passent par `app()` en mémoire (`tower::ServiceExt::oneshot`) ;
//! la WebSocket, par un écouteur sur un port libre de 127.0.0.1 (jamais 8080,
//! Ruling R24).
//!
//! Aucune route de page ne répond sans session : un test de pages se donne un
//! compte ([`TestApp::account`], avec son espace) ou une équipe ([`Team`],
//! un espace où chacun a son rôle). La synchro non plus : sa WebSocket
//! s'ouvre avec la session d'un navigateur ([`sync_as`], [`synced_as`]).
//!
//! Deux façons de lire les journaux :
//! - [`Logs`] : ceux du fil du test. Les tests qui en lisent tournent en
//!   `current_thread` (défaut de `#[tokio::test]`), routes et acteurs compris ;
//! - [`all_logs`] : ceux de tous les fils du processus de test, au niveau le
//!   plus bavard (fils de SQLite et des calculs bloquants compris).
#![allow(dead_code)]

pub mod sync;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_schema::SCHEMA_VERSION;
use mementum_server::{Config, PageNode, Tree};
use mementum_store::{NewNode, NodeKind, Store};
use mementum_sync::testkit::Client;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use uuid::Uuid;

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

    /// Crée le compte `<nom en minuscules>@exemple.fr` et rend ce qu'il faut pour écrire des
    /// pages : sa session et son espace. Hors du premier compte, les inscriptions doivent être
    /// ouvertes.
    pub async fn account(&self, name: &str) -> Account<'_> {
        let email = format!("{}@exemple.fr", name.to_lowercase());
        let (browser, me) = self.signed_up(&email, name).await;
        Account {
            id: uuid(&me["user"]["id"]),
            space: uuid(&me["spaces"][0]["id"]),
            browser,
        }
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

    /// L'en-tête `Cookie` que ce navigateur envoie : celle de sa session,
    /// s'il en porte une.
    pub fn cookie(&self) -> Option<String> {
        self.session()
            .map(|value| format!("{SESSION_COOKIE}={value}"))
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

    pub async fn put(&self, uri: &str, body: Value) -> Reply {
        self.send(json_request(Method::PUT, uri, &body)).await
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
}

/// Un id écrit dans une réponse JSON.
pub fn uuid(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap_or_else(|| panic!("un id : {value}")))
        .unwrap_or_else(|e| panic!("{value} n'est pas un UUID : {e}"))
}

/// Un compte : le navigateur qui porte sa session, son id, et l'espace que son inscription lui
/// a donné (« Espace de `<nom>` », dont il est propriétaire). Les aides de pages visent cet
/// espace.
pub struct Account<'a> {
    pub browser: Browser<'a>,
    pub id: Uuid,
    pub space: Uuid,
}

impl Account<'_> {
    /// `POST /api/v1/pages`. À la racine (`parentId` absent ou `null`), `spaceId` est ajouté au
    /// corps s'il n'y est pas : l'espace du compte.
    pub async fn post(&self, mut body: Value) -> Reply {
        if let Some(fields) = body.as_object_mut()
            && fields.get("parentId").is_none_or(Value::is_null)
            && !fields.contains_key("spaceId")
        {
            fields.insert("spaceId".to_string(), json!(self.space.to_string()));
        }
        self.browser.post("/api/v1/pages", body).await
    }

    /// Crée une page (voir [`Account::post`]) : `201`.
    pub async fn create(&self, body: Value) -> PageNode {
        let reply = self.post(body).await;
        reply.assert_status(StatusCode::CREATED);
        serde_json::from_slice(&reply.body).unwrap()
    }

    pub async fn patch(&self, id: &str, body: Value) -> Reply {
        self.browser
            .patch(&format!("/api/v1/pages/{id}"), body)
            .await
    }

    pub async fn patched(&self, id: &str, body: Value) -> PageNode {
        let reply = self.patch(id, body).await;
        reply.assert_status(StatusCode::OK);
        serde_json::from_slice(&reply.body).unwrap()
    }

    pub async fn get_mmd(&self, id: &str) -> Reply {
        self.browser.get_mmd(id).await
    }

    pub async fn put_mmd(&self, id: &str, mmd: impl Into<Body>) -> Reply {
        self.browser.put_mmd(id, mmd).await
    }

    /// L'arbre de l'espace du compte (`GET /api/v1/spaces/{id}/tree`).
    pub async fn tree(&self) -> Tree {
        let reply = self
            .browser
            .get(&format!("/api/v1/spaces/{}/tree", self.space))
            .await;
        reply.assert_status(StatusCode::OK);
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

    pub async fn node_titled(&self, title: &str) -> PageNode {
        self.tree()
            .await
            .nodes
            .into_iter()
            .find(|n| n.title == title)
            .unwrap_or_else(|| panic!("aucune page « {title} »"))
    }

    /// Titres des enfants de `parent` (`None` = racine), dans l'ordre des positions.
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
}

/// Le champ texte `key` d'un objet JSON.
pub fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("« {key} » en texte attendu dans {value}"))
}

/// Les nœuds de l'arbre d'un espace, tels que `by` les voit : `200`, `spaceId` est l'espace
/// demandé.
pub async fn tree_nodes(by: &Browser<'_>, space: Uuid) -> Vec<Value> {
    let reply = by.get(&format!("/api/v1/spaces/{space}/tree")).await;
    reply.assert_status(StatusCode::OK);
    let tree = reply.json();
    assert_eq!(tree["spaceId"], space.to_string(), "{tree}");
    tree["nodes"]
        .as_array()
        .unwrap_or_else(|| panic!("« nodes » en liste attendu dans {tree}"))
        .clone()
}

/// La corbeille d'un espace, telle que `by` la voit (`GET /api/v1/spaces/{id}/trash`) : `200`.
pub async fn trash_list(by: &Browser<'_>, space: Uuid) -> Vec<Value> {
    let reply = by.get(&format!("/api/v1/spaces/{space}/trash")).await;
    reply.assert_status(StatusCode::OK);
    reply
        .json()
        .as_array()
        .unwrap_or_else(|| panic!("une liste attendue : {}", reply.text()))
        .clone()
}

/// Une équipe, pour les tests de droits : l'espace « Équipe » d'Olga (propriétaire), avec Adam
/// (admin), Mona et Tom (membres) et Gina (invitée). Sam, le premier compte, administre
/// l'instance mais n'a aucun rôle dans l'espace ; Zoé non plus, qui n'a que son propre espace.
///
/// Les inscriptions de l'application doivent être ouvertes. Les rôles et les accès sont posés
/// par le store : les routes qui les posent ne sont pas celles de ces tests.
pub struct Team<'a> {
    pub sam: Account<'a>,
    pub olga: Account<'a>,
    pub adam: Account<'a>,
    pub mona: Account<'a>,
    pub tom: Account<'a>,
    pub gina: Account<'a>,
    pub zoe: Account<'a>,
    /// L'espace « Équipe ».
    pub space: Uuid,
    /// Le store, sur la même base que l'application.
    pub store: Store,
}

impl<'a> Team<'a> {
    pub async fn new(t: &'a TestApp) -> Team<'a> {
        let sam = t.account("Sam").await;
        let olga = t.account("Olga").await;
        let adam = t.account("Adam").await;
        let mona = t.account("Mona").await;
        let tom = t.account("Tom").await;
        let gina = t.account("Gina").await;
        let zoe = t.account("Zoé").await;
        let store = Store::open(&t.db_path()).await.unwrap();
        let mut team = Team {
            sam,
            olga,
            adam,
            mona,
            tom,
            gina,
            zoe,
            space: Uuid::nil(),
            store,
        };
        team.space = team.another_space("Équipe").await;
        team
    }

    /// Un espace de plus, avec les mêmes rôles que « Équipe ».
    pub async fn another_space(&self, name: &str) -> Uuid {
        let space = self
            .store
            .create_space(name, Some(self.olga.id))
            .await
            .unwrap()
            .id;
        for (account, role) in [
            (&self.adam, Role::Admin),
            (&self.mona, Role::Member),
            (&self.tom, Role::Member),
            (&self.gina, Role::Guest),
        ] {
            self.store
                .add_member(space, account.id, role)
                .await
                .unwrap();
        }
        space
    }

    /// Une page de l'espace « Équipe », créée par Olga, avec ces accès posés sur elle : en
    /// tête des enfants de `parent` (`None` = la racine).
    pub async fn page(
        &self,
        parent: Option<Uuid>,
        title: &str,
        grants: &[(Subject, Level)],
    ) -> Uuid {
        self.page_in(self.space, parent, title, grants).await
    }

    /// Comme [`Team::page`], dans un autre espace.
    pub async fn page_in(
        &self,
        space: Uuid,
        parent: Option<Uuid>,
        title: &str,
        grants: &[(Subject, Level)],
    ) -> Uuid {
        self.store
            .create_node(NewNode {
                space_id: space,
                parent_id: parent,
                after: None,
                kind: NodeKind::Page,
                title: title.to_string(),
                created_by: Some(self.olga.id),
                grants: grants
                    .iter()
                    .map(|&(subject, level)| Grant { subject, level })
                    .collect(),
                doc: None,
            })
            .await
            .unwrap()
            .id
    }

    /// Met la page à la corbeille, par le store, au nom d'Olga.
    pub async fn trash(&self, page: Uuid) {
        self.store.trash_node(page, self.olga.id).await.unwrap();
    }

    /// Les accès posés sur la page, tels qu'ils sont en base.
    pub async fn grants(&self, page: Uuid) -> Vec<(Subject, Level)> {
        self.store
            .node_grants(page)
            .await
            .unwrap()
            .into_iter()
            .map(|g| (g.subject, g.level))
            .collect()
    }
}

/// Une requête sur chaque route qui porte l'id d'une page dans son chemin, corps valides et
/// corps illisibles : `(nom, requête)`.
pub fn page_routes(id: &str) -> Vec<(&'static str, Request<Body>)> {
    let page = format!("/api/v1/pages/{id}");
    let bare = |method: Method, uri: String| {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    };
    let patch = |body: Value| json_request(Method::PATCH, &page, &body);
    let put = |mmd: &'static str| {
        Request::put(format!("{page}/mmd"))
            .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
            .body(Body::from(mmd))
            .unwrap()
    };
    vec![
        ("GET page", bare(Method::GET, page.clone())),
        ("PATCH titre", patch(json!({ "title": "Autre titre" }))),
        ("PATCH place", patch(json!({ "afterId": null }))),
        ("PATCH parent", patch(json!({ "parentId": null }))),
        ("PATCH sans rien", patch(json!({}))),
        ("PATCH champ inconnu", patch(json!({ "titre": "Autre" }))),
        (
            "PATCH illisible",
            Request::patch(&page)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{"))
                .unwrap(),
        ),
        ("POST trash", bare(Method::POST, format!("{page}/trash"))),
        (
            "POST restore",
            bare(Method::POST, format!("{page}/restore")),
        ),
        ("DELETE", bare(Method::DELETE, page.clone())),
        ("GET mmd", bare(Method::GET, format!("{page}/mmd"))),
        (
            "PUT mmd",
            put("Texte
"),
        ),
        (
            "PUT mmd illisible",
            put("* x
"),
        ),
        // Le partage de la page (`routes/sharing.rs`), pour un compte quelconque.
        ("GET access", bare(Method::GET, format!("{page}/access"))),
        (
            "PUT accès général",
            json_request(
                Method::PUT,
                &format!("{page}/access/general"),
                &json!({ "level": "read" }),
            ),
        ),
        (
            "PUT accès général illisible",
            Request::put(format!("{page}/access/general"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{"))
                .unwrap(),
        ),
        (
            "PUT accès d'une personne",
            json_request(
                Method::PUT,
                &format!("{page}/access/users/{SOMEONE}"),
                &json!({ "level": "read" }),
            ),
        ),
        (
            "DELETE accès d'une personne",
            bare(Method::DELETE, format!("{page}/access/users/{SOMEONE}")),
        ),
        (
            "POST invitation sur la page",
            json_request(
                Method::POST,
                &format!("{page}/invitations"),
                &json!({ "email": "quelqu-un@exemple.fr", "level": "read" }),
            ),
        ),
    ]
}

/// Un compte quelconque, visé par les routes de partage de [`page_routes`].
const SOMEONE: &str = "01890000-0000-7000-8000-000000000001";

/// Ce qui fait qu'une réponse est la même qu'une autre : statut, type de contenu, corps.
pub fn outcome(reply: &Reply) -> (StatusCode, Option<String>, String) {
    (reply.status, reply.content_type.clone(), reply.text())
}

/// Journaux `tracing` émis sur le fil courant tant que la valeur vit.
///
/// Elle tient aussi un second abonné, inerte, pour toute sa durée. Sans lui, quand cet abonné
/// est le seul du processus, `tracing` décide de l'intérêt d'un point de journal d'après le fil
/// qui l'atteint le premier (`tracing-core`, `Rebuilder::JustOne`) : atteint d'abord par le fil
/// d'un autre test, qui n'a pas d'abonné, il est tenu pour éteint, et ce fil-ci n'en reçoit
/// plus rien. Un test qui lit une ligne que d'autres tests font aussi écrire échouait ainsi
/// une fois sur deux ou trois. Avec deux abonnés inscrits, l'intérêt se décide sur tous.
pub struct Logs {
    buffer: Arc<Mutex<Vec<u8>>>,
    _guard: tracing::subscriber::DefaultGuard,
    _never_alone: tracing::Dispatch,
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
        // Inscrit avant l'abonné du fil, et lâché après lui.
        let never_alone = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
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
            _never_alone: never_alone,
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

/// Combien de temps un test tient le verrou d'écriture de SQLite avant d'y écrire : la moitié
/// du délai d'attente d'une connexion (5 s, `Store::open`), la marge la plus large des deux
/// côtés. La requête a 2,5 s pour lire la base d'avant et venir attendre le verrou ; le test en
/// garde 2,5 pour écrire et valider avant qu'elle renonce.
pub const HELD: Duration = Duration::from_millis(2_500);

/// Une requête prise de vitesse par une écriture ([`overtaken`]) : sa réponse, et ce que le
/// serveur a journalisé pendant qu'elle tournait.
pub struct Overtaken {
    pub reply: Reply,
    pub journal: String,
}

impl Overtaken {
    /// Combien de fois la route d'un `PATCH` a relu `page` et redécidé, son écriture refusée
    /// par le store. Une fois au moins : la requête avait bien décidé sur la base d'avant.
    /// Arrivée après l'écriture concurrente, elle n'aurait rien eu à relire, et le test ne
    /// prouverait rien.
    pub fn redecided(&self, page: Uuid) -> usize {
        reprises(&self.journal, page)
    }
}

/// Le nombre de reprises d'un `PATCH` sur `page` que porte le journal du serveur : une ligne
/// par écriture que le store a refusée, et que la route a redécidée.
pub fn reprises(journal: &str, page: Uuid) -> usize {
    let page = page.to_string();
    journal
        .lines()
        .filter(|line| line.contains("relit") && line.contains(&page))
        .count()
}

/// Envoie `request` pendant que le test tient le verrou d'écriture de SQLite : elle lit donc
/// la base d'avant, décide, puis attend pour écrire. `meanwhile` (du SQL) est alors écrit, avec
/// la révision de l'espace comme le fait le store, et le verrou est rendu : l'écriture de la
/// requête passe après.
///
/// Le journal est celui du fil du test ([`Logs`]) : à appeler d'un test en `current_thread`.
pub async fn overtaken(
    t: &TestApp,
    space: Uuid,
    request: impl Future<Output = Reply>,
    meanwhile: &[String],
) -> Overtaken {
    let logs = Logs::capture();
    let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", t.db_path().display()))
        .await
        .unwrap();
    let mut lock = side.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let written_meanwhile = async {
        tokio::time::sleep(HELD).await;
        let rev = format!("UPDATE spaces SET rev = rev + 1 WHERE id = '{space}'");
        for sql in meanwhile.iter().chain([&rev]) {
            sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .execute(&mut *lock)
                .await
                .unwrap();
        }
        lock.commit().await.unwrap();
    };
    let (reply, ()) = tokio::join!(request, written_meanwhile);
    side.close().await;
    Overtaken {
        reply,
        journal: logs.text(),
    }
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

/// L'URL de la WebSocket de synchro d'une page, pour un client du schéma
/// courant, sur l'adresse rendue par [`TestApp::listen`].
pub fn sync_url(addr: std::net::SocketAddr, page: impl std::fmt::Display) -> String {
    format!("ws://{addr}/sync/{page}?schema={SCHEMA_VERSION}")
}

/// Ouvre la WebSocket de synchro `url` avec la session de `by` (sans cookie
/// s'il n'en porte pas). `Err` : le refus de la poignée de main, où figure
/// son statut HTTP.
pub async fn sync_as(by: &Browser<'_>, url: &str) -> Result<Client, String> {
    let cookie = by.cookie();
    let headers: Vec<(&str, &str)> = cookie
        .iter()
        .map(|cookie| ("cookie", cookie.as_str()))
        .collect();
    Client::connect_with_headers(url, &headers).await
}

/// Comme [`sync_as`], pour une connexion qui doit aboutir : attend la
/// première étape 2 du serveur ([`WAIT`] au plus).
pub async fn synced_as(by: &Browser<'_>, url: &str) -> Client {
    let client = sync_as(by, url)
        .await
        .unwrap_or_else(|refusal| panic!("connexion de synchro refusée : {refusal}"));
    assert!(
        client.wait_until(WAIT, |c| c.is_synced()).await,
        "le client n'a pas reçu l'état du serveur"
    );
    client
}

/// Poignée de main WebSocket avec une en-tête `Origin` et un cookie
/// facultatifs : `Ok` si elle aboutit, sinon le statut HTTP de la réponse.
pub async fn handshake(url: &str, origin: Option<&str>, cookie: Option<&str>) -> Result<(), u16> {
    use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
    let mut request = url.into_client_request().unwrap();
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert(header::ORIGIN, origin.parse().unwrap());
    }
    if let Some(cookie) = cookie {
        request
            .headers_mut()
            .insert(header::COOKIE, cookie.parse().unwrap());
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
