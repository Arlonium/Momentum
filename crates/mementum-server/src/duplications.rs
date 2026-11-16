//! Le débit des duplications de pages, par compte, tenu en mémoire (Rulings R605, R606).
//!
//! Une duplication lit des pages entières, bâtit leurs documents sur un fil de calcul, puis les
//! écrit en une transaction qui tient le tour d'écriture de toute l'instance. Sans borne, un seul
//! compte qui duplique en boucle sortait tout le monde du budget de la frappe et remplissait le
//! disque (constat C1 de la relecture de la Task S6). Deux règles, par compte :
//! - **une duplication à la fois** : la suivante est refusée tout de suite (`429
//!   duplication_busy`), jamais mise en attente ;
//! - **un budget d'octets de documents bâtis par fenêtre glissante** ([`DUPLICATION_BUDGET`] par
//!   [`DUPLICATION_WINDOW`]) : une duplication demandée quand le compte a déjà bâti ce budget dans
//!   la fenêtre est refusée, avec le délai juste dans `Retry-After`. Ce qui est compté est ce qui
//!   a été **bâti**, écrit ou non : une copie refusée après coup (trop lourde, page partie) a
//!   coûté son calcul. Une duplication admise va au bout : le compte bâtit donc au plus son
//!   budget, plus une copie ([`MAX_DUPLICATED_BYTES`](crate::MAX_DUPLICATED_BYTES)), par fenêtre.
//!
//! Ce n'est pas une file : rien n'attend, donc rien ne s'accumule. La table ne garde que les
//! comptes qui ont dupliqué pendant la dernière fenêtre.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use uuid::Uuid;

/// Octets de documents qu'un compte bâtit en dupliquant, par [`DUPLICATION_WINDOW`] (Ruling R606).
pub const DUPLICATION_BUDGET: usize = 64 << 20;

/// La fenêtre glissante du budget des duplications.
pub const DUPLICATION_WINDOW: Duration = Duration::from_secs(60);

/// Pourquoi une duplication n'est pas admise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Busy {
    /// Le compte en a déjà une en cours.
    Running,
    /// Le compte a dépensé son budget : il en retrouve dans ce nombre de secondes.
    Spent { retry_after: u64 },
}

/// Le débit des duplications de tous les comptes (voir le module).
pub(crate) struct Duplications {
    budget: usize,
    window: Duration,
    accounts: Mutex<HashMap<Uuid, Account>>,
}

#[derive(Default)]
struct Account {
    running: bool,
    /// Ce que le compte a bâti, du plus ancien au plus récent : `(quand, octets)`.
    built: VecDeque<(Instant, usize)>,
}

impl Default for Duplications {
    fn default() -> Self {
        Duplications::new(DUPLICATION_BUDGET, DUPLICATION_WINDOW)
    }
}

impl Duplications {
    pub(crate) fn new(budget: usize, window: Duration) -> Duplications {
        Duplications {
            budget,
            window,
            accounts: Mutex::default(),
        }
    }

