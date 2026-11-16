//! Stockage SQLite de Mementum : comptes et sessions, espaces et leurs membres, invitations,
//! arbre de pages, accès, fils de commentaires, fichiers des pages, favoris, journal Yjs,
//! instantanés et versions.
//!
//! - [`Store`] ouvre la base (WAL, `synchronous=FULL`, clés étrangères, délai d'attente de 5 s,
//!   appliqués à **chaque** connexion) et lance les migrations. Elle a deux pools : celui des
//!   lectures, dont chaque connexion est en `query_only`, et celui de la seule connexion qui
//!   écrit.
//! - `queue.rs` : la file d'écriture ; une écriture n'a la connexion qui écrit qu'à son tour,
//!   donné voie par voie (l'acteur d'une page, un compte nommé par le serveur, une voie
//!   commune).
//! - `backup.rs` : la copie de la base, à côté d'elle, avant toute migration en attente.
//! - `users.rs` : les comptes et les sessions (le store ne voit que des hachages).
//! - `spaces.rs` : les espaces, leur révision et leur version ; la transaction d'écriture qui
//!   annonce les versions avancées (`WriteTx`).
//! - `members.rs` : les rôles dans un espace, le départ, l'adoption des espaces sans membre.
//! - `invitations.rs` : les invitations dans un espace ou sur une page.
//! - `signup.rs` : l'inscription (compte, espaces repris, invitation, premier espace), en une
//!   transaction.
//! - `nodes.rs` : l'arbre de pages, rangé par positions fractionnaires ; la corbeille.
//! - `acl.rs` : les accès posés sur les pages.
//! - `access_inputs.rs` : ce que le calcul des accès lit d'un espace, en une transaction.
//! - `comments.rs` : les fils de commentaires d'une page, leurs messages et leurs réactions.
//! - `files.rs` : les fichiers d'une page, contenus rangés par leur hash et attaches ; le ménage.
//! - `favorites.rs` : les favoris d'un compte, qui n'avancent ni `rev` ni `lsn`.
//! - `duplicate.rs` : la copie d'un sous-arbre de pages, en une transaction.
//! - `docs.rs` : le journal des mises à jour Yjs et les instantanés, qui ne reculent jamais.
//! - `versions.rs` : les versions d'une page (état Yjs complet et auteurs), cent par page.
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
//!
//! **Règle de `lsn`** : `spaces.lsn` est la version de l'espace, que le serveur pousse aux
//! navigateurs pour qu'ils relisent la barre latérale (Ruling R133). Elle avance, dans la même
//! transaction, pour toute écriture qui change ce que la barre latérale montre : tout ce qui
//! avance `rev` (`bump_rev` avance les deux), plus le titre d'un nœud, sa place sous le même
//! parent et le nom de l'espace (`bump_lsn` ; Ruling R150 pour le nom). Le contenu d'une page ne
//! la touche pas. Elle n'avance que par ces deux fonctions, qui demandent une transaction
//! d'écriture du store (`WriteTx`, ouverte par `Store::write`). Toute transaction validée
//! annonce sur [`Store::changes`] les espaces qu'elle a avancés, après la validation, même si
//! l'appel est abandonné pendant qu'il l'attend (sqlx valide quand même ; Ruling R151). Dans ce
//! seul cas, une annonce peut partir pour une validation qui n'a finalement pas abouti, jamais
//! l'inverse : sans effet, puisque qui la reçoit relit la version et n'agit que si elle a monté.
//! Abandonnée avant sa validation, ou refusée, une transaction n'annonce rien. Aucune requête
//! n'écrit `lsn` ailleurs.

mod access_inputs;
mod acl;
mod backup;
mod comments;
mod docs;
mod duplicate;
mod favorites;
mod files;
mod invitations;
mod members;
mod nodes;
mod position;
mod queue;
mod signup;
mod spaces;
mod users;
mod versions;

use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use uuid::Uuid;

