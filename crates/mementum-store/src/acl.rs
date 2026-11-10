//! Accès posés sur les pages (`acl`) : lecture, pose, retrait, et le changement d'un accès par
//! le partage d'une page ([`Store::change_access`]).
//!
//! Un sujet s'écrit dans `acl.subject` sous la forme texte de `mementum-acl` (`user:<uuid>` ou
//! `members`), un niveau sous celle de `Level::as_str`. Une ligne qui ne se relit pas est une
//! erreur, jamais une ligne sautée : un accès ignoré en silence fausserait le calcul des droits.

use std::collections::{HashMap, HashSet};
use std::fmt;

use mementum_acl::{Grant, Level, Subject, chain_level};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::members::{guest_leaves_if_bare, keeps_full, role_of};
use crate::spaces::bump_rev;
use crate::{Store, StoreError, parse_uuid, unreadable};

/// Un changement des accès posés sur une page, par [`Store::change_access`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessChange {
    /// La ligne `members` de la page (son accès général) : ce niveau, ou retirée (`None`).
    Members(Option<Level>),
    /// La ligne `user:<id>` de la page : ce niveau, ou retirée (`None`).
    User(Uuid, Option<Level>),
}

/// Ce qu'a écrit [`Store::change_access`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessWritten {
    /// L'espace de la page.
    pub space_id: Uuid,
    /// Une ligne a été posée, changée ou retirée. Faux : elle était déjà ainsi ; rien n'a été
    /// écrit, et la révision de l'espace n'a pas bougé.
    pub changed: bool,
    /// La personne dont la ligne est retirée, invitée, n'a plus aucune ligne dans l'espace :
    /// elle en est sortie.
    pub left_space: bool,
    /// La ligne propre du sujet sur la page après l'appel : son niveau, ou `None` si la page
    /// n'en a pas pour lui. Ce n'est pas toujours ce qui était demandé : un accès général
    /// demandé au niveau du plancher retire la ligne propre (Ruling R93).
    pub line: Option<Level>,
}

/// Pourquoi [`Store::change_access`] refuse un changement, vérifié dans sa transaction (Ruling
/// R87), dans l'ordre où les cas sont regardés.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareRefusal {
    /// La page n'existe pas pour qui demande : inconnue, d'un espace où il n'a aucun rôle, ou
    /// qu'il ne peut pas lire (y compris sous un cycle de parents, R58).
    Unseen,
    /// Il la voit, sous `full`.
    BelowFull,
    /// Il est invité, et le changement touche à l'accès général (Ruling R85) : poser ou retirer
    /// la ligne `members` est réservé aux rôles `member` et plus.
    GuestGeneral,
    /// La page, ou un de ses ancêtres, est à la corbeille.
    Trashed,
    /// La personne qui recevrait l'accès n'a aucun rôle dans l'espace (ou n'existe pas).
    NotInSpace,
    /// La page n'a pas de ligne propre pour la personne dont on retire l'accès (elle n'en a
    /// aucune, ou seulement sur un ancêtre).
    NoOwnLine,
    /// Ruling R93 : l'accès général demandé est sous le plancher que donne la ligne `members`
    /// d'un ancêtre, ou c'est `null` alors que la page n'a pas de ligne propre et qu'un ancêtre
    /// en a une : il ne se baisse ni ne se retire ici.
    InheritedAccess,
}

impl fmt::Display for ShareRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ShareRefusal::Unseen => "page introuvable",
            ShareRefusal::BelowFull => "accès complet nécessaire",
            ShareRefusal::GuestGeneral => "un invité ne change pas l'accès général",
            ShareRefusal::Trashed => "page à la corbeille",
            ShareRefusal::NotInSpace => "la personne visée n'a pas de rôle dans l'espace",
            ShareRefusal::NoOwnLine => "pas de ligne propre pour la personne visée",
            ShareRefusal::InheritedAccess => "accès général hérité d'une page parente",
        })
    }
}

