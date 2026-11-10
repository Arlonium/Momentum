//! Membres d'un espace : rôles, départ, adoption des espaces sans membre, retrait de l'accès
//! d'une personne à une page.
//!
//! Un espace garde toujours au moins un `owner` (`LastOwner`). Toute écriture dans `members`, et
//! toute écriture dans `acl` faite d'ici, incrémente la révision de l'espace dans sa transaction
//! (règle de `rev`, en tête de la crate) ; un appel qui n'écrit rien ne la touche pas.

use mementum_acl::{Role, Subject};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::acl::space_of;
use crate::spaces::{Space, bump_rev, space_from_row};
use crate::{Store, StoreError, now_ms, parse_uuid, unreadable};

/// Une personne qui a un rôle dans un espace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub user_id: Uuid,
    pub name: String,
    pub email: String,
    pub role: Role,
}

/// Rang d'un rôle : `guest` < `member` < `admin` < `owner`.
fn rank(role: Role) -> u8 {
    match role {
        Role::Guest => 0,
        Role::Member => 1,
        Role::Admin => 2,
        Role::Owner => 3,
    }
}

/// Relit `members.role` : une valeur inconnue est une erreur, jamais une ligne sautée.
pub(crate) fn role_from(text: &str) -> Result<Role, StoreError> {
    Role::parse(text).ok_or_else(|| unreadable("rôle", text))
}

fn role_from_row(r: &SqliteRow) -> Result<Role, StoreError> {
    role_from(&r.get::<String, _>("role"))
}

/// Le rôle de `user_id` dans l'espace, lu dans la transaction.
async fn role_of(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
    user_id: &str,
) -> Result<Option<Role>, StoreError> {
    sqlx::query("SELECT role FROM members WHERE space_id = ? AND user_id = ?")
        .bind(space_id)
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?
        .as_ref()
        .map(role_from_row)
        .transpose()
}

/// Le nombre de propriétaires de l'espace, lu dans la transaction.
async fn owners(tx: &mut Transaction<'_, Sqlite>, space_id: &str) -> Result<i64, StoreError> {
    Ok(
        sqlx::query("SELECT COUNT(*) FROM members WHERE space_id = ? AND role = 'owner'")
            .bind(space_id)
            .fetch_one(&mut **tx)
            .await?
            .get(0),
    )
}

/// Donne au moins `role` à `user_id` dans l'espace, dans la transaction : un rôle déjà plus
/// haut reste. Rend vrai s'il a écrit ; la révision est l'affaire de l'appelant. `NotFound` si
/// l'espace ou le compte n'existe pas.
pub(crate) async fn raise_role(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
    user_id: &str,
    role: Role,
) -> Result<bool, StoreError> {
    match role_of(tx, space_id, user_id).await? {
        Some(current) if rank(current) >= rank(role) => return Ok(false),
        Some(_) => {
            sqlx::query("UPDATE members SET role = ? WHERE space_id = ? AND user_id = ?")
                .bind(role.as_str())
                .bind(space_id)
                .bind(user_id)
                .execute(&mut **tx)
                .await?;
        }
        None => {
            sqlx::query(
                "INSERT INTO members (space_id, user_id, role, created_at) VALUES (?, ?, ?, ?)",
            )
            .bind(space_id)
            .bind(user_id)
            .bind(role.as_str())
            .bind(now_ms())
            .execute(&mut **tx)
            .await
            .map_err(StoreError::from_fk)?;
        }
    }
    Ok(true)
}

