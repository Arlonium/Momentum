//! Éditions d'agent : remplacer du texte, insérer et déplacer des blocs.
//!
//! Les positions de [`PageDoc::replace_text`] se comptent en caractères de
//! `mementum_md::block_text` (une mention, une formule ou un saut de ligne
//! valent un caractère). Elles sont converties en UTF-16 sur le texte brut
//! issu de `diff()`, jamais sur `get_string()`, qui écrit les marques en
//! balises.

use std::collections::HashSet;

use mementum_md::Block;
use mementum_schema::{ContentKind, block};
use yrs::types::Attrs;
use yrs::types::text::YChange;
use yrs::{
    Any, Out, ReadTxn, Text, Transact, TransactionMut, Xml, XmlElementPrelim, XmlElementRef,
    XmlFragment, XmlFragmentRef, XmlOut, XmlTextPrelim, XmlTextRef,
};

use crate::blocknote::{
    BLOCK_CONTAINER, BLOCK_GROUP, COLUMN, Ids, attr_string, elements, is_block, write_nodes,
};
use crate::{CrdtError, PageDoc};

impl PageDoc {
    /// Remplace `len` caractères à partir de `start` dans le texte du bloc
    /// par `new`. Le texte inséré prend les marques du premier caractère
    /// remplacé (à défaut, du caractère qui précède). Une mention ou une
    /// formule couverte par la plage est supprimée.
    pub fn replace_text(
        &self,
        block_id: &str,
        start: usize,
        len: usize,
        new: &str,
    ) -> Result<Vec<u8>, CrdtError> {
        let mut txn = self.doc.transact_mut();
        let out_of_range = || CrdtError::OutOfRange {
            block_id: block_id.to_string(),
        };
        let container = find_block(&txn, &self.fragment, block_id)
            .ok_or_else(|| CrdtError::BlockNotFound(block_id.to_string()))?;
        let node = inline_node(&txn, &container).ok_or_else(out_of_range)?;
        let pieces = pieces(&txn, &node);
        let end = start.checked_add(len).ok_or_else(out_of_range)?;
        if end > pieces.iter().map(|p| p.chars).sum() {
            return Err(out_of_range());
        }

        let attrs = (len > 0)
            .then(|| attrs_at(&pieces, start))
            .flatten()
            .or_else(|| start.checked_sub(1).and_then(|p| attrs_at(&pieces, p)))
            .or_else(|| attrs_at(&pieces, start))
            .unwrap_or_default();

        // Suppressions : le texte de chaque XmlText couvert, puis les éléments
        // couverts, du dernier au premier pour garder les index.
        let mut doomed = Vec::new();
        for piece in &pieces {
            match &piece.kind {
                Kind::Text(text, runs) => {
                    let from = start.clamp(piece.start, piece.end());
                    let to = end.clamp(piece.start, piece.end());
                    if from < to {
                        let a = utf16_at(runs, from - piece.start);
                        let b = utf16_at(runs, to - piece.start);
                        text.remove_range(&mut txn, a, b - a);
                    }
                }
                Kind::Element => {
                    let inside = match piece.chars {
                        0 => start < piece.start && piece.start < end,
                        _ => start <= piece.start && piece.start < end,
                    };
                    if inside {
                        doomed.push(piece.index);
                    }
                }
            }
        }
        for index in doomed.into_iter().rev() {
            node.remove_range(&mut txn, index, 1);
        }

        if !new.is_empty() {
            let pieces = self::pieces(&txn, &node);
            let target = pieces.iter().find_map(|p| match &p.kind {
                Kind::Text(text, runs) if p.start <= start && start <= p.end() => {
                    Some((text.clone(), utf16_at(runs, start - p.start)))
                }
                _ => None,
            });
            let (text, at) = match target {
                Some(found) => found,
                None => {
                    // Entre deux éléments : un nouvel XmlText à cette place.
                    let index = pieces
                        .iter()
                        .find(|p| p.start >= start)
                        .map_or(node.len(&txn), |p| p.index);
                    (node.insert(&mut txn, index, XmlTextPrelim::new("")), 0)
                }
            };
            text.insert_with_attributes(&mut txn, at, new, attrs);
        }
        Ok(txn.encode_update_v1())
    }

    /// Insère `blocks` juste après le bloc `after_id`, au même niveau, ou en
    /// tête de page sans `after_id`. Un bloc sans id, ou dont l'id existe
    /// déjà dans la page, reçoit un UUIDv7 (sinon le dédoublonnage le
    /// supprimerait).
    ///
    /// Comme dans BlockNote, une colonne ne se place qu'après une colonne, et
    /// un autre bloc jamais après une colonne (`OutOfRange`).
    pub fn insert_blocks_after(
        &self,
        after_id: Option<&str>,
        blocks: &[Block],
    ) -> Result<Vec<u8>, CrdtError> {
        let mut txn = self.doc.transact_mut();
        let slot = self.slot_after(&txn, after_id)?;
        if let Some(b) = blocks
            .iter()
            .find(|b| (b.kind == COLUMN) != slot.after_column)
        {
            return Err(CrdtError::OutOfRange {
                block_id: after_id.unwrap_or(&b.id).to_string(),
            });
        }
        let mut ids = Ids::taken(block_ids(&txn, &self.fragment));
        let parent = slot.parent(&mut txn, &self.fragment);
        write_nodes(&mut txn, &parent, slot.index, blocks, &mut ids);
        Ok(txn.encode_update_v1())
    }

