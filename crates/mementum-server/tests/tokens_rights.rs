//! L'identité par jeton d'API (lot 5, Task S5) : un `Authorization: Bearer mmt_…` reconnu par la
//! porte des jetons, restreint dans `access.rs`, borné par le débit (Rulings R1214 à R1218,
//! R1222, R1241, R1242, R1244).
//!
//! Les jetons des tests sont tirés par le test et écrits par le store ([`api_token`]) : jamais
//! un vrai jeton.

mod common;
// La seule limite connue de ce binaire ne se produit que sous Windows (voir `known_limits`).
#[cfg(windows)]
#[path = "../../mementum-sync/tests/common/limit.rs"]
mod limit;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::*;
use mementum_acl::{Level, Role, Subject};
use mementum_server::SignupMode;
use mementum_server::auth::{SESSION_ONLY_ROUTES, TOKEN_ROUTES};
use mementum_store::{Expected, TokenLevel, TokenScope};
use serde_json::{Value, json};
use uuid::Uuid;

async fn open_app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

fn bare(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

/// `GET uri` par le jeton.
async fn get_by(t: &TestApp, token: &str, uri: &str) -> Reply {
    t.send(with_bearer(bare(Method::GET, uri), token)).await
}

/// Une requête JSON par le jeton.
async fn json_by(t: &TestApp, token: &str, method: Method, uri: &str, body: Value) -> Reply {
    t.send(with_bearer(json_request(method, uri, &body), token))
        .await
}

fn page_uri(id: Uuid) -> String {
    format!("/api/v1/pages/{id}")
}

/// Statut, code d'erreur et message : deux refus pareils ont les trois.
fn refusal(reply: &Reply) -> (StatusCode, Value) {
    (reply.status, reply.json())
}

/// Une page des membres de l'espace « Équipe » (Olga l'a créée ; Mona, membre, y a « Peut
/// modifier »).
async fn team_page(team: &Team<'_>, parent: Option<Uuid>, title: &str) -> Uuid {
    team.page(parent, title, &[(Subject::Members, Level::Edit)])
        .await
}

// ---------------------------------------------------------------------------
// La porte.

/// Les routes que déclare le routeur (`fn router` de `src/lib.rs`), par méthode et chemin.
fn declared_routes() -> Vec<(String, String)> {
    let source = include_str!("../src/lib.rs");
    let router = &source[source.find("fn router(").expect("fn router")..];
    let mut routes = Vec::new();
    let mut rest = router;
    while let Some(at) = rest.find(".route(") {
        rest = &rest[at + ".route(".len()..];
        // La fin de l'appel : la parenthèse qui ferme `.route(`.
        let mut depth = 1;
        let mut end = 0;
        for (i, c) in rest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                end = i;
                break;
            }
        }
        let call = &rest[..end];
        let path = call.split('"').nth(1).expect("le chemin de la route");
        for method in ["get", "post", "put", "patch", "delete"] {
            let mut search = call;
            while let Some(found) = search.find(&format!("{method}(")) {
                let before = search[..found].chars().last();
                if before.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == ':')) {
                    routes.push((method.to_uppercase(), path.to_string()));
                    break;
                }
                search = &search[found + 1..];
            }
        }
    }
    routes.sort();
    routes.dedup();
    routes
}

/// Toute route est dans l'une des deux listes, et une seule ; aucune liste ne nomme une route
/// que le routeur n'a pas. Une route neuve doit choisir (Ruling R1222).
#[test]
fn only_listed_routes_accept_a_token() {
    let declared = declared_routes();
    assert!(declared.len() > 40, "le routeur se lit : {declared:?}");
    let listed = |list: &[(&str, &str)], route: &(String, String)| {
        list.iter()
            .any(|(method, path)| *method == route.0 && *path == route.1)
    };
    for route in &declared {
        let open = listed(TOKEN_ROUTES, route);
        let closed = listed(SESSION_ONLY_ROUTES, route);
        assert!(
            open != closed,
            "{route:?} : ouverte {open}, fermée {closed}"
        );
    }
    for (method, path) in TOKEN_ROUTES.iter().chain(SESSION_ONLY_ROUTES) {
        assert!(
            declared.iter().any(|(m, p)| m == method && p == path),
            "{method} {path} n'est pas une route du routeur"
        );
    }
    // Ce que R1222 ferme toujours, nommé ici pour qu'une erreur de liste se voie.
    for closed in [
        ("PUT", "/api/v1/pages/{id}/mmd"),
        ("DELETE", "/api/v1/pages/{id}"),
        ("PUT", "/api/v1/pages/{id}/access/general"),
        ("POST", "/api/v1/pages/{id}/files"),
        ("POST", "/api/v1/auth/password"),
        // Un jeton ne crée, ne liste ni ne révoque un jeton (Task S6).
        ("GET", "/api/v1/tokens"),
        ("POST", "/api/v1/tokens"),
        ("DELETE", "/api/v1/tokens/{id}"),
        ("GET", "/api/v1/tokens/{id}/calls"),
        ("GET", "/sync/{id}"),
        ("GET", "/events/{id}"),
    ] {
        assert!(SESSION_ONLY_ROUTES.contains(&closed), "{closed:?}");
    }
}

