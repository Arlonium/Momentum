//! Les routes des jetons d'API (lot 5, Task S6 ; Rulings R1213, R1218 à R1221, R1365 à R1371) :
//! créer (une session et le mot de passe), lister, révoquer, lire l'activité. Le texte d'un jeton
//! ne paraît qu'une fois, dans la réponse de la création ; une portée qu'on ne voit pas reçoit la
//! réponse d'une portée inconnue ; un jeton d'un autre compte, celle d'un jeton inconnu ; l'activité
//! et la liste ne nomment une page que si le compte la voit encore ; un jeton n'ouvre aucune de ces
//! routes.
//!
//! Les jetons de ces tests sont créés par le test, par la route ou par le store
//! ([`api_token`]) : jamais un vrai jeton.

mod common;

use std::path::Path;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::sync::{members, open_app, user};
use common::*;
use mementum_acl::Level;
use mementum_store::{TokenLevel, TokenScope};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

const TOKENS: &str = "/api/v1/tokens";

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Le corps d'une création de portée « tous les espaces », au niveau « Modification ».
fn all_scope(name: &str) -> Value {
    json!({ "name": name, "level": "edit", "scope": "all", "password": PASSWORD })
}

/// Statut et corps : deux refus pareils ont les deux.
fn refusal(reply: &Reply) -> (StatusCode, Value) {
    (reply.status, reply.json())
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

/// Le jeton créé par la route : son texte et sa ligne.
async fn created(by: &Browser<'_>, body: Value) -> (String, Value) {
    let reply = by.post(TOKENS, body).await;
    reply.assert_status(StatusCode::CREATED);
    let body = reply.json();
    (text(&body, "token").to_string(), body["info"].clone())
}

/// Chaque ligne de chaque table de la base, colonnes comprises (le modèle de
/// `tests/expected_account.rs`).
async fn dump(db: &Path) -> String {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    let tables: Vec<String> = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
         ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .iter()
    .map(|row| row.get(0))
    .collect();
    let mut all = String::new();
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
        for row in sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT {line} FROM \"{table}\""
        )))
        .fetch_all(&pool)
        .await
        .unwrap()
        {
            all.push_str(&format!("{table}: {}\n", row.get::<String, _>(0)));
        }
    }
    pool.close().await;
    all
}

/// Le SHA-256 du jeton, en hexadécimal minuscule, tel que la base le garde (le jeton sans son
/// préfixe, décodé de base64url).
fn hash_hex(token: &str) -> String {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let secret = token
        .strip_prefix("mmt_")
        .expect("un jeton commence par mmt_");
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(secret)
        .unwrap();
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Les titres de pages demandés au store de l'application depuis `since` (Ruling R1201).
fn titles_asked_since(t: &TestApp, since: usize) -> Vec<Uuid> {
    t.store.titles_asked()[since..].to_vec()
}

// ---------------------------------------------------------------------------
// Créer.

/// Le jeton n'est rendu qu'une fois : la création le donne (`mmt_` et 43 caractères), avec sa
/// ligne de la liste (ses quatre derniers caractères) ; la liste, ensuite, ne le porte plus, ni son
/// hachage, et le jeton ouvre ce que sa portée ouvre.
#[tokio::test]
async fn the_token_is_shown_once() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let (token, info) = created(&mona.browser, all_scope("  Claude Code  ")).await;
    assert!(token.starts_with("mmt_"), "{token}");
    assert_eq!(token.len(), 4 + 43);
    assert_eq!(info["name"], "Claude Code");
    assert_eq!(info["hint"], token[token.len() - 4..]);
    assert_eq!(info["level"], "edit");
    assert_eq!(info["scope"], "all");
    assert_eq!(info["space"], Value::Null);
    assert_eq!(info["pages"], json!([]));
    assert_eq!(info["canTrash"], false);
    assert_eq!(info["expiresAt"], Value::Null);
    assert_eq!(info["revokedAt"], Value::Null);
    assert_eq!(info["live"], true);

    let list = mona.browser.get(TOKENS).await;
    list.assert_status(StatusCode::OK);
    let text = list.text();
    let secret = &token["mmt_".len()..];
    for hidden in [token.as_str(), secret, &secret[..20], &hash_hex(&token)] {
        assert!(
            !text.contains(hidden),
            "« {hidden} » dans la liste : {text}"
        );
    }
    let rows = list.json();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(
        rows[0], info,
        "la ligne de la liste est celle de la création"
    );
    assert!(rows[0].get("token").is_none());

    // Le jeton vaut : il lit l'arbre de l'espace de Mona.
    get_by(&t, &token, &format!("/api/v1/spaces/{}/tree", mona.space))
        .await
        .assert_status(StatusCode::OK);
}

