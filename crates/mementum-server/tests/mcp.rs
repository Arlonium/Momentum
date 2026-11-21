//! Le serveur MCP du binaire (lot 5, Task M2) : `POST /mcp` et `POST /mcp/readonly`, les six
//! outils de `mementum-mcp` branchés sur les fonctions d'`agent/` (Rulings R1236, R1237, R1240).
//!
//! Un outil n'a aucun droit à lui : chacun mène la fonction que mène la route REST de la même
//! opération, au nom du compte du jeton, restreint. Les requêtes passent par le client de test de
//! `common/mcp.rs`, dans l'une ou l'autre révision du protocole.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::mcp::{ERAS, Era, Mcp, ToolResult};
use common::*;
use mementum_acl::{Level, Subject};
use mementum_server::SignupMode;
use mementum_store::{CallOutcome, CallSurface, TokenLevel, TokenScope};
use serde_json::{Value, json};
use uuid::Uuid;

async fn open_app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn bare(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

/// Un mot de lettres seules, propre au test : un mot à chercher (lot 4, piège de C1).
fn word() -> String {
    Uuid::now_v7()
        .simple()
        .to_string()
        .chars()
        .rev()
        .take(12)
        .map(|c| match c.to_digit(16) {
            Some(n) => char::from(b'a' + n as u8),
            None => c,
        })
        .collect()
}

/// Une page de l'espace d'équipe, ouverte aux membres en écriture, avec ce contenu (écrit par
/// Olga).
async fn page_with(team: &Team<'_>, title: &str, mmd: &str) -> Uuid {
    let page = team
        .page(None, title, &[(Subject::Members, Level::Edit)])
        .await;
    team.olga
        .put_mmd(&page.to_string(), mmd.to_string())
        .await
        .assert_status(StatusCode::NO_CONTENT);
    page
}

async fn mmd_of(team: &Team<'_>, page: Uuid) -> String {
    team.olga.get_mmd(&page.to_string()).await.text()
}

/// Ce que la base garde de l'espace d'équipe : chaque page (titre, parent, corbeille), triée, et
/// le MMD de `page`.
async fn base_of(t: &TestApp, team: &Team<'_>, page: Uuid) -> (Vec<String>, String) {
    let mut nodes: Vec<String> = t
        .store
        .list_nodes(team.space)
        .await
        .unwrap()
        .into_iter()
        .map(|n| format!("{} {:?} {:?} {}", n.id, n.parent_id, n.trashed_at, n.title))
        .collect();
    nodes.sort();
    (nodes, mmd_of(team, page).await)
}

/// L'outil et la route ont la même issue : réussis tous deux, ou refusés sous le même code.
#[track_caller]
fn same_issue(what: &str, tool: &ToolResult, route: &Reply) {
    if route.status.is_success() {
        assert!(
            !tool.is_error,
            "{what} : la route passe ({}), l'outil refuse : {}",
            route.status, tool.text
        );
    } else {
        let code = route.json()["error"].as_str().unwrap().to_string();
        assert!(
            tool.is_error,
            "{what} : la route refuse ({code}), l'outil passe : {}",
            tool.text
        );
        assert_eq!(tool.code(), Some(code.as_str()), "{what} : {}", tool.text);
    }
}

/// Sans jeton, `/mcp` répond `401 invalid_token`, avec `WWW-Authenticate: Bearer`, et ne regarde
/// pas le cookie : celui d'une session vivante reçoit exactement la même réponse, dans les deux
/// révisions ; un autre `Authorization` aussi (Ruling R1214). Un jeton accompagné du cookie d'un
/// autre compte agit pour le compte du jeton.
#[tokio::test]
async fn mcp_needs_a_token_and_never_takes_a_cookie() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let cookie = team.olga.browser.cookie().unwrap();
    for era in ERAS {
        let bare_client = Mcp::without_token(&t, era);
        let call = json!({ "name": "workspace_info", "arguments": {} });
        let without = bare_client.send("tools/call", call.clone()).await;
        let body = without.assert_error(StatusCode::UNAUTHORIZED, "invalid_token");
        assert_eq!(
            without.headers[header::WWW_AUTHENTICATE],
            "Bearer",
            "{era:?}"
        );
        assert!(
            body["message"].as_str().unwrap().contains("jeton"),
            "{body}"
        );
        for method in ["tools/call", "tools/list", "initialize"] {
            let mut request = bare_client.request(method, call.clone());
            request
                .headers_mut()
                .insert(header::COOKIE, cookie.parse().unwrap());
            let with_cookie = t.send(request).await;
            assert_eq!(
                with_cookie.status,
                StatusCode::UNAUTHORIZED,
                "{era:?} {method}"
            );
            assert_eq!(with_cookie.body, without.body, "{era:?} {method}");
        }
        for other in [
            "Basic b2xnYTpzZWNyZXQ=",
            "Bearer abc",
            "Bearer",
            "Token mmt_x",
        ] {
            let mut request = bare_client.request("tools/call", call.clone());
            request
                .headers_mut()
                .insert(header::AUTHORIZATION, other.parse().unwrap());
            request
                .headers_mut()
                .insert(header::COOKIE, cookie.parse().unwrap());
            t.send(request)
                .await
                .assert_error(StatusCode::UNAUTHORIZED, "invalid_token");
        }
    }
    // Le jeton de Mona, avec le cookie d'Olga : l'appel est celui de Mona.
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Read,
        TokenScope::All,
        false,
    )
    .await;
    let mcp = Mcp::new(&t, &token, Era::V2026);
    let mut request = mcp.request(
        "tools/call",
        json!({ "name": "workspace_info", "arguments": {} }),
    );
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse().unwrap());
    let reply = t.send(request).await;
    reply.assert_status(StatusCode::OK);
    let text = reply.json()["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("compte : Mona"), "{text}");
    assert!(!text.contains("Olga"), "{text}");
}

