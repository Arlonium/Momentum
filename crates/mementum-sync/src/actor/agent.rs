//! L'écriture d'un agent dans l'acteur de sa page (Task S4 du lot 5 ; Rulings R1226, R1229,
//! R1231, R1234, R1572).
//!
//! **Une commande** quand l'écriture ne coupe pas de version `before_agent` : l'acteur lit les
//! blocs et la révision, fait le plan des opérations (`mementum_crdt::ops::plan`), estime
//! l'écriture (`diff_cost`), puis `version_if_due`, la transaction (par différence), le journal
//! (avec le jeton), la diffusion, `wrote` et la révision rendue, celle de l'état écrit : rien ne
//! passe entre la lecture et l'écriture (RESULTS, leçon 8).
//!
//! **Deux commandes** quand elle en coupe une (une opération destructrice d'un jeton qui remplace
//! ce qu'aucune version ne garde, et qu'un autre que ce jeton a écrit, R1234 règle 1) : la
//! première lit, planifie, juge le conflit, encode l'état et garde les empreintes de ses blocs,
//! puis confie l'état à une tâche, hors de l'acteur, qui le relit (`PageDoc::check_state`) et
//! écrit la version (R1257) ; l'acteur sert les frappes pendant ce temps. La seconde relit la
//! page, refait le plan **avec la base de l'agent** (R1562 : jamais avec les empreintes de l'état
//! encodé), juge `Plan::changed_since` contre l'état encodé (vide : la version garde ce que
//! l'opération remplace), puis écrit. Sinon elle recoupe une version, une fois, puis rend
//! `version_refused`. Une version que la tâche ne relit pas, ou que le store refuse : rien n'est
//! écrit.
//!
//! La version porte `covers_seq` = le `seq` de l'état encodé (R1572 c) : une frappe servie entre
//! les deux commandes n'y est pas, et reste dans la tranche en cours. Un acteur qui s'arrête
//! attend la tâche en vol avant de quitter le registre (R1572 b, comme la compaction, R826).

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use mementum_crdt::anchors::{BlockHashes, hashes};
use mementum_crdt::ops::{self, Base, OpRefusal, Operation, Plan, PlanContext};
use mementum_crdt::{PageDoc, WriteCost, diff_cost};
use mementum_md::Block;
use mementum_store::{Store, VersionReason, Writer};
use tokio::sync::{RwLock, oneshot};
use tracing::instrument::WithSubscriber;
use uuid::Uuid;

use super::history::Slice;
use super::{Actor, Broken};
use crate::long_work::off_the_engine;
use crate::protocol::{self, EMPTY_UPDATE};
use crate::{PageEvent, SyncError};

/// Versions `before_agent` qu'une même écriture coupe au plus (R1234, règle 4) : la première, puis
/// une seconde si la page a changé, pendant que la première s'écrivait, dans les blocs que
/// l'écriture touche ; au-delà, `version_refused`.
const MAX_CUTS: u8 = 2;

/// Ce que fait un appel dont la révision n'est plus celle de la page (R1231).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnConflict {
    /// L'appel s'applique si les blocs qu'il touche n'ont pas changé depuis la révision lue.
    #[default]
    Rebase,
    /// L'appel est refusé dès que la révision diffère.
    Fail,
}

/// Ce qu'un agent demande d'écrire dans une page ([`crate::Hub::edit_page`]).
#[derive(Debug, Clone, Default)]
pub struct Edit {
    /// Les opérations, leurs MMD déjà lus (`mementum_crdt::ops::read_markdown`, hors de l'acteur).
    pub ops: Vec<Operation>,
    /// La révision que l'agent a lue, et les empreintes des blocs de cette révision si le serveur
    /// les garde encore (R1226) ; `None` : l'appel n'en nomme pas (une opération destructrice est
    /// alors refusée, R1231).
    pub base: Option<(String, Option<Arc<BlockHashes>>)>,
    pub on_conflict: OnConflict,
    /// Qui écrit : le compte, et le jeton par lequel il écrit. Une écriture par jeton ne fait
    /// entrer aucun média du web que la page ne portait pas (R1235), et seule elle coupe une
    /// version `before_agent` (R1234).
    pub writer: Writer,
    /// Les identifiants des blocs qui portent un fil de commentaires : un contenu protégé (R1232,
    /// R1571 ; le hub ne connaît pas les fils, la route les donne).
    pub commented: Option<HashSet<String>>,
}

