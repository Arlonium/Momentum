//! Invitations : dans un espace (un rôle) ou sur une page (un niveau d'accès).
//!
//! C'est le jeton qui fait foi, jamais l'adresse e-mail, qui n'est pas vérifiée (Ruling R47) :
//! l'adresse est gardée pour l'affichage, rognée et en minuscules. Le store ne voit que le
//! SHA-256 du jeton. Une invitation sert une fois ; échue, révoquée (supprimée) ou déjà
//! acceptée, elle n'existe plus pour personne (`NotFound`).
//!
//! Une invitation ne vaut que tant que son auteur a encore le droit de la faire (Ruling R69) :
//! celles qu'il attendait encore partent avec lui quand il quitte l'espace (`remove_member`) ;
//! une invitation d'espace ne s'accepte plus, et ne se consulte plus par son jeton, dès que son
//! auteur n'est plus `owner` ni `admin` de l'espace. Pour une invitation de page, c'est le
//! serveur qui vérifie que l'auteur a encore `full` sur la page (le calcul des droits n'est pas
//! ici).

use std::fmt;

use mementum_acl::{Grant, Level, Role, Subject};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::acl::write_grant;
use crate::members::{raise_role, role_from};
use crate::spaces::bump_rev;
use crate::users::normalize_email;
use crate::{Store, StoreError, now_ms, parse_uuid, unreadable};

/// Ce que donne une invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationGrant {
    /// Ce rôle dans l'espace.
    Space(Role),
    /// Ce niveau sur cette page (et, par héritage, ses sous-pages) ; la personne entre dans
    /// l'espace comme `guest` si elle n'y a pas de rôle.
    Page { node_id: Uuid, level: Level },
}

/// Une invitation à créer.
#[derive(Clone)]
pub struct NewInvitation {
    pub space_id: Uuid,
    pub grant: InvitationGrant,
    /// Rognée et mise en minuscules par le store.
    pub email: String,
    /// SHA-256 du jeton : le jeton lui-même n'est jamais stocké.
    pub token_hash: [u8; 32],
    pub created_by: Uuid,
    /// Millisecondes Unix.
    pub expires_at: i64,
}

/// `Debug` masque le hachage du jeton : une invitation écrite dans un journal ne le livre pas.
impl fmt::Debug for NewInvitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewInvitation")
            .field("space_id", &self.space_id)
            .field("grant", &self.grant)
            .field("email", &self.email)
            .field("token_hash", &"<masqué>")
            .field("created_by", &self.created_by)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Une invitation, sans son jeton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitation {
    pub id: Uuid,
    pub space_id: Uuid,
    pub grant: InvitationGrant,
    pub email: String,
    pub created_by: Uuid,
    /// Millisecondes Unix.
    pub created_at: i64,
    /// Millisecondes Unix : l'invitation vaut tant que `now < expires_at`.
    pub expires_at: i64,
}

/// Colonnes d'une invitation, en littéral pour que le SQL reste statique (`concat!`).
macro_rules! invitation_columns {
    () => {
        "id, space_id, node_id, email, grant_role, grant_level, created_by, created_at, expires_at"
    };
}

/// Une invitation en attente : ni acceptée, ni échue au paramètre lié (l'heure). Une révoquée
/// n'existe plus.
macro_rules! pending {
    () => {
        "accepted_at IS NULL AND expires_at > ?"
    };
}

/// Une invitation qu'on peut encore accepter : en attente, et, pour une invitation d'espace,
/// dont l'auteur est encore `owner` ou `admin` de l'espace (Ruling R69).
macro_rules! acceptable {
    () => {
        concat!(
            pending!(),
            " AND (node_id IS NOT NULL OR EXISTS ( \
                 SELECT 1 FROM members m WHERE m.space_id = invitations.space_id \
                 AND m.user_id = invitations.created_by AND m.role IN ('owner', 'admin')))"
        )
    };
}