/// La requête récursive `chain(id)` : une page (liée en `?1`) et ses ancêtres. `UNION` (et non
/// `UNION ALL`) : la remontée s'arrête même si des parents tournaient en rond. En littéral pour
/// que le SQL reste statique (`concat!`).
macro_rules! chain_of {
    () => {
        "WITH RECURSIVE chain(id) AS ( \
             SELECT ?1 \
             UNION \
             SELECT n.parent_id FROM chain JOIN nodes n ON n.id = chain.id \
             WHERE n.parent_id IS NOT NULL \
         ) "
    };
}

/// Relit les colonnes `subject` et `level` d'une ligne d'`acl`.
fn grant_from_row(r: &SqliteRow) -> Result<Grant, StoreError> {
    let subject: String = r.get("subject");
    let level: String = r.get("level");
    Ok(Grant {
        subject: Subject::parse(&subject).ok_or_else(|| unreadable("sujet d'accès", &subject))?,
        level: Level::parse(&level).ok_or_else(|| unreadable("niveau d'accès", &level))?,
    })
}

/// Pose `grant` sur le nœud, dans la transaction : le niveau remplace celui que le sujet y
/// avait. Rend vrai s'il a écrit (faux : le sujet y avait déjà ce niveau). La révision de
/// l'espace est l'affaire de l'appelant.
pub(crate) async fn write_grant(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
    grant: Grant,
) -> Result<bool, StoreError> {
    let written = sqlx::query(
        "INSERT INTO acl (node_id, subject, level) VALUES (?1, ?2, ?3) \
         ON CONFLICT(node_id, subject) DO UPDATE SET level = excluded.level \
         WHERE level != excluded.level",
    )
    .bind(node_id)
    .bind(grant.subject.to_string())
    .bind(grant.level.as_str())
    .execute(&mut **tx)
    .await
    .map_err(StoreError::from_fk)?
    .rows_affected();
    Ok(written > 0)
}