/// Un niveau autre que `read` ou `edit` est refusé (Ruling R1218 : pas d'accès complet par jeton),
/// et rien n'est créé ; `read` passe, et la ligne dit « read ».
#[tokio::test]
async fn a_level_full_is_refused() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    for level in ["full", "comment", "Edit", ""] {
        let reply = mona
            .browser
            .post(
                TOKENS,
                json!({ "name": "agent", "level": level, "scope": "all", "password": PASSWORD }),
            )
            .await;
        let body = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_level");
        // Les mots de l'écran, pas ceux de l'API (Ruling R1379).
        assert_french(&body, &["« Lecture »", "« Modification »"]);
    }
    assert!(t.store.tokens_of(mona.id, now()).await.unwrap().is_empty());
    let (_, info) = created(
        &mona.browser,
        json!({ "name": "lecteur", "level": "read", "scope": "all", "password": PASSWORD }),
    )
    .await;
    assert_eq!(info["level"], "read");
}

/// Le texte d'un jeton n'est gardé nulle part (la base relue en entier : ni lui, ni son secret,
/// seulement son hachage, qu'on y retrouve en témoin), et aucun journal du serveur ne le porte, ni
/// son hachage, au niveau `TRACE`, de sa création à son usage et à sa révocation (Review Focus 8).
#[tokio::test]
async fn the_token_text_is_never_stored_and_never_logged() {
    let logs = all_logs();
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let (token, info) = created(&mona.browser, all_scope("agent")).await;
    get_by(&t, &token, &format!("/api/v1/spaces/{}/tree", mona.space))
        .await
        .assert_status(StatusCode::OK);
    mona.browser.get(TOKENS).await.assert_status(StatusCode::OK);
    let id = text(&info, "id").to_string();
    mona.browser
        .get(&format!("{TOKENS}/{id}/calls"))
        .await
        .assert_status(StatusCode::OK);
    mona.browser
        .delete(&format!("{TOKENS}/{id}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    t.calls.flush(&t.store).await;
    t.indexer.settled().await;

    let secret = &token["mmt_".len()..];
    let hash = hash_hex(&token);
    let base = dump(&t.db_path()).await;
    for hidden in [token.as_str(), secret, &secret[..20]] {
        assert!(!base.contains(hidden), "« {hidden} » dans la base");
    }
    // Témoin : la base relue porte bien le jeton, par son hachage (`quote` l'écrit en majuscules).
    assert!(
        base.contains(&format!("X'{}'", hash.to_uppercase())),
        "le hachage du jeton est dans la base"
    );
    let journal = logs.text();
    assert!(
        journal.contains("jeton d'API créé") && journal.contains(&id),
        "le journal a été lu"
    );
    for hidden in [
        token.as_str(),
        secret,
        &secret[..20],
        &hash,
        &hash.to_uppercase(),
    ] {
        assert!(!journal.contains(hidden), "« {hidden} » dans le journal");
    }
}

/// Créer redemande le mot de passe (décision L2) : faux, `401 invalid_credentials`, absent, le
/// refus d'un corps sans lui ; rien n'est créé. Dix mots de passe faux ferment la création comme
/// la connexion (`429 too_many_attempts`, bon mot de passe compris), dans des fenêtres à elle
/// (Ruling R1366) : la connexion et le changement de mot de passe du compte n'en sont pas fermés.
#[tokio::test]
async fn creating_needs_the_password() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let wrong = mona
        .browser
        .post(
            TOKENS,
            json!({ "name": "agent", "level": "edit", "scope": "all", "password": "pas-le-bon" }),
        )
        .await;
    let body = wrong.assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    assert_french(&body, &["mot de passe"]);
    let missing = mona
        .browser
        .post(
            TOKENS,
            json!({ "name": "agent", "level": "edit", "scope": "all" }),
        )
        .await;
    assert!(
        missing.status.is_client_error() && missing.status != StatusCode::UNAUTHORIZED,
        "{}",
        missing.text()
    );
    assert!(missing.text().contains("password"), "{}", missing.text());
    assert!(t.store.tokens_of(mona.id, now()).await.unwrap().is_empty());

    // Neuf autres faux : la fenêtre est pleine, le bon mot de passe aussi est refusé.
    for _ in 0..9 {
        mona.browser
            .post(
                TOKENS,
                json!({ "name": "a", "level": "edit", "scope": "all", "password": "pas-le-bon" }),
            )
            .await
            .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    let closed = mona.browser.post(TOKENS, all_scope("agent")).await;
    closed.assert_error(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts");
    assert!(closed.headers.contains_key(header::RETRY_AFTER));
    assert!(t.store.tokens_of(mona.id, now()).await.unwrap().is_empty());
    // Ses propres fenêtres : la connexion passe, et le changement de mot de passe juge encore le
    // mot de passe (un faux y reçoit `401`, pas le `429` de la création).
    t.browser()
        .log_in("mona@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    mona.browser
        .change_password("pas-le-bon", "un-mot-de-passe-neuf")
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
}

/// Un corps mal formé est refusé avant le mot de passe, sans rien compter au frein : un nom vide,
/// trop long ou porteur d'un caractère de contrôle, une portée qui ne se lit pas, une échéance hors
/// de ses bornes ; après douze tels refus, une création juste passe du premier coup.
#[tokio::test]
async fn malformed_bodies_are_refused_before_the_password() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let space = mona.space.to_string();
    let page = Uuid::now_v7().to_string();
    let base = |patch: Value| {
        let mut body = all_scope("agent");
        body["password"] = json!("pas-le-bon");
        for (key, value) in patch.as_object().unwrap() {
            if value.is_null() {
                body.as_object_mut().unwrap().remove(key);
            } else {
                body[key] = value.clone();
            }
        }
        body
    };
    let cases = [
        (json!({ "name": "  " }), "invalid_name"),
        (json!({ "name": "x".repeat(61) }), "invalid_name"),
        (json!({ "name": "deux\nlignes" }), "invalid_name"),
        (json!({ "scope": "everything" }), "invalid_scope"),
        (json!({ "scope": "all", "spaceId": space }), "invalid_scope"),
        (json!({ "scope": "space" }), "invalid_scope"),
        (
            json!({ "scope": "space", "spaceId": space, "pageIds": [page] }),
            "invalid_scope",
        ),
        (
            json!({ "scope": "pages", "spaceId": space, "pageIds": [] }),
            "invalid_scope",
        ),
        (
            json!({ "scope": "pages", "pageIds": [page] }),
            "invalid_scope",
        ),
        (
            json!({
                "scope": "pages",
                "spaceId": space,
                "pageIds": (0..51).map(|_| Uuid::now_v7().to_string()).collect::<Vec<_>>(),
            }),
            "invalid_scope",
        ),
        (json!({ "expiresInDays": 0 }), "invalid_expiry"),
        (json!({ "expiresInDays": 3651 }), "invalid_expiry"),
    ];
    for (patch, code) in cases {
        let what = patch.to_string();
        let reply = mona.browser.post(TOKENS, base(patch)).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{what}");
        assert_eq!(reply.json()["error"], code, "{what} : {}", reply.text());
    }
    // Rien n'a été compté au frein : dix faux mots de passe l'auraient fermé.
    let (_, info) = created(
        &mona.browser,
        json!({
            "name": "agent", "level": "edit", "scope": "all", "password": PASSWORD,
            "expiresInDays": 3650,
        }),
    )
    .await;
    let expires = info["expiresAt"].as_i64().unwrap();
    let created_at = info["createdAt"].as_i64().unwrap();
    assert_eq!(expires - created_at, 3650 * 86_400_000);
    assert!(t.store.tokens_of(mona.id, now()).await.unwrap().len() == 1);
}

/// Une portée qu'on ne voit pas reçoit la réponse d'une portée inconnue, octet pour octet (Ruling
/// R1367) : un espace où l'on n'a aucun rôle, un espace inconnu, un identifiant mal écrit ; une
/// page privée d'un autre, une page inconnue, une page d'un autre espace, une page à la corbeille.
/// Rien n'est créé. Témoins : l'espace et une page qu'on voit passent, et l'invitée de l'espace y
/// prend un jeton de portée « espace ».
#[tokio::test]
async fn a_scope_one_does_not_see_is_refused_like_an_unknown_one() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let open = team
        .page(None, "Commune", &[members(Level::Edit)])
        .await
        .to_string();
    let private = team
        .page(None, "Privée de Tom", &[user(&team.tom, Level::Full)])
        .await
        .to_string();
    let trashed = team.page(None, "Jetée", &[members(Level::Edit)]).await;
    team.trash(trashed).await;
    let elsewhere = team
        .page_in(team.zoe.space, None, "Chez Zoé", &[members(Level::Full)])
        .await
        .to_string();
    let space = team.space.to_string();
    let scope_space = |space: &str| json!({ "name": "agent", "level": "read", "scope": "space", "spaceId": space, "password": PASSWORD });
    let scope_pages = |pages: &[&str]| json!({ "name": "agent", "level": "read", "scope": "pages", "spaceId": space, "pageIds": pages, "password": PASSWORD });

    let unknown = mona
        .browser
        .post(TOKENS, scope_space(&Uuid::now_v7().to_string()))
        .await;
    unknown.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_scope");
    for (what, body) in [
        ("espace de Zoé", scope_space(&team.zoe.space.to_string())),
        ("espace mal écrit", scope_space("pas-un-espace")),
        ("page privée de Tom", scope_pages(&[&open, &private])),
        (
            "page inconnue",
            scope_pages(&[&open, &Uuid::now_v7().to_string()]),
        ),
        ("page d'un autre espace", scope_pages(&[&elsewhere])),
        ("page à la corbeille", scope_pages(&[&trashed.to_string()])),
        ("page mal écrite", scope_pages(&["pas-une-page"])),
    ] {
        let reply = mona.browser.post(TOKENS, body).await;
        assert_eq!(refusal(&reply), refusal(&unknown), "{what}");
    }
    assert!(t.store.tokens_of(mona.id, now()).await.unwrap().is_empty());

    // Témoins : ce qu'elle voit passe.
    let (_, info) = created(&mona.browser, scope_space(&space)).await;
    assert_eq!(info["space"], json!({ "id": space, "name": "Équipe" }));
    let (_, info) = created(&mona.browser, scope_pages(&[&open, &open])).await;
    assert_eq!(info["pages"], json!([{ "id": open, "title": "Commune" }]));
    // Une invitée a un rôle : sa portée « espace » passe.
    created(&team.gina.browser, scope_space(&space)).await;
}

// ---------------------------------------------------------------------------
// Révoquer, l'activité.

/// Un jeton révoqué par la route ne vaut plus dès l'appel suivant (`401 invalid_token`) ; révoqué
/// deux fois, `204`, rien de plus n'est écrit ; il reste dans la liste, mort, avec sa date.
#[tokio::test]
async fn a_token_revoked_by_the_route_is_refused_from_the_next_call() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let (token, info) = created(&mona.browser, all_scope("agent")).await;
    let tree = format!("/api/v1/spaces/{}/tree", mona.space);
    get_by(&t, &token, &tree)
        .await
        .assert_status(StatusCode::OK);
    let uri = format!("{TOKENS}/{}", text(&info, "id"));
    mona.browser
        .delete(&uri)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    get_by(&t, &token, &tree)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_token");
    let revoked_at = mona.browser.get(TOKENS).await.json()[0]["revokedAt"].clone();
    assert!(revoked_at.is_i64());
    // La date de révocation reculée d'une minute : une seconde révocation qui la récrirait se
    // verrait, même dans la même milliseconde que la première.
    let past = revoked_at.as_i64().unwrap() - 60_000;
    exec_sql(
        &t.db_path(),
        &format!(
            "UPDATE api_tokens SET revoked_at = {past} WHERE id = '{}'",
            text(&info, "id")
        ),
    )
    .await;
    mona.browser
        .delete(&uri)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let rows = mona.browser.get(TOKENS).await.json();
    assert_eq!(rows[0]["revokedAt"], past, "rien de plus n'est écrit");
    assert_eq!(rows[0]["live"], false);
}

/// Révoque le jeton `id`, ou lit son activité, selon `what`, par `browser`.
async fn mine(browser: &Browser<'_>, what: &str, uri: &str, id: &str) -> Reply {
    let uri = uri.replace("{}", id);
    if what == "révoquer" {
        browser.delete(&uri).await
    } else {
        browser.get(&uri).await
    }
}

/// Le jeton d'un autre compte reçoit la réponse d'un jeton inconnu, octet pour octet, à la
/// révocation comme à l'activité, comme un identifiant mal écrit ; il vit toujours.
#[tokio::test]
async fn the_token_of_another_account_is_not_found() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let tom = t.account("Tom").await;
    let (_, info) = created(&tom.browser, all_scope("agent de Tom")).await;
    let toms = text(&info, "id").to_string();
    for (what, uri) in [
        ("révoquer", format!("{TOKENS}/{{}}")),
        ("l'activité", format!("{TOKENS}/{{}}/calls")),
    ] {
        let unknown = mine(&mona.browser, what, &uri, &Uuid::now_v7().to_string()).await;
        unknown.assert_error(StatusCode::NOT_FOUND, "not_found");
        for id in [toms.as_str(), "pas-un-jeton"] {
            let reply = mine(&mona.browser, what, &uri, id).await;
            assert_eq!(refusal(&reply), refusal(&unknown), "{what}, {id}");
        }
    }
    let id: Uuid = toms.parse().unwrap();
    assert!(t.store.token_is_live(id, now()).await.unwrap());
    // Une adresse d'activité qui ne se lit pas reçoit `400` avant tout `404`, pour le jeton d'un
    // autre compte comme pour le sien : le jeton n'est pas encore cherché (Ruling R1378).
    let (_, own) = created(&mona.browser, all_scope("agent de Mona")).await;
    for id in [toms.as_str(), text(&own, "id"), "pas-un-jeton"] {
        mona.browser
            .get(&format!("{TOKENS}/{id}/calls?outcome=tout"))
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid_request");
    }
}

