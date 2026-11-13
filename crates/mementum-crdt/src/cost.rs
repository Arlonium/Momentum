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
    let linear = (tally.elements as u128)
        .saturating_mul(ELEMENT_PS)
        .saturating_add((tally.text_items as u128).saturating_mul(TEXT_ITEM_PS));
    let lists = tally
        .kinds
        .iter()
        .fold(0u128, |sum, k| sum.saturating_add(k.ps));
    let (heaviest, kind) = KINDS
        .iter()
        .zip(&tally.kinds)
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
            elements: tally.elements,
        }
    };
    WriteCost {
        nanos: u64::try_from(lists.saturating_add(linear) / 1_000).unwrap_or(u64::MAX),
        cause,
    }
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
    text_items: usize,
}

impl Tally {
    /// Une liste de `len` blocs, lignes, cellules ou morceaux, qui coûte `ps` picosecondes.
    fn list(&mut self, kind: ListKind, len: usize, ps: u128) {
        let index = KINDS.iter().position(|k| *k == kind).expect("sorte connue");
        let tally = &mut self.kinds[index];
        tally.ps = tally.ps.saturating_add(ps);
        tally.longest = tally.longest.max(len);
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
                stack.push(&b.children);
            }
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
        }
        for row in rows {
            let cells = row.len().max(1);
            self.list(ListKind::Cells, cells, square(cells) * CELL_PAIR_PS);
            self.elements += cells;
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
