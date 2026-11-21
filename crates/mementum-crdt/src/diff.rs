//! L'écriture d'une page par différence (Rulings R1227, R1254, R1255 ; Task S2 du lot 5).
//!
//! `PUT /mmd`, `replace_page` et la restauration écrivaient tout le contenu (`write_all`) : une
//! frappe arrivée au serveur après l'écriture, où qu'elle soit, était perdue. [`PageDoc::write_diff`]
//! n'écrit que ce qui diffère de la page :
//!
//! - **liste de frères par liste de frères**, après `keep_ids` (qui pose dans les blocs écrits
//!   l'identifiant du bloc courant qui leur correspond) : un bloc écrit et un bloc courant de même
//!   identifiant et de même sorte (conteneur, colonne, liste de colonnes) se répondent ; ceux qui
//!   restent à leur place sont la plus longue sous-suite croissante de ces paires ;
//! - un bloc qui reste n'est pas touché, s'il n'a pas changé (aucun élément Yjs créé ni supprimé) ;
//!   ses props changées s'écrivent en attributs de son nœud de contenu, comme `write_props` les
//!   écrirait ; un type changé recrée son seul nœud de contenu (conteneur et enfants gardés) ;
//! - un contenu en ligne changé s'écrit **par préfixe et suffixe communs** quand le milieu changé
//!   tient dans un seul texte et ne porte aucun élément en ligne (mention, formule, saut de ligne),
//!   des deux côtés ; **en entier** sinon ; un tableau changé est récrit en entier ;
//! - un bloc déplacé parmi ses frères (hors de la sous-suite gardée) est supprimé et recréé au même
//!   identifiant (la règle de R223 : le moins de blocs recréés), un bloc neuf est inséré, un bloc
//!   absent supprimé ;
//! - les enfants de chaque bloc gardé, de même.
//!
//! **Deux transactions** (R367, R1254) : les suppressions, puis les écritures. La première ne crée
//! rien : elle supprime les éléments retirés **par leur identifiant** (une mise à jour qui ne porte
//! qu'un jeu de suppressions), jamais par leur rang (chaque `remove_range` de yrs reparcourt la
//! liste depuis son début : un bloc sur deux retiré coûtait le carré de la liste, Ruling R1281), les
//! attributs retirés, les contenus vidés et le texte que le préfixe et le suffixe remplacent. La
//! seconde écrit ; un attribut écrasé y supprime son ancienne entrée. La mise à jour rendue est celle
//! de la seconde, ses suppressions augmentées de celles de la première (R1280) : une seule mise à
//! jour, que le journal garde et que l'acteur relaie.
//!
//! **Repli.** Une page dont les éléments Yjs ne répondent pas un à un aux blocs que `read_blocks`
//! en lit (une seconde racine, un contenu hors de la première `blockGroup`, une liste de colonnes
//! défaite, des blocs aplatis au-delà de `MAX_DEPTH`, un nœud étranger hors conteneur, un enfant
//! qui n'est pas un élément, un conteneur sans nœud de contenu) est récrite en entier
//! (`write_all`), qui retire ce que la lecture ne montre pas : l'écriture se lit ensuite comme les
//! blocs écrits. Aucun seuil par part de blocs touchés (R1254).
//!
//! Ce que l'écriture garde et perd des frappes concurrentes : limite LA-a du plan du lot 5.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mementum_md::{Block, Content, Inline, PropValue, Props, Styles, Table, normalize};
use mementum_schema::{PropSpec, block as spec_of};
use uuid::Uuid;
use yrs::branch::{Branch, BranchID};
use yrs::types::Attrs;
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, IdSet, Out, ReadTxn, Text, Transact, TransactionMut, Update, Xml, XmlElementRef,
    XmlFragment, XmlOut, XmlTextRef,
};

use crate::blocknote::{
    BLOCK_CONTAINER, BLOCK_GROUP, COLUMN, COLUMN_LIST, Ids, attr_string, elements, marks,
    prop_attribute, styles_of, write_children, write_content, write_inline, write_nodes,
};
use crate::cost::{WriteCost, diff_cost_of};
use crate::foreign::Foreign;
use crate::guard::{MARKS, value_refusal};
use crate::kept_ids::keep_ids;
use crate::page::writable;
use crate::table::write_table;
use crate::{CrdtError, PageDoc};

/// Ce que [`PageDoc::write_diff`] a écrit.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffWritten {
    /// La mise à jour (v1) qui fait toute l'écriture, ses deux transactions comprises : à écrire
    /// au journal, puis à diffuser. `[0, 0]` (vide) quand rien ne diffère : aucune transaction
    /// n'a été ouverte.
    pub update: Vec<u8>,
    /// Les identifiants des blocs dont un élément Yjs a été créé, supprimé ou changé (attribut,
    /// texte), à toute profondeur, chacun une fois, dans l'ordre du plan : blocs retirés et leurs
    /// descendants, blocs écrits (neufs ou recréés) et leurs descendants, blocs gardés dont les
    /// props, le type ou le contenu ont changé. Un bloc gardé dont seuls les enfants changent n'y
    /// est pas.
    pub touched: Vec<String>,
    /// L'écriture est retombée sur la récriture complète (la page porte ce que `read_blocks`
    /// normalise) : toute frappe concurrente de la page est perdue.
    pub fell_back: bool,
    /// Les blocs de la page après l'écriture, tels que `read_blocks` les lira (normalisés,
    /// identifiants compris), quand l'écriture ne retombe pas et que chaque bloc écrit garde
    /// l'identifiant que l'arbre préparé lui donnait ; `None` sinon : relire la page (Task S4 du
    /// lot 5, tour de correction : l'acteur relisait 10 000 blocs après chaque écriture d'agent).
    pub blocks: Option<Vec<Block>>,
}

