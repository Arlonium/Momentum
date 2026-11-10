//! Les données dont se calculent les accès d'un espace (`AccessInputs` : ses nœuds, ses lignes
//! d'accès, ses rôles), gardées en mémoire et partagées entre les personnes. C'est le premier
//! étage du cache des droits, que l'en-tête d'`access.rs` décrit en entier ; le second, le
//! calcul de chaque personne, est `mementum_acl::AccessCache`.
//!
//! **À qui des données gardées sont rendues** (Ruling R98). Une requête lit la révision
//! courante de l'espace, `r`, puis demande ses données ([`InputsCache::get_or_load`]). Des
//! données gardées lui sont rendues si leur révision est **supérieure ou égale** à `r`, qu'elle
//! les trouve en arrivant ou après avoir attendu la lecture d'une autre requête. Des données de
//! révision inférieure à `r` ne lui sont jamais rendues : elle les relit dans la base.
//!
//! Cela repose sur une hypothèse : **`spaces.rev` ne fait que croître**. Le store ne l'écrit
//! que par `rev = rev + 1`, dans la transaction de l'écriture qu'elle date (`bump_rev`, dans
//! `mementum-store`), et rien de ce que portent ces données ne change sans elle (la règle de
//! `rev`). Alors :
//! - des données de révision `r` sont l'état de l'espace que la requête vient de lire comme
//!   courant ;
//! - des données de révision supérieure sont un état que l'espace a pris **après** cette
//!   lecture (quand elle a lu `r`, leur écriture n'était pas validée) et avant que la requête
//!   ne les reçoive : ce qu'elle aurait vu en arrivant un instant plus tard. Les refuser ne
//!   la protégerait de rien : elle relirait la base, pour y trouver cet état ou un plus
//!   récent. Et sous des écritures plus rapprochées qu'une lecture, ces relectures passent
//!   une à une par le tour de lecture : leurs attentes s'additionnent ;
//! - des données de révision inférieure sont un état que la requête sait dépassé : un droit
//!   retiré depuis y figure encore.
//!
//! Le calcul tiré de données est rangé sous **leur** révision, jamais sous `r` (`access.rs`) :
//! il n'est ensuite rendu qu'à qui lit cette révision-là comme courante.
//!
//! `/sync` tient par la même règle (`routes/sync.rs`, Ruling R71). La route lit `r`, puis les
//! droits, et inscrit la connexion avec `r` ; le hub relit alors la révision, et ferme la
//! connexion si elle n'est plus `r`. Des droits tirés de données de révision `r + 1` n'y
//! changent rien : la relecture trouve `r + 1` ou plus, qui n'est pas `r`, et la connexion est
//! fermée ; elle revient. Une fermeture de trop, jamais un droit périmé. Ce qui casserait ce
//! raisonnement, ce sont des droits tirés de données **plus anciennes** que `r` : la relecture
//! pourrait trouver `r`, et laisser vivre des droits d'avant.
//!
//! Si l'hypothèse tombait (une révision qui recule, écrite à la main dans la base), ce n'est
//! pas cette règle seule qui serait fausse, mais tout le cache : une révision déjà vue
//! désignerait un autre état de l'espace.

use std::collections::HashMap;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mementum_acl::{Grant, Role, TreeNode};
use mementum_store::{AccessInputs, StoreError};
use uuid::Uuid;

/// Plafond du cache des données des espaces ([`InputsCache`]), en unités : une par nœud, par
/// ligne d'accès et par rôle gardés, plus le coût fixe de chaque espace
/// ([`INPUTS_FIXED_COST`]). C'est l'unité du cache des accès (Ruling R59) ; elle pèse ici 34
/// octets au plus, soit 17 Mo environ, cache plein.
///
/// Un espace de 10 000 pages et d'une dizaine de membres pèse 10 000 unités environ si ses
/// pages ne portent aucune ligne d'accès, 20 000 si chacune en porte une : le cache en tient
/// quarante-neuf dans le premier cas, vingt-quatre dans le second. Ces données ne servent
/// qu'entre une écriture et la requête suivante de chacun.
pub(crate) const INPUTS_CACHE_UNITS: usize = 500_000;