/// Aucune route des jetons ne s'ouvre à un jeton, même porté avec le cookie vivant de son compte
/// et un corps juste (le bon mot de passe compris) : `403 token_not_allowed`, aucun jeton créé ni
/// révoqué, aucune ligne au journal d'activité.
#[tokio::test]
async fn no_token_route_opens_with_a_token() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let (token, id) = api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, true).await;
    let requests = [
        bare(Method::GET, TOKENS),
        json_request(Method::POST, TOKENS, &all_scope("par un jeton")),
        bare(Method::DELETE, &format!("{TOKENS}/{id}")),
        bare(Method::GET, &format!("{TOKENS}/{id}/calls")),
    ];
    for mut request in requests {
        let what = format!("{} {}", request.method(), request.uri());
        request.headers_mut().insert(
            header::COOKIE,
            mona.browser.cookie().unwrap().parse().unwrap(),
        );
        let reply = t.send(with_bearer(request, &token)).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{what}");
        reply.assert_error(StatusCode::FORBIDDEN, "token_not_allowed");
    }
    let tokens = t.store.tokens_of(mona.id, now()).await.unwrap();
    assert_eq!(tokens.len(), 1, "aucun jeton créé par un jeton");
    assert_eq!(tokens[0].revoked_at, None, "ni révoqué");
    t.calls.flush(&t.store).await;
    assert!(
        t.store
            .calls_of(mona.id, id, None, None, 10, now())
            .await
            .unwrap()
            .is_empty()
    );
    // Témoin : la session seule les ouvre.
    mona.browser.get(TOKENS).await.assert_status(StatusCode::OK);
}

