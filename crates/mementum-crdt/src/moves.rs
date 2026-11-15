//! Le report sur un bloc recréé de ce que l'auteur du déplacement n'avait pas vu (Task 13b,
//! Rulings R221 et R371 à R379).
//!
//! Yjs n'a pas de déplacement. Le client (correctif de y-prosemirror, Ruling R220) écrit un bloc
//! déplacé comme **sa suppression, et sa recréation au même `id`** ; Tab, Maj+Tab et un
//! déplacement d'un niveau à l'autre l'écrivaient déjà ainsi. Ce qu'un autre a tapé dans
//! l'original, et que l'auteur du déplacement n'avait pas vu, est dans l'élément supprimé : il
//! part avec lui.
//!
//! Le serveur le reporte quand le déplacement arrive après la frappe. Un **bloc recréé**, pour
//! lui, est un `blockContainer` que la mise à jour d'un client insère avec l'attribut `id` d'un
//! bloc vivant de la page, dont le jeu de suppressions de la même mise à jour couvre l'élément.
//! Peu importe que ce soit le bloc du geste ou un bloc croisé (Rulings R227, R234) : la règle ne
//! regarde que la mise à jour, et ne suppose aucun `id` stable hors de celle-ci (Ruling R223).
//!
//! **Ce que l'auteur n'avait pas vu**, c'est ce que son jeu de suppressions ne couvre pas dans
//! l'original : il a supprimé tout ce qu'il en connaissait, y compris dans une étape 2 qui porte
//! tout son jeu de suppressions. C'est relevé **avant** d'appliquer la mise à jour
//! ([`capture`]) : appliquée, elle supprime l'original et tout ce qu'il porte, vu ou non, et yrs
//! ne laisse plus rien lire d'un élément supprimé. Puis, la mise à jour appliquée, c'est écrit
//! dans la copie par une écriture du serveur ([`carry`]), à écrire au journal et à diffuser avec
//! elle.
//!
//! Ce qui est reporté :
//! - **le texte** du contenu en ligne de l'original (paragraphe, titre…), avec ses marques, à la
//!   place qu'il avait parmi les caractères vus, quand cette place est **sûre** dans la copie
//!   (voir [`Places`], Ruling R373) ;
//! - **les sous-blocs** que l'original a reçus sans que l'auteur les voie, recopiés sous la copie,
//!   après la copie du sous-bloc vu qui les précédait.
//!
//! Ce qui ne l'est pas, et reste perdu : le texte d'un contenu en ligne fait de plusieurs textes
//! (mention, formule, saut de ligne), d'un tableau, d'un bloc dont le type a changé, ou dont la
//! place n'est pas sûre ; un changement de type ou de réglage ; tout ce qui arrive au serveur
//! après le déplacement (Ruling R222).

use std::collections::HashMap;

use yrs::types::Attrs;
use yrs::types::text::{ChangeKind, YChange};
use yrs::{
    Any, BranchID, ClientID, ID, IdSet, Out, ReadTxn, Snapshot, StateVector, Text, TransactionMut,
    Xml, XmlElementPrelim, XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut, XmlTextPrelim,
    XmlTextRef,
};

use crate::MAX_YJS_DEPTH;
use crate::blocknote::{BLOCK_CONTAINER, BLOCK_GROUP, attr_string, elements};
use crate::guard::Shape;
use crate::structure::depth_of;

/// Au-delà de ce nombre d'horloges supprimées, les originaux se cherchent en parcourant la page
/// plutôt que le jeu de suppressions (une étape 2 porte toutes les suppressions de son client).
const MAX_SCANNED_CLOCKS: u64 = 1 << 16;

/// Un bloc recréé par une mise à jour, et ce qu'il faut reporter sur sa copie.
pub(crate) struct Recreated {
    /// L'élément de la copie, inséré par la mise à jour.
    copy: ID,
    /// Son identifiant de bloc.
    block_id: String,
    /// Le texte que l'auteur n'avait pas vu.
    text: Option<TextCarry>,
    /// Les identifiants des sous-blocs vus de l'original, dans l'ordre.
    seen_children: Vec<String>,
    /// Les sous-blocs que l'auteur n'avait pas vus.
    children: Vec<ChildCarry>,
}

