//! L'acteur d'une page : une tâche tokio qui possède le `PageDoc` de la page
//! et traite ses commandes une par une.
//!
//! Invariant : entre deux commandes, le document vivant est exactement
//! l'instantané du store suivi de son journal. Toute mise à jour appliquée est
//! écrite (`append_update` attendu) avant la commande suivante et avant d'être
//! diffusée ; une mise à jour refusée ou non écrite fait recharger le document
//! depuis le store. D'où une compaction sûre : l'instantané pris entre deux
//! commandes couvre exactement les mises à jour écrites jusqu'à son `seq`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use mementum_crdt::PageDoc;
use mementum_md::Block;
use mementum_store::{DocState, Store, StoreError};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, WeakUnboundedSender};
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;
use yrs::sync::{Awareness, AwarenessUpdate};
use yrs::updates::encoder::Encode;
use yrs::{ClientID, Doc, OffsetKind, Options};

use crate::hub::Hub;
use crate::protocol::{self, ClientMessage, EMPTY_UPDATE};
use crate::{SyncConfig, SyncError};

/// Identifiant d'une connexion, propre à l'acteur.
pub(crate) type ConnId = u64;

/// Boîte aux lettres d'un acteur.
pub(crate) type Commands = UnboundedSender<Command>;

/// États d'awareness qu'une connexion peut tenir (un onglet n'en a qu'un) :
/// au-delà, son message d'awareness est refusé et la connexion fermée.
const MAX_AWARENESS_CLIENTS: usize = 8;

/// Message diffusé aux connexions de la page.
pub(crate) struct Broadcast {
    /// Connexion d'origine, qui ne le reçoit pas ; `None` : toutes le reçoivent.
    pub(crate) from: Option<ConnId>,
    /// Message y-websocket.
    pub(crate) message: Vec<u8>,
}

/// Ce que l'acteur sait faire.
pub(crate) enum Command {
    /// Inscrire une connexion ; `writable` est faux pour un autre schéma.
    Join {
        writable: bool,
        reply: oneshot::Sender<Result<Joined, SyncError>>,
    },
    /// Désinscrire une connexion (envoyé par [`Membership`]).
    Leave { conn: ConnId },
    /// Messages d'une connexion, dans l'ordre où elle les a envoyés.
    Messages {
        conn: ConnId,
        messages: Vec<ClientMessage>,
        reply: oneshot::Sender<Outcome>,
    },
    /// Créer le document d'une page neuve.
    Create {
        reply: oneshot::Sender<Result<(), SyncError>>,
    },
    /// Lire les blocs.
    Read {
        reply: oneshot::Sender<Result<Vec<Block>, SyncError>>,
    },
    /// Remplacer les blocs (écriture serveur).
    Write {
        blocks: Vec<Block>,
        reply: oneshot::Sender<Result<(), SyncError>>,
    },
}

impl Command {
    /// Répond par `error` (document impossible à charger).
    fn fail(self, error: SyncError) {
        match self {
            Command::Join { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Command::Create { reply } | Command::Write { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Command::Read { reply } => {
                let _ = reply.send(Err(error));
            }
            Command::Messages { reply, .. } => {
                let _ = reply.send(Outcome::Close);
            }
            Command::Leave { .. } => {}
        }
    }
}

/// Réponse à [`Command::Join`].
pub(crate) struct Joined {
    /// Diffusion de la page, abonnée au moment de l'inscription.
    pub(crate) events: broadcast::Receiver<Arc<Broadcast>>,
    /// Accueil : l'étape 1 du serveur et les états d'awareness connus.
    pub(crate) hello: Vec<Vec<u8>>,
    pub(crate) membership: Membership,
}

/// Inscription d'une connexion. La lâcher la désinscrit, même si la connexion
/// est abandonnée en route (tâche annulée, réponse jamais lue).
pub(crate) struct Membership {
    conn: ConnId,
    commands: Commands,
}

impl Membership {
    pub(crate) fn conn(&self) -> ConnId {
        self.conn
    }

    pub(crate) fn commands(&self) -> &Commands {
        &self.commands
    }
}

impl Drop for Membership {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Leave { conn: self.conn });
    }
}

/// Suite à donner à des messages d'une connexion.
pub(crate) enum Outcome {
    /// Réponses à envoyer à cette connexion seulement.
    Reply(Vec<Vec<u8>>),
    /// Fermer la connexion.
    Close,
}

/// Le document n'a pas pu être rechargé : l'acteur s'arrête.
struct Broken;

