//! Le débit des appels par jeton d'API, et leur journal d'activité (lot 5, Rulings R1241, R1244).
//!
//! **Débit** ([`Calls::admit`]) : compté en mémoire, par compte, tous ses jetons ensemble, avec
//! un plafond par jeton en plus, sur une minute glissante : un compte fait au plus
//! [`Limits::account_calls`] appels (240 par défaut, `MEMENTUM_ACCOUNT_TOKEN_CALLS_PER_MIN`) et
//! envoie au plus [`ACCOUNT_BODY_BYTES`] octets de corps ; chaque jeton au plus
//! [`Limits::token_calls`] appels (120, `MEMENTUM_TOKEN_CALLS_PER_MIN`) et
//! [`TOKEN_BODY_BYTES`] octets. Le premier plafond atteint refuse (`429 token_busy`), avec le
//! délai juste : quand assez d'appels de la fenêtre en seront sortis. Un appel refusé n'est pas
//! compté. Jugé par la porte des jetons (`auth::token_gate`), avant tout travail de la route.
//!
//! **Journal** ([`Calls::record`], [`Calls::busy`]) : une ligne par appel admis, gardée en
//! mémoire et écrite par paquets ([`BATCH`] lignes au plus, toutes les [`FLUSH_EVERY`] au plus)
//! dans la voie du journal de la file d'écriture (`Store::append_calls`, `Lane::Calls`) ; un
//! refus du débit n'écrit qu'un compte sur le jeton, jamais une ligne. Ni texte cherché, ni MMD,
//! ni contenu (Ruling R1245) : l'outil (la route), la page ou l'espace de son chemin, l'issue,
//! le code d'un refus. Un arrêt brutal perd ce qui n'était pas encore écrit (deux secondes au
//! plus).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mementum_store::{Busy, Call, Store};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use uuid::Uuid;

/// Appels par minute d'un jeton, par défaut (Ruling R1241).
pub const TOKEN_CALLS_PER_MIN: u32 = 120;

/// Appels par minute de tous les jetons d'un compte, par défaut (Ruling R1241).
pub const ACCOUNT_TOKEN_CALLS_PER_MIN: u32 = 240;

/// Octets de corps envoyés par minute par un jeton (Ruling R1241).
pub const TOKEN_BODY_BYTES: u64 = 16 << 20;

/// Octets de corps envoyés par minute par tous les jetons d'un compte (Ruling R1241).
pub const ACCOUNT_BODY_BYTES: u64 = 32 << 20;

/// La fenêtre glissante du débit.
pub const RATE_WINDOW: Duration = Duration::from_secs(60);

/// Lignes du journal écrites en une transaction, au plus (Ruling R1244).
pub const BATCH: usize = 256;

/// Le plus long délai entre l'arrivée d'une ligne et son écriture (Ruling R1244).
pub const FLUSH_EVERY: Duration = Duration::from_secs(2);

/// Lignes gardées pour un nouvel essai quand un paquet n'a pas pu s'écrire, au plus : au-delà,
/// les plus anciennes sont perdues, et le journal du serveur le dit.
const MAX_PENDING: usize = 4 * BATCH;

/// Fenêtres de débit gardées sans être relues avant qu'on retire les vides : la mémoire du débit
/// reste en proportion des jetons qui ont appelé dans la dernière minute.
const SWEEP_AFTER: usize = 1024;

/// Les plafonds du débit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub token_calls: u32,
    pub token_bytes: u64,
    pub account_calls: u32,
    pub account_bytes: u64,
    pub window: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            token_calls: TOKEN_CALLS_PER_MIN,
            token_bytes: TOKEN_BODY_BYTES,
            account_calls: ACCOUNT_TOKEN_CALLS_PER_MIN,
            account_bytes: ACCOUNT_BODY_BYTES,
            window: RATE_WINDOW,
        }
    }
}

/// Les appels admis d'un jeton ou d'un compte dans la fenêtre, du plus ancien au plus récent.
#[derive(Default)]
struct Window {
    calls: VecDeque<(Instant, u64)>,
    bytes: u64,
}

