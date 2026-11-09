//! Dédoublonnage des blocs après des déplacements concurrents.
//!
//! Avec Yjs, un déplacement vaut copie + suppression : deux personnes qui
//! déplacent le même bloc en font deux exemplaires de même id. On garde le
//! premier dans l'ordre du document ; le résultat est le même sur tous les
//! clients, puisqu'ils ont convergé avant. Les éditions faites en même
//! temps sur l'original restent perdues (voir `CLAUDE.md`).
//!
//! La réparation respecte le schéma des colonnes de BlockNote sans jamais
//! déplacer de contenu (Ruling R18) : un déplacement côté serveur perdrait
//! les éditions concurrentes.

use std::collections::HashSet;

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
    pub fn dedupe_blocks(&self) -> Option<Vec<u8>> {
        if plan(&self.doc.transact(), &self.fragment).is_empty() {
            return None;
        }
        let mut txn = self.doc.transact_mut();
        let fixes = plan(&txn, &self.fragment);
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
fn plan<T: ReadTxn>(txn: &T, fragment: &XmlFragmentRef) -> Vec<Fix> {
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
                if !id.is_empty() && !seen.insert(id) {
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
