//! La file d'écriture du store (Rulings R510 à R516).
//!
//! SQLite n'a qu'un écrivain à la fois, et son verrou d'écriture n'est pas une file : qui l'attend
//! le redemande par intervalles (jusqu'à 100 ms), et qui le rend puis le redemande aussitôt le
//! reprend. Au bout du délai d'attente de sa connexion, une écriture qui n'a jamais eu son tour
//! échouait (`database is locked`, une route répondait `500`), alors que la base allait bien
//! (Ruling R485) ; et une rafale d'un seul compte, un simple lecteur qui pose et retire des
//! favoris, faisait attendre la frappe des autres jusqu'à la fin de la rafale.
//!
//! Le store tient donc sa propre file, en mémoire :
//! - **une seule connexion écrit** ([`Writer`], un pool d'une connexion, que ce module garde pour
//!   lui) ; les lectures passent par l'autre pool, dont chaque connexion est en `query_only` : une
//!   écriture qui y passerait échoue, au lieu de contourner la file ;
//! - qui veut écrire prend son **tour** ([`Turn`]) avant de prendre cette connexion, et le garde
//!   jusqu'à la fin de sa transaction ; il l'attend sans délai, et sans tenir de connexion : le
//!   verrou de SQLite n'est jamais disputé entre écritures du store, et `busy_timeout` ne joue
//!   plus que contre un autre processus sur la même base (`mementum user set-password`, une
//!   connexion de test) ;
//! - les tours sont donnés **voie par voie, tour à tour** ([`Lane`]) : un tour rendu va à la voie
//!   suivante de la ronde de celles qui attendent, et, dans cette voie, à sa plus ancienne
//!   écriture. Une voie, c'est l'acteur d'une page (son journal, ses instantanés, ses versions),
//!   ou le compte que le serveur a nommé pour la tâche qui écrit ([`writes_by`],
//!   [`attribute_writes_to`]), ou, quand personne n'est nommé, une voie commune. Une écriture
//!   attend donc au plus l'écriture en cours et une écriture de chaque autre voie qui attend,
//!   jamais toute la rafale d'un compte ; et les écritures d'un même compte gardent leur ordre
//!   d'arrivée.
//!
//! Un tour tenu à tort gèlerait toutes les écritures, sans erreur : une écriture qui attend son
//! tour plus de [`crate::StoreOptions::slow_write`], ou un tour tenu plus longtemps, se
//! journalise (avertissement) ; et, en debug, une tâche qui redemande un tour en tenant déjà un
//! panique, au lieu de s'attendre elle-même (Ruling R518).
//!
//! Ce que la file ne fait pas : borner le nombre d'écritures qui attendent (une requête qui
//! attend garde son corps ; les plafonds de débit sont au lot 7, Ruling R121), ni départager deux
//! comptes qui en ouvrent beaucoup d'autres (un compte qui écrit dans cent pages a cent voies
//! d'acteur ; chacune n'a qu'une écriture à la fois).

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use sqlx::pool::PoolConnection;
use sqlx::{Sqlite, SqliteConnection, SqlitePool, Transaction};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::StoreError;

/// Une voie de la file : les écritures d'une même voie passent dans leur ordre d'arrivée, et les
/// voies passent tour à tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Lane {
    /// L'acteur d'une page : son journal, ses instantanés et ses versions. Il n'a qu'une
    /// écriture à la fois ; sa voie ne passe donc jamais derrière la rafale d'un compte, même
    /// celle du compte dont il écrit la frappe.
    Page(Uuid),
    /// Le compte que le serveur a nommé pour la tâche qui écrit.
    Account(Uuid),
    /// Le dépôt des fichiers : les écritures faites sous son verrou, commun à toute l'instance
    /// (l'attache d'un envoi, le ménage ; Ruling R522). Sous ce verrou, elles passent une à une :
    /// la voie n'a jamais d'arriéré, et le verrou n'attend jamais la rafale d'un compte. Elle
    /// passe en priorité, sans jamais prendre deux tours de suite quand d'autres attendent : le
    /// verrou du dépôt n'est tenu que le temps de l'écriture en cours, pas d'un tour de la file.
    Files,
    /// Les écritures dont personne n'est nommé : inscription, ménage, commandes, tests.
    Common,
}

impl Lane {
    /// La voie de la tâche qui écrit : le compte qu'elle a nommé, sinon la voie commune.
    pub(crate) fn caller() -> Lane {
        writer().map_or(Lane::Common, Lane::Account)
    }
}