/// L'activité ne nomme une page que si le compte la voit encore (Ruling R1368) : le titre d'une
/// page qu'il voit est rendu, celui d'une page privée d'un autre que le jeton a essayée n'est
/// jamais demandé au store (le compteur des titres demandés, R1201), pas plus que celui d'une page
/// dont l'accès vient d'être retiré ; son identifiant, que le jeton avait envoyé, reste.
#[tokio::test]
async fn the_activity_never_names_a_page_one_no_longer_sees() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let seen = team.page(None, "Notes", &[members(Level::Edit)]).await;
    let hidden = team
        .page(None, "Privée de Tom", &[user(&team.tom, Level::Full)])
        .await;
    let (token, id) = api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    get_by(&t, &token, &format!("/api/v1/pages/{seen}"))
        .await
        .assert_status(StatusCode::OK);
    get_by(&t, &token, &format!("/api/v1/pages/{hidden}"))
        .await
        .assert_status(StatusCode::NOT_FOUND);
    t.calls.flush(&t.store).await;
    let activity = format!("{TOKENS}/{id}/calls");

    let before = t.store.titles_asked().len();
    let reply = mona.browser.get(&activity).await;
    reply.assert_status(StatusCode::OK);
    let calls = reply.json()["calls"].as_array().unwrap().clone();
    assert_eq!(calls.len(), 2, "{calls:?}");
    // Le plus récent d'abord.
    assert_eq!(calls[0]["pageId"], hidden.to_string());
    assert_eq!(calls[0]["pageTitle"], Value::Null);
    assert_eq!(calls[0]["outcome"], "refused");
    assert_eq!(calls[0]["code"], "not_found");
    assert_eq!(calls[1]["pageId"], seen.to_string());
    assert_eq!(calls[1]["pageTitle"], "Notes");
    assert_eq!(calls[1]["outcome"], "ok");
    assert_eq!(calls[1]["tool"], "GET /api/v1/pages/{id}");
    assert_eq!(calls[1]["surface"], "rest");
    let asked = titles_asked_since(&t, before);
    assert!(asked.contains(&seen), "{asked:?}");
    assert!(!asked.contains(&hidden), "titre demandé d'une page non vue");

    // L'accès de Mona à « Notes » retiré : plus de titre, et rien n'est demandé.
    exec_sql(
        &t.db_path(),
        &format!("DELETE FROM acl WHERE node_id = '{seen}'"),
    )
    .await;
    exec_sql(
        &t.db_path(),
        &format!(
            "UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = '{}'",
            team.space
        ),
    )
    .await;
    let before = t.store.titles_asked().len();
    let calls = mona.browser.get(&activity).await.json()["calls"].clone();
    assert_eq!(calls[1]["pageId"], seen.to_string());
    assert_eq!(calls[1]["pageTitle"], Value::Null);
    assert_eq!(titles_asked_since(&t, before), Vec::<Uuid>::new());
}

