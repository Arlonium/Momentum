//! Les fichiers d'une page, par l'API (Rulings R245 à R247, Task S4a) : qui envoie et qui lit, à
//! chaque niveau, à la corbeille ou non ; un fichier ne se lit que par sa page ; ce qui est servi
//! (toujours un téléchargement, jamais un document) ; les plages et les revalidations ; les bornes
//! d'un envoi.
//!
//! Le droit qui change pendant un envoi est éprouvé dans `tests/sync_rights.rs`, avec les autres
//! contrôles refaits après une attente ; les routes des fichiers d'une page qu'on ne voit pas,
//! dans `foreign_page_is_indistinguishable_from_missing` (`tests/permissions.rs`) ; la mémoire
//! d'un envoi et d'une lecture, seule dans `tests/files_alloc.rs`.

mod common;

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use common::sync::{members, open_app, user};
use common::{
    Browser, Logs, RawUpload, Reply, Team, TestApp, outcome, query_i64, temporaries, upload_uri,
};
use mementum_acl::Level;
use mementum_server::SignupMode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Connection;
use uuid::Uuid;

/// Un vrai PNG, d'un pixel.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// Du HTML qui exécuterait un script, ouvert comme document depuis l'origine de l'application.
const HTML: &[u8] = b"<!doctype html><script>fetch('/api/v1/me')</script>";

/// Un SVG qui exécuterait un script, de même.
const SVG: &[u8] =
    b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"fetch('/api/v1/me')\"><script>1</script></svg>";

/// Le début d'un PDF.
const PDF: &[u8] = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n1 0 obj\n<<>>\nendobj\n";

/// Les niveaux d'accès, du plus bas au plus haut.
const LEVELS: [Level; 4] = [Level::Read, Level::Comment, Level::Edit, Level::Full];

/// La politique de contenu de tout fichier servi (Ruling R246).
const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

/// Envoie `bytes` dans la page `page` sous le nom `name`, avec la session de `by`.
async fn upload(by: &Browser<'_>, page: Uuid, name: &str, bytes: impl Into<Body>) -> Reply {
    by.send(
        Request::post(upload_uri(page, name))
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(bytes.into())
            .unwrap(),
    )
    .await
}

/// Comme [`upload`], pour un envoi qui doit aboutir : `201`, et le `FileInfo` rendu.
async fn uploaded(by: &Browser<'_>, page: Uuid, name: &str, bytes: &[u8]) -> Value {
    let reply = upload(by, page, name, bytes.to_vec()).await;
    reply.assert_status(StatusCode::CREATED);
    reply.json()
}

/// Lit l'adresse `url` d'un fichier, avec ces en-têtes en plus.
async fn read(by: &Browser<'_>, url: &str, headers: &[(header::HeaderName, &str)]) -> Reply {
    let mut request = Request::get(url);
    for (name, value) in headers {
        request = request.header(name, *value);
    }
    by.send(request.body(Body::empty()).unwrap()).await
}

/// L'adresse d'un fichier d'une page.
fn file_uri(page: impl std::fmt::Display, file: impl std::fmt::Display) -> String {
    format!("/api/v1/pages/{page}/files/{file}")
}

/// `"<sha-256 en hexadécimal>"` : l'`ETag` d'un contenu.
fn etag(bytes: &[u8]) -> String {
    let hash: [u8; 32] = Sha256::digest(bytes).into();
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("\"{hex}\"")
}

/// La seule valeur de l'en-tête `name` de la réponse : deux valeurs font échouer le test.
fn only(reply: &Reply, name: &str) -> String {
    let values: Vec<_> = reply.headers.get_all(name).iter().collect();
    assert_eq!(values.len(), 1, "une seule valeur de {name} : {values:?}");
    values[0].to_str().unwrap().to_string()
}

/// Les en-têtes d'un fichier servi, sauf `Content-Length` et `Content-Range` : exactement ceux de
/// Ruling R246, sur un `200`, un `206` et un `304`.
fn assert_served_headers(reply: &Reply, disposition: &str, mime: &str, bytes: &[u8], what: &str) {
    for (name, expected) in [
        ("content-disposition", disposition),
        ("content-type", mime),
        ("x-content-type-options", "nosniff"),
        ("content-security-policy", CSP),
        ("cross-origin-resource-policy", "same-origin"),
        ("accept-ranges", "bytes"),
        ("cache-control", "private, no-cache"),
        ("etag", &etag(bytes)),
    ] {
        assert_eq!(only(reply, name), expected, "{what} : {name}");
    }
}

/// Le nombre d'attaches en base, toutes pages confondues.
async fn attachments(t: &TestApp) -> i64 {
    query_i64(&t.db_path(), "SELECT COUNT(*) FROM attachments").await
}

/// Le chemin du contenu `bytes` dans le dépôt : `files/<2 hex>/<62 hex>`.
fn content_path(t: &TestApp, bytes: &[u8]) -> PathBuf {
    let hex = etag(bytes).trim_matches('"').to_string();
    t.cfg.data_dir.join("files").join(&hex[..2]).join(&hex[2..])
}