tokio::task_local! {
    /// Le compte auquel la tâche attribue ses écritures, dans une portée de [`writes_by`].
    static WRITER: Cell<Option<Uuid>>;
}

/// Mène `work` en attribuant ses écritures dans le store au compte `account` (`None` : à
/// personne, jusqu'à ce que [`attribute_writes_to`] en nomme un). C'est la voie de la file où
/// elles attendent leur tour. Une tâche lancée depuis `work` n'en hérite pas : la lancer dans sa
/// propre portée, avec [`writer`].
pub fn writes_by<F: Future>(account: Option<Uuid>, work: F) -> impl Future<Output = F::Output> {
    WRITER.scope(Cell::new(account), work)
}

/// Le compte auquel la tâche attribue ses écritures ; `None` hors d'une portée de [`writes_by`],
/// ou si personne n'y est encore nommé.
pub fn writer() -> Option<Uuid> {
    WRITER.try_with(Cell::get).ok().flatten()
}

/// Attribue désormais au compte `account` les écritures de la tâche, si elle est dans une portée
/// de [`writes_by`] ; hors d'une portée, rien. Le serveur l'appelle une fois la session d'une
/// requête lue.
pub fn attribute_writes_to(account: Uuid) {
    let _ = WRITER.try_with(|writer| writer.set(Some(account)));
}

/// L'écrivain du store : la seule connexion qui écrit, et la file de ceux qui attendent leur tour.
pub(crate) struct Writer {
    /// Un pool d'une seule connexion. Seul ce module le voit : on n'y écrit qu'avec un tour.
    pool: SqlitePool,
    queue: Arc<Mutex<Queue>>,
    /// Au-delà, une attente ou un tour tenu se journalise.
    slow: Duration,
}

impl Writer {
    /// L'écrivain de ce pool, qui ne doit avoir qu'une connexion, migrations déjà passées. Une
    /// attente ou un tour tenu plus de `slow` se journalise.
    pub(crate) fn new(pool: SqlitePool, slow: Duration) -> Writer {
        Writer {
            pool,
            queue: Arc::default(),
            slow,
        }
    }

    /// Attend le tour de la voie `lane`, sans délai et sans tenir de connexion. Une attente plus
    /// longue que `slow` se journalise, une fois, et continue.
    ///
    /// En debug, panique si la tâche tient déjà le tour : elle s'attendrait elle-même.
    pub(crate) async fn turn(&self, lane: Lane) -> Turn {
        let me = tokio::task::try_id();
        let entered = {
            let mut queue = lock(&self.queue);
            let again = me.is_some() && queue.holder == me;
            if again {
                Err(None)
            } else {
                queue.enter(lane).map(|()| queue.holder = me).map_err(Some)
            }
        };
        let (id, wake) = match entered {
            Ok(()) => return self.held(),
            Err(None) => {
                if cfg!(debug_assertions) {
                    panic!(
                        "une tâche qui tient son tour d'écriture en redemande un : elle \
                         s'attendrait elle-même (interblocage)"
                    );
                }
                // En release : la file, comme toute autre écriture, et le journal le dira.
                lock(&self.queue).enter(lane).unwrap_err()
            }
            Err(Some(waiting)) => waiting,
        };
        let mut waiting = Waiting {
            queue: self.queue.clone(),
            lane,
            id,
            wake: Some(wake),
        };
        let wake = waiting.wake.as_mut().expect("posé juste avant");
        let given = match tokio::time::timeout(self.slow, &mut *wake).await {
            Ok(given) => given,
            Err(_) => {
                tracing::warn!(
                    ?lane,
                    queued = self.queued(),
                    "une écriture attend son tour depuis plus de {:?} : un tour tenu trop \
                     longtemps fait attendre toutes les écritures du store",
                    self.slow
                );
                wake.await
            }
        };
        given.expect("la file ne lâche une écriture en attente qu'en lui donnant son tour");
        // Le tour est à nous : `Waiting` n'a plus rien à rendre.
        waiting.wake = None;
        lock(&self.queue).holder = me;
        self.held()
    }

    /// Le tour qu'on vient de prendre.
    fn held(&self) -> Turn {
        Turn {
            queue: self.queue.clone(),
            since: Instant::now(),
            slow: self.slow,
        }
    }

    /// Une transaction d'écriture (`BEGIN IMMEDIATE`), une fois le tour de `lane` venu. Le tour
    /// rendu doit tomber après la transaction.
    pub(crate) async fn begin(
        &self,
        lane: Lane,
    ) -> Result<(Transaction<'static, Sqlite>, Turn), StoreError> {
        let turn = self.turn(lane).await;
        let tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        Ok((tx, turn))
    }

