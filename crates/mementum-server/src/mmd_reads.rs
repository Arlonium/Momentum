//! La lecture d'un MMD reçu par `PUT /mmd`, et l'estimation de son écriture, hors des fils du
//! serveur (Ruling R383).
//!
//! Lire 2 Mio de MMD (le plus gros corps admis) et en estimer l'écriture occupe un fil des
//! dizaines de millisecondes en release, bien plus en debug ; sur un fil du serveur, c'était
//! autant de temps où aucune autre requête n'y avançait. Le calcul part donc sur le fil bloquant
//! de tokio (`spawn_blocking`), et [`MAX_MMD_READS`] au plus tournent de front : les autres
//! attendent leur tour, dans l'ordre d'arrivée (le sémaphore de tokio est équitable). Le tour est
//! rendu quand le calcul finit, pas quand la requête part : un calcul lancé va au bout (on
//! n'arrête pas un fil bloquant), et une requête abandonnée ne rend pas son tour plus tôt.
//! Abandonnée pendant qu'elle attend, elle ne calcule rien et ne garde rien.
//!
//! Ce qui n'est pas borné (Ruling R383) : la file. Chaque requête qui attend garde son corps
//! (2 Mio au plus), et un compte qui enverrait cent `PUT /mmd` d'un coup ferait passer les
//! requêtes des autres après ses cent calculs. Une part par compte a été essayée : refuser
//! au-delà (`429`) refusait aussi les rafales légitimes d'un agent ; faire attendre un compte
//! derrière ses propres calculs changeait l'ordre où une rafale atteint le verrou de la page
//! (Ruling R200), et affaiblissait les tests qui le gardent. Les plafonds de débit, par compte,
//! sont au lot 7 (Ruling R121).

use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::error::ApiError;

/// Calculs de MMD menés de front.
pub const MAX_MMD_READS: usize = 4;

/// La file des calculs de MMD (voir le module).
pub(crate) struct MmdReads {
    running: Arc<Semaphore>,
}

impl Default for MmdReads {
    fn default() -> Self {
        MmdReads {
            running: Arc::new(Semaphore::new(MAX_MMD_READS)),
        }
    }
}

impl MmdReads {
    /// Mène `work` sur le fil bloquant, à son tour.
    pub(crate) async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, ApiError> {
        let turn = self
            .running
            .clone()
            .acquire_owned()
            .await
            .map_err(crate::error::internal)?;
        tokio::task::spawn_blocking(move || {
            // Le tour est rendu à la fin du calcul, même si la requête est partie entre-temps.
            let _turn = turn;
            work()
        })
        .await
        .map_err(crate::error::internal)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use tokio::task::JoinHandle;

    use super::*;

    type Held = (Vec<mpsc::Sender<()>>, Vec<JoinHandle<Result<(), ApiError>>>);

    /// [`MAX_MMD_READS`] calculs retenus jusqu'à ce que le test les libère, rendus une fois tous
    /// commencés.
    async fn hold(reads: &Arc<MmdReads>) -> Held {
        let (started, starts) = mpsc::channel();
        let (mut releases, mut tasks) = (Vec::new(), Vec::new());
        for _ in 0..MAX_MMD_READS {
            let (release, wait) = mpsc::channel::<()>();
            releases.push(release);
            let (reads, started) = (reads.clone(), started.clone());
            tasks.push(tokio::spawn(async move {
                reads
                    .run(move || {
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn four_run_at_once_and_the_others_wait_their_turn() {
        let reads = Arc::new(MmdReads::default());
        let held = hold(&reads).await;
        assert_eq!(reads.running.available_permits(), 0);
        // Un cinquième attend un tour : il ne peut pas aboutir tant que le test les tient.
        let fifth = reads.clone();
        let mut waiting = tokio::spawn(async move { fifth.run(|| ()).await });
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
        let held = hold(&reads).await;
        for _ in 0..3 {
            let waiting = reads.run(|| ());
            assert!(
                tokio::time::timeout(Duration::from_millis(20), waiting)
                    .await
                    .is_err()
            );
        }
        release_all(held).await;
        assert_eq!(reads.running.available_permits(), MAX_MMD_READS);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_started_read_keeps_its_turn_until_it_ends() {
        let reads = Arc::new(MmdReads::default());
        let (started, starts) = mpsc::channel();
        let (release, wait) = mpsc::channel::<()>();
        let running = reads.clone();
        let request = tokio::spawn(async move {
            running
                .run(move || {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                })
                .await
        });
        starts.recv_timeout(Duration::from_secs(10)).unwrap();
        // La requête part ; son calcul, lui, continue et garde son tour.
        request.abort();
        let _ = request.await;
        assert_eq!(reads.running.available_permits(), MAX_MMD_READS - 1);
        release.send(()).unwrap();
        while reads.running.available_permits() < MAX_MMD_READS {
            tokio::task::yield_now().await;
        }
    }
}
