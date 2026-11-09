//! Ce qu'une écriture doit respecter avant de toucher au document : le
//! schéma de `@blocknote/xl-multi-column` 0.57 (Ruling R18) et la borne de
//! profondeur (Ruling R17).
//!
//! y-prosemirror supprime chez tous les clients un nœud qu'il ne sait pas
//! construire, en cascade vers son parent, avec le contenu voisin. Une
//! colonne n'existe donc que dans un `columnList`, qui en a deux au moins ;
//! une colonne n'est jamais vide et ne contient jamais directement un
//! `columnList`.

use mementum_md::{Block, Content};
use yrs::{ReadTxn, Xml, XmlElementRef, XmlFragment, XmlOut};

use crate::blocknote::{COLUMN, COLUMN_LIST, elements};
use crate::foreign::Foreign;
use crate::{CrdtError, MAX_YJS_DEPTH};

/// Genre de parent dans lequel des blocs s'écrivent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Slot {
    /// Un `blockGroup` (la racine ou les enfants d'un bloc).
    Group,
    ColumnList,
    Column,
}

impl Slot {
    pub(crate) fn of(parent: &XmlElementRef) -> Slot {
        match parent.tag().as_ref() {
            COLUMN_LIST => Slot::ColumnList,
            COLUMN => Slot::Column,
            _ => Slot::Group,
        }
    }
}

pub(crate) fn structure(message: &str) -> CrdtError {
    CrdtError::Structure(message.to_string())
}

/// Vérifie, sans récursion, des blocs normalisés à écrire dans un parent du
/// genre `slot`, le premier élément écrit étant à la profondeur `depth`
/// (la racine `blockGroup` est à 1).
pub(crate) fn check_blocks(blocks: &[Block], slot: Slot, depth: usize) -> Result<(), CrdtError> {
    let mut stack: Vec<(&[Block], Slot, usize)> = vec![(blocks, slot, depth)];
    while let Some((blocks, slot, depth)) = stack.pop() {
        if depth > MAX_YJS_DEPTH && !blocks.is_empty() {
            return Err(CrdtError::TooDeep);
        }
        for b in blocks {
            let column = b.kind == COLUMN;
            match slot {
                Slot::ColumnList if !column => {
                    return Err(structure(
                        "une liste de colonnes ne contient que des colonnes",
                    ));
                }
                Slot::Group | Slot::Column if column => {
                    return Err(structure(
                        "une colonne n'existe que dans une liste de colonnes",
                    ));
                }
                Slot::Column if b.kind == COLUMN_LIST => {
                    return Err(structure(
                        "une liste de colonnes ne va pas directement dans une colonne",
                    ));
                }
                _ => {}
            }
            // Profondeur du contenu le plus profond du bloc, enfants à part.
            let (deepest, children) = match b.kind.as_str() {
                COLUMN_LIST if b.children.len() < 2 => {
                    return Err(structure("une liste de colonnes a deux colonnes au moins"));
                }
                COLUMN if b.children.is_empty() => {
                    return Err(structure("une colonne contient au moins un bloc"));
                }
                COLUMN_LIST => (depth, Some((Slot::ColumnList, depth + 1))),
                COLUMN => (depth, Some((Slot::Column, depth + 1))),
                _ => match Foreign::of(b) {
                    // Hors enveloppe : ses enfants s'écrivent à sa suite.
                    Some(f) if f.unwrapped() => {
                        stack.push((&b.children, slot, depth));
                        (depth + f.content_depth(), None)
                    }
                    Some(f) => (
                        depth + 1 + f.content_depth(),
                        Some((Slot::Group, depth + 2)),
                    ),
                    None => {
                        let content = match &b.content {
                            Content::Table(_) => 5,
                            Content::Inline(c) if !c.is_empty() => 2,
                            _ => 1,
                        };
                        (depth + content, Some((Slot::Group, depth + 2)))
                    }
                },
            };
            if deepest > MAX_YJS_DEPTH {
                return Err(CrdtError::TooDeep);
            }
            if let Some((slot, depth)) = children {
                stack.push((&b.children, slot, depth));
            }
        }
    }
    Ok(())
}

/// Profondeur d'un élément (la racine `blockGroup` est à 1).
pub(crate) fn depth_of(element: &XmlElementRef) -> usize {
    depth_in_page(&XmlOut::Element(element.clone()))
}

/// Profondeur d'un nœud XML : 1 sous le fragment (ou sous la map `META`),
/// un de plus par élément ou texte parent. S'arrête dès qu'elle dépasse
/// [`MAX_YJS_DEPTH`].
pub(crate) fn depth_in_page(node: &XmlOut) -> usize {
    let parent = |node: &XmlOut| match node {
        XmlOut::Element(e) => e.parent(),
        XmlOut::Text(t) => t.parent(),
        XmlOut::Fragment(_) => None,
    };
    let mut depth = 1;
    let mut current = parent(node);
    while let Some(p) = current {
        if matches!(p, XmlOut::Fragment(_)) || depth > MAX_YJS_DEPTH {
            break;
        }
        depth += 1;
        current = parent(&p);
    }
    depth
}

/// Hauteur d'un sous-arbre : 1 pour un élément sans enfant (un texte compte
/// pour un niveau), sans récursion.
pub(crate) fn height_of<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> usize {
    let mut tallest = 0;
    let mut stack = vec![(element.clone(), 1)];
    while let Some((e, height)) = stack.pop() {
        tallest = tallest.max(height);
        for child in e.children(txn) {
            match child {
                XmlOut::Element(c) => stack.push((c, height + 1)),
                _ => tallest = tallest.max(height + 1),
            }
        }
    }
    tallest
}

/// Nombre de blocs (éléments) restant dans `parent` si l'on en retire un et
/// qu'on en ajoute `added`.
pub(crate) fn remaining<T: ReadTxn>(txn: &T, parent: &XmlElementRef, added: usize) -> usize {
    (elements(txn, parent).len() + added).saturating_sub(1)
}