/// Une application où tout le monde s'inscrit, et dont la borne d'un fichier est de 1 Mio.
async fn small_app() -> TestApp {
    TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.max_file_bytes = 1 << 20;
    })
    .await
}

/// Envoyer demande « Peut modifier » (`403` en dessous, qui le nomme), hors corbeille (`409
/// trashed`, après le refus de niveau) ; lire demande « Peut lire », corbeille comprise. Sans
/// session : `401` sur les deux.
#[tokio::test]
async fn file_routes_follow_the_levels() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let tom = &team.tom;
    for trashed in [false, true] {
        for level in LEVELS {
            let what = format!("{level:?}, corbeille : {trashed}");
            let grants = [user(&team.olga, Level::Full), user(tom, level)];
            let page = team.page(None, &what, &grants).await;
            let file = uploaded(&team.olga.browser, page, "photo.png", PNG).await;
            if trashed {
                team.trash(page).await;
            }
            let url = file["url"].as_str().unwrap();

            let served = read(&tom.browser, url, &[]).await;
            served.assert_status(StatusCode::OK);
            assert_eq!(served.body.as_ref(), PNG, "{what}");

            let sent = upload(&tom.browser, page, "autre.png", PNG.to_vec()).await;
            if level < Level::Edit {
                let refusal = sent.assert_error(StatusCode::FORBIDDEN, "forbidden");
                let message = refusal["message"].as_str().unwrap();
                assert!(message.contains("« Peut modifier »"), "{what} : {message}");
            } else if trashed {
                sent.assert_error(StatusCode::CONFLICT, "trashed");
            } else {
                sent.assert_status(StatusCode::CREATED);
                let info = sent.json();
                let id = info["id"].as_str().unwrap();
                assert_eq!(info["url"], file_uri(page, id), "{what}");
                assert_eq!(info["name"], "autre.png", "{what}");
                assert_eq!(info["size"], PNG.len(), "{what}");
                assert_eq!(info["mime"], "image/png", "{what}");
                let mut keys: Vec<_> = info.as_object().unwrap().keys().cloned().collect();
                keys.sort();
                assert_eq!(keys, ["id", "mime", "name", "size", "url"], "{what}");
            }
            let expected = 1 + i64::from(level >= Level::Edit && !trashed);
            let sql = format!("SELECT COUNT(*) FROM attachments WHERE node_id = '{page}'");
            assert_eq!(query_i64(&t.db_path(), &sql).await, expected, "{what}");
        }
    }
    // Sans session.
    let page = team
        .page(None, "Sans session", &[members(Level::Full)])
        .await;
    let file = uploaded(&team.mona.browser, page, "photo.png", PNG).await;
    let nobody = t.browser();
    read(&nobody, file["url"].as_str().unwrap(), &[])
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    upload(&nobody, page, "x.png", PNG.to_vec())
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
}

/// Un fichier ne se lit que par sa page : celui d'une autre page (que la personne lit pourtant),
/// un identifiant inconnu ou mal formé reçoivent exactement la réponse d'une page introuvable.
#[tokio::test]
async fn file_of_another_page_is_not_found() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona.browser;
    let here = team.page(None, "Ici", &[members(Level::Full)]).await;
    let there = team.page(None, "Là", &[members(Level::Full)]).await;
    let theirs = uploaded(mona, there, "là.png", PNG).await;
    let theirs_id = theirs["id"].as_str().unwrap();
    // La page d'ici porte aussi un fichier : sa page n'est pas vide de fichiers.
    uploaded(mona, here, "ici.png", PDF).await;

    let unknown = Uuid::now_v7();
    let missing = read(mona, &file_uri(unknown, theirs_id), &[]).await;
    let body = missing.assert_error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(body["message"], "page introuvable");
    for file in [
        theirs_id.to_string(),
        Uuid::now_v7().to_string(),
        "pas-un-fichier".to_string(),
    ] {
        for headers in [&[][..], &[(header::RANGE, "bytes=0-0")][..]] {
            let reply = read(mona, &file_uri(here, &file), headers).await;
            assert_eq!(outcome(&reply), outcome(&missing), "{file}, {headers:?}");
        }
    }
    // Lu par sa page, il se lit.
    let served = read(mona, theirs["url"].as_str().unwrap(), &[]).await;
    served.assert_status(StatusCode::OK);
    assert_eq!(served.body.as_ref(), PNG);
}

