//! Accès posés sur les pages (`acl`) : lecture, pose, retrait.
//!
//! Un sujet s'écrit dans `acl.subject` sous la forme texte de `mementum-acl` (`user:<uuid>` ou
//! `members`), un niveau sous celle de `Level::as_str`. Une ligne qui ne se relit pas est une
//! erreur, jamais une ligne sautée : un accès ignoré en silence fausserait le calcul des droits.

use mementum_acl::{Grant, Level, Subject};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::spaces::bump_rev;
use crate::{Store, StoreError, parse_uuid, unreadable};

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
/// avait. La révision de l'espace est l'affaire de l'appelant.
pub(crate) async fn write_grant(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
    grant: Grant,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO acl (node_id, subject, level) VALUES (?1, ?2, ?3) \
         ON CONFLICT(node_id, subject) DO UPDATE SET level = excluded.level",
    )
    .bind(node_id)
    .bind(grant.subject.to_string())
    .bind(grant.level.as_str())
    .execute(&mut **tx)
    .await
    .map_err(StoreError::from_fk)?;
    Ok(())
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
}
