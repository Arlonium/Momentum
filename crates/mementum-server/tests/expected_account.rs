//! Le compte attendu (Rulings R540 à R542) : une requête peut nommer le compte sous lequel elle
//! part, par l'en-tête `X-Mementum-Account` (ou, pour une WebSocket, le paramètre d'adresse
//! `account`). Le client de l'application le pose sur ses appels : un autre onglet du même
//! navigateur a pu se connecter sous un autre compte depuis, et le cookie, commun, est alors le
//! sien. Le serveur la refuse alors, juste après le `401` et avant tout le reste, par `409
//! account_changed`, jamais `401` ; rien n'est lu ni écrit de ce que la route ferait. Sans en-tête,
//! rien ne change (agents, outils, tests).

mod common;

use std::path::Path;

use axum::body::Body;
use axum::http::{HeaderValue, Method, Request, StatusCode, header};
use common::sync::{members, open_app, user};
use common::{
    Account, Browser, PASSWORD, Reply, Team, handshake, json_request, outcome, page_routes,
    sync_url,
};
use mementum_acl::Level;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

/// L'en-tête du compte attendu.
const ACCOUNT: &str = "x-mementum-account";

/// `request`, avec l'en-tête du compte attendu, une ligne par valeur de `values`.
fn naming(mut request: Request<Body>, values: &[String]) -> Request<Body> {
    for value in values {
        request
            .headers_mut()
            .append(ACCOUNT, HeaderValue::from_str(value).unwrap());
    }
    request
}

/// Ce que la vérification doit répondre.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Expected {
    /// La route répond comme sans en-tête.
    Passes,
    /// `409 account_changed`.
    Changed,
    /// `400 invalid_request`.
    Unreadable,
}

/// Les valeurs de l'en-tête à éprouver pour le compte `me`, `other` étant un autre compte de
/// l'instance : `(nom, valeurs, réponse attendue)`.
fn cases(me: Uuid, other: Uuid) -> Vec<(&'static str, Vec<String>, Expected)> {
    let me = me.to_string();
    vec![
        ("absent", vec![], Expected::Passes),
        ("juste", vec![me.clone()], Expected::Passes),
        (
            "juste, en majuscules",
            vec![me.to_uppercase()],
            Expected::Passes,
        ),
        (
            "un autre compte",
            vec![other.to_string()],
            Expected::Changed,
        ),
        (
            "un compte qui n'existe pas",
            vec![Uuid::now_v7().to_string()],
            Expected::Changed,
        ),
        (
            "le compte nil",
            vec![Uuid::nil().to_string()],
            Expected::Changed,
        ),
        (
            "mal formé",
            vec!["pas-un-compte".to_string()],
            Expected::Unreadable,
        ),
        ("vide", vec![String::new()], Expected::Unreadable),
        (
            "entouré d'espaces",
            vec![format!(" {me} ")],
            Expected::Unreadable,
        ),
        (
            "deux fois, juste",
            vec![me.clone(), me.clone()],
            Expected::Unreadable,
        ),
        (
            "deux fois, juste puis un autre",
            vec![me.clone(), other.to_string()],
            Expected::Unreadable,
        ),
        (
            "deux fois, un autre puis juste",
            vec![other.to_string(), me],
            Expected::Unreadable,
        ),
    ]
}

/// Vérifie le refus attendu, ou rend `false` si la route doit répondre comme sans en-tête.
fn refused(reply: &Reply, expected: Expected, what: &str) -> bool {
    match expected {
        Expected::Passes => false,
        Expected::Changed => {
            let body = reply.assert_error(StatusCode::CONFLICT, "account_changed");
            assert_eq!(
                body["message"],
                "tu t'es connecté sous un autre compte dans un autre onglet ; rien n'a été fait",
                "{what}"
            );
            true
        }
        Expected::Unreadable => {
            let body = reply.assert_error(StatusCode::BAD_REQUEST, "invalid_request");
            assert!(
                body["message"]
                    .as_str()
                    .unwrap()
                    .contains("X-Mementum-Account"),
                "{what} : {body}"
            );
            true
        }
    }
}