/// Une `Origin` d'un autre site est refusée avant tout (Ruling R1237) : `403 bad_origin`, que la
/// requête porte un jeton ou non, et l'outil n'est jamais appelé (aucune ligne d'activité).
#[tokio::test]
async fn a_foreign_origin_is_refused() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Notes", "Un\n").await;
    let (token, token_id) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    for era in ERAS {
        for client in [
            Mcp::new(&t, &token, era),
            Mcp::new(&t, &token, era).readonly(),
            Mcp::without_token(&t, era),
        ] {
            let mut request = client.request(
                "tools/call",
                json!({
                    "name": "update_page",
                    "arguments": {
                        "page": page.to_string(),
                        "operations": [{ "op": "insert", "position": "end", "markdown": "Volé\n" }],
                    },
                }),
            );
            request
                .headers_mut()
                .insert(header::ORIGIN, "https://ailleurs.example".parse().unwrap());
            t.send(request)
                .await
                .assert_error(StatusCode::FORBIDDEN, "bad_origin");
        }
    }
    // Une page de la boucle locale passe (l'application de développement).
    let mcp = Mcp::new(&t, &token, Era::V2026);
    let mut request = mcp.request("tools/list", json!({}));
    request
        .headers_mut()
        .insert(header::ORIGIN, "http://localhost:5173".parse().unwrap());
    t.send(request).await.assert_status(StatusCode::OK);
    assert_eq!(mmd_of(&team, page).await, "Un\n");
    t.calls.flush(&t.store).await;
    let lines = t
        .store
        .calls_of(team.mona.id, token_id, None, None, 50, now())
        .await
        .unwrap();
    assert_eq!(lines.len(), 1, "seul tools/list a été admis : {lines:?}");
}

/// `GET` et `DELETE` répondent `405`, sur les deux adresses, jeton ou non (Ruling R1236 : ni
/// flux, ni session).
#[tokio::test]
async fn get_and_delete_are_405() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Read,
        TokenScope::All,
        false,
    )
    .await;
    for path in ["/mcp", "/mcp/readonly"] {
        for method in [Method::GET, Method::DELETE] {
            for request in [
                bare(method.clone(), path),
                with_bearer(bare(method.clone(), path), &token),
            ] {
                let reply = t.send(request).await;
                reply.assert_error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
                assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store", "{path}");
            }
        }
    }
}