/// La liste ne nomme que ce que le compte voit encore (Ruling R1368) : le titre d'une page de la
/// portée tant qu'il la voit, le nom de l'espace tant qu'il y a un rôle.
#[tokio::test]
async fn the_list_names_only_what_the_account_still_sees() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let page = team.page(None, "Notes", &[members(Level::Edit)]).await;
    let space = team.space.to_string();
    created(
        &mona.browser,
        json!({
            "name": "agent", "level": "read", "scope": "pages", "spaceId": space,
            "pageIds": [page.to_string()], "password": PASSWORD,
        }),
    )
    .await;
    let row = &mona.browser.get(TOKENS).await.json()[0];
    assert_eq!(row["space"], json!({ "id": space, "name": "Équipe" }));
    assert_eq!(
        row["pages"],
        json!([{ "id": page.to_string(), "title": "Notes" }])
    );

    let bump = format!(
        "UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = '{}'",
        team.space
    );
    exec_sql(
        &t.db_path(),
        &format!("DELETE FROM acl WHERE node_id = '{page}'"),
    )
    .await;
    exec_sql(&t.db_path(), &bump).await;
    let before = t.store.titles_asked().len();
    let row = &mona.browser.get(TOKENS).await.json()[0];
    assert_eq!(
        row["pages"],
        json!([{ "id": page.to_string(), "title": null }])
    );
    assert!(!titles_asked_since(&t, before).contains(&page));

    exec_sql(
        &t.db_path(),
        &format!(
            "DELETE FROM members WHERE space_id = '{space}' AND user_id = '{}'",
            mona.id
        ),
    )
    .await;
    exec_sql(&t.db_path(), &bump).await;
    let row = &mona.browser.get(TOKENS).await.json()[0];
    assert_eq!(row["space"], json!({ "id": space, "name": null }));
}

