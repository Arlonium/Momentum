//! Les opérations d'édition des agents (`update_page`, §6.3 ; Rulings R1228 à R1232, R1235 ; Task
//! S3 du lot 5).
//!
//! Un appel est une liste d'opérations, atomique par construction (R1229) : [`plan`] les résout et
//! les vérifie une à une, dans l'ordre, sur les blocs que la page vient de rendre (lus **une
//! fois**), sans toucher au document ; la première qui ne passe pas arrête tout, et son refus la
//! nomme par son rang. Le plan est l'arbre de blocs que l'écriture donnera ; [`PageDoc::apply`]
//! l'écrit **par différence** avec les blocs lus (`write_diff_over`, Task S2) : ce qui ne change
//! pas n'est pas touché, et ce que l'écriture garde des frappes concurrentes est celui de
//! l'écriture par différence (limite LA-a). Le plan appliqué est exactement le plan vérifié.
//!
//! - `str_replace` cherche `old` dans le **texte brut** de chaque bloc (`mementum_md::block_text` :
//!   une mention ou une formule y valent `U+FFFC`, un saut de ligne `\n`), jamais à cheval sur deux
//!   blocs ni dans le MMD rendu ; `new` prend les marques (styles, lien) du premier caractère
//!   remplacé, à défaut du caractère d'avant (R1228). L'écriture par différence n'en récrit que le
//!   texte qui change (préfixe et suffixe communs).
//! - Les blocs qu'une opération insère reçoivent des identifiants neufs (UUIDv7), quoi que porte
//!   leur MMD ; `replace_block` donne l'identifiant du bloc remplacé au premier bloc neuf de son
//!   type, parmi ceux de premier niveau (R1232).
//! - Un bloc `pageLink`, `databaseRef`, `syncedBlock` ou `unknown` que l'opération retirerait ne
//!   part que sous `allow_deleting_content` (R1232).
//! - Une écriture par jeton ne fait entrer aucune image, vidéo ou son dont l'adresse n'est ni
//!   celle d'un fichier de l'instance ni une adresse que la page portait déjà (R1235).
//! - En `rebase` (la page a changé depuis la lecture de l'agent), chaque bloc de la page que
//!   l'appel touche doit avoir gardé ses empreintes (R1231) ; `replace_page` passe par la fusion à
//!   trois ([`crate::anchors::three_way`]).
//!
//! L'arbre de travail est une arène : chaque bloc une fois, ses enfants par rang, l'index des
//! identifiants et le texte brut de chaque bloc gardés (R641 : jamais une relecture de la page par
//! bloc cherché).

use std::collections::{HashMap, HashSet};

use mementum_md::{Block, Content, Inline, Styles, block_text, excerpt, normalize, parse};
use mementum_schema::{MmdForm, block as spec_of};
use uuid::Uuid;

use crate::anchors::{AnchorError, Anchors, BlockHashes, HashKey, hashes, id_key, three_way};
use crate::blocknote::{COLUMN, COLUMN_LIST};
use crate::foreign::Foreign;
use crate::kept_ids::keep_ids;
use crate::structure::{Slot, check_blocks};
use crate::{CrdtError, DiffWritten, PageDoc};

/// Où insérer des blocs, ou où déplacer un bloc.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    /// Juste après le bloc de cette ancre, au même niveau.
    After(String),
    /// Juste avant le bloc de cette ancre, au même niveau.
    Before(String),
    /// En tête de la page.
    Start,
    /// En fin de page.
    End,
}

/// Une opération d'`update_page`, ses MMD déjà lus ([`read_markdown`]) : la lecture d'un MMD se
/// fait hors de l'acteur de la page (R383), le plan dans l'acteur.
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    StrReplace {
        old: String,
        new: String,
        anchor: Option<String>,
        replace_all: bool,
    },
    Insert {
        at: Place,
        blocks: Vec<Block>,
    },
    ReplaceBlock {
        anchor: String,
        blocks: Vec<Block>,
        allow_deleting_content: bool,
    },
    DeleteBlock {
        anchor: String,
        allow_deleting_content: bool,
    },
    MoveBlock {
        anchor: String,
        to: Place,
    },
    ReplacePage {
        blocks: Vec<Block>,
        allow_deleting_content: bool,
    },
}

impl Operation {
    /// `replace_page`, `replace_block` et `delete_block` : ils exigent une révision (R1231) et
    /// coupent une version `before_agent` (R1234).
    pub fn destructive(&self) -> bool {
        matches!(
            self,
            Operation::ReplacePage { .. }
                | Operation::ReplaceBlock { .. }
                | Operation::DeleteBlock { .. }
        )
    }
}

/// Ce que l'appel sait de la révision qu'il nomme.
#[derive(Debug, Clone, Copy)]
pub enum Base<'a> {
    /// La page est à la révision que l'agent a lue, ou l'appel n'en nomme pas : les opérations
    /// s'appliquent telles qu'elles se résolvent.
    Current,
    /// La page a changé depuis la révision lue (`on_conflict: "rebase"`) : voici les empreintes
    /// de cette révision ; un bloc que l'appel touche doit les avoir gardées (R1231).
    Changed(&'a BlockHashes),
}

/// Le contexte d'un plan.
#[derive(Debug, Clone, Copy)]
pub struct PlanContext<'a> {
    pub base: Base<'a>,
    /// La clé des empreintes du processus.
    pub key: &'a HashKey,
    /// Une écriture par jeton : aucun média du web que la page ne portait pas (R1235).
    pub forbid_external_media: bool,
}

/// La raison d'un refus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NoMatch,
    AmbiguousMatch,
    StaleAnchor,
    AmbiguousAnchor,
    InvalidMarkdown,
    InvalidStructure,
    InvalidMove,
    ProtectedContent,
    ExternalMedia,
    RevisionConflict,
    /// Ce que l'appel écrirait par `str_replace` dépasse [`MAX_REPLACED_BYTES`].
    TooLong,
}

