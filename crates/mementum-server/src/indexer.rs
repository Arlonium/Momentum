//! L'indexeur (lot 4, Task S4b ; plan du lot, « Quand ») : une tâche de fond qui tient
//! `blocks_index`, `page_links` et `search_state` à jour, sans jamais rien coûter à une frappe.
//! L'index plein texte n'est pas encore nourri : c'est la Task S4c, qui lira l'outbox que les
//! écritures d'ici remplissent.
//!
//! **Ce qui met une page à l'ordre du jour.**
//! - Le contenu écrit par un acteur : le hub nomme la page ([`Hub::take_touched`]). Elle attend
//!   alors le calme : [`IndexerConfig::after`] sans écriture, [`IndexerConfig::at_most`] au plus
//!   depuis la première écriture non indexée (Ruling R906). C'est le seul signal gardé en
//!   mémoire.
//! - Une page créée ou dupliquée : sa ligne `search_state` à `-1`, écrite dans la transaction qui
//!   la crée (Ruling R918). L'indexeur les relit ([`Store::never_indexed`]) quand
//!   [`Store::changes`] annonce un espace, et les indexe sans attendre de calme.
//! - Le démarrage : le rattrapage ([`Store::stale_pages`]) compare, par lots, le dernier `seq`
//!   gardé de chaque page à ce que l'index reflète. C'est lui qui indexe une instance existante et
//!   rattrape un arrêt brutal. Il tourne en fond : rien ne l'attend.
//! - Une version de la projection qui n'est pas celle du binaire (Ruling R960) : au démarrage,
//!   toutes les pages redeviennent jamais indexées ([`Store::reproject_everything`]), par
//!   tranches, puis la version du binaire est écrite.
//!
//! **Une page à la fois, les pages tour à tour.** Une indexation relit la page que le store garde
//! ([`stored_page`], jamais l'acteur : Ruling R907), la projette ([`project`]) sur le fil bloquant
//! (Ruling R863), puis écrit la différence ([`Store::index_page`], voie de l'index, une
//! transaction bornée à la fois). Elle est menée dans une tâche à elle : `stored_page` gèle sa
//! tâche le temps de relire, et la boucle doit continuer d'entendre l'arrêt et les pages écrites.
//! Quand une page en attente de calme et une page à indexer tout de suite sont dues ensemble,
//! elles passent tour à tour : ni une page écrite sans relâche, ni la première indexation d'une
//! instance ne retient l'autre.
//!
//! **Ce qui échoue.** Une page qu'on ne sait plus relire quitte l'index, est notée à son `seq`
//! ([`Store::index_failed`]) et journalisée une fois : elle n'est pas réessayée avant que son
//! `seq` change. Une indexation qui panique finit de même (Ruling R995). Une erreur de la base
//! est réessayée (1 s, puis le double, 60 s au plus) : sans compter si elle passe d'elle-même (un
//! verrou, une base occupée), [`MAX_TRIES`] fois sinon, après quoi la page quitte l'index elle
//! aussi. Aucune page n'est réessayée sans fin pour une raison qui ne passe pas.
//!
//! **Le démarrage.** Tant que la reprojection et le rattrapage ne sont pas finis, l'indexeur ne
//! sait pas ce qui est en retard : [`IndexerHandle::behind`] répond « en retard » (Ruling R996).
//!
//! **L'arrêt.** [`IndexerHandle::stop`] ne laisse rien commencer et attend l'indexation en
//! cours, [`STOP_WAIT`] au plus (Ruling R997) : `serve` l'appelle après [`Hub::shutdown`] et
//! avant de fermer la base. L'indexeur s'arrête aussi de lui-même quand plus personne ne tient sa
//! poignée.
//!
//! **Limite.** Un seul indexeur, une page à la fois, sans part par compte : une page coûteuse à
//! relire (Ruling R860) retient les pages de toute l'instance le temps de sa relecture.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mementum_search::{PROJECTION_VERSION, project};
use mementum_store::{IndexedBlock, Store, StoreError};
use mementum_sync::{Hub, SyncError, stored_page};
use tokio::sync::broadcast::error::{RecvError, TryRecvError};
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use uuid::Uuid;

/// Le calme attendu sur une page, après sa dernière écriture, avant de l'indexer (Ruling R906).
pub const INDEX_AFTER: Duration = Duration::from_secs(2);

/// Le plus long délai entre la première écriture non indexée d'une page et son indexation, même
/// écrite sans relâche (Ruling R906).
pub const INDEX_AT_MOST: Duration = Duration::from_secs(15);

/// Pages lues par lot : les marqueurs, le rattrapage, la reprojection.
const BATCH: usize = 500;

/// Pages en retard lues dans la base pour répondre à [`IndexerHandle::behind`].
const BEHIND_LIMIT: usize = 1_000;

/// Le premier délai avant de réessayer après une erreur de la base, doublé à chaque échec.
const RETRY_FIRST: Duration = Duration::from_secs(1);

