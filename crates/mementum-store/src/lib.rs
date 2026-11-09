//! Stockage SQLite de Mementum : arbre de pages, journal Yjs et instantanés.
//!
//! - [`Store`] ouvre la base (WAL, `synchronous=FULL`, clés étrangères, délai d'attente de 5 s,
//!   appliqués à **chaque** connexion du pool) et lance les migrations.
//! - `nodes.rs` : l'arbre de pages, rangé par positions fractionnaires.
//! - `docs.rs` : le journal des mises à jour Yjs et les instantanés, qui ne reculent jamais.
//! - `position.rs` : l'index fractionnaire.
//!
//! Les horodatages sont en millisecondes Unix. Les identifiants sont des UUIDv7 stockés en
//! `TEXT`. Toutes les requêtes sont écrites à l'exécution, valeurs liées en paramètres.

mod docs;
mod nodes;
mod position;

use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use uuid::Uuid;

pub use docs::DocState;
pub use nodes::{NewNode, Node, NodeKind, Space};
pub use position::position_between;

/// Erreurs du store.
#[derive(Debug)]
pub enum StoreError {
    /// Un nœud, un parent ou un frère de référence (`after`) n'existe pas.
    NotFound,
    /// Le déplacement ferait d'un nœud son propre descendant.
    Cycle,
    /// Erreur de la base.
    Sqlx(sqlx::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound => write!(f, "introuvable"),
            StoreError::Cycle => write!(f, "un nœud ne peut pas devenir son propre descendant"),
            StoreError::Sqlx(e) => write!(f, "erreur de base de données : {e}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Sqlx(e) => Some(e),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        StoreError::Sqlx(e)
    }
}

impl From<sqlx::migrate::MigrateError> for StoreError {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        StoreError::Sqlx(e.into())
    }
}

impl StoreError {
    /// Une clé étrangère violée veut dire que le nœud visé n'existe pas.
    pub(crate) fn from_fk(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::Database(d) if d.is_foreign_key_violation() => StoreError::NotFound,
            _ => StoreError::Sqlx(e),
        }
    }
}

/// La base de Mementum. Se clone à bas coût (le pool est partagé).
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Ouvre (et crée au besoin, dossier compris) la base, applique les pragmas à chaque
    /// connexion et lance les migrations.
    pub async fn open(path: &Path) -> Result<Store, StoreError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| StoreError::Sqlx(sqlx::Error::Io(e)))?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_millis(5000));
        let pool = SqlitePoolOptions::new().connect_with(options).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Store { pool })
    }

    /// Ferme le pool (attend la fin des requêtes en cours).
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Valeurs des pragmas de la connexion qui répond : `(journal_mode, synchronous,
    /// foreign_keys, busy_timeout)`. Sert aux tests.
    #[doc(hidden)]
    pub async fn pragmas(&self) -> Result<(String, i64, i64, i64), StoreError> {
        use sqlx::Row;
        let mut conn = self.pool.acquire().await?;
        let mut out = (String::new(), 0, 0, 0);
        out.0 = sqlx::query("PRAGMA journal_mode")
            .fetch_one(&mut *conn)
            .await?
            .get(0);
        out.1 = sqlx::query("PRAGMA synchronous")
            .fetch_one(&mut *conn)
            .await?
            .get(0);
        out.2 = sqlx::query("PRAGMA foreign_keys")
            .fetch_one(&mut *conn)
            .await?
            .get(0);
        out.3 = sqlx::query("PRAGMA busy_timeout")
            .fetch_one(&mut *conn)
            .await?
            .get(0);
        Ok(out)
    }
}

/// Millisecondes Unix.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Relit un UUID stocké en `TEXT`.
pub(crate) fn parse_uuid(s: &str) -> Result<Uuid, StoreError> {
    Uuid::from_str(s).map_err(|e| StoreError::Sqlx(sqlx::Error::Decode(Box::new(e))))
}
