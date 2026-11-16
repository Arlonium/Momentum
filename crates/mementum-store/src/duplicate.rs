//! La copie d'un sous-arbre de pages (Ruling R252) : les nœuds, les accès de la racine, les
//! documents et les attaches, en une transaction.
//!
//! Le store ne lit ni ne bâtit rien de ce qu'il écrit : le serveur lui donne les pages de la
//! copie, déjà choisies d'après les droits de qui copie, leurs documents déjà bâtis, et les
//! attaches à poser. Il décide, dans la transaction qui écrit, de ce qui ne se décide qu'avec
//! l'écriture (Ruling R590) : la page copiée est encore lue par qui copie, hors corbeille ; il
//! peut encore créer là où va la copie.
//!
//! **Jamais de ligne `files`** (Ruling R579). Les attaches de l'original ont été lues avant la
//! transaction, hors du verrou du dépôt : une attache de la copie nomme un contenu par son hash,
//! et c'est la clé étrangère `attachments.file_hash` qui refuse celui dont la ligne est partie
//! entre-temps (`NotFound`, rien n'est écrit). Recréer la ligne ferait une attache sans contenu,
//! le ménage ayant pu retirer le fichier du disque.

use mementum_acl::{Action, Grant, chain_level};
use uuid::Uuid;

use crate::acl::{read_chain, write_grant};
use crate::files::insert_attachment;
use crate::members::role_of;
use crate::nodes::{Node, NodeKind, Sibling, new_slot};
use crate::queue::Lane;
use crate::spaces::bump_rev;
use crate::{Store, StoreError, position_between};

/// Une copie à écrire ([`Store::duplicate_pages`]).
#[derive(Debug, Clone)]
pub struct NewCopy {
    pub space_id: Uuid,
    /// La page copiée : relue dans la transaction, au nom de `created_by` (Ruling R590).
    pub source: Uuid,
    /// Où va la racine de la copie : sous cette page (`None` = à la racine de l'espace).
    pub parent_id: Option<Uuid>,
    /// Le frère après lequel la ranger ; `None` = en tête.
    pub after: Option<Sibling>,
    /// Les accès posés sur la racine de la copie ; ses descendantes n'en reçoivent aucun.
    pub grants: Vec<Grant>,
    /// Qui copie : l'auteur de chaque page et de chaque attache.
    pub created_by: Uuid,
    /// `pages[0]` est la racine de la copie ; chaque autre page vient après sa page parente.
    pub pages: Vec<CopiedPage>,
}

/// Une page de la copie.
#[derive(Debug, Clone)]
pub struct CopiedPage {
    /// L'identifiant de la page neuve, choisi par l'appelant (les adresses de ses fichiers le
    /// nomment déjà dans son document).
    pub id: Uuid,
    /// Le rang, dans la liste, de sa page parente ; `None` pour la racine, et elle seule.
    pub parent: Option<usize>,
    /// Enregistré tel quel : les règles de titre sont celles de la route HTTP.
    pub title: String,
    /// Sa position parmi ses sœurs : celle de l'original pour une descendante (sans elle, en
    /// tête) ; sans effet pour la racine, placée par [`NewCopy::after`].
    pub position: Option<String>,
    /// Son document, écrit en instantané de `seq` 0.
    pub doc: Vec<u8>,
    pub attachments: Vec<CopiedAttachment>,
}

/// Une attache de la copie : un contenu que la base connaît déjà, sous un nom d'affichage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopiedAttachment {
    pub id: Uuid,
    pub hash: [u8; 32],
    pub name: String,
}

/// Une liste de pages que l'appelant a mal bâtie : une erreur, pas un refus.
fn malformed(why: &str) -> StoreError {
    StoreError::Sqlx(sqlx::Error::Protocol(format!(
        "copie de pages mal formée : {why}"
    )))
}

/// La forme de `pages` : une racine en tête, et elle seule ; chaque autre page après sa page
/// parente.
fn check_shape(pages: &[CopiedPage]) -> Result<(), StoreError> {
    let Some(root) = pages.first() else {
        return Err(malformed("aucune page"));
    };
    if root.parent.is_some() {
        return Err(malformed("la première page n'est pas une racine"));
    }
    for (i, page) in pages.iter().enumerate().skip(1) {
        match page.parent {
            Some(parent) if parent < i => {}
            Some(_) => return Err(malformed("une page vient avant sa page parente")),
            None => return Err(malformed("une seconde racine")),
        }
    }
    Ok(())
}

/// Les copies que le store doit refuser, pour les tests ([`Store::refuse_next_copies`]).
#[cfg(feature = "test-hooks")]
#[derive(Debug, Default)]
pub(crate) struct CopiesToRefuse(std::sync::Mutex<usize>);

#[cfg(feature = "test-hooks")]
impl CopiesToRefuse {
    fn left(&self) -> std::sync::MutexGuard<'_, usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Vrai, et une de moins à refuser, s'il en reste.
    fn take_one(&self) -> bool {
        let mut left = self.left();
        let any = *left > 0;
        *left = left.saturating_sub(1);
        any
    }
}