/// Le plus long délai entre deux essais.
const RETRY_AT_MOST: Duration = Duration::from_secs(60);

/// Essais d'une page que la base refuse pour une raison qui ne passe pas d'elle-même : au-delà,
/// la page quitte l'index comme une page illisible (Ruling R995).
const MAX_TRIES: u32 = 5;

/// Ce que l'arrêt attend au plus de l'indexation en cours (Ruling R997). Passé ce délai, la base
/// ferme : une transaction pas encore validée n'a rien écrit, et la page, restée en retard dans
/// la base, est reprise par le rattrapage du démarrage suivant.
pub const STOP_WAIT: Duration = Duration::from_secs(10);

/// Les réglages de l'indexeur.
#[derive(Debug, Clone)]
pub(crate) struct IndexerConfig {
    /// [`INDEX_AFTER`], ou `Config::index_after`.
    pub(crate) after: Duration,
    /// [`INDEX_AT_MOST`], ou `Config::index_at_most`.
    pub(crate) at_most: Duration,
    /// L'origine publique du serveur : un lien qui la porte et mène à une page est un rétrolien.
    pub(crate) origin: Option<Arc<str>>,
}

/// Une page écrite par un acteur, qui attend le calme.
#[derive(Debug, Clone, Copy)]
struct Waiting {
    /// Sa première écriture non indexée.
    first: Instant,
    /// Sa dernière écriture.
    last: Instant,
}

impl Waiting {
    /// Quand l'indexer : après le calme, et au plus tard `at_most` après la première écriture.
    fn due(&self, cfg: &IndexerConfig) -> Instant {
        (self.last + cfg.after).min(self.first + cfg.at_most)
    }
}

/// Une page dont l'indexation a échoué sur une erreur de la base.
#[derive(Debug, Clone, Copy)]
struct Retry {
    at: Instant,
    delay: Duration,
    /// Les essais refusés pour une raison qui ne passe pas d'elle-même.
    tries: u32,
}

/// Ce que l'indexeur sait en retard, en mémoire. Verrouillé brièvement, jamais à travers une
/// attente.
#[derive(Default)]
struct Queues {
    /// Les pages écrites par un acteur, qui attendent le calme.
    waiting: HashMap<Uuid, Waiting>,
    /// Les pages à indexer tout de suite (jamais indexées, rattrapage, nouvel essai), dans
    /// l'ordre d'arrivée, chacune une fois.
    ready: VecDeque<Uuid>,
    queued: HashSet<Uuid>,
    /// Les pages à réessayer plus tard.
    retry: HashMap<Uuid, Retry>,
    /// La page en cours d'indexation.
    running: Option<Uuid>,
    /// À qui le tour, quand une page en attente de calme et une page prête sont dues ensemble.
    ready_turn: bool,
}

impl Queues {
    /// Un acteur a écrit la page.
    fn touch(&mut self, page: Uuid, now: Instant) {
        self.waiting
            .entry(page)
            .and_modify(|waiting| waiting.last = now)
            .or_insert(Waiting {
                first: now,
                last: now,
            });
    }

    /// La page est à indexer tout de suite, sauf si elle l'est déjà, est en cours, ou attend un
    /// nouvel essai. Vrai si elle est entrée dans la file.
    fn make_ready(&mut self, page: Uuid) -> bool {
        if self.running == Some(page) || self.retry.contains_key(&page) {
            return false;
        }
        if self.queued.insert(page) {
            self.ready.push_back(page);
            return true;
        }
        false
    }

    /// La page à indexer maintenant, s'il y en a une : elle devient la page en cours.
    fn next(&mut self, now: Instant, cfg: &IndexerConfig) -> Option<Uuid> {
        let due: Vec<Uuid> = self
            .retry
            .iter()
            .filter(|(_, retry)| retry.at <= now)
            .map(|(page, _)| *page)
            .collect();
        for page in due {
            // Son délai reste noté jusqu'à ce qu'elle passe : un nouvel échec le double.
            if self.queued.insert(page) {
                self.ready.push_back(page);
            }
        }
        let calm = self
            .waiting
            .iter()
            .map(|(page, waiting)| (waiting.due(cfg), *page))
            .filter(|(due, _)| *due <= now)
            .min();
        let page = match (calm, self.ready.front().copied()) {
            (Some((_, page)), Some(_)) if !self.ready_turn => {
                self.ready_turn = true;
                self.waiting.remove(&page);
                page
            }
            (Some((_, page)), None) => {
                self.waiting.remove(&page);
                page
            }
            (_, Some(page)) => {
                self.ready_turn = false;
                self.ready.pop_front();
                self.queued.remove(&page);
                // Une page prête n'attend plus le calme : elle est relue maintenant.
                self.waiting.remove(&page);
                page
            }
            (None, None) => return None,
        };
        self.running = Some(page);
        Some(page)
    }