/// Un jeton vivant, de niveau « Modification », de portée « tous les espaces », du propriétaire
/// d'un espace, reçoit `403 token_not_allowed` sur chaque route fermée, cookie vivant compris, et
/// rien n'est écrit (la page est toujours là, son contenu et ses accès aussi, aucun jeton de plus).
#[tokio::test]
async fn every_closed_route_refuses_a_live_token() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team_page(&team, None, "Notes").await;
    let (token, _) = api_token(
        &t.store,
        team.olga.id,
        TokenLevel::Edit,
        TokenScope::All,
        true,
    )
    .await;
    let before = team.grants(page).await;
    let mmd_before = team.olga.get_mmd(&page.to_string()).await.text();
    for (method, path) in SESSION_ONLY_ROUTES {
        let id = if path.starts_with("/api/v1/spaces/") || path.starts_with("/events/") {
            team.space
        } else {
            page
        };
        let uri = path
            .replace("{id}", &id.to_string())
            .replace("{user_id}", &team.mona.id.to_string())
            .replace("{emoji}", "%F0%9F%91%8D")
            .replace("{thread_id}", &Uuid::now_v7().to_string())
            .replace("{comment_id}", &Uuid::now_v7().to_string())
            .replace("{version_id}", &Uuid::now_v7().to_string());
        let mut request = json_request(
            method.parse().unwrap(),
            &uri,
            &json!({ "title": "x", "level": "read", "email": "a@exemple.fr" }),
        );
        // Le cookie vivant d'Olga : le jeton l'emporte, et ne retombe pas sur lui.
        request.headers_mut().insert(
            header::COOKIE,
            team.olga.browser.cookie().unwrap().parse().unwrap(),
        );
        let reply = t.send(with_bearer(request, &token)).await;
        reply.assert_error(StatusCode::FORBIDDEN, "token_not_allowed");
    }
    assert_eq!(team.grants(page).await, before);
    assert_eq!(
        team.olga.get_mmd(&page.to_string()).await.text(),
        mmd_before
    );
    let tokens = t.store.tokens_of(team.olga.id, now()).await.unwrap();
    assert_eq!(tokens.len(), 1, "aucun jeton créé par un jeton");
    // Aucune ligne au journal : refusé avant d'être admis.
    t.calls.flush(&t.store).await;
    let calls = t
        .store
        .calls_of(team.olga.id, tokens[0].id, None, None, 100, now())
        .await
        .unwrap();
    assert!(calls.is_empty(), "{calls:?}");
}

/// `PUT /mmd`, la restauration d'une version (les routes de `SessionUser`), `/sync` et `/events`
/// refusent un `Bearer`, même porté avec le cookie vivant de son compte (Ruling R1214).
#[tokio::test]
async fn session_user_routes_sync_and_events_refuse_a_bearer() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let page = mona
        .create(json!({ "spaceId": mona.space, "title": "P" }))
        .await;
    let (token, _) = api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    let cases = [
        Request::put(format!("/api/v1/pages/{}/mmd", page.id))
            .body(Body::from("Texte\n"))
            .unwrap(),
        bare(
            Method::POST,
            &format!(
                "/api/v1/pages/{}/versions/{}/restore",
                page.id,
                Uuid::now_v7()
            ),
        ),
        bare(Method::GET, &format!("/sync/{}?schema=1", page.id)),
        bare(Method::GET, &format!("/events/{}", mona.space)),
    ];
    for mut request in cases {
        request.headers_mut().insert(
            header::COOKIE,
            mona.browser.cookie().unwrap().parse().unwrap(),
        );
        let reply = t.send(with_bearer(request, &token)).await;
        reply.assert_error(StatusCode::FORBIDDEN, "token_not_allowed");
    }
}

/// Le `Bearer` de Mona avec le cookie vivant d'Olga agit pour Mona, restreint : il ne voit pas la
/// page privée d'Olga, et ce qu'il crée est à Mona ; révoqué, il reçoit `401`, jamais Olga.
#[tokio::test]
async fn a_bearer_of_one_account_with_the_live_cookie_of_another_acts_for_the_first() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let shared = team_page(&team, None, "Commune").await;
    let olgas = team
        .page(
            None,
            "À Olga",
            &[(Subject::User(team.olga.id), Level::Full)],
        )
        .await;
    let (token, token_id) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    let with_olga = |request: Request<Body>| {
        let mut request = with_bearer(request, &token);
        request.headers_mut().insert(
            header::COOKIE,
            team.olga.browser.cookie().unwrap().parse().unwrap(),
        );
        request
    };
    let reply = t.send(with_olga(bare(Method::GET, &page_uri(olgas)))).await;
    reply.assert_error(StatusCode::NOT_FOUND, "not_found");
    let reply = t
        .send(with_olga(bare(Method::GET, &page_uri(shared))))
        .await;
    assert_eq!(reply.assert_status(StatusCode::OK).json()["level"], "edit");
    let created = t
        .send(with_olga(json_request(
            Method::POST,
            "/api/v1/pages",
            &json!({ "spaceId": team.space, "title": "Par l'agent" }),
        )))
        .await;
    let id = uuid(&created.assert_status(StatusCode::CREATED).json()["id"]);
    let node = t.store.get_node(id).await.unwrap().unwrap();
    assert_eq!(node.created_by, Some(team.mona.id));
    t.store
        .revoke_token(team.mona.id, token_id, now())
        .await
        .unwrap();
    let reply = t
        .send(with_olga(bare(Method::GET, &page_uri(shared))))
        .await;
    reply.assert_error(StatusCode::UNAUTHORIZED, "invalid_token");
}

/// Un `Bearer mmt_…` qui n'est pas un jeton vivant ne retombe jamais sur la session vivante qu'il
/// accompagne.
#[tokio::test]
async fn an_invalid_bearer_never_falls_back_on_the_session() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let page = mona
        .create(json!({ "spaceId": mona.space, "title": "P" }))
        .await;
    let (unknown, _) = mementum_server::auth::new_token();
    for token in [
        "mmt_".to_string(),
        "mmt_pas-un-jeton".to_string(),
        format!("mmt_{unknown}"),
    ] {
        let mut request = with_bearer(
            bare(Method::GET, &format!("/api/v1/pages/{}", page.id)),
            &token,
        );
        request.headers_mut().insert(
            header::COOKIE,
            mona.browser.cookie().unwrap().parse().unwrap(),
        );
        t.send(request)
            .await
            .assert_error(StatusCode::UNAUTHORIZED, "invalid_token");
    }
}

/// Un `Authorization` qui n'est pas un `Bearer mmt_…` est ignoré hors de `/mcp` : la session
/// joue, et sans session, c'est le `401` d'une session absente (Ruling R1214).
#[tokio::test]
async fn an_authorization_without_the_prefix_is_ignored_outside_mcp() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let page = mona
        .create(json!({ "spaceId": mona.space, "title": "P" }))
        .await;
    let uri = format!("/api/v1/pages/{}", page.id);
    for value in ["Bearer abc", "Basic bW9uYTpzZWNyZXQ=", "Bearer mmtx_abc"] {
        let mut request = bare(Method::GET, &uri);
        request
            .headers_mut()
            .insert(header::AUTHORIZATION, value.parse().unwrap());
        assert_eq!(
            mona.browser.send(request).await.status,
            StatusCode::OK,
            "{value}"
        );
        let mut request = bare(Method::GET, &uri);
        request
            .headers_mut()
            .insert(header::AUTHORIZATION, value.parse().unwrap());
        t.send(request)
            .await
            .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
}

