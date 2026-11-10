//! Journal des mises à jour Yjs et instantanés.

use sqlx::{Row, SqliteExecutor};
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

/// Le `seq` où en est la page `?1` : `max(plus grand seq du journal, seq de l'instantané, 0)`.
/// En littéral, pour que le SQL reste statique (`concat!`).
macro_rules! last_seq {
    () => {
        "MAX(COALESCE((SELECT MAX(seq) FROM doc_updates WHERE node_id = ?1), 0), \
             COALESCE((SELECT seq FROM doc_snapshots WHERE node_id = ?1), 0))"
    };
}

/// Ajoute une mise à jour au journal de la page et rend le `seq` attribué : le `seq` où en est
/// la page, plus un, en un seul `INSERT … SELECT`. Le `seq` repart donc de l'instantané quand
/// une compaction a vidé le journal. `NotFound` si la page ou l'acteur n'existe pas.
async fn append<'e>(
    db: impl SqliteExecutor<'e>,
    node_id: &str,
    update: &[u8],
    actor: Option<&str>,
    now: i64,
) -> Result<i64, StoreError> {
    let row = sqlx::query(concat!(
        "INSERT INTO doc_updates (node_id, seq, update_blob, created_at, actor_id) \
         SELECT ?1, ",
        last_seq!(),
        " + 1, ?2, ?3, ?4 RETURNING seq"
    ))
    .bind(node_id)
    .bind(update)
    .bind(now)
    .bind(actor)
    .fetch_one(db)
    .await
    .map_err(StoreError::from_fk)?;
    Ok(row.get(0))
}

impl Store {
    /// Ajoute une mise à jour au journal de la page, sans acteur, et renvoie son `seq`.
    /// Transitoire : simple appel à [`Store::append_updates`]. Plus rien ne s'en sert que des
    /// tests du store (`tests/migration.rs`, `tests/nodes.rs`, `tests/spaces.rs`) : à retirer
    /// avec ces appels.
    pub async fn append_update(&self, node_id: Uuid, update: &[u8]) -> Result<i64, StoreError> {
        self.append_updates(node_id, &[update], None).await
    }

    /// Ajoute des mises à jour au journal de la page, dans l'ordre, et renvoie le `seq` de la
    /// dernière. Elles sont écrites toutes ou aucune, avec des `seq` qui se suivent. `actor` :
    /// le compte qui écrit (`None` : inconnu, ou le serveur). `NotFound` si la page ou le compte
    /// n'existe pas, même sans mise à jour. Sans mise à jour, rien n'est écrit : c'est le `seq`
    /// où en est la page qui est rendu (0 si elle n'a ni journal ni instantané).
    pub async fn append_updates(
        &self,
        node_id: Uuid,
        updates: &[&[u8]],
        actor: Option<Uuid>,
    ) -> Result<i64, StoreError> {
        let id = node_id.to_string();
        let actor = actor.map(|a| a.to_string());
        let actor = actor.as_deref();
        let now = now_ms();
        match updates {
            [] => {
                // Rien à écrire, donc aucune clé étrangère à violer : la page et le compte
                // sont cherchés ici.
                let row = sqlx::query(concat!(
                    "SELECT ",
                    last_seq!(),
                    " FROM nodes WHERE id = ?1 \
                     AND (?2 IS NULL OR EXISTS (SELECT 1 FROM users WHERE id = ?2))"
                ))
                .bind(&id)
                .bind(actor)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(StoreError::NotFound)?;
                Ok(row.get(0))
            }
            // Une seule instruction est déjà atomique : le chemin de chaque frappe ne paie pas
            // une transaction explicite.
            [update] => append(&self.pool, &id, update, actor, now).await,
            _ => {
                // IMMEDIATE : on prend le verrou d'écriture d'emblée, sans montée de verrou qui
                // échouerait.
                let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
                let mut seq = 0;
                for update in updates {
                    seq = append(&mut *tx, &id, update, actor, now).await?;
                }
                tx.commit().await?;
                Ok(seq)
            }
        }
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