impl Window {
    /// Oublie ce qui est sorti de la fenêtre à `now`.
    fn prune(&mut self, now: Instant, window: Duration) {
        while let Some(&(at, bytes)) = self.calls.front() {
            if now.duration_since(at) < window {
                break;
            }
            self.calls.pop_front();
            self.bytes -= bytes;
        }
    }

    /// Le délai avant qu'un appel de `bytes` octets tienne sous `calls` appels et `max_bytes`
    /// octets ; `None` s'il tient déjà.
    fn wait(
        &self,
        bytes: u64,
        calls: u32,
        max_bytes: u64,
        now: Instant,
        window: Duration,
    ) -> Option<Duration> {
        let too_many = self.calls.len() >= calls as usize;
        let too_heavy = self.bytes + bytes > max_bytes;
        if !too_many && !too_heavy {
            return None;
        }
        // Le premier appel dont la sortie rend la place : en nombre, assez pour descendre sous
        // le plafond ; en octets, assez pour que celui-ci tienne.
        let mut kept = self.calls.len();
        let mut weight = self.bytes;
        for &(at, gone) in &self.calls {
            kept -= 1;
            weight -= gone;
            if kept < calls as usize && weight + bytes <= max_bytes {
                return Some((at + window).saturating_duration_since(now));
            }
        }
        // Plus lourd que le plafond à lui seul : jamais, ce que la borne des corps (2 Mio) évite.
        Some(window)
    }

    fn admit(&mut self, bytes: u64, now: Instant) {
        self.calls.push_back((now, bytes));
        self.bytes += bytes;
    }
}

#[derive(Default)]
struct Rate {
    tokens: HashMap<Uuid, Window>,
    accounts: HashMap<Uuid, Window>,
}

#[derive(Default)]
struct Log {
    lines: Vec<Call>,
    /// Par jeton : appels refusés par le débit depuis le dernier paquet, et l'instant du dernier.
    busy: HashMap<Uuid, (i64, i64)>,
}

/// Le débit et le journal des appels par jeton de l'instance.
pub struct Calls {
    limits: Limits,
    rate: Mutex<Rate>,
    log: Mutex<Log>,
    /// Un paquet plein attend, ou l'arrêt est demandé.
    wake: Notify,
    stopping: std::sync::atomic::AtomicBool,
}

impl Default for Calls {
    fn default() -> Calls {
        Calls::new(Limits::default())
    }
}

