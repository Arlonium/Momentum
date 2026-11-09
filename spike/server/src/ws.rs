//! WebSocket `/sync/{page}` au protocole y-websocket (0 = sync, 1 = awareness).

use std::sync::Arc;

use axum::extract::ws::{Message as WsMsg, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use yrs::sync::{Message, MessageReader, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{ReadTxn, Transact, Update};

use crate::rooms::{next_conn_id, Room, Rooms};

pub async fn sync_handler(ws: WebSocketUpgrade, Path(page): Path<String>, State(rooms): State<Rooms>) -> Response {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_socket(socket, page, rooms).await {
            eprintln!("ws: {e:#}");
        }
    })
}

async fn handle_socket(socket: WebSocket, page: String, rooms: Rooms) -> anyhow::Result<()> {
    let room = rooms.get_or_load(&page).await?;
    room.client_joined();
    let conn = next_conn_id();
    let result = run(socket, &room, conn).await;
    if room.client_left() == 0 {
        rooms.schedule_release(room.clone());
    }
    result
}

async fn run(socket: WebSocket, room: &Arc<Room>, conn: u64) -> anyhow::Result<()> {
    let (mut sink, mut stream) = socket.split();
    let mut rx = room.tx.subscribe();

    // Ouverture : étape 1 du serveur + présence actuelle.
    let hello = {
        let aw = room.awareness.lock().await;
        let sv = aw.doc().transact().state_vector();
        let mut out = vec![Message::Sync(SyncMessage::SyncStep1(sv)).encode_v1()];
        if let Ok(update) = aw.update() {
            out.push(Message::Awareness(update).encode_v1());
        }
        out
    };
    for m in hello {
        sink.send(WsMsg::Binary(m.into())).await?;
    }

    loop {
        tokio::select! {
            incoming = stream.next() => {
                let Some(Ok(msg)) = incoming else { break };
                let data = match msg {
                    WsMsg::Binary(b) => b,
                    WsMsg::Close(_) => break,
                    _ => continue,
                };
                for reply in handle_message(room, conn, &data).await? {
                    sink.send(WsMsg::Binary(reply.into())).await?;
                }
            }
            out = rx.recv() => {
                match out {
                    Ok((from, bytes)) if from != conn => sink.send(WsMsg::Binary(bytes.into())).await?,
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        }
    }
    Ok(())
}

/// Traite un message client et renvoie les réponses destinées à ce seul client.
async fn handle_message(room: &Arc<Room>, conn: u64, data: &[u8]) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut replies = Vec::new();
    let mut updates = Vec::new();
    {
        let mut aw = room.awareness.lock().await;
        let mut decoder = DecoderV1::from(data);
        for msg in MessageReader::new(&mut decoder) {
            match msg? {
                Message::Sync(SyncMessage::SyncStep1(sv)) => {
                    let update = aw.doc().transact().encode_state_as_update_v1(&sv);
                    replies.push(Message::Sync(SyncMessage::SyncStep2(update)).encode_v1());
                }
                Message::Sync(SyncMessage::SyncStep2(u)) | Message::Sync(SyncMessage::Update(u)) => {
                    let update = Update::decode_v1(&u)?;
                    // Une poignée de main d'un document vide ne change rien : ni stockage ni diffusion.
                    if update.is_empty() {
                        continue;
                    }
                    aw.doc().transact_mut_with("client").apply_update(update)?;
                    updates.push(u);
                }
                Message::Awareness(update) => {
                    let bytes = Message::Awareness(update.clone()).encode_v1();
                    aw.apply_update(update)?;
                    let _ = room.tx.send((conn, bytes));
                }
                Message::AwarenessQuery => {
                    replies.push(Message::Awareness(aw.update()?).encode_v1());
                }
                _ => {}
            }
        }
    }
    for u in updates {
        room.commit_update(conn, u, "client").await?;
    }
    Ok(replies)
}
