//! Task 5 — l'agent, pair CRDT côté serveur : str_replace, insert, conflits, Unicode.

mod common;

use std::time::Duration;

use common::*;
use serde_json::{json, Value};
use yrs::{Text, Transact, XmlFragment, XmlOut};

const PAGE: &str = "# Titre\n\nPremier paragraphe.\n\nDeuxième paragraphe.\n\nCafé 🚀 déjà\n\nCinquième bloc à éditer.\n";

fn agent() -> Value {
    json!({ "id": "scribe", "run": "run-1" })
}

async fn setup(page: &str, mmd: &str) -> (tempfile::TempDir, u16, tokio::task::JoinHandle<()>) {
    let dir = tempfile::tempdir().unwrap();
    let (port, server) = start_server(&dir.path().join("t.db")).await;
    put_mmd(port, page, mmd).await;
    (dir, port, server)
}

fn str_replace(rev: &Value, old: &str, new: &str) -> Value {
    json!({ "base_revision": rev, "agent": agent(), "operations": [{ "op": "str_replace", "old": old, "new": new }] })
}

/// Le client Yjs de test modifie le début d'un bloc (repéré par son id).
fn client_prepend(client: &Client, block_id: &str, s: &str) {
    let frag = client.doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = client.doc.transact_mut();
    let el = spike_mmd::ydoc::find_block_xml(&txn, &frag, block_id).expect("bloc introuvable côté client");
    let text = el
        .children(&txn)
        .find_map(|c| match c {
            XmlOut::Text(t) => Some(t),
            _ => None,
        })
        .unwrap();
    text.insert(&mut txn, 0, s);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn str_replace_applies() {
    let (_d, port, _s) = setup("a1", PAGE).await;
    let page = get_page(port, "a1", false).await;
    let (status, body) = update(port, "a1", str_replace(&page["revision"], "Premier paragraphe", "Paragraphe modifié")).await;
    assert_eq!(status, 200, "{body}");
    let md = get_page(port, "a1", false).await["markdown"].as_str().unwrap().to_string();
    assert!(md.contains("Paragraphe modifié.") && !md.contains("Premier paragraphe"), "{md}");
    assert_eq!(body["touched_markdown"][0], "Paragraphe modifié.\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn insert_after_anchor() {
    let (_d, port, _s) = setup("a2", PAGE).await;
    let page = get_page(port, "a2", true).await;
    assert!(page["markdown"].as_str().unwrap().contains("Deuxième paragraphe. ^b3"));
    let body = json!({ "base_revision": page["revision"], "agent": agent(),
        "operations": [{ "op": "insert", "after": "b3", "markdown": "- [ ] Nouvelle tâche" }] });
    let (status, resp) = update(port, "a2", body).await;
    assert_eq!(status, 200, "{resp}");
    let md = get_page(port, "a2", false).await["markdown"].as_str().unwrap().to_string();
    assert!(md.contains("Deuxième paragraphe.\n\n- [ ] Nouvelle tâche\n\nCafé"), "{md}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_match_returns_candidates() {
    let (_d, port, _s) = setup("a3", PAGE).await;
    let page = get_page(port, "a3", false).await;
    let (status, body) = update(port, "a3", str_replace(&page["revision"], "Troisième paragraphe", "x")).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"], "no_match");
    let candidates = body["candidates"].as_array().unwrap();
    assert!(!candidates.is_empty() && candidates.len() <= 3, "{body}");
    assert!(candidates.iter().any(|c| c.as_str().unwrap().contains("paragraphe")), "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ambiguous_match_rejected() {
    let (_d, port, _s) = setup("a4", PAGE).await;
    let page = get_page(port, "a4", false).await;
    let (status, body) = update(port, "a4", str_replace(&page["revision"], "paragraphe", "x")).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"], "ambiguous_match");
    let md = get_page(port, "a4", false).await["markdown"].as_str().unwrap().to_string();
    assert_eq!(md, PAGE, "rien ne doit changer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unicode_safe() {
    let (_d, port, _s) = setup("a5", PAGE).await;
    let client = connect(port, "a5", new_doc()).await;
    let page = get_page(port, "a5", false).await;
    let (status, body) = update(port, "a5", str_replace(&page["revision"], "déjà", "prêt")).await;
    assert_eq!(status, 200, "{body}");
    let md = get_page(port, "a5", false).await["markdown"].as_str().unwrap().to_string();
    assert!(md.contains("\nCafé 🚀 prêt\n"), "{md}");
    assert!(
        wait_until(Duration::from_secs(2), || text_content(&client.doc).contains("Café 🚀 prêt")).await,
        "le client Yjs doit lire la même chaîne : {}",
        text_content(&client.doc)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conflict_when_block_changed() {
    let (_d, port, _s) = setup("a6", PAGE).await;
    let client = connect(port, "a6", new_doc()).await;
    let page = get_page(port, "a6", true).await;
    let target = page["anchors"]["b5"].as_str().unwrap().to_string();
    assert!(wait_until(Duration::from_secs(2), || text_content(&client.doc).contains("Cinquième")).await);
    client_prepend(&client, &target, "Humain : ");
    let mut seen = false;
    for _ in 0..200 {
        if get_page(port, "a6", false).await["markdown"].as_str().unwrap().contains("Humain : Cinquième") {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(seen, "la modification humaine doit atteindre le serveur");
    let (status, body) = update(port, "a6", str_replace(&page["revision"], "à éditer", "édité")).await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], "conflict");
    assert!(body["blocks"][0]["markdown"].as_str().unwrap().contains("Humain : Cinquième bloc à éditer."), "{body}");
    let md = get_page(port, "a6", false).await["markdown"].as_str().unwrap().to_string();
    assert!(md.contains("Humain : Cinquième bloc à éditer."), "document inchangé : {md}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rebase_when_other_block_changed() {
    let (_d, port, _s) = setup("a7", PAGE).await;
    let client = connect(port, "a7", new_doc()).await;
    let page = get_page(port, "a7", true).await;
    let other = page["anchors"]["b2"].as_str().unwrap().to_string();
    assert!(wait_until(Duration::from_secs(2), || text_content(&client.doc).contains("Premier")).await);
    client_prepend(&client, &other, "Humain : ");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (status, body) = update(port, "a7", str_replace(&page["revision"], "à éditer", "édité")).await;
    assert_eq!(status, 200, "{body}");
    let md = get_page(port, "a7", false).await["markdown"].as_str().unwrap().to_string();
    assert!(md.contains("Humain : Premier paragraphe.") && md.contains("Cinquième bloc édité."), "{md}");
}