/// L'espace du nœud, lu dans la transaction ; `NotFound` s'il n'existe pas.
pub(crate) async fn space_of(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
) -> Result<String, StoreError> {
    Ok(sqlx::query("SELECT space_id FROM nodes WHERE id = ?")
        .bind(node_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(StoreError::NotFound)?
        .get(0))
}

/// Une page et ses ancêtres jusqu'à la racine, lus dans une transaction.
pub(crate) struct Chain {
    /// L'espace de la page.
    pub(crate) space: String,
    /// La page, ou un de ses ancêtres, est à la corbeille.
    pub(crate) in_trash: bool,
    /// Les lignes de la page et de ses ancêtres : `(nœud, accès)`.
    pub(crate) lines: Vec<(String, Grant)>,
}

impl Chain {
    /// La page `node` a-t-elle sa propre ligne pour `subject` ?
    fn has_own(&self, node: &str, subject: Subject) -> bool {
        self.lines
            .iter()
            .any(|(on, grant)| on == node && grant.subject == subject)
    }
}

/// Ruling R93 : la ligne `members` de la page `node` (de chaîne `chain`) quand on demande
/// `wanted` comme accès général : posée à ce niveau, ou retirée (`None`). Le plancher est la
/// plus forte des lignes `members` des ancêtres ; il n'y a jamais de ligne redondante :
/// - `null` : la ligne propre part (sans elle, rien) ; sans ligne propre sous un plancher,
///   `InheritedAccess`, le plancher ne se retire pas d'ici ;
/// - un niveau sous le plancher : `InheritedAccess` (la ligne n'aurait aucun effet) ; au
///   plancher : la ligne propre part ; au-dessus, ou sans plancher : elle est posée.
fn general_line(
    chain: &Chain,
    node: &str,
    wanted: Option<Level>,
) -> Result<Option<Level>, StoreError> {
    let floor = chain
        .lines
        .iter()
        .filter(|(on, grant)| on != node && grant.subject == Subject::Members)
        .map(|(_, grant)| grant.level)
        .max();
    let refused = || Err(StoreError::Refused(ShareRefusal::InheritedAccess));
    match (wanted, floor) {
        (None, Some(_)) if !chain.has_own(node, Subject::Members) => refused(),
        (None, _) => Ok(None),
        (Some(level), Some(floor)) if level < floor => refused(),
        (Some(level), Some(floor)) if level == floor => Ok(None),
        (Some(level), _) => Ok(Some(level)),
    }
}

/// La chaîne de la page `node_id`, lue dans la transaction. `None` : la page n'existe pas, ou
/// sa remontée tourne en rond (un nœud pris dans un cycle de parents, ou suspendu sous un
/// cycle, n'est vu par personne, R58). Comme pour `SpaceAccess::compute`, un parent hors de
/// l'espace vaut la racine.
pub(crate) async fn read_chain(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
) -> Result<Option<Chain>, StoreError> {
    let mut nodes: HashMap<String, (Option<String>, bool, String)> = HashMap::new();
    for r in sqlx::query(concat!(
        chain_of!(),
        "SELECT n.id, n.parent_id, n.trashed_at IS NOT NULL, n.space_id \
         FROM nodes n JOIN chain ON n.id = chain.id"
    ))
    .bind(node_id)
    .fetch_all(&mut **tx)
    .await?
    {
        nodes.insert(r.get(0), (r.get(1), r.get(2), r.get(3)));
    }
    let Some((_, _, space)) = nodes.get(node_id) else {
        return Ok(None);
    };
    let space = space.clone();
    let mut on_chain = HashSet::new();
    let mut in_trash = false;
    let mut current = Some(node_id.to_string());
    while let Some(id) = current {
        let Some((parent, trashed, _)) = nodes.get(&id).filter(|(_, _, s)| *s == space) else {
            break;
        };
        if !on_chain.insert(id) {
            return Ok(None);
        }
        in_trash |= *trashed;
        current = parent.clone();
    }
    let mut lines = Vec::new();
    for r in sqlx::query(concat!(
        chain_of!(),
        "SELECT a.node_id, a.subject, a.level FROM acl a JOIN chain ON a.node_id = chain.id"
    ))
    .bind(node_id)
    .fetch_all(&mut **tx)
    .await?
    {
        let on: String = r.get("node_id");
        if on_chain.contains(&on) {
            lines.push((on, grant_from_row(&r)?));
        }
    }
    Ok(Some(Chain {
        space,
        in_trash,
        lines,
    }))
}

/// Ce sur quoi repose un changement d'accès de `actor` sur la page, vérifié dans la transaction
/// (Ruling R87), dans l'ordre des refus : la page existe pour lui (un rôle dans son espace, au
/// moins `read` sur sa chaîne, par [`chain_level`]) ; il y a `full` ; pour l'accès général
/// (`general`), il n'est pas invité (R85) ; ni la page ni un ancêtre n'est à la corbeille. Rend
/// la chaîne de la page.
async fn check_sharer(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
    actor: Uuid,
    general: bool,
) -> Result<Chain, StoreError> {
    let refused = StoreError::Refused;
    let chain = read_chain(tx, node_id)
        .await?
        .ok_or(refused(ShareRefusal::Unseen))?;
    let role = role_of(tx, &chain.space, &actor.to_string())
        .await?
        .ok_or(refused(ShareRefusal::Unseen))?;
    let level = chain_level(chain.lines.iter().map(|(_, grant)| grant), actor, role)
        .ok_or(refused(ShareRefusal::Unseen))?;
    if level < Level::Full {
        return Err(refused(ShareRefusal::BelowFull));
    }
    if general && !role.is_member() {
        return Err(refused(ShareRefusal::GuestGeneral));
    }
    if chain.in_trash {
        return Err(refused(ShareRefusal::Trashed));
    }
    Ok(chain)
}

impl Store {
    /// Tous les accès posés dans l'espace, corbeille comprise : `(nœud, accès)`, triés par
    /// nœud puis par sujet.
    ///
    /// Ne sert plus qu'aux tests, qui y lisent toutes les lignes d'un espace : le serveur les
    /// lit avec le reste de ce dont se calculent les accès, en une transaction
    /// ([`Store::access_inputs`]).
    pub async fn space_grants(&self, space_id: Uuid) -> Result<Vec<(Uuid, Grant)>, StoreError> {
        sqlx::query(
            "SELECT a.node_id, a.subject, a.level FROM acl a \
             JOIN nodes n ON n.id = a.node_id \
             WHERE n.space_id = ? ORDER BY a.node_id, a.subject",
        )
        .bind(space_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(|r| {
            Ok((
                parse_uuid(&r.get::<String, _>("node_id"))?,
                grant_from_row(r)?,
            ))
        })
        .collect()
    }

    /// Les accès posés sur ce nœud (pas ceux qu'il hérite), triés par sujet. Vide pour un nœud
    /// inconnu.
    pub async fn node_grants(&self, node_id: Uuid) -> Result<Vec<Grant>, StoreError> {
        sqlx::query("SELECT subject, level FROM acl WHERE node_id = ? ORDER BY subject")
            .bind(node_id.to_string())
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(grant_from_row)
            .collect()
    }

    /// Les accès posés sur ce nœud et sur chacun de ses ancêtres : `(nœud, accès)`, triés par
    /// nœud puis par sujet. Vide pour un nœud inconnu.
    ///
    /// La chaîne est celle que lit le partage (`read_chain`), en une transaction de lecture :
    /// un parent d'un autre espace vaut la racine, et un nœud pris dans un cycle de parents, ou
    /// suspendu dessous, n'a pas de chaîne (vide aussi : personne ne le voit, R58).
    pub async fn chain_grants(&self, node_id: Uuid) -> Result<Vec<(Uuid, Grant)>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let chain = read_chain(&mut tx, &node_id.to_string()).await?;
        tx.commit().await?;
        let mut lines = chain.map(|chain| chain.lines).unwrap_or_default();
        // L'ordre du texte en base, comme le rendait `ORDER BY node_id, subject`.
        lines.sort_by_cached_key(|(on, line)| (on.clone(), line.subject.to_string()));
        lines
            .into_iter()
            .map(|(on, line)| Ok((parse_uuid(&on)?, line)))
            .collect()
    }

    /// Pose un accès sur le nœud : le niveau remplace celui que le sujet y avait. `NotFound` si
    /// le nœud n'existe pas.
    ///
    /// Sans la règle `last_full_access` (R70) ni contrôle de qui demande : pour les tests. Le
    /// partage d'une page passe par [`Store::change_access`].
    pub async fn set_grant(&self, node_id: Uuid, grant: Grant) -> Result<(), StoreError> {
        let id = node_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let space = space_of(&mut tx, &id).await?;
        write_grant(&mut tx, &id, grant).await?;
        bump_rev(&mut tx, &space).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Retire l'accès du sujet sur le nœud ; sans effet (et sans rien écrire) s'il n'en avait
    /// pas. `NotFound` si le nœud n'existe pas.
    ///
    /// Sans la règle `last_full_access` (R70) ni contrôle de qui demande : pour les tests. Le
    /// partage d'une page passe par [`Store::change_access`].
    pub async fn remove_grant(&self, node_id: Uuid, subject: Subject) -> Result<(), StoreError> {
        let id = node_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let space = space_of(&mut tx, &id).await?;
        let removed = sqlx::query("DELETE FROM acl WHERE node_id = ? AND subject = ?")
            .bind(&id)
            .bind(subject.to_string())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if removed > 0 {
            bump_rev(&mut tx, &space).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Change, au nom de `actor`, un accès posé sur la page, en une transaction : le partage
    /// d'une page. Tout ce sur quoi le changement repose est vérifié dans cette transaction
    /// (Ruling R87), sans dépendre de la révision de l'espace, et chaque refus a son cas
    /// ([`ShareRefusal`], dans `StoreError::Refused`), dans cet ordre :
    /// - la page existe pour `actor` (un rôle dans son espace, au moins `read` sur la page et
    ///   ses ancêtres), sinon `Unseen` ; il y a `full`, sinon `BelowFull` ;
    /// - l'accès général ne se pose ni ne se retire par un invité (Ruling R85) : `GuestGeneral` ;
    /// - ni la page ni un ancêtre n'est à la corbeille : `Trashed` ;
    /// - la personne qui reçoit un accès a un rôle dans l'espace (`NotInSpace`) ; celle dont on
    ///   retire l'accès a une ligne sur la page (`NoOwnLine`) ;
    /// - l'accès général a pour plancher la ligne `members` d'un ancêtre (Ruling R93, voir
    ///   `general_line`) : sous lui, ou `null` sans ligne propre, `InheritedAccess` ; à son
    ///   niveau, la ligne propre part ; au-dessus, elle est posée ; jamais de ligne redondante ;
    /// - Ruling R70 : après le changement, la page garde quelqu'un en `full`, par ses lignes ou
    ///   par celles d'un ancêtre (une ligne `members` compte ; une ligne restée à quelqu'un sans
    ///   rôle, non) ; sinon `LastFullAccess`.
    ///
    /// Aucun refus n'écrit rien. Retirer la ligne d'un invité qui n'en a plus aucune autre dans
    /// l'espace l'en fait sortir, avec ses invitations en attente. Une ligne déjà ainsi n'écrit
    /// rien (`changed` faux) : la révision ne bouge pas.
    pub async fn change_access(
        &self,
        node_id: Uuid,
        change: AccessChange,
        actor: Uuid,
    ) -> Result<AccessWritten, StoreError> {
        let node = node_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let general = matches!(change, AccessChange::Members(_));
        let chain = check_sharer(&mut tx, &node, actor, general).await?;
        let space_id = parse_uuid(&chain.space)?;
        // La ligne de la page après le changement : posée à ce niveau, ou retirée (`None`).
        let (subject, line) = match change {
            AccessChange::User(target, Some(level)) => {
                if role_of(&mut tx, &chain.space, &target.to_string())
                    .await?
                    .is_none()
                {
                    return Err(StoreError::Refused(ShareRefusal::NotInSpace));
                }
                (Subject::User(target), Some(level))
            }
            AccessChange::User(target, None) => {
                let subject = Subject::User(target);
                if !chain.has_own(&node, subject) {
                    return Err(StoreError::Refused(ShareRefusal::NoOwnLine));
                }
                (subject, None)
            }
            AccessChange::Members(wanted) => {
                (Subject::Members, general_line(&chain, &node, wanted)?)
            }
        };
        let changed = match line {
            Some(level) => write_grant(&mut tx, &node, Grant { subject, level }).await?,
            None => {
                sqlx::query("DELETE FROM acl WHERE node_id = ? AND subject = ?")
                    .bind(&node)
                    .bind(subject.to_string())
                    .execute(&mut *tx)
                    .await?
                    .rows_affected()
                    > 0
            }
        };
        let unchanged = AccessWritten {
            space_id,
            changed: false,
            left_space: false,
            line,
        };
        if !changed {
            return Ok(unchanged);
        }
        let left_space = match change {
            AccessChange::User(user, None) => {
                guest_leaves_if_bare(&mut tx, &chain.space, user).await?
            }
            _ => false,
        };
        if !keeps_full(&mut tx, &chain.space, &node).await? {
            // La transaction, abandonnée, n'écrit rien.
            return Err(StoreError::LastFullAccess);
        }
        bump_rev(&mut tx, &chain.space).await?;
        tx.commit().await?;
        Ok(AccessWritten {
            changed: true,
            left_space,
            ..unchanged
        })
    }
}
