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
//! La mise à jour d'un client, le report d'un bloc qu'elle recrée (Task 13b,
//! `PageDoc::apply_client_update`) et le correctif de dédoublonnage qui la
//! suit sont écrits ensemble, tous ou aucun, au nom du compte de la connexion
//! (report M3 du lot 1).
//!
//! La mise à jour d'un client qui laisserait un trou dans les horloges de
//! son client, ou qui supprimerait des horloges que la page n'a pas encore,
//! n'est ni appliquée, ni écrite, ni relayée : elle attend dans la file de sa
//! connexion que le trou soit comblé (Rulings R131, R198, voir
//! [`Actor::receive_update`]). Elle n'est donc jamais dans le document vivant
//! sans être au journal : l'invariant tient.
//!
//! L'acteur coupe aussi les versions de la page (Ruling R135, module
//! `history`) : l'état complet n'est encodé qu'au moment de couper, jamais à
//! chaque mise à jour.
//!
//! Chaque fin anormale (mise à jour refusée, panne du store, document
//! impossible à charger, compaction en échec, awareness refusée, trop de
//! mises à jour retenues, version impossible à écrire) est journalisée par
//! `tracing`.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Weak};

use mementum_crdt::{Applied, Awaited, CrdtError, PageDoc, Refused};
use mementum_md::Block;
use mementum_store::{DocState, Store, StoreError};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, WeakUnboundedSender};
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;
use yrs::ClientID;
use yrs::sync::awareness::AwarenessUpdateEntry;
use yrs::sync::{Awareness, AwarenessUpdate};
use yrs::updates::encoder::Encode;

use crate::hub::Hub;
use crate::long_work::off_the_engine;
use crate::presence::{Holders, Holding};
use crate::protocol::{self, ClientMessage, EMPTY_UPDATE, RosterLine};
use crate::{PageEvent, SyncConfig, SyncError, presence};

mod history;

use history::{History, Slice, Watch};

/// Identifiant d'une connexion, propre à l'acteur.
pub(crate) type ConnId = u64;

/// Boîte aux lettres d'un acteur.
pub(crate) type Commands = UnboundedSender<Command>;

/// États d'awareness qu'une connexion peut tenir (un onglet n'en a qu'un) :
/// au-delà, son message d'awareness est refusé et la connexion fermée.
const MAX_AWARENESS_CLIENTS: usize = 8;

/// L'état (JSON) d'une entrée d'awareness qui retire celui du client.
const AWARENESS_REMOVED: &str = "null";

/// La dernière horloge qu'un état d'awareness peut porter (Ruling R108). yrs
/// incrémente l'horloge d'un client quand il retire son état
/// (`Awareness::remove_state`), sans regarder si elle peut encore l'être : à
/// cette valeur, l'addition déborde. L'acteur paniquait dans un build de
/// développement, et toutes les connexions de la page se fermaient ; en
/// release, l'horloge repartait de zéro. Depuis le Ruling R136, l'acteur
/// n'appelle plus `remove_state` (un départ retire l'état à son horloge,
/// [`Actor::leave`]) : rien n'incrémente plus une horloge. Une entrée à cette
/// valeur reste ignorée, comme filet. Une horloge honnête, qui avance d'un cran
/// par changement d'état, n'y arrive jamais.
const LAST_AWARENESS_CLOCK: u32 = u32::MAX;

/// Mises à jour qu'une connexion peut retenir en attendant que le trou
/// d'horloge qu'elles laisseraient soit comblé (Ruling R131) : au-delà, elle
/// est fermée.
const MAX_HELD_UPDATES: usize = 32;

/// Octets qu'une connexion peut retenir ainsi, toutes ses mises à jour
/// retenues comptées ensemble : au-delà, elle est fermée.
const MAX_HELD_BYTES: usize = 1 << 20;

/// Ce qu'une écriture du serveur fait des identifiants des blocs qu'elle écrit.
#[derive(Debug, Clone, Copy)]
enum Ids {
    /// `PUT /mmd` : un bloc sans identifiant reçoit celui du bloc courant qui lui correspond
    /// (`PageDoc::rewrite_blocks`, Ruling R201). Le MMD lu ne porte pas d'ancres.
    Matched,
    /// Une restauration : les blocs de la version portent les leurs, écrits tels quels.
    AsGiven,
}

/// Message diffusé aux connexions de la page.
pub(crate) struct Broadcast {
    /// Connexion d'origine, qui ne le reçoit pas ; `None` : toutes le reçoivent.
    pub(crate) from: Option<ConnId>,
    /// La seule connexion qui le reçoit ; `None` : toutes (moins `from`). Il passe par la
    /// diffusion, et pas à côté, pour arriver à sa place parmi les autres messages de la page.
    pub(crate) to: Option<ConnId>,
    /// Seulement aux connexions dont le compte a le droit d'écrire la page (`ConnInfo::can_write`),
    /// quel que soit leur schéma : la trame `versions` (Ruling R355). Un onglet d'un autre schéma,
    /// en lecture seule, lit encore l'historique.
    pub(crate) editors_only: bool,
    /// Message y-websocket, ou trame de page.
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
    /// Ces connexions-là se ferment (`Hub::kick_user`) ; les autres
    /// continuent.
    CloseSome(Arc<[ConnId]>),
}

/// Ce que l'acteur sait faire.
pub(crate) enum Command {
    /// Inscrire une connexion. `writable` est faux pour un autre schéma, ou
    /// sans le droit d'écrire ; `actor` est son compte : celui à qui
    /// attribuer ses mises à jour, et celui dont `KickUser` ferme les
    /// connexions ; `name` est le nom de ce compte (Ruling R136).
    Join {
        writable: bool,
        actor: Option<Uuid>,
        name: Option<Arc<str>>,
        reply: oneshot::Sender<Result<Joined, SyncError>>,
    },
    /// La connexion a passé les contrôles de son inscription (Rulings R71,
    /// R88) : elle entre dans la liste des présents (trame `roster`).
    Admit { conn: ConnId },
    /// Désinscrire une connexion (envoyé par [`Membership`]).
    Leave { conn: ConnId },
    /// Dire aux connexions de la page qu'il y a du neuf à relire par l'API
    /// (`Hub::page_event`).
    PageEvent { event: PageEvent },
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
    /// Dire si un bloc porte cet identifiant, sans lire la liste des blocs (Ruling R206).
    HasBlock {
        id: String,
        reply: oneshot::Sender<Result<bool, SyncError>>,
    },
    /// Remplacer les blocs (écriture serveur), au nom du compte `actor`.
    Write {
        blocks: Vec<Block>,
        actor: Option<Uuid>,
        reply: oneshot::Sender<Result<(), SyncError>>,
    },
    /// Restaurer la version `version` de la page, au nom du compte `actor` (Ruling R135).
    Restore {
        version: Uuid,
        actor: Option<Uuid>,
        reply: oneshot::Sender<Result<(), SyncError>>,
    },
    /// Fermer toutes les connexions, sans toucher au document (`Hub::kick`).
    /// La réponse part quand plus aucun de leurs messages ne sera traité.
    Kick { reply: oneshot::Sender<()> },
    /// Fermer les connexions du compte `user`, et elles seules
    /// (`Hub::kick_user`). La réponse part quand plus aucun de leurs
    /// messages ne sera traité.
    KickUser {
        user: Uuid,
        reply: oneshot::Sender<()>,
    },
    /// Arrêter l'acteur sans rien écrire (`Hub::forget`). La réponse part
    /// quand il a quitté le registre.
    Forget { reply: oneshot::Sender<()> },
    /// Arrêter l'acteur, le serveur s'arrête (`Hub::shutdown`) : la version
    /// `closing` est coupée s'il le faut, puis les connexions sont fermées. La
    /// réponse part quand il a quitté le registre.
    Shutdown { reply: oneshot::Sender<()> },
    /// Attendre la fin de la compaction en vol, s'il y en a une (`Hub::compacted`, pour les
    /// tests : la compaction se fait hors de l'acteur, Ruling R826).
    Compacted { reply: oneshot::Sender<()> },
}