/// Mal formé, inconnu, révoqué ou échu : la même réponse, octet pour octet, avec
/// `WWW-Authenticate: Bearer` (Ruling R1215).
#[tokio::test]
async fn invalid_token_has_one_answer() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let page = mona
        .create(json!({ "spaceId": mona.space, "title": "P" }))
        .await;
    let uri = format!("/api/v1/pages/{}", page.id);
    let (revoked, revoked_id) =
        api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    t.store
        .revoke_token(mona.id, revoked_id, now())
        .await
        .unwrap();
    let (text, hash) = mementum_server::auth::new_token();
    t.store
        .create_token(mementum_store::NewToken {
            user_id: mona.id,
            name: "échu".to_string(),
            hash,
            hint: text[text.len() - 4..].to_string(),
            level: TokenLevel::Edit,
            scope: TokenScope::All,
            can_trash: false,
            expires_at: Some(now() - 1),
            now: now() - 10,
        })
        .await
        .unwrap();
    let expired = format!("mmt_{text}");
    let (unknown, _) = mementum_server::auth::new_token();
    let mut answers = Vec::new();
    for token in [
        "mmt_court".to_string(),
        format!("mmt_{unknown}"),
        revoked,
        expired,
    ] {
        let reply = get_by(&t, &token, &uri).await;
        reply.assert_error(StatusCode::UNAUTHORIZED, "invalid_token");
        assert_eq!(
            reply.headers.get(header::WWW_AUTHENTICATE).unwrap(),
            "Bearer"
        );
        answers.push((reply.status, reply.body.clone()));
    }
    assert!(answers.windows(2).all(|w| w[0] == w[1]), "{answers:?}");
}

/// Révoqué, un jeton ne fait plus rien dès l'appel suivant.
#[tokio::test]
async fn a_revoked_token_is_refused_at_the_next_call() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let page = mona
        .create(json!({ "spaceId": mona.space, "title": "P" }))
        .await;
    let uri = format!("/api/v1/pages/{}", page.id);
    let (token, id) = api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    get_by(&t, &token, &uri).await.assert_status(StatusCode::OK);
    t.store.revoke_token(mona.id, id, now()).await.unwrap();
    get_by(&t, &token, &uri)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_token");
}

// ---------------------------------------------------------------------------
// Les droits.

/// Les requêtes des routes ouvertes à un jeton sur la page `id` (et son fichier).
fn open_page_routes(id: Uuid) -> Vec<(&'static str, Request<Body>)> {
    let page = page_uri(id);
    vec![
        ("GET page", bare(Method::GET, &page)),
        (
            "PATCH titre",
            json_request(Method::PATCH, &page, &json!({ "title": "Autre" })),
        ),
        ("POST trash", bare(Method::POST, &format!("{page}/trash"))),
        (
            "POST restore",
            bare(Method::POST, &format!("{page}/restore")),
        ),
        (
            "POST duplicate",
            bare(Method::POST, &format!("{page}/duplicate")),
        ),
        ("GET mmd", bare(Method::GET, &format!("{page}/mmd"))),
        (
            "GET rétroliens",
            bare(Method::GET, &format!("{page}/backlinks")),
        ),
        (
            "GET fichier",
            bare(Method::GET, &format!("{page}/files/{}", Uuid::now_v7())),
        ),
        // Les routes des agents (Task S7).
        (
            "GET document",
            bare(Method::GET, &format!("{page}/document")),
        ),
        (
            "POST opérations",
            json_request(
                Method::POST,
                &format!("{page}/operations"),
                &json!({ "operations": [{ "op": "insert", "position": "end", "markdown": "Plus\n" }] }),
            ),
        ),
    ]
}

/// Une page hors de la portée d'un jeton reçoit, sur chaque route de page qu'il ouvre, la réponse
/// d'une page inconnue, octet pour octet : une page sœur d'une page de sa portée, une page d'un
/// espace hors de sa portée.
#[tokio::test]
async fn a_page_out_of_scope_is_indistinguishable_from_missing() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let scoped = team_page(&team, None, "Dans la portée").await;
    let sibling = team_page(&team, None, "À côté").await;
    let elsewhere = team.another_space("Ailleurs").await;
    let far = team
        .page_in(elsewhere, None, "Loin", &[(Subject::Members, Level::Edit)])
        .await;
    let (by_pages, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::Pages {
            space: team.space,
            pages: vec![scoped],
        },
        true,
    )
    .await;
    let (by_space, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::Space(team.space),
        true,
    )
    .await;
    let unknown = Uuid::now_v7();
    for (token, hidden) in [(&by_pages, sibling), (&by_pages, far), (&by_space, far)] {
        let mut seen = Vec::new();
        let mut missing = Vec::new();
        for ((name, hidden_request), (_, unknown_request)) in open_page_routes(hidden)
            .into_iter()
            .zip(open_page_routes(unknown))
        {
            let hidden_reply = t.send(with_bearer(hidden_request, token)).await;
            let unknown_reply = t.send(with_bearer(unknown_request, token)).await;
            assert_eq!(hidden_reply.status, StatusCode::NOT_FOUND, "{name}");
            seen.push(refusal(&hidden_reply));
            missing.push(refusal(&unknown_reply));
        }
        assert_eq!(seen, missing);
    }
    // La création en lot dans un espace hors de la portée : la réponse d'un espace inconnu.
    let batch = |space: Uuid| {
        json_request(
            Method::POST,
            &format!("/api/v1/spaces/{space}/pages/batch"),
            &json!({ "pages": [{ "title": "Neuve" }] }),
        )
    };
    for token in [&by_pages, &by_space] {
        let hidden = t.send(with_bearer(batch(elsewhere), token)).await;
        let unknown = t.send(with_bearer(batch(Uuid::now_v7()), token)).await;
        assert_eq!(hidden.status, StatusCode::NOT_FOUND, "lot ailleurs");
        assert_eq!(refusal(&hidden), refusal(&unknown));
    }
    // La session de Mona voit la page sœur : c'est bien le jeton qui la cache.
    team.mona
        .browser
        .get(&page_uri(sibling))
        .await
        .assert_status(StatusCode::OK);
}

