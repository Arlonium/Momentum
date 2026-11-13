//! Membres d'un espace : rôles, départ, adoption des espaces sans membre, retrait de l'accès
//! d'une personne à une page.
//!
//! Un espace garde toujours au moins un `owner` (`LastOwner`). Toute écriture dans `members`, et
//! toute écriture dans `acl` faite d'ici, incrémente la révision de l'espace dans sa transaction
//! (règle de `rev`, en tête de la crate) ; un appel qui n'écrit rien ne la touche pas.

use std::collections::{HashMap, HashSet};

use mementum_acl::{Level, Role, Subject};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::acl::{read_chain, space_of};
use crate::spaces::{Space, WriteTx, bump_rev, space_from_row};
use crate::{Store, StoreError, now_ms, parse_uuid, unreadable};

/// Une personne qui a un rôle dans un espace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub user_id: Uuid,
    pub name: String,
    pub email: String,
    pub role: Role,
}

/// Ce sur quoi l'appelant de [`Store::set_role`] ou de [`Store::remove_member`] a décidé :
/// qui le demande, son rôle, et le rôle de la personne visée, tels qu'il les a lus. Le store les
/// relit dans sa transaction ; s'ils ont changé, il refuse (`Stale`) sans rien écrire, et
/// l'appelant redécide sur les rôles relus. Pour qui part, `actor` est la personne visée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expected {
    pub actor: Uuid,
    pub actor_role: Role,
    pub target_role: Role,
}

/// Relit, dans la transaction, les rôles sur lesquels l'appelant a décidé (voir [`Expected`]),
/// et rend celui de la personne visée. `NotFound` si elle n'a pas de rôle dans l'espace ;
/// `Stale` si elle ou l'acteur n'a plus le rôle attendu.
async fn check_expected(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
    user_id: &str,
    expected: Expected,
) -> Result<Role, StoreError> {
    let current = role_of(tx, space_id, user_id)
        .await?
        .ok_or(StoreError::NotFound)?;
    let actor = role_of(tx, space_id, &expected.actor.to_string()).await?;
    if current != expected.target_role || actor != Some(expected.actor_role) {
        return Err(StoreError::Stale);
    }
    Ok(current)
}

/// Relit `members.role` : une valeur inconnue est une erreur, jamais une ligne sautée.
pub(crate) fn role_from(text: &str) -> Result<Role, StoreError> {
    Role::parse(text).ok_or_else(|| unreadable("rôle", text))
}

fn role_from_row(r: &SqliteRow) -> Result<Role, StoreError> {
    role_from(&r.get::<String, _>("role"))
}

/// Le rôle de `user_id` dans l'espace, lu dans la transaction.
pub(crate) async fn role_of(
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
        Some(current) if current.rank() >= role.rank() => return Ok(false),
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

/// Fait le compte `owner` de tout espace qui n'a aucun membre, dans la transaction, et rend ces
/// espaces, par date de création, à leur révision nouvelle. `NotFound` si le compte n'existe
/// pas et qu'il y avait un espace à prendre.
pub(crate) async fn adopt_orphans(
    tx: &mut WriteTx,
    user_id: Uuid,
) -> Result<Vec<Space>, StoreError> {
    let user = user_id.to_string();
    let mut spaces = sqlx::query(
        "SELECT id, name, rev, created_at FROM spaces s \
         WHERE NOT EXISTS (SELECT 1 FROM members m WHERE m.space_id = s.id) \
         ORDER BY created_at, id",
    )
    .fetch_all(&mut ***tx)
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
        .execute(&mut ***tx)
        .await
        .map_err(StoreError::from_fk)?;
        bump_rev(tx, &id).await?;
        space.rev += 1;
    }
    Ok(spaces)
}

/// Ceux qui ont un rôle dans l'espace, invités compris, lus dans la transaction.
async fn with_role(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
) -> Result<HashSet<Uuid>, StoreError> {
    sqlx::query("SELECT user_id FROM members WHERE space_id = ?")
        .bind(space_id)
        .fetch_all(&mut **tx)
        .await?
        .iter()
        .map(|r| parse_uuid(&r.get::<String, _>(0)))
        .collect()
}