/// Une revalidation est une lecture : `If-None-Match` qui nomme l'`ETag` rend `304`, sans corps,
/// avec les en-têtes d'un fichier servi, à qui lit la page ; l'accès retiré, la même requête rend
/// la réponse d'une page introuvable. Témoin : un autre `ETag` rend le fichier.
#[tokio::test]
async fn revalidation_is_checked_like_a_read() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let tom = &team.tom;
    let grants = [user(&team.olga, Level::Full), user(tom, Level::Read)];
    let page = team.page(None, "Revalidée", &grants).await;
    let file = uploaded(&team.olga.browser, page, "photo.png", PNG).await;
    let url = file["url"].as_str().unwrap();
    let disposition = "attachment; filename=\"photo.png\"; filename*=UTF-8''photo.png";

    for matching in [
        etag(PNG),
        format!("W/{}", etag(PNG)),
        format!("\"autre\", {}", etag(PNG)),
        "*".to_string(),
    ] {
        let fresh = read(&tom.browser, url, &[(header::IF_NONE_MATCH, &matching)]).await;
        fresh.assert_status(StatusCode::NOT_MODIFIED);
        assert!(
            fresh.body.is_empty(),
            "{matching} : un 304 n'a pas de corps"
        );
        assert_served_headers(&fresh, disposition, "image/png", PNG, &matching);
    }
    // Témoin : un autre `ETag`, le fichier entier.
    let stale = read(&tom.browser, url, &[(header::IF_NONE_MATCH, "\"autre\"")]).await;
    stale.assert_status(StatusCode::OK);
    assert_eq!(stale.body.as_ref(), PNG);

    // L'accès retiré, la revalidation ne dit plus rien du fichier.
    let removed = team
        .olga
        .browser
        .delete(&format!("/api/v1/pages/{page}/access/users/{}", tom.id))
        .await;
    assert_eq!(removed.status, StatusCode::OK, "{}", removed.text());
    let missing = read(&tom.browser, &file_uri(Uuid::now_v7(), Uuid::now_v7()), &[]).await;
    missing.assert_error(StatusCode::NOT_FOUND, "not_found");
    let gone = read(&tom.browser, url, &[(header::IF_NONE_MATCH, &etag(PNG))]).await;
    assert_eq!(outcome(&gone), outcome(&missing));
}

/// Preuve 1 du plan : un fichier dont le type ment n'est jamais rendu comme un document. Pour un
/// vrai PNG, un HTML nommé `x.png` et déclaré `image/png`, un SVG à script, un PDF et des octets
/// quelconques, la réponse porte exactement les en-têtes de Ruling R246, et le type que le
/// serveur a lu dans les octets (jamais celui que le navigateur a déclaré). Un nom à guillemets
/// et à accents donne un en-tête lisible.
#[tokio::test]
async fn download_headers_never_let_a_file_run() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Fichiers" })).await;
    let page = Uuid::parse_str(&page.id).unwrap();
    let cases: [(&str, &[u8], &str, &str, &str); 6] = [
        (
            "photo.png",
            PNG,
            "image/png",
            "image/png",
            "attachment; filename=\"photo.png\"; filename*=UTF-8''photo.png",
        ),
        (
            "x.png",
            HTML,
            "image/png",
            "application/octet-stream",
            "attachment; filename=\"x.png\"; filename*=UTF-8''x.png",
        ),
        (
            "dessin.svg",
            SVG,
            "image/svg+xml",
            "application/octet-stream",
            "attachment; filename=\"dessin.svg\"; filename*=UTF-8''dessin.svg",
        ),
        (
            "rapport.pdf",
            PDF,
            "application/pdf",
            "application/pdf",
            "attachment; filename=\"rapport.pdf\"; filename*=UTF-8''rapport.pdf",
        ),
        (
            "données.bin",
            b"\x00\x01\x02 des octets quelconques \xff\xfe",
            "text/html",
            "application/octet-stream",
            "attachment; filename=\"donn_es.bin\"; filename*=UTF-8''donn%C3%A9es.bin",
        ),
        (
            "été \"2026\".png",
            &PNG[..16],
            "image/png",
            "image/png",
            "attachment; filename=\"_t_ _2026_.png\"; filename*=UTF-8''%C3%A9t%C3%A9%20%222026%22.png",
        ),
    ];
    for (name, bytes, declared, served_as, disposition) in cases {
        let sent = alice
            .browser
            .send(
                Request::post(upload_uri(page, name))
                    .header(header::CONTENT_TYPE, declared)
                    .body(Body::from(bytes.to_vec()))
                    .unwrap(),
            )
            .await;
        sent.assert_status(StatusCode::CREATED);
        let info = sent.json();
        assert_eq!(info["mime"], served_as, "{name}");
        assert_eq!(info["name"], name, "{name}");
        let served = read(&alice.browser, info["url"].as_str().unwrap(), &[]).await;
        served.assert_status(StatusCode::OK);
        assert_eq!(served.body.as_ref(), bytes, "{name}");
        assert_served_headers(&served, disposition, served_as, bytes, name);
        assert_eq!(
            only(&served, "content-length"),
            bytes.len().to_string(),
            "{name}"
        );
        assert!(served.headers.get("content-range").is_none(), "{name}");
    }
}