/// Toute réponse de `/mcp` porte `Cache-Control: no-store` et `nosniff`, et aucun en-tête CORS,
/// réussite, erreur JSON-RPC, refus de l'outil, `202` ou `401` (Ruling R1237).
#[tokio::test]
async fn mcp_answers_carry_no_store_and_no_cors() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Read,
        TokenScope::All,
        false,
    )
    .await;
    let mcp = Mcp::new(&t, &token, Era::V2026);
    let legacy = Mcp::new(&t, &token, Era::V2025);
    let with_origin = |mut request: Request<Body>| {
        request
            .headers_mut()
            .insert(header::ORIGIN, "http://localhost:5173".parse().unwrap());
        request
    };
    let mut replies = vec![
        t.send(with_origin(mcp.request("tools/list", json!({}))))
            .await,
        t.send(with_origin(mcp.request("ping", json!({})))).await,
        t.send(with_origin(mcp.request(
            "tools/call",
            json!({ "name": "fetch", "arguments": { "id": Uuid::now_v7().to_string() } }),
        )))
        .await,
        t.send(with_origin(
            Mcp::without_token(&t, Era::V2026).request("tools/list", json!({})),
        ))
        .await,
        t.send(with_origin(legacy.request(
            "initialize",
            json!({ "protocolVersion": "2025-11-25", "capabilities": {} }),
        )))
        .await,
    ];
    let notification = Request::post("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header("MCP-Protocol-Version", "2025-11-25")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        ))
        .unwrap();
    replies.push(t.send(with_bearer(with_origin(notification), &token)).await);
    let mut preflight = bare(Method::OPTIONS, "/mcp");
    preflight
        .headers_mut()
        .insert(header::ORIGIN, "https://ailleurs.example".parse().unwrap());
    preflight.headers_mut().insert(
        header::ACCESS_CONTROL_REQUEST_METHOD,
        "POST".parse().unwrap(),
    );
    replies.push(t.send(preflight).await);
    let statuses: Vec<u16> = replies.iter().map(|r| r.status.as_u16()).collect();
    assert_eq!(statuses, [200, 404, 200, 401, 200, 202, 405]);
    for reply in &replies {
        assert_eq!(
            reply.headers[header::CACHE_CONTROL],
            "no-store",
            "{}",
            reply.status
        );
        assert_eq!(
            reply.headers[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff",
            "{}",
            reply.status
        );
        for (name, _) in &reply.headers {
            assert!(
                !name.as_str().starts_with("access-control-"),
                "{name} sur un {}",
                reply.status
            );
        }
    }
    assert_eq!(replies[0].content_type.as_deref(), Some("application/json"));
    assert!(replies[5].body.is_empty(), "un 202 sans corps");
}

/// La matrice des outils (Ruling R1237 : aucun outil n'a de droit à lui) : pour chaque outil et
/// chaque niveau de jeton, l'outil a l'issue de la route REST de la même opération, menée par le
/// même jeton ; un jeton de lecture n'a rien écrit (la base relue), un jeton d'écriture a écrit
/// par l'outil ce qu'il a écrit par la route.
#[tokio::test]
async fn each_tool_does_what_its_route_does() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let marker = word();
    let page = page_with(&team, "Notes", &format!("Un {marker}.\n\nDeux.\n")).await;
    t.indexer.settled().await;
    for level in [TokenLevel::Read, TokenLevel::Edit] {
        let (token, _) = api_token(&t.store, team.mona.id, level, TokenScope::All, false).await;
        let mcp = Mcp::new(&t, &token, Era::V2026);
        let rest = |request: Request<Body>| t.send(with_bearer(request, &token));
        let before = base_of(&t, &team, page).await;
        let tag = format!("{level:?}");

        let tool = mcp
            .call("search", json!({ "query": marker, "space": "Équipe" }))
            .await;
        let route = rest(json_request(
            Method::POST,
            &format!("/api/v1/spaces/{}/search", team.space),
            &json!({ "q": marker }),
        ))
        .await;
        same_issue("search", &tool, &route);
        let found = route.json();
        assert_eq!(found["results"][0]["pageId"], page.to_string());
        assert_eq!(
            ToolResult::uuids(tool.content().unwrap()).first(),
            Some(&page),
            "{}",
            tool.text
        );
        // Le bloc trouvé est dit par un alias (écrit par le serveur), jamais par son identifiant,
        // qu'un client écrit (Ruling R1239).
        let block = found["results"][0]["blockId"].as_str().unwrap();
        assert!(!tool.text.contains(block), "{}", tool.text);
        let alias = tool
            .content()
            .unwrap()
            .split(", bloc ")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .unwrap_or_else(|| panic!("aucun bloc : {}", tool.text));
        assert!(
            alias.len() == 13
                && alias.starts_with('b')
                && alias[1..]
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || (b'2'..=b'7').contains(&c)),
            "{alias}"
        );

        let tool = mcp.call("fetch", json!({ "id": page.to_string() })).await;
        let route = rest(bare(Method::GET, &format!("/api/v1/pages/{page}/document"))).await;
        same_issue("fetch", &tool, &route);
        let document = route.json();
        assert_eq!(tool.revision(), document["revision"].as_str().unwrap());
        assert!(
            tool.content()
                .unwrap()
                .contains(document["mmd"].as_str().unwrap()),
            "{}",
            tool.text
        );

        let tool = mcp.call("workspace_info", json!({})).await;
        let route = rest(bare(Method::GET, "/api/v1/me")).await;
        same_issue("workspace_info", &tool, &route);
        for space in route.json()["spaces"].as_array().unwrap() {
            assert!(
                tool.content()
                    .unwrap()
                    .contains(space["id"].as_str().unwrap()),
                "{}",
                tool.text
            );
        }

        let tool = mcp
            .call(
                "create_pages",
                json!({ "space": team.space.to_string(), "pages": [{ "title": format!("Par l'outil {tag}") }] }),
            )
            .await;
        let route = rest(json_request(
            Method::POST,
            &format!("/api/v1/spaces/{}/pages/batch", team.space),
            &json!({ "pages": [{ "title": format!("Par la route {tag}") }] }),
        ))
        .await;
        same_issue("create_pages", &tool, &route);

        let insert = |text: &str| json!([{ "op": "insert", "position": "end", "markdown": format!("{text}\n") }]);
        let tool = mcp
            .call(
                "update_page",
                json!({ "page": page.to_string(), "operations": insert("Par l'outil") }),
            )
            .await;
        let route = rest(json_request(
            Method::POST,
            &format!("/api/v1/pages/{page}/operations"),
            &json!({ "operations": insert("Par la route") }),
        ))
        .await;
        same_issue("update_page", &tool, &route);

        let tool = mcp
            .call(
                "manage_pages",
                json!({ "action": "rename", "page": page.to_string(), "title": format!("Outil {tag}") }),
            )
            .await;
        let route = rest(json_request(
            Method::PATCH,
            &format!("/api/v1/pages/{page}"),
            &json!({ "title": format!("Route {tag}") }),
        ))
        .await;
        same_issue("manage_pages rename", &tool, &route);

        // Sans l'option corbeille, ni l'un ni l'autre ne jette la page.
        let tool = mcp
            .call(
                "manage_pages",
                json!({ "action": "trash", "page": page.to_string() }),
            )
            .await;
        let route = rest(bare(Method::POST, &format!("/api/v1/pages/{page}/trash"))).await;
        same_issue("manage_pages trash", &tool, &route);
        tool.refused("forbidden");

        let after = base_of(&t, &team, page).await;
        match level {
            TokenLevel::Read => {
                tool.refused("forbidden");
                assert_eq!(after, before, "un jeton de lecture n'écrit rien");
            }
            TokenLevel::Edit => {
                let titles = after.0.join("\n");
                for title in ["Par l'outil Edit", "Par la route Edit", "Route Edit"] {
                    assert!(titles.contains(title), "{title} : {titles}");
                }
                assert!(
                    after.1.ends_with("Par l'outil\n\nPar la route\n"),
                    "{}",
                    after.1
                );
            }
        }
    }
}

