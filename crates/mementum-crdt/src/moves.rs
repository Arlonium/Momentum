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
use std::sync::atomic::{AtomicU64, Ordering};

use yrs::types::Attrs;
use yrs::types::text::{ChangeKind, YChange};
use yrs::{
    Any, BranchID, ClientID, ID, IdSet, Out, ReadTxn, Snapshot, StateVector, Text, TransactionMut,
    Xml, XmlElementPrelim, XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut, XmlTextPrelim,
    XmlTextRef,
};

use crate::MAX_YJS_DEPTH;
use crate::blocknote::{BLOCK_CONTAINER, BLOCK_GROUP, COLUMN, attr_string, elements};
use crate::guard::Shape;
use crate::structure::depth_of;

/// Au-delà de ce nombre d'horloges supprimées, les originaux se cherchent en parcourant la page
/// plutôt que le jeu de suppressions (une étape 2 porte toutes les suppressions de son client).
const MAX_SCANNED_CLOCKS: u64 = 1 << 16;

/// Au plus ce que le report d'une mise à jour recopie de sous-blocs (Ruling R451) : au-delà, il
/// n'écrit rien. Un client honnête ne fait reporter que ce que d'autres ont mis dans le bloc
/// depuis qu'il l'a vu ; sans borne, une copie vide de quelques dizaines d'octets faisait
/// recopier, écrire et diffuser tout un sous-arbre, autant de fois que voulu.
pub(crate) const MAX_CARRIED_BLOCKS: usize = 64;

/// Au plus ce que le report d'une mise à jour écrit, en octets (Ruling R451) : estimé avant
/// d'écrire, puis mesuré sur la mise à jour encodée (`PageDoc::apply_client_update`).
pub(crate) const MAX_CARRIED_BYTES: usize = 64 * 1024;

/// Les voisins vus que la place d'un morceau reporté demande de chaque côté, chacun à une seule
/// place possible dans la copie et contigus (Ruling R457).
const CONTEXT: usize = 3;

/// Ce qu'ajoute au plus une écriture encodée (v1), hors de ses éléments : l'en-tête de son client
/// (nombre d'éléments, client, première horloge), le nombre de clients, le jeu de suppressions
/// vide (Ruling R458).
const UPDATE_BYTES: usize = 32;

/// Le coût d'une écriture en éléments de yrs et en octets de contenu ; ce qu'elle pèse encodée
/// au plus vaut `items × en-tête + payload` ([`item_bytes`], Ruling R458).
#[derive(Clone, Copy, Default)]
struct Cost {
    items: usize,
    payload: usize,
}

impl Cost {
    fn add(self, other: Cost) -> Cost {
        Cost {
            items: self.items + other.items,
            payload: self.payload + other.payload,
        }
    }

    fn times(self, n: usize) -> Cost {
        Cost {
            items: self.items * n,
            payload: self.payload * n,
        }
    }

    fn bytes(self, item: usize) -> usize {
        self.items * item + self.payload
    }
}

/// Le plus que pèse, encodé (v1), l'en-tête d'un élément écrit par le serveur dans une page dont
/// le vecteur d'état est `state` (Ruling R458) : la marque (1 octet), une origine et une origine
/// droite, chacune un client (au plus 8 octets, 53 bits) et une horloge (au plus ce que demande
/// la plus grande horloge de la page, plus ce que le report peut ajouter), et la longueur du
/// contenu (5 octets). Sans origine, le parent tient dans la même place.
fn item_bytes(state: &StateVector) -> usize {
    let clock = state.iter().map(|(_, clock)| *clock).max().unwrap_or(0);
    let clock = u64::from(clock) + MAX_CARRIED_BYTES as u64;
    let clock_bytes = (64 - clock.leading_zeros() as usize).div_ceil(7).max(1);
    1 + 2 * (8 + clock_bytes) + 5
}