/// Garde : le type que déclare le navigateur n'est jamais lu, ni à l'envoi ni à la lecture.
#[tokio::test]
async fn declared_content_type_is_never_read() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let sent = alice
        .browser
        .send(
            Request::post(upload_uri(page, "page.html"))
                .header(header::CONTENT_TYPE, "text/html")
                .body(Body::from(PNG))
                .unwrap(),
        )
        .await;
    sent.assert_status(StatusCode::CREATED);
    assert_eq!(sent.json()["mime"], "image/png");
    let served = read(&alice.browser, sent.json()["url"].as_str().unwrap(), &[]).await;
    assert_eq!(only(&served, "content-type"), "image/png");
}

/// Aucun nom venu du navigateur n'entre dans un chemin : après des envois aux noms hostiles, le
/// dépôt ne porte que `tmp/` (vide) et des chemins `<2 hex>/<62 hex>`, et rien d'autre n'est
/// apparu dans le dossier des données ; le nom rendu est le nom nettoyé.
#[tokio::test]
async fn upload_is_kept_under_its_hash_and_never_under_its_name() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let long = "a".repeat(300);
    let cases = [
        ("../../etc/passwd", ".._.._etc_passwd".to_string()),
        ("C:\\Windows\\x", "C:_Windows_x".to_string()),
        ("photo\u{202E}gnp.exe", "photognp.exe".to_string()),
        ("ligne\nsuivante", "lignesuivante".to_string()),
        (long.as_str(), "a".repeat(255)),
        ("", "fichier".to_string()),
        ("   ", "fichier".to_string()),
        ("..", "fichier".to_string()),
    ];
    for (i, (name, kept)) in cases.iter().enumerate() {
        let info = uploaded(
            &alice.browser,
            page,
            name,
            format!("contenu {i}").as_bytes(),
        )
        .await;
        assert_eq!(info["name"], kept.as_str(), "{name:?}");
    }
    // Sans paramètre `name` : « fichier ».
    let bare = alice
        .browser
        .send(
            Request::post(format!("/api/v1/pages/{page}/files"))
                .body(Body::from("sans nom"))
                .unwrap(),
        )
        .await;
    bare.assert_status(StatusCode::CREATED);
    assert_eq!(bare.json()["name"], "fichier");

    let files = t.cfg.data_dir.join("files");
    let mut contents = 0;
    for entry in std::fs::read_dir(&files).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        let path = entry.path();
        if name == "tmp" {
            assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0, "tmp vide");
            continue;
        }
        assert!(is_hex(&name, 2), "{}", path.display());
        for inner in std::fs::read_dir(&path).unwrap() {
            let inner = inner.unwrap();
            let inner_name = inner.file_name().into_string().unwrap();
            assert!(is_hex(&inner_name, 62), "{}", inner.path().display());
            assert!(inner.file_type().unwrap().is_file());
            contents += 1;
        }
    }
    assert_eq!(contents, cases.len() + 1);
    let mut top: Vec<String> = std::fs::read_dir(&t.cfg.data_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|name| !name.starts_with("mementum.db"))
        .collect();
    top.sort();
    assert_eq!(top, ["files"]);
}

/// `name` fait `len` caractères hexadécimaux en minuscules.
fn is_hex(name: &str, len: usize) -> bool {
    name.len() == len && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Une plage simple est servie (`206`), avec `Content-Range` et les en-têtes d'un fichier servi ;
/// une plage qui commence après la fin, `416` et `Content-Range: bytes */N` ; deux plages, ou un
/// en-tête illisible, le fichier entier (`200`).
#[tokio::test]
async fn range_requests_serve_a_part() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let bytes = b"0123456789";
    let info = uploaded(&alice.browser, page, "chiffres.txt", bytes).await;
    let url = info["url"].as_str().unwrap();
    let disposition = "attachment; filename=\"chiffres.txt\"; filename*=UTF-8''chiffres.txt";
    for (range, content_range, part) in [
        ("bytes=0-", "bytes 0-9/10", &bytes[..]),
        ("bytes=0-0", "bytes 0-0/10", &bytes[..1]),
        ("bytes=5-", "bytes 5-9/10", &bytes[5..]),
        ("bytes=-3", "bytes 7-9/10", &bytes[7..]),
        ("bytes=2-4", "bytes 2-4/10", &bytes[2..5]),
        ("bytes=8-100", "bytes 8-9/10", &bytes[8..]),
        ("bytes=-100", "bytes 0-9/10", &bytes[..]),
    ] {
        let served = read(&alice.browser, url, &[(header::RANGE, range)]).await;
        served.assert_status(StatusCode::PARTIAL_CONTENT);
        assert_eq!(served.body.as_ref(), part, "{range}");
        assert_eq!(only(&served, "content-range"), content_range, "{range}");
        assert_eq!(
            only(&served, "content-length"),
            part.len().to_string(),
            "{range}"
        );
        assert_served_headers(
            &served,
            disposition,
            "application/octet-stream",
            bytes,
            range,
        );
    }
    for range in ["bytes=10-", "bytes=99-", "bytes=-0"] {
        let refused = read(&alice.browser, url, &[(header::RANGE, range)]).await;
        refused.assert_error(StatusCode::RANGE_NOT_SATISFIABLE, "range_not_satisfiable");
        assert_eq!(only(&refused, "content-range"), "bytes */10", "{range}");
        assert_eq!(only(&refused, "cache-control"), "no-store", "{range}");
    }
    for range in [
        "bytes=0-1,3-4",
        "bytes=abc",
        "octets=0-1",
        "bytes=5-2",
        "bytes=",
        "bytes=-",
        "bytes=1-2-3",
    ] {
        let served = read(&alice.browser, url, &[(header::RANGE, range)]).await;
        served.assert_status(StatusCode::OK);
        assert_eq!(served.body.as_ref(), bytes, "{range}");
        assert!(served.headers.get("content-range").is_none(), "{range}");
        assert_served_headers(
            &served,
            disposition,
            "application/octet-stream",
            bytes,
            range,
        );
    }
}

