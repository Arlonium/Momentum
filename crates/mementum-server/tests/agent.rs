//! Lire pour un agent (lot 5, Task S7) : `GET /api/v1/pages/{id}/document` en tranches, l'`ETag`
//! de `GET …/mmd`, les filtres de la recherche, `/me` et les membres par jeton.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::*;
use mementum_acl::{Level, Subject};
use mementum_server::SignupMode;
use mementum_store::{TokenLevel, TokenScope};
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

async fn get_by(t: &TestApp, token: &str, uri: &str) -> Reply {
    t.send(with_bearer(bare(Method::GET, uri), token)).await
}

async fn json_by(t: &TestApp, token: &str, method: Method, uri: &str, body: Value) -> Reply {
    t.send(with_bearer(json_request(method, uri, &body), token))
        .await
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

async fn edit_token(t: &TestApp, team: &Team<'_>) -> String {
    api_token(
        &t.store,
        team.olga.id,
        TokenLevel::Edit,
        TokenScope::All,
        false,
    )
    .await
    .0
}

async fn document(t: &TestApp, token: &str, page: Uuid, query: &str) -> Value {
    let reply = get_by(t, token, &format!("/api/v1/pages/{page}/document{query}")).await;
    reply.assert_status(StatusCode::OK);
    reply.json()
}

/// `count` paragraphes « Paragraphe i … », chacun d'une soixantaine d'octets.
fn paragraphs(count: usize) -> String {
    let mut text: String = (0..count)
        .map(|i| format!("Paragraphe {i} : un texte de quelques mots, pour la longueur.\n\n"))
        .collect();
    text.pop();
    text
}

/// Les lignes non vides d'un MMD.
fn lines(mmd: &str) -> Vec<String> {
    mmd.lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// La révision d'une lecture sort de la même commande que ses blocs (Ruling R1226) : la tranche
/// et l'`ETag` de `GET …/mmd` disent la même chose de la page, avant et après une écriture.
#[tokio::test]
async fn a_document_carries_the_revision_of_its_blocks() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Notes", "Un\n\nDeux\n").await;
    let token = edit_token(&t, &team).await;
    for round in 0..2 {
        let doc = document(&t, &token, page, "").await;
        let mmd = get_by(&t, &token, &format!("/api/v1/pages/{page}/mmd")).await;
        mmd.assert_status(StatusCode::OK);
        assert_eq!(doc["mmd"], mmd.text(), "tour {round}");
        assert_eq!(
            mmd.headers[header::ETAG].to_str().unwrap(),
            format!("\"{}\"", doc["revision"].as_str().unwrap()),
            "tour {round}"
        );
        assert_eq!(doc["title"], "Notes");
        assert_eq!(doc["level"], "edit");
        assert_eq!(doc["truncated"], false);
        assert!(doc.get("changed").is_none(), "{doc}");
        team.olga
            .put_mmd(&page.to_string(), "Un\n\nDeux et trois\n")
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
}

/// Une longue page se lit en tranches coupées entre deux blocs, chacune sous sa borne, la suivante
/// reprise par `next` : mises bout à bout, elles rendent chaque bloc une fois, dans l'ordre.
#[tokio::test]
async fn a_long_page_is_cut_between_blocks_and_resumed() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Longue", &paragraphs(200)).await;
    let token = edit_token(&t, &team).await;
    let mut seen = Vec::new();
    let mut query = "?limit=2048".to_string();
    let mut slices = 0;
    loop {
        let doc = document(&t, &token, page, &query).await;
        let mmd = doc["mmd"].as_str().unwrap();
        assert!(mmd.len() <= 2048, "tranche de {} octets", mmd.len());
        seen.extend(lines(mmd));
        slices += 1;
        match doc["next"].as_str() {
            Some(next) => {
                assert_eq!(doc["truncated"], true);
                query = format!("?limit=2048&from={next}");
            }
            None => {
                assert_eq!(doc["truncated"], false);
                break;
            }
        }
    }
    assert!(slices > 4, "{slices} tranches");
    assert_eq!(seen, lines(&paragraphs(200)));
}

/// Un bloc plus gros que la borne se pagine en lui-même : sa ligne, ceux de ses sous-blocs qui
/// tiennent, puis `<unloaded id="…"/>` du premier qui ne tient pas, que `next` désigne ; la tranche
/// suivante reprend à lui (Ruling R1225).
#[tokio::test]
async fn a_block_larger_than_the_bound_is_paged_inside_itself() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let children: String = (0..80)
        .map(|i| format!("  - Sous-élément {i} : un peu de texte pour peser.\n"))
        .collect();
    let mmd = format!("- Parent\n{children}\nAprès\n");
    let page = page_with(&team, "Gros", &mmd).await;
    let token = edit_token(&t, &team).await;
    let first = document(&t, &token, page, "?limit=1024").await;
    let text = first["mmd"].as_str().unwrap();
    assert!(text.len() <= 1024);
    assert!(text.starts_with("- Parent"), "{text}");
    let next = first["next"].as_str().unwrap();
    assert!(
        text.contains(&format!("<unloaded id=\"{next}\"/>")),
        "{text}"
    );
    let second = document(&t, &token, page, &format!("?limit=1024&from={next}")).await;
    let resumed = second["mmd"].as_str().unwrap();
    assert!(!resumed.contains("Parent"), "{resumed}");
    let last_before = lines(text)
        .iter()
        .rev()
        .find(|l| l.contains("Sous-élément"))
        .cloned()
        .unwrap();
    let n: usize = last_before
        .split_whitespace()
        .nth(2)
        .unwrap()
        .trim_end_matches(':')
        .parse()
        .unwrap();
    assert!(
        resumed.contains(&format!("Sous-élément {} :", n + 1)),
        "{resumed}"
    );
}