    /// La connexion qui écrit, une fois le tour de `lane` venu : pour une seule instruction, qui
    /// est sa propre transaction.
    pub(crate) async fn connection(&self, lane: Lane) -> Result<WriteConn, StoreError> {
        let turn = self.turn(lane).await;
        let conn = self.pool.acquire().await?;
        Ok(WriteConn { conn, _turn: turn })
    }

    /// Les écritures qui attendent leur tour.
    pub(crate) fn queued(&self) -> usize {
        lock(&self.queue).queued
    }

    /// Ferme la connexion qui écrit (attend la fin de ce qu'elle fait).
    pub(crate) async fn close(&self) {
        self.pool.close().await;
    }

    /// La connexion qui écrit, tenue avec un tour : pour lire ses réglages (tests).
    pub(crate) async fn held_connection(&self) -> Result<WriteConn, StoreError> {
        self.connection(Lane::caller()).await
    }
}

/// Le tour d'écrire : un seul à la fois. Le lâcher le donne à l'écriture suivante ; tenu plus
/// que `slow`, il se journalise en tombant.
pub(crate) struct Turn {
    queue: Arc<Mutex<Queue>>,
    since: Instant,
    slow: Duration,
}

impl Drop for Turn {
    fn drop(&mut self) {
        let held = self.since.elapsed();
        if held > self.slow {
            tracing::warn!(
                held_ms = held.as_millis() as u64,
                "tour d'écriture tenu {held:?}, plus que {:?} : les autres écritures du store \
                 l'ont attendu",
                self.slow
            );
        }
        lock(&self.queue).release();
    }
}

/// La connexion qui écrit, avec le tour : une instruction y est sa propre transaction. Se prête
/// comme une connexion (`&mut *conn`).
pub(crate) struct WriteConn {
    // Dans cet ordre : la connexion rentre au pool avant que le tour soit rendu.
    conn: PoolConnection<Sqlite>,
    _turn: Turn,
}

impl Deref for WriteConn {
    type Target = SqliteConnection;

    fn deref(&self) -> &SqliteConnection {
        &self.conn
    }
}

impl DerefMut for WriteConn {
    fn deref_mut(&mut self) -> &mut SqliteConnection {
        &mut self.conn
    }
}

/// La file : qui a le tour, et qui l'attend, voie par voie.
#[derive(Default)]
struct Queue {
    /// Le tour est donné : à une écriture en cours, ou à une tâche réveillée qui va le prendre.
    taken: bool,
    /// Les voies qui ont une écriture en file, dans l'ordre où leur tour viendra. Une voie y est
    /// une fois, et seulement si elle a une écriture en file.
    turns: VecDeque<Lane>,
    /// Les écritures en file, par voie, dans leur ordre d'arrivée.
    waiting: HashMap<Lane, VecDeque<Waiter>>,
    /// Les écritures en file, toutes voies confondues.
    queued: usize,
    next: u64,
    /// La tâche qui a pris le tour, si elle est connue (la garde de réentrance).
    holder: Option<tokio::task::Id>,
    /// Le dernier tour donné l'a été à la voie du dépôt : le suivant n'y va pas en priorité.
    files_served: bool,
}

struct Waiter {
    id: u64,
    wake: oneshot::Sender<()>,
}

impl Queue {
    /// Une écriture de la voie `lane` demande le tour : `Ok` s'il est libre et que personne
    /// n'attend (elle le prend), sinon elle prend place en file, et attend d'être réveillée.
    fn enter(&mut self, lane: Lane) -> Result<(), (u64, oneshot::Receiver<()>)> {
        if !self.taken && self.queued == 0 {
            self.taken = true;
            return Ok(());
        }
        let (wake, woken) = oneshot::channel();
        let id = self.next;
        self.next += 1;
        let list = self.waiting.entry(lane).or_default();
        if list.is_empty() {
            self.turns.push_back(lane);
        }
        list.push_back(Waiter { id, wake });
        self.queued += 1;
        Err((id, woken))
    }