/// Relit le sujet et le niveau d'une ligne d'`acl` : une valeur inconnue est une erreur.
fn line_from(subject: &str, level: &str) -> Result<(Subject, Level), StoreError> {
    Ok((
        Subject::parse(subject).ok_or_else(|| unreadable("sujet d'accès", subject))?,
        Level::parse(level).ok_or_else(|| unreadable("niveau d'accès", level))?,
    ))
}

/// Ruling R70 : une ligne d'accès donne-t-elle un droit ? `members`, toujours ; `user:<id>`, si
/// le compte a encore un rôle dans l'espace (`with_role`). Une ligne restée à quelqu'un sans
/// rôle (celles d'un compte parti, sur ses pages privées) ne compte pas.
fn grants_a_right(subject: Subject, with_role: &HashSet<Uuid>) -> bool {
    match subject {
        Subject::Members => true,
        Subject::User(id) => with_role.contains(&id),
    }
}

/// Ruling R70 : ces lignes, qui donnent un droit ([`grants_a_right`]), mettent-elles quelqu'un
/// en `full` sur leur page, et par héritage sur ses descendants ? Une ligne `members` compte à
/// tout niveau : les propriétaires et les admins y ont `full`, et l'espace a toujours un
/// propriétaire.
fn someone_in_full(lines: &[(Subject, Level)]) -> bool {
    lines
        .iter()
        .any(|(subject, level)| *subject == Subject::Members || *level == Level::Full)
}

/// Ruling R70, dans la transaction d'un changement d'accès (`Store::change_access`) : la page
/// `node_id` a-t-elle encore quelqu'un en `full`, par ses propres lignes ou par celles d'un
/// ancêtre ? La même notion que [`hand_over_full`] : seules comptent les lignes qui donnent un
/// droit, et une ligne `members` met quelqu'un en `full`.
///
/// La chaîne est relue après l'écriture, par la même lecture que le reste du partage
/// (`read_chain`) : les lignes d'un parent d'un autre espace ne comptent pas, et une page dont
/// la chaîne ne se lit pas (un cycle de parents) n'a personne.
pub(crate) async fn keeps_full(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
    node_id: &str,
) -> Result<bool, StoreError> {
    let Some(chain) = read_chain(tx, node_id).await? else {
        return Ok(false);
    };
    let with_role = with_role(tx, space_id).await?;
    let lines: Vec<(Subject, Level)> = chain
        .lines
        .iter()
        .map(|(_, line)| (line.subject, line.level))
        .filter(|(subject, _)| grants_a_right(*subject, &with_role))
        .collect();
    Ok(someone_in_full(&lines))
}

