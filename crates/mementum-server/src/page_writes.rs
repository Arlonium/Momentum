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
//! La file d'une page est bornée (Rulings R435, R494, R535) : une requête y prend sa place
//! ([`PageWrites::reserve`]) une fois son corps lu et validé (le MMD d'un `PUT /mmd`, la version
//! d'une restauration), et la garde jusqu'à la fin de son écriture. Un `PUT /mmd` garde l'arbre
//! de son MMD jusqu'à son tour (31 Mio pour un MMD accepté de 789 Kio, à la re-relecture du
//! tour 4 ; 64 requêtes vers une page : 2,2 Gio) : au-delà de [`MAX_PAGE_WRITES`] requêtes dans
//! la file de la page (attentes et écriture en cours), ou de [`MAX_ACCOUNT_WRITES`] pour le
//! compte qui écrit, la requête est refusée sur-le-champ (`429 page_busy`). Une restauration y
//! prend sa place comme un `PUT /mmd`, sur la part de son compte : sans borne, douze boucles de
//! restaurations sans effet (Ruling R180) d'un même compte tenaient la file pleine, et un autre
//! rédacteur n'écrivait plus (aucune écriture et 93 refus en dix secondes, à la re-relecture du
//! tour 6). Une requête refusée à sa lecture (un MMD illisible,
//! trop long à écrire) n'a jamais de place : prise avant la lecture, elle laissait un compte tenir
//! la file pleine sans rien écrire (huit boucles de `PUT /mmd` refusés : quatre écritures et
//! quatre-vingt-un refus pour un autre rédacteur en dix secondes, à la re-relecture du tour 5).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::OwnedMutexGuard;
use uuid::Uuid;

/// Requêtes au plus dans la file d'une page quand un `PUT /mmd` y prend sa place (Ruling R435) :
/// celle qui écrit et celles qui attendent le verrou. Au pire, autant d'arbres de MMD acceptés
/// gardés en mémoire pour la page (environ 31 Mio chacun pour les plus lourds relevés : 250 Mio),
/// et une écriture qui attend derrière sept autres (8 s d'acteur au plus chacune).
pub const MAX_PAGE_WRITES: usize = 8;

/// Places au plus d'un même compte dans la file d'une page (Ruling R494) : une qui écrit, une qui
/// attend. Un compte n'en tient jamais assez pour fermer la page aux autres rédacteurs.
pub const MAX_ACCOUNT_WRITES: usize = 2;

/// Les verrous des pages dont une écriture du serveur est en cours ou attend.
#[derive(Default)]
pub(crate) struct PageWrites {
    locks: Mutex<HashMap<Uuid, Entry>>,
}

/// Le verrou d'une page, le nombre de requêtes qui le tiennent ou l'attendent, et ce nombre par
/// compte (Rulings R494, R535).
#[derive(Default)]
struct Entry {
    lock: Arc<tokio::sync::Mutex<()>>,
    users: usize,
    accounts: HashMap<Uuid, usize>,
}

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
    /// Le compte de la requête, compté dans sa part.
    account: Uuid,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut locks = self
            .writes
            .locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = locks.get_mut(&self.page) {
            entry.users -= 1;
            if let Some(places) = entry.accounts.get_mut(&self.account) {
                *places -= 1;
                if *places == 0 {
                    entry.accounts.remove(&self.account);
                }
            }
            if entry.users == 0 {
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
    /// Une place dans la file de `page` pour une écriture de `account` (un `PUT /mmd`, une
    /// restauration), s'il y a moins de [`MAX_PAGE_WRITES`] requêtes qui la tiennent ou y
    /// attendent, et moins de [`MAX_ACCOUNT_WRITES`] de ce compte ; sinon `None`, et rien n'est
    /// pris (Rulings R435, R494, R535).
    pub(crate) fn reserve(self: &Arc<Self>, page: Uuid, account: Uuid) -> Option<Place> {
        let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = locks.entry(page).or_default();
        let held = entry.accounts.get(&account).copied().unwrap_or(0);
        if entry.users >= MAX_PAGE_WRITES || held >= MAX_ACCOUNT_WRITES {
            if entry.users == 0 {
                locks.remove(&page);
            }
            return None;
        }
        entry.users += 1;
        *entry.accounts.entry(account).or_default() += 1;
        Some(Place {
            lock: entry.lock.clone(),
            ticket: Ticket {
                writes: self.clone(),
                page,
                account,
            },
        })
    }

    /// Les requêtes qui tiennent le verrou de `page` ou y ont une place (pour les tests).
    #[cfg(test)]
    pub(crate) fn places(&self, page: Uuid) -> usize {
        self.locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&page)
            .map_or(0, |entry| entry.users)
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

    /// Le verrou de `page`, pris par un compte neuf, s'il se prend sans attendre : l'appel est
    /// interrogé une fois, puis abandonné s'il attend.
    async fn at_once(writes: &Arc<PageWrites>, page: Uuid) -> Option<PageWrite> {
        let place = writes.reserve(page, Uuid::now_v7())?;
        tokio::time::timeout(Duration::ZERO, place.lock())
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
            let place = writes.reserve(page, Uuid::now_v7()).unwrap();
            tokio::spawn(async move { drop(place.lock().await) })
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

    /// Ruling R435 : une page n'admet que `MAX_PAGE_WRITES` requêtes dans sa file, celle qui
    /// tient le verrou comprise ; une de plus est refusée sans rien prendre, une place rendue se
    /// reprend, et une autre page n'en est pas touchée. Ruling R494 : un compte n'y a que
    /// `MAX_ACCOUNT_WRITES` places.
    #[tokio::test]
    async fn a_page_queue_is_bounded_and_shared() {
        let writes = Arc::new(PageWrites::default());
        let (page, other) = (Uuid::now_v7(), Uuid::now_v7());
        let accounts: Vec<Uuid> = (0..MAX_PAGE_WRITES).map(|_| Uuid::now_v7()).collect();
        let (tom, mona) = (accounts[0], accounts[1]);
        let held = writes.reserve(page, tom).unwrap().lock().await;
        let second = writes.reserve(page, tom).unwrap();
        assert!(
            writes.reserve(page, tom).is_none(),
            "la part de Tom est prise"
        );
        assert!(
            writes.reserve(page, mona).is_some(),
            "Mona a encore une place"
        );
        assert!(writes.reserve(other, tom).is_some(), "une autre page");
        let places: Vec<Place> = accounts[1..MAX_PAGE_WRITES - 1]
            .iter()
            .map(|a| writes.reserve(page, *a).unwrap())
            .collect();
        assert!(
            writes
                .reserve(page, accounts[MAX_PAGE_WRITES - 1])
                .is_none(),
            "la file est pleine"
        );
        drop(places);
        drop(second);
        let again = writes.reserve(page, tom);
        assert!(again.is_some(), "une place rendue se reprend");
        drop((held, again));
        assert_eq!(writes.len(), 0);
        // Une place refusée ne laisse pas d'entrée.
        let full: Vec<Place> = (0..MAX_ACCOUNT_WRITES)
            .map(|_| writes.reserve(page, tom).unwrap())
            .collect();
        assert!(writes.reserve(page, tom).is_none());
        drop(full);
        assert_eq!(writes.len(), 0);
    }
}
