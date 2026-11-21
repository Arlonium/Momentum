//! Le journal d'activité des jetons d'API (lot 5 ; Rulings R1244, R1245).
//!
//! Une ligne par appel par jeton qui a passé l'extracteur du serveur, sauf un appel refusé par le
//! débit (`429 token_busy`), qui n'écrit qu'un compte sur le jeton. Le serveur garde les lignes en
//! mémoire et les écrit par paquets ([`Store::append_calls`]), dans une voie de la file à elles
//! (`Lane::Calls`). Le journal ne garde ni le texte cherché, ni un MMD, ni un texte remplacé
//! (R1245) : l'outil, la page, l'issue, le code du refus, les révisions, le nombre de blocs.
//!
//! **Purge par parts** (R1244) : chaque paquet garde, pour chaque jeton qu'il touche, ses
//! [`MAX_KEPT_CALLS`] derniers appels réussis et ses [`MAX_KEPT_REFUSALS`] derniers autres
//! (refusés ou en erreur), dans l'ordre où ils ont été écrits ; un refus ne fait jamais partir une
//! ligne réussie, si bien qu'un voleur ne chasse pas la trace de ses lectures par des appels
//! refusés. Ce qui part est compté (`api_tokens.calls_purged`). Les lignes d'un jeton partent avec
//! lui (R1220, cascade).

use std::collections::BTreeMap;

use sqlx::Row;
use uuid::Uuid;

use crate::queue::Lane;
use crate::tokens::shown_to;
use crate::{Store, StoreError, parse_uuid, unreadable};

/// Appels réussis gardés par jeton, au plus (Ruling R1244).
pub const MAX_KEPT_CALLS: usize = 1000;

/// Appels refusés (ou en erreur) gardés par jeton, au plus : une part à eux, qui ne fait jamais
/// partir un appel réussi (Ruling R1244).
pub const MAX_KEPT_REFUSALS: usize = 200;

/// Par où l'appel est venu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallSurface {
    Mcp,
    Rest,
}

impl CallSurface {
    /// Le nom écrit en base (`token_calls.surface`).
    pub fn as_str(self) -> &'static str {
        match self {
            CallSurface::Mcp => "mcp",
            CallSurface::Rest => "rest",
        }
    }

    fn parse(text: &str) -> Result<CallSurface, StoreError> {
        [CallSurface::Mcp, CallSurface::Rest]
            .into_iter()
            .find(|surface| surface.as_str() == text)
            .ok_or_else(|| unreadable("surface d'un appel", text))
    }
}

/// Son issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    Ok,
    Refused,
    Error,
}

impl CallOutcome {
    /// Le nom écrit en base (`token_calls.outcome`).
    pub fn as_str(self) -> &'static str {
        match self {
            CallOutcome::Ok => "ok",
            CallOutcome::Refused => "refused",
            CallOutcome::Error => "error",
        }
    }

    fn parse(text: &str) -> Result<CallOutcome, StoreError> {
        [CallOutcome::Ok, CallOutcome::Refused, CallOutcome::Error]
            .into_iter()
            .find(|outcome| outcome.as_str() == text)
            .ok_or_else(|| unreadable("issue d'un appel", text))
    }
}

/// Un appel par jeton, tel que le journal le garde (Ruling R1245) : ni texte cherché, ni MMD, ni
/// texte remplacé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub token_id: Uuid,
    /// Millisecondes Unix.
    pub at: i64,
    pub surface: CallSurface,
    /// L'outil, ou le nom de la route.
    pub tool: String,
    pub space_id: Option<Uuid>,
    pub node_id: Option<Uuid>,
    pub outcome: CallOutcome,
    /// Le code du refus.
    pub code: Option<String>,
    pub revision_before: Option<String>,
    pub revision_after: Option<String>,
    pub blocks_touched: Option<i64>,
}

/// Des appels refusés par le débit (`429 token_busy`) : un compte, sans ligne (Ruling R1244).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Busy {
    pub token_id: Uuid,
    pub count: i64,
    /// Millisecondes Unix : le dernier d'entre eux.
    pub at: i64,
}

/// Une ligne du journal, relue ([`Store::calls_of`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRow {
    /// Son rang dans le journal, qui croît à chaque ligne écrite : le curseur de la page suivante.
    pub id: i64,
    pub call: Call,
}

/// Retire, des lignes du jeton `?1` de cette part, celles qui sont plus anciennes que la `?2`-ième
/// plus récente (rangées par `id`, l'ordre d'écriture), en lisant l'index `token_calls_by_token`.
const PURGE_OK: &str = "DELETE FROM token_calls WHERE token_id = ?1 AND outcome = 'ok' AND id < ( \
         SELECT id FROM token_calls WHERE token_id = ?1 AND outcome = 'ok' \
         ORDER BY id DESC LIMIT 1 OFFSET ?2)";
const PURGE_OTHERS: &str = "DELETE FROM token_calls \
     WHERE token_id = ?1 AND outcome IN ('refused', 'error') AND id < ( \
         SELECT id FROM token_calls WHERE token_id = ?1 AND outcome IN ('refused', 'error') \
         ORDER BY id DESC LIMIT 1 OFFSET ?2)";