pub use access_inputs::AccessInputs;
pub use acl::{AccessChange, AccessWritten, ShareRefusal};
pub use backup::{Backup, backup_path};
pub use comments::{
    Author, Authorship, Comment, CommentLimit, MAX_MESSAGES_PER_ACCOUNT, MAX_MESSAGES_PER_PAGE,
    MAX_MESSAGES_PER_THREAD, MAX_PAGE_REACTIONS_PER_ACCOUNT, MAX_REACTIONS_PER_ACCOUNT,
    MAX_REACTIONS_PER_MESSAGE, MAX_REACTIONS_PER_PAGE, MAX_TEXT_BYTES_PER_ACCOUNT,
    MAX_TEXT_BYTES_PER_PAGE, MAX_THREADS_PER_ACCOUNT, MAX_THREADS_PER_PAGE, Reaction, Thread,
    ThreadAuthors,
};
pub use docs::DocState;
pub use duplicate::{CopiedAttachment, CopiedPage, NewCopy};
pub use files::{Attachment, NewAttachment};
pub use invitations::{Invitation, InvitationGrant, NewInvitation};
pub use members::{Expected, Member};
pub use nodes::{AtRoot, NewNode, Node, NodeKind, Place, Sibling, Updated};
pub use position::position_between;
pub use queue::{attribute_writes_to, writer, writes_by};
pub use signup::SignedUp;
pub use spaces::Space;
pub use users::{NewSession, NewUser, User};
pub use versions::{MAX_BEFORE_RESTORE, MAX_VERSIONS, Unversioned, Version, VersionReason};

/// Erreurs du store.
#[derive(Debug)]
pub enum StoreError {
    /// Un espace, un nœud, un parent, un frère de référence (`after`) ou un compte n'existe
    /// pas ; la personne n'a pas de rôle dans l'espace ; l'invitation est inconnue, échue,
    /// révoquée ou déjà acceptée, ou son auteur n'a plus le rôle qu'il faut pour la faire
    /// (Ruling R69).
    NotFound,
    /// Le déplacement ferait d'un nœud son propre descendant.
    Cycle,
    /// Un compte porte déjà cette adresse e-mail.
    EmailTaken,
    /// L'espace perdrait son dernier propriétaire.
    LastOwner,
    /// Ce sur quoi l'appelant a décidé a changé depuis qu'il l'a lu : les rôles (`Expected`),
    /// le parent d'un nœud ([`Place::from`]), ou ce qu'il faut à qui passe un nœud à la racine
    /// ([`Store::update_node`]). Rien n'est écrit : il relit et redécide.
    Stale,
    /// La page n'aurait plus personne en `full`, ni par ses lignes ni par un ancêtre (Ruling
    /// R70) : rien n'est écrit.
    LastFullAccess,
    /// Un changement d'accès refusé ([`Store::change_access`]) : le cas, dans l'ordre où ils
    /// sont vérifiés. Rien n'est écrit.
    Refused(ShareRefusal),
    /// Un ancêtre du nœud est à la corbeille : c'est lui qu'on restaure.
    ParentTrashed,
    /// Le nœud n'est pas à la corbeille : on ne supprime définitivement que ce qui y est.
    NotTrashed,
    /// La page où l'on écrirait est à la corbeille, elle-même ou par une page parente, vu dans la
    /// transaction qui écrirait (une recopie de fichier, Ruling R577) : rien n'est écrit.
    Trashed,
    /// Le message n'est pas de ce compte, ou le fil n'a pas été ouvert par lui, alors que
    /// l'appelant le demandait (`Authorship::Of`) : rien n'est écrit.
    NotAuthor,
    /// Un plafond des commentaires est atteint (Ruling R204), vérifié dans la transaction qui
    /// écrirait : rien n'est écrit.
    CommentLimit(CommentLimit),
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
            StoreError::Stale => write!(
                f,
                "ce sur quoi la demande a été décidée a changé depuis sa lecture"
            ),
            StoreError::LastFullAccess => {
                write!(f, "la page n'aurait plus personne en accès complet")
            }
            StoreError::Refused(why) => write!(f, "changement d'accès refusé : {why}"),
            StoreError::ParentTrashed => write!(f, "une page parente est à la corbeille"),
            StoreError::NotTrashed => write!(f, "le nœud n'est pas à la corbeille"),
            StoreError::Trashed => write!(f, "la page est à la corbeille"),
            StoreError::NotAuthor => write!(f, "ce message, ou ce fil, est d'un autre compte"),
            StoreError::CommentLimit(limit) => write!(f, "plafond des commentaires : {limit}"),
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

/// Le délai d'attente d'une connexion sur un verrou de SQLite, par défaut.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Au-delà, par défaut, une écriture qui attend encore son tour, ou un tour tenu, se journalise.
pub const SLOW_WRITE: Duration = Duration::from_secs(5);

/// Les réglages d'ouverture du store ([`Store::open_with`]). Aucun n'a de variable ni d'option :
/// [`Store::open`] prend les défauts, et seuls les tests en changent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreOptions {
    /// Le délai d'attente de chaque connexion sur un verrou de SQLite ([`BUSY_TIMEOUT`] par
    /// défaut) : au-delà, la requête échoue (`database is locked`).
    pub busy_timeout: Duration,
    /// Un disque lent, simulé : chaque validation d'une écriture dure ce temps de plus, verrou
    /// d'écriture tenu (un crochet de validation de SQLite qui attend, sur le fil de la
    /// connexion). Nul par défaut : seuls les tests le posent, pour rendre sûre une attente qui ne
    /// se voyait que sur un disque lent (la CI sous Windows, Ruling R485).
    pub write_delay: Duration,
    /// Au-delà ([`SLOW_WRITE`] par défaut), une écriture qui attend encore son tour dans la file,
    /// ou un tour tenu, se journalise (avertissement, Ruling R518).
    pub slow_write: Duration,
}