/// Chaque ligne de chaque table de la base, colonnes comprises, dans un ordre fixe : deux bases
/// qui rendent la même chose n'ont pas reçu d'écriture entre-temps.
async fn dump(db: &Path) -> Vec<String> {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    let tables: Vec<String> = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' \
         AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx%' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .iter()
    .map(|row| row.get(0))
    .collect();
    assert!(tables.len() > 10, "tables : {tables:?}");
    let mut rows = Vec::new();
    for table in tables {
        let columns: Vec<String> = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT name FROM pragma_table_info('{table}')"
        )))
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|row| row.get(0))
        .collect();
        let line = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        let mut lines: Vec<String> = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {line} FROM \"{table}\""
        )))
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|row| format!("{table}: {}", row.get::<String, _>(0)))
        .collect();
        lines.sort();
        rows.extend(lines);
    }
    pool.close().await;
    rows
}

/// Les valeurs de l'en-tête sur une route de lecture, une route d'écriture et `GET /me` : la
/// matrice de R540. Le refus ne dit jamais `401`, et une écriture refusée n'a rien écrit.
#[tokio::test]
async fn the_named_account_is_checked_on_reads_writes_and_the_account() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let bruno = t.account("Bruno").await;
    let page = alice.create(json!({ "title": "Notes" })).await;
    let uri = format!("/api/v1/pages/{}", page.id);
    for (what, values, expected) in cases(alice.id, bruno.id) {
        let read = alice
            .browser
            .send(naming(
                Request::get(&uri).body(Body::empty()).unwrap(),
                &values,
            ))
            .await;
        if !refused(&read, expected, what) {
            read.assert_status(StatusCode::OK);
            assert_eq!(read.json()["id"], page.id.as_str(), "{what}");
        }

        let title = format!("Titre {what}");
        let write = alice
            .browser
            .send(naming(
                json_request(Method::PATCH, &uri, &json!({ "title": title })),
                &values,
            ))
            .await;
        let stored = alice.node(&page.id).await.title;
        if refused(&write, expected, what) {
            assert_ne!(stored, title, "{what} : rien n'est écrit");
        } else {
            write.assert_status(StatusCode::OK);
            assert_eq!(stored, title, "{what}");
        }

        let me = alice
            .browser
            .send(naming(
                Request::get("/api/v1/me").body(Body::empty()).unwrap(),
                &values,
            ))
            .await;
        if !refused(&me, expected, what) {
            me.assert_status(StatusCode::OK);
            assert_eq!(me.json()["user"]["id"], alice.id.to_string(), "{what}");
        }
    }
}

/// Le changement de mot de passe, route de session qui demande une session : un en-tête qui ne
/// nomme pas le compte de la session est refusé avant que rien ne soit vérifié ni changé (ni le
/// mot de passe, ni les sessions). La connexion, l'inscription
/// et la déconnexion ne lisent pas de session : l'en-tête n'y change rien (R542).
#[tokio::test]
async fn the_password_change_checks_the_named_account_and_the_other_session_routes_ignore_it() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let bruno = t.account("Bruno").await;
    let body = json!({ "current": PASSWORD, "new": "un-tout-nouveau-mot-de-passe" });
    for (what, values, expected) in cases(alice.id, bruno.id) {
        if expected == Expected::Passes {
            continue;
        }
        let before = dump(&t.db_path()).await;
        let reply = alice
            .browser
            .send(naming(
                json_request(Method::POST, "/api/v1/auth/password", &body),
                &values,
            ))
            .await;
        assert!(refused(&reply, expected, what));
        assert!(reply.set_cookies().is_empty(), "{what}");
        assert_eq!(
            dump(&t.db_path()).await,
            before,
            "{what} : rien n'est écrit"
        );
    }
    // Le compte de la session : le mot de passe change.
    let reply = alice
        .browser
        .send(naming(
            json_request(Method::POST, "/api/v1/auth/password", &body),
            &[alice.id.to_string()],
        ))
        .await;
    reply.assert_status(StatusCode::NO_CONTENT);

    // La connexion et l'inscription ouvrent une session, la déconnexion ferme celle du
    // navigateur : un en-tête qui nomme un autre compte n'y change rien.
    let other = [bruno.id.to_string()];
    let browser = t.browser();
    let login = browser
        .send(naming(
            json_request(
                Method::POST,
                "/api/v1/auth/login",
                &json!({ "email": "alice@exemple.fr", "password": "un-tout-nouveau-mot-de-passe" }),
            ),
            &other,
        ))
        .await;
    login.assert_status(StatusCode::OK);
    let signup = t
        .browser()
        .send(naming(
            json_request(
                Method::POST,
                "/api/v1/auth/signup",
                &json!({ "email": "chloe@exemple.fr", "name": "Chloé", "password": PASSWORD }),
            ),
            &other,
        ))
        .await;
    signup.assert_status(StatusCode::CREATED);
    let logout = browser
        .send(naming(
            Request::post("/api/v1/auth/logout")
                .body(Body::empty())
                .unwrap(),
            &other,
        ))
        .await;
    logout.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(browser.session(), None);
}

