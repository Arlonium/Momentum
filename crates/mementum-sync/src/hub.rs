//! Le registre des acteurs de page et l'entrée des connexions.
//!
//! Le registre associe à chaque page ouverte la boîte aux lettres de son
//! acteur et un numéro de génération. Un acteur qui s'arrête ne se retire que
//! si la génération inscrite est encore la sienne : jamais deux acteurs pour
//! une page, et jamais un acteur neuf retiré par l'ancien.
//!
//! Le hub ne connaît ni session ni droits : la route qui accepte une
//! connexion les vérifie et les lui résume dans [`ConnInfo`] ; quand ils
//! changent, elle appelle [`Hub::kick`], et [`Hub::forget`] quand la page est
//! supprimée.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use futures::{Sink, SinkExt, Stream, StreamExt};
use mementum_md::Block;
use mementum_schema::SCHEMA_VERSION;
use mementum_store::Store;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::actor::{self, Broadcast, Command, Commands, Event, Joined, Membership, Outcome};
use crate::protocol;
use crate::{SyncConfig, SyncError};

/// Essais d'une demande dont l'acteur s'est arrêté avant de répondre (un
/// acteur neuf est lancé à chaque essai).
const ATTEMPTS: usize = 8;

/// Délai laissé à une socket pour se fermer proprement (trame de fermeture).
const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// Attente maximale d'un acteur par [`Hub::kick`] et [`Hub::forget`] : passé
/// ce délai, sa demande reste dans sa boîte, et l'appelant n'attend plus.
const ASK_WAIT: Duration = Duration::from_secs(2);