/// L'activité se lit par pages de cinquante, de la plus récente ligne à la plus ancienne (`more`,
/// puis `before`), par issue, avec les refus du débit comptés et les lignes purgées ; une adresse
/// qui ne se lit pas reçoit `400 invalid_request`.
#[tokio::test]
async fn the_activity_is_read_page_by_page_and_by_outcome() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = mementum_server::SignupMode::Open;
        // Cinquante-huit appels admis, le cinquante-neuvième refusé par le débit.
        cfg.token_calls_per_min = 58;
        cfg.account_token_calls_per_min = 58;
    })
    .await;
    let mona = t.account("Mona").await;
    let (token, id) = api_token(&t.store, mona.id, TokenLevel::Read, TokenScope::All, false).await;
    let tree = format!("/api/v1/spaces/{}/tree", mona.space);
    for _ in 0..55 {
        get_by(&t, &token, &tree)
            .await
            .assert_status(StatusCode::OK);
    }
    for _ in 0..3 {
        get_by(&t, &token, &format!("/api/v1/pages/{}", Uuid::now_v7()))
            .await
            .assert_status(StatusCode::NOT_FOUND);
    }
    // Au-delà du débit : comptés, sans ligne.
    get_by(&t, &token, &tree)
        .await
        .assert_error(StatusCode::TOO_MANY_REQUESTS, "token_busy");
    t.calls.flush(&t.store).await;

    let uri = format!("{TOKENS}/{id}/calls");
    let first = mona.browser.get(&uri).await.json();
    let calls = first["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 50);
    assert_eq!(first["more"], true);
    assert_eq!(first["busyCount"], 1);
    assert_eq!(first["purged"], 0);
    assert_eq!(calls[0]["outcome"], "refused");
    assert_eq!(calls[0]["spaceId"], Value::Null);
    let ids: Vec<i64> = calls.iter().map(|c| c["id"].as_i64().unwrap()).collect();
    assert!(
        ids.windows(2).all(|w| w[0] > w[1]),
        "du plus récent au plus ancien"
    );
    let last = ids[49];
    let rest = mona
        .browser
        .get(&format!("{uri}?before={last}"))
        .await
        .json();
    assert_eq!(rest["calls"].as_array().unwrap().len(), 8);
    assert_eq!(rest["more"], false);
    assert_eq!(rest["calls"][0]["spaceId"], mona.space.to_string());

    let refused = mona
        .browser
        .get(&format!("{uri}?outcome=refused"))
        .await
        .json();
    assert_eq!(refused["calls"].as_array().unwrap().len(), 3);
    assert_eq!(refused["more"], false);
    let ok = mona.browser.get(&format!("{uri}?outcome=ok")).await.json();
    assert_eq!(ok["calls"].as_array().unwrap().len(), 50);
    assert_eq!(ok["more"], true);
    assert!(
        ok["calls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["outcome"] == "ok")
    );
    for bad in ["?outcome=tout", "?before=hier", "?apres=3"] {
        mona.browser
            .get(&format!("{uri}{bad}"))
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid_request");
    }
}

// ---------------------------------------------------------------------------
// Le mot de passe changé.

/// Changer de mot de passe dit combien de jetons vivants il a révoqués (Ruling R1221), ceux de
/// la route comme ceux d'un autre chemin, pas un jeton déjà révoqué ; chacun reçoit `401
/// invalid_token` à l'appel suivant.
#[tokio::test]
async fn changing_the_password_says_how_many_tokens_went() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let (by_route, _) = created(&mona.browser, all_scope("par la route")).await;
    let (by_store, _) =
        api_token(&t.store, mona.id, TokenLevel::Read, TokenScope::All, false).await;
    let (_, revoked) = created(&mona.browser, all_scope("déjà révoqué")).await;
    mona.browser
        .delete(&format!("{TOKENS}/{}", text(&revoked, "id")))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let tree = format!("/api/v1/spaces/{}/tree", mona.space);
    for token in [&by_route, &by_store] {
        get_by(&t, token, &tree).await.assert_status(StatusCode::OK);
    }

    let reply = mona
        .browser
        .change_password(PASSWORD, "un-mot-de-passe-neuf")
        .await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(reply.json(), json!({ "tokensRevoked": 2 }));
    assert!(reply.session_set_cookie().contains("mementum_session="));
    for token in [&by_route, &by_store] {
        get_by(&t, token, &tree)
            .await
            .assert_error(StatusCode::UNAUTHORIZED, "invalid_token");
    }
    // Plus rien à révoquer : la réponse le dit.
    let again = mona
        .browser
        .change_password("un-mot-de-passe-neuf", "encore-un-mot-de-passe")
        .await;
    again.assert_status(StatusCode::OK);
    assert_eq!(again.json(), json!({ "tokensRevoked": 0 }));
}

