//! Outils de test partagés (feature `testkit`, Ruling R6).
//!
//! - [`TestServer`] : un petit serveur axum sur un port libre de `127.0.0.1`,
//!   qui monte [`Hub::serve_connection`] sur `GET /sync/{id}?schema=N` ;
//! - [`Client`] : un client Yjs en yrs, qui parle y-websocket comme un
//!   navigateur (étapes 1 et 2, mises à jour, awareness) et sait se connecter
//!   à n'importe quelle URL (le serveur de la Task 6 compris) ;
//! - [`memory_socket`] : une paire de sockets en mémoire, pour appeler
//!   `serve_connection` sans réseau.
//!
//! Ce sont des outils de test : ils paniquent sur une erreur d'installation.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use futures::channel::mpsc as memory;
use futures::{Sink, SinkExt, Stream, StreamExt};
use mementum_crdt::{CrdtError, FRAGMENT, PageDoc};
use mementum_md::Block;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message as TMessage;
use uuid::Uuid;
use yrs::sync::{Awareness, Message, MessageReader, SyncMessage};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::Encode;
use yrs::{
    Doc, ReadTxn, StateVector, Text, Transact, Update, Xml, XmlFragment, XmlOut, XmlTextPrelim,
};

use crate::Hub;
use crate::ws::WsBytes;

/// Origine des transactions qui appliquent ce que le serveur envoie : elles
/// ne repartent pas vers le serveur.
const REMOTE: &str = "remote";

// ---------------------------------------------------------------------------
// Serveur
// ---------------------------------------------------------------------------

/// Petit serveur axum de test : `GET /sync/{id}?schema=N` monte
/// [`Hub::serve_connection`]. Un `schema` absent ou illisible vaut 0.
pub struct TestServer {
    addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    alive: mpsc::Receiver<()>,
    task: JoinHandle<()>,
}

#[derive(Clone)]
struct AppState {
    hub: Arc<Hub>,
    shutdown: watch::Receiver<bool>,
    /// Chaque connexion en garde un clone : [`TestServer::stop`] attend qu'ils
    /// soient tous lâchés.
    alive: mpsc::Sender<()>,
}

impl TestServer {
    /// Démarre le serveur sur un port libre de `127.0.0.1`.
    pub async fn start(hub: Arc<Hub>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("un port libre sur 127.0.0.1");
        let addr = listener.local_addr().expect("l'adresse du serveur de test");
        let (shutdown, shutdown_rx) = watch::channel(false);
        let (alive_tx, alive) = mpsc::channel(1);
        let app = Router::new()
            .route("/sync/{id}", get(sync_route))
            .with_state(AppState {
                hub,
                shutdown: shutdown_rx,
                alive: alive_tx,
            });
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        TestServer {
            addr,
            shutdown,
            alive,
            task,
        }
    }

    /// Adresse d'écoute.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// URL WebSocket de la page `node_id` pour un client du schéma `schema`.
    pub fn url(&self, node_id: Uuid, schema: u32) -> String {
        format!("ws://{}/sync/{node_id}?schema={schema}", self.addr)
    }

    /// N'accepte plus de connexion, ferme celles qui restent et attend
    /// qu'elles soient finies (5 s au plus).
    pub async fn stop(mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
        let _ = (&mut self.task).await;
        let _ = tokio::time::timeout(Duration::from_secs(5), self.alive.recv()).await;
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        self.task.abort();
    }
}

