//! Invitations : dans un espace (un rôle) ou sur une page (un niveau d'accès).
//!
//! C'est le jeton qui fait foi, jamais l'adresse e-mail, qui n'est pas vérifiée (Ruling R47) :
//! l'adresse est gardée pour l'affichage, rognée et en minuscules. Le store ne voit que le
//! SHA-256 du jeton. Une invitation sert une fois : une fois qu'elle a donné quelque chose à un
//! compte (Ruling R257 : acceptée par qui avait déjà ce qu'elle donne, elle n'est pas
//! consommée). Échue, révoquée (supprimée) ou déjà acceptée, elle n'existe plus pour personne
//! (`NotFound`).
//!
//! Une invitation ne vaut que tant que son auteur a encore le droit de la faire (Ruling R69) :
//! celles qu'il attendait encore partent avec lui quand il quitte l'espace (`remove_member`) ;
//! une invitation ne s'accepte plus, et ne se consulte plus par son jeton, dès que son auteur
//! n'a plus le rôle qu'elle demande : `owner` ou `admin` pour une invitation d'espace, `owner`
//! pour une invitation au rôle `owner`, un rôle quelconque pour une invitation de page. Pour
//! une invitation de page, c'est en plus le serveur qui vérifie que l'auteur a encore `full`
//! sur la page (le calcul des droits n'est pas ici).

use std::fmt;

use mementum_acl::{Grant, Level, Role, Subject, chain_level};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::acl::{read_chain, write_grant};
use crate::members::{raise_role, role_from, role_of};
use crate::spaces::{WriteTx, bump_rev};
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