// ---------------------------------------------------------------------------
// Le tour de correction (R1375 à R1379).

/// Attend `ready` ([`WAIT`] au plus) : une condition, pas une durée.
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

/// Le mot de passe d'un compte qu'un autre changement remplace pendant la création.
const DONOR_PASSWORD: &str = "un-mot-de-passe-donneur";

/// Une création vérifiée contre l'ancien mot de passe, qui écrit après un changement de mot de
/// passe (R120 ; le modèle de `password_change_verified_before_another_one_changes_nothing`,
/// `tests/auth.rs`) : aucun jeton n'est créé, la réponse est celle d'un mot de passe faux, et la
/// tentative compte au frein de la création. Sans la condition du hachage vérifié dans la
/// transaction, un jeton créé contre l'ancien mot de passe survivrait au changement qui devait tous
/// les révoquer.
#[tokio::test]
async fn a_creation_verified_before_a_password_change_creates_nothing() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    // Bob, inscrit avec le mot de passe que le changement donne à Alice : son hachage sert.
    let donor = t.browser();
    let reply = donor
        .sign_up("bob@exemple.fr", "Bob", DONOR_PASSWORD, None)
        .await;
    reply.assert_status(StatusCode::CREATED);
    let bob = uuid(&reply.json()["user"]["id"]);
    // Ce que répond un mot de passe faux : un échec au frein.
    let mut wrong_body = all_scope("agent");
    wrong_body["password"] = json!("pas-le-bon-mot-de-passe");
    let wrong = alice.browser.post(TOKENS, wrong_body.clone()).await;
    wrong.assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");

    let replaced = [
        format!(
            "UPDATE users SET password_hash = (SELECT password_hash FROM users WHERE id = \
             '{bob}') WHERE id = '{}'",
            alice.id
        ),
        format!("DELETE FROM sessions WHERE user_id = '{}'", alice.id),
    ];
    let raced = overtaken(
        &t,
        alice.space,
        alice.browser.post(TOKENS, all_scope("agent")),
        &replaced,
    )
    .await;

    // La création avait bien vérifié l'ancien mot de passe ; c'est à l'écriture qu'elle est refusée.
    assert!(
        raced
            .journal
            .contains("mot de passe changé pendant sa vérification"),
        "réponse {} : {}\njournal : {}",
        raced.reply.status,
        raced.reply.text(),
        raced.journal
    );
    assert!(
        !raced
            .journal
            .contains("création d'un jeton d'API refusée : mot de passe invalide"),
        "la création est arrivée après le changement : {}",
        raced.journal
    );
    assert_eq!(refusal(&raced.reply), refusal(&wrong));
    assert!(t.store.tokens_of(alice.id, now()).await.unwrap().is_empty());

    // Elle compte comme un échec : avec celui d'avant et huit autres, la fenêtre est pleine, et
    // même le bon mot de passe (le nouveau) est refusé.
    let again = t.browser();
    again
        .log_in("alice@exemple.fr", DONOR_PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    for attempt in 1..=8 {
        let reply = again.post(TOKENS, wrong_body.clone()).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "essai {attempt}");
    }
    let mut right = all_scope("agent");
    right["password"] = json!(DONOR_PASSWORD);
    again
        .post(TOKENS, right)
        .await
        .assert_error(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts");
    assert!(t.store.tokens_of(alice.id, now()).await.unwrap().is_empty());
}

