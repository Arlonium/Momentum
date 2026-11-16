//! La lecture d'un MMD reçu par `PUT /mmd`, et l'estimation de son écriture, hors des fils du
//! serveur (Ruling R383), chacun à son tour, les comptes tour à tour (Ruling R436).
//!
//! Lire 2 Mio de MMD (le plus gros corps admis) et en estimer l'écriture occupe un fil des
//! dizaines de millisecondes en release, bien plus en debug ; sur un fil du serveur, c'était
//! autant de temps où aucune autre requête n'y avançait. Le calcul part donc sur le fil bloquant
//! de tokio (`spawn_blocking`), et [`MAX_MMD_READS`] au plus tournent de front. Le tour est rendu
//! quand le calcul finit, pas quand la requête part : un calcul lancé va au bout (on n'arrête pas
//! un fil bloquant), et une requête abandonnée ne rend pas son tour plus tôt. Abandonnée pendant
//! qu'elle attend, elle ne calcule rien et ne garde rien.
//!
//! Les requêtes qui attendent sont servies **compte par compte, tour à tour** (Ruling R436) : une
//! place qui se libère va au compte suivant dans la ronde de ceux qui attendent, et, pour ce
//! compte, à sa plus ancienne requête. Un compte qui envoie trente-deux `PUT /mmd` lents d'un coup
//! ne fait donc plus attendre les autres derrière ses trente-deux lectures (13,5 s pour une ligne
//! à la re-relecture du tour 4), mais au plus une lecture de chaque compte qui attend ; et l'ordre
//! où une rafale d'un même compte atteint le verrou de la page (Ruling R200) ne change pas, ses
//! requêtes passant dans leur ordre d'arrivée. Une part par compte essayée avant (R383 : refuser
//! au-delà, ou attendre derrière ses propres calculs) changeait cet ordre.
//!
//! Ce que la file ne borne pas : le nombre de requêtes qui attendent. Une requête qui attend garde
//! son corps (2 Mio au plus) ; la file d'écriture d'une page, elle, ne prend une requête qu'une fois
//! son MMD lu (Rulings R435, R494). Les plafonds de débit, par compte, sont au lot 7 (Ruling R121).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::oneshot;
use uuid::Uuid;

use crate::error::ApiError;

/// Calculs de MMD menés de front.
pub const MAX_MMD_READS: usize = 4;

/// La file des calculs de MMD (voir le module).
#[derive(Default)]
pub(crate) struct MmdReads {
    queue: Arc<Mutex<Queue>>,
}

#[derive(Default)]
struct Queue {
    /// Calculs en cours, ou places données à une requête qui ne l'a pas encore prise.
    running: usize,
    /// Les comptes qui attendent, dans l'ordre où leur tour viendra.
    turns: VecDeque<Uuid>,
    /// Les requêtes qui attendent, par compte, dans leur ordre d'arrivée.
    waiting: HashMap<Uuid, VecDeque<Waiter>>,
    next: u64,
}

struct Waiter {
    id: u64,
    wake: oneshot::Sender<()>,
}

impl Queue {
    /// Donne les places libres aux requêtes qui attendent : le compte en tête de la ronde, sa plus
    /// ancienne requête, puis le compte repasse en queue de la ronde s'il en a d'autres. Une place
    /// donnée sort sa requête de la file : si elle est partie entre-temps, c'est elle qui rend la
    /// place (voir [`Waiting`]).
    fn give(&mut self) {
        while self.running < MAX_MMD_READS {
            let Some(account) = self.turns.pop_front() else {
                return;
            };
            let Some(list) = self.waiting.get_mut(&account) else {
                continue;
            };
            let Some(waiter) = list.pop_front() else {
                self.waiting.remove(&account);
                continue;
            };
            if list.is_empty() {
                self.waiting.remove(&account);
            } else {
                self.turns.push_back(account);
            }
            self.running += 1;
            let _ = waiter.wake.send(());
        }
    }

    /// Une place rendue.
    fn release(&mut self) {
        self.running -= 1;
        self.give();
    }
}

fn lock(queue: &Mutex<Queue>) -> std::sync::MutexGuard<'_, Queue> {
    queue.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Une place de calcul : la lâcher la rend, et la donne à la requête suivante.
