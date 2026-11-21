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

use mementum_crdt::PageDoc;
use mementum_store::{Store, StoreError, Unversioned, VersionReason, Writer};
use tokio::time::Instant;
use uuid::Uuid;

use super::{Actor, Broken};
use crate::long_work::off_the_engine;
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

/// Ce qu'une écriture du serveur fait de l'historique.
#[derive(Debug, Clone, Copy)]
pub(super) enum History {
    /// Si elle change la page, elle entre dans la tranche en cours : `PUT /mmd`,
    /// ou une restauration dont la version partirait à la prochaine purge.
    Slice,
    /// Son contenu est celui de cette version gardée (une restauration) : elle
    /// n'ouvre pas de tranche, et le store retient, dans la transaction qui
    /// l'écrit au journal, que la version vaut jusqu'à ce `seq` (Ruling R197).
    Kept(Uuid),
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
    /// Ceux d'entre eux qui ont écrit par un jeton d'API (Ruling R1234) : la version les garde
    /// à part (« par un agent »).
    agent_authors: BTreeSet<Uuid>,
    /// Les jetons par lesquels ces écritures ont été faites (Ruling R1234).
    tokens: BTreeSet<Uuid>,
    /// Une de ces écritures n'est pas d'un jeton (une connexion de synchro, une session, le
    /// serveur), ou peut ne pas l'être : une part de la tranche relue au démarrage que la
    /// compaction a purgée (Ruling R1234, règle 2 : « au doute, couper »).
    others: bool,
    /// Le contenu du document est celui d'une version gardée : la dernière
    /// coupée, ou la dernière restaurée (Ruling R190). Faux tant qu'une écriture
    /// l'a changé depuis, et pour une page qu'aucune version ne garde encore
    /// (créée avec son contenu, jamais versionnée).
    kept: bool,
}

impl Slice {
    /// La tranche qui suit une version que l'acteur confie à une tâche (Ruling R1572) : vide,
    /// mais son contenu n'est tenu pour gardé qu'une fois la version écrite
    /// ([`Actor::versioning_finished`]) ; d'ici là, une coupe d'avant restauration, par
    /// exemple, garde ce qu'elle remplace, au cas où la version serait refusée.
    pub(super) fn after_a_version_in_flight() -> Slice {
        Slice::default()
    }

    /// Ruling R1234, règle 1 (R1278) : quelqu'un d'autre que le jeton `token` a-t-il écrit la
    /// tranche, ou peut-il l'avoir écrite ? La règle est celle du store
    /// ([`Unversioned::written_by_others`]), relue sur ce que l'acteur sait de la tranche.
    pub(super) fn written_by_others(&self, token: Uuid) -> bool {
        Unversioned {
            tokens: self.tokens.iter().copied().collect(),
            others: self.others,
            ..Unversioned::default()
        }
        .written_by_others(token)
    }

    /// Le contenu est-il celui d'une version gardée ?
    pub(super) fn kept(&self) -> bool {
        self.kept
    }

    /// Les comptes de la tranche, triés.
    pub(super) fn authors(&self) -> Vec<Uuid> {
        self.authors.iter().copied().collect()
    }

    /// Ceux qui ont écrit par un jeton, triés.
    pub(super) fn agent_authors(&self) -> Vec<Uuid> {
        self.agent_authors.iter().copied().collect()
    }

    /// La version confiée à une tâche est écrite : si rien ne s'est écrit depuis l'état encodé,
    /// le contenu est le sien.
    pub(super) fn version_written(&mut self) {
        if self.since.is_none() {
            self.kept = true;
        }
    }

    /// Ruling R1234, règle 3 : une tranche écrite par des jetons seuls ne coupe pas de version
    /// `closing`. Elle reste au journal, et la `periodic` qui suit d'un intervalle sa première
    /// écriture la garde : un agent seul coupe au plus une version par intervalle.
    fn closes(&self) -> bool {
        self.since.is_some() && self.others
    }