impl Command {
    /// Répond par `error` (document impossible à charger).
    fn fail(self, error: SyncError) {
        match self {
            Command::Join { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Command::Create { reply }
            | Command::Write { reply, .. }
            | Command::Restore { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Command::Read { reply } => {
                let _ = reply.send(Err(error));
            }
            Command::HasBlock { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Command::Messages { reply, .. } => {
                let _ = reply.send(Outcome::Close);
            }
            // Il n'y a ni connexion à fermer ni acteur à arrêter : la réponse
            // lâchée le dit au demandeur.
            Command::Leave { .. }
            | Command::Admit { .. }
            | Command::PageEvent { .. }
            | Command::Kick { .. }
            | Command::KickUser { .. }
            | Command::Forget { .. }
            | Command::Shutdown { .. }
            | Command::Compacted { .. } => {}
        }
    }
}

/// Réponse à [`Command::Join`].
pub(crate) struct Joined {
    /// Diffusion de la page, abonnée au moment de l'inscription.
    pub(crate) events: broadcast::Receiver<Event>,
    /// Accueil : l'étape 1 du serveur et les états d'awareness connus. La liste des présents
    /// vient juste derrière, par la diffusion, quand la connexion est admise
    /// ([`Command::Admit`]).
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
    /// Fermer la connexion, après lui avoir envoyé ce dernier message (Ruling R788 : la trame
    /// qui dit que son compte est freiné).
    CloseAfter(Vec<u8>),
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
    /// Plus personne ne peut écrire à l'acteur (hub lâché, connexions finies).
    Orphaned,
    /// Le document n'a pas pu être rechargé.
    Broken,
    /// La page est supprimée (`Hub::forget`) : la réponse part une fois
    /// l'acteur hors du registre.
    Forgotten(oneshot::Sender<()>),
    /// Le serveur s'arrête (`Hub::shutdown`) : la réponse part une fois
    /// l'acteur hors du registre.
    Shutdown(oneshot::Sender<()>),
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
            let slice = Slice::read(&store, id.node_id, cfg.version_every, loaded.created).await;
            let mut actor = Actor::new(id, store, cfg, loaded, slice);
            // Un journal qui porte une grosse suppression (écrite avant une
            // panne, ou avant ce réglage) est compacté dès le chargement : il
            // ne coûte qu'une fois.
            actor.compact_if_due();
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

/// La compaction d'une page hors de son acteur (Ruling R826) : `state`, l'état que l'acteur a
/// encodé au `seq` donné, est relu par un `PageDoc` neuf sur un fil bloquant (Ruling R816), puis
/// écrit comme instantané dans la voie de la page, qui retire le journal jusqu'à `seq` dans la
/// même transaction. S'il ne se relit pas, rien n'est écrit et une ligne `error` le dit ; une
/// écriture impossible (une panne, une page supprimée entre-temps) ne retire rien.
async fn compact(store: Store, node_id: Uuid, seq: i64, state: Vec<u8>) {
    let read = tokio::task::spawn_blocking(move || PageDoc::check_state(&state).map(|()| state));
    let state = match read.await {
        Ok(Ok(state)) => state,
        Ok(Err(error)) => {
            tracing::error!(%node_id, seq, %error, "compaction refusée : l'état de la page ne se relit pas, le journal est gardé");
            return;
        }
        Err(error) => {
            tracing::error!(%node_id, seq, %error, "compaction interrompue pendant la relecture de l'état");
            return;
        }
    };
    if let Err(error) = store.put_snapshot(node_id, seq, &state).await {
        tracing::warn!(%node_id, seq, %error, "compaction impossible : réessai plus tard");
    }
}

/// Document relu depuis le store.
struct Loaded {
    page: PageDoc,
    /// Plus grand `seq` écrit (instantané ou journal).
    last_seq: i64,
    /// Mises à jour du journal depuis l'instantané.
    since_snapshot: u64,
    /// Ce qui est compté des transactions du document, à partir du journal
    /// (les suppressions de l'instantané sont déjà nettoyées).
    watch: Watch,
    /// La page a reçu son document à ce chargement : aucune version ne le garde.
    created: bool,
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
        let watch = Watch::on(&page);
        return Ok(Loaded {
            page,
            last_seq: 0,
            since_snapshot: 0,
            watch,
            created: true,
        });
    }
    let last_seq = last_seq_of(&snapshot, &updates);
    // Les suppressions sont comptées à partir du journal : celles de
    // l'instantané sont déjà nettoyées par le ramasse-miettes.
    //
    // Ruling R743 : une entrée qui dépend, par une origine ou un parent, de ce qu'une entrée
    // écartée portait est écartée à son tour ; une page attaquée avant la garde où l'on a
    // continué d'écrire se charge avec le reste. Ruling R787 : `replay` le voit avant de
    // l'appliquer, en une passe. La boucle ne reste qu'en filet : une dépendance vue seulement
    // après l'application (yrs garde l'entrée en attente, la page est à jeter) fait reprendre la
    // relecture du début sans elle.
    // Ruling R863 : relire un journal intègre ce que des clients ont écrit.
    let (page, watch) =
        off_the_engine(|| replay_whole(node_id, &snapshot, &updates, Reader::Actor))?;
    Ok(Loaded {
        page,
        last_seq,
        since_snapshot: updates.len() as u64,
        watch,
        created: false,
    })
}

/// La page que le store garde, et son dernier `seq` (instantané ou journal), relue comme l'acteur
/// la relit en se chargeant (`load` : mêmes règles, mêmes entrées écartées), **sans passer par
/// un acteur et sans rien écrire** (Ruling R907). `Ok(None)` pour une page sans document (une
/// création interrompue, que seul l'acteur répare, Ruling R28) ou inconnue. C'est ce que lit
/// l'indexeur : il ne demande rien à l'acteur d'une page, dont les frappes attendraient.
///
/// L'instantané et le journal sont lus dans une même transaction de lecture : une compaction ou
/// une frappe qui tombent pendant l'appel ne donnent jamais un état d'entre deux. La page rendue
/// est celle du `seq` rendu, entrées écartées ou non : une écriture faite ensuite porte un `seq`
/// plus grand.
///
/// Les entrées écartées sont dites en `debug`, pas en `error` : l'acteur de la page les a déjà
/// dites, ou les dira en se chargeant. Une page qui ne se relit pas rend l'erreur que `load`
/// rendrait, sans la journaliser : c'est l'appelant qui décide de ce qu'il en dit.
///
/// Le calcul dépend de ce que des clients ont écrit (Ruling R863) : il rend son fil au moteur
/// tokio le temps de relire, comme l'acteur. L'appelant peut aussi mener tout l'appel sur un fil
/// bloquant.
pub async fn stored_page(
    store: &Store,
    node_id: Uuid,
) -> Result<Option<(PageDoc, i64)>, SyncError> {
    let DocState { snapshot, updates } = store.load_doc(node_id).await?;
    if snapshot.is_none() && updates.is_empty() {
        return Ok(None);
    }
    let last_seq = last_seq_of(&snapshot, &updates);
    let (page, _) = off_the_engine(|| replay_whole(node_id, &snapshot, &updates, Reader::Index))?;
    Ok(Some((page, last_seq)))
}

/// Le plus grand `seq` écrit d'une page : celui de la dernière entrée du journal, sinon celui de
/// l'instantané.
fn last_seq_of(snapshot: &Option<(i64, Vec<u8>)>, updates: &[(i64, Vec<u8>)]) -> i64 {
    updates
        .last()
        .or(snapshot.as_ref())
        .map_or(0, |(seq, _)| *seq)
}

/// Qui relit une page gardée : cela ne change que le niveau auquel une entrée écartée est dite.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reader {
    /// L'acteur de la page, qui se charge : en `error`.
    Actor,
    /// Une lecture seule ([`stored_page`]), après ou avant lui : en `debug`.
    Index,
}

/// Dit une entrée écartée à la relecture, au niveau de qui relit.
macro_rules! set_aside {
    ($reader:expr, $($line:tt)*) => {
        match $reader {
            Reader::Actor => tracing::error!($($line)*),
            Reader::Index => tracing::debug!($($line)*),
        }
    };
}

/// Rejoue l'instantané puis le journal d'une page, en écartant ce qui doit l'être : [`replay`],
/// repris du début sans chaque entrée qui dépend de ce qu'une entrée écartée portait (Rulings
/// R743, R787 ; la boucle ne reste qu'en filet).
fn replay_whole(
    node_id: Uuid,
    snapshot: &Option<(i64, Vec<u8>)>,
    updates: &[(i64, Vec<u8>)],
    reader: Reader,
) -> Result<(PageDoc, Watch), SyncError> {
    let mut dependents: HashSet<i64> = HashSet::new();
    loop {
        match replay(node_id, snapshot, updates, &dependents, reader)? {
            Ok(replayed) => return Ok(replayed),
            Err(seq) => {
                dependents.insert(seq);
            }
        }
    }
}

/// Rejoue l'instantané puis le journal d'une page, sans les entrées `dependents` (Ruling R743).
/// `Ok(Err(seq))` : l'entrée `seq` dépend de ce qu'une entrée écartée portait ; la page bâtie est
/// à jeter, et l'appelant reprend sans elle.
#[allow(clippy::type_complexity)]
fn replay(
    node_id: Uuid,
    snapshot: &Option<(i64, Vec<u8>)>,
    updates: &[(i64, Vec<u8>)],
    dependents: &HashSet<i64>,
    reader: Reader,
) -> Result<Result<(PageDoc, Watch), i64>, SyncError> {
    let page = PageDoc::from_updates(snapshot.iter().map(|(_, bytes)| bytes.as_slice()))?;
    let watch = Watch::on(&page);
    let mut set_aside = false;
    for (seq, update) in updates {
        if dependents.contains(seq) {
            set_aside = true;
            continue;
        }
        // Ruling R366 : après une entrée écartée, une entrée qui laisserait un trou (des blocs
        // qui suivent, chez leur client, ceux de l'entrée écartée) est écartée aussi. yrs
        // l'intégrerait en laissant le trou ; un navigateur la garde en attente (Ruling R131) :
        // le serveur lirait un texte qu'aucun navigateur n'affiche.
        if set_aside && matches!(page.stored_update_leaves_gap(update), Ok(true)) {
            set_aside!(reader, %node_id, seq, "entrée du journal écartée à la relecture : elle suit une entrée écartée et laisserait un trou");
            continue;
        }
        // Ruling R787 : après une entrée écartée, une entrée qui dépend, par une origine ou un
        // parent, de ce que la page n'a pas est écartée **avant** d'être appliquée : la page
        // reste bonne, et le journal se relit en une passe, quel que soit le nombre de ces
        // entrées (une relecture entière par entrée, avant).
        if set_aside && matches!(page.stored_update_lacks_dependency(update), Ok(true)) {
            set_aside!(reader, %node_id, seq, "entrée du journal écartée à la relecture : elle dépend de ce qu'une entrée écartée portait");
            continue;
        }
        match page.apply_update(update) {
            Ok(()) => {}
            // Ruling R258 : une entrée où un client paraît deux fois (une base d'avant R130
            // gardait les octets reçus d'un client) est refusée avant que yrs la lise ; la page
            // est intacte, et se charge sans elle. Une entrée suivante qui en dépendrait resterait
            // en attente d'une dépendance, et la page serait refusée à elle.
            Err(error) if PageDoc::repeats_a_client(update) => {
                set_aside!(reader, %node_id, seq, %error, "entrée du journal écartée à la relecture : un client y paraît deux fois");
                set_aside = true;
            }
            // Rulings R723, R784 : une entrée de map à origine droite, écrite par un serveur
            // d'avant cette garde (ses octets le disent : yrs l'a encodée), ou du JSON à cheval
            // sur ce que la page connaît (tout autre élément à cheval est coupé, pas refusé).
            // Elle est refusée avant d'être appliquée (c'est l'erreur
            // qui le dit, Ruling R742), la page est intacte, et se charge sans elle ni la
            // suppression qu'elle portait. Appliquée, elle laisserait une entrée que plus rien ne tient, et un état
            // illisible le jour où son bloc est supprimé.
            Err(error) if PageDoc::refused_intact(&error) => {
                set_aside!(reader, %node_id, seq, %error, "entrée du journal écartée à la relecture : aucun client n'écrit ce qu'elle porte");
                set_aside = true;
            }
            // Rulings R723, R803, R806, R809 : ce que yrs a fait de l'entrée n'est pas ce qu'un client écrit
            // (une entrée de map à origine droite, un élément rangé autrement que ses octets ne
            // le disent, une entrée de map de plusieurs valeurs restée vivante), et cela ne se
            // voit qu'une fois l'entrée appliquée. La page bâtie est à jeter, et la relecture
            // reprend du début sans l'entrée ; ce qui dépend d'elle est écarté à son tour.
            Err(error) if PageDoc::refused_made(&error) => {
                set_aside!(reader, %node_id, seq, %error, set_aside = dependents.len() + 1, "entrée du journal écartée à la relecture, qui reprend du début : aucun client n'écrit ce qu'elle porte");
                return Ok(Err(*seq));
            }
            // En filet de R787 : une dépendance que la lecture n'a pas vue (un cycle entre deux
            // clients de la même entrée). La page est à jeter, et la relecture reprend sans elle.
            Err(error) if set_aside && PageDoc::lacks_a_dependency(&error) => {
                set_aside!(reader, %node_id, seq, %error, set_aside = dependents.len() + 1, "entrée du journal écartée à la relecture, qui reprend du début : elle dépend de ce qu'une entrée écartée portait");
                return Ok(Err(*seq));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(Ok((page, watch)))
}

struct Conn {
    /// Faux pour un client d'un autre schéma, ou sans le droit d'écrire : ses
    /// mises à jour sont ignorées.
    writable: bool,
    /// Son compte : celui à qui ses mises à jour sont attribuées, et dont
    /// `Hub::kick_user` ferme les connexions.
    actor: Option<Uuid>,
    /// Le nom de son compte, tel que le serveur le montre (`display_name`, Ruling R136) ;
    /// `None` : sans nom (tests du hub).
    name: Option<Arc<str>>,
    /// Elle a passé les contrôles de son inscription ([`Command::Admit`]) : elle compte dans la
    /// liste des présents, jusqu'à son départ.
    listed: bool,
    /// Son compte est freiné sur cette page, pour ce temps encore (Rulings R725, R788) : posé
    /// quand une de ses mises à jour est écartée pour cela, lu à sa fermeture.
    blocked: Option<std::time::Duration>,
    /// La connexion doit se fermer (panne du store, Ruling R21, fermeture à
    /// la demande, ou l'un de ses messages refusé) : ses messages ne sont plus
    /// traités. Posé par [`Conn::close`].
    closing: bool,
    /// Les clients d'awareness que cette connexion tient (Ruling R101) : ceux dont elle a
    /// annoncé un état gardé. Ils sont à son compte, et le restent quand elle part
    /// ([`Actor::leave`], Ruling R136) ; la table de l'acteur ([`presence::Holders`]) en tient
    /// le compte.
    clients: HashSet<ClientID>,
    /// Ses mises à jour qui laisseraient un trou d'horloge, dans l'ordre où elle les a
    /// envoyées (Ruling R131). Elles partent avec elle, et quand elle se ferme.
    held: VecDeque<Held>,
    /// Leur taille, en octets reçus.
    held_bytes: usize,
}

impl Conn {
    fn new(writable: bool, actor: Option<Uuid>, name: Option<&str>) -> Conn {
        Conn {
            writable,
            actor,
            name: name.map(|name| presence::display_name(name).into()),
            listed: false,
            blocked: None,
            closing: false,
            clients: HashSet::new(),
            held: VecDeque::new(),
            held_bytes: 0,
        }
    }

    /// La connexion se ferme : ses messages ne sont plus traités, et rien de ce qu'elle
    /// retenait ne passera (Ruling R131). Sans cela, une mise à jour retenue par une connexion
    /// dont le droit d'écrire vient d'être retiré serait appliquée plus tard.
    fn close(&mut self) {
        self.closing = true;
        self.held.clear();
        self.held_bytes = 0;
    }
}

/// Une mise à jour retenue (Ruling R131) : les octets reçus, que la page lira quand le trou sera
/// comblé, et ce qu'elle attend de la page pour ne plus en laisser. Ces octets ne sont jamais
/// écrits ni relayés : ce qui passe suit le chemin de toute mise à jour de client
/// ([`Actor::client_update`], Ruling R130).
struct Held {
    update: Vec<u8>,
    awaited: Awaited,
}

/// Ce que devient une entrée d'awareness reçue d'une connexion (Ruling R101).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    /// Ignorée : ni appliquée, ni relayée, et la connexion reste ouverte.
    Ignored,
    /// Appliquée et relayée, sans que la connexion prenne le client : le retrait d'un état que
    /// son compte tient déjà.
    Applied,
    /// Appliquée et relayée, et la connexion tient désormais le client.
    Held,
}

/// Rulings R101 et R136 : à qui appartient un `ClientID` d'awareness. Un client appartient au
/// compte de la première connexion inscrite qui en annonce un état gardé ; les autres connexions
/// du même compte peuvent l'annoncer aussi (un onglet qui revient avant que son ancienne
/// connexion soit fermée), et le tiennent alors avec elle. Quand la dernière part, il reste à
/// son compte, sans connexion vivante, tant que l'acteur vit (`holding`, de
/// [`presence::Holders`], qui le dit).
///
/// `removal` : l'entrée retire l'état (`null`). `newer` : son horloge est strictement
/// au-dessus de celle que le serveur connaît pour ce client (ou il n'en connaît aucune) :
/// c'est une entrée que l'awareness applique. Pour une connexion du compte `account` :
/// - un client d'un **autre** compte, qu'une de ses connexions le tienne ou qu'il soit retenu
///   pour lui : ignorée. Un client y-websocket honnête renvoie au serveur tous les états qu'il
///   reçoit des autres, et les retraits qu'il déduit de leur silence : rien de cela ne doit
///   toucher l'état de leur titulaire. Et personne ne prend le client de quelqu'un qui est
///   coupé, ni n'avance son horloge (Ruling R109, cas a et c) ;
/// - le retrait d'un client qu'aucune connexion vivante ne tient : ignoré aussi. Il n'y a pas
///   d'état à retirer (un état gardé a toujours une connexion qui le tient, et son départ l'a
///   retiré) ; il ne ferait qu'avancer l'horloge du client, et l'état que son titulaire
///   annoncerait ensuite serait ignoré de tous. Et il ne donne pas le client à qui l'envoie ;
/// - un état pour un client qu'aucune connexion vivante ne tient, qui n'est pas plus récent que
///   ce que le serveur en sait : ignoré de même (Ruling R112). C'est le renvoi d'un état arrivé
///   après le départ de son titulaire, à l'horloge de son retrait. Il ne serait pas appliqué ;
///   il ne doit pas non plus donner le client, à son compte qui revient comme à un autre si la
///   table l'a oublié : ce dernier dépasserait la limite de [`MAX_AWARENESS_CLIENTS`] pour des
///   clients qui ne sont pas les siens, et rendrait invisible le titulaire à son retour. Un
///   client libre ou retenu ne se prend donc que par une entrée que l'awareness applique ;
/// - sinon, l'entrée est appliquée ; si elle annonce un état, la connexion tient le client.
///   Un client que son compte tient déjà par une connexion vivante, elle le tient avec elle
///   quelle que soit l'horloge : l'onglet qui revient renvoie son état à l'horloge qu'il avait.
///
/// Sans compte (`None` : les tests du hub), les connexions ne sont pas bridées entre elles ;
/// elles le sont face à un compte, et lui face à elles.
fn presence_rule(
    holding: Option<Holding>,
    account: Option<Uuid>,
    removal: bool,
    newer: bool,
) -> Presence {
    let held = match holding {
        Some(holding) if holding.account != account => return Presence::Ignored,
        Some(holding) => holding.live,
        None => false,
    };
    match (removal, held) {
        (true, false) => Presence::Ignored,
        (true, true) => Presence::Applied,
        (false, true) => Presence::Held,
        (false, false) if newer => Presence::Held,
        (false, false) => Presence::Ignored,
    }
}

struct Actor {
    id: Identity,
    store: Store,
    cfg: SyncConfig,
    page: PageDoc,
    last_seq: i64,
    /// Mises à jour écrites depuis la dernière compaction (ou tentative).
    since_snapshot: u64,
    /// Suppressions faites depuis la dernière compaction (ou tentative), et
    /// transactions qui ont changé le document.
    watch: Watch,
    /// La tranche en cours de l'historique (Ruling R135). Elle survit au
    /// rechargement du document : ce qu'elle compte a été écrit.
    slice: Slice,
    /// Awareness des clients, sur un document à part : elle ne touche jamais
    /// au contenu, et survit au rechargement du document.
    awareness: Awareness,
    /// À quel compte appartient chaque client d'awareness, connexions parties
    /// comprises (Rulings R101, R136).
    holders: Holders,
    events: broadcast::Sender<Event>,
    conns: HashMap<ConnId, Conn>,
    next_conn: ConnId,
    /// La liste des présents que la page a diffusée en dernier ([`Actor::roster_changed`]) :
    /// celle que chaque connexion admise tient du serveur.
    roster: Vec<RosterLine>,
    /// La compaction en vol (Ruling R826) : l'état encodé par l'acteur, relu puis écrit par une
    /// tâche à part pendant que l'acteur sert la page. Une seule à la fois ; attendue avant que
    /// l'acteur quitte le registre.
    compaction: Option<tokio::task::JoinHandle<()>>,
}

impl Actor {
    fn new(id: Identity, store: Store, cfg: SyncConfig, loaded: Loaded, slice: Slice) -> Actor {
        let awareness = Awareness::new(crate::utf16_doc());
        let (events, _) = broadcast::channel(cfg.broadcast_capacity.max(1));
        Actor {
            id,
            store,
            cfg,
            page: loaded.page,
            last_seq: loaded.last_seq,
            since_snapshot: loaded.since_snapshot,
            watch: loaded.watch,
            slice,
            awareness,
            holders: Holders::default(),
            events,
            conns: HashMap::new(),
            next_conn: 0,
            roster: Vec::new(),
            compaction: None,
        }
    }

    fn install(&mut self, loaded: Loaded) {
        self.page = loaded.page;
        self.last_seq = loaded.last_seq;
        self.since_snapshot = loaded.since_snapshot;
        self.watch = loaded.watch;
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
            let Some(command) = next else {
                break Stop::Orphaned;
            };
            if let Err(stop) = self.handle(command).await {
                break stop;
            }
        };
        // Ruling R826 : la compaction en vol va au bout avant que l'acteur quitte le registre ;
        // un acteur neuf ne relit donc jamais la page pendant qu'elle s'écrit, et le serveur ne
        // ferme pas la base sous elle (`Hub::shutdown`).
        self.compaction_finished().await;
        // Ruling R135 : avant de disparaître, la version `closing` des écritures
        // pas encore versionnées. L'acteur est encore au registre : ce qui arrive
        // pendant ce temps attend dans sa boîte, puis repart vers un acteur neuf,
        // qui trouve la version écrite. Ni pour une page supprimée, ni pour un
        // document qui n'a pas pu être rechargé (il peut porter une mise à jour
        // que le store n'a pas).
        if matches!(stop, Stop::Idle | Stop::Orphaned | Stop::Shutdown(_)) {
            self.cut_closing().await;
        }
        // Les commandes reçues entre-temps sont lâchées sans réponse : leurs
        // demandeurs réessaient auprès d'un nouvel acteur.
        drop(self.id.retire(&mut rx));
        match stop {
            Stop::Forgotten(reply) => {
                // Rien n'est écrit : ni instantané, ni version, ni ce qui restait dans la boîte.
                tracing::debug!(node_id = %self.id.node_id, connections = self.conns.len(), "page oubliée : acteur arrêté, connexions fermées");
                self.close_connections();
                let _ = reply.send(());
            }
            Stop::Shutdown(reply) => {
                tracing::debug!(node_id = %self.id.node_id, connections = self.conns.len(), "arrêt du serveur : acteur arrêté, connexions fermées");
                self.close_connections();
                let _ = reply.send(());
            }
            Stop::Idle | Stop::Orphaned | Stop::Broken => {}
        }
    }

    async fn handle(&mut self, command: Command) -> Result<(), Stop> {
        match command {
            Command::Join {
                writable,
                actor,
                name,
                reply,
            } => self.join(writable, actor, name.as_deref(), reply),
            Command::Admit { conn } => self.admit(conn),
            Command::Leave { conn } => self.leave(conn),
            Command::PageEvent { event } => self.page_event(event),
            Command::Messages {
                conn,
                messages,
                reply,
            } => {
                let mut outcome = self.messages(conn, messages).await?;
                if let (Outcome::Close, Some(c)) = (&outcome, self.conns.get_mut(&conn)) {
                    // Ruling R788 : fermée parce que son compte est freiné, elle l'apprend.
                    if let Some(left) = c.blocked.take() {
                        outcome = Outcome::CloseAfter(protocol::blocked(left));
                    }
                    // Le hub la ferme : rien de ce qu'elle retenait ne passera après elle, et
                    // elle n'est plus nommée.
                    c.close();
                    self.roster_changed();
                }
                let _ = reply.send(outcome);
            }
            Command::Create { reply } => {
                // Le document existe depuis le chargement (Ruling R28).
                let _ = reply.send(Ok(()));
            }
            Command::Read { reply } => {
                let _ = reply.send(Ok(self.page.read_blocks()));
            }
            Command::HasBlock { id, reply } => {
                let _ = reply.send(Ok(self.page.has_block(&id)));
            }
            Command::Write {
                blocks,
                actor,
                reply,
            } => {
                let result = self.write(blocks, actor).await?;
                let written = result.is_ok();
                // La réponse d'abord : l'écriture est faite, quoi qu'il arrive ensuite.
                let _ = reply.send(result);
                if written {
                    self.release_held().await?;
                }
            }
            Command::Restore {
                version,
                actor,
                reply,
            } => {
                let result = self.restore(version, actor).await?;
                let written = result.is_ok();
                // Comme une écriture du serveur : la réponse, puis les mises à jour retenues.
                let _ = reply.send(result);
                if written {
                    self.release_held().await?;
                }
            }
            Command::Kick { reply } => {
                // Ni rechargement ni écriture : le document vivant est celui
                // du store, et le reste.
                tracing::debug!(node_id = %self.id.node_id, connections = self.conns.len(), "connexions fermées à la demande");
                self.close_connections();
                let _ = reply.send(());
            }
            Command::KickUser { user, reply } => {
                self.close_connections_of(user);
                let _ = reply.send(());
            }
            Command::Forget { reply } => return Err(Stop::Forgotten(reply)),
            Command::Shutdown { reply } => return Err(Stop::Shutdown(reply)),
            Command::Compacted { reply } => {
                self.compaction_finished().await;
                let _ = reply.send(());
            }
        }
        Ok(())
    }

    fn join(
        &mut self,
        writable: bool,
        actor: Option<Uuid>,
        name: Option<&str>,
        reply: oneshot::Sender<Result<Joined, SyncError>>,
    ) {
        let Some(commands) = self.id.me.upgrade() else {
            let _ = reply.send(Err(SyncError::Closed));
            return;
        };
        let conn = self.next_conn;
        self.next_conn += 1;
        self.conns.insert(conn, Conn::new(writable, actor, name));
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

    /// La connexion `conn` a passé les contrôles de son inscription : elle compte désormais dans
    /// la liste des présents (Ruling R136). Si la liste en change, toutes les connexions admises
    /// la reçoivent, elle comprise (abonnée à la diffusion depuis son inscription) ; sinon (un
    /// second onglet du même compte, une connexion sans compte), elle seule la reçoit. Dans les
    /// deux cas, elle arrive derrière son accueil, et derrière ce que la page a diffusé depuis.
    /// Une liste vide ne s'envoie pas à qui arrive : il n'en tient aucune autre.
    fn admit(&mut self, conn: ConnId) {
        // Partie entre-temps, ou déjà fermée à la demande : jamais nommée.
        let Some(c) = self.conns.get_mut(&conn).filter(|c| !c.closing) else {
            return;
        };
        c.listed = true;
        if !self.roster_changed() && !self.roster.is_empty() {
            self.send_to(conn, protocol::roster(&self.roster));
        }
    }

    fn leave(&mut self, conn: ConnId) {
        let Some(gone) = self.conns.remove(&conn) else {
            return;
        };
        self.release_clients(gone.clients);
        if gone.listed {
            self.roster_changed();
        }
    }

    /// Les clients d'awareness d'une connexion qui part. Un client que tient encore une autre
    /// connexion (du même compte : l'onglet est revenu entre-temps) reste annoncé. Les autres
    /// restent à leur compte, sans connexion vivante, tant que l'acteur vit (Ruling R136), et
    /// leur état est retiré, chez tous : aucun curseur ne reste d'une connexion partie.
    ///
    /// Le retrait se fait à l'horloge de l'état retiré, sans l'avancer : yrs l'applique ainsi à
    /// un client qui n'est pas le sien, comme y-protocols chez les clients (une entrée `null` à
    /// l'horloge connue retire un état présent). `Awareness::remove_state`, qu'appelait le lot
    /// 2, avançait l'horloge d'un cran : l'état que l'onglet annonçait en revenant, à l'horloge
    /// suivante de la sienne, n'était donc pas plus récent, et sa présence ne revenait qu'à son
    /// deuxième changement d'état (piste notée avec R109). Et rien n'incrémente plus une
    /// horloge : le débordement à `u32::MAX` de R108 n'a plus d'occasion. Le client du document
    /// d'awareness de l'acteur lui-même, que yrs traite à part, n'est tenu par personne : toute
    /// entrée qui le nomme est écartée (`awareness_update`).
    ///
    /// Ce qui est appliqué et relayé est écrit ici, une entrée par client (Ruling R123).
    fn release_clients(&mut self, clients: HashSet<ClientID>) {
        let freed: Vec<ClientID> = clients
            .into_iter()
            .filter(|client| self.holders.release(*client))
            .collect();
        self.holders.forget_beyond_bound();
        let removals: HashMap<ClientID, AwarenessUpdateEntry> = freed
            .into_iter()
            .filter_map(|client| {
                let mut current = self.awareness.update_with_clients([client]).ok()?;
                let entry = current.clients.remove(&client)?;
                // Un état déjà retiré (par son client lui-même) : rien à dire.
                (entry.json.as_ref() != AWARENESS_REMOVED).then(|| {
                    let removal = AwarenessUpdateEntry {
                        clock: entry.clock,
                        json: AWARENESS_REMOVED.into(),
                    };
                    (client, removal)
                })
            })
            .collect();
        if removals.is_empty() {
            return;
        }
        let update = AwarenessUpdate { clients: removals };
        let message = protocol::awareness(&update.encode_v1());
        if let Err(error) = self.awareness.apply_update(update) {
            tracing::warn!(node_id = %self.id.node_id, %error, "retrait d'états de présence refusé");
            return;
        }
        self.broadcast(None, message);
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
                    if !self.receive_update(conn, actor, update).await? {
                        return Ok(Outcome::Close);
                    }
                }
                ClientMessage::Awareness(update) => {
                    if !self.awareness_update(conn, update) {
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

    /// Mise à jour reçue d'une connexion qui écrit. `false` : la connexion est
    /// à fermer.
    ///
    /// Ruling R131 : si elle laisserait un trou dans les horloges de son
    /// client, elle est retenue ([`Actor::hold`]) : ni appliquée, ni écrite,
    /// ni relayée. yrs l'intégrerait en laissant le trou, mais un navigateur
    /// la garde en attente tant que le trou n'est pas comblé, y compris dans
    /// l'état complet que le serveur lui donne : le serveur, l'API MMD et les
    /// agents liraient un texte qu'aucun navigateur n'affiche. Elle n'est pas
    /// refusée non plus : un client honnête en envoie une quand il tape entre
    /// l'ouverture de sa connexion et sa réponse à l'étape 1 du serveur, qui
    /// porte ce qu'il avait tapé hors ligne ; un refus fermerait sa connexion
    /// à chaque retour.
    ///
    /// Ruling R198 : une suppression qui vise des horloges que la page n'aura
    /// pas, les blocs de la même mise à jour intégrés, est un trou comme un
    /// autre. yrs la garderait en attente, et supprimerait à l'arrivée ce que le
    /// client visé écrirait dans la plage : dix octets suffisaient à effacer,
    /// sur le serveur et chez tous, tout ce qu'une autre personne tapait
    /// ensuite. Retenue, elle passe quand la page a ces horloges, comme une
    /// suppression ordinaire, ou part avec sa connexion.
    ///
    /// Sinon, elle est appliquée ([`Actor::client_update`]), même si sa
    /// connexion en retient d'autres : c'est d'ordinaire celle qui comble leur
    /// trou. Puis les mises à jour retenues dont le trou est comblé passent
    /// ([`Actor::release_held`]).
    async fn receive_update(
        &mut self,
        conn: ConnId,
        actor: Option<Uuid>,
        update: Vec<u8>,
    ) -> Result<bool, Broken> {
        let awaited = match self.page.awaits(&update) {
            Ok(awaited) => awaited,
            Err(error) => {
                // Lue avant d'être appliquée : le document n'a pas à être rechargé.
                tracing::warn!(node_id = %self.id.node_id, conn, %error, "mise à jour illisible : connexion fermée");
                return Ok(false);
            }
        };
        if let Some(awaited) = awaited {
            return Ok(self.hold(conn, update, awaited));
        }
        if !self.client_update(conn, actor, &update).await? {
            return Ok(false);
        }
        self.release_held().await?;
        // Une mise à jour qu'elle retenait a pu être refusée entre-temps.
        Ok(self.conns.get(&conn).is_some_and(|c| !c.closing))
    }

    /// Retient la mise à jour d'une connexion, au bout de sa file (Ruling
    /// R131). `false` : la file dépasserait [`MAX_HELD_UPDATES`] mises à jour
    /// ou [`MAX_HELD_BYTES`] octets, et la connexion est à fermer, elle seule.
    /// Chaque connexion a sa file, et rien de ce qu'envoie une autre n'y
    /// entre.
    fn hold(&mut self, conn: ConnId, update: Vec<u8>, awaited: Awaited) -> bool {
        let Some(c) = self.conns.get_mut(&conn) else {
            return false;
        };
        if c.held.len() >= MAX_HELD_UPDATES || c.held_bytes + update.len() > MAX_HELD_BYTES {
            tracing::warn!(
                node_id = %self.id.node_id,
                conn,
                held = c.held.len(),
                bytes = c.held_bytes,
                "trop de mises à jour retenues : connexion fermée"
            );
            return false;
        }
        tracing::debug!(node_id = %self.id.node_id, conn, bytes = update.len(), "mise à jour retenue : elle laisserait un trou d'horloge");
        c.held_bytes += update.len();
        c.held.push_back(Held { update, awaited });
        true
    }

    /// Fait passer les mises à jour retenues dont le trou est comblé (Ruling
    /// R131). Appelé après chaque mise à jour appliquée, d'où qu'elle vienne :
    /// le trou d'une connexion peut être comblé par une autre.
    ///
    /// Chaque file est réessayée dans l'ordre où sa connexion l'a remplie, et
    /// s'arrête à la première mise à jour qui attend encore : une connexion ne
    /// fait jamais passer une mise à jour avant celles qu'elle a envoyées
    /// plus tôt. On recommence tant que quelque chose passe, puisque ce qui
    /// passe peut combler un autre trou. Savoir si un trou est comblé ne
    /// relit pas la mise à jour (`PageDoc::has_reached`) : ce réessai, fait à
    /// chaque frappe, ne coûte pas plus cher quand des connexions retiennent
    /// de gros messages.
    ///
    /// Ce qui passe suit le chemin d'une mise à jour reçue
    /// ([`Actor::client_update`]) : lu et réencodé, dédoublonné, écrit au nom
    /// du compte de sa connexion, puis relayé à toutes les autres ; une mise à
    /// jour que l'étape 2 du client couvrait déjà ne change plus rien, et n'est
    /// ni écrite ni relayée (Ruling R181). Ce que la
    /// page refuse alors est traité comme à la réception : le document est
    /// rechargé, et la connexion qui l'avait envoyé est fermée, elle seule.
    /// Une connexion qui se ferme (départ, fermeture à la demande, refus,
    /// panne du store) n'a plus rien en file ([`Conn::close`]).
    async fn release_held(&mut self) -> Result<(), Broken> {
        loop {
            let mut waiting: Vec<ConnId> = self
                .conns
                .iter()
                .filter(|(_, c)| !c.closing && !c.held.is_empty())
                .map(|(conn, _)| *conn)
                .collect();
            if waiting.is_empty() {
                return Ok(());
            }
            waiting.sort_unstable();
            let mut released = false;
            for conn in waiting {
                while let Some((update, actor)) = self.next_released(conn) {
                    released = true;
                    if !self.client_update(conn, actor, &update).await? {
                        self.close_connection(conn);
                        break;
                    }
                }
            }
            if !released {
                return Ok(());
            }
        }
    }

    /// La plus ancienne mise à jour que retient `conn`, retirée de sa file si
    /// son trou est comblé ; avec le compte de la connexion, à qui l'attribuer.
    fn next_released(&mut self, conn: ConnId) -> Option<(Vec<u8>, Option<Uuid>)> {
        let c = self.conns.get_mut(&conn).filter(|c| !c.closing)?;
        if !self.page.has_reached(&c.held.front()?.awaited) {
            return None;
        }
        let held = c.held.pop_front()?;
        c.held_bytes -= held.update.len();
        Some((held.update, c.actor))
    }

    /// Mise à jour d'un client : appliquer (avec le report d'un bloc recréé,
    /// Task 13b), dédoublonner, écrire au nom de `actor` (le compte de la
    /// connexion), puis diffuser. `false` : la connexion est à fermer.
    ///
    /// Ruling R130 : ce qui est écrit et diffusé est la mise à jour telle que
    /// la page l'a lue, réencodée, jamais les octets reçus. Ce que les clients
    /// appliquent, et ce que le journal garde, est ce que le serveur a
    /// appliqué.
    ///
    /// Ruling R135 : une version est coupée avant, si elle est due
    /// ([`Actor::version_if_due`]) ; l'écriture compte dans la tranche en
    /// cours si elle a changé la page ([`Actor::wrote`]).
    async fn client_update(
        &mut self,
        conn: ConnId,
        actor: Option<Uuid>,
        update: &[u8],
    ) -> Result<bool, Broken> {
        // Ruling R725 : un compte freiné pour ses mises à jour refusées après leur application
        // n'écrit plus pendant un temps : sa connexion est fermée sans que rien soit appliqué,
        // donc sans rien recharger. Son onglet se reconnecte, et renvoie ce qui manque.
        let hub = self.id.hub.upgrade();
        if let (Some(account), Some(hub)) = (actor, &hub)
            && let Some(left) =
                hub.brake()
                    .blocked(account, self.id.node_id, tokio::time::Instant::now())
        {
            tracing::warn!(node_id = %self.id.node_id, conn, %account, ?left, "compte freiné après des mises à jour refusées : connexion fermée sans rien appliquer");
            // Ruling R788 : la connexion l'apprend avant d'être fermée, pour que l'onglet cesse
            // de revenir en boucle et dise que ce qui s'y tape ne s'enregistre plus.
            if let Some(c) = self.conns.get_mut(&conn) {
                c.blocked = Some(left);
            }
            return Ok(false);
        }
        self.version_if_due().await;
        let changes = self.watch.changes();
        // Ce qui vient d'un client ne touche pas aux métadonnées de la page
        // (report M7 du lot 1) ; ce que `load` relit du store, si. Un report refusé
        // (`CrdtError::CarryRefused`, Rulings R452, R459) suit le chemin de tout refus fait
        // après l'application. Ruling R863 : ce calcul dépend de ce que le client a écrit, et le
        // dédoublonnage de toute la page.
        let (applied, fix) = off_the_engine(|| {
            let applied = self.page.try_client_update(update);
            let fix = applied.is_ok().then(|| self.page.dedupe_blocks()).flatten();
            (applied, fix)
        });
        let Applied {
            read,
            carried,
            withheld,
        } = match applied {
            Ok(applied) => applied,
            // Ruling R492 : refusée avant d'être appliquée (illisible, hors du schéma, une marque
            // refusée…), le document est intact : la connexion est fermée, sans rien recharger.
            // Un onglet qui renvoie la même mise à jour à chaque reconnexion ne coûte plus un
            // rechargement de la page à chaque tour.
            Err(Refused {
                error,
                intact: true,
            }) => {
                tracing::warn!(node_id = %self.id.node_id, conn, %error, "mise à jour refusée avant d'être appliquée : connexion fermée");
                return Ok(false);
            }
            Err(Refused { error, .. }) => {
                // Rulings R17 et R22 : ni écrite ni diffusée ; le document,
                // peut-être entamé (yrs garde en attente une mise à jour à
                // dépendance manquante), est rechargé depuis le store ; le
                // client fautif est fermé. Il se reconnecte, et sa synchro
                // complète apporte ce qui manquait.
                tracing::warn!(node_id = %self.id.node_id, conn, %error, "mise à jour refusée : document rechargé, connexion fermée");
                // Ruling R725 : chaque refus de ce genre coûte un rechargement à tous ; au-delà
                // de quelques-uns, le compte est freiné.
                if let (Some(account), Some(hub)) = (actor, &hub)
                    && let Some(block) =
                        hub.brake()
                            .refused(account, self.id.node_id, tokio::time::Instant::now())
                {
                    tracing::warn!(node_id = %self.id.node_id, %account, ?block, "trop de mises à jour refusées après leur application : compte freiné");
                }
                self.reload().await?;
                return Ok(false);
            }
        };
        // Ruling R451 : un report au-delà de ses bornes n'est pas écrit.
        if let Some(why) = withheld {
            tracing::warn!(node_id = %self.id.node_id, conn, why, "report d'un bloc recréé retenu : rien d'écrit de plus que la mise à jour");
        }
        // Ruling R181 : une mise à jour qui ne change rien à la page (rien d'inséré, rien de
        // supprimé) n'est ni écrite ni relayée. Tout ce qu'elle porte est déjà dans le document,
        // donc déjà au journal et chez les autres : c'est l'étape 2 d'un onglet qui revient sans
        // rien de neuf (Yjs y met tout son jeu de suppressions), ou une mise à jour retenue que
        // l'étape 2 couvrait déjà. Écrite, le rechargement la relisait comme une écriture de son
        // compte. Elle répond à son expéditeur comme une autre. Elle ne laisse pas non plus de
        // suppression en attente qui ne l'était pas déjà : une telle mise à jour attend que la
        // page ait ces horloges (Ruling R198), et ce qui l'était déjà vient du journal (R199).
        if !self.watch.changed_since(changes) {
            return Ok(true);
        }
        // La mise à jour, son report et son correctif en une écriture, tous ou aucun (report M3
        // du lot 1, Task 13b) : sans le correctif, le store garderait le doublon, que le
        // rechargement servirait ; sans le report, un rechargement perdrait ce qu'il a repris
        // de l'original.
        let mut updates: Vec<&[u8]> = vec![&read];
        updates.extend(carried.as_deref());
        updates.extend(fix.as_deref());
        if let Err(error) = self.persist(&updates, actor).await {
            self.store_failed(&error).await?;
            return Ok(false);
        }
        self.wrote(changes, actor);
        self.relay(conn, &read, carried.as_deref(), fix.as_deref());
        self.compact_if_due();
        Ok(true)
    }

    /// Diffuse la mise à jour lue d'un client, `read`, et ce que le serveur a écrit à sa suite :
    /// le report d'un bloc recréé et le correctif du dédoublonnage, que l'expéditeur n'a pas non
    /// plus. Ruling R454 : aux autres, les trois **en un seul message**, et à l'expéditeur les
    /// deux siens en un seul : un client ne voit jamais l'état d'entre eux (deux exemplaires d'un
    /// bloc, que BlockNote renomme et peut écrire sous des `id` neufs ; le texte reporté qui
    /// disparaît un instant). Si la fusion échouait, chacun part dans son message, dans cet
    /// ordre.
    fn relay(&self, conn: ConnId, read: &[u8], carried: Option<&[u8]>, fix: Option<&[u8]>) {
        let ours: Vec<&[u8]> = carried.into_iter().chain(fix).collect();
        if ours.is_empty() {
            self.broadcast(Some(conn), protocol::update(read));
            return;
        }
        let all: Vec<&[u8]> = std::iter::once(read).chain(ours.iter().copied()).collect();
        match (yrs::merge_updates_v1(&all), yrs::merge_updates_v1(&ours)) {
            (Ok(to_others), Ok(to_sender)) => {
                self.broadcast(Some(conn), protocol::update(&to_others));
                self.send_to(conn, protocol::update(&to_sender));
            }
            (to_others, to_sender) => {
                let error = to_others.err().or(to_sender.err()).map(|e| e.to_string());
                tracing::warn!(node_id = %self.id.node_id, conn, ?error, "mises à jour impossibles à fusionner : diffusées une à une");
                self.broadcast(Some(conn), protocol::update(read));
                for update in ours {
                    self.broadcast(None, protocol::update(update));
                }
            }
        }
    }

    /// Message d'awareness d'une connexion : chaque entrée est gardée ou ignorée, **avant**
    /// d'être appliquée. Un état porte une horloge par client, et seule une horloge plus haute
    /// le remplace : appliquer puis défaire laisserait l'horloge de l'intrus, que le titulaire
    /// ne rattraperait pas. `false` : la connexion est à fermer.
    ///
    /// - Ruling R118 : d'une connexion en lecture seule, rien n'est gardé. Le client officiel
    ///   n'annonce rien en lecture seule, et ce qu'il renvoie des autres serait ignoré de toute
    ///   façon. Elle continue de recevoir la présence des autres. Le hub ne rend déjà aucun
    ///   message d'awareness d'une telle connexion (`protocol::decode`, pour ne pas même
    ///   copier ses états) : le contrôle fait ici est un filet.
    /// - Ruling R119 : un état qui n'a pas la forme connue ([`presence::known_shape`]) est
    ///   ignoré ; il ne donne pas son client.
    /// - Ruling R136 : pour une connexion qui a un compte, le nom de l'état est celui du compte,
    ///   écrit par le serveur quoi que le client annonce ; le reste de la forme est jugé.
    /// - Rulings R101, R104, R108, R112, R136 : le compte qui tient le client, ou l'a tenu
    ///   ([`presence_rule`]).
    /// - Ruling R123 : ce qui reste est appliqué, puis relayé aux autres connexions, réencodé.
    ///   Jamais les octets reçus : ce que les clients lisent est ce qui a été lu, vérifié et
    ///   gardé ici, une entrée par client, chaque état réécrit par le vérificateur.
    fn awareness_update(&mut self, conn: ConnId, mut update: AwarenessUpdate) -> bool {
        let Some((writable, account, name)) = self
            .conns
            .get(&conn)
            .map(|c| (c.writable, c.actor, c.name.clone()))
        else {
            return false;
        };
        if !writable {
            return true;
        }
        // Le client du document d'awareness de l'acteur : personne ne l'annonce. yrs le traite
        // à part (un retrait venu d'ailleurs y fait monter l'horloge au lieu de retirer l'état).
        let own = self.awareness.client_id();
        // Les entrées ignorées le sont en silence : un client honnête en envoie sans cesse.
        // Celles d'une forme inconnue sont comptées : aucun client honnête n'en envoie.
        let mut unknown_shapes = 0usize;
        let mut taken = Vec::new();
        update.clients.retain(|client, entry| {
            // Ruling R108 : une entrée à la dernière horloge est ignorée, quoi qu'elle porte.
            // Elle ne donne pas non plus le client à la connexion.
            if *client == own || entry.clock == LAST_AWARENESS_CLOCK {
                return false;
            }
            let removal = entry.json.as_ref() == AWARENESS_REMOVED;
            if !removal {
                // Ruling R119 : la forme d'abord. L'état gardé est celui que le vérificateur
                // a réécrit, avec le nom du compte de la connexion (Ruling R136).
                let Some(known) = presence::known_shape(&entry.json, name.as_deref()) else {
                    unknown_shapes += 1;
                    return false;
                };
                entry.json = known.into();
            }
            // Ce que le serveur sait de ce client : l'horloge de son dernier état, ou de son
            // retrait.
            let known = self.awareness.meta(*client).map(|(clock, _)| clock);
            let newer = known.is_none_or(|known| entry.clock > known);
            match presence_rule(self.holders.get(*client), account, removal, newer) {
                Presence::Ignored => false,
                Presence::Applied => true,
                Presence::Held => {
                    taken.push(*client);
                    true
                }
            }
        });
        if unknown_shapes > 0 {
            tracing::debug!(
                node_id = %self.id.node_id,
                conn,
                entries = unknown_shapes,
                "états de présence d'une forme inconnue : ignorés"
            );
        }
        if update.clients.is_empty() {
            return true;
        }
        let Some(c) = self.conns.get_mut(&conn) else {
            return false;
        };
        taken.retain(|id| !c.clients.contains(id));
        if c.clients.len() + taken.len() > MAX_AWARENESS_CLIENTS {
            tracing::warn!(node_id = %self.id.node_id, conn, "trop de clients d'awareness : connexion fermée");
            return false;
        }
        for client in taken {
            c.clients.insert(client);
            self.holders.take(client, account);
        }
        // Ruling R123 : ce qui est relayé est ce qui a été gardé, réencodé ici.
        let kept = protocol::awareness(&update.encode_v1());
        if let Err(error) = self.awareness.apply_update(update) {
            tracing::warn!(node_id = %self.id.node_id, conn, %error, "awareness refusée : connexion fermée");
            return false;
        }
        self.broadcast(Some(conn), kept);
        true
    }

    /// Tous les états d'awareness connus, s'il y en a.
    fn awareness_state(&self) -> Option<Vec<u8>> {
        let update = self.awareness.update().ok()?;
        (!update.clients.is_empty()).then(|| protocol::awareness(&update.encode_v1()))
    }

    /// Écriture du serveur (`PUT /mmd`), au nom de `actor` : elle compte dans
    /// l'historique comme celle d'un client (Ruling R135).
    async fn write(
        &mut self,
        blocks: Vec<Block>,
        actor: Option<Uuid>,
    ) -> Result<Result<(), SyncError>, Broken> {
        self.server_write(blocks, actor, History::Slice, Ids::Matched)
            .await
    }

    /// Une écriture du serveur, au nom de `actor` : `PUT /mmd`, ou une
    /// restauration ; `history` dit ce qu'elle fait de l'historique, `ids` des
    /// identifiants des blocs.
    async fn server_write(
        &mut self,
        blocks: Vec<Block>,
        actor: Option<Uuid>,
        history: History,
        ids: Ids,
    ) -> Result<Result<(), SyncError>, Broken> {
        self.version_if_due().await;
        let changes = self.watch.changes();
        // Refusée sans rien écrire : le document reste intact.
        let written = off_the_engine(|| match ids {
            Ids::Matched => self.page.rewrite_blocks(&blocks),
            Ids::AsGiven => self.page.write_blocks(&blocks),
        });
        let update = match written {
            Ok(update) => update,
            // Ruling R389 : le retrait est fait dans le document vivant, pas au journal.
            Err(e @ CrdtError::HalfWritten) => {
                tracing::error!(node_id = %self.id.node_id, "écriture complète faite à moitié : le document est rechargé");
                self.reload().await?;
                return Ok(Err(e.into()));
            }
            Err(e) => return Ok(Err(e.into())),
        };
        let written = match history {
            History::Slice => self.persist(&[&update], actor).await,
            History::Kept(version) => self.persist_restore(&update, actor, version).await,
        };
        if let Err(error) = written {
            self.store_failed(&error).await?;
            return Ok(Err(error.into()));
        }
        match history {
            History::Slice => self.wrote(changes, actor),
            History::Kept(_) => self.wrote_kept(),
        }
        self.broadcast(None, protocol::update(&update));
        self.compact_if_due();
        Ok(Ok(()))
    }

    /// Écrit au journal la mise à jour d'une restauration de `version`, au nom
    /// de `actor`, avec la borne de la version (`covers_seq`) : les deux, ou rien
    /// (Ruling R197). Un rechargement relit ainsi un contenu gardé, sans tranche.
    async fn persist_restore(
        &mut self,
        update: &[u8],
        actor: Option<Uuid>,
        version: Uuid,
    ) -> Result<(), StoreError> {
        self.last_seq = self
            .store
            .append_restore(self.id.node_id, update, actor, version)
            .await?;
        self.since_snapshot += 1;
        self.name_written_page();
        Ok(())
    }

    /// Écrit des mises à jour au journal, dans l'ordre, toutes ou aucune, au
    /// nom du compte `actor`.
    async fn persist(&mut self, updates: &[&[u8]], actor: Option<Uuid>) -> Result<(), StoreError> {
        self.last_seq = self
            .store
            .append_updates(self.id.node_id, updates, actor)
            .await?;
        self.since_snapshot += updates.len() as u64;
        self.name_written_page();
        Ok(())
    }

    /// Le contenu de la page vient d'être écrit au journal : elle entre dans l'ensemble des pages
    /// écrites du hub (Ruling R907), **après** l'écriture, jamais avant (une écriture refusée ne
    /// nomme rien, et qui lit l'ensemble trouve l'écriture dans le store). C'est tout ce que
    /// l'acteur fait pour l'index : ni lecture de ses blocs, ni encodage, ni attente. `persist`
    /// et `persist_restore` sont les deux seuls points par où il écrit un contenu ; l'instantané
    /// d'une compaction et une version n'en changent rien.
    fn name_written_page(&self) {
        if let Some(hub) = self.id.hub.upgrade() {
            hub.touched(self.id.node_id);
        }
    }

    /// L'état complet de la page, encodé, et compté ([`Hub::state_encodings`]) : il ne l'est que
    /// pour une version ou une compaction, jamais à chaque frappe.
    fn whole_state(&self) -> Vec<u8> {
        if let Some(hub) = self.id.hub.upgrade() {
            hub.count_state_encoding();
        }
        self.page.encode_state()
    }

    /// Ferme toutes les connexions inscrites : leurs messages ne sont plus
    /// traités, et chacune se ferme en lisant la diffusion, après ce qui a été
    /// diffusé jusqu'ici. Ni le document ni le store ne sont touchés.
    fn close_connections(&mut self) {
        for c in self.conns.values_mut() {
            c.close();
        }
        // Sans abonné, il n'y a personne à fermer.
        let _ = self.events.send(Event::Close);
        // Plus personne n'est nommé : qui arrive avant leur départ ne voit que lui.
        self.roster_changed();
    }

    /// Ferme la connexion `conn`, elle seule, comme
    /// [`Actor::close_connections_of`] celles d'un compte.
    fn close_connection(&mut self, conn: ConnId) {
        if let Some(c) = self.conns.get_mut(&conn) {
            c.close();
        }
        let _ = self.events.send(Event::CloseSome(Arc::from([conn])));
        self.roster_changed();
    }

    /// Ferme les connexions inscrites du compte `user`, comme
    /// [`Actor::close_connections`] ferme toutes celles de la page : leurs
    /// messages ne sont plus traités, et chacune se ferme en lisant la
    /// diffusion. Les autres ne voient rien.
    fn close_connections_of(&mut self, user: Uuid) {
        let conns: Arc<[ConnId]> = self
            .conns
            .iter_mut()
            .filter(|(_, c)| c.actor == Some(user))
            .map(|(conn, c)| {
                c.close();
                *conn
            })
            .collect();
        if conns.is_empty() {
            return;
        }
        tracing::debug!(node_id = %self.id.node_id, user_id = %user, connections = conns.len(), "connexions du compte fermées à la demande");
        let _ = self.events.send(Event::CloseSome(conns));
        // Les autres apprennent tout de suite que le compte n'a plus la page ouverte.
        self.roster_changed();
    }

    /// Compaction toutes les `snapshot_every` mises à jour écrites, ou dès que
    /// les mises à jour écrites depuis la dernière ont supprimé au moins
    /// `compact_after_deletions` unités (Ruling R40) : l'instantané garde
    /// l'état déjà nettoyé par le ramasse-miettes, que le chargement ne refait
    /// pas. Prise entre deux commandes, elle couvre exactement le journal
    /// jusqu'à `last_seq`. Un échec ne perd rien (le journal reste) : il est
    /// journalisé, et la compaction se réessaie plus tard (après
    /// `snapshot_every` mises à jour ou autant de suppressions), pas à chacune.
    ///
    /// Ruling R816 : **une compaction ne remplace jamais le journal par un état que le serveur ne
    /// relit pas.** L'instantané efface le journal qu'il couvre ; s'il ne se relisait pas, la page
    /// serait perdue au chargement suivant. L'état est donc relu par un `PageDoc` neuf avant
    /// d'être écrit ([`PageDoc::check_state`]) ; s'il ne se relit pas, rien n'est écrit, le
    /// journal reste, et une ligne `error` le dit.
    ///
    /// Ruling R826 : **la relecture et l'écriture se font hors de l'acteur.** L'acteur encode
    /// l'état au `seq` courant (7 ms pour 10 000 blocs, en release) et le confie à une tâche à
    /// part, qui le relit sur un fil bloquant (81 ms) puis l'écrit dans la voie de la page
    /// ([`compact`]). Faites dans l'acteur, elles retardaient la frappe qui déclenchait la
    /// compaction (la tâche qui écrit sa connexion attend que l'acteur rende la main) ou la
    /// suivante, de 90 à 140 ms. L'instantané porte l'état au `seq` encodé, l'écriture ne retire
    /// que le journal jusqu'à ce `seq` (pas ce que l'acteur a écrit depuis), et le store écarte un
    /// instantané plus ancien que celui qu'il garde. Une seule compaction à la fois : celle qui
    /// tombe pendant une autre attend l'écriture suivante.
    fn compact_if_due(&mut self) {
        let many_updates = self.since_snapshot >= self.cfg.snapshot_every.max(1);
        let many_deletions = self.watch.deletions() >= self.cfg.compact_after_deletions.max(1);
        if !many_updates && !many_deletions {
            return;
        }
        if self
            .compaction
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return;
        }
        self.since_snapshot = 0;
        self.watch.reset_deletions();
        let state = self.whole_state();
        let task = compact(self.store.clone(), self.id.node_id, self.last_seq, state);
        self.compaction = Some(tokio::spawn(task.with_current_subscriber()));
    }

    /// Attend la fin de la compaction en vol, s'il y en a une (Ruling R826).
    async fn compaction_finished(&mut self) {
        if let Some(task) = self.compaction.take()
            && let Err(error) = task.await
        {
            tracing::error!(node_id = %self.id.node_id, %error, "compaction interrompue");
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
            c.close();
        }
        // Les connexions voient la fin de l'ancienne diffusion et se ferment.
        let (events, _) = broadcast::channel(self.cfg.broadcast_capacity.max(1));
        self.events = events;
        // Personne ne l'entend (la nouvelle diffusion n'a pas d'abonné) : la liste retenue
        // devient vide, celle que la prochaine arrivée change.
        self.roster_changed();
        Ok(())
    }

    async fn reload(&mut self) -> Result<(), Broken> {
        if let Some(hub) = self.id.hub.upgrade() {
            hub.count_reload();
        }
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

    /// La liste des présents d'après les connexions admises et pas en train de se fermer
    /// (Rulings R136, R143) : une ligne par compte, dans l'ordre de leurs identifiants, qui ne
    /// bouge pas quand un onglet arrive ou part. Une connexion qui se ferme (session fermée,
    /// droits retirés, message refusé) n'y figure plus dès sa fermeture, sans attendre son
    /// départ : un arrivant ne voit pas quelqu'un dont le droit vient d'être retiré. Une
    /// connexion sans compte n'y figure pas. Un compte peut écrire si l'une de ses connexions le
    /// peut (droit et schéma : ce qu'elle peut faire, pas ce qu'elle a demandé). Son nom est
    /// celui de sa connexion la plus récente, lu avec la session la plus récente.
    fn roster(&self) -> Vec<RosterLine> {
        // Par compte : la connexion la plus récente, son nom, et le droit d'écrire.
        let mut accounts: BTreeMap<Uuid, (ConnId, Arc<str>, bool)> = BTreeMap::new();
        let named = |c: &&Conn| c.listed && !c.closing;
        for (conn, c) in self.conns.iter().filter(|(_, c)| named(c)) {
            let Some(account) = c.actor else { continue };
            let name = c
                .name
                .clone()
                .unwrap_or_else(|| presence::display_name("").into());
            let line = accounts
                .entry(account)
                .or_insert_with(|| (*conn, name.clone(), false));
            if *conn > line.0 {
                (line.0, line.1) = (*conn, name);
            }
            line.2 |= c.writable;
        }
        accounts
            .into_iter()
            .map(|(id, (_, name, can_write))| RosterLine {
                id,
                name,
                can_write,
            })
            .collect()
    }

    /// Diffuse la liste des présents à toutes les connexions de la page, si elle a changé
    /// depuis la dernière diffusion. Appelé à chaque arrivée, à chaque fermeture et à chaque
    /// départ, et seulement là : une frappe ne calcule ni n'envoie rien de plus. Le départ d'une
    /// connexion déjà fermée ne change plus rien, et n'envoie rien. `true` : elle a changé, et
    /// elle est partie.
    fn roster_changed(&mut self) -> bool {
        let roster = self.roster();
        if roster == self.roster {
            return false;
        }
        self.roster = roster;
        self.broadcast(None, protocol::roster(&self.roster));
        true
    }

    fn broadcast(&self, from: Option<ConnId>, message: Vec<u8>) {
        // Sans abonné, il n'y a personne à prévenir.
        let _ = self.events.send(Event::Message(Arc::new(Broadcast {
            from,
            to: None,
            editors_only: false,
            message,
        })));
    }

    /// La trame de page `event`, à qui peut relire ce qui a changé : `comments` à toutes les
    /// connexions de la page, lecteurs compris ; `versions` aux seules connexions dont le compte
    /// a le droit d'écrire (`ConnInfo::can_write`, Ruling R355) : l'historique se lit à partir de
    /// « Peut modifier » (Ruling R203), et un lecteur n'a pas à apprendre qu'une version vient
    /// d'être coupée, ni combien.
    pub(crate) fn page_event(&self, event: PageEvent) {
        let _ = self.events.send(Event::Message(Arc::new(Broadcast {
            from: None,
            to: None,
            editors_only: event == PageEvent::Versions,
            message: protocol::page_event(event),
        })));
    }

    /// Un message pour la connexion `conn` seule, à sa place dans la diffusion de la page.
    fn send_to(&self, conn: ConnId, message: Vec<u8>) {
        let _ = self.events.send(Event::Message(Arc::new(Broadcast {
            from: None,
            to: Some(conn),
            editors_only: false,
            message,
        })));
    }
}

#[cfg(test)]
mod tests {
    //! Ce que les tests d'intégration ne voient pas : l'instant entre la fermeture d'une
    //! connexion et son départ. Une connexion fermée part dès qu'elle lit la diffusion, trop
    //! vite pour qu'un arrivant tombe à coup sûr entre les deux. Ici, l'acteur est mené à la
    //! main, et le départ n'arrive que quand le test l'appelle.

    use mementum_store::{NewNode, NodeKind, VersionReason};
    use yrs::Transact;
    use yrs::encoding::read::{Cursor, Read};
    use yrs::sync::awareness::AwarenessUpdateEntry;
    use yrs::updates::decoder::Decode;

    use super::*;

    /// Un acteur sur une page neuve d'une base temporaire, sans hub. La boîte aux lettres est
    /// rendue pour rester ouverte (`join` en a besoin) ; rien ne la lit.
    async fn actor(dir: &tempfile::TempDir) -> (Actor, Commands, UnboundedReceiver<Command>) {
        actor_with(dir, SyncConfig::default()).await
    }

    /// [`actor`], réglé par `cfg`.
    async fn actor_with(
        dir: &tempfile::TempDir,
        cfg: SyncConfig,
    ) -> (Actor, Commands, UnboundedReceiver<Command>) {
        actor_on(dir, cfg, mementum_store::StoreOptions::default()).await
    }

    /// [`actor_with`], sur une base ouverte avec `options` (un disque lent, par exemple).
    async fn actor_on(
        dir: &tempfile::TempDir,
        cfg: SyncConfig,
        options: mementum_store::StoreOptions,
    ) -> (Actor, Commands, UnboundedReceiver<Command>) {
        let store = Store::open_with(&dir.path().join("mementum.db"), options)
            .await
            .unwrap();
        let space = store.create_space("Essais", None).await.unwrap();
        let node_id = store
            .create_node(NewNode {
                space_id: space.id,
                parent_id: None,
                after: None,
                kind: NodeKind::Page,
                title: "Page".into(),
                created_by: None,
                grants: Vec::new(),
                doc: None,
            })
            .await
            .unwrap()
            .id;
        let loaded = load(&store, node_id).await.unwrap();
        let (commands, rx) = mpsc::unbounded_channel();
        let id = Identity {
            node_id,
            generation: 0,
            hub: Weak::new(),
            me: commands.downgrade(),
        };
        let actor = Actor::new(id, store, cfg, loaded, Slice::default());
        (actor, commands, rx)
    }

    /// Rend illisible l'état que la page encode, par yrs seul, sans la garde : un texte dans le
    /// paragraphe, et dedans un contenu JSON écrit comme yrs le lit (`len`, puis `len + 1`
    /// chaînes). yrs l'écrit ensuite en `len` chaînes, qu'il ne relit pas (Ruling R815).
    fn spoil(page: &PageDoc) {
        let state = yrs::StateVector::decode_v1(&page.state_vector()).unwrap();
        let server = state.iter().next().unwrap().0.get();
        let var = |out: &mut Vec<u8>, mut value: u64| {
            while value > 0x7F {
                out.push(0x80 | (value & 0x7F) as u8);
                value >>= 7;
            }
            out.push(value as u8);
        };
        let mut u = vec![1, 2];
        var(&mut u, 7_000);
        // Le texte, dans le paragraphe (horloge 3 du client du serveur).
        u.extend_from_slice(&[0, 7, 0]);
        var(&mut u, server);
        u.extend_from_slice(&[3, 6]);
        // Le JSON, dans le texte.
        u.extend_from_slice(&[2, 0]);
        var(&mut u, 7_000);
        u.extend_from_slice(&[0, 0, 3, b'"', b'x', b'"', 0]);
        page.yrs_doc()
            .transact_mut()
            .apply_update(yrs::Update::decode_v1(&u).unwrap())
            .unwrap();
    }

    /// Ruling R816 : une compaction ne remplace jamais le journal par un état que le serveur ne
    /// relit pas. Sur une page saine, la compaction écrit l'instantané et efface le journal ; sur
    /// une page dont l'état ne se relit plus, elle n'écrit rien et le journal reste. Et la version
    /// d'avant une restauration, ou de fin, n'est pas écrite.
    #[tokio::test]
    async fn a_compaction_never_replaces_the_journal_with_an_unreadable_state() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = SyncConfig {
            snapshot_every: 1,
            ..SyncConfig::default()
        };
        let (mut actor, _commands, _mailbox) = actor_with(&dir, cfg).await;
        let node = actor.id.node_id;
        // Saine : la compaction a lieu.
        actor.persist(&[&[0, 0]], None).await.unwrap();
        actor.compact_if_due();
        actor.compaction_finished().await;
        let state = actor.store.load_doc(node).await.unwrap();
        assert_eq!(state.snapshot.as_ref().map(|(seq, _)| *seq), Some(1));
        assert!(state.updates.is_empty(), "le journal compacté reste");

        spoil(&actor.page);
        let spoiled = actor.page.encode_state();
        assert!(
            PageDoc::check_state(&spoiled).is_err(),
            "l'état abîmé se relit"
        );
        actor.persist(&[&[0, 0]], None).await.unwrap();
        actor.compact_if_due();
        actor.compaction_finished().await;
        let state = actor.store.load_doc(node).await.unwrap();
        assert_eq!(
            state.snapshot.as_ref().map(|(seq, _)| *seq),
            Some(1),
            "l'instantané illisible a été écrit"
        );
        assert_eq!(state.updates.len(), 1, "le journal a été effacé");
        assert!(PageDoc::check_state(&state.snapshot.unwrap().1).is_ok());

        for reason in [VersionReason::BeforeRestore, VersionReason::Closing] {
            assert!(actor.cut_version(reason).await.is_err(), "{reason:?}");
        }
        assert!(actor.store.list_versions(node).await.unwrap().is_empty());
    }

    /// Ruling R826 : la compaction se fait hors de l'acteur, une à la fois. Sur un disque lent
    /// (chaque écriture dure 300 ms de plus), celle qui tombe pendant une autre attend l'écriture
    /// suivante, sans rien perdre de son compte ; et l'acteur qui s'arrête attend celle qui est en
    /// vol avant de quitter le registre (sinon le serveur fermerait la base sous elle).
    #[tokio::test]
    async fn one_compaction_at_a_time_and_the_actor_waits_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = SyncConfig {
            snapshot_every: 1,
            ..SyncConfig::default()
        };
        let slow = mementum_store::StoreOptions {
            write_delay: std::time::Duration::from_millis(300),
            ..mementum_store::StoreOptions::default()
        };
        let (mut actor, commands, mailbox) = actor_on(&dir, cfg, slow).await;
        let node = actor.id.node_id;
        actor.persist(&[&[0, 0]], None).await.unwrap();
        actor.compact_if_due();
        assert!(actor.compaction.is_some(), "aucune compaction lancée");
        // Une autre est due pendant que la première s'écrit : elle attend.
        actor.since_snapshot = 5;
        actor.compact_if_due();
        assert_eq!(
            actor.since_snapshot, 5,
            "une seconde compaction a été lancée"
        );
        actor.compaction_finished().await;
        let state = actor.store.load_doc(node).await.unwrap();
        assert_eq!(state.snapshot.as_ref().map(|(seq, _)| *seq), Some(1));
        // L'écriture suivante la lance ; l'acteur qui s'arrête l'attend.
        actor.persist(&[&[0, 0]], None).await.unwrap();
        actor.compact_if_due();
        assert_eq!(actor.since_snapshot, 0);
        drop(commands);
        actor.serve(mailbox).await;
        let state = Store::open(&dir.path().join("mementum.db"))
            .await
            .unwrap()
            .load_doc(node)
            .await
            .unwrap();
        assert_eq!(
            state.snapshot.as_ref().map(|(seq, _)| *seq),
            Some(2),
            "l'acteur s'est arrêté avant la fin de la compaction"
        );
        assert!(state.updates.is_empty());
    }

    /// Ruling R859 : la relecture de l'état d'une compaction (81 ms pour 10 000 blocs) ne se fait
    /// jamais sur le fil de l'acteur : la frappe qui la déclenche, et celles qui suivent, l'y
    /// attendraient. Le compte est celui du fil qui lance la compaction ; la relecture a bien lieu,
    /// ailleurs, puisque l'instantané s'écrit.
    #[tokio::test]
    async fn the_state_of_a_compaction_is_read_again_off_the_thread_of_the_actor() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = SyncConfig {
            snapshot_every: 1,
            ..SyncConfig::default()
        };
        let (mut actor, _commands, _mailbox) =
            actor_on(&dir, cfg, mementum_store::StoreOptions::default()).await;
        actor.persist(&[&[0, 0]], None).await.unwrap();
        let before = PageDoc::state_checks_on_this_thread();
        actor.compact_if_due();
        actor.compaction_finished().await;
        assert_eq!(
            PageDoc::state_checks_on_this_thread(),
            before,
            "l'état a été relu sur le fil de l'acteur"
        );
        let state = actor.store.load_doc(actor.id.node_id).await.unwrap();
        assert_eq!(state.snapshot.as_ref().map(|(seq, _)| *seq), Some(1));
    }

    /// Ruling R832 : une restauration que le filet de R816 refuse (l'état de la page ne se relit
    /// pas : aucune version ne garderait ce qu'elle remplace) le dit comme une restauration
    /// refusée, et ne change rien ; de même pour une version dont l'état ne se relit pas.
    #[tokio::test]
    async fn a_restore_refused_by_an_unreadable_state_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let (mut actor, _commands, _mailbox) = actor(&dir).await;
        let node = actor.id.node_id;
        let other = {
            let (page, _) = PageDoc::new_page();
            page.write_blocks(&mementum_md::parse("Autre contenu\n").unwrap())
                .unwrap();
            page.encode_state()
        };
        let version = actor
            .store
            .put_version(node, 0, &other, &[], VersionReason::Periodic)
            .await
            .unwrap();
        spoil(&actor.page);
        let journal = actor.store.load_doc(node).await.unwrap();
        let refused = actor.restore(version, None).await.ok().unwrap();
        assert!(
            matches!(refused, Err(SyncError::RestoreRefused(why)) if why == crate::PAGE_UNREADABLE),
            "{refused:?}"
        );
        assert_eq!(actor.store.load_doc(node).await.unwrap(), journal);
        assert_eq!(actor.store.list_versions(node).await.unwrap().len(), 1);
        // Une version dont l'état ne se relit pas.
        let unreadable = actor
            .store
            .put_version(node, 0, &[1, 2, 3], &[], VersionReason::Periodic)
            .await
            .unwrap();
        let refused = actor.restore(unreadable, None).await.ok().unwrap();
        assert!(
            matches!(refused, Err(SyncError::RestoreRefused(why)) if why == crate::VERSION_UNREADABLE),
            "{refused:?}"
        );
        assert_eq!(actor.store.load_doc(node).await.unwrap(), journal);
    }

    /// Ce qu'un test voit des calculs longs de son moteur.
    #[derive(Default)]
    struct LongWork {
        armed: std::sync::atomic::AtomicBool,
        seen: std::sync::atomic::AtomicUsize,
    }

    impl LongWork {
        /// Les calculs longs comptent à partir d'ici.
        fn arm(&self) {
            self.armed.store(true, std::sync::atomic::Ordering::SeqCst);
        }

        fn seen(&self) -> usize {
            self.seen.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    /// Mène `scene` sur un moteur tokio à **un seul fil**. Au début de chaque calcul long, une tâche
    /// neuve est lancée sur ce moteur, et le calcul n'avance pas tant qu'elle n'a pas tourné : un
    /// calcul qui retient son fil ne la voit jamais, et le test échoue au bout du délai. Aucun
    /// minutage ne peut le faire passer à tort (Ruling R863).
    fn engine_stays_free<F>(case: &str, scene: impl FnOnce(Arc<LongWork>) -> F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        use std::sync::atomic::Ordering;
        let thread = format!("travail-long-{case}");
        let watch = Arc::new(LongWork::default());
        let (ran, wait) = std::sync::mpsc::channel::<()>();
        let wait = std::sync::Mutex::new(wait);
        let seen = watch.clone();
        crate::long_work::testing::on_long_work(
            &thread,
            Arc::new(move || {
                if !seen.armed.load(Ordering::SeqCst) {
                    return;
                }
                seen.seen.fetch_add(1, Ordering::SeqCst);
                let ran = ran.clone();
                tokio::runtime::Handle::current().spawn(async move {
                    let _ = ran.send(());
                });
                wait.lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(20))
                    .expect("le calcul long retient le fil du moteur");
            }),
        );
        let engine = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name(thread)
            .enable_all()
            .build()
            .unwrap();
        let scene = scene(watch);
        engine.block_on(async { tokio::spawn(scene).await.unwrap() });
    }

    /// Deux frappes de l'onglet `client`, la seconde à la suite de la première.
    fn two_keystrokes(page: &PageDoc, client: u64) -> (Vec<u8>, Vec<u8>) {
        use yrs::{Text, XmlFragment, XmlOut, XmlTextPrelim};
        let tab = yrs::Doc::with_options(yrs::Options {
            client_id: ClientID::new(client),
            offset_kind: yrs::OffsetKind::Utf16,
            ..yrs::Options::default()
        });
        let fragment = tab.get_or_insert_xml_fragment(mementum_crdt::FRAGMENT);
        tab.transact_mut()
            .apply_update(yrs::Update::decode_v1(&page.encode_state()).unwrap())
            .unwrap();
        let (first, text) = {
            let mut txn = tab.transact_mut();
            let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
                panic!("le groupe");
            };
            let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
                panic!("le bloc");
            };
            let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
                panic!("le paragraphe");
            };
            let text = paragraph.insert(&mut txn, 0, XmlTextPrelim::new("a"));
            (txn.encode_update_v1(), text)
        };
        let mut txn = tab.transact_mut();
        text.insert(&mut txn, 1, "b");
        (first, txn.encode_update_v1())
    }

