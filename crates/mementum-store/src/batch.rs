//! Des pages créées en lot avec leur contenu (lot 5, Task S7, Rulings R1243, R590) : les nœuds,
//! leurs accès et leurs documents, en une transaction, toutes ou aucune.
//!
//! Le store ne lit ni ne bâtit rien de ce qu'il écrit : le serveur lui donne les pages, leurs
//! documents déjà bâtis et vérifiés. Il décide, dans la transaction qui écrit, de ce qui ne se
//! décide qu'avec l'écriture : l'auteur peut encore créer là où vont les pages (son niveau sur la
//! page parente, hors corbeille ; ou son rôle de membre à la racine), et le jeton d'API par lequel
//! il écrit, s'il y en a un, vit encore (Ruling R1356).

use mementum_acl::{Action, Grant, chain_level};
use uuid::Uuid;

use crate::acl::{read_chain, write_grant};
use crate::members::role_of;
use crate::nodes::{Node, NodeKind, Sibling, new_slot};
use crate::queue::Lane;
use crate::search::mark_born;
use crate::spaces::bump_rev;
use crate::{Store, StoreError};

/// Des pages à créer ensemble ([`Store::create_pages`]).
#[derive(Debug, Clone)]
pub struct NewPages {
    pub space_id: Uuid,
    /// La page sous laquelle elles vont (`None` = à la racine de l'espace), en fin de ses
    /// enfants, dans l'ordre de [`NewPages::pages`].
    pub parent_id: Option<Uuid>,
    /// Les accès posés sur chacune ; aucun sous une page parente (elles en héritent).
    pub grants: Vec<Grant>,
    /// Qui crée : l'auteur de chaque page.
    pub created_by: Uuid,
    /// Le jeton d'API par lequel il crée, relu vivant dans la transaction ; `None` pour une
    /// session.
    pub token: Option<Uuid>,
    /// Les pages de la portée d'un jeton de portée « pages » (ses racines) : la page parente doit
    /// être l'une d'elles, ou sous l'une d'elles, dans la transaction ; aucune page à la racine.
    /// `None` : une session, ou un jeton de tout un espace.
    pub token_roots: Option<Vec<Uuid>>,
    pub pages: Vec<NewPageDoc>,
}

/// Une page du lot.
#[derive(Debug, Clone)]
pub struct NewPageDoc {
    /// Son identifiant, choisi par l'appelant.
    pub id: Uuid,
    /// Enregistré tel quel : les règles de titre sont celles de la route HTTP.
    pub title: String,
    /// Son document, écrit en instantané de `seq` 0.
    pub doc: Vec<u8>,
}

