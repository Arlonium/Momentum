//! L'acteur d'une page : une tâche tokio qui possède le `PageDoc` de la page
//! et traite ses commandes une par une.
//!
//! Invariant : entre deux commandes, le document vivant est exactement
//! l'instantané du store suivi de son journal. Toute mise à jour appliquée est
//! écrite (`append_updates` attendu) avant la commande suivante et avant
//! d'être diffusée ; une mise à jour refusée ou non écrite fait recharger le
//! document depuis le store. D'où une compaction sûre : l'instantané pris
//! entre deux commandes couvre exactement les mises à jour écrites jusqu'à
//! son `seq`.
//!
//! La mise à jour d'un client et le correctif de dédoublonnage qui la suit
//! sont écrits ensemble, tous deux ou aucun, au nom du compte de la
//! connexion (report M3 du lot 1).
//!
//! Chaque fin anormale (mise à jour refusée, panne du store, document
//! impossible à charger, compaction en échec, awareness refusée) est
//! journalisée par `tracing`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use mementum_crdt::PageDoc;
use mementum_md::Block;
use mementum_store::{DocState, Store, StoreError};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, WeakUnboundedSender};
use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;
use yrs::ClientID;
use yrs::sync::{Awareness, AwarenessUpdate};
use yrs::updates::encoder::Encode;

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

/// Taille maximale d'un état d'awareness (JSON) : un message qui en porte un
/// plus gros est ignoré, ni gardé ni relayé (Ruling R27).
const MAX_AWARENESS_STATE: usize = 64 * 1024;

/// Message diffusé aux connexions de la page.
pub(crate) struct Broadcast {
    /// Connexion d'origine, qui ne le reçoit pas ; `None` : toutes le reçoivent.
    pub(crate) from: Option<ConnId>,
    /// Message y-websocket.
    pub(crate) message: Vec<u8>,
}

/// Ce que l'acteur diffuse aux connexions de la page, dans l'ordre.
#[derive(Clone)]
pub(crate) enum Event {
    /// Un message y-websocket, partagé entre les connexions.
    Message(Arc<Broadcast>),
    /// Les connexions inscrites jusqu'ici se ferment (`Hub::kick`,
    /// `Hub::forget`) : rien de ce qui suit ne leur parvient.
    Close,
}

/// Ce que l'acteur sait faire.
pub(crate) enum Command {
    /// Inscrire une connexion. `writable` est faux pour un autre schéma, ou
    /// sans le droit d'écrire ; `actor` est le compte à qui attribuer ses
    /// mises à jour.
    Join {
        writable: bool,
        actor: Option<Uuid>,
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
    /// Créer le document d'une page neuve : fait au chargement de l'acteur
    /// s'il manque (Ruling R28), la commande le confirme.
    Create {
        reply: oneshot::Sender<Result<(), SyncError>>,
    },
    /// Lire les blocs.
    Read {
        reply: oneshot::Sender<Result<Vec<Block>, SyncError>>,
    },
    /// Remplacer les blocs (écriture serveur), au nom du compte `actor`.
    Write {
        blocks: Vec<Block>,
        actor: Option<Uuid>,
        reply: oneshot::Sender<Result<(), SyncError>>,
    },
    /// Fermer toutes les connexions, sans toucher au document (`Hub::kick`).
    /// La réponse part quand plus aucun de leurs messages ne sera traité.
    Kick { reply: oneshot::Sender<()> },
    /// Arrêter l'acteur sans rien écrire (`Hub::forget`). La réponse part
    /// quand il a quitté le registre.
    Forget { reply: oneshot::Sender<()> },
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
            // Il n'y a ni connexion à fermer ni acteur à arrêter : la réponse
            // lâchée le dit au demandeur.
            Command::Leave { .. } | Command::Kick { .. } | Command::Forget { .. } => {}
        }
    }
}

/// Réponse à [`Command::Join`].
pub(crate) struct Joined {
    /// Diffusion de la page, abonnée au moment de l'inscription.
    pub(crate) events: broadcast::Receiver<Event>,
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

/// Réponses à un même message, bornées en octets comme la file de la
/// connexion : `max_bytes` plus une réponse (une étape 2 plus grosse que la
/// borne, l'état complet d'une page, passe donc seule). Ruling R29.
struct Replies {
    messages: Vec<Vec<u8>>,
    bytes: usize,
    max_bytes: usize,
}

impl Replies {
    fn new(max_bytes: usize) -> Replies {
        Replies {
            messages: Vec::new(),
            bytes: 0,
            max_bytes,
        }
    }