impl PageDoc {
    /// Écrit `blocks` par différence avec la page (voir le module) : ce qui ne diffère pas n'est
    /// pas touché. Les blocs sont normalisés et vérifiés comme par [`PageDoc::write_blocks`], puis
    /// reçoivent l'identifiant du bloc courant qui leur correspond (`keep_ids`, comme
    /// [`PageDoc::rewrite_blocks`]) ; un bloc qui porte un identifiant le garde.
    ///
    /// Refusé sans rien écrire comme `write_blocks` (`Structure`, `TooDeep`). Après
    /// `HalfWritten`, la page est à recharger.
    pub fn write_diff(&self, blocks: &[Block]) -> Result<DiffWritten, CrdtError> {
        // Avant toute transaction d'écriture : `read_blocks` en ouvre une de lecture.
        let current = self.read_blocks();
        self.write_diff_over(&current, blocks)
    }

    /// Comme [`PageDoc::write_diff`], sur les blocs que [`PageDoc::read_blocks`] vient de rendre
    /// (`current`), sans les relire : l'acteur, qui lit déjà la page pour une opération ou pour
    /// `diff_cost`, ne la lit qu'une fois (relecture de S2). `current` doit être la lecture de la
    /// page telle qu'elle est, sans écriture entre les deux : un bloc dont `current` dirait le
    /// contenu inchangé ne serait pas écrit.
    pub fn write_diff_over(
        &self,
        current: &[Block],
        blocks: &[Block],
    ) -> Result<DiffWritten, CrdtError> {
        match self.write_within(current, blocks, None)? {
            Ok(written) => Ok(written),
            Err(_) => unreachable!("sans budget, rien n'est refusé pour son coût"),
        }
    }

    /// Comme [`PageDoc::write_diff_over`], refusée sans rien écrire si son coût estimé dépasse
    /// `budget_nanos` (`Ok(Err(coût))`) : l'estimation de [`crate::diff_cost`], sur les blocs que
    /// l'écriture vient de préparer et d'apparier, sans refaire `keep_ids` (Task S4 du lot 5,
    /// tour de correction : l'acteur estimait puis écrivait, et payait l'appariement deux fois).
    pub fn write_diff_within(
        &self,
        current: &[Block],
        blocks: &[Block],
        budget_nanos: u128,
    ) -> Result<Result<DiffWritten, WriteCost>, CrdtError> {
        self.write_within(current, blocks, Some(budget_nanos))
    }

    fn write_within(
        &self,
        current: &[Block],
        blocks: &[Block],
        budget_nanos: Option<u128>,
    ) -> Result<Result<DiffWritten, WriteCost>, CrdtError> {
        let mut target = writable(&prepared(blocks))?;
        keep_ids(current, &mut target);
        if let Some(budget) = budget_nanos {
            let cost = diff_cost_of(current, &target);
            if u128::from(cost.nanos) > budget {
                return Ok(Err(cost));
            }
        }
        // Les blocs neufs reçoivent leur identifiant ici, avant le plan, pour que l'arbre écrit
        // se rende tel que la page le lira (`DiffWritten::blocks`) ; un identifiant neuf ne répond
        // à aucun bloc de la page, comme un identifiant vide.
        for b in &mut target {
            fill_missing_ids(b);
        }
        let plan = {
            let txn = self.doc.transact();
            plan_page(&txn, &self.fragment, current, &target)
        };
        match plan {
            Some(plan) => self.write_plan(plan, target).map(Ok),
            None => self.fall_back(current, target).map(Ok),
        }
    }

    /// La récriture complète, ses identifiants posés d'avance pour les dire touchés (comme
    /// `write_nodes` les poserait : le premier en ordre du document garde le sien).
    fn fall_back(
        &self,
        current: &[Block],
        mut target: Vec<Block>,
    ) -> Result<DiffWritten, CrdtError> {
        let mut ids = Ids::default();
        for b in &mut target {
            assign(b, &mut ids);
        }
        let mut touched = Vec::new();
        for b in current.iter().chain(&target) {
            subtree_ids(b, &mut touched);
        }
        let update = self.write_all(&target)?;
        Ok(DiffWritten {
            update,
            touched: once(touched),
            fell_back: true,
            blocks: None,
        })
    }