/// Ce que [`carry`] a fait.
pub(crate) enum Carried {
    /// Rien à écrire.
    Nothing,
    /// Écrit dans ces copies : `(identifiant de bloc, élément de la copie)`, et la borne haute de
    /// ce que l'écriture pèse encodée (Ruling R458).
    Written(Vec<(String, ID)>, usize),
    /// Rien d'écrit : le report dépasserait ses bornes (Ruling R451). La raison, pour le journal.
    Withheld(String),
}

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
    /// Pour chaque caractère vu, les horloges qui manquent entre lui et le caractère vu d'avant,
    /// quand tous deux viennent du même client (sinon 0) : des caractères que la page a supprimés
    /// entre eux, que la copie peut encore porter (Ruling R457).
    gaps: Vec<u32>,
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
/// des blocs de yrs aux bornes du jeu de suppressions, une fois pour tous les blocs recréés
/// ([`Items`]), ce que l'application ferait de toute façon, sans rien changer au contenu.
///
/// `walks` compte les parcours de la page faits pour trouver les originaux (pour les tests).
pub(crate) fn capture(
    txn: &mut TransactionMut,
    fragment: &XmlFragmentRef,
    shape: &Shape,
    walks: &AtomicU64,
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
        walks.fetch_add(1, Ordering::SeqCst);
        originals_in_page(txn, fragment, &deleted, &wanted)
    };
    let mut items = Items::new(state, &deleted);
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
            Some(content) if covered(content, &deleted) => {
                capture_text(txn, content, &mut items, &deleted)
            }
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

/// Écrit dans chaque copie, la mise à jour appliquée, ce que [`capture`] a relevé, en trois
/// temps : ce qui s'écrirait, copie par copie ; les règles qui portent sur tout le report ; puis
/// l'écriture, ou rien.
///
/// - Une copie absente, supprimée, qui n'est plus ce qu'elle était, ou qui n'est pas posée dans
///   un groupe de blocs ou une colonne (Ruling R452), ne reçoit rien.
/// - Rien ne s'écrit au-delà de [`MAX_YJS_DEPTH`] : ni un texte neuf, ni un sous-bloc (R452).
/// - Un morceau de texte n'est reporté qu'à une place sûre, entourée de voisins vus contigus
///   (Rulings R373, R457).
/// - Au-delà de [`MAX_CARRIED_BLOCKS`] sous-blocs, ou si la borne haute de ce que l'écriture pèse
///   dépasse [`MAX_CARRIED_BYTES`] octets, rien ne s'écrit (Rulings R451, R458).
pub(crate) fn carry(txn: &mut TransactionMut, recreated: &[Recreated]) -> Carried {
    let mut plans: Vec<Plan> = Vec::new();
    for r in recreated {
        let Some(copy) = copy_of(txn, r) else {
            continue;
        };
        let text = r.text.as_ref().and_then(|text| plan_text(txn, &copy, text));
        let children = plan_children(txn, &copy, r);
        if text.is_some() || !children.trees.is_empty() {
            plans.push(Plan {
                recreated: r,
                copy,
                text,
                children,
            });
        }
    }
    let blocks: usize = plans.iter().map(|p| p.children.blocks()).sum();
    let item = item_bytes(&txn.state_vector());
    let bytes: usize = UPDATE_BYTES + plans.iter().map(|p| p.cost().bytes(item)).sum::<usize>();
    if blocks > MAX_CARRIED_BLOCKS || bytes > MAX_CARRIED_BYTES {
        return Carried::Withheld(format!(
            "report de {blocks} sous-blocs et d'au plus {bytes} octets, au-delà de \
             {MAX_CARRIED_BLOCKS} sous-blocs ou {MAX_CARRIED_BYTES} octets"
        ));
    }
    let mut written = Vec::new();
    for plan in plans {
        let r = plan.recreated;
        if plan.write(txn) {
            written.push((r.block_id.clone(), r.copy));
        }
    }
    if written.is_empty() {
        Carried::Nothing
    } else {
        Carried::Written(written, bytes)
    }
}

/// La copie d'un bloc recréé, si elle reçoit un report : vivante, `blockContainer`, toujours à
/// l'identifiant relevé, posée dans un groupe de blocs ou une colonne, comme BlockNote les pose
/// (Ruling R452 : une copie posée ailleurs, au fond d'une chaîne d'éléments, ne reçoit rien).
fn copy_of<T: ReadTxn>(txn: &T, r: &Recreated) -> Option<XmlElementRef> {
    let copy = live_element(txn, &r.copy)?;
    if copy.tag().as_ref() != BLOCK_CONTAINER || attr_string(txn, &copy, "id") != r.block_id {
        return None;
    }
    match copy.parent() {
        Some(XmlOut::Element(parent)) if matches!(parent.tag().as_ref(), BLOCK_GROUP | COLUMN) => {
            Some(copy)
        }
        _ => None,
    }
}

/// Ce qui s'écrirait dans une copie.
struct Plan<'a> {
    recreated: &'a Recreated,
    copy: XmlElementRef,
    text: Option<TextPlan>,
    children: ChildrenPlan<'a>,
}

