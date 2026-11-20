//! L'indexeur (lot 4, Tasks S4b et S4c ; plan du lot, « Quand ») : une tâche de fond qui tient
//! `blocks_index`, `page_links` et `search_state` à jour, puis l'index plein texte, sans jamais
//! rien coûter à une frappe.
//!
//! **L'index plein texte** (`<données>/search/`, Task S4c) est nourri depuis la base, jamais
//! depuis les documents (Ruling R901) : chaque écriture de la projection laisse une ligne
//! d'outbox par bloc changé (R919), un titre, une création, une suppression ou un échec de
//! lecture une ligne de page entière (R918). Un passage ([`feed`]) se fait en trois temps (R909) :
//! l'**intention** ([`Store::begin_feed`] : un jeton, donné aux lignes prises) ; la
//! **validation** (chaque ligne retirée de l'index puis récrite d'après `blocks_index` et
//! `nodes`, et validée avec le jeton pour charge utile) ; la **clôture**
//! ([`Store::finish_feed`]). Une validation refusée lâche l'index et le rouvre : seul le jeton
//! relu dit si elle a eu lieu. Une clôture refusée laisse l'intention posée : aucune autre ne
//! commence, et le passage suivant **rejoue** ses lignes sous le même jeton, puis clôt. Une
//! intention posée que l'index n'a pas validée est défaite.
//!
//! **L'index et la base, comparés** ([`reconcile`]) à l'ouverture, au démarrage comme après une
//! réouverture : le jeton de l'index est celui de la dernière validation close (en ordre), ou
//! celui de l'intention posée (elle est rejouée, jamais tenue pour faite), ou tout autre : l'index
//! est alors vidé, et toutes les pages passent à l'outbox ([`Store::outbox_everything`]). Tant
//! que ce n'est pas fait, rien n'est cherché ([`IndexerHandle::search`] ne rend rien) et tout
//! est dit en retard.
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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::Context as _;
use mementum_search::{
    Found, IndexError, Opened, PROJECTION_VERSION, PageSet, Query, SearchIndex, project,
};
use mementum_store::{FeedLine, FeedStart, IndexRow, IndexedBlock, Store, StoreError};
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

/// Le dossier de l'index plein texte, dans le dossier des données, à côté de `files/`. Il ne
/// porte rien d'autre que l'index (Ruling R990).
pub(crate) const SEARCH_DIR: &str = "search";

/// Lignes de l'outbox prises par une intention, au plus (Ruling R1003). Une ligne de page entière
/// récrit toutes les entrées de sa page : un passage n'est pas borné en entrées.
const FEED_LINES: usize = 1_000;

/// Tant que des pages sont à indexer, un passage de l'outbox à l'index plein texte attend qu'il
/// n'y en ait plus, mais pas plus longtemps que cela (Ruling R1003) : la première indexation
/// d'une instance ne valide pas l'index à chaque page, et ne le laisse pas vide jusqu'à sa fin.
const FEED_AT_MOST: Duration = Duration::from_secs(1);

/// Ce qu'un index lâché attend au plus des recherches en cours qui le tiennent encore.
const RELEASE_WAIT: Duration = Duration::from_secs(5);

/// Ce que le démarrage attend au plus d'un index encore tenu dans ce processus (une application
/// lâchée juste avant, dont l'indexeur n'a pas fini de s'arrêter), avant de le dire occupé.
const BUSY_WAIT: Duration = Duration::from_secs(2);

/// Les points où un test arrête l'indexeur, comme un processus tué là.
const HALT_BEFORE_COMMIT: u8 = 1;
const HALT_BEFORE_CLOSE: u8 = 2;

/// L'index plein texte ouvert au démarrage, et son dossier.
pub(crate) struct Text {
    dir: PathBuf,
    opened: Opened,
}

/// Ouvre l'index plein texte du dossier des données, créé s'il manque. Un dossier illisible ou
/// d'une autre forme est vidé par [`SearchIndex::open`] ; un dossier qui porte autre chose qu'un
/// index, ou qu'un autre processus tient, arrête le démarrage sans que rien y soit touché.
pub(crate) async fn open_text(data_dir: &Path) -> anyhow::Result<Text> {
    let dir = data_dir.join(SEARCH_DIR);
    let deadline = std::time::Instant::now() + BUSY_WAIT;
    loop {
        let at = dir.clone();
        let opened = tokio::task::spawn_blocking(move || SearchIndex::open(&at))
            .await
            .context("ouverture de l'index de recherche")?;
        match opened {
            Ok(opened) => return Ok(Text { dir, opened }),
            Err(IndexError::Busy) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => {
                return Err(anyhow::Error::new(error).context(format!(
                    "impossible d'ouvrir l'index de recherche {}",
                    dir.display()
                )));
            }
        }
    }
}