/// Un jeton de lecture n'écrit rien, par aucune route qu'il ouvre ; la page reste telle quelle.
#[tokio::test]
async fn a_read_token_writes_nothing() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team_page(&team, None, "Notes").await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Read,
        TokenScope::All,
        true,
    )
    .await;
    let nodes_before = t.store.list_nodes(team.space).await.unwrap().len();
    let writes = [
        json_request(Method::PATCH, &page_uri(page), &json!({ "title": "Autre" })),
        json_request(Method::PATCH, &page_uri(page), &json!({ "afterId": null })),
        bare(Method::POST, &format!("{}/trash", page_uri(page))),
        bare(Method::POST, &format!("{}/restore", page_uri(page))),
        bare(Method::POST, &format!("{}/duplicate", page_uri(page))),
        json_request(
            Method::POST,
            "/api/v1/pages",
            &json!({ "spaceId": team.space, "title": "Neuve" }),
        ),
        json_request(
            Method::POST,
            "/api/v1/pages",
            &json!({ "parentId": page, "title": "Sous-page" }),
        ),
        // Les routes des agents qui écrivent (Task S7).
        json_request(
            Method::POST,
            &format!("{}/operations", page_uri(page)),
            &json!({ "operations": [{ "op": "insert", "position": "end", "markdown": "Plus\n" }] }),
        ),
        json_request(
            Method::POST,
            &format!("/api/v1/spaces/{}/pages/batch", team.space),
            &json!({ "pages": [{ "title": "Neuve", "markdown": "Texte\n" }] }),
        ),
        json_request(
            Method::POST,
            &format!("/api/v1/spaces/{}/pages/batch", team.space),
            &json!({ "parentId": page, "pages": [{ "title": "Sous-page" }] }),
        ),
    ];
    for request in writes {
        let reply = t.send(with_bearer(request, &token)).await;
        reply.assert_error(StatusCode::FORBIDDEN, "forbidden");
    }
    let node = t.store.get_node(page).await.unwrap().unwrap();
    assert_eq!((node.title.as_str(), node.trashed_at), ("Notes", None));
    assert_eq!(
        t.store.list_nodes(team.space).await.unwrap().len(),
        nodes_before
    );
    // Ce qu'il lit, il le lit.
    get_by(&t, &token, &format!("{}/mmd", page_uri(page)))
        .await
        .assert_status(StatusCode::OK);
}

/// Un jeton ne change jamais une page de parent, même au compte qui y a l'accès complet : ni à
/// la racine, ni sous une page de l'équipe, ni depuis « Privé ». Il la déplace parmi ses sœurs.
#[tokio::test]
async fn a_token_never_moves_a_page_under_another_parent() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let parent = team_page(&team, None, "Parent").await;
    let child = team_page(&team, Some(parent), "Enfant").await;
    let other = team_page(&team, None, "Autre").await;
    let private = team
        .page(
            None,
            "Privée",
            &[(Subject::User(team.olga.id), Level::Full)],
        )
        .await;
    let (token, _) = api_token(
        &t.store,
        team.olga.id,
        TokenLevel::Edit,
        TokenScope::All,
        true,
    )
    .await;
    for (page, body) in [
        (child, json!({ "parentId": null })),
        (child, json!({ "parentId": other })),
        (private, json!({ "parentId": parent })),
        (other, json!({ "parentId": parent })),
    ] {
        let before = t.store.get_node(page).await.unwrap().unwrap().parent_id;
        let reply = json_by(&t, &token, Method::PATCH, &page_uri(page), body).await;
        reply.assert_error(StatusCode::FORBIDDEN, "forbidden");
        let after = t.store.get_node(page).await.unwrap().unwrap().parent_id;
        assert_eq!(before, after);
    }
    // Parmi ses sœurs, oui.
    json_by(
        &t,
        &token,
        Method::PATCH,
        &page_uri(other),
        json!({ "afterId": null }),
    )
    .await
    .assert_status(StatusCode::OK);
}

/// Une page créée ou dupliquée par un jeton est rendue au niveau du jeton, pas à l'accès complet
/// que son compte y a (Ruling R1216 : `own_access` restreint aussi).
#[tokio::test]
async fn own_access_is_narrowed_too() {
    let t = open_app().await;
    let olga = t.account("Olga").await;
    let (token, _) = api_token(&t.store, olga.id, TokenLevel::Edit, TokenScope::All, true).await;
    for section in ["private", "space"] {
        let created = json_by(
            &t,
            &token,
            Method::POST,
            "/api/v1/pages",
            json!({ "spaceId": olga.space, "title": "Racine", "section": section }),
        )
        .await;
        assert_eq!(
            created.assert_status(StatusCode::CREATED).json()["level"],
            "edit",
            "{section}"
        );
    }
    let page = olga
        .create(json!({ "spaceId": olga.space, "title": "P" }))
        .await;
    let sub = json_by(
        &t,
        &token,
        Method::POST,
        "/api/v1/pages",
        json!({ "parentId": page.id, "title": "Sous-page" }),
    )
    .await;
    assert_eq!(
        sub.assert_status(StatusCode::CREATED).json()["level"],
        "edit"
    );
    let copy = t
        .send(with_bearer(
            bare(
                Method::POST,
                &format!("/api/v1/pages/{}/duplicate", page.id),
            ),
            &token,
        ))
        .await;
    assert_eq!(
        copy.assert_status(StatusCode::CREATED).json()["level"],
        "edit"
    );
    // La session d'Olga, elle, a l'accès complet sur la même page.
    let seen = olga
        .browser
        .get(&format!("/api/v1/pages/{}", page.id))
        .await;
    assert_eq!(seen.json()["level"], "full");
}