/// Un refus d'outil est un résultat `isError`, pas une erreur JSON-RPC (Ruling R1240) : il dit son
/// code, le message du serveur, l'opération en cause, ce qu'il faut faire et un exemple ; le texte
/// de la page n'y paraît que dans les blocs proposés, entre les lignes de borne (Ruling R1239).
#[tokio::test]
async fn a_tool_refusal_is_an_error_result_that_says_what_to_do() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(
        &team,
        "Notes",
        "Le chat dort au soleil.\n\nLe chien aboie.\n",
    )
    .await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    for era in ERAS {
        let mcp = Mcp::new(&t, &token, era);
        mcp.open().await;
        let refused = mcp
            .call(
                "update_page",
                json!({
                    "page": page.to_string(),
                    "operations": [
                        { "op": "insert", "position": "end", "markdown": "Fin.\n" },
                        { "op": "str_replace", "old": "Le chat noir", "new": "Le chat blanc" },
                    ],
                }),
            )
            .await;
        refused.refused("operation_failed");
        let head = refused.head();
        assert!(head.contains("Refus à l'opération 2 (no_match)"), "{head}");
        assert!(head.contains("Que faire : "), "{head}");
        assert!(head.contains("fetch avec anchors"), "{head}");
        assert!(head.contains("Exemple : update_page {"), "{head}");
        assert!(
            !head.contains("soleil"),
            "le texte de la page hors des bornes : {head}"
        );
        let content = refused.content().expect("les blocs proposés");
        assert!(content.contains("blocs proches"), "{content}");
        assert!(content.contains("Le chat dort au soleil."), "{content}");
        // Arguments refusés : le champ en cause, et un exemple de l'outil.
        let refused = mcp
            .call(
                "update_page",
                json!({ "page": page.to_string(), "operations": "tout" }),
            )
            .await;
        refused.refused("invalid_arguments");
        assert!(refused.head().contains("operations"), "{}", refused.text);
        assert!(
            refused.head().contains("Exemple : update_page {"),
            "{}",
            refused.text
        );
        // Une page qu'on ne voit pas : la réponse d'une page inconnue, et quoi faire.
        let refused = mcp
            .call("fetch", json!({ "id": Uuid::now_v7().to_string() }))
            .await;
        refused.refused("not_found");
        assert!(refused.head().contains("Que faire : "), "{}", refused.text);
    }
    assert_eq!(
        mmd_of(&team, page).await,
        "Le chat dort au soleil.\n\nLe chien aboie.\n",
        "un appel refusé n'écrit rien"
    );
}

/// Le débit d'un jeton (Ruling R1241) refuse l'appel d'outil suivant par un résultat
/// `token_busy` qui dit son délai (`retry_after_seconds`, R1240), sans rien faire ; un refus du
/// débit ne laisse qu'un compte, aucune ligne (Ruling R1244).
#[tokio::test]
async fn a_rate_limit_carries_retry_after_seconds() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.token_calls_per_min = 2;
    })
    .await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Notes", "Un\n").await;
    for era in ERAS {
        let (token, token_id) = api_token(
            &t.store,
            team.mona.id,
            TokenLevel::Edit,
            TokenScope::All,
            false,
        )
        .await;
        let mcp = Mcp::new(&t, &token, era);
        mcp.call("workspace_info", json!({})).await.ok();
        mcp.call("fetch", json!({ "id": page.to_string() }))
            .await
            .ok();
        let busy = mcp
            .call(
                "update_page",
                json!({
                    "page": page.to_string(),
                    "operations": [{ "op": "insert", "position": "end", "markdown": "Trop\n" }],
                }),
            )
            .await;
        busy.refused("token_busy");
        let at = busy
            .text
            .find("retry_after_seconds: ")
            .unwrap_or_else(|| panic!("{}", busy.text))
            + "retry_after_seconds: ".len();
        let seconds: u64 = busy.text[at..]
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!((1..=60).contains(&seconds), "{}", busy.text);
        assert!(
            busy.text.contains(&format!("Réessaie dans {seconds} s")),
            "{}",
            busy.text
        );
        t.calls.flush(&t.store).await;
        let lines = t
            .store
            .calls_of(team.mona.id, token_id, None, None, 50, now())
            .await
            .unwrap();
        assert_eq!(lines.len(), 2, "{era:?} : {lines:?}");
        let row = t
            .store
            .tokens_of(team.mona.id, now())
            .await
            .unwrap()
            .into_iter()
            .find(|row| row.id == token_id)
            .unwrap();
        assert_eq!(row.busy_count, 1, "{era:?}");
    }
    assert_eq!(mmd_of(&team, page).await, "Un\n");
}