/// Ce que les données d'un espace comptent dans le plafond en plus de leurs lignes : leur coût
/// fixe (leur case dans la table, leur `Arc`, leurs trois `Vec`, le tour de lecture : 250 octets
/// environ).
const INPUTS_FIXED_COST: usize = 8;

// Ce que pèse une unité, au plus : moins qu'une unité du cache des accès (une quarantaine
// d'octets).
const _: () = {
    assert!(size_of::<TreeNode>() <= 34);
    assert!(size_of::<(Uuid, Grant)>() <= 34);
    assert!(size_of::<(Uuid, Role)>() <= 34);
};

/// Ce que comptent les données d'un espace dans le plafond : leurs nœuds, leurs lignes d'accès
/// et leurs rôles, plus leur coût fixe.
fn weight(inputs: &AccessInputs) -> usize {
    inputs.nodes.len() + inputs.grants.len() + inputs.roles.len() + INPUTS_FIXED_COST
}

/// Les données d'accès des espaces (`AccessInputs`), chacune avec la révision à laquelle elle
/// a été lue, partagées entre les personnes : après une écriture dans un espace, ses données
/// sont lues une fois, et chaque personne refait son calcul en mémoire.
///
/// - Une seule révision par espace : les dernières données lues remplacent les précédentes.
/// - Elles sont rendues à qui a lu leur révision ou une révision plus ancienne, jamais à qui
///   a lu une révision plus récente ([`InputsCache::get_or_load`] ; l'argument est en tête du
///   module).
/// - Une lecture à la fois par espace : qui arrive pendant une lecture l'attend au lieu de la
///   refaire, puis prend ce qu'elle a lu, si ce n'est pas plus ancien que sa révision. Les
///   autres espaces n'attendent pas : le verrou de la table n'est jamais tenu pendant une
///   lecture de la base.
/// - Le cache est borné en unités ([`weight`]), comme celui des accès. Quand des données n'y
///   tiennent pas, celles des espaces les plus anciennement servis sortent d'abord ; des
///   données plus grosses que le plafond, à elles seules, ne sont pas gardées (chacun les relit).
pub(crate) struct InputsCache {
    max_units: usize,
    spaces: Mutex<Spaces>,
    /// Le nombre de lectures de la base demandées, pour les tests.
    #[cfg(test)]
    loads: AtomicUsize,
}

/// Le contenu du cache, sous un seul verrou.
#[derive(Default)]
struct Spaces {
    /// Une case par espace qui a des données gardées, ou une lecture en cours ou attendue.
    map: HashMap<Uuid, Slot>,
    /// La somme des poids ([`weight`]) des données gardées, en unités.
    units: usize,
    /// L'heure du dernier service : avance d'un cran chaque fois que des données sont servies
    /// ou gardées.
    clock: u64,
}

/// La case d'un espace.
#[derive(Default)]
struct Slot {
    /// Le tour de lecture de l'espace : qui le tient lit la base, les autres attendent.
    gate: Arc<tokio::sync::Mutex<()>>,
    /// Les requêtes qui tiennent ou attendent le tour ([`Turn`]).
    waiting: usize,
    /// Les données gardées, et l'heure ([`Spaces::clock`]) de leur dernier service.
    kept: Option<(Arc<AccessInputs>, u64)>,
}

impl Spaces {
    /// Cesse de garder les données de l'espace ; sa case part avec elles si personne n'y
    /// attend son tour.
    fn drop_kept(&mut self, space: Uuid) {
        let Some(slot) = self.map.get_mut(&space) else {
            return;
        };
        if let Some((old, _)) = slot.kept.take() {
            self.units -= weight(&old);
        }
        if slot.waiting == 0 {
            self.map.remove(&space);
        }
    }
}

/// Une requête dans la file du tour de lecture d'un espace. Tant qu'elle vit, la case de
/// l'espace reste ; à sa fin (requête abandonnée comprise), elle en sort.
struct Turn<'a> {
    cache: &'a InputsCache,
    space: Uuid,
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let mut spaces = self.cache.lock();
        if let Some(slot) = spaces.map.get_mut(&self.space) {
            slot.waiting -= 1;
            if slot.waiting == 0 && slot.kept.is_none() {
                spaces.map.remove(&self.space);
            }
        }
    }
}