/// Une page lue par son acteur ([`crate::Hub::read_page`]) : ses blocs et sa révision, sortis de
/// la même commande, et les empreintes de ces blocs sous la clé du hub ([`crate::Hub::hash_key`]),
/// calculées hors de l'acteur.
#[derive(Debug, Clone)]
pub struct PageRead {
    pub blocks: Vec<Block>,
    pub revision: String,
    pub hashes: Arc<BlockHashes>,
}

/// Une écriture d'agent faite ([`crate::Hub::edit_page`]).
#[derive(Debug, Clone)]
pub struct Edited {
    /// La révision de l'état écrit, sortie de la commande qui écrit (leçon 8) : celle que lit
    /// ensuite qui lit la page, tant que personne d'autre n'y écrit.
    pub revision: String,
    /// La révision sur laquelle le plan a été fait et écrit (lue dans la même commande).
    pub before: String,
    /// Les blocs dont un élément Yjs a été créé, supprimé ou changé (`DiffWritten::touched`) ;
    /// vide pour une écriture qui ne change rien.
    pub touched: Vec<String>,
    /// Les blocs de la page après l'écriture, lus dans la commande qui écrit.
    pub blocks: Vec<Block>,
    /// Leurs empreintes, sous la clé du hub, calculées hors de l'acteur.
    pub hashes: Arc<BlockHashes>,
    /// La version `before_agent` coupée avant l'écriture, s'il y en a eu une (la dernière).
    pub version: Option<Uuid>,
}

/// Pourquoi une écriture d'agent n'a rien écrit.
#[derive(Debug)]
pub enum EditRefusal {
    /// Une opération refusée (`OpRefusal`, R1229 : `no_match`, `stale_anchor`,
    /// `revision_conflict` en `rebase`…), avec la révision et les blocs de la page lus dans la
    /// commande, pour traduire `refusal.blocks` en alias et rendre leur MMD.
    Operation {
        refusal: Box<OpRefusal>,
        revision: String,
        blocks: Vec<Block>,
    },
    /// La page n'est plus à la révision nommée et l'appel ne se rejoue pas dessus
    /// (`on_conflict: "fail"`, ou les empreintes de la révision ne sont plus connues) : `409
    /// revision_conflict`, avec la révision et les blocs courants (R1231).
    Conflict {
        revision: String,
        blocks: Vec<Block>,
    },
    /// Une opération destructrice sans révision : `422 revision_required` (R1231).
    RevisionRequired,
    /// L'écriture est estimée au-delà de [`crate::SyncConfig::agent_write_budget`] : `422
    /// too_long_to_write`, sans repli sur une récriture complète (Task S2, R1294).
    TooLong(WriteCost),
    /// La version `before_agent` n'a pas pu être écrite (l'état ne se relit pas, le store la
    /// refuse), ou la page a changé deux fois de suite dans les blocs touchés pendant qu'elle
    /// s'écrivait : `409 version_refused` (R1234, règle 4).
    VersionRefused,
    /// La page ou son acteur (introuvable, document illisible, store en panne).
    Failed(SyncError),
}

impl From<SyncError> for EditRefusal {
    fn from(e: SyncError) -> EditRefusal {
        EditRefusal::Failed(e)
    }
}

