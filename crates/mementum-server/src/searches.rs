//! Le débit des recherches, par compte, en mémoire (lot 4, Task S5, Rulings R910, R932).
//!
//! Une recherche est un calcul sur le fil bloquant (bâtir l'ensemble des pages permises, lire
//! l'index). Deux places la bornent :
//!
//! - **la place du compte** ([`Seat`]) : un compte n'a qu'une recherche admise à la fois, et une
//!   seule qui attend. Une recherche neuve prend la place de celle qui attendait, qui est refusée
//!   ([`Busy`], `429 search_busy`) : quand on tape, seule la dernière recherche compte. La place
//!   est tenue **jusqu'au départ de la réponse**, plancher de temps compris, pas jusqu'à la fin du
//!   calcul : rendue plus tôt, l'heure où la recherche suivante du compte commence dirait la durée
//!   du calcul d'avant (Ruling R932) ;
//! - **la place de l'instance** ([`Turn`]) : [`MAX_SEARCHES`] recherches de front au plus. Elle
//!   est tenue par le calcul jusqu'à sa fin, même si la requête est partie entre-temps (on
//!   n'arrête pas un fil bloquant), et par la recherche jusqu'au départ de sa réponse, plancher
//!   compris (Ruling R1054) : rendue à la fin du calcul, elle disait sa durée au compte qui
//!   attendait derrière.
//!
//! Les comptes qui attendent une place de l'instance passent **tour à tour**, dans l'ordre où ils
//! sont devenus prêts : un compte dont une recherche est admise n'est pas prêt, si bien que la
//! seconde recherche d'un compte qui en lance en rafale passe après celle de tout compte qui
//! attendait déjà. Une recherche abandonnée pendant qu'elle attend sort de la file sans rien
//! calculer.
//!
//! Tout est en mémoire : un compte et sa file disparaissent dès qu'il n'a plus ni recherche admise
//! ni recherche en attente.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;
use uuid::Uuid;

/// Calculs de recherche menés de front, pour toute l'instance (Ruling R910).
pub const MAX_SEARCHES: usize = 4;

/// Ce qu'un test fait faire au début de chaque calcul, sur le fil du calcul.
#[cfg(any(test, feature = "test-hooks"))]
type Hook = Arc<dyn Fn(Uuid) + Send + Sync>;

/// Le débit des recherches (voir le module). Se clone : toutes les copies partagent la file.
#[derive(Clone, Default)]
pub struct Searches {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    /// Compteurs, pour les tests : calculs commencés, ensembles de pages permises bâtis, calculs
    /// finis.
    started: AtomicU64,
    built: AtomicU64,
    computed: AtomicU64,
    /// Réponses prêtes, qui n'attendent plus que leur plancher, pour les tests.
    #[cfg(any(test, feature = "test-hooks"))]
    ready: AtomicU64,
    #[cfg(any(test, feature = "test-hooks"))]
    hook: Mutex<Option<Hook>>,
}

impl Shared {
    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Default)]
struct Queue {
    /// Places de l'instance prises.
    running: usize,
    accounts: HashMap<Uuid, Account>,
    /// Les comptes prêts (une recherche en attente, aucune admise), dans l'ordre où leur tour
    /// viendra.
    turns: VecDeque<Uuid>,
    next: u64,
}

#[derive(Default)]
struct Account {
    /// Une recherche du compte est admise : elle calcule, ou attend le départ de sa réponse.
    seated: bool,
    /// La recherche du compte qui attend : une au plus.
    waiting: Option<Waiter>,
    /// Le compte est dans `turns`.
    queued: bool,
}

struct Waiter {
    id: u64,
    wake: oneshot::Sender<()>,
}

