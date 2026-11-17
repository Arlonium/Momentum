//! Le frein des mises à jour refusées après leur application (Ruling R725).
//!
//! Une mise à jour refusée après avoir été appliquée fait recharger la page du store (Rulings
//! R17, R22, R538) : 85 ms pour une page de 10 000 blocs, pendant lesquelles son acteur ne sert
//! personne. Un compte qui en envoyait en boucle soutenait sept rechargements par seconde par
//! connexion, et la frappe d'un autre sortait de son budget (296 ms au p95 pour quatre boucles,
//! contre 150 ; mesure `keystroke_during_loops_of_refused_updates_under_150ms`).
//!
//! Le frein compte ces refus par compte, pour toutes les pages du hub. Les [`FREE_REFUSALS`]
//! premiers d'une minute ne coûtent rien de plus (un onglet honnête n'en reçoit pas ; une
//! course rare en donnerait un). Le suivant freine le compte : pendant un temps, ses mises à
//! jour ferment leur connexion sans être appliquées, donc sans rien recharger. Le temps part de
//! [`SyncConfig::refusal_block`](crate::SyncConfig::refusal_block) et double à chaque refus qui
//! suit, jusqu'à [`MAX_BLOCK`] ; le compte est oublié après [`FORGET_AFTER`] sans refus.
//!
//! Rien n'est perdu pour un onglet honnête du compte : y-websocket se reconnecte, et son étape 2
//! renvoie ce que le serveur n'a pas. Une connexion sans compte (les tests du hub) n'est pas
//! freinée.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tokio::time::Instant;
use uuid::Uuid;

/// Les refus d'après l'application qu'un compte peut recevoir en [`WINDOW`] sans être freiné.
pub const FREE_REFUSALS: usize = 3;

/// La fenêtre où se comptent les refus d'un compte.
const WINDOW: Duration = Duration::from_secs(60);

/// Le temps le plus long pendant lequel un compte est freiné.
pub const MAX_BLOCK: Duration = Duration::from_secs(5 * 60);

/// Un compte sans refus depuis ce délai est oublié : il repart de zéro.
const FORGET_AFTER: Duration = Duration::from_secs(10 * 60);

/// Au plus tant de comptes retenus ; au-delà, ceux qui ne sont plus freinés partent d'abord.
const MAX_ACCOUNTS: usize = 4_096;

#[derive(Default)]
struct Account {
    /// Les refus des [`WINDOW`] dernières secondes.
    recent: Vec<Instant>,
    /// Combien de fois le compte a été freiné depuis qu'on le retient.
    blocks: u32,
    /// Freiné jusque-là.
    until: Option<Instant>,
}

/// Le frein d'un hub.
pub(crate) struct Brake {
    first_block: Duration,
    accounts: Mutex<HashMap<Uuid, Account>>,
}

impl Brake {
    pub(crate) fn new(first_block: Duration) -> Brake {
        Brake {
            first_block,
            accounts: Mutex::new(HashMap::new()),
        }
    }