/// La corbeille demande l'option du jeton (Ruling R1218) : sans elle, `403` et la page reste ;
/// avec elle, la page y va.
#[tokio::test]
async fn trash_needs_the_option() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let page = mona
        .create(json!({ "spaceId": mona.space, "title": "P" }))
        .await;
    let trash = format!("/api/v1/pages/{}/trash", page.id);
    let (without, _) = api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    let reply = t
        .send(with_bearer(bare(Method::POST, &trash), &without))
        .await;
    let body = reply.assert_error(StatusCode::FORBIDDEN, "forbidden");
    assert!(
        body["message"].as_str().unwrap().contains("corbeille"),
        "{body}"
    );
    let id = Uuid::parse_str(&page.id).unwrap();
    assert_eq!(
        t.store.get_node(id).await.unwrap().unwrap().trashed_at,
        None
    );
    let (with, _) = api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, true).await;
    t.send(with_bearer(bare(Method::POST, &trash), &with))
        .await
        .assert_status(StatusCode::OK);
    assert!(
        t.store
            .get_node(id)
            .await
            .unwrap()
            .unwrap()
            .trashed_at
            .is_some()
    );
    // En sortir demande la même option (Ruling R1345 récrit).
    let restore = format!("/api/v1/pages/{}/restore", page.id);
    let reply = t
        .send(with_bearer(bare(Method::POST, &restore), &without))
        .await;
    let body = reply.assert_error(StatusCode::FORBIDDEN, "forbidden");
    assert!(
        body["message"].as_str().unwrap().contains("corbeille"),
        "{body}"
    );
    assert!(
        t.store
            .get_node(id)
            .await
            .unwrap()
            .unwrap()
            .trashed_at
            .is_some()
    );
    t.send(with_bearer(bare(Method::POST, &restore), &with))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        t.store.get_node(id).await.unwrap().unwrap().trashed_at,
        None
    );
}

/// Un jeton suit son compte : passée invitée, Mona ne voit plus les pages des membres, son jeton
/// non plus, dès l'appel suivant.
#[tokio::test]
async fn a_token_follows_its_account_when_a_right_is_withdrawn() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team_page(&team, None, "Notes").await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    get_by(&t, &token, &page_uri(page))
        .await
        .assert_status(StatusCode::OK);
    team.store
        .set_role(
            team.space,
            team.mona.id,
            Role::Guest,
            Expected {
                actor: team.olga.id,
                actor_role: Role::Owner,
                target_role: Role::Member,
            },
        )
        .await
        .unwrap();
    get_by(&t, &token, &page_uri(page))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
}

/// Créer une page à la racine par jeton demande le niveau « Modification » et le rôle de membre
/// (Ruling R1218) : un jeton de lecture, un invité, un jeton de portée « pages » (qui voit
/// l'espace comme un invité) reçoivent `403`.
#[tokio::test]
async fn a_token_creates_at_the_root_only_with_edit_and_membership() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team_page(&team, None, "Notes").await;
    let root = |token: String| {
        let t = &t;
        let space = team.space;
        async move {
            json_by(
                t,
                &token,
                Method::POST,
                "/api/v1/pages",
                json!({ "spaceId": space, "title": "Neuve" }),
            )
            .await
        }
    };
    let (edit, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    root(edit.clone()).await.assert_status(StatusCode::CREATED);
    let (read, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Read,
        TokenScope::All,
        false,
    )
    .await;
    root(read.clone())
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    let (guest, _) = api_token(
        &t.store,
        team.gina.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    root(guest.clone())
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    let (pages, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::Pages {
            space: team.space,
            pages: vec![page],
        },
        false,
    )
    .await;
    root(pages.clone())
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    // Sous une page de sa portée, il crée.
    json_by(
        &t,
        &pages,
        Method::POST,
        "/api/v1/pages",
        json!({ "parentId": page, "title": "Sous-page" }),
    )
    .await
    .assert_status(StatusCode::CREATED);
}

/// Dans « Privé » (le défaut) comme dans « Espace » s'il le demande (Ruling R1218).
#[tokio::test]
async fn a_token_creates_at_the_root_in_both_sections() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    for (section, subject) in [
        (None, Subject::User(team.mona.id)),
        (Some("private"), Subject::User(team.mona.id)),
        (Some("space"), Subject::Members),
    ] {
        let mut body = json!({ "spaceId": team.space, "title": "Neuve" });
        if let Some(section) = section {
            body["section"] = json!(section);
        }
        let created = json_by(&t, &token, Method::POST, "/api/v1/pages", body).await;
        let id = uuid(&created.assert_status(StatusCode::CREATED).json()["id"]);
        assert_eq!(
            team.grants(id).await,
            vec![(subject, Level::Full)],
            "{section:?}"
        );
    }
}

/// Le compte attendu (Ruling R540) vaut pour un jeton : un autre compte nommé, `409
/// account_changed` ; le sien, l'appel passe ; un nom illisible, `400`.
#[tokio::test]
async fn the_account_header_is_checked_for_a_token_too() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let olga = t.account("Olga").await;
    let page = mona
        .create(json!({ "spaceId": mona.space, "title": "P" }))
        .await;
    let (token, _) = api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    let named = |account: &str| {
        let mut request = with_bearer(
            bare(Method::GET, &format!("/api/v1/pages/{}", page.id)),
            &token,
        );
        request
            .headers_mut()
            .insert("x-mementum-account", account.parse().unwrap());
        request
    };
    t.send(named(&olga.id.to_string()))
        .await
        .assert_error(StatusCode::CONFLICT, "account_changed");
    t.send(named(&mona.id.to_string()))
        .await
        .assert_status(StatusCode::OK);
    t.send(named("pas-un-compte"))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_request");
}