/// `If-None-Match` est jugé avant `Range` (RFC 9110, §13.2.2), comme Chromium les envoie
/// ensemble pour un média : s'il nomme l'`ETag`, `304` ; sinon la plage.
#[tokio::test]
async fn range_with_a_matching_if_none_match_is_a_304() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let info = uploaded(&alice.browser, page, "photo.png", PNG).await;
    let url = info["url"].as_str().unwrap();
    let fresh = read(
        &alice.browser,
        url,
        &[
            (header::RANGE, "bytes=0-0"),
            (header::IF_NONE_MATCH, &etag(PNG)),
        ],
    )
    .await;
    fresh.assert_status(StatusCode::NOT_MODIFIED);
    assert!(fresh.body.is_empty());
    assert!(fresh.headers.get("content-range").is_none());
    let disposition = "attachment; filename=\"photo.png\"; filename*=UTF-8''photo.png";
    assert_served_headers(&fresh, disposition, "image/png", PNG, "304");
    // Un `If-None-Match` sur une plage qui sortirait du fichier : `304` aussi.
    let beyond = read(
        &alice.browser,
        url,
        &[
            (header::RANGE, "bytes=999-"),
            (header::IF_NONE_MATCH, &etag(PNG)),
        ],
    )
    .await;
    beyond.assert_status(StatusCode::NOT_MODIFIED);
    // Un autre `ETag` : la plage.
    let part = read(
        &alice.browser,
        url,
        &[
            (header::RANGE, "bytes=0-0"),
            (header::IF_NONE_MATCH, "\"autre\""),
        ],
    )
    .await;
    part.assert_status(StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.body.as_ref(), &PNG[..1]);
}

/// Un `If-Range` qui ne nomme pas l'`ETag` fait ignorer `Range` : le fichier entier (`200`) ;
/// avec l'`ETag`, la plage.
#[tokio::test]
async fn if_range_that_does_not_match_serves_the_whole_file() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let info = uploaded(&alice.browser, page, "photo.png", PNG).await;
    let url = info["url"].as_str().unwrap();
    for if_range in [
        "\"autre\"",
        "W/\"autre\"",
        "Sun, 06 Nov 1994 08:49:37 GMT",
        &format!("W/{}", etag(PNG)),
    ] {
        let whole = read(
            &alice.browser,
            url,
            &[(header::RANGE, "bytes=0-0"), (header::IF_RANGE, if_range)],
        )
        .await;
        whole.assert_status(StatusCode::OK);
        assert_eq!(whole.body.as_ref(), PNG, "{if_range}");
    }
    let part = read(
        &alice.browser,
        url,
        &[(header::RANGE, "bytes=0-0"), (header::IF_RANGE, &etag(PNG))],
    )
    .await;
    part.assert_status(StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.body.as_ref(), &PNG[..1]);
}

/// Garde : les refus des routes des fichiers portent `Cache-Control: no-store`, comme toute
/// réponse de l'API : un `404`, un `403`, un `413`.
#[tokio::test]
async fn refusals_of_file_routes_are_not_cached() {
    let t = small_app().await;
    let team = Team::new(&t).await;
    let grants = [user(&team.olga, Level::Full), user(&team.tom, Level::Read)];
    let page = team.page(None, "P", &grants).await;
    let refusals = [
        read(&team.tom.browser, &file_uri(page, Uuid::now_v7()), &[]).await,
        read(&team.zoe.browser, &file_uri(page, Uuid::now_v7()), &[]).await,
        upload(&team.tom.browser, page, "x", "abc").await,
        upload(&team.olga.browser, page, "x", vec![0u8; (1 << 20) + 1]).await,
    ];
    let statuses: Vec<_> = refusals.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses,
        [
            StatusCode::NOT_FOUND,
            StatusCode::NOT_FOUND,
            StatusCode::FORBIDDEN,
            StatusCode::PAYLOAD_TOO_LARGE
        ]
    );
    for refusal in &refusals {
        assert_eq!(
            only(refusal, "cache-control"),
            "no-store",
            "{}",
            refusal.status
        );
        assert_eq!(only(refusal, "x-content-type-options"), "nosniff");
    }
}