impl Reason {
    /// Le code du refus, tel que l'API le rend.
    pub fn code(self) -> &'static str {
        match self {
            Reason::NoMatch => "no_match",
            Reason::AmbiguousMatch => "ambiguous_match",
            Reason::StaleAnchor => "stale_anchor",
            Reason::AmbiguousAnchor => "ambiguous_anchor",
            Reason::InvalidMarkdown => "invalid_markdown",
            Reason::InvalidStructure => "invalid_structure",
            Reason::InvalidMove => "invalid_move",
            Reason::ProtectedContent => "protected_content",
            Reason::ExternalMedia => "external_media",
            Reason::RevisionConflict => "revision_conflict",
            Reason::TooLong => "too_long_to_write",
        }
    }
}

/// Un bloc proposé par un refus : son alias et 120 caractères de son texte (R1230).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub alias: String,
    pub text: String,
}

/// Le refus d'une opération : rien n'est écrit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpRefusal {
    /// Le rang de l'opération refusée, à partir de 0.
    pub index: usize,
    pub reason: Reason,
    /// Ce qu'il faut faire, en français ; ne cite l'entrée que par `mementum_md::excerpt`.
    pub message: String,
    /// Pour `no_match` (trois au plus) et `ambiguous_match` (cinq au plus) : du texte de la page.
    pub candidates: Vec<Candidate>,
    /// Les identifiants des blocs de la page en cause (un conflit, un contenu protégé).
    pub blocks: Vec<String>,
}

/// Un plan vérifié : les blocs que l'écriture donnera, sur les blocs lus.
#[derive(Debug, Clone)]
pub struct Plan<'a> {
    current: &'a [Block],
    blocks: Vec<Block>,
    touched: Vec<String>,
}

impl Plan<'_> {
    /// Les blocs que l'écriture donnera.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Les identifiants des blocs de la page lue que l'appel touche, comme R1231 les juge, chacun
    /// une fois, dans l'ordre des opérations.
    pub fn touched(&self) -> &[String] {
        &self.touched
    }
}

impl PageDoc {
    /// Écrit un plan, par différence avec les blocs sur lesquels il a été fait (que la page doit
    /// encore porter : l'acteur fait le plan et l'écriture dans la même commande).
    pub fn apply(&self, plan: &Plan<'_>) -> Result<DiffWritten, CrdtError> {
        self.write_diff_over(plan.current, &plan.blocks)
    }
}

/// Lit le MMD de l'opération `index` : `invalid_markdown`, avec la ligne, s'il ne se lit pas.
pub fn read_markdown(index: usize, mmd: &str) -> Result<Vec<Block>, OpRefusal> {
    parse(mmd).map_err(|e| {
        refusal(
            index,
            Reason::InvalidMarkdown,
            format!(
                "le MMD ne se lit pas, ligne {} : {} Le format est décrit par la ressource \
                 mementum://spec/mmd.",
                e.line, e.message
            ),
        )
    })
}

fn refusal(index: usize, reason: Reason, message: String) -> OpRefusal {
    OpRefusal {
        index,
        reason,
        message,
        candidates: Vec::new(),
        blocks: Vec::new(),
    }
}

/// Caractères d'un candidat (R1230).
const CANDIDATE_CHARS: usize = 120;
/// Caractères gardés devant le début trouvé, dans un candidat.
const CANDIDATE_LEAD: usize = 20;
/// Octets que les `str_replace` d'un appel écrivent au plus : autant que le corps d'une requête
/// (2 Mio). Un `replace_all` d'un caractère courant par un long texte écrirait sinon le produit
/// des deux (R380).
pub const MAX_REPLACED_BYTES: usize = 2 << 20;
/// Candidats d'un `no_match`, et d'un `ambiguous_match`.
const NO_MATCH_CANDIDATES: usize = 3;
const AMBIGUOUS_CANDIDATES: usize = 5;
/// Un candidat porte au moins ce début de `old` (ou `old` entier s'il est plus court).
const MIN_CANDIDATE_PREFIX: usize = 2;

/// Les types de blocs qu'une opération ne retire que sous `allow_deleting_content` (R1232).
const PROTECTED: [&str; 4] = ["pageLink", "databaseRef", "syncedBlock", "unknown"];
/// Les blocs dont l'adresse se charge d'elle-même chez le lecteur (R1235).
const MEDIA: [&str; 3] = ["image", "video", "audio"];

/// Résout et vérifie `ops` sur `current`, les blocs que la page vient de rendre : toutes, ou le
/// refus de la première qui ne passe pas.
pub fn plan<'a>(
    current: &'a [Block],
    ops: &[Operation],
    ctx: &PlanContext<'_>,
) -> Result<Plan<'a>, OpRefusal> {
    let mut planner = Planner::new(current, ctx);
    for (index, op) in ops.iter().enumerate() {
        planner.index = index;
        planner.operation(op)?;
        if !planner.conflicts.is_empty() {
            let blocks = std::mem::take(&mut planner.conflicts);
            return Err(OpRefusal {
                blocks,
                ..refusal(
                    index,
                    Reason::RevisionConflict,
                    "la page a changé depuis ta lecture dans les blocs que cette opération \
                     touche : relis-les, puis réécris avec la révision courante."
                        .into(),
                )
            });
        }
    }
    let blocks = planner.work.into_blocks();
    // Le filet : chaque opération vérifie déjà la place de ce qu'elle écrit.
    if let Err(e) = check_blocks(&blocks, Slot::Group, 2) {
        let index = planner.last_structural.unwrap_or(0);
        return Err(structure_refusal(index, &e));
    }
    Ok(Plan {
        current,
        blocks,
        touched: planner.touched,
    })
}

fn structure_refusal(index: usize, e: &CrdtError) -> OpRefusal {
    let why = match e {
        CrdtError::Structure(why) => why.clone(),
        other => other.to_string(),
    };
    refusal(
        index,
        Reason::InvalidStructure,
        format!(
            "la page n'aurait plus une forme que l'éditeur sait lire ({why}) : choisis une autre place ou un autre contenu."
        ),
    )
}

