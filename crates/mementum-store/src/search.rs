//! L'index de recherche, côté base (lot 4, plan du lot, « Où » et « L'intention, la validation, la
//! clôture » ; Task S1).
//!
//! - `blocks_index` et `page_links` : la projection d'une page, écrite par différence
//!   ([`Store::index_page`]) : seuls les blocs dont l'empreinte a changé sont récrits, et chacun
//!   d'eux, changé ou parti, reçoit une ligne d'outbox (Ruling R919). L'appelant (l'indexeur du
//!   serveur) projette la page ; le store ne lit pas les documents.
//! - `search_state` : où en est la projection de chaque page (`indexed_seq`, `-1` pour une page
//!   jamais indexée).
//! - `search_outbox` : ce que l'index plein texte n'a pas encore validé. Il le prend par lots,
//!   en trois temps : l'intention ([`Store::begin_feed`]), la validation (dans l'index plein
//!   texte, hors du store), la clôture ([`Store::finish_feed`]) ; ou l'intention défaite
//!   ([`Store::undo_feed`]). Les deux jetons sont dans `instance_settings` (Ruling R909).
//! - Les **marqueurs durables** (Ruling R918) : ce qui change l'index sans passer par un acteur
//!   (une page créée ou dupliquée, un titre changé, une page supprimée) laisse une trace dans la
//!   transaction même de son écriture, par les fonctions `mark_*` d'ici, que seuls `nodes.rs` et
//!   `duplicate.rs` appellent. Un test lit les sources (`only_listed_places_write_a_title_or_a_node`) :
//!   une écriture neuve d'un titre ou d'un nœud sans son marqueur le fait rougir.
//!
//! Toutes les écritures de l'index passent par la voie de la file qui lui est propre
//! (`Lane::Index`, Ruling R908), une transaction bornée à la fois ([`MAX_INDEX_TX_BYTES`]) :
//! une frappe n'attend jamais plus d'une transaction de l'index. Aucune n'avance `rev` ni `lsn`,
//! ni n'annonce rien sur [`Store::changes`] : l'index est dérivé, et ne change ce que montre la
//! barre latérale de personne. Une écriture qui n'a rien à faire ne prend pas de tour : elle lit
//! d'abord (Ruling R270). Les lectures (le rattrapage, les rétroliens, ce qui nourrit l'index plein
//! texte) ne prennent aucun tour.
//!
//! **Le verrou de l'indexeur** (Ruling R975) : [`Store::index_page`] lit les empreintes gardées
//! avant de prendre son tour d'écriture, et ne les relit pas dans ses transactions (seul le `seq`
//! l'est). Deux indexations de front sur une page laissaient donc un index faux noté à jour : la
//! seconde lisait les empreintes d'avant la première, tenait pour inchangé un bloc que la
//! première venait de retirer, et notait son `seq`. `index_page` et [`Store::index_failed`]
//! prennent donc un verrou du `Store`, propre à l'index, avant de lire, et le gardent jusqu'à leur
//! dernière transaction validée : une projection écrite à la fois, toutes pages confondues (il n'y
//! a qu'un indexeur). Il est toujours pris avant le tour d'écriture, jamais en le tenant, et rien
//! d'autre que l'indexeur ne le prend : aucune écriture d'un compte ne l'attend.

use std::collections::{HashMap, HashSet};

use sqlx::{Row, Sqlite, SqliteExecutor, Transaction};
use uuid::Uuid;

use crate::queue::Lane;
use crate::{Store, StoreError, parse_uuid, unreadable};

/// Ce qu'une transaction de l'index porte au plus de lignes changées, en octets
/// ([`Store::index_page`]) : au-delà, la page s'écrit en plusieurs transactions. Valeur de départ
/// (plan du lot 4, « Quand ») : le point de contrôle automatique du WAL tombe toutes les 1 000
/// pages, environ 4 Mio ; une transaction de cette taille en déclencherait un à chaque fois, sur
/// son tour d'écriture. La mesure de la Task S4b l'arrête.
pub const MAX_INDEX_TX_BYTES: usize = 256 * 1024;

/// Ce qu'une ligne compte en plus de ses octets (sa clé dans l'outbox, l'en-tête de ses
/// enregistrements) : une page de blocs minuscules ne fait pas une transaction sans fin.
const ROW_OVERHEAD: usize = 64;

/// Ce qu'un identifiant de page visée compte, en octets.
const LINK_BYTES: usize = 36;

/// Lignes écrites par instruction : une instruction par paquet, pas par ligne, pour que le tour
/// d'écriture tienne au travail de SQLite et non aux allers et retours vers son fil (au plus
/// 5 valeurs par ligne, 500 par instruction, loin de la limite de SQLite).
const ROWS_PER_STATEMENT: usize = 100;

/// La clé, dans `instance_settings`, du jeton de la dernière validation close.
const COMMITTED: &str = "search.committed";

/// La clé, dans `instance_settings`, du jeton de l'intention posée et pas encore close.
const INTENT: &str = "search.intent";

/// La clé, dans `instance_settings`, de la version de la projection que l'index reflète
/// ([`Store::projection_version`], Ruling R977).
const PROJECTION: &str = "search.projection";

/// Un bloc projeté, à écrire dans `blocks_index` ([`Store::index_page`]). Le store ne le calcule
/// pas : l'indexeur le tire de la projection d'une page (`mementum-search`, qui ne dépend pas du
/// store ; Ruling R938), et le prête sans le recopier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexedBlock<'a> {
    /// La clé du bloc dans la page : son identifiant, ou une clé de remplacement ; jamais vide
    /// (`''` nomme la page entière dans l'outbox).
    pub block_id: &'a str,
    /// Son type (`paragraph`, `heading`…), gardé dans la colonne `type`.
    pub kind: &'a str,
    pub text: &'a str,
    /// Son empreinte. Elle doit couvrir le type, le texte **et** les liens : un bloc dont
    /// l'empreinte n'a pas changé n'est pas récrit, ses liens non plus.
    pub hash: &'a [u8],
    /// Les pages auxquelles il mène.
    pub links: &'a [Uuid],
}

/// Ce que [`Store::index_page`] a écrit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indexed {
    /// Les blocs écrits : récrits (neufs ou changés) et retirés.
    pub changed: usize,
}

/// Un bloc que garde `blocks_index` ([`Store::indexed_blocks`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRow {
    pub block_id: String,
    pub kind: String,
    pub text: String,
}

/// Un lot du rattrapage ([`Store::stale_pages`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalePages {
    /// Les pages du lot dont le dernier `seq` gardé dépasse ce que l'index reflète.
    pub pages: Vec<Uuid>,
    /// Où reprendre (le dernier identifiant lu) ; `None` : toutes les pages ont été lues.
    pub next: Option<Uuid>,
}

/// Une ligne de l'outbox prise par une intention.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FeedLine {
    pub node_id: Uuid,
    /// Le bloc ; `None` : la page entière (son titre a changé, elle est supprimée, ou tout est à
    /// reconstruire).
    pub block_id: Option<String>,
}

/// Une intention ([`Store::begin_feed`]) : son jeton et ses lignes, par page puis par bloc, la
/// ligne de page entière en tête de sa page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    pub token: String,
    pub lines: Vec<FeedLine>,
}