/// Ce que la poignée de main sait d'une connexion. Le hub ne vérifie rien :
/// c'est à l'appelant de lire la session et les droits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnInfo {
    /// Version de schéma du client. Autre que [`SCHEMA_VERSION`], la
    /// connexion est en lecture seule.
    pub client_schema: u32,
    /// Le compte a le droit d'écrire la page. Sinon, la connexion est en
    /// lecture seule.
    pub can_write: bool,
    /// Le compte à qui sont attribuées les mises à jour de la connexion
    /// (`None` : inconnu).
    pub actor: Option<Uuid>,
}

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

    /// Les réglages du hub (la route WebSocket de la Task 6 y lit
    /// `max_message_bytes`).
    pub fn config(&self) -> &SyncConfig {
        &self.cfg
    }

    /// Crée le document d'une page neuve (`PageDoc::new_page`) et l'écrit en
    /// instantané de `seq` 0. Idempotent : l'acteur de la page le fait à son
    /// premier chargement si la page n'a pas encore de document (Ruling R28).
    pub async fn create_page_doc(&self, node_id: Uuid) -> Result<(), SyncError> {
        self.call(node_id, |reply| Command::Create { reply })
            .await?
    }

    /// Les blocs de la page, lus par son acteur.
    pub async fn read_blocks(&self, node_id: Uuid) -> Result<Vec<Block>, SyncError> {
        self.call(node_id, |reply| Command::Read { reply }).await?
    }

    /// Remplace les blocs de la page, au nom de `actor` (le compte qui demande
    /// l'écriture ; `None` : inconnu, ou le serveur). L'écriture est persistée,
    /// puis diffusée à toutes les connexions.
    pub async fn write_blocks(
        &self,
        node_id: Uuid,
        blocks: Vec<Block>,
        actor: Option<Uuid>,
    ) -> Result<(), SyncError> {
        self.call(node_id, |reply| Command::Write {
            blocks: blocks.clone(),
            actor,
            reply,
        })
        .await?
    }

    /// Sert une connexion au protocole de y-websocket jusqu'à sa fin.
    ///
    /// `Err(NotFound)` avant toute écriture si la page n'existe pas. La
    /// connexion écrit si `conn.client_schema` vaut [`SCHEMA_VERSION`] **et**
    /// si `conn.can_write` ; sinon elle est en lecture seule : elle reçoit
    /// tout, ses mises à jour sont ignorées. Les mises à jour qu'elle écrit
    /// sont attribuées à `conn.actor`. `conn` vaut pour toute la connexion :
    /// quand les droits changent, [`Hub::kick`] la ferme, et celle qui la
    /// remplace les relit.
    ///
    /// La connexion est fermée (et la fonction rend `Ok`) quand le client
    /// part, envoie un message illisible, trop gros ou une mise à jour
    /// refusée, prend trop de retard (`broadcast_capacity` messages ou
    /// `max_queue_bytes` octets), ou à la demande ([`Hub::kick`],
    /// [`Hub::forget`]) : il se reconnecte et se resynchronise.
    /// Toute fin anormale est journalisée par `tracing` (`warn`), avec la page,
    /// la connexion et sa raison.
    pub async fn serve_connection<S>(
        self: Arc<Self>,
        node_id: Uuid,
        conn: ConnInfo,
        socket: S,
    ) -> Result<(), SyncError>
    where
        S: Stream<Item = Vec<u8>> + Sink<Vec<u8>> + Unpin + Send + 'static,
    {
        let writable = conn.client_schema == SCHEMA_VERSION && conn.can_write;
        let actor = conn.actor;
        let joined = self
            .call(node_id, |reply| Command::Join {
                writable,
                actor,
                reply,
            })
            .await??;
        let conn = joined.membership.conn();
        match pump(socket, joined, &self.cfg).await {
            // Fin normale, à la demande, ou fermeture dont l'acteur a
            // journalisé la raison.
            end @ (End::Left | End::Refused | End::Kicked) => {
                tracing::debug!(node_id = %node_id, conn, reason = end.describe(), "connexion fermée");
            }
            end => {
                tracing::warn!(node_id = %node_id, conn, reason = end.describe(), "connexion fermée");
            }
        }
        Ok(())
    }

    /// Ferme toutes les connexions de ces pages (celles qui ont un acteur
    /// vivant). Les clients se reconnectent, et la poignée de main relit leurs
    /// droits.
    ///
    /// Le document n'est ni rechargé ni écrit : ce que ces connexions avaient
    /// envoyé, et que l'acteur avait traité, reste écrit. Une page sans acteur
    /// vivant (jamais ouverte, refermée, inconnue) est ignorée, sans erreur et
    /// sans lancer d'acteur.
    ///
    /// La fonction rend quand l'acteur de chaque page a fermé ses
    /// connexions : plus aucun de leurs messages n'est traité, et rien de ce
    /// qui est diffusé ensuite ne leur parvient ; leurs sockets se ferment
    /// dans la foulée. Elle n'attend jamais plus de deux secondes
    /// (`ASK_WAIT`) un acteur occupé (grosse page en cours de chargement,
    /// store lent) : la demande reste alors dans sa boîte, et les connexions
    /// sont fermées dès qu'il la lit.
    pub async fn kick(&self, node_ids: &[Uuid]) {
        self.ask_all(node_ids, |reply| Command::Kick { reply })
            .await;
    }

    /// Arrête sans rien écrire les acteurs de pages supprimées (ni instantané
    /// ni mise à jour : la page n'existe plus en base), et ferme leurs
    /// connexions. À appeler après la suppression en base : qui revient ne
    /// trouve plus la page. Une page sans acteur vivant est ignorée.
    ///
    /// La fonction rend quand chaque acteur s'est arrêté : il a quitté le
    /// registre, et les demandes qu'il n'avait pas encore lues sont lâchées.
    /// Comme [`Hub::kick`], elle n'attend jamais plus de deux secondes un
    /// acteur occupé, qui s'arrête alors dès qu'il lit la demande.
    pub async fn forget(&self, node_ids: &[Uuid]) {
        self.ask_all(node_ids, |reply| Command::Forget { reply })
            .await;
    }

    /// Les pages qui ont un acteur vivant, avec ou sans connexion.
    pub async fn live_pages(&self) -> Vec<Uuid> {
        self.lock()
            .iter()
            .filter(|(_, entry)| !entry.commands.is_closed())
            .map(|(node_id, _)| *node_id)
            .collect()
    }

    /// Envoie une demande à l'acteur vivant de chacune de ces pages, sans en
    /// lancer aucun, et attend qu'ils l'aient tous traitée, [`ASK_WAIT`] au
    /// plus.
    async fn ask_all(&self, node_ids: &[Uuid], make: impl Fn(oneshot::Sender<()>) -> Command) {
        let answers: Vec<oneshot::Receiver<()>> = node_ids
            .iter()
            .filter_map(|node_id| {
                let commands = self.live(*node_id)?;
                let (reply, answer) = oneshot::channel();
                // Boîte fermée à l'instant : l'acteur s'arrête, ses connexions
                // avec lui.
                commands.send(make(reply)).ok()?;
                Some(answer)
            })
            .collect();
        // Une réponse lâchée vaut réponse : l'acteur s'est arrêté avant de
        // lire la demande, et ses connexions sont fermées.
        let _ = tokio::time::timeout(ASK_WAIT, futures::future::join_all(answers)).await;
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
    pub(crate) fn unregister(&self, node_id: Uuid, generation: u64) {
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

/// Pourquoi une connexion s'est fermée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// Le client est parti (fin du flux, socket en erreur).
    Left,
    /// Message plus gros que `max_message_bytes`.
    TooBig,
    /// Message illisible.
    Illegible,
    /// Fermée à la demande de l'acteur, qui en a journalisé la raison.
    Refused,
    /// Fermée avec toutes celles de la page : [`Hub::kick`] ou [`Hub::forget`].
    Kicked,
    /// File vers la socket pleine (en messages ou en octets) : le client ne
    /// lit plus assez vite.
    QueueFull,
    /// Plus de `broadcast_capacity` diffusions arrivées pendant que la
    /// connexion attendait l'acteur (ses propres échos comptent).
    Lagged,
    /// L'acteur s'est arrêté ou a fermé la diffusion (Ruling R21).
    ActorGone,
}

impl End {
    fn describe(self) -> &'static str {
        match self {
            End::Left => "client parti",
            End::TooBig => "message trop gros",
            End::Illegible => "message illisible",
            End::Refused => "fermée par l'acteur",
            End::Kicked => "fermée à la demande (droits changés ou page supprimée)",
            End::QueueFull => "file vers le client pleine",
            End::Lagged => "client en retard sur la diffusion",
            End::ActorGone => "acteur arrêté ou diffusion close",
        }
    }
}