/// `/mcp/readonly` ne liste et n'appelle que `search`, `fetch` et `workspace_info`, même pour un
/// jeton d'écriture : un outil d'écriture y est un outil inconnu (`-32602`), jamais appelé, et la
/// base n'a pas changé (Ruling R1417).
#[tokio::test]
async fn the_readonly_endpoint_refuses_a_write_tool_even_with_an_edit_token() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Notes", "Un\n").await;
    let (token, _) = api_token(
        &t.store,
        team.olga.id,
        TokenLevel::Edit,
        TokenScope::All,
        true,
    )
    .await;
    let before = base_of(&t, &team, page).await;
    for era in ERAS {
        let mcp = Mcp::new(&t, &token, era).readonly();
        mcp.open().await;
        let listed = mcp.rpc("tools/list", json!({})).await;
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["search", "fetch", "workspace_info"], "{era:?}");
        for (tool, arguments) in [
            (
                "update_page",
                json!({ "page": page.to_string(), "operations": [{ "op": "insert", "position": "end", "markdown": "Non\n" }] }),
            ),
            (
                "create_pages",
                json!({ "space": team.space.to_string(), "pages": [{ "title": "Non" }] }),
            ),
            (
                "manage_pages",
                json!({ "action": "trash", "page": page.to_string() }),
            ),
        ] {
            let reply = mcp
                .send(
                    "tools/call",
                    json!({ "name": tool, "arguments": arguments }),
                )
                .await;
            reply.assert_status(StatusCode::OK);
            assert_eq!(
                reply.json()["error"]["code"],
                -32602,
                "{era:?} {tool} : {}",
                reply.text()
            );
        }
        mcp.call("fetch", json!({ "id": page.to_string() }))
            .await
            .ok();
    }
    assert_eq!(base_of(&t, &team, page).await, before);
}

/// Un espace se nomme par son nom, sans casse, ou par son identifiant (Ruling R1236 ; plan,
/// « Le serveur MCP ») ; un espace hors de la portée du jeton répond exactement comme un nom
/// inconnu (Review Focus de M2).
#[tokio::test]
async fn a_space_is_named_by_its_name() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let marker = word();
    let page = page_with(&team, "Notes", &format!("Un {marker}.\n")).await;
    t.indexer.settled().await;
    let (all, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    let mcp = Mcp::new(&t, &all, Era::V2026);
    for name in ["Équipe", "équipe", " ÉQUIPE ", &team.space.to_string()] {
        let found = mcp
            .call("search", json!({ "query": marker, "space": name }))
            .await;
        found.ok();
        assert_eq!(
            ToolResult::uuids(found.content().unwrap()).first(),
            Some(&page),
            "{name} : {}",
            found.text
        );
    }
    // L'alias du bloc trouvé vise ce bloc dans `update_page`, sans relire la page.
    let found = mcp
        .call("search", json!({ "query": marker, "space": "Équipe" }))
        .await;
    let alias = found
        .content()
        .unwrap()
        .split(", bloc ")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .unwrap()
        .to_string();
    mcp.call(
        "update_page",
        json!({
            "page": page.to_string(),
            "operations": [{ "op": "str_replace", "anchor": alias, "old": "Un ", "new": "Une " }],
        }),
    )
    .await
    .ok();
    assert_eq!(mmd_of(&team, page).await, format!("Une {marker}.\n"));
    let created = mcp
        .call(
            "create_pages",
            json!({ "space": "équipe", "pages": [{ "title": "Nommée" }] }),
        )
        .await;
    created.ok();
    let id = ToolResult::uuids(created.content().unwrap())[0];
    assert_eq!(
        t.store.get_node(id).await.unwrap().unwrap().space_id,
        team.space
    );
    // Le jeton de l'espace d'équipe seulement : l'espace de Mona est hors de sa portée.
    let (narrow, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::Space(team.space),
        false,
    )
    .await;
    let mcp = Mcp::new(&t, &narrow, Era::V2026);
    let own_space = t.store.spaces_of(team.mona.id).await.unwrap();
    let own = own_space
        .iter()
        .find(|(space, _)| space.id != team.space)
        .unwrap()
        .0
        .clone();
    for tool in ["search", "create_pages", "workspace_info"] {
        let args = |space: &str| match tool {
            "search" => json!({ "query": marker, "space": space }),
            "create_pages" => json!({ "space": space, "pages": [{ "title": "Non" }] }),
            _ => json!({ "space": space }),
        };
        let outside = mcp.call(tool, args(&own.name)).await;
        outside.refused("not_found");
        let by_id = mcp.call(tool, args(&own.id.to_string())).await;
        let unknown = mcp.call(tool, args("Nulle part")).await;
        assert_eq!(outside.text, unknown.text, "{tool}");
        assert_eq!(by_id.text, unknown.text, "{tool}");
    }
    assert!(
        t.store
            .list_nodes(own.id)
            .await
            .unwrap()
            .iter()
            .all(|n| n.title != "Non")
    );
}

/// Deux espaces du même nom : le nom ne suffit pas (`ambiguous_space`, avec leurs identifiants) ;
/// l'identifiant, oui.
#[tokio::test]
async fn two_spaces_of_one_name_are_ambiguous() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let twin = team.another_space("Équipe").await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    let mcp = Mcp::new(&t, &token, Era::V2026);
    for (tool, args) in [
        ("search", json!({ "query": "notes", "space": "équipe" })),
        (
            "create_pages",
            json!({ "space": "Équipe", "pages": [{ "title": "Où ?" }] }),
        ),
        ("workspace_info", json!({ "space": "Équipe" })),
        ("fetch", json!({ "id": "Équipe" })),
    ] {
        let refused = mcp.call(tool, args).await;
        refused.refused("ambiguous_space");
        for id in [team.space, twin] {
            assert!(
                refused.head().contains(&id.to_string()),
                "{tool} : {}",
                refused.text
            );
        }
    }
    mcp.call(
        "search",
        json!({ "query": "notes", "space": twin.to_string() }),
    )
    .await
    .ok();
    for space in [team.space, twin] {
        assert!(
            t.store
                .list_nodes(space)
                .await
                .unwrap()
                .iter()
                .all(|n| n.title != "Où ?")
        );
    }
}

