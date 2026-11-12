//! L'historique d'une page, vu de son acteur (Ruling R135) : ce qu'il compte des transactions
//! de son document ([`Watch`]), la tranche en cours ([`Slice`]), et la coupe des versions et
//! la restauration (`impl Actor`).
//!
//! Une version est l'état complet du document de l'acteur, encodé par lui au moment de couper,
//! jamais à chaque frappe ni depuis des octets reçus d'un client. Elle est coupée juste avant
//! l'écriture qui suit de [`crate::SyncConfig::version_every`] la première écriture non versionnée, à
//! l'arrêt de l'acteur s'il en reste, et avant une restauration qui remplace ce qu'aucune
//! version ne garde.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mementum_crdt::{CrdtError, PageDoc};
use mementum_store::{Store, StoreError, VersionReason};
use tokio::time::Instant;
use uuid::Uuid;

use super::{Actor, Broken};
use crate::protocol;
use crate::{PageEvent, SyncError};

/// Ce que l'acteur compte des transactions validées dans son document, d'après
/// l'événement de fin de chacune :
/// - les **suppressions**, en unités d'horloge Yjs (un caractère, un élément
///   ou un attribut chacun), d'après son jeu de suppressions. Sert au
///   déclenchement de la compaction (Ruling R40) : le ramasse-miettes de yrs,
///   qui nettoie ce qui est supprimé, est rejoué à chaque chargement tant que
///   la suppression est au journal ; une grosse suppression coûte alors
///   plusieurs secondes à chaque ouverture de la page ;
/// - les **changements** : les transactions qui ont inséré quelque chose (le
///   vecteur d'état a avancé) ou supprimé quelque chose (yrs n'inscrit au jeu
///   de suppressions que ce qu'elle supprime vraiment). Sert à l'historique
///   (Ruling R135) et au journal (Ruling R181) : une mise à jour qui n'a rien
///   changé (une mise à jour retenue que l'étape 2 couvrait déjà, l'étape 2
///   d'un onglet qui revient sans rien de neuf) n'est ni écrite, ni relayée, ni
///   versionnée. Le compte se lit avant et après une écriture : rien n'est
///   encodé ni relu du document pour le savoir.
#[derive(Default)]
pub(super) struct Watch(Arc<Counters>);

#[derive(Default)]
struct Counters {
    deletions: AtomicU64,
    changes: AtomicU64,
    /// L'observateur n'a pas pu être posé : toute écriture compte alors comme
    /// un changement, pour que rien ne manque au journal.
    blind: AtomicBool,
}

impl Watch {
    /// Compte les transactions de `page` validées désormais.
    pub(super) fn on(page: &PageDoc) -> Watch {
        let watch = Watch::default();
        let counters = watch.0.clone();
        let watched =
            page.yrs_doc()
                .observe_transaction_cleanup("mementum-sync/watch", move |_, event| {
                    let units: u64 = event
                        .delete_set
                        .iter()
                        .flat_map(|(_, ranges)| ranges.iter())
                        .map(|range| u64::from(range.end - range.start))
                        .sum();
                    counters.deletions.fetch_add(units, Ordering::Relaxed);
                    if units > 0 || event.before_state != event.after_state {
                        counters.changes.fetch_add(1, Ordering::Relaxed);
                    }
                });
        if let Err(error) = watched {
            // Sans compteur, la compaction n'a plus lieu que toutes les `snapshot_every` mises à
            // jour (plus lent au chargement, sans perte), et toute écriture compte comme un
            // changement : écrite au journal et relayée, comme avant le Ruling R181.
            tracing::warn!(%error, "transactions non comptées : compaction au rythme ordinaire, toute écriture vue comme un changement");
            watch.0.blind.store(true, Ordering::Relaxed);
        }
        watch
    }

    pub(super) fn deletions(&self) -> u64 {
        self.0.deletions.load(Ordering::Relaxed)
    }

