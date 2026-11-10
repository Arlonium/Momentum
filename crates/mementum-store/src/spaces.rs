//! Espaces : création, lecture, renommage, et la révision que suit le cache des accès.

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{Store, StoreError, now_ms, parse_uuid};

/// Un espace : ce qui porte les pages, les membres et leurs droits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    pub id: Uuid,
    pub name: String,
    /// Révision : incrémentée par toute écriture qui touche à la structure ou aux droits de
    /// l'espace (règle de `rev`, en tête de la crate).
    pub rev: i64,
    /// Millisecondes Unix.
    pub created_at: i64,
}

/// Colonnes d'un espace, en littéral pour que le SQL reste statique (`concat!`).
macro_rules! space_columns {
    () => {
        "id, name, rev, created_at"
    };
}

/// Relit un espace par les noms de colonnes `id`, `name`, `rev`, `created_at`.
pub(crate) fn space_from_row(r: &SqliteRow) -> Result<Space, StoreError> {
    Ok(Space {
        id: parse_uuid(&r.get::<String, _>("id"))?,
        name: r.get("name"),
        rev: r.get("rev"),
        created_at: r.get("created_at"),
    })
}

/// Incrémente la révision de l'espace. À appeler dans la transaction de toute écriture qui
/// touche à sa structure ou à ses droits (règle de `rev`).
pub(crate) async fn bump_rev(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
) -> Result<(), StoreError> {
    sqlx::query("UPDATE spaces SET rev = rev + 1 WHERE id = ?")
        .bind(space_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Écrit un espace neuf, de révision 0, dans la transaction.
pub(crate) async fn insert_space(
    tx: &mut Transaction<'_, Sqlite>,
    name: &str,
    created_by: Option<Uuid>,
) -> Result<Space, StoreError> {
    let space = Space {
        id: Uuid::now_v7(),
        name: name.to_string(),
        rev: 0,
        created_at: now_ms(),
    };
    sqlx::query(
        "INSERT INTO spaces (id, name, rev, created_by, created_at) VALUES (?, ?, 0, ?, ?)",
    )
    .bind(space.id.to_string())
    .bind(name)
    .bind(created_by.map(|u| u.to_string()))
    .bind(space.created_at)
    .execute(&mut **tx)
    .await
    .map_err(StoreError::from_fk)?;
    Ok(space)
}

impl Store {
    /// Crée un espace ; le nom est enregistré tel quel. `owner`, s'il est donné, en devient
    /// membre au rôle `owner` (la révision passe alors à 1 : une écriture dans `members`).
    /// `NotFound`, sans rien écrire, si ce compte n'existe pas.
    pub async fn create_space(&self, name: &str, owner: Option<Uuid>) -> Result<Space, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut space = insert_space(&mut tx, name, owner).await?;
        if let Some(owner) = owner {
            let id = space.id.to_string();
            sqlx::query(
                "INSERT INTO members (space_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)",
            )
            .bind(&id)
            .bind(owner.to_string())
            .bind(space.created_at)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::from_fk)?;
            bump_rev(&mut tx, &id).await?;
            space.rev += 1;
        }
        tx.commit().await?;
        Ok(space)
    }

    pub async fn get_space(&self, id: Uuid) -> Result<Option<Space>, StoreError> {
        sqlx::query(concat!(
            "SELECT ",
            space_columns!(),
            " FROM spaces WHERE id = ?"
        ))
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(space_from_row)
        .transpose()
    }

    /// Renomme un espace ; le nom est enregistré tel quel. La révision ne bouge pas : un nom ne
    /// change ni la structure ni les droits.
    pub async fn rename_space(&self, id: Uuid, name: &str) -> Result<Space, StoreError> {
        let row = sqlx::query(concat!(
            "UPDATE spaces SET name = ? WHERE id = ? RETURNING ",
            space_columns!()
        ))
        .bind(name)
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)?;
        space_from_row(&row)
    }

    /// La révision de l'espace ; `None` s'il n'existe pas.
    pub async fn space_rev(&self, id: Uuid) -> Result<Option<i64>, StoreError> {
        Ok(sqlx::query("SELECT rev FROM spaces WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .map(|r| r.get(0)))
    }
}