    /// Le prochain instant où une page sera due, sans rien de prêt.
    fn next_wake(&self, cfg: &IndexerConfig) -> Option<Instant> {
        let calm = self.waiting.values().map(|waiting| waiting.due(cfg));
        let retries = self.retry.values().map(|retry| retry.at);
        calm.chain(retries).min()
    }

    fn is_empty(&self) -> bool {
        self.waiting.is_empty()
            && self.ready.is_empty()
            && self.retry.is_empty()
            && self.running.is_none()
    }

    /// Une page que `seen` voit est en retard.
    fn any(&self, seen: impl Fn(Uuid) -> bool) -> bool {
        self.waiting
            .keys()
            .chain(&self.ready)
            .chain(self.retry.keys())
            .chain(&self.running)
            .any(|page| seen(*page))
    }
}

/// Où en est la boucle, publié à chacun de ses tours.
#[derive(Debug, Clone, Copy, Default)]
struct Round {
    n: u64,
    /// Plus rien à projeter : ni page en attente, ni marqueur à lire, ni rattrapage en cours.
    idle: bool,
    /// La boucle a fini.
    stopped: bool,
}

struct Shared {
    store: Store,
    hub: Arc<Hub>,
    cfg: IndexerConfig,
    queues: Mutex<Queues>,
    /// Réveille la boucle : l'arrêt, ou [`IndexerHandle::settled`].
    wake: Notify,
    stopping: AtomicBool,
    /// [`IndexerHandle::settled`] demande de relire les marqueurs.
    asked: AtomicBool,
    rounds: watch::Sender<Round>,
    /// La tâche de fond a été lancée ([`spawn`]).
    spawned: AtomicBool,
    /// Le démarrage n'est pas fini : la reprojection, s'il y en a une, puis le rattrapage. Tant
    /// qu'il dure, l'indexeur ne sait pas ce qui est en retard ([`IndexerHandle::behind`]).
    starting: AtomicBool,
    /// Les pages relues ([`stored_page`]), pour les tests.
    reads: AtomicU64,
    /// Les pages dont la relecture panique, pour les tests ([`IndexerHandle::panic_reading`]).
    panics: Mutex<HashSet<Uuid>>,
}

impl Shared {
    fn queues(&self) -> MutexGuard<'_, Queues> {
        self.queues.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }
}

/// Tant qu'une poignée vit, l'indexeur tourne ; la dernière lâchée, il s'arrête.
struct Lifeline(Arc<Shared>);

impl Drop for Lifeline {
    fn drop(&mut self) {
        self.0.stopping.store(true, Ordering::SeqCst);
        self.0.wake.notify_one();
    }
}

/// La poignée de l'indexeur ([`spawn`]).
#[derive(Clone)]
pub struct IndexerHandle {
    shared: Arc<Shared>,
    _life: Arc<Lifeline>,
}

/// Lance l'indexeur sur le moteur courant : une tâche de fond, qui commence par son rattrapage.
pub(crate) fn spawn(store: Store, hub: Arc<Hub>, cfg: IndexerConfig) -> IndexerHandle {
    let handle = not_running(store, hub, cfg);
    handle.shared.starting.store(true, Ordering::SeqCst);
    handle.shared.spawned.store(true, Ordering::SeqCst);
    // La tâche n'est pas gardée : sa fin se lit dans les tours qu'elle publie (`Round::stopped`).
    tokio::spawn(run(handle.shared.clone()));
    handle
}

/// Une poignée sans tâche de fond : rien n'est indexé. Pour les tests des routes, qui montent un
/// état sans serveur ; [`IndexerHandle::behind`] y répond d'après la base seule.
pub(crate) fn not_running(store: Store, hub: Arc<Hub>, cfg: IndexerConfig) -> IndexerHandle {
    let shared = Arc::new(Shared {
        store,
        hub,
        cfg,
        queues: Mutex::default(),
        wake: Notify::new(),
        stopping: AtomicBool::new(false),
        asked: AtomicBool::new(false),
        rounds: watch::Sender::new(Round::default()),
        spawned: AtomicBool::new(false),
        starting: AtomicBool::new(false),
        reads: AtomicU64::new(0),
        panics: Mutex::default(),
    });
    IndexerHandle {
        _life: Arc::new(Lifeline(shared.clone())),
        shared,
    }
}

