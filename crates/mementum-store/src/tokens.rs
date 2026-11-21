//! Jetons d'API personnels (lot 5 ; Rulings R1213, R1218 à R1221).
//!
//! Le store ne voit jamais un jeton en clair : seulement son SHA-256, calculé par le serveur, et
//! ses quatre derniers caractères, pour qu'on le reconnaisse dans une liste. Aucune fonction ne
//! rend le hachage. Un jeton ne donne aucun droit par lui-même : le serveur restreint ceux de son
//! compte à sa portée et à son niveau (`access.rs`) ; le store ne fait que le garder.
//!
//! **Vivant, mort.** Un jeton vit tant qu'il n'est ni révoqué, ni échu (`now < expires_at`). Mort,
//! il est gardé, avec son journal, [`DEAD_TOKEN_KEPT_MS`] après sa mort (la plus tôt de sa
//! révocation et de son échéance), et un compte en garde [`MAX_DEAD_TOKENS`] au plus (Ruling
//! R1220) : la transaction qui crée ou révoque un jeton (un changement de mot de passe compris)
//! retire les autres, le plus anciennement mort d'abord, avec leurs lignes (cascade). Entre deux
//! purges, un jeton mort depuis plus longtemps n'est plus lu ([`Store::tokens_of`],
//! [`Store::calls_of`]) ; un compte peut en garder un peu plus de cinquante (des jetons qui
//! échoient sans que rien ne soit créé ni révoqué), jamais plus de cinquante après une purge.
//!
//! Les écritures d'ici sont dans la voie du compte qui les demande ; aucune n'avance `rev` ni
//! `lsn`. [`Store::token_by_hash`], que tout appel par jeton déclenche, n'écrit jamais : un anonyme
//! qui essaie des jetons ne remplit aucune voie (Ruling R517).

use std::collections::{BTreeSet, HashMap};
use std::fmt;

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqliteExecutor, Transaction};
use uuid::Uuid;

use crate::search::json_ids;
use crate::{Store, StoreError, parse_uuid, unreadable};

/// Jetons vivants d'un compte, au plus (Ruling R1220).
pub const MAX_LIVE_TOKENS: usize = 20;

/// Jetons morts (révoqués ou échus) qu'un compte garde, au plus, avec leur journal (Ruling R1220).
pub const MAX_DEAD_TOKENS: usize = 50;

/// Ce qu'un jeton mort est gardé, avec son journal, après sa mort : quatre-vingt-dix jours, en
/// millisecondes (Ruling R1220).
pub const DEAD_TOKEN_KEPT_MS: i64 = 90 * 86_400_000;

/// Pages qu'un jeton de portée « pages » nomme, au plus (Ruling R1219).
pub const MAX_TOKEN_PAGES: usize = 50;

/// Le jeton est vivant à l'instant lié en `?2` : ni révoqué, ni échu. En littéral, pour que le SQL
/// reste statique (`concat!`).
macro_rules! live {
    () => {
        "(revoked_at IS NULL AND (expires_at IS NULL OR expires_at > ?2))"
    };
}

/// La même règle, pour une écriture d'ailleurs dans le crate qui relit un jeton (le journal d'une
/// page, lot 5, Task S7) : jamais une expression récrite.
pub(crate) use live as live_token;

/// Mort, le jeton l'est à l'instant lié en `?2`.
macro_rules! dead {
    () => {
        "(revoked_at IS NOT NULL OR expires_at <= ?2)"
    };
}

/// Quand un jeton mort est mort : la plus tôt de sa révocation et de son échéance (`NULL` pour un
/// jeton sans l'une ni l'autre).
macro_rules! died_at {
    () => {
        "MIN(COALESCE(revoked_at, expires_at), COALESCE(expires_at, revoked_at))"
    };
}

/// Le jeton se lit encore à l'instant `?2` : vivant, ou mort depuis moins de quatre-vingt-dix jours
/// (`?3` : l'instant moins ce délai).
macro_rules! shown {
    () => {
        concat!("(", live!(), " OR ", died_at!(), " > ?3)")
    };
}

