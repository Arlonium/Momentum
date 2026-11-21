//! Écrire pour un agent (lot 5, Task S7) : `POST /api/v1/pages/{id}/operations`, `PUT …/mmd`
//! avec `If-Match`, `POST /api/v1/spaces/{id}/pages/batch`, et le journal d'activité.

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

async fn token_of(t: &TestApp, user: Uuid, level: TokenLevel) -> (String, Uuid) {
    api_token(&t.store, user, level, TokenScope::All, false).await
}

/// La révision d'une lecture par l'agent.
async fn revision(t: &TestApp, token: &str, page: Uuid) -> String {
    let reply = get_by(t, token, &format!("/api/v1/pages/{page}/document")).await;
    reply.assert_status(StatusCode::OK);
    reply.json()["revision"].as_str().unwrap().to_string()
}

/// Les alias de la page, par le début du texte de leur bloc.
async fn aliases(t: &TestApp, token: &str, page: Uuid) -> Vec<(String, String)> {
    let reply = get_by(
        t,
        token,
        &format!("/api/v1/pages/{page}/document?anchors=block"),
    )
    .await;
    reply.assert_status(StatusCode::OK);
    reply.json()["mmd"]
        .as_str()
        .unwrap()
        .lines()
        .filter_map(|l| {
            l.rsplit_once(" ^")
                .map(|(t, a)| (t.to_string(), a.to_string()))
        })
        .collect()
}

fn alias_of(aliases: &[(String, String)], text: &str) -> String {
    aliases
        .iter()
        .find(|(t, _)| t.starts_with(text))
        .unwrap_or_else(|| panic!("aucun bloc « {text} » : {aliases:?}"))
        .1
        .clone()
}

async fn operations(t: &TestApp, token: &str, page: Uuid, body: Value) -> Reply {
    json_by(
        t,
        token,
        Method::POST,
        &format!("/api/v1/pages/{page}/operations"),
        body,
    )
    .await
}

async fn mmd_of(team: &Team<'_>, page: Uuid) -> String {
    team.olga.get_mmd(&page.to_string()).await.text()
}

/// Toutes les opérations d'un appel, ou aucune (Ruling R1229) : la seconde ne trouve pas son
/// texte, le refus la nomme par son rang, et la page n'a pas bougé, révision comprise.
#[tokio::test]
async fn operations_apply_all_or_nothing() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Atomique", "Un\n\nDeux\n").await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let before = revision(&t, &token, page).await;
    let refused = operations(
        &t,
        &token,
        page,
        json!({ "operations": [
            { "op": "str_replace", "old": "Un", "new": "Uno" },
            { "op": "str_replace", "old": "Absent", "new": "Rien" },
        ]}),
    )
    .await;
    let body = refused.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "operation_failed");
    assert_eq!(body["index"], 1, "{body}");
    assert_eq!(body["reason"], "no_match", "{body}");
    assert!(body["hint"].as_str().is_some_and(|h| !h.is_empty()));
    assert_eq!(mmd_of(&team, page).await, "Un\n\nDeux\n");
    assert_eq!(revision(&t, &token, page).await, before);
    let done = operations(
        &t,
        &token,
        page,
        json!({ "operations": [
            { "op": "str_replace", "old": "Un", "new": "Uno" },
            { "op": "insert", "position": "end", "markdown": "Trois\n" },
        ]}),
    )
    .await;
    done.assert_status(StatusCode::OK);
    let done = done.json();
    assert_eq!(mmd_of(&team, page).await, "Uno\n\nDeux\n\nTrois\n");
    assert_eq!(done["revision"], revision(&t, &token, page).await);
    assert_eq!(done["warnings"], json!([]));
}

/// Un `rebase` passe quand les blocs qu'il touche n'ont pas changé depuis la révision lue : Olga a
/// changé un autre bloc, et les deux écritures restent (Ruling R1231).
#[tokio::test]
async fn a_rebase_passes_when_the_touched_blocks_did_not_change() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Rebase", "Un\n\nDeux\n").await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let base = revision(&t, &token, page).await;
    team.olga
        .put_mmd(&page.to_string(), "Un d'Olga\n\nDeux\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    operations(
        &t,
        &token,
        page,
        json!({ "baseRevision": base, "operations": [{ "op": "str_replace", "old": "Deux", "new": "Deux de l'agent" }] }),
    )
    .await
    .assert_status(StatusCode::OK);
    assert_eq!(mmd_of(&team, page).await, "Un d'Olga\n\nDeux de l'agent\n");
}