/// Message en file vers la socket : une diffusion partagée par toutes les
/// files (jamais copiée dans chacune), ou une réponse propre à la connexion.
enum Outgoing {
    Shared(Arc<Broadcast>),
    Own(Vec<u8>),
}

impl Outgoing {
    fn len(&self) -> usize {
        match self {
            Outgoing::Shared(b) => b.message.len(),
            Outgoing::Own(m) => m.len(),
        }
    }

    /// Les octets à envoyer : la seule copie d'une diffusion, faite au moment
    /// de l'écrire dans la socket.
    fn into_bytes(self) -> Vec<u8> {
        match self {
            Outgoing::Shared(b) => b.message.clone(),
            Outgoing::Own(m) => m,
        }
    }
}

/// File bornée vers la socket : `broadcast_capacity` messages, et
/// `max_queue_bytes` octets plus un message (une réponse plus grosse que la
/// borne, l'état complet d'une page par exemple, passe donc seule).
struct Outbox {
    tx: mpsc::Sender<Outgoing>,
    bytes: AtomicUsize,
    max_bytes: usize,
}

impl Outbox {
    /// `false` : la file est pleine, la connexion est à fermer.
    fn push(&self, message: Outgoing) -> bool {
        if self.bytes.load(Ordering::Relaxed) >= self.max_bytes {
            return false;
        }
        let len = message.len();
        self.bytes.fetch_add(len, Ordering::Relaxed);
        if self.tx.try_send(message).is_err() {
            // Le message n'est pas en file : le compte reste celui de la file.
            self.bytes.fetch_sub(len, Ordering::Relaxed);
            return false;
        }
        true
    }