    /// `target` : l'arbre que le plan écrit, ses blocs neufs déjà nommés ([`fill_missing_ids`]) ;
    /// rendu tel quel dans [`DiffWritten::blocks`] si l'écriture garde tous ses identifiants.
    fn write_plan(&self, mut plan: Plan, target: Vec<Block>) -> Result<DiffWritten, CrdtError> {
        // Les identifiants des blocs écrits, posés d'avance : aucun ne prend celui d'un bloc
        // gardé ; écrits ensuite tels quels.
        let mut ids = Ids::taken(std::mem::take(&mut plan.kept));
        let mut written = Vec::new();
        for (_, entries) in &mut plan.lists {
            for b in entries.iter_mut().flatten() {
                assign(b, &mut ids);
                subtree_ids(b, &mut written);
            }
        }
        for (_, children) in &mut plan.groups {
            for b in children.iter_mut() {
                assign(b, &mut ids);
                subtree_ids(b, &mut written);
            }
        }
        // L'arbre écrit est `target` tant qu'aucun bloc écrit n'a changé d'identifiant (un
        // identifiant que portaient deux blocs, un nœud étranger qui reprend le sien) : sinon
        // l'appelant relit la page.
        let blocks = {
            let mut named = Vec::new();
            for b in &target {
                subtree_ids(b, &mut named);
            }
            let named: HashSet<&String> = named.iter().collect();
            written
                .iter()
                .all(|id| named.contains(id))
                .then_some(target)
        };
        plan.touched.extend(written);
        if !plan.deletes() && !plan.writes() {
            return Ok(DiffWritten {
                update: Update::EMPTY_V1.to_vec(),
                touched: Vec::new(),
                fell_back: false,
                blocks,
            });
        }
        let removed = if plan.deletes() {
            let mut txn = self.doc.transact_mut();
            delete(&mut txn, &plan)?;
            if !txn.insert_set().is_empty() {
                return Err(CrdtError::HalfWritten);
            }
            txn.delete_set().clone()
        } else {
            IdSet::new()
        };
        let (structs, written_ds) = if plan.writes() {
            let mut txn = self.doc.transact_mut();
            write(&mut txn, &plan);
            let update = txn.encode_update_v1();
            let deletions = txn.delete_set().clone();
            // Une mise à jour v1 : ses éléments, puis son jeu de suppressions.
            let tail = deletions.encode_v1();
            let Some(structs) = update.strip_suffix(tail.as_slice()) else {
                return Err(CrdtError::HalfWritten);
            };
            (structs.to_vec(), deletions)
        } else {
            (vec![0], IdSet::new())
        };
        let mut update = structs;
        update.extend(removed.merge(&written_ds).encode_v1());
        Ok(DiffWritten {
            update,
            touched: once(plan.touched),
            fell_back: false,
            blocks,
        })
    }
}

/// Les blocs à écrire avant `writable` : un type hors du registre devient le bloc `unknown` que
/// `write_content` écrirait (son `originalType` posé), pour que la comparaison avec la page, qui
/// le lit ainsi, ne le voie pas changé à chaque écriture.
pub(crate) fn prepared(blocks: &[Block]) -> Vec<Block> {
    fn canonical(b: &mut Block) {
        if !matches!(b.kind.as_str(), COLUMN_LIST | COLUMN) && spec_of(&b.kind).is_none() {
            let kind = std::mem::replace(&mut b.kind, "unknown".into());
            b.props
                .entry("originalType".into())
                .or_insert(PropValue::Str(kind));
        }
        b.children.iter_mut().for_each(canonical);
    }
    let mut blocks = blocks.to_vec();
    blocks.iter_mut().for_each(canonical);
    normalize(blocks)
}

// ================================================================ ce qui se répond, sans Yjs

/// La sorte d'élément qui porte un bloc.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sort {
    List,
    Column,
    Container,
}

pub(crate) fn sort_of(kind: &str) -> Sort {
    match kind {
        COLUMN_LIST => Sort::List,
        COLUMN => Sort::Column,
        _ => Sort::Container,
    }
}

/// Un nœud étranger, lu (`read_foreign`) ou à écrire : il ne se compare qu'en entier.
pub(crate) fn is_foreign(b: &Block) -> bool {
    b.kind == "unknown" && b.props.get("foreign") == Some(&PropValue::Bool(true))
}

/// Même nœud de contenu : type, props et contenu (sans les enfants).
fn same_node(a: &Block, b: &Block) -> bool {
    a.kind == b.kind && a.props == b.props && a.content == b.content
}

/// Un bloc écrit peut-il garder l'élément du bloc courant de même identifiant ? Même sorte ; et
/// pour un nœud étranger, d'un côté ou de l'autre, le même nœud.
fn pairable(current: &Block, target: &Block) -> bool {
    sort_of(&current.kind) == sort_of(&target.kind)
        && (sort_of(&target.kind) != Sort::Container
            || !(is_foreign(current) || is_foreign(target))
            || same_node(current, target))
}

/// Les blocs de `target` qui gardent l'élément d'un bloc de `current` sans changer de place :
/// `(rang écrit, rang courant)`, croissants des deux côtés, le plus grand nombre possible.
pub(crate) fn stays(current: &[Block], target: &[Block]) -> Vec<(usize, usize)> {
    let at: HashMap<&str, usize> = current
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.id.is_empty())
        .map(|(i, b)| (b.id.as_str(), i))
        .collect();
    let pairs: Vec<(usize, usize)> = target
        .iter()
        .enumerate()
        .filter_map(|(j, b)| {
            let i = *at.get(b.id.as_str()).filter(|_| !b.id.is_empty())?;
            pairable(&current[i], b).then_some((j, i))
        })
        .collect();
    longest_increasing(&pairs)
}

/// La plus longue sous-suite de `items` (rangés par leur premier membre) croissante par leur
/// second, en `n log n`.
fn longest_increasing(items: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; items.len()];
    for (k, &(_, i)) in items.iter().enumerate() {
        let pile = tails.partition_point(|&t| items[t].1 < i);
        if pile > 0 {
            previous[k] = Some(tails[pile - 1]);
        }
        if pile == tails.len() {
            tails.push(k);
        } else {
            tails[pile] = k;
        }
    }
    let mut out = Vec::with_capacity(tails.len());
    let mut at = tails.last().copied();
    while let Some(k) = at {
        out.push(items[k]);
        at = previous[k];
    }
    out.reverse();
    out
}