    pub(super) fn reset_deletions(&self) {
        self.0.deletions.store(0, Ordering::Relaxed);
    }

    pub(super) fn changes(&self) -> u64 {
        self.0.changes.load(Ordering::Relaxed)
    }

    /// Une transaction a-t-elle changé le document depuis que le compte valait
    /// `changes` ? Toujours vrai sans observateur.
    pub(super) fn changed_since(&self, changes: u64) -> bool {
        self.0.blind.load(Ordering::Relaxed) || self.changes() != changes
    }
}

/// La tranche en cours de l'historique (Ruling R135) : les écritures qui ont
/// changé la page depuis sa dernière version.
#[derive(Debug, Default)]
pub(super) struct Slice {
    /// L'instant de la première ; `None` : aucune.
    since: Option<Instant>,
    /// Les comptes qui les ont faites : ceux des connexions qui ont écrit, et
    /// celui donné à une écriture du serveur. Une écriture sans compte n'ajoute
    /// personne.
    authors: BTreeSet<Uuid>,
}

impl Slice {
    /// La tranche en cours quand l'acteur démarre, relue au journal : les
    /// écritures au-delà de la dernière version (`Store::unversioned`), pour
    /// ce qu'il en reste après la compaction. C'est ce qu'un arrêt brutal ne
    /// perd pas : le contenu est au journal, seule la version `closing` manque.
    /// Sa date est celle de la plus ancienne écriture connue, ramenée à
    /// l'horloge de l'acteur. Illisible, la tranche est vide : la page est
    /// servie quand même, et la prochaine écriture en ouvre une.
    pub(super) async fn read(store: &Store, node_id: Uuid) -> Slice {
        match store.unversioned(node_id).await {
            Ok(found) => Slice {
                since: found.since.map(|at| {
                    let ago = Duration::from_millis(now_ms().saturating_sub(at).max(0) as u64);
                    Instant::now().checked_sub(ago).unwrap_or_else(Instant::now)
                }),
                authors: found.authors.into_iter().collect(),
            },
            Err(error) => {
                tracing::warn!(node_id = %node_id, %error, "tranche en cours de l'historique illisible : elle repart de zéro");
                Slice::default()
            }
        }
    }
}