/// Lance l'acteur de la page `node_id` et renvoie sa boîte aux lettres.
pub(crate) fn spawn(
    node_id: Uuid,
    generation: u64,
    store: Store,
    cfg: SyncConfig,
    hub: Weak<Hub>,
) -> Commands {
    let (commands, rx) = mpsc::unbounded_channel();
    let me = commands.downgrade();
    tokio::spawn(run(
        Identity {
            node_id,
            generation,
            hub,
            me,
        },
        store,
        cfg,
        rx,
    ));
    commands
}

/// Qui est l'acteur, pour le registre.
struct Identity {
    node_id: Uuid,
    generation: u64,
    hub: Weak<Hub>,
    /// Sa propre boîte aux lettres, sans la garder ouverte.
    me: WeakUnboundedSender<Command>,
}

impl Identity {
    /// Ferme la boîte aux lettres et quitte le registre. Les commandes déjà
    /// reçues ne sont pas traitées : leur demandeur voit sa réponse abandonnée
    /// et réessaie auprès d'un nouvel acteur, qui relira le store.
    fn retire(&self, rx: &mut UnboundedReceiver<Command>) -> Vec<Command> {
        rx.close();
        if let Some(hub) = self.hub.upgrade() {
            hub.forget(self.node_id, self.generation);
        }
        let mut pending = Vec::new();
        while let Ok(command) = rx.try_recv() {
            pending.push(command);
        }
        pending
    }
}

async fn run(id: Identity, store: Store, cfg: SyncConfig, mut rx: UnboundedReceiver<Command>) {
    match load(&store, id.node_id).await {
        Ok(loaded) => Actor::new(id, store, cfg, loaded).serve(rx).await,
        Err(error) => {
            // Le premier demandeur reçoit l'erreur, les autres réessaient.
            let mut pending = id.retire(&mut rx).into_iter();
            if let Some(first) = pending.next() {
                first.fail(error);
            }
        }
    }
}

/// Document relu depuis le store.
struct Loaded {
    page: PageDoc,
    /// La page a un document (instantané ou mises à jour).
    created: bool,
    /// Plus grand `seq` écrit (instantané ou journal).
    last_seq: i64,
    /// Mises à jour du journal depuis l'instantané.
    since_snapshot: u64,
}

/// Relit l'instantané, puis les mises à jour dans l'ordre des `seq`.
async fn load(store: &Store, node_id: Uuid) -> Result<Loaded, SyncError> {
    let DocState { snapshot, updates } = store.load_doc(node_id).await?;
    let created = snapshot.is_some() || !updates.is_empty();
    let last_seq = updates
        .last()
        .or(snapshot.as_ref())
        .map_or(0, |(seq, _)| *seq);
    let page = PageDoc::from_updates(
        snapshot
            .iter()
            .chain(updates.iter())
            .map(|(_, bytes)| bytes.as_slice()),
    )?;
    Ok(Loaded {
        page,
        created,
        last_seq,
        since_snapshot: updates.len() as u64,
    })
}

struct Conn {
    /// Faux pour un client d'un autre schéma : ses mises à jour sont ignorées.
    writable: bool,
    /// La connexion doit se fermer (Ruling R21) : ses messages ne sont plus traités.
    closing: bool,
    /// Clients d'awareness annoncés par cette connexion.
    clients: HashSet<ClientID>,
}

struct Actor {
    id: Identity,
    store: Store,
    cfg: SyncConfig,
    page: PageDoc,
    created: bool,
    last_seq: i64,
    since_snapshot: u64,
    /// Awareness des clients, sur un document à part : elle ne touche jamais
    /// au contenu, et survit au rechargement du document.
    awareness: Awareness,
    events: broadcast::Sender<Arc<Broadcast>>,
    conns: HashMap<ConnId, Conn>,
    next_conn: ConnId,
}

