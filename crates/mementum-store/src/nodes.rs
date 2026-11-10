//! Arbre de pages d'un espace : création, lecture, renommage, déplacement, corbeille,
//! suppression définitive.
//!
//! Toute écriture d'ici incrémente la révision de l'espace dans sa transaction (règle de `rev`,
//! en tête de la crate).

use mementum_acl::Grant;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqliteExecutor, Transaction};
use uuid::Uuid;

use crate::acl::write_grant;
use crate::spaces::bump_rev;
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
    pub after: Option<Uuid>,
    pub kind: NodeKind,
    /// Enregistré tel quel : les règles de titre sont celles de la route HTTP.
    pub title: String,
    pub created_by: Option<Uuid>,
    /// Accès posés sur le nœud, écrits avec lui. Si un sujet revient, son dernier niveau reste.
    pub grants: Vec<Grant>,
    /// Document de la page : écrit avec le nœud, en instantané de `seq` 0.
    pub doc: Option<Vec<u8>>,
}

/// Où mettre un nœud : sous `parent_id` (`None` = racine), après `after` (`None` = en tête).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub parent_id: Option<Uuid>,
    pub after: Option<Uuid>,
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
    /// Le nœud est déjà là où on le veut (déplacé après lui-même).
    Keep,
}

/// Calcule la position d'un nœud parmi les enfants de `parent`, après `after` (`None` = en tête).
/// `exclude` est le nœud qu'on déplace : il ne compte pas parmi ses frères. `NotFound` si
/// `after` n'est pas, dans cet espace, un enfant de `parent`.
async fn slot(
    tx: &mut Transaction<'_, Sqlite>,
    space_id: &str,
    parent: Option<&str>,
    after: Option<Uuid>,
    exclude: Option<&str>,
) -> Result<Slot, StoreError> {
    let exclude = exclude.unwrap_or("");
    let (lower, next): (Option<String>, Option<String>) = match after {
        Some(a) => {
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
    Ok(Slot::Set(position_between(
        lower.as_deref(),
        next.as_deref(),
    )))
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
    /// enfant du parent visé.
    pub async fn create_node(&self, n: NewNode) -> Result<Node, StoreError> {
        let space = n.space_id.to_string();
        let parent = n.parent_id.map(|p| p.to_string());
        let author = n.created_by.map(|u| u.to_string());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("SELECT 1 FROM spaces WHERE id = ?")
            .bind(&space)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::NotFound)?;
        if let Some(p) = &parent {
            parent_in_space(&mut tx, p, &space).await?;
        }
        let Slot::Set(position) = slot(&mut tx, &space, parent.as_deref(), n.after, None).await?
        else {
            unreachable!("aucun nœud exclu : le calcul de place donne toujours une position");
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
        .execute(&mut *tx)
        .await
        .map_err(StoreError::from_fk)?;
        if let Some(doc) = &n.doc {
            sqlx::query(
                "INSERT INTO doc_snapshots (node_id, seq, state_blob, created_at) VALUES (?, 0, ?, ?)",
            )
            .bind(&id_s)
            .bind(doc.as_slice())
            .bind(now)
            .execute(&mut *tx)
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
        self.update_node(id, Some(title), None, by, None).await
    }

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
        self.update_node(id, None, Some(Place { parent_id, after }), by, None)
            .await
    }

    /// Change le titre, la place, ou les deux, en une transaction : un refus (`Cycle`,
    /// `NotFound`, comme pour [`Store::move_node`]) n'écrit rien, pas même le titre. `by` : le
    /// compte qui fait le changement, gardé dans `updated_by`. Sans titre ni place, rien n'est
    /// écrit et le nœud est rendu tel quel.
    ///
    /// `root_grant` (Ruling R66) : l'accès posé sur le nœud, dans la même transaction, quand il
    /// arrive à la racine (il avait un parent, il n'en a plus) sans aucune ligne d'accès propre.
    /// Un nœud qui ne tenait ses accès que de ses ancêtres n'est ainsi jamais, entre deux
    /// écritures, visible de personne. Dans tous les autres cas, `root_grant` est ignoré.
    pub async fn update_node(
        &self,
        id: Uuid,
        title: Option<&str>,
        place: Option<Place>,
        by: Option<Uuid>,
        root_grant: Option<Grant>,
    ) -> Result<Node, StoreError> {
        if title.is_none() && place.is_none() {
            return self.get_node(id).await?.ok_or(StoreError::NotFound);
        }
        let id_s = id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT space_id, parent_id FROM nodes WHERE id = ?")
            .bind(&id_s)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::NotFound)?;
        let space: String = row.get(0);
        let had_parent = row.get::<Option<String>, _>(1).is_some();
        // `Some((parent, position))` : le nœud change de place ; `None` : il garde la sienne.
        let mut target: Option<(Option<String>, String)> = None;
        if let Some(place) = place {
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
                .fetch_optional(&mut *tx)
                .await?;
                if cycle.is_some() {
                    return Err(StoreError::Cycle);
                }
            }
            if let Slot::Set(position) =
                slot(&mut tx, &space, parent.as_deref(), place.after, Some(&id_s)).await?
            {
                target = Some((parent, position));
            }
        }
        let (moves, parent, position) = match target {
            Some((parent, position)) => (true, parent, Some(position)),
            None => (false, None, None),
        };
        let reaches_root = moves && had_parent && parent.is_none();
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
        .bind(parent)
        .bind(position)
        .bind(now_ms())
        .bind(by.map(|u| u.to_string()))
        .bind(&id_s)
        .fetch_one(&mut *tx)
        .await
        .map_err(StoreError::from_fk)?;
        let node = node_from_row(&row)?;
        if let Some(grant) = root_grant.filter(|_| reaches_root) {
            let bare = sqlx::query("SELECT 1 FROM acl WHERE node_id = ? LIMIT 1")
                .bind(&id_s)
                .fetch_optional(&mut *tx)
                .await?
                .is_none();
            if bare {
                write_grant(&mut tx, &id_s, grant).await?;
            }
        }
        bump_rev(&mut tx, &space).await?;
        tx.commit().await?;
        Ok(node)
    }

    /// Met un nœud à la corbeille, pour le compte `by`. Ses descendants y sont par héritage :
    /// leurs lignes ne changent pas. Sans effet (ni date, ni révision) s'il y est déjà.
    /// `NotFound` si le nœud ou le compte n'existe pas.
    pub async fn trash_node(&self, id: Uuid, by: Uuid) -> Result<Node, StoreError> {
        let id_s = id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
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
        .fetch_one(&mut *tx)
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
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
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
        .fetch_one(&mut *tx)
        .await?;
        let node = node_from_row(&row)?;
        bump_rev(&mut tx, &node.space_id.to_string()).await?;
        tx.commit().await?;
        Ok(node)
    }

    /// Supprime définitivement un nœud et tout son sous-arbre, avec leurs documents, leurs
    /// accès et leurs invitations, et rend les ids supprimés (le nœud compris). `NotTrashed`
    /// (sans rien écrire) si le nœud n'est pas à la corbeille, lui-même ou par un ancêtre.
    pub async fn delete_node(&self, id: Uuid) -> Result<Vec<Uuid>, StoreError> {
        let id_s = id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let node = node_in(&mut tx, &id_s).await?;
        if node.trashed_at.is_none() && !ancestor_trashed(&mut tx, &id_s).await? {
            return Err(StoreError::NotTrashed);
        }
        let ids = subtree(&mut *tx, &id_s).await?;
        // Les ids en un seul paramètre, un tableau JSON : ce sont des UUID relus, sans rien à
        // échapper.
        let list = format!(
            "[{}]",
            ids.iter()
                .map(|id| format!("\"{id}\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        // Les documents d'abord (`nodes` est leur table mère) ; `acl` et `invitations` suivent
        // les nœuds (`ON DELETE CASCADE`). Les nœuds partent en une instruction : la clé
        // `parent_id` n'est vérifiée qu'à la fin, quand parents et enfants sont tous partis.
        for sql in [
            "DELETE FROM doc_updates WHERE node_id IN (SELECT value FROM json_each(?))",
            "DELETE FROM doc_snapshots WHERE node_id IN (SELECT value FROM json_each(?))",
            "DELETE FROM nodes WHERE id IN (SELECT value FROM json_each(?))",
        ] {
            sqlx::query(sql).bind(&list).execute(&mut *tx).await?;
        }
        bump_rev(&mut tx, &node.space_id.to_string()).await?;
        tx.commit().await?;
        Ok(ids)
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