impl IndexerHandle {
    /// Vrai si l'indexeur sait en retard, dans l'espace, une page que `seen` voit : une page
    /// écrite qui attend le calme, une page jamais indexée, une page dont l'index plein texte
    /// n'a pas encore validé un changement (Ruling R915). `seen` ne doit dire vrai que pour des
    /// pages de l'espace que le compte lit : rien d'une page qu'il ne voit pas ne sort d'ici.
    ///
    /// La base ne rend que les [`BEHIND_LIMIT`] premières pages en retard de l'espace, sans les
    /// droits : quand elle en rend autant sans qu'aucune soit vue, la réponse est « en retard »
    /// (Ruling R983).
    ///
    /// Tant que le démarrage n'est pas fini (la reprojection d'une base indexée sous une autre
    /// version, puis le rattrapage), l'indexeur ne sait pas ce qu'un arrêt a laissé en retard :
    /// la réponse est « en retard », pour tous (Ruling R996). Elle le reste si la version gardée
    /// de la projection ne se lit pas.
    ///
    /// Une page dont la lecture a échoué n'est pas en retard : elle a quitté l'index, et rien
    /// d'autre ne viendra avant sa prochaine écriture. Seule sa ligne d'outbox la dit en retard,
    /// le temps que l'index plein texte la valide (Ruling R995).
    pub async fn behind(
        &self,
        space_id: Uuid,
        seen: impl Fn(Uuid) -> bool,
    ) -> Result<bool, StoreError> {
        if self.shared.starting.load(Ordering::SeqCst) {
            return Ok(true);
        }
        if self.shared.queues().any(&seen) {
            return Ok(true);
        }
        let late = self
            .shared
            .store
            .behind_in_space(space_id, BEHIND_LIMIT)
            .await?;
        Ok(late.len() >= BEHIND_LIMIT || late.into_iter().any(seen))
    }

    /// Rend quand l'indexeur n'a plus rien à projeter de ce qui était écrit à l'appel : pages
    /// écrites (leur calme passé), pages jamais indexées, rattrapage. Pour les tests ; rend aussi
    /// si l'indexeur est arrêté, ou n'a jamais été lancé.
    #[doc(hidden)]
    pub async fn settled(&self) {
        if !self.shared.spawned.load(Ordering::SeqCst) {
            return;
        }
        let mut rounds = self.shared.rounds.subscribe();
        let from = rounds.borrow().n;
        self.shared.asked.store(true, Ordering::SeqCst);
        loop {
            let round = *rounds.borrow_and_update();
            // Deux tours au moins : celui qui était en route à l'appel a pu prendre les pages
            // écrites avant ce qui vient de l'être.
            if round.stopped || (round.idle && round.n >= from + 2) {
                return;
            }
            if round.n < from + 2 {
                self.shared.wake.notify_one();
            }
            if rounds.changed().await.is_err() {
                return;
            }
        }
    }

    /// Arrête l'indexeur, et rend quand sa boucle a fini : rien d'autre n'est commencé, et
    /// l'indexation en cours est attendue (sa transaction n'est pas coupée), [`STOP_WAIT`] au
    /// plus (Ruling R997). Ce qui restait à indexer l'est au démarrage suivant. Plusieurs appels
    /// de front attendent tous la même fin.
    pub async fn stop(&self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.shared.wake.notify_one();
        if !self.shared.spawned.load(Ordering::SeqCst) {
            return;
        }
        let mut rounds = self.shared.rounds.subscribe();
        while !rounds.borrow_and_update().stopped {
            if rounds.changed().await.is_err() {
                return;
            }
        }
    }

    /// L'arrêt est demandé. Pour les tests.
    #[doc(hidden)]
    pub fn stopping(&self) -> bool {
        self.shared.stopping()
    }

    /// La page, écrite par un acteur, attend son calme : l'indexeur l'a prise au hub et ne l'a
    /// pas encore relue. Pour les tests.
    #[doc(hidden)]
    pub fn waits_for_calm(&self, page: Uuid) -> bool {
        self.shared.queues().waiting.contains_key(&page)
    }

    /// Combien de pages l'indexeur a relues depuis son lancement. Pour les tests.
    #[doc(hidden)]
    pub fn reads(&self) -> u64 {
        self.shared.reads.load(Ordering::SeqCst)
    }

    /// La relecture de cette page paniquera, comme un défaut de yrs devant un journal hostile.
    /// Pour les tests : rien d'autre ne fait paniquer une relecture à la demande.
    #[doc(hidden)]
    pub fn panic_reading(&self, page: Uuid) {
        self.shared
            .panics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(page);
    }
}

/// Ce qu'est devenue une indexation.
enum Outcome {
    /// La page est à jour dans la base, ou n'existe plus, ou ne se relit plus (notée).
    Done,
    /// La base a refusé. `passing` : un verrou, une base occupée, à réessayer sans compter ; sinon
    /// l'essai compte contre [`MAX_TRIES`].
    Retry { passing: bool },
}

/// Vrai pour une erreur de la base qui passe d'elle-même : un verrou tenu par un autre, une base
/// occupée, un pool qui n'a pas rendu de connexion à temps. Le serveur ne dépend pas de sqlx : la
/// sorte se lit dans le message de SQLite (« database is locked », « database table is locked »,
/// « busy »). Toute autre erreur est tenue pour durable.
fn passing(error: &StoreError) -> bool {
    let StoreError::Sqlx(error) = error else {
        return false;
    };
    let said = error.to_string();
    ["locked", "busy", "timed out"]
        .iter()
        .any(|word| said.contains(word))
}

