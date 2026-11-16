//! Dédoublonnage des blocs après des déplacements concurrents.
//!
//! Avec Yjs, un déplacement vaut copie + suppression : deux personnes qui
//! déplacent le même bloc en font deux exemplaires de même id. On garde le
//! premier dans l'ordre du document ; le résultat est le même sur tous les
//! clients, puisqu'ils ont convergé avant. Sauf un exemplaire où le serveur a
//! reporté ce qu'un autre avait tapé dans l'original (Task 13b) : c'est lui
//! qu'on garde, pour ne pas perdre le report (Ruling R453). Les éditions faites
//! en même temps sur l'original restent sinon perdues (voir `CLAUDE.md`).
//!
//! La réparation respecte le schéma des colonnes de BlockNote sans jamais
//! déplacer de contenu (Ruling R18) : un déplacement côté serveur perdrait
//! les éditions concurrentes.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

use yrs::branch::BranchID;
use yrs::{
    ReadTxn, Transact, TransactionMut, Xml, XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut,
};

use crate::PageDoc;
use crate::blocknote::{
    BLOCK_GROUP, COLUMN, Ids, attr_string, block_ids, elements, empty_paragraph, is_block,
    write_nodes,
};
use crate::edit::remove_if_empty_group;
use crate::structure::Slot;

/// Ce que le dédoublonnage fait d'un doublon.
enum Fix {
    /// Supprimé avec son sous-arbre : (parent, index).
    Remove(XmlElementRef, u32),
    /// Colonne gardée sous un nouvel id : la supprimer ferait tomber sa liste
    /// sous deux colonnes.
    Rename(XmlElementRef),
}

impl PageDoc {
    /// Supprime chaque bloc (`blockContainer`, `columnList` ou `column`) dont
    /// l'id est déjà apparu plus haut dans le document, avec son sous-arbre.
    /// Une colonne dont la suppression laisserait sa liste à moins de deux
    /// colonnes reçoit un nouvel id (UUIDv7) ; une colonne vidée reçoit un
    /// paragraphe vide ; un `blockGroup` d'enfants vidé est retiré. Renvoie
    /// `None` s'il n'y a rien à faire.
    ///
    /// La page n'est parcourue que si une mise à jour appliquée par
    /// [`PageDoc::apply_update`] depuis le dernier appel a inséré des éléments
    /// (Ruling R25) : une frappe seule ne duplique rien. Les écritures du
    /// serveur (`write_blocks`, `insert_blocks_after`, `move_block`) ne créent
    /// pas de doublon.
    pub fn dedupe_blocks(&self) -> Option<Vec<u8>> {
        if !self.inserted_elements.swap(false, Ordering::SeqCst) {
            return None;
        }
        self.dedupe_scans.fetch_add(1, Ordering::SeqCst);
        let prefer = self.preferred(&self.doc.transact());
        if plan(&self.doc.transact(), &self.fragment, &prefer).is_empty() {
            return None;
        }
        let mut txn = self.doc.transact_mut();
        let fixes = plan(&txn, &self.fragment, &prefer);
        if fixes.is_empty() {
            return None;
        }
        let mut ids = Ids::taken(block_ids(&txn, &self.fragment));
        let mut touched = Vec::new();
        // Du dernier au premier : les index relevés restent justes.
        for fix in fixes.into_iter().rev() {
            match fix {
                Fix::Remove(parent, index) => {
                    parent.remove_range(&mut txn, index, 1);
                    if !touched.contains(&parent) {
                        touched.push(parent);
                    }
                }
                Fix::Rename(column) => {
                    column.insert_attribute(&mut txn, "id", ids.take(""));
                }
            }
        }
        for parent in touched {
            repair(&mut txn, &parent, &mut ids);
        }
        Some(txn.encode_update_v1())
    }