impl Default for StoreOptions {
    fn default() -> Self {
        StoreOptions {
            busy_timeout: BUSY_TIMEOUT,
            write_delay: Duration::ZERO,
            slow_write: SLOW_WRITE,
        }
    }
}

/// La base de Mementum. Se clone à bas coût (les pools, la file d'écriture et le canal des
/// changements sont partagés).
#[derive(Clone)]
pub struct Store {
    /// Le pool des lectures : chaque connexion y est en `query_only`, une écriture y échoue.
    pool: SqlitePool,
    /// La seule connexion qui écrit, et la file de ceux qui attendent leur tour (`queue.rs`).
    writer: Arc<queue::Writer>,
    backup: Option<Arc<Backup>>,
    /// Les espaces dont la version vient d'avancer ([`Store::changes`]).
    changes: tokio::sync::broadcast::Sender<Uuid>,
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
        Store::open_with(path, StoreOptions::default()).await
    }

    /// [`Store::open`], avec ces réglages.
    pub async fn open_with(path: &Path, settings: StoreOptions) -> Result<Store, StoreError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| StoreError::Sqlx(sqlx::Error::Io(e)))?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(settings.busy_timeout);
        // La connexion qui écrit d'abord : la copie et les migrations passent par elle, et la base
        // est en WAL avant qu'une connexion en lecture seule ne s'ouvre.
        let writes = slow_disk(
            SqlitePoolOptions::new().max_connections(1),
            settings.write_delay,
        )
        .connect_with(options.clone())
        .await?;
        let backup = match migrate(&writes, path).await {
            Ok(backup) => backup,
            Err(e) => {
                writes.close().await;
                return Err(e);
            }
        };
        // `query_only` en dernier, après les autres pragmas de sqlx (`journal_mode` compris).
        let reads = match SqlitePoolOptions::new()
            .connect_with(options.pragma("query_only", "ON"))
            .await
        {
            Ok(reads) => reads,
            Err(e) => {
                writes.close().await;
                return Err(e.into());
            }
        };
        Ok(Store {
            pool: reads,
            writer: Arc::new(queue::Writer::new(writes, settings.slow_write)),
            backup: backup.map(Arc::new),
            changes: tokio::sync::broadcast::channel(spaces::CHANGES_CAPACITY).0,
        })
    }

    /// La copie qui porte l'état de la base juste avant les migrations de cette ouverture : celle
    /// qui vient d'être écrite, ou celle qui était déjà là si elle lui est identique ; `None` si
    /// aucune migration n'était en attente. Le store n'a pas de journal : c'est à l'appelant de la
    /// journaliser.
    pub fn backup(&self) -> Option<&Backup> {
        self.backup.as_deref()
    }

    /// Ferme les deux pools (attend la fin des requêtes en cours).
    pub async fn close(&self) {
        self.pool.close().await;
        self.writer.close().await;
    }

    /// Valeurs des pragmas de `connections` connexions du pool des lectures, tenues toutes à la
    /// fois (donc distinctes), puis de la connexion qui écrit, tenue avec un tour de la file :
    /// `(journal_mode, synchronous, foreign_keys, busy_timeout, query_only)` pour chacune. Sert
    /// aux tests.
    #[doc(hidden)]
    pub async fn pragmas(
        &self,
        connections: usize,
    ) -> Result<Vec<(String, i64, i64, i64, i64)>, StoreError> {
        let mut held = Vec::with_capacity(connections);
        for _ in 0..connections {
            held.push(self.pool.acquire().await?);
        }
        let mut all = Vec::with_capacity(connections + 1);
        for conn in &mut held {
            all.push(pragmas_of(conn).await?);
        }
        drop(held);
        let mut writes = self.writer.held_connection().await?;
        all.push(pragmas_of(&mut writes).await?);
        Ok(all)
    }

    /// Les écritures qui attendent leur tour dans la file. Pour les tests seulement (fonction
    /// `test-hooks`).
    #[cfg(feature = "test-hooks")]
    pub fn queued_writes(&self) -> usize {
        self.writer.queued()
    }

    /// Prend un tour de la file, dans la voie de la tâche, et le garde tant que la garde vit :
    /// aucune autre écriture du store ne passe pendant ce temps. Pour les tests seulement
    /// (fonction `test-hooks`), qui mettent ainsi des écritures en file dans un ordre connu.
    #[cfg(feature = "test-hooks")]
    pub async fn hold_writes(&self) -> WritesHeld {
        WritesHeld(self.writer.turn(queue::Lane::caller()).await)
    }
}