/// Un bloc sans sous-bloc dont le texte seul dépasse la borne est rendu `<unloaded id="…"
/// bytes=N/>`, et la lecture reprend au bloc suivant (limite LA-o).
#[tokio::test]
async fn a_childless_block_larger_than_the_bound_is_unloaded() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let long = "mot ".repeat(700);
    let page = page_with(&team, "Bavard", &format!("{}\n\nSuite\n", long.trim_end())).await;
    let token = edit_token(&t, &team).await;
    let doc = document(&t, &token, page, "?limit=1024&anchors=block").await;
    let text = doc["mmd"].as_str().unwrap();
    assert!(
        text.starts_with("<unloaded id=\"b") && text.contains(" bytes="),
        "{text}"
    );
    let next = doc["next"].as_str().unwrap();
    let after = document(&t, &token, page, &format!("?limit=1024&from={next}")).await;
    assert_eq!(after["mmd"], "Suite\n");
    assert_eq!(after["truncated"], false);
}

/// B2 de la relecture du plan : une page lue en trois tranches ; Olga change un bloc de la
/// première pendant la lecture de la deuxième ; la troisième, demandée avec `since`, le dit
/// (`changedSince`, et son alias dans `changed`), et une écriture `rebase` qui le touche avec la
/// révision de la première tranche est refusée, ce qu'Olga a tapé restant.
#[tokio::test]
async fn a_block_changed_between_two_slices_is_told_and_refuses_the_write() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Lue", &paragraphs(60)).await;
    let token = edit_token(&t, &team).await;
    let first = document(&t, &token, page, "?limit=1500&anchors=block").await;
    let since = first["revision"].as_str().unwrap().to_string();
    let alias_of_two = lines(first["mmd"].as_str().unwrap())
        .into_iter()
        .find(|l| l.starts_with("Paragraphe 2 :"))
        .and_then(|l| l.rsplit_once(" ^").map(|(_, a)| a.to_string()))
        .unwrap();
    let next = first["next"].as_str().unwrap().to_string();
    let second = document(
        &t,
        &token,
        page,
        &format!("?limit=1500&anchors=block&from={next}&since={since}"),
    )
    .await;
    assert_eq!(second["changedSince"], false, "{second}");
    // Olga change le paragraphe 2, déjà servi.
    let changed = paragraphs(60).replace("Paragraphe 2 :", "Paragraphe 2 (relu par Olga) :");
    team.olga
        .put_mmd(&page.to_string(), changed.clone())
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let next = second["next"].as_str().unwrap().to_string();
    let third = document(
        &t,
        &token,
        page,
        &format!("?limit=1500&anchors=block&from={next}&since={since}"),
    )
    .await;
    assert_eq!(third["changedSince"], true, "{third}");
    assert_eq!(third["changed"], json!([alias_of_two]), "{third}");
    let write = json_by(
        &t,
        &token,
        Method::POST,
        &format!("/api/v1/pages/{page}/operations"),
        json!({
            "baseRevision": since,
            "operations": [{ "op": "str_replace", "old": "Paragraphe 2", "new": "Second", "anchor": alias_of_two }],
        }),
    )
    .await;
    write.assert_error(StatusCode::CONFLICT, "revision_conflict");
    assert_eq!(team.olga.get_mmd(&page.to_string()).await.text(), changed);
}

