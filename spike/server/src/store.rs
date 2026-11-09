//! Persistance des documents Yjs : instantané + journal de mises à jour (SQLite).

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use yrs::updates::decoder::Decode;
use yrs::{Doc, OffsetKind, Options, Transact, Update};

/// Nombre de mises à jour après lequel on compacte en un instantané.
pub const COMPACT_EVERY: i64 = 500;

pub fn new_doc() -> Doc {
    Doc::with_options(Options { offset_kind: OffsetKind::Utf16, ..Default::default() })
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).ok();
            }
        }
        let url = format!("sqlite://{}", path.display());
        let opts = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await.context("ouverture SQLite")?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS doc_updates (
                page TEXT NOT NULL, seq INTEGER NOT NULL, update_blob BLOB NOT NULL,
                origin TEXT NOT NULL, created_at INTEGER NOT NULL, PRIMARY KEY(page, seq))",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS doc_snapshots (
                page TEXT PRIMARY KEY, seq INTEGER NOT NULL, state_blob BLOB NOT NULL)",
        )
        .execute(&pool)
        .await?;
        Ok(Store { pool })
    }

    /// Recharge un document : instantané puis mises à jour postérieures.
    pub async fn load_doc(&self, page: &str) -> Result<Doc> {
        let doc = new_doc();
        let snap = sqlx::query("SELECT seq, state_blob FROM doc_snapshots WHERE page = ?")
            .bind(page)
            .fetch_optional(&self.pool)
            .await?;
        let mut from_seq = 0i64;
        {
            let mut txn = doc.transact_mut();
            if let Some(row) = snap {
                from_seq = row.get::<i64, _>("seq");
                let blob: Vec<u8> = row.get("state_blob");
                txn.apply_update(Update::decode_v1(&blob)?)?;
            }
            let rows = sqlx::query("SELECT update_blob FROM doc_updates WHERE page = ? AND seq > ? ORDER BY seq")
                .bind(page)
                .bind(from_seq)
                .fetch_all(&self.pool)
                .await?;
            for row in rows {
                let blob: Vec<u8> = row.get("update_blob");
                txn.apply_update(Update::decode_v1(&blob)?)?;
            }
        }
        Ok(doc)
    }

    /// Ajoute une mise à jour au journal et renvoie son numéro de séquence.
    pub async fn append_update(&self, page: &str, update: &[u8], origin: &str) -> Result<i64> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as i64;
        let row = sqlx::query(
            "INSERT INTO doc_updates (page, seq, update_blob, origin, created_at)
             VALUES (?1, COALESCE((SELECT MAX(seq) FROM doc_updates WHERE page = ?1),
                                  (SELECT seq FROM doc_snapshots WHERE page = ?1), 0) + 1, ?2, ?3, ?4)
             RETURNING seq",
        )
        .bind(page)
        .bind(update)
        .bind(origin)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("seq"))
    }

    /// Nombre de mises à jour en attente de compaction pour une page.
    pub async fn pending_updates(&self, page: &str) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) AS n FROM doc_updates WHERE page = ?").bind(page).fetch_one(&self.pool).await?;
        Ok(row.get("n"))
    }

    /// Remplace le journal jusqu'à `upto_seq` par un instantané de l'état complet.
    pub async fn compact(&self, page: &str, state: &[u8], upto_seq: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO doc_snapshots (page, seq, state_blob) VALUES (?1, ?2, ?3)
             ON CONFLICT(page) DO UPDATE SET seq = excluded.seq, state_blob = excluded.state_blob",
        )
        .bind(page)
        .bind(upto_seq)
        .bind(state)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM doc_updates WHERE page = ? AND seq <= ?").bind(page).bind(upto_seq).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Remplace entièrement une page par un instantané (génération de pages de mesure).
    pub async fn put_snapshot(&self, page: &str, state: &[u8]) -> Result<()> {
        sqlx::query("DELETE FROM doc_updates WHERE page = ?").bind(page).execute(&self.pool).await?;
        self.compact(page, state, 0).await
    }

    /// (instantanés, mises à jour) stockés pour une page — utilisé par les tests et les mesures.
    pub async fn counts(&self, page: &str) -> Result<(i64, i64)> {
        let s: i64 = sqlx::query("SELECT COUNT(*) AS n FROM doc_snapshots WHERE page = ?").bind(page).fetch_one(&self.pool).await?.get("n");
        Ok((s, self.pending_updates(page).await?))
    }
}