    /// Ruling R863 : la mise à jour d'un client, avec le report d'un bloc recréé et le dédoublonnage
    /// qui la suivent, en un seul calcul.
    #[test]
    fn a_client_update_leaves_the_engine_free() {
        engine_stays_free("client", |watch| async move {
            let dir = tempfile::tempdir().unwrap();
            let (mut actor, _commands, _mailbox) = actor(&dir).await;
            let (keystroke, _) = two_keystrokes(&actor.page, 77);
            watch.arm();
            assert!(actor.client_update(1, None, &keystroke).await.is_ok());
            assert_eq!(watch.seen(), 1);
        });
    }

    /// Ruling R863 : ce qu'une connexion retenait passe par le même chemin.
    #[test]
    fn updates_a_connection_held_leave_the_engine_free() {
        engine_stays_free("retenue", |watch| async move {
            let dir = tempfile::tempdir().unwrap();
            let (mut actor, _commands, _mailbox) = actor(&dir).await;
            let (reply, mut answer) = oneshot::channel();
            actor.join(true, None, None, reply);
            let tab = answer.try_recv().unwrap().unwrap();
            let conn = tab.membership.conn();
            let (first, second) = two_keystrokes(&actor.page, 77);
            let answer = actor.receive_update(conn, None, second).await;
            assert!(matches!(answer, Ok(true)), "la seconde frappe est retenue");
            watch.arm();
            let answer = actor.receive_update(conn, None, first).await;
            assert!(matches!(answer, Ok(true)));
            assert_eq!(watch.seen(), 2, "la frappe, puis celle qui attendait");
        });
    }