    /// Déplace le bloc (et ses enfants) juste après `after_id`, ou en tête de
    /// page sans `after_id`. Yjs n'a pas de déplacement : le sous-arbre est
    /// recopié à l'identique (même id), puis l'original supprimé.
    ///
    /// Une colonne ne se place qu'après une colonne, un autre bloc jamais
    /// après une colonne, et aucun bloc dans son propre sous-arbre
    /// (`OutOfRange`).
    pub fn move_block(&self, block_id: &str, after_id: Option<&str>) -> Result<Vec<u8>, CrdtError> {
        let mut txn = self.doc.transact_mut();
        let source = find_block(&txn, &self.fragment, block_id)
            .ok_or_else(|| CrdtError::BlockNotFound(block_id.to_string()))?;
        if after_id == Some(block_id) {
            return Ok(txn.encode_update_v1());
        }
        let slot = self.slot_after(&txn, after_id)?;
        if let (Some(id), Some(target)) = (after_id, &slot.target)
            && inside(target, &source)
        {
            return Err(CrdtError::OutOfRange {
                block_id: id.to_string(),
            });
        }
        if (source.tag().as_ref() == COLUMN) != slot.after_column {
            return Err(CrdtError::OutOfRange {
                block_id: block_id.to_string(),
            });
        }
        let parent = slot.parent(&mut txn, &self.fragment);
        copy_tree(&mut txn, &source, &parent, slot.index);
        let (old_parent, old_index) = position(&txn, &source).expect("bloc source en place");
        old_parent.remove_range(&mut txn, old_index, 1);
        Ok(txn.encode_update_v1())
    }

    /// Place juste après `after_id`, ou en tête de la racine sans lui.
    fn slot_after<T: ReadTxn>(&self, txn: &T, after_id: Option<&str>) -> Result<Slot, CrdtError> {
        let Some(id) = after_id else {
            return Ok(Slot {
                parent: self.root(txn),
                index: 0,
                target: None,
                after_column: false,
            });
        };
        let not_found = || CrdtError::BlockNotFound(id.to_string());
        let target = find_block(txn, &self.fragment, id).ok_or_else(not_found)?;
        let (parent, index) = position(txn, &target).ok_or_else(not_found)?;
        Ok(Slot {
            parent: Some(parent),
            index: index + 1,
            after_column: target.tag().as_ref() == COLUMN,
            target: Some(target),
        })
    }
}

/// Où insérer des blocs.
struct Slot {
    /// `None` : en tête d'une page sans racine, à créer.
    parent: Option<XmlElementRef>,
    index: u32,
    /// Le bloc après lequel on insère.
    target: Option<XmlElementRef>,
    after_column: bool,
}

impl Slot {
    fn parent(&self, txn: &mut TransactionMut, fragment: &XmlFragmentRef) -> XmlElementRef {
        match &self.parent {
            Some(parent) => parent.clone(),
            None => fragment.insert(txn, 0, XmlElementPrelim::empty(BLOCK_GROUP)),
        }
    }
}

/// Premier élément de bloc portant cet id, dans l'ordre du document, sous
/// un élément (un bloc posé directement dans le fragment n'est pas lu).
pub(crate) fn find_block<T: ReadTxn>(
    txn: &T,
    fragment: &XmlFragmentRef,
    id: &str,
) -> Option<XmlElementRef> {
    if id.is_empty() {
        return None;
    }
    fragment.successors(txn).find_map(|node| match node {
        XmlOut::Element(e)
            if is_block(&e)
                && matches!(e.parent(), Some(XmlOut::Element(_)))
                && attr_string(txn, &e, "id") == id =>
        {
            Some(e)
        }
        _ => None,
    })
}

/// Ids de tous les blocs de la page.
fn block_ids<T: ReadTxn>(txn: &T, fragment: &XmlFragmentRef) -> HashSet<String> {
    fragment
        .successors(txn)
        .filter_map(|node| match node {
            XmlOut::Element(e) if is_block(&e) => Some(attr_string(txn, &e, "id")),
            _ => None,
        })
        .filter(|id| !id.is_empty())
        .collect()
}