/// La version déjà coupée d'une écriture menée en plusieurs commandes.
#[derive(Debug, Clone)]
pub(crate) struct Versioned {
    /// Les empreintes des blocs de l'état encodé, sous la clé du hub : celles des blocs que la
    /// commande qui l'a encodé venait de lire, jamais relues de l'état (R1572 a).
    pub(crate) encoded: Arc<BlockHashes>,
    /// La révision de l'état encodé.
    pub(crate) encoded_revision: String,
    /// Les versions coupées jusqu'ici pour cette écriture.
    pub(crate) cuts: u8,
    /// La dernière.
    pub(crate) version: Uuid,
}

/// La réponse de l'acteur à une commande d'écriture d'agent.
pub(crate) enum Step {
    /// L'écriture est faite, ou refusée.
    Done(Result<Written, EditRefusal>),
    /// Une version `before_agent` est confiée à une tâche : le hub attend `written`, puis renvoie
    /// la commande avec la version ([`Versioned`]).
    Version {
        written: oneshot::Receiver<Result<Uuid, SyncError>>,
        encoded: Arc<BlockHashes>,
        encoded_revision: String,
    },
}

/// Une écriture faite, telle que l'acteur la rend (le hub y ajoute les empreintes).
pub(crate) struct Written {
    pub(crate) revision: String,
    pub(crate) before: String,
    pub(crate) touched: Vec<String>,
    pub(crate) blocks: Vec<Block>,
    /// La page a changé (une mise à jour écrite au journal).
    pub(crate) wrote: bool,
}

/// Une page écrite par une commande d'agent : la révision et les blocs de l'état écrit, et les
/// blocs dont un élément Yjs a changé.
struct WrittenPage {
    revision: String,
    blocks: Vec<Block>,
    touched: Vec<String>,
}

/// La version `before_agent` en vol : la tâche qui la relit et l'écrit, et la tranche qu'elle
/// garde, rendue à l'acteur si elle échoue.
pub(super) struct Versioning {
    task: tokio::task::JoinHandle<bool>,
    held: Slice,
}

/// Une barrière devant la relecture de chaque version `before_agent`, que seuls les tests
/// tiennent ([`crate::Hub::hold_agent_versions`]) : une frappe servie pendant la version se prouve
/// alors sans minutage.
#[derive(Default)]
pub(crate) struct VersionGate {
    lock: Arc<RwLock<()>>,
    waiting: AtomicU64,
}

impl VersionGate {
    pub(crate) fn hold(&self) -> tokio::sync::OwnedRwLockWriteGuard<()> {
        self.lock
            .clone()
            .try_write_owned()
            .expect("les versions d'agent sont déjà retenues")
    }

    pub(crate) fn waiting(&self) -> u64 {
        self.waiting.load(Ordering::SeqCst)
    }

    async fn pass(self: &Arc<VersionGate>) {
        self.waiting.fetch_add(1, Ordering::SeqCst);
        drop(self.lock.read().await);
        self.waiting.fetch_sub(1, Ordering::SeqCst);
    }
}

/// La tâche d'une version `before_agent` (R1234, règle 4 ; R1257) : l'état que l'acteur a encodé
/// au `seq` donné est relu par un `PageDoc` neuf sur un fil bloquant (R816 : la version devient la
/// seule copie de ce que l'opération remplace), puis écrit dans la voie de la page, `covers_seq`
/// à ce `seq`. Le résultat part à qui attend l'écriture ; la valeur rendue dit à l'acteur si la
/// version est écrite.
#[allow(clippy::too_many_arguments)]
async fn cut_before_agent(
    store: Store,
    node_id: Uuid,
    seq: i64,
    state: Vec<u8>,
    authors: Vec<Uuid>,
    agent_authors: Vec<Uuid>,
    gate: Option<Arc<VersionGate>>,
    done: oneshot::Sender<Result<Uuid, SyncError>>,
) -> bool {
    let result = async {
        if let Some(gate) = &gate {
            gate.pass().await;
        }
        let read = tokio::task::spawn_blocking(move || PageDoc::check_state(&state).map(|()| state));
        let state = match read.await {
            Ok(Ok(state)) => state,
            Ok(Err(error)) => {
                tracing::error!(%node_id, seq, %error, "version d'avant un agent refusée : l'état de la page ne se relit pas, rien n'est écrit");
                return Err(SyncError::Crdt(error));
            }
            Err(error) => {
                tracing::error!(%node_id, seq, %error, "version d'avant un agent interrompue pendant la relecture de l'état");
                return Err(SyncError::Closed);
            }
        };
        let id = store
            .put_version_by(
                node_id,
                seq,
                &state,
                &authors,
                &agent_authors,
                VersionReason::BeforeAgent,
            )
            .await
            .inspect_err(|error| {
                tracing::warn!(%node_id, seq, %error, "version d'avant un agent impossible à écrire : rien n'est écrit");
            })?;
        Ok(id)
    }
    .await;
    let written = result.is_ok();
    let _ = done.send(result);
    written
}