/// Ce que devient une écriture de l'index refusée.
fn refused(page: Uuid, error: &StoreError) -> Outcome {
    let passing = passing(error);
    tracing::warn!(
        %page, %error, passing,
        "indexation d'une page refusée par la base : nouvel essai plus tard"
    );
    Outcome::Retry { passing }
}

/// Indexe une page : relit ce que le store garde, projette, écrit la différence.
async fn index_one(shared: Arc<Shared>, page: Uuid) -> Outcome {
    shared.reads.fetch_add(1, Ordering::SeqCst);
    let panics = shared
        .panics
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(&page);
    assert!(!panics, "relecture de la page {page} : panique demandée");
    let written = match stored_page(&shared.store, page).await {
        // Une page sans document, ou supprimée entre-temps : elle sort quand même de `-1`.
        Ok(None) => shared.store.index_page(page, 0, &[], false).await,
        Ok(Some((doc, seq))) => {
            let origin = shared.cfg.origin.clone();
            // Lire les blocs et les projeter dépend du contenu : hors des fils du moteur (R863).
            let projected = tokio::task::spawn_blocking(move || {
                #[cfg(test)]
                testing::reached();
                project(&doc.read_blocks(), origin.as_deref())
            })
            .await;
            match projected {
                Ok(projection) => {
                    let rows: Vec<IndexedBlock<'_>> = projection
                        .rows
                        .iter()
                        .map(|row| IndexedBlock {
                            block_id: &row.block_id,
                            kind: row.kind,
                            text: &row.text,
                            hash: &row.hash,
                            links: &row.links,
                        })
                        .collect();
                    shared
                        .store
                        .index_page(page, seq, &rows, projection.truncated)
                        .await
                }
                Err(error) => {
                    tracing::error!(
                        %page, seq, %error,
                        "page impossible à projeter : elle quitte l'index de recherche jusqu'à \
                         sa prochaine écriture"
                    );
                    shared
                        .store
                        .index_failed(page, seq)
                        .await
                        .map(|()| mementum_store::Indexed { changed: 0 })
                }
            }
        }
        Err(SyncError::Store(error)) => Err(error),
        Err(error) => unreadable(&shared.store, page, error.to_string()).await,
    };
    match written {
        Ok(_) => Outcome::Done,
        // Supprimée entre-temps : ses lignes sont parties avec elle.
        Err(StoreError::NotFound) => Outcome::Done,
        Err(error) => refused(page, &error),
    }
}

/// Renonce à indexer la page à ce `seq` (sa relecture a paniqué, ou la base a refusé son
/// indexation [`MAX_TRIES`] fois) : elle quitte l'index comme une page illisible (Ruling R995).
async fn give_up(shared: Arc<Shared>, page: Uuid, why: &'static str) -> Outcome {
    match unreadable(&shared.store, page, why.to_string()).await {
        Ok(_) | Err(StoreError::NotFound) => Outcome::Done,
        Err(error) => refused(page, &error),
    }
}

/// La page ne se relit pas : elle quitte l'index, notée à son `seq`, et c'est dit une fois.
async fn unreadable(
    store: &Store,
    page: Uuid,
    error: String,
) -> Result<mementum_store::Indexed, StoreError> {
    let doc = store.load_doc(page).await?;
    let seq = doc
        .updates
        .last()
        .map(|(seq, _)| *seq)
        .or(doc.snapshot.as_ref().map(|(seq, _)| *seq))
        .unwrap_or(0);
    store.index_failed(page, seq).await?;
    // Dit une fois, l'échec noté : un refus de la base ci-dessus ne le redit pas à chaque essai.
    tracing::error!(
        %page, seq, %error,
        "page impossible à relire : elle quitte l'index de recherche jusqu'à sa prochaine \
         écriture"
    );
    Ok(mementum_store::Indexed { changed: 0 })
}

/// Avant d'indexer : si la base a été indexée sous une autre version de la projection, toutes
/// les pages redeviennent jamais indexées, par tranches ; puis la version du binaire est écrite
/// (Ruling R960). Sans version gardée (une instance neuve, ou d'avant ce lot, dont chaque page
/// est déjà à `-1`), seule la version est écrite.
async fn same_projection(shared: &Shared) -> Result<(), StoreError> {
    match shared.store.projection_version().await {
        Ok(Some(PROJECTION_VERSION)) => return Ok(()),
        Ok(None) => {}
        // Une version illisible n'est pas tenue pour une autre : l'erreur est rendue, et l'appelant
        // réessaie (une erreur de la base se lit pareil).
        Err(error) => return Err(error),
        Ok(Some(_)) => {
            tracing::info!(
                "l'index de recherche a été bâti sous une autre version de la projection : \
                 toutes les pages sont indexées de nouveau, en fond"
            );
            let mut after = None;
            loop {
                if shared.stopping() {
                    return Ok(());
                }
                match shared.store.reproject_everything(after, BATCH).await? {
                    Some(next) => after = Some(next),
                    None => break,
                }
            }
        }
    }
    shared
        .store
        .set_projection_version(PROJECTION_VERSION)
        .await
}

