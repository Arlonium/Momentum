//! Éditions d'agent : remplacer du texte, insérer et déplacer des blocs.
//!
//! Les positions de [`PageDoc::replace_text`] se comptent en caractères de
//! `mementum_md::block_text` (une mention, une formule ou un saut de ligne
//! valent un caractère). Elles sont converties en UTF-16 sur le texte brut
//! issu de `diff()`, jamais sur `get_string()`, qui écrit les marques en
//! balises.

use mementum_md::{Block, normalize};
use mementum_schema::{ContentKind, MmdForm, block};
use yrs::types::Attrs;
use yrs::types::text::YChange;
use yrs::{
    Any, Out, ReadTxn, Text, Transact, TransactionMut, Xml, XmlElementPrelim, XmlElementRef,
    XmlFragment, XmlFragmentRef, XmlOut, XmlTextPrelim, XmlTextRef,
};

use crate::blocknote::{
    BLOCK_CONTAINER, BLOCK_GROUP, COLUMN, COLUMN_LIST, Ids, attr_string, block_ids, elements,
    is_block, write_nodes,
};
use crate::structure::{Slot, check_blocks, depth_of, height_of, remaining, structure};
use crate::{CrdtError, MAX_YJS_DEPTH, PageDoc};

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
        let (node, plain) = inline_node(&txn, &container).ok_or_else(out_of_range)?;
        let pieces = pieces(&txn, &node, plain);
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
            let pieces = self::pieces(&txn, &node, plain);
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
    /// Refusé, sans rien écrire : une structure que BlockNote refuserait
    /// (`Structure` : colonne hors d'une liste de colonnes, autre chose
    /// qu'une colonne dans une liste, liste de colonnes dans une colonne,
    /// liste de moins de deux colonnes, colonne vide) ou un arbre qui
    /// dépasserait [`MAX_YJS_DEPTH`] (`TooDeep`).
    pub fn insert_blocks_after(
        &self,
        after_id: Option<&str>,
        blocks: &[Block],
    ) -> Result<Vec<u8>, CrdtError> {
        let blocks = normalize(blocks.to_vec());
        let mut txn = self.doc.transact_mut();
        let place = self.place_after(&txn, after_id)?;
        let (slot, depth) = place.slot();
        check_blocks(&blocks, slot, depth)?;
        let mut ids = Ids::taken(block_ids(&txn, &self.fragment));
        let parent = place.parent(&mut txn, &self.fragment);
        write_nodes(&mut txn, &parent, place.index, &blocks, &mut ids);
        Ok(txn.encode_update_v1())
    }

    /// Déplace le bloc (et ses enfants) juste après `after_id`, ou en tête de
    /// page sans `after_id`. Yjs n'a pas de déplacement : le sous-arbre est
    /// recopié à l'identique (même id), puis l'original supprimé ; un
    /// `blockGroup` vidé par le départ du bloc est retiré.
    ///
    /// Refusé, sans rien écrire : un déplacement dans son propre sous-arbre
    /// (`OutOfRange`), ce qui casserait le schéma des colonnes de BlockNote
    /// (`Structure` : colonne hors d'une liste ou autre bloc dans une liste,
    /// liste de colonnes dans une colonne, colonne laissée vide, liste
    /// laissée à moins de deux colonnes), ou ce qui dépasserait
    /// [`MAX_YJS_DEPTH`] (`TooDeep`).
    pub fn move_block(&self, block_id: &str, after_id: Option<&str>) -> Result<Vec<u8>, CrdtError> {
        let mut txn = self.doc.transact_mut();
        let source = find_block(&txn, &self.fragment, block_id)
            .ok_or_else(|| CrdtError::BlockNotFound(block_id.to_string()))?;
        if after_id == Some(block_id) {
            return Ok(txn.encode_update_v1());
        }
        let place = self.place_after(&txn, after_id)?;
        if let (Some(id), Some(target)) = (after_id, &place.target)
            && inside(target, &source)
        {
            return Err(CrdtError::OutOfRange {
                block_id: id.to_string(),
            });
        }
        let lost = || CrdtError::BlockNotFound(block_id.to_string());
        let (old_parent, _) = position(&txn, &source).ok_or_else(lost)?;
        let (slot, depth) = place.slot();
        match (source.tag().as_ref(), slot) {
            (COLUMN, Slot::ColumnList) => {}
            (COLUMN, _) => {
                return Err(structure(
                    "une colonne n'existe que dans une liste de colonnes",
                ));
            }
            (_, Slot::ColumnList) => {
                return Err(structure(
                    "une liste de colonnes ne contient que des colonnes",
                ));
            }
            (COLUMN_LIST, Slot::Column) => {
                return Err(structure(
                    "une liste de colonnes ne va pas directement dans une colonne",
                ));
            }
            _ => {}
        }
        let stays = usize::from(place.parent.as_ref() == Some(&old_parent));
        match Slot::of(&old_parent) {
            Slot::Column if remaining(&txn, &old_parent, stays) == 0 => {
                return Err(structure("la colonne resterait vide"));
            }
            Slot::ColumnList if remaining(&txn, &old_parent, stays) < 2 => {
                return Err(structure(
                    "la liste de colonnes garderait moins de deux colonnes",
                ));
            }
            _ => {}
        }
        if depth + height_of(&txn, &source) - 1 > MAX_YJS_DEPTH {
            return Err(CrdtError::TooDeep);
        }
        let parent = place.parent(&mut txn, &self.fragment);
        copy_tree(&mut txn, &source, &parent, place.index);
        let (old_parent, old_index) = position(&txn, &source).ok_or_else(lost)?;
        old_parent.remove_range(&mut txn, old_index, 1);
        remove_if_empty_group(&mut txn, &old_parent);
        Ok(txn.encode_update_v1())
    }

    /// Place juste après `after_id`, ou en tête de la racine sans lui.
    fn place_after<T: ReadTxn>(&self, txn: &T, after_id: Option<&str>) -> Result<Place, CrdtError> {
        let Some(id) = after_id else {
            return Ok(Place {
                parent: self.root(txn),
                index: 0,
                target: None,
            });
        };
        let not_found = || CrdtError::BlockNotFound(id.to_string());
        let target = find_block(txn, &self.fragment, id).ok_or_else(not_found)?;
        let (parent, index) = position(txn, &target).ok_or_else(not_found)?;
        Ok(Place {
            parent: Some(parent),
            index: index + 1,
            target: Some(target),
        })
    }
}