impl Plan<'_> {
    /// Le coût de l'écriture, par le haut (Ruling R458) : par morceau, un élément de texte et,
    /// par marque, deux éléments de marque (l'ouverture et la fermeture), ceux du morceau et ceux
    /// qui défont puis rétablissent les marques de la copie autour de lui ; un texte ou un groupe
    /// à créer ; les sous-blocs ([`Tree::cost`]).
    fn cost(&self) -> Cost {
        let element = Cost {
            items: 1,
            payload: 6,
        };
        let mut cost = Cost::default();
        if let Some(t) = &self.text {
            if t.node.is_none() {
                cost = cost.add(element);
            }
            for (_, piece) in &t.inserts {
                cost = cost
                    .add(Cost {
                        items: 1,
                        payload: piece.text.len(),
                    })
                    .add(format_cost(&piece.attrs).times(2))
                    .add(t.copy_marks.times(2));
            }
        }
        if self.children.group.is_none() && !self.children.trees.is_empty() {
            cost = cost.add(element).add(Cost {
                items: 0,
                payload: BLOCK_GROUP.len(),
            });
        }
        self.children
            .trees
            .iter()
            .fold(cost, |cost, c| cost.add(c.tree.cost))
    }

    /// L'écrit ; vrai si quelque chose l'a été.
    fn write(self, txn: &mut TransactionMut) -> bool {
        let mut wrote = false;
        if let Some(text) = self.text {
            let node = text
                .node
                .unwrap_or_else(|| text.content.insert(txn, 0, XmlTextPrelim::new("")));
            // Du dernier au premier : les places relevées restent justes, et deux morceaux à la
            // même place gardent leur ordre.
            for (at, piece) in text.inserts.into_iter().rev() {
                node.insert_with_attributes(txn, at, &piece.text, piece.attrs.clone());
            }
            wrote = true;
        }
        if !self.children.trees.is_empty() {
            let group = match self.children.group {
                Some(group) => group,
                None => self
                    .copy
                    .push_back(txn, XmlElementPrelim::empty(BLOCK_GROUP)),
            };
            let seen = &self.recreated.seen_children;
            let mut cursor: Option<(usize, u32)> = None;
            for child in self.children.trees {
                let index = match cursor {
                    Some((after, index)) if after == child.after => index,
                    _ => index_after(txn, &group, &seen[..child.after]),
                };
                child.tree.write(txn, &group, index);
                cursor = Some((child.after, index + 1));
            }
            wrote = true;
        }
        wrote
    }
}

/// Le texte qui s'écrirait dans une copie.
struct TextPlan {
    content: XmlElementRef,
    /// Le texte de la copie ; `None` : il est à créer.
    node: Option<XmlTextRef>,
    /// Chaque morceau dont la place est sûre, à sa place en UTF-16.
    inserts: Vec<(u32, Unseen)>,
    /// Le coût d'un élément de marque par marque du texte de la copie : un morceau inséré au
    /// milieu d'elles peut devoir les défaire puis les rétablir (Ruling R458).
    copy_marks: Cost,
}

/// Le texte non vu dont la place est sûre dans la copie, et ce qui s'écrirait. `None` si rien ne
/// s'écrit : type changé, contenu à plusieurs textes, aucune place sûre, ou texte neuf au-delà de
/// la borne de profondeur (Ruling R452).
fn plan_text(txn: &TransactionMut, copy: &XmlElementRef, text: &TextCarry) -> Option<TextPlan> {
    let content = elements(txn, copy)
        .into_iter()
        .find(|e| e.tag().as_ref() != BLOCK_GROUP)?;
    // Le type a changé chez l'auteur du déplacement : pas le cas simple (Ruling R377).
    if content.tag().as_ref() != text.tag {
        return None;
    }
    let children: Vec<XmlOut> = content.children(txn).collect();
    let mut marks: HashMap<String, Cost> = HashMap::new();
    let (node, chars): (Option<XmlTextRef>, Vec<char>) = match children.as_slice() {
        [] => (None, Vec::new()),
        [XmlOut::Text(node)] => {
            let mut chars = Vec::new();
            for diff in node.diff(txn, YChange::identity) {
                match diff.insert {
                    Out::Any(Any::String(s)) => chars.extend(s.chars()),
                    _ => return None,
                }
                for (key, value) in diff.attributes.iter().flat_map(|a| a.iter()) {
                    let cost = format_cost_of(key, value);
                    let known = marks.entry(key.to_string()).or_default();
                    known.items = 1;
                    known.payload = known.payload.max(cost.payload);
                }
            }
            (Some(node.clone()), chars)
        }
        _ => return None,
    };
    if node.is_none() && depth_of(&content) + 1 > MAX_YJS_DEPTH {
        return None;
    }
    let places = Places::of(&text.seen, &chars)?;
    let inserts: Vec<(u32, Unseen)> = text
        .unseen
        .iter()
        .filter_map(|piece| {
            let at = places.sure_in_context(piece.after, CONTEXT, &text.gaps, &chars)?;
            let utf16 = chars[..at].iter().map(|c| c.len_utf16() as u32).sum();
            Some((
                utf16,
                Unseen {
                    after: piece.after,
                    text: piece.text.clone(),
                    attrs: piece.attrs.clone(),
                },
            ))
        })
        .collect();
    if inserts.is_empty() {
        return None;
    }
    Some(TextPlan {
        content,
        node,
        inserts,
        copy_marks: marks.values().fold(Cost::default(), |a, b| a.add(*b)),
    })
}