    fn accounts(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Account>> {
        self.accounts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Le temps pendant lequel le compte est encore freiné, s'il l'est.
    pub(crate) fn blocked(&self, account: Uuid, now: Instant) -> Option<Duration> {
        let accounts = self.accounts();
        let until = accounts.get(&account)?.until?;
        (until > now).then(|| until - now)
    }

    /// Un refus d'après l'application de plus pour ce compte. Rend le temps pendant lequel il
    /// est freiné, si ce refus le freine.
    pub(crate) fn refused(&self, account: Uuid, now: Instant) -> Option<Duration> {
        let mut accounts = self.accounts();
        if accounts.len() >= MAX_ACCOUNTS && !accounts.contains_key(&account) {
            accounts.retain(|_, a| a.until.is_some_and(|until| until > now));
            if accounts.len() >= MAX_ACCOUNTS {
                accounts.clear();
            }
        }
        let entry = accounts.entry(account).or_default();
        if entry
            .recent
            .last()
            .is_some_and(|last| now.saturating_duration_since(*last) >= FORGET_AFTER)
        {
            *entry = Account::default();
        }
        entry
            .recent
            .retain(|at| now.saturating_duration_since(*at) < WINDOW);
        entry.recent.push(now);
        if entry.blocks == 0 && entry.recent.len() <= FREE_REFUSALS {
            return None;
        }
        // Déjà freiné une fois : chaque refus qui suit freine de nouveau, deux fois plus.
        let block = self
            .first_block
            .saturating_mul(1u32.checked_shl(entry.blocks).unwrap_or(u32::MAX))
            .min(MAX_BLOCK);
        entry.blocks = entry.blocks.saturating_add(1);
        entry.until = Some(now + block);
        // Un seul refus retenu suffit à dire quand le compte a fauté pour la dernière fois.
        entry.recent.drain(..entry.recent.len() - 1);
        Some(block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: Duration = Duration::from_secs(2);

    #[tokio::test]
    async fn the_first_refusals_of_a_minute_are_free_then_each_one_doubles_the_block() {
        let brake = Brake::new(FIRST);
        let (account, other) = (Uuid::now_v7(), Uuid::now_v7());
        let start = Instant::now();
        for n in 0..FREE_REFUSALS {
            assert_eq!(
                brake.refused(account, start + Duration::from_secs(n as u64)),
                None
            );
        }
        assert_eq!(brake.blocked(account, start + Duration::from_secs(3)), None);
        let at = start + Duration::from_secs(4);
        assert_eq!(brake.refused(account, at), Some(FIRST));
        assert_eq!(
            brake.blocked(account, at + Duration::from_secs(1)),
            Some(Duration::from_secs(1))
        );
        assert_eq!(brake.blocked(account, at + FIRST), None);
        assert_eq!(
            brake.blocked(other, at),
            None,
            "un autre compte n'est pas freiné"
        );
        // Chaque refus qui suit freine de nouveau, deux fois plus, jusqu'au plafond.
        let mut at = at + FIRST;
        let mut expected = FIRST * 2;
        for _ in 0..12 {
            assert_eq!(brake.refused(account, at), Some(expected.min(MAX_BLOCK)));
            at += expected.min(MAX_BLOCK);
            expected = (expected * 2).min(MAX_BLOCK * 2);
        }
        assert_eq!(brake.refused(account, at), Some(MAX_BLOCK));
    }

    #[tokio::test]
    async fn refusals_spread_over_more_than_a_minute_are_never_blocked() {
        let brake = Brake::new(FIRST);
        let account = Uuid::now_v7();
        let start = Instant::now();
        for n in 0..20u64 {
            let at = start + Duration::from_secs(n * 25);
            assert_eq!(brake.refused(account, at), None, "refus {n}");
        }
    }

    #[tokio::test]
    async fn an_account_is_forgotten_after_ten_minutes_without_a_refusal() {
        let brake = Brake::new(FIRST);
        let account = Uuid::now_v7();
        let start = Instant::now();
        for _ in 0..FREE_REFUSALS {
            assert_eq!(brake.refused(account, start), None);
        }
        assert_eq!(brake.refused(account, start), Some(FIRST));
        // Avant l'oubli : freiné dès le premier refus, deux fois plus.
        let later = start + FORGET_AFTER - Duration::from_secs(1);
        assert_eq!(brake.refused(account, later), Some(FIRST * 2));
        // Après : il repart de zéro.
        let forgotten = later + FORGET_AFTER;
        for _ in 0..FREE_REFUSALS {
            assert_eq!(brake.refused(account, forgotten), None);
        }
        assert_eq!(brake.refused(account, forgotten), Some(FIRST));
    }

    #[tokio::test]
    async fn the_table_of_accounts_is_bounded() {
        let brake = Brake::new(FIRST);
        let now = Instant::now();
        for _ in 0..MAX_ACCOUNTS + 10 {
            brake.refused(Uuid::now_v7(), now);
        }
        assert!(brake.accounts().len() <= MAX_ACCOUNTS);
    }
}