    fn accounts(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Account>> {
        self.accounts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Admet une duplication de `account`, ou la refuse. La garde rendue tient la place du
    /// compte : la lâcher la rend, et compte ce qui a été bâti ([`Running::built`]).
    pub(crate) fn begin(self: &Arc<Self>, account: Uuid) -> Result<Running, Busy> {
        self.begin_at(account, Instant::now())
    }

    fn begin_at(self: &Arc<Self>, account: Uuid, now: Instant) -> Result<Running, Busy> {
        let mut accounts = self.accounts();
        let entry = accounts.entry(account).or_default();
        if entry.running {
            return Err(Busy::Running);
        }
        forget_old(entry, now, self.window);
        let mut spent: usize = entry.built.iter().map(|(_, bytes)| bytes).sum();
        if spent >= self.budget {
            // Le budget revient quand assez de ce qui a été bâti sort de la fenêtre, le plus
            // ancien d'abord.
            let mut back = now;
            for (at, bytes) in &entry.built {
                spent -= bytes;
                back = *at + self.window;
                if spent < self.budget {
                    break;
                }
            }
            let wait = back.saturating_duration_since(now);
            let retry_after = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
            return Err(Busy::Spent {
                retry_after: retry_after.max(1),
            });
        }
        entry.running = true;
        Ok(Running {
            all: Arc::clone(self),
            account,
            built: AtomicUsize::new(0),
        })
    }

    fn end_at(&self, account: Uuid, built: usize, now: Instant) {
        let mut accounts = self.accounts();
        if let Some(entry) = accounts.get_mut(&account) {
            entry.running = false;
            if built > 0 {
                entry.built.push_back((now, built));
            }
        }
        // Ne restent que les comptes qui ont bâti quelque chose pendant la dernière fenêtre, ou
        // qui dupliquent en ce moment.
        let window = self.window;
        accounts.retain(|_, entry| {
            forget_old(entry, now, window);
            entry.running || !entry.built.is_empty()
        });
    }

    /// Comptes retenus (pour les tests).
    #[cfg(test)]
    fn remembered(&self) -> usize {
        self.accounts().len()
    }
}

/// Oublie ce que le compte a bâti avant la fenêtre.
fn forget_old(entry: &mut Account, now: Instant, window: Duration) {
    while let Some((at, _)) = entry.built.front()
        && now.saturating_duration_since(*at) >= window
    {
        entry.built.pop_front();
    }
}

/// Une duplication admise, en cours : la place de son compte. La lâcher rend la place, et compte
/// au budget du compte ce que la duplication a bâti.
///
/// Elle se partage (`Arc`) entre la requête et chacun de ses calculs (constat N1 de la
/// re-relecture) : un calcul lancé va au bout sur son fil, même si la requête est abandonnée, et
/// la place n'est rendue que quand le dernier des deux l'a lâchée. Sinon une boucle de requêtes
/// abandonnées pendant la construction tenait les places de calcul sans rien dépenser.
pub(crate) struct Running {
    all: Arc<Duplications>,
    account: Uuid,
    built: AtomicUsize,
}

impl Running {
    /// La duplication vient de bâtir `bytes` octets de document.
    pub(crate) fn built(&self, bytes: usize) {
        self.built.fetch_add(bytes, Ordering::Relaxed);
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let built = self.built.load(Ordering::Relaxed);
        self.all.end_at(self.account, built, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    fn after(start: Instant, secs: u64) -> Instant {
        start + Duration::from_secs(secs)
    }

    /// Une duplication à la fois par compte : la seconde est refusée tant que la première tient
    /// sa place, admise dès qu'elle l'a rendue ; un autre compte n'attend rien.
    #[test]
    fn one_duplication_at_a_time_for_an_account() {
        let all = Arc::new(Duplications::new(100, MINUTE));
        let (mona, tom) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let first = all.begin(mona).expect("la première");
        assert_eq!(all.begin(mona).err(), Some(Busy::Running));
        let other = all.begin(tom).expect("un autre compte");
        drop(first);
        let again = all.begin(mona).expect("la place est rendue");
        drop((other, again));
        assert_eq!(all.remembered(), 0, "rien de bâti, rien de retenu");
    }

    /// Le budget : ce qui est bâti dans la fenêtre se compte, écrit ou non ; à la borne, la
    /// duplication suivante est refusée, avec le délai au bout duquel assez de ce qui a été bâti
    /// sort de la fenêtre ; une duplication admise sous la borne va au bout, même si elle la
    /// passe. Le budget d'un compte n'est pas celui d'un autre.
    #[test]
    fn budget_is_spent_by_what_is_built_and_comes_back_with_the_window() {
        let all = Arc::new(Duplications::new(100, MINUTE));
        let (mona, tom) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let start = Instant::now();
        let spend = |account: Uuid, at: u64, bytes: usize| {
            // La garde est oubliée : la place est rendue à la main, à l'heure voulue.
            std::mem::forget(all.begin_at(account, after(start, at))?);
            all.end_at(account, bytes, after(start, at));
            Ok::<(), Busy>(())
        };
        // 40 octets à 0 s, 59 à 10 s : 99, sous la borne ; puis 30 à 20 s : admise, 129.
        spend(mona, 0, 40).unwrap();
        spend(mona, 10, 59).unwrap();
        spend(mona, 20, 30).unwrap();
        // À 21 s : 129. Sans les 40 de 0 s : 89, sous la borne : dans 39 s.
        assert_eq!(
            spend(mona, 21, 1).err(),
            Some(Busy::Spent { retry_after: 39 })
        );
        assert!(spend(tom, 21, 100).is_ok(), "le budget d'un autre compte");
        // Tom, lui, a tout dépensé d'un coup : il attend la fenêtre entière.
        assert_eq!(
            spend(tom, 21, 1).err(),
            Some(Busy::Spent { retry_after: 60 })
        );
        // À 59 s, toujours refusée (une seconde) ; à 60 s, les 40 de 0 s sont sortis : admise.
        assert_eq!(
            spend(mona, 59, 1).err(),
            Some(Busy::Spent { retry_after: 1 })
        );
        spend(mona, 60, 11).unwrap();
        // 59 + 30 + 11 = 100 : à la borne, refusée, jusqu'à ce que les 59 de 10 s sortent.
        assert_eq!(
            spend(mona, 61, 1).err(),
            Some(Busy::Spent { retry_after: 9 })
        );
        // Un refus n'a rien dépensé, et une fenêtre plus tard tout est oublié.
        spend(mona, 200, 0).unwrap();
        all.end_at(tom, 0, after(start, 200));
        assert_eq!(all.remembered(), 0);
    }

    /// Une garde lâchée compte ce qu'elle a bâti, et rend la place.
    #[test]
    fn a_dropped_guard_charges_what_it_built() {
        let all = Arc::new(Duplications::new(10, MINUTE));
        let mona = Uuid::from_u128(1);
        let running = all.begin(mona).unwrap();
        running.built(6);
        running.built(6);
        drop(running);
        assert!(matches!(all.begin(mona), Err(Busy::Spent { retry_after }) if retry_after > 55));
        assert_eq!(all.remembered(), 1);
    }
}