    /// `false` : la borne est déjà atteinte, la connexion est à fermer.
    fn take(&mut self, reply: Vec<u8>) -> bool {
        if self.bytes >= self.max_bytes {
            return false;
        }
        self.bytes += reply.len();
        self.messages.push(reply);
        true
    }

    fn into_inner(self) -> Vec<Vec<u8>> {
        self.messages
    }
}

/// Le document n'a pas pu être rechargé : l'acteur s'arrête.
struct Broken;

/// Pourquoi l'acteur s'arrête.
enum Stop {
    /// Ni connexion ni demande pendant `idle_close`.
    Idle,
    /// Le document n'a pas pu être rechargé.
    Broken,
    /// La page est supprimée (`Hub::forget`) : la réponse part une fois
    /// l'acteur hors du registre.
    Forgotten(oneshot::Sender<()>),
}

impl From<Broken> for Stop {
    fn from(_: Broken) -> Stop {
        Stop::Broken
    }
}

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
            hub.unregister(self.node_id, self.generation);
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
        Ok(loaded) => {
            let mut actor = Actor::new(id, store, cfg, loaded);
            // Un journal qui porte une grosse suppression (écrite avant une
            // panne, ou avant ce réglage) est compacté dès le chargement : il
            // ne coûte qu'une fois.
            actor.compact_if_due().await;
            actor.serve(rx).await;
        }
        Err(error) => {
            tracing::error!(node_id = %id.node_id, %error, "document de la page impossible à charger");
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
    /// Plus grand `seq` écrit (instantané ou journal).
    last_seq: i64,
    /// Mises à jour du journal depuis l'instantané.
    since_snapshot: u64,
    /// Suppressions faites dans le document depuis l'instantané (journal compris).
    deletions: Deletions,
}

/// Compte les suppressions faites dans un document, en unités d'horloge Yjs
/// (un caractère, un élément ou un attribut chacun), d'après le jeu de
/// suppressions de chaque transaction validée. Sert au déclenchement de la
/// compaction (Ruling R40) : le ramasse-miettes de yrs, qui nettoie ce qui est
/// supprimé, est rejoué à chaque chargement tant que la suppression est au
/// journal ; une grosse suppression coûte alors plusieurs secondes à chaque
/// ouverture de la page.
#[derive(Default)]
struct Deletions(Arc<AtomicU64>);

impl Deletions {
    /// Compte les suppressions des transactions de `page` validées désormais.
    fn watch(page: &PageDoc) -> Deletions {
        let deletions = Deletions::default();
        let count = deletions.0.clone();
        let watched = page.yrs_doc().observe_transaction_cleanup(
            "mementum-sync/deletions",
            move |_, event| {
                let units: u64 = event
                    .delete_set
                    .iter()
                    .flat_map(|(_, ranges)| ranges.iter())
                    .map(|range| u64::from(range.end - range.start))
                    .sum();
                count.fetch_add(units, Ordering::Relaxed);
            },
        );
        if let Err(error) = watched {
            // Sans compteur, la compaction n'a plus lieu que toutes les
            // `snapshot_every` mises à jour : plus lent au chargement, sans perte.
            tracing::warn!(%error, "suppressions non comptées : compaction au rythme ordinaire");
        }
        deletions
    }

    fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    fn reset(&self) {
        self.0.store(0, Ordering::Relaxed);
    }
}

/// Relit l'instantané, puis les mises à jour dans l'ordre des `seq`.
///
/// Une page sans document (création interrompue après `create_node`) reçoit
/// le sien ici, avant toute mise à jour d'un client : `new_page`, puis
/// instantané de `seq` 0 (Ruling R28). Sinon, la première mise à jour d'un
/// client poserait la racine, avec le piège « Page neuve » du `CLAUDE.md`.
async fn load(store: &Store, node_id: Uuid) -> Result<Loaded, SyncError> {
    let DocState { snapshot, updates } = store.load_doc(node_id).await?;
    if snapshot.is_none() && updates.is_empty() {
        let (page, _) = PageDoc::new_page();
        store.put_snapshot(node_id, 0, &page.encode_state()).await?;
        let deletions = Deletions::watch(&page);
        return Ok(Loaded {
            page,
            last_seq: 0,
            since_snapshot: 0,
            deletions,
        });
    }
    let last_seq = updates
        .last()
        .or(snapshot.as_ref())
        .map_or(0, |(seq, _)| *seq);
    // Les suppressions sont comptées à partir du journal : celles de
    // l'instantané sont déjà nettoyées par le ramasse-miettes.
    let page = PageDoc::from_updates(snapshot.iter().map(|(_, bytes)| bytes.as_slice()))?;
    let deletions = Deletions::watch(&page);
    for (_, update) in &updates {
        page.apply_update(update)?;
    }
    Ok(Loaded {
        page,
        last_seq,
        since_snapshot: updates.len() as u64,
        deletions,
    })
}

struct Conn {
    /// Faux pour un client d'un autre schéma, ou sans le droit d'écrire : ses
    /// mises à jour sont ignorées.
    writable: bool,
    /// Le compte à qui ses mises à jour sont attribuées.
    actor: Option<Uuid>,
    /// La connexion doit se fermer (panne du store, Ruling R21, ou fermeture
    /// à la demande) : ses messages ne sont plus traités.
    closing: bool,
    /// Clients d'awareness annoncés par cette connexion.
    clients: HashSet<ClientID>,
}

struct Actor {
    id: Identity,
    store: Store,
    cfg: SyncConfig,
    page: PageDoc,
    last_seq: i64,
    /// Mises à jour écrites depuis la dernière compaction (ou tentative).
    since_snapshot: u64,
    /// Suppressions faites depuis la dernière compaction (ou tentative).
    deletions: Deletions,
    /// Awareness des clients, sur un document à part : elle ne touche jamais
    /// au contenu, et survit au rechargement du document.
    awareness: Awareness,
    events: broadcast::Sender<Event>,
    conns: HashMap<ConnId, Conn>,
    next_conn: ConnId,
}

impl Actor {
    fn new(id: Identity, store: Store, cfg: SyncConfig, loaded: Loaded) -> Actor {
        let awareness = Awareness::new(crate::utf16_doc());
        let (events, _) = broadcast::channel(cfg.broadcast_capacity.max(1));
        Actor {
            id,
            store,
            cfg,
            page: loaded.page,
            last_seq: loaded.last_seq,
            since_snapshot: loaded.since_snapshot,
            deletions: loaded.deletions,
            awareness,
            events,
            conns: HashMap::new(),
            next_conn: 0,
        }
    }

    fn install(&mut self, loaded: Loaded) {
        self.page = loaded.page;
        self.last_seq = loaded.last_seq;
        self.since_snapshot = loaded.since_snapshot;
        self.deletions = loaded.deletions;
    }

    async fn serve(mut self, mut rx: UnboundedReceiver<Command>) {
        let stop = loop {
            let next = if self.conns.is_empty() {
                match tokio::time::timeout(self.cfg.idle_close, rx.recv()).await {
                    Ok(next) => next,
                    Err(_) => break Stop::Idle,
                }
            } else {
                rx.recv().await
            };
            // Plus personne ne peut écrire à l'acteur (hub lâché, connexions finies).
            let Some(command) = next else { return };
            if let Err(stop) = self.handle(command).await {
                break stop;
            }
        };
        // Les commandes reçues entre-temps sont lâchées sans réponse : leurs
        // demandeurs réessaient auprès d'un nouvel acteur.
        drop(self.id.retire(&mut rx));
        if let Stop::Forgotten(reply) = stop {
            // Rien n'est écrit : ni instantané, ni ce qui restait dans la boîte.
            tracing::debug!(node_id = %self.id.node_id, connections = self.conns.len(), "page oubliée : acteur arrêté, connexions fermées");
            self.close_connections();
            let _ = reply.send(());
        }
    }

    async fn handle(&mut self, command: Command) -> Result<(), Stop> {
        match command {
            Command::Join {
                writable,
                actor,
                reply,
            } => self.join(writable, actor, reply),
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
                // Le document existe depuis le chargement (Ruling R28).
                let _ = reply.send(Ok(()));
            }
            Command::Read { reply } => {
                let _ = reply.send(Ok(self.page.read_blocks()));
            }
            Command::Write {
                blocks,
                actor,
                reply,
            } => {
                let result = self.write(blocks, actor).await?;
                let _ = reply.send(result);
            }
            Command::Kick { reply } => {
                // Ni rechargement ni écriture : le document vivant est celui
                // du store, et le reste.
                tracing::debug!(node_id = %self.id.node_id, connections = self.conns.len(), "connexions fermées à la demande");
                self.close_connections();
                let _ = reply.send(());
            }
            Command::Forget { reply } => return Err(Stop::Forgotten(reply)),
        }
        Ok(())
    }

