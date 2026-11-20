//! Coût estimé de l'écriture complète d'une page, avant de l'écrire (Ruling R368).
//!
//! L'écriture bâtit le document par insertions successives à un index, et yrs 0.28 retrouve
//! chaque place en reparcourant la liste depuis son début (`Branch::index_to_ptr` pour les
//! éléments, `find_position` pour le texte) : chaque liste coûte en proportion du carré de sa
//! longueur. Ce sont les listes de `write_nodes` et des fonctions qu'il appelle :
//! - les frères d'un même parent (racine, enfants d'un bloc, colonnes d'une liste de colonnes,
//!   blocs d'une colonne) ;
//! - les lignes d'un tableau, et les cellules de chaque ligne ;
//! - les enfants du nœud de contenu d'un bloc ou d'une cellule : un texte par suite de plages,
//!   un élément par mention, formule ou saut de ligne ;
//! - les éléments de chacun de ces textes : une chaîne par plage, plus deux marques (l'une qui
//!   s'ouvre, l'autre qui se ferme) par marque qui change d'une plage à la suivante.
//!
//! S'y ajoute un terme linéaire, par élément écrit. Il couvre l'écriture, et le retrait puis la
//! relecture d'un contenu de même taille : une récriture remplace ce que la page porte, que
//! l'estimation ne connaît pas avant de charger la page ; elle le suppose de la taille du
//! nouveau, le cas d'un agent qui relit une page puis la récrit.
//!
//! Les poids sont mesurés en release, sur trois écritures successives d'une même page, la pire
//! des trois, puis arrondis vers le haut (Ruling R368) ; `measure_worst_accepted_writes`
//! (`mementum-server`) mesure la pire écriture que le budget de `PUT /mmd` accepte.
//!
//! Un bloc étranger (`unknown` à `foreign = true`) compte comme le bloc `unknown` que le MMD
//! porte : `PUT /mmd`, seul à demander ce coût, n'en écrit jamais (il retire `foreign`), et la
//! restauration, qui en écrit, n'est pas plafonnée (Ruling R259).

use mementum_md::{Block, Content, Inline, Styles, Table};
use mementum_schema::{ContentKind, block};

/// Picosecondes par paire d'une liste de blocs frères ou de lignes de tableau (au plus 11,6 ns
/// mesurées, 12 000 lignes d'une cellule).
const SIBLING_PAIR_PS: u128 = 15_000;
/// Picosecondes par paire de cellules d'une même ligne (13,7 ns mesurées à 30 000 cellules).
const CELL_PAIR_PS: u128 = 15_000;
/// Picosecondes par paire d'enfants du nœud de contenu (11,4 ns mesurées à 30 000 mentions et à
/// 40 000 textes et sauts de ligne alternés).
const INLINE_PAIR_PS: u128 = 15_000;
/// Picosecondes par paire d'éléments d'un même texte (5,2 ns mesurées à 20 000 plages alternées,
/// grasses et maigres, 60 000 éléments comptés ; à 6 ns, la récriture par `PUT /mmd` des 12 060
/// plages que le budget acceptait prenait 7,0 s).
const TEXT_PAIR_PS: u128 = 7_000;
/// Picosecondes par élément écrit : bloc, ligne ou cellule de tableau, texte, élément en ligne
/// (environ 12 µs mesurées pour la seule écriture, récriture d'un tableau de 150 000 cellules ;
/// à 15 µs, la récriture par `PUT /mmd`, journal compris, des 222 100 cellules que le budget
/// acceptait prenait 8,4 s).
const ELEMENT_PS: u128 = 20_000_000;
/// Picosecondes par élément d'un texte, chaîne ou marque (environ 2,8 µs mesurées, récriture de
/// 10 000 paragraphes de 20 plages).
const TEXT_ITEM_PS: u128 = 4_000_000;
/// Picosecondes de plus par cellule vide de tableau, au-delà de son élément (Ruling R386) : une
/// cellule écrit deux éléments (la cellule et son paragraphe) et toutes ses props, vide ou non.
/// Comptée comme un seul élément (20 µs), une cellule vide en coûtait environ 32 à la re-relecture
/// du tour 2, sur le poste où les poids ont été mesurés : 3 597 lignes de 100 cellules vides
/// passaient le budget et occupaient l'acteur 11 à 12 s. Une cellule qui porte un texte compte
/// déjà ce texte (un élément de plus et ses chaînes : 44 µs estimées, environ 37 mesurées) ; vide,
/// elle compte 35 µs.
const EMPTY_CELL_EXTRA_PS: u128 = 15_000_000;