    /// Le tour rendu : il va à la voie du dépôt si elle attend et n'a pas eu le tour d'avant
    /// (Ruling R522), sinon à la voie en tête de la ronde ; dans la voie, à sa plus ancienne
    /// écriture, et la voie repasse en queue de la ronde s'il lui en reste.
    fn release(&mut self) {
        self.taken = false;
        self.holder = None;
        while let Some(lane) = self.next_lane() {
            let Some(list) = self.waiting.get_mut(&lane) else {
                continue;
            };
            let Some(waiter) = list.pop_front() else {
                self.waiting.remove(&lane);
                continue;
            };
            if list.is_empty() {
                self.waiting.remove(&lane);
            } else {
                self.turns.push_back(lane);
            }
            self.queued -= 1;
            // Une tâche partie a retiré son écriture sous ce même verrou ([`Waiting`]) : le
            // réveil ne peut pas se perdre ; s'il se perdait, le tour irait au suivant.
            if waiter.wake.send(()).is_ok() {
                self.taken = true;
                self.files_served = lane == Lane::Files;
                return;
            }
        }
    }

    /// La voie à qui revient le tour : celle du dépôt d'abord, si elle attend et n'a pas eu le
    /// tour d'avant ; sinon la tête de la ronde.
    fn next_lane(&mut self) -> Option<Lane> {
        if !self.files_served
            && let Some(at) = self.turns.iter().position(|lane| *lane == Lane::Files)
        {
            return self.turns.remove(at);
        }
        self.turns.pop_front()
    }

    /// Retire de la file l'écriture `id` de la voie `lane`, si elle y est encore.
    fn leave(&mut self, lane: Lane, id: u64) {
        let Some(list) = self.waiting.get_mut(&lane) else {
            return;
        };
        let Some(at) = list.iter().position(|w| w.id == id) else {
            return;
        };
        list.remove(at);
        self.queued -= 1;
        if list.is_empty() {
            self.waiting.remove(&lane);
            self.turns.retain(|l| *l != lane);
        }
    }
}

/// Une écriture qui attend son tour. Lâchée avant de l'avoir pris (la requête est abandonnée),
/// elle sort de la file ; si le tour lui avait déjà été donné, elle le rend.
struct Waiting {
    queue: Arc<Mutex<Queue>>,
    lane: Lane,
    id: u64,
    /// `None` une fois le tour pris.
    wake: Option<oneshot::Receiver<()>>,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        let Some(mut wake) = self.wake.take() else {
            return;
        };
        let mut queue = lock(&self.queue);
        // Sous le verrou de la file, où le tour se donne : fermé, le réveil ne peut plus venir.
        wake.close();
        if wake.try_recv().is_ok() {
            queue.release();
        } else {
            queue.leave(self.lane, self.id);
        }
    }
}

