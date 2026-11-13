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

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::OwnedMutexGuard;
use uuid::Uuid;

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

impl PageWrites {
    /// Attend le verrou de `page`, puis le rend tenu. Abandonné en route, l'appel ne garde
    /// rien : sa place dans la table est libérée.
    pub(crate) async fn lock(self: &Arc<Self>, page: Uuid) -> PageWrite {
        let (lock, ticket) = {
            let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
            let (lock, users) = locks.entry(page).or_default();
            *users += 1;
            let ticket = Ticket {
                writes: self.clone(),
                page,
            };
            (lock.clone(), ticket)
        };
        let guard = lock.lock_owned().await;
        PageWrite {
            _guard: guard,
            _ticket: ticket,
        }
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
}