/// Les empreintes de `since` ne sont plus gardées : `changed: null`, et `changedSince` vrai (relire
/// depuis le début).
#[tokio::test]
async fn since_out_of_the_cache_says_read_again() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.kept_revision_bytes = 0;
    })
    .await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Oubliée", &paragraphs(40)).await;
    let token = edit_token(&t, &team).await;
    let first = document(&t, &token, page, "?limit=1024").await;
    let since = first["revision"].as_str().unwrap().to_string();
    team.olga
        .put_mmd(&page.to_string(), paragraphs(41))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let next = first["next"].as_str().unwrap();
    let second = document(
        &t,
        &token,
        page,
        &format!("?limit=1024&from={next}&since={since}"),
    )
    .await;
    assert_eq!(second["changedSince"], true, "{second}");
    assert_eq!(second["changed"], Value::Null, "{second}");
}

/// L'`ETag` de `GET …/mmd` est la révision ; celui de `PUT …/mmd`, la révision de l'état écrit,
/// celle que lit ensuite qui lit la page ; avec `?anchors=block`, chaque bloc porte l'alias qu'une
/// opération vise.
#[tokio::test]
async fn etag_is_the_revision() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Étiquetée", "Un\n\nDeux\n").await;
    let id = page.to_string();
    let read = team.olga.get_mmd(&id).await;
    let etag = read.headers[header::ETAG].to_str().unwrap().to_string();
    let written = team.olga.put_mmd(&id, "Un\n\nDeux bis\n").await;
    written.assert_status(StatusCode::NO_CONTENT);
    let written_etag = written.headers[header::ETAG].to_str().unwrap().to_string();
    assert_ne!(written_etag, etag);
    let again = team.olga.get_mmd(&id).await;
    assert_eq!(again.headers[header::ETAG].to_str().unwrap(), written_etag);
    let anchored = team
        .olga
        .browser
        .get(&format!("/api/v1/pages/{id}/mmd?anchors=block"))
        .await;
    anchored.assert_status(StatusCode::OK);
    let text = anchored.text();
    assert!(
        lines(&text).iter().all(|l| l.contains(" ^b")),
        "chaque bloc porte son alias : {text}"
    );
}