/// Les identifiants d'un bloc et de ses descendants.
fn subtree_ids(b: &Block, out: &mut Vec<String>) {
    if !b.id.is_empty() {
        out.push(b.id.clone());
    }
    for c in &b.children {
        subtree_ids(c, out);
    }
}

/// Chaque identifiant une fois, dans l'ordre.
fn once(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// Donne un identifiant neuf à chaque bloc de l'arbre qui n'en a pas (un nœud étranger hors
/// enveloppe excepté : il reprend le sien à l'écriture, `assign`).
fn fill_missing_ids(b: &mut Block) {
    let unwrapped = Foreign::of(b).is_some_and(|node| node.unwrapped());
    if b.id.is_empty() && !unwrapped {
        b.id = Uuid::now_v7().to_string();
    }
    for c in &mut b.children {
        fill_missing_ids(c);
    }
}

/// Pose l'identifiant d'un bloc à écrire et de ses descendants comme `write_node` le ferait
/// (`Ids::take` ; un nœud étranger hors enveloppe garde, sans identifiant donné, le sien), pour
/// que l'écriture, faite ensuite avec des `Ids` neufs, les garde tels quels.
fn assign(b: &mut Block, ids: &mut Ids) {
    match Foreign::of(b) {
        Some(node) if node.unwrapped() => {
            let wanted = if b.id.is_empty() {
                node.own_id()
            } else {
                b.id.clone()
            };
            if !wanted.is_empty() {
                b.id = ids.take(&wanted);
            }
        }
        _ => b.id = ids.take(&b.id),
    }
    for c in &mut b.children {
        assign(c, ids);
    }
}

// ======================================================================= le plan, sur Yjs

/// La page ne se lit pas élément pour bloc : l'écriture retombe sur la récriture complète.
struct Mismatch;

/// Ce que fera l'écriture, lu avant d'écrire (dans une transaction de lecture).
#[derive(Default)]
struct Plan {
    // Première transaction : ce qui se supprime.
    /// Les éléments retirés (blocs, groupes d'enfants, nœuds de contenu remplacés), par leur
    /// identifiant.
    removed: IdSet,
    /// Les attributs retirés.
    unset: Vec<(XmlElementRef, String)>,
    /// Les contenus vidés en entier : un contenu en ligne récrit, les lignes d'un tableau.
    cleared: Vec<(XmlElementRef, u32)>,
    /// Le texte que le préfixe et le suffixe communs remplacent : (texte, place, longueur).
    cut: Vec<(XmlTextRef, u32, u32)>,
    // Seconde transaction : ce qui s'écrit.
    attrs: Vec<(XmlElementRef, String, Any)>,
    /// (liste, ses entrées dans l'ordre écrit) : `None` pour un élément gardé, `Some` pour un bloc
    /// écrit.
    lists: Vec<(XmlElementRef, Vec<Option<Block>>)>,
    /// Le nœud de contenu d'un bloc dont le type change, à poser en tête de son conteneur.
    contents: Vec<(XmlElementRef, Block)>,
    /// Le groupe d'enfants à créer d'un conteneur qui n'en avait pas.
    groups: Vec<(XmlElementRef, Vec<Block>)>,
    /// Un contenu en ligne écrit en entier dans son nœud vidé.
    inlines: Vec<(XmlElementRef, Vec<Inline>)>,
    /// Le texte écrit à la place du milieu retiré : (texte, place, plages et leurs marques).
    pasted: Vec<(XmlTextRef, u32, Runs)>,
    /// Un tableau écrit dans son nœud vidé.
    tables: Vec<(XmlElementRef, Table)>,
    /// Les identifiants des blocs gardés en place, à tout niveau.
    kept: HashSet<String>,
    touched: Vec<String>,
}

impl Plan {
    fn deletes(&self) -> bool {
        !self.removed.is_empty()
            || !self.unset.is_empty()
            || !self.cleared.is_empty()
            || !self.cut.is_empty()
    }

    fn writes(&self) -> bool {
        !self.attrs.is_empty()
            || !self.lists.is_empty()
            || !self.contents.is_empty()
            || !self.groups.is_empty()
            || !self.inlines.is_empty()
            || !self.pasted.is_empty()
            || !self.tables.is_empty()
    }

    fn touch(&mut self, id: &str) {
        if !id.is_empty() {
            self.touched.push(id.to_string());
        }
    }

    /// Retire l'élément (par son identifiant Yjs).
    fn remove(&mut self, element: &XmlElementRef) -> Result<(), Mismatch> {
        match AsRef::<Branch>::as_ref(element).id() {
            BranchID::Nested(id) => {
                self.removed.insert(id, 1);
                Ok(())
            }
            BranchID::Root(_) => Err(Mismatch),
        }
    }
}

fn plan_page<T: ReadTxn>(
    txn: &T,
    fragment: &yrs::XmlFragmentRef,
    current: &[Block],
    target: &[Block],
) -> Option<Plan> {
    if fragment.len(txn) != 1 {
        return None;
    }
    let Some(XmlOut::Element(root)) = fragment.get(txn, 0) else {
        return None;
    };
    if root.tag().as_ref() != BLOCK_GROUP {
        return None;
    }
    let mut plan = Plan::default();
    plan_list(txn, &root, current, target, &mut plan).ok()?;
    Some(plan)
}

/// Le genre d'élément qui porte un bloc de cette sorte.
fn tag_of(kind: &str) -> &str {
    match sort_of(kind) {
        Sort::List | Sort::Column => kind,
        Sort::Container => BLOCK_CONTAINER,
    }
}

fn plan_list<T: ReadTxn>(
    txn: &T,
    list: &XmlElementRef,
    current: &[Block],
    target: &[Block],
    plan: &mut Plan,
) -> Result<(), Mismatch> {
    let elems = elements(txn, list);
    // Chaque enfant de la liste est un élément, et chaque élément le bloc lu à son rang.
    if elems.len() != current.len() || list.len(txn) as usize != elems.len() {
        return Err(Mismatch);
    }
    for (e, b) in elems.iter().zip(current) {
        if e.tag().as_ref() != tag_of(&b.kind) || attr_string(txn, e, "id") != b.id {
            return Err(Mismatch);
        }
    }
    let stay = stays(current, target);
    let mut staying = vec![false; current.len()];
    let mut kept_at = vec![None; target.len()];
    for &(j, i) in &stay {
        staying[i] = true;
        kept_at[j] = Some(i);
    }
    for (i, b) in current.iter().enumerate() {
        if !staying[i] {
            plan.remove(&elems[i])?;
            subtree_ids(b, &mut plan.touched);
        }
    }
    let entries: Vec<Option<Block>> = target
        .iter()
        .zip(&kept_at)
        .map(|(b, kept)| kept.is_none().then(|| b.clone()))
        .collect();
    if entries.iter().any(Option::is_some) {
        plan.lists.push((list.clone(), entries));
    }
    for (j, kept) in kept_at.iter().enumerate() {
        if let Some(i) = *kept {
            plan.kept.insert(target[j].id.clone());
            plan_block(txn, &elems[i], &current[i], &target[j], plan)?;
        }
    }
    Ok(())
}

fn plan_block<T: ReadTxn>(
    txn: &T,
    element: &XmlElementRef,
    current: &Block,
    target: &Block,
    plan: &mut Plan,
) -> Result<(), Mismatch> {
    if sort_of(&target.kind) != Sort::Container {
        let spec = spec_of(&target.kind).ok_or(Mismatch)?;
        if plan_props(element, &current.props, &target.props, spec.props, plan) {
            plan.touch(&target.id);
        }
        return plan_list(txn, element, &current.children, &target.children, plan);
    }
    // Un conteneur : son nœud de contenu en tête, puis, s'il en a, son groupe d'enfants.
    let parts = elements(txn, element);
    if element.len(txn) as usize != parts.len() {
        return Err(Mismatch);
    }
    let (content, group) = match parts.as_slice() {
        [c] if c.tag().as_ref() != BLOCK_GROUP => (c, None),
        [c, g] if c.tag().as_ref() != BLOCK_GROUP && g.tag().as_ref() == BLOCK_GROUP => {
            (c, Some(g))
        }
        _ => return Err(Mismatch),
    };
    let expected = if is_foreign(current) {
        match current.props.get("originalType") {
            Some(PropValue::Str(tag)) => tag.as_str(),
            _ => return Err(Mismatch),
        }
    } else {
        current.kind.as_str()
    };
    if content.tag().as_ref() != expected {
        return Err(Mismatch);
    }
    if is_foreign(current) || is_foreign(target) {
        // `pairable` : le même nœud, rien à écrire.
    } else if current.kind != target.kind {
        plan.remove(content)?;
        plan.contents.push((
            element.clone(),
            Block {
                children: Vec::new(),
                ..target.clone()
            },
        ));
        plan.touch(&target.id);
    } else {
        let spec = spec_of(&target.kind).ok_or(Mismatch)?;
        let mut changed = plan_props(content, &current.props, &target.props, spec.props, plan);
        if current.content != target.content {
            changed |= match &target.content {
                Content::Inline(inline) => plan_inline(txn, content, inline, plan),
                Content::Table(table) => {
                    let len = content.len(txn);
                    if len > 0 {
                        plan.cleared.push((content.clone(), len));
                    }
                    plan.tables.push((content.clone(), table.clone()));
                    true
                }
                Content::None => false,
            };
        }
        if changed {
            plan.touch(&target.id);
        }
    }
    match (group, target.children.is_empty()) {
        (None, _) if !current.children.is_empty() => Err(Mismatch),
        (None, true) => Ok(()),
        (None, false) => {
            plan.groups.push((element.clone(), target.children.clone()));
            Ok(())
        }
        (Some(g), true) => {
            if !current.children.is_empty() {
                plan.remove(g)?;
                for c in &current.children {
                    subtree_ids(c, &mut plan.touched);
                }
            }
            Ok(())
        }
        (Some(g), false) => plan_list(txn, g, &current.children, &target.children, plan),
    }
}

/// Les props qui diffèrent, écrites comme `write_props` les écrirait ([`prop_attribute`]) ; une
/// prop inconnue du registre que les blocs écrits n'ont plus est retirée. Vrai si une prop change.
fn plan_props(
    element: &XmlElementRef,
    current: &Props,
    target: &Props,
    specs: &[PropSpec],
    plan: &mut Plan,
) -> bool {
    if current == target {
        return false;
    }
    let tag = element.tag().clone();
    let names: std::collections::BTreeSet<&String> = current.keys().chain(target.keys()).collect();
    let mut changed = false;
    for name in names {
        if current.get(name) == target.get(name) {
            continue;
        }
        match prop_attribute(&tag, name, target.get(name), specs) {
            Some(value) => plan.attrs.push((element.clone(), name.clone(), value)),
            None => plan.unset.push((element.clone(), name.clone())),
        }
        changed = true;
    }
    changed
}

/// Le contenu en ligne changé : par préfixe et suffixe communs ([`affix`]), sinon en entier.
/// Vrai si quelque chose s'écrit.
fn plan_inline<T: ReadTxn>(
    txn: &T,
    content: &XmlElementRef,
    inline: &[Inline],
    plan: &mut Plan,
) -> bool {
    match affix(txn, content, inline) {
        Some(edit) => {
            if edit.removed == 0 && edit.runs.is_empty() {
                return false;
            }
            if edit.removed > 0 {
                plan.cut.push((edit.text.clone(), edit.at, edit.removed));
            }
            if !edit.runs.is_empty() {
                plan.pasted.push((edit.text, edit.at, edit.runs));
            }
        }
        None => {
            let len = content.len(txn);
            if len > 0 {
                plan.cleared.push((content.clone(), len));
            }
            plan.inlines.push((content.clone(), inline.to_vec()));
        }
    }
    true
}

// =========================================================== contenu en ligne, par préfixe

/// Des plages à écrire, chacune avec ses marques explicites.
type Runs = Vec<(String, Attrs)>;

/// Où vit une unité courante : le rang de l'enfant du nœud de contenu, la place UTF-16 dans son
/// texte.
type Place = (usize, u32);

/// Une marque, telle que l'`Interner` la numérote : les cinq marques simples en bits, les deux
/// couleurs, le numéro du lien.
type MarkKey = (u8, Option<String>, Option<String>, Option<u32>);

/// Une unité de contenu en ligne : un caractère et ses marques (un numéro, voir [`Interner`]),
/// ou un élément en ligne (un numéro aussi ; un élément que la lecture ignore en reçoit un à lui,
/// qui n'est égal à rien).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Piece {
    Char(char, u32),
    Element(u32),
}

