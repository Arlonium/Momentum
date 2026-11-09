//! Serveur tué en pleine frappe (Review Focus 1) : tout ce qu'un autre client
//! a reçu est encore là après le redémarrage.

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
    let (status, page) = server.http("POST", "/api/v1/pages", Some(r#"{"title":"Frappe"}"#));
    assert_eq!(status, 201, "{page}");
    let page = serde_json::from_str::<Value>(&page).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let a = Client::connect_synced(&server.ws_url(&page, schema)).await;
    let b = Client::connect_synced(&server.ws_url(&page, schema)).await;
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
    let c = Client::connect_synced(&server.ws_url(&page, schema)).await;
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

    // Le MMD de la page dit la même chose.
    let (status, mmd) = server.http("GET", &format!("/api/v1/pages/{page}/mmd"), None);
    assert_eq!(status, 200);
    assert_eq!(mmd, format!("{stored}\n"));
}