/// L'ordre des refus : `401` d'abord (sans session, ou une session fermée, quel que soit
/// l'en-tête), puis `409 account_changed`, avant tout le reste : la page, le niveau, la
/// corbeille, le corps. Sur chaque route d'une page, la même réponse, que la page existe ou non,
/// qu'on la voie ou non, à la corbeille ou non : elle ne dit rien de la page. Et rien n'est écrit,
/// ni dans les pages, ni dans les accès, ni dans les sessions.
#[tokio::test]
async fn the_account_refusal_comes_right_after_the_401_and_writes_nothing() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    // Mona peut tout faire de cette page : sans en-tête, chacune de ces routes écrirait.
    let open = team
        .page(None, "Commune", &[members(Level::Full)])
        .await
        .to_string();
    let private = team
        .page(None, "Privée de Tom", &[user(&team.tom, Level::Full)])
        .await
        .to_string();
    let read_only = team
        .page(None, "En lecture", &[members(Level::Read)])
        .await
        .to_string();
    let trashed = team.page(None, "Jetée", &[members(Level::Full)]).await;
    team.trash(trashed).await;
    let unknown = Uuid::now_v7().to_string();
    let olga = [team.olga.id.to_string()];

    // `401` d'abord.
    let nobody = t.browser();
    let closed = t.browser();
    let leaving = t.browser();
    leaving
        .log_in("mona@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    closed.set_session(leaving.session().as_deref());
    leaving
        .log_out()
        .await
        .assert_status(StatusCode::NO_CONTENT);
    for (who, by) in [("sans session", &nobody), ("session fermée", &closed)] {
        let me = Request::get("/api/v1/me").body(Body::empty()).unwrap();
        for (route, request) in page_routes(&open).into_iter().chain([("GET me", me)]) {
            let reply = by.send(naming(request, &olga)).await;
            assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{who}, {route}");
            reply.assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
        }
    }

    // Puis `409`, avant tout le reste, et sans rien écrire.
    let mona = &team.mona.browser;
    let before = dump(&t.db_path()).await;
    let first = mona
        .send(naming(
            Request::get(format!("/api/v1/pages/{open}"))
                .body(Body::empty())
                .unwrap(),
            &olga,
        ))
        .await;
    first.assert_error(StatusCode::CONFLICT, "account_changed");
    for (which, id) in [
        ("modifiable", open.as_str()),
        ("qu'elle ne voit pas", private.as_str()),
        ("qu'elle ne fait que lire", read_only.as_str()),
        ("à la corbeille", &trashed.to_string()),
        ("inconnue", unknown.as_str()),
        ("mal nommée", "pas-une-page"),
    ] {
        for (route, request) in page_routes(id) {
            let reply = mona.send(naming(request, &olga)).await;
            assert_eq!(outcome(&reply), outcome(&first), "page {which}, {route}");
        }
    }
    let space = team.space;
    let space_routes = [
        (
            "arbre",
            Request::get(format!("/api/v1/spaces/{space}/tree"))
                .body(Body::empty())
                .unwrap(),
        ),
        (
            "arbre d'un espace où elle n'est rien",
            Request::get(format!("/api/v1/spaces/{}/tree", team.zoe.space))
                .body(Body::empty())
                .unwrap(),
        ),
        (
            "créer une page",
            json_request(
                Method::POST,
                "/api/v1/pages",
                &json!({ "spaceId": space.to_string(), "section": "shared", "title": "Neuve" }),
            ),
        ),
        (
            "créer un espace",
            json_request(Method::POST, "/api/v1/spaces", &json!({ "name": "Neuf" })),
        ),
        (
            "renommer l'espace",
            json_request(
                Method::PATCH,
                &format!("/api/v1/spaces/{space}"),
                &json!({ "name": "Renommé" }),
            ),
        ),
        (
            "quitter l'espace",
            Request::delete(format!("/api/v1/spaces/{space}/members/{}", team.mona.id))
                .body(Body::empty())
                .unwrap(),
        ),
        (
            "accepter une invitation",
            json_request(
                Method::POST,
                "/api/v1/invitations/accept",
                &json!({ "token": "x" }),
            ),
        ),
        (
            "bornes des fichiers",
            Request::get("/api/v1/files/limits")
                .body(Body::empty())
                .unwrap(),
        ),
    ];
    for (route, request) in space_routes {
        let reply = mona.send(naming(request, &olga)).await;
        assert_eq!(outcome(&reply), outcome(&first), "{route}");
    }
    assert_eq!(
        dump(&t.db_path()).await,
        before,
        "aucune écriture sous account_changed"
    );

    // Témoin : sans l'en-tête, la même suite écrit, et `dump` le voit.
    for (_, request) in page_routes(&open) {
        mona.send(request).await;
    }
    assert_ne!(dump(&t.db_path()).await, before);
}

