//! Dédoublonnage des blocs après des déplacements concurrents.
//!
//! Avec Yjs, un déplacement vaut copie + suppression : deux personnes qui
//! déplacent le même bloc en font deux exemplaires de même id. On garde le
//! premier dans l'ordre du document ; le résultat est le même sur tous les
//! clients, puisqu'ils ont convergé avant. Les éditions faites en même
//! temps sur l'original restent perdues (voir `CLAUDE.md`).

use std::collections::HashSet;

use yrs::{ReadTxn, Transact, XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut};

use crate::PageDoc;
use crate::blocknote::{attr_string, is_block};

impl PageDoc {
    /// Supprime chaque bloc (`blockContainer`, `columnList` ou `column`) dont
    /// l'id est déjà apparu plus haut dans le document, avec son sous-arbre.
    /// Renvoie `None` s'il n'y a rien à faire.
    pub fn dedupe_blocks(&self) -> Option<Vec<u8>> {
        if duplicates(&self.doc.transact(), &self.fragment).is_empty() {
            return None;
        }
        let mut txn = self.doc.transact_mut();
        let doomed = duplicates(&txn, &self.fragment);
        if doomed.is_empty() {
            return None;
        }
        // Du dernier au premier : les index relevés restent justes.
        for (parent, index) in doomed.into_iter().rev() {
            parent.remove_range(&mut txn, index, 1);
        }
        Some(txn.encode_update_v1())
    }
}

/// Doublons (parent, index), dans l'ordre du document, sans récursion. On
/// ne descend pas dans un doublon : tout son sous-arbre part avec lui.
fn duplicates<T: ReadTxn>(txn: &T, fragment: &XmlFragmentRef) -> Vec<(XmlElementRef, u32)> {
    let mut seen = HashSet::new();
    let mut doomed = Vec::new();
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
                    doomed.push((parent, index));
                    continue;
                }
            }
            let next = children(txn, &element).into_iter();
            stack.push((element, next));
        }
    }
    doomed
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
