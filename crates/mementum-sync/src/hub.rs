//! Le registre des acteurs de page et l'entrée des connexions.
//!
//! Le registre associe à chaque page ouverte la boîte aux lettres de son
//! acteur et un numéro de génération. Un acteur qui s'arrête ne se retire que
//! si la génération inscrite est encore la sienne : jamais deux acteurs pour
//! une page, et jamais un acteur neuf retiré par l'ancien.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use futures::{Sink, SinkExt, Stream, StreamExt};
use mementum_md::Block;
use mementum_schema::SCHEMA_VERSION;
use mementum_store::Store;
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::actor::{self, Broadcast, Command, Commands, Joined, Membership, Outcome};
use crate::protocol;
use crate::{SyncConfig, SyncError};

/// Essais d'une demande dont l'acteur s'est arrêté avant de répondre (un
/// acteur neuf est lancé à chaque essai).
const ATTEMPTS: usize = 8;

/// Délai laissé à une socket pour se fermer proprement (trame de fermeture).
const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// Le registre des acteurs de page.
pub struct Hub {
    store: Store,
    cfg: SyncConfig,
    actors: Mutex<HashMap<Uuid, Entry>>,
    generations: AtomicU64,
    /// Le hub lui-même, sans le garder en vie : les acteurs s'en servent pour
    /// quitter le registre.
    me: Weak<Hub>,
}

struct Entry {
    generation: u64,
    commands: Commands,
}

impl Hub {
    pub fn new(store: Store, cfg: SyncConfig) -> Arc<Hub> {
        Arc::new_cyclic(|me| Hub {
            store,
            cfg,
            actors: Mutex::new(HashMap::new()),
            generations: AtomicU64::new(0),
            me: me.clone(),
        })
    }

    /// Crée le document d'une page neuve (`PageDoc::new_page`) et l'écrit en
    /// instantané de `seq` 0. Sans effet si la page a déjà un document.
    pub async fn create_page_doc(&self, node_id: Uuid) -> Result<(), SyncError> {
        self.call(node_id, |reply| Command::Create { reply })
            .await?
    }

    /// Les blocs de la page, lus par son acteur.
    pub async fn read_blocks(&self, node_id: Uuid) -> Result<Vec<Block>, SyncError> {
        self.call(node_id, |reply| Command::Read { reply }).await?
    }

    /// Remplace les blocs de la page. L'écriture est persistée, puis diffusée
    /// à toutes les connexions.
    pub async fn write_blocks(&self, node_id: Uuid, blocks: Vec<Block>) -> Result<(), SyncError> {
        self.call(node_id, |reply| Command::Write {
            blocks: blocks.clone(),
            reply,
        })
        .await?
    }

    /// Sert une connexion au protocole de y-websocket jusqu'à sa fin.
    ///
    /// `Err(NotFound)` avant toute écriture si la page n'existe pas. Un
    /// `client_schema` différent de [`SCHEMA_VERSION`] donne une connexion en
    /// lecture seule : elle reçoit tout, ses mises à jour sont ignorées.
    ///
    /// La connexion est fermée (et la fonction rend `Ok`) quand le client
    /// part, envoie un message illisible ou une mise à jour refusée, ou prend
    /// plus de `broadcast_capacity` messages de retard : il se reconnecte et
    /// se resynchronise.
    pub async fn serve_connection<S>(
        self: Arc<Self>,
        node_id: Uuid,
        client_schema: u32,
        socket: S,
    ) -> Result<(), SyncError>
    where
        S: Stream<Item = Vec<u8>> + Sink<Vec<u8>> + Unpin + Send + 'static,
    {
        let writable = client_schema == SCHEMA_VERSION;
        let joined = self
            .call(node_id, |reply| Command::Join { writable, reply })
            .await??;
        pump(socket, joined, self.cfg.broadcast_capacity.max(1)).await;
        Ok(())
    }

    /// Envoie une demande à l'acteur de la page et attend sa réponse. Un
    /// acteur qui s'arrête lâche les demandes reçues sans les traiter : elles
    /// repartent vers un acteur neuf.
    async fn call<T>(
        &self,
        node_id: Uuid,
        make: impl Fn(oneshot::Sender<T>) -> Command,
    ) -> Result<T, SyncError> {
        for _ in 0..ATTEMPTS {
            let commands = self.actor(node_id).await?;
            let (reply, answer) = oneshot::channel();
            if commands.send(make(reply)).is_err() {
                continue;
            }
            if let Ok(value) = answer.await {
                return Ok(value);
            }
        }
        Err(SyncError::Closed)
    }