impl Queue {
    /// Donne les places libres de l'instance aux comptes prêts, dans l'ordre de la ronde.
    fn give(&mut self) {
        while self.running < MAX_SEARCHES {
            let Some(account) = self.turns.pop_front() else {
                return;
            };
            let Some(entry) = self.accounts.get_mut(&account) else {
                continue;
            };
            entry.queued = false;
            if entry.seated {
                // Remis dans la ronde quand sa place de compte sera rendue.
                continue;
            }
            let Some(waiter) = entry.waiting.take() else {
                self.accounts.remove(&account);
                continue;
            };
            entry.seated = true;
            self.running += 1;
            // Si la requête est partie entre-temps, c'est sa garde qui rend les deux places
            // ([`Waiting`]).
            let _ = waiter.wake.send(());
        }
    }

    /// Le compte entre dans la ronde s'il est prêt, ou disparaît s'il n'a plus rien.
    fn settle(&mut self, account: Uuid) {
        let Some(entry) = self.accounts.get_mut(&account) else {
            return;
        };
        if entry.seated {
            return;
        }
        if entry.waiting.is_none() {
            self.accounts.remove(&account);
            self.turns.retain(|a| *a != account);
        } else if !entry.queued {
            entry.queued = true;
            self.turns.push_back(account);
        }
    }

    /// La place du compte est rendue.
    fn unseat(&mut self, account: Uuid) {
        if let Some(entry) = self.accounts.get_mut(&account) {
            entry.seated = false;
        }
        self.settle(account);
        self.give();
    }

    /// Une place de l'instance est rendue.
    fn release(&mut self) {
        self.running -= 1;
        self.give();
    }
}

/// La place d'un compte : la lâcher laisse passer sa recherche suivante.
pub(crate) struct Seat {
    shared: Arc<Shared>,
    account: Uuid,
}

impl Drop for Seat {
    fn drop(&mut self) {
        self.shared.queue().unseat(self.account);
    }
}

/// Une place de calcul de l'instance : la lâcher la donne au compte suivant de la ronde.
pub(crate) struct Turn {
    shared: Arc<Shared>,
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.shared.queue().release();
    }
}

/// Une place de l'instance tenue par un test ([`Searches::hold`]).
#[cfg(any(test, feature = "test-hooks"))]
#[doc(hidden)]
pub struct Held(#[allow(dead_code)] Turn);

/// Une recherche plus récente du compte a pris la place de celle-ci, qui attendait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Busy;

/// Une recherche qui attend ses places. Lâchée avant de les avoir (la requête est abandonnée),
/// elle sort de la file ; si les places venaient de lui être données, elle les rend.
struct Waiting {
    shared: Arc<Shared>,
    account: Uuid,
    id: u64,
    woken: oneshot::Receiver<()>,
    done: bool,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let mut queue = self.shared.queue();
        // Les places ne se donnent que sous ce verrou : ce qui est lu ici ne change plus.
        match self.woken.try_recv() {
            Ok(()) => {
                queue.running -= 1;
                queue.unseat(self.account);
            }
            // Une recherche plus récente a pris sa place : elle n'a plus rien dans la file.
            Err(TryRecvError::Closed) => {}
            Err(TryRecvError::Empty) => {
                if let Some(entry) = queue.accounts.get_mut(&self.account)
                    && entry.waiting.as_ref().is_some_and(|w| w.id == self.id)
                {
                    entry.waiting = None;
                    if !entry.seated {
                        entry.queued = false;
                        queue.accounts.remove(&self.account);
                        let account = self.account;
                        queue.turns.retain(|a| *a != account);
                    }
                }
            }
        }
    }
}