/// Les sous-blocs qui s'écriraient sous une copie.
struct ChildrenPlan<'a> {
    /// Le groupe de sous-blocs de la copie ; `None` : il est à créer.
    group: Option<XmlElementRef>,
    trees: Vec<&'a ChildCarry>,
}

impl ChildrenPlan<'_> {
    fn blocks(&self) -> usize {
        self.trees.iter().map(|c| c.tree.blocks).sum()
    }
}

/// Les sous-blocs non vus qui tiennent sous la borne de profondeur (Ruling R374).
fn plan_children<'a>(
    txn: &TransactionMut,
    copy: &XmlElementRef,
    r: &'a Recreated,
) -> ChildrenPlan<'a> {
    let group = elements(txn, copy)
        .into_iter()
        .find(|e| e.tag().as_ref() == BLOCK_GROUP);
    let base = match &group {
        Some(group) => depth_of(group),
        None => depth_of(copy) + 1,
    };
    let trees = r
        .children
        .iter()
        .filter(|child| base + child.tree.height <= MAX_YJS_DEPTH)
        .collect();
    ChildrenPlan { group, trees }
}

/// Le coût des éléments de marque d'un jeu de marques, par le haut : un élément par marque
/// (Ruling R458).
fn format_cost(attrs: &Attrs) -> Cost {
    attrs.iter().fold(Cost::default(), |cost, (k, v)| {
        cost.add(format_cost_of(k, v))
    })
}

/// Le coût d'un élément de marque `key` = `value` : la clé et la valeur en JSON, chacune avec sa
/// longueur.
fn format_cost_of(key: &str, value: &Any) -> Cost {
    Cost {
        items: 1,
        payload: 5 + key.len() + 5 + json_bytes(value),
    }
}

/// Vrai si `deleted` couvre toutes les horloges `[start, end)` de `client`.
fn covers(deleted: &IdSet, client: ClientID, start: u32, end: u32) -> bool {
    let mut ranges: Vec<(u32, u32)> = deleted
        .iter()
        .filter(|(c, _)| **c == client)
        .flat_map(|(_, ranges)| ranges.iter().map(|r| (r.start, r.end)))
        .collect();
    ranges.sort_unstable();
    let mut reached = start;
    for (from, to) in ranges {
        if from > reached {
            break;
        }
        reached = reached.max(to);
        if reached >= end {
            return true;
        }
    }
    reached >= end
}

/// Une borne haute de la longueur d'une valeur encodée en `Any` de lib0 (une marque de type, la
/// longueur en tête, le contenu), pour un attribut d'élément (Ruling R504).
fn any_bytes(value: &Any) -> usize {
    match value {
        Any::String(s) => s.len() + 6,
        Any::Buffer(b) => b.len() + 6,
        Any::Array(values) => values.iter().map(any_bytes).sum::<usize>() + 6,
        Any::Map(map) => {
            map.iter()
                .map(|(k, v)| k.len() + 5 + any_bytes(v))
                .sum::<usize>()
                + 6
        }
        _ => 10,
    }
}