    /// La boîte aux lettres de l'acteur de la page, lancé au besoin.
    async fn actor(&self, node_id: Uuid) -> Result<Commands, SyncError> {
        if let Some(commands) = self.live(node_id) {
            return Ok(commands);
        }
        // Ni acteur ni écriture pour une page qui n'existe pas.
        if self.store.get_node(node_id).await?.is_none() {
            return Err(SyncError::NotFound);
        }
        let mut actors = self.lock();
        if let Some(entry) = actors.get(&node_id)
            && !entry.commands.is_closed()
        {
            return Ok(entry.commands.clone());
        }
        let generation = self.generations.fetch_add(1, Ordering::Relaxed);
        let commands = actor::spawn(
            node_id,
            generation,
            self.store.clone(),
            self.cfg,
            self.me.clone(),
        );
        actors.insert(
            node_id,
            Entry {
                generation,
                commands: commands.clone(),
            },
        );
        Ok(commands)
    }

    fn live(&self, node_id: Uuid) -> Option<Commands> {
        self.lock()
            .get(&node_id)
            .filter(|entry| !entry.commands.is_closed())
            .map(|entry| entry.commands.clone())
    }

    /// Retire l'acteur de la page du registre, s'il est de cette génération.
    pub(crate) fn forget(&self, node_id: Uuid, generation: u64) {
        let mut actors = self.lock();
        if actors
            .get(&node_id)
            .is_some_and(|entry| entry.generation == generation)
        {
            actors.remove(&node_id);
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<Uuid, Entry>> {
        self.actors.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Fait passer les messages entre la socket et l'acteur jusqu'à la fin de la
/// connexion.
///
/// Un écrivain vide une file bornée vers la socket ; le lecteur traite les
/// messages du client (par l'acteur, un à la fois, dans l'ordre) et remplit la
/// file avec les réponses et la diffusion de la page. Un client qui ne lit
/// plus remplit sa file : il est fermé, au lieu de faire grossir la mémoire
/// du serveur ou de passer à côté du retard sur la diffusion.
async fn pump<S>(socket: S, joined: Joined, capacity: usize)
where
    S: Stream<Item = Vec<u8>> + Sink<Vec<u8>> + Unpin + Send + 'static,
{
    let Joined {
        mut events,
        hello,
        membership,
    } = joined;
    let conn = membership.conn();
    let commands = membership.commands().clone();
    let (mut sink, mut stream) = socket.split();
    let (out, mut queue) = mpsc::channel::<Vec<u8>>(capacity + hello.len());
    for message in hello {
        let _ = out.try_send(message);
    }

    let writer = async {
        while let Some(message) = queue.recv().await {
            if sink.send(message).await.is_err() {
                break;
            }
        }
    };
    let reader = async {
        loop {
            tokio::select! {
                // La diffusion d'abord : sinon les échos de ses propres mises à
                // jour, qui occupent aussi la diffusion, mettraient en retard un
                // client qui écrit vite.
                biased;
                event = events.recv() => match event {
                    Ok(event) if event.from == Some(conn) => {}
                    Ok(event) => {
                        // File pleine : le client ne lit plus assez vite.
                        if out.try_send(event.message.clone()).is_err() {
                            break;
                        }
                    }
                    // En retard sur la diffusion, ou diffusion close par l'acteur.
                    Err(broadcast::error::RecvError::Lagged(_) | broadcast::error::RecvError::Closed) => break,
                },
                incoming = stream.next() => {
                    let Some(data) = incoming else { break };
                    let Ok(messages) = protocol::decode(&data) else { break };
                    if messages.is_empty() {
                        continue;
                    }
                    if !exchange(&commands, conn, messages, &out).await {
                        break;
                    }
                }
            }
        }
    };
    tokio::select! {
        () = writer => {}
        () = reader => {}
    }
    drop(membership);
    let _ = tokio::time::timeout(CLOSE_GRACE, sink.close()).await;
}

/// Confie des messages du client à l'acteur et met ses réponses en file.
/// `false` : la connexion est à fermer.
async fn exchange(
    commands: &Commands,
    conn: actor::ConnId,
    messages: Vec<protocol::ClientMessage>,
    out: &mpsc::Sender<Vec<u8>>,
) -> bool {
    let (reply, answer) = oneshot::channel();
    if commands
        .send(Command::Messages {
            conn,
            messages,
            reply,
        })
        .is_err()
    {
        return false;
    }
    match answer.await {
        Ok(Outcome::Reply(replies)) => replies.into_iter().all(|m| out.try_send(m).is_ok()),
        Ok(Outcome::Close) | Err(_) => false,
    }
}

// Les types qui passent d'une tâche à l'autre.
const _: fn() = || {
    fn sendable<T: Send>() {}
    sendable::<Command>();
    sendable::<Arc<Broadcast>>();
    sendable::<Membership>();
};