impl Searches {
    /// Les deux places d'une recherche de `account` : tout de suite si le compte n'a rien en
    /// cours et qu'une place de l'instance est libre sans que personne l'attende ; sinon à son
    /// tour. [`Busy`] si une recherche plus récente du compte arrive pendant l'attente.
    pub(crate) async fn enter(&self, account: Uuid) -> Result<(Seat, Turn), Busy> {
        let places = || {
            (
                Seat {
                    shared: self.shared.clone(),
                    account,
                },
                Turn {
                    shared: self.shared.clone(),
                },
            )
        };
        let mut waiting = {
            let mut queue = self.shared.queue();
            let free = queue.running < MAX_SEARCHES && queue.turns.is_empty();
            let entry = queue.accounts.entry(account).or_default();
            if free && !entry.seated && entry.waiting.is_none() {
                entry.seated = true;
                queue.running += 1;
                return Ok(places());
            }
            let (wake, woken) = oneshot::channel();
            let id = queue.next;
            queue.next += 1;
            // L'émetteur de la recherche qui attendait est lâché ici : elle est refusée.
            let entry = queue.accounts.entry(account).or_default();
            entry.waiting = Some(Waiter { id, wake });
            queue.settle(account);
            queue.give();
            Waiting {
                shared: self.shared.clone(),
                account,
                id,
                woken,
                done: false,
            }
        };
        let woken = (&mut waiting.woken).await;
        waiting.done = true;
        match woken {
            Ok(()) => Ok(places()),
            Err(_) => Err(Busy),
        }
    }

    /// Un calcul commence, sur son fil : compté, puis ce qu'un test y fait faire.
    pub(crate) fn computing(&self, account: Uuid) {
        self.shared.started.fetch_add(1, Ordering::SeqCst);
        #[cfg(any(test, feature = "test-hooks"))]
        {
            let hook = self
                .shared
                .hook
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let Some(hook) = hook {
                hook(account);
            }
        }
        #[cfg(not(any(test, feature = "test-hooks")))]
        let _ = account;
    }

    /// Un ensemble de pages permises vient d'être bâti.
    pub(crate) fn built(&self) {
        self.shared.built.fetch_add(1, Ordering::SeqCst);
    }

    /// Un calcul vient de finir.
    pub(crate) fn finished(&self) {
        self.shared.computed.fetch_add(1, Ordering::SeqCst);
    }

    /// Une réponse est prête (calcul, titres et retard lus) : il ne lui reste qu'à attendre son
    /// plancher. Compté pour les tests seulement.
    pub(crate) fn answer_ready(&self) {
        #[cfg(any(test, feature = "test-hooks"))]
        self.shared.ready.fetch_add(1, Ordering::SeqCst);
    }

    /// Réponses prêtes depuis le lancement, parties ou en attente de leur plancher. Pour les
    /// tests : la barrière d'un « elle n'est pas partie avant le plancher ».
    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn answers_ready(&self) -> u64 {
        self.shared.ready.load(Ordering::SeqCst)
    }

    /// Calculs commencés depuis le lancement. Pour les tests.
    #[doc(hidden)]
    pub fn started(&self) -> u64 {
        self.shared.started.load(Ordering::SeqCst)
    }

    /// Ensembles de pages permises bâtis depuis le lancement. Pour les tests.
    #[doc(hidden)]
    pub fn sets_built(&self) -> u64 {
        self.shared.built.load(Ordering::SeqCst)
    }

    /// Calculs finis depuis le lancement (leur réponse attend peut-être son plancher). Pour les
    /// tests.
    #[doc(hidden)]
    pub fn computed(&self) -> u64 {
        self.shared.computed.load(Ordering::SeqCst)
    }

    /// Places de l'instance prises, et recherches qui attendent. Pour les tests.
    #[doc(hidden)]
    pub fn counts(&self) -> (usize, usize) {
        let queue = self.shared.queue();
        (
            queue.running,
            queue
                .accounts
                .values()
                .filter(|account| account.waiting.is_some())
                .count(),
        )
    }

    /// Prend une place de l'instance, sans compte ni calcul, jusqu'à ce que la garde soit
    /// lâchée. Pour les tests ; il doit en rester une libre.
    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn hold(&self) -> Held {
        let mut queue = self.shared.queue();
        assert!(queue.running < MAX_SEARCHES, "aucune place libre à tenir");
        queue.running += 1;
        Held(Turn {
            shared: self.shared.clone(),
        })
    }