/// Un verrou empoisonné ne laisse pas un état faux : rien ne panique entre deux écritures qui
/// doivent aller ensemble.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Calls {
    pub fn new(limits: Limits) -> Calls {
        Calls {
            limits,
            rate: Mutex::default(),
            log: Mutex::default(),
            wake: Notify::new(),
            stopping: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Admet l'appel du jeton `token`, du compte `account`, qui envoie `bytes` octets de corps,
    /// à `now`, ou rend le délai avant qu'il passe : le plus long de ceux du jeton et du compte.
    /// Un appel refusé n'est pas compté.
    pub fn admit(
        &self,
        account: Uuid,
        token: Uuid,
        bytes: u64,
        now: Instant,
    ) -> Result<(), Duration> {
        let limits = self.limits;
        let mut guard = lock(&self.rate);
        let rate = &mut *guard;
        if rate.tokens.len() + rate.accounts.len() > SWEEP_AFTER {
            for windows in [&mut rate.tokens, &mut rate.accounts] {
                windows.retain(|_, window| {
                    window.prune(now, limits.window);
                    !window.calls.is_empty()
                });
            }
        }
        let by_token = rate.tokens.entry(token).or_default();
        by_token.prune(now, limits.window);
        let token_wait = by_token.wait(
            bytes,
            limits.token_calls,
            limits.token_bytes,
            now,
            limits.window,
        );
        let by_account = rate.accounts.entry(account).or_default();
        by_account.prune(now, limits.window);
        let account_wait = by_account.wait(
            bytes,
            limits.account_calls,
            limits.account_bytes,
            now,
            limits.window,
        );
        if let Some(wait) = token_wait.max(account_wait) {
            return Err(wait);
        }
        by_account.admit(bytes, now);
        by_token.admit(bytes, now);
        Ok(())
    }

    /// Une ligne de plus au journal ; un paquet plein réveille l'écriture.
    pub fn record(&self, call: Call) {
        let full = {
            let mut log = lock(&self.log);
            log.lines.push(call);
            log.lines.len() >= BATCH
        };
        if full {
            self.wake.notify_one();
        }
    }

    /// Un appel du jeton refusé par le débit à `at` : un compte, sans ligne (Ruling R1244).
    pub fn busy(&self, token: Uuid, at: i64) {
        let mut log = lock(&self.log);
        let entry = log.busy.entry(token).or_insert((0, at));
        entry.0 += 1;
        entry.1 = entry.1.max(at);
    }

    /// Écrit ce que le journal garde en mémoire, par paquets de [`BATCH`] lignes. Un paquet qui
    /// ne s'écrit pas est gardé pour l'essai suivant, dans la limite de [`MAX_PENDING`] lignes.
    pub async fn flush(&self, store: &Store) {
        loop {
            let (lines, busy) = {
                let mut log = lock(&self.log);
                let take = log.lines.len().min(BATCH);
                let lines: Vec<Call> = log.lines.drain(..take).collect();
                let busy: Vec<Busy> = log
                    .busy
                    .drain()
                    .map(|(token_id, (count, at))| Busy {
                        token_id,
                        count,
                        at,
                    })
                    .collect();
                (lines, busy)
            };
            if lines.is_empty() && busy.is_empty() {
                return;
            }
            if let Err(error) = store.append_calls(&lines, &busy).await {
                let mut log = lock(&self.log);
                let mut kept = lines;
                kept.append(&mut log.lines);
                let lost = kept.len().saturating_sub(MAX_PENDING);
                kept.drain(..lost);
                log.lines = kept;
                for Busy {
                    token_id,
                    count,
                    at,
                } in busy
                {
                    let entry = log.busy.entry(token_id).or_insert((0, at));
                    entry.0 += count;
                    entry.1 = entry.1.max(at);
                }
                tracing::warn!(
                    %error,
                    lost,
                    "journal d'activité des jetons : un paquet n'a pas pu s'écrire ; il sera \
                     réessayé"
                );
                return;
            }
        }
    }

    /// L'écriture du journal en fond : un paquet dès qu'il est plein, sinon toutes les
    /// [`FLUSH_EVERY`] ; à l'arrêt ([`CallsHandle::stop`]), un dernier passage.
    pub(crate) fn spawn_writer(self: &Arc<Calls>, store: Store) -> CallsHandle {
        let calls = Arc::clone(self);
        let writer = Arc::clone(self);
        let task = tokio::spawn(mementum_store::writes_by(None, async move {
            loop {
                let stopping = writer.stopping.load(std::sync::atomic::Ordering::Acquire);
                tokio::select! {
                    () = writer.wake.notified() => {}
                    () = tokio::time::sleep(FLUSH_EVERY) => {}
                }
                writer.flush(&store).await;
                if stopping || writer.stopping.load(std::sync::atomic::Ordering::Acquire) {
                    writer.flush(&store).await;
                    return;
                }
            }
        }));
        CallsHandle {
            calls,
            task: Arc::new(Mutex::new(Some(task))),
        }
    }
}

/// La poignée de l'écriture du journal d'activité : l'arrêter avant de fermer la base.
#[derive(Clone)]
pub struct CallsHandle {
    calls: Arc<Calls>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl CallsHandle {
    /// Écrit tout de suite ce que le journal garde. Pour les tests.
    #[doc(hidden)]
    pub async fn flush(&self, store: &Store) {
        self.calls.flush(store).await;
    }

    /// Arrête l'écriture en fond, après un dernier passage. Sans effet la seconde fois.
    pub async fn stop(&self) {
        let task = lock(&self.task).take();
        let Some(task) = task else { return };
        self.calls
            .stopping
            .store(true, std::sync::atomic::Ordering::Release);
        self.calls.wake.notify_one();
        if let Err(error) = task.await {
            tracing::warn!(%error, "écriture du journal d'activité des jetons interrompue");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    /// Le 121e appel d'un jeton dans la minute est refusé, avec le délai juste : quand le premier
    /// sort de la fenêtre ; il passe à cet instant même.
    #[test]
    fn the_121st_call_of_a_token_in_a_minute_is_refused_with_its_delay() {
        let calls = Calls::default();
        let (account, token) = (Uuid::now_v7(), Uuid::now_v7());
        let base = Instant::now();
        for i in 0..120 {
            assert_eq!(
                calls.admit(account, token, 0, at(base, 1_000 + i * 100)),
                Ok(()),
                "{i}"
            );
        }
        let refused = calls.admit(account, token, 0, at(base, 30_000));
        assert_eq!(refused, Err(Duration::from_millis(31_000)));
        // Refusé, il n'est pas compté : toujours le même délai.
        assert_eq!(calls.admit(account, token, 0, at(base, 30_000)), refused);
        assert_eq!(
            calls.admit(account, token, 0, at(base, 60_999)),
            Err(Duration::from_millis(1))
        );
        assert_eq!(calls.admit(account, token, 0, at(base, 61_000)), Ok(()));
    }

    /// Les jetons d'un compte partagent son débit : le 241e appel de la minute est refusé quel
    /// que soit le jeton, même un jeton qui n'a encore rien fait.
    #[test]
    fn the_tokens_of_one_account_share_its_rate() {
        let calls = Calls::default();
        let account = Uuid::now_v7();
        let tokens = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
        let base = Instant::now();
        // Quatre-vingts appels par jeton : aucun n'atteint son propre plafond.
        for i in 0..240 {
            let token = tokens[(i % 3) as usize];
            assert_eq!(
                calls.admit(account, token, 0, at(base, i * 10)),
                Ok(()),
                "{i}"
            );
        }
        for token in tokens {
            assert_eq!(
                calls.admit(account, token, 0, at(base, 5_000)),
                Err(Duration::from_millis(55_000)),
                "le compte a pris ses 240 appels"
            );
        }
        // Un autre compte n'en souffre pas.
        assert_eq!(
            calls.admit(Uuid::now_v7(), Uuid::now_v7(), 0, at(base, 5_000)),
            Ok(())
        );
    }

    /// Les octets comptent aussi : un jeton envoie au plus 16 Mio de corps par minute, un compte
    /// 32 Mio ; le délai est celui qui libère assez d'octets.
    #[test]
    fn bodies_count_against_the_rate() {
        let calls = Calls::default();
        let (account, token) = (Uuid::now_v7(), Uuid::now_v7());
        let base = Instant::now();
        let mib = 1 << 20;
        for i in 0..8 {
            assert_eq!(
                calls.admit(account, token, 2 * mib, at(base, i * 1_000)),
                Ok(())
            );
        }
        // 16 Mio pris : il faut que les deux premiers appels sortent pour 3 Mio de plus.
        assert_eq!(
            calls.admit(account, token, 3 * mib, at(base, 10_000)),
            Err(Duration::from_millis(51_000))
        );
        // Un second jeton du compte : 16 Mio de plus, puis le plafond du compte.
        let other = Uuid::now_v7();
        for i in 0..8 {
            assert_eq!(
                calls.admit(account, other, 2 * mib, at(base, 10_000 + i)),
                Ok(())
            );
        }
        assert_eq!(
            calls.admit(account, Uuid::now_v7(), mib, at(base, 20_000)),
            Err(Duration::from_millis(40_000))
        );
    }

    /// Un refus du débit n'écrit qu'un compte ; une ligne admise s'écrit au paquet suivant.
    #[tokio::test]
    async fn a_busy_call_is_counted_without_a_line() {
        let calls = Calls::default();
        let token = Uuid::now_v7();
        calls.busy(token, 10);
        calls.busy(token, 30);
        calls.busy(token, 20);
        let log = lock(&calls.log);
        assert!(log.lines.is_empty());
        assert_eq!(log.busy.get(&token), Some(&(3, 30)));
    }
}