/// Un tour de la file tenu par un test ([`Store::hold_writes`]) : le lâcher le rend.
#[cfg(feature = "test-hooks")]
pub struct WritesHeld(#[allow(dead_code)] queue::Turn);

/// `(journal_mode, synchronous, foreign_keys, busy_timeout, query_only)` de la connexion.
async fn pragmas_of(
    conn: &mut sqlx::SqliteConnection,
) -> Result<(String, i64, i64, i64, i64), StoreError> {
    use sqlx::Row;
    let mut out = (String::new(), 0, 0, 0, 0);
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
    out.4 = sqlx::query("PRAGMA query_only")
        .fetch_one(&mut *conn)
        .await?
        .get(0);
    Ok(out)
}

/// Pose sur chaque connexion du pool le disque lent de [`StoreOptions::write_delay`] : un crochet
/// de validation qui attend `delay` sur le fil de la connexion, verrou d'écriture tenu. SQLite ne
/// l'appelle qu'à la validation d'une transaction qui écrit. Sans délai, rien n'est posé.
fn slow_disk(pool: SqlitePoolOptions, delay: Duration) -> SqlitePoolOptions {
    if delay.is_zero() {
        return pool;
    }
    pool.after_connect(move |conn, _| {
        Box::pin(async move {
            conn.lock_handle().await?.set_commit_hook(move || {
                std::thread::sleep(delay);
                true
            });
            Ok(())
        })
    })
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
