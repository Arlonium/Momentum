//! Fils de commentaires d'une page (Ruling R134) : un fil est attaché à une page, et au plus à
//! un bloc par son identifiant ; il porte des messages, et chaque message des réactions. Rien
//! n'est écrit dans le document de la page.
//!
//! Le store ne juge ni le texte d'un message ni un emoji (les bornes sont celles de la route),
//! ni le niveau de la personne (`access.rs`, côté serveur), ni l'existence du bloc nommé (lue
//! dans le document vivant, par le serveur). Il vérifie, dans la transaction qui écrit, ce qui
//! ne se vérifie qu'avec l'écriture : le fil est bien de cette page, le message bien de ce fil,
//! et, quand la route le demande ([`Authorship::Of`]), le message est bien de ce compte, ou le
//! fil a bien été ouvert par lui. Sinon : [`StoreError::NotFound`] (le même, que l'identifiant
//! existe ailleurs ou nulle part) ou [`StoreError::NotAuthor`], et rien n'est écrit.
//!
//! Une écriture de commentaire n'avance ni `rev` ni `lsn` : elle ne change ni les droits ni ce
//! que montre la barre latérale. Ses transactions ne sont donc pas des `WriteTx`, et n'annoncent
//! rien ; c'est le serveur qui prévient les lecteurs de la page, après la validation.
//!
//! **Ordre de lecture.** Les fils, du plus ancien au plus récent ; les messages de même, dans
//! leur fil (un message modifié garde sa place) ; les réactions d'un message dans l'ordre où
//! chaque emoji y est apparu, et les comptes d'une réaction dans l'ordre où ils l'ont posée. À
//! égalité de date, l'identifiant départage les fils et les messages (UUIDv7, croissant dans le
//! processus), l'ordre d'insertion les réactions (`rowid`).
//!
//! **Plafonds** (Ruling R204), comptés dans la transaction qui écrirait : une page porte au plus
//! [`MAX_THREADS_PER_PAGE`] fils (résolus compris), un fil [`MAX_MESSAGES_PER_THREAD`] messages,
//! un message [`MAX_REACTIONS_PER_MESSAGE`] emojis différents. Au-delà :
//! [`StoreError::CommentLimit`], et rien n'est écrit.
//!
//! **Suppression.** Les réactions partent avec leur message, les messages avec leur fil, les
//! fils avec leur page (`ON DELETE CASCADE`, migration `0003`) ; un fil part avec son dernier
//! message, dans la même transaction.

use std::collections::HashMap;

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use uuid::Uuid;

use crate::{Store, StoreError, now_ms, parse_uuid};

/// Fils qu'une page porte au plus, résolus compris (Ruling R204).
pub const MAX_THREADS_PER_PAGE: u32 = 2_000;

/// Messages qu'un fil porte au plus, le premier compris (Ruling R204).
pub const MAX_MESSAGES_PER_THREAD: u32 = 500;

/// Emojis différents qu'un message porte au plus (Ruling R204) ; chaque compte peut poser chacun
/// d'eux.
pub const MAX_REACTIONS_PER_MESSAGE: u32 = 32;

/// Le plafond des commentaires qu'une écriture dépasserait ([`StoreError::CommentLimit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentLimit {
    /// [`MAX_THREADS_PER_PAGE`] fils sur la page.
    Threads,
    /// [`MAX_MESSAGES_PER_THREAD`] messages dans le fil.
    Messages,
    /// [`MAX_REACTIONS_PER_MESSAGE`] emojis différents sur le message.
    Reactions,
}

impl std::fmt::Display for CommentLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommentLimit::Threads => write!(f, "{MAX_THREADS_PER_PAGE} fils sur la page"),
            CommentLimit::Messages => write!(f, "{MAX_MESSAGES_PER_THREAD} messages dans le fil"),
            CommentLimit::Reactions => {
                write!(
                    f,
                    "{MAX_REACTIONS_PER_MESSAGE} emojis différents sur le message"
                )
            }
        }
    }
}

/// Un compte, tel qu'un fil le nomme : son identifiant et son nom, jamais son adresse. Un
/// compte qui n'est plus membre de l'espace reste l'auteur de ses messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    pub id: Uuid,
    /// Le nom du compte, tel qu'il est en base.
    pub name: String,
}

/// Les comptes qui ont posé un même emoji sur un message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaction {
    pub emoji: String,
    /// Dans l'ordre où ils l'ont posé.
    pub users: Vec<Author>,
}