/// L'espace est implicite quand la portée du jeton n'en ouvre qu'un (conception §6.5), pour un
/// jeton d'espace comme pour un jeton de pages ; un jeton qui en ouvre plusieurs doit le nommer
/// (`space_required`).
#[tokio::test]
async fn the_space_is_implicit_when_the_token_opens_one() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let marker = word();
    let page = page_with(&team, "Notes", &format!("Un {marker}.\n")).await;
    t.indexer.settled().await;
    let (space_token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::Space(team.space),
        false,
    )
    .await;
    let (pages_token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Read,
        TokenScope::Pages {
            space: team.space,
            pages: vec![page],
        },
        false,
    )
    .await;
    for token in [&space_token, &pages_token] {
        let found = Mcp::new(&t, token, Era::V2026)
            .call("search", json!({ "query": marker }))
            .await;
        found.ok();
        assert!(
            found.head().contains(&team.space.to_string()),
            "{}",
            found.text
        );
        assert_eq!(
            ToolResult::uuids(found.content().unwrap()).first(),
            Some(&page)
        );
    }
    let created = Mcp::new(&t, &space_token, Era::V2025)
        .call(
            "create_pages",
            json!({ "pages": [{ "title": "Implicite" }] }),
        )
        .await;
    created.ok();
    let id = ToolResult::uuids(created.content().unwrap())[0];
    assert_eq!(
        t.store.get_node(id).await.unwrap().unwrap().space_id,
        team.space
    );
    let (all, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    let mcp = Mcp::new(&t, &all, Era::V2026);
    mcp.call("search", json!({ "query": marker }))
        .await
        .refused("space_required");
    mcp.call("create_pages", json!({ "pages": [{ "title": "Où ?" }] }))
        .await
        .refused("space_required");
}

/// `fetch` lit une page par son adresse : l'adresse publique de l'instance, ou le chemin de
/// l'application, et `#b=<bloc>` (échappé) fait commencer la lecture à ce bloc ; une page créée dit
/// son adresse publique.
#[tokio::test]
async fn fetch_takes_a_page_address() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.public_url = Some("https://notes.exemple.fr".to_string());
    })
    .await;
    let team = Team::new(&t).await;
    let page = page_with(
        &team,
        "Notes",
        "Un premier. ^a1\n\nDeux second. ^a2\n\nTrois troisième. ^a3\n",
    )
    .await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    let mcp = Mcp::new(&t, &token, Era::V2026);
    let whole = mcp
        .call(
            "fetch",
            json!({ "id": format!("https://notes.exemple.fr/p/{page}") }),
        )
        .await;
    whole.ok();
    assert!(
        whole.content().unwrap().contains("Un premier."),
        "{}",
        whole.text
    );
    for address in [
        format!("/p/{page}#b=a2"),
        format!("https://notes.exemple.fr/p/{page}#b=a%32"),
        format!("http://localhost:5173/p/{page}#b=a2"),
    ] {
        let from = mcp.call("fetch", json!({ "id": address })).await;
        from.ok();
        let content = from.content().unwrap();
        assert!(
            content.contains("Deux second."),
            "{address} : {}",
            from.text
        );
        assert!(content.contains("Trois troisième."), "{address}");
        assert!(
            !content.contains("Un premier."),
            "{address} : {}",
            from.text
        );
    }
    for address in [
        format!("https://notes.exemple.fr/x/{page}"),
        "https://notes.exemple.fr/p/pas-une-page".to_string(),
        format!("/p/{}", Uuid::now_v7()),
    ] {
        mcp.call("fetch", json!({ "id": address }))
            .await
            .refused("not_found");
    }
    let created = mcp
        .call(
            "create_pages",
            json!({ "space": "Équipe", "parent": format!("https://notes.exemple.fr/p/{page}"), "pages": [{ "title": "Sous" }] }),
        )
        .await;
    created.ok();
    let id = ToolResult::uuids(created.content().unwrap())[0];
    assert!(
        created
            .content()
            .unwrap()
            .contains(&format!("https://notes.exemple.fr/p/{id}")),
        "{}",
        created.text
    );
    assert_eq!(
        t.store.get_node(id).await.unwrap().unwrap().parent_id,
        Some(page)
    );
}