/// L'index plein texte, tel que l'indexeur le tient. Verrouillé brièvement, jamais à travers une
/// attente.
#[derive(Default)]
struct TextIndex {
    /// Son dossier ; `None` : une poignée sans index ([`not_running`]).
    dir: Option<PathBuf>,
    /// L'index ouvert ; `None` : lâché après une erreur, à rouvrir, ou jamais ouvert.
    index: Option<Arc<SearchIndex>>,
    /// Le jeton de sa dernière validation, lu à l'ouverture puis tenu à chaque validation.
    token: Option<String>,
    /// Ce jeton a été comparé à ceux de la base ([`reconcile`]) depuis l'ouverture.
    reconciled: bool,
}
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
    /// Plus rien à projeter ni à valider : ni page en attente, ni marqueur à lire, ni rattrapage
    /// en cours, ni ligne d'outbox à passer à l'index plein texte, ni intention posée.
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
    /// L'index plein texte.
    text: Mutex<TextIndex>,
    /// L'index plein texte peut être cherché : son jeton est celui de la dernière validation
    /// close, ou il vient d'être vidé, ou une intention rejouée vient d'être close. Faux de
    /// l'ouverture à la comparaison des jetons, et tant qu'une intention retrouvée validée n'est
    /// pas rejouée : l'index pourrait porter un texte que la base n'a pas.
    searchable: AtomicBool,
    /// Une reconstruction a vidé l'index et n'a pas fini de passer toutes les pages à l'outbox :
    /// rien ne dit encore lesquelles sont en retard.
    rebuilding: AtomicBool,
    /// Lignes de l'outbox prises par intention ([`FEED_LINES`] ; réglable pour les tests).
    feed_lines: AtomicUsize,
    /// Compteurs, pour les tests : entrées écrites dans l'index plein texte, intentions posées,
    /// validations refusées, clôtures refusées, reconstructions lancées.
    puts: AtomicU64,
    intents: AtomicU64,
    commit_refusals: AtomicU64,
    close_refusals: AtomicU64,
    rebuilds: AtomicU64,
    /// Le dernier passage clos, pour les mesures : ses lignes, ses entrées écrites, sa durée de
    /// l'intention à la clôture.
    last_feed: Mutex<Option<(usize, u64, Duration)>>,
    /// Les validations et les clôtures à refuser, pour les tests.
    refuse_commits: AtomicU32,
    refuse_closes: AtomicU32,
    /// Le point où l'indexeur s'arrête, pour les tests ([`HALT_BEFORE_COMMIT`],
    /// [`HALT_BEFORE_CLOSE`]), et s'il y est arrivé.
    halt: AtomicU8,
    halted: AtomicBool,
    /// Plus aucun travail n'est commencé, pour les tests ([`IndexerHandle::freeze`]).
    frozen: AtomicBool,
    /// Les travaux commencés par la boucle (indexations, passages), pour les tests.
    started: AtomicU64,
}

impl Shared {
    fn queues(&self) -> MutexGuard<'_, Queues> {
        self.queues.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn text(&self) -> MutexGuard<'_, TextIndex> {
        self.text.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    /// L'index plein texte ne dit pas encore ce que la base porte : il n'est pas comparé, pas
    /// rejoué, ou sa reconstruction n'a pas encore nommé toutes les pages.
    fn text_unsure(&self) -> bool {
        let has_text = self.text().dir.is_some();
        has_text
            && (self.rebuilding.load(Ordering::SeqCst) || !self.searchable.load(Ordering::SeqCst))
    }

    /// Vrai si l'indexeur doit s'arrêter ici, à la demande d'un test.
    fn halts_at(&self, point: u8) -> bool {
        let here = self.halt.load(Ordering::SeqCst) == point;
        if here {
            self.halted.store(true, Ordering::SeqCst);
        }
        here
    }
}

/// Prend un des refus demandés par un test, s'il en reste.
fn take_refusal(left: &AtomicU32) -> bool {
    let mut n = left.load(Ordering::SeqCst);
    while n > 0 {
        match left.compare_exchange(n, n - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(now) => n = now,
        }
    }
    false
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
pub(crate) fn spawn(store: Store, hub: Arc<Hub>, cfg: IndexerConfig, text: Text) -> IndexerHandle {
    let handle = with_text(store, hub, cfg, Some(text));
    handle.shared.starting.store(true, Ordering::SeqCst);
    handle.shared.spawned.store(true, Ordering::SeqCst);
    // La tâche n'est pas gardée : sa fin se lit dans les tours qu'elle publie (`Round::stopped`).
    tokio::spawn(run(handle.shared.clone()));
    handle
}

/// Une poignée sans tâche de fond : rien n'est indexé. Pour les tests des routes, qui montent un
/// état sans serveur ; [`IndexerHandle::behind`] y répond d'après la base seule.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn not_running(store: Store, hub: Arc<Hub>, cfg: IndexerConfig) -> IndexerHandle {
    with_text(store, hub, cfg, None)
}