/// Le texte d'un original.
struct TextCarry {
    /// La balise du nœud de contenu de l'original (`paragraph`…).
    tag: String,
    /// Les caractères que l'auteur avait vus, dans l'ordre.
    seen: Vec<char>,
    /// Ce qu'il n'avait pas vu, dans l'ordre.
    unseen: Vec<Unseen>,
}

/// Un morceau de texte non vu : il suivait `after` caractères vus.
struct Unseen {
    after: usize,
    text: String,
    attrs: Attrs,
}

/// Un sous-bloc non vu : il suivait les `after` premiers sous-blocs vus.
struct ChildCarry {
    after: usize,
    tree: Tree,
}

/// Relève, avant d'appliquer la mise à jour lue `shape`, les blocs qu'elle recrée et ce que
/// son auteur n'en avait pas vu. Vide pour une mise à jour qui ne recrée aucun bloc, ou dont
/// l'auteur avait tout vu.
///
/// `txn` est la transaction qui l'appliquera, encore intacte : la lecture du texte y découpe
/// des blocs de yrs aux bornes du jeu de suppressions, ce que l'application ferait de toute
/// façon, sans rien changer au contenu.
pub(crate) fn capture(
    txn: &mut TransactionMut,
    fragment: &XmlFragmentRef,
    shape: &Shape,
) -> Vec<Recreated> {
    let state = txn.state_vector();
    let candidates = shape.inserted_blocks(&state);
    if candidates.is_empty() {
        return Vec::new();
    }
    let (deleted, clocks) = deleted_in_page(shape, &state);
    if deleted.is_empty() {
        return Vec::new();
    }
    let wanted: HashMap<&str, ID> = candidates
        .iter()
        .map(|(id, block)| (block.as_str(), *id))
        .collect();
    let originals = if clocks <= MAX_SCANNED_CLOCKS {
        originals_in_deletions(txn, shape, &state, &wanted)
    } else {
        originals_in_page(txn, fragment, &deleted, &wanted)
    };
    let hi = Snapshot::new(state.clone(), IdSet::new());
    let lo = Snapshot::new(state, deleted.clone());
    let mut found = Vec::new();
    for (copy, block_id) in candidates {
        let Some([original]) = originals.get(block_id.as_str()).map(Vec::as_slice) else {
            continue;
        };
        let original = original.clone();
        let parts = elements(txn, &original);
        let content = parts.iter().find(|e| e.tag().as_ref() != BLOCK_GROUP);
        let group = parts.iter().find(|e| e.tag().as_ref() == BLOCK_GROUP);
        let text = match content {
            // Un nœud de contenu que l'auteur n'avait pas vu : le type a changé.
            Some(content) if covered(content, &deleted) => capture_text(txn, content, &hi, &lo),
            _ => None,
        };
        let (seen_children, children) = match group {
            Some(group) => capture_children(txn, group, &deleted),
            None => (Vec::new(), Vec::new()),
        };
        if text.is_some() || !children.is_empty() {
            found.push(Recreated {
                copy,
                block_id,
                text,
                seen_children,
                children,
            });
        }
    }
    found
}

/// Écrit dans chaque copie, la mise à jour appliquée, ce que [`capture`] a relevé. Une copie
/// absente, supprimée ou qui n'est plus ce qu'elle était ne reçoit rien.
pub(crate) fn carry(txn: &mut TransactionMut, recreated: &[Recreated]) {
    for r in recreated {
        let Some(copy) = live_element(txn, &r.copy) else {
            continue;
        };
        if copy.tag().as_ref() != BLOCK_CONTAINER || attr_string(txn, &copy, "id") != r.block_id {
            continue;
        }
        if let Some(text) = &r.text {
            carry_text(txn, &copy, text);
        }
        if !r.children.is_empty() {
            carry_children(txn, &copy, r);
        }
    }
}

/// Le jeu de suppressions de la mise à jour, borné à ce que la page connaît : seul ce qu'elle a
/// peut être un original. Rend aussi le nombre d'horloges qu'il couvre.
fn deleted_in_page(shape: &Shape, state: &StateVector) -> (IdSet, u64) {
    let mut deleted = IdSet::new();
    let mut clocks = 0u64;
    for (client, ranges) in shape.deletions() {
        let floor = state.get(&ClientID::new(*client));
        for &(start, end) in ranges {
            let end = end.min(floor);
            if start < end {
                deleted.insert(ID::new(ClientID::new(*client), start), end - start);
                clocks += u64::from(end - start);
            }
        }
    }
    (deleted, clocks)
}