    /// Les copies qui ont reçu un report et vivent encore, par identifiant de bloc (Ruling
    /// R453). Celles qui ne vivent plus sont oubliées.
    fn preferred<T: ReadTxn>(&self, txn: &T) -> HashMap<String, XmlElementRef> {
        let mut copies = self
            .carried_copies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut live = HashMap::new();
        copies.retain(|block, id| {
            let element = BranchID::get_nested(txn, id)
                .filter(|branch| !branch.is_deleted())
                .and_then(|branch| match XmlOut::try_from(branch) {
                    Ok(XmlOut::Element(e)) => Some(e),
                    _ => None,
                });
            match element {
                Some(e) => {
                    live.insert(block.clone(), e);
                    true
                }
                None => false,
            }
        });
        live
    }

    /// Nombre de parcours de la page faits par [`PageDoc::dedupe_blocks`].
    /// Sert aux tests.
    #[doc(hidden)]
    pub fn dedupe_scans(&self) -> u64 {
        self.dedupe_scans.load(Ordering::SeqCst)
    }
}

/// Un parent qui a perdu des blocs : une colonne ou une racine vide reçoit
/// un paragraphe vide, un `blockGroup` d'enfants vide est retiré.
fn repair(txn: &mut TransactionMut, parent: &XmlElementRef, ids: &mut Ids) {
    if !elements(txn, parent).is_empty() {
        return;
    }
    let root = matches!(parent.parent(), Some(XmlOut::Fragment(_)));
    if parent.tag().as_ref() == COLUMN || (root && parent.tag().as_ref() == BLOCK_GROUP) {
        write_nodes(txn, parent, 0, &[empty_paragraph()], ids);
    } else {
        remove_if_empty_group(txn, parent);
    }
}

/// Doublons dans l'ordre du document, sans récursion. On ne descend pas dans
/// un doublon supprimé : tout son sous-arbre part avec lui.
///
/// Pour un identifiant de `prefer`, c'est l'exemplaire nommé qui reste, et les autres partent,
/// avant comme après lui (Ruling R453). Si cet exemplaire partait lui-même avec un bloc qui le
/// contient, l'identifiant suit la règle ordinaire (le premier reste) : sinon tous ses
/// exemplaires partiraient.
fn plan<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    prefer: &HashMap<String, XmlElementRef>,
) -> Vec<Fix> {
    let fixes = plan_with(txn, fragment, prefer);
    if prefer.is_empty() {
        return fixes;
    }
    let removed: Vec<XmlElementRef> = fixes
        .iter()
        .filter_map(|fix| match fix {
            Fix::Remove(parent, index) => match parent.get(txn, *index) {
                Some(XmlOut::Element(e)) => Some(e),
                _ => None,
            },
            Fix::Rename(_) => None,
        })
        .collect();
    let kept: HashMap<String, XmlElementRef> = prefer
        .iter()
        .filter(|(_, element)| !removed.iter().any(|r| inside(element, r)))
        .map(|(id, element)| (id.clone(), element.clone()))
        .collect();
    if kept.len() == prefer.len() {
        fixes
    } else {
        plan_with(txn, fragment, &kept)
    }
}

/// Vrai si `element` est `ancestor` ou l'un de ses descendants.
fn inside(element: &XmlElementRef, ancestor: &XmlElementRef) -> bool {
    let mut current = Some(element.clone());
    while let Some(e) = current {
        if e == *ancestor {
            return true;
        }
        current = match e.parent() {
            Some(XmlOut::Element(p)) => Some(p),
            _ => None,
        };
    }
    false
}