impl Actor {
    /// Les blocs de la page et sa révision, lus ensemble (R1226).
    pub(super) fn read_page(&self) -> (Vec<Block>, String) {
        off_the_engine(|| (self.page.read_blocks(), self.page.revision()))
    }

    /// Les connexions inscrites qui peuvent écrire, et ne sont pas en train de se fermer (R1233).
    pub(super) fn writers(&self) -> usize {
        self.conns
            .values()
            .filter(|c| c.writable && !c.closing)
            .count()
    }

    /// Une commande d'écriture d'agent (voir le module). `then` : la version déjà coupée pour
    /// cette écriture, à la seconde commande.
    pub(super) async fn edit(
        &mut self,
        edit: &Edit,
        then: Option<Versioned>,
    ) -> Result<Step, Broken> {
        let destructive = edit.ops.iter().any(Operation::destructive);
        if destructive && edit.base.is_none() {
            return Ok(Step::Done(Err(EditRefusal::RevisionRequired)));
        }
        let token = edit.writer.token;
        // Une version en vol se règle avant de rien décider : la sienne (la seconde commande), ou
        // celle d'une autre écriture destructrice, qui dit ce que garde la tranche.
        if then.is_some() || (destructive && token.is_some()) {
            self.versioning_finished().await;
        }
        let (current, revision) = self.read_page();
        // R1231 : la révision nommée, et les empreintes de ses blocs. Celles de l'état encodé ne
        // servent de base que si l'agent avait lu cet état même (R1562 : jamais un autre).
        let mut known: Option<Arc<BlockHashes>> = None;
        let changed_page = match &edit.base {
            None => false,
            Some((base, _)) if *base == revision => false,
            Some((base, hashes)) => {
                let from_encoded = then
                    .as_ref()
                    .filter(|v| v.encoded_revision == *base)
                    .map(|v| v.encoded.clone());
                known = match edit.on_conflict {
                    OnConflict::Rebase => hashes.clone().or(from_encoded),
                    // R1338 : la coupe en deux commandes est le fait du serveur. L'agent avait lu
                    // l'état encodé : la seconde commande se juge comme un `rebase` restreint aux
                    // blocs touchés ; sans cela, toute frappe pendant la version était un conflit.
                    OnConflict::Fail => from_encoded,
                };
                if known.is_none() {
                    return Ok(Step::Done(Err(EditRefusal::Conflict {
                        revision,
                        blocks: current,
                    })));
                }
                true
            }
        };
        let base = match (&known, changed_page) {
            (Some(hashes), true) => Base::Changed(hashes),
            _ => Base::Current,
        };
        let ctx = PlanContext {
            base,
            key: &self.key,
            revision: &revision,
            forbid_external_media: token.is_some(),
            commented: edit.commented.as_ref(),
        };
        let planned = off_the_engine(|| ops::plan(&current, &edit.ops, &ctx));
        let plan = match planned {
            Ok(plan) => plan,
            Err(refusal) => {
                return Ok(Step::Done(Err(EditRefusal::Operation {
                    refusal: Box::new(refusal),
                    revision,
                    blocks: current,
                })));
            }
        };
        let cuts = then.as_ref().map_or(0, |v| v.cuts);
        let need_version = match (&then, token) {
            // R1234, règle 1 (R1278) : une opération destructrice d'un jeton, sur un contenu
            // qu'aucune version ne garde, que quelqu'un d'autre que ce jeton a écrit ; et qui
            // change quelque chose (R1339 : une opération sans effet n'a rien à garder).
            (None, Some(token)) => {
                destructive
                    && !self.slice.kept()
                    && self.slice.written_by_others(token)
                    && plan.blocks() != current.as_slice()
            }
            (None, None) => false,
            // R1572 d : la page a changé, pendant que la version s'écrivait, dans le contenu de ce
            // que l'écriture remplace ; la version ne le garde pas (R1336 : la place d'un bloc
            // touché par elle seule est jugée par le plan refait).
            (Some(versioned), _) => !plan.changed_since(&versioned.encoded, &self.key).is_empty(),
        };
        if need_version {
            if cuts >= MAX_CUTS {
                return Ok(Step::Done(Err(EditRefusal::VersionRefused)));
            }
            // Task S2 (R1294) : une écriture qu'on refusera pour son coût ne coupe pas de version.
            let (cost, encoded) = off_the_engine(|| {
                (
                    diff_cost(&current, plan.blocks()),
                    hashes(&current, &self.key),
                )
            });
            if u128::from(cost.nanos) > self.cfg.agent_write_budget.as_nanos() {
                return Ok(Step::Done(Err(EditRefusal::TooLong(cost))));
            }
            drop(plan);
            let written = self.start_version();
            return Ok(Step::Version {
                written,
                encoded: Arc::new(encoded),
                encoded_revision: revision,
            });
        }
        let written = self.write_plan(&plan, &edit.writer).await?;
        drop(plan);
        Ok(Step::Done(match written {
            Ok(Some(page)) => Ok(Written {
                revision: page.revision,
                before: revision,
                touched: page.touched,
                blocks: page.blocks,
                wrote: true,
            }),
            Ok(None) => Ok(Written {
                revision: revision.clone(),
                before: revision,
                touched: Vec::new(),
                blocks: current,
                wrote: false,
            }),
            Err(refusal) => Err(refusal),
        }))
    }