fn invitation_from_row(r: &SqliteRow) -> Result<Invitation, StoreError> {
    let id: String = r.get("id");
    let node: Option<String> = r.get("node_id");
    let role: Option<String> = r.get("grant_role");
    let level: Option<String> = r.get("grant_level");
    let grant = match (node, role, level) {
        (None, Some(role), None) => InvitationGrant::Space(role_from(&role)?),
        (Some(node), None, Some(level)) => InvitationGrant::Page {
            node_id: parse_uuid(&node)?,
            level: Level::parse(&level).ok_or_else(|| unreadable("niveau d'accès", &level))?,
        },
        // Le schéma l'interdit (`CHECK`) : une erreur, jamais une ligne sautée.
        _ => return Err(unreadable("invitation", &id)),
    };
    Ok(Invitation {
        id: parse_uuid(&id)?,
        space_id: parse_uuid(&r.get::<String, _>("space_id"))?,
        grant,
        email: r.get("email"),
        created_by: parse_uuid(&r.get::<String, _>("created_by"))?,
        created_at: r.get("created_at"),
        expires_at: r.get("expires_at"),
    })
}

/// Pose `level` pour `subject` sur le nœud, dans la transaction, sans jamais abaisser le niveau
/// que le sujet y a déjà. Rend vrai s'il a écrit ; la révision est l'affaire de l'appelant.
async fn raise_level(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
    subject: Subject,
    level: Level,
) -> Result<bool, StoreError> {
    let current: Option<String> =
        sqlx::query("SELECT level FROM acl WHERE node_id = ? AND subject = ?")
            .bind(node_id)
            .bind(subject.to_string())
            .fetch_optional(&mut **tx)
            .await?
            .map(|r| r.get(0));
    if let Some(text) = current {
        let current = Level::parse(&text).ok_or_else(|| unreadable("niveau d'accès", &text))?;
        if current >= level {
            return Ok(false);
        }
    }
    write_grant(tx, node_id, Grant { subject, level }).await?;
    Ok(true)
}

