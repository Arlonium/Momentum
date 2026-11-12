//! Espaces : création, lecture, renommage ; la révision que suit le cache des accès, et la
//! version que le serveur pousse aux navigateurs, annoncée par [`WriteTx`] une fois validée.

use std::ops::{Deref, DerefMut};

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{Store, StoreError, now_ms, parse_uuid};

/// Combien d'annonces le canal des changements ([`Store::changes`]) garde pour un récepteur qui
/// n'a pas encore lu les précédentes. Une annonce par transaction validée et par espace avancé :
/// 1 024 couvrent une rafale d'écritures (un import, une suppression en masse) pendant qu'un
/// récepteur attend son tour, pour 16 octets chacune. Au-delà, le récepteur en retard reçoit
/// `Lagged` : il a manqué des annonces, et relit la version des espaces qu'il suit. Ce n'est pas
/// au store d'attendre les récepteurs lents.
pub(crate) const CHANGES_CAPACITY: usize = 1024;

/// Un espace : ce qui porte les pages, les membres et leurs droits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    pub id: Uuid,
    pub name: String,
    /// Révision : incrémentée par toute écriture qui change une entrée du calcul des accès de
    /// l'espace, et par elles seules (règle de `rev`, en tête de la crate).
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

/// Une transaction d'écriture du store (`BEGIN IMMEDIATE`, [`Store::write`]), qui retient les
/// espaces dont elle a avancé la version ([`bump_rev`], [`bump_lsn`]) et les annonce sur
/// [`Store::changes`] en se validant ([`WriteTx::commit`]), après la validation.
///
/// L'annonce part ainsi d'un seul endroit, par construction (règle de `lsn`, en tête de la
/// crate) :
/// - avancer la version demande une `WriteTx` : une écriture neuve qui l'avance ne peut pas
///   oublier de l'annoncer ;
/// - seul [`WriteTx::commit`] valide la transaction (la `Transaction` de sqlx n'est prêtée que
///   par référence, et la valider la consomme), et il n'annonce qu'une fois la validation
///   faite : un lecteur qui relit la version à l'annonce voit la nouvelle ;
/// - une `WriteTx` abandonnée (un refus, une erreur, un `?`) est annulée par sqlx et n'annonce
///   rien, même si elle avait avancé la version avant d'échouer.
///
/// Elle se prête comme la `Transaction` qu'elle tient : `&mut tx` pour les fonctions qui
/// prennent une `&mut Transaction`, `&mut **tx` pour une requête (`&mut ***tx` quand on ne tient
/// qu'une `&mut WriteTx`).
pub(crate) struct WriteTx {
    tx: Transaction<'static, Sqlite>,
    /// Les espaces dont la version a avancé, chacun une fois.
    advanced: Vec<Uuid>,
    changes: broadcast::Sender<Uuid>,
}

impl WriteTx {
    /// Valide la transaction, puis annonce chaque espace dont elle a avancé la version. Si la
    /// validation échoue, rien n'est annoncé.
    pub(crate) async fn commit(self) -> Result<(), StoreError> {
        self.tx.commit().await?;
        for space in self.advanced {
            // Sans récepteur, personne n'est à prévenir : ce n'est pas une erreur.
            let _ = self.changes.send(space);
        }
        Ok(())
    }

    /// Retient `space_id`, dont la transaction vient d'avancer la version.
    fn advanced(&mut self, space_id: &str) -> Result<(), StoreError> {
        let id = parse_uuid(space_id)?;
        if !self.advanced.contains(&id) {
            self.advanced.push(id);
        }
        Ok(())
    }
}

impl Deref for WriteTx {
    type Target = Transaction<'static, Sqlite>;

    fn deref(&self) -> &Self::Target {
        &self.tx
    }
}

impl DerefMut for WriteTx {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tx
    }
}

/// Incrémente la révision de l'espace, et sa version avec elle. À appeler dans la transaction de
/// toute écriture qui change une entrée du calcul des accès (règle de `rev`).
pub(crate) async fn bump_rev(tx: &mut WriteTx, space_id: &str) -> Result<(), StoreError> {
    let bumped = sqlx::query("UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = ?")
        .bind(space_id)
        .execute(&mut *tx.tx)
        .await?
        .rows_affected();
    if bumped > 0 {
        tx.advanced(space_id)?;
    }
    Ok(())
}