/// … et refuse quand Olga a changé le bloc qu'il touche : `409 revision_conflict`, avec la
/// révision courante et le MMD à jour du bloc en cause ; rien n'est écrit.
#[tokio::test]
async fn a_rebase_refuses_with_the_current_blocks() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Conflit", "Un\n\nDeux\n").await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let base = revision(&t, &token, page).await;
    team.olga
        .put_mmd(&page.to_string(), "Un\n\nDeux d'Olga\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let current = revision(&t, &token, page).await;
    let refused = operations(
        &t,
        &token,
        page,
        json!({ "baseRevision": base, "operations": [{ "op": "str_replace", "old": "Deux", "new": "Second" }] }),
    )
    .await;
    let body = refused.assert_error(StatusCode::CONFLICT, "revision_conflict");
    assert_eq!(body["revision"], current, "{body}");
    assert!(
        body["mmd"]
            .as_str()
            .is_some_and(|m| m.contains("Deux d'Olga")),
        "{body}"
    );
    assert_eq!(mmd_of(&team, page).await, "Un\n\nDeux d'Olga\n");
}

/// Une révision dont le serveur n'a plus les empreintes vaut un conflit dès que la page a changé,
/// même ailleurs (Ruling R1226).
#[tokio::test]
async fn a_revision_out_of_the_cache_refuses_once_the_page_changed() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.kept_revision_bytes = 0;
    })
    .await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Oubliée", "Un\n\nDeux\n").await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let base = revision(&t, &token, page).await;
    // La page n'a pas changé : la révision est la courante, l'écriture passe.
    operations(
        &t,
        &token,
        page,
        json!({ "baseRevision": base, "operations": [{ "op": "str_replace", "old": "Deux", "new": "Deux bis" }] }),
    )
    .await
    .assert_status(StatusCode::OK);
    let base = revision(&t, &token, page).await;
    team.olga
        .put_mmd(&page.to_string(), "Un d'Olga\n\nDeux bis\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    operations(
        &t,
        &token,
        page,
        json!({ "baseRevision": base, "operations": [{ "op": "str_replace", "old": "Deux bis", "new": "Deux ter" }] }),
    )
    .await
    .assert_error(StatusCode::CONFLICT, "revision_conflict");
    assert_eq!(mmd_of(&team, page).await, "Un d'Olga\n\nDeux bis\n");
}

/// `replace_page`, `replace_block` et `delete_block` demandent la révision de la dernière lecture
/// (Ruling R1231) ; `suggest` n'est pas servi (R1211) ; cent opérations au plus.
#[tokio::test]
async fn replacing_needs_a_revision() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Révision", "Un\n\nDeux\n").await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let alias = alias_of(&aliases(&t, &token, page).await, "Deux");
    for op in [
        json!({ "op": "replace_block", "anchor": alias, "markdown": "Autre\n" }),
        json!({ "op": "delete_block", "anchor": alias }),
        json!({ "op": "replace_page", "markdown": "Tout\n" }),
    ] {
        let body = operations(
            &t,
            &token,
            page,
            json!({ "operations": [{ "op": "str_replace", "old": "Un", "new": "Uno" }, op] }),
        )
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "revision_required");
        assert_eq!(body["index"], 1, "{body}");
    }
    operations(
        &t,
        &token,
        page,
        json!({ "mode": "suggest", "operations": [{ "op": "str_replace", "old": "Un", "new": "Uno" }] }),
    )
    .await
    .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "mode_unavailable");
    let many: Vec<Value> = (0..101)
        .map(|_| json!({ "op": "str_replace", "old": "Un", "new": "Un" }))
        .collect();
    operations(&t, &token, page, json!({ "operations": many }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "too_many_operations");
    assert_eq!(mmd_of(&team, page).await, "Un\n\nDeux\n");
}

