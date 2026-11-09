//! Journal des mises à jour Yjs et instantanés.

use sqlx::Row;
use uuid::Uuid;

use crate::{Store, StoreError, now_ms};

/// État relisible d'une page : l'instantané (s'il existe) puis les mises à jour postérieures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocState {
    /// `(seq, état)` de l'instantané.
    pub snapshot: Option<(i64, Vec<u8>)>,
    /// `(seq, mise à jour)` de `seq` strictement supérieur à celui de l'instantané, en ordre.
    pub updates: Vec<(i64, Vec<u8>)>,
}

impl Store {
    /// Ajoute une mise à jour au journal de la page et renvoie le `seq` attribué :
    /// `max(plus grand seq du journal, seq de l'instantané, 0) + 1`, en un seul `INSERT … SELECT`.
    /// Le `seq` repart donc de l'instantané quand une compaction a vidé le journal.
    pub async fn append_update(&self, node_id: Uuid, update: &[u8]) -> Result<i64, StoreError> {
        let id = node_id.to_string();
        let row = sqlx::query(
            "INSERT INTO doc_updates (node_id, seq, update_blob, created_at) \
             SELECT ?1, \
                    MAX(COALESCE((SELECT MAX(seq) FROM doc_updates WHERE node_id = ?1), 0), \
                        COALESCE((SELECT seq FROM doc_snapshots WHERE node_id = ?1), 0)) + 1, \
                    ?2, ?3 \
             RETURNING seq",
        )
        .bind(&id)
        .bind(update)
        .bind(now_ms())
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from_fk)?;
        Ok(row.get(0))
    }

    /// Relit l'instantané et les mises à jour de `seq` supérieur au sien (vue cohérente).
    pub async fn load_doc(&self, node_id: Uuid) -> Result<DocState, StoreError> {
        let id = node_id.to_string();
        let mut tx = self.pool.begin().await?;
        let snapshot = sqlx::query("SELECT seq, state_blob FROM doc_snapshots WHERE node_id = ?")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?
            .map(|r| (r.get::<i64, _>(0), r.get::<Vec<u8>, _>(1)));
        let floor = snapshot.as_ref().map_or(0, |(seq, _)| *seq);
        let updates = sqlx::query(
            "SELECT seq, update_blob FROM doc_updates WHERE node_id = ? AND seq > ? ORDER BY seq",
        )
        .bind(&id)
        .bind(floor)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|r| (r.get::<i64, _>(0), r.get::<Vec<u8>, _>(1)))
        .collect();
        tx.commit().await?;
        Ok(DocState { snapshot, updates })
    }

    /// Range un instantané qui couvre les mises à jour jusqu'à `seq`. Un instantané plus ancien
    /// que celui déjà gardé est ignoré. Les mises à jour de `seq` inférieur ou égal à celui de
    /// l'instantané **gardé** sont supprimées, le tout dans une transaction.
    pub async fn put_snapshot(
        &self,
        node_id: Uuid,
        seq: i64,
        state: &[u8],
    ) -> Result<(), StoreError> {
        let id = node_id.to_string();
        // IMMEDIATE : on prend le verrou d'écriture d'emblée, sans montée de verrou qui échouerait.
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "INSERT INTO doc_snapshots (node_id, seq, state_blob, created_at) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(node_id) DO UPDATE SET \
                 seq = excluded.seq, state_blob = excluded.state_blob, created_at = excluded.created_at \
             WHERE excluded.seq > doc_snapshots.seq",
        )
        .bind(&id)
        .bind(seq)
        .bind(state)
        .bind(now_ms())
        .execute(&mut *tx)
        .await
        .map_err(StoreError::from_fk)?;
        let kept: i64 = sqlx::query("SELECT seq FROM doc_snapshots WHERE node_id = ?")
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?
            .get(0);
        sqlx::query("DELETE FROM doc_updates WHERE node_id = ? AND seq <= ?")
            .bind(&id)
            .bind(kept)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