/// L'activité ne nomme pas une page d'un espace où le compte n'a plus de rôle (Ruling R1368) :
/// sorti de l'espace, l'appel garde l'identifiant de la page, sans titre, et ce titre n'est pas
/// demandé au store.
#[tokio::test]
async fn the_activity_never_names_a_page_of_a_space_the_account_left() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let page = team.page(None, "Notes", &[members(Level::Edit)]).await;
    let (token, id) = api_token(&t.store, mona.id, TokenLevel::Read, TokenScope::All, false).await;
    get_by(&t, &token, &format!("/api/v1/pages/{page}"))
        .await
        .assert_status(StatusCode::OK);
    t.calls.flush(&t.store).await;
    let activity = format!("{TOKENS}/{id}/calls");
    // Témoin : membre, elle lit le titre.
    let calls = mona.browser.get(&activity).await.json()["calls"].clone();
    assert_eq!(calls[0]["pageTitle"], "Notes");

    exec_sql(
        &t.db_path(),
        &format!(
            "DELETE FROM members WHERE space_id = '{}' AND user_id = '{}'",
            team.space, mona.id
        ),
    )
    .await;
    exec_sql(
        &t.db_path(),
        &format!(
            "UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = '{}'",
            team.space
        ),
    )
    .await;
    let before = t.store.titles_asked().len();
    let calls = mona.browser.get(&activity).await.json()["calls"].clone();
    assert_eq!(calls[0]["pageId"], page.to_string());
    assert_eq!(calls[0]["pageTitle"], Value::Null);
    assert_eq!(titles_asked_since(&t, before), Vec::<Uuid>::new());
}

/// Une révocation dont le client part pendant qu'elle attend son tour d'écriture va au bout (Ruling
/// R1376, comme la déconnexion) : la file tenue, la requête abandonnée, puis la file rendue, le
/// jeton est révoqué.
#[tokio::test]
async fn a_revocation_whose_client_left_still_lands() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let (_, id) = api_token(&t.store, mona.id, TokenLevel::Edit, TokenScope::All, false).await;
    t.indexer.settled().await;
    t.indexer.freeze();
    let held = t.store.hold_writes().await;
    let mut request = bare(Method::DELETE, &format!("{TOKENS}/{id}"));
    request
        .headers_mut()
        .insert(header::HOST, "localhost".parse().unwrap());
    request.headers_mut().insert(
        header::COOKIE,
        mona.browser.cookie().unwrap().parse().unwrap(),
    );
    let router = t.router.clone();
    let asked = tokio::spawn(async move {
        use tower::ServiceExt;
        router.oneshot(request).await
    });
    until("la révocation attend son tour", || {
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
    while t.store.token_is_live(id, now()).await.unwrap() {
        assert!(
            std::time::Instant::now() < deadline,
            "le jeton vit encore : la révocation n'est pas écrite"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

/// Un changement de mot de passe réussi rouvre la fenêtre du frein de la création (Ruling R1377) :
/// neuf mots de passe faux à la création, le mot de passe changé, puis neuf autres et une création
/// juste, qui passe ; sans la remise à zéro, la fenêtre fermait au dixième.
#[tokio::test]
async fn a_password_change_reopens_the_token_creation_window() {
    let t = open_app().await;
    let mona = t.account("Mona").await;
    let mut wrong = all_scope("agent");
    wrong["password"] = json!("pas-le-bon-mot-de-passe");
    for attempt in 1..=9 {
        let reply = mona.browser.post(TOKENS, wrong.clone()).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "essai {attempt}");
    }
    mona.browser
        .change_password(PASSWORD, "un-mot-de-passe-neuf")
        .await
        .assert_status(StatusCode::OK);
    for attempt in 1..=9 {
        let reply = mona.browser.post(TOKENS, wrong.clone()).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "essai {attempt}");
    }
    let mut right = all_scope("agent");
    right["password"] = json!("un-mot-de-passe-neuf");
    mona.browser
        .post(TOKENS, right)
        .await
        .assert_status(StatusCode::CREATED);
}