    /// Écrit un plan : `version_if_due` avant, l'estimation et la transaction (par différence,
    /// `PageDoc::apply_within` : un seul appariement des blocs pour les deux), le journal au nom de
    /// `writer`, `wrote`, la diffusion. `Ok(None)` : rien ne diffère, rien n'est écrit (ni journal,
    /// ni diffusion, ni tranche, Task S2). Sinon la révision de l'état écrit et ses blocs, lus dans
    /// cette commande, hors du fil du moteur : les blocs que l'écriture rend, sans relire la page,
    /// quand elle n'est pas retombée sur la récriture complète (R1337).
    async fn write_plan(
        &mut self,
        plan: &Plan<'_>,
        writer: &Writer,
    ) -> Result<Result<Option<WrittenPage>, EditRefusal>, Broken> {
        self.version_if_due().await;
        let changes = self.watch.changes();
        let budget = self.cfg.agent_write_budget.as_nanos();
        let written = off_the_engine(|| -> Result<_, mementum_crdt::CrdtError> {
            let mut written = match self.page.apply_within(plan, budget)? {
                Ok(written) => written,
                Err(cost) => return Ok(Err(cost)),
            };
            if written.update == EMPTY_UPDATE {
                return Ok(Ok(None));
            }
            let blocks = match written.blocks.take() {
                Some(blocks) => {
                    debug_assert_eq!(blocks, self.page.read_blocks(), "l'arbre écrit");
                    blocks
                }
                None => self.page.read_blocks(),
            };
            Ok(Ok(Some((written, blocks, self.page.revision()))))
        });
        let (written, blocks, revision) = match written {
            Ok(Ok(Some(written))) => written,
            Ok(Ok(None)) => return Ok(Ok(None)),
            // Task S2 (R1294) : au-delà du budget, refusée, sans repli silencieux.
            Ok(Err(cost)) => return Ok(Err(EditRefusal::TooLong(cost))),
            Err(error) => {
                // Une erreur levée après la première transaction (`HalfWritten`) : la page est à
                // recharger.
                if self.watch.changed_since(changes) {
                    tracing::error!(node_id = %self.id.node_id, %error, "écriture d'un agent faite à moitié : le document est rechargé");
                    self.reload().await?;
                }
                return Ok(Err(EditRefusal::Failed(error.into())));
            }
        };
        if let Err(error) = self.persist(&[&written.update], writer).await {
            self.store_failed(&error).await?;
            return Ok(Err(EditRefusal::Failed(error.into())));
        }
        self.wrote(changes, writer);
        self.broadcast(None, protocol::update(&written.update));
        self.compact_if_due();
        Ok(Ok(Some(WrittenPage {
            revision,
            blocks,
            touched: written.touched,
        })))
    }

