//! Outils partagés par les tests d'intégration : serveur sur un port libre, client Yjs, HTTP.
#![allow(dead_code)]

use std::path::Path;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMsg};
use yrs::sync::{Message, MessageReader, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, OffsetKind, Options, ReadTxn, Transact, Update, XmlFragment};

pub const FRAGMENT: &str = "document-store";
const REMOTE: &str = "remote";

pub fn new_doc() -> Doc {
    Doc::with_options(Options { offset_kind: OffsetKind::Utf16, ..Default::default() })
}

pub async fn start_server(db: &Path) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let db = db.to_path_buf();
    let handle = tokio::spawn(async move {
        spike_server::serve(listener, db).await.unwrap();
    });
    (port, handle)
}

pub struct Client {
    pub doc: Doc,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Client {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

pub async fn connect(port: u16, page: &str, doc: Doc) -> Client {
    let (ws, _) = connect_async(format!("ws://127.0.0.1:{port}/sync/{page}")).await.unwrap();
    let (mut sink, mut stream) = ws.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let sv = doc.transact().state_vector();
    out_tx.send(Message::Sync(SyncMessage::SyncStep1(sv)).encode_v1()).unwrap();
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
            for m in MessageReader::new(&mut decoder) {
                match m {
                    Ok(Message::Sync(SyncMessage::SyncStep1(sv))) => {
                        let update = rdoc.transact().encode_state_as_update_v1(&sv);
                        let _ = reply.send(Message::Sync(SyncMessage::SyncStep2(update)).encode_v1());
                    }
                    Ok(Message::Sync(SyncMessage::SyncStep2(u))) | Ok(Message::Sync(SyncMessage::Update(u))) => {
                        rdoc.transact_mut_with(REMOTE).apply_update(Update::decode_v1(&u).unwrap()).unwrap();
                    }
                    _ => {}
                }
            }
        }
    });
    Client { doc, tasks: vec![writer, reader] }
}

/// Texte de tous les `XmlText` du document (sans balises).
pub fn text_content(doc: &Doc) -> String {
    let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    frag.successors(&txn)
        .filter_map(|n| match n {
            yrs::XmlOut::Text(t) => Some(t.get_string(&txn)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    false
}

pub fn http() -> reqwest::Client {
    reqwest::Client::new()
}

pub async fn put_mmd(port: u16, page: &str, mmd: &str) {
    let r = http().put(format!("http://127.0.0.1:{port}/pages/{page}/mmd")).body(mmd.to_string()).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 204, "PUT mmd");
}

pub async fn get_page(port: u16, page: &str, anchors: bool) -> Value {
    let q = if anchors { "?anchors=block" } else { "" };
    http().get(format!("http://127.0.0.1:{port}/pages/{page}{q}")).send().await.unwrap().json().await.unwrap()
}

pub async fn update(port: u16, page: &str, body: Value) -> (u16, Value) {
    let r = http().post(format!("http://127.0.0.1:{port}/pages/{page}/update")).json(&body).send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}