    fn sent(&self, len: usize) {
        self.bytes.fetch_sub(len, Ordering::Relaxed);
    }
}

/// Fait passer les messages entre la socket et l'acteur jusqu'à la fin de la
/// connexion, et en rend la raison.
///
/// Un écrivain vide une file bornée vers la socket ; le lecteur traite les
/// messages du client (par l'acteur, un lot à la fois, dans l'ordre) et
/// remplit la file avec les réponses et la diffusion de la page. Un client qui
/// ne lit plus remplit sa file : il est fermé, au lieu de faire grossir la
/// mémoire du serveur ou de passer à côté du retard sur la diffusion.
async fn pump<S>(socket: S, joined: Joined, cfg: &SyncConfig) -> End
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
    let (tx, mut queue) = mpsc::channel(cfg.broadcast_capacity.max(1) + hello.len());
    let outbox = Outbox {
        tx,
        bytes: AtomicUsize::new(0),
        max_bytes: cfg.max_queue_bytes,
    };
    for message in hello {
        outbox.push(Outgoing::Own(message));
    }

    let writer = async {
        while let Some(message) = queue.recv().await {
            let len = message.len();
            let sent = sink.send(message.into_bytes()).await;
            outbox.sent(len);
            if sent.is_err() {
                break;
            }
        }
        End::Left
    };
    let reader = async {
        loop {
            tokio::select! {
                // La diffusion d'abord : sinon les échos de ses propres mises à
                // jour, qui occupent aussi la diffusion, mettraient en retard un
                // client qui écrit vite.
                biased;
                event = events.recv() => match event {
                    Ok(Event::Message(message)) if message.from == Some(conn) => {}
                    Ok(Event::Message(message)) => {
                        if !outbox.push(Outgoing::Shared(message)) {
                            return End::QueueFull;
                        }
                    }
                    Ok(Event::Close) => return End::Kicked,
                    Err(RecvError::Lagged(_)) => return End::Lagged,
                    Err(RecvError::Closed) => return End::ActorGone,
                },
                incoming = stream.next() => {
                    let Some(data) = incoming else { return End::Left };
                    // Avant tout décodage (Ruling R27).
                    if data.len() > cfg.max_message_bytes {
                        return End::TooBig;
                    }
                    let Ok(messages) = protocol::decode(&data) else {
                        return End::Illegible;
                    };
                    if messages.is_empty() {
                        continue;
                    }
                    if let Some(end) = exchange(&commands, conn, messages, &outbox).await {
                        return end;
                    }
                }
            }
        }
    };
    let end = tokio::select! {
        end = writer => end,
        end = reader => end,
    };
    drop(membership);
    let _ = tokio::time::timeout(CLOSE_GRACE, sink.close()).await;
    end
}

/// Confie des messages du client à l'acteur et met ses réponses en file.
/// `Some` : la connexion est à fermer, pour cette raison.
async fn exchange(
    commands: &Commands,
    conn: actor::ConnId,
    messages: Vec<protocol::ClientMessage>,
    outbox: &Outbox,
) -> Option<End> {
    let (reply, answer) = oneshot::channel();
    let command = Command::Messages {
        conn,
        messages,
        reply,
    };
    if commands.send(command).is_err() {
        return Some(End::ActorGone);
    }
    match answer.await {
        Ok(Outcome::Reply(replies)) => {
            if replies.into_iter().all(|m| outbox.push(Outgoing::Own(m))) {
                None
            } else {
                Some(End::QueueFull)
            }
        }
        Ok(Outcome::Close) => Some(End::Refused),
        Err(_) => Some(End::ActorGone),
    }
}

// Les types qui passent d'une tâche à l'autre.
const _: fn() = || {
    fn sendable<T: Send>() {}
    sendable::<Command>();
    sendable::<Event>();
    sendable::<Membership>();
};