/// Les refus d'un appel par jeton viennent dans l'ordre : `403 bad_origin`, `401 invalid_token`,
/// `403 token_not_allowed`, `429 token_busy`, `409 account_changed`, puis le `404` d'une page.
#[tokio::test]
async fn refusals_come_in_order() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.token_calls_per_min = 1;
    })
    .await;
    let mona = t.account("Mona").await;
    let olga = t.account("Olga").await;
    let (token, _) = api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    let missing = page_uri(Uuid::now_v7());
    let foreign = |request: Request<Body>| {
        let mut request = request;
        request
            .headers_mut()
            .insert(header::ORIGIN, "https://ailleurs.example".parse().unwrap());
        request
    };
    // Une Origin d'un autre site passe avant tout, jeton faux compris, écriture ou lecture.
    for method in [Method::GET, Method::POST] {
        let request = with_bearer(bare(method.clone(), &missing), "mmt_faux");
        t.send(foreign(request))
            .await
            .assert_error(StatusCode::FORBIDDEN, "bad_origin");
        // Un `Authorization` qui ne se lit pas comme un seul jeton : l'`Origin` d'abord aussi.
        let mut request = with_bearer(bare(method, &missing), &token);
        request
            .headers_mut()
            .append(header::AUTHORIZATION, "Basic YTpi".parse().unwrap());
        t.send(foreign(request))
            .await
            .assert_error(StatusCode::FORBIDDEN, "bad_origin");
    }
    // Sans Origin étrangère, cet en-tête illisible est un 401.
    let mut request = with_bearer(bare(Method::GET, &missing), &token);
    request
        .headers_mut()
        .append(header::AUTHORIZATION, "Basic YTpi".parse().unwrap());
    t.send(request)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_token");
    // Un jeton faux sur une route fermée : 401, pas 403.
    let closed = format!("{missing}/access");
    get_by(&t, "mmt_faux", &closed)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_token");
    // Le compte attendu passe avant la page : 409, pas 404 (ce premier appel prend le débit).
    let mut request = with_bearer(bare(Method::GET, &missing), &token);
    request
        .headers_mut()
        .insert("x-mementum-account", olga.id.to_string().parse().unwrap());
    t.send(request)
        .await
        .assert_error(StatusCode::CONFLICT, "account_changed");
    // Le débit pris : une route fermée dit encore 403, une route ouverte 429, compte faux compris.
    get_by(&t, &token, &closed)
        .await
        .assert_error(StatusCode::FORBIDDEN, "token_not_allowed");
    let mut request = with_bearer(bare(Method::GET, &missing), &token);
    request
        .headers_mut()
        .insert("x-mementum-account", olga.id.to_string().parse().unwrap());
    let reply = t.send(request).await;
    reply.assert_error(StatusCode::TOO_MANY_REQUESTS, "token_busy");
}

// ---------------------------------------------------------------------------
// Le débit, et le journal.

/// Au-delà de son débit, un jeton reçoit `429 token_busy` avec `Retry-After`, avant tout travail
/// de la route ; le refus n'écrit qu'un compte sur le jeton, les appels admis une ligne chacun.
#[tokio::test]
async fn a_token_over_its_rate_is_refused_and_only_counted() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.token_calls_per_min = 2;
    })
    .await;
    let mona = t.account("Mona").await;
    let page = mona
        .create(json!({ "spaceId": mona.space, "title": "P" }))
        .await;
    let page_id = Uuid::parse_str(&page.id).unwrap();
    let (token, token_id) =
        api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    let uri = format!("/api/v1/pages/{}", page.id);
    get_by(&t, &token, &uri).await.assert_status(StatusCode::OK);
    json_by(
        &t,
        &token,
        Method::PATCH,
        &uri,
        json!({ "title": "Renommée" }),
    )
    .await
    .assert_status(StatusCode::OK);
    let reply = json_by(
        &t,
        &token,
        Method::PATCH,
        &uri,
        json!({ "title": "Encore" }),
    )
    .await;
    reply.assert_error(StatusCode::TOO_MANY_REQUESTS, "token_busy");
    let wait: u64 = reply.headers[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=60).contains(&wait), "{wait}");
    // Refusé avant tout travail : le titre est celui du deuxième appel.
    assert_eq!(
        t.store.get_node(page_id).await.unwrap().unwrap().title,
        "Renommée"
    );
    t.calls.flush(&t.store).await;
    let calls = t
        .store
        .calls_of(mona.id, token_id, None, None, 50, now())
        .await
        .unwrap();
    let lines: Vec<(&str, Option<Uuid>, &str)> = calls
        .iter()
        .map(|row| {
            (
                row.call.tool.as_str(),
                row.call.node_id,
                row.call.outcome.as_str(),
            )
        })
        .collect();
    assert_eq!(
        lines,
        vec![
            ("PATCH /api/v1/pages/{id}", Some(page_id), "ok"),
            ("GET /api/v1/pages/{id}", Some(page_id), "ok"),
        ]
    );
    let row = &t.store.tokens_of(mona.id, now()).await.unwrap()[0];
    assert_eq!(row.busy_count, 1);
}

/// Les jetons d'un compte partagent son débit : le compte pris par deux jetons, un troisième jeton
/// qui n'a rien fait est refusé ; un jeton d'un autre compte passe.
#[tokio::test]
async fn the_tokens_of_one_account_share_its_rate() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.account_token_calls_per_min = 3;
    })
    .await;
    let mona = t.account("Mona").await;
    let olga = t.account("Olga").await;
    let tree = format!("/api/v1/spaces/{}/tree", mona.space);
    let mut tokens = Vec::new();
    for _ in 0..3 {
        tokens.push(
            api_token(&t.store, mona.id, TokenLevel::Read, TokenScope::All, false)
                .await
                .0,
        );
    }
    for token in [&tokens[0], &tokens[1], &tokens[0]] {
        get_by(&t, token, &tree).await.assert_status(StatusCode::OK);
    }
    get_by(&t, &tokens[2], &tree)
        .await
        .assert_error(StatusCode::TOO_MANY_REQUESTS, "token_busy");
    let (olgas, _) = api_token(&t.store, olga.id, TokenLevel::Read, TokenScope::All, false).await;
    get_by(&t, &olgas, &format!("/api/v1/spaces/{}/tree", olga.space))
        .await
        .assert_status(StatusCode::OK);
}

/// Le texte d'un jeton n'entre dans aucun journal du serveur, niveau `TRACE` compris : ni un
/// jeton admis, ni un jeton refusé, ni un jeton révoqué.
#[tokio::test]
async fn the_bearer_is_never_logged() {
    let logs = all_logs();
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let page = mona
        .create(json!({ "spaceId": mona.space, "title": "P" }))
        .await;
    let (token, id) = api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    let uri = format!("/api/v1/pages/{}", page.id);
    get_by(&t, &token, &uri).await.assert_status(StatusCode::OK);
    get_by(&t, &token, &format!("{uri}/access")).await;
    t.store.revoke_token(mona.id, id, now()).await.unwrap();
    get_by(&t, &token, &uri).await;
    t.calls.flush(&t.store).await;
    let secret = token.trim_start_matches("mmt_");
    let hash = mementum_server::auth::token_hash(secret).unwrap();
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    let text = logs.text();
    assert!(!text.is_empty(), "le journal se lit");
    assert!(!text.contains(secret), "le jeton est dans le journal");
    assert!(
        !text.to_lowercase().contains(&hex),
        "le hachage du jeton est dans le journal"
    );
    // Ni sous la forme où SQLite écrit un blob lié (`x'…'`, en majuscules).
    assert!(
        !text.contains(&hex.to_uppercase()),
        "le hachage du jeton est dans le journal"
    );
}