async fn sync_route(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Response {
    let node_id = Uuid::parse_str(&id).unwrap_or(Uuid::nil());
    let schema = query
        .get("schema")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    // Comme le serveur de la Task 6 : axum refuse un message trop gros (R27).
    let max = state.hub.config().max_message_bytes;
    ws.max_message_size(max)
        .on_upgrade(move |socket| async move {
            let AppState {
                hub,
                mut shutdown,
                alive,
            } = state;
            tokio::select! {
                _ = hub.serve_connection(node_id, schema, WsBytes(socket)) => {}
                _ = shutdown.wait_for(|stop| *stop) => {}
            }
            drop(alive);
        })
}

// ---------------------------------------------------------------------------
// Socket en mémoire
// ---------------------------------------------------------------------------

/// Une extrémité d'une socket en mémoire, qui transporte des messages binaires.
pub struct MemorySocket {
    rx: memory::Receiver<Vec<u8>>,
    tx: memory::Sender<Vec<u8>>,
}

/// Deux extrémités reliées : ce que l'une envoie, l'autre le reçoit. Dans
/// chaque sens, `buffer + 1` messages attendent au plus d'être lus ; l'envoi
/// suivant attend. Une extrémité qui ne lit plus bloque donc celle d'en face.
pub fn memory_socket(buffer: usize) -> (MemorySocket, MemorySocket) {
    let (a_tx, b_rx) = memory::channel(buffer);
    let (b_tx, a_rx) = memory::channel(buffer);
    (
        MemorySocket { rx: a_rx, tx: a_tx },
        MemorySocket { rx: b_rx, tx: b_tx },
    )
}

impl Stream for MemorySocket {
    type Item = Vec<u8>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Vec<u8>>> {
        self.rx.poll_next_unpin(cx)
    }
}

impl Sink<Vec<u8>> for MemorySocket {
    type Error = memory::SendError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.tx.poll_ready_unpin(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> Result<(), Self::Error> {
        self.tx.start_send_unpin(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.tx.poll_flush_unpin(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.tx.poll_close_unpin(cx)
    }
}

// ---------------------------------------------------------------------------
// Client Yjs
// ---------------------------------------------------------------------------

/// Client Yjs de test, en yrs : un document local (`OffsetKind::Utf16`)
/// synchronisé au protocole y-websocket.
///
/// À la connexion, il envoie son étape 1 et répond à celle du serveur. Toute
/// transaction locale part vers le serveur ; ce que le serveur envoie est
/// appliqué sans repartir. Les tâches du client s'arrêtent quand il est lâché
/// (la connexion se coupe).
pub struct Client {
    doc: Doc,
    outbox: mpsc::UnboundedSender<Vec<u8>>,
    shared: Arc<Shared>,
    changes: watch::Receiver<u64>,
    tasks: Vec<JoinHandle<()>>,
}

struct Shared {
    /// Étapes 2 reçues du serveur.
    step2: AtomicU64,
    /// Plus rien ne viendra du serveur (connexion fermée ou message illisible).
    closed: AtomicBool,
    /// Awareness locale et reçue.
    awareness: Mutex<Awareness>,
    /// Incrémenté après chaque message traité et à la fermeture.
    changes: watch::Sender<u64>,
}

impl Shared {
    fn bump(&self) {
        self.changes.send_modify(|n| *n += 1);
    }

    fn awareness(&self) -> MutexGuard<'_, Awareness> {
        self.awareness
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl Client {
    /// Se connecte à `url` (`ws://…`) avec un document vide et lance la
    /// synchronisation. `Err` si la poignée de main WebSocket échoue (une
    /// réponse 404, par exemple).
    pub async fn connect(url: &str) -> Result<Client, String> {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| e.to_string())?;
        let (mut sink, mut stream) = ws.split();
        let doc = new_doc();
        let (changes_tx, changes) = watch::channel(0);
        let shared = Arc::new(Shared {
            step2: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            awareness: Mutex::new(Awareness::new(doc.clone())),
            changes: changes_tx,
        });
        let (outbox, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();

        // Les éditions locales partent vers le serveur ; ce qu'il envoie, non.
        let to_server = outbox.clone();
        doc.observe_update_v1("testkit", move |txn, event| {
            let remote = txn
                .origin()
                .is_some_and(|o| o.as_ref() == REMOTE.as_bytes());
            if !remote {
                let message = Message::Sync(SyncMessage::Update(event.update.clone()));
                let _ = to_server.send(message.encode_v1());
            }
        })
        .expect("l'observateur du client de test");
        let _ = outbox.send(sync_step1(&doc));

        let writer = tokio::spawn(async move {
            while let Some(bytes) = out_rx.recv().await {
                if sink.send(TMessage::Binary(bytes.into())).await.is_err() {
                    break;
                }
            }
        });
        let reader = {
            let doc = doc.clone();
            let shared = shared.clone();
            let reply = outbox.clone();
            tokio::spawn(async move {
                while let Some(Ok(message)) = stream.next().await {
                    let data = match message {
                        TMessage::Binary(data) => data,
                        TMessage::Close(_) => break,
                        _ => continue,
                    };
                    let ok = receive(&doc, &shared, &reply, &data);
                    shared.bump();
                    if !ok {
                        break;
                    }
                }
                shared.closed.store(true, Ordering::SeqCst);
                shared.bump();
            })
        };
        Ok(Client {
            doc,
            outbox,
            shared,
            changes,
            tasks: vec![writer, reader],
        })
    }

    /// Se connecte, puis attend la première étape 2 du serveur (10 s au plus).
    pub async fn connect_synced(url: &str) -> Client {
        let client = Client::connect(url).await.expect("connexion du client");
        assert!(
            client
                .wait_until(Duration::from_secs(10), |c| c.is_synced())
                .await,
            "le client n'a pas reçu l'état du serveur"
        );
        client
    }

    /// Le client a reçu au moins une étape 2 du serveur.
    pub fn is_synced(&self) -> bool {
        self.step2_count() > 0
    }

    /// Nombre d'étapes 2 reçues du serveur.
    pub fn step2_count(&self) -> u64 {
        self.shared.step2.load(Ordering::SeqCst)
    }

    /// Le serveur a fermé la connexion (ou envoyé un message illisible).
    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::SeqCst)
    }

    /// Attend que `cond` soit vraie, réévaluée après chaque message reçu,
    /// pendant `timeout` au plus. Renvoie la dernière valeur de `cond`.
    pub async fn wait_until(
        &self,
        timeout: Duration,
        mut cond: impl FnMut(&Client) -> bool,
    ) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut changes = self.changes.clone();
        loop {
            if cond(self) {
                return true;
            }
            match tokio::time::timeout_at(deadline, changes.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => return cond(self),
            }
        }
    }

    /// Identifiant Yjs du client.
    pub fn client_id(&self) -> u64 {
        self.doc.client_id().get()
    }

    /// Vecteur d'état du document local.
    pub fn state_vector(&self) -> StateVector {
        self.doc.transact().state_vector()
    }

    /// Instantané du document local, en `PageDoc`.
    pub fn page(&self) -> Result<PageDoc, CrdtError> {
        let state = self
            .doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default());
        PageDoc::from_updates([state.as_slice()])
    }

    /// Les blocs du document local (vide s'il est illisible pour l'instant,
    /// par exemple en attente d'une dépendance).
    pub fn blocks(&self) -> Vec<Block> {
        self.page().map(|p| p.read_blocks()).unwrap_or_default()
    }

    /// Applique `update` comme une édition locale : elle part vers le serveur.
    /// Sert à jouer chez ce client une édition préparée sur [`Client::page`].
    pub fn apply_local(&self, update: &[u8]) {
        let update = Update::decode_v1(update).expect("une mise à jour lisible");
        self.doc
            .transact_mut()
            .apply_update(update)
            .expect("une mise à jour applicable");
    }

    /// Tape `text` à la fin du texte du bloc `block_id`, en une transaction,
    /// comme un éditeur : le `XmlText` du nœud de contenu est créé s'il manque.
    pub fn type_text(&self, block_id: &str, text: &str) {
        // Avant toute transaction (interblocage de yrs avec la feature `sync`).
        let fragment = self.doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = self.doc.transact_mut();
        let container = fragment
            .successors(&txn)
            .find_map(|node| match node {
                XmlOut::Element(e)
                    if e.tag().as_ref() == "blockContainer"
                        && matches!(
                            e.get_attribute(&txn, "id"),
                            Some(yrs::Out::Any(yrs::Any::String(id))) if id.as_ref() == block_id
                        ) =>
                {
                    Some(e)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("aucun bloc « {block_id} » chez ce client"));
        let Some(XmlOut::Element(content)) = container.get(&txn, 0) else {
            panic!("le bloc « {block_id} » n'a pas de nœud de contenu");
        };
        let node = match content.get(&txn, 0) {
            Some(XmlOut::Text(node)) => node,
            _ => content.insert(&mut txn, 0, XmlTextPrelim::new("")),
        };
        let end = node.len(&txn);
        node.insert(&mut txn, end, text);
    }

    /// Envoie un message y-websocket tel quel.
    pub fn send_raw(&self, message: Vec<u8>) {
        let _ = self.outbox.send(message);
    }

    /// Envoie une mise à jour Yjs (v1) telle quelle, sans l'appliquer chez soi.
    pub fn send_update(&self, update: &[u8]) {
        self.send_raw(Message::Sync(SyncMessage::Update(update.to_vec())).encode_v1());
    }

    /// Renvoie une étape 1 : le serveur répond par une étape 2 quand il a
    /// traité tout ce que ce client a envoyé avant (une connexion est servie
    /// dans l'ordre).
    pub fn resync(&self) {
        self.send_raw(sync_step1(&self.doc));
    }

    /// Publie l'état d'awareness local (JSON).
    pub fn set_awareness(&self, json: &str) {
        let message = {
            let mut awareness = self.shared.awareness();
            awareness.set_local_state_raw(json);
            let update = awareness
                .update_with_clients([awareness.client_id()])
                .expect("l'état local vient d'être posé");
            Message::Awareness(update).encode_v1()
        };
        self.send_raw(message);
    }

    /// États d'awareness des autres clients, reçus du serveur : id → JSON.
    pub fn awareness_states(&self) -> HashMap<u64, String> {
        let awareness = self.shared.awareness();
        let own = awareness.client_id();
        awareness
            .iter()
            .filter(|(id, _)| *id != own)
            .filter_map(|(id, state)| Some((id.get(), state.data?.to_string())))
            .collect()
    }
}

/// Un document yrs comme ceux des navigateurs (`OffsetKind::Utf16`).
pub fn new_doc() -> Doc {
    crate::utf16_doc()
}

fn sync_step1(doc: &Doc) -> Vec<u8> {
    Message::Sync(SyncMessage::SyncStep1(doc.transact().state_vector())).encode_v1()
}

/// Traite un message du serveur. `false` s'il est illisible ou inapplicable.
fn receive(
    doc: &Doc,
    shared: &Shared,
    reply: &mpsc::UnboundedSender<Vec<u8>>,
    data: &[u8],
) -> bool {
    let apply = |update: &[u8]| -> bool {
        let Ok(update) = Update::decode_v1(update) else {
            return false;
        };
        doc.transact_mut_with(REMOTE).apply_update(update).is_ok()
    };
    let mut decoder = DecoderV1::from(data);
    for message in MessageReader::new(&mut decoder) {
        match message {
            Ok(Message::Sync(SyncMessage::SyncStep1(sv))) => {
                let update = doc.transact().encode_state_as_update_v1(&sv);
                let _ = reply.send(Message::Sync(SyncMessage::SyncStep2(update)).encode_v1());
            }
            Ok(Message::Sync(SyncMessage::SyncStep2(update))) => {
                if !apply(&update) {
                    return false;
                }
                shared.step2.fetch_add(1, Ordering::SeqCst);
            }
            Ok(Message::Sync(SyncMessage::Update(update))) => {
                if !apply(&update) {
                    return false;
                }
            }
            Ok(Message::Awareness(update)) => {
                if shared.awareness().apply_update(update).is_err() {
                    return false;
                }
            }
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    true
}