/// `fetch` d'un espace (son nom, ou son identifiant) en dessine le plan tel que le jeton le voit :
/// une page qu'il ne voit pas n'y est pas, et un jeton de pages n'y voit que les siennes, à la
/// racine de ce qu'il voit.
#[tokio::test]
async fn fetch_of_a_space_draws_its_tree_as_the_token_sees_it() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let public = team
        .page(None, "Publique", &[(Subject::Members, Level::Read)])
        .await;
    let child = team.page(Some(public), "Enfant", &[]).await;
    let secret = team
        .page(
            None,
            "Secrète",
            &[(Subject::User(team.olga.id), Level::Full)],
        )
        .await;
    let (all, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Read,
        TokenScope::All,
        false,
    )
    .await;
    for id in ["Équipe".to_string(), team.space.to_string()] {
        let plan = Mcp::new(&t, &all, Era::V2026)
            .call("fetch", json!({ "id": id }))
            .await;
        plan.ok();
        let content = plan.content().unwrap();
        assert!(
            content.contains(&format!("\n- Publique ({public})\n  - Enfant ({child})")),
            "{content}"
        );
        assert!(!content.contains("Secrète"), "{content}");
        assert!(!content.contains(&secret.to_string()), "{content}");
        assert!(
            plan.head()
                .contains(&format!("Plan de l'espace {}", team.space)),
            "{}",
            plan.text
        );
    }
    let (pages, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Read,
        TokenScope::Pages {
            space: team.space,
            pages: vec![child],
        },
        false,
    )
    .await;
    let plan = Mcp::new(&t, &pages, Era::V2026)
        .call("fetch", json!({ "id": "Équipe" }))
        .await;
    plan.ok();
    let content = plan.content().unwrap();
    assert!(
        content.contains(&format!("\n- Enfant ({child})")),
        "{content}"
    );
    assert!(!content.contains("Publique"), "{content}");
}

/// Une ligne du journal d'activité : l'outil, la surface, l'issue, le code, la page.
type Line = (
    String,
    CallSurface,
    CallOutcome,
    Option<String>,
    Option<Uuid>,
);

/// Chaque appel admis laisse une ligne au journal d'activité du jeton (Ruling R1244) : un appel
/// d'outil y est dit par son outil, sa page, son issue et son code, refus du protocole compris ;
/// une écriture, avec ses révisions et ses blocs touchés ; une autre méthode, par la route.
#[tokio::test]
async fn each_tool_call_leaves_one_activity_line() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Notes", "Un\n").await;
    let (token, token_id) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await;
    let mcp = Mcp::new(&t, &token, Era::V2026);
    let p = page.to_string();
    mcp.call("workspace_info", json!({})).await.ok();
    mcp.call("fetch", json!({ "id": p })).await.ok();
    let updated = mcp
        .call(
            "update_page",
            json!({ "page": p, "operations": [{ "op": "insert", "position": "end", "markdown": "Deux\n" }] }),
        )
        .await;
    updated.ok();
    let missing = Uuid::now_v7();
    mcp.call("fetch", json!({ "id": missing.to_string() }))
        .await
        .refused("not_found");
    mcp.send("tools/call", json!({ "name": "nope", "arguments": {} }))
        .await
        .assert_status(StatusCode::OK);
    mcp.call("update_page", json!({ "page": p }))
        .await
        .refused("invalid_arguments");
    mcp.rpc("tools/list", json!({})).await;
    t.calls.flush(&t.store).await;
    let mut lines = t
        .store
        .calls_of(team.mona.id, token_id, None, None, 50, now())
        .await
        .unwrap();
    lines.reverse();
    let seen: Vec<Line> = lines
        .iter()
        .map(|row| {
            (
                row.call.tool.clone(),
                row.call.surface,
                row.call.outcome,
                row.call.code.clone(),
                row.call.node_id,
            )
        })
        .collect();
    use CallOutcome::{Ok as Done, Refused};
    use CallSurface::Mcp as Tool;
    assert_eq!(
        seen,
        [
            ("workspace_info".to_string(), Tool, Done, None, None),
            ("fetch".to_string(), Tool, Done, None, Some(page)),
            ("update_page".to_string(), Tool, Done, None, Some(page)),
            (
                "fetch".to_string(),
                Tool,
                Refused,
                Some("not_found".to_string()),
                Some(missing)
            ),
            (
                "nope".to_string(),
                Tool,
                Refused,
                Some("unknown_tool".to_string()),
                None
            ),
            (
                "update_page".to_string(),
                Tool,
                Refused,
                Some("invalid_arguments".to_string()),
                None
            ),
            ("POST /mcp".to_string(), Tool, Done, None, None),
        ]
    );
    let write = &lines[2].call;
    assert_eq!(
        write.revision_after.as_deref(),
        Some(updated.revision().as_str())
    );
    assert!(write.revision_before.is_some());
    assert_eq!(write.blocks_touched, Some(1));
}

/// Les deux révisions sont servies sur la même adresse, chacune dans sa forme (Ruling R1236) : un
/// client de la 2026-07-28 sonde par `server/discover` et reçoit `resultType` ; un client de la
/// 2025-11-25 ouvre par `initialize` et ne le reçoit pas.
#[tokio::test]
async fn both_eras_are_served_on_one_endpoint() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (token, _) = api_token(
        &t.store,
        team.mona.id,
        TokenLevel::Read,
        TokenScope::All,
        false,
    )
    .await;
    let current = Mcp::new(&t, &token, Era::V2026);
    current.open().await;
    let listed = current.rpc("tools/list", json!({})).await;
    assert_eq!(listed["resultType"], "complete");
    assert_eq!(listed["tools"].as_array().unwrap().len(), 6);
    let resource = current
        .rpc("resources/read", json!({ "uri": "mementum://spec/mmd" }))
        .await;
    assert_eq!(resource["resultType"], "complete");
    assert!(
        resource["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("MMD")
    );
    let reply = current
        .send(
            "tools/call",
            json!({ "name": "workspace_info", "arguments": {} }),
        )
        .await;
    assert_eq!(reply.json()["result"]["resultType"], "complete");

    let legacy = Mcp::new(&t, &token, Era::V2025);
    legacy.open().await;
    let listed = legacy.rpc("tools/list", json!({})).await;
    assert!(listed.get("resultType").is_none(), "{listed}");
    assert_eq!(listed["tools"].as_array().unwrap().len(), 6);
    let reply = legacy
        .send(
            "tools/call",
            json!({ "name": "workspace_info", "arguments": {} }),
        )
        .await;
    let result = &reply.json()["result"];
    assert!(result.get("resultType").is_none(), "{result}");
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("compte : Mona")
    );
}