/// Attend `delay`, ou un réveil (l'arrêt).
async fn pause(shared: &Shared, delay: Duration) {
    tokio::select! {
        () = shared.wake.notified() => {}
        () = tokio::time::sleep(delay) => {}
    }
}

/// La boucle de l'indexeur.
async fn run(shared: Arc<Shared>) {
    let _stopped = Stopped(shared.clone());
    // Abonné avant toute lecture des marqueurs : aucune annonce ne passe entre les deux.
    let mut changes = shared.store.changes();
    let mut delay = RETRY_FIRST;
    while !shared.stopping() {
        match same_projection(&shared).await {
            Ok(()) => break,
            Err(error) => {
                tracing::error!(%error, "version de la projection impossible à lire ou à écrire : l'indexation attend");
                pause(&shared, delay).await;
                delay = (delay * 2).min(RETRY_AT_MOST);
            }
        }
    }
    // Les marqueurs sont à lire ; le rattrapage part du début.
    let mut markers = true;
    // Après une erreur de lecture, ou un lot dont toutes les pages attendent un nouvel essai :
    // les deux sources durables ne sont pas relues avant cet instant.
    let mut sources_after: Option<Instant> = None;
    let mut catch_up: Option<Option<Uuid>> = Some(None);
    let mut announced = true;
    // Le travail en cours, un seul à la fois : une indexation, ou (`true`) le renoncement à une
    // page qu'on ne sait pas indexer.
    let mut job: Option<(JoinHandle<Outcome>, bool)> = None;
    let mut round = 0u64;
    while !shared.stopping() {
        let now = Instant::now();
        // Ce qui est arrivé depuis le tour d'avant.
        let touched = shared.hub.take_touched();
        if !touched.is_empty() {
            let mut queues = shared.queues();
            for page in touched {
                queues.touch(page, now);
            }
        }
        while announced {
            match changes.try_recv() {
                Ok(_) | Err(TryRecvError::Lagged(_)) => markers = true,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => announced = false,
            }
        }
        if shared.asked.swap(false, Ordering::SeqCst) {
            markers = true;
            sources_after = None;
        }
        if sources_after.is_some_and(|at| at <= now) {
            sources_after = None;
        }
        // Les sources durables, quand il n'y a rien de prêt : jamais plus d'un lot en mémoire.
        let nothing_ready = job.is_none() && shared.queues().ready.is_empty();
        if nothing_ready && markers && sources_after.is_none() {
            match shared.store.never_indexed(BATCH).await {
                Ok(pages) => {
                    let full = pages.len() >= BATCH;
                    let mut queues = shared.queues();
                    let entered = pages
                        .into_iter()
                        .filter(|page| queues.make_ready(*page))
                        .count();
                    markers = full;
                    if full && entered == 0 {
                        // Tout le lot attend un nouvel essai : ne pas le relire en boucle.
                        sources_after = Some(now + RETRY_FIRST);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "pages jamais indexées impossibles à lire : nouvel essai plus tard");
                    sources_after = Some(now + RETRY_FIRST);
                }
            }
        }
        let nothing_ready =
            job.is_none() && sources_after.is_none() && shared.queues().ready.is_empty();
        if let (true, Some(after)) = (nothing_ready, catch_up) {
            match shared.store.stale_pages(after, BATCH).await {
                Ok(stale) => {
                    let mut queues = shared.queues();
                    for page in stale.pages {
                        queues.make_ready(page);
                    }
                    catch_up = stale.next.map(Some);
                    if catch_up.is_none() {
                        // Le démarrage est fini : ce qui est en retard est en file, ou noté
                        // dans la base.
                        shared.starting.store(false, Ordering::SeqCst);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "rattrapage de l'index impossible à lire : nouvel essai plus tard");
                    sources_after = Some(now + RETRY_FIRST);
                }
            }
        }
        // Rien n'est commencé une fois l'arrêt demandé.
        if job.is_none() && !shared.stopping() {
            let next = shared.queues().next(Instant::now(), &shared.cfg);
            if let Some(page) = next {
                job = Some((tokio::spawn(index_one(shared.clone(), page)), false));
            }
        }
        round += 1;
        let idle = job.is_none() && !markers && catch_up.is_none() && shared.queues().is_empty();
        shared.rounds.send_replace(Round {
            n: round,
            idle,
            stopped: false,
        });
        // Un lot de plus à lire (le rattrapage d'une instance où rien n'est en retard dans ce
        // lot) : pas d'attente.
        let more = job.is_none() && sources_after.is_none() && (markers || catch_up.is_some());
        if more {
            tokio::task::yield_now().await;
            continue;
        }
        let wake_at = if job.is_none() {
            let due = shared.queues().next_wake(&shared.cfg);
            match (due, sources_after) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            }
        } else {
            None
        };
        tokio::select! {
            () = shared.wake.notified() => {}
            () = shared.hub.touched_changed() => {}
            change = changes.recv(), if announced => match change {
                Ok(_) | Err(RecvError::Lagged(_)) => markers = true,
                Err(RecvError::Closed) => announced = false,
            },
            done = async { (&mut job.as_mut().expect("un travail en cours").0).await }, if job.is_some() => {
                let giving_up = job.take().is_some_and(|(_, giving_up)| giving_up);
                job = finished(&shared, done, giving_up).map(|job| (job, true));
            }
            () = async { tokio::time::sleep_until(wake_at.expect("un réveil")).await }, if wake_at.is_some() => {}
        }
    }
    // L'indexation en cours va au bout, sa transaction n'est pas coupée ; mais l'arrêt ne l'attend
    // pas sans fin (une page coûteuse à relire, R860). Elle n'est pas interrompue pour autant :
    // la base, en fermant, attend sa requête en cours et refuse les suivantes, et ce qui n'est
    // pas noté indexé est repris au démarrage suivant.
    if let Some((job, _)) = job
        && tokio::time::timeout(STOP_WAIT, job).await.is_err()
    {
        tracing::warn!(
            "arrêt : l'indexation en cours n'a pas fini à temps, elle sera reprise au démarrage \
             suivant"
        );
    }
}