/// La fusion à trois de `replace_page` (Ruling R1231), par la route : Olga a changé un bloc depuis
/// la lecture de l'agent, qui récrit la page en ne changeant qu'un autre bloc ; les deux restent.
#[tokio::test]
async fn replace_page_keeps_what_a_human_changed_since_the_base() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Fusion", "Un\n\nDeux\n").await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let read = get_by(
        &t,
        &token,
        &format!("/api/v1/pages/{page}/document?anchors=block"),
    )
    .await
    .json();
    let base = read["revision"].as_str().unwrap().to_string();
    let anchored = read["mmd"].as_str().unwrap().to_string();
    team.olga
        .put_mmd(&page.to_string(), "Un d'Olga\n\nDeux\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let rewritten = anchored.replace("Deux ^", "Deux de l'agent ^");
    operations(
        &t,
        &token,
        page,
        json!({ "baseRevision": base, "operations": [{ "op": "replace_page", "markdown": rewritten }] }),
    )
    .await
    .assert_status(StatusCode::OK);
    assert_eq!(mmd_of(&team, page).await, "Un d'Olga\n\nDeux de l'agent\n");
}

/// Tant qu'une connexion de synchro qui écrit est ouverte sur la page, un jeton n'en récrit pas
/// tout le contenu (Ruling R1233) ; une opération ciblée passe.
#[tokio::test]
async fn replace_page_is_refused_while_someone_writes() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Occupée", "Un\n").await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let addr = t.listen().await;
    let _mona = synced_as(&team.mona.browser, &sync_url(addr, page)).await;
    let base = revision(&t, &token, page).await;
    operations(
        &t,
        &token,
        page,
        json!({ "baseRevision": base, "operations": [{ "op": "replace_page", "markdown": "Tout\n" }] }),
    )
    .await
    .assert_error(StatusCode::CONFLICT, "page_in_use");
    operations(
        &t,
        &token,
        page,
        json!({ "baseRevision": base, "operations": [{ "op": "str_replace", "old": "Un", "new": "Uno" }] }),
    )
    .await
    .assert_status(StatusCode::OK);
}

/// `PUT …/mmd` avec `If-Match` : la révision nommée est jugée dans la commande qui écrit ; à jour,
/// l'écriture passe et rend son `ETag` ; périmée, `409 revision_conflict`, rien d'écrit.
#[tokio::test]
async fn put_mmd_with_if_match() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Précondition", "Un\n").await;
    let id = page.to_string();
    let etag = team.olga.get_mmd(&id).await.headers[header::ETAG]
        .to_str()
        .unwrap()
        .to_string();
    let put = |mmd: &'static str, tag: String| {
        let by = &team.olga.browser;
        let uri = format!("/api/v1/pages/{id}/mmd");
        async move {
            by.send(
                Request::put(uri)
                    .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
                    .header(header::IF_MATCH, tag)
                    .body(Body::from(mmd))
                    .unwrap(),
            )
            .await
        }
    };
    let written = put("Deux\n", etag.clone()).await;
    written.assert_status(StatusCode::NO_CONTENT);
    let new_tag = written.headers[header::ETAG].to_str().unwrap().to_string();
    let stale = put("Trois\n", etag).await;
    let body = stale.assert_error(StatusCode::CONFLICT, "revision_conflict");
    assert_eq!(
        format!("\"{}\"", body["revision"].as_str().unwrap()),
        new_tag
    );
    assert_eq!(mmd_of(&team, page).await, "Deux\n");
    put("Trois\n", new_tag)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(mmd_of(&team, page).await, "Trois\n");
}

/// `PUT …/mmd` reste fermé aux jetons (Ruling R1222) : `403 token_not_allowed`, rien d'écrit.
#[tokio::test]
async fn put_mmd_is_closed_to_a_token() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Fermée", "Un\n").await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let reply = t
        .send(with_bearer(
            Request::put(format!("/api/v1/pages/{page}/mmd"))
                .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
                .body(Body::from("Deux\n"))
                .unwrap(),
            &token,
        ))
        .await;
    reply.assert_error(StatusCode::FORBIDDEN, "token_not_allowed");
    assert_eq!(mmd_of(&team, page).await, "Un\n");
}