impl Store {
    /// Écrit un paquet du journal, en une transaction, dans la voie du journal (`Lane::Calls`,
    /// quel que soit le compte de la tâche) : les appels, dans l'ordre donné (ceux d'un jeton parti
    /// entre-temps sont laissés : son journal est parti avec lui) ; pour chaque jeton qu'ils
    /// touchent, la date de dernière utilisation (qui ne recule pas), puis la purge par parts, et
    /// son compte ; pour chaque compte de refus par le débit, `busy_count` et `busy_last_at`, sans
    /// ligne. Un paquet vide n'écrit rien, et ne prend aucun tour. N'avance ni `rev` ni `lsn`.
    pub async fn append_calls(&self, calls: &[Call], busy: &[Busy]) -> Result<(), StoreError> {
        if calls.is_empty() && busy.is_empty() {
            return Ok(());
        }
        let mut latest: BTreeMap<Uuid, i64> = BTreeMap::new();
        for call in calls {
            let at = latest.entry(call.token_id).or_insert(call.at);
            *at = (*at).max(call.at);
        }
        let mut tx = self.write_in(Lane::Calls).await?;
        for call in calls {
            sqlx::query(
                "INSERT INTO token_calls (token_id, at, surface, tool, space_id, node_id, \
                     outcome, code, revision_before, revision_after, blocks_touched) \
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11 \
                 WHERE EXISTS (SELECT 1 FROM api_tokens WHERE id = ?1)",
            )
            .bind(call.token_id.to_string())
            .bind(call.at)
            .bind(call.surface.as_str())
            .bind(&call.tool)
            .bind(call.space_id.map(|id| id.to_string()))
            .bind(call.node_id.map(|id| id.to_string()))
            .bind(call.outcome.as_str())
            .bind(&call.code)
            .bind(&call.revision_before)
            .bind(&call.revision_after)
            .bind(call.blocks_touched)
            .execute(&mut **tx)
            .await?;
        }
        for (token, at) in latest {
            let token = token.to_string();
            sqlx::query(
                "UPDATE api_tokens SET last_used_at = MAX(COALESCE(last_used_at, ?2), ?2) \
                 WHERE id = ?1",
            )
            .bind(&token)
            .bind(at)
            .execute(&mut **tx)
            .await?;
            let mut purged = 0;
            for (purge, kept) in [
                (PURGE_OK, MAX_KEPT_CALLS),
                (PURGE_OTHERS, MAX_KEPT_REFUSALS),
            ] {
                purged += sqlx::query(purge)
                    .bind(&token)
                    .bind(kept as i64 - 1)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected();
            }
            if purged > 0 {
                sqlx::query("UPDATE api_tokens SET calls_purged = calls_purged + ?2 WHERE id = ?1")
                    .bind(&token)
                    .bind(purged as i64)
                    .execute(&mut **tx)
                    .await?;
            }
        }
        for refused in busy {
            sqlx::query(
                "UPDATE api_tokens SET busy_count = busy_count + ?2, \
                     busy_last_at = MAX(COALESCE(busy_last_at, ?3), ?3) \
                 WHERE id = ?1",
            )
            .bind(refused.token_id.to_string())
            .bind(refused.count)
            .bind(refused.at)
            .execute(&mut **tx)
            .await?;
        }
        tx.commit().await
    }

    /// Le journal du jeton `token` du compte `user`, du plus récent au plus ancien, au plus
    /// `limit` lignes, avant la ligne `before` (son `id` ; `None` : depuis la plus récente), lu
    /// en une transaction ; avec `outcome`, les seules lignes de cette issue (l'index
    /// `token_calls_by_token` les rend alors dans l'ordre, sans tri). `NotFound` pour un jeton
    /// inconnu, d'un autre compte, ou qui ne se lit plus à `now` (mort depuis
    /// [`crate::DEAD_TOKEN_KEPT_MS`] : son journal non plus) : la même réponse.
    pub async fn calls_of(
        &self,
        user: Uuid,
        token: Uuid,
        before: Option<i64>,
        outcome: Option<CallOutcome>,
        limit: usize,
        now: i64,
    ) -> Result<Vec<CallRow>, StoreError> {
        let mut tx = self.pool.begin().await?;
        if !shown_to(&mut *tx, user, token, now).await? {
            return Err(StoreError::NotFound);
        }
        // Deux requêtes statiques plutôt qu'un `?4 IS NULL OR outcome = ?4`, que l'index ne
        // servirait pas.
        let sql = match outcome {
            None => {
                "SELECT id, at, surface, tool, space_id, node_id, outcome, code, revision_before, \
                     revision_after, blocks_touched \
                 FROM token_calls WHERE token_id = ?1 AND id < ?2 ORDER BY id DESC LIMIT ?3"
            }
            Some(_) => {
                "SELECT id, at, surface, tool, space_id, node_id, outcome, code, revision_before, \
                     revision_after, blocks_touched \
                 FROM token_calls WHERE token_id = ?1 AND outcome = ?4 AND id < ?2 \
                 ORDER BY id DESC LIMIT ?3"
            }
        };
        let mut query = sqlx::query(sql)
            .bind(token.to_string())
            .bind(before.unwrap_or(i64::MAX))
            .bind(i64::try_from(limit).unwrap_or(i64::MAX));
        if let Some(outcome) = outcome {
            query = query.bind(outcome.as_str());
        }
        let rows = query.fetch_all(&mut *tx).await?;
        tx.commit().await?;
        rows.iter()
            .map(|r| {
                let id_of = |column: &str| {
                    r.get::<Option<String>, _>(column)
                        .map(|id| parse_uuid(&id))
                        .transpose()
                };
                Ok(CallRow {
                    id: r.get("id"),
                    call: Call {
                        token_id: token,
                        at: r.get("at"),
                        surface: CallSurface::parse(&r.get::<String, _>("surface"))?,
                        tool: r.get("tool"),
                        space_id: id_of("space_id")?,
                        node_id: id_of("node_id")?,
                        outcome: CallOutcome::parse(&r.get::<String, _>("outcome"))?,
                        code: r.get("code"),
                        revision_before: r.get("revision_before"),
                        revision_after: r.get("revision_after"),
                        blocks_touched: r.get("blocks_touched"),
                    },
                })
            })
            .collect()
    }
}