/// Colonnes d'un jeton tel que son compte le voit, en littéral.
macro_rules! token_columns {
    () => {
        "id, user_id, name, hint, level, scope, space_id, can_trash, created_at, expires_at, \
         revoked_at, last_used_at, busy_count, busy_last_at, calls_purged"
    };
}

/// Le niveau d'un jeton : au lot 5, lire ou modifier, jamais l'accès complet (Ruling R1218).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenLevel {
    Read,
    Edit,
}

impl TokenLevel {
    /// Le nom écrit en base (`api_tokens.level`).
    pub fn as_str(self) -> &'static str {
        match self {
            TokenLevel::Read => "read",
            TokenLevel::Edit => "edit",
        }
    }

    fn parse(text: &str) -> Result<TokenLevel, StoreError> {
        [TokenLevel::Read, TokenLevel::Edit]
            .into_iter()
            .find(|level| level.as_str() == text)
            .ok_or_else(|| unreadable("niveau de jeton", text))
    }
}

/// Ce qu'un jeton ouvre (Ruling R1219) : tous les espaces de son compte, un seul, ou des pages
/// d'un seul espace, avec leurs sous-pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenScope {
    All,
    Space(Uuid),
    /// Les pages, rangées par identifiant ; celles que la suppression a retirées n'y sont plus,
    /// si bien qu'un jeton dont toutes les pages sont parties n'en nomme aucune.
    Pages {
        space: Uuid,
        pages: Vec<Uuid>,
    },
}

impl TokenScope {
    /// Le nom écrit en base (`api_tokens.scope`) et l'espace nommé.
    fn parts(&self) -> (&'static str, Option<Uuid>) {
        match self {
            TokenScope::All => ("all", None),
            TokenScope::Space(space) => ("space", Some(*space)),
            TokenScope::Pages { space, .. } => ("pages", Some(*space)),
        }
    }

    /// La portée relue : son nom, son espace, et les pages qui restent.
    fn read(
        scope: &str,
        space: Option<String>,
        pages: Vec<Uuid>,
    ) -> Result<TokenScope, StoreError> {
        let space = space.map(|id| parse_uuid(&id)).transpose()?;
        match (scope, space) {
            ("all", None) => Ok(TokenScope::All),
            ("space", Some(space)) => Ok(TokenScope::Space(space)),
            ("pages", Some(space)) => Ok(TokenScope::Pages { space, pages }),
            _ => Err(unreadable("portée de jeton", scope)),
        }
    }
}

/// Un jeton à créer ([`Store::create_token`]).
#[derive(Clone)]
pub struct NewToken {
    pub user_id: Uuid,
    /// Enregistré tel quel : les règles de nom sont celles de la route.
    pub name: String,
    /// SHA-256 du jeton : le jeton lui-même n'est jamais stocké.
    pub hash: [u8; 32],
    /// Ses quatre derniers caractères, pour qu'on le reconnaisse dans une liste.
    pub hint: String,
    pub level: TokenLevel,
    pub scope: TokenScope,
    pub can_trash: bool,
    /// Millisecondes Unix : le jeton vaut tant que `now < expires_at`.
    pub expires_at: Option<i64>,
    /// Millisecondes Unix : sa création, et l'horloge de la purge des jetons morts.
    pub now: i64,
}

/// `Debug` masque le hachage : un jeton à créer écrit dans un journal ne le livre pas.
impl fmt::Debug for NewToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewToken")
            .field("user_id", &self.user_id)
            .field("name", &self.name)
            .field("hash", &"<masqué>")
            .field("hint", &self.hint)
            .field("level", &self.level)
            .field("scope", &self.scope)
            .field("can_trash", &self.can_trash)
            .field("expires_at", &self.expires_at)
            .field("now", &self.now)
            .finish()
    }
}

/// Un jeton, tel que son compte le voit dans sa liste : jamais son hachage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub hint: String,
    pub level: TokenLevel,
    pub scope: TokenScope,
    pub can_trash: bool,
    /// Millisecondes Unix, comme les dates qui suivent.
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub revoked_at: Option<i64>,
    /// Le plus récent des appels écrits au journal (un appel refusé par le débit n'y compte pas).
    pub last_used_at: Option<i64>,
    /// Les appels refusés par le débit (`429 token_busy`), comptés sans ligne (Ruling R1244).
    pub busy_count: i64,
    pub busy_last_at: Option<i64>,
    /// Les lignes du journal que la purge a retirées (Ruling R1244).
    pub calls_purged: i64,
}