/// Une poignée sans tâche de fond, avec ou sans index plein texte.
fn with_text(store: Store, hub: Arc<Hub>, cfg: IndexerConfig, text: Option<Text>) -> IndexerHandle {
    let text = text.map_or_else(TextIndex::default, |text| TextIndex {
        dir: Some(text.dir),
        index: Some(Arc::new(text.opened.index)),
        token: text.opened.token,
        reconciled: false,
    });
    let shared = Arc::new(Shared {
        text: Mutex::new(text),
        searchable: AtomicBool::new(false),
        rebuilding: AtomicBool::new(false),
        feed_lines: AtomicUsize::new(FEED_LINES),
        puts: AtomicU64::new(0),
        intents: AtomicU64::new(0),
        commit_refusals: AtomicU64::new(0),
        close_refusals: AtomicU64::new(0),
        rebuilds: AtomicU64::new(0),
        last_feed: Mutex::default(),
        refuse_commits: AtomicU32::new(0),
        refuse_closes: AtomicU32::new(0),
        halt: AtomicU8::new(0),
        halted: AtomicBool::new(false),
        frozen: AtomicBool::new(false),
        started: AtomicU64::new(0),
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
        if self.shared.starting.load(Ordering::SeqCst) || self.shared.text_unsure() {
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

    /// Cherche dans l'index plein texte, parmi les pages de `allowed` seulement
    /// ([`SearchIndex::search`]) : c'est l'appelant qui bâtit cet ensemble, des droits du compte,
    /// à chaque requête (Rulings R921, R927). Un calcul, à mener sur le fil bloquant.
    ///
    /// Rien n'est rendu tant que l'index n'a pas été comparé à la base, ni pendant qu'une
    /// intention retrouvée validée est rejouée, ni par une poignée sans index : l'index pourrait
    /// porter un texte que la base n'a pas (Review Focus 5). [`IndexerHandle::behind`] dit alors
    /// « en retard ». Pendant une reconstruction, seul ce qui est déjà reconstruit est trouvé.
    pub fn search(
        &self,
        query: &Query,
        allowed: &PageSet,
        skip: usize,
        limit: usize,
    ) -> Result<Found, IndexError> {
        if !self.shared.searchable.load(Ordering::SeqCst) {
            return Ok(Found::default());
        }
        let Some(index) = self.shared.text().index.clone() else {
            return Ok(Found::default());
        };
        index.search(query, allowed, skip, limit)
    }

    /// Rend quand l'indexeur n'a plus rien à projeter ni à valider de ce qui était écrit à
    /// l'appel : pages écrites (leur calme passé), pages jamais indexées, rattrapage, lignes
    /// d'outbox passées à l'index plein texte et closes. Pour les tests ; rend aussi si
    /// l'indexeur est arrêté, ou n'a jamais été lancé.
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

    /// Entrées écrites dans l'index plein texte depuis le lancement. Pour les tests.
    #[doc(hidden)]
    pub fn entries_written(&self) -> u64 {
        self.shared.puts.load(Ordering::SeqCst)
    }

    /// Intentions posées depuis le lancement (une intention rejouée n'en est pas une neuve). Pour
    /// les tests.
    #[doc(hidden)]
    pub fn intents(&self) -> u64 {
        self.shared.intents.load(Ordering::SeqCst)
    }

    /// Validations de l'index plein texte refusées depuis le lancement. Pour les tests.
    #[doc(hidden)]
    pub fn commit_refusals(&self) -> u64 {
        self.shared.commit_refusals.load(Ordering::SeqCst)
    }

    /// Clôtures refusées depuis le lancement. Pour les tests.
    #[doc(hidden)]
    pub fn close_refusals(&self) -> u64 {
        self.shared.close_refusals.load(Ordering::SeqCst)
    }

    /// Reconstructions de l'index plein texte lancées depuis le lancement. Pour les tests.
    #[doc(hidden)]
    pub fn rebuilds(&self) -> u64 {
        self.shared.rebuilds.load(Ordering::SeqCst)
    }

    /// Le dernier passage clos de l'outbox à l'index plein texte : ses lignes, ses entrées
    /// écrites, sa durée de l'intention à la clôture. Pour les mesures.
    #[doc(hidden)]
    pub fn last_feed(&self) -> Option<(usize, u64, Duration)> {
        *self
            .shared
            .last_feed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Les `n` prochaines validations de l'index plein texte sont refusées, sans être tentées.
    /// Pour les tests : rien d'autre ne fait refuser une validation à la demande.
    #[doc(hidden)]
    pub fn refuse_commits(&self, n: u32) {
        self.shared.refuse_commits.store(n, Ordering::SeqCst);
    }

    /// Les `n` prochaines clôtures sont refusées, sans être tentées. Pour les tests.
    #[doc(hidden)]
    pub fn refuse_closes(&self, n: u32) {
        self.shared.refuse_closes.store(n, Ordering::SeqCst);
    }

    /// L'indexeur s'arrêtera, comme un processus tué là, entre l'intention et la validation de
    /// son prochain passage : rien n'est validé dans l'index plein texte. Pour les tests.
    #[doc(hidden)]
    pub fn halt_before_the_commit(&self) {
        self.shared.halt.store(HALT_BEFORE_COMMIT, Ordering::SeqCst);
    }

    /// L'indexeur s'arrêtera, comme un processus tué là, entre la validation et la clôture de
    /// son prochain passage. Pour les tests.
    #[doc(hidden)]
    pub fn halt_before_the_close(&self) {
        self.shared.halt.store(HALT_BEFORE_CLOSE, Ordering::SeqCst);
    }

    /// L'indexeur s'est arrêté au point demandé. Pour les tests.
    #[doc(hidden)]
    pub fn halted(&self) -> bool {
        self.shared.halted.load(Ordering::SeqCst)
    }

    /// Fige l'indexeur : plus aucune indexation ni aucun passage à l'index plein texte n'est
    /// commencé (celui qui est en cours finit), et la recherche répond sur l'index tel qu'il
    /// est, ce que [`IndexerHandle::stop`] ne permet pas (il lâche l'index). Tant qu'il reste du
    /// travail, [`IndexerHandle::settled`] ne rend plus. Pour les tests : ce qu'une requête doit
    /// tenir sans attendre l'indexeur (un droit retiré) se prouve ainsi.
    #[doc(hidden)]
    pub fn freeze(&self) {
        self.shared.frozen.store(true, Ordering::SeqCst);
    }

    /// Les tours de la boucle depuis le lancement. Pour les tests : une barrière (la boucle a
    /// tourné depuis tel événement), jamais une mesure.
    #[doc(hidden)]
    pub fn rounds(&self) -> u64 {
        self.shared.rounds.borrow().n
    }

    /// Les travaux (indexations, passages à l'index plein texte) que la boucle a commencés
    /// depuis le lancement, comptés avant que le tour qui les commence soit publié. Pour les
    /// tests.
    #[doc(hidden)]
    pub fn jobs_started(&self) -> u64 {
        self.shared.started.load(Ordering::SeqCst)
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

/// Ce qu'est devenu un passage de l'outbox à l'index plein texte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fed {
    /// Rien n'attend l'index plein texte, et aucune intention n'est posée.
    Nothing,
    /// Une intention a été close, ou défaite : il peut rester des lignes.
    Again,
    /// Refusé (la base, l'index) : nouvel essai plus tard, l'intention restée telle quelle.
    Retry,
    /// Arrêté au point nommé par un test.
    Halted,
}

/// Mène une fonction de l'index sur le fil bloquant : écrire dans le moteur, valider, ouvrir et
/// vider un dossier dépendent d'un contenu ou du disque (Ruling R863).
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, IndexError> + Send + 'static,
) -> anyhow::Result<T> {
    Ok(tokio::task::spawn_blocking(work)
        .await
        .context("une tâche de l'index de recherche a paniqué")??)
}

/// Attend que plus personne d'autre ne tienne l'index (une recherche en cours), [`RELEASE_WAIT`]
/// au plus, et le rend ; `None` s'il est encore tenu : il sera lâché par qui le tient. Bloquant.
fn sole_owner(mut index: Arc<SearchIndex>) -> Option<SearchIndex> {
    let deadline = std::time::Instant::now() + RELEASE_WAIT;
    loop {
        match Arc::try_unwrap(index) {
            Ok(index) => return Some(index),
            Err(_) if std::time::Instant::now() >= deadline => return None,
            Err(held) => {
                index = held;
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

/// Lâche l'index plein texte : après une erreur d'écriture ou de validation, il ne se réutilise
/// pas (Ruling R992), et à l'arrêt, plus aucun fichier de son dossier ne doit rester tenu. Rien
/// n'est cherché jusqu'à ce qu'il soit rouvert et comparé à la base.
async fn release(shared: &Shared) {
    shared.searchable.store(false, Ordering::SeqCst);
    let_go(shared).await;
}

/// Lâche l'index sans rien dire de ce qu'il valait : à l'arrêt, [`IndexerHandle::behind`] répond
/// encore d'après la base, et plus rien n'est cherché faute d'index.
async fn let_go(shared: &Shared) {
    let held = {
        let mut text = shared.text();
        text.token = None;
        text.reconciled = false;
        text.index.take()
    };
    if let Some(index) = held {
        // Lâché sur le fil bloquant : l'écrivain du moteur joint ses fils en partant.
        let _ = tokio::task::spawn_blocking(move || drop(sole_owner(index))).await;
    }
}

/// L'index plein texte, ouvert et comparé à la base : rouvert s'il a été lâché, puis ses jetons
/// comparés ([`reconcile`]) si ce n'est pas fait depuis son ouverture.
async fn ready_index(shared: &Shared) -> anyhow::Result<Arc<SearchIndex>> {
    // Sans tenir l'index d'ici là : une reconstruction doit en être seule propriétaire.
    let (lacking, dir) = {
        let text = shared.text();
        (text.index.is_none(), text.dir.clone())
    };
    if lacking {
        let dir = dir.context("aucun index de recherche n'est ouvert")?;
        let opened = blocking(move || SearchIndex::open(&dir)).await?;
        let mut text = shared.text();
        text.index = Some(Arc::new(opened.index));
        text.token = opened.token;
        text.reconciled = false;
    }
    if !shared.text().reconciled {
        reconcile(shared).await?;
    }
    shared
        .text()
        .index
        .clone()
        .context("aucun index de recherche n'est ouvert")
}

/// Compare le jeton de l'index plein texte aux deux jetons de la base (Ruling R909 ; plan du lot,
/// « L'intention, la validation, la clôture ») :
/// - celui de la dernière validation close : en ordre. Une intention restée posée n'a jamais été
///   validée ; le passage suivant la défait ([`feed`]) ;
/// - celui de l'intention posée : un arrêt entre la validation et la clôture, ou une copie de la
///   base prise entre les deux puis remise en place. Le passage suivant **rejoue** ses lignes
///   depuis la base, et rien n'est cherché d'ici là ;
/// - tout autre jeton, ou aucun (un index neuf, ou que [`SearchIndex::open`] a vidé) : la base et
///   l'index ne vont pas ensemble. L'index est vidé, l'intention posée défaite (Ruling R978),
///   et toutes les pages passent à l'outbox, par tranches ; aucun passage ne commence avant la
///   dernière tranche, si bien qu'un arrêt en route laisse un index sans jeton, que le démarrage
///   suivant reconstruit de même.
async fn reconcile(shared: &Shared) -> anyhow::Result<()> {
    let tokens = shared.store.feed_tokens().await?;
    let token = shared.text().token.clone();
    if token.is_some() && token == tokens.committed {
        shared.searchable.store(true, Ordering::SeqCst);
    } else if token.is_some() && token == tokens.intent {
        // Validée, pas close : rejouée par le prochain passage, qui rendra l'index cherchable.
    } else {
        // Une instance neuve (ni jeton dans l'index, ni validation close) passe par ici aussi,
        // sans le dire : ses pages d'avant ce lot n'ont pas de ligne de page entière.
        if token.is_some() || tokens.committed.is_some() {
            shared.rebuilds.fetch_add(1, Ordering::SeqCst);
            tracing::info!(
                "l'index de recherche n'est pas celui de cette base (base restaurée, dossier \
                 d'une autre instance, index perdu ou d'une autre forme) : il est reconstruit, \
                 en fond"
            );
        }
        rebuild(shared, token.is_some()).await?;
    }
    shared.text().reconciled = true;
    Ok(())
}

/// Reconstruit l'index plein texte : vidé (`wipe` : il porte des entrées), l'intention posée
/// défaite, puis toutes les pages à l'outbox. Les passages suivants le remplissent.
async fn rebuild(shared: &Shared, wipe: bool) -> anyhow::Result<()> {
    shared.rebuilding.store(true, Ordering::SeqCst);
    if wipe {
        shared.searchable.store(false, Ordering::SeqCst);
        let (held, dir) = {
            let mut text = shared.text();
            text.token = None;
            (text.index.take(), text.dir.clone())
        };
        let dir = dir.context("aucun index de recherche n'est ouvert")?;
        let opened = blocking(move || {
            if let Some(index) = held {
                // Fermé avant d'être vidé : un fichier qu'une fusion tient ne se supprime pas
                // sous Windows.
                sole_owner(index).ok_or(IndexError::Busy)?.close()?;
            }
            SearchIndex::reset(&dir)
        })
        .await?;
        let mut text = shared.text();
        text.index = Some(Arc::new(opened.index));
        text.token = None;
    }
    // Un index vide ne montre rien que la base n'a pas.
    shared.searchable.store(true, Ordering::SeqCst);
    shared.store.undo_feed().await?;
    let mut after = None;
    loop {
        anyhow::ensure!(!shared.stopping(), "arrêt pendant la reconstruction");
        match shared.store.outbox_everything(after, BATCH).await? {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    shared.rebuilding.store(false, Ordering::SeqCst);
    Ok(())
}

/// Passe des lignes de l'outbox à l'index plein texte, sans valider : chaque entrée nommée est
/// retirée, puis récrite d'après `blocks_index` et `nodes` si elle existe encore. Rend le nombre
/// d'entrées écrites.
///
/// Une ligne de page entière récrit **toutes** les entrées de la page, son titre étant sur
/// chacune (Ruling R902) ; une page supprimée, ou dont la lecture a échoué, n'en garde aucune
/// ([`Store::indexed_title`]). Une ligne de bloc ne récrit que son entrée (Ruling R919). Un bloc
/// au texte vide n'a pas d'entrée. Les clés et les textes viennent de `blocks_index`, donc de la
/// projection, qui les borne : rien d'autre n'est donné à [`SearchIndex::put_entry`].
///
/// `lines` est rangé par page, la ligne de page entière en tête ([`Store::begin_feed`]). Une page
/// à la fois : ses lignes lues, écrites sur le fil bloquant, puis lâchées.
pub(crate) async fn write_lines(
    store: &Store,
    index: &Arc<SearchIndex>,
    lines: &[FeedLine],
) -> anyhow::Result<u64> {
    let mut written = 0;
    let mut at = 0;
    while at < lines.len() {
        let node = lines[at].node_id;
        let end = at + lines[at..].iter().take_while(|l| l.node_id == node).count();
        let page = &lines[at..end];
        at = end;
        let title = store.indexed_title(node).await?;
        let whole = page.iter().any(|line| line.block_id.is_none());
        let mut rows: Vec<IndexRow> = Vec::new();
        let mut blocks: Vec<String> = Vec::new();
        if whole {
            if title.is_some() {
                rows = store.indexed_blocks(node).await?;
            }
        } else {
            for block in page.iter().filter_map(|line| line.block_id.as_ref()) {
                if title.is_some()
                    && let Some(row) = store.indexed_block(node, block).await?
                {
                    rows.push(row);
                }
                blocks.push(block.clone());
            }
        }
        let index = index.clone();
        written += blocking(move || {
            if whole {
                index.remove_page(node)?;
            }
            for block in &blocks {
                index.remove_entry(node, Some(block))?;
            }
            let Some(title) = title else {
                return Ok(0);
            };
            let mut written = 0;
            if whole {
                index.put_entry(node, None, &title, "")?;
                written += 1;
            }
            for row in rows.iter().filter(|row| !row.text.is_empty()) {
                index.put_entry(node, Some(&row.block_id), &title, &row.text)?;
                written += 1;
            }
            Ok(written)
        })
        .await?;
    }
    Ok(written)
}

/// Un passage de l'outbox à l'index plein texte, dans une tâche à elle ([`run`]).
async fn feed_once(shared: Arc<Shared>) -> Fed {
    match feed(&shared).await {
        Ok(fed) => fed,
        Err(error) => {
            tracing::error!(
                error = %format!("{error:#}"),
                "index de recherche : passage refusé, nouvel essai plus tard ; la recherche \
                 répond sur ce qui est déjà validé"
            );
            Fed::Retry
        }
    }
}

/// L'intention, la validation, la clôture (Ruling R909).
async fn feed(shared: &Shared) -> anyhow::Result<Fed> {
    let index = ready_index(shared).await?;
    let started = std::time::Instant::now();
    let limit = shared.feed_lines.load(Ordering::SeqCst);
    let feed = match shared.store.begin_feed(limit).await? {
        FeedStart::Nothing => return Ok(Fed::Nothing),
        FeedStart::Begun(feed) => {
            shared.intents.fetch_add(1, Ordering::SeqCst);
            feed
        }
        // Une intention est posée. L'index l'a validée (une clôture refusée, un arrêt entre la
        // validation et la clôture) : ses lignes sont rejouées depuis la base, jamais tenues pour
        // faites. Sinon elle n'a jamais été validée : elle est défaite.
        FeedStart::Pending(token) => {
            if shared.text().token.as_deref() != Some(token.as_str()) {
                shared.store.undo_feed().await?;
                return Ok(Fed::Again);
            }
            match shared.store.intent_lines().await? {
                Some(feed) if feed.token == token => feed,
                _ => return Ok(Fed::Again),
            }
        }
    };
    let written = match write_lines(&shared.store, &index, &feed.lines).await {
        Ok(written) => written,
        Err(error) => {
            // Des écritures pas validées restent dans l'écrivain : l'index ne se réutilise pas.
            drop(index);
            release(shared).await;
            return Err(error.context("écriture des entrées"));
        }
    };
    shared.puts.fetch_add(written, Ordering::SeqCst);
    if shared.halts_at(HALT_BEFORE_COMMIT) {
        return Ok(Fed::Halted);
    }
    let token = feed.token.clone();
    let committed = if take_refusal(&shared.refuse_commits) {
        drop(index);
        Err(anyhow::anyhow!("validation refusée à la demande d'un test"))
    } else {
        blocking(move || index.commit(&token)).await
    };
    if let Err(error) = committed {
        // Rien n'est dit de ce que le disque porte (Ruling R992) : l'index est lâché, et c'est le
        // jeton relu à sa réouverture qui dira si la validation a eu lieu (elle est alors
        // rejouée) ou non (l'intention est défaite).
        shared.commit_refusals.fetch_add(1, Ordering::SeqCst);
        release(shared).await;
        return Err(error.context("validation de l'index"));
    }
    shared.text().token = Some(feed.token.clone());
    if shared.halts_at(HALT_BEFORE_CLOSE) {
        return Ok(Fed::Halted);
    }
    let closed = if take_refusal(&shared.refuse_closes) {
        Err(anyhow::anyhow!("clôture refusée à la demande d'un test"))
    } else {
        match shared.store.finish_feed(&feed.token).await {
            // `Stale` : l'intention n'est plus celle-ci (défaite par une reconstruction). Ses
            // lignes sont restées dans l'outbox.
            Ok(()) | Err(StoreError::Stale) => Ok(()),
            Err(error) => Err(anyhow::Error::new(error)),
        }
    };
    if let Err(error) = closed {
        // L'index a validé, la base tient encore l'intention : aucune autre ne commencera, et le
        // passage suivant la rejoue puis tente de clore.
        shared.close_refusals.fetch_add(1, Ordering::SeqCst);
        return Err(error.context("clôture de l'intention"));
    }
    shared.searchable.store(true, Ordering::SeqCst);
    *shared
        .last_feed
        .lock()
        .unwrap_or_else(PoisonError::into_inner) =
        Some((feed.lines.len(), written, started.elapsed()));
    Ok(Fed::Again)
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
    // L'index plein texte comparé à la base (Ruling R909), et reconstruit s'il n'est pas le sien :
    // avant tout passage, et avant de dire quoi que ce soit à jour.
    let mut delay = RETRY_FIRST;
    while !shared.stopping() {
        match ready_index(&shared).await {
            Ok(_) => break,
            Err(_) if shared.stopping() => break,
            Err(error) => {
                tracing::error!(
                    error = %format!("{error:#}"),
                    "index de recherche impossible à comparer à la base ou à reconstruire : \
                     nouvel essai plus tard"
                );
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
    // Le passage de l'outbox à l'index plein texte en cours : un travail de plus de la boucle,
    // jamais mené en même temps qu'une indexation.
    let mut feeding: Option<JoinHandle<Fed>> = None;
    // Des lignes d'outbox attendent peut-être : au démarrage, puis après chaque indexation,
    // chaque annonce de la base (un titre, une création, une suppression) et chaque demande de
    // `settled`. `feed_asks` compte ces demandes : un passage qui ne trouve rien ne baisse le
    // drapeau que si rien n'a été demandé depuis son départ.
    let mut feed_wanted = true;
    let mut feed_asks = 0u64;
    let mut feed_asks_at_start = 0u64;
    // Depuis quand un passage est voulu, et quand le réessayer après un refus.
    let mut feed_since: Option<Instant> = None;
    let mut feed_retry: Option<(Instant, Duration)> = None;
    let mut round = 0u64;
    while !shared.stopping() {
        let now = Instant::now();
        let busy = job.is_some() || feeding.is_some();
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
                Ok(_) | Err(TryRecvError::Lagged(_)) => {
                    markers = true;
                    feed_wanted = true;
                    feed_asks += 1;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Closed) => announced = false,
            }
        }
        if shared.asked.swap(false, Ordering::SeqCst) {
            markers = true;
            sources_after = None;
            feed_wanted = true;
            feed_asks += 1;
            // Qui attend l'indexeur n'attend pas le délai d'un nouvel essai.
            feed_retry = feed_retry.map(|(_, delay)| (now, delay));
        }
        if sources_after.is_some_and(|at| at <= now) {
            sources_after = None;
        }
        // Les sources durables, quand il n'y a rien de prêt : jamais plus d'un lot en mémoire.
        let nothing_ready = !busy && shared.queues().ready.is_empty();
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
        let nothing_ready = !busy && sources_after.is_none() && shared.queues().ready.is_empty();
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
        // Rien n'est commencé une fois l'arrêt demandé. Un travail à la fois : une indexation, ou
        // un passage de l'outbox à l'index plein texte. Le passage attend qu'aucune page ne soit
        // à indexer, [`FEED_AT_MOST`] au plus depuis qu'il est voulu (Ruling R1003).
        if !busy && !shared.stopping() && !shared.frozen.load(Ordering::SeqCst) {
            let now = Instant::now();
            let feed_due = feed_wanted && feed_retry.is_none_or(|(at, _)| at <= now);
            let overdue = feed_due && now >= *feed_since.get_or_insert(now) + FEED_AT_MOST;
            let next = if overdue {
                None
            } else {
                shared.queues().next(now, &shared.cfg)
            };
            // Compté avant que le tour soit publié : qui lit le tour sait ce qu'il a commencé.
            if let Some(page) = next {
                shared.started.fetch_add(1, Ordering::SeqCst);
                job = Some((tokio::spawn(index_one(shared.clone(), page)), false));
            } else if feed_due {
                shared.started.fetch_add(1, Ordering::SeqCst);
                feed_asks_at_start = feed_asks;
                feeding = Some(tokio::spawn(feed_once(shared.clone())));
            }
        }
        let busy = job.is_some() || feeding.is_some();
        round += 1;
        let idle =
            !busy && !feed_wanted && !markers && catch_up.is_none() && shared.queues().is_empty();
        shared.rounds.send_replace(Round {
            n: round,
            idle,
            stopped: false,
        });
        // Un lot de plus à lire (le rattrapage d'une instance où rien n'est en retard dans ce
        // lot) : pas d'attente.
        let more = !busy && sources_after.is_none() && (markers || catch_up.is_some());
        if more {
            tokio::task::yield_now().await;
            continue;
        }
        // Figé, rien de dû n'est commencé : seul un événement réveille la boucle.
        let wake_at = if busy || shared.frozen.load(Ordering::SeqCst) {
            None
        } else {
            let due = shared.queues().next_wake(&shared.cfg);
            let retry = feed_retry.filter(|_| feed_wanted).map(|(at, _)| at);
            [due, sources_after, retry].into_iter().flatten().min()
        };
        tokio::select! {
            () = shared.wake.notified() => {}
            () = shared.hub.touched_changed() => {}
            change = changes.recv(), if announced => match change {
                Ok(_) | Err(RecvError::Lagged(_)) => {
                    markers = true;
                    feed_wanted = true;
                    feed_asks += 1;
                }
                Err(RecvError::Closed) => announced = false,
            },
            done = async { (&mut job.as_mut().expect("un travail en cours").0).await }, if job.is_some() => {
                let giving_up = job.take().is_some_and(|(_, giving_up)| giving_up);
                job = finished(&shared, done, giving_up).map(|job| (job, true));
                // Ce qu'elle a écrit a laissé ses lignes d'outbox.
                feed_wanted = true;
                feed_asks += 1;
            }
            fed = async { feeding.as_mut().expect("un passage en cours").await }, if feeding.is_some() => {
                feeding = None;
                match fed {
                    Ok(Fed::Nothing) => {
                        feed_wanted = feed_asks != feed_asks_at_start;
                        feed_since = None;
                        feed_retry = None;
                    }
                    // Il peut rester des lignes : un autre passage, sans attendre.
                    Ok(Fed::Again) => {
                        feed_since = None;
                        feed_retry = None;
                    }
                    // Comme un processus tué là : plus rien n'est fait.
                    Ok(Fed::Halted) => shared.stopping.store(true, Ordering::SeqCst),
                    // Refusé, ou le passage a paniqué : l'intention est restée telle quelle, et
                    // aucune autre ne commence. 1 s, puis le double, 60 s au plus.
                    Ok(Fed::Retry) | Err(_) => {
                        if let Err(error) = &fed {
                            tracing::error!(%error, "index de recherche : un passage a paniqué, nouvel essai plus tard");
                        }
                        let delay = feed_retry
                            .map_or(RETRY_FIRST, |(_, delay)| (delay * 2).min(RETRY_AT_MOST));
                        feed_retry = Some((Instant::now() + delay, delay));
                    }
                }
            }
            () = async { tokio::time::sleep_until(wake_at.expect("un réveil")).await }, if wake_at.is_some() => {}
        }
    }
    // Le passage en cours va au bout de même : sa validation est close. Coupé par le délai, il
    // laisse une intention posée, que le démarrage suivant rejoue ou défait.
    if let Some(feeding) = feeding
        && tokio::time::timeout(STOP_WAIT, feeding).await.is_err()
    {
        tracing::warn!(
            "arrêt : la validation en cours de l'index de recherche n'a pas fini à temps, elle \
             sera reprise au démarrage suivant"
        );
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
    // L'index plein texte est lâché avant que l'arrêt soit publié : son dossier n'est plus tenu
    // (le verrou de son écrivain), et une application relancée dans ce processus peut l'ouvrir.
    let_go(&shared).await;
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

    /// Une base, l'index plein texte de son dossier, et une poignée sans tâche de fond : les
    /// passages de l'outbox à l'index s'y mènent un à un, à la main.
    struct Bench {
        dir: tempfile::TempDir,
        store: Store,
        handle: IndexerHandle,
        space: Uuid,
    }

    impl Bench {
        async fn new() -> Bench {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(&dir.path().join("m.db")).await.unwrap();
            let space = store.create_space("Espace", None).await.unwrap().id;
            let hub = Hub::new(store.clone(), SyncConfig::default());
            let text = open_text(dir.path()).await.unwrap();
            let handle = with_text(store.clone(), hub, config(), Some(text));
            Bench {
                dir,
                store,
                handle,
                space,
            }
        }

        /// Une page à ce titre, dont la projection porte un bloc à ce texte.
        async fn page(&self, title: &str, text: &str) -> Uuid {
            let page = self
                .store
                .create_node(NewNode {
                    space_id: self.space,
                    parent_id: None,
                    after: None,
                    kind: NodeKind::Page,
                    title: title.to_string(),
                    created_by: None,
                    grants: Vec::new(),
                    doc: None,
                })
                .await
                .unwrap()
                .id;
            self.set_text(page, 1, text).await;
            page
        }

        /// La projection de la page, au `seq` donné : un bloc à ce texte.
        async fn set_text(&self, page: Uuid, seq: i64, text: &str) {
            let row = IndexedBlock {
                block_id: "bloc",
                kind: "paragraph",
                text,
                hash: text.as_bytes(),
                links: &[],
            };
            self.store
                .index_page(page, seq, &[row], false)
                .await
                .unwrap();
        }

        /// Un passage.
        async fn feed(&self) -> Fed {
            feed(&self.handle.shared).await.unwrap()
        }

        /// Des passages, jusqu'à ce que plus rien n'attende.
        async fn drain(&self) {
            for _ in 0..100 {
                if self.feed().await == Fed::Nothing {
                    return;
                }
            }
            panic!("l'outbox ne se vide pas");
        }

        fn found(&self, words: &str, pages: &[Uuid]) -> Vec<Uuid> {
            let query = mementum_search::read_query(words).unwrap();
            let allowed: PageSet = pages.iter().copied().collect();
            let found = self.handle.search(&query, &allowed, 0, 20).unwrap();
            let mut pages: Vec<Uuid> = found.pages.into_iter().map(|hit| hit.node_id).collect();
            pages.sort();
            pages
        }

        async fn behind(&self, page: Uuid) -> bool {
            self.handle
                .behind(self.space, |id| id == page)
                .await
                .unwrap()
        }

        /// Ferme l'index (ses fusions finies : son dossier peut être retiré) et la base.
        async fn close(self) -> tempfile::TempDir {
            let index = self.handle.shared.text().index.take();
            if let Some(index) = index {
                Arc::try_unwrap(index).ok().unwrap().close().unwrap();
            }
            self.store.close().await;
            self.dir
        }
    }

    /// Une ligne qui change entre la validation et la clôture (son bloc est récrit) perd son
    /// jeton : la clôture ne la retire pas, et elle est passée de nouveau à l'index plein texte,
    /// par l'intention suivante. Le passage est arrêté entre la validation et la clôture, le
    /// bloc récrit là, puis le passage repris : l'intention est rejouée et close, la ligne reste.
    #[tokio::test]
    async fn a_line_changed_during_a_commit_is_fed_again() {
        let b = Bench::new().await;
        let page = b.page("Page", "premier texte").await;
        b.handle.halt_before_the_close();
        assert_eq!(b.feed().await, Fed::Halted);
        let posed = b.store.feed_tokens().await.unwrap();
        assert!(posed.intent.is_some(), "validée, pas close");

        b.set_text(page, 2, "second texte").await;
        b.handle.shared.halt.store(0, Ordering::SeqCst);
        assert_eq!(
            b.feed().await,
            Fed::Again,
            "l'intention rejouée, puis close"
        );
        let closed = b.store.feed_tokens().await.unwrap();
        assert_eq!((closed.intent, closed.committed), (None, posed.intent));
        assert_eq!(b.handle.intents(), 1);
        assert!(b.behind(page).await, "la ligne changée n'est pas close");

        b.drain().await;
        assert_eq!(b.handle.intents(), 2, "une seconde intention la prend");
        assert_eq!(b.found("second", &[page]), [page]);
        assert!(b.found("premier", &[page]).is_empty());
        assert!(!b.behind(page).await);
        b.close().await;
    }

    /// Pendant une reconstruction, la recherche ne rend que ce qui est déjà reconstruit, et le
    /// reste est dit en retard. L'index perd son dossier (comme une sauvegarde remise en place
    /// sans `search/`) : son jeton n'est plus celui de la base, tout passe à l'outbox, et les
    /// passages, d'une ligne ici, le remplissent page après page.
    #[tokio::test]
    async fn searching_during_a_rebuild_finds_only_what_is_rebuilt() {
        let b = Bench::new().await;
        let mut pages = [
            b.page("Première", "un texte commun").await,
            b.page("Seconde", "un texte commun").await,
        ];
        pages.sort();
        let [first, second] = pages;
        b.drain().await;
        assert_eq!(b.found("commun", &pages), pages, "témoin");
        assert_eq!(b.handle.rebuilds(), 0);
        assert!(!b.behind(second).await, "témoin");

        // L'index est fermé, son dossier retiré : il sera rouvert neuf, sans jeton.
        let index = {
            let mut text = b.handle.shared.text();
            text.token = None;
            text.reconciled = false;
            text.index.take().unwrap()
        };
        Arc::try_unwrap(index).ok().unwrap().close().unwrap();
        std::fs::remove_dir_all(b.dir.path().join(SEARCH_DIR)).unwrap();
        b.handle.shared.searchable.store(false, Ordering::SeqCst);
        assert!(b.found("commun", &pages).is_empty());
        assert!(b.behind(first).await, "rien n'est sûr avant la comparaison");

        b.handle.shared.feed_lines.store(1, Ordering::SeqCst);
        assert_eq!(b.feed().await, Fed::Again);
        assert_eq!(b.handle.rebuilds(), 1);
        assert_eq!(
            b.found("commun", &pages),
            [first],
            "seule la page déjà reconstruite"
        );
        assert!(!b.behind(first).await);
        assert!(b.behind(second).await, "le reste est dit en retard");

        b.drain().await;
        assert_eq!(b.found("commun", &pages), pages);
        assert!(!b.behind(second).await);
        b.close().await;
    }
}