    /// `self`, la tranche d'avant une version confiée à une tâche que le store a refusée, et
    /// `after`, ce qui a été écrit depuis : la tranche reprend le tout, comme si la version
    /// n'avait pas été tentée.
    pub(super) fn merged_with(self, after: Slice) -> Slice {
        let since = match (self.since, after.since) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let mut authors = self.authors;
        authors.extend(after.authors);
        let mut agent_authors = self.agent_authors;
        agent_authors.extend(after.agent_authors);
        let mut tokens = self.tokens;
        tokens.extend(after.tokens);
        Slice {
            since,
            authors,
            agent_authors,
            tokens,
            others: self.others || after.others,
            kept: self.kept && after.kept,
        }
    }

    /// La tranche en cours quand l'acteur démarre, relue au journal : les
    /// écritures au-delà de la dernière version (`Store::unversioned`), pour
    /// ce qu'il en reste après la compaction. C'est ce qu'un arrêt brutal ne
    /// perd pas : le contenu est au journal, seule la version `closing` manque.
    /// Sa date est celle de la plus ancienne écriture connue, ramenée à
    /// l'horloge de l'acteur, sans remonter plus loin que `version_every` :
    /// une tranche plus ancienne est due tout de suite, et l'instant de tokio
    /// ne remonte pas avant le démarrage de la machine. Illisible, la tranche
    /// est vide : la page est servie quand même, et la prochaine écriture en
    /// ouvre une. `created` : la page vient de recevoir son document, qu'aucune
    /// version ne garde.
    ///
    /// Ruling R1234, règle 2 : une part de la tranche que la compaction a purgée ne dit plus qui
    /// l'a écrite ; elle compte comme écrite par d'autres (`others`), si bien que la première
    /// opération destructrice d'un jeton coupe une version, et que l'arrêt coupe la `closing`.
    pub(super) async fn read(
        store: &Store,
        node_id: Uuid,
        version_every: Duration,
        created: bool,
    ) -> Slice {
        match store.unversioned(node_id).await {
            Ok(found) => Slice {
                since: found.since.map(|at| {
                    let ago = Duration::from_millis(now_ms().saturating_sub(at).max(0) as u64);
                    let ago = ago.min(version_every);
                    Instant::now().checked_sub(ago).unwrap_or_else(Instant::now)
                }),
                authors: found.authors.into_iter().collect(),
                agent_authors: found.agent_authors.into_iter().collect(),
                tokens: found.tokens.into_iter().collect(),
                others: found.others || found.unknown_writers,
                kept: !created && found.since.is_none() && found.versioned,
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
    ///
    /// Ruling R1234 : `writer` dit aussi par quel jeton elle a été faite ; une écriture sans
    /// jeton marque la tranche comme écrite par d'autres qu'un agent.
    pub(super) fn wrote(&mut self, changes: u64, writer: &Writer) {
        if !self.watch.changed_since(changes) {
            return;
        }
        self.slice.since.get_or_insert_with(Instant::now);
        self.slice.authors.extend(writer.account);
        match writer.token {
            Some(token) => {
                self.slice.tokens.insert(token);
                self.slice.agent_authors.extend(writer.account);
            }
            None => self.slice.others = true,
        }
        self.slice.kept = false;
    }

    /// La version `version` est-elle partie, ou celle que la prochaine version
    /// **ordinaire** ferait partir (Ruling R197) ? Son contenu ne serait alors
    /// plus atteignable seul après la coupe qu'une tranche ouverte produirait.
    /// Le plafond des `before_restore` n'y entre pas : seule une autre coupe
    /// d'avant restauration le fait jouer, et il lui faut un contenu que rien
    /// ne garde, donc une écriture après celle-ci, qui ouvre la tranche de
    /// toute façon. Le compter ouvrait une tranche à chaque restauration de la
    /// plus ancienne d'entre elles, et chaque arrêt de l'acteur coupait une
    /// copie `closing`.
    async fn next_to_go(&self, version: Uuid) -> Result<bool, StoreError> {
        let node = self.id.node_id;
        Ok(!self.store.has_version(node, version).await?
            || self.store.purged_by_next_ordinary(node).await? == Some(version))
    }

    /// Une restauration vient d'écrire au journal le contenu d'une version
    /// gardée, avec sa borne : le contenu est celui de cette version, et le
    /// store le sait ; aucune tranche ne s'ouvre (Ruling R197).
    pub(super) fn wrote_kept(&mut self) {
        self.slice.kept = true;
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
    /// connexions de la page qui peuvent écrire reçoivent la trame `versions` (Rulings R136,
    /// R355).
    ///
    /// Ruling R816 : la version d'avant une restauration deviendra la seule copie de ce que la
    /// restauration remplace, et la version de fin est coupée quand personne n'attend : leur état
    /// est relu par un `PageDoc` neuf avant d'être écrit ([`PageDoc::check_state`]) ; s'il ne se
    /// relit pas, rien n'est écrit, et l'erreur le dit (la restauration est alors refusée). Pas
    /// une version périodique : elle est coupée sur le chemin d'une frappe, que la relecture
    /// (81 ms pour 10 000 blocs) sortirait de son budget ; elle ne remplace rien (le contenu est
    /// au journal, ou dans un instantané relu), et un état qu'on ne relit pas n'est pas servi
    /// (Ruling R717).
    pub(super) async fn cut_version(&mut self, reason: VersionReason) -> Result<(), SyncError> {
        let state = self.whole_state();
        if matches!(
            reason,
            VersionReason::BeforeRestore | VersionReason::Closing
        ) && let Err(error) = off_the_engine(|| PageDoc::check_state(&state))
        {
            tracing::error!(node_id = %self.id.node_id, ?reason, %error, "version refusée : l'état de la page ne se relit pas");
            return Err(error.into());
        }
        let authors: Vec<Uuid> = self.slice.authors.iter().copied().collect();
        let agent_authors: Vec<Uuid> = self.slice.agent_authors.iter().copied().collect();
        self.store
            .put_version_by(
                self.id.node_id,
                self.last_seq,
                &state,
                &authors,
                &agent_authors,
                reason,
            )
            .await?;
        self.slice = Slice {
            kept: true,
            ..Slice::default()
        };
        self.page_event(PageEvent::Versions);
        Ok(())
    }

    /// À l'arrêt de l'acteur : la version `closing`, s'il reste des écritures non
    /// versionnées. Si le store la refuse, l'arrêt se fait quand même : le
    /// contenu est au journal, et l'acteur suivant y relit la tranche
    /// ([`Slice::read`]).
    ///
    /// Ruling R1234, règle 3 : pas pour une tranche écrite par des jetons seuls. L'acteur
    /// s'arrête `idle_close` après sa dernière connexion ; un agent REST qui écrivait toutes les
    /// trente et une secondes coupait une version par écriture, et vidait les cent versions de
    /// la page en moins d'une heure. La tranche reste au journal, relue par l'acteur suivant avec
    /// la date de sa plus ancienne écriture, et versionnée par la `periodic` qui suit d'un
    /// intervalle cette écriture.
    pub(super) async fn cut_closing(&mut self) {
        if !self.slice.closes() {
            return;
        }
        if let Err(error) = self.cut_version(VersionReason::Closing).await {
            tracing::error!(node_id = %self.id.node_id, %error, "version de fin impossible à écrire : l'acteur s'arrête quand même, le contenu est au journal");
        }
    }

    /// Ruling R135 : restaure la version `version`, au nom de `actor`. Ses blocs
    /// sont lus dans un document à part, bâti depuis l'état gardé, puis écrits
    /// par-dessus le contenu vivant par une écriture du serveur (journal,
    /// diffusion, compaction), avec leurs identifiants : les fils de
    /// commentaires retrouvent leurs blocs. Une édition faite en même temps sur
    /// le contenu remplacé est perdue, comme sous `PUT /mmd` (Ruling R188).
    ///
    /// Ruling R190 : une version `before_restore` garde ce qui est remplacé,
    /// dans la même commande (rien ne passe entre les deux), **si aucune version
    /// ne garde le contenu actuel** : la page a changé depuis sa dernière
    /// version, ou n'en a jamais eu (créée avec son contenu) ; sinon ce qui est
    /// remplacé est le contenu d'une version gardée (la dernière coupée, ou la
    /// dernière restaurée), et restaurable. Si le store la refuse, rien n'est
    /// restauré.
    /// L'écriture d'une restauration n'ouvre pas de tranche, son contenu étant
    /// celui de la version : elle passe au journal avec la borne de la version
    /// (`covers_seq`, Ruling R197), si bien qu'un acteur rechargé sait encore
    /// que le contenu est gardé. Des restaurations successives, sans écriture
    /// entre elles, ne coupent donc rien, l'acteur arrêté entre elles ou non.
    /// Sauf quand la version restaurée est partie, ou celle que la prochaine
    /// version **ordinaire** ferait partir (la plus ancienne d'un historique
    /// plein ; le plafond des `before_restore` n'y entre pas, voir
    /// `next_to_go`), jugé **après** la coupe d'avant restauration, qui peut
    /// elle-même la purger ou la mettre en tête : l'état que la page garde
    /// ensuite ne serait plus atteignable seul après la coupe suivante ;
    /// l'écriture passe alors au journal sans borne et ouvre une tranche, que la
    /// coupe suivante (avant une écriture qui suit d'un intervalle, ou à
    /// l'arrêt, rechargement compris) garde comme une version neuve : la
    /// version restaurée part, une copie de son contenu reste.
    ///
    /// Une version qui n'est pas de cette page, ou qui n'existe pas :
    /// `NotFound`. Un état gardé que la page ne relit pas (base abîmée) : une
    /// erreur, sans toucher au document. Des blocs que l'écriture refuserait
    /// sont refusés avant de couper quoi que ce soit. Restaurer ce que la page a
    /// déjà comme contenu ne fait rien : ni écriture, ni version (Ruling R180).
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
        let kept = off_the_engine(|| {
            PageDoc::from_updates([state.as_slice()]).map(|kept| kept.read_blocks())
        });
        let blocks = match kept {
            Ok(blocks) => blocks,
            Err(error) => {
                tracing::error!(node_id = %self.id.node_id, version_id = %version, %error, "état d'une version illisible : restauration refusée, page intacte");
                return Ok(Err(SyncError::RestoreRefused(crate::VERSION_UNREADABLE)));
            }
        };
        if blocks == self.page.read_blocks() {
            return Ok(Ok(()));
        }
        if let Err(error) = PageDoc::check_writable(&blocks) {
            return Ok(Err(error.into()));
        }
        if !self.slice.kept
            && let Err(error) = self.cut_version(VersionReason::BeforeRestore).await
        {
            tracing::warn!(node_id = %self.id.node_id, version_id = %version, %error, "version d'avant restauration impossible à écrire : rien n'est restauré");
            // Ruling R832 : un état que le serveur ne relit pas (Ruling R816) se dit comme tel ;
            // une erreur du store reste une erreur du store.
            return Ok(Err(match error {
                SyncError::Crdt(_) => SyncError::RestoreRefused(crate::PAGE_UNREADABLE),
                error => error,
            }));
        }
        // Après la coupe : c'est elle qui peut avoir purgé la version, ou en avoir fait celle que
        // la prochaine version ordinaire ferait partir.
        let next_in_line = match self.next_to_go(version).await {
            Ok(next) => next,
            Err(error) => return Ok(Err(error.into())),
        };
        let history = if next_in_line {
            History::Slice
        } else {
            History::Kept(version)
        };
        self.server_write(blocks, actor, history).await
    }
}