impl Store {
    /// Écrit les pages d'un lot, leurs accès et leurs documents (instantanés de `seq` 0), en une
    /// transaction qui avance la révision et la version de l'espace une fois. Rend les nœuds,
    /// dans l'ordre. Tout est écrit, ou rien.
    ///
    /// Dans la transaction, avant d'écrire (Ruling R590), au nom de `created_by` :
    /// - `NotFound` si l'espace n'existe pas, ou si la page parente n'en est pas une page ;
    /// - `TokenRevoked` si le jeton nommé n'est plus vivant (Ruling R1356) ;
    /// - `Stale` si l'auteur n'a plus de rôle dans l'espace, ne peut plus créer sous la page
    ///   parente (son niveau, ou la corbeille), ou n'est plus membre pour une page à la racine.
    ///
    /// Elle s'écrit dans la voie des copies (`Lane::Copies`, Rulings R607, R1243) : comme une
    /// duplication, des documents bâtis hors de tout acteur, écrits d'un coup ; entre deux lots,
    /// la file sert une écriture de chaque autre voie qui attend.
    pub async fn create_pages(&self, new: &NewPages, now: i64) -> Result<Vec<Node>, StoreError> {
        if new.pages.is_empty() {
            return Err(StoreError::Sqlx(sqlx::Error::Protocol(
                "lot de pages vide".to_string(),
            )));
        }
        let space = new.space_id.to_string();
        let author = new.created_by.to_string();
        let parent = new.parent_id.map(|p| p.to_string());
        let mut tx = self.write_in(Lane::Copies).await?;
        #[cfg(feature = "test-hooks")]
        if self.copies_to_refuse.take_one() {
            return Err(StoreError::NotFound);
        }
        sqlx::query("SELECT 1 FROM spaces WHERE id = ?")
            .bind(&space)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(StoreError::NotFound)?;
        if let Some(token) = new.token {
            let live = sqlx::query(concat!(
                "SELECT 1 FROM api_tokens WHERE id = ?1 AND ",
                crate::tokens::live_token!()
            ))
            .bind(token.to_string())
            .bind(now)
            .fetch_optional(&mut **tx)
            .await?
            .is_some();
            if !live {
                return Err(StoreError::TokenRevoked);
            }
        }
        // La portée d'un jeton de pages (Rulings R1219, R1574) : la page parente sous une de ses
        // racines, lue par les parents de l'arbre (une page déplacée hors de la portée pendant
        // l'attente du tour d'écriture est refusée ; la remontée des parents visibles est celle de
        // la route, juste avant).
        if let Some(roots) = &new.token_roots {
            let Some(parent) = &parent else {
                return Err(StoreError::Stale);
            };
            let chain: Vec<String> = sqlx::query_scalar(
                "WITH RECURSIVE up(id, parent, depth) AS ( \
                     SELECT id, parent_id, 0 FROM nodes WHERE id = ?1 \
                     UNION ALL \
                     SELECT n.id, n.parent_id, up.depth + 1 FROM nodes n \
                     JOIN up ON n.id = up.parent WHERE up.depth < 10000 \
                 ) SELECT id FROM up",
            )
            .bind(parent)
            .fetch_all(&mut **tx)
            .await?;
            let roots: Vec<String> = roots.iter().map(Uuid::to_string).collect();
            if !chain.iter().any(|id| roots.contains(id)) {
                return Err(StoreError::Stale);
            }
        }
        let role = role_of(&mut tx, &space, &author)
            .await?
            .ok_or(StoreError::Stale)?;
        match &parent {
            Some(parent) => {
                let above = read_chain(&mut tx, parent)
                    .await?
                    .filter(|chain| chain.space == space)
                    .ok_or(StoreError::NotFound)?;
                let may_create = chain_level(
                    above.lines.iter().map(|(_, line)| line),
                    new.created_by,
                    role,
                )
                .is_some_and(|level| level.allows(Action::CreateChild));
                if !may_create || above.in_trash {
                    return Err(StoreError::Stale);
                }
            }
            None if !role.is_member() => return Err(StoreError::Stale),
            None => {}
        }
        let mut nodes = Vec::with_capacity(new.pages.len());
        for page in &new.pages {
            let id = page.id.to_string();
            let position =
                new_slot(&mut tx, &space, parent.as_deref(), Some(Sibling::Last)).await?;
            sqlx::query(
                "INSERT INTO nodes (id, space_id, parent_id, kind, title, position, \
                                    created_by, created_at, updated_by, updated_at) \
                 VALUES (?1, ?2, ?3, 'page', ?4, ?5, ?6, ?7, ?6, ?7)",
            )
            .bind(&id)
            .bind(&space)
            .bind(&parent)
            .bind(&page.title)
            .bind(&position)
            .bind(&author)
            .bind(now)
            .execute(&mut **tx)
            .await
            .map_err(StoreError::from_fk)?;
            sqlx::query(
                "INSERT INTO doc_snapshots (node_id, seq, state_blob, created_at) \
                 VALUES (?, 0, ?, ?)",
            )
            .bind(&id)
            .bind(page.doc.as_slice())
            .bind(now)
            .execute(&mut **tx)
            .await?;
            // Ruling R918 : chaque page du lot naît jamais indexée, dans cette transaction.
            mark_born(&mut tx, &id).await?;
            for grant in &new.grants {
                write_grant(&mut tx, &id, *grant).await?;
            }
            nodes.push(Node {
                id: page.id,
                space_id: new.space_id,
                parent_id: new.parent_id,
                kind: NodeKind::Page,
                title: page.title.clone(),
                position,
                created_by: Some(new.created_by),
                created_at: now,
                updated_by: Some(new.created_by),
                updated_at: now,
                trashed_at: None,
            });
        }
        bump_rev(&mut tx, &space).await?;
        tx.commit().await?;
        Ok(nodes)
    }
}