/// `ws://…/<chemin>` avec `query` en plus de ce que l'adresse porte déjà.
fn with_query(url: &str, query: &str) -> String {
    if url.contains('?') {
        format!("{url}&{query}")
    } else {
        format!("{url}?{query}")
    }
}

/// `GET` sans demande de WebSocket, avec une `Origin` facultative : la réponse d'avant la
/// poignée de main, corps compris.
async fn plain_get(by: &Browser<'_>, uri: &str, origin: Option<&str>) -> Reply {
    let mut request = Request::get(uri);
    if let Some(origin) = origin {
        request = request.header(header::ORIGIN, origin);
    }
    by.send(request.body(Body::empty()).unwrap()).await
}

/// Une poignée de main avec la session de `by` et l'en-tête du compte attendu.
async fn handshake_naming(url: &str, by: &Account<'_>, value: &str) -> Result<(), u16> {
    use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
    let mut request = url.into_client_request().unwrap();
    let headers = request.headers_mut();
    headers.insert(
        header::COOKIE,
        by.browser.cookie().unwrap().parse().unwrap(),
    );
    headers.insert(ACCOUNT, value.parse().unwrap());
    match tokio_tungstenite::connect_async(request).await {
        Ok(_) => Ok(()),
        Err(tungstenite::Error::Http(response)) => Err(response.status().as_u16()),
        Err(e) => panic!("poignée de main : {e}"),
    }
}