    /// Ce que chaque calcul fera d'abord, sur son fil, avec le compte qui cherche : y attendre un
    /// signal du test, noter l'ordre de service. Pour les tests.
    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn before_each_computation(&self, hook: impl Fn(Uuid) + Send + Sync + 'static) {
        *self
            .shared
            .hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(hook));
    }
}

#[cfg(test)]
mod tests {
    use tokio::task::JoinHandle;

    use super::*;

    type Entered = JoinHandle<Result<(Seat, Turn), Busy>>;

    /// Une recherche de `account` qui demande ses places, dans une tâche.
    fn enter(searches: &Searches, account: Uuid) -> Entered {
        let searches = searches.clone();
        tokio::spawn(async move { searches.enter(account).await })
    }

    /// Cède la main assez pour que toute tâche prête ait tourné.
    async fn settle() {
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
    }

    /// Un compte a une recherche admise et une qui attend, au plus : la troisième prend la place
    /// de la deuxième, qui est refusée, et attend à son tour.
    #[tokio::test]
    async fn a_newer_search_takes_the_place_of_the_one_waiting() {
        let searches = Searches::default();
        let ana = Uuid::now_v7();
        let first = searches.enter(ana).await.unwrap();
        let second = enter(&searches, ana);
        settle().await;
        assert!(!second.is_finished(), "la deuxième attend");
        assert_eq!(searches.counts(), (1, 1));
        let third = enter(&searches, ana);
        settle().await;
        assert_eq!(second.await.unwrap().err(), Some(Busy));
        assert!(!third.is_finished(), "la troisième attend à sa place");
        assert_eq!(searches.counts(), (1, 1));
        drop(first);
        assert!(third.await.unwrap().is_ok());
    }

    /// Ruling R932 : la recherche suivante du compte ne passe pas à la fin du calcul de la
    /// première (sa place de l'instance rendue), mais au départ de sa réponse (sa place de compte
    /// rendue).
    #[tokio::test]
    async fn the_next_search_of_an_account_waits_for_its_seat_not_for_a_turn() {
        let searches = Searches::default();
        let ana = Uuid::now_v7();
        let (seat, turn) = searches.enter(ana).await.unwrap();
        let second = enter(&searches, ana);
        settle().await;
        drop(turn);
        settle().await;
        assert!(!second.is_finished(), "le calcul fini ne suffit pas");
        assert_eq!(searches.counts(), (0, 1));
        drop(seat);
        let places = second.await.unwrap();
        assert!(places.is_ok());
        assert_eq!(searches.counts(), (1, 0));
    }

    /// Quatre calculs de front, pas un de plus ; les comptes qui attendent passent dans l'ordre
    /// où ils sont arrivés, un par place rendue.
    #[tokio::test]
    async fn four_computations_at_once_and_accounts_in_turn() {
        let searches = Searches::default();
        let mut running = Vec::new();
        for _ in 0..MAX_SEARCHES {
            running.push(searches.enter(Uuid::now_v7()).await.unwrap());
        }
        let fifth = enter(&searches, Uuid::now_v7());
        settle().await;
        let sixth = enter(&searches, Uuid::now_v7());
        settle().await;
        assert!(!fifth.is_finished() && !sixth.is_finished());
        assert_eq!(searches.counts(), (MAX_SEARCHES, 2));
        // Le calcul du premier finit : sa place de l'instance suffit au cinquième.
        let (seat, turn) = running.remove(0);
        drop(turn);
        settle().await;
        assert!(fifth.is_finished(), "le cinquième passe");
        assert!(!sixth.is_finished(), "pas le sixième");
        assert_eq!(searches.counts(), (MAX_SEARCHES, 1));
        drop(seat);
        settle().await;
        assert!(
            !sixth.is_finished(),
            "une place de compte n'est pas un tour"
        );
        drop(running.remove(0));
        assert!(sixth.await.unwrap().is_ok());
        assert_eq!(MAX_SEARCHES, 4);
    }