/// Un corps qui note s'il a été lu.
struct Watched {
    bytes: Option<Bytes>,
    polled: Arc<AtomicBool>,
}

impl http_body::Body for Watched {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        self.polled.store(true, Ordering::SeqCst);
        Poll::Ready(
            self.bytes
                .take()
                .map(|bytes| Ok(http_body::Frame::data(bytes))),
        )
    }
}

/// Un corps sans longueur, fait de `chunks` morceaux de `size` octets.
struct Chunks {
    left: usize,
    size: usize,
}

impl http_body::Body for Chunks {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        if self.left == 0 {
            return Poll::Ready(None);
        }
        self.left -= 1;
        Poll::Ready(Some(Ok(http_body::Frame::data(Bytes::from(vec![
            7u8;
            self.size
        ])))))
    }
}

/// Plus que la borne est refusé (`413 file_too_large`) : sur `Content-Length`, avant de lire le
/// corps ; sans longueur, en cours de lecture ; dans les deux cas, rien n'est gardé. La borne
/// juste passe.
#[tokio::test]
async fn file_too_large_is_refused_before_and_while_reading() {
    let t = small_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let max = 1usize << 20;

    // Annoncé trop gros : refusé sans lire.
    let polled = Arc::new(AtomicBool::new(false));
    let announced = alice
        .browser
        .send(
            Request::post(upload_uri(page, "gros.bin"))
                .header(header::CONTENT_LENGTH, (max + 1).to_string())
                .body(Body::new(Watched {
                    bytes: Some(Bytes::from(vec![1u8; max + 1])),
                    polled: polled.clone(),
                }))
                .unwrap(),
        )
        .await;
    let refusal = announced.assert_error(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large");
    common::assert_french(&refusal, &["1 Mio"]);
    assert!(!polled.load(Ordering::SeqCst), "le corps a été lu");

    // Sans longueur : refusé en cours de lecture.
    let streamed = upload(
        &alice.browser,
        page,
        "gros.bin",
        Body::new(Chunks {
            left: 17,
            size: 64 * 1024,
        }),
    )
    .await;
    streamed.assert_error(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large");
    common::eventually("files/tmp vide", || temporaries(&t).is_empty()).await;
    assert_eq!(attachments(&t).await, 0);

    // La borne juste.
    let exact = upload(
        &alice.browser,
        page,
        "juste.bin",
        Body::new(Chunks {
            left: 16,
            size: 64 * 1024,
        }),
    )
    .await;
    exact.assert_status(StatusCode::CREATED);
    assert_eq!(exact.json()["size"], max);
    assert_eq!(attachments(&t).await, 1);
}

/// Un envoi qui ne reçoit plus rien pendant `upload_idle` est abandonné : la connexion est
/// fermée sans un octet de réponse, son temporaire part, rien n'est attaché, et le journal le
/// dit. Le délai est d'une seconde : le test doit voir le temporaire de l'envoi avant qu'il parte
/// (une borne qui peut le faire échouer à tort sur un poste très chargé, jamais passer).
#[tokio::test]
async fn stalled_upload_is_abandoned() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.upload_idle = Duration::from_secs(1);
    })
    .await;
    let logs = Logs::capture();
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let addr = t.listen().await;
    let mut raw = RawUpload::start(addr, &alice.browser, page, "lent.bin", Some(1000)).await;
    raw.send(&[5u8; 100]).await;
    // L'envoi a commencé : son temporaire porte ce qui a été reçu.
    common::eventually("le temporaire de l'envoi", || temporaries(&t) == [100]).await;
    let received = raw.closed().await;
    assert!(
        received.is_empty(),
        "réponse reçue : {:?}",
        String::from_utf8_lossy(&received)
    );
    common::eventually("files/tmp vide", || temporaries(&t).is_empty()).await;
    assert_eq!(attachments(&t).await, 0);
    let journal = logs.text();
    assert!(
        journal.contains("envoi abandonné") && journal.contains(&page.to_string()),
        "{journal}"
    );
}

/// Un envoi que le navigateur coupe au milieu ne laisse rien : ni temporaire ni attache.
#[tokio::test]
async fn interrupted_upload_leaves_nothing() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let addr = t.listen().await;
    let mut raw = RawUpload::start(addr, &alice.browser, page, "coupé.bin", Some(1000)).await;
    raw.send(&[5u8; 500]).await;
    common::eventually("le temporaire de l'envoi", || temporaries(&t) == [500]).await;
    drop(raw);
    common::eventually("files/tmp vide", || temporaries(&t).is_empty()).await;
    assert_eq!(attachments(&t).await, 0);
}