// =========================================================================== l'arbre de travail

struct Node {
    /// Le bloc, sans ses enfants.
    block: Block,
    children: Vec<usize>,
    parent: Option<usize>,
    alive: bool,
    /// Son texte brut, gardé tant que son contenu ne change pas.
    text: Option<String>,
}

#[derive(Default)]
struct Work {
    nodes: Vec<Node>,
    root: Vec<usize>,
    /// Les blocs de chaque identifiant (vivants ou non : `alive` le dit).
    by_id: HashMap<String, Vec<usize>>,
}

impl Work {
    fn of(blocks: Vec<Block>) -> Work {
        let mut work = Work::default();
        let root: Vec<usize> = blocks.into_iter().map(|b| work.add(b, None)).collect();
        work.root = root;
        work
    }

    fn add(&mut self, mut block: Block, parent: Option<usize>) -> usize {
        let children = std::mem::take(&mut block.children);
        let at = self.nodes.len();
        if !block.id.is_empty() {
            self.by_id.entry(block.id.clone()).or_default().push(at);
        }
        self.nodes.push(Node {
            block,
            children: Vec::new(),
            parent,
            alive: true,
            text: None,
        });
        let kids: Vec<usize> = children
            .into_iter()
            .map(|c| self.add(c, Some(at)))
            .collect();
        self.nodes[at].children = kids;
        at
    }

    fn list(&self, parent: Option<usize>) -> &Vec<usize> {
        match parent {
            None => &self.root,
            Some(p) => &self.nodes[p].children,
        }
    }

    fn list_mut(&mut self, parent: Option<usize>) -> &mut Vec<usize> {
        match parent {
            None => &mut self.root,
            Some(p) => &mut self.nodes[p].children,
        }
    }

    /// Le parent d'un bloc vivant, et son rang parmi ses frères.
    fn position(&self, node: usize) -> (Option<usize>, usize) {
        let parent = self.nodes[node].parent;
        let at = self
            .list(parent)
            .iter()
            .position(|&n| n == node)
            .expect("un bloc vivant est dans la liste de son parent");
        (parent, at)
    }

    fn detach(&mut self, node: usize) -> (Option<usize>, usize) {
        let (parent, at) = self.position(node);
        self.list_mut(parent).remove(at);
        (parent, at)
    }

    fn attach(&mut self, node: usize, parent: Option<usize>, at: usize) {
        self.nodes[node].parent = parent;
        self.list_mut(parent).insert(at, node);
    }