/// Une liste que l'écriture bâtit par insertions successives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    /// Des blocs frères.
    Siblings,
    /// Les lignes d'un tableau.
    Rows,
    /// Les cellules d'une ligne de tableau.
    Cells,
    /// Le contenu en ligne d'un bloc ou d'une cellule, compté en morceaux : plages de texte de
    /// marques ou de liens différents, mentions, formules, sauts de ligne.
    Inline,
}

const KINDS: [ListKind; 4] = [
    ListKind::Siblings,
    ListKind::Rows,
    ListKind::Cells,
    ListKind::Inline,
];

/// Ce qui pèse le plus dans le coût estimé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// Les listes d'une sorte, dont la plus longue compte `longest` blocs, lignes, cellules ou
    /// morceaux.
    Lists { kind: ListKind, longest: usize },
    /// Le terme linéaire : `elements` éléments écrits (blocs, lignes et cellules de tableau,
    /// textes et éléments en ligne).
    Volume { elements: usize },
}

/// Coût estimé d'une écriture complète, en nanosecondes d'acteur en release (voir le module),
/// et ce qui y pèse le plus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteCost {
    pub nanos: u64,
    pub cause: Cause,
}

/// Coût estimé de l'écriture de `blocks` à la place de tout le contenu d'une page, tels que
/// `mementum_md::parse` les rend. L'écriture les normalise de nouveau, ce qui ne peut que
/// fusionner des plages ou retirer des textes vides : l'estimation ne descend pas sous ce qui
/// est écrit.
pub fn write_cost(blocks: &[Block]) -> WriteCost {
    let mut tally = Tally::default();
    tally.siblings(blocks);
    tally.finish(0)
}

/// Picosecondes, par bloc de la page et par bloc écrit, de ce que l'écriture par différence fait
/// avant d'écrire : la lecture des blocs de la page, `keep_ids` et le plan (Task S0 du lot 5 :
/// 47 ms pour une page de 10 000 paragraphes récrite d'un MMD de 10 000 blocs, environ 2,4 µs
/// par bloc ; arrondi vers le haut, Ruling R1282).
const DIFF_BLOCK_PS: u128 = 4_000_000;

/// Picosecondes par élément retiré à part (une suite de blocs retirés d'une liste, un contenu
/// vidé, un nœud de contenu remplacé) et par bloc de la page : à la validation de la transaction
/// des suppressions, yrs remplace le contenu de chaque élément retiré par une plage ramassée puis
/// fond ces plages dans la liste des blocs de son client, en décalant ce qui les suit
/// (`squash_left_range_compaction`). Mesuré (Ruling R1284) : un paragraphe sur deux retiré coûte
/// 0,1 s sur 10 000 paragraphes, 0,8 s sur 20 000, 3,8 s sur 40 000, soit de 3 à 4,5 ns par paire
/// (élément retiré, bloc de la page) ; arrondi vers le haut.
const REMOVED_RUN_PS: u128 = 8_000;