/// Une invitation qu'on peut encore accepter (Ruling R69) : en attente, et dont l'auteur a
/// encore un rôle dans l'espace qui lui permet de la faire :
/// - une invitation de page : un rôle, quel qu'il soit (le niveau sur la page, c'est le
///   serveur qui le vérifie) ;
/// - une invitation d'espace : `owner`, ou `admin` si elle ne donne pas le rôle `owner`.
macro_rules! acceptable {
    () => {
        concat!(
            pending!(),
            " AND EXISTS ( \
                 SELECT 1 FROM members m WHERE m.space_id = invitations.space_id \
                 AND m.user_id = invitations.created_by \
                 AND (invitations.node_id IS NOT NULL \
                      OR m.role = 'owner' \
                      OR (m.role = 'admin' AND invitations.grant_role != 'owner')))"
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

/// L'invitation de ce jeton, lue dans la transaction, si elle peut encore être acceptée à
/// `now` (voir `acceptable!`) ; sinon `None`.
pub(crate) async fn acceptable(
    tx: &mut Transaction<'_, Sqlite>,
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
    .fetch_optional(&mut **tx)
    .await?
    .as_ref()
    .map(invitation_from_row)
    .transpose()
}

/// Le compte a-t-il déjà, sur la page `node_id`, au moins `level` ? Jugé dans la transaction,
/// sur son niveau effectif : celui que lui donnent, avec son rôle dans l'espace, les lignes de
/// la page et de ses ancêtres, par le calcul du partage ([`chain_level`], le même que celui de
/// tout l'espace). Une ligne à son nom, une page parente, l'accès général des membres (où un
/// propriétaire ou un admin a `full`) comptent tous. Sans rôle dans l'espace, non : il y
/// entrerait. Une page dont la remontée tourne en rond n'est vue de personne (R58) : non plus.
async fn already_has(
    tx: &mut Transaction<'_, Sqlite>,
    space: &str,
    node_id: &str,
    user_id: Uuid,
    level: Level,
) -> Result<bool, StoreError> {
    let Some(role) = role_of(tx, space, &user_id.to_string()).await? else {
        return Ok(false);
    };
    let Some(chain) = read_chain(tx, node_id).await? else {
        return Ok(false);
    };
    let has = chain_level(chain.lines.iter().map(|(_, line)| line), user_id, role);
    Ok(has.is_some_and(|has| has >= level))
}

/// Donne au compte ce que donne l'invitation (lue par [`acceptable`] dans la même
/// transaction), sans rien abaisser, puis la marque acceptée et fait bouger la révision de
/// l'espace. Rend vrai dans ce cas. `NotFound` si le compte n'existe pas.
///
/// Ruling R257 (report M7 du lot 2) : une invitation qui ne lui donnerait rien n'est pas
/// consommée. Rend faux, sans rien écrire (ni droit, ni acceptation, ni révision) : elle vaut
/// encore, pour qui elle donnerait quelque chose. « Rien » se juge d'abord, dans cette
/// transaction : pour un rôle, un rôle au moins égal ; pour une page, un niveau effectif au
/// moins égal ([`already_has`]), pas sa seule ligne propre. L'auteur d'une invitation de page y
/// a `full` le plus souvent par l'accès général ou par une page parente, sans ligne à son nom :
/// jugée sur cette ligne, son essai consommait le lien, et lui posait une ligne de plus.
pub(crate) async fn accept(
    tx: &mut WriteTx,
    invitation: &Invitation,
    user_id: Uuid,
    now: i64,
) -> Result<bool, StoreError> {
    let user = user_id.to_string();
    let space = invitation.space_id.to_string();
    let gave = match invitation.grant {
        InvitationGrant::Space(role) => raise_role(tx, &space, &user, role).await?,
        InvitationGrant::Page { node_id, level } => {
            let node = node_id.to_string();
            if already_has(tx, &space, &node, user_id, level).await? {
                false
            } else {
                let joined = raise_role(tx, &space, &user, Role::Guest).await?;
                let raised = raise_level(tx, &node, Subject::User(user_id), level).await?;
                joined || raised
            }
        }
    };
    // Rien n'a été écrit : l'invitation n'a rien donné, elle reste en attente.
    if !gave {
        return Ok(false);
    }
    sqlx::query("UPDATE invitations SET accepted_at = ?, accepted_by = ? WHERE id = ?")
        .bind(now)
        .bind(&user)
        .bind(invitation.id.to_string())
        .execute(&mut ***tx)
        .await
        .map_err(StoreError::from_fk)?;
    bump_rev(tx, &space).await?;
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
    /// acceptée, et dont l'auteur a encore le rôle qu'elle demande (R69, voir `acceptable!`).
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
    /// jeton est inconnu, échu, révoqué ou déjà accepté, si son auteur n'a plus le rôle qu'elle
    /// demande (R69, lu dans la même transaction), ou si le compte n'existe pas. Une invitation
    /// d'espace donne son rôle ; une invitation de page pose `(user:<id>, level)` et fait de la
    /// personne un `guest` si elle n'a pas de rôle dans l'espace. Rien n'est jamais abaissé, ni
    /// rôle ni niveau. Puis l'invitation est marquée acceptée : elle ne sert plus.
    ///
    /// Le booléen dit si l'invitation a donné quelque chose. Faux (Ruling R257) : le compte
    /// avait déjà ce rôle ou plus, ou, pour une page, ce niveau effectif ou plus (par une ligne
    /// à son nom, une page parente ou l'accès général) ; rien n'est écrit, et l'invitation
    /// n'est **pas** marquée acceptée : elle vaut encore, pour quelqu'un d'autre.
    pub async fn accept_invitation(
        &self,
        token_hash: [u8; 32],
        user_id: Uuid,
        now: i64,
    ) -> Result<(Invitation, bool), StoreError> {
        let mut tx = self.write().await?;
        let invitation = acceptable(&mut tx, token_hash, now)
            .await?
            .ok_or(StoreError::NotFound)?;
        let gave = accept(&mut tx, &invitation, user_id, now).await?;
        tx.commit().await?;
        Ok((invitation, gave))
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