/// Aucune route ne lit par où l'appel est venu pour juger un droit (Ruling R1216) : `via`, et le
/// type `Via`, ne paraissent que dans `auth.rs`, `access.rs` et `agent/`.
#[test]
fn only_access_and_agent_read_how_a_call_came() {
    fn sources(dir: &std::path::Path, into: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, into);
            } else if path.extension().is_some_and(|e| e == "rs") {
                into.push(path);
            }
        }
    }
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&src, &mut files);
    assert!(files.len() > 20, "les sources se lisent");
    let allowed = |path: &std::path::Path| {
        let relative = path.strip_prefix(&src).unwrap();
        relative.starts_with("agent")
            || relative == std::path::Path::new("auth.rs")
            || relative == std::path::Path::new("access.rs")
    };
    // `.token()` dit si l'appel vient d'un jeton, sans rien dire de ses droits : il ne sert hors
    // d'`auth.rs`, d'`access.rs` et d'`agent/` qu'à refuser un jeton, à ces places-là seulement.
    let token_places = ["routes/sync.rs"];
    let mut token_seen = Vec::new();
    for file in files.iter().filter(|f| !allowed(f)) {
        let text = std::fs::read_to_string(file).unwrap();
        let relative = file
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap();
            let reads = code.contains(".via") || code.contains("via:") || code.contains("Via");
            assert!(!reads, "{}:{} lit `via` : {line}", file.display(), n + 1);
            if code.contains(".token()") {
                token_seen.push(relative.clone());
            }
        }
    }
    token_seen.dedup();
    assert_eq!(
        token_seen, token_places,
        "`.token()` hors de sa liste fermée"
    );
}

/// Une page que le compte voit par une ligne posée sous une racine de la portée qu'il ne voit plus
/// est hors de la portée du jeton (Ruling R1574) : la route rend la réponse d'une page inconnue,
/// alors que la session la lit ; la racine rendue au compte, elle revient.
#[tokio::test]
async fn a_page_seen_under_a_root_the_account_no_longer_sees_is_out_of_scope() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let root = team
        .page(
            None,
            "Racine",
            &[(Subject::User(team.mona.id), Level::Read)],
        )
        .await;
    let between = team.page(Some(root), "Entre", &[]).await;
    let page = team
        .page(
            Some(between),
            "Page",
            &[(Subject::User(team.mona.id), Level::Edit)],
        )
        .await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::Pages {
            space: team.space,
            pages: vec![root],
        },
        false,
    )
    .await;
    get_by(&t, &token, &page_uri(page))
        .await
        .assert_status(StatusCode::OK);
    let line = format!("user:{}", team.mona.id);
    let bump = format!(
        "UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = '{}'",
        team.space
    );
    exec_sql(
        &t.db_path(),
        &format!("DELETE FROM acl WHERE node_id = '{root}' AND subject = '{line}'"),
    )
    .await;
    exec_sql(&t.db_path(), &bump).await;
    team.mona
        .browser
        .get(&page_uri(page))
        .await
        .assert_status(StatusCode::OK);
    let hidden = get_by(&t, &token, &page_uri(page)).await;
    let unknown = get_by(&t, &token, &page_uri(Uuid::now_v7())).await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    assert_eq!(refusal(&hidden), refusal(&unknown));
    exec_sql(
        &t.db_path(),
        &format!("INSERT INTO acl (node_id, subject, level) VALUES ('{root}', '{line}', 'read')"),
    )
    .await;
    exec_sql(&t.db_path(), &bump).await;
    get_by(&t, &token, &page_uri(page))
        .await
        .assert_status(StatusCode::OK);
}

/// Les routes de tout l'espace et les rétroliens d'un jeton de portée « pages » : l'arbre ne porte
/// que la racine de la portée et ses sous-pages, la racine sans parent ; la recherche ne trouve
/// qu'elles, et leurs fils d'Ariane s'arrêtent à la racine ; les rétroliens d'une page ne nomment
/// que des sources de la portée.
#[tokio::test]
async fn a_scope_of_pages_sees_only_its_subtree_in_the_tree_the_search_and_the_backlinks() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.index_after = std::time::Duration::from_millis(50);
    })
    .await;
    let team = Team::new(&t).await;
    let above = team_page(&team, None, "Dessus commun").await;
    let scoped = team_page(&team, Some(above), "Portée commun").await;
    let inside = team_page(&team, Some(scoped), "Dedans commun").await;
    let aside = team_page(&team, None, "Côté commun").await;
    // Une source dans la portée, une hors d'elle, toutes deux vers la racine de la portée.
    for source in [inside, aside] {
        team.olga
            .put_mmd(
                &source.to_string(),
                format!("Vers [la portée](/p/{scoped}).\n"),
            )
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
    t.indexer.settled().await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Read,
        TokenScope::Pages {
            space: team.space,
            pages: vec![scoped],
        },
        false,
    )
    .await;
    let tree = get_by(&t, &token, &format!("/api/v1/spaces/{}/tree", team.space)).await;
    let tree = tree.assert_status(StatusCode::OK).json();
    let mut nodes: Vec<(String, String)> = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            (
                n["id"].as_str().unwrap().to_string(),
                n["parentId"].to_string(),
            )
        })
        .collect();
    nodes.sort();
    let mut expected = vec![
        (scoped.to_string(), "null".to_string()),
        (inside.to_string(), format!("\"{scoped}\"")),
    ];
    expected.sort();
    assert_eq!(nodes, expected);
    let found = json_by(
        &t,
        &token,
        Method::POST,
        &format!("/api/v1/spaces/{}/search", team.space),
        json!({ "q": "commun" }),
    )
    .await;
    let found = found.assert_status(StatusCode::OK).json();
    let mut hits: Vec<(String, Vec<String>)> = found["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| {
            let path = hit["path"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a["id"].as_str().unwrap().to_string())
                .collect();
            (hit["pageId"].as_str().unwrap().to_string(), path)
        })
        .collect();
    hits.sort();
    let mut expected = vec![
        (scoped.to_string(), Vec::new()),
        (inside.to_string(), vec![scoped.to_string()]),
    ];
    expected.sort();
    assert_eq!(hits, expected);
    // Témoin : la session de Mona trouve les quatre pages.
    let all = team
        .mona
        .browser
        .post(
            &format!("/api/v1/spaces/{}/search", team.space),
            json!({ "q": "commun" }),
        )
        .await;
    assert_eq!(all.json()["results"].as_array().unwrap().len(), 4);
    let links = get_by(&t, &token, &format!("{}/backlinks", page_uri(scoped))).await;
    let links = links.assert_status(StatusCode::OK).json();
    let sources: Vec<&str> = links["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["pageId"].as_str().unwrap())
        .collect();
    assert_eq!(sources, vec![inside.to_string().as_str()]);
    let witness = team
        .mona
        .browser
        .get(&format!("{}/backlinks", page_uri(scoped)))
        .await;
    assert_eq!(witness.json()["pages"].as_array().unwrap().len(), 2);
}