/// Les `blockContainer` vivants que le jeu de suppressions couvre, et dont l'identifiant de bloc
/// est l'un de `wanted` : chaque horloge supprimée est essayée (un élément tient une horloge).
fn originals_in_deletions(
    txn: &TransactionMut,
    shape: &Shape,
    state: &StateVector,
    wanted: &HashMap<&str, ID>,
) -> HashMap<String, Vec<XmlElementRef>> {
    let mut found: HashMap<String, Vec<XmlElementRef>> = HashMap::new();
    for (client, ranges) in shape.deletions() {
        let floor = state.get(&ClientID::new(*client));
        for &(start, end) in ranges {
            for clock in start..end.min(floor) {
                let id = ID::new(ClientID::new(*client), clock);
                let Some(element) = live_element(txn, &id) else {
                    continue;
                };
                if element.tag().as_ref() != BLOCK_CONTAINER {
                    continue;
                }
                let block = attr_string(txn, &element, "id");
                if wanted.contains_key(block.as_str()) {
                    found.entry(block).or_default().push(element);
                }
            }
        }
    }
    found
}

/// Comme [`originals_in_deletions`], en parcourant la page (sans récursion).
fn originals_in_page(
    txn: &TransactionMut,
    fragment: &XmlFragmentRef,
    deleted: &IdSet,
    wanted: &HashMap<&str, ID>,
) -> HashMap<String, Vec<XmlElementRef>> {
    let mut found: HashMap<String, Vec<XmlElementRef>> = HashMap::new();
    for node in fragment.successors(txn) {
        let XmlOut::Element(element) = node else {
            continue;
        };
        if element.tag().as_ref() != BLOCK_CONTAINER || !covered(&element, deleted) {
            continue;
        }
        let block = attr_string(txn, &element, "id");
        if wanted.contains_key(block.as_str()) {
            found.entry(block).or_default().push(element);
        }
    }
    found
}

/// L'élément XML vivant dont l'élément Yjs commence à `id`.
fn live_element<T: ReadTxn>(txn: &T, id: &ID) -> Option<XmlElementRef> {
    let branch = BranchID::get_nested(txn, id)?;
    if branch.is_deleted() {
        return None;
    }
    match XmlOut::try_from(branch) {
        Ok(XmlOut::Element(element)) => Some(element),
        _ => None,
    }
}

/// Vrai si le jeu de suppressions couvre l'élément Yjs de ce type : l'auteur l'avait vu.
fn covered<B: AsRef<yrs::branch::Branch>>(node: &B, deleted: &IdSet) -> bool {
    match node.as_ref().id() {
        BranchID::Nested(id) => deleted.contains(&id),
        BranchID::Root(_) => false,
    }
}

/// Le texte du nœud de contenu d'un original, partagé entre ce que l'auteur avait vu et ce
/// qu'il n'avait pas vu. `None` s'il n'y a rien à reporter, ou si le contenu en ligne est fait
/// de plusieurs textes (mention, formule, saut de ligne) ou porte un objet incorporé.
///
/// La lecture passe par `diff_range` de yrs, entre deux instantanés de même vecteur d'état :
/// `hi` sans suppression, où tout est visible, et `lo`, où ce que le jeu de suppressions couvre
/// ne l'est pas. Un caractère vu sort marqué « ajouté », un caractère non vu sans marque ; un
/// caractère que la page a déjà supprimé ne sort pas (son contenu est ramassé). Un seul passage
/// sur les éléments du texte, sans aucune lecture des éléments supprimés (Ruling R371).
fn capture_text(
    txn: &mut TransactionMut,
    content: &XmlElementRef,
    hi: &Snapshot,
    lo: &Snapshot,
) -> Option<TextCarry> {
    let children: Vec<XmlOut> = content.children(txn).collect();
    let text = match children.as_slice() {
        [XmlOut::Text(text)] => text.clone(),
        // Aucun texte : rien à reporter. Plusieurs, ou un élément : pas le cas simple.
        _ => return None,
    };
    let mut seen = Vec::new();
    let mut unseen = Vec::new();
    for diff in text.diff_range(txn, Some(hi), Some(lo), YChange::identity) {
        let Out::Any(Any::String(s)) = diff.insert else {
            return None;
        };
        match diff.ychange {
            Some(YChange {
                kind: ChangeKind::Added,
                ..
            }) => seen.extend(s.chars()),
            None => unseen.push(Unseen {
                after: seen.len(),
                text: s.to_string(),
                attrs: diff.attributes.map(|a| *a).unwrap_or_default(),
            }),
            Some(_) => return None,
        }
    }
    (!unseen.is_empty()).then(|| TextCarry {
        tag: content.tag().to_string(),
        seen,
        unseen,
    })
}