/// Millisecondes Unix, comme les dates du store.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Actor {
    /// Une écriture vient d'être appliquée et écrite au journal au nom de
    /// `actor`, `changes` étant le compte des changements d'avant elle : si
    /// elle a changé la page, elle entre dans la tranche en cours (Ruling
    /// R135), qu'elle ouvre au besoin. Une écriture qui n'a rien changé n'y
    /// entre pas : sinon, une page qui n'a pas changé couperait une version.
    /// (Une mise à jour de client sans effet n'arrive même pas ici : elle n'est
    /// pas écrite, Ruling R181.)
    pub(super) fn wrote(&mut self, changes: u64, actor: Option<Uuid>) {
        if !self.watch.changed_since(changes) {
            return;
        }
        self.slice.since.get_or_insert_with(Instant::now);
        self.slice.authors.extend(actor);
    }

    /// Ruling R135 : juste avant d'appliquer une écriture, une version
    /// `periodic` est coupée si la première écriture non versionnée a au moins
    /// [`crate::SyncConfig::version_every`]. Elle porte donc l'état d'avant cette
    /// écriture, et la tranche suivante commence avec elle.
    ///
    /// L'état n'est encodé qu'ici, au plus une fois par intervalle : une
    /// frappe ordinaire ne paie qu'une comparaison d'instants. Une version que
    /// le store refuse est journalisée, et réessayée après un nouvel
    /// intervalle, pas à chaque frappe : l'écriture passe quand même, et la
    /// tranche garde ses auteurs.
    pub(super) async fn version_if_due(&mut self) {
        let Some(since) = self.slice.since else {
            return;
        };
        if since.elapsed() < self.cfg.version_every {
            return;
        }
        if let Err(error) = self.cut_version(VersionReason::Periodic).await {
            tracing::warn!(node_id = %self.id.node_id, %error, "version impossible à écrire : réessai après un autre intervalle");
            self.slice.since = Some(Instant::now());
        }
    }

    /// Coupe une version : l'état complet du document vivant, encodé ici (jamais
    /// des octets reçus d'un client), le `seq` du journal qu'il couvre (entre
    /// deux commandes, le document est exactement ce que le journal a écrit) et
    /// les comptes de la tranche. Une fois écrite, la tranche est vide, et les
    /// connexions de la page reçoivent la trame `versions` (Ruling R136).
    async fn cut_version(&mut self, reason: VersionReason) -> Result<(), StoreError> {
        let state = self.page.encode_state();
        let authors: Vec<Uuid> = self.slice.authors.iter().copied().collect();
        self.store
            .put_version(self.id.node_id, self.last_seq, &state, &authors, reason)
            .await?;
        self.slice = Slice::default();
        self.broadcast(None, protocol::page_event(PageEvent::Versions));
        Ok(())
    }

    /// À l'arrêt de l'acteur : la version `closing`, s'il reste des écritures non
    /// versionnées. Si le store la refuse, l'arrêt se fait quand même : le
    /// contenu est au journal, et l'acteur suivant y relit la tranche
    /// ([`Slice::read`]).
    pub(super) async fn cut_closing(&mut self) {
        if self.slice.since.is_none() {
            return;
        }
        if let Err(error) = self.cut_version(VersionReason::Closing).await {
            tracing::error!(node_id = %self.id.node_id, %error, "version de fin impossible à écrire : l'acteur s'arrête quand même, le contenu est au journal");
        }
    }

    /// Ruling R135 : restaure la version `version`, au nom de `actor`. Ses blocs
    /// sont lus dans un document à part, bâti depuis l'état gardé, puis écrits
    /// par-dessus le contenu vivant par une écriture ordinaire ([`Actor::write`] :
    /// journal, diffusion, compaction), avec leurs identifiants : les fils de
    /// commentaires retrouvent leurs blocs. Juste avant, dans la même commande
    /// (rien ne passe entre les deux), une version `before_restore` garde ce
    /// qui est remplacé ; si le store la refuse, rien n'est restauré.
    ///
    /// Une version qui n'est pas de cette page, ou qui n'existe pas :
    /// `NotFound`. Un état gardé que la page ne relit pas (base abîmée) : une
    /// erreur, sans toucher au document. Restaurer ce que la page a déjà comme
    /// contenu ne fait rien : ni écriture, ni version (Ruling R180).
    pub(super) async fn restore(
        &mut self,
        version: Uuid,
        actor: Option<Uuid>,
    ) -> Result<Result<(), SyncError>, Broken> {
        let state = match self.store.get_version(self.id.node_id, version).await {
            Ok(Some(state)) => state,
            Ok(None) => return Ok(Err(SyncError::NotFound)),
            Err(error) => return Ok(Err(error.into())),
        };
        let blocks = match PageDoc::from_updates([state.as_slice()]) {
            Ok(kept) => kept.read_blocks(),
            Err(error) => {
                tracing::error!(node_id = %self.id.node_id, version_id = %version, %error, "état d'une version illisible : restauration refusée, page intacte");
                let unreadable = format!("état de la version {version} : {error}");
                return Ok(Err(SyncError::Crdt(CrdtError::Decode(unreadable))));
            }
        };
        if blocks == self.page.read_blocks() {
            return Ok(Ok(()));
        }
        if let Err(error) = self.cut_version(VersionReason::BeforeRestore).await {
            tracing::warn!(node_id = %self.id.node_id, version_id = %version, %error, "version d'avant restauration impossible à écrire : rien n'est restauré");
            return Ok(Err(error.into()));
        }
        self.write(blocks, actor).await
    }
}