struct Turn {
    queue: Arc<Mutex<Queue>>,
}

impl Drop for Turn {
    fn drop(&mut self) {
        lock(&self.queue).release();
    }
}

/// Une requête qui attend sa place. Lâchée avant de l'avoir prise (la requête est abandonnée), elle
/// sort de la file ; si la place lui avait déjà été donnée, elle la rend.
struct Waiting {
    queue: Arc<Mutex<Queue>>,
    account: Uuid,
    id: u64,
    taken: bool,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if self.taken {
            return;
        }
        let mut queue = lock(&self.queue);
        let mut found = false;
        if let Some(list) = queue.waiting.get_mut(&self.account)
            && let Some(at) = list.iter().position(|w| w.id == self.id)
        {
            list.remove(at);
            found = true;
            if list.is_empty() {
                queue.waiting.remove(&self.account);
                queue.turns.retain(|a| *a != self.account);
            }
        }
        if !found {
            // La place lui avait été donnée : elle la rend.
            queue.release();
        }
    }
}

impl MmdReads {
    /// Mène `work` sur le fil bloquant, au tour de `account`.
    pub(crate) async fn run<T: Send + 'static>(
        &self,
        account: Uuid,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, ApiError> {
        let turn = self.turn(account).await;
        tokio::task::spawn_blocking(move || {
            // Le tour est rendu à la fin du calcul, même si la requête est partie entre-temps.
            let _turn = turn;
            work()
        })
        .await
        .map_err(crate::error::internal)
    }

    /// Une place de calcul pour `account` : tout de suite s'il y en a une libre et que personne
    /// n'attend, sinon à son tour.
    async fn turn(&self, account: Uuid) -> Turn {
        let (woken, waiting) = {
            let mut queue = lock(&self.queue);
            if queue.running < MAX_MMD_READS && queue.turns.is_empty() {
                queue.running += 1;
                return Turn {
                    queue: self.queue.clone(),
                };
            }
            let id = queue.next;
            queue.next += 1;
            let (wake, woken) = oneshot::channel();
            let list = queue.waiting.entry(account).or_default();
            let first = list.is_empty();
            list.push_back(Waiter { id, wake });
            if first {
                queue.turns.push_back(account);
            }
            let waiting = Waiting {
                queue: self.queue.clone(),
                account,
                id,
                taken: false,
            };
            (woken, waiting)
        };
        let mut waiting = waiting;
        // L'émetteur ne part qu'en donnant la place (la file le garde jusque-là).
        let _ = woken.await;
        waiting.taken = true;
        Turn {
            queue: self.queue.clone(),
        }
    }