/// Un fichier de zéro octet est accepté, et se relit vide.
#[tokio::test]
async fn empty_file_is_accepted() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let info = uploaded(&alice.browser, page, "vide.txt", b"").await;
    assert_eq!(info["size"], 0);
    assert_eq!(info["mime"], "application/octet-stream");
    let served = read(&alice.browser, info["url"].as_str().unwrap(), &[]).await;
    served.assert_status(StatusCode::OK);
    assert!(served.body.is_empty());
    assert_eq!(only(&served, "content-length"), "0");
    // Une plage sur un fichier vide est ignorée : il n'a aucun octet à donner.
    let ranged = read(
        &alice.browser,
        info["url"].as_str().unwrap(),
        &[(header::RANGE, "bytes=0-")],
    )
    .await;
    ranged.assert_status(StatusCode::OK);
    assert!(ranged.body.is_empty());
}

/// `GET /api/v1/files/limits` dit la borne de l'instance, à qui a une session.
#[tokio::test]
async fn limits_route_says_the_maximum() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let limits = alice.browser.get("/api/v1/files/limits").await;
    limits.assert_status(StatusCode::OK);
    assert_eq!(limits.json(), json!({ "maxFileBytes": 104_857_600 }));
    t.browser()
        .get("/api/v1/files/limits")
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    let small = small_app().await;
    let bob = small.account("Bob").await;
    let limits = bob.browser.get("/api/v1/files/limits").await;
    assert_eq!(limits.json(), json!({ "maxFileBytes": 1_048_576 }));
}

/// Un contenu que le dépôt n'a plus, ou qu'il a d'une autre taille que la base, est dit
/// indisponible ; jamais servi pour une taille qu'on n'a pas lue (constat m10 de la Task S3), et
/// la réponse ne nomme rien du serveur (m3). Témoin : remis entier, il se relit.
#[tokio::test]
async fn unavailable_content_is_told_without_naming_the_server() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let info = uploaded(&alice.browser, page, "photo.png", PNG).await;
    let url = info["url"].as_str().unwrap();
    let path = content_path(&t, PNG);
    let data_dir = t.cfg.data_dir.display().to_string();
    let longer = [PNG, &b"en trop"[..]].concat();
    for (what, damage) in [
        ("absent", None),
        ("tronqué", Some(&PNG[..10])),
        ("plus long", Some(&longer[..])),
    ] {
        match damage {
            None => std::fs::remove_file(&path).unwrap(),
            Some(bytes) => std::fs::write(&path, bytes).unwrap(),
        }
        for headers in [&[][..], &[(header::RANGE, "bytes=0-")][..]] {
            let reply = read(&alice.browser, url, headers).await;
            let body = reply.assert_error(StatusCode::NOT_FOUND, "not_found");
            let message = body["message"].as_str().unwrap();
            assert!(
                message.contains("fichier indisponible"),
                "{what} : {message}"
            );
            let text = reply.text();
            assert!(!text.contains(&data_dir), "{what} : {text}");
            assert!(!text.contains("files"), "{what} : {text}");
            assert_eq!(only(&reply, "cache-control"), "no-store");
        }
    }
    std::fs::write(&path, PNG).unwrap();
    let served = read(&alice.browser, url, &[]).await;
    served.assert_status(StatusCode::OK);
    assert_eq!(served.body.as_ref(), PNG);
}

/// Un envoi que le dépôt ne peut pas poser (son dossier de préfixe est un fichier) répond `500`
/// sans rien nommer du serveur (constat m3 de la Task S3), et ne laisse rien.
#[tokio::test]
async fn failed_upload_names_nothing_of_the_server() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    let prefix = content_path(&t, b"abc").parent().unwrap().to_path_buf();
    std::fs::write(&prefix, b"un fichier a la place du dossier").unwrap();
    let reply = upload(&alice.browser, page, "abc.txt", "abc").await;
    let body = reply.assert_error(StatusCode::INTERNAL_SERVER_ERROR, "internal");
    assert_eq!(body["message"], "erreur interne du serveur");
    assert_eq!(temporaries(&t), Vec::<u64>::new());
    assert_eq!(attachments(&t).await, 0);
}

/// Une connexion qui tient le verrou d'écriture de SQLite.
struct WriteLock(sqlx::SqliteConnection);

impl WriteLock {
    async fn take(db: &Path) -> WriteLock {
        let url = format!("sqlite:{}", db.display());
        let mut connection = sqlx::SqliteConnection::connect(&url).await.unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut connection)
            .await
            .unwrap();
        WriteLock(connection)
    }

    async fn run(&mut self, sql: &str) {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
            .execute(&mut self.0)
            .await
            .unwrap_or_else(|error| panic!("{sql} : {error}"));
    }
}