/// Avance la version de l'espace seule : pour une écriture qui change ce que montre la barre
/// latérale sans changer les droits de personne (un titre, une place parmi les mêmes frères, le
/// nom de l'espace ; règle de `lsn`). Ce qui change les droits passe par [`bump_rev`].
pub(crate) async fn bump_lsn(tx: &mut WriteTx, space_id: &str) -> Result<(), StoreError> {
    let bumped = sqlx::query("UPDATE spaces SET lsn = lsn + 1 WHERE id = ?")
        .bind(space_id)
        .execute(&mut *tx.tx)
        .await?
        .rows_affected();
    if bumped > 0 {
        tx.advanced(space_id)?;
    }
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
    /// Ouvre une transaction d'écriture (`BEGIN IMMEDIATE` : le verrou d'écriture est pris
    /// d'emblée, sans montée de verrou qui échouerait), qui annoncera en se validant les espaces
    /// dont elle aura avancé la version ([`WriteTx`]).
    pub(crate) async fn write(&self) -> Result<WriteTx, StoreError> {
        Ok(WriteTx {
            tx: self.pool.begin_with("BEGIN IMMEDIATE").await?,
            advanced: Vec::new(),
            changes: self.changes.clone(),
        })
    }

    /// Crée un espace ; le nom est enregistré tel quel. `owner`, s'il est donné, en devient
    /// membre au rôle `owner` (la révision passe alors à 1 : une écriture dans `members`).
    /// `NotFound`, sans rien écrire, si ce compte n'existe pas.
    pub async fn create_space(&self, name: &str, owner: Option<Uuid>) -> Result<Space, StoreError> {
        let mut tx = self.write().await?;
        let mut space = insert_space(&mut tx, name, owner).await?;
        if let Some(owner) = owner {
            let id = space.id.to_string();
            sqlx::query(
                "INSERT INTO members (space_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)",
            )
            .bind(&id)
            .bind(owner.to_string())
            .bind(space.created_at)
            .execute(&mut **tx)
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
    /// change ni la structure ni les droits. La version avance : la barre latérale montre le nom
    /// de l'espace (Ruling R150).
    pub async fn rename_space(&self, id: Uuid, name: &str) -> Result<Space, StoreError> {
        let mut tx = self.write().await?;
        let row = sqlx::query(concat!(
            "UPDATE spaces SET name = ? WHERE id = ? RETURNING ",
            space_columns!()
        ))
        .bind(name)
        .bind(id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(StoreError::NotFound)?;
        let space = space_from_row(&row)?;
        bump_lsn(&mut tx, &id.to_string()).await?;
        tx.commit().await?;
        Ok(space)
    }

    /// La révision de l'espace ; `None` s'il n'existe pas.
    pub async fn space_rev(&self, id: Uuid) -> Result<Option<i64>, StoreError> {
        Ok(sqlx::query("SELECT rev FROM spaces WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .map(|r| r.get(0)))
    }

    /// La version de l'espace (règle de `lsn`) ; `None` s'il n'existe pas.
    pub async fn space_lsn(&self, id: Uuid) -> Result<Option<i64>, StoreError> {
        Ok(sqlx::query("SELECT lsn FROM spaces WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .map(|r| r.get(0)))
    }

    /// Les espaces dont la version vient d'avancer : un message par transaction validée et par
    /// espace qu'elle a avancé, envoyé après la validation ([`WriteTx`]). Un récepteur n'entend
    /// que ce qui est validé après sa création ; il entend les espaces de tout le store, et
    /// trie. Trop lent, il reçoit `Lagged` ([`CHANGES_CAPACITY`]) : il relit alors la version
    /// ([`Store::space_lsn`]) de ce qu'il suit.
    pub fn changes(&self) -> broadcast::Receiver<Uuid> {
        self.changes.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::broadcast::error::TryRecvError;

    use super::*;

    async fn store_with_space() -> (tempfile::TempDir, Store, Uuid) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("m.db")).await.unwrap();
        let space = store.create_space("Essais", None).await.unwrap().id;
        (dir, store, space)
    }

    /// Une transaction abandonnée après avoir avancé la version n'annonce rien, et n'avance rien.
    #[tokio::test]
    async fn dropped_write_announces_nothing() {
        let (_dir, store, space) = store_with_space().await;
        let mut changes = store.changes();
        let mut tx = store.write().await.unwrap();
        bump_lsn(&mut tx, &space.to_string()).await.unwrap();
        bump_rev(&mut tx, &space.to_string()).await.unwrap();
        drop(tx);
        assert_eq!(changes.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(store.space_lsn(space).await.unwrap(), Some(0));
        assert_eq!(store.space_rev(space).await.unwrap(), Some(0));
    }

    /// Validée, elle annonce une fois chaque espace avancé, quel que soit le nombre de pas ;
    /// un espace inconnu n'avance pas et n'est pas annoncé.
    #[tokio::test]
    async fn committed_write_announces_each_space_once() {
        let (_dir, store, space) = store_with_space().await;
        let other = store.create_space("Autre", None).await.unwrap().id;
        let mut changes = store.changes();
        let mut tx = store.write().await.unwrap();
        bump_lsn(&mut tx, &space.to_string()).await.unwrap();
        bump_rev(&mut tx, &other.to_string()).await.unwrap();
        bump_lsn(&mut tx, &space.to_string()).await.unwrap();
        bump_lsn(&mut tx, &Uuid::now_v7().to_string())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(changes.try_recv(), Ok(space));
        assert_eq!(changes.try_recv(), Ok(other));
        assert_eq!(changes.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(store.space_lsn(space).await.unwrap(), Some(2));
        assert_eq!(store.space_lsn(other).await.unwrap(), Some(1));
        assert_eq!(store.space_rev(other).await.unwrap(), Some(1));
    }

    /// Sans aucun récepteur, une annonce n'est pas une erreur.
    #[tokio::test]
    async fn announcement_without_receiver_is_not_an_error() {
        let (_dir, store, space) = store_with_space().await;
        let mut tx = store.write().await.unwrap();
        bump_lsn(&mut tx, &space.to_string()).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(store.space_lsn(space).await.unwrap(), Some(1));
    }
}