impl Store {
    /// Les espaces où le compte a un rôle (invité compris), avec ce rôle, par date de création.
    pub async fn spaces_of(&self, user_id: Uuid) -> Result<Vec<(Space, Role)>, StoreError> {
        sqlx::query(
            "SELECT s.id AS id, s.name AS name, s.rev AS rev, s.created_at AS created_at, \
                    m.role AS role \
             FROM members m JOIN spaces s ON s.id = m.space_id \
             WHERE m.user_id = ? ORDER BY s.created_at, s.id",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(|r| Ok((space_from_row(r)?, role_from_row(r)?)))
        .collect()
    }

    /// Le rôle du compte dans l'espace ; `None` s'il n'en a pas (ou si l'un des deux n'existe
    /// pas).
    pub async fn role_in(&self, space_id: Uuid, user_id: Uuid) -> Result<Option<Role>, StoreError> {
        sqlx::query("SELECT role FROM members WHERE space_id = ? AND user_id = ?")
            .bind(space_id.to_string())
            .bind(user_id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(role_from_row)
            .transpose()
    }

    /// Les personnes qui ont un rôle dans l'espace (invités compris), dans l'ordre de leur
    /// arrivée. Vide pour un espace inconnu.
    pub async fn members(&self, space_id: Uuid) -> Result<Vec<Member>, StoreError> {
        sqlx::query(
            "SELECT m.user_id AS user_id, u.name AS name, u.email AS email, m.role AS role \
             FROM members m JOIN users u ON u.id = m.user_id \
             WHERE m.space_id = ? ORDER BY m.created_at, m.user_id",
        )
        .bind(space_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(|r| {
            Ok(Member {
                user_id: parse_uuid(&r.get::<String, _>("user_id"))?,
                name: r.get("name"),
                email: r.get("email"),
                role: role_from_row(r)?,
            })
        })
        .collect()
    }

    /// Ajoute le compte à l'espace avec ce rôle, ou le lui donne s'il en a un plus bas : un rôle
    /// existant n'est jamais abaissé (sans effet, et sans rien écrire, s'il est déjà au moins
    /// aussi haut). `NotFound` si l'espace ou le compte n'existe pas.
    pub async fn add_member(
        &self,
        space_id: Uuid,
        user_id: Uuid,
        role: Role,
    ) -> Result<(), StoreError> {
        let space = space_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if raise_role(&mut tx, &space, &user_id.to_string(), role).await? {
            bump_rev(&mut tx, &space).await?;
            tx.commit().await?;
        }
        Ok(())
    }

    /// Change le rôle d'un membre, dans les deux sens. `NotFound` s'il n'a pas de rôle dans
    /// l'espace ; `LastOwner` si c'est le dernier propriétaire et qu'il cesserait de l'être.
    /// Les refus n'écrivent rien ; garder le même rôle non plus.
    pub async fn set_role(
        &self,
        space_id: Uuid,
        user_id: Uuid,
        role: Role,
    ) -> Result<(), StoreError> {
        let (space, user) = (space_id.to_string(), user_id.to_string());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = role_of(&mut tx, &space, &user)
            .await?
            .ok_or(StoreError::NotFound)?;
        if current == role {
            return Ok(());
        }
        if current == Role::Owner && owners(&mut tx, &space).await? == 1 {
            return Err(StoreError::LastOwner);
        }
        sqlx::query("UPDATE members SET role = ? WHERE space_id = ? AND user_id = ?")
            .bind(role.as_str())
            .bind(&space)
            .bind(&user)
            .execute(&mut *tx)
            .await?;
        bump_rev(&mut tx, &space).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Retire le compte de l'espace, en une transaction :
    /// - ses lignes `user:<id>` de l'`acl` de l'espace partent, **sauf** sur ses pages
    ///   privées : celles où aucune ligne, sur la page ni sur ses ancêtres, ne nomme un autre
    ///   sujet que lui. Sans rôle, il ne les voit plus ; il les retrouve s'il revient dans
    ///   l'espace ;
    /// - les invitations qu'il avait faites dans l'espace et qui attendaient encore sont
    ///   supprimées (Ruling R69).
    ///
    /// Aucun nœud n'est supprimé. `NotFound` s'il n'a pas de rôle dans l'espace ; `LastOwner`
    /// si c'est le dernier propriétaire. Tout est écrit, ou rien.
    pub async fn remove_member(&self, space_id: Uuid, user_id: Uuid) -> Result<(), StoreError> {
        let (space, user) = (space_id.to_string(), user_id.to_string());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = role_of(&mut tx, &space, &user)
            .await?
            .ok_or(StoreError::NotFound)?;
        if current == Role::Owner && owners(&mut tx, &space).await? == 1 {
            return Err(StoreError::LastOwner);
        }
        sqlx::query("DELETE FROM members WHERE space_id = ? AND user_id = ?")
            .bind(&space)
            .bind(&user)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM invitations WHERE space_id = ? AND created_by = ? AND accepted_at IS NULL",
        )
        .bind(&space)
        .bind(&user)
        .execute(&mut *tx)
        .await?;
        // `chain` : chaque page qui porte une ligne de la personne, avec elle-même et ses
        // ancêtres. Une ligne part si un autre sujet apparaît sur cette chaîne. `UNION` (et non
        // `UNION ALL`) : la remontée s'arrête même si des parents tournaient en rond.
        sqlx::query(
            "WITH RECURSIVE \
                 mine(node_id) AS ( \
                     SELECT a.node_id FROM acl a JOIN nodes n ON n.id = a.node_id \
                     WHERE n.space_id = ?1 AND a.subject = ?2 \
                 ), \
                 chain(start, id) AS ( \
                     SELECT node_id, node_id FROM mine \
                     UNION \
                     SELECT chain.start, n.parent_id FROM chain JOIN nodes n ON n.id = chain.id \
                     WHERE n.parent_id IS NOT NULL \
                 ) \
             DELETE FROM acl WHERE subject = ?2 AND node_id IN ( \
                 SELECT chain.start FROM chain JOIN acl a ON a.node_id = chain.id \
                 WHERE a.subject != ?2 \
             )",
        )
        .bind(&space)
        .bind(Subject::User(user_id).to_string())
        .execute(&mut *tx)
        .await?;
        bump_rev(&mut tx, &space).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Fait le compte `owner` de tout espace qui n'a aucun membre (les espaces venus du lot 1),
    /// et rend ces espaces, par date de création, à leur révision nouvelle. `NotFound` (sans
    /// rien écrire) si le compte n'existe pas et qu'il y avait un espace à prendre.
    pub async fn adopt_orphan_spaces(&self, user_id: Uuid) -> Result<Vec<Space>, StoreError> {
        let user = user_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut spaces = sqlx::query(
            "SELECT id, name, rev, created_at FROM spaces s \
             WHERE NOT EXISTS (SELECT 1 FROM members m WHERE m.space_id = s.id) \
             ORDER BY created_at, id",
        )
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(space_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        let now = now_ms();
        for space in &mut spaces {
            let id = space.id.to_string();
            sqlx::query(
                "INSERT INTO members (space_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)",
            )
            .bind(&id)
            .bind(&user)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::from_fk)?;
            bump_rev(&mut tx, &id).await?;
            space.rev += 1;
        }
        tx.commit().await?;
        Ok(spaces)
    }

    /// Retire la ligne `user:<id>` posée sur le nœud (sans effet s'il n'y en a pas) ; si la
    /// personne est `guest` et n'a plus aucune ligne dans l'espace, elle en sort. `NotFound` si
    /// le nœud n'existe pas.
    pub async fn revoke_user_grant(&self, node_id: Uuid, user_id: Uuid) -> Result<(), StoreError> {
        let (node, user) = (node_id.to_string(), user_id.to_string());
        let subject = Subject::User(user_id).to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let space = space_of(&mut tx, &node).await?;
        let mut wrote = sqlx::query("DELETE FROM acl WHERE node_id = ? AND subject = ?")
            .bind(&node)
            .bind(&subject)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        if role_of(&mut tx, &space, &user).await? == Some(Role::Guest) {
            let left: bool = sqlx::query(
                "SELECT EXISTS (SELECT 1 FROM acl a JOIN nodes n ON n.id = a.node_id \
                                WHERE n.space_id = ? AND a.subject = ?)",
            )
            .bind(&space)
            .bind(&subject)
            .fetch_one(&mut *tx)
            .await?
            .get(0);
            if !left {
                sqlx::query("DELETE FROM members WHERE space_id = ? AND user_id = ?")
                    .bind(&space)
                    .bind(&user)
                    .execute(&mut *tx)
                    .await?;
                wrote = true;
            }
        }
        if wrote {
            bump_rev(&mut tx, &space).await?;
            tx.commit().await?;
        }
        Ok(())
    }
}