    /// Le bloc et ses descendants, dans l'ordre du document.
    fn subtree(&self, node: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.nodes[n].children.iter().rev());
        }
        out
    }

    fn kill(&mut self, node: usize) {
        for n in self.subtree(node) {
            self.nodes[n].alive = false;
        }
    }

    /// Les blocs vivants, dans l'ordre du document.
    fn preorder(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack: Vec<usize> = self.root.iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.nodes[n].children.iter().rev());
        }
        out
    }

    /// Vrai si `node` est `ancestor` ou l'un de ses descendants.
    fn is_inside(&self, node: usize, ancestor: usize) -> bool {
        let mut current = Some(node);
        while let Some(n) = current {
            if n == ancestor {
                return true;
            }
            current = self.nodes[n].parent;
        }
        false
    }

    fn alive_with(&self, id: &str) -> Vec<usize> {
        self.by_id
            .get(id)
            .map(|nodes| {
                nodes
                    .iter()
                    .copied()
                    .filter(|&n| self.nodes[n].alive)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn ensure_text(&mut self, node: usize) {
        if self.nodes[node].text.is_none() {
            self.nodes[node].text = Some(block_text(&self.nodes[node].block));
        }
    }

    fn text(&self, node: usize) -> &str {
        self.nodes[node].text.as_deref().unwrap_or_default()
    }

    fn set_content(&mut self, node: usize, content: Vec<Inline>) {
        let b = &mut self.nodes[node].block;
        let mut normalized = normalize(vec![Block {
            id: String::new(),
            kind: b.kind.clone(),
            props: Default::default(),
            content: Content::Inline(content),
            children: Vec::new(),
        }]);
        b.content = normalized.remove(0).content;
        self.nodes[node].text = None;
    }

    /// Le bloc et ses descendants, recopiés.
    fn to_block(&self, node: usize) -> Block {
        let n = &self.nodes[node];
        Block {
            children: n.children.iter().map(|&c| self.to_block(c)).collect(),
            ..n.block.clone()
        }
    }

    fn blocks(&self) -> Vec<Block> {
        self.root.iter().map(|&n| self.to_block(n)).collect()
    }

    fn into_blocks(mut self) -> Vec<Block> {
        fn take(nodes: &mut [Node], node: usize) -> Block {
            let children = std::mem::take(&mut nodes[node].children);
            let mut b = std::mem::replace(
                &mut nodes[node].block,
                Block {
                    id: String::new(),
                    kind: String::new(),
                    props: Default::default(),
                    content: Content::None,
                    children: Vec::new(),
                },
            );
            b.children = children.into_iter().map(|c| take(nodes, c)).collect();
            b
        }
        let root = std::mem::take(&mut self.root);
        root.into_iter().map(|n| take(&mut self.nodes, n)).collect()
    }

    /// Le genre de liste des enfants de `parent`, et la profondeur Yjs du premier élément qu'on y
    /// écrit : ce que `check_blocks` attend (la racine `blockGroup` est à 1, un bloc de premier
    /// niveau à 2 ; une colonne et ses blocs à un cran de leur liste ; les enfants d'un conteneur
    /// à deux crans, son groupe d'enfants entre les deux ; ceux d'un nœud étranger hors
    /// enveloppe, à sa suite).
    fn slot(&self, parent: Option<usize>) -> (Slot, usize) {
        let Some(p) = parent else {
            return (Slot::Group, 2);
        };
        let (own, depth) = self.slot(self.nodes[p].parent);
        let b = &self.nodes[p].block;
        match b.kind.as_str() {
            COLUMN_LIST => (Slot::ColumnList, depth + 1),
            COLUMN => (Slot::Column, depth + 1),
            _ => match Foreign::of(b) {
                Some(f) if f.unwrapped() => (own, depth),
                _ => (Slot::Group, depth + 2),
            },
        }
    }
}

// ============================================================================ le planificateur

/// Ce que R1231 compare : les empreintes de la révision lue et de la page d'aujourd'hui, par
/// identifiant (`None` : plusieurs blocs le portent).
struct Judge {
    base: HashMap<u64, Option<(u64, u64)>>,
    now: HashMap<u64, Option<(u64, u64)>>,
}

impl Judge {
    fn index(hashes: &BlockHashes) -> HashMap<u64, Option<(u64, u64)>> {
        let mut out: HashMap<u64, Option<(u64, u64)>> = HashMap::with_capacity(hashes.len());
        for h in hashes.as_slice() {
            out.entry(h.id)
                .and_modify(|seen| *seen = None)
                .or_insert(Some((h.content, h.place)));
        }
        out
    }

    /// Le bloc `id` de la page a-t-il changé depuis la révision lue (son contenu et sa place, ou
    /// sa place seule) ? Un bloc qui n'est pas de la page (inséré par l'appel) ne se juge pas ;
    /// un bloc arrivé depuis, ou que plusieurs blocs nomment, a changé.
    fn changed(&self, id: &str, whole: bool) -> bool {
        let key = id_key(id);
        let Some(now) = self.now.get(&key) else {
            return false;
        };
        match (self.base.get(&key), now) {
            (Some(Some(base)), Some(now)) => {
                if whole {
                    base != now
                } else {
                    base.1 != now.1
                }
            }
            _ => true,
        }
    }
}

struct Planner<'c> {
    work: Work,
    anchors: Anchors,
    ctx: PlanContext<'c>,
    /// Les identifiants des blocs de la page lue.
    page_ids: HashSet<String>,
    /// Les adresses des médias de la page lue.
    page_media: HashSet<String>,
    judge: Option<Judge>,
    index: usize,
    last_structural: Option<usize>,
    touched: Vec<String>,
    seen: HashSet<String>,
    conflicts: Vec<String>,
    /// Les octets que les `str_replace` de l'appel ont écrits jusqu'ici.
    replaced: usize,
}

impl<'c> Planner<'c> {
    fn new(current: &[Block], ctx: &PlanContext<'c>) -> Planner<'c> {
        fn walk(blocks: &[Block], ids: &mut HashSet<String>, media: &mut HashSet<String>) {
            for b in blocks {
                if !b.id.is_empty() {
                    ids.insert(b.id.clone());
                }
                if let Some(url) = media_url(b) {
                    media.insert(url.to_string());
                }
                walk(&b.children, ids, media);
            }
        }
        let (mut page_ids, mut page_media) = (HashSet::new(), HashSet::new());
        walk(current, &mut page_ids, &mut page_media);
        let judge = match ctx.base {
            Base::Current => None,
            Base::Changed(base) => Some(Judge {
                base: Judge::index(base),
                now: Judge::index(&hashes(current, ctx.key)),
            }),
        };
        Planner {
            work: Work::of(current.to_vec()),
            anchors: Anchors::of(current),
            ctx: *ctx,
            page_ids,
            page_media,
            judge,
            index: 0,
            last_structural: None,
            touched: Vec::new(),
            seen: HashSet::new(),
            conflicts: Vec::new(),
            replaced: 0,
        }
    }

    fn refuse(&self, reason: Reason, message: String) -> OpRefusal {
        refusal(self.index, reason, message)
    }

    fn operation(&mut self, op: &Operation) -> Result<(), OpRefusal> {
        match op {
            Operation::StrReplace {
                old,
                new,
                anchor,
                replace_all,
            } => self.str_replace(old, new, anchor.as_deref(), *replace_all),
            Operation::Insert { at, blocks } => self.insert(at, blocks),
            Operation::ReplaceBlock {
                anchor,
                blocks,
                allow_deleting_content,
            } => self.replace_block(anchor, blocks, *allow_deleting_content),
            Operation::DeleteBlock {
                anchor,
                allow_deleting_content,
            } => self.delete_block(anchor, *allow_deleting_content),
            Operation::MoveBlock { anchor, to } => self.move_block(anchor, to),
            Operation::ReplacePage {
                blocks,
                allow_deleting_content,
            } => self.replace_page(blocks, *allow_deleting_content),
        }
    }

    // ------------------------------------------------------------------- ancres, jugement

    /// Le bloc vivant que désigne une ancre : l'alias ou l'identifiant d'un bloc de la page lue,
    /// encore dans l'arbre de travail.
    fn resolve(&self, anchor: &str) -> Result<usize, OpRefusal> {
        let nodes = match self.anchors.resolve(anchor) {
            Ok(id) => self.work.alive_with(id),
            Err(AnchorError::Ambiguous) => return Err(self.ambiguous_anchor(anchor)),
            Err(AnchorError::Stale) => self.work.alive_with(anchor),
        };
        match nodes.as_slice() {
            [node] => Ok(*node),
            [] => Err(self.refuse(
                Reason::StaleAnchor,
                format!(
                    "l'ancre « {} » ne désigne aucun bloc de la page : relis la page avec ses \
                     ancres (fetch avec anchors), puis vise le bloc par l'alias qu'elle donne.",
                    excerpt(anchor)
                ),
            )),
            _ => Err(self.ambiguous_anchor(anchor)),
        }
    }

    fn ambiguous_anchor(&self, anchor: &str) -> OpRefusal {
        self.refuse(
            Reason::AmbiguousAnchor,
            format!(
                "l'ancre « {} » désigne plusieurs blocs de la page : relis la page avec ses \
                 ancres, qui s'allongent pour rester seules.",
                excerpt(anchor)
            ),
        )
    }

    /// Note que l'appel touche le bloc `node` (sa place seule, ou tout) et, en `rebase`, juge
    /// s'il a changé depuis la lecture de l'agent.
    fn touch(&mut self, node: usize, whole: bool) {
        let id = self.work.nodes[node].block.id.clone();
        if id.is_empty() || !self.page_ids.contains(&id) {
            return;
        }
        if let Some(judge) = &self.judge
            && judge.changed(&id, whole)
            && !self.conflicts.contains(&id)
        {
            self.conflicts.push(id.clone());
        }
        self.record(id);
    }

    /// Note que l'appel touche le bloc de la page `id`, sans le juger.
    fn record(&mut self, id: String) {
        if !id.is_empty() && self.page_ids.contains(&id) && self.seen.insert(id.clone()) {
            self.touched.push(id);
        }
    }

    fn touch_subtree(&mut self, node: usize) {
        for n in self.work.subtree(node) {
            self.touch(n, true);
        }
    }

    // ---------------------------------------------------------------------- vérifications

    /// Les blocs protégés parmi `nodes`, retirés sans l'option : refusé (R1232).
    fn check_protected(&self, nodes: &[usize], allow: bool) -> Result<(), OpRefusal> {
        if allow {
            return Ok(());
        }
        let gone: Vec<&Block> = nodes
            .iter()
            .map(|&n| &self.work.nodes[n].block)
            .filter(|b| PROTECTED.contains(&b.kind.as_str()))
            .collect();
        if gone.is_empty() {
            return Ok(());
        }
        Err(OpRefusal {
            blocks: gone
                .iter()
                .filter(|b| !b.id.is_empty())
                .map(|b| b.id.clone())
                .collect(),
            ..self.refuse(
                Reason::ProtectedContent,
                format!(
                    "l'opération retirerait {} bloc(s) qui montrent une sous-page, une base, un bloc \
                     synchronisé ou un contenu inconnu : garde-les dans ce que tu écris, ou pose \
                     allow_deleting_content.",
                    gone.len()
                ),
            )
        })
    }

    /// Une image, une vidéo ou un son à adresse du web, que la page ne portait pas, dans des
    /// blocs écrits par jeton : refusé (R1235).
    fn check_media(&self, blocks: &[Block]) -> Result<(), OpRefusal> {
        fn first<'b>(blocks: &'b [Block], page: &HashSet<String>) -> Option<&'b str> {
            blocks.iter().find_map(|b| {
                media_url(b)
                    .filter(|url| !url.is_empty() && !is_file_address(url) && !page.contains(*url))
                    .or_else(|| first(&b.children, page))
            })
        }
        if !self.ctx.forbid_external_media {
            return Ok(());
        }
        match first(blocks, &self.page_media) {
            None => Ok(()),
            Some(url) => Err(self.refuse(
                Reason::ExternalMedia,
                format!(
                    "l'opération pose une image, une vidéo ou un son dont l'adresse « {} » n'est \
                     pas un fichier de l'instance : un agent ne pose que des médias déjà dans la \
                     page ou envoyés à l'instance ; un lien ([texte](adresse)) passe.",
                    excerpt(url)
                ),
            )),
        }
    }

    /// Des blocs écrits dans les enfants de `parent` : la forme que BlockNote admet là, et la
    /// profondeur.
    fn check_fits(&self, blocks: &[Block], parent: Option<usize>) -> Result<(), OpRefusal> {
        let (slot, depth) = self.work.slot(parent);
        check_blocks(blocks, slot, depth).map_err(|e| structure_refusal(self.index, &e))
    }

    /// Le parent d'un bloc retiré garde la forme que BlockNote admet (une colonne n'est jamais
    /// vide ; une liste de colonnes en a deux au moins).
    fn check_left(&self, parent: Option<usize>) -> Result<(), OpRefusal> {
        let Some(p) = parent else {
            return Ok(());
        };
        let left = self.work.nodes[p].children.len();
        let why = match self.work.nodes[p].block.kind.as_str() {
            COLUMN if left == 0 => "la colonne resterait vide",
            COLUMN_LIST if left < 2 => "la liste de colonnes garderait moins de deux colonnes",
            _ => return Ok(()),
        };
        Err(structure_refusal(
            self.index,
            &CrdtError::Structure(why.to_string()),
        ))
    }

    /// Où mène une place : le parent, le rang, et le bloc d'ancrage.
    fn place(&self, at: &Place) -> Result<(Option<usize>, usize, Option<usize>), OpRefusal> {
        Ok(match at {
            Place::Start => (None, 0, None),
            Place::End => (None, self.work.root.len(), None),
            Place::After(anchor) => {
                let node = self.resolve(anchor)?;
                let (parent, at) = self.work.position(node);
                (parent, at + 1, Some(node))
            }
            Place::Before(anchor) => {
                let node = self.resolve(anchor)?;
                let (parent, at) = self.work.position(node);
                (parent, at, Some(node))
            }
        })
    }

    // ------------------------------------------------------------------------- opérations

    fn str_replace(
        &mut self,
        old: &str,
        new: &str,
        anchor: Option<&str>,
        replace_all: bool,
    ) -> Result<(), OpRefusal> {
        let scope = match anchor {
            Some(anchor) => vec![self.resolve(anchor)?],
            None => self.work.preorder(),
        };
        let mut hits: Vec<(usize, Vec<(usize, usize)>)> = Vec::new();
        let mut total = 0;
        for &node in &scope {
            self.work.ensure_text(node);
            let text = self.work.text(node);
            if text.len() < old.len() {
                continue;
            }
            let ranges = char_ranges(text, old);
            if !ranges.is_empty() {
                total += ranges.len();
                hits.push((node, ranges));
            }
        }
        if total == 0 {
            return Err(self.no_match(old, anchor.is_some()));
        }
        if total > 1 && !replace_all {
            let candidates = hits
                .iter()
                .filter_map(|(node, ranges)| self.candidate(*node, ranges[0].0))
                .take(AMBIGUOUS_CANDIDATES)
                .collect();
            return Err(OpRefusal {
                candidates,
                ..self.refuse(
                    Reason::AmbiguousMatch,
                    format!(
                        "« {} » se trouve {total} fois : vise le bloc par son ancre, allonge le \
                         texte cherché pour qu'il ne s'y trouve qu'une fois, ou pose replace_all.",
                        excerpt(old)
                    ),
                )
            });
        }
        // Avant de bâtir le moindre texte : ce que l'appel aura écrit par `str_replace`.
        self.replaced = self
            .replaced
            .saturating_add(total.saturating_mul(new.len()));
        if self.replaced > MAX_REPLACED_BYTES {
            return Err(self.refuse(
                Reason::TooLong,
                format!(
                    "les remplacements de cet appel écriraient plus de {} Mio de texte : remplace \
                     moins d'occurrences par appel, ou par un texte plus court.",
                    MAX_REPLACED_BYTES >> 20
                ),
            ));
        }
        for (node, ranges) in hits {
            let b = &self.work.nodes[node].block;
            let plain = spec_of(&b.kind).is_some_and(|s| s.form == MmdForm::Code);
            let Content::Inline(content) = &b.content else {
                continue;
            };
            let replaced = replace_ranges(content, &ranges, new, plain);
            self.work.set_content(node, replaced);
            self.touch(node, true);
        }
        Ok(())
    }

    /// Le refus d'un `old` introuvable, avec les blocs de la page lue qui portent son plus long
    /// début (R1230).
    fn no_match(&mut self, old: &str, anchored: bool) -> OpRefusal {
        let bounds: Vec<usize> = old
            .char_indices()
            .map(|(i, _)| i)
            .skip(1)
            .chain(std::iter::once(old.len()))
            .collect();
        let old_chars = bounds.len();
        let floor = MIN_CANDIDATE_PREFIX.min(old_chars);
        let mut found: Vec<(usize, usize, usize, usize)> = Vec::new(); // (début, ordre, bloc, place)
        for (order, node) in self.work.preorder().into_iter().enumerate() {
            let id = &self.work.nodes[node].block.id;
            if !self.page_ids.contains(id) || self.anchors.alias(id).is_none() {
                continue;
            }
            self.work.ensure_text(node);
            let text = self.work.text(node);
            // Le plus long début de `old` dans ce texte, par dichotomie (un début trouvé dit que
            // tous les plus courts le sont) ; jamais plus long que le texte.
            let (mut lo, mut hi) = (0, old_chars.min(text.chars().count()));
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                if text.contains(&old[..bounds[mid - 1]]) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            if lo >= floor.max(1) {
                let byte = text.find(&old[..bounds[lo - 1]]).unwrap_or(0);
                found.push((lo, order, node, text[..byte].chars().count()));
            }
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let candidates = found
            .iter()
            .take(NO_MATCH_CANDIDATES)
            .filter_map(|&(_, _, node, at)| self.candidate(node, at))
            .collect();
        let place = if anchored {
            "du bloc visé"
        } else {
            "d'aucun bloc"
        };
        OpRefusal {
            candidates,
            ..self.refuse(
                Reason::NoMatch,
                format!(
                    "« {} » ne se trouve dans le texte {place} : un texte se cherche dans un seul \
                     bloc, en texte brut, sans mise en forme ; relis la page, ou vois les blocs \
                     proposés.",
                    excerpt(old)
                ),
            )
        }
    }

    /// Un candidat : l'alias du bloc, et 120 caractères de son texte autour de `at`.
    fn candidate(&self, node: usize, at: usize) -> Option<Candidate> {
        let id = &self.work.nodes[node].block.id;
        let alias = self.anchors.alias(id)?;
        if !self.page_ids.contains(id) {
            return None;
        }
        let text = self.work.text(node);
        Some(Candidate {
            alias: alias.to_string(),
            text: text
                .chars()
                .skip(at.saturating_sub(CANDIDATE_LEAD))
                .take(CANDIDATE_CHARS)
                .collect(),
        })
    }

    fn insert(&mut self, at: &Place, blocks: &[Block]) -> Result<(), OpRefusal> {
        if blocks.is_empty() {
            return Err(self.refuse(
                Reason::InvalidMarkdown,
                "le MMD à insérer est vide : écris au moins un bloc.".into(),
            ));
        }
        let (parent, rank, anchor) = self.place(at)?;
        let fresh = fresh_ids(blocks);
        self.check_media(&fresh)?;
        self.check_fits(&fresh, parent)?;
        self.last_structural = Some(self.index);
        if let Some(node) = anchor {
            self.touch(node, false);
        }
        for (k, b) in fresh.into_iter().enumerate() {
            let node = self.work.add(b, parent);
            self.work.attach(node, parent, rank + k);
        }
        Ok(())
    }

    fn delete_block(&mut self, anchor: &str, allow: bool) -> Result<(), OpRefusal> {
        let node = self.resolve(anchor)?;
        let gone = self.work.subtree(node);
        self.check_protected(&gone, allow)?;
        self.last_structural = Some(self.index);
        self.touch_subtree(node);
        let (parent, _) = self.work.detach(node);
        self.work.kill(node);
        self.check_left(parent)
    }

    fn replace_block(
        &mut self,
        anchor: &str,
        blocks: &[Block],
        allow: bool,
    ) -> Result<(), OpRefusal> {
        let node = self.resolve(anchor)?;
        let (kind, id) = {
            let b = &self.work.nodes[node].block;
            (b.kind.clone(), b.id.clone())
        };
        let mut fresh = fresh_ids(blocks);
        let heir = fresh.iter().position(|b| b.kind == kind);
        if let Some(k) = heir {
            fresh[k].id = id;
        }
        let gone: Vec<usize> = self
            .work
            .subtree(node)
            .into_iter()
            .filter(|&n| n != node || heir.is_none())
            .collect();
        self.check_protected(&gone, allow)?;
        self.check_media(&fresh)?;
        let parent = self.work.nodes[node].parent;
        self.check_fits(&fresh, parent)?;
        self.last_structural = Some(self.index);
        self.touch_subtree(node);
        let (parent, rank) = self.work.detach(node);
        self.work.kill(node);
        let empty = fresh.is_empty();
        for (k, b) in fresh.into_iter().enumerate() {
            let added = self.work.add(b, parent);
            self.work.attach(added, parent, rank + k);
        }
        if empty {
            self.check_left(parent)?;
        }
        Ok(())
    }

    fn move_block(&mut self, anchor: &str, to: &Place) -> Result<(), OpRefusal> {
        let node = self.resolve(anchor)?;
        let target = match to {
            Place::After(a) | Place::Before(a) => Some(self.resolve(a)?),
            Place::Start | Place::End => None,
        };
        if let Some(t) = target {
            if t == node {
                return Ok(());
            }
            if self.work.is_inside(t, node) {
                return Err(self.refuse(
                    Reason::InvalidMove,
                    "un bloc ne va pas dans ses propres sous-blocs : choisis une place hors de \
                     lui."
                        .into(),
                ));
            }
        }
        self.last_structural = Some(self.index);
        self.touch_subtree(node);
        if let Some(t) = target {
            self.touch(t, false);
        }
        let moved = self.work.to_block(node);
        let (old_parent, _) = self.work.detach(node);
        let (parent, rank, _) = self.place(to)?;
        self.check_fits(std::slice::from_ref(&moved), parent)?;
        self.work.attach(node, parent, rank);
        self.check_left(old_parent)
    }

    fn replace_page(&mut self, blocks: &[Block], allow: bool) -> Result<(), OpRefusal> {
        let mut written = blocks.to_vec();
        self.resolve_written(&mut written)?;
        let now = self.work.blocks();
        keep_ids(&now, &mut written);
        let written = crate::anchors::with_ids(&written);
        let target = match self.ctx.base {
            Base::Current => written,
            Base::Changed(base) => match three_way(base, &written, &now, self.ctx.key) {
                Ok(merged) => merged,
                Err(conflict) => {
                    return Err(OpRefusal {
                        blocks: conflict.blocks,
                        ..self.refuse(
                            Reason::RevisionConflict,
                            "la page a changé depuis ta lecture, dans des blocs que ce que tu \
                             écris change aussi : relis-les, puis réécris avec la révision \
                             courante."
                                .into(),
                        )
                    });
                }
            },
        };
        let mut kept: HashSet<&str> = HashSet::new();
        collect_ids(&target, &mut kept);
        let gone: Vec<usize> = self
            .work
            .preorder()
            .into_iter()
            .filter(|&n| !kept.contains(self.work.nodes[n].block.id.as_str()))
            .collect();
        self.check_protected(&gone, allow)?;
        self.check_media(&target)?;
        check_blocks(&normalize(target.clone()), Slot::Group, 2)
            .map_err(|e| structure_refusal(self.index, &e))?;
        self.last_structural = Some(self.index);
        // Les blocs touchés : retirés, ou dont le contenu ou la place changent. La fusion à trois
        // les a jugés, contenu et place à part : ils ne se jugent pas de nouveau ici.
        let written: HashMap<u64, (u64, u64)> = hashes(&target, self.ctx.key)
            .as_slice()
            .iter()
            .map(|h| (h.id, (h.content, h.place)))
            .collect();
        let before = hashes(&now, self.ctx.key);
        let order = self.work.preorder();
        for (node, h) in order.into_iter().zip(before.as_slice()) {
            if written.get(&h.id) != Some(&(h.content, h.place)) {
                let id = self.work.nodes[node].block.id.clone();
                self.record(id);
            }
        }
        self.work = Work::of(target);
        Ok(())
    }

    /// Les ancres d'un MMD de `replace_page` : un alias ou un identifiant de la page lue désigne
    /// son bloc ; un alias périmé ou ambigu est refusé ; un autre identifiant est oublié (le bloc
    /// est neuf).
    fn resolve_written(&self, blocks: &mut [Block]) -> Result<(), OpRefusal> {
        for b in blocks {
            if !b.id.is_empty() {
                match self.resolve(&b.id) {
                    Ok(node) => b.id = self.work.nodes[node].block.id.clone(),
                    Err(refusal)
                        if refusal.reason == Reason::StaleAnchor
                            && !crate::anchors::is_alias(&b.id) =>
                    {
                        b.id.clear();
                    }
                    Err(refusal) => return Err(refusal),
                }
            }
            self.resolve_written(&mut b.children)?;
        }
        Ok(())
    }
}

// ===================================================================================== aides

fn collect_ids<'b>(blocks: &'b [Block], into: &mut HashSet<&'b str>) {
    for b in blocks {
        if !b.id.is_empty() {
            into.insert(&b.id);
        }
        collect_ids(&b.children, into);
    }
}

