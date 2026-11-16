//! Une écriture du serveur à la fois par page (Ruling R200).
//!
//! `PUT /mmd` et la restauration d'une version contrôlent les droits, chargent la page, les
//! relisent (Ruling R89), puis envoient une commande à l'acteur. Envoyées d'un coup, des
//! dizaines de ces requêtes passaient toutes leurs contrôles dans les premières millisecondes,
//! puis attendaient dans la boîte de l'acteur, sans borne ni délai : un droit retiré, une
//! session fermée, une mise à la corbeille ne les arrêtaient plus, et chacune remplaçait le
//! contenu de la page à son tour (constat S1 de la relecture finale).
//!
//! Chaque page a donc un verrou asynchrone, que ces routes prennent **après** leur premier
//! contrôle et la lecture de leur corps, et tiennent pendant « second contrôle, commande à
//! l'acteur, réponse » : chaque requête relit sa session (Ruling R209) et ses droits après la
//! précédente, et une déconnexion, un changement de mot de passe ou un droit retiré arrêtent la
//! file. Ce qui suit l'obtention du verrou va au bout dans sa propre tâche
//! (`to_completion`), même si le client coupe : sinon, couper chaque requête aussitôt envoyée
//! relâcherait le verrou avant l'écriture, et la file se reformerait dans la boîte de l'acteur.
//! Un client qui coupe pendant qu'il attend le verrou, lui, ne le garde pas.
//!
//! Ce que le verrou ne permet pas : bloquer une page au-delà de sa propre écriture (l'acteur
//! traitait déjà les écritures une par une ; le verrou, équitable, les fait attendre dans le
//! même ordre, en dehors de sa boîte), ni en bloquer une autre (une entrée par page), ni faire
//! grossir la table : une page n'y a d'entrée que tant que quelqu'un tient ou attend son verrou,
//! et seule une requête qui a passé le premier contrôle (le droit de modifier la page) en crée
//! une. Les lectures, la synchro et les commentaires ne le prennent pas.
//!
//! La file d'une page est bornée pour `PUT /mmd` (Ruling R435) : une requête y prend sa place
//! ([`PageWrites::reserve`]) avant de lire son MMD, et la garde jusqu'à la fin de son écriture.
//! Une fois lue, elle garde l'arbre de son MMD jusqu'à son tour (31 Mio pour un MMD accepté de
//! 789 Kio, à la re-relecture du tour 4 ; 64 requêtes vers une page : 2,2 Gio) : au-delà de
//! [`MAX_PAGE_WRITES`] requêtes dans la file de la page (lectures, attentes et écriture en cours,
//! restaurations comprises), un `PUT /mmd` est refusé sur-le-champ (`429 page_busy`), sans que son
//! corps soit lu.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::OwnedMutexGuard;
use uuid::Uuid;

/// Requêtes au plus dans la file d'une page quand un `PUT /mmd` y prend sa place (Ruling R435) :
/// celle qui écrit, celles qui attendent le verrou et celles qui lisent leur MMD. Au pire, autant
/// d'arbres de MMD acceptés gardés en mémoire pour la page (environ 31 Mio chacun pour les plus
/// lourds relevés : 250 Mio), et une écriture qui attend derrière sept autres (8 s d'acteur au
/// plus chacune).
pub const MAX_PAGE_WRITES: usize = 8;

/// Les verrous des pages dont une écriture du serveur est en cours ou attend.
#[derive(Default)]
pub(crate) struct PageWrites {
    locks: Mutex<HashMap<Uuid, Entry>>,
}

/// Le verrou d'une page, et le nombre de requêtes qui le tiennent ou l'attendent.
type Entry = (Arc<tokio::sync::Mutex<()>>, usize);

/// Le verrou d'une page, tenu : le lâcher le rend.
pub(crate) struct PageWrite {
    // Dans cet ordre : le verrou est rendu avant que la place dans la table soit libérée.
    _guard: OwnedMutexGuard<()>,
    _ticket: Ticket,
}

/// Une place dans la table : tant qu'il en reste une, l'entrée de la page reste.
struct Ticket {
    writes: Arc<PageWrites>,
    page: Uuid,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut locks = self
            .writes
            .locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((_, users)) = locks.get_mut(&self.page) {
            *users -= 1;
            if *users == 0 {
                locks.remove(&self.page);
            }
        }
    }
}

/// Une place dans la file d'une page, prise avant d'attendre son verrou ([`PageWrites::reserve`]).
/// La lâcher la rend.
pub(crate) struct Place {
    lock: Arc<tokio::sync::Mutex<()>>,
    ticket: Ticket,
}