/// Coût estimé de l'écriture par différence (`PageDoc::write_diff`) de `written` sur une page
/// dont `read_blocks` rend `current`, avant de l'écrire, en nanosecondes d'acteur en release
/// (Rulings R1254, R1282). Les blocs se répondent comme dans l'écriture (`keep_ids`, puis les
/// blocs qui restent à leur place, liste par liste), et l'estimation compte :
/// - la lecture de la page, `keep_ids` et le plan, par bloc des deux côtés ;
/// - chaque bloc écrit (neuf, recréé parce qu'il a changé de place, ou enfant d'un bloc qui n'en
///   avait pas) comme [`write_cost`] le compte, plus le parcours de sa liste jusqu'à sa place :
///   yrs retrouve la place d'une insertion en reparcourant la liste depuis son début, si bien que
///   des blocs insérés un sur deux dans une longue liste coûtent le carré de sa longueur ;
/// - chaque bloc retiré, et le contenu remplacé d'un bloc gardé, par ses éléments (les retraits se
///   font par identifiant, sans parcours de liste, Ruling R1281) ;
/// - chaque élément retiré à part (une suite de blocs retirés, un contenu récrit en entier, un nœud
///   de contenu remplacé, un groupe d'enfants retiré) par la taille de la page : la validation des
///   suppressions fond leurs plages ramassées dans la liste des blocs de yrs, en décalant ce qui
///   suit (Ruling R1284) ; un bloc sur deux retiré coûte ainsi le carré de la page ;
/// - le contenu changé d'un bloc gardé comme s'il était récrit en entier (le préfixe et le suffixe
///   communs coûtent moins, sauf à récrire toute une page à six plages, où S0 a mesuré 35 µs par
///   bloc, sous les 48 µs comptés ici), et chaque prop changée pour un élément.
///
/// Les poids sont ceux de [`write_cost`] ; ceux de la lecture et des retraits sont propres à la
/// différence.
pub fn diff_cost(current: &[Block], written: &[Block]) -> WriteCost {
    let mut target = crate::diff::prepared(written);
    crate::kept_ids::keep_ids(current, &mut target);
    let mut tally = Tally::default();
    tally.diff_lists(current, &target);
    let page = blocks_in(current);
    let read = ((page + blocks_in(&target)) as u128).saturating_mul(DIFF_BLOCK_PS);
    let removed = (tally.removed_runs as u128)
        .saturating_mul(page as u128)
        .saturating_mul(REMOVED_RUN_PS);
    tally.finish(read.saturating_add(removed))
}

fn blocks_in(blocks: &[Block]) -> usize {
    blocks.iter().map(|b| 1 + blocks_in(&b.children)).sum()
}

fn square(n: usize) -> u128 {
    (n as u128) * (n as u128)
}

/// Les listes d'une sorte : leur coût, en picosecondes, et la longueur de la plus longue.
#[derive(Default, Clone, Copy)]
struct KindTally {
    ps: u128,
    longest: usize,
}

#[derive(Default)]
struct Tally {
    /// Par sorte de liste, dans l'ordre de [`KINDS`].
    kinds: [KindTally; 4],
    elements: usize,
    /// Les cellules vides de tableau, comptées aussi dans `elements`.
    empty_cells: usize,
    text_items: usize,
    /// Pour `diff_cost` : les éléments que l'écriture par différence retire à part.
    removed_runs: usize,
}

impl Tally {
    /// Une liste de `len` blocs, lignes, cellules ou morceaux, qui coûte `ps` picosecondes.
    fn list(&mut self, kind: ListKind, len: usize, ps: u128) {
        let index = KINDS.iter().position(|k| *k == kind).expect("sorte connue");
        let tally = &mut self.kinds[index];
        tally.ps = tally.ps.saturating_add(ps);
        tally.longest = tally.longest.max(len);
    }

    /// Le coût estimé : les listes, le terme linéaire, et `extra` picosecondes de plus, linéaires.
    fn finish(&self, extra: u128) -> WriteCost {
        let linear = (self.elements as u128)
            .saturating_mul(ELEMENT_PS)
            .saturating_add((self.empty_cells as u128).saturating_mul(EMPTY_CELL_EXTRA_PS))
            .saturating_add((self.text_items as u128).saturating_mul(TEXT_ITEM_PS))
            .saturating_add(extra);
        let lists = self
            .kinds
            .iter()
            .fold(0u128, |sum, k| sum.saturating_add(k.ps));
        let (heaviest, kind) = KINDS
            .iter()
            .zip(&self.kinds)
            .max_by_key(|(_, k)| k.ps)
            .map(|(kind, k)| (k, *kind))
            .expect("quatre sortes de listes");
        let cause = if heaviest.ps >= linear {
            Cause::Lists {
                kind,
                longest: heaviest.longest,
            }
        } else {
            Cause::Volume {
                elements: self.elements,
            }
        };
        WriteCost {
            nanos: u64::try_from(lists.saturating_add(linear) / 1_000).unwrap_or(u64::MAX),
            cause,
        }
    }