/// Les marques et les éléments, numérotés une fois chacun. Rien de ce qu'une marque porte (une
/// couleur, une adresse de lien de 16 Kio) n'est recopié par caractère ni comparé caractère par
/// caractère (Rulings R380, R381) : une plage se lit une fois, une adresse se reconnaît d'abord à
/// l'identité de sa valeur dans le document, puis, la première fois, à son texte.
#[derive(Default)]
struct Interner {
    links: HashMap<String, u32>,
    link_values: HashMap<usize, u32>,
    link_texts: Vec<String>,
    marks: HashMap<MarkKey, u32>,
    mark_list: Vec<(Styles, Option<u32>)>,
    elements: HashMap<String, u32>,
    opaque: u32,
}

impl Interner {
    fn link(&mut self, href: &str) -> u32 {
        if let Some(&id) = self.links.get(href) {
            return id;
        }
        let id = self.link_texts.len() as u32;
        self.links.insert(href.to_string(), id);
        self.link_texts.push(href.to_string());
        id
    }

    fn mark(&mut self, styles: &Styles, link: Option<u32>) -> u32 {
        let flags = u8::from(styles.bold)
            | u8::from(styles.italic) << 1
            | u8::from(styles.underline) << 2
            | u8::from(styles.strike) << 3
            | u8::from(styles.code) << 4;
        let key = (
            flags,
            styles.text_color.clone(),
            styles.background_color.clone(),
            link,
        );
        if let Some(&id) = self.marks.get(&key) {
            return id;
        }
        let id = self.mark_list.len() as u32;
        self.marks.insert(key, id);
        self.mark_list.push((styles.clone(), link));
        id
    }

