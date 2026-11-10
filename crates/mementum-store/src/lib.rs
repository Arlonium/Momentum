//! Stockage SQLite de Mementum : comptes et sessions, espaces et leurs membres, invitations,
//! arbre de pages, accès, journal Yjs et instantanés.
//!
//! - [`Store`] ouvre la base (WAL, `synchronous=FULL`, clés étrangères, délai d'attente de 5 s,
//!   appliqués à **chaque** connexion du pool) et lance les migrations.
//! - `backup.rs` : la copie de la base, à côté d'elle, avant toute migration en attente.
//! - `users.rs` : les comptes et les sessions (le store ne voit que des hachages).
//! - `spaces.rs` : les espaces et leur révision.
//! - `members.rs` : les rôles dans un espace, le départ, l'adoption des espaces sans membre.
//! - `invitations.rs` : les invitations dans un espace ou sur une page.
//! - `signup.rs` : l'inscription (compte, espaces repris, invitation, premier espace), en une
//!   transaction.
//! - `nodes.rs` : l'arbre de pages, rangé par positions fractionnaires ; la corbeille.
//! - `acl.rs` : les accès posés sur les pages.
//! - `docs.rs` : le journal des mises à jour Yjs et les instantanés, qui ne reculent jamais.
//! - `position.rs` : l'index fractionnaire.
//!
//! Les horodatages sont en millisecondes Unix. Les identifiants sont des UUIDv7 stockés en
//! `TEXT`. Toutes les requêtes sont écrites à l'exécution, valeurs liées en paramètres.
//!
//! **Règle de `rev`** : `spaces.rev` est la clé du cache des accès (`mementum-acl`), et rien
//! d'autre. Elle est incrémentée, dans la même transaction, par toute écriture qui change une
//! entrée du calcul des accès : la création et la suppression d'un nœud, son changement de
//! parent (passage à la racine compris), sa mise à la corbeille et sa restauration, et toute
//! écriture dans `acl` ou `members`. Une écriture qui l'oublierait laisserait des droits périmés
//! en cache. Ce qui ne change les droits de personne ne la touche pas (Ruling R83) : le titre
//! d'un nœud, sa place sous le même parent, et le contenu d'une page (journal, instantanés).
//! Sinon, chaque titre tapé viderait le cache de tout l'espace.

mod acl;
mod backup;
mod docs;
mod invitations;
mod members;
mod nodes;
mod position;
mod signup;
mod spaces;
mod users;

use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use uuid::Uuid;

pub use acl::{AccessChange, AccessWritten};
pub use backup::{Backup, backup_path};
pub use docs::DocState;
pub use invitations::{Invitation, InvitationGrant, NewInvitation};
pub use members::{Expected, Member};
pub use nodes::{NewNode, Node, NodeKind, Place, Sibling};
pub use position::position_between;
pub use signup::SignedUp;
pub use spaces::Space;
pub use users::{NewUser, User};

/// Erreurs du store.
#[derive(Debug)]
pub enum StoreError {
    /// Un espace, un nœud, un parent, un frère de référence (`after`) ou un compte n'existe
    /// pas ; la personne n'a pas de rôle dans l'espace ; l'invitation est inconnue, échue,
    /// révoquée ou déjà acceptée.
    NotFound,
    /// Le déplacement ferait d'un nœud son propre descendant.
    Cycle,
    /// Un compte porte déjà cette adresse e-mail.
    EmailTaken,
    /// L'espace perdrait son dernier propriétaire.
    LastOwner,
    /// Ce sur quoi l'appelant a décidé a changé depuis qu'il l'a lu : les rôles (`Expected`), ou
    /// la révision de l'espace (`Store::change_access`). Rien n'est écrit ; il relit et
    /// redécide.
    Stale,
    /// La page n'aurait plus personne en `full`, ni par ses lignes ni par un ancêtre (Ruling
    /// R70) : rien n'est écrit.
    LastFullAccess,
    /// Un ancêtre du nœud est à la corbeille : c'est lui qu'on restaure.
    ParentTrashed,
    /// Le nœud n'est pas à la corbeille : on ne supprime définitivement que ce qui y est.
    NotTrashed,
    /// Erreur de la base.
    Sqlx(sqlx::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound => write!(f, "introuvable"),
            StoreError::Cycle => write!(f, "un nœud ne peut pas devenir son propre descendant"),
            StoreError::EmailTaken => write!(f, "un compte porte déjà cette adresse e-mail"),
            StoreError::LastOwner => write!(f, "un espace garde au moins un propriétaire"),
            StoreError::Stale => write!(f, "les droits ont changé depuis leur lecture"),
            StoreError::LastFullAccess => {
                write!(f, "la page n'aurait plus personne en accès complet")
            }
            StoreError::ParentTrashed => write!(f, "une page parente est à la corbeille"),
            StoreError::NotTrashed => write!(f, "le nœud n'est pas à la corbeille"),
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
    /// Une clé étrangère violée veut dire que ce qu'on vise (un nœud, un compte) n'existe pas.
    pub(crate) fn from_fk(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::Database(d) if d.is_foreign_key_violation() => StoreError::NotFound,
            _ => StoreError::Sqlx(e),
        }
    }
}