/// Les WebSockets (`/sync`, `/events`), à qui un navigateur ne laisse poser aucun en-tête : le
/// paramètre `account` en tient lieu, vérifié par la même règle (R541). Ordre des refus d'avant
/// la poignée de main : `403 bad_origin`, `401`, puis `409 account_changed` (ou `400` pour un
/// paramètre illisible), avant le `404` d'une page ou d'un espace qu'on ne voit pas et le `400`
/// d'une requête sans demande de WebSocket. L'en-tête, qu'un client hors navigateur peut poser,
/// vaut aussi.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websockets_check_the_account_parameter_in_the_order_of_their_refusals() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let page = team.page(None, "Commune", &[members(Level::Edit)]).await;
    let private = team
        .page(None, "Privée de Tom", &[user(&team.tom, Level::Full)])
        .await;
    let mona = &team.mona;
    let me = mona.id.to_string();
    let olga = team.olga.id.to_string();
    let cookie = mona.browser.cookie();
    let sync_ws = |page: Uuid, query: &str| with_query(&sync_url(addr, page), query);
    let sync_path = |page: Uuid, query: &str| {
        with_query(
            &format!("/sync/{page}?schema={}", mementum_schema::SCHEMA_VERSION),
            query,
        )
    };
    let events_ws = |space: Uuid, query: &str| format!("ws://{addr}/events/{space}?{query}");
    let events_path = |space: Uuid, query: &str| format!("/events/{space}?{query}");
    let right = format!("account={me}");
    let other = format!("account={olga}");

    // Le compte de la session : la connexion s'ouvre, sur les deux routes ; sans paramètre aussi.
    for url in [
        sync_ws(page, &right),
        sync_ws(page, &format!("account={}", me.to_uppercase())),
        sync_ws(page, "rien=0"),
        events_ws(team.space, &right),
        events_ws(team.space, "rien=0"),
    ] {
        assert_eq!(
            handshake(&url, None, cookie.as_deref()).await,
            Ok(()),
            "{url}"
        );
    }

    // Un autre compte : `409`, même pour une page ou un espace qu'on ne voit pas, et sans demande
    // de WebSocket ; un nom de paramètre écrit en `%xx` est le même nom.
    let changed = plain_get(&mona.browser, &sync_path(page, &other), None).await;
    changed.assert_error(StatusCode::CONFLICT, "account_changed");
    for (what, path, url) in [
        ("/sync", sync_path(page, &other), sync_ws(page, &other)),
        (
            "/sync, page qu'elle ne voit pas",
            sync_path(private, &other),
            sync_ws(private, &other),
        ),
        (
            "/sync, nom en %xx",
            sync_path(page, &format!("acc%6Funt={olga}")),
            sync_ws(page, &format!("acc%6Funt={olga}")),
        ),
        (
            "/sync, compte qui n'existe pas",
            sync_path(page, &format!("account={}", Uuid::now_v7())),
            sync_ws(page, &format!("account={}", Uuid::now_v7())),
        ),
        (
            "/events",
            events_path(team.space, &other),
            events_ws(team.space, &other),
        ),
        (
            "/events, espace où elle n'est rien",
            events_path(team.zoe.space, &other),
            events_ws(team.zoe.space, &other),
        ),
    ] {
        let reply = plain_get(&mona.browser, &path, None).await;
        assert_eq!(outcome(&reply), outcome(&changed), "{what}");
        assert_eq!(
            handshake(&url, None, cookie.as_deref()).await,
            Err(409),
            "{what}"
        );
    }

    // Illisible : un seul identifiant, une seule fois.
    for (what, query) in [
        ("mal formé", "account=pas-un-compte".to_string()),
        ("vide", "account=".to_string()),
        ("deux fois, juste", format!("{right}&{right}")),
        ("deux fois, juste puis un autre", format!("{right}&{other}")),
    ] {
        for (route, path, url) in [
            ("/sync", sync_path(page, &query), sync_ws(page, &query)),
            (
                "/events",
                events_path(team.space, &query),
                events_ws(team.space, &query),
            ),
        ] {
            let reply = plain_get(&mona.browser, &path, None).await;
            let body = reply.assert_error(StatusCode::BAD_REQUEST, "invalid_request");
            assert!(
                body["message"]
                    .as_str()
                    .unwrap()
                    .contains("paramètre account"),
                "{route}, {what} : {body}"
            );
            assert_eq!(
                handshake(&url, None, cookie.as_deref()).await,
                Err(400),
                "{route}, {what}"
            );
        }
    }

    // Avant lui, `403 bad_origin` puis `401`.
    let evil = Some("http://evil.example");
    plain_get(&mona.browser, &sync_path(page, &other), evil)
        .await
        .assert_error(StatusCode::FORBIDDEN, "bad_origin");
    plain_get(&mona.browser, &events_path(team.space, &other), evil)
        .await
        .assert_error(StatusCode::FORBIDDEN, "bad_origin");
    let nobody = t.browser();
    plain_get(&nobody, &sync_path(page, &other), None)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    plain_get(&nobody, &events_path(team.space, &other), None)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert_eq!(
        handshake(&sync_ws(page, &other), None, None).await,
        Err(401)
    );

    // L'en-tête, pour un client hors navigateur : la même règle.
    assert_eq!(
        handshake_naming(&sync_url(addr, page), mona, &olga).await,
        Err(409)
    );
    assert_eq!(
        handshake_naming(&sync_url(addr, page), mona, &me).await,
        Ok(())
    );
}
