//! Adaptateur WebSocket d'axum (feature `axum`).
//!
//! [`Hub::serve_connection`](crate::Hub::serve_connection) attend un flux de
//! messages binaires (`Stream<Item = Vec<u8>> + Sink<Vec<u8>>`). [`WsBytes`]
//! y ramène la WebSocket d'axum : les messages binaires passent, le texte, les
//! pings et les pongs sont ignorés (tungstenite répond seul aux pings), une
//! trame de fermeture ou une erreur termine le flux.
//!
//! Côté route, régler `WebSocketUpgrade::max_message_size` sur
//! [`SyncConfig::max_message_bytes`](crate::SyncConfig::max_message_bytes)
//! (Ruling R27) : axum refuse alors un message trop gros avant de le garder
//! en entier.

use std::pin::Pin;
use std::task::{Context, Poll};

use axum::extract::ws::{Message, WebSocket};
use futures::{Sink, SinkExt, Stream, StreamExt};

/// La WebSocket d'axum vue comme un flux de messages binaires.
pub struct WsBytes(pub WebSocket);

impl From<WebSocket> for WsBytes {
    fn from(socket: WebSocket) -> Self {
        WsBytes(socket)
    }
}

impl Stream for WsBytes {
    type Item = Vec<u8>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Vec<u8>>> {
        loop {
            match self.0.poll_next_unpin(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(Message::Binary(bytes)))) => {
                    return Poll::Ready(Some(bytes.to_vec()));
                }
                Poll::Ready(Some(Ok(Message::Close(_)) | Err(_)) | None) => {
                    return Poll::Ready(None);
                }
                // Texte, ping, pong : rien pour y-websocket.
                Poll::Ready(Some(Ok(_))) => {}
            }
        }
    }
}

impl Sink<Vec<u8>> for WsBytes {
    type Error = axum::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready_unpin(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        self.0.start_send_unpin(Message::Binary(item.into()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_flush_unpin(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_close_unpin(cx)
    }
}