impl Store {
    /// Fait refuser les `count` prochaines copies (`NotFound`, à leur tour d'écriture, sans rien
    /// écrire), comme si ce qu'elles visent venait de disparaître. Pour les tests seulement
    /// (fonction `test-hooks`) : la reprise d'une route ne se prouve pas autrement sans course.
    #[cfg(feature = "test-hooks")]
    pub fn refuse_next_copies(&self, count: usize) {
        *self.copies_to_refuse.left() = count;
    }

    /// Écrit la copie d'un sous-arbre de pages : les nœuds, les accès de la racine, les documents
    /// (instantanés de `seq` 0) et les attaches, en une transaction, qui avance la révision et la
    /// version de l'espace une fois. Rend la racine de la copie. Tout est écrit, ou rien.
    ///
    /// Dans la transaction, avant d'écrire (Ruling R590), au nom de `created_by` :
    /// - `NotFound` si l'espace n'existe pas, si la page copiée ou la page parente visée n'en
    ///   est pas une page, si `after` n'est pas un frère à l'endroit visé (la page copiée a
    ///   changé de parent), ou si une attache nomme un contenu sans ligne `files` ;
    /// - `Stale` si ce sur quoi l'appelant a décidé ne tient plus : il n'a plus de rôle dans
    ///   l'espace, ne lit plus la page copiée, elle est à la corbeille (elle-même ou par une page
    ///   parente), il ne peut plus créer sous la page parente visée (son niveau, ou la
    ///   corbeille), ou n'est plus membre pour une copie à la racine. L'appelant relit et
    ///   redécide.
    ///
    /// Seule la page copiée est relue, pas ses sous-pages : ce qu'elles valaient quand l'appelant
    /// les a lues est écrit. Une liste de pages mal formée est une erreur.
    ///
    /// Elle s'écrit dans la voie des copies (`Lane::Copies`, Ruling R607), commune à tous les
    /// comptes : entre deux copies, la file sert une écriture de chaque autre voie qui attend,
    /// si bien qu'une frappe n'attend jamais plus d'une copie. Jamais dans la voie du dépôt,
    /// qu'elle ne tient pas (Ruling R522).
    pub async fn duplicate_pages(&self, copy: &NewCopy, now: i64) -> Result<Node, StoreError> {
        check_shape(&copy.pages)?;
        let space = copy.space_id.to_string();
        let author = copy.created_by.to_string();
        let parent = copy.parent_id.map(|p| p.to_string());
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

        // Ruling R590 : la décision de l'appelant, relue ici.
        let source = read_chain(&mut tx, &copy.source.to_string())
            .await?
            .filter(|chain| chain.space == space)
            .ok_or(StoreError::NotFound)?;
        let role = role_of(&mut tx, &space, &author)
            .await?
            .ok_or(StoreError::Stale)?;
        let level_on = |chain: &crate::acl::Chain| {
            chain_level(
                chain.lines.iter().map(|(_, line)| line),
                copy.created_by,
                role,
            )
        };
        if level_on(&source).is_none() || source.in_trash {
            return Err(StoreError::Stale);
        }
        match &parent {
            Some(parent) => {
                let above = read_chain(&mut tx, parent)
                    .await?
                    .filter(|chain| chain.space == space)
                    .ok_or(StoreError::NotFound)?;
                let may_create =
                    level_on(&above).is_some_and(|level| level.allows(Action::CreateChild));
                if !may_create || above.in_trash {
                    return Err(StoreError::Stale);
                }
            }
            None if !role.is_member() => return Err(StoreError::Stale),
            None => {}
        }

        let root_position = new_slot(&mut tx, &space, parent.as_deref(), copy.after).await?;
        for (i, page) in copy.pages.iter().enumerate() {
            let id = page.id.to_string();
            let (parent, position) = match page.parent {
                None => (parent.clone(), root_position.clone()),
                Some(above) => (
                    Some(copy.pages[above].id.to_string()),
                    page.position
                        .clone()
                        .unwrap_or_else(|| position_between(None, None)),
                ),
            };
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
            if i == 0 {
                for grant in &copy.grants {
                    write_grant(&mut tx, &id, *grant).await?;
                }
            }
            for attachment in &page.attachments {
                insert_attachment(
                    &mut tx,
                    attachment.id,
                    page.id,
                    &attachment.hash,
                    &attachment.name,
                    copy.created_by,
                    now,
                )
                .await?;
            }
        }
        bump_rev(&mut tx, &space).await?;
        tx.commit().await?;
        let root = &copy.pages[0];
        Ok(Node {
            id: root.id,
            space_id: copy.space_id,
            parent_id: copy.parent_id,
            kind: NodeKind::Page,
            title: root.title.clone(),
            position: root_position,
            created_by: Some(copy.created_by),
            created_at: now,
            updated_by: Some(copy.created_by),
            updated_at: now,
            trashed_at: None,
        })
    }
}