    /// Des frères, et sous chacun ce qu'il porte, comme `write_nodes` (sans récursion).
    fn siblings(&mut self, blocks: &[Block]) {
        let mut stack = vec![blocks];
        while let Some(blocks) = stack.pop() {
            if blocks.is_empty() {
                continue;
            }
            let n = blocks.len();
            self.list(ListKind::Siblings, n, square(n) * SIBLING_PAIR_PS);
            self.elements += n;
            for b in blocks {
                self.content(b);
                stack.push(&b.children);
            }
        }
    }

    /// Le contenu d'un bloc, comme `write_content` l'écrit.
    fn content(&mut self, b: &Block) {
        // Un type hors registre s'écrit en bloc `unknown`, comme dans `write_content`.
        let content = block(&b.kind)
            .or_else(|| block("unknown"))
            .map(|spec| spec.content);
        match (content, &b.content) {
            (Some(ContentKind::Inline), Content::Inline(content)) => self.inline(content),
            (Some(ContentKind::Table), Content::Table(table)) => self.table(Some(table)),
            (Some(ContentKind::Table), _) => self.table(None),
            _ => {}
        }
    }

    /// Ce que coûte de retirer ce qu'une autre estimation compterait : ses seuls termes
    /// linéaires (un retrait ne parcourt aucune liste).
    fn removing(&mut self, count: impl FnOnce(&mut Tally)) {
        let mut other = Tally::default();
        count(&mut other);
        self.elements += other.elements;
        self.empty_cells += other.empty_cells;
        self.text_items += other.text_items;
    }

    /// Deux listes de frères, comme `PageDoc::write_diff` les écrit (voir [`diff_cost`]).
    fn diff_lists(&mut self, current: &[Block], target: &[Block]) {
        let stay = crate::diff::stays(current, target);
        let mut staying = vec![false; current.len()];
        let mut kept = vec![false; target.len()];
        for &(j, i) in &stay {
            staying[i] = true;
            kept[j] = true;
        }
        for (b, _) in current.iter().zip(&staying).filter(|(_, s)| !**s) {
            self.removing(|t| t.siblings(std::slice::from_ref(b)));
        }
        // Une suite par bloc retiré qui suit un bloc gardé (ou ouvre la liste).
        self.removed_runs += (0..current.len())
            .filter(|&i| !staying[i] && (i == 0 || staying[i - 1]))
            .count();
        let mut walks = 0u128;
        for (j, b) in target.iter().enumerate().filter(|(j, _)| !kept[*j]) {
            walks = walks.saturating_add((j as u128).saturating_mul(SIBLING_PAIR_PS));
            self.siblings(std::slice::from_ref(b));
        }
        if walks > 0 {
            self.list(ListKind::Siblings, target.len(), walks);
        }
        for (j, i) in stay {
            self.diff_block(&current[i], &target[j]);
        }
    }

    /// Un bloc gardé à sa place.
    fn diff_block(&mut self, current: &Block, target: &Block) {
        use crate::diff::{Sort, sort_of};
        if current.props != target.props {
            self.elements += 1;
        }
        if sort_of(&target.kind) != Sort::Container {
            self.diff_lists(&current.children, &target.children);
            return;
        }
        if current.kind != target.kind || current.content != target.content {
            self.content(target);
            self.removing(|t| t.content(current));
            self.elements += 1;
            let by_affix = current.kind == target.kind
                && matches!(
                    (&current.content, &target.content),
                    (Content::Inline(a), Content::Inline(b)) if crate::diff::affix_fits(a, b)
                );
            if !by_affix {
                self.removed_runs += 1;
            }
        }
        match (current.children.is_empty(), target.children.is_empty()) {
            (true, false) => self.siblings(&target.children),
            (false, true) => {
                self.removing(|t| t.siblings(&current.children));
                self.removed_runs += 1;
            }
            (false, false) => self.diff_lists(&current.children, &target.children),
            (true, true) => {}
        }
    }