impl Actor {
    fn new(id: Identity, store: Store, cfg: SyncConfig, loaded: Loaded) -> Actor {
        let awareness = Awareness::new(Doc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        }));
        let (events, _) = broadcast::channel(cfg.broadcast_capacity.max(1));
        Actor {
            id,
            store,
            cfg,
            page: loaded.page,
            created: loaded.created,
            last_seq: loaded.last_seq,
            since_snapshot: loaded.since_snapshot,
            awareness,
            events,
            conns: HashMap::new(),
            next_conn: 0,
        }
    }

    fn install(&mut self, loaded: Loaded) {
        self.page = loaded.page;
        self.created = loaded.created;
        self.last_seq = loaded.last_seq;
        self.since_snapshot = loaded.since_snapshot;
    }

    async fn serve(mut self, mut rx: UnboundedReceiver<Command>) {
        loop {
            let next = if self.conns.is_empty() {
                match tokio::time::timeout(self.cfg.idle_close, rx.recv()).await {
                    Ok(next) => next,
                    // Ni connexion ni demande pendant `idle_close`.
                    Err(_) => break,
                }
            } else {
                rx.recv().await
            };
            // Plus personne ne peut écrire à l'acteur (hub lâché, connexions finies).
            let Some(command) = next else { return };
            if self.handle(command).await.is_err() {
                break;
            }
        }
        // Les commandes reçues entre-temps sont lâchées sans réponse : leurs
        // demandeurs réessaient auprès d'un nouvel acteur.
        drop(self.id.retire(&mut rx));
    }

    async fn handle(&mut self, command: Command) -> Result<(), Broken> {
        match command {
            Command::Join { writable, reply } => self.join(writable, reply),
            Command::Leave { conn } => self.leave(conn),
            Command::Messages {
                conn,
                messages,
                reply,
            } => {
                let outcome = self.messages(conn, messages).await?;
                let _ = reply.send(outcome);
            }
            Command::Create { reply } => {
                let _ = reply.send(self.create().await);
            }
            Command::Read { reply } => {
                let _ = reply.send(Ok(self.page.read_blocks()));
            }
            Command::Write { blocks, reply } => {
                let result = self.write(blocks).await?;
                let _ = reply.send(result);
            }
        }
        Ok(())
    }

    fn join(&mut self, writable: bool, reply: oneshot::Sender<Result<Joined, SyncError>>) {
        let Some(commands) = self.id.me.upgrade() else {
            let _ = reply.send(Err(SyncError::Closed));
            return;
        };
        let conn = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(
            conn,
            Conn {
                writable,
                closing: false,
                clients: HashSet::new(),
            },
        );
        let mut hello = vec![protocol::sync_step1(&self.page.state_vector())];
        hello.extend(self.awareness_state());
        // Si le demandeur est déjà parti, la réponse lâchée lâche `Membership`,
        // qui désinscrit la connexion.
        let _ = reply.send(Ok(Joined {
            events: self.events.subscribe(),
            hello,
            membership: Membership { conn, commands },
        }));
    }

    fn leave(&mut self, conn: ConnId) {
        let Some(gone) = self.conns.remove(&conn) else {
            return;
        };
        // Un client repris entre-temps par une autre connexion reste annoncé.
        let ids: Vec<ClientID> = gone
            .clients
            .into_iter()
            .filter(|id| !self.conns.values().any(|c| c.clients.contains(id)))
            .collect();
        if ids.is_empty() {
            return;
        }
        for id in &ids {
            self.awareness.remove_state(*id);
        }
        if let Ok(update) = self.awareness.update_with_clients(ids) {
            self.broadcast(None, protocol::awareness(&update.encode_v1()));
        }
    }

    async fn messages(
        &mut self,
        conn: ConnId,
        messages: Vec<ClientMessage>,
    ) -> Result<Outcome, Broken> {
        let writable = match self.conns.get(&conn) {
            Some(c) if !c.closing => c.writable,
            _ => return Ok(Outcome::Close),
        };
        let mut replies = Vec::new();
        for message in messages {
            match message {
                ClientMessage::SyncStep1(state_vector) => {
                    match self.page.encode_diff(&state_vector) {
                        Ok(diff) => replies.push(protocol::sync_step2(&diff)),
                        Err(_) => return Ok(Outcome::Close),
                    }
                }
                // Lecture seule : ni appliquée, ni écrite, ni diffusée.
                ClientMessage::Update(_) if !writable => {}
                ClientMessage::Update(update) if update == EMPTY_UPDATE => {}
                ClientMessage::Update(update) => {
                    if !self.client_update(conn, &update).await? {
                        return Ok(Outcome::Close);
                    }
                }
                ClientMessage::Awareness { update, raw } => {
                    if !self.awareness_update(conn, update, &raw) {
                        return Ok(Outcome::Close);
                    }
                }
                ClientMessage::AwarenessQuery => replies.extend(self.awareness_state()),
            }
        }
        Ok(Outcome::Reply(replies))
    }

    /// Mise à jour d'un client : appliquer, dédoublonner, écrire, puis
    /// diffuser. `false` : la connexion est à fermer.
    async fn client_update(&mut self, conn: ConnId, update: &[u8]) -> Result<bool, Broken> {
        if self.page.apply_update(update).is_err() {
            // Rulings R17 et R22 : ni écrite ni diffusée ; le document, peut-être
            // entamé, est rechargé depuis le store ; le client fautif est fermé.
            // Il se reconnecte, et sa synchro complète apporte ce qui manquait.
            self.reload().await?;
            return Ok(false);
        }
        // Après des déplacements concurrents, un bloc peut exister en double.
        let fix = self.page.dedupe_blocks();
        for written in std::iter::once(update).chain(fix.as_deref()) {
            if self.persist(written).await.is_err() {
                self.store_failed().await?;
                return Ok(false);
            }
        }
        self.broadcast(Some(conn), protocol::update(update));
        if let Some(fix) = fix {
            // À toutes les connexions, l'expéditeur compris.
            self.broadcast(None, protocol::update(&fix));
        }
        self.compact_if_due().await;
        Ok(true)
    }

    fn awareness_update(&mut self, conn: ConnId, update: AwarenessUpdate, raw: &[u8]) -> bool {
        let Some(c) = self.conns.get_mut(&conn) else {
            return false;
        };
        let new = update
            .clients
            .keys()
            .filter(|id| !c.clients.contains(id))
            .count();
        if c.clients.len() + new > MAX_AWARENESS_CLIENTS {
            return false;
        }
        c.clients.extend(update.clients.keys().copied());
        if self.awareness.apply_update(update).is_err() {
            return false;
        }
        // Relayée même pour un client en lecture seule : ce n'est pas du contenu.
        self.broadcast(Some(conn), protocol::awareness(raw));
        true
    }

    /// Tous les états d'awareness connus, s'il y en a.
    fn awareness_state(&self) -> Option<Vec<u8>> {
        let update = self.awareness.update().ok()?;
        (!update.clients.is_empty()).then(|| protocol::awareness(&update.encode_v1()))
    }

    async fn create(&mut self) -> Result<(), SyncError> {
        if self.created {
            return Ok(());
        }
        // Sans document, rien n'a pu être appliqué : remplacer le document
        // vivant, vide, ne perd rien.
        let (page, update) = PageDoc::new_page();
        self.store
            .put_snapshot(self.id.node_id, 0, &page.encode_state())
            .await?;
        self.page = page;
        self.created = true;
        self.last_seq = 0;
        self.since_snapshot = 0;
        // Écrite, puis diffusée aux clients déjà ouverts sur la page vide.
        self.broadcast(None, protocol::update(&update));
        Ok(())
    }

    async fn write(&mut self, blocks: Vec<Block>) -> Result<Result<(), SyncError>, Broken> {
        // Refusée sans rien écrire : le document reste intact.
        let update = match self.page.write_blocks(&blocks) {
            Ok(update) => update,
            Err(e) => return Ok(Err(e.into())),
        };
        if let Err(e) = self.persist(&update).await {
            self.store_failed().await?;
            return Ok(Err(e.into()));
        }
        self.broadcast(None, protocol::update(&update));
        self.compact_if_due().await;
        Ok(Ok(()))
    }

    async fn persist(&mut self, update: &[u8]) -> Result<(), StoreError> {
        self.last_seq = self.store.append_update(self.id.node_id, update).await?;
        self.since_snapshot += 1;
        self.created = true;
        Ok(())
    }

    /// Compaction toutes les `snapshot_every` mises à jour écrites. Prise
    /// entre deux commandes, elle couvre exactement le journal jusqu'à
    /// `last_seq`. Un échec ne perd rien (le journal reste) : on réessaiera à
    /// la prochaine mise à jour.
    async fn compact_if_due(&mut self) {
        if self.since_snapshot < self.cfg.snapshot_every.max(1) {
            return;
        }
        let state = self.page.encode_state();
        if self
            .store
            .put_snapshot(self.id.node_id, self.last_seq, &state)
            .await
            .is_ok()
        {
            self.since_snapshot = 0;
        }
    }

    /// Ruling R21 : le document vivant contient une mise à jour que le store
    /// n'a pas écrite, et que personne n'a reçue. On recharge depuis le store
    /// et on ferme toutes les connexions : elles se resynchronisent en
    /// revenant, sans dépendre d'un état jamais écrit.
    async fn store_failed(&mut self) -> Result<(), Broken> {
        self.reload().await?;
        for c in self.conns.values_mut() {
            c.closing = true;
        }
        // Les connexions voient la fin de l'ancienne diffusion et se ferment.
        let (events, _) = broadcast::channel(self.cfg.broadcast_capacity.max(1));
        self.events = events;
        Ok(())
    }

    async fn reload(&mut self) -> Result<(), Broken> {
        let loaded = load(&self.store, self.id.node_id)
            .await
            .map_err(|_| Broken)?;
        self.install(loaded);
        Ok(())
    }

    fn broadcast(&self, from: Option<ConnId>, message: Vec<u8>) {
        // Sans abonné, il n'y a personne à prévenir.
        let _ = self.events.send(Arc::new(Broadcast { from, message }));
    }
}