/// Parent d'un élément et son index parmi les enfants.
fn position<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> Option<(XmlElementRef, u32)> {
    let Some(XmlOut::Element(parent)) = element.parent() else {
        return None;
    };
    let index = parent
        .children(txn)
        .position(|c| matches!(&c, XmlOut::Element(e) if e == element))?;
    Some((parent, index as u32))
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

/// Nœud de contenu en ligne d'un `blockContainer` (paragraphe, titre…).
fn inline_node<T: ReadTxn>(txn: &T, container: &XmlElementRef) -> Option<XmlElementRef> {
    if container.tag().as_ref() != BLOCK_CONTAINER {
        return None;
    }
    let node = elements(txn, container)
        .into_iter()
        .find(|e| e.tag().as_ref() != BLOCK_GROUP)?;
    let spec = block(node.tag())?;
    (spec.wrapped && spec.content == ContentKind::Inline).then_some(node)
}

/// Un enfant du nœud de contenu, et sa place dans `block_text`.
struct Piece {
    /// Index parmi les enfants du nœud.
    index: u32,
    /// Position de son premier caractère.
    start: usize,
    /// Nombre de caractères : ceux du texte ; 1 pour une mention, une
    /// formule ou un saut de ligne ; 0 pour un élément que la lecture ignore.
    chars: usize,
    kind: Kind,
}

impl Piece {
    fn end(&self) -> usize {
        self.start + self.chars
    }
}

enum Kind {
    Text(XmlTextRef, Vec<Run>),
    Element,
}

/// Une plage de `diff()` : texte brut (vide pour un objet incorporé), sa
/// longueur UTF-16 et ses marques.
struct Run {
    text: String,
    utf16: u32,
    attrs: Attrs,
}

fn pieces<T: ReadTxn>(txn: &T, node: &XmlElementRef) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut start = 0;
    for (index, child) in node.children(txn).enumerate() {
        let index = index as u32;
        let piece = match child {
            XmlOut::Text(text) => {
                let runs: Vec<Run> = text
                    .diff(txn, YChange::identity)
                    .into_iter()
                    .map(|d| {
                        let attrs = d.attributes.map(|a| *a).unwrap_or_default();
                        match d.insert {
                            Out::Any(Any::String(s)) => Run {
                                utf16: s.encode_utf16().count() as u32,
                                text: s.to_string(),
                                attrs,
                            },
                            // Objet incorporé : une unité Yjs, aucun caractère lu.
                            _ => Run {
                                text: String::new(),
                                utf16: 1,
                                attrs,
                            },
                        }
                    })
                    .collect();
                let chars = runs.iter().map(|r| r.text.chars().count()).sum();
                Piece {
                    index,
                    start,
                    chars,
                    kind: Kind::Text(text, runs),
                }
            }
            XmlOut::Element(e) => Piece {
                index,
                start,
                chars: usize::from(matches!(e.tag().as_ref(), "mention" | "math" | "hardBreak")),
                kind: Kind::Element,
            },
            XmlOut::Fragment(_) => continue,
        };
        start += piece.chars;
        out.push(piece);
    }
    out
}

/// Position UTF-16 du caractère `k` d'un XmlText.
fn utf16_at(runs: &[Run], k: usize) -> u32 {
    let (mut chars, mut offset) = (0, 0);
    for run in runs {
        if run.text.is_empty() {
            if chars == k {
                return offset;
            }
            offset += run.utf16;
            continue;
        }
        for c in run.text.chars() {
            if chars == k {
                return offset;
            }
            chars += 1;
            offset += c.len_utf16() as u32;
        }
    }
    offset
}

/// Marques du caractère de texte à la position `pos`.
fn attrs_at(pieces: &[Piece], pos: usize) -> Option<Attrs> {
    let piece = pieces.iter().find(|p| p.start <= pos && pos < p.end())?;
    let Kind::Text(_, runs) = &piece.kind else {
        return None;
    };
    let mut local = pos - piece.start;
    for run in runs {
        let n = run.text.chars().count();
        if local < n {
            return Some(run.attrs.clone());
        }
        local -= n;
    }
    None
}

/// Recopie à l'identique (balises, attributs, textes et marques) le
/// sous-arbre `source` à l'index `at` de `parent`, sans récursion.
fn copy_tree(txn: &mut TransactionMut, source: &XmlElementRef, parent: &XmlElementRef, at: u32) {
    let root = parent.insert(txn, at, XmlElementPrelim::empty(source.tag().as_ref()));
    let mut stack = vec![(source.clone(), root)];
    while let Some((from, to)) = stack.pop() {
        let attrs: Vec<(String, Any)> = from
            .attributes(txn)
            .filter_map(|(k, v)| match v {
                Out::Any(a) => Some((k.to_string(), a)),
                _ => None,
            })
            .collect();
        for (key, value) in attrs {
            to.insert_attribute(txn, key, value);
        }
        let children: Vec<XmlOut> = from.children(txn).collect();
        for child in children {
            match child {
                XmlOut::Element(e) => {
                    let copy = to.push_back(txn, XmlElementPrelim::empty(e.tag().as_ref()));
                    stack.push((e, copy));
                }
                XmlOut::Text(text) => {
                    let runs = text.diff(txn, YChange::identity);
                    let copy = to.push_back(txn, XmlTextPrelim::new(""));
                    for run in runs {
                        // y-prosemirror n'incorpore pas d'objet dans un texte.
                        if let Out::Any(Any::String(s)) = run.insert {
                            let at = copy.len(txn);
                            let attrs = run.attributes.map(|a| *a).unwrap_or_default();
                            copy.insert_with_attributes(txn, at, &s, attrs);
                        }
                    }
                }
                XmlOut::Fragment(_) => {}
            }
        }
    }
}