/// Un contenu que l'envoi vient de poser lui-même, et qu'il n'a pas pu attacher (la page
/// supprimée entre le second contrôle et l'écriture de l'attache), est retiré aussitôt ; la
/// réponse est celle d'une page introuvable. Témoin : un contenu qui était déjà là reste, et se
/// lit encore par l'autre page qui le porte.
///
/// L'écriture de l'attache attend le verrou que le test tient ; le test sait qu'elle y est quand
/// le temporaire de l'envoi a quitté `files/tmp` : le contenu est posé, ou tenu pour déjà là.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_whose_page_is_deleted_before_its_attachment_takes_its_content_back() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let kept = team.page(None, "Gardée", &[members(Level::Full)]).await;
    let first = uploaded(&team.mona.browser, kept, "déjà.png", PNG).await;
    for (what, bytes) in [("posé par l'envoi", &b"tout neuf"[..]), ("déjà là", PNG)] {
        let page = team.page(None, what, &[members(Level::Full)]).await;
        let was_there = content_path(&t, bytes).exists();
        let mut lock = WriteLock::take(&t.db_path()).await;
        let length = bytes.len() as u64;
        let mut raw = RawUpload::start(addr, &team.mona.browser, page, "x", Some(length)).await;
        raw.send(&bytes[..1]).await;
        common::eventually("le temporaire de l'envoi", || temporaries(&t) == [1]).await;
        raw.send(&bytes[1..]).await;
        common::eventually("files/tmp vide", || temporaries(&t).is_empty()).await;
        assert!(
            content_path(&t, bytes).exists(),
            "{what} : posé avant l'attache"
        );
        lock.run(&format!("DELETE FROM nodes WHERE id = '{page}'"))
            .await;
        lock.run(&format!(
            "UPDATE spaces SET rev = rev + 1 WHERE id = '{}'",
            team.space
        ))
        .await;
        lock.run("COMMIT").await;

        let reply = raw.response().await;
        assert_eq!(reply.status, 404, "{what} : {:?}", reply);
        assert_eq!(reply.json()["message"], "page introuvable", "{what}");
        assert_eq!(
            content_path(&t, bytes).exists(),
            was_there,
            "{what} : le contenu"
        );
        let sql = format!("SELECT COUNT(*) FROM attachments WHERE node_id = '{page}'");
        assert_eq!(query_i64(&t.db_path(), &sql).await, 0, "{what}");
    }
    let served = read(&team.mona.browser, first["url"].as_str().unwrap(), &[]).await;
    served.assert_status(StatusCode::OK);
    assert_eq!(served.body.as_ref(), PNG);
}

/// Au démarrage, le journal dit en avertissement les contenus du dépôt que la base ne connaît
/// pas, et ceux qu'elle connaît et que le dépôt n'a plus ; **aucun contenu n'est supprimé**
/// (Ruling R245), seul `files/tmp` est vidé. Témoin : un dépôt d'accord avec la base ne dit rien.
#[tokio::test]
async fn startup_counts_strangers_and_missing_contents_and_removes_none() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = Uuid::parse_str(&alice.create(json!({ "title": "P" })).await.id).unwrap();
    uploaded(&alice.browser, page, "photo.png", PNG).await;
    uploaded(&alice.browser, page, "rapport.pdf", PDF).await;
    let restart = async || {
        let logs = Logs::capture();
        let _restarted = mementum_server::app(&t.cfg).await.unwrap();
        logs.text()
    };
    let quiet = restart().await;
    assert!(!quiet.contains("ne connaît pas"), "{quiet}");
    assert!(!quiet.contains("n'a plus"), "{quiet}");

    // Un contenu que la base ne connaît pas, un qu'elle connaît et que le dépôt n'a plus, un
    // reste d'envoi.
    let stranger = b"un contenu que la base ne connait pas";
    let stranger_path = content_path(&t, stranger);
    std::fs::create_dir_all(stranger_path.parent().unwrap()).unwrap();
    std::fs::write(&stranger_path, stranger).unwrap();
    std::fs::remove_file(content_path(&t, PDF)).unwrap();
    let leftover = t.cfg.data_dir.join("files").join("tmp").join("reste");
    std::fs::write(&leftover, b"reste").unwrap();

    let journal = restart().await;
    let warning = |marker: &str| {
        journal
            .lines()
            .find(|line| line.contains(marker))
            .unwrap_or_else(|| panic!("« {marker} » absent du journal : {journal}"))
            .to_string()
    };
    let strangers = warning("que la base ne connaît pas");
    assert!(strangers.contains("WARN"), "{strangers}");
    assert!(strangers.contains("count=1"), "{strangers}");
    assert!(
        strangers.contains(&format!("bytes={}", stranger.len())),
        "{strangers}"
    );
    let missing = warning("que le dépôt n'a plus");
    assert!(missing.contains("WARN"), "{missing}");
    assert!(missing.contains("missing=1"), "{missing}");
    // Rien n'a été supprimé, hors du reste d'envoi.
    assert!(
        stranger_path.exists(),
        "le contenu inconnu de la base reste"
    );
    assert!(content_path(&t, PNG).exists());
    assert!(!leftover.exists(), "files/tmp est vidé");
}