/// Les blocs, chacun sous un identifiant neuf (R1232).
fn fresh_ids(blocks: &[Block]) -> Vec<Block> {
    fn renew(blocks: &mut [Block]) {
        for b in blocks {
            b.id = Uuid::now_v7().to_string();
            renew(&mut b.children);
        }
    }
    let mut blocks = blocks.to_vec();
    renew(&mut blocks);
    blocks
}

/// L'adresse d'un bloc image, vidéo ou son.
fn media_url(b: &Block) -> Option<&str> {
    if !MEDIA.contains(&b.kind.as_str()) {
        return None;
    }
    match b.props.get("url") {
        Some(mementum_md::PropValue::Str(url)) => Some(url),
        _ => None,
    }
}

/// Vrai si `url` est exactement l'adresse d'un fichier de l'instance, sous la forme que l'éditeur
/// montre (`/api/v1/pages/<uuid>/files/<uuid>`, chaque identifiant à tirets, sans casse ; la règle
/// de `file_address` du serveur et de `filePath` du client, table `spec/files/addresses.json`).
fn is_file_address(url: &str) -> bool {
    fn hyphenated(id: &str) -> bool {
        id.len() == 36 && Uuid::parse_str(id).is_ok()
    }
    url.strip_prefix("/api/v1/pages/")
        .and_then(|rest| rest.split_once("/files/"))
        .is_some_and(|(page, file)| hyphenated(page) && hyphenated(file))
}