/// Un message d'un fil.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub id: Uuid,
    pub author: Author,
    pub body: String,
    /// Millisecondes Unix.
    pub created_at: i64,
    /// Millisecondes Unix : la dernière modification du texte ; `None` s'il n'a jamais changé.
    pub edited_at: Option<i64>,
    pub reactions: Vec<Reaction>,
}

/// Un fil de commentaires, avec ses messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: Uuid,
    /// Le bloc auquel le fil est attaché ; `None` : un commentaire de la page. Il reste tel quel
    /// quand le bloc disparaît du document.
    pub block_id: Option<String>,
    /// Qui l'a ouvert : il le reste quand son premier message est supprimé.
    pub opened_by: Author,
    /// Millisecondes Unix.
    pub created_at: i64,
    /// Millisecondes Unix ; `None` : le fil est ouvert.
    pub resolved_at: Option<i64>,
    pub resolved_by: Option<Author>,
    pub comments: Vec<Comment>,
}

/// Ce que la route lit d'un fil avant d'écrire, pour dire ses refus dans l'ordre
/// ([`Store::thread_authors`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadAuthors {
    /// Qui a ouvert le fil.
    pub opened_by: Uuid,
    /// L'auteur du message demandé ; `None` si le fil n'a pas ce message, ou si aucun n'était
    /// demandé.
    pub comment_author: Option<Uuid>,
}

/// Qui peut toucher à un message (le supprimer) ou à un fil (le résoudre), d'après ce que la
/// route a décidé du niveau de la personne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authorship {
    /// Seulement ce compte : il doit être l'auteur du message, ou avoir ouvert le fil. Vérifié
    /// dans la transaction qui écrit ([`StoreError::NotAuthor`]).
    Of(Uuid),
    /// N'importe qui : le niveau de la personne suffit.
    Any,
}

/// Colonnes d'un fil, avec le nom de qui l'a ouvert et de qui l'a résolu. En littéral, pour que
/// le SQL reste statique (`concat!`).
macro_rules! thread_select {
    () => {
        "SELECT t.id, t.block_id, t.created_by, o.name AS opened_name, t.created_at, \
                t.resolved_at, t.resolved_by, u.name AS resolved_name \
         FROM comment_threads t JOIN users o ON o.id = t.created_by \
         LEFT JOIN users u ON u.id = t.resolved_by "
    };
}

/// Colonnes d'un message, avec son fil et le nom de son auteur.
macro_rules! comment_select {
    () => {
        "SELECT c.id, c.thread_id, c.author_id, u.name AS author_name, c.body, c.created_at, \
                c.edited_at \
         FROM comments c JOIN users u ON u.id = c.author_id "
    };
}

/// Colonnes d'une réaction, avec le nom de son compte.
macro_rules! reaction_select {
    () => {
        "SELECT r.comment_id, r.emoji, r.user_id, u.name AS user_name \
         FROM comment_reactions r JOIN users u ON u.id = r.user_id "
    };
}

fn thread_from_row(r: &SqliteRow) -> Result<Thread, StoreError> {
    let resolved_by = r
        .get::<Option<String>, _>("resolved_by")
        .map(|id| {
            Ok::<_, StoreError>(Author {
                id: parse_uuid(&id)?,
                // Le compte existe (clé étrangère) : son nom aussi.
                name: r
                    .get::<Option<String>, _>("resolved_name")
                    .unwrap_or_default(),
            })
        })
        .transpose()?;
    Ok(Thread {
        id: parse_uuid(&r.get::<String, _>("id"))?,
        block_id: r.get("block_id"),
        opened_by: Author {
            id: parse_uuid(&r.get::<String, _>("created_by"))?,
            name: r.get("opened_name"),
        },
        created_at: r.get("created_at"),
        resolved_at: r.get("resolved_at"),
        resolved_by,
        comments: Vec::new(),
    })
}

/// Un message, sans ses réactions, et l'identifiant de son fil.
fn comment_from_row(r: &SqliteRow) -> Result<(Uuid, Comment), StoreError> {
    let thread = parse_uuid(&r.get::<String, _>("thread_id"))?;
    let comment = Comment {
        id: parse_uuid(&r.get::<String, _>("id"))?,
        author: Author {
            id: parse_uuid(&r.get::<String, _>("author_id"))?,
            name: r.get("author_name"),
        },
        body: r.get("body"),
        created_at: r.get("created_at"),
        edited_at: r.get("edited_at"),
        reactions: Vec::new(),
    };
    Ok((thread, comment))
}