/// Où insérer des blocs.
struct Place {
    /// `None` : en tête d'une page sans racine, à créer.
    parent: Option<XmlElementRef>,
    index: u32,
    /// Le bloc après lequel on insère.
    target: Option<XmlElementRef>,
}

impl Place {
    /// Genre du parent, et profondeur du premier élément écrit dedans.
    fn slot(&self) -> (Slot, usize) {
        match &self.parent {
            Some(parent) => (Slot::of(parent), depth_of(parent) + 1),
            None => (Slot::Group, 2),
        }
    }

    fn parent(&self, txn: &mut TransactionMut, fragment: &XmlFragmentRef) -> XmlElementRef {
        match &self.parent {
            Some(parent) => parent.clone(),
            None => fragment.insert(txn, 0, XmlElementPrelim::empty(BLOCK_GROUP)),
        }
    }
}

/// Retire un `blockGroup` d'enfants resté vide (BlockNote n'en admet pas).
pub(crate) fn remove_if_empty_group(txn: &mut TransactionMut, group: &XmlElementRef) {
    if group.tag().as_ref() != BLOCK_GROUP || group.len(txn) > 0 {
        return;
    }
    if let Some((container, index)) = position(txn, group)
        && container.tag().as_ref() == BLOCK_CONTAINER
    {
        container.remove_range(txn, index, 1);
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

/// Parent d'un élément et son index parmi les enfants.
pub(crate) fn position<T: ReadTxn>(
    txn: &T,
    element: &XmlElementRef,
) -> Option<(XmlElementRef, u32)> {
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

/// Nœud de contenu en ligne d'un `blockContainer` (paragraphe, titre…), et
/// vrai pour un bloc de code, dont le contenu est du texte brut.
fn inline_node<T: ReadTxn>(txn: &T, container: &XmlElementRef) -> Option<(XmlElementRef, bool)> {
    if container.tag().as_ref() != BLOCK_CONTAINER {
        return None;
    }
    let node = elements(txn, container)
        .into_iter()
        .find(|e| e.tag().as_ref() != BLOCK_GROUP)?;
    let spec = block(node.tag())?;
    (spec.wrapped && spec.content == ContentKind::Inline)
        .then_some((node, spec.form == MmdForm::Code))
}

/// Un enfant du nœud de contenu, et sa place dans `block_text`.
struct Piece {
    /// Index parmi les enfants du nœud.
    index: u32,
    /// Position de son premier caractère.
    start: usize,
    /// Nombre de caractères : ceux du texte ; 1 pour une mention, une
    /// formule ou un saut de ligne ; 0 pour un élément que la lecture ignore
    /// (et, dans un bloc de code, pour une mention ou une formule).
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

fn pieces<T: ReadTxn>(txn: &T, node: &XmlElementRef, plain: bool) -> Vec<Piece> {
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
                chars: usize::from(match e.tag().as_ref() {
                    "hardBreak" => true,
                    "mention" | "math" => !plain,
                    _ => false,
                }),
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
                        let at = copy.len(txn);
                        let attrs = run.attributes.map(|a| *a).unwrap_or_default();
                        match run.insert {
                            Out::Any(Any::String(s)) => {
                                copy.insert_with_attributes(txn, at, &s, attrs);
                            }
                            // Objet incorporé (nœud étranger) ; un type incorporé est
                            // perdu, comme dans `data`.
                            Out::Any(other) => {
                                copy.insert_embed_with_attributes(txn, at, other, attrs);
                            }
                            _ => {}
                        }
                    }
                }
                XmlOut::Fragment(_) => {}
            }
        }
    }
}
