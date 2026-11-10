//! Accès posés sur les pages (`acl`) : lecture, pose, retrait, et le changement d'un accès par
//! le partage d'une page ([`Store::change_access`]).
//!
//! Un sujet s'écrit dans `acl.subject` sous la forme texte de `mementum-acl` (`user:<uuid>` ou
//! `members`), un niveau sous celle de `Level::as_str`. Une ligne qui ne se relit pas est une
//! erreur, jamais une ligne sautée : un accès ignoré en silence fausserait le calcul des droits.

use mementum_acl::{Grant, Level, Subject};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::members::{guest_leaves_if_bare, keeps_full};
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccessWritten {
    /// Une ligne a été posée, changée ou retirée. Faux : elle était déjà ainsi ; rien n'a été
    /// écrit, et la révision de l'espace n'a pas bougé.
    pub changed: bool,
    /// La personne dont la ligne est retirée, invitée, n'a plus aucune ligne dans l'espace :
    /// elle en est sortie.
    pub left_space: bool,
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

impl Store {
    /// Tous les accès posés dans l'espace, corbeille comprise : `(nœud, accès)`, triés par
    /// nœud puis par sujet.
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

    /// Pose un accès sur le nœud : le niveau remplace celui que le sujet y avait. `NotFound` si
    /// le nœud n'existe pas.
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

    /// Change un accès posé sur la page, en une transaction, pour le partage d'une page :
    /// - l'appelant a décidé sur l'état de l'espace à la révision `expected_rev` (son droit de
    ///   partager, la personne visée…) : si l'espace n'y est plus, `Stale`, et rien n'est
    ///   écrit ; il relit et redécide ;
    /// - retirer la ligne d'un invité qui n'en a plus aucune autre dans l'espace l'en fait
    ///   sortir ;
    /// - Ruling R70 : après le changement, la page doit garder quelqu'un en `full`, par ses
    ///   lignes ou par celles d'un ancêtre (une ligne `members` compte ; une ligne restée à
    ///   quelqu'un sans rôle, non) ; sinon `LastFullAccess`, et rien n'est écrit.
    ///
    /// Une ligne déjà ainsi (même niveau, ou absente à retirer) n'écrit rien (`changed` faux) :
    /// la révision ne bouge pas. `NotFound` si le nœud n'existe pas.
    pub async fn change_access(
        &self,
        node_id: Uuid,
        change: AccessChange,
        expected_rev: i64,
    ) -> Result<AccessWritten, StoreError> {
        let node = node_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let space = space_of(&mut tx, &node).await?;
        let rev: i64 = sqlx::query("SELECT rev FROM spaces WHERE id = ?")
            .bind(&space)
            .fetch_one(&mut *tx)
            .await?
            .get(0);
        if rev != expected_rev {
            return Err(StoreError::Stale);
        }
        let (subject, level) = match change {
            AccessChange::Members(level) => (Subject::Members, level),
            AccessChange::User(user, level) => (Subject::User(user), level),
        };
        let changed = match level {
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
        if !changed {
            return Ok(AccessWritten::default());
        }
        let left_space = match change {
            AccessChange::User(user, None) => guest_leaves_if_bare(&mut tx, &space, user).await?,
            _ => false,
        };
        if !keeps_full(&mut tx, &space, &node).await? {
            // La transaction, abandonnée, n'écrit rien.
            return Err(StoreError::LastFullAccess);
        }
        bump_rev(&mut tx, &space).await?;
        tx.commit().await?;
        Ok(AccessWritten {
            changed,
            left_space,
        })
    }
}