/// Publie la fin de la boucle quand elle tombe, même sur une panique : qui attend l'arrêt
/// ([`IndexerHandle::stop`], [`IndexerHandle::settled`]) n'attend jamais une boucle morte.
struct Stopped(Arc<Shared>);

impl Drop for Stopped {
    fn drop(&mut self) {
        self.0.rounds.send_modify(|round| {
            round.n += 1;
            round.idle = false;
            round.stopped = true;
        });
    }
}

/// Ce que devient la page en cours quand son travail finit (`giving_up` : c'était un
/// renoncement). Rend le renoncement à lancer, s'il y en a un : la page reste alors la page en
/// cours.
fn finished(
    shared: &Arc<Shared>,
    done: Result<Outcome, tokio::task::JoinError>,
    giving_up: bool,
) -> Option<JoinHandle<Outcome>> {
    let mut queues = shared.queues();
    let page = queues.running?;
    let why = match (done, giving_up) {
        (Ok(Outcome::Done), _) => {
            queues.running = None;
            queues.retry.remove(&page);
            return None;
        }
        // Même la page notée illisible, la base refuse, ou le renoncement a paniqué : la page
        // sort de la mémoire. Restée en retard dans la base, elle est reprise au démarrage
        // suivant, ou à sa prochaine écriture.
        (_, true) => {
            queues.running = None;
            queues.retry.remove(&page);
            tracing::error!(
                %page,
                "page impossible à indexer, et impossible à noter comme telle : abandonnée \
                 jusqu'à sa prochaine écriture ou au prochain démarrage"
            );
            return None;
        }
        (Ok(Outcome::Retry { passing }), false) => {
            let before = queues.retry.get(&page).copied();
            let tries = before.map_or(0, |retry| retry.tries) + u32::from(!passing);
            if tries < MAX_TRIES {
                let delay =
                    before.map_or(RETRY_FIRST, |retry| (retry.delay * 2).min(RETRY_AT_MOST));
                queues.running = None;
                queues.retry.insert(
                    page,
                    Retry {
                        at: Instant::now() + delay,
                        delay,
                        tries,
                    },
                );
                return None;
            }
            "la base a refusé son indexation à chaque essai"
        }
        (Err(_), false) => "son indexation a paniqué",
    };
    queues.retry.remove(&page);
    Some(tokio::spawn(give_up(shared.clone(), page, why)))
}

/// Ce qui permet à un test de se tenir à l'intérieur d'une projection.
#[cfg(test)]
pub(crate) mod testing {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    type Hook = Arc<dyn Fn() + Send + Sync>;

    /// Par nom de fil : les tests d'un même binaire tournent ensemble, chacun sur son moteur.
    static HOOKS: Mutex<Option<HashMap<String, Hook>>> = Mutex::new(None);

    /// `hook` sera appelé au début de chaque projection menée par un fil nommé `thread`.
    pub(crate) fn on_projection(thread: &str, hook: Hook) {
        HOOKS
            .lock()
            .unwrap()
            .get_or_insert_default()
            .insert(thread.to_string(), hook);
    }