/// Une écriture par jeton dont le client coupe pendant qu'elle attend son tour va au bout (elle est
/// menée par `to_completion`), et laisse une ligne au journal d'activité, `error` et
/// `client_gone` (Ruling R1355) : la scène de la sonde de la relecture.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_token_write_whose_client_left_leaves_a_line() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let page = mona
        .create(json!({ "spaceId": mona.space, "title": "Avant" }))
        .await;
    let id = Uuid::parse_str(&page.id).unwrap();
    let (token, token_id) =
        api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    t.indexer.settled().await;
    t.indexer.freeze();
    let held = t.store.hold_writes().await;
    let mut request = with_bearer(
        json_request(Method::PATCH, &page_uri(id), &json!({ "title": "Après" })),
        &token,
    );
    request
        .headers_mut()
        .insert(header::HOST, "localhost".parse().unwrap());
    let router = t.router.clone();
    let asked = tokio::spawn(async move {
        use tower::ServiceExt;
        router.oneshot(request).await
    });
    until("le renommage attend son tour", || {
        t.store.queued_writes() >= 1
    })
    .await;
    asked.abort();
    assert!(
        asked.await.unwrap_err().is_cancelled(),
        "le client est parti"
    );
    drop(held);
    let deadline = std::time::Instant::now() + WAIT;
    while t.store.get_node(id).await.unwrap().unwrap().title != "Après" {
        assert!(
            std::time::Instant::now() < deadline,
            "le renommage n'est pas écrit"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    t.calls.flush(&t.store).await;
    let calls = t
        .store
        .calls_of(mona.id, token_id, None, None, 50, now())
        .await
        .unwrap();
    let lines: Vec<(&str, &str, Option<&str>)> = calls
        .iter()
        .map(|row| {
            let call = &row.call;
            (
                call.tool.as_str(),
                call.outcome.as_str(),
                call.code.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        lines,
        vec![("PATCH /api/v1/pages/{id}", "error", Some("client_gone"))]
    );
}

/// Attend que `ready` soit vrai, `WAIT` au plus.
async fn until(what: &str, ready: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + WAIT;
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "{what} : attente dépassée"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[cfg(windows)]
mod known_limits {
    use super::limit::known_limit;
    use super::*;

    /// Voulu : un client d'agent qui envoie tout le corps d'un `PUT …/mmd` sans attendre la réponse
    /// reçoit le `403 token_not_allowed` de la porte. Aujourd'hui (Ruling R505, plan de S5,
    /// « Pièges ») : la porte refuse avant de lire le corps, le serveur ferme en laissant des
    /// octets non lus, le système coupe par `RST`, et la réponse est perdue pour ce client. Aucun
    /// outil n'appelle cette route ; la lever demanderait de lire et jeter le corps d'un jeton
    /// refusé, ce que la porte ne fait pas pour ne rien lire d'une route fermée.
    ///
    /// Sous Windows seulement : sous Linux (CI du 2026-11-21, `e0c17a6`), le noyau garde assez
    /// d'octets reçus pour que la réponse arrive avant le `RST`, et la perte ne se produit pas
    /// avec ce client ; la limite y reste possible avec un corps plus gros ou un réseau lent.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_token_refused_before_the_body_of_put_mmd_may_lose_its_answer() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let t = open_app().await;
        let mona = t.account("Mona").await;
        let page = mona
            .create(json!({ "spaceId": mona.space, "title": "P" }))
            .await;
        let (token, _) =
            api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
        let addr = t.listen().await;
        let length = 8usize << 20;
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let head = format!(
            "PUT /api/v1/pages/{}/mmd HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer \
             {token}\r\nContent-Type: text/markdown\r\nContent-Length: {length}\r\n\r\n",
            page.id
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        let pushing = async {
            let block = vec![b'a'; 64 * 1024];
            let mut sent = 0;
            while sent < length {
                let size = block.len().min(length - sent);
                if stream.write_all(&block[..size]).await.is_err() {
                    return;
                }
                sent += size;
            }
        };
        let _ = tokio::time::timeout(WAIT, pushing).await;
        let mut received = Vec::new();
        let mut buffer = [0u8; 8192];
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            match tokio::time::timeout_at(deadline, stream.read(&mut buffer)).await {
                Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                Ok(Ok(n)) => received.extend_from_slice(&buffer[..n]),
            }
        }
        let text = String::from_utf8_lossy(&received);
        let mut unmet = Vec::new();
        if !(text.starts_with("HTTP/1.1 403") && text.contains("token_not_allowed")) {
            unmet.push(
                "le 403 token_not_allowed arrive au client qui envoie tout son corps".to_string(),
            );
        }
        known_limit(
            "refus rendu avant de lire le corps, perdu par RST (R505)",
            &unmet,
            &["le 403 token_not_allowed arrive au client qui envoie tout son corps".to_string()],
        );
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
