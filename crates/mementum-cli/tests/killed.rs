//! Serveur tué en pleine frappe (Review Focus 1) : tout ce qu'un autre client
//! a reçu est encore là après le redémarrage.
//!
//! Ce test voit le résultat, pas l'ordre « persister, puis diffuser » : après
//! un `kill -9`, une écriture SQLite survit dès son `write()`, même si la
//! diffusion l'a précédée. Cet ordre est gardé par
//! `crates/mementum-sync/tests/durability.rs`
//! (`update_is_persisted_before_broadcast`).

mod common;

use std::time::Duration;

use common::Server;
use mementum_md::block_text;
use mementum_sync::testkit::Client;
use serde_json::Value;

/// Délai maximal d'une attente.
const WAIT: Duration = Duration::from_secs(30);

/// Mises à jour envoyées par A.
const UPDATES: usize = 300;

/// Caractères reçus par B avant que le serveur soit tué.
const KILL_AFTER: usize = 30;

/// Texte du premier bloc chez ce client.
fn text(client: &Client) -> String {
    client.blocks().first().map(block_text).unwrap_or_default()
}

/// Ouvre la WebSocket de synchro avec le cookie de session (la synchro ne
/// répond pas sans lui), et attend la première synchro.
async fn connect(url: &str, cookie: &str) -> Client {
    let client = Client::connect_with_headers(url, &[("cookie", cookie)])
        .await
        .expect("connexion du client");
    assert!(
        client.wait_until(WAIT, |c| c.is_synced()).await,
        "le client n'a pas reçu l'état du serveur"
    );
    client
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn killed_server_loses_nothing_broadcast() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");

    let server = Server::start(&data);
    let (status, info) = server.http("GET", "/api/v1/info", None);
    assert_eq!(status, 200, "{info}");
    let schema = serde_json::from_str::<Value>(&info).unwrap()["schemaVersion"]
        .as_u64()
        .unwrap();
    // Aucune route de page ne répond sans session : un compte, et son espace.
    let account =
        r#"{"email":"alice@exemple.fr","name":"Alice","password":"mot-de-passe-de-test"}"#;
    let signup = server.request("POST", "/api/v1/auth/signup", Some(account), &[]);
    assert_eq!(signup.status, 201, "{}", signup.body);
    let cookie = signup.session_cookie().expect("cookie de session");
    let space = serde_json::from_str::<Value>(&signup.body).unwrap()["spaces"][0]["id"].clone();
    let body = serde_json::json!({ "spaceId": space, "title": "Frappe" }).to_string();
    let page = server.request("POST", "/api/v1/pages", Some(&body), &[("Cookie", &cookie)]);
    assert_eq!(page.status, 201, "{}", page.body);
    let page = serde_json::from_str::<Value>(&page.body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let a = connect(&server.ws_url(&page, schema), &cookie).await;
    let b = connect(&server.ws_url(&page, schema), &cookie).await;
    let block = a.blocks()[0].id.clone();

    // A tape 300 caractères à la fin du bloc, une mise à jour chacun. Chaque
    // frappe s'ajoute en fin de texte : ce que B a reçu est toujours un
    // préfixe de ce que A a tapé.
    let typed: String = (0..UPDATES)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    for c in typed.chars() {
        a.type_text(&block, &c.to_string());
    }

    // Tué pendant l'envoi, dès que B a reçu les premières frappes.
    assert!(
        b.wait_until(WAIT, |b| text(b).len() >= KILL_AFTER).await,
        "B n'a rien reçu ; stderr :\n{}",
        server.stderr()
    );
    server.kill();
    // Ce que B a reçu : tout ce qui lui est arrivé avant la coupure.
    assert!(
        b.wait_until(WAIT, |b| b.is_closed()).await,
        "B voit encore le serveur"
    );
    let received = text(&b);
    // Sans ce contrôle, `starts_with("")` serait vrai d'office.
    assert!(
        received.len() >= KILL_AFTER,
        "B a perdu ce qu'il avait reçu : {received:?}"
    );
    assert!(
        typed.starts_with(&received),
        "B a reçu un texte qui n'est pas un préfixe de la frappe : {received:?}"
    );
    assert!(
        received.len() < UPDATES,
        "le serveur a été tué trop tard ({} caractères sur {UPDATES} déjà reçus)",
        received.len()
    );
    drop(a);
    drop(b);

    // Relancé sur le même dossier : le texte contient tout ce que B a reçu.
    let server = Server::start(&data);
    let c = connect(&server.ws_url(&page, schema), &cookie).await;
    let stored = text(&c);
    assert!(
        stored.starts_with(&received),
        "perdu après le redémarrage : B avait reçu {} caractères, la page en garde {} ({stored:?})",
        received.len(),
        stored.len()
    );
    assert!(typed.starts_with(&stored), "texte inattendu : {stored:?}");
    // Mesures, visibles avec `--nocapture`.
    println!(
        "reçus par B avant la coupure : {} ; gardés après redémarrage : {} ; tapés : {UPDATES}",
        received.len(),
        stored.len()
    );

    // Le MMD de la page dit la même chose (la session a survécu au redémarrage).
    let path = format!("/api/v1/pages/{page}/mmd");
    let mmd = server.request("GET", &path, None, &[("Cookie", &cookie)]);
    assert_eq!(mmd.status, 200, "{}", mmd.body);
    assert_eq!(mmd.body, format!("{stored}\n"));
}