    /// Ruling R863 : recharger une page relit son journal.
    #[test]
    fn reloading_a_page_leaves_the_engine_free() {
        engine_stays_free("rechargement", |watch| async move {
            let dir = tempfile::tempdir().unwrap();
            let (mut actor, _commands, _mailbox) = actor(&dir).await;
            watch.arm();
            assert!(actor.reload().await.is_ok());
            assert_eq!(watch.seen(), 1);
        });
    }

    /// Ruling R863 : la lecture seule d'une page gardée (Ruling R907) relit son journal de même.
    #[test]
    fn reading_a_stored_page_leaves_the_engine_free() {
        engine_stays_free("page-gardee", |watch| async move {
            let dir = tempfile::tempdir().unwrap();
            let (actor, _commands, _mailbox) = actor(&dir).await;
            watch.arm();
            let read = stored_page(&actor.store, actor.id.node_id).await;
            assert!(matches!(read, Ok(Some((_, 0)))));
            assert_eq!(watch.seen(), 1);
        });
    }

    /// Ruling R863 : une restauration relit l'état de la version, relit celui de la page avant de
    /// le garder, et écrit le contenu.
    #[test]
    fn a_restore_leaves_the_engine_free() {
        engine_stays_free("restauration", |watch| async move {
            let dir = tempfile::tempdir().unwrap();
            let (mut actor, _commands, _mailbox) = actor(&dir).await;
            let other = {
                let (page, _) = PageDoc::new_page();
                page.write_blocks(&mementum_md::parse("Autre contenu\n").unwrap())
                    .unwrap();
                page.encode_state()
            };
            let version = actor
                .store
                .put_version(actor.id.node_id, 0, &other, &[], VersionReason::Periodic)
                .await
                .unwrap();
            watch.arm();
            let restored = actor.restore(version, None).await.ok().unwrap();
            assert!(restored.is_ok(), "{restored:?}");
            assert_eq!(watch.seen(), 3);
        });
    }