impl Place {
    /// Attend le verrou de la page, puis le rend tenu, avec la place. Abandonné en route, l'appel
    /// ne garde rien : sa place dans la table est libérée.
    pub(crate) async fn lock(self) -> PageWrite {
        let guard = self.lock.lock_owned().await;
        PageWrite {
            _guard: guard,
            _ticket: self.ticket,
        }
    }
}

impl PageWrites {
    /// Attend le verrou de `page`, puis le rend tenu. Abandonné en route, l'appel ne garde
    /// rien : sa place dans la table est libérée. Sans borne : la restauration d'une version.
    pub(crate) async fn lock(self: &Arc<Self>, page: Uuid) -> PageWrite {
        self.reserve(page, usize::MAX)
            .expect("une file sans borne a toujours une place")
            .lock()
            .await
    }

    /// Une place dans la file de `page`, s'il y a moins de `limit` requêtes qui la tiennent,
    /// l'attendent ou y ont une place ; sinon `None`, et rien n'est pris (Ruling R435).
    pub(crate) fn reserve(self: &Arc<Self>, page: Uuid, limit: usize) -> Option<Place> {
        let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
        let (lock, users) = locks.entry(page).or_default();
        if *users >= limit {
            return None;
        }
        *users += 1;
        Some(Place {
            lock: lock.clone(),
            ticket: Ticket {
                writes: self.clone(),
                page,
            },
        })
    }

    /// Les pages qui ont une entrée (pour les tests).
    #[cfg(test)]
    fn len(&self) -> usize {
        self.locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Le verrou de `page`, s'il se prend sans attendre : l'appel est interrogé une fois, puis
    /// abandonné s'il attend.
    async fn at_once(writes: &Arc<PageWrites>, page: Uuid) -> Option<PageWrite> {
        tokio::time::timeout(Duration::ZERO, writes.lock(page))
            .await
            .ok()
    }

    #[tokio::test]
    async fn a_page_waits_for_its_own_writes_only() {
        let writes = Arc::new(PageWrites::default());
        let (one, two) = (Uuid::now_v7(), Uuid::now_v7());
        let held = at_once(&writes, one).await.expect("libre");
        assert!(
            at_once(&writes, one).await.is_none(),
            "la même page n'attend pas"
        );
        let other = at_once(&writes, two).await;
        assert!(other.is_some(), "une autre page attend la première");
        drop(held);
        assert!(
            at_once(&writes, one).await.is_some(),
            "rendu, il se reprend"
        );
    }

    /// La table ne garde que les pages dont le verrou est tenu ou attendu : rien après la
    /// dernière écriture, ni après un appel abandonné pendant qu'il attendait.
    #[tokio::test]
    async fn nothing_is_left_once_nobody_holds_or_waits() {
        let writes = Arc::new(PageWrites::default());
        let page = Uuid::now_v7();
        let held = at_once(&writes, page).await.expect("libre");
        // Un appel qui attend, puis abandonné (un client qui coupe).
        assert!(at_once(&writes, page).await.is_none());
        assert_eq!(writes.len(), 1);
        // Un appel qui attend, et l'obtient quand le premier rend le verrou.
        let waiting = {
            let writes = writes.clone();
            tokio::spawn(async move { drop(writes.lock(page).await) })
        };
        tokio::task::yield_now().await;
        drop(held);
        waiting.await.unwrap();
        assert_eq!(writes.len(), 0, "une entrée reste sans personne");
        for _ in 0..1_000 {
            drop(at_once(&writes, Uuid::now_v7()).await);
        }
        assert_eq!(writes.len(), 0);
    }

    /// Ruling R435 : une page n'admet que `limit` requêtes dans sa file, celle qui tient le
    /// verrou comprise ; une de plus est refusée sans rien prendre, une place rendue se reprend,
    /// et une autre page n'en est pas touchée.
    #[tokio::test]
    async fn a_page_queue_is_bounded() {
        let writes = Arc::new(PageWrites::default());
        let (page, other) = (Uuid::now_v7(), Uuid::now_v7());
        let held = writes.reserve(page, 3).unwrap().lock().await;
        let places: Vec<Place> = (0..2).map(|_| writes.reserve(page, 3).unwrap()).collect();
        assert!(writes.reserve(page, 3).is_none(), "une de trop");
        assert!(writes.reserve(other, 3).is_some(), "une autre page");
        drop(places);
        let again = writes.reserve(page, 3);
        assert!(again.is_some(), "une place rendue se reprend");
        // Le verrou tenu, la place prise : la page en compte deux.
        assert!(writes.reserve(page, 2).is_none());
        drop((held, again));
        assert_eq!(writes.len(), 0);
    }
}