    /// Une rafale d'un compte ne passe pas devant un autre : une seule place de l'instance est
    /// libre, Ana la prend et relance ; Ben arrive ensuite. Au calcul d'Ana fini, c'est Ben qui
    /// passe ; la seconde recherche d'Ana attend le départ de sa première réponse, puis un tour.
    #[tokio::test]
    async fn a_burst_of_one_account_does_not_pass_before_another() {
        let searches = Searches::default();
        let held: Vec<Held> = (1..MAX_SEARCHES).map(|_| searches.hold()).collect();
        let (ana, ben) = (Uuid::now_v7(), Uuid::now_v7());
        let (seat, turn) = searches.enter(ana).await.unwrap();
        let mut burst = Vec::new();
        for _ in 0..10 {
            burst.push(enter(&searches, ana));
            settle().await;
        }
        let last = burst.pop().unwrap();
        for refused in burst {
            assert_eq!(refused.await.unwrap().err(), Some(Busy));
        }
        let other = enter(&searches, ben);
        settle().await;
        assert_eq!(searches.counts(), (MAX_SEARCHES, 2));
        drop(turn);
        settle().await;
        assert!(other.is_finished(), "Ben passe au calcul d'Ana fini");
        assert!(!last.is_finished());
        drop(seat);
        settle().await;
        assert!(!last.is_finished(), "Ana attend le tour que Ben tient");
        drop(other.await.unwrap().unwrap());
        assert!(last.await.unwrap().is_ok());
        drop(held);
    }

    /// Une recherche abandonnée pendant qu'elle attend sort de la file : la place rendue ensuite
    /// ne va à personne, et son compte repart de rien.
    #[tokio::test]
    async fn an_abandoned_wait_leaves_the_queue() {
        let searches = Searches::default();
        let held: Vec<Held> = (0..MAX_SEARCHES).map(|_| searches.hold()).collect();
        let ben = Uuid::now_v7();
        let waiting = enter(&searches, ben);
        settle().await;
        assert_eq!(searches.counts(), (MAX_SEARCHES, 1));
        waiting.abort();
        settle().await;
        assert_eq!(searches.counts(), (MAX_SEARCHES, 0));
        drop(held);
        assert_eq!(searches.counts(), (0, 0));
        let places = searches.enter(ben).await;
        assert!(places.is_ok(), "son compte n'est pas resté admis");
        assert_eq!(searches.counts(), (1, 0));
    }

    /// Les places données à une recherche déjà partie sont rendues : elle est abandonnée entre le
    /// don et son réveil, et la suivante passe.
    #[tokio::test]
    async fn places_given_to_a_search_already_gone_are_given_back() {
        let searches = Searches::default();
        let mut held: Vec<Held> = (0..MAX_SEARCHES).map(|_| searches.hold()).collect();
        let (ben, eve) = (Uuid::now_v7(), Uuid::now_v7());
        let gone = enter(&searches, ben);
        settle().await;
        let next = enter(&searches, eve);
        settle().await;
        // La place est donnée à Ben (compté), qui part avant d'avoir été réveillé.
        held.pop();
        assert_eq!(searches.counts(), (MAX_SEARCHES, 1));
        gone.abort();
        let places = next.await.unwrap();
        assert!(places.is_ok(), "la place rendue va à la suivante");
        assert!(matches!(gone.await, Err(gone) if gone.is_cancelled()));
        // Le compte de Ben n'est pas resté admis.
        drop(held);
        settle().await;
        assert_eq!(searches.counts(), (1, 0));
        let again = enter(&searches, ben);
        settle().await;
        assert!(again.is_finished());
    }
}