/// Range les réactions `rows` (lues dans leur ordre) sous leurs messages.
fn attach_reactions(
    comments: &mut [(Uuid, Comment)],
    rows: &[SqliteRow],
) -> Result<(), StoreError> {
    let index: HashMap<Uuid, usize> = comments
        .iter()
        .enumerate()
        .map(|(i, (_, comment))| (comment.id, i))
        .collect();
    // La place de chaque emoji parmi les réactions de son message : une table, et non une
    // recherche dans la liste, quel que soit le nombre d'emojis d'un message.
    let mut places: HashMap<(usize, String), usize> = HashMap::new();
    for r in rows {
        let comment_id = parse_uuid(&r.get::<String, _>("comment_id"))?;
        // Lues dans la même transaction que les messages : chacune a le sien.
        let Some(&at) = index.get(&comment_id) else {
            continue;
        };
        let emoji: String = r.get("emoji");
        let user = Author {
            id: parse_uuid(&r.get::<String, _>("user_id"))?,
            name: r.get("user_name"),
        };
        let reactions = &mut comments[at].1.reactions;
        let place = *places.entry((at, emoji.clone())).or_insert_with(|| {
            reactions.push(Reaction {
                emoji,
                users: Vec::new(),
            });
            reactions.len() - 1
        });
        reactions[place].users.push(user);
    }
    Ok(())
}

/// Range les messages (lus dans leur ordre) sous leurs fils.
fn attach_comments(threads: &mut [Thread], comments: Vec<(Uuid, Comment)>) {
    let index: HashMap<Uuid, usize> = threads
        .iter()
        .enumerate()
        .map(|(i, thread)| (thread.id, i))
        .collect();
    for (thread, comment) in comments {
        if let Some(&at) = index.get(&thread) {
            threads[at].comments.push(comment);
        }
    }
}

/// Les fils de la page, avec leurs messages et leurs réactions, sur une même connexion (dans
/// une transaction, pour une vue cohérente).
async fn read_threads(
    conn: &mut SqliteConnection,
    node_id: &str,
) -> Result<Vec<Thread>, StoreError> {
    let mut threads = sqlx::query(concat!(
        thread_select!(),
        "WHERE t.node_id = ? ORDER BY t.created_at, t.id"
    ))
    .bind(node_id)
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(thread_from_row)
    .collect::<Result<Vec<_>, _>>()?;
    let mut comments = sqlx::query(concat!(
        comment_select!(),
        "JOIN comment_threads t ON t.id = c.thread_id \
         WHERE t.node_id = ? ORDER BY c.created_at, c.id"
    ))
    .bind(node_id)
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(comment_from_row)
    .collect::<Result<Vec<_>, _>>()?;
    let reactions = sqlx::query(concat!(
        reaction_select!(),
        "JOIN comments c ON c.id = r.comment_id JOIN comment_threads t ON t.id = c.thread_id \
         WHERE t.node_id = ? ORDER BY r.created_at, r.rowid"
    ))
    .bind(node_id)
    .fetch_all(&mut *conn)
    .await?;
    attach_reactions(&mut comments, &reactions)?;
    attach_comments(&mut threads, comments);
    Ok(threads)
}