/// Le tour de lecture d'un espace, tenu par un test : tant qu'il vit, aucune requête ne lit
/// les données de cet espace ; celles qui en ont besoin attendent.
#[cfg(test)]
pub(crate) struct HeldTurn<'a> {
    // Dans cet ordre : le tour est rendu, puis la place dans la file.
    _reading: tokio::sync::OwnedMutexGuard<()>,
    _turn: Turn<'a>,
}

impl InputsCache {
    /// Un cache qui garde au plus `max_units` unités, tous espaces confondus.
    pub(crate) fn new(max_units: usize) -> InputsCache {
        InputsCache {
            max_units,
            spaces: Mutex::new(Spaces::default()),
            #[cfg(test)]
            loads: AtomicUsize::new(0),
        }
    }

    /// Les données de l'espace, pour une requête qui vient de lire `rev` comme sa révision
    /// courante : celles du cache si leur révision est au moins `rev` (Ruling R98, en tête du
    /// module) ; sinon celles que lit `load` (`Store::access_inputs`), gardées sous leur
    /// propre révision, qui est plus récente que `rev` si une écriture est passée entre-temps.
    ///
    /// Une seule lecture à la fois par espace : qui arrive pendant une lecture attend, puis
    /// prend ce qu'elle a gardé si c'est au moins sa révision, et lit à son tour sinon.
    /// `None` : l'espace n'existe pas. Si la lecture échoue, l'erreur remonte et rien n'est
    /// gardé ; qui attendait lit à son tour.
    ///
    /// Ce que lit `load` est rendu sans que sa révision soit regardée : une lecture de la base
    /// part du dernier commit, et ne rapporte donc rien de plus ancien que `rev` tant que la
    /// révision ne fait que croître. C'est l'appelant qui s'en assure (`find_space_access`).
    pub(crate) async fn get_or_load<F, Fut>(
        &self,
        space: Uuid,
        rev: i64,
        load: F,
    ) -> Result<Option<Arc<AccessInputs>>, StoreError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Option<AccessInputs>, StoreError>>,
    {
        if let Some(inputs) = self.kept_at_least(space, rev) {
            return Ok(Some(inputs));
        }
        let turn = self.turn(space);
        let _reading = turn.gate.lock().await;
        // La lecture qu'on vient d'attendre a pu garder ce qu'il faut.
        if let Some(inputs) = self.kept_at_least(space, rev) {
            return Ok(Some(inputs));
        }
        #[cfg(test)]
        self.loads.fetch_add(1, Ordering::Relaxed);
        let Some(inputs) = load().await? else {
            return Ok(None);
        };
        let inputs = Arc::new(inputs);
        self.keep(space, Arc::clone(&inputs));
        Ok(Some(inputs))
    }

    /// Les données gardées pour l'espace, si leur révision est au moins `rev` : jamais des
    /// données plus anciennes. Les servir en fait les plus récemment servies.
    fn kept_at_least(&self, space: Uuid, rev: i64) -> Option<Arc<AccessInputs>> {
        let mut spaces = self.lock();
        let spaces = &mut *spaces;
        let (inputs, served) = spaces.map.get_mut(&space)?.kept.as_mut()?;
        if inputs.rev < rev {
            return None;
        }
        spaces.clock += 1;
        *served = spaces.clock;
        Some(Arc::clone(inputs))
    }