/// Un jeton vivant, trouvé par son hachage ([`Store::token_by_hash`]) : de quoi restreindre les
/// droits de son compte, jamais son hachage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub level: TokenLevel,
    pub scope: TokenScope,
    pub can_trash: bool,
    pub expires_at: Option<i64>,
}

/// Un appelant mal écrit : une erreur, pas un refus (la route valide avant).
fn malformed(why: &str) -> StoreError {
    StoreError::Sqlx(sqlx::Error::Protocol(format!("jeton mal formé : {why}")))
}

fn token_row(r: &SqliteRow, pages: Vec<Uuid>) -> Result<TokenRow, StoreError> {
    Ok(TokenRow {
        id: parse_uuid(&r.get::<String, _>("id"))?,
        user_id: parse_uuid(&r.get::<String, _>("user_id"))?,
        name: r.get("name"),
        hint: r.get("hint"),
        level: TokenLevel::parse(&r.get::<String, _>("level"))?,
        scope: TokenScope::read(&r.get::<String, _>("scope"), r.get("space_id"), pages)?,
        can_trash: r.get("can_trash"),
        created_at: r.get("created_at"),
        expires_at: r.get("expires_at"),
        revoked_at: r.get("revoked_at"),
        last_used_at: r.get("last_used_at"),
        busy_count: r.get("busy_count"),
        busy_last_at: r.get("busy_last_at"),
        calls_purged: r.get("calls_purged"),
    })
}