    /// Places prises, et requêtes qui attendent (pour les tests).
    #[cfg(test)]
    pub(crate) fn counts(&self) -> (usize, usize) {
        let queue = lock(&self.queue);
        (
            queue.running,
            queue.waiting.values().map(VecDeque::len).sum(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use tokio::task::JoinHandle;

    use super::*;

    type Held = (Vec<mpsc::Sender<()>>, Vec<JoinHandle<Result<(), ApiError>>>);

    /// [`MAX_MMD_READS`] calculs de `account` retenus jusqu'à ce que le test les libère, rendus une
    /// fois tous commencés.
    async fn hold(reads: &Arc<MmdReads>, account: Uuid) -> Held {
        let (started, starts) = mpsc::channel();
        let (mut releases, mut tasks) = (Vec::new(), Vec::new());
        for _ in 0..MAX_MMD_READS {
            let (release, wait) = mpsc::channel::<()>();
            releases.push(release);
            let (reads, started) = (reads.clone(), started.clone());
            tasks.push(tokio::spawn(async move {
                reads
                    .run(account, move || {
                        started.send(()).unwrap();
                        wait.recv().unwrap();
                    })
                    .await
            }));
        }
        for _ in 0..MAX_MMD_READS {
            starts.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        (releases, tasks)
    }

    async fn release_all((releases, tasks): Held) {
        for release in releases {
            release.send(()).unwrap();
        }
        for task in tasks {
            task.await.unwrap().unwrap();
        }
    }

    /// Attend que la file ait `waiting` requêtes en attente (ce que les tâches lancées y mettent).
    async fn until_waiting(reads: &MmdReads, waiting: usize) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while reads.counts().1 < waiting {
            assert!(std::time::Instant::now() < deadline, "{:?}", reads.counts());
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn four_run_at_once_and_the_others_wait_their_turn() {
        let reads = Arc::new(MmdReads::default());
        let held = hold(&reads, Uuid::now_v7()).await;
        assert_eq!(reads.counts(), (MAX_MMD_READS, 0));
        // Un cinquième attend un tour : il ne peut pas aboutir tant que le test les tient.
        let fifth = reads.clone();
        let mut waiting = tokio::spawn(async move { fifth.run(Uuid::now_v7(), || ()).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut waiting)
                .await
                .is_err()
        );
        release_all(held).await;
        waiting.await.unwrap().unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_request_abandoned_while_waiting_keeps_nothing() {
        let reads = Arc::new(MmdReads::default());
        let held = hold(&reads, Uuid::now_v7()).await;
        for _ in 0..3 {
            let waiting = reads.run(Uuid::now_v7(), || ());
            assert!(
                tokio::time::timeout(Duration::from_millis(20), waiting)
                    .await
                    .is_err()
            );
        }
        assert_eq!(reads.counts(), (MAX_MMD_READS, 0), "rien ne reste en file");
        release_all(held).await;
        assert_eq!(reads.counts(), (0, 0));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_started_read_keeps_its_turn_until_it_ends() {
        let reads = Arc::new(MmdReads::default());
        let (started, starts) = mpsc::channel();
        let (release, wait) = mpsc::channel::<()>();
        let running = reads.clone();
        let request = tokio::spawn(async move {
            running
                .run(Uuid::now_v7(), move || {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                })
                .await
        });
        starts.recv_timeout(Duration::from_secs(10)).unwrap();
        // La requête part ; son calcul, lui, continue et garde son tour.
        request.abort();
        let _ = request.await;
        assert_eq!(reads.counts().0, 1);
        release.send(()).unwrap();
        while reads.counts().0 > 0 {
            tokio::task::yield_now().await;
        }
    }

    /// Ruling R436 : pendant que Tom a trente-deux calculs en file derrière ses quatre en cours, le
    /// calcul de Mona passe au deuxième tour qui se libère (le premier va au compte en tête de la
    /// ronde, Tom), pas après les trente-deux ; ceux de Tom passent dans leur ordre d'arrivée. Une
    /// seule des quatre places tourne (les trois autres restent tenues) : les calculs passent un à
    /// un, dans l'ordre où la file les sert.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn accounts_take_turns_and_a_burst_keeps_its_order() {
        let reads = Arc::new(MmdReads::default());
        let (tom, mona) = (Uuid::now_v7(), Uuid::now_v7());
        let (mut releases, held) = hold(&reads, tom).await;
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut tasks = Vec::new();
        let queued = |who: &'static str, n: usize, account: Uuid| {
            let (reads, order) = (reads.clone(), order.clone());
            tokio::spawn(async move {
                reads
                    .run(account, move || order.lock().unwrap().push((who, n)))
                    .await
            })
        };
        for n in 0..32 {
            tasks.push(queued("tom", n, tom));
            until_waiting(&reads, n + 1).await;
        }
        tasks.push(queued("mona", 0, mona));
        until_waiting(&reads, 33).await;
        releases.remove(0).send(()).unwrap();
        for task in tasks {
            task.await.unwrap().unwrap();
        }
        release_all((releases, held)).await;
        let order = order.lock().unwrap().clone();
        let mona_at = order.iter().position(|(who, _)| *who == "mona").unwrap();
        assert_eq!(mona_at, 1, "Mona servie au rang {mona_at} : {order:?}");
        let toms: Vec<usize> = order
            .iter()
            .filter(|(who, _)| *who == "tom")
            .map(|(_, n)| *n)
            .collect();
        assert_eq!(
            toms,
            (0..32).collect::<Vec<_>>(),
            "ordre de la rafale de Tom"
        );
    }
}