    fn element(&mut self, key: String) -> u32 {
        let next = self.elements.len() as u32;
        *self.elements.entry(key).or_insert(next)
    }

    /// Un numéro qu'aucune autre unité n'a.
    fn opaque(&mut self) -> u32 {
        self.opaque += 1;
        u32::MAX - self.opaque
    }

    /// Les marques explicites d'une plage, gardées par [`value_refusal`] comme toute marque que
    /// le serveur pose (Ruling R493).
    fn attrs(&self, mark: u32) -> Attrs {
        let (styles, link) = &self.mark_list[mark as usize];
        let link = link.map(|l| self.link_texts[l as usize].as_str());
        let mut attrs = marks(styles, link);
        attrs.retain(|key, value| {
            let Some(&known) = MARKS.iter().find(|(k, _)| *k == key.as_ref()) else {
                return false;
            };
            let json = match value {
                Any::Map(map) => serde_json::Value::Object(
                    map.iter()
                        .map(|(k, v)| {
                            let v = match v {
                                Any::String(s) => serde_json::Value::String(s.to_string()),
                                _ => serde_json::Value::Null,
                            };
                            (k.clone(), v)
                        })
                        .collect(),
                ),
                _ => serde_json::Value::Null,
            };
            value_refusal(known, &json.to_string()).is_none()
        });
        attrs
    }
}

/// La clé d'un élément en ligne : sa balise et ce que la lecture en garde.
fn mention_key(kind: &str, id: &str, label: &str) -> String {
    format!("mention\u{0}{kind}\u{0}{id}\u{0}{label}")
}

fn math_key(latex: &str) -> String {
    format!("math\u{0}{latex}")
}