/// Ruling R1356 : une écriture d'agent qui attend son tour d'écriture pendant qu'on révoque son
/// jeton n'arrive pas après la révocation : le jeton est relu dans la transaction qui écrit le
/// journal. Sans minutage (R886) : le test tient le tour du store, révoque par une connexion à
/// part, puis rend le tour.
#[tokio::test]
async fn a_write_queued_before_the_revocation_does_not_land_after_it() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Révoquée", "Un\n").await;
    let (token, token_id) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    t.hub.load_page(page).await.unwrap();
    let held = t.store.hold_writes().await;
    let router = t.router.clone();
    let request = with_bearer(
        json_request(
            Method::POST,
            &format!("/api/v1/pages/{page}/operations"),
            &json!({ "operations": [{ "op": "str_replace", "old": "Un", "new": "Uno" }] }),
        ),
        &token,
    );
    let mut request = request;
    request
        .headers_mut()
        .insert(header::HOST, "localhost".parse().unwrap());
    let write = tokio::spawn(async move {
        use tower::ServiceExt;
        router.oneshot(request).await.unwrap()
    });
    eventually("l'écriture d'agent n'attend pas son tour", || {
        t.store.queued_writes() == 1
    })
    .await;
    exec_sql(
        &t.db_path(),
        &format!("UPDATE api_tokens SET revoked_at = 1 WHERE id = '{token_id}'"),
    )
    .await;
    drop(held);
    let response = write.await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let by_token = query_i64(
        &t.db_path(),
        &format!("SELECT COUNT(*) FROM doc_updates WHERE token_id = '{token_id}'"),
    )
    .await;
    assert_eq!(by_token, 0, "l'écriture est arrivée après la révocation");
    assert_eq!(mmd_of(&team, page).await, "Un\n");
}

/// La part de tous les jetons d'un compte dans la file d'une page est de deux écritures (Rulings
/// R494, R1242) : trois écritures par jeton tenues de front, la troisième est refusée tout de suite
/// (`429 page_busy`).
#[tokio::test]
async fn the_third_write_of_an_account_by_token_is_busy() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Pleine", "Un\n\nDeux\n\nTrois\n").await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    t.hub.load_page(page).await.unwrap();
    let held = t.store.hold_writes().await;
    let send = |old: &str, new: &str| {
        let router = t.router.clone();
        let mut request = with_bearer(
            json_request(
                Method::POST,
                &format!("/api/v1/pages/{page}/operations"),
                &json!({ "operations": [{ "op": "str_replace", "old": old, "new": new }] }),
            ),
            &token,
        );
        request
            .headers_mut()
            .insert(header::HOST, "localhost".parse().unwrap());
        tokio::spawn(async move {
            use tower::ServiceExt;
            router.oneshot(request).await.unwrap().status()
        })
    };
    let first = send("Un", "Uno");
    eventually("la première n'attend pas son tour", || {
        t.store.queued_writes() == 1
    })
    .await;
    let (mut second, mut third) = (send("Deux", "Dos"), send("Trois", "Tres"));
    // L'une des deux prend la seconde place et attend le verrou de la page ; l'autre est refusée
    // sur-le-champ, avant que le tour soit rendu (une borne large, qui ne fait jamais passer le
    // test à tort : sans refus, rien ne finit avant le tour rendu).
    let (refused, was_second) = tokio::time::timeout(WAIT, async {
        tokio::select! {
            status = &mut second => (status.unwrap(), true),
            status = &mut third => (status.unwrap(), false),
        }
    })
    .await
    .expect("aucune des deux n'est refusée tant que le tour est tenu");
    assert_eq!(refused, StatusCode::TOO_MANY_REQUESTS);
    let pending = if was_second { third } else { second };
    drop(held);
    assert_eq!(first.await.unwrap(), StatusCode::OK);
    assert_eq!(pending.await.unwrap(), StatusCode::OK);
}

