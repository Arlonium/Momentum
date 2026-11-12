//! Arbre de pages d'un espace : création, lecture, renommage, déplacement, corbeille,
//! suppression définitive.
//!
//! Une écriture d'ici qui change ce dont dépendent les droits (un nœud créé ou supprimé, un
//! changement de parent, la corbeille, un accès posé) incrémente la révision de l'espace dans sa
//! transaction ; un renommage et un changement de place sous le même parent n'y touchent pas
//! (règle de `rev`, en tête de la crate). Toutes avancent sa version (règle de `lsn`).

use mementum_acl::{Grant, Level, Role, Subject, chain_level};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqliteExecutor, Transaction};
use uuid::Uuid;

use crate::acl::{Chain, read_chain, write_grant};
use crate::members::role_of;
use crate::spaces::{bump_lsn, bump_rev};
use crate::{Store, StoreError, now_ms, parse_uuid, position_between, unreadable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Page,
    Folder,
}

impl NodeKind {
    fn as_str(self) -> &'static str {
        match self {
            NodeKind::Page => "page",
            NodeKind::Folder => "folder",
        }
    }

    fn parse(s: &str) -> Result<NodeKind, StoreError> {
        match s {
            "page" => Ok(NodeKind::Page),
            "folder" => Ok(NodeKind::Folder),
            other => Err(unreadable("genre de nœud", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: Uuid,
    pub space_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: NodeKind,
    pub title: String,
    /// Index fractionnaire, à comparer en ordre d'octets parmi les frères.
    pub position: String,
    /// Le compte qui a créé le nœud ; `None` : inconnu (lot 1).
    pub created_by: Option<Uuid>,
    /// Millisecondes Unix.
    pub created_at: i64,
    /// Le compte qui a renommé ou déplacé le nœud en dernier (son auteur, tant que personne ne
    /// l'a fait) ; `None` : inconnu.
    pub updated_by: Option<Uuid>,
    /// Millisecondes Unix.
    pub updated_at: i64,
    /// Millisecondes Unix ; `None` : le nœud n'a pas été mis à la corbeille. Ses descendants y
    /// sont par héritage, sans que leur `trashed_at` soit posé.
    pub trashed_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewNode {
    pub space_id: Uuid,
    pub parent_id: Option<Uuid>,
    /// Frère après lequel placer le nœud ; `None` = en tête de ses frères.
    pub after: Option<Sibling>,
    pub kind: NodeKind,
    /// Enregistré tel quel : les règles de titre sont celles de la route HTTP.
    pub title: String,
    pub created_by: Option<Uuid>,
    /// Accès posés sur le nœud, écrits avec lui. Si un sujet revient, son dernier niveau reste.
    pub grants: Vec<Grant>,
    /// Document de la page : écrit avec le nœud, en instantané de `seq` 0.
    pub doc: Option<Vec<u8>>,
}

/// Le frère après lequel ranger un nœud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sibling {
    /// Ce frère-là.
    Node(Uuid),
    /// Le dernier : le nœud va en fin de ses frères. C'est le store qui le trouve, dans la
    /// transaction qui écrit, sans que l'appelant ait à lire l'espace.
    Last,
}

/// Où mettre un nœud : sous `parent_id` (`None` = racine), après `after` (`None` = en tête).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub parent_id: Option<Uuid>,
    pub after: Option<Sibling>,
    /// Le parent sous lequel l'appelant a lu le nœud quand il a décidé (`Some(None)` : à la
    /// racine). Si le nœud n'est plus sous ce parent, rien n'est écrit (`Stale`) : l'appelant
    /// relit et redécide. `None` : pas de contrôle.
    pub from: Option<Option<Uuid>>,
}

/// Ce que rend [`Store::update_node`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Updated {
    /// Le nœud, tel qu'il est après l'appel.
    pub node: Node,
    /// Le nœud vient d'arriver à la racine : ce que la transaction y a décidé. `None` dans tous
    /// les autres cas.
    pub at_root: Option<AtRoot>,
}

/// Un nœud qui vient d'arriver à la racine, tel que la transaction de son déplacement l'a
/// écrit : de quoi dire ce que chacun y peut, sans relire l'espace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtRoot {
    /// Ses lignes propres, une fois le déplacement écrit.
    pub grants: Vec<Grant>,
    /// Le rôle dans l'espace de qui déplace (le sujet de `root_grant`), lu dans la
    /// transaction : `member` au moins. `None` sans `root_grant`.
    pub role: Option<Role>,
}