/// Ce que rend [`Store::begin_feed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedStart {
    /// Rien n'attend l'index plein texte.
    Nothing,
    /// Une intention est posée, pas encore close ni défaite (son jeton) : aucune autre ne
    /// commence, rien n'est pris (Ruling R909).
    Pending(String),
    /// L'intention posée.
    Begun(Feed),
}

/// Les deux jetons de `instance_settings` ([`Store::feed_tokens`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeedTokens {
    /// Celui de la dernière validation close.
    pub committed: Option<String>,
    /// Celui de l'intention posée et pas encore close.
    pub intent: Option<String>,
}

/// Une page qui mène à une autre, et l'extrait du premier de ses blocs qui y mène
/// ([`Store::backlink_excerpts`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlink {
    pub node_id: Uuid,
    pub block_id: String,
    pub excerpt: String,
}

/// Où en est la projection d'une page (`search_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PageState {
    indexed_seq: i64,
    failed_seq: Option<i64>,
    truncated: bool,
}

async fn read_state<'e>(
    db: impl SqliteExecutor<'e>,
    node_id: &str,
) -> Result<Option<PageState>, StoreError> {
    Ok(
        sqlx::query(
            "SELECT indexed_seq, failed_seq, truncated FROM search_state WHERE node_id = ?",
        )
        .bind(node_id)
        .fetch_optional(db)
        .await?
        .map(|r| PageState {
            indexed_seq: r.get(0),
            failed_seq: r.get(1),
            truncated: r.get::<i64, _>(2) != 0,
        }),
    )
}

async fn write_state(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
    state: PageState,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO search_state (node_id, indexed_seq, failed_seq, truncated) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (node_id) DO UPDATE SET indexed_seq = excluded.indexed_seq, \
             failed_seq = excluded.failed_seq, truncated = excluded.truncated",
    )
    .bind(node_id)
    .bind(state.indexed_seq)
    .bind(state.failed_seq)
    .bind(state.truncated)
    .execute(&mut **tx)
    .await
    .map_err(StoreError::from_fk)?;
    Ok(())
}