/// Retire, dans la transaction, les jetons morts du compte qu'il ne garde plus à `now` (Ruling
/// R1220) : ceux morts depuis [`DEAD_TOKEN_KEPT_MS`] ou plus, puis, au-delà des
/// [`MAX_DEAD_TOKENS`] plus récemment morts, les autres. Leurs pages et leur journal partent avec
/// eux (cascade).
pub(crate) async fn purge_dead_tokens(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    now: i64,
) -> Result<(), StoreError> {
    sqlx::query(concat!(
        "DELETE FROM api_tokens WHERE user_id = ?1 AND ",
        dead!(),
        " AND ",
        died_at!(),
        " <= ?3"
    ))
    .bind(user_id)
    .bind(now)
    .bind(now.saturating_sub(DEAD_TOKEN_KEPT_MS))
    .execute(&mut **tx)
    .await?;
    sqlx::query(concat!(
        "DELETE FROM api_tokens WHERE id IN (SELECT id FROM api_tokens WHERE user_id = ?1 AND ",
        dead!(),
        " ORDER BY ",
        died_at!(),
        " DESC, id DESC LIMIT -1 OFFSET ?3)"
    ))
    .bind(user_id)
    .bind(now)
    .bind(MAX_DEAD_TOKENS as i64)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Le jeton `id` est-il au compte `user`, et se lit-il encore à `now` (vivant, ou mort depuis
/// moins de [`DEAD_TOKEN_KEPT_MS`]) ?
pub(crate) async fn shown_to<'e>(
    db: impl SqliteExecutor<'e>,
    user: Uuid,
    id: Uuid,
    now: i64,
) -> Result<bool, StoreError> {
    Ok(sqlx::query(concat!(
        "SELECT 1 FROM api_tokens WHERE user_id = ?1 AND id = ?4 AND ",
        shown!()
    ))
    .bind(user.to_string())
    .bind(now)
    .bind(now.saturating_sub(DEAD_TOKEN_KEPT_MS))
    .bind(id.to_string())
    .fetch_optional(db)
    .await?
    .is_some())
}

/// Révoque, dans la transaction, tous les jetons du compte qui ne le sont pas encore, à `now`, puis
/// retire ceux qu'il ne garde plus : le changement de mot de passe (Ruling R1221). Rend le nombre
/// de jetons qui vivaient à `now` (un jeton échu reçoit aussi sa date de révocation, sans compter :
/// il ne valait déjà plus rien).
pub(crate) async fn revoke_all_tokens(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    now: i64,
) -> Result<u64, StoreError> {
    let live: i64 = sqlx::query_scalar(concat!(
        "SELECT COUNT(*) FROM api_tokens WHERE user_id = ?1 AND ",
        live!()
    ))
    .bind(user_id)
    .bind(now)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("UPDATE api_tokens SET revoked_at = ?2 WHERE user_id = ?1 AND revoked_at IS NULL")
        .bind(user_id)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    purge_dead_tokens(tx, user_id, now).await?;
    Ok(u64::try_from(live).unwrap_or(0))
}

impl Store {
    /// Crée un jeton, et rend ce que son compte en voit. Dans une transaction (`WriteTx`, voie du
    /// compte qui la demande) : le compte a moins de [`MAX_LIVE_TOKENS`] jetons vivants à
    /// `token.now` (sinon [`StoreError::TokenLimit`]) ; il a un rôle dans l'espace qu'une portée
    /// nomme, et chaque page nommée est de cet espace (sinon `NotFound`) ; puis les jetons morts
    /// qu'il ne garde plus partent (Ruling R1220), et le jeton s'écrit, avec ses pages. Un refus
    /// n'écrit rien. Les pages d'une portée sont gardées une fois chacune ; aucune, ou plus de
    /// [`MAX_TOKEN_PAGES`], est une erreur de l'appelant (la route le dit avant). `NotFound` aussi
    /// pour un compte inconnu.
    ///
    /// Sans condition sur le mot de passe : les tests, et toute création qui n'en vérifie aucun.
    /// La route des jetons, qui vient de vérifier le mot de passe du compte, passe par
    /// [`Store::create_token_verified`].
    pub async fn create_token(&self, token: NewToken) -> Result<TokenRow, StoreError> {
        // Sans hachage vérifié, la création n'a pas de condition de plus : jamais `None`.
        self.insert_token(token, None)
            .await?
            .ok_or(StoreError::NotFound)
    }

    /// Comme [`Store::create_token`], si le hachage du mot de passe du compte est encore
    /// `verified` : `None`, sans rien écrire, s'il ne l'est plus (ou si le compte n'en a pas, ou
    /// n'existe pas). La condition est jugée dans la transaction qui écrit, avant tout le reste.
    ///
    /// Ruling R120 : `verified` est le hachage contre lequel l'appelant vient de vérifier le mot
    /// de passe redemandé à la création (décision L2). Entre cette vérification (un calcul
    /// Argon2) et l'écriture, un changement de mot de passe peut passer, qui révoque tous les
    /// jetons du compte (Ruling R1221) : une demande vérifiée contre l'ancien mot de passe ne
    /// doit pas créer, après lui, un jeton que ce changement n'a pas pu révoquer.
    pub async fn create_token_verified(
        &self,
        token: NewToken,
        verified: &str,
    ) -> Result<Option<TokenRow>, StoreError> {
        self.insert_token(token, Some(verified)).await
    }

    async fn insert_token(
        &self,
        token: NewToken,
        verified: Option<&str>,
    ) -> Result<Option<TokenRow>, StoreError> {
        let (scope, space) = token.scope.parts();
        let pages: Vec<Uuid> = match &token.scope {
            TokenScope::Pages { pages, .. } => {
                let pages: BTreeSet<Uuid> = pages.iter().copied().collect();
                if pages.is_empty() || pages.len() > MAX_TOKEN_PAGES {
                    return Err(malformed("une portée de pages en nomme de 1 à 50"));
                }
                pages.into_iter().collect()
            }
            _ => Vec::new(),
        };
        let (id, user) = (Uuid::now_v7(), token.user_id.to_string());
        let space_text = space.map(|s| s.to_string());
        let mut tx = self.write().await?;
        if let Some(verified) = verified {
            let holds = sqlx::query("SELECT 1 FROM users WHERE id = ? AND password_hash IS ?")
                .bind(&user)
                .bind(verified)
                .fetch_optional(&mut **tx)
                .await?;
            if holds.is_none() {
                return Ok(None);
            }
        }
        let live: i64 = sqlx::query_scalar(concat!(
            "SELECT COUNT(*) FROM api_tokens WHERE user_id = ?1 AND ",
            live!()
        ))
        .bind(&user)
        .bind(token.now)
        .fetch_one(&mut **tx)
        .await?;
        if live >= MAX_LIVE_TOKENS as i64 {
            return Err(StoreError::TokenLimit);
        }
        if let Some(space) = &space_text {
            let member = sqlx::query("SELECT 1 FROM members WHERE space_id = ? AND user_id = ?")
                .bind(space)
                .bind(&user)
                .fetch_optional(&mut **tx)
                .await?;
            if member.is_none() {
                return Err(StoreError::NotFound);
            }
        }
        if !pages.is_empty() {
            let found: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM nodes \
                 WHERE space_id = ?1 AND id IN (SELECT value FROM json_each(?2))",
            )
            .bind(space_text.as_deref())
            .bind(json_ids(&pages))
            .fetch_one(&mut **tx)
            .await?;
            if found != pages.len() as i64 {
                return Err(StoreError::NotFound);
            }
        }
        purge_dead_tokens(&mut tx, &user, token.now).await?;
        sqlx::query(
            "INSERT INTO api_tokens \
             (id, user_id, name, hash, hint, level, scope, space_id, can_trash, created_at, \
              expires_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&user)
        .bind(&token.name)
        .bind(&token.hash[..])
        .bind(&token.hint)
        .bind(token.level.as_str())
        .bind(scope)
        .bind(space_text.as_deref())
        .bind(token.can_trash)
        .bind(token.now)
        .bind(token.expires_at)
        .execute(&mut **tx)
        .await
        .map_err(StoreError::from_fk)?;
        if !pages.is_empty() {
            sqlx::query(
                "INSERT INTO api_token_pages (token_id, node_id) \
                 SELECT ?1, value FROM json_each(?2)",
            )
            .bind(id.to_string())
            .bind(json_ids(&pages))
            .execute(&mut **tx)
            .await
            .map_err(StoreError::from_fk)?;
        }
        tx.commit().await?;
        let scope = match token.scope {
            TokenScope::Pages { space, .. } => TokenScope::Pages { space, pages },
            other => other,
        };
        Ok(Some(TokenRow {
            id,
            user_id: token.user_id,
            name: token.name,
            hint: token.hint,
            level: token.level,
            scope,
            can_trash: token.can_trash,
            created_at: token.now,
            expires_at: token.expires_at,
            revoked_at: None,
            last_used_at: None,
            busy_count: 0,
            busy_last_at: None,
            calls_purged: 0,
        }))
    }

    /// Le jeton vivant à `now` de ce hachage, avec son compte et les pages qui lui restent, lus en
    /// une transaction ; `None` pour un hachage inconnu, un jeton révoqué ou échu (à son échéance
    /// même), ou d'un compte supprimé (ses jetons sont partis avec lui). Une lecture, sans tour :
    /// rien n'est écrit, pas même la date de dernière utilisation (le journal d'activité la pose,
    /// par paquets).
    pub async fn token_by_hash(
        &self,
        hash: [u8; 32],
        now: i64,
    ) -> Result<Option<LiveToken>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let Some(row) = sqlx::query(concat!(
            "SELECT id, user_id, name, level, scope, space_id, can_trash, expires_at \
             FROM api_tokens WHERE hash = ?1 AND ",
            live!()
        ))
        .bind(&hash[..])
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(None);
        };
        let id: String = row.get("id");
        let pages = sqlx::query_scalar::<_, String>(
            "SELECT node_id FROM api_token_pages WHERE token_id = ? ORDER BY node_id",
        )
        .bind(&id)
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|page| parse_uuid(page))
        .collect::<Result<Vec<_>, _>>()?;
        tx.commit().await?;
        Ok(Some(LiveToken {
            id: parse_uuid(&id)?,
            user_id: parse_uuid(&row.get::<String, _>("user_id"))?,
            name: row.get("name"),
            level: TokenLevel::parse(&row.get::<String, _>("level"))?,
            scope: TokenScope::read(&row.get::<String, _>("scope"), row.get("space_id"), pages)?,
            can_trash: row.get("can_trash"),
            expires_at: row.get("expires_at"),
        }))
    }

    /// Les jetons du compte qui se lisent encore à `now` (vivants, ou morts depuis moins de
    /// [`DEAD_TOKEN_KEPT_MS`]), lus en une transaction : les vivants d'abord, du plus récent au
    /// plus ancien, puis les morts, du plus récemment mort au plus anciennement.
    pub async fn tokens_of(&self, user: Uuid, now: i64) -> Result<Vec<TokenRow>, StoreError> {
        let user = user.to_string();
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(concat!(
            "SELECT ",
            token_columns!(),
            " FROM api_tokens WHERE user_id = ?1 AND ",
            shown!(),
            " ORDER BY ",
            live!(),
            " DESC, CASE WHEN ",
            live!(),
            " THEN created_at ELSE ",
            died_at!(),
            " END DESC, id DESC"
        ))
        .bind(&user)
        .bind(now)
        .bind(now.saturating_sub(DEAD_TOKEN_KEPT_MS))
        .fetch_all(&mut *tx)
        .await?;
        let mut pages: HashMap<String, Vec<Uuid>> = HashMap::new();
        for r in sqlx::query(
            "SELECT p.token_id, p.node_id FROM api_token_pages p \
             JOIN api_tokens t ON t.id = p.token_id WHERE t.user_id = ? \
             ORDER BY p.token_id, p.node_id",
        )
        .bind(&user)
        .fetch_all(&mut *tx)
        .await?
        {
            pages
                .entry(r.get(0))
                .or_default()
                .push(parse_uuid(&r.get::<String, _>(1))?);
        }
        tx.commit().await?;
        rows.iter()
            .map(|r| {
                let pages = pages.remove(&r.get::<String, _>("id")).unwrap_or_default();
                token_row(r, pages)
            })
            .collect()
    }

    /// Révoque à `now` le jeton `id` du compte, puis retire les jetons morts qu'il ne garde plus
    /// (Ruling R1220), dans une transaction (voie du compte). `true` : révoqué maintenant ;
    /// `false` : il l'était déjà, rien n'est écrit. Lit d'abord, et ne prend son tour que s'il y a
    /// à écrire (Ruling R270). `NotFound`, sans rien écrire, pour un jeton inconnu, d'un autre
    /// compte, ou qui ne se lit plus (mort depuis [`DEAD_TOKEN_KEPT_MS`]) : la même réponse.
    pub async fn revoke_token(&self, user: Uuid, id: Uuid, now: i64) -> Result<bool, StoreError> {
        let (user, id) = (user.to_string(), id.to_string());
        let found = sqlx::query(concat!(
            "SELECT revoked_at IS NOT NULL FROM api_tokens WHERE user_id = ?1 AND id = ?4 AND ",
            shown!()
        ))
        .bind(&user)
        .bind(now)
        .bind(now.saturating_sub(DEAD_TOKEN_KEPT_MS))
        .bind(&id)
        .fetch_optional(&self.pool)
        .await?;
        match found.map(|r| r.get::<bool, _>(0)) {
            None => return Err(StoreError::NotFound),
            Some(true) => return Ok(false),
            Some(false) => {}
        }
        let mut tx = self.write().await?;
        let revoked = sqlx::query(
            "UPDATE api_tokens SET revoked_at = ? WHERE id = ? AND user_id = ? \
             AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(&id)
        .bind(&user)
        .execute(&mut **tx)
        .await?
        .rows_affected();
        if revoked == 0 {
            // Révoqué (ou retiré) entre la lecture et le tour : rien à écrire.
            return Ok(false);
        }
        purge_dead_tokens(&mut tx, &user, now).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Le jeton `id` vit-il encore à `now` (ni révoqué, ni échu, ni parti) ? Une lecture, sans
    /// tour : pour qui relit, juste avant d'écrire, qu'un appel long a encore le droit de le faire.
    pub async fn token_is_live(&self, id: Uuid, now: i64) -> Result<bool, StoreError> {
        Ok(sqlx::query(concat!(
            "SELECT 1 FROM api_tokens WHERE id = ?1 AND ",
            live!()
        ))
        .bind(id.to_string())
        .bind(now)
        .fetch_optional(&self.pool)
        .await?
        .is_some())
    }
}