impl Store {
    /// Crée une invitation. `NotFound` (sans rien écrire) si l'espace ou l'auteur n'existe pas,
    /// ou si la page d'une invitation de page n'est pas un nœud de cet espace. Une invitation
    /// n'est pas un droit : la révision de l'espace ne bouge pas.
    pub async fn create_invitation(&self, i: NewInvitation) -> Result<Invitation, StoreError> {
        let space = i.space_id.to_string();
        let (node, role, level) = match i.grant {
            InvitationGrant::Space(role) => (None, Some(role.as_str()), None),
            InvitationGrant::Page { node_id, level } => {
                (Some(node_id.to_string()), None, Some(level.as_str()))
            }
        };
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(node) = &node {
            sqlx::query("SELECT 1 FROM nodes WHERE id = ? AND space_id = ?")
                .bind(node)
                .bind(&space)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(StoreError::NotFound)?;
        }
        let invitation = Invitation {
            id: Uuid::now_v7(),
            space_id: i.space_id,
            grant: i.grant,
            email: normalize_email(&i.email),
            created_by: i.created_by,
            created_at: now_ms(),
            expires_at: i.expires_at,
        };
        sqlx::query(concat!(
            "INSERT INTO invitations (",
            invitation_columns!(),
            ", token_hash) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ))
        .bind(invitation.id.to_string())
        .bind(&space)
        .bind(node)
        .bind(&invitation.email)
        .bind(role)
        .bind(level)
        .bind(invitation.created_by.to_string())
        .bind(invitation.created_at)
        .bind(invitation.expires_at)
        .bind(&i.token_hash[..])
        .execute(&mut *tx)
        .await
        .map_err(StoreError::from_fk)?;
        tx.commit().await?;
        Ok(invitation)
    }

    /// L'invitation de ce jeton, si elle vaut encore à `now` : ni échue, ni révoquée, ni
    /// acceptée, et, pour une invitation d'espace, dont l'auteur est encore `owner` ou `admin`
    /// de l'espace (R69).
    pub async fn invitation_by_token(
        &self,
        token_hash: [u8; 32],
        now: i64,
    ) -> Result<Option<Invitation>, StoreError> {
        sqlx::query(concat!(
            "SELECT ",
            invitation_columns!(),
            " FROM invitations WHERE token_hash = ? AND ",
            acceptable!()
        ))
        .bind(&token_hash[..])
        .bind(now)
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(invitation_from_row)
        .transpose()
    }

    /// Accepte l'invitation de ce jeton pour le compte, en une transaction. `NotFound` si le
    /// jeton est inconnu, échu, révoqué ou déjà accepté, si l'auteur d'une invitation d'espace
    /// n'est plus `owner` ni `admin` de l'espace (R69, lu dans la même transaction), ou si le
    /// compte n'existe pas. Une invitation d'espace donne son rôle ; une invitation de page pose
    /// `(user:<id>, level)` et fait de la personne un `guest` si elle n'a pas de rôle dans
    /// l'espace. Rien n'est jamais abaissé, ni rôle ni niveau. Puis l'invitation est marquée
    /// acceptée : elle ne sert plus.
    pub async fn accept_invitation(
        &self,
        token_hash: [u8; 32],
        user_id: Uuid,
        now: i64,
    ) -> Result<Invitation, StoreError> {
        let user = user_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query(concat!(
            "SELECT ",
            invitation_columns!(),
            " FROM invitations WHERE token_hash = ? AND ",
            acceptable!()
        ))
        .bind(&token_hash[..])
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
        let invitation = invitation_from_row(&row)?;
        let space = invitation.space_id.to_string();
        let wrote = match invitation.grant {
            InvitationGrant::Space(role) => raise_role(&mut tx, &space, &user, role).await?,
            InvitationGrant::Page { node_id, level } => {
                let joined = raise_role(&mut tx, &space, &user, Role::Guest).await?;
                let raised =
                    raise_level(&mut tx, &node_id.to_string(), Subject::User(user_id), level)
                        .await?;
                joined || raised
            }
        };
        sqlx::query("UPDATE invitations SET accepted_at = ?, accepted_by = ? WHERE id = ?")
            .bind(now)
            .bind(&user)
            .bind(invitation.id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(StoreError::from_fk)?;
        if wrote {
            bump_rev(&mut tx, &space).await?;
        }
        tx.commit().await?;
        Ok(invitation)
    }

    /// L'invitation `id` si elle est en attente à `now` : `None` si elle est inconnue, échue,
    /// révoquée ou acceptée (Ruling R68). Une invitation d'espace dont l'auteur a perdu son
    /// rôle d'admin est rendue : elle ne s'accepte plus, mais la route doit pouvoir la révoquer.
    pub async fn get_invitation(
        &self,
        id: Uuid,
        now: i64,
    ) -> Result<Option<Invitation>, StoreError> {
        sqlx::query(concat!(
            "SELECT ",
            invitation_columns!(),
            " FROM invitations WHERE id = ? AND ",
            pending!()
        ))
        .bind(id.to_string())
        .bind(now)
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(invitation_from_row)
        .transpose()
    }

    /// Révoque une invitation qui n'a pas été acceptée : elle est supprimée. `NotFound` si elle
    /// n'existe pas ou a déjà été acceptée (c'est alors un droit, qui se retire autrement).
    pub async fn revoke_invitation(&self, id: Uuid) -> Result<(), StoreError> {
        let removed = sqlx::query("DELETE FROM invitations WHERE id = ? AND accepted_at IS NULL")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?
            .rows_affected();
        if removed == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    /// Les invitations de l'espace qui attendent encore d'être acceptées, d'espace et de pages,
    /// par date de création : ni acceptées, ni révoquées, ni échues à l'heure du serveur. Comme
    /// pour [`Store::get_invitation`], une invitation d'espace dont l'auteur a perdu son rôle
    /// d'admin y figure encore : elle ne s'accepte plus, mais se révoque.
    pub async fn pending_invitations(&self, space_id: Uuid) -> Result<Vec<Invitation>, StoreError> {
        sqlx::query(concat!(
            "SELECT ",
            invitation_columns!(),
            " FROM invitations WHERE space_id = ? AND ",
            pending!(),
            " ORDER BY created_at, id"
        ))
        .bind(space_id.to_string())
        .bind(now_ms())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(invitation_from_row)
        .collect()
    }
}