/// Ruling R1235 : aucune route de `TOKEN_ROUTES` qui reçoit un MMD ne laisse un jeton poser une
/// image, une vidéo ou un son à adresse web (opérations : `insert`, `replace_block`,
/// `replace_page` ; création en lot) ; une duplication et une restauration recopient ce que la
/// page portait, et passent.
#[tokio::test]
async fn no_token_route_adds_external_media() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Médias", "Un\n").await;
    let (token, _) = api_token(
        &t.store,
        team.olga.id,
        TokenLevel::Edit,
        TokenScope::All,
        true,
    )
    .await;
    let base = revision(&t, &token, page).await;
    let alias = alias_of(&aliases(&t, &token, page).await, "Un");
    for media in [
        "![](https://ailleurs.example/x.png?q=secret)\n",
        "<video src=\"https://ailleurs.example/x.mp4?q=secret\"/>\n",
        "<audio src=\"https://ailleurs.example/x.mp3?q=secret\"/>\n",
    ] {
        for op in [
            json!({ "op": "insert", "position": "end", "markdown": media }),
            json!({ "op": "replace_block", "anchor": alias, "markdown": media }),
            json!({ "op": "replace_page", "markdown": media }),
        ] {
            operations(
                &t,
                &token,
                page,
                json!({ "baseRevision": base, "operations": [op] }),
            )
            .await
            .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "external_media");
        }
        let batch = json_by(
            &t,
            &token,
            Method::POST,
            &format!("/api/v1/spaces/{}/pages/batch", team.space),
            json!({ "pages": [{ "title": "Neuve", "markdown": media }] }),
        )
        .await;
        let body = batch.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "external_media");
        assert_eq!(body["index"], 0, "{body}");
    }
    assert_eq!(mmd_of(&team, page).await, "Un\n");
    // Une page qui porte déjà une image du web (posée par la session) : la copier et la sortir de
    // la corbeille par un jeton passent.
    let pictured = page_with(&team, "Illustrée", "![](https://ailleurs.example/x.png)\n").await;
    json_by(
        &t,
        &token,
        Method::POST,
        &format!("/api/v1/pages/{pictured}/duplicate"),
        json!({}),
    )
    .await
    .assert_status(StatusCode::CREATED);
    team.trash(pictured).await;
    json_by(
        &t,
        &token,
        Method::POST,
        &format!("/api/v1/pages/{pictured}/restore"),
        json!({}),
    )
    .await
    .assert_status(StatusCode::OK);
}

/// Un bloc qui porte un fil de commentaires est un contenu protégé (Ruling R1571) : un agent ne le
/// retire que sous `allow_deleting_content`.
#[tokio::test]
async fn an_agent_does_not_remove_a_commented_block_without_the_option() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Commentée", "Un\n\nDeux\n").await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let blocks = t.hub.read_blocks(page).await.unwrap();
    let two = blocks
        .iter()
        .find(|b| mementum_md::block_text(b) == "Deux")
        .unwrap()
        .id
        .clone();
    team.olga
        .browser
        .post(
            &format!("/api/v1/pages/{page}/comments"),
            json!({ "body": "À garder", "blockId": two }),
        )
        .await
        .assert_status(StatusCode::CREATED);
    let base = revision(&t, &token, page).await;
    let alias = alias_of(&aliases(&t, &token, page).await, "Deux");
    let body = operations(
        &t,
        &token,
        page,
        json!({ "baseRevision": base, "operations": [{ "op": "delete_block", "anchor": alias }] }),
    )
    .await
    .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "operation_failed");
    assert_eq!(body["reason"], "protected_content", "{body}");
    assert_eq!(body["blocks"], json!([alias]), "{body}");
    assert_eq!(mmd_of(&team, page).await, "Un\n\nDeux\n");
    operations(
        &t,
        &token,
        page,
        json!({ "baseRevision": base, "operations": [{ "op": "delete_block", "anchor": alias, "allow_deleting_content": true }] }),
    )
    .await
    .assert_status(StatusCode::OK);
    assert_eq!(mmd_of(&team, page).await, "Un\n");
}