const HARD_BREAK_KEY: &str = "hardBreak";

/// Les unités du contenu courant, et où chacune vit : (rang de l'enfant du nœud de contenu, place
/// UTF-16 dans son texte). `None` si un texte porte ce que `diff` ne montre pas (sa longueur ne
/// tombe pas juste) : les places seraient fausses.
fn current_units<T: ReadTxn>(
    txn: &T,
    content: &XmlElementRef,
    names: &mut Interner,
) -> Option<(Vec<Piece>, Vec<Place>)> {
    let mut pieces = Vec::new();
    let mut places = Vec::new();
    for (k, child) in content.children(txn).enumerate() {
        match child {
            XmlOut::Text(text) => {
                let mut offset = 0u32;
                for run in text.diff(txn, YChange::identity) {
                    let Out::Any(Any::String(s)) = &run.insert else {
                        return None;
                    };
                    let attrs = run.attributes.as_deref();
                    let (styles, href) = styles_of(attrs);
                    let link = href.map(|href| match attrs.and_then(|a| a.get("link")) {
                        Some(Any::Map(value)) => {
                            let at = Arc::as_ptr(value) as *const () as usize;
                            match names.link_values.get(&at) {
                                Some(&id) => id,
                                None => {
                                    let id = names.link(href);
                                    names.link_values.insert(at, id);
                                    id
                                }
                            }
                        }
                        _ => names.link(href),
                    });
                    let mark = names.mark(&styles, link);
                    for c in s.chars() {
                        pieces.push(Piece::Char(c, mark));
                        places.push((k, offset));
                        offset += c.len_utf16() as u32;
                    }
                }
                if offset != text.len(txn) {
                    return None;
                }
            }
            XmlOut::Element(e) => {
                let id = match e.tag().as_ref() {
                    "mention" => names.element(mention_key(
                        &attr_string(txn, &e, "kind"),
                        &attr_string(txn, &e, "id"),
                        &attr_string(txn, &e, "label"),
                    )),
                    "math" => names.element(math_key(&attr_string(txn, &e, "latex"))),
                    "hardBreak" => names.element(HARD_BREAK_KEY.to_string()),
                    _ => names.opaque(),
                };
                pieces.push(Piece::Element(id));
                places.push((k, 0));
            }
            XmlOut::Fragment(_) => {
                let id = names.opaque();
                pieces.push(Piece::Element(id));
                places.push((k, 0));
            }
        }
    }
    Some((pieces, places))
}

fn target_units(content: &[Inline], link: Option<u32>, names: &mut Interner, out: &mut Vec<Piece>) {
    for inline in content {
        match inline {
            Inline::Text { text, styles } => {
                let mark = names.mark(styles, link);
                out.extend(text.chars().map(|c| Piece::Char(c, mark)));
            }
            Inline::Link { href, children } => {
                let id = names.link(href);
                target_units(children, Some(id), names, out);
            }
            Inline::Mention { kind, id, label } => {
                let id = names.element(mention_key(kind, id, label));
                out.push(Piece::Element(id));
            }
            Inline::Math { latex } => {
                let id = names.element(math_key(latex));
                out.push(Piece::Element(id));
            }
            Inline::HardBreak => {
                let id = names.element(HARD_BREAK_KEY.to_string());
                out.push(Piece::Element(id));
            }
        }
    }
}