    /// Ruling R863 : une écriture du serveur (`PUT /mmd`), avec ou sans les identifiants gardés.
    #[test]
    fn a_write_of_the_server_leaves_the_engine_free() {
        engine_stays_free("ecriture", |watch| async move {
            let dir = tempfile::tempdir().unwrap();
            let (mut actor, _commands, _mailbox) = actor(&dir).await;
            watch.arm();
            for (text, ids) in [("Un\n", Ids::AsGiven), ("Deux\n", Ids::Matched)] {
                let blocks = mementum_md::parse(text).unwrap();
                let written = actor.server_write(blocks, None, History::Slice, ids).await;
                assert!(written.ok().unwrap().is_ok());
            }
            assert_eq!(watch.seen(), 2);
        });
    }

    /// Une connexion du compte `account`, qui s'appelle `name` et peut écrire : inscrite, puis
    /// admise, comme le fait `Hub::serve_connection_if`.
    fn joined(actor: &mut Actor, account: Uuid, name: &str) -> Joined {
        let (reply, mut answer) = oneshot::channel();
        actor.join(true, Some(account), Some(name), reply);
        let joined = answer.try_recv().unwrap().unwrap();
        actor.admit(joined.membership.conn());
        joined
    }

    /// Les listes des présents que la connexion `joined` a reçues depuis la dernière lecture :
    /// les noms de chacune, dans l'ordre.
    fn rosters(joined: &mut Joined) -> Vec<Vec<String>> {
        let conn = joined.membership.conn();
        let mut received = Vec::new();
        while let Ok(event) = joined.events.try_recv() {
            let Event::Message(message) = event else {
                continue;
            };
            let for_others = message.from == Some(conn) || message.to.is_some_and(|to| to != conn);
            if for_others || message.message.first() != Some(&protocol::MESSAGE_PAGE_EVENT) {
                continue;
            }
            let mut cursor = Cursor::new(&message.message[1..]);
            let frame: serde_json::Value =
                serde_json::from_slice(cursor.read_buf().unwrap()).unwrap();
            if frame["type"] == "roster" {
                let names = frame["users"].as_array().unwrap().iter();
                received.push(
                    names
                        .map(|user| user["name"].as_str().unwrap().to_string())
                        .collect(),
                );
            }
        }
        received
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    /// Une connexion qui se ferme (session fermée, droits retirés, message refusé) ne figure
    /// plus dans la liste des présents dès sa fermeture, avant même son départ : les autres le
    /// reçoivent tout de suite, et un arrivant ne la voit pas. Son départ, qui ne change plus
    /// rien, n'envoie rien.
    #[tokio::test]
    async fn a_connection_being_closed_is_no_longer_named() {
        let dir = tempfile::tempdir().unwrap();
        let (mut actor, _commands, _mailbox) = actor(&dir).await;
        // Dans l'ordre de leurs identifiants, celui de la liste.
        let [alice, bob, mallory, carol, dave] = [1u128, 2, 3, 4, 5].map(Uuid::from_u128);
        let mut alice_tab = joined(&mut actor, alice, "Alice");
        let mallory_tab = joined(&mut actor, mallory, "Mallory");
        assert_eq!(
            rosters(&mut alice_tab).last(),
            Some(&names(&["Alice", "Mallory"]))
        );

        // La session de Mallory se ferme (`Hub::kick_user`) : Alice l'apprend aussitôt.
        actor.close_connections_of(mallory);
        assert_eq!(rosters(&mut alice_tab), [names(&["Alice"])]);
        // Carol arrive avant le départ de Mallory : la liste ne nomme pas Mallory.
        let mut carol_tab = joined(&mut actor, carol, "Carol");
        let both = names(&["Alice", "Carol"]);
        assert_eq!(rosters(&mut alice_tab), vec![both.clone()]);
        assert_eq!(rosters(&mut carol_tab), vec![both.clone()]);
        // Son départ ne change plus rien : rien ne part.
        actor.leave(mallory_tab.membership.conn());
        assert!(rosters(&mut alice_tab).is_empty());

        // Un message refusé (plus de huit clients de présence) ferme la connexion de Bob, par
        // le chemin de toute commande : la liste suit tout de suite.
        let bob_tab = joined(&mut actor, bob, "Bob");
        assert_eq!(rosters(&mut alice_tab), [names(&["Alice", "Bob", "Carol"])]);
        let clients = (100..109)
            .map(|client| {
                let entry = AwarenessUpdateEntry {
                    clock: 1,
                    json: "{}".into(),
                };
                (ClientID::new(client), entry)
            })
            .collect();
        let (reply, mut outcome) = oneshot::channel();
        let refused = Command::Messages {
            conn: bob_tab.membership.conn(),
            messages: vec![ClientMessage::Awareness(AwarenessUpdate { clients })],
            reply,
        };
        assert!(actor.handle(refused).await.is_ok());
        assert!(matches!(outcome.try_recv(), Ok(Outcome::Close)));
        assert_eq!(rosters(&mut alice_tab), vec![both.clone()]);
        actor.leave(bob_tab.membership.conn());
        assert!(rosters(&mut alice_tab).is_empty());

        // Toutes les connexions fermées (`Hub::kick`) : qui arrive avant leur départ n'y voit
        // que lui.
        actor.close_connections();
        let mut dave_tab = joined(&mut actor, dave, "Dave");
        assert_eq!(rosters(&mut dave_tab), [names(&["Dave"])]);
    }
}