/// Les migrations du store, embarquées dans le binaire.
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// La base de Mementum. Se clone à bas coût (le pool est partagé).
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    backup: Option<Arc<Backup>>,
}

impl Store {
    /// Ouvre (et crée au besoin, dossier compris) la base, applique les pragmas à chaque
    /// connexion et lance les migrations.
    ///
    /// Si le fichier existe déjà et qu'au moins une migration est en attente, une copie cohérente
    /// de la base est d'abord écrite à côté ([`backup_path`]), puis les migrations tournent. Une
    /// copie déjà rangée n'est jamais écrasée : si l'une d'elles est identique à la nouvelle, elle
    /// suffit ; sinon, la nouvelle prend le nom sans date s'il est libre, ou un nom daté. Si la
    /// copie échoue, l'ouverture échoue, avant toute migration. [`Store::backup`] dit ce qui a été
    /// fait.
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
        match migrate(&pool, path).await {
            Ok(backup) => Ok(Store {
                pool,
                backup: backup.map(Arc::new),
            }),
            Err(e) => {
                pool.close().await;
                Err(e)
            }
        }
    }

    /// La copie qui porte l'état de la base juste avant les migrations de cette ouverture : celle
    /// qui vient d'être écrite, ou celle qui était déjà là si elle lui est identique ; `None` si
    /// aucune migration n'était en attente. Le store n'a pas de journal : c'est à l'appelant de la
    /// journaliser.
    pub fn backup(&self) -> Option<&Backup> {
        self.backup.as_deref()
    }

    /// Ferme le pool (attend la fin des requêtes en cours).
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Valeurs des pragmas de `connections` connexions du pool, tenues toutes
    /// à la fois (donc distinctes) : `(journal_mode, synchronous,
    /// foreign_keys, busy_timeout)` pour chacune. Sert aux tests.
    #[doc(hidden)]
    pub async fn pragmas(
        &self,
        connections: usize,
    ) -> Result<Vec<(String, i64, i64, i64)>, StoreError> {
        use sqlx::Row;
        let mut held = Vec::with_capacity(connections);
        for _ in 0..connections {
            held.push(self.pool.acquire().await?);
        }
        let mut all = Vec::with_capacity(connections);
        for conn in &mut held {
            let mut out = (String::new(), 0, 0, 0);
            out.0 = sqlx::query("PRAGMA journal_mode")
                .fetch_one(&mut **conn)
                .await?
                .get(0);
            out.1 = sqlx::query("PRAGMA synchronous")
                .fetch_one(&mut **conn)
                .await?
                .get(0);
            out.2 = sqlx::query("PRAGMA foreign_keys")
                .fetch_one(&mut **conn)
                .await?
                .get(0);
            out.3 = sqlx::query("PRAGMA busy_timeout")
                .fetch_one(&mut **conn)
                .await?
                .get(0);
            all.push(out);
        }
        Ok(all)
    }
}

/// Copie la base si une migration est en attente, puis lance les migrations. Sans copie, rien ne
/// migre.
async fn migrate(pool: &SqlitePool, path: &Path) -> Result<Option<Backup>, StoreError> {
    let backup = backup::before_migrations(pool, path, &MIGRATOR).await?;
    MIGRATOR.run(pool).await?;
    Ok(backup)
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

/// Une valeur lue en base que le code ne sait pas relire : une erreur, jamais une ligne sautée.
pub(crate) fn unreadable(what: &str, text: &str) -> StoreError {
    StoreError::Sqlx(sqlx::Error::Decode(
        format!("{what} illisible : {text}").into(),
    ))
}