async fn read_setting<'e>(
    db: impl SqliteExecutor<'e>,
    key: &str,
) -> Result<Option<String>, StoreError> {
    Ok(
        sqlx::query("SELECT value FROM instance_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(db)
            .await?
            .map(|r| r.get(0)),
    )
}

async fn write_setting(
    tx: &mut Transaction<'_, Sqlite>,
    key: &str,
    value: Option<&str>,
) -> Result<(), StoreError> {
    match value {
        Some(value) => {
            sqlx::query(
                "INSERT INTO instance_settings (key, value) VALUES (?1, ?2) \
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            )
            .bind(key)
            .bind(value)
            .execute(&mut **tx)
            .await?;
        }
        None => {
            sqlx::query("DELETE FROM instance_settings WHERE key = ?")
                .bind(key)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}

/// `head`, puis `count` fois `group` séparés par des virgules, puis `tail` : une instruction à
/// plusieurs lignes. Rien d'autre que ces morceaux fixes et des paramètres n'y entre.
fn repeated(head: &str, group: &str, count: usize, tail: &str) -> String {
    let mut sql = String::with_capacity(head.len() + (group.len() + 2) * count + tail.len());
    sql.push_str(head);
    for i in 0..count {
        if i > 0 {
            sql.push_str(", ");
        }
        sql.push_str(group);
    }
    sql.push_str(tail);
    sql
}

/// Une ligne de l'outbox pour chacun de ces blocs de la page (`''` : la page entière), sans jeton :
/// une ligne déjà prise par une intention perd le sien, si bien que la clôture de cette intention
/// la laisse pour la suivante (Ruling R939).
///
/// L'outbox n'a pas de clé étrangère : des lignes de bloc orphelines restent possibles quand une
/// page est supprimée pendant que sa projection s'écrit (une tranche qui ne fait que retirer des
/// blocs ne touche aucune table liée à `nodes`, et passe après `mark_deleted`). Elles sont sans
/// effet : l'entrée qu'elles nomment n'a plus de ligne dans `blocks_index`, et la ligne de page
/// entière que la suppression a posée retire de toute façon toutes les entrées de la page.
async fn put_outbox(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
    blocks: &[&str],
) -> Result<(), StoreError> {
    for chunk in blocks.chunks(ROWS_PER_STATEMENT) {
        let sql = repeated(
            "INSERT INTO search_outbox (node_id, block_id, token) VALUES ",
            "(?, ?, NULL)",
            chunk.len(),
            " ON CONFLICT (node_id, block_id) DO UPDATE SET token = NULL",
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for block in chunk {
            query = query.bind(node_id).bind(*block);
        }
        query.execute(&mut **tx).await?;
    }
    Ok(())
}

/// Marqueur d'une page née (créée, ou d'une copie ; Ruling R918), dans la transaction qui l'écrit :
/// jamais indexée (`indexed_seq = -1`), et sa ligne d'outbox de page entière, pour son titre.
pub(crate) async fn mark_born(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
) -> Result<(), StoreError> {
    let never = PageState {
        indexed_seq: -1,
        failed_seq: None,
        truncated: false,
    };
    write_state(tx, node_id, never).await?;
    put_outbox(tx, node_id, &[""]).await
}

/// Marqueur d'un titre changé (Ruling R918), dans la transaction qui l'écrit : la ligne d'outbox
/// de page entière, puisque le titre est sur chacune de ses entrées dans l'index plein texte.
pub(crate) async fn mark_renamed(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
) -> Result<(), StoreError> {
    put_outbox(tx, node_id, &[""]).await
}

/// Marqueur de nœuds supprimés pour de bon (Ruling R918), dans la transaction qui les supprime,
/// avant `DELETE FROM nodes` (il lit leur genre) : `ids` est un tableau JSON d'identifiants. Leurs
/// lignes de bloc quittent l'outbox, et chaque page reçoit sa ligne de page entière, qui survit à
/// la page : ses entrées quitteront l'index plein texte. Ses lignes de `blocks_index`,
/// `page_links` et `search_state` partent avec elle, par la cascade.
pub(crate) async fn mark_deleted(
    tx: &mut Transaction<'_, Sqlite>,
    ids: &str,
) -> Result<(), StoreError> {
    sqlx::query(
        "DELETE FROM search_outbox \
         WHERE node_id IN (SELECT value FROM json_each(?)) AND block_id != ''",
    )
    .bind(ids)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO search_outbox (node_id, block_id, token) \
         SELECT id, '', NULL FROM nodes \
         WHERE id IN (SELECT value FROM json_each(?)) AND kind = 'page' \
         ON CONFLICT (node_id, block_id) DO UPDATE SET token = NULL",
    )
    .bind(ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Ce qu'une transaction de l'index écrit pour un bloc.
enum Change<'r, 'a> {
    /// Le bloc est neuf, ou son empreinte a changé : il est récrit, avec ses liens.
    Put(&'r IndexedBlock<'a>),
    /// Le bloc est parti : il est retiré, avec ses liens.
    Remove(&'r str),
}

impl Change<'_, '_> {
    fn block_id(&self) -> &str {
        match self {
            Change::Put(row) => row.block_id,
            Change::Remove(block) => block,
        }
    }

    /// Ce que le changement compte contre la borne d'une transaction.
    fn bytes(&self) -> usize {
        ROW_OVERHEAD
            + match self {
                Change::Put(row) => {
                    row.block_id.len()
                        + row.kind.len()
                        + row.text.len()
                        + row.hash.len()
                        + LINK_BYTES * row.links.len()
                }
                Change::Remove(block) => block.len(),
            }
    }
}

/// Les changements, par tranches d'au plus `bound` octets, une tranche ayant au moins un
/// changement. Sans changement, une tranche vide : celle qui note le `seq`.
fn slices<'c, 'r, 'a>(changes: &'c [Change<'r, 'a>], bound: usize) -> Vec<&'c [Change<'r, 'a>]> {
    let mut out = Vec::new();
    let (mut start, mut size) = (0, 0);
    for (i, change) in changes.iter().enumerate() {
        let bytes = change.bytes();
        if i > start && size + bytes > bound {
            out.push(&changes[start..i]);
            (start, size) = (i, 0);
        }
        size += bytes;
    }
    if start < changes.len() || out.is_empty() {
        out.push(&changes[start..]);
    }
    out
}

/// Écrit une tranche de changements de la page, de l'espace `space_id`, et l'outbox de chaque
/// bloc touché.
async fn write_slice(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
    space_id: &str,
    slice: &[Change<'_, '_>],
) -> Result<(), StoreError> {
    let touched: Vec<&str> = slice.iter().map(Change::block_id).collect();
    let removed: Vec<&str> = slice
        .iter()
        .filter_map(|c| match c {
            Change::Remove(block) => Some(*block),
            Change::Put(_) => None,
        })
        .collect();
    let puts: Vec<&IndexedBlock<'_>> = slice
        .iter()
        .filter_map(|c| match c {
            Change::Put(row) => Some(*row),
            Change::Remove(_) => None,
        })
        .collect();
    for (table, blocks) in [("blocks_index", &removed), ("page_links", &touched)] {
        for chunk in blocks.chunks(ROWS_PER_STATEMENT) {
            let head = format!("DELETE FROM {table} WHERE node_id = ? AND block_id IN (");
            let sql = repeated(&head, "?", chunk.len(), ")");
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(node_id);
            for block in chunk {
                query = query.bind(*block);
            }
            query.execute(&mut **tx).await?;
        }
    }
    for chunk in puts.chunks(ROWS_PER_STATEMENT) {
        let sql = repeated(
            "INSERT INTO blocks_index (node_id, block_id, type, text, hash) VALUES ",
            "(?, ?, ?, ?, ?)",
            chunk.len(),
            " ON CONFLICT (node_id, block_id) DO UPDATE SET \
              type = excluded.type, text = excluded.text, hash = excluded.hash",
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for row in chunk {
            query = query
                .bind(node_id)
                .bind(row.block_id)
                .bind(row.kind)
                .bind(row.text)
                .bind(row.hash);
        }
        query
            .execute(&mut **tx)
            .await
            .map_err(StoreError::from_fk)?;
    }
    let links: Vec<(&str, String)> = puts
        .iter()
        .flat_map(|row| {
            row.links
                .iter()
                .map(move |target| (row.block_id, target.to_string()))
        })
        .collect();
    for chunk in links.chunks(ROWS_PER_STATEMENT) {
        // `OR IGNORE` : un bloc qui mène deux fois à la même page n'a qu'une ligne.
        let sql = repeated(
            "INSERT OR IGNORE INTO page_links (node_id, block_id, target_id, space_id) VALUES ",
            "(?, ?, ?, ?)",
            chunk.len(),
            "",
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for (block, target) in chunk {
            query = query
                .bind(node_id)
                .bind(*block)
                .bind(target.as_str())
                .bind(space_id);
        }
        query
            .execute(&mut **tx)
            .await
            .map_err(StoreError::from_fk)?;
    }
    put_outbox(tx, node_id, &touched).await
}

/// Des lignes mal bâties par l'appelant : une erreur, pas un refus.
fn malformed(why: &str) -> StoreError {
    StoreError::Sqlx(sqlx::Error::Protocol(format!(
        "projection de page mal formée : {why}"
    )))
}

/// Les identifiants d'une liste, en un seul paramètre : un tableau JSON. Ce sont des UUID, sans
/// rien à échapper.
fn json_ids(ids: &[Uuid]) -> String {
    format!(
        "[{}]",
        ids.iter()
            .map(|id| format!("\"{id}\""))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn uuids(rows: &[sqlx::sqlite::SqliteRow]) -> Result<Vec<Uuid>, StoreError> {
    rows.iter()
        .map(|r| parse_uuid(&r.get::<String, _>(0)))
        .collect()
}

fn index_row(r: &sqlx::sqlite::SqliteRow) -> IndexRow {
    IndexRow {
        block_id: r.get("block_id"),
        kind: r.get("type"),
        text: r.get("text"),
    }
}

/// Une ligne d'outbox relue : `''` est la page entière.
fn feed_line(r: &sqlx::sqlite::SqliteRow) -> Result<FeedLine, StoreError> {
    let block: String = r.get(1);
    Ok(FeedLine {
        node_id: parse_uuid(&r.get::<String, _>(0))?,
        block_id: (!block.is_empty()).then_some(block),
    })
}

/// Un nombre de lignes demandé, tel que SQLite le lit.
fn sql_limit(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

/// Les sources d'un rétrolien ([`Store::backlink_sources`]). En constante, pour que le plan de la
/// requête se vérifie : elle ne lit que l'index `page_links_by_target`, dans la plage de la cible
/// **et de son espace** (Task S6, R1015 : les liens d'un autre espace ne sont même pas lus), et y
/// saute d'une source à la suivante (R1016) : chaque pas de la récursion cherche la première
/// source après la précédente, si bien qu'une source qui porte dix mille liens vers la cible se lit
/// une fois, pas dix mille. La récursion fait au plus `?4 + 1` pas : la cible elle-même, si elle
/// mène à soi, en prend un, et n'est pas rendue ; la borne compte des pages, pas des liens.
const BACKLINK_SOURCES: &str = "WITH RECURSIVE walk(id, n) AS ( \
         SELECT ?2, 0 \
         UNION ALL \
         SELECT (SELECT node_id FROM page_links \
                 WHERE target_id = ?1 AND space_id = ?3 AND node_id > walk.id \
                 ORDER BY node_id LIMIT 1), n + 1 \
         FROM walk WHERE walk.id IS NOT NULL AND n <= ?4 \
     ) \
     SELECT id FROM walk WHERE n > 0 AND id IS NOT NULL AND id != ?1 ORDER BY id LIMIT ?4";

/// Les extraits des rétroliens ([`Store::backlink_excerpts`]) : le premier bloc de chaque source
/// qui mène à la cible, dans l'ordre des clés, lu par la même clé de l'index (la cible, son
/// espace, la source : un seul pas, quel que soit le nombre de ses liens, R1016), et son texte
/// coupé par la base.
const BACKLINK_EXCERPTS: &str = "SELECT f.node_id, f.block_id, substr(b.text, 1, ?3) \
     FROM (SELECT j.value AS node_id, \
                  (SELECT l.block_id FROM page_links l \
                   WHERE l.target_id = ?1 AND l.space_id = ?4 AND l.node_id = j.value \
                   ORDER BY l.block_id LIMIT 1) AS block_id \
           FROM json_each(?2) j) f \
     JOIN blocks_index b ON b.node_id = f.node_id AND b.block_id = f.block_id \
     ORDER BY f.node_id";

/// Le retard d'un espace ([`Store::behind_in_space`]), lu à partir des deux petits ensembles : les
/// pages jamais indexées (l'index partiel `search_state_never` ; aucune page n'est sans ligne,
/// R964) et les pages de l'outbox ; chacune est cherchée dans `nodes` par sa clé, pour son espace.
/// `CROSS JOIN` impose cet ordre à SQLite, quelles que soient ses statistiques : sans lui, il
/// parcourait les pages de l'espace (`nodes_by_parent`), voire de l'instance après `ANALYZE`
/// (88 ms pour 100 000 pages à jour, relecture de S1, I2). Le `+` devant `n.space_id` et `n.kind`
/// lui interdit de chercher un nœud par eux : sans lui, il cherchait chaque page en retard par
/// `nodes_by_parent (space_id=?)`, c'est-à-dire en relisant les pages de l'espace pour chacune.
/// L'outbox se lit en entier : petite, sauf pendant une reconstruction, où tout est en retard de
/// toute façon.
const BEHIND_IN_SPACE: &str = "SELECT behind.id FROM ( \
         SELECT node_id AS id FROM search_state WHERE indexed_seq = -1 \
         UNION \
         SELECT node_id FROM search_outbox \
     ) behind CROSS JOIN nodes n ON n.id = behind.id \
     WHERE +n.space_id = ?1 AND +n.kind = 'page' \
     ORDER BY behind.id LIMIT ?2";

/// Une écriture de l'index qui attend le verrou de l'indexeur, comptée pour les tests
/// ([`Store::index_waits`]) ; elle sort du compte en tombant, verrou pris ou abandonnée.
#[cfg(feature = "test-hooks")]
struct IndexWait<'s>(&'s std::sync::atomic::AtomicUsize);

#[cfg(feature = "test-hooks")]
impl<'s> IndexWait<'s> {
    fn enter(count: &'s std::sync::atomic::AtomicUsize) -> IndexWait<'s> {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        IndexWait(count)
    }
}

#[cfg(feature = "test-hooks")]
impl Drop for IndexWait<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Store {
    /// Le verrou de l'indexeur (Ruling R975, en tête du module) : à prendre avant de lire ce
    /// qu'on écrira, avant tout tour d'écriture, et à garder jusqu'à la dernière transaction.
    async fn index_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        #[cfg(feature = "test-hooks")]
        let _waiting = IndexWait::enter(&self.index_waits);
        self.index_lock.lock().await
    }

    /// Les écritures de l'index qui attendent le verrou de l'indexeur. Pour les tests seulement
    /// (fonction `test-hooks`) : une indexation qui attend ce verrou n'est pas en file d'écriture.
    #[cfg(feature = "test-hooks")]
    pub fn index_waits(&self) -> usize {
        self.index_waits.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// La version de la projection que l'index reflète (Ruling R977) : `PROJECTION_VERSION` de
    /// `mementum-search` (R960), telle que l'indexeur l'a écrite ([`Store::set_projection_version`]) ;
    /// `None` tant qu'aucune ne l'est (une instance neuve, ou d'avant ce lot). Quand elle diffère de
    /// la sienne, l'indexeur reprojette tout. Le store garde l'entier qu'on lui donne, sans en
    /// connaître le sens. Une valeur illisible est une erreur. Lue sans tour.
    pub async fn projection_version(&self) -> Result<Option<u32>, StoreError> {
        read_setting(&self.pool, PROJECTION)
            .await?
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| unreadable("version de la projection", &value))
            })
            .transpose()
    }

    /// Écrit la version de la projection que l'index reflète (voie de l'index ; ni `rev` ni `lsn`).
    /// Déjà celle-là, rien n'est écrit, sans tour ; une valeur illisible est remplacée.
    pub async fn set_projection_version(&self, version: u32) -> Result<(), StoreError> {
        let value = version.to_string();
        if read_setting(&self.pool, PROJECTION).await?.as_deref() == Some(value.as_str()) {
            return Ok(());
        }
        let mut tx = self.write_in(Lane::Index).await?;
        write_setting(&mut tx, PROJECTION, Some(&value)).await?;
        tx.commit().await
    }

    /// Écrit la projection de la page au `seq` lu (`rows` : ses blocs, `truncated` : la projection
    /// a été coupée à ses bornes), **par différence** (Ruling R919) : les blocs dont l'empreinte
    /// a changé, et les neufs, sont récrits avec leurs liens ; les blocs partis sont retirés avec
    /// les leurs ; chacun reçoit une ligne d'outbox, dans la même transaction ; un bloc inchangé
    /// n'est pas touché. La page est notée à `seq` (et un échec de lecture oublié) dans la
    /// dernière transaction : au-delà de [`MAX_INDEX_TX_BYTES`] de changements, la page s'écrit en
    /// plusieurs, et entre deux, `blocks_index` et `page_links` portent une partie de la page
    /// (limite L-i du plan).
    ///
    /// Rien n'est écrit, et aucun tour n'est pris, si la page reflète déjà un `seq` plus récent
    /// (une lecture plus ancienne n'écrase jamais l'index), ou si rien n'a changé ni dans les
    /// blocs ni dans son état. Le `seq` est relu dans chaque transaction : un `seq` plus récent
    /// posé entre-temps arrête l'écriture.
    ///
    /// Elle prend le verrou de l'indexeur avant de lire les empreintes gardées, et le garde
    /// jusqu'à sa dernière transaction (Ruling R975) : deux appels de front, même sur une page,
    /// passent l'un après l'autre. Ne pas l'appeler en tenant un tour d'écriture.
    ///
    /// `NotFound` si la page n'existe pas (ou plus). Une clé vide ou répétée est une erreur, sans
    /// rien écrire. Voie de l'index (Ruling R908) ; n'avance ni `rev` ni `lsn`.
    pub async fn index_page(
        &self,
        node_id: Uuid,
        seq: i64,
        rows: &[IndexedBlock<'_>],
        truncated: bool,
    ) -> Result<Indexed, StoreError> {
        self.index_page_in_slices(node_id, seq, rows, truncated, MAX_INDEX_TX_BYTES)
            .await
    }

    /// [`Store::index_page`], par tranches d'au plus `max_tx_bytes`. Pour les tests et les
    /// mesures seulement (fonction `test-hooks`).
    #[cfg(feature = "test-hooks")]
    pub async fn index_page_bounded(
        &self,
        node_id: Uuid,
        seq: i64,
        rows: &[IndexedBlock<'_>],
        truncated: bool,
        max_tx_bytes: usize,
    ) -> Result<Indexed, StoreError> {
        self.index_page_in_slices(node_id, seq, rows, truncated, max_tx_bytes)
            .await
    }

    async fn index_page_in_slices(
        &self,
        node_id: Uuid,
        seq: i64,
        rows: &[IndexedBlock<'_>],
        truncated: bool,
        bound: usize,
    ) -> Result<Indexed, StoreError> {
        let mut wanted = HashSet::with_capacity(rows.len());
        for row in rows {
            if row.block_id.is_empty() {
                return Err(malformed("un bloc sans clé"));
            }
            if !wanted.insert(row.block_id) {
                return Err(malformed("une clé de bloc répétée"));
            }
        }
        let id = node_id.to_string();
        // Le verrou de l'indexeur, avant de lire, gardé jusqu'à la dernière tranche (R975).
        let _indexing = self.index_lock().await;
        // Lu sans tour, dans une vue cohérente : ce qu'il y a à écrire.
        let mut read = self.pool.begin().await?;
        // L'espace de la page, recopié sur ses liens (Task S6) : une page n'en change jamais.
        let Some(space) =
            sqlx::query_scalar::<_, String>("SELECT space_id FROM nodes WHERE id = ?")
                .bind(&id)
                .fetch_optional(&mut *read)
                .await?
        else {
            return Err(StoreError::NotFound);
        };
        let state = read_state(&mut *read, &id).await?;
        let kept: HashMap<String, Vec<u8>> =
            sqlx::query("SELECT block_id, hash FROM blocks_index WHERE node_id = ?")
                .bind(&id)
                .fetch_all(&mut *read)
                .await?
                .iter()
                .map(|r| (r.get(0), r.get(1)))
                .collect();
        read.commit().await?;
        if state.is_some_and(|state| seq < state.indexed_seq) {
            return Ok(Indexed { changed: 0 });
        }
        let mut removed: Vec<&str> = kept
            .keys()
            .map(String::as_str)
            .filter(|block| !wanted.contains(block))
            .collect();
        removed.sort_unstable();
        let changes: Vec<Change<'_, '_>> = removed
            .into_iter()
            .map(Change::Remove)
            .chain(
                rows.iter()
                    .filter(|row| kept.get(row.block_id).is_none_or(|hash| hash != row.hash))
                    .map(Change::Put),
            )
            .collect();
        let done = PageState {
            indexed_seq: seq,
            failed_seq: None,
            truncated,
        };
        if changes.is_empty() && state == Some(done) {
            return Ok(Indexed { changed: 0 });
        }
        let slices = slices(&changes, bound);
        let mut changed = 0;
        for (i, slice) in slices.iter().enumerate() {
            let mut tx = self.write_in(Lane::Index).await?;
            let now = read_state(&mut **tx, &id).await?;
            if now.is_some_and(|now| seq < now.indexed_seq) {
                return Ok(Indexed { changed });
            }
            write_slice(&mut tx, &id, &space, slice).await?;
            if i + 1 == slices.len() {
                write_state(&mut tx, &id, done).await?;
            }
            tx.commit().await?;
            changed += slice.len();
        }
        Ok(Indexed { changed })
    }

    /// La lecture de la page au `seq` donné a échoué (un journal qu'on ne sait plus relire) : elle
    /// quitte l'index. Ses lignes de `blocks_index` et de `page_links` sont retirées, ses lignes
    /// de bloc quittent l'outbox, et sa ligne de page entière y est posée, pour que ses entrées
    /// quittent l'index plein texte ; la page est notée à ce `seq`, comme échouée (Ruling R962) :
    /// ni [`Store::never_indexed`] ni [`Store::stale_pages`] ne la rendent avant que son `seq`
    /// change. Rien n'est écrit, sans tour, si la page reflète un `seq` plus récent, ou si cet
    /// échec est déjà noté. `NotFound` si la page n'existe pas. Le verrou de l'indexeur est tenu
    /// de la lecture à la validation (R975).
    ///
    /// Toutes les lignes de la page partent en une transaction, sans la borne de
    /// [`MAX_INDEX_TX_BYTES`] (Ruling R976) : une suppression par la clé ne récrit rien, et coûte
    /// peu au tour d'écriture, même pour une page de dix mille blocs.
    pub async fn index_failed(&self, node_id: Uuid, seq: i64) -> Result<(), StoreError> {
        let id = node_id.to_string();
        let failed = PageState {
            indexed_seq: seq,
            failed_seq: Some(seq),
            truncated: false,
        };
        let _indexing = self.index_lock().await;
        let state = read_state(&self.pool, &id).await?;
        if state.is_some_and(|state| seq < state.indexed_seq || state == failed) {
            return Ok(());
        }
        let mut tx = self.write_in(Lane::Index).await?;
        let now = read_state(&mut **tx, &id).await?;
        if now.is_some_and(|now| seq < now.indexed_seq) {
            return Ok(());
        }
        for sql in [
            "DELETE FROM blocks_index WHERE node_id = ?",
            "DELETE FROM page_links WHERE node_id = ?",
            "DELETE FROM search_outbox WHERE node_id = ? AND block_id != ''",
        ] {
            sqlx::query(sql).bind(&id).execute(&mut **tx).await?;
        }
        put_outbox(&mut tx, &id, &[""]).await?;
        write_state(&mut tx, &id, failed).await?;
        tx.commit().await
    }

    /// Les pages jamais indexées (`indexed_seq = -1` : créées ou dupliquées, Ruling R918, et
    /// chaque page d'une instance d'avant ce lot, que la migration `0005` note ainsi, R964), par
    /// identifiant, au plus `limit`, lues par l'index partiel `search_state_never`. Une page en
    /// sort quand elle est indexée, ou que sa lecture échoue ([`Store::index_failed`]) : une page
    /// sans document s'indexe sans blocs (`index_page(id, 0, &[], false)`), sans quoi elle y reste.
    pub async fn never_indexed(&self, limit: usize) -> Result<Vec<Uuid>, StoreError> {
        let rows = sqlx::query(
            "SELECT node_id FROM search_state WHERE indexed_seq = -1 ORDER BY node_id LIMIT ?",
        )
        .bind(sql_limit(limit))
        .fetch_all(&self.pool)
        .await?;
        uuids(&rows)
    }

    /// Le rattrapage du démarrage : parmi les `limit` pages qui suivent `after` dans l'ordre des
    /// identifiants (corbeille comprise, pas un dossier), celles dont le dernier `seq` gardé
    /// (journal ou instantané, 0 sans document) dépasse ce que l'index reflète : une page jamais
    /// indexée (`-1`), et ce qu'un autre `Store` a écrit (une commande, un test), qui n'est annoncé
    /// à personne, ou ce qu'un arrêt brutal a laissé. Une page sans ligne compte pour `-1` : aucune
    /// ne l'est depuis la migration `0005` (R964), le filet reste pour une ligne écrite à la main.
    /// Lue sans tour d'écriture ; chaque appel lit au plus `limit` pages.
    pub async fn stale_pages(
        &self,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<StalePages, StoreError> {
        let limit = limit.max(1);
        let rows = sqlx::query(
            "SELECT p.id, \
                 MAX(COALESCE((SELECT MAX(seq) FROM doc_updates u WHERE u.node_id = p.id), 0), \
                     COALESCE((SELECT seq FROM doc_snapshots s WHERE s.node_id = p.id), 0)), \
                 COALESCE((SELECT indexed_seq FROM search_state t WHERE t.node_id = p.id), -1) \
             FROM (SELECT id FROM nodes WHERE kind = 'page' AND id > ?1 ORDER BY id LIMIT ?2) p \
             ORDER BY p.id",
        )
        .bind(after.map(|id| id.to_string()).unwrap_or_default())
        .bind(sql_limit(limit))
        .fetch_all(&self.pool)
        .await?;
        let mut pages = Vec::new();
        for r in &rows {
            let (last, indexed): (i64, i64) = (r.get(1), r.get(2));
            if last > indexed {
                pages.push(parse_uuid(&r.get::<String, _>(0))?);
            }
        }
        let next = match rows.last() {
            Some(r) if rows.len() == limit => Some(parse_uuid(&r.get::<String, _>(0))?),
            _ => None,
        };
        Ok(StalePages { pages, next })
    }

    /// Les pages de l'espace dont l'index sait le retard, par identifiant, au plus `limit` : une
    /// page jamais indexée (à `-1` : créée, dupliquée, ou d'une instance d'avant ce lot, R964), ou
    /// qui a une ligne d'outbox (ce que l'index plein texte n'a pas encore validé). Une page dont
    /// seul le contenu a été écrit depuis sa dernière indexation n'y est pas : c'est à l'indexeur
    /// de le savoir. Lue sans tour d'écriture, à partir de ces deux ensembles, jamais en
    /// parcourant les pages de l'espace (`BEHIND_IN_SPACE`).
    pub async fn behind_in_space(
        &self,
        space_id: Uuid,
        limit: usize,
    ) -> Result<Vec<Uuid>, StoreError> {
        let rows = sqlx::query(BEHIND_IN_SPACE)
            .bind(space_id.to_string())
            .bind(sql_limit(limit))
            .fetch_all(&self.pool)
            .await?;
        uuids(&rows)
    }

    /// L'intention (Ruling R909) : prend au plus `limit` lignes de l'outbox sans jeton, par page
    /// puis par bloc, leur donne un jeton neuf, et le pose comme l'intention de l'instance, en une
    /// transaction (voie de l'index). Rien n'est encore écrit dans l'index plein texte : c'est à
    /// l'appelant de valider ces lignes sous ce jeton, puis de clore ([`Store::finish_feed`]) ou de
    /// défaire ([`Store::undo_feed`]).
    ///
    /// [`FeedStart::Pending`], sans rien prendre, tant qu'une intention est posée ;
    /// [`FeedStart::Nothing`], sans tour, si rien n'attend.
    pub async fn begin_feed(&self, limit: usize) -> Result<FeedStart, StoreError> {
        if let Some(intent) = read_setting(&self.pool, INTENT).await? {
            return Ok(FeedStart::Pending(intent));
        }
        let waiting: bool =
            sqlx::query("SELECT EXISTS (SELECT 1 FROM search_outbox WHERE token IS NULL)")
                .fetch_one(&self.pool)
                .await?
                .get(0);
        if !waiting || limit == 0 {
            return Ok(FeedStart::Nothing);
        }
        let mut tx = self.write_in(Lane::Index).await?;
        if let Some(intent) = read_setting(&mut **tx, INTENT).await? {
            return Ok(FeedStart::Pending(intent));
        }
        let token = Uuid::now_v7().to_string();
        let rows = sqlx::query(
            "UPDATE search_outbox SET token = ?1 WHERE (node_id, block_id) IN ( \
                 SELECT node_id, block_id FROM search_outbox WHERE token IS NULL \
                 ORDER BY node_id, block_id LIMIT ?2) \
             RETURNING node_id, block_id",
        )
        .bind(&token)
        .bind(sql_limit(limit))
        .fetch_all(&mut **tx)
        .await?;
        if rows.is_empty() {
            return Ok(FeedStart::Nothing);
        }
        let mut lines = rows.iter().map(feed_line).collect::<Result<Vec<_>, _>>()?;
        lines.sort();
        write_setting(&mut tx, INTENT, Some(&token)).await?;
        tx.commit().await?;
        Ok(FeedStart::Begun(Feed { token, lines }))
    }

    /// La clôture (Ruling R909) : les lignes qui portent encore le jeton de l'intention `token`
    /// quittent l'outbox (une ligne qui a bougé depuis l'intention l'a perdu, et reste pour la
    /// suivante, R939) ; `token` devient celui de la dernière validation close, et l'intention est
    /// retirée, en une transaction. `Stale`, sans rien écrire, si `token` n'est pas l'intention
    /// posée.
    pub async fn finish_feed(&self, token: &str) -> Result<(), StoreError> {
        let mut tx = self.write_in(Lane::Index).await?;
        if read_setting(&mut **tx, INTENT).await?.as_deref() != Some(token) {
            return Err(StoreError::Stale);
        }
        sqlx::query("DELETE FROM search_outbox WHERE token = ?")
            .bind(token)
            .execute(&mut **tx)
            .await?;
        write_setting(&mut tx, COMMITTED, Some(token)).await?;
        write_setting(&mut tx, INTENT, None).await?;
        tx.commit().await
    }

    /// Défait l'intention posée : ses lignes perdent leur jeton, et l'intention est retirée ; le
    /// jeton de la dernière validation close ne bouge pas. Sans intention, rien, sans tour.
    pub async fn undo_feed(&self) -> Result<(), StoreError> {
        if read_setting(&self.pool, INTENT).await?.is_none() {
            return Ok(());
        }
        let mut tx = self.write_in(Lane::Index).await?;
        let Some(intent) = read_setting(&mut **tx, INTENT).await? else {
            return Ok(());
        };
        sqlx::query("UPDATE search_outbox SET token = NULL WHERE token = ?")
            .bind(&intent)
            .execute(&mut **tx)
            .await?;
        write_setting(&mut tx, INTENT, None).await?;
        tx.commit().await
    }

    /// Les deux jetons : celui de la dernière validation close, celui de l'intention posée. Lus
    /// sans tour.
    pub async fn feed_tokens(&self) -> Result<FeedTokens, StoreError> {
        let mut tokens = FeedTokens::default();
        let rows = sqlx::query("SELECT key, value FROM instance_settings WHERE key IN (?, ?)")
            .bind(COMMITTED)
            .bind(INTENT)
            .fetch_all(&self.pool)
            .await?;
        for r in &rows {
            let (key, value): (String, String) = (r.get(0), r.get(1));
            match key.as_str() {
                COMMITTED => tokens.committed = Some(value),
                _ => tokens.intent = Some(value),
            }
        }
        Ok(tokens)
    }

    /// L'intention posée et ses lignes, pour la rejouer (un arrêt entre la validation et la
    /// clôture, une clôture refusée) ; `None` sans intention. Lue sans tour.
    pub async fn intent_lines(&self) -> Result<Option<Feed>, StoreError> {
        let mut read = self.pool.begin().await?;
        let Some(token) = read_setting(&mut *read, INTENT).await? else {
            return Ok(None);
        };
        let rows = sqlx::query(
            "SELECT node_id, block_id FROM search_outbox WHERE token = ? \
             ORDER BY node_id, block_id",
        )
        .bind(&token)
        .fetch_all(&mut *read)
        .await?;
        read.commit().await?;
        let lines = rows.iter().map(feed_line).collect::<Result<Vec<_>, _>>()?;
        Ok(Some(Feed { token, lines }))
    }

    /// Tout reconstruire, une tranche : les `limit` pages qui suivent `after` dans l'ordre des
    /// identifiants (corbeille comprise, pas un dossier) reçoivent leur ligne de page entière, sans
    /// jeton, en une transaction (voie de l'index). Rend où reprendre (la dernière page de la
    /// tranche), `None` une fois toutes les pages passées. Jamais une transaction en proportion
    /// de l'instance : l'appelant enchaîne les tranches, et entre deux, la file sert les autres
    /// voies.
    ///
    /// Elle ne touche pas à une intention posée : les lignes de page qu'elle récrit perdent leur
    /// jeton (R939), mais l'intention reste, et ses lignes de bloc aussi. Qui reconstruit (S4c,
    /// un index vidé) défait d'abord l'intention ([`Store::undo_feed`]) : l'index plein texte
    /// qu'elle visait n'est plus là (Ruling R978).
    pub async fn outbox_everything(
        &self,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<Option<Uuid>, StoreError> {
        let limit = limit.max(1);
        let mut tx = self.write_in(Lane::Index).await?;
        let rows = sqlx::query(
            "INSERT INTO search_outbox (node_id, block_id, token) \
             SELECT id, '', NULL FROM ( \
                 SELECT id FROM nodes WHERE kind = 'page' AND id > ?1 ORDER BY id LIMIT ?2) \
             WHERE true \
             ON CONFLICT (node_id, block_id) DO UPDATE SET token = NULL \
             RETURNING node_id",
        )
        .bind(after.map(|id| id.to_string()).unwrap_or_default())
        .bind(sql_limit(limit))
        .fetch_all(&mut **tx)
        .await?;
        tx.commit().await?;
        if rows.len() < limit {
            return Ok(None);
        }
        Ok(uuids(&rows)?.into_iter().max())
    }

    /// Tout reprojeter, une tranche (Ruling R980) : les `limit` pages qui suivent `after` dans
    /// l'ordre des identifiants (corbeille comprise, pas un dossier) redeviennent jamais indexées
    /// (`indexed_seq = -1`, un échec de lecture oublié), et reçoivent leur ligne d'outbox de page
    /// entière, sans jeton, en une transaction (voie de l'index). Rend où reprendre (la dernière
    /// page de la tranche), `None` une fois toutes les pages passées. C'est ce que l'indexeur fait
    /// au démarrage quand la version de la projection gardée ([`Store::projection_version`]) n'est
    /// pas la sienne : chaque page est relue et reprojetée, et ses entrées de l'index plein texte
    /// sont récrites, même celles dont l'empreinte n'a pas changé (un mot plié autrement).
    ///
    /// Les lignes de `blocks_index` et de `page_links` restent : la page reste trouvable sous
    /// l'ancienne forme tant qu'elle n'est pas reprojetée, et [`Store::index_page`] n'en récrit
    /// que ce qui change. Le verrou de l'indexeur est tenu le temps de la tranche (R975) ;
    /// n'avance ni `rev` ni `lsn`.
    pub async fn reproject_everything(
        &self,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<Option<Uuid>, StoreError> {
        let limit = limit.max(1);
        let _indexing = self.index_lock().await;
        let mut tx = self.write_in(Lane::Index).await?;
        let rows = sqlx::query(
            "INSERT INTO search_state (node_id, indexed_seq, failed_seq, truncated) \
             SELECT id, -1, NULL, 0 FROM ( \
                 SELECT id FROM nodes WHERE kind = 'page' AND id > ?1 ORDER BY id LIMIT ?2) \
             WHERE true \
             ON CONFLICT (node_id) DO UPDATE SET indexed_seq = -1, failed_seq = NULL \
             RETURNING node_id",
        )
        .bind(after.map(|id| id.to_string()).unwrap_or_default())
        .bind(sql_limit(limit))
        .fetch_all(&mut **tx)
        .await?;
        let pages = uuids(&rows)?;
        sqlx::query(
            "INSERT INTO search_outbox (node_id, block_id, token) \
             SELECT value, '', NULL FROM json_each(?) WHERE true \
             ON CONFLICT (node_id, block_id) DO UPDATE SET token = NULL",
        )
        .bind(json_ids(&pages))
        .execute(&mut **tx)
        .await?;
        tx.commit().await?;
        if pages.len() < limit {
            return Ok(None);
        }
        Ok(pages.into_iter().max())
    }

    /// Un bloc que garde `blocks_index` pour la page ; `None` s'il n'y est pas.
    pub async fn indexed_block(
        &self,
        node_id: Uuid,
        block_id: &str,
    ) -> Result<Option<IndexRow>, StoreError> {
        Ok(sqlx::query(
            "SELECT block_id, type, text FROM blocks_index WHERE node_id = ? AND block_id = ?",
        )
        .bind(node_id.to_string())
        .bind(block_id)
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(index_row))
    }

    /// Les blocs que garde `blocks_index` pour la page, dans l'ordre de leur clé.
    pub async fn indexed_blocks(&self, node_id: Uuid) -> Result<Vec<IndexRow>, StoreError> {
        Ok(sqlx::query(
            "SELECT block_id, type, text FROM blocks_index WHERE node_id = ? ORDER BY block_id",
        )
        .bind(node_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(index_row)
        .collect())
    }

    /// Les pages qui mènent à `target_id` : distinctes, de l'espace `space_id` (celui de la cible),
    /// hors la cible elle-même, dans l'ordre des identifiants, après `after`, au plus `limit`
    /// (des pages, pas des liens). Sans les droits ni la corbeille : c'est à la route de trier ce
    /// que la personne voit. Lue par l'index `page_links_by_target`, sans tour.
    pub async fn backlink_sources(
        &self,
        target_id: Uuid,
        space_id: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<Uuid>, StoreError> {
        read_sources(&self.pool, target_id, space_id, after, limit).await
    }

    /// Pour chacune de ces sources de l'espace `space_id` (celui de la cible) qui mène à
    /// `target_id`, le premier de ses blocs qui y mène, dans l'ordre des clés (Ruling R963), et
    /// son texte coupé par la base à `chars` caractères : jamais un texte de 64 Kio ramené pour en
    /// garder quelques-uns. Une source qui n'y mène pas n'est pas rendue. Par identifiant de page,
    /// sans tour.
    pub async fn backlink_excerpts(
        &self,
        target_id: Uuid,
        space_id: Uuid,
        sources: &[Uuid],
        chars: usize,
    ) -> Result<Vec<Backlink>, StoreError> {
        read_excerpts(&self.pool, target_id, space_id, sources, chars).await
    }

    /// Les deux lectures d'un rétrolien ([`Store::backlink_sources`] depuis le début, au plus
    /// `limit`, puis [`Store::backlink_excerpts`] de ce qu'elle rend), sur une même connexion,
    /// avec ce qu'elles coûtent à SQLite : le nombre de pas de sa machine virtuelle, compté par un
    /// gestionnaire de progression appelé à chaque pas. Les requêtes sont préparées avant d'être
    /// comptées. Ce compte ne dépend que du chemin suivi dans les index : une ligne qui n'est pas
    /// lue n'y paraît pas. Pour les tests seulement (fonction `test-hooks`, Task S6).
    #[cfg(feature = "test-hooks")]
    pub async fn backlink_read_steps(
        &self,
        target_id: Uuid,
        space_id: Uuid,
        limit: usize,
        chars: usize,
    ) -> Result<(Vec<Uuid>, Vec<Backlink>, BacklinkSteps), StoreError> {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};

        let mut conn = self.pool.acquire().await?;
        // Préparées une fois, hors du compte.
        let sources = read_sources(&mut *conn, target_id, space_id, None, limit).await?;
        read_excerpts(&mut *conn, target_id, space_id, &sources, chars).await?;
        let steps = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&steps);
        conn.lock_handle().await?.set_progress_handler(1, move || {
            counter.fetch_add(1, Ordering::Relaxed);
            true
        });
        let counted = async {
            let sources = read_sources(&mut *conn, target_id, space_id, None, limit).await?;
            let source_steps = steps.swap(0, Ordering::Relaxed);
            let found = read_excerpts(&mut *conn, target_id, space_id, &sources, chars).await?;
            let excerpt_steps = steps.swap(0, Ordering::Relaxed);
            let counts = BacklinkSteps {
                sources: source_steps,
                excerpts: excerpt_steps,
            };
            Ok::<_, StoreError>((sources, found, counts))
        }
        .await;
        conn.lock_handle().await?.remove_progress_handler();
        counted
    }
}

/// Ce que coûtent à SQLite les deux lectures d'un rétrolien, en pas de sa machine virtuelle
/// ([`Store::backlink_read_steps`]). Pour les tests seulement.
#[cfg(feature = "test-hooks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BacklinkSteps {
    pub sources: u64,
    pub excerpts: u64,
}

/// [`Store::backlink_sources`], sur `executor`.
async fn read_sources<'e>(
    executor: impl SqliteExecutor<'e>,
    target_id: Uuid,
    space_id: Uuid,
    after: Option<Uuid>,
    limit: usize,
) -> Result<Vec<Uuid>, StoreError> {
    let rows = sqlx::query(BACKLINK_SOURCES)
        .bind(target_id.to_string())
        .bind(after.map(|id| id.to_string()).unwrap_or_default())
        .bind(space_id.to_string())
        .bind(sql_limit(limit))
        .fetch_all(executor)
        .await?;
    uuids(&rows)
}

/// [`Store::backlink_excerpts`], sur `executor`.
async fn read_excerpts<'e>(
    executor: impl SqliteExecutor<'e>,
    target_id: Uuid,
    space_id: Uuid,
    sources: &[Uuid],
    chars: usize,
) -> Result<Vec<Backlink>, StoreError> {
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query(BACKLINK_EXCERPTS)
        .bind(target_id.to_string())
        .bind(json_ids(sources))
        .bind(sql_limit(chars))
        .bind(space_id.to_string())
        .fetch_all(executor)
        .await?
        .iter()
        .map(|r| {
            Ok(Backlink {
                node_id: parse_uuid(&r.get::<String, _>(0))?,
                block_id: r.get(1),
                excerpt: r.get(2),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le plan d'une requête : une ligne par étape, son texte en quatrième colonne.
    async fn plan(store: &Store, sql: &str, binds: usize) -> Vec<String> {
        let mut query = sqlx::query(sqlx::AssertSqlSafe(format!("EXPLAIN QUERY PLAN {sql}")));
        for _ in 0..binds {
            query = query.bind("x");
        }
        query
            .fetch_all(&store.pool)
            .await
            .unwrap()
            .iter()
            .map(|step| step.get::<String, _>(3))
            .collect()
    }

    /// Les requêtes de l'index et ce qu'elles doivent lire : `(SQL, nombre de paramètres, étapes
    /// voulues (chacune : des morceaux qu'une même étape du plan porte tous), tables dont un
    /// parcours est voulu)`.
    type Expected = (
        &'static str,
        usize,
        &'static [&'static [&'static str]],
        &'static [&'static str],
    );

    const QUERIES: [Expected; 5] = [
        // Les liens vers la cible, de son espace seulement, à partir d'une source : jamais ceux
        // d'un autre espace (Task S6).
        (
            BACKLINK_SOURCES,
            4,
            &[&[
                "page_links_by_target",
                "target_id=? AND space_id=? AND node_id>?",
            ]],
            &[],
        ),
        (
            BACKLINK_EXCERPTS,
            4,
            &[&[
                "page_links_by_target",
                "target_id=? AND space_id=? AND node_id=?",
            ]],
            &[],
        ),
        (
            "DELETE FROM search_outbox WHERE token = ?",
            1,
            &[&["search_outbox_taken"]],
            &[],
        ),
        (
            "SELECT node_id FROM search_state WHERE indexed_seq = -1 ORDER BY node_id LIMIT ?",
            1,
            &[&["search_state_never"]],
            &[],
        ),
        // Le retard d'un espace part des deux petits ensembles (les pages jamais indexées, par
        // l'index partiel ; les pages de l'outbox, qu'il lit en entier), puis lit chaque nœud par
        // sa clé : jamais toutes les pages de l'espace (relecture de S1, I2).
        (
            BEHIND_IN_SPACE,
            2,
            &[&["search_state_never"], &["SEARCH n ", "(id=?)"]],
            &["search_outbox", "o"],
        ),
    ];

    /// Les tables et leurs alias dans ces requêtes.
    const TABLES: [&str; 10] = [
        "page_links",
        "l",
        "nodes",
        "n",
        "blocks_index",
        "b",
        "search_outbox",
        "o",
        "search_state",
        "s",
    ];

    /// Vérifie le plan de chaque requête de [`QUERIES`] sur `store`.
    async fn check_plans(store: &Store) {
        for (sql, binds, wanted, scans) in QUERIES {
            let steps = plan(store, sql, binds).await;
            for parts in wanted {
                assert!(
                    steps
                        .iter()
                        .any(|step| parts.iter().all(|part| step.contains(part))),
                    "{parts:?} : {steps:?}"
                );
            }
            // Parcourir un index partiel, c'est lire les seules lignes qu'il garde.
            let partial = |step: &String| {
                ["search_state_never", "search_outbox_taken"]
                    .iter()
                    .any(|index| step.ends_with(&format!(" INDEX {index}")))
            };
            let scanned = |step: &String| {
                !partial(step)
                    && TABLES
                        .iter()
                        .filter(|table| !scans.contains(table))
                        .any(|table| {
                            let scan = format!("SCAN {table}");
                            *step == scan || step.starts_with(&format!("{scan} "))
                        })
            };
            assert!(
                !steps.iter().any(scanned),
                "un parcours de table : {steps:?}"
            );
        }
    }

    /// Les sources d'un rétrolien se lisent dans l'index `page_links_by_target` (la cible et son
    /// espace, puis les sources dans l'ordre), sans parcourir de table : des centaines de liens
    /// posés ailleurs, dans un autre espace, ne sont même pas lus (Task S6). De même la clôture d'une
    /// intention lit ses lignes par l'index partiel `search_outbox_taken`, pas toute l'outbox, les
    /// pages jamais indexées par `search_state_never`, et le retard d'un espace part de ses deux
    /// petits ensembles. Seuls se parcourent la liste des sources donnée (`json_each`), la
    /// sous-requête qui en sort (`f`), un index partiel, et l'outbox pour le retard d'un espace.
    /// Le plan est le même quand les statistiques de `ANALYZE` font croire un espace minuscule et
    /// les pages jamais indexées innombrables : le retard d'un espace a son ordre imposé.
    #[tokio::test]
    async fn backlink_sources_are_read_from_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.db");
        let store = Store::open(&path).await.unwrap();
        check_plans(&store).await;
        store.close().await;

        let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", path.display()))
            .await
            .unwrap();
        for sql in [
            "ANALYZE",
            "DELETE FROM sqlite_stat1",
            "INSERT INTO sqlite_stat1 (tbl, idx, stat) VALUES \
             ('nodes', 'nodes_by_parent', '100000 1 1 1'), \
             ('search_state', 'search_state_never', '100000 1'), \
             ('search_outbox', 'search_outbox', '100000 1 1')",
        ] {
            sqlx::query(sql).execute(&side).await.unwrap();
        }
        side.close().await;
        let store = Store::open(&path).await.unwrap();
        check_plans(&store).await;
        store.close().await;
    }
}