/// Le parcours d'Alex (plan du lot 5), dans chaque révision : son jeton n'ouvre que son espace
/// « Pro » ; l'agent dit qui il est, verse un compte rendu dans « Pro », le relit, puis le complète
/// pendant qu'Alex tape dans la page ouverte : rien de ce qu'Alex tape n'est perdu, et Alex voit ce
/// que l'agent a écrit (critère de fin de la P1, conception §9).
#[tokio::test]
async fn an_agent_pours_a_page_and_completes_it_while_someone_types() {
    let t = open_app().await;
    let alex = t.account("Alex").await;
    let pro = t.store.create_space("Pro", Some(alex.id)).await.unwrap().id;
    let addr = t.listen().await;
    for era in ERAS {
        let (token, _) = api_token(
            &t.store,
            alex.id,
            TokenLevel::Edit,
            TokenScope::Space(pro),
            false,
        )
        .await;
        let mcp = Mcp::new(&t, &token, era);
        mcp.open().await;
        let me = mcp.call("workspace_info", json!({})).await;
        me.ok();
        assert!(
            me.content().unwrap().contains(&format!("- Pro ({pro})")),
            "{}",
            me.text
        );
        let title = format!("Compte rendu {era:?}");
        let created = mcp
            .call(
                "create_pages",
                json!({
                    "space": "Pro",
                    "pages": [{ "title": title, "markdown": "# Décisions\n\nSortir la v1.\n" }],
                }),
            )
            .await;
        created.ok();
        let page = ToolResult::uuids(created.content().unwrap())[0];
        let tree: Vec<Value> = tree_nodes(&alex.browser, pro).await;
        assert!(
            tree.iter()
                .any(|n| n["id"] == page.to_string() && n["title"] == title.as_str()),
            "la page paraît dans la barre latérale d'Alex : {tree:?}"
        );
        let read = mcp
            .call("fetch", json!({ "id": page.to_string(), "anchors": true }))
            .await;
        read.ok();
        assert!(read.content().unwrap().contains("Sortir la v1."));
        let revision = read.revision();
        let tab = synced_as(&alex.browser, &sync_url(addr, page)).await;
        let block = tab.blocks()[1].id.clone();
        tab.type_text(&block, " Avant décembre.");
        let completed = mcp
            .call(
                "update_page",
                json!({
                    "page": page.to_string(),
                    "base_revision": revision,
                    "operations": [{ "op": "insert", "position": "end", "markdown": "- Relire le plan.\n" }],
                }),
            )
            .await;
        completed.ok();
        tab.type_text(&block, " Sans faute.");
        let wanted =
            "# Décisions\n\nSortir la v1. Avant décembre. Sans faute.\n\n- Relire le plan.\n";
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let mmd = alex.get_mmd(&page.to_string()).await.text();
            if mmd == wanted {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "{era:?} : le serveur garde « {mmd} »"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(
            tab.wait_until(WAIT, |c| c
                .blocks()
                .iter()
                .any(|b| mementum_md::block_text(b) == "Relire le plan."))
                .await,
            "{era:?} : Alex ne voit pas ce que l'agent a écrit"
        );
        let reread = mcp.call("fetch", json!({ "id": page.to_string() })).await;
        assert!(
            reread.content().unwrap().contains(wanted),
            "{}",
            reread.text
        );
    }
}

/// Un outil ne lit rien du store lui-même (Review Focus de M2 : aucun outil n'a de droit à lui) :
/// la route MCP ne nomme jamais la base, et ne regarde jamais comment l'appel est venu ; elle
/// passe par les fonctions d'`agent/`, qui jugent les droits par `access.rs`.
#[test]
fn no_tool_reads_the_store_itself() {
    let source = include_str!("../src/routes/mcp.rs");
    let code: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [".store", "mementum_store::Store", ".via", ".token()"] {
        assert!(
            !code.contains(forbidden),
            "« {forbidden} » dans routes/mcp.rs"
        );
    }
}

/// Les bornes que le schéma d'un outil donne au modèle sont celles de la route qu'il mène
/// (Task M1 : `MAX_QUERY_CHARS` et `MAX_TITLE_CHARS` recopient celles du lot 4, sans lien).
#[test]
fn the_bounds_of_the_tools_are_those_of_the_routes() {
    use mementum_mcp::tools;
    assert_eq!(tools::MAX_QUERY_CHARS, mementum_search::MAX_QUERY_CHARS);
    assert_eq!(tools::MAX_TITLE_CHARS, mementum_server::MAX_TITLE_CHARS);
    assert_eq!(
        tools::MAX_LIMIT as usize,
        mementum_mcp::text::MAX_CONTENT_BYTES
    );
}
