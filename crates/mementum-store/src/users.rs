//! Comptes et sessions.
//!
//! Le store ne voit jamais un mot de passe ni un jeton en clair : seulement le hachage Argon2id
//! du mot de passe et le SHA-256 du jeton de session, calculés par le serveur. Une adresse
//! e-mail est gardée rognée et en minuscules, et cherchée de même.

use std::fmt;

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{Store, StoreError, now_ms, parse_uuid};

/// Un jour, en millisecondes : une session ne glisse qu'une fois par jour d'usage au plus.
const DAY_MS: i64 = 86_400_000;

/// Un compte.
#[derive(Clone, PartialEq, Eq)]
pub struct User {
    pub id: Uuid,
    /// Rognée et en minuscules.
    pub email: String,
    pub name: String,
    /// Hachage Argon2id (format PHC) ; `None` : un compte sans mot de passe (connexion par OIDC,
    /// prévue plus tard).
    pub password_hash: Option<String>,
    /// Admin de l'instance : le premier compte créé.
    pub instance_admin: bool,
    /// Millisecondes Unix.
    pub created_at: i64,
}

/// `Debug` masque le hachage du mot de passe : un compte écrit dans un journal ne le livre pas.
impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("email", &self.email)
            .field("name", &self.name)
            .field(
                "password_hash",
                &self.password_hash.as_ref().map(|_| "<masqué>"),
            )
            .field("instance_admin", &self.instance_admin)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// Un compte à créer. Au lot 2, tout compte naît avec un mot de passe.
#[derive(Clone)]
pub struct NewUser {
    /// Rognée et mise en minuscules par le store.
    pub email: String,
    /// Enregistré tel quel : les règles de nom sont celles de la route HTTP.
    pub name: String,
    /// Hachage Argon2id, calculé par le serveur.
    pub password_hash: String,
}

/// Une session à ouvrir.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NewSession {
    /// SHA-256 du jeton : le jeton lui-même n'est jamais stocké.
    pub token_hash: [u8; 32],
    /// Millisecondes Unix : la session vaut tant que `now < expires_at`.
    pub expires_at: i64,
}

/// `Debug` masque le hachage du jeton : une session écrite dans un journal ne le livre pas.
impl fmt::Debug for NewSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewSession")
            .field("token_hash", &"<masqué>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Écrit la session du compte dans la transaction. `NotFound` si le compte n'existe pas.
pub(crate) async fn insert_session(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: Uuid,
    session: NewSession,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&session.token_hash[..])
    .bind(user_id.to_string())
    .bind(now_ms())
    .bind(session.expires_at)
    .execute(&mut **tx)
    .await
    .map_err(StoreError::from_fk)?;
    Ok(())
}

/// La forme sous laquelle une adresse e-mail est gardée et cherchée.
pub(crate) fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Colonnes d'un compte, en littéral pour que le SQL reste statique (`concat!`).
macro_rules! user_columns {
    () => {
        "id, email, name, password_hash, instance_admin, created_at"
    };
}

fn user_from_row(r: &SqliteRow) -> Result<User, StoreError> {
    Ok(User {
        id: parse_uuid(&r.get::<String, _>("id"))?,
        email: r.get("email"),
        name: r.get("name"),
        password_hash: r.get("password_hash"),
        instance_admin: r.get("instance_admin"),
        created_at: r.get("created_at"),
    })
}

/// Écrit un compte neuf dans la transaction (ouverte par `BEGIN IMMEDIATE` : deux créations
/// concurrentes ne voient pas toutes deux une base vide). Le premier compte de l'instance en
/// devient l'admin. `EmailTaken` si l'adresse, rognée et en minuscules, est déjà prise.
pub(crate) async fn insert_user(
    tx: &mut Transaction<'_, Sqlite>,
    u: NewUser,
) -> Result<User, StoreError> {
    let first = sqlx::query("SELECT NOT EXISTS (SELECT 1 FROM users)")
        .fetch_one(&mut **tx)
        .await?
        .get::<bool, _>(0);
    let user = User {
        id: Uuid::now_v7(),
        email: normalize_email(&u.email),
        name: u.name,
        password_hash: Some(u.password_hash),
        instance_admin: first,
        created_at: now_ms(),
    };
    sqlx::query(concat!(
        "INSERT INTO users (",
        user_columns!(),
        ") VALUES (?, ?, ?, ?, ?, ?)"
    ))
    .bind(user.id.to_string())
    .bind(&user.email)
    .bind(&user.name)
    .bind(&user.password_hash)
    .bind(user.instance_admin)
    .bind(user.created_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => StoreError::EmailTaken,
        _ => StoreError::Sqlx(e),
    })?;
    Ok(user)
}