/// Les places (en caractères du texte brut) de chaque `old` de `text`, sans chevauchement, dans
/// l'ordre.
fn char_ranges(text: &str, old: &str) -> Vec<(usize, usize)> {
    let old_chars = old.chars().count();
    let (mut byte, mut chars) = (0, 0);
    text.match_indices(old)
        .map(|(at, _)| {
            let start = chars + text[byte..at].chars().count();
            byte = at + old.len();
            chars = start + old_chars;
            (start, chars)
        })
        .collect()
}

/// Un morceau d'un contenu en ligne à plat : un texte, ses marques et son lien ; ou un élément en
/// ligne (un caractère du texte brut).
enum Atom {
    Text {
        text: String,
        styles: Styles,
        link: Option<String>,
    },
    Element(Inline),
}

fn flatten(content: &[Inline], link: Option<&str>, out: &mut Vec<Atom>) {
    for inline in content {
        match inline {
            Inline::Text { text, styles } => out.push(Atom::Text {
                text: text.clone(),
                styles: styles.clone(),
                link: link.map(str::to_string),
            }),
            Inline::Link { href, children } => flatten(children, Some(href), out),
            other => out.push(Atom::Element(other.clone())),
        }
    }
}

/// Les marques d'un caractère : ses styles et son lien.
type Marks = (Styles, Option<String>);

