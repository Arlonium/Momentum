//! Task 1 — synchro Yjs : convergence, persistance, reconnexion, compaction.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMsg};
use yrs::sync::{Message, MessageReader, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, OffsetKind, Options, ReadTxn, Transact, Update, XmlFragment, XmlTextPrelim};

const FRAGMENT: &str = "document-store";
const REMOTE: &str = "remote";

fn new_doc() -> Doc {
    Doc::with_options(Options { offset_kind: OffsetKind::Utf16, ..Default::default() })
}

async fn start_server(db: &Path) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let db: PathBuf = db.to_path_buf();
    let handle = tokio::spawn(async move {
        spike_server::serve(listener, db).await.unwrap();
    });
    (port, handle)
}

/// Client de test : un `yrs::Doc` relié au serveur par le protocole y-websocket.
struct Client {
    doc: Doc,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Client {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

async fn connect(port: u16, page: &str, doc: Doc) -> Client {
    let (ws, _) = connect_async(format!("ws://127.0.0.1:{port}/sync/{page}")).await.unwrap();
    let (mut sink, mut stream) = ws.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Étape 1 : notre vecteur d'état.
    let sv = doc.transact().state_vector();
    out_tx.send(Message::Sync(SyncMessage::SyncStep1(sv)).encode_v1()).unwrap();

    // Mises à jour locales → serveur (sauf celles venues du serveur).
    let tx = out_tx.clone();
    doc.observe_update_v1("client-out", move |txn, e| {
        let remote = txn.origin().map(|o| o.as_ref() == REMOTE.as_bytes()).unwrap_or(false);
        if !remote {
            let _ = tx.send(Message::Sync(SyncMessage::Update(e.update.clone())).encode_v1());
        }
    })
    .unwrap();

    let writer = tokio::spawn(async move {
        while let Some(bytes) = out_rx.recv().await {
            if sink.send(WsMsg::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });

    let rdoc = doc.clone();
    let reply = out_tx.clone();
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            let WsMsg::Binary(data) = msg else { continue };
            let mut decoder = DecoderV1::from(data.as_ref());
            let reader = MessageReader::new(&mut decoder);
            for m in reader {
                match m {
                    Ok(Message::Sync(SyncMessage::SyncStep1(sv))) => {
                        let update = rdoc.transact().encode_state_as_update_v1(&sv);
                        let _ = reply.send(Message::Sync(SyncMessage::SyncStep2(update)).encode_v1());
                    }
                    Ok(Message::Sync(SyncMessage::SyncStep2(u))) | Ok(Message::Sync(SyncMessage::Update(u))) => {
                        let mut txn = rdoc.transact_mut_with(REMOTE);
                        txn.apply_update(Update::decode_v1(&u).unwrap()).unwrap();
                    }
                    _ => {}
                }
            }
        }
    });

    Client { doc, tasks: vec![writer, reader] }
}

fn write_text(doc: &Doc, text: &str) {
    let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = doc.transact_mut();
    let len = frag.len(&txn);
    frag.insert(&mut txn, len, XmlTextPrelim::new(text));
}

fn read_text(doc: &Doc) -> String {
    let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    frag.get_string(&txn)
}

async fn wait_for(doc: &Doc, expected: &str, timeout: Duration) -> Option<Duration> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if read_text(doc).contains(expected) {
            return Some(start.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_clients_converge() {
    let dir = tempfile::tempdir().unwrap();
    let (port, _server) = start_server(&dir.path().join("t.db")).await;
    let a = connect(port, "p1", new_doc()).await;
    let b = connect(port, "p1", new_doc()).await;
    tokio::time::sleep(Duration::from_millis(100)).await; // poignées de main terminées

    write_text(&a.doc, "Bonjour");
    let took = wait_for(&b.doc, "Bonjour", Duration::from_secs(2)).await.expect("B n'a jamais reçu le texte");
    assert!(took < Duration::from_millis(150), "propagation trop lente : {took:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn persists_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let (port, server) = start_server(&db).await;
    {
        let a = connect(port, "p2", new_doc()).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        write_text(&a.doc, "Bonjour");
        tokio::time::sleep(Duration::from_millis(200)).await; // le serveur a persisté
    }
    server.abort();
    let _ = server.await;

    let (port2, _server2) = start_server(&db).await;
    let c = connect(port2, "p2", new_doc()).await;
    assert!(wait_for(&c.doc, "Bonjour", Duration::from_secs(2)).await.is_some(), "texte perdu au redémarrage");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reconnect_after_restart_keeps_unsent_edits() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let (port, server) = start_server(&db).await;
    let doc_a = new_doc();
    {
        let a = connect(port, "p3", doc_a.clone()).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        write_text(&a.doc, "avant ");
        tokio::time::sleep(Duration::from_millis(200)).await;
    } // A se déconnecte
    server.abort();
    let _ = server.await;

    // Serveur arrêté : A continue d'écrire hors ligne.
    write_text(&doc_a, "pendant");

    let (port2, _server2) = start_server(&db).await;
    let _a = connect(port2, "p3", doc_a.clone()).await;
    let c = connect(port2, "p3", new_doc()).await;
    assert!(wait_for(&c.doc, "avant pendant", Duration::from_secs(2)).await.is_some(), "édition hors ligne perdue : {:?}", read_text(&c.doc));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compacts_after_500_updates() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let (port, _server) = start_server(&db).await;
    let a = connect(port, "p4", new_doc()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    for i in 0..501 {
        write_text(&a.doc, &format!("{} ", i % 10));
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let store = spike_server::store::Store::open(&db).await.unwrap();
    let (snapshots, updates) = store.counts("p4").await.unwrap();
    assert_eq!(snapshots, 1, "pas d'instantané après 501 mises à jour");
    assert!(updates <= 1, "{updates} mises à jour restantes après compaction");
    // Et le contenu complet se recharge depuis l'instantané.
    let doc = store.load_doc("p4").await.unwrap();
    assert_eq!(read_text(&doc).matches(' ').count(), 501);
}