/// Pages d'Olga et de Mona qui portent un même mot ; un filtre ne garde que ce qu'il dit, parmi
/// ce que le compte voit (Ruling R1248).
#[tokio::test]
async fn a_filter_only_narrows_the_allowed_set() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let olga_page = page_with(&team, "Chez Olga", "Le mot pivoine est ici.\n").await;
    let mona_page = team
        .mona
        .create(
            json!({ "spaceId": team.space.to_string(), "title": "Chez Mona", "section": "space" }),
        )
        .await;
    team.mona
        .put_mmd(&mona_page.id, "La pivoine aussi.\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    t.indexer.settled().await;
    let search = |body: Value| {
        let by = &team.olga.browser;
        let uri = format!("/api/v1/spaces/{}/search", team.space);
        async move { by.post(&uri, body).await }
    };
    let pages = |reply: Reply| -> Vec<String> {
        reply.assert_status(StatusCode::OK);
        reply.json()["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["pageId"].as_str().unwrap().to_string())
            .collect()
    };
    let all = pages(search(json!({ "q": "pivoine" })).await);
    assert_eq!(all.len(), 2, "{all:?}");
    let by_mona =
        pages(search(json!({ "q": "pivoine", "createdBy": team.mona.id.to_string() })).await);
    assert_eq!(by_mona, std::slice::from_ref(&mona_page.id));
    let by_olga =
        pages(search(json!({ "q": "pivoine", "createdBy": team.olga.id.to_string() })).await);
    assert_eq!(by_olga, [olga_page.to_string()]);
    let future =
        pages(search(json!({ "q": "pivoine", "editedAfter": 32_503_680_000_000_i64 })).await);
    assert!(future.is_empty(), "{future:?}");
    let past = pages(
        search(json!({ "q": "pivoine", "editedAfter": 0, "editedBefore": 32_503_680_000_000_i64 }))
            .await,
    );
    assert_eq!(past.len(), 2, "{past:?}");
    search(json!({ "q": "pivoine", "createdBy": "pas-un-compte" }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_filter");
    search(json!({ "q": "pivoine", "editedAfter": 5, "editedBefore": 5 }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_filter");
}

/// Vingt et une pages d'Olga que Mona voit, qui portent le mot ; puis cinq pages privées d'Olga
/// qui le portent davantage (mieux classées) et passeraient le filtre « créée par Olga ». Les
/// réponses de Mona, à `after` 0 et 20 (`more`, lignes, `indexing`), restent les mêmes, octet pour
/// octet : le filtre ne fait que réduire ce qu'elle voit (R1389 ; tour de correction de S7).
#[tokio::test]
async fn a_filter_never_shows_an_unseen_page() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    for i in 0..21 {
        page_with(&team, &format!("Vue {i}"), "Le mot glycine est ici.\n").await;
    }
    t.indexer.settled().await;
    let uri = format!("/api/v1/spaces/{}/search", team.space);
    let ask = |after: u32| {
        let by = &team.mona.browser;
        let uri = uri.clone();
        let body = json!({ "q": "glycine", "createdBy": team.olga.id.to_string(), "after": after });
        async move { by.post(&uri, body).await }
    };
    let before = [ask(0).await, ask(20).await];
    assert_eq!(before[0].json()["results"].as_array().unwrap().len(), 20);
    assert_eq!(before[0].json()["more"], true);
    assert_eq!(before[1].json()["results"].as_array().unwrap().len(), 1);
    for i in 0..5 {
        let private = team
            .page(
                None,
                &format!("Glycine glycine {i}"),
                &[(Subject::User(team.olga.id), Level::Full)],
            )
            .await;
        team.olga
            .put_mmd(
                &private.to_string(),
                "Glycine, glycine, glycine : la glycine d'Olga.\n",
            )
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
    t.indexer.settled().await;
    // Olga les trouve : elles sont indexées, et mieux classées.
    let olga = team
        .olga
        .browser
        .post(
            &uri,
            json!({ "q": "glycine", "createdBy": team.olga.id.to_string() }),
        )
        .await;
    assert!(
        olga.json()["results"][0]["title"]
            .as_str()
            .unwrap()
            .starts_with("Glycine"),
        "{}",
        olga.text()
    );
    let after = [ask(0).await, ask(20).await];
    for (before, after) in before.iter().zip(&after) {
        assert_eq!(after.status, before.status);
        assert_eq!(after.text(), before.text());
    }
}

/// Le coût d'une recherche filtrée suit ce que le compte voit (tour de correction de S7, la forme
/// de R1201) : le store juge autant de pages, que des pages qu'il ne voit pas passent le filtre ou
/// non.
#[tokio::test]
async fn a_filtered_search_reads_only_the_pages_the_account_sees() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    for i in 0..3 {
        page_with(&team, &format!("Vue {i}"), "Le mot lavande est ici.\n").await;
    }
    t.indexer.settled().await;
    let uri = format!("/api/v1/spaces/{}/search", team.space);
    let body = json!({ "q": "lavande", "createdBy": team.olga.id.to_string() });
    let judged = async || {
        let before = t.store.filtered_pages_asked();
        team.mona
            .browser
            .post(&uri, body.clone())
            .await
            .assert_status(StatusCode::OK);
        t.store.filtered_pages_asked() - before
    };
    let without = judged().await;
    assert!(without > 0, "aucune page jugée");
    for i in 0..10 {
        team.page(
            None,
            &format!("Privée {i}"),
            &[(Subject::User(team.olga.id), Level::Full)],
        )
        .await;
    }
    t.indexer.settled().await;
    assert_eq!(judged().await, without);
}

/// La date du contenu d'une page (`editedAt`) est posée par l'indexeur, d'après le journal
/// (Ruling R1246) : la date de l'écriture, pas celle de l'indexation ; les filtres de date de la
/// recherche la lisent.
#[tokio::test]
async fn edited_at_follows_the_indexer() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let token = edit_token(&t, &team).await;
    let page = page_with(&team, "Datée", "Un texte au mot capucine.\n").await;
    t.indexer.settled().await;
    let written = query_i64(
        &t.db_path(),
        &format!(
            "SELECT created_at FROM doc_updates WHERE node_id = '{page}' ORDER BY seq DESC LIMIT 1"
        ),
    )
    .await;
    let doc = document(&t, &token, page, "").await;
    assert_eq!(doc["editedAt"], json!(written), "{doc}");
    let uri = format!("/api/v1/spaces/{}/search", team.space);
    let found = |reply: Reply| reply.json()["results"].as_array().unwrap().len();
    let at = json!({ "q": "capucine", "editedAfter": written });
    assert_eq!(found(team.olga.browser.post(&uri, at).await), 1);
    let later = json!({ "q": "capucine", "editedAfter": written + 1 });
    assert_eq!(found(team.olga.browser.post(&uri, later).await), 0);
}

/// `GET /api/v1/me` par un jeton : le compte sans son adresse, les espaces de sa portée, le jeton ;
/// la liste des membres par un jeton : leurs noms et leurs rôles, jamais leurs adresses (Ruling
/// R1247).
#[tokio::test]
async fn me_by_token_has_no_email() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (token, token_id) = api_token(
        &t.store,
        team.olga.id,
        TokenLevel::Read,
        TokenScope::Space(team.space),
        false,
    )
    .await;
    let me = get_by(&t, &token, "/api/v1/me").await;
    me.assert_status(StatusCode::OK);
    let text = me.text();
    assert!(!text.contains("@exemple.fr"), "une adresse : {text}");
    let me = me.json();
    assert_eq!(me["user"]["name"], "Olga");
    let spaces: Vec<&str> = me["spaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        spaces,
        [team.space.to_string()],
        "seul l'espace de la portée"
    );
    assert_eq!(me["token"]["id"], token_id.to_string());
    let members = get_by(
        &t,
        &token,
        &format!("/api/v1/spaces/{}/members", team.space),
    )
    .await;
    members.assert_status(StatusCode::OK);
    assert!(
        !members.text().contains("@exemple.fr"),
        "{}",
        members.text()
    );
    let listed = members.json();
    let names: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Mona"), "{names:?}");
    // Par la session, l'application garde ses réponses d'avant (les adresses).
    let session = team
        .olga
        .browser
        .get(&format!("/api/v1/spaces/{}/members", team.space))
        .await;
    assert!(session.text().contains("@exemple.fr"));
    assert!(
        team.olga
            .browser
            .me()
            .await
            .text()
            .contains("olga@exemple.fr")
    );
}
