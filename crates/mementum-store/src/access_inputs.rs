//! Ce que le calcul des accès lit d'un espace, pris en une seule transaction de lecture.

use mementum_acl::{Grant, Level, Role, Subject, TreeNode};
use sqlx::Row;
use uuid::Uuid;

use crate::members::role_from;
use crate::{Store, StoreError, parse_uuid, unreadable};

/// Tout ce que le calcul des accès lit d'un espace, pris dans une seule transaction de lecture :
/// les quatre valeurs sont celles du même instant.
///
/// Rien ici ne change sans que la révision change (règle de `rev`, en tête de la crate) : ni
/// titre, ni position. Ces données valent donc pour toute lecture qui trouve l'espace à la
/// révision `rev`, et pour elle seule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessInputs {
    /// La révision de l'espace à l'instant de la lecture.
    pub rev: i64,
    /// Tous les nœuds de l'espace, corbeille comprise, sans ordre.
    pub nodes: Vec<TreeNode>,
    /// Toutes les lignes d'accès posées dans l'espace, corbeille comprise : `(nœud, accès)`,
    /// sans ordre.
    pub grants: Vec<(Uuid, Grant)>,
    /// Tous les comptes qui ont un rôle dans l'espace, invités compris, sans ordre.
    pub roles: Vec<(Uuid, Role)>,
}

impl AccessInputs {
    /// Le rôle du compte dans l'espace ; `None` s'il n'en a pas.
    pub fn role_of(&self, user: Uuid) -> Option<Role> {
        self.roles
            .iter()
            .find(|(id, _)| *id == user)
            .map(|(_, role)| *role)
    }
}

impl Store {
    /// Ce que le calcul des accès lit de l'espace. `None` : l'espace n'existe pas.
    ///
    /// Tout est lu dans une seule transaction, sur une seule connexion. Elle s'ouvre par un
    /// `BEGIN` différé : c'est sa première lecture, celle de la révision, qui fixe l'instantané
    /// de la base (mode WAL), et les trois suivantes le voient tel quel, quoi qu'écrivent les
    /// autres connexions entre-temps. La révision rendue est donc celle des nœuds, des lignes
    /// d'accès et des rôles rendus avec elle. La transaction ne prend pas le verrou d'écriture :
    /// elle n'attend aucune écriture, et aucune ne l'attend.
    pub async fn access_inputs(&self, space_id: Uuid) -> Result<Option<AccessInputs>, StoreError> {
        let space = space_id.to_string();
        let mut tx = self.pool.begin().await?;
        let Some(rev) = sqlx::query("SELECT rev FROM spaces WHERE id = ?")
            .bind(&space)
            .fetch_optional(&mut *tx)
            .await?
            .map(|r| r.get(0))
        else {
            return Ok(None);
        };
        let mut nodes = sqlx::query(
            "SELECT id, parent_id, trashed_at IS NOT NULL FROM nodes WHERE space_id = ?",
        )
        .bind(&space)
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|r| {
            Ok(TreeNode {
                id: parse_uuid(r.get(0))?,
                parent_id: r.get::<Option<&str>, _>(1).map(parse_uuid).transpose()?,
                trashed: r.get(2),
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
        let mut grants = sqlx::query(
            "SELECT a.node_id, a.subject, a.level FROM acl a \
             JOIN nodes n ON n.id = a.node_id WHERE n.space_id = ?",
        )
        .bind(&space)
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|r| {
            let (subject, level): (&str, &str) = (r.get(1), r.get(2));
            let grant = Grant {
                subject: Subject::parse(subject)
                    .ok_or_else(|| unreadable("sujet d'accès", subject))?,
                level: Level::parse(level).ok_or_else(|| unreadable("niveau d'accès", level))?,
            };
            Ok((parse_uuid(r.get(0))?, grant))
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
        let mut roles = sqlx::query("SELECT user_id, role FROM members WHERE space_id = ?")
            .bind(&space)
            .fetch_all(&mut *tx)
            .await?
            .iter()
            .map(|r| Ok((parse_uuid(r.get(0))?, role_from(r.get(1))?)))
            .collect::<Result<Vec<_>, StoreError>>()?;
        tx.commit().await?;
        // Gardées en cache par le serveur : pas de place réservée en trop.
        nodes.shrink_to_fit();
        grants.shrink_to_fit();
        roles.shrink_to_fit();
        Ok(Some(AccessInputs {
            rev,
            nodes,
            grants,
            roles,
        }))
    }
}