    /// Prend place dans la file du tour de lecture de l'espace.
    fn turn(&self, space: Uuid) -> Turn<'_> {
        let mut spaces = self.lock();
        let slot = spaces.map.entry(space).or_default();
        slot.waiting += 1;
        Turn {
            cache: self,
            space,
            gate: Arc::clone(&slot.gate),
        }
    }

    /// Garde `inputs`, qu'on vient de lire, à la place de ce qui était gardé pour l'espace.
    /// S'il ne tient pas dans ce qui reste, les données des espaces les plus anciennement
    /// servis sortent d'abord ; s'il est à lui seul plus gros que le plafond, il n'est pas
    /// gardé (les données d'avant partent quand même : elles sont d'une autre révision).
    fn keep(&self, space: Uuid, inputs: Arc<AccessInputs>) {
        let added = weight(&inputs);
        let mut spaces = self.lock();
        let spaces = &mut *spaces;
        spaces.drop_kept(space);
        if added > self.max_units {
            return;
        }
        while spaces.units + added > self.max_units {
            let oldest = spaces
                .map
                .iter()
                .filter_map(|(id, slot)| Some((slot.kept.as_ref()?.1, *id)))
                .min();
            let Some((_, oldest)) = oldest else {
                break;
            };
            spaces.drop_kept(oldest);
        }
        spaces.clock += 1;
        spaces.map.entry(space).or_default().kept = Some((inputs, spaces.clock));
        spaces.units += added;
    }

    /// Un verrou empoisonné (panique pendant un autre appel) ne laisse pas la table dans un
    /// état faux : rien ne panique entre deux écritures qui doivent aller ensemble.
    fn lock(&self) -> MutexGuard<'_, Spaces> {
        self.spaces.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Prend le tour de lecture de l'espace, comme une requête qui va lire, et le tient
    /// jusqu'à la fin de ce qu'il rend : pour qu'un test décide de ce qui se passe pendant que
    /// des requêtes attendent.
    #[cfg(test)]
    pub(crate) async fn hold_turn(&self, space: Uuid) -> HeldTurn<'_> {
        let turn = self.turn(space);
        let reading = Arc::clone(&turn.gate).lock_owned().await;
        HeldTurn {
            _reading: reading,
            _turn: turn,
        }
    }

    /// Le nombre de lectures de la base demandées depuis la création du cache.
    #[cfg(test)]
    pub(crate) fn loads(&self) -> usize {
        self.loads.load(Ordering::Relaxed)
    }

    /// Les données gardées pour l'espace, quelle que soit leur révision, sans les servir.
    #[cfg(test)]
    pub(crate) fn kept(&self, space: Uuid) -> Option<Arc<AccessInputs>> {
        let spaces = self.lock();
        let (inputs, _) = spaces.map.get(&space)?.kept.as_ref()?;
        Some(Arc::clone(inputs))
    }

    /// La somme des poids des données gardées, en unités.
    #[cfg(test)]
    pub(crate) fn kept_units(&self) -> usize {
        self.lock().units
    }

    /// Le nombre de requêtes qui tiennent ou attendent le tour de lecture de l'espace.
    #[cfg(test)]
    pub(crate) fn waiting(&self, space: Uuid) -> usize {
        self.lock().map.get(&space).map_or(0, |slot| slot.waiting)
    }

    /// Le nombre de cases de la table.
    #[cfg(test)]
    pub(crate) fn slots(&self) -> usize {
        self.lock().map.len()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Duration;

    use super::*;

    /// Attend que `ready` soit vrai : dix secondes au plus.
    pub(crate) async fn wait_until(what: &str, ready: impl Fn() -> bool) {
        for _ in 0..2000 {
            if ready() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("jamais arrivé : {what}");
    }

    /// Des données d'accès à la révision `rev`, de `nodes` nœuds sans lignes ni rôles.
    fn inputs(rev: i64, nodes: usize) -> AccessInputs {
        let node = TreeNode {
            id: Uuid::nil(),
            parent_id: None,
            trashed: false,
        };
        AccessInputs {
            rev,
            nodes: vec![node; nodes],
            grants: Vec::new(),
            roles: Vec::new(),
        }
    }

    /// Qui attendait une lecture qui échoue lit à son tour : il ne reçoit ni l'erreur d'un
    /// autre, ni une donnée absente.
    #[tokio::test]
    async fn waiting_request_reads_after_a_failed_read() {
        let cache = InputsCache::new(100);
        let space = Uuid::now_v7();
        let fail = tokio::sync::Notify::new();
        let first = cache.get_or_load(space, 3, || async {
            fail.notified().await;
            Err(StoreError::Sqlx(sqlx::Error::PoolTimedOut))
        });
        let second = async {
            wait_until("la première lecture est en cours", || {
                cache.waiting(space) == 1
            })
            .await;
            cache
                .get_or_load(space, 3, || async { Ok(Some(inputs(3, 2))) })
                .await
        };
        let failure = async {
            wait_until("la seconde requête attend", || cache.waiting(space) == 2).await;
            fail.notify_one();
        };
        let (first, second, ()) = tokio::join!(first, second, failure);

        assert!(matches!(first, Err(StoreError::Sqlx(_))));
        assert_eq!(second.unwrap().as_deref(), Some(&inputs(3, 2)));
        assert_eq!(cache.loads(), 2);
        assert_eq!(cache.kept(space).as_deref(), Some(&inputs(3, 2)));
    }

    /// Des données gardées servent qui a lu leur révision, ou une révision plus ancienne (Ruling
    /// R98) ; qui a lu une révision plus récente les relit. Ce qui est lu est gardé sous sa
    /// propre révision, quelle que soit celle qui était demandée.
    #[tokio::test]
    async fn kept_inputs_serve_their_revision_and_older_ones() {
        let cache = InputsCache::new(100);
        let space = Uuid::now_v7();
        let read = |rev: i64, found: i64| {
            cache.get_or_load(
                space,
                rev,
                move || async move { Ok(Some(inputs(found, 1))) },
            )
        };

        // Demandées à la révision 4, lues à la 5 : rendues, et gardées sous la 5.
        assert_eq!(read(4, 5).await.unwrap().map(|i| i.rev), Some(5));
        assert_eq!(cache.loads(), 1);
        // À la révision 5, ou à une révision plus ancienne : servies, sans lecture.
        assert_eq!(read(5, 9).await.unwrap().map(|i| i.rev), Some(5));
        assert_eq!(read(4, 9).await.unwrap().map(|i| i.rev), Some(5));
        assert_eq!(read(1, 9).await.unwrap().map(|i| i.rev), Some(5));
        assert_eq!(cache.loads(), 1);
        // À une révision plus récente : jamais celles-là. Elles sont relues.
        assert_eq!(read(6, 7).await.unwrap().map(|i| i.rev), Some(7));
        assert_eq!(cache.loads(), 2);
        assert_eq!(cache.kept(space).map(|i| i.rev), Some(7));
    }

    /// Une lecture est en cours, qui rapporte des données à la révision `found`. Une requête
    /// qui a lu la révision `rev` arrive pendant ce temps, et attend son tour ; si elle doit
    /// lire à son tour, elle trouve des données à sa révision. Rend la révision des données
    /// qu'elle reçoit, et le nombre de lectures faites en tout.
    async fn wait_behind_a_read(found: i64, rev: i64) -> (i64, usize) {
        let cache = InputsCache::new(100);
        let space = Uuid::now_v7();
        let done = tokio::sync::Notify::new();
        let reader = cache.get_or_load(space, found, || async {
            done.notified().await;
            Ok(Some(inputs(found, 1)))
        });
        let waiter = async {
            wait_until("la lecture est en cours", || cache.waiting(space) == 1).await;
            cache
                .get_or_load(space, rev, || async { Ok(Some(inputs(rev, 1))) })
                .await
        };
        let release = async {
            wait_until("la requête attend son tour", || cache.waiting(space) == 2).await;
            done.notify_one();
        };
        let (read, waited, ()) = tokio::join!(reader, waiter, release);
        assert_eq!(read.unwrap().map(|i| i.rev), Some(found));
        (waited.unwrap().expect("des données").rev, cache.loads())
    }

    /// Après l'attente de son tour (Ruling R98), une requête prend ce que la lecture attendue
    /// a gardé si c'est au moins sa révision, sans relire ; si c'est plus ancien, elle relit.
    #[tokio::test]
    async fn after_its_turn_a_request_takes_newer_inputs_and_rereads_older_ones() {
        // La révision qu'elle a lue : prise, une seule lecture.
        assert_eq!(wait_behind_a_read(5, 5).await, (5, 1));
        // Une révision plus récente, une écriture étant passée : prise aussi.
        assert_eq!(wait_behind_a_read(6, 5).await, (6, 1));
        // Une révision plus ancienne : jamais. Elle relit.
        assert_eq!(wait_behind_a_read(4, 5).await, (5, 2));
    }
}