impl Store {
    /// Crée un compte. Le premier compte de l'instance en devient l'admin : la question est
    /// tranchée dans la transaction (`BEGIN IMMEDIATE`), pour que deux créations concurrentes
    /// ne voient pas toutes deux une base vide. `EmailTaken` si l'adresse, rognée et en
    /// minuscules, est déjà prise.
    ///
    /// Le compte seul : ni espace, ni invitation. Une inscription passe par
    /// [`Store::sign_up`], qui écrit le tout en une transaction.
    ///
    /// **Réservée aux tests** : elle n'applique ni le réglage des inscriptions, ni une
    /// invitation. Aucune route ne l'appelle.
    pub async fn create_user(&self, u: NewUser) -> Result<User, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let user = insert_user(&mut tx, u).await?;
        tx.commit().await?;
        Ok(user)
    }

    pub async fn get_user(&self, id: Uuid) -> Result<Option<User>, StoreError> {
        sqlx::query(concat!(
            "SELECT ",
            user_columns!(),
            " FROM users WHERE id = ?"
        ))
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(user_from_row)
        .transpose()
    }

    /// Le compte de cette adresse, rognée et mise en minuscules comme à la création.
    pub async fn user_by_email(&self, email: &str) -> Result<Option<User>, StoreError> {
        sqlx::query(concat!(
            "SELECT ",
            user_columns!(),
            " FROM users WHERE email = ?"
        ))
        .bind(normalize_email(email))
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(user_from_row)
        .transpose()
    }

    pub async fn count_users(&self) -> Result<i64, StoreError> {
        Ok(sqlx::query("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?
            .get(0))
    }

    /// Change le hachage du mot de passe et supprime toutes les sessions du compte, dans la
    /// même transaction. `NotFound` (sans rien écrire) si le compte n'existe pas.
    ///
    /// Sans condition : pour qui n'a vérifié aucun mot de passe (la commande `mementum user
    /// set-password`). Une demande qui a vérifié le mot de passe actuel passe par
    /// [`Store::change_password`].
    pub async fn set_password(&self, user_id: Uuid, password_hash: &str) -> Result<(), StoreError> {
        let id = user_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let changed = sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(password_hash)
            .bind(&id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Remplace le hachage `verified` du compte par `new_hash`, supprime toutes ses sessions
    /// et lui en ouvre une (`token_hash`, `expires_at`), dans la même transaction. `false`,
    /// sans rien écrire, si le hachage du compte n'est plus `verified` (ou si le compte
    /// n'existe pas).
    ///
    /// Ruling R120 : `verified` est le hachage contre lequel l'appelant vient de vérifier le
    /// mot de passe actuel. Entre cette vérification (un calcul Argon2, et l'attente de son
    /// tour) et l'écriture, un autre changement de mot de passe peut passer. La demande, qui a
    /// prouvé la connaissance d'un mot de passe qui n'est plus celui du compte, ne doit alors
    /// ni le remplacer, ni ouvrir de session : qui change son mot de passe pour chasser un
    /// intrus le verrait sinon repris par lui.
    pub async fn change_password(
        &self,
        user_id: Uuid,
        verified: &str,
        new_hash: &str,
        token_hash: [u8; 32],
        expires_at: i64,
    ) -> Result<bool, StoreError> {
        let id = user_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let changed =
            sqlx::query("UPDATE users SET password_hash = ? WHERE id = ? AND password_hash IS ?")
                .bind(new_hash)
                .bind(&id)
                .bind(verified)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        if changed == 0 {
            return Ok(false);
        }
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&token_hash[..])
        .bind(&id)
        .bind(now_ms())
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Ouvre une session pour le compte, si le hachage de son mot de passe est encore
    /// `verified`. `false`, sans rien écrire, s'il ne l'est plus (ou si le compte n'existe pas,
    /// ou n'a pas de mot de passe).
    ///
    /// Ruling R120 : `verified` est le hachage contre lequel l'appelant vient de vérifier un
    /// mot de passe. Entre cette vérification et l'écriture, un changement de mot de passe peut
    /// passer : il supprime les sessions qui existent à son commit, pas celle qui naîtrait
    /// après. La lecture du hachage et l'écriture de la session tiennent donc en une seule
    /// instruction : la session naît avant le changement, qui la supprime, ou ne naît pas.
    pub async fn create_session_verified(
        &self,
        user_id: Uuid,
        verified: &str,
        token_hash: [u8; 32],
        expires_at: i64,
    ) -> Result<bool, StoreError> {
        let created = sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, created_at, expires_at) \
             SELECT ?, id, ?, ? FROM users WHERE id = ? AND password_hash IS ?",
        )
        .bind(&token_hash[..])
        .bind(now_ms())
        .bind(expires_at)
        .bind(user_id.to_string())
        .bind(verified)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(created == 1)
    }

    /// Ouvre une session : `token_hash` est le SHA-256 du jeton, `expires_at` son échéance
    /// (millisecondes Unix). `NotFound` si le compte n'existe pas.
    ///
    /// **Réservée aux tests** : sans condition, elle ouvre une session à n'importe quel compte.
    /// Aucune route ne l'appelle. L'inscription ouvre la sienne dans la transaction qui crée le
    /// compte ([`Store::sign_up`], report m1 du lot 2) ; après la vérification d'un mot de passe,
    /// c'est [`Store::create_session_verified`].
    pub async fn create_session(
        &self,
        user_id: Uuid,
        token_hash: [u8; 32],
        expires_at: i64,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&token_hash[..])
        .bind(user_id.to_string())
        .bind(now_ms())
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(StoreError::from_fk)?;
        Ok(())
    }

    /// Le compte de la session ; `None` si le jeton est inconnu ou échu (`expires_at <= now`).
    /// Une session vivante dont il reste moins de `ttl_ms` moins un jour repart pour `ttl_ms`
    /// à partir de `now` : l'échéance glisse avec l'usage, sans écrire à chaque requête.
    pub async fn session_user(
        &self,
        token_hash: [u8; 32],
        now: i64,
        ttl_ms: i64,
    ) -> Result<Option<User>, StoreError> {
        let Some(row) = sqlx::query(concat!(
            "SELECT ",
            user_columns!(),
            ", (SELECT expires_at FROM sessions WHERE token_hash = ?1) AS expires_at \
             FROM users WHERE id = (SELECT user_id FROM sessions WHERE token_hash = ?1)"
        ))
        .bind(&token_hash[..])
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let expires_at: i64 = row.get("expires_at");
        if expires_at <= now {
            return Ok(None);
        }
        if expires_at - now < ttl_ms - DAY_MS {
            sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
                .bind(now + ttl_ms)
                .bind(&token_hash[..])
                .execute(&self.pool)
                .await?;
        }
        user_from_row(&row).map(Some)
    }

    /// La session vaut-elle encore : connue, et non échue (`expires_at > now`) ? Lecture seule :
    /// contrairement à [`Store::session_user`], l'échéance ne glisse pas.
    pub async fn session_is_live(
        &self,
        token_hash: [u8; 32],
        now: i64,
    ) -> Result<bool, StoreError> {
        Ok(
            sqlx::query("SELECT 1 FROM sessions WHERE token_hash = ? AND expires_at > ?")
                .bind(&token_hash[..])
                .bind(now)
                .fetch_optional(&self.pool)
                .await?
                .is_some(),
        )
    }

    /// Ferme une session, et rend le compte qu'elle portait, qu'elle soit échue ou non : à
    /// l'appelant de fermer ce qu'elle avait ouvert. `None`, sans rien écrire, pour un jeton
    /// inconnu.
    pub async fn delete_session(&self, token_hash: [u8; 32]) -> Result<Option<Uuid>, StoreError> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ? RETURNING user_id")
            .bind(&token_hash[..])
            .fetch_optional(&self.pool)
            .await?
            .map(|row| parse_uuid(&row.get::<String, _>(0)))
            .transpose()
    }

    /// Supprime les sessions échues (`expires_at <= now`) et rend leur nombre.
    pub async fn purge_sessions(&self, now: i64) -> Result<u64, StoreError> {
        Ok(sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await?
            .rows_affected())
    }
}