    /// Encode l'état au `seq` courant et confie la version `before_agent` à une tâche (R1257) ;
    /// la tranche en cours part avec elle (ses auteurs sont ceux de la version), et une tranche
    /// neuve reçoit ce qui s'écrit pendant ce temps (R1572 c).
    fn start_version(&mut self) -> oneshot::Receiver<Result<Uuid, SyncError>> {
        let state = self.whole_state();
        let held = std::mem::replace(&mut self.slice, Slice::after_a_version_in_flight());
        let (done, written) = oneshot::channel();
        let gate = self.id.hub.upgrade().map(|hub| hub.version_gate());
        let task = cut_before_agent(
            self.store.clone(),
            self.id.node_id,
            self.last_seq,
            state,
            held.authors(),
            held.agent_authors(),
            gate,
            done,
        );
        self.versioning = Some(Versioning {
            task: tokio::spawn(task.with_current_subscriber()),
            held,
        });
        written
    }

    /// La version `before_agent` en vol est-elle finie (écrite ou refusée) ? L'acteur la règle
    /// alors en tête de sa commande suivante (R1335) : une seconde commande qui ne vient pas (un
    /// appel abandonné, une version refusée) ne laisse pas la tranche en vol.
    pub(super) fn versioning_done(&self) -> bool {
        self.versioning
            .as_ref()
            .is_some_and(|versioning| versioning.task.is_finished())
    }

    /// Pour les tests : une version en vol menée par `task`, la tranche en cours partie avec elle.
    #[cfg(test)]
    pub(super) fn version_in_flight(&mut self, task: tokio::task::JoinHandle<bool>) {
        let held = std::mem::replace(&mut self.slice, Slice::after_a_version_in_flight());
        self.versioning = Some(Versioning { task, held });
    }

    /// Attend la version `before_agent` en vol, s'il y en a une, et en tire la tranche : écrite,
    /// la tranche est ce qui s'est écrit depuis l'état encodé, et les connexions qui écrivent
    /// reçoivent la trame `versions` ; refusée, la tranche d'avant revient, avec ce qui s'est écrit
    /// depuis.
    pub(super) async fn versioning_finished(&mut self) {
        let Some(Versioning { task, held }) = self.versioning.take() else {
            return;
        };
        let written = match task.await {
            Ok(written) => written,
            Err(error) => {
                tracing::error!(node_id = %self.id.node_id, %error, "version d'avant un agent interrompue");
                false
            }
        };
        if written {
            self.slice.version_written();
            self.page_event(PageEvent::Versions);
        } else {
            let after = std::mem::take(&mut self.slice);
            self.slice = held.merged_with(after);
        }
    }
}