/// Une borne haute de la longueur d'une valeur écrite en JSON (une chaîne peut voir chaque
/// caractère échappé en six) : une valeur de marque, un objet incorporé.
fn json_bytes(value: &Any) -> usize {
    match value {
        Any::String(s) => 6 * s.len() + 7,
        Any::Buffer(b) => 6 * b.len() + 7,
        Any::Array(values) => values.iter().map(|v| json_bytes(v) + 1).sum::<usize>() + 7,
        Any::Map(map) => {
            map.iter()
                .map(|(k, v)| 6 * k.len() + 8 + json_bytes(v))
                .sum::<usize>()
                + 7
        }
        _ => 32,
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

/// La lecture d'un texte élément par élément, chacun avec son identifiant (Ruling R371).
///
/// Elle passe par `diff_range` de yrs, entre deux instantanés : `all`, au vecteur d'état de la
/// page et sans suppression, où tout est visible, et `none`, au vecteur d'état vide, où rien ne
/// l'est. Chaque élément de texte vivant sort alors seul, marqué « ajouté », avec l'identifiant
/// de son premier caractère ; un élément que la page a déjà supprimé ne sort pas (son contenu
/// est ramassé). Un élément peut porter des caractères vus et d'autres non (des horloges qui se
/// suivent, du même client) : la première lecture découpe donc d'abord les blocs de yrs aux
/// bornes du jeu de suppressions, une fois pour toute la page, par un `diff_range` dont
/// l'instantané porte ce jeu. Chaque lecture ne parcourt ensuite que les éléments de son texte :
/// découper à chaque texte parcourait chaque fois tous les éléments supprimés (un groupe de 2 000
/// blocs déplacés sur une page de 10 000 : 4 s au lieu de 0,4 s).
struct Items {
    all: Snapshot,
    none: Snapshot,
    /// Le découpage aux bornes du jeu de suppressions, tant qu'il reste à faire.
    split: Option<Snapshot>,
}

impl Items {
    fn new(state: StateVector, deleted: &IdSet) -> Items {
        Items {
            all: Snapshot::new(state.clone(), IdSet::new()),
            none: Snapshot::new(StateVector::default(), IdSet::new()),
            split: Some(Snapshot::new(state, deleted.clone())),
        }
    }

    /// Les éléments vivants de `text`, dans l'ordre : leur identifiant, leur contenu et leurs
    /// marques.
    fn of(&mut self, txn: &mut TransactionMut, text: &XmlTextRef) -> Vec<(Option<ID>, Out, Attrs)> {
        if let Some(split) = self.split.take() {
            text.diff_range(txn, Some(&split), None, YChange::identity);
        }
        text.diff_range(txn, Some(&self.all), Some(&self.none), YChange::identity)
            .into_iter()
            .map(|diff| {
                let id = match diff.ychange {
                    Some(YChange {
                        kind: ChangeKind::Added,
                        id,
                    }) => Some(id),
                    _ => None,
                };
                (
                    id,
                    diff.insert,
                    diff.attributes.map(|a| *a).unwrap_or_default(),
                )
            })
            .collect()
    }
}

/// Le texte du nœud de contenu d'un original, partagé entre ce que l'auteur avait vu (ce que
/// le jeu de suppressions `deleted` couvre) et ce qu'il n'avait pas vu. `None` s'il n'y a rien à
/// reporter, ou si le contenu en ligne est fait de plusieurs textes (mention, formule, saut de
/// ligne) ou porte un objet incorporé.
fn capture_text(
    txn: &mut TransactionMut,
    content: &XmlElementRef,
    items: &mut Items,
    deleted: &IdSet,
) -> Option<TextCarry> {
    let children: Vec<XmlOut> = content.children(txn).collect();
    let text = match children.as_slice() {
        [XmlOut::Text(text)] => text.clone(),
        // Aucun texte : rien à reporter. Plusieurs, ou un élément : pas le cas simple.
        _ => return None,
    };
    let mut seen = Vec::new();
    let mut gaps = Vec::new();
    let mut unseen: Vec<Unseen> = Vec::new();
    // Le client et l'horloge qui suivent le dernier caractère vu.
    let mut after_seen: Option<(ClientID, u32)> = None;
    for (id, insert, attrs) in items.of(txn, &text) {
        let (Some(id), Out::Any(Any::String(s))) = (id, insert) else {
            return None;
        };
        if deleted.contains(&id) {
            // Ruling R503 : un trou d'horloges entre deux caractères vus du même client n'est
            // tenu pour « supprimé par un autre pendant le déplacement » que si le jeu de
            // suppressions de la mise à jour en couvre toutes les horloges : l'auteur du
            // déplacement les avait vues vivantes. Une horloge qui manque parce que l'auteur du
            // texte a tapé ailleurs, ou parce qu'elle a été supprimée avant ce que l'auteur du
            // déplacement a vu, n'est pas dans ce jeu (sauf dans une étape 2, qui porte toutes
            // ses suppressions : limite affirmée).
            let gap = match after_seen {
                Some((client, clock))
                    if client == id.client
                        && id.clock > clock
                        && covers(deleted, client, clock, id.clock) =>
                {
                    id.clock - clock
                }
                _ => 0,
            };
            for (k, c) in s.chars().enumerate() {
                seen.push(c);
                gaps.push(if k == 0 { gap } else { 0 });
            }
            let len: u32 = s.encode_utf16().count() as u32;
            after_seen = Some((id.client, id.clock + len));
            continue;
        }
        // Deux éléments non vus qui se suivent, aux mêmes marques, font un seul morceau.
        match unseen.last_mut() {
            Some(last) if last.after == seen.len() && last.attrs == attrs => last.text.push_str(&s),
            _ => unseen.push(Unseen {
                after: seen.len(),
                text: s.to_string(),
                attrs,
            }),
        }
    }
    (!unseen.is_empty()).then(|| TextCarry {
        tag: content.tag().to_string(),
        seen,
        gaps,
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

    /// [`Places::sure`], durcie (Ruling R457) : les `k` voisins vus de chaque côté du morceau
    /// (moins au bord du texte) ont chacun une seule place possible dans la copie, et s'y suivent
    /// sans rien entre eux, sauf, entre deux voisins, exactement ce que la page a supprimé entre
    /// eux (`gaps`, en UTF-16) : la copie de l'auteur porte encore ce qu'un autre a effacé pendant
    /// le déplacement. Les deux voisins immédiats, eux, se suivent sans rien entre eux.
    /// Une faute corrigée (un caractère vu effacé) puis un mot ajouté par l'auteur du déplacement
    /// faisaient lire la place sur un caractère pareil plus loin : ce contexte en écarte la plupart.
    fn sure_in_context(
        &self,
        after: usize,
        k: usize,
        gaps: &[u32],
        copy: &[char],
    ) -> Option<usize> {
        let at = self.sure(after)?;
        let n = self.left.len();
        let (lo, hi) = (after.saturating_sub(k), (after + k).min(n));
        if (lo..hi).any(|i| self.left[i] != self.right[i]) {
            return None;
        }
        for i in lo..hi.saturating_sub(1) {
            let (from, to) = (self.left[i], self.left[i + 1]);
            if to == from + 1 {
                continue;
            }
            let between: u32 = copy[from + 1..to]
                .iter()
                .map(|c| c.len_utf16() as u32)
                .sum();
            if gaps.get(i + 1) != Some(&between) {
                return None;
            }
        }
        Some(at)
    }
}

/// Un sous-arbre recopié, relevé avant que l'application de la mise à jour ne le supprime.
struct Tree {
    nodes: Vec<Node>,
    /// Sa hauteur en éléments Yjs, textes compris.
    height: usize,
    /// Le nombre de blocs (`blockContainer`) qu'il porte.
    blocks: usize,
    /// Le coût de son écriture, par le haut (Ruling R458).
    cost: Cost,
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
        let blocks = nodes.iter().filter(|n| n.tag == BLOCK_CONTAINER).count();
        // Une borne haute (Ruling R458) : un élément par nœud et par attribut ; par texte, un
        // élément, puis par plage un élément et, par marque, de quoi l'ouvrir et la fermer, et
        // défaire celles de la plage d'avant.
        let mut cost = Cost::default();
        for n in &nodes {
            cost = cost.add(Cost {
                items: 1,
                payload: 6 + n.tag.len(),
            });
            for (key, value) in &n.attrs {
                cost = cost.add(Cost {
                    items: 1,
                    payload: 5 + key.len() + 1 + any_bytes(value),
                });
            }
            for part in &n.parts {
                let Part::Text(runs) = part else { continue };
                cost = cost.add(Cost {
                    items: 1,
                    payload: 1,
                });
                for (value, attrs) in runs {
                    cost = cost
                        .add(Cost {
                            items: 1,
                            // Ruling R504 : une plage de texte s'écrit en chaîne (sa longueur en
                            // tête), un objet incorporé en JSON.
                            payload: match value {
                                Any::String(s) => s.len() + 5,
                                other => json_bytes(other),
                            },
                        })
                        .add(format_cost(attrs).times(4));
                }
            }
        }
        Tree {
            nodes,
            height,
            blocks,
            cost,
        }
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