    fn join(
        &mut self,
        writable: bool,
        actor: Option<Uuid>,
        reply: oneshot::Sender<Result<Joined, SyncError>>,
    ) {
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
                actor,
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
        let (writable, actor) = match self.conns.get(&conn) {
            Some(c) if !c.closing => (c.writable, c.actor),
            _ => return Ok(Outcome::Close),
        };
        // Ruling R29 : les réponses d'un même message sont bornées avant
        // d'atteindre la file de la connexion. Une seule étape 2 et une seule
        // réponse d'awareness complète par message (un client légitime n'envoie
        // jamais deux étapes 1 dans un message), et un cumul en octets.
        let mut replies = Replies::new(self.cfg.max_queue_bytes);
        let mut answered_sync = false;
        let mut answered_awareness = false;
        for message in messages {
            match message {
                ClientMessage::SyncStep1(_) if answered_sync => {}
                ClientMessage::SyncStep1(state_vector) => {
                    answered_sync = true;
                    match self.page.encode_diff(&state_vector) {
                        Ok(diff) => {
                            if !replies.take(protocol::sync_step2(&diff)) {
                                return Ok(self.too_many_replies(conn));
                            }
                        }
                        Err(error) => {
                            tracing::warn!(node_id = %self.id.node_id, conn, %error, "étape 1 illisible : connexion fermée");
                            return Ok(Outcome::Close);
                        }
                    }
                }
                // Lecture seule : ni appliquée, ni écrite, ni diffusée.
                ClientMessage::Update(_) if !writable => {}
                ClientMessage::Update(update) if update == EMPTY_UPDATE => {}
                ClientMessage::Update(update) => {
                    if !self.client_update(conn, actor, &update).await? {
                        return Ok(Outcome::Close);
                    }
                }
                ClientMessage::Awareness { update, raw } => {
                    if !self.awareness_update(conn, update, &raw) {
                        return Ok(Outcome::Close);
                    }
                }
                ClientMessage::AwarenessQuery if answered_awareness => {}
                ClientMessage::AwarenessQuery => {
                    answered_awareness = true;
                    if let Some(state) = self.awareness_state()
                        && !replies.take(state)
                    {
                        return Ok(self.too_many_replies(conn));
                    }
                }
            }
        }
        Ok(Outcome::Reply(replies.into_inner()))
    }

    fn too_many_replies(&self, conn: ConnId) -> Outcome {
        tracing::warn!(node_id = %self.id.node_id, conn, "réponses d'un message au-delà de max_queue_bytes : connexion fermée");
        Outcome::Close
    }