/// Les sous-blocs d'un original : les identifiants de ceux que l'auteur avait vus, et ceux
/// qu'il n'avait pas vus, recopiés. Un groupe de sous-blocs qu'il n'avait pas vu (l'original
/// n'en avait pas) n'a que des sous-blocs non vus.
fn capture_children(
    txn: &TransactionMut,
    group: &XmlElementRef,
    deleted: &IdSet,
) -> (Vec<String>, Vec<ChildCarry>) {
    let group_seen = covered(group, deleted);
    let mut seen = Vec::new();
    let mut unseen = Vec::new();
    for child in elements(txn, group) {
        if group_seen && covered(&child, deleted) {
            seen.push(attr_string(txn, &child, "id"));
        } else {
            unseen.push(ChildCarry {
                after: seen.len(),
                tree: Tree::of(txn, &child),
            });
        }
    }
    (seen, unseen)
}

/// Écrit dans la copie le texte non vu dont la place est sûre.
fn carry_text(txn: &mut TransactionMut, copy: &XmlElementRef, text: &TextCarry) {
    let Some(content) = elements(txn, copy)
        .into_iter()
        .find(|e| e.tag().as_ref() != BLOCK_GROUP)
    else {
        return;
    };
    // Le type a changé chez l'auteur du déplacement : pas le cas simple.
    if content.tag().as_ref() != text.tag {
        return;
    }
    let children: Vec<XmlOut> = content.children(txn).collect();
    let (node, chars): (Option<XmlTextRef>, Vec<char>) = match children.as_slice() {
        [] => (None, Vec::new()),
        [XmlOut::Text(node)] => {
            let mut chars = Vec::new();
            for diff in node.diff(txn, YChange::identity) {
                match diff.insert {
                    Out::Any(Any::String(s)) => chars.extend(s.chars()),
                    _ => return,
                }
            }
            (Some(node.clone()), chars)
        }
        _ => return,
    };
    let Some(places) = Places::of(&text.seen, &chars) else {
        return;
    };
    let inserts: Vec<(u32, &Unseen)> = text
        .unseen
        .iter()
        .filter_map(|piece| {
            let at = places.sure(piece.after)?;
            let utf16 = chars[..at].iter().map(|c| c.len_utf16() as u32).sum();
            Some((utf16, piece))
        })
        .collect();
    if inserts.is_empty() {
        return;
    }
    let node = node.unwrap_or_else(|| content.insert(txn, 0, XmlTextPrelim::new("")));
    // Du dernier au premier : les places relevées restent justes, et deux morceaux à la même
    // place gardent leur ordre.
    for (at, piece) in inserts.into_iter().rev() {
        node.insert_with_attributes(txn, at, &piece.text, piece.attrs.clone());
    }
}

/// Écrit sous la copie les sous-blocs non vus : chacun après la copie du dernier sous-bloc vu
/// qui le précédait et qu'elle porte, sinon en tête ; ceux qui se suivaient restent ensemble,
/// dans leur ordre.
fn carry_children(txn: &mut TransactionMut, copy: &XmlElementRef, r: &Recreated) {
    let mut group = elements(txn, copy)
        .into_iter()
        .find(|e| e.tag().as_ref() == BLOCK_GROUP);
    let mut cursor: Option<(usize, u32)> = None;
    for child in &r.children {
        let base = match &group {
            Some(group) => depth_of(group),
            None => depth_of(copy) + 1,
        };
        if base + child.tree.height > MAX_YJS_DEPTH {
            continue;
        }
        let group = match &group {
            Some(group) => group.clone(),
            None => {
                let created = copy.push_back(txn, XmlElementPrelim::empty(BLOCK_GROUP));
                group = Some(created.clone());
                created
            }
        };
        let index = match cursor {
            Some((after, index)) if after == child.after => index,
            _ => index_after(txn, &group, &r.seen_children[..child.after]),
        };
        child.tree.write(txn, &group, index);
        cursor = Some((child.after, index + 1));
    }
}

