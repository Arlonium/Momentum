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
//! Ce que la file ne fait pas : borner le nombre d'écritures qui attendent (une requête qui
//! attend garde son corps ; les plafonds de débit sont au lot 7, Ruling R121), ni départager deux
//! comptes qui en ouvrent beaucoup d'autres (un compte qui écrit dans cent pages a cent voies
//! d'acteur ; chacune n'a qu'une écriture à la fois).

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

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
}

impl Writer {
    /// L'écrivain de ce pool, qui ne doit avoir qu'une connexion, migrations déjà passées.
    pub(crate) fn new(pool: SqlitePool) -> Writer {
        Writer {
            pool,
            queue: Arc::default(),
        }
    }

    /// Attend le tour de la voie `lane`, sans délai et sans tenir de connexion.
    pub(crate) async fn turn(&self, lane: Lane) -> Turn {
        let (id, wake) = match lock(&self.queue).enter(lane) {
            Ok(()) => {
                return Turn {
                    queue: self.queue.clone(),
                };
            }
            Err(waiting) => waiting,
        };
        let mut waiting = Waiting {
            queue: self.queue.clone(),
            lane,
            id,
            wake: Some(wake),
        };
        let wake = waiting.wake.as_mut().expect("posé juste avant");
        wake.await
            .expect("la file ne lâche une écriture en attente qu'en lui donnant son tour");
        // Le tour est à nous : `Waiting` n'a plus rien à rendre.
        waiting.wake = None;
        Turn {
            queue: self.queue.clone(),
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

/// Le tour d'écrire : un seul à la fois. Le lâcher le donne à l'écriture suivante.
pub(crate) struct Turn {
    queue: Arc<Mutex<Queue>>,
}

impl Drop for Turn {
    fn drop(&mut self) {
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

    /// Le tour rendu : il va à la voie en tête de la ronde, à sa plus ancienne écriture, et la
    /// voie repasse en queue de la ronde s'il lui en reste.
    fn release(&mut self) {
        self.taken = false;
        while let Some(lane) = self.turns.pop_front() {
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
                return;
            }
        }
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

    /// Une tâche qui a reçu le tour mais part sans l'avoir pris le rend : il va à la suivante.
    #[tokio::test]
    async fn a_turn_given_to_a_task_that_left_goes_to_the_next() {
        let writer = Writer::new(sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap());
        let first = writer.turn(Lane::Common).await;
        let mut cx = Context::from_waker(Waker::noop());
        let mut second = Box::pin(writer.turn(account(1)));
        assert!(second.as_mut().poll(&mut cx).is_pending());
        let mut third = pin!(writer.turn(account(2)));
        assert!(third.as_mut().poll(&mut cx).is_pending());
        assert_eq!(writer.queued(), 2);
        // Le tour va à la deuxième, réveillée ; elle part sans l'avoir pris.
        drop(first);
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
        let writer = Writer::new(sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap());
        let first = writer.turn(Lane::Common).await;
        let mut cx = Context::from_waker(Waker::noop());
        let mut second = Box::pin(writer.turn(account(1)));
        assert!(second.as_mut().poll(&mut cx).is_pending());
        drop(second);
        assert_eq!(writer.queued(), 0);
        drop(first);
        let mut third = pin!(writer.turn(account(2)));
        assert!(matches!(third.as_mut().poll(&mut cx), Poll::Ready(_)));
    }
}