/// Remplace dans `content` chaque place de `ranges` (caractères du texte brut, rangées, sans
/// chevauchement) par `new`, qui prend les marques du premier caractère remplacé s'il est un
/// texte, sinon celles du caractère d'avant, sinon aucune ; un saut de ligne de `new` devient un
/// saut de ligne du bloc, sauf dans un bloc de code (`plain`). En une passe sur le contenu.
fn replace_ranges(
    content: &[Inline],
    ranges: &[(usize, usize)],
    new: &str,
    plain: bool,
) -> Vec<Inline> {
    let mut atoms = Vec::new();
    flatten(content, None, &mut atoms);
    let mut out: Vec<Atom> = Vec::new();
    let mut next = 0; // la prochaine place
    let mut inside = false;
    let mut pos = 0; // le caractère courant, dans le texte brut
    let mut previous: Option<Marks> = None; // les marques du caractère d'avant, s'il est un texte
    let push_new = |out: &mut Vec<Atom>, marks: Option<Marks>| {
        let (styles, link) = marks.unwrap_or_default();
        let mut parts = if plain {
            vec![new]
        } else {
            new.split('\n').collect()
        };
        let last = parts.pop().unwrap_or_default();
        for part in parts {
            out.push(Atom::Text {
                text: part.to_string(),
                styles: styles.clone(),
                link: link.clone(),
            });
            out.push(Atom::Element(Inline::HardBreak));
        }
        out.push(Atom::Text {
            text: last.to_string(),
            styles,
            link,
        });
    };
    // Une place finit ou commence avant le caractère `pos`.
    let at_boundary = |pos: usize, inside: bool, next: usize| {
        (inside && ranges[next].1 == pos)
            || (!inside && next < ranges.len() && ranges[next].0 == pos)
    };
    // Avant le caractère `pos` : ferme la place qui finit là, ouvre celle qui commence là (deux
    // places peuvent se toucher), en posant `new` sous les marques `here` (le caractère `pos`, s'il
    // est un texte), à défaut celles du caractère d'avant.
    let boundary = |pos: usize,
                    here: Option<Marks>,
                    previous: &Option<Marks>,
                    out: &mut Vec<Atom>,
                    inside: &mut bool,
                    next: &mut usize| {
        if *inside && ranges[*next].1 == pos {
            *inside = false;
            *next += 1;
        }
        if !*inside && *next < ranges.len() && ranges[*next].0 == pos {
            push_new(out, here.or_else(|| previous.clone()));
            *inside = true;
        }
    };
    for atom in atoms {
        match atom {
            Atom::Element(inline) => {
                boundary(pos, None, &previous, &mut out, &mut inside, &mut next);
                if !inside {
                    out.push(Atom::Element(inline));
                }
                previous = None;
                pos += 1;
            }
            Atom::Text { text, styles, link } => {
                let marks: Marks = (styles.clone(), link.clone());
                let mut kept = String::new();
                for c in text.chars() {
                    if at_boundary(pos, inside, next) {
                        if !kept.is_empty() {
                            out.push(Atom::Text {
                                text: std::mem::take(&mut kept),
                                styles: styles.clone(),
                                link: link.clone(),
                            });
                        }
                        let here = Some(marks.clone());
                        boundary(pos, here, &previous, &mut out, &mut inside, &mut next);
                    }
                    if !inside {
                        kept.push(c);
                    }
                    previous = Some(marks.clone());
                    pos += 1;
                }
                if !kept.is_empty() {
                    out.push(Atom::Text {
                        text: kept,
                        styles,
                        link,
                    });
                }
            }
        }
    }
    boundary(pos, None, &previous, &mut out, &mut inside, &mut next);
    unflatten(out)
}

/// Le contenu en ligne de morceaux à plat : les textes voisins d'un même lien sous ce lien.
fn unflatten(atoms: Vec<Atom>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for atom in atoms {
        match atom {
            Atom::Element(inline) => out.push(inline),
            Atom::Text { text, .. } if text.is_empty() => {}
            Atom::Text {
                text,
                styles,
                link: None,
            } => out.push(Inline::Text { text, styles }),
            Atom::Text {
                text,
                styles,
                link: Some(href),
            } => match out.last_mut() {
                Some(Inline::Link {
                    href: last,
                    children,
                }) if *last == href => children.push(Inline::Text { text, styles }),
                _ => out.push(Inline::Link {
                    href,
                    children: vec![Inline::Text { text, styles }],
                }),
            },
        }
    }
    out
}