/// La place juste après le dernier de `seen` que porte `group`, sinon la tête.
fn index_after<T: ReadTxn>(txn: &T, group: &XmlElementRef, seen: &[String]) -> u32 {
    let children = elements(txn, group);
    for id in seen.iter().rev().filter(|id| !id.is_empty()) {
        if let Some(at) = children
            .iter()
            .position(|c| attr_string(txn, c, "id") == *id)
        {
            return at as u32 + 1;
        }
    }
    0
}

/// Où vont, dans le texte d'une copie, les caractères qu'un original avait autour de ce qui y
/// est reporté (Ruling R373).
///
/// Les caractères vus doivent se retrouver dans la copie, dans l'ordre, sans en manquer : la
/// copie peut en avoir d'autres (une suppression faite par quelqu'un d'autre et que l'auteur
/// n'avait pas vue, ou ce que l'auteur a tapé lui-même avant de déplacer), jamais moins. Toutes
/// les façons de les y retrouver sont bornées par la plus à gauche et la plus à droite.
/// Un morceau non vu a une place **sûre** quand ses deux voisins vus sont chacun à une seule
/// place possible, côte à côte : rien d'autre ne les sépare dans la copie, et le morceau va
/// entre eux. En tête (aucun voisin à gauche), le premier caractère vu doit être le premier de
/// la copie ; en fin, le dernier, le dernier. Sinon le morceau n'est pas reporté.
struct Places {
    left: Vec<usize>,
    right: Vec<usize>,
    len: usize,
}

impl Places {
    /// `None` si les caractères vus ne se retrouvent pas tous dans la copie, dans l'ordre.
    fn of(seen: &[char], copy: &[char]) -> Option<Places> {
        let mut left = Vec::with_capacity(seen.len());
        let mut j = 0;
        for &c in seen {
            while j < copy.len() && copy[j] != c {
                j += 1;
            }
            if j == copy.len() {
                return None;
            }
            left.push(j);
            j += 1;
        }
        let mut right = vec![0; seen.len()];
        let mut j = copy.len();
        for (k, &c) in seen.iter().enumerate().rev() {
            loop {
                if j == 0 {
                    return None;
                }
                j -= 1;
                if copy[j] == c {
                    break;
                }
            }
            right[k] = j;
        }
        Some(Places {
            left,
            right,
            len: copy.len(),
        })
    }

    /// La place sûre, en caractères de la copie, d'un morceau qui suivait `after` caractères
    /// vus.
    fn sure(&self, after: usize) -> Option<usize> {
        let n = self.left.len();
        if n == 0 {
            return (self.len == 0).then_some(0);
        }
        if after == 0 {
            return (self.right[0] == 0).then_some(0);
        }
        if after >= n {
            return (self.left[n - 1] + 1 == self.len).then_some(self.len);
        }
        let (a, b) = (after - 1, after);
        (self.left[a] == self.right[a]
            && self.left[b] == self.right[b]
            && self.left[b] == self.left[a] + 1)
            .then_some(self.left[b])
    }
}

/// Un sous-arbre recopié, relevé avant que l'application de la mise à jour ne le supprime.
struct Tree {
    nodes: Vec<Node>,
    /// Sa hauteur en éléments Yjs, textes compris.
    height: usize,
}

struct Node {
    tag: String,
    attrs: Vec<(String, Any)>,
    parts: Vec<Part>,
}

enum Part {
    Element(usize),
    /// Les plages d'un texte : une chaîne ou un objet incorporé, et ses marques.
    Text(Vec<(Any, Attrs)>),
}