/// Un lot s'écrit tout entier, ou pas du tout (Ruling R1243) : un MMD illisible dans la troisième
/// page refuse tout le lot (`422 invalid_mmd`, son rang et sa ligne), rien n'est créé ; un lot
/// juste crée ses pages, dans l'ordre, avec leur contenu.
#[tokio::test]
async fn a_batch_is_written_whole_or_not_at_all() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let uri = format!("/api/v1/spaces/{}/pages/batch", team.space);
    let before = tree_nodes(&team.olga.browser, team.space).await.len();
    let refused = json_by(
        &t,
        &token,
        Method::POST,
        &uri,
        json!({ "pages": [
            { "title": "Une", "markdown": "Texte un\n" },
            { "title": "Deux", "markdown": "Texte deux\n" },
            { "title": "Trois", "markdown": "Texte\n\n\n\ntrois\n" },
        ]}),
    )
    .await;
    let body = refused.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_mmd");
    assert_eq!(body["index"], 2, "{body}");
    assert!(body["line"].as_u64().is_some(), "{body}");
    assert_eq!(
        tree_nodes(&team.olga.browser, team.space).await.len(),
        before
    );
    let created = json_by(
        &t,
        &token,
        Method::POST,
        &uri,
        json!({ "section": "space", "pages": [
            { "title": "Une", "markdown": "Texte un\n" },
            { "title": "Deux" },
        ]}),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    let pages = created.json();
    let pages = pages.as_array().unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0]["title"], "Une");
    assert_eq!(pages[1]["title"], "Deux");
    assert_eq!(pages[0]["section"], "space");
    let one = pages[0]["id"].as_str().unwrap();
    assert_eq!(team.olga.get_mmd(one).await.text(), "Texte un\n");
    let none = json_by(&t, &token, Method::POST, &uri, json!({ "pages": [] })).await;
    none.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "too_many_pages");
}

/// Les pages nées d'un lot, avec leur contenu, sont indexées comme toute page neuve (`mark_born`) :
/// la recherche les trouve.
#[tokio::test]
async fn pages_born_with_content_are_indexed() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    json_by(
        &t,
        &token,
        Method::POST,
        &format!("/api/v1/spaces/{}/pages/batch", team.space),
        json!({ "pages": [{ "title": "Versée", "markdown": "Le mot hellébore est ici.\n" }] }),
    )
    .await
    .assert_status(StatusCode::CREATED);
    t.indexer.settled().await;
    let found = json_by(
        &t,
        &token,
        Method::POST,
        &format!("/api/v1/spaces/{}/search", team.space),
        json!({ "q": "hellébore" }),
    )
    .await;
    found.assert_status(StatusCode::OK);
    let results = found.json();
    assert_eq!(results["results"][0]["title"], "Versée", "{results}");
}

/// Chaque appel par jeton laisse une ligne au journal d'activité, qui ne garde ni le texte cherché,
/// ni un MMD, ni un `old` ou un `new` (Ruling R1245) ; une écriture y laisse ses révisions et le
/// nombre de blocs touchés.
#[tokio::test]
async fn each_call_leaves_one_line_without_any_content() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = page_with(&team, "Journal", "Avant\n").await;
    let (token, token_id) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    get_by(&t, &token, &format!("/api/v1/pages/{page}/document"))
        .await
        .assert_status(StatusCode::OK);
    operations(
        &t,
        &token,
        page,
        json!({ "operations": [{ "op": "str_replace", "old": "Avant", "new": "Ancolie secrète" }] }),
    )
    .await
    .assert_status(StatusCode::OK);
    json_by(
        &t,
        &token,
        Method::POST,
        &format!("/api/v1/spaces/{}/search", team.space),
        json!({ "q": "edelweiss" }),
    )
    .await
    .assert_status(StatusCode::OK);
    json_by(
        &t,
        &token,
        Method::POST,
        &format!("/api/v1/spaces/{}/pages/batch", team.space),
        json!({ "pages": [{ "title": "Rose trémière", "markdown": "Bruyère cachée\n" }] }),
    )
    .await
    .assert_status(StatusCode::CREATED);
    t.calls.flush(&t.store).await;
    let lines = query_i64(
        &t.db_path(),
        &format!("SELECT COUNT(*) FROM token_calls WHERE token_id = '{token_id}'"),
    )
    .await;
    assert_eq!(lines, 4);
    let written = query_i64(
        &t.db_path(),
        &format!(
            "SELECT COUNT(*) FROM token_calls WHERE token_id = '{token_id}' \
             AND revision_before IS NOT NULL AND revision_after IS NOT NULL \
             AND revision_before <> revision_after AND blocks_touched = 1"
        ),
    )
    .await;
    assert_eq!(
        written, 1,
        "l'écriture garde ses révisions et ses blocs touchés"
    );
    let all: String = sqlx_dump(&t.db_path(), &token_id.to_string()).await;
    for secret in ["Ancolie", "Avant", "edelweiss", "Rose trémière", "Bruyère"] {
        assert!(!all.contains(secret), "« {secret} » est au journal : {all}");
    }
}