/// Colonnes d'un nœud, en littéral pour que le SQL reste statique (`concat!`).
macro_rules! node_columns {
    () => {
        "id, space_id, parent_id, kind, title, position, \
         created_by, created_at, updated_by, updated_at, trashed_at"
    };
}

fn optional_uuid(r: &SqliteRow, column: &str) -> Result<Option<Uuid>, StoreError> {
    let text: Option<String> = r.get(column);
    text.as_deref().map(parse_uuid).transpose()
}

fn node_from_row(r: &SqliteRow) -> Result<Node, StoreError> {
    Ok(Node {
        id: parse_uuid(&r.get::<String, _>("id"))?,
        space_id: parse_uuid(&r.get::<String, _>("space_id"))?,
        parent_id: optional_uuid(r, "parent_id")?,
        kind: NodeKind::parse(&r.get::<String, _>("kind"))?,
        title: r.get("title"),
        position: r.get("position"),
        created_by: optional_uuid(r, "created_by")?,
        created_at: r.get("created_at"),
        updated_by: optional_uuid(r, "updated_by")?,
        updated_at: r.get("updated_at"),
        trashed_at: r.get("trashed_at"),
    })
}

/// Le nœud `id`, lu dans la transaction ; `NotFound` s'il n'existe pas.
async fn node_in(tx: &mut Transaction<'_, Sqlite>, id: &str) -> Result<Node, StoreError> {
    let row = sqlx::query(concat!(
        "SELECT ",
        node_columns!(),
        " FROM nodes WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    node_from_row(&row)
}

/// Résultat du calcul de place.
enum Slot {
    /// Nouvelle position.
    Set(String),
    /// Le nœud tient déjà la place demandée, sous le même parent : nommé après lui-même, ou
    /// déjà entre les deux frères voulus. Sa position est gardée.
    Keep,
}

/// Calcule la position d'un nœud parmi les enfants de `parent`, après `after` (`None` = en tête).
/// `exclude` est le nœud qu'on déplace : il ne compte pas parmi ses frères. `NotFound` si
/// `after` n'est pas, dans cet espace, un enfant de `parent`.
///
/// `current` est la position du nœud qu'on déplace, s'il est déjà sous `parent` : s'il est déjà
/// à la place demandée (entre les deux frères voulus), sa position est gardée.
async fn slot(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
    parent: Option<&str>,
    after: Option<Sibling>,
    exclude: Option<&str>,
    current: Option<&str>,
) -> Result<Slot, StoreError> {
    let exclude = exclude.unwrap_or("");
    let (lower, next): (Option<String>, Option<String>) = match after {
        Some(Sibling::Node(a)) => {
            let a = a.to_string();
            let pos: String = sqlx::query(
                "SELECT position FROM nodes WHERE id = ? AND space_id = ? AND parent_id IS ?",
            )
            .bind(&a)
            .bind(space_id)
            .bind(parent)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(StoreError::NotFound)?
            .get(0);
            if a == exclude {
                return Ok(Slot::Keep);
            }
            let next = sqlx::query(
                "SELECT MIN(position) FROM nodes \
                 WHERE space_id = ? AND parent_id IS ? AND position > ? AND id != ?",
            )
            .bind(space_id)
            .bind(parent)
            .bind(&pos)
            .bind(exclude)
            .fetch_one(&mut **tx)
            .await?
            .get(0);
            (Some(pos), next)
        }
        // Le dernier des autres enfants : une seule entrée de l'index `nodes_by_parent`, lue
        // par la fin.
        Some(Sibling::Last) => {
            let last = sqlx::query(
                "SELECT position FROM nodes WHERE space_id = ? AND parent_id IS ? AND id != ? \
                 ORDER BY position DESC LIMIT 1",
            )
            .bind(space_id)
            .bind(parent)
            .bind(exclude)
            .fetch_optional(&mut **tx)
            .await?
            .map(|r| r.get(0));
            (last, None)
        }
        None => {
            let first = sqlx::query(
                "SELECT MIN(position) FROM nodes WHERE space_id = ? AND parent_id IS ? AND id != ?",
            )
            .bind(space_id)
            .bind(parent)
            .bind(exclude)
            .fetch_one(&mut **tx)
            .await?
            .get(0);
            (None, first)
        }
    };
    // Les positions se comparent en ordre d'octets, comme dans SQLite.
    if let Some(current) = current
        && lower.as_deref().is_none_or(|lower| lower < current)
        && next.as_deref().is_none_or(|next| current < next)
    {
        return Ok(Slot::Keep);
    }
    Ok(Slot::Set(position_between(
        lower.as_deref(),
        next.as_deref(),
    )))
}

/// Qui passe un nœud à la racine, tel que la transaction l'a relu ([`checked_mover`]) : son
/// rôle dans l'espace, et le niveau qu'il doit avoir sur le nœud une fois celui-ci à la racine
/// (Ruling R82).
#[derive(Debug, Clone, Copy)]
struct Mover {
    user: Uuid,
    role: Role,
    level: Level,
}

/// Les lignes propres d'une page qui arrive à la racine, d'après celles qu'elle a (`own`) et
/// celles de ses anciens ancêtres (`above`), qu'elle n'héritera plus :
/// 1. Ruling R94, l'accès général gardé : la page porte la plus forte ligne `members` de son
///    ancienne chaîne (la sienne comprise, elle ne baisse donc jamais). Sans ligne `members`
///    dans la chaîne, aucune n'est posée : la page reste privée ;
/// 2. Ruling R82, au plus juste : la ligne de qui déplace (`mover`) n'est posée, ou relevée, à
///    son niveau, que si ces lignes ne le lui donnent pas déjà. Un membre que l'accès général
///    met en `full`, ou une propriétaire là où une ligne `members` s'applique, ne reçoit donc
///    pas de ligne à son nom ;
/// 3. rien d'autre ne change : les lignes nommées des anciens ancêtres ne suivent pas la page,
///    les autres lignes de la page restent.
fn root_lines(own: &[Grant], above: &[Grant], mover: Option<Mover>) -> Vec<Grant> {
    // Pose `grant` à la place de la ligne de son sujet, ou en plus.
    fn put(lines: &mut Vec<Grant>, grant: Grant) {
        match lines.iter_mut().find(|line| line.subject == grant.subject) {
            Some(line) => line.level = grant.level,
            None => lines.push(grant),
        }
    }
    let mut lines = own.to_vec();
    let general = own
        .iter()
        .chain(above)
        .filter(|line| line.subject == Subject::Members)
        .map(|line| line.level)
        .max();
    if let Some(level) = general {
        let subject = Subject::Members;
        put(&mut lines, Grant { subject, level });
    }
    if let Some(mover) = mover {
        let held = chain_level(&lines, mover.user, mover.role);
        if held.is_none_or(|held| held < mover.level) {
            let subject = Subject::User(mover.user);
            let level = mover.level;
            put(&mut lines, Grant { subject, level });
        }
    }
    lines
}

/// La décision de l'appelant, revérifiée dans la transaction du passage à la racine : ce sur
/// quoi elle repose a pu changer depuis qu'il l'a lu. `root_grant` nomme qui déplace le nœud de
/// chaîne `chain`, et le niveau qu'il y a. `Stale`, pour que rien ne soit écrit et que
/// l'appelant relise et redécide, si :
/// - son sujet n'est pas un compte (`members` relèverait l'accès général du nœud au lieu de
///   garder celui de sa chaîne) ;
/// - son niveau n'est pas `full` : c'est lui qui garantit qu'à la racine le nœud garde
///   quelqu'un pour le gérer (Rulings R70 et R82). Plus bas, le nœud pourrait n'y avoir pour
///   toute ligne que `(user:<lui>, edit)`. Ce n'est pas une course mais une faute de
///   l'appelant, refusée de la même façon : rien n'est écrit ;
/// - ce compte n'a pas de rôle dans l'espace, ou y est invité : la racine est aux membres, et
///   une ligne posée pour quelqu'un sans rôle ne donne aucun droit (le nœud pourrait rester
///   sans personne en `full`, Ruling R70) ;
/// - il a moins que ce niveau sur l'ancienne chaîne (les lignes du nœud et de ses ancêtres) :
///   le passage à la racine lui donnerait pour de bon un accès qu'il n'a plus ;
/// - le nœud, ou un de ses ancêtres, est à la corbeille.
///
/// C'est la règle des autres écritures qui reposent sur ce que l'appelant a lu : `Expected`
/// pour les rôles, Ruling R87 pour le partage, [`Place::from`] pour le parent.
async fn checked_mover(
    tx: &mut Transaction<'_, Sqlite>,
    chain: &Chain,
    root_grant: Grant,
) -> Result<Mover, StoreError> {
    let Subject::User(user) = root_grant.subject else {
        return Err(StoreError::Stale);
    };
    if root_grant.level != Level::Full {
        return Err(StoreError::Stale);
    }
    let role = role_of(tx, &chain.space, &user.to_string())
        .await?
        .filter(|role| role.is_member())
        .ok_or(StoreError::Stale)?;
    let level = root_grant.level;
    let held = chain_level(chain.lines.iter().map(|(_, line)| line), user, role);
    if chain.in_trash || held.is_none_or(|held| held < level) {
        return Err(StoreError::Stale);
    }
    Ok(Mover { user, role, level })
}

/// Ce qu'un nœud porte en arrivant à la racine, décidé dans la transaction de son déplacement.
struct Arrival {
    /// Ses lignes propres, avant.
    own: Vec<Grant>,
    /// Celles qu'il porte à la racine ([`root_lines`]).
    lines: Vec<Grant>,
    /// Le rôle dans l'espace de qui déplace (le sujet de `root_grant`).
    role: Option<Role>,
}

/// Ce que le nœud porte en arrivant à la racine, d'après sa chaîne, lue dans la transaction
/// avant qu'il change de parent. Avec `root_grant`, la décision de qui déplace y est d'abord
/// revérifiée ([`checked_mover`]) : `Stale` si elle ne tient plus.
///
/// Un nœud dont la remontée tourne en rond n'est vu par personne (R58) : `Stale` avec
/// `root_grant`, puisque qui dit le déplacer ne le voit pas non plus ; `None` sans lui, et
/// aucune ligne ne sera écrite.
async fn arrival_at_root(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
    root_grant: Option<Grant>,
) -> Result<Option<Arrival>, StoreError> {
    let Some(chain) = read_chain(tx, node_id).await? else {
        return match root_grant {
            Some(_) => Err(StoreError::Stale),
            None => Ok(None),
        };
    };
    let mover = match root_grant {
        Some(grant) => Some(checked_mover(tx, &chain, grant).await?),
        None => None,
    };
    let (mut own, mut above) = (Vec::new(), Vec::new());
    for (on, line) in chain.lines {
        if on == node_id {
            own.push(line);
        } else {
            above.push(line);
        }
    }
    let lines = root_lines(&own, &above, mover);
    let role = mover.map(|mover| mover.role);
    Ok(Some(Arrival { own, lines, role }))
}

/// `NotFound` si `parent` n'est pas un nœud de cet espace.
async fn parent_in_space(
    tx: &mut Transaction<'_, Sqlite>,
    parent: &str,
    space_id: &str,
) -> Result<(), StoreError> {
    sqlx::query("SELECT 1 FROM nodes WHERE id = ? AND space_id = ?")
        .bind(parent)
        .bind(space_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    Ok(())
}

/// Un ancêtre de `id` est-il à la corbeille ? Le nœud lui-même ne compte pas. `UNION` (et non
/// `UNION ALL`) : la remontée s'arrête même si des parents tournaient en rond.
async fn ancestor_trashed(tx: &mut Transaction<'_, Sqlite>, id: &str) -> Result<bool, StoreError> {
    let found = sqlx::query(
        "WITH RECURSIVE anc(id) AS ( \
             SELECT parent_id FROM nodes WHERE id = ?1 \
             UNION \
             SELECT n.parent_id FROM nodes n JOIN anc ON n.id = anc.id \
         ) SELECT 1 FROM nodes n JOIN anc ON n.id = anc.id \
           WHERE n.trashed_at IS NOT NULL LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(found.is_some())
}

/// Les ids du sous-arbre de `id`, lui d'abord, puis ses descendants niveau par niveau (par id
/// dans un même niveau) : chaque ligne porte sa profondeur, et le `ORDER BY` trie dessus. Vide si
/// le nœud n'existe pas. La descente suit l'index `nodes_by_parent`.
///
/// Elle s'arrête même si des parents tournaient en rond : un nœud n'a qu'un parent, la descente
/// ne peut donc repasser par un nœud qu'en revenant à `id`, que le pas récursif écarte.
async fn subtree<'e>(db: impl SqliteExecutor<'e>, id: &str) -> Result<Vec<Uuid>, StoreError> {
    sqlx::query(
        "WITH RECURSIVE sub(id, space_id, depth) AS ( \
             SELECT id, space_id, 0 FROM nodes WHERE id = ?1 \
             UNION ALL \
             SELECT n.id, n.space_id, sub.depth + 1 FROM nodes n \
             JOIN sub ON n.space_id = sub.space_id AND n.parent_id = sub.id \
             WHERE n.id != ?1 \
         ) SELECT id FROM sub ORDER BY depth, id",
    )
    .bind(id)
    .fetch_all(db)
    .await?
    .iter()
    .map(|r| parse_uuid(&r.get::<String, _>(0)))
    .collect()
}

impl Store {
    /// Crée un nœud, avec ses accès (`grants`) et son document (`doc`, instantané de `seq` 0) :
    /// tout est écrit, ou rien. `NotFound` (sans rien écrire) si l'espace, le parent, `after` ou
    /// l'auteur n'existe pas, si le parent est d'un autre espace, ou si `after` n'est pas un
    /// enfant du parent visé. `after: Some(Sibling::Last)` le range en fin de ses frères.
    pub async fn create_node(&self, n: NewNode) -> Result<Node, StoreError> {
        let space = n.space_id.to_string();
        let parent = n.parent_id.map(|p| p.to_string());
        let author = n.created_by.map(|u| u.to_string());
        let mut tx = self.write().await?;
        sqlx::query("SELECT 1 FROM spaces WHERE id = ?")
            .bind(&space)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(StoreError::NotFound)?;
        if let Some(p) = &parent {
            parent_in_space(&mut tx, p, &space).await?;
        }
        let Slot::Set(position) =
            slot(&mut tx, &space, parent.as_deref(), n.after, None, None).await?
        else {
            unreachable!("aucun nœud déplacé : le calcul de place donne toujours une position");
        };
        let id = Uuid::now_v7();
        let id_s = id.to_string();
        let now = now_ms();
        sqlx::query(
            "INSERT INTO nodes (id, space_id, parent_id, kind, title, position, \
                                created_by, created_at, updated_by, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?7, ?8)",
        )
        .bind(&id_s)
        .bind(&space)
        .bind(&parent)
        .bind(n.kind.as_str())
        .bind(&n.title)
        .bind(&position)
        .bind(&author)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(StoreError::from_fk)?;
        if let Some(doc) = &n.doc {
            sqlx::query(
                "INSERT INTO doc_snapshots (node_id, seq, state_blob, created_at) VALUES (?, 0, ?, ?)",
            )
            .bind(&id_s)
            .bind(doc.as_slice())
            .bind(now)
            .execute(&mut **tx)
            .await?;
        }
        for grant in &n.grants {
            write_grant(&mut tx, &id_s, *grant).await?;
        }
        bump_rev(&mut tx, &space).await?;
        tx.commit().await?;
        Ok(Node {
            id,
            space_id: n.space_id,
            parent_id: n.parent_id,
            kind: n.kind,
            title: n.title,
            position,
            created_by: n.created_by,
            created_at: now,
            updated_by: n.created_by,
            updated_at: now,
            trashed_at: None,
        })
    }

    pub async fn get_node(&self, id: Uuid) -> Result<Option<Node>, StoreError> {
        sqlx::query(concat!(
            "SELECT ",
            node_columns!(),
            " FROM nodes WHERE id = ?"
        ))
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .as_ref()
        .map(node_from_row)
        .transpose()
    }

    /// Tous les nœuds de l'espace, corbeille comprise, triés par `(parent_id, position)`.
    pub async fn list_nodes(&self, space_id: Uuid) -> Result<Vec<Node>, StoreError> {
        sqlx::query(concat!(
            "SELECT ",
            node_columns!(),
            " FROM nodes WHERE space_id = ? ORDER BY parent_id, position"
        ))
        .bind(space_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(node_from_row)
        .collect()
    }

    /// Renomme un nœud ; le titre est enregistré tel quel. `by` : le compte qui renomme.
    pub async fn rename_node(
        &self,
        id: Uuid,
        title: &str,
        by: Option<Uuid>,
    ) -> Result<Node, StoreError> {
        let updated = self.update_node(id, Some(title), None, by, None).await?;
        Ok(updated.node)
    }

    /// **Réservée aux tests** : n'applique aucune vérification de droits. Le déplacement n'est
    /// pas lié au parent lu ([`Place::from`], R91), et rien ne nomme qui déplace : à la racine,
    /// le nœud ne reçoit aucune ligne pour lui, et la transaction ne revérifie ni son rôle ni
    /// son niveau (`root_grant`, R82 et R105). Une route passe par [`Store::update_node`].
    ///
    /// Déplace un nœud sous `parent_id` (`None` = racine), après `after` (`None` = en tête).
    /// `Cycle` si le nouveau parent est le nœud ou un de ses descendants ; `NotFound` si le nœud,
    /// le parent ou `after` manque, ou s'ils sont d'un autre espace. Les refus n'écrivent rien.
    pub async fn move_node(
        &self,
        id: Uuid,
        parent_id: Option<Uuid>,
        after: Option<Uuid>,
        by: Option<Uuid>,
    ) -> Result<Node, StoreError> {
        let place = Place {
            parent_id,
            after: after.map(Sibling::Node),
            from: None,
        };
        let updated = self.update_node(id, None, Some(place), by, None).await?;
        Ok(updated.node)
    }

    /// Change le titre, la place, ou les deux, en une transaction : un refus (`Cycle`,
    /// `NotFound`, comme pour [`Store::move_node`] ; `Stale`, voir [`Place::from`] et, pour un
    /// passage à la racine, plus bas) n'écrit rien, pas même le titre. `by` : le compte qui fait
    /// le changement, gardé dans `updated_by`.
    ///
    /// Rien n'est écrit, et le nœud est rendu tel quel, quand il n'y a rien à changer : ni titre
    /// ni place demandés, ou une place où le nœud est déjà (même parent, entre les mêmes
    /// frères) sans titre à changer.
    ///
    /// La révision de l'espace n'est incrémentée que si le nœud change de parent : un titre et
    /// une place sous le même parent ne changent les droits de personne (règle de `rev`). Sa
    /// version avance dès que quelque chose est écrit : la barre latérale montre titres et places
    /// (règle de `lsn`).
    ///
    /// Quand le nœud arrive à la racine (il avait un parent, il n'en a plus), il n'hérite plus
    /// de rien. Dans la même transaction, d'après son ancienne chaîne (voir `root_lines`) :
    /// - Ruling R94 : il garde son accès général. La plus forte ligne `members` de l'ancienne
    ///   chaîne devient la sienne ; une page d'équipe reste donc une page de l'équipe, au même
    ///   niveau pour les membres. Sans ligne `members` dans la chaîne, il reste privé ;
    /// - Ruling R82 : `root_grant` est l'accès que doit y avoir celui qui déplace, toujours
    ///   `full`. Il n'est posé, ou relevé, que si les lignes du nœud ne le lui donnent pas déjà,
    ///   compte tenu de son rôle dans l'espace ; jamais abaissé. Le nœud a ainsi toujours
    ///   quelqu'un pour le gérer, sans ligne redondante qui resterait à quelqu'un parti de
    ///   l'espace ;
    /// - les lignes nommées des anciens ancêtres ne le suivent pas, et ses autres lignes ne
    ///   changent pas.
    ///
    /// `root_grant` porte aussi la décision de l'appelant : il a lu que ce compte pouvait
    /// passer le nœud à la racine (un membre, qui a ce niveau sur le nœud, hors corbeille). La
    /// transaction le revérifie avant tout le reste, sur l'ancienne chaîne et le rôle relu
    /// (`checked_mover`) : si ce n'est plus vrai, si le sujet de `root_grant` n'est pas un
    /// compte, ou si son niveau n'est pas `full`, rien n'est écrit (`Stale`), et l'appelant
    /// relit et redécide. Sans `root_grant`, le store ne sait pas qui déplace : il ne vérifie
    /// rien, et n'écrit de ligne pour personne.
    ///
    /// Dans tous les autres cas (autre parent, simple place à la racine, titre), aucune ligne
    /// n'est écrite, et `root_grant` est ignoré.
    ///
    /// Ce que la transaction a décidé pour un nœud arrivé à la racine est rendu avec lui
    /// ([`Updated::at_root`] : ses lignes, et le rôle qu'elle a lu pour qui déplace) :
    /// l'appelant dit de là ce que le nœud est devenu, sans rien relire.
    pub async fn update_node(
        &self,
        id: Uuid,
        title: Option<&str>,
        place: Option<Place>,
        by: Option<Uuid>,
        root_grant: Option<Grant>,
    ) -> Result<Updated, StoreError> {
        let unchanged = |node| Updated {
            node,
            at_root: None,
        };
        if title.is_none() && place.is_none() {
            let node = self.get_node(id).await?.ok_or(StoreError::NotFound)?;
            return Ok(unchanged(node));
        }
        let id_s = id.to_string();
        let mut tx = self.write().await?;
        let before = node_in(&mut tx, &id_s).await?;
        let space = before.space_id.to_string();
        // `Some((parent, position))` : le nœud change de place ; `None` : il garde la sienne.
        let mut target: Option<(Option<Uuid>, String)> = None;
        let mut arrival = None;
        if let Some(place) = place {
            if place.from.is_some_and(|from| from != before.parent_id) {
                return Err(StoreError::Stale);
            }
            // Le nœud arrive à la racine. Ce qu'il y portera se décide sur son ancienne chaîne,
            // lue avant qu'il change de parent ; et la décision de qui le déplace est revérifiée
            // avant le calcul de sa place : un `after` introuvable ne se dit qu'à qui peut
            // encore le déplacer.
            if place.parent_id.is_none() && before.parent_id.is_some() {
                arrival = arrival_at_root(&mut tx, &id_s, root_grant).await?;
            }
            let parent = place.parent_id.map(|p| p.to_string());
            if let Some(p) = &parent {
                parent_in_space(&mut tx, p, &space).await?;
                // On remonte du nouveau parent vers la racine : si on croise le nœud, c'est un
                // cycle.
                let cycle = sqlx::query(
                    "WITH RECURSIVE anc(id) AS ( \
                         SELECT ?1 \
                         UNION ALL \
                         SELECT n.parent_id FROM nodes n JOIN anc ON n.id = anc.id \
                         WHERE n.parent_id IS NOT NULL \
                     ) SELECT 1 FROM anc WHERE id = ?2 LIMIT 1",
                )
                .bind(p)
                .bind(&id_s)
                .fetch_optional(&mut **tx)
                .await?;
                if cycle.is_some() {
                    return Err(StoreError::Cycle);
                }
            }
            // Sa position ne compte que s'il reste sous le même parent.
            let current = (place.parent_id == before.parent_id).then_some(before.position.as_str());
            let slot = slot(
                &mut tx,
                &space,
                parent.as_deref(),
                place.after,
                Some(&id_s),
                current,
            )
            .await?;
            if let Slot::Set(position) = slot {
                target = Some((place.parent_id, position));
            }
        }
        if title.is_none() && target.is_none() {
            // Déjà à cette place, et pas de titre : la transaction n'a rien écrit.
            return Ok(unchanged(before));
        }
        let (moves, parent, position) = match target {
            Some((parent, position)) => (true, parent, Some(position)),
            None => (false, None, None),
        };
        let changes_parent = moves && parent != before.parent_id;
        let row = sqlx::query(concat!(
            "UPDATE nodes SET \
                 title = COALESCE(?1, title), \
                 parent_id = CASE WHEN ?2 THEN ?3 ELSE parent_id END, \
                 position = COALESCE(?4, position), \
                 updated_at = ?5, updated_by = ?6 \
             WHERE id = ?7 RETURNING ",
            node_columns!()
        ))
        .bind(title)
        .bind(moves)
        .bind(parent.map(|p| p.to_string()))
        .bind(position)
        .bind(now_ms())
        .bind(by.map(|u| u.to_string()))
        .bind(&id_s)
        .fetch_one(&mut **tx)
        .await
        .map_err(StoreError::from_fk)?;
        let node = node_from_row(&row)?;
        if changes_parent {
            // Un nœud qui arrive à la racine change de parent : sa place y est toujours neuve.
            if let Some(arrival) = &arrival {
                // Seules les lignes qui changent sont écrites.
                for line in &arrival.lines {
                    if !arrival.own.contains(line) {
                        write_grant(&mut tx, &id_s, *line).await?;
                    }
                }
            }
            bump_rev(&mut tx, &space).await?;
        } else {
            // Un titre, une place parmi les mêmes frères : ce que montre la barre latérale
            // change, pas les droits de personne (R83).
            bump_lsn(&mut tx, &space).await?;
        }
        tx.commit().await?;
        let at_root = arrival.map(|arrival| AtRoot {
            grants: arrival.lines,
            role: arrival.role,
        });
        Ok(Updated { node, at_root })
    }

    /// Met un nœud à la corbeille, pour le compte `by`. Ses descendants y sont par héritage :
    /// leurs lignes ne changent pas. Sans effet (ni date, ni révision) s'il y est déjà.
    /// `NotFound` si le nœud ou le compte n'existe pas.
    pub async fn trash_node(&self, id: Uuid, by: Uuid) -> Result<Node, StoreError> {
        let id_s = id.to_string();
        let mut tx = self.write().await?;
        let node = node_in(&mut tx, &id_s).await?;
        if node.trashed_at.is_some() {
            return Ok(node);
        }
        let row = sqlx::query(concat!(
            "UPDATE nodes SET trashed_at = ?, trashed_by = ? WHERE id = ? RETURNING ",
            node_columns!()
        ))
        .bind(now_ms())
        .bind(by.to_string())
        .bind(&id_s)
        .fetch_one(&mut **tx)
        .await
        .map_err(StoreError::from_fk)?;
        let node = node_from_row(&row)?;
        bump_rev(&mut tx, &node.space_id.to_string()).await?;
        tx.commit().await?;
        Ok(node)
    }

    /// Sort un nœud de la corbeille. `ParentTrashed` dès qu'un de ses ancêtres y est, que le
    /// nœud y soit lui-même ou non : c'est l'ancêtre qu'on restaure. Sans effet sur un nœud qui
    /// n'y est pas.
    pub async fn restore_node(&self, id: Uuid) -> Result<Node, StoreError> {
        let id_s = id.to_string();
        let mut tx = self.write().await?;
        let node = node_in(&mut tx, &id_s).await?;
        if ancestor_trashed(&mut tx, &id_s).await? {
            return Err(StoreError::ParentTrashed);
        }
        if node.trashed_at.is_none() {
            return Ok(node);
        }
        let row = sqlx::query(concat!(
            "UPDATE nodes SET trashed_at = NULL, trashed_by = NULL WHERE id = ? RETURNING ",
            node_columns!()
        ))
        .bind(&id_s)
        .fetch_one(&mut **tx)
        .await?;
        let node = node_from_row(&row)?;
        bump_rev(&mut tx, &node.space_id.to_string()).await?;
        tx.commit().await?;
        Ok(node)
    }

    /// Supprime définitivement un nœud et tout son sous-arbre, avec leurs documents, leurs
    /// accès, leurs invitations, leurs fils de commentaires, leurs versions et leurs
    /// déplacements de blocs, et rend les ids supprimés (le nœud compris). `NotTrashed` (sans
    /// rien écrire) si le nœud n'est pas à la corbeille, lui-même ou par un ancêtre.
    pub async fn delete_node(&self, id: Uuid) -> Result<Vec<Uuid>, StoreError> {
        let id_s = id.to_string();
        let mut tx = self.write().await?;
        let node = node_in(&mut tx, &id_s).await?;
        if node.trashed_at.is_none() && !ancestor_trashed(&mut tx, &id_s).await? {
            return Err(StoreError::NotTrashed);
        }
        let ids = subtree(&mut **tx, &id_s).await?;
        // Les ids en un seul paramètre, un tableau JSON : ce sont des UUID relus, sans rien à
        // échapper.
        let list = format!(
            "[{}]",
            ids.iter()
                .map(|id| format!("\"{id}\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        // Les documents d'abord (`nodes` est leur table mère) ; `acl`, `invitations`, les fils de
        // commentaires (avec leurs messages et leurs réactions), les versions et les
        // déplacements de blocs suivent les nœuds (`ON DELETE CASCADE`, migration `0003`). Les
        // nœuds partent en une instruction : la clé `parent_id` n'est vérifiée qu'à la fin,
        // quand parents et enfants sont tous partis.
        for sql in [
            "DELETE FROM doc_updates WHERE node_id IN (SELECT value FROM json_each(?))",
            "DELETE FROM doc_snapshots WHERE node_id IN (SELECT value FROM json_each(?))",
            "DELETE FROM nodes WHERE id IN (SELECT value FROM json_each(?))",
        ] {
            sqlx::query(sql).bind(&list).execute(&mut **tx).await?;
        }
        bump_rev(&mut tx, &node.space_id.to_string()).await?;
        tx.commit().await?;
        Ok(ids)
    }

    /// Depuis quand le nœud est à la corbeille, en millisecondes Unix : sa propre date, sinon
    /// celle de son plus proche ancêtre qui y est. `None` s'il n'y est pas, ou s'il n'existe pas.
    ///
    /// La remontée va de parent en parent par la clé primaire, et s'arrête au premier nœud à la
    /// corbeille ; `UNION` (et non `UNION ALL`) : elle s'arrête aussi si des parents tournaient
    /// en rond.
    pub async fn trashed_since(&self, id: Uuid) -> Result<Option<i64>, StoreError> {
        let row = sqlx::query(
            "WITH RECURSIVE anc(id, parent_id, trashed_at) AS ( \
                 SELECT id, parent_id, trashed_at FROM nodes WHERE id = ?1 \
                 UNION \
                 SELECT n.id, n.parent_id, n.trashed_at FROM nodes n \
                 JOIN anc ON n.id = anc.parent_id \
                 WHERE anc.trashed_at IS NULL \
             ) SELECT trashed_at FROM anc WHERE trashed_at IS NOT NULL LIMIT 1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get(0)))
    }

    /// Les ids du sous-arbre d'un nœud : lui d'abord, puis ses descendants niveau par niveau,
    /// corbeille comprise. `NotFound` si le nœud n'existe pas.
    pub async fn subtree_ids(&self, id: Uuid) -> Result<Vec<Uuid>, StoreError> {
        let ids = subtree(&self.pool, &id.to_string()).await?;
        if ids.is_empty() {
            return Err(StoreError::NotFound);
        }
        Ok(ids)
    }
}