fn plan_with<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    prefer: &HashMap<String, XmlElementRef>,
) -> Vec<Fix> {
    let mut seen = HashSet::new();
    let mut fixes = Vec::new();
    // Colonnes déjà promises à la suppression, par liste.
    let mut removed_columns: Vec<(XmlElementRef, usize)> = Vec::new();
    for root in fragment.children(txn) {
        let XmlOut::Element(root) = root else {
            continue;
        };
        let mut stack = vec![(root.clone(), children(txn, &root).into_iter())];
        while let Some((parent, iter)) = stack.last_mut() {
            let Some((index, element)) = iter.next() else {
                stack.pop();
                continue;
            };
            let parent = parent.clone();
            if is_block(&element) {
                let id = attr_string(txn, &element, "id");
                if let Some(preferred) = prefer.get(&id) {
                    if *preferred != element {
                        fixes.push(Fix::Remove(parent, index));
                        continue;
                    }
                    seen.insert(id);
                } else if !id.is_empty() && !seen.insert(id) {
                    if Slot::of(&parent) == Slot::ColumnList && element.tag().as_ref() == COLUMN {
                        let gone = match removed_columns.iter().position(|(l, _)| *l == parent) {
                            Some(k) => k,
                            None => {
                                removed_columns.push((parent.clone(), 0));
                                removed_columns.len() - 1
                            }
                        };
                        let columns = elements(txn, &parent).len();
                        if columns.saturating_sub(removed_columns[gone].1 + 1) < 2 {
                            fixes.push(Fix::Rename(element.clone()));
                        } else {
                            removed_columns[gone].1 += 1;
                            fixes.push(Fix::Remove(parent, index));
                            continue;
                        }
                    } else {
                        fixes.push(Fix::Remove(parent, index));
                        continue;
                    }
                }
            }
            let next = children(txn, &element).into_iter();
            stack.push((element, next));
        }
    }
    fixes
}

/// Éléments enfants avec leur index parmi tous les enfants.
fn children<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> Vec<(u32, XmlElementRef)> {
    element
        .children(txn)
        .enumerate()
        .filter_map(|(i, c)| match c {
            XmlOut::Element(e) => Some((i as u32, e)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use yrs::branch::{Branch, BranchID};
    use yrs::{Text, Xml, XmlElementPrelim, XmlTextPrelim};

    use super::*;
    use crate::blocknote::BLOCK_CONTAINER;

    /// Un `blockContainer` d'`id` donné, avec un paragraphe et son texte, à la fin de `parent`.
    fn container(
        txn: &mut TransactionMut,
        parent: &XmlElementRef,
        id: &str,
        text: &str,
    ) -> XmlElementRef {
        let c = parent.push_back(txn, XmlElementPrelim::empty(BLOCK_CONTAINER));
        c.insert_attribute(txn, "id", id.to_string());
        let p = c.push_back(txn, XmlElementPrelim::empty("paragraph"));
        let t = p.push_back(txn, XmlTextPrelim::new(""));
        t.insert(txn, 0, text);
        c
    }

    /// Ruling R453, relecture du tour 1 (I2b) : la copie reportée de `x` est dans un exemplaire de
    /// `b` que le dédoublonnage retire (le second dans l'ordre du document). La préférence tombe,
    /// et `x` suit la règle ordinaire : l'exemplaire qui reste, hors de `b`, n'est pas retiré ;
    /// sans ce repli, tous les exemplaires de `x` partiraient.
    #[test]
    fn a_preferred_copy_removed_with_its_parent_falls_back_to_the_first() {
        let (page, _) = PageDoc::new_page();
        let preferred = {
            let mut txn = page.doc.transact_mut();
            let root = page.root(&txn).unwrap();
            let len = root.len(&txn);
            root.remove_range(&mut txn, 0, len);
            container(&mut txn, &root, "b", "B1");
            let b2 = container(&mut txn, &root, "b", "B2");
            let group = b2.push_back(&mut txn, XmlElementPrelim::empty(BLOCK_GROUP));
            let c1 = container(&mut txn, &group, "x", "dans B2");
            container(&mut txn, &root, "x", "dehors");
            match AsRef::<Branch>::as_ref(&c1).id() {
                BranchID::Nested(id) => id,
                BranchID::Root(_) => unreachable!(),
            }
        };
        page.carried_copies
            .lock()
            .unwrap()
            .insert("x".to_string(), preferred);
        page.inserted_elements.store(true, Ordering::SeqCst);
        assert!(page.dedupe_blocks().is_some());
        let blocks: Vec<(String, String)> = page
            .read_blocks()
            .iter()
            .map(|b| (b.id.clone(), mementum_md::block_text(b)))
            .collect();
        assert_eq!(
            blocks,
            [
                ("b".to_string(), "B1".to_string()),
                ("x".to_string(), "dehors".to_string())
            ]
        );
    }
}