fn lock(queue: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    // Rien ne panique sous ce verrou ; s'il était empoisonné, l'état reste celui d'avant.
    queue.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use super::*;

    /// L'ordre où les tours sont donnés : chaque écriture en file note sa voie et son rang, puis
    /// rend son tour.
    fn served(queue: &Arc<Mutex<Queue>>, entries: &[(Lane, u32)]) -> Vec<(Lane, u32)> {
        let mut woken: Vec<_> = entries
            .iter()
            .map(|&(lane, n)| {
                let (_, wake) = lock(queue).enter(lane).unwrap_err();
                (lane, n, wake)
            })
            .collect();
        let mut order = Vec::new();
        lock(queue).release();
        while !woken.is_empty() {
            let at = woken
                .iter_mut()
                .position(|(_, _, wake)| wake.try_recv().is_ok())
                .expect("un tour donné");
            let (lane, n, _) = woken.remove(at);
            order.push((lane, n));
            lock(queue).release();
        }
        order
    }

    fn account(n: u128) -> Lane {
        Lane::Account(Uuid::from_u128(n))
    }

    /// Les voies passent tour à tour ; dans une voie, l'ordre d'arrivée.
    #[test]
    fn lanes_take_turns_and_keep_their_order() {
        let queue = Arc::new(Mutex::new(Queue::default()));
        lock(&queue).enter(Lane::Common).unwrap();
        let (tom, mona, page) = (account(1), account(2), Lane::Page(Uuid::from_u128(3)));
        let order = served(
            &queue,
            &[(tom, 1), (tom, 2), (tom, 3), (mona, 1), (page, 1), (tom, 4)],
        );
        assert_eq!(
            order,
            [(tom, 1), (mona, 1), (page, 1), (tom, 2), (tom, 3), (tom, 4)]
        );
        assert!(!lock(&queue).taken);
        assert_eq!(lock(&queue).queued, 0);
    }

    /// Une écriture abandonnée en file en sort, et sa voie aussi si elle n'y avait plus rien :
    /// revenue plus tard, la voie ne compte qu'une fois dans la ronde.
    #[test]
    fn a_waiter_that_leaves_takes_its_lane_out_of_the_round() {
        let queue = Arc::new(Mutex::new(Queue::default()));
        lock(&queue).enter(Lane::Common).unwrap();
        let (tom, mona) = (account(1), account(2));
        let (id, _wake) = lock(&queue).enter(mona).unwrap_err();
        lock(&queue).leave(mona, id);
        assert_eq!(lock(&queue).queued, 0);
        assert!(lock(&queue).turns.is_empty());
        let order = served(&queue, &[(tom, 1), (mona, 1), (tom, 2), (mona, 2)]);
        assert_eq!(order, [(tom, 1), (mona, 1), (tom, 2), (mona, 2)]);
    }

    /// Un écrivain sans base : `turn` ne touche pas à SQLite.
    fn writer() -> Arc<Writer> {
        let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        Arc::new(Writer::new(pool, Duration::from_secs(5)))
    }

    /// Un tour pris et tenu par une autre tâche, jusqu'à ce que `release` soit lâché.
    async fn held_elsewhere(writer: &Arc<Writer>) -> oneshot::Sender<()> {
        let (release, released) = oneshot::channel::<()>();
        let (taken, took) = oneshot::channel::<()>();
        let writer = writer.clone();
        tokio::spawn(async move {
            let turn = writer.turn(Lane::Common).await;
            let _ = taken.send(());
            let _ = released.await;
            drop(turn);
        });
        took.await.unwrap();
        release
    }

    /// Une tâche qui a reçu le tour mais part sans l'avoir pris le rend : il va à la suivante.
    #[tokio::test]
    async fn a_turn_given_to_a_task_that_left_goes_to_the_next() {
        let writer = writer();
        let first = held_elsewhere(&writer).await;
        let mut cx = Context::from_waker(Waker::noop());
        let mut second = Box::pin(writer.turn(account(1)));
        assert!(second.as_mut().poll(&mut cx).is_pending());
        let mut third = pin!(writer.turn(account(2)));
        assert!(third.as_mut().poll(&mut cx).is_pending());
        assert_eq!(writer.queued(), 2);
        // Le tour va à la deuxième, réveillée ; elle part sans l'avoir pris.
        drop(first);
        while lock(&writer.queue).queued != 1 {
            tokio::task::yield_now().await;
        }
        drop(second);
        assert_eq!(writer.queued(), 0);
        let Poll::Ready(turn) = third.as_mut().poll(&mut cx) else {
            panic!("le tour rendu par la deuxième va à la troisième");
        };
        drop(turn);
        assert!(!lock(&writer.queue).taken);
    }

    /// Une tâche abandonnée pendant qu'elle attend ne garde rien : le tour libre se prend aussitôt.
    #[tokio::test]
    async fn a_task_dropped_while_waiting_keeps_nothing() {
        let writer = writer();
        let first = held_elsewhere(&writer).await;
        let mut cx = Context::from_waker(Waker::noop());
        let mut second = Box::pin(writer.turn(account(1)));
        assert!(second.as_mut().poll(&mut cx).is_pending());
        drop(second);
        assert_eq!(writer.queued(), 0);
        drop(first);
        while lock(&writer.queue).taken {
            tokio::task::yield_now().await;
        }
        let mut third = pin!(writer.turn(account(2)));
        assert!(matches!(third.as_mut().poll(&mut cx), Poll::Ready(_)));
    }

    /// En debug, une tâche qui redemande un tour en tenant déjà un panique, au lieu de s'attendre
    /// elle-même sans fin (Ruling R518) ; une autre tâche, elle, attend son tour.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn a_task_asking_a_second_turn_panics() {
        let writer = writer();
        let again = tokio::spawn({
            let writer = writer.clone();
            async move {
                let _first = writer.turn(Lane::Common).await;
                let _second = writer.turn(account(1)).await;
            }
        });
        let panic = tokio::time::timeout(Duration::from_secs(10), again)
            .await
            .expect("la seconde demande attend sans fin")
            .unwrap_err()
            .into_panic();
        let message = panic.downcast_ref::<&str>().copied().unwrap_or_default();
        assert!(message.contains("en redemande un"), "{message}");
        // Le tour de la tâche partie est rendu : une autre le prend.
        let other = tokio::spawn({
            let writer = writer.clone();
            async move { drop(writer.turn(account(2)).await) }
        });
        tokio::time::timeout(Duration::from_secs(10), other)
            .await
            .expect("le tour rendu")
            .unwrap();
    }
}