    pub(super) fn reached() {
        let thread = std::thread::current();
        let hook = thread.name().and_then(|name| {
            let hooks = HOOKS.lock().unwrap();
            hooks.as_ref()?.get(name).cloned()
        });
        if let Some(hook) = hook {
            hook();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use mementum_crdt::PageDoc;
    use mementum_store::{NewNode, NodeKind};
    use mementum_sync::SyncConfig;

    use super::*;

    fn config() -> IndexerConfig {
        IndexerConfig {
            after: INDEX_AFTER,
            at_most: INDEX_AT_MOST,
            origin: None,
        }
    }

    /// Une page prête (jamais indexée, rattrapage) et une page dont le calme est passé sont dues
    /// ensemble : elles passent tour à tour. Ni la première indexation d'une instance ne retient
    /// une page qu'on vient d'écrire, ni des pages écrites sans relâche ne retiennent le
    /// rattrapage.
    #[tokio::test(start_paused = true)]
    async fn ready_pages_and_calm_pages_take_turns() {
        let cfg = config();
        let mut queues = Queues::default();
        let ready: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();
        let written: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();
        let start = Instant::now();
        for (i, page) in written.iter().enumerate() {
            queues.touch(*page, start + Duration::from_millis(i as u64));
        }
        for page in &ready {
            assert!(queues.make_ready(*page));
        }
        // Avant le calme, seules les pages prêtes passent.
        let mut served = Vec::new();
        served.push(queues.next(start, &cfg).unwrap());
        queues.running = None;
        assert_eq!(served, [ready[0]]);
        let later = start + INDEX_AFTER + Duration::from_secs(1);
        while let Some(page) = queues.next(later, &cfg) {
            queues.running = None;
            served.push(page);
        }
        assert_eq!(
            served,
            [
                ready[0], written[0], ready[1], written[1], ready[2], written[2]
            ],
            "tour à tour, les pages écrites dans l'ordre de leur échéance"
        );
        assert!(queues.is_empty());
    }

    /// Une page écrite de nouveau avant son échéance attend de nouveau son calme, mais pas
    /// au-delà de la borne comptée de sa première écriture (Ruling R906).
    #[tokio::test(start_paused = true)]
    async fn the_calm_restarts_at_each_write_up_to_the_bound() {
        let cfg = config();
        let page = Uuid::now_v7();
        let start = Instant::now();
        let mut queues = Queues::default();
        queues.touch(page, start);
        assert_eq!(queues.next_wake(&cfg), Some(start + INDEX_AFTER));
        queues.touch(page, start + Duration::from_secs(1));
        assert_eq!(
            queues.next_wake(&cfg),
            Some(start + Duration::from_secs(1) + INDEX_AFTER)
        );
        queues.touch(page, start + Duration::from_secs(14));
        assert_eq!(queues.next_wake(&cfg), Some(start + INDEX_AT_MOST));
        assert_eq!(queues.next(start + Duration::from_secs(14), &cfg), None);
        assert_eq!(queues.next(start + INDEX_AT_MOST, &cfg), Some(page));
    }

    /// Ruling R863 : lire les blocs d'une page et les projeter ne retient pas le fil du moteur.
    /// Sur un moteur à **un seul fil**, au début de la projection, une tâche neuve est lancée sur
    /// ce moteur, et la projection n'avance pas tant qu'elle n'a pas tourné : menée sur le fil du
    /// moteur, elle ne la verrait jamais. Aucun minutage ne peut faire passer le test à tort.
    #[test]
    fn indexing_leaves_the_engine_free() {
        let thread = "indexation-moteur-libre";
        let seen = Arc::new(AtomicUsize::new(0));
        let (ran, wait) = std::sync::mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let count = seen.clone();
        testing::on_projection(
            thread,
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                let ran = ran.clone();
                tokio::runtime::Handle::current().spawn(async move {
                    let _ = ran.send(());
                });
                wait.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(20))
                    .expect("la projection retient le fil du moteur");
            }),
        );
        let engine = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name(thread)
            .enable_all()
            .build()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.db");
        engine.block_on(async {
            tokio::spawn(async move {
                let store = Store::open(&path).await.unwrap();
                let space = store.create_space("Espace", None).await.unwrap();
                let (doc, _) = PageDoc::new_page();
                doc.write_blocks(&mementum_md::parse("Un texte\n").unwrap())
                    .unwrap();
                let page = store
                    .create_node(NewNode {
                        space_id: space.id,
                        parent_id: None,
                        after: None,
                        kind: NodeKind::Page,
                        title: "Page".to_string(),
                        created_by: None,
                        grants: Vec::new(),
                        doc: Some(doc.encode_state()),
                    })
                    .await
                    .unwrap()
                    .id;
                let hub = Hub::new(store.clone(), SyncConfig::default());
                let handle = not_running(store.clone(), hub, config());
                let outcome = index_one(handle.shared.clone(), page).await;
                assert!(matches!(outcome, Outcome::Done));
                let rows = store.indexed_blocks(page).await.unwrap();
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].text, "Un texte");
                store.close().await;
            })
            .await
            .unwrap();
        });
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }
}