    /// Un tableau, comme `write_table` : une ligne d'une cellule vide pour un tableau sans
    /// ligne, une cellule vide pour une ligne qui n'en a pas.
    fn table(&mut self, table: Option<&Table>) {
        let rows = table.map_or(&[][..], |t| t.rows.as_slice());
        let n = rows.len().max(1);
        self.list(ListKind::Rows, n, square(n) * SIBLING_PAIR_PS);
        self.elements += n;
        if rows.is_empty() {
            self.elements += 1;
            self.empty_cells += 1;
        }
        for row in rows {
            let cells = row.len().max(1);
            self.list(ListKind::Cells, cells, square(cells) * CELL_PAIR_PS);
            self.elements += cells;
            // Une ligne sans cellule en reçoit une vide.
            self.empty_cells += row
                .iter()
                .filter(|c| c.content.is_empty())
                .count()
                .max(cells - row.len());
            for cell in row {
                self.inline(&cell.content);
            }
        }
    }

    /// Le contenu en ligne d'un nœud, comme `write_inline` : ses enfants (un texte par suite de
    /// plages, un élément par mention, formule ou saut de ligne) et les éléments de chacun de ses
    /// textes. Le tout compte pour une liste, longue de ses morceaux.
    fn inline(&mut self, content: &[Inline]) {
        let mut leaves = Vec::new();
        flatten(content, None, &mut leaves);
        if leaves.is_empty() {
            return;
        }
        let (mut ps, mut children) = (0u128, 0usize);
        let mut text = Text::default();
        for leaf in &leaves {
            match *leaf {
                Leaf::Run(styles, link) => {
                    if text.items == 0 {
                        // Le premier morceau d'un texte ouvre un `XmlText`.
                        children += 1;
                    }
                    text.push(styles, link);
                }
                Leaf::Element => {
                    ps += self.close(&mut text);
                    children += 1;
                }
            }
        }
        ps += self.close(&mut text);
        self.elements += children;
        ps += square(children) * INLINE_PAIR_PS;
        self.list(ListKind::Inline, leaves.len(), ps);
    }

    /// Ferme le texte en cours ; rend son coût, en picosecondes.
    fn close(&mut self, text: &mut Text) -> u128 {
        let items = std::mem::take(text).items;
        self.text_items += items;
        square(items) * TEXT_PAIR_PS
    }
}

/// Un morceau du contenu en ligne : une plage de texte non vide (ses marques et son lien), ou un
/// élément (mention, formule, saut de ligne).
enum Leaf<'a> {
    Run(&'a Styles, Option<&'a str>),
    Element,
}

/// Les morceaux, dans l'ordre où `write_inline_item` les écrit ; le lien le plus proche vaut.
fn flatten<'a>(content: &'a [Inline], link: Option<&'a str>, out: &mut Vec<Leaf<'a>>) {
    for inline in content {
        match inline {
            Inline::Text { text, .. } if text.is_empty() => {}
            Inline::Text { styles, .. } => out.push(Leaf::Run(styles, link)),
            Inline::Link { href, children } => flatten(children, Some(href), out),
            Inline::Mention { .. } | Inline::Math { .. } | Inline::HardBreak => {
                out.push(Leaf::Element)
            }
        }
    }
}

/// Le texte en cours : la dernière plage écrite, et ses éléments. La première plage d'un texte
/// change ses marques depuis aucune.
#[derive(Default)]
struct Text<'a> {
    previous: Option<(&'a Styles, Option<&'a str>)>,
    items: usize,
}

impl<'a> Text<'a> {
    fn push(&mut self, styles: &'a Styles, link: Option<&'a str>) {
        let changed = match self.previous {
            Some((before, before_link)) => changed_marks(before, before_link, styles, link),
            None => changed_marks(&Styles::default(), None, styles, link),
        };
        self.items += 1 + 2 * changed;
        self.previous = Some((styles, link));
    }
}

/// Clés d'attribut du texte dont la valeur change d'une plage à la suivante : les cinq marques
/// simples, les deux couleurs et le lien.
fn changed_marks(a: &Styles, a_link: Option<&str>, b: &Styles, b_link: Option<&str>) -> usize {
    [
        a.bold != b.bold,
        a.italic != b.italic,
        a.underline != b.underline,
        a.strike != b.strike,
        a.code != b.code,
        a.text_color != b.text_color,
        a.background_color != b.background_color,
        a_link != b_link,
    ]
    .into_iter()
    .filter(|&changed| changed)
    .count()
}