/// Le texte de chaque ligne du journal d'activité du jeton, colonnes jointes.
async fn sqlx_dump(db: &std::path::Path, token: &str) -> String {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT COALESCE(tool, '') || ' ' || COALESCE(code, '') || ' ' || \
                COALESCE(node_id, '') || ' ' || COALESCE(space_id, '') \
         FROM token_calls WHERE token_id = ?",
    )
    .bind(token)
    .fetch_all(&pool)
    .await
    .unwrap();
    pool.close().await;
    rows.into_iter()
        .map(|(r,)| r)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tour de correction de S7, sur le modèle de `a_copy_refused_at_its_turn_is_tried_three_times` :
/// un lot refusé à son tour d'écriture (ce sur quoi il a décidé ne tient plus) est repris ; deux
/// refus, il s'écrit en entier ; trois, `409 page_moved`, rien d'écrit.
#[tokio::test]
async fn a_batch_refused_at_its_turn_is_tried_three_times() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (token, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    let uri = format!("/api/v1/spaces/{}/pages/batch", team.space);
    t.store.refuse_next_copies(2);
    let created = json_by(
        &t,
        &token,
        Method::POST,
        &uri,
        json!({ "section": "space", "pages": [
            { "title": "Reprise", "markdown": "Texte repris\n" },
            { "title": "Reprise bis" },
        ]}),
    )
    .await;
    created.assert_status(StatusCode::CREATED);
    let pages = created.json();
    let one = pages[0]["id"].as_str().unwrap();
    assert_eq!(team.olga.get_mmd(one).await.text(), "Texte repris\n");
    let titles = || async {
        tree_nodes(&team.olga.browser, team.space)
            .await
            .into_iter()
            .filter_map(|n| n["title"].as_str().map(str::to_string))
            .collect::<Vec<_>>()
    };
    let now = titles().await;
    assert_eq!(now.iter().filter(|t| *t == "Reprise").count(), 1);
    assert_eq!(now.iter().filter(|t| *t == "Reprise bis").count(), 1);
    t.store.refuse_next_copies(3);
    json_by(
        &t,
        &token,
        Method::POST,
        &uri,
        json!({ "section": "space", "pages": [{ "title": "Jamais" }] }),
    )
    .await
    .assert_error(StatusCode::CONFLICT, "page_moved");
    assert!(!titles().await.iter().any(|t| t == "Jamais"));
}

/// Tour de correction de S7 : la destination d'un lot est jugée avant la forme de son corps ; un
/// jeton qui ne peut pas créer sous la page reçoit `403`, même si son corps nomme aussi une
/// section ; celui qui peut, `422`.
#[tokio::test]
async fn a_batch_judges_its_destination_before_its_section() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let parent = page_with(&team, "Parente", "Un\n").await;
    let uri = format!("/api/v1/spaces/{}/pages/batch", team.space);
    let body = json!({ "parentId": parent.to_string(), "section": "space", "pages": [{ "title": "Neuve" }] });
    let (reader, _) = token_of(&t, team.olga.id, TokenLevel::Read).await;
    json_by(&t, &reader, Method::POST, &uri, body.clone())
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    let (writer, _) = token_of(&t, team.olga.id, TokenLevel::Edit).await;
    json_by(&t, &writer, Method::POST, &uri, body)
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_request");
}