/// Le milieu changé entre les unités courantes `old` (et la place de chacune : enfant du nœud
/// de contenu, place UTF-16 dans son texte) et les unités voulues `new` : l'enfant et la place où
/// écrire, la longueur UTF-16 retirée, et le rang des unités écrites dans `new`. `None` si le
/// milieu porte un élément en ligne, d'un côté ou de l'autre, ne tient pas dans un seul texte, ou
/// n'a aucun texte voisin où s'écrire.
fn middle(
    old: &[Piece],
    places: &[Place],
    new: &[Piece],
) -> Option<(usize, u32, u32, std::ops::Range<usize>)> {
    let p = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let s = old[p..]
        .iter()
        .rev()
        .zip(new[p..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_mid = &old[p..old.len() - s];
    let inserted = p..new.len() - s;
    if old_mid
        .iter()
        .chain(&new[inserted.clone()])
        .any(|u| matches!(u, Piece::Element(_)))
    {
        return None;
    }
    let char_len = |piece: &Piece| match piece {
        Piece::Char(c, _) => c.len_utf16() as u32,
        Piece::Element(_) => 0,
    };
    // Le texte où écrire : celui du milieu retiré, sinon celui de l'unité d'avant (à sa fin),
    // sinon celui de l'unité d'après (à son début).
    let (child, at) = if !old_mid.is_empty() {
        let (from, to) = (places[p], places[p + old_mid.len() - 1]);
        if from.0 != to.0 {
            return None;
        }
        from
    } else if p > 0 && matches!(old[p - 1], Piece::Char(..)) {
        let (k, offset) = places[p - 1];
        (k, offset + char_len(&old[p - 1]))
    } else if p < old.len() && matches!(old[p], Piece::Char(..)) {
        places[p]
    } else {
        return None;
    };
    Some((child, at, old_mid.iter().map(char_len).sum(), inserted))
}

/// Le contenu en ligne `current`, tel que `write_inline` l'écrit, se récrirait-il en `target` par
/// préfixe et suffixe communs ? Ce que [`affix`] déciderait sur la page, d'après les blocs seuls
/// (pour `diff_cost`) : `None` sinon ; s'il le fait, le nombre de coupes d'un texte existant (le
/// début du milieu retiré et sa fin, ou la place d'une insertion, quand elles tombent à
/// l'intérieur de leur texte et non à un de ses bords), de 0 à 2.
pub(crate) fn affix_cuts(current: &[Inline], target: &[Inline]) -> Option<usize> {
    let mut names = Interner::default();
    let (mut old, mut new) = (Vec::new(), Vec::new());
    target_units(current, None, &mut names, &mut old);
    target_units(target, None, &mut names, &mut new);
    // Un texte par suite de plages, un enfant par élément en ligne ; et la longueur de chaque texte.
    let mut places = Vec::with_capacity(old.len());
    let mut lengths: HashMap<usize, u32> = HashMap::new();
    let (mut child, mut offset, mut in_text) = (0usize, 0u32, false);
    for piece in &old {
        match piece {
            Piece::Char(c, _) => {
                in_text = true;
                places.push((child, offset));
                offset += c.len_utf16() as u32;
                lengths.insert(child, offset);
            }
            Piece::Element(_) => {
                if in_text {
                    child += 1;
                }
                places.push((child, 0));
                (child, offset, in_text) = (child + 1, 0, false);
            }
        }
    }
    let (child, at, removed, _) = middle(&old, &places, &new)?;
    let len = lengths.get(&child).copied().unwrap_or(0);
    let inside = |at: u32| at > 0 && at < len;
    Some(usize::from(inside(at)) + usize::from(removed > 0 && inside(at + removed)))
}

/// Une écriture par préfixe et suffixe communs.
struct Affix {
    text: XmlTextRef,
    at: u32,
    removed: u32,
    runs: Runs,
}

/// Le préfixe et le suffixe communs du contenu courant et du contenu voulu ; le milieu changé
/// doit ne porter aucun élément en ligne, des deux côtés, et tenir dans un seul texte. `None`
/// sinon (le contenu se récrit en entier).
fn affix<T: ReadTxn>(txn: &T, content: &XmlElementRef, inline: &[Inline]) -> Option<Affix> {
    let mut names = Interner::default();
    let (old, places) = current_units(txn, content, &mut names)?;
    let mut new = Vec::new();
    target_units(inline, None, &mut names, &mut new);
    let (child, at, removed, inserted) = middle(&old, &places, &new)?;
    let Some(XmlOut::Text(text)) = content.get(txn, child as u32) else {
        return None;
    };
    let mut runs: Vec<(String, u32)> = Vec::new();
    for piece in &new[inserted] {
        let Piece::Char(c, mark) = *piece else {
            return None;
        };
        match runs.last_mut() {
            Some((s, m)) if *m == mark => s.push(c),
            _ => runs.push((c.to_string(), mark)),
        }
    }
    let runs = runs
        .into_iter()
        .map(|(s, mark)| (s, names.attrs(mark)))
        .collect();
    Some(Affix {
        text,
        at,
        removed,
        runs,
    })
}

// ============================================================================== écrire

/// La première transaction : ce qui se supprime, sans rien créer.
fn delete(txn: &mut TransactionMut, plan: &Plan) -> Result<(), CrdtError> {
    if !plan.removed.is_empty() {
        // Par identifiant : une mise à jour qui ne porte qu'un jeu de suppressions. yrs trouve
        // chaque élément dans le magasin des blocs, sans parcourir sa liste.
        let mut bytes = vec![0];
        bytes.extend(plan.removed.encode_v1());
        let update = Update::decode_v1(&bytes).map_err(|e| CrdtError::Decode(e.to_string()))?;
        // Une erreur de yrs ici peut laisser la transaction à moitié faite : la page est à
        // recharger (relecture de S2, M3).
        txn.apply_update(update)
            .map_err(|_| CrdtError::HalfWritten)?;
    }
    for (element, name) in &plan.unset {
        element.remove_attribute(txn, &name.as_str());
    }
    for (node, len) in &plan.cleared {
        node.remove_range(txn, 0, *len);
    }
    for (text, at, len) in &plan.cut {
        text.remove_range(txn, *at, *len);
    }
    Ok(())
}

/// La seconde transaction : ce qui s'écrit.
fn write(txn: &mut TransactionMut, plan: &Plan) {
    // Les identifiants sont déjà posés et distincts : des `Ids` neufs les gardent tels quels.
    let mut ids = Ids::default();
    for (element, name, value) in &plan.attrs {
        element.insert_attribute(txn, name.as_str(), value.clone());
    }
    for (list, entries) in &plan.lists {
        let mut at = 0u32;
        for entry in entries {
            match entry {
                None => at += 1,
                Some(b) => at += write_nodes(txn, list, at, std::slice::from_ref(b), &mut ids),
            }
        }
    }
    for (container, content) in &plan.contents {
        write_content(txn, container, 0, content);
    }
    for (container, children) in &plan.groups {
        write_children(txn, container, children, &mut ids);
    }
    for (node, content) in &plan.inlines {
        write_inline(txn, node, content);
    }
    for (text, at, runs) in &plan.pasted {
        let mut position = *at;
        for (s, attrs) in runs {
            text.insert_with_attributes(txn, position, s, attrs.clone());
            position += s.encode_utf16().count() as u32;
        }
    }
    for (node, table) in &plan.tables {
        write_table(txn, node, Some(table));
    }
}