    /// Mise à jour d'un client : appliquer, dédoublonner, écrire au nom de
    /// `actor` (le compte de la connexion), puis diffuser. `false` : la
    /// connexion est à fermer.
    async fn client_update(
        &mut self,
        conn: ConnId,
        actor: Option<Uuid>,
        update: &[u8],
    ) -> Result<bool, Broken> {
        // Ce qui vient d'un client ne touche pas aux métadonnées de la page
        // (report M7 du lot 1) ; ce que `load` relit du store, si.
        if let Err(error) = self.page.apply_client_update(update) {
            // Rulings R17 et R22 : ni écrite ni diffusée ; le document, peut-être
            // entamé (yrs garde en attente une mise à jour à dépendance
            // manquante), est rechargé depuis le store ; le client fautif est
            // fermé. Il se reconnecte, et sa synchro complète apporte ce qui
            // manquait.
            tracing::warn!(node_id = %self.id.node_id, conn, %error, "mise à jour refusée : document rechargé, connexion fermée");
            self.reload().await?;
            return Ok(false);
        }
        // Après des déplacements concurrents, un bloc peut exister en double.
        let fix = self.page.dedupe_blocks();
        // La mise à jour et son correctif en une écriture, tous deux ou aucun
        // (report M3 du lot 1) : sans le correctif, le store garderait le
        // doublon, que le rechargement servirait.
        let written = match &fix {
            Some(fix) => self.persist(&[update, fix], actor).await,
            None => self.persist(&[update], actor).await,
        };
        if let Err(error) = written {
            self.store_failed(&error).await?;
            return Ok(false);
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
        if update
            .clients
            .values()
            .any(|entry| entry.json.len() > MAX_AWARENESS_STATE)
        {
            tracing::warn!(node_id = %self.id.node_id, conn, "état d'awareness de plus de 64 Kio : ignoré");
            return true;
        }
        let Some(c) = self.conns.get_mut(&conn) else {
            return false;
        };
        let new = update
            .clients
            .keys()
            .filter(|id| !c.clients.contains(id))
            .count();
        if c.clients.len() + new > MAX_AWARENESS_CLIENTS {
            tracing::warn!(node_id = %self.id.node_id, conn, "trop de clients d'awareness : connexion fermée");
            return false;
        }
        c.clients.extend(update.clients.keys().copied());
        if let Err(error) = self.awareness.apply_update(update) {
            tracing::warn!(node_id = %self.id.node_id, conn, %error, "awareness refusée : connexion fermée");
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

    async fn write(
        &mut self,
        blocks: Vec<Block>,
        actor: Option<Uuid>,
    ) -> Result<Result<(), SyncError>, Broken> {
        // Refusée sans rien écrire : le document reste intact.
        let update = match self.page.write_blocks(&blocks) {
            Ok(update) => update,
            Err(e) => return Ok(Err(e.into())),
        };
        if let Err(error) = self.persist(&[&update], actor).await {
            self.store_failed(&error).await?;
            return Ok(Err(error.into()));
        }
        self.broadcast(None, protocol::update(&update));
        self.compact_if_due().await;
        Ok(Ok(()))
    }

    /// Écrit des mises à jour au journal, dans l'ordre, toutes ou aucune, au
    /// nom du compte `actor`.
    async fn persist(&mut self, updates: &[&[u8]], actor: Option<Uuid>) -> Result<(), StoreError> {
        self.last_seq = self
            .store
            .append_updates(self.id.node_id, updates, actor)
            .await?;
        self.since_snapshot += updates.len() as u64;
        Ok(())
    }

    /// Ferme toutes les connexions inscrites : leurs messages ne sont plus
    /// traités, et chacune se ferme en lisant la diffusion, après ce qui a été
    /// diffusé jusqu'ici. Ni le document ni le store ne sont touchés.
    fn close_connections(&mut self) {
        for c in self.conns.values_mut() {
            c.closing = true;
        }
        // Sans abonné, il n'y a personne à fermer.
        let _ = self.events.send(Event::Close);
    }

    /// Compaction toutes les `snapshot_every` mises à jour écrites, ou dès que
    /// les mises à jour écrites depuis la dernière ont supprimé au moins
    /// `compact_after_deletions` unités (Ruling R40) : l'instantané garde
    /// l'état déjà nettoyé par le ramasse-miettes, que le chargement ne refait
    /// pas. Prise entre deux commandes, elle couvre exactement le journal
    /// jusqu'à `last_seq`. Un échec ne perd rien (le journal reste) : il est
    /// journalisé, et la compaction se réessaie plus tard (après
    /// `snapshot_every` mises à jour ou autant de suppressions), pas à chacune.
    async fn compact_if_due(&mut self) {
        let many_updates = self.since_snapshot >= self.cfg.snapshot_every.max(1);
        let many_deletions = self.deletions.count() >= self.cfg.compact_after_deletions.max(1);
        if !many_updates && !many_deletions {
            return;
        }
        self.since_snapshot = 0;
        self.deletions.reset();
        let state = self.page.encode_state();
        if let Err(error) = self
            .store
            .put_snapshot(self.id.node_id, self.last_seq, &state)
            .await
        {
            tracing::warn!(node_id = %self.id.node_id, seq = self.last_seq, %error, "compaction impossible : réessai plus tard");
        }
    }

    /// Ruling R21 : le document vivant contient une mise à jour que le store
    /// n'a pas écrite, et que personne n'a reçue. On recharge depuis le store
    /// et on ferme toutes les connexions : elles se resynchronisent en
    /// revenant, sans dépendre d'un état jamais écrit.
    async fn store_failed(&mut self, error: &StoreError) -> Result<(), Broken> {
        tracing::error!(node_id = %self.id.node_id, %error, "écriture impossible : document rechargé, connexions fermées");
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
        match load(&self.store, self.id.node_id).await {
            Ok(loaded) => {
                self.install(loaded);
                Ok(())
            }
            Err(error) => {
                tracing::error!(node_id = %self.id.node_id, %error, "document impossible à recharger : l'acteur s'arrête");
                Err(Broken)
            }
        }
    }

    fn broadcast(&self, from: Option<ConnId>, message: Vec<u8>) {
        // Sans abonné, il n'y a personne à prévenir.
        let _ = self
            .events
            .send(Event::Message(Arc::new(Broadcast { from, message })));
    }
}