impl Tree {
    /// Relève le sous-arbre `root`, sans récursion.
    fn of<T: ReadTxn>(txn: &T, root: &XmlElementRef) -> Tree {
        let mut nodes = vec![Node {
            tag: root.tag().to_string(),
            attrs: Vec::new(),
            parts: Vec::new(),
        }];
        let mut height = 1;
        let mut stack = vec![(root.clone(), 0usize, 1usize)];
        while let Some((element, index, depth)) = stack.pop() {
            nodes[index].attrs = element
                .attributes(txn)
                .filter_map(|(key, value)| match value {
                    Out::Any(value) => Some((key.to_string(), value)),
                    _ => None,
                })
                .collect();
            for child in element.children(txn) {
                match child {
                    XmlOut::Element(e) => {
                        let k = nodes.len();
                        nodes.push(Node {
                            tag: e.tag().to_string(),
                            attrs: Vec::new(),
                            parts: Vec::new(),
                        });
                        nodes[index].parts.push(Part::Element(k));
                        height = height.max(depth + 1);
                        stack.push((e, k, depth + 1));
                    }
                    XmlOut::Text(text) => {
                        let runs = text
                            .diff(txn, YChange::identity)
                            .into_iter()
                            .filter_map(|d| match d.insert {
                                Out::Any(value) => {
                                    Some((value, d.attributes.map(|a| *a).unwrap_or_default()))
                                }
                                // Un type incorporé est perdu, comme dans `copy_tree`.
                                _ => None,
                            })
                            .collect();
                        nodes[index].parts.push(Part::Text(runs));
                        height = height.max(depth + 1);
                    }
                    XmlOut::Fragment(_) => {}
                }
            }
        }
        Tree { nodes, height }
    }

    /// L'écrit à l'index `at` de `parent`, sans récursion.
    fn write(&self, txn: &mut TransactionMut, parent: &XmlElementRef, at: u32) {
        let root = parent.insert(txn, at, XmlElementPrelim::empty(self.nodes[0].tag.as_str()));
        let mut stack = vec![(0usize, root)];
        while let Some((k, to)) = stack.pop() {
            let node = &self.nodes[k];
            for (key, value) in &node.attrs {
                to.insert_attribute(txn, key.as_str(), value.clone());
            }
            for part in &node.parts {
                match part {
                    Part::Element(c) => {
                        let element =
                            to.push_back(txn, XmlElementPrelim::empty(self.nodes[*c].tag.as_str()));
                        stack.push((*c, element));
                    }
                    Part::Text(runs) => {
                        let text = to.push_back(txn, XmlTextPrelim::new(""));
                        for (value, attrs) in runs {
                            let at = text.len(txn);
                            match value {
                                Any::String(s) => {
                                    text.insert_with_attributes(txn, at, s, attrs.clone());
                                }
                                other => {
                                    text.insert_embed_with_attributes(
                                        txn,
                                        at,
                                        other.clone(),
                                        attrs.clone(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Places;

    fn sure(seen: &str, copy: &str, after: usize) -> Option<usize> {
        let seen: Vec<char> = seen.chars().collect();
        let copy: Vec<char> = copy.chars().collect();
        Places::of(&seen, &copy)?.sure(after)
    }

    #[test]
    fn same_text_gives_every_place() {
        for after in 0..=6 {
            assert_eq!(sure("Bloc X", "Bloc X", after), Some(after));
        }
    }

    #[test]
    fn a_missing_seen_character_gives_no_place() {
        assert_eq!(sure("Bloc X", "Boc X", 6), None);
        assert_eq!(sure("Bloc X", "", 0), None);
    }

    /// Une suppression faite par un autre (« oc »), que l'auteur avait vue en place : la fin
    /// reste sûre, l'endroit de la suppression non.
    #[test]
    fn extra_characters_only_blur_their_own_place() {
        // Caractères vus : « Bl X » ; la copie a encore « oc ».
        assert_eq!(sure("Bl X", "Bloc X", 4), Some(6));
        assert_eq!(sure("Bl X", "Bloc X", 0), Some(0));
        assert_eq!(sure("Bl X", "Bloc X", 1), Some(1));
        assert_eq!(sure("Bl X", "Bloc X", 2), None);
        assert_eq!(sure("Bl X", "Bloc X", 3), Some(5));
    }

    /// Des places possibles qui varient : aucune n'est sûre, même en tête ou en fin (« ab »
    /// peut être les deux premiers caractères de « abab » ou les deux derniers).
    #[test]
    fn a_seen_character_with_two_possible_places_is_not_sure() {
        assert_eq!(sure("ab", "aab", 1), None);
        assert_eq!(sure("ab", "aab", 0), None);
        assert_eq!(sure("ab", "aab", 2), Some(3));
        assert_eq!(sure("ab", "abab", 0), None);
        assert_eq!(sure("ab", "abab", 1), None);
        assert_eq!(sure("ab", "abab", 2), None);
    }

    #[test]
    fn nothing_seen_is_sure_only_in_an_empty_copy() {
        assert_eq!(sure("", "", 0), Some(0));
        assert_eq!(sure("", "abc", 0), None);
    }
}
