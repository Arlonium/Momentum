//! Arbre de pages : organisation et coffre par défaut, création, lecture, renommage, déplacement.

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{Store, StoreError, now_ms, parse_uuid, position_between};

/// Organisation et coffre où vivent les pages (raccourci du lot 1, remplacé au lot 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    pub org_id: Uuid,
    pub vault_id: Uuid,
}

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
            other => Err(StoreError::Sqlx(sqlx::Error::Decode(
                format!("genre de nœud inconnu : {other}").into(),
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: Uuid,
    pub vault_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: NodeKind,
    pub title: String,
    /// Index fractionnaire, à comparer en ordre d'octets parmi les frères.
    pub position: String,
    /// Millisecondes Unix.
    pub created_at: i64,
    /// Millisecondes Unix.
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewNode {
    pub vault_id: Uuid,
    pub parent_id: Option<Uuid>,
    /// Frère après lequel placer le nœud ; `None` = en tête de ses frères.
    pub after: Option<Uuid>,
    pub kind: NodeKind,
    /// Enregistré tel quel : les règles de titre sont celles de la route HTTP.
    pub title: String,
}

/// Colonnes d'un nœud, en littéral pour que le SQL reste statique (`concat!`).
macro_rules! node_columns {
    () => {
        "id, vault_id, parent_id, kind, title, position, created_at, updated_at"
    };
}

fn node_from_row(r: &SqliteRow) -> Result<Node, StoreError> {
    let parent: Option<String> = r.get("parent_id");
    Ok(Node {
        id: parse_uuid(&r.get::<String, _>("id"))?,
        vault_id: parse_uuid(&r.get::<String, _>("vault_id"))?,
        parent_id: parent.as_deref().map(parse_uuid).transpose()?,
        kind: NodeKind::parse(&r.get::<String, _>("kind"))?,
        title: r.get("title"),
        position: r.get("position"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    })
}

/// Résultat du calcul de place.
enum Slot {
    /// Nouvelle position.
    Set(String),
    /// Le nœud est déjà là où on le veut (déplacé après lui-même).
    Keep,
}

/// Calcule la position d'un nœud parmi les enfants de `parent`, après `after` (`None` = en tête).
/// `exclude` est le nœud qu'on déplace : il ne compte pas parmi ses frères.
async fn slot(
    tx: &mut Transaction<'_, Sqlite>,
    vault_id: &str,
    parent: Option<&str>,
    after: Option<Uuid>,
    exclude: Option<&str>,
) -> Result<Slot, StoreError> {
    let exclude = exclude.unwrap_or("");
    let (lower, next): (Option<String>, Option<String>) = match after {
        Some(a) => {
            let a = a.to_string();
            let pos: String = sqlx::query(
                "SELECT position FROM nodes WHERE id = ? AND vault_id = ? AND parent_id IS ?",
            )
            .bind(&a)
            .bind(vault_id)
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
                 WHERE vault_id = ? AND parent_id IS ? AND position > ? AND id != ?",
            )
            .bind(vault_id)
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
                "SELECT MIN(position) FROM nodes WHERE vault_id = ? AND parent_id IS ? AND id != ?",
            )
            .bind(vault_id)
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

impl Store {
    /// Renvoie l'organisation `default` et son coffre « Mon coffre », en les créant à la première
    /// demande.
    pub async fn default_space(&self) -> Result<Space, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let org: String = match sqlx::query("SELECT id FROM orgs WHERE slug = 'default'")
            .fetch_optional(&mut *tx)
            .await?
        {
            Some(r) => r.get(0),
            None => {
                let id = Uuid::now_v7().to_string();
                sqlx::query("INSERT INTO orgs (id, slug, name, created_at) VALUES (?, 'default', 'default', ?)")
                    .bind(&id)
                    .bind(now_ms())
                    .execute(&mut *tx)
                    .await?;
                id
            }
        };
        let vault: String = match sqlx::query(
            "SELECT id FROM vaults WHERE org_id = ? AND kind = 'personal' ORDER BY created_at, id LIMIT 1",
        )
        .bind(&org)
        .fetch_optional(&mut *tx)
        .await?
        {
            Some(r) => r.get(0),
            None => {
                let id = Uuid::now_v7().to_string();
                sqlx::query(
                    "INSERT INTO vaults (id, org_id, name, kind, created_at) \
                     VALUES (?, ?, 'Mon coffre', 'personal', ?)",
                )
                .bind(&id)
                .bind(&org)
                .bind(now_ms())
                .execute(&mut *tx)
                .await?;
                id
            }
        };
        tx.commit().await?;
        Ok(Space {
            org_id: parse_uuid(&org)?,
            vault_id: parse_uuid(&vault)?,
        })
    }

    /// Crée un nœud. `NotFound` (sans rien écrire) si le coffre, le parent ou `after` n'existe
    /// pas, ou si `after` n'est pas un enfant du parent visé.
    pub async fn create_node(&self, n: NewNode) -> Result<Node, StoreError> {
        let vault = n.vault_id.to_string();
        let parent = n.parent_id.map(|p| p.to_string());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let org: String = sqlx::query("SELECT org_id FROM vaults WHERE id = ?")
            .bind(&vault)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::NotFound)?
            .get(0);
        if let Some(p) = &parent {
            sqlx::query("SELECT 1 FROM nodes WHERE id = ? AND vault_id = ?")
                .bind(p)
                .bind(&vault)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(StoreError::NotFound)?;
        }
        let Slot::Set(position) = slot(&mut tx, &vault, parent.as_deref(), n.after, None).await?
        else {
            unreachable!("aucun nœud exclu : le calcul de place donne toujours une position");
        };
        let id = Uuid::now_v7();
        let now = now_ms();
        sqlx::query(
            "INSERT INTO nodes (id, org_id, vault_id, parent_id, kind, title, position, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        )
        .bind(id.to_string())
        .bind(&org)
        .bind(&vault)
        .bind(&parent)
        .bind(n.kind.as_str())
        .bind(&n.title)
        .bind(&position)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Node {
            id,
            vault_id: n.vault_id,
            parent_id: n.parent_id,
            kind: n.kind,
            title: n.title,
            position,
            created_at: now,
            updated_at: now,
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

    /// Tous les nœuds du coffre, triés par `(parent_id, position)`.
    pub async fn list_nodes(&self, vault_id: Uuid) -> Result<Vec<Node>, StoreError> {
        sqlx::query(concat!(
            "SELECT ",
            node_columns!(),
            " FROM nodes WHERE vault_id = ? ORDER BY parent_id, position"
        ))
        .bind(vault_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(node_from_row)
        .collect()
    }

    /// Renomme un nœud ; le titre est enregistré tel quel.
    pub async fn rename_node(&self, id: Uuid, title: &str) -> Result<Node, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query(concat!(
            "UPDATE nodes SET title = ?, updated_at = ? WHERE id = ? RETURNING ",
            node_columns!()
        ))
        .bind(title)
        .bind(now_ms())
        .bind(id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
        let node = node_from_row(&row)?;
        tx.commit().await?;
        Ok(node)
    }

    /// Déplace un nœud sous `parent_id` (`None` = racine), après `after` (`None` = en tête).
    /// `Cycle` si le nouveau parent est le nœud ou un de ses descendants ; `NotFound` si le nœud,
    /// le parent ou `after` manque. Les refus n'écrivent rien (tout se passe dans une transaction).
    pub async fn move_node(
        &self,
        id: Uuid,
        parent_id: Option<Uuid>,
        after: Option<Uuid>,
    ) -> Result<Node, StoreError> {
        let id_s = id.to_string();
        let parent = parent_id.map(|p| p.to_string());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let vault: String = sqlx::query("SELECT vault_id FROM nodes WHERE id = ?")
            .bind(&id_s)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::NotFound)?
            .get(0);
        if let Some(p) = &parent {
            sqlx::query("SELECT 1 FROM nodes WHERE id = ? AND vault_id = ?")
                .bind(p)
                .bind(&vault)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(StoreError::NotFound)?;
            // On remonte du nouveau parent vers la racine : si on croise le nœud, c'est un cycle.
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
        let slot = slot(&mut tx, &vault, parent.as_deref(), after, Some(&id_s)).await?;
        let now = now_ms();
        let row =
            match slot {
                Slot::Set(position) => sqlx::query(concat!(
                    "UPDATE nodes SET parent_id = ?, position = ?, updated_at = ? WHERE id = ? ",
                    "RETURNING ",
                    node_columns!()
                ))
                .bind(&parent)
                .bind(position)
                .bind(now)
                .bind(&id_s)
                .fetch_one(&mut *tx)
                .await?,
                Slot::Keep => {
                    sqlx::query(concat!(
                        "UPDATE nodes SET updated_at = ? WHERE id = ? RETURNING ",
                        node_columns!()
                    ))
                    .bind(now)
                    .bind(&id_s)
                    .fetch_one(&mut *tx)
                    .await?
                }
            };
        let node = node_from_row(&row)?;
        tx.commit().await?;
        Ok(node)
    }
}