/// Supprime, dans la transaction, les invitations que `user_id` avait faites dans l'espace et
/// qui attendaient encore (Ruling R69) : au départ d'un membre comme à la sortie d'un invité.
async fn drop_pending_invitations(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    sqlx::query(
        "DELETE FROM invitations WHERE space_id = ? AND created_by = ? AND accepted_at IS NULL",
    )
    .bind(space_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Si `user_id` est invité dans l'espace et n'y a plus aucune ligne d'accès, il en sort, dans la
/// transaction, avec les invitations qu'il y avait faites et qui attendaient encore, comme au
/// départ d'un membre. Rend vrai s'il est sorti ; la révision est l'affaire de l'appelant.
pub(crate) async fn guest_leaves_if_bare(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
    user_id: Uuid,
) -> Result<bool, StoreError> {
    let user = user_id.to_string();
    if role_of(tx, space_id, &user).await? != Some(Role::Guest) {
        return Ok(false);
    }
    let left: bool = sqlx::query(
        "SELECT EXISTS (SELECT 1 FROM acl a JOIN nodes n ON n.id = a.node_id \
                        WHERE n.space_id = ? AND a.subject = ?)",
    )
    .bind(space_id)
    .bind(Subject::User(user_id).to_string())
    .fetch_one(&mut **tx)
    .await?
    .get(0);
    if left {
        return Ok(false);
    }
    sqlx::query("DELETE FROM members WHERE space_id = ? AND user_id = ?")
        .bind(space_id)
        .bind(&user)
        .execute(&mut **tx)
        .await?;
    drop_pending_invitations(tx, space_id, &user).await?;
    Ok(true)
}

/// Ruling R70, dans la transaction de [`Store::remove_member`], une fois la personne qui part
/// retirée de `members` et ses lignes partagées retirées : sur toute page dont elle était le
/// dernier sujet en `full` (`had_full` : les pages où elle avait une ligne `full` ; le `full`
/// d'un ancêtre vaut pour ses descendants) et qui garde d'autres lignes propres, les lignes de
/// plus haut niveau passent à `full`. Une ligne `members` qui s'applique (sur la page ou un
/// ancêtre) compte comme un sujet en `full`, les admins y ayant `full` : rien à faire alors.
/// Aucun sujet nouveau n'apparaît.
///
/// Seules comptent les lignes qui donnent un droit ([`grants_a_right`]) : `members`, et celles
/// des comptes qui ont encore un rôle dans l'espace. Une ligne restée à quelqu'un sans rôle
/// (celles de la personne qui part, sur ses pages privées, ou d'un départ plus ancien) ne
/// compte pas, et n'est pas élevée. `Store::change_access` refuse un changement sur la même
/// notion ([`keeps_full`]).
///
/// L'arbre est parcouru des racines vers les feuilles : une page dont un ancêtre vient d'être
/// confié à quelqu'un en `full` en hérite, et n'a pas besoin d'élévation. Un nœud pris dans un
/// cycle de parents (que le store interdit) n'est vu par personne (R58) et n'est pas visité.
async fn hand_over_full(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
    had_full: &HashSet<String>,
) -> Result<(), StoreError> {
    if had_full.is_empty() {
        return Ok(());
    }
    let mut children: HashMap<Option<String>, Vec<String>> = HashMap::new();
    for r in sqlx::query("SELECT id, parent_id FROM nodes WHERE space_id = ?")
        .bind(space_id)
        .fetch_all(&mut **tx)
        .await?
    {
        children.entry(r.get(1)).or_default().push(r.get(0));
    }
    // Ceux qui ont encore un rôle dans l'espace (la personne qui part n'en a plus).
    let with_role = with_role(tx, space_id).await?;
    // Les lignes propres de chaque page qui donnent un droit.
    let mut lines: HashMap<String, Vec<(Subject, Level)>> = HashMap::new();
    for r in sqlx::query(
        "SELECT a.node_id, a.subject, a.level FROM acl a JOIN nodes n ON n.id = a.node_id \
         WHERE n.space_id = ?",
    )
    .bind(space_id)
    .fetch_all(&mut **tx)
    .await?
    {
        let line = line_from(&r.get::<String, _>(1), &r.get::<String, _>(2))?;
        if grants_a_right(line.0, &with_role) {
            lines.entry(r.get(0)).or_default().push(line);
        }
    }

    // (page, la personne y avait `full`, quelqu'un d'autre y a `full`), hérités du parent.
    let mut todo: Vec<(String, bool, bool)> = children
        .get(&None)
        .into_iter()
        .flatten()
        .map(|id| (id.clone(), false, false))
        .collect();
    let mut raised: Vec<(String, Subject)> = Vec::new();
    while let Some((id, parent_had_full, parent_others_full)) = todo.pop() {
        let own = lines.get(&id).map(Vec::as_slice).unwrap_or_default();
        let had = parent_had_full || had_full.contains(&id);
        let mut others = parent_others_full || someone_in_full(own);
        if had
            && !others
            && let Some(top) = own.iter().map(|(_, l)| *l).max()
        {
            raised.extend(
                own.iter()
                    .filter(|(_, l)| *l == top)
                    .map(|(s, _)| (id.clone(), *s)),
            );
            others = true;
        }
        for child in children.get(&Some(id)).into_iter().flatten() {
            todo.push((child.clone(), had, others));
        }
    }
    for (node, subject) in raised {
        sqlx::query("UPDATE acl SET level = 'full' WHERE node_id = ? AND subject = ?")
            .bind(&node)
            .bind(subject.to_string())
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Les espaces où un compte (lié en paramètre) a un rôle, avec ce rôle, par date de création.
const SPACES_OF: &str = "SELECT s.id AS id, s.name AS name, s.rev AS rev, \
                         s.created_at AS created_at, m.role AS role \
                         FROM members m JOIN spaces s ON s.id = m.space_id \
                         WHERE m.user_id = ? ORDER BY s.created_at, s.id";

/// [`Store::spaces_of`], lu dans la transaction : ce qu'elle a écrit compris.
pub(crate) async fn spaces_of_in(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: Uuid,
) -> Result<Vec<(Space, Role)>, StoreError> {
    sqlx::query(SPACES_OF)
        .bind(user_id.to_string())
        .fetch_all(&mut **tx)
        .await?
        .iter()
        .map(|r| Ok((space_from_row(r)?, role_from_row(r)?)))
        .collect()
}

impl Store {
    /// Les espaces où le compte a un rôle (invité compris), avec ce rôle, par date de création.
    pub async fn spaces_of(&self, user_id: Uuid) -> Result<Vec<(Space, Role)>, StoreError> {
        sqlx::query(SPACES_OF)
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

    /// **Réservée aux tests** : n'applique aucune vérification de droits. Elle ne sait pas qui
    /// demande, et ne relit pas les rôles sur lesquels il a décidé (`Expected`, R100). Une route
    /// passe par une invitation ([`Store::accept_invitation`], [`Store::sign_up`]) pour faire
    /// entrer quelqu'un dans un espace, et par [`Store::set_role`] pour changer un rôle.
    ///
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
        let mut tx = self.write().await?;
        if raise_role(&mut tx, &space, &user_id.to_string(), role).await? {
            bump_rev(&mut tx, &space).await?;
            tx.commit().await?;
        }
        Ok(())
    }

    /// Change le rôle d'un membre, dans les deux sens, et rend son rôle d'avant, lu dans la
    /// transaction. `NotFound` s'il n'a pas de rôle dans l'espace ; `Stale` si lui ou l'acteur
    /// n'a plus le rôle sur lequel l'appelant a décidé (`expected`) ; `LastOwner` si c'est le
    /// dernier propriétaire et qu'il cesserait de l'être. Les refus n'écrivent rien ; garder le
    /// même rôle non plus.
    pub async fn set_role(
        &self,
        space_id: Uuid,
        user_id: Uuid,
        role: Role,
        expected: Expected,
    ) -> Result<Role, StoreError> {
        let (space, user) = (space_id.to_string(), user_id.to_string());
        let mut tx = self.write().await?;
        let current = check_expected(&mut tx, &space, &user, expected).await?;
        if current == role {
            return Ok(current);
        }
        if current == Role::Owner && owners(&mut tx, &space).await? == 1 {
            return Err(StoreError::LastOwner);
        }
        sqlx::query("UPDATE members SET role = ? WHERE space_id = ? AND user_id = ?")
            .bind(role.as_str())
            .bind(&space)
            .bind(&user)
            .execute(&mut **tx)
            .await?;
        bump_rev(&mut tx, &space).await?;
        tx.commit().await?;
        Ok(current)
    }

    /// Retire le compte de l'espace, en une transaction :
    /// - ses lignes `user:<id>` de l'`acl` de l'espace partent, **sauf** sur ses pages
    ///   privées : celles où aucune ligne, sur la page ni sur ses ancêtres, ne nomme un autre
    ///   sujet que lui. Sans rôle, il ne les voit plus ; il les retrouve s'il revient dans
    ///   l'espace ;
    /// - les invitations qu'il avait faites dans l'espace et qui attendaient encore sont
    ///   supprimées (Ruling R69) ;
    /// - une page partagée ne reste pas sans personne en `full` (Ruling R70) : sur une page dont
    ///   il était le dernier sujet en `full`, les lignes de plus haut niveau qui restent passent
    ///   à `full` (détail dans `hand_over_full`).
    ///
    /// Aucun nœud n'est supprimé. `NotFound` s'il n'a pas de rôle dans l'espace ; `Stale` si lui
    /// ou l'acteur n'a plus le rôle sur lequel l'appelant a décidé (`expected` ; pour qui part,
    /// l'acteur est lui) ; `LastOwner` si c'est le dernier propriétaire. Tout est écrit, ou
    /// rien.
    pub async fn remove_member(
        &self,
        space_id: Uuid,
        user_id: Uuid,
        expected: Expected,
    ) -> Result<(), StoreError> {
        let (space, user) = (space_id.to_string(), user_id.to_string());
        let subject = Subject::User(user_id).to_string();
        let mut tx = self.write().await?;
        let current = check_expected(&mut tx, &space, &user, expected).await?;
        if current == Role::Owner && owners(&mut tx, &space).await? == 1 {
            return Err(StoreError::LastOwner);
        }
        // Les pages où la personne a une ligne `full`, lues avant que ses lignes partent.
        let had_full: HashSet<String> = sqlx::query(
            "SELECT a.node_id FROM acl a JOIN nodes n ON n.id = a.node_id \
             WHERE n.space_id = ? AND a.subject = ? AND a.level = 'full'",
        )
        .bind(&space)
        .bind(&subject)
        .fetch_all(&mut **tx)
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
        sqlx::query("DELETE FROM members WHERE space_id = ? AND user_id = ?")
            .bind(&space)
            .bind(&user)
            .execute(&mut **tx)
            .await?;
        drop_pending_invitations(&mut tx, &space, &user).await?;
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
        .bind(&subject)
        .execute(&mut **tx)
        .await?;
        hand_over_full(&mut tx, &space, &had_full).await?;
        bump_rev(&mut tx, &space).await?;
        tx.commit().await?;
        Ok(())
    }

    /// **Réservée aux tests** : n'applique aucune vérification de droits. Elle fait de n'importe
    /// quel compte le propriétaire des espaces sans membre. Le serveur ne les confie qu'au
    /// premier compte de l'instance, dans la transaction de son inscription : une route passe
    /// par [`Store::sign_up`].
    ///
    /// Fait le compte `owner` de tout espace qui n'a aucun membre (les espaces venus du lot 1),
    /// et rend ces espaces, par date de création, à leur révision nouvelle. `NotFound` (sans
    /// rien écrire) si le compte n'existe pas et qu'il y avait un espace à prendre.
    pub async fn adopt_orphan_spaces(&self, user_id: Uuid) -> Result<Vec<Space>, StoreError> {
        let mut tx = self.write().await?;
        let spaces = adopt_orphans(&mut tx, user_id).await?;
        tx.commit().await?;
        Ok(spaces)
    }

    /// **Réservée aux tests** : n'applique aucune vérification de droits. Ni qui demande, ni son
    /// niveau, ni la corbeille (R87), ni la règle `last_full_access` (R70). Une route passe par
    /// [`Store::change_access`].
    ///
    /// Retire la ligne `user:<id>` posée sur le nœud (sans effet s'il n'y en a pas) ; si la
    /// personne est `guest` et n'a plus aucune ligne dans l'espace, elle en sort, avec ses
    /// invitations en attente. `NotFound` si le nœud n'existe pas.
    pub async fn revoke_user_grant(&self, node_id: Uuid, user_id: Uuid) -> Result<(), StoreError> {
        let node = node_id.to_string();
        let mut tx = self.write().await?;
        let space = space_of(&mut tx, &node).await?;
        let removed = sqlx::query("DELETE FROM acl WHERE node_id = ? AND subject = ?")
            .bind(&node)
            .bind(Subject::User(user_id).to_string())
            .execute(&mut **tx)
            .await?
            .rows_affected()
            > 0;
        let left = guest_leaves_if_bare(&mut tx, &space, user_id).await?;
        if removed || left {
            bump_rev(&mut tx, &space).await?;
            tx.commit().await?;
        }
        Ok(())
    }
}
