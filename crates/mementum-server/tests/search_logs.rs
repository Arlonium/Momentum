//! La recherche d'une personne n'est jamais journalisée (lot 4, R917), seule dans son binaire.
//!
//! Le test lit le journal de tout le processus (`all_logs`, l'abonné global : les instructions de
//! sqlx sont écrites depuis le fil de SQLite, que `Logs::capture` ne voit pas). Logé dans
//! `tests/search.rs` avec les autres tests de la route, il rougissait sous Linux (« le journal ne
//! voit rien », deux passages sur trois de la CI du 2026-11-21) : les autres tests du binaire
//! touchaient les points de journal de sqlx avant que l'abonné global soit posé. Comme le
//! compte des requêtes d'une route (R266), il est donc seul dans son binaire.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::{Account, Reply, Team, TestApp, all_logs};
use mementum_acl::{Level, Subject};
use mementum_server::SignupMode;
use serde_json::{Value, json};
use uuid::Uuid;

/// Le calme attendu avant d'indexer une page écrite.
const QUICK: Duration = Duration::from_millis(20);

/// La réponse d'une recherche qui ne trouve rien, index à jour.
fn nothing() -> Value {
    json!({ "results": [], "more": false, "indexing": false })
}

/// Une application aux inscriptions ouvertes, dont l'indexeur n'attend que [`QUICK`] de calme.
async fn app() -> TestApp {
    TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.index_after = QUICK;
    })
    .await
}

fn members(level: Level) -> (Subject, Level) {
    (Subject::Members, level)
}

fn uri(space: Uuid) -> String {
    format!("/api/v1/spaces/{space}/search")
}

/// `POST …/search` avec `{ "q": q }`.
async fn search(by: &Account<'_>, space: Uuid, q: &str) -> Reply {
    by.browser.post(&uri(space), json!({ "q": q })).await
}

/// Les pages qu'une recherche rend à `by`, rangées.
async fn pages_found(by: &Account<'_>, space: Uuid, q: &str) -> Vec<Uuid> {
    let reply = search(by, space, q).await;
    reply.assert_status(StatusCode::OK);
    let results = reply.json();
    let mut pages: Vec<Uuid> = results["results"]
        .as_array()
        .unwrap_or_else(|| panic!("« results » en liste attendu dans {results}"))
        .iter()
        .map(|hit| hit["pageId"].as_str().unwrap().parse().unwrap())
        .collect();
    pages.sort();
    pages
}

/// La recherche d'une personne n'est jamais journalisée, à aucun niveau, sur aucun fil (R917) :
/// ni quand elle passe, ni refusée. Témoin : le journal de tout le processus voit bien les
/// lectures de la base. Et elle ne voyage pas dans une adresse : un `GET` répond `405`, et un `q`
/// dans l'adresse d'un `POST` n'est pas lu.
#[tokio::test]
async fn the_query_is_never_logged_and_never_in_an_address() {
    let logs = all_logs();
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Mot xyzzytémoin", &[members(Level::Read)])
        .await;
    t.indexer.settled().await;
    // Rien d'autre que la recherche ne mettrait ce mot au journal (le titre n'y va pas).
    assert!(!logs.text().to_lowercase().contains("xyzzyt"));
    assert_eq!(
        pages_found(&team.mona, team.space, "xyzzytémoin").await,
        [page]
    );
    assert_eq!(
        pages_found(&team.mona, team.space, "XyzzyTém").await,
        [page]
    );
    let many = "xyzzytémoin deux trois quatre cinq six sept huit neuf";
    search(&team.mona, team.space, many)
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_query");
    search(&team.sam, team.space, "xyzzytémoin")
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");

    let get = team
        .mona
        .browser
        .get(&format!("{}?q=xyzzyt%C3%A9moin", uri(team.space)))
        .await;
    get.assert_error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    let in_the_address = team
        .mona
        .browser
        .post(
            &format!("{}?q=xyzzyt%C3%A9moin", uri(team.space)),
            json!({ "q": "" }),
        )
        .await;
    in_the_address.assert_status(StatusCode::OK);
    assert_eq!(in_the_address.json(), nothing());

    let text = logs.text().to_lowercase();
    assert!(text.contains("sqlx::query"), "le journal ne voit rien");
    assert!(!text.contains("xyzzyt"), "la recherche est au journal");
}