/// Le fil `thread_id`, avec ses messages et leurs réactions. `NotFound` s'il n'existe pas.
async fn read_thread(conn: &mut SqliteConnection, thread_id: &str) -> Result<Thread, StoreError> {
    let row = sqlx::query(concat!(thread_select!(), "WHERE t.id = ?"))
        .bind(thread_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(StoreError::NotFound)?;
    let mut threads = [thread_from_row(&row)?];
    let mut comments = sqlx::query(concat!(
        comment_select!(),
        "WHERE c.thread_id = ? ORDER BY c.created_at, c.id"
    ))
    .bind(thread_id)
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(comment_from_row)
    .collect::<Result<Vec<_>, _>>()?;
    let reactions = sqlx::query(concat!(
        reaction_select!(),
        "JOIN comments c ON c.id = r.comment_id \
         WHERE c.thread_id = ? ORDER BY r.created_at, r.rowid"
    ))
    .bind(thread_id)
    .fetch_all(&mut *conn)
    .await?;
    attach_reactions(&mut comments, &reactions)?;
    attach_comments(&mut threads, comments);
    let [thread] = threads;
    Ok(thread)
}

/// Le message `comment_id`, avec ses réactions. `NotFound` s'il n'existe pas.
async fn read_comment(
    conn: &mut SqliteConnection,
    comment_id: &str,
) -> Result<Comment, StoreError> {
    let row = sqlx::query(concat!(comment_select!(), "WHERE c.id = ?"))
        .bind(comment_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(StoreError::NotFound)?;
    let mut comments = [comment_from_row(&row)?];
    let reactions = sqlx::query(concat!(
        reaction_select!(),
        "WHERE r.comment_id = ? ORDER BY r.created_at, r.rowid"
    ))
    .bind(comment_id)
    .fetch_all(&mut *conn)
    .await?;
    attach_reactions(&mut comments, &reactions)?;
    let [(_, comment)] = comments;
    Ok(comment)
}

/// L'auteur du message `comment_id`, s'il est du fil `thread_id` et que ce fil est de la page
/// `node_id`. `NotFound` sinon.
async fn comment_author(
    conn: &mut SqliteConnection,
    node_id: &str,
    thread_id: &str,
    comment_id: &str,
) -> Result<Uuid, StoreError> {
    let row = sqlx::query(
        "SELECT c.author_id FROM comments c JOIN comment_threads t ON t.id = c.thread_id \
         WHERE c.id = ? AND c.thread_id = ? AND t.node_id = ?",
    )
    .bind(comment_id)
    .bind(thread_id)
    .bind(node_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(StoreError::NotFound)?;
    parse_uuid(&row.get::<String, _>(0))
}

/// Qui a ouvert le fil `thread_id`, s'il est de la page `node_id`. `NotFound` sinon.
async fn thread_opener(
    conn: &mut SqliteConnection,
    node_id: &str,
    thread_id: &str,
) -> Result<Uuid, StoreError> {
    let row = sqlx::query("SELECT created_by FROM comment_threads WHERE id = ? AND node_id = ?")
        .bind(thread_id)
        .bind(node_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(StoreError::NotFound)?;
    parse_uuid(&row.get::<String, _>(0))
}

/// `Ok` si `who` permet à l'auteur `author` d'être touché.
fn allowed(who: Authorship, author: Uuid) -> Result<(), StoreError> {
    match who {
        Authorship::Of(user) if user != author => Err(StoreError::NotAuthor),
        _ => Ok(()),
    }
}

impl Store {
    /// Les fils de la page, résolus compris, avec leurs messages et leurs réactions, dans
    /// l'ordre de lecture (en tête du module), lus en une transaction. Une page sans fil, ou
    /// qui n'existe pas : aucun.
    pub async fn comment_threads(&self, node_id: Uuid) -> Result<Vec<Thread>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let threads = read_threads(&mut tx, &node_id.to_string()).await?;
        tx.commit().await?;
        Ok(threads)
    }

    /// Ouvre un fil sur la page, attaché au bloc `block_id` s'il est donné, avec son premier
    /// message, écrit par `author` ; les deux ensemble, ou rien. `NotFound` si la page ou le
    /// compte n'existe pas.
    pub async fn create_thread(
        &self,
        node_id: Uuid,
        block_id: Option<&str>,
        author: Uuid,
        body: &str,
    ) -> Result<Thread, StoreError> {
        let (thread_id, now) = (Uuid::now_v7().to_string(), now_ms());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        // Ruling R204 : le plafond se compte dans la transaction qui écrit, résolus compris.
        let threads: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM comment_threads WHERE node_id = ?")
                .bind(node_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        if threads >= i64::from(MAX_THREADS_PER_PAGE) {
            return Err(StoreError::CommentLimit(CommentLimit::Threads));
        }
        sqlx::query(
            "INSERT INTO comment_threads (id, node_id, block_id, created_by, created_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&thread_id)
        .bind(node_id.to_string())
        .bind(block_id)
        .bind(author.to_string())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(StoreError::from_fk)?;
        insert_comment(&mut tx, &thread_id, author, body, now).await?;
        let thread = read_thread(&mut tx, &thread_id).await?;
        tx.commit().await?;
        Ok(thread)
    }

    /// Ajoute au fil un message de `author`. `NotFound` si le fil n'est pas de cette page, ou
    /// si le compte n'existe pas.
    pub async fn reply_to_thread(
        &self,
        node_id: Uuid,
        thread_id: Uuid,
        author: Uuid,
        body: &str,
    ) -> Result<Comment, StoreError> {
        let thread_id = thread_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        thread_opener(&mut tx, &node_id.to_string(), &thread_id).await?;
        let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE thread_id = ?")
            .bind(&thread_id)
            .fetch_one(&mut *tx)
            .await?;
        if messages >= i64::from(MAX_MESSAGES_PER_THREAD) {
            return Err(StoreError::CommentLimit(CommentLimit::Messages));
        }
        let comment_id = insert_comment(&mut tx, &thread_id, author, body, now_ms()).await?;
        let comment = read_comment(&mut tx, &comment_id).await?;
        tx.commit().await?;
        Ok(comment)
    }

    /// Qui a ouvert le fil, et qui a écrit son message `comment_id` s'il est demandé : ce que
    /// la route lit avant d'écrire, pour dire ses refus dans l'ordre. `None` si le fil n'est pas
    /// de cette page ; `comment_author: None` si le fil n'a pas ce message. Ce n'est pas ce qui
    /// autorise l'écriture : la transaction qui écrit le revérifie.
    pub async fn thread_authors(
        &self,
        node_id: Uuid,
        thread_id: Uuid,
        comment_id: Option<Uuid>,
    ) -> Result<Option<ThreadAuthors>, StoreError> {
        let row = sqlx::query(
            "SELECT t.created_by, c.author_id FROM comment_threads t \
             LEFT JOIN comments c ON c.thread_id = t.id AND c.id = ? \
             WHERE t.id = ? AND t.node_id = ?",
        )
        .bind(comment_id.map(|id| id.to_string()))
        .bind(thread_id.to_string())
        .bind(node_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(ThreadAuthors {
            opened_by: parse_uuid(&row.get::<String, _>(0))?,
            comment_author: row
                .get::<Option<String>, _>(1)
                .map(|id| parse_uuid(&id))
                .transpose()?,
        }))
    }

    /// Remplace le texte du message, s'il est de `author` : `NotAuthor` sinon. `NotFound` si le
    /// message n'est pas de ce fil, ou le fil de cette page. Le même texte qu'il a déjà ne change
    /// rien, pas même la date de modification : rend le message, et s'il a changé.
    pub async fn edit_comment(
        &self,
        node_id: Uuid,
        thread_id: Uuid,
        comment_id: Uuid,
        author: Uuid,
        body: &str,
    ) -> Result<(Comment, bool), StoreError> {
        let comment_id = comment_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let written_by = comment_author(
            &mut tx,
            &node_id.to_string(),
            &thread_id.to_string(),
            &comment_id,
        )
        .await?;
        allowed(Authorship::Of(author), written_by)?;
        // Textes comparés octet pour octet (collation `BINARY`, celle de SQLite par défaut).
        let changed = sqlx::query(
            "UPDATE comments SET body = ?, edited_at = ? WHERE id = ? AND body IS NOT ?",
        )
        .bind(body)
        .bind(now_ms())
        .bind(&comment_id)
        .bind(body)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let comment = read_comment(&mut tx, &comment_id).await?;
        tx.commit().await?;
        Ok((comment, changed > 0))
    }

    /// Supprime le message, et ses réactions ; si c'était le dernier du fil, le fil aussi, dans
    /// la même transaction. Rend `true` si le fil est parti. `NotFound` si le message n'est pas
    /// de ce fil, ou le fil de cette page ; `NotAuthor` si `who` demande un autre auteur.
    pub async fn delete_comment(
        &self,
        node_id: Uuid,
        thread_id: Uuid,
        comment_id: Uuid,
        who: Authorship,
    ) -> Result<bool, StoreError> {
        let (thread_id, comment_id) = (thread_id.to_string(), comment_id.to_string());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let written_by =
            comment_author(&mut tx, &node_id.to_string(), &thread_id, &comment_id).await?;
        allowed(who, written_by)?;
        sqlx::query("DELETE FROM comments WHERE id = ?")
            .bind(&comment_id)
            .execute(&mut *tx)
            .await?;
        let emptied = sqlx::query(
            "DELETE FROM comment_threads WHERE id = ? \
             AND NOT EXISTS (SELECT 1 FROM comments WHERE thread_id = ?)",
        )
        .bind(&thread_id)
        .bind(&thread_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(emptied > 0)
    }

    /// Résout le fil au nom de `by` (`resolved`), ou le rouvre. Un fil déjà dans cet état ne
    /// change pas, pas même qui l'a résolu : rend le fil, et s'il a changé. `NotFound` si le fil
    /// n'est pas de cette page ; `NotAuthor` si `who` demande qu'il ait été ouvert par un autre.
    pub async fn resolve_thread(
        &self,
        node_id: Uuid,
        thread_id: Uuid,
        by: Uuid,
        resolved: bool,
        who: Authorship,
    ) -> Result<(Thread, bool), StoreError> {
        let thread_id = thread_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let opened_by = thread_opener(&mut tx, &node_id.to_string(), &thread_id).await?;
        allowed(who, opened_by)?;
        let written = if resolved {
            sqlx::query(
                "UPDATE comment_threads SET resolved_at = ?, resolved_by = ? \
                 WHERE id = ? AND resolved_at IS NULL",
            )
            .bind(now_ms())
            .bind(by.to_string())
            .bind(&thread_id)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::from_fk)?
        } else {
            sqlx::query(
                "UPDATE comment_threads SET resolved_at = NULL, resolved_by = NULL \
                 WHERE id = ? AND resolved_at IS NOT NULL",
            )
            .bind(&thread_id)
            .execute(&mut *tx)
            .await?
        };
        let changed = written.rows_affected() > 0;
        let thread = read_thread(&mut tx, &thread_id).await?;
        tx.commit().await?;
        Ok((thread, changed))
    }

    /// Pose la réaction `emoji` de `user` sur le message ; déjà posée, rien ne change. Rend le
    /// message, et s'il a changé. `NotFound` si le message n'est pas de ce fil, le fil de cette
    /// page, ou si le compte n'existe pas.
    pub async fn add_reaction(
        &self,
        node_id: Uuid,
        thread_id: Uuid,
        comment_id: Uuid,
        user: Uuid,
        emoji: &str,
    ) -> Result<(Comment, bool), StoreError> {
        let comment_id = comment_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        comment_author(
            &mut tx,
            &node_id.to_string(),
            &thread_id.to_string(),
            &comment_id,
        )
        .await?;
        // Un emoji neuf sur ce message compte dans le plafond ; un emoji déjà là, non (un autre
        // compte le pose aussi), ni une réaction déjà posée, qui ne change rien (Ruling R170).
        let (present, distinct): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*) FILTER (WHERE emoji = ?), COUNT(DISTINCT emoji) \
             FROM comment_reactions WHERE comment_id = ?",
        )
        .bind(emoji)
        .bind(&comment_id)
        .fetch_one(&mut *tx)
        .await?;
        if present == 0 && distinct >= i64::from(MAX_REACTIONS_PER_MESSAGE) {
            return Err(StoreError::CommentLimit(CommentLimit::Reactions));
        }
        let added = sqlx::query(
            "INSERT INTO comment_reactions (comment_id, user_id, emoji, created_at) \
             VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(&comment_id)
        .bind(user.to_string())
        .bind(emoji)
        .bind(now_ms())
        .execute(&mut *tx)
        .await
        .map_err(StoreError::from_fk)?
        .rows_affected();
        let comment = read_comment(&mut tx, &comment_id).await?;
        tx.commit().await?;
        Ok((comment, added > 0))
    }

    /// Retire la réaction `emoji` de `user` sur le message, la sienne seulement ; absente, rien
    /// ne change. Rend le message, et s'il a changé. `NotFound` si le message n'est pas de ce
    /// fil, ou le fil de cette page.
    pub async fn remove_reaction(
        &self,
        node_id: Uuid,
        thread_id: Uuid,
        comment_id: Uuid,
        user: Uuid,
        emoji: &str,
    ) -> Result<(Comment, bool), StoreError> {
        let comment_id = comment_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        comment_author(
            &mut tx,
            &node_id.to_string(),
            &thread_id.to_string(),
            &comment_id,
        )
        .await?;
        let removed = sqlx::query(
            "DELETE FROM comment_reactions WHERE comment_id = ? AND user_id = ? AND emoji = ?",
        )
        .bind(&comment_id)
        .bind(user.to_string())
        .bind(emoji)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let comment = read_comment(&mut tx, &comment_id).await?;
        tx.commit().await?;
        Ok((comment, removed > 0))
    }
}

/// Écrit un message de `author` dans le fil, et rend son identifiant. `NotFound` si le compte
/// n'existe pas.
async fn insert_comment(
    conn: &mut SqliteConnection,
    thread_id: &str,
    author: Uuid,
    body: &str,
    now: i64,
) -> Result<String, StoreError> {
    let id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO comments (id, thread_id, author_id, body, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(thread_id)
    .bind(author.to_string())
    .bind(body)
    .bind(now)
    .execute(&mut *conn)
    .await
    .map_err(StoreError::from_fk)?;
    Ok(id)
}
