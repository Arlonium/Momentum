//! Tableaux au format Yjs de BlockNote 0.57 :
//! `table > tableRow > tableHeader | tableCell > tableParagraph`.
//!
//! BlockNote n'écrit ni `headerRows` ni `columnWidths` dans Yjs (Ruling R15) :
//! une cellule d'en-tête est un `tableHeader`, et les largeurs sont
//! l'attribut `colwidth` de chaque cellule (prosemirror-tables), une valeur
//! par colonne de la grille qu'elle couvre : `[120]`, ou `[120, undefined]`
//! pour une cellule fusionnée sur deux colonnes dont seule la première a une
//! largeur. Les props de cellule sont des attributs du nœud de cellule,
//! défauts compris.
//!
//! Un client Yjs écrit les cellules et les fusions qu'il veut : des lignes
//! inégales, des `colspan` et `rowspan` qui sortent du tableau ou se
//! recouvrent. Le serveur ne récrit pas un tel tableau (Ruling R137) : il le
//! lit toujours en une grille que `mementum_md::parse` accepte (la règle de
//! `check_grid`, Ruling R37), sans rien écrire dans le document. Voir
//! [`read_table`].

use std::collections::BTreeMap;

use mementum_md::{
    Cell, Inline, MAX_WIDTH_COLUMNS, PropValue, Props, Table, cell_rowspan, cell_span,
};
use mementum_schema::cell_props;
use yrs::{Any, Out, ReadTxn, TransactionMut, Xml, XmlElementPrelim, XmlElementRef, XmlFragment};

use crate::blocknote::{
    MAX_SAFE, elements, js_number, read_inline, read_props, write_inline, write_props,
};

const COLWIDTH: &str = "colwidth";

/// Marge du bourrage : la lecture en grille ajoute au plus autant de cellules
/// vides que de cellules lues, plus cette marge (Ruling R147). La part
/// relative couvre les tableaux qu'une édition concurrente a rendus inégaux ;
/// la marge, les tout petits tableaux inégaux (une ligne de cinq cellules et
/// deux lignes d'une : huit vides pour sept lues), qui sont ainsi toujours
/// bourrés. Elle vaut par tableau, et une page en porte plusieurs : elle
/// reste petite. Au pire (deux cellules, dont un `colspan` de 19 sur deux
/// lignes que l'autre coupe), un tableau de 163 octets de Yjs se lit en 20
/// cellules ; mesuré en release sur 10 000 tels tableaux, 0,09 s, contre
/// 0,06 s pour des paragraphes de même taille (une marge de 100 en faisait
/// lire 101 par tableau).
const PADDING_ALLOWANCE: usize = 16;

fn empty_cell() -> Cell {
    Cell {
        content: vec![],
        props: Props::new(),
    }
}

/// Une cellule telle que le document la porte : la cellule, sa balise
/// (`tableHeader`) et son `colwidth`, que seule la grille sait lire.
struct ReadCell {
    cell: Cell,
    header: bool,
    colwidth: Option<Out>,
}

impl ReadCell {
    /// La cellule vide que reçoit une ligne qui n'en a pas.
    fn empty() -> Self {
        ReadCell {
            cell: empty_cell(),
            header: false,
            colwidth: None,
        }
    }
}

/// Tableau : au moins une ligne, au moins une cellule par ligne (une ligne
/// sans cellule en reçoit une vide, un tableau sans ligne une ligne), en une
/// grille que `parse` accepte, quoi que porte le document (Ruling R137). Les
/// cellules sont lues dans l'ordre, aucune n'est perdue ; voir [`grid`] pour
/// la grille, et [`one_column`] quand il faudrait trop de cellules vides
/// pour la fermer (Ruling R147).
///
/// Les en-têtes sont lus sur les balises des cellules de la grille obtenue :
/// lignes entières en tête du tableau, colonnes entières à gauche ;
/// `normalize` les met ensuite sous leur forme canonique (Ruling R19). Les
/// largeurs sont celles des colonnes que couvre chaque cellule de la
/// première ligne obtenue : les premières valeurs de son `colwidth`, une par
/// colonne, comme prosemirror-tables les lit (Ruling R149).
pub(crate) fn read_table<T: ReadTxn>(txn: &T, table: &XmlElementRef) -> Table {
    let mut rows = Vec::new();
    for row in elements(txn, table) {
        if row.tag().as_ref() != "tableRow" {
            continue;
        }
        let mut cells = Vec::new();
        for cell in elements(txn, &row) {
            let tag = cell.tag().as_ref();
            if tag != "tableCell" && tag != "tableHeader" {
                continue;
            }
            cells.push(ReadCell {
                header: tag == "tableHeader",
                colwidth: cell.get_attribute(txn, COLWIDTH),
                cell: Cell {
                    content: cell_inline(txn, &cell),
                    props: read_props(txn, &cell, cell_props(), &[COLWIDTH]),
                },
            });
        }
        if cells.is_empty() {
            cells.push(ReadCell::empty());
        }
        rows.push(cells);
    }
    if rows.is_empty() {
        rows.push(vec![ReadCell::empty()]);
    }
    let wanted: Vec<Vec<Span>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| Span {
                    colspan: cell_span(&c.cell),
                    rowspan: cell_rowspan(&c.cell),
                })
                .collect()
        })
        .collect();
    match grid(&wanted) {
        Some(grid) => in_grid(rows, &grid),
        None => one_column(rows),
    }
}

/// Fusions d'une cellule : colonnes et lignes de la grille qu'elle couvre.
#[derive(Clone, Copy)]
struct Span {
    colspan: usize,
    rowspan: usize,
}

/// La grille obtenue : les fusions de chaque cellule lue, et le nombre de
/// cellules vides ajoutées au bout de chaque ligne.
struct Grid {
    spans: Vec<Vec<Span>>,
    padding: Vec<usize>,
}

/// Grille d'un tableau dont `wanted` donne les fusions écrites, `colspan`
/// déjà ramené entre 1 et [`MAX_WIDTH_COLUMNS`] et `rowspan` au moins 1
/// ([`cell_span`], [`cell_rowspan`]). C'est la règle de `check_grid` (Ruling
/// R37), lue à l'envers : ce qu'il refuserait est ramené à ce qu'il accepte.
///
/// - La largeur est celle de la ligne qui couvre le moins de colonnes,
///   fusions et cellules d'au-dessus comptées, mais jamais moins que le
///   nombre de cellules de la plus longue ligne : aucune cellule n'est
///   perdue, et une fusion ne rend pas la grille plus large que ce que les
///   autres lignes portent ([`grid_width`], Ruling R148). Pour une grille
///   déjà valide, c'est sa largeur, et rien ne change.
/// - Les lignes se lisent de haut en bas, chaque cellule à la première
///   colonne libre après la précédente, comme dans prosemirror-tables.
/// - Si les cellules d'une ligne n'ont pas chacune une colonne libre, les
///   `rowspan` d'au-dessus qui la couvrent s'arrêtent avant elle, du plus à
///   gauche au plus à droite, autant qu'il faut (Ruling R148).
/// - Un `colspan` est ramené avant la colonne suivante prise par une cellule
///   d'au-dessus, et à ce qui reste de la ligne une fois gardée une colonne
///   pour chacune des cellules suivantes ; un `rowspan`, au nombre de lignes
///   restantes, ligne courante comprise.
/// - Une ligne qui couvre encore moins de colonnes que la grille reçoit des
///   cellules vides au bout.
///
/// `None` si ces cellules vides seraient plus nombreuses que les cellules
/// lues plus [`PADDING_ALLOWANCE`] (Ruling R147) : le calcul s'arrête alors,
/// dès la ligne qui dépasse, sans rien allouer de plus.
/// Sinon, le coût est en `O(n log n)` pour `n` cellules, quels que soient
/// la largeur de la grille et les `colspan` ou `rowspan` écrits.
fn grid(wanted: &[Vec<Span>]) -> Option<Grid> {
    let height = wanted.len();
    let width = grid_width(wanted);
    let cells: usize = wanted.iter().map(Vec::len).sum();
    let mut spans: Vec<Vec<Span>> = Vec::with_capacity(height);
    let mut padding = Vec::with_capacity(height);
    let mut padded = 0usize;
    let mut taken = Taken::default();
    // Cellules d'au-dessus qui couvrent encore la ligne, par colonne de
    // début ; et, par ligne, celles dont le `rowspan` s'y arrête.
    let mut covering: BTreeMap<usize, Cover> = BTreeMap::new();
    let mut ending: Vec<Vec<usize>> = vec![Vec::new(); height + 1];
    for (r, row) in wanted.iter().enumerate() {
        for start in std::mem::take(&mut ending[r]) {
            // Une cellule raccourcie plus haut n'est plus à libérer ici.
            if covering.get(&start).is_some_and(|c| c.until == r) {
                let cover = covering.remove(&start).expect("cellule qui couvre");
                taken.release(start, cover.end);
            }
        }
        while width.saturating_sub(taken.width) < row.len() {
            // La largeur n'est jamais sous le nombre de cellules d'une ligne :
            // libérer toutes les colonnes suffit toujours.
            let Some((start, cover)) = covering.pop_first() else {
                break;
            };
            taken.release(start, cover.end);
            spans[cover.row][cover.index].rowspan = r - cover.row;
        }
        let mut placed = Vec::with_capacity(row.len());
        let mut starts = Vec::with_capacity(row.len());
        // `cursor` : fin de la cellule précédente ; `left` : colonnes prises
        // par une cellule d'au-dessus avant `cursor`.
        let (mut cursor, mut left) = (0usize, 0usize);
        for (i, want) in row.iter().enumerate() {
            let start = taken.first_free(cursor);
            left += start - cursor;
            let next = taken.next_start(start).unwrap_or(width);
            let right = taken.width - left;
            let later = row.len() - i - 1;
            // Colonnes libres de `start` à la fin de la ligne, moins une par
            // cellule suivante : au moins une, puisque chaque cellule de la
            // ligne a une colonne libre.
            let room = width
                .saturating_sub(start)
                .saturating_sub(right)
                .saturating_sub(later)
                .max(1);
            let colspan = want
                .colspan
                .min(next.saturating_sub(start))
                .min(room)
                .max(1);
            let rowspan = want.rowspan.min(height - r);
            placed.push(Span { colspan, rowspan });
            starts.push(start);
            cursor = start + colspan;
        }
        let covered = taken.width + placed.iter().map(|s| s.colspan).sum::<usize>();
        let pad = width.saturating_sub(covered);
        padded = padded.saturating_add(pad);
        if padded > cells.saturating_add(PADDING_ALLOWANCE) {
            return None;
        }
        for (index, (&start, span)) in starts.iter().zip(&placed).enumerate() {
            if span.rowspan > 1 {
                let end = start + span.colspan;
                let until = r + span.rowspan;
                taken.occupy(start, end);
                covering.insert(
                    start,
                    Cover {
                        end,
                        until,
                        row: r,
                        index,
                    },
                );
                ending[until].push(start);
            }
        }
        spans.push(placed);
        padding.push(pad);
    }
    Some(Grid { spans, padding })
}

/// Une cellule qui couvre des lignes sous la sienne : sa dernière colonne
/// (exclue), la ligne où elle s'arrête (exclue), et sa place dans le tableau.
struct Cover {
    end: usize,
    until: usize,
    row: usize,
    index: usize,
}

/// Largeur de la grille (voir [`grid`]) : la plus petite des largeurs que
/// couvrent les lignes, chacune avec ses `colspan` et ceux des cellules
/// d'au-dessus qui descendent jusqu'à elle (comptés comme prosemirror-tables
/// les compte, même s'ils se recouvrent), et au moins le nombre de cellules
/// de la plus longue ligne.
fn grid_width(wanted: &[Vec<Span>]) -> usize {
    let height = wanted.len();
    let mut ending = vec![0usize; height + 1];
    let (mut above, mut narrowest, mut longest) = (0usize, usize::MAX, 0usize);
    for (r, row) in wanted.iter().enumerate() {
        above -= ending[r];
        let own = row.iter().map(|s| s.colspan).fold(0, usize::saturating_add);
        narrowest = narrowest.min(above.saturating_add(own));
        longest = longest.max(row.len());
        for span in row.iter().filter(|s| s.rowspan > 1) {
            let until = r.saturating_add(span.rowspan).min(height);
            above += span.colspan;
            ending[until] += span.colspan;
        }
    }
    narrowest.max(longest)
}

/// Colonnes prises par une cellule d'au-dessus, en plages qui ne se touchent
/// pas (début → fin exclue), et leur nombre : comme `check_grid`, chaque
/// cellule se place en temps logarithmique, quelle que soit la largeur.
#[derive(Default)]
struct Taken {
    ranges: BTreeMap<usize, usize>,
    width: usize,
}

impl Taken {
    /// Première colonne libre à partir de `column`.
    fn first_free(&self, column: usize) -> usize {
        match self.ranges.range(..=column).next_back() {
            Some((_, &end)) if end > column => end,
            _ => column,
        }
    }

    /// Début de la première plage prise après la colonne libre `column`.
    fn next_start(&self, column: usize) -> Option<usize> {
        self.ranges.range(column..).next().map(|(&start, _)| start)
    }

    /// Prend les colonnes libres `[start, end)`.
    fn occupy(&mut self, mut start: usize, mut end: usize) {
        self.width += end - start;
        if let Some((&before, &stop)) = self.ranges.range(..start).next_back()
            && stop == start
        {
            self.ranges.remove(&before);
            start = before;
        }
        if let Some(after) = self.ranges.remove(&end) {
            end = after;
        }
        self.ranges.insert(start, end);
    }

    /// Libère les colonnes prises `[start, end)`.
    fn release(&mut self, start: usize, end: usize) {
        let Some((&from, &to)) = self.ranges.range(..=start).next_back() else {
            return;
        };
        self.width -= end - start;
        self.ranges.remove(&from);
        if from < start {
            self.ranges.insert(from, start);
        }
        if end < to {
            self.ranges.insert(end, to);
        }
    }
}

/// Le tableau lu en grille : chaque cellule avec ses fusions obtenues, puis
/// les cellules vides de sa ligne, à la balise de la première cellule de la
/// ligne, comme les ajoute `fixTables` (Ruling R148).
fn in_grid(rows: Vec<Vec<ReadCell>>, grid: &Grid) -> Table {
    let mut column_widths = Vec::new();
    let (mut out, mut headers) = (Vec::with_capacity(rows.len()), Vec::new());
    for (r, row) in rows.into_iter().enumerate() {
        let pad_header = row.first().is_some_and(|c| c.header);
        let (mut cells, mut header) = (Vec::new(), Vec::new());
        for (read, span) in row.into_iter().zip(&grid.spans[r]) {
            if r == 0 {
                let room = MAX_WIDTH_COLUMNS.saturating_sub(column_widths.len());
                column_widths
                    .extend(covered_widths(read.colwidth.as_ref(), span.colspan).take(room));
            }
            let mut cell = read.cell;
            set_span(&mut cell.props, "colspan", span.colspan);
            set_span(&mut cell.props, "rowspan", span.rowspan);
            cells.push(cell);
            header.push(read.header);
        }
        let pad = grid.padding[r];
        cells.extend(std::iter::repeat_with(empty_cell).take(pad));
        header.extend(std::iter::repeat_n(pad_header, pad));
        out.push(cells);
        headers.push(header);
    }
    with_headers(out, &headers, column_widths)
}

/// Le tableau lu en une colonne (Ruling R147) : une cellule par ligne, dans
/// l'ordre du document, sans fusion. Sa grille vaut toujours, et sa taille
/// est celle des cellules lues.
fn one_column(rows: Vec<Vec<ReadCell>>) -> Table {
    let mut column_widths = Vec::new();
    let (mut out, mut headers) = (Vec::new(), Vec::new());
    for read in rows.into_iter().flatten() {
        if out.is_empty() {
            column_widths.extend(covered_widths(read.colwidth.as_ref(), 1));
        }
        let mut cell = read.cell;
        cell.props.remove("colspan");
        cell.props.remove("rowspan");
        out.push(vec![cell]);
        headers.push(vec![read.header]);
    }
    with_headers(out, &headers, column_widths)
}

/// Lignes et colonnes d'en-tête lues sur les balises des cellules de la
/// grille : lignes entières en tête du tableau, colonnes entières à gauche
/// (indice de la cellule dans sa ligne, comme BlockNote).
fn with_headers(
    rows: Vec<Vec<Cell>>,
    headers: &[Vec<bool>],
    column_widths: Vec<Option<f64>>,
) -> Table {
    let shortest = headers.iter().map(Vec::len).min().unwrap_or(0);
    let header_rows = headers.iter().take_while(|r| r.iter().all(|&h| h)).count();
    let header_cols = (0..shortest)
        .take_while(|&c| headers.iter().all(|r| r[c]))
        .count();
    Table {
        header_rows: header_rows as u32,
        header_cols: header_cols as u32,
        column_widths,
        rows,
    }
}

/// `colspan` ou `rowspan` obtenu : absent à 1, son défaut.
fn set_span(props: &mut Props, name: &str, value: usize) {
    if value > 1 {
        props.insert(name.into(), PropValue::Int(value as i64));
    } else {
        props.remove(name);
    }
}

/// Largeurs des `span` colonnes qu'une cellule couvre, d'après son
/// `colwidth` : sa valeur pour chaque colonne, automatique au-delà de ses
/// valeurs ; un nombre seul vaut pour la première colonne.
fn covered_widths(colwidth: Option<&Out>, span: usize) -> impl Iterator<Item = Option<f64>> + '_ {
    (0..span).map(move |i| match colwidth {
        Some(Out::Any(Any::Array(values))) => values.get(i).and_then(width),
        Some(Out::Any(value @ Any::Number(_))) if i == 0 => width(value),
        _ => None,
    })
}

/// Une largeur : un nombre fini, dans les entiers sûrs (au-delà, `parse` ne
/// relirait pas la liste `widths`) ; sinon automatique.
fn width(value: &Any) -> Option<f64> {
    match value {
        Any::Number(n) => n.as_f64().filter(|w| w.is_finite() && w.abs() <= MAX_SAFE),
        _ => None,
    }
}

/// Contenu d'une cellule : ses paragraphes, séparés par un saut de ligne.
fn cell_inline<T: ReadTxn>(txn: &T, cell: &XmlElementRef) -> Vec<Inline> {
    let mut out = Vec::new();
    for (i, paragraph) in elements(txn, cell).iter().enumerate() {
        if i > 0 {
            out.push(Inline::HardBreak);
        }
        out.extend(read_inline(txn, paragraph));
    }
    out
}

/// Tableau normalisé : `tableHeader` pour une cellule d'une ligne ou d'une
/// colonne d'en-tête (indice de la cellule dans sa ligne, comme BlockNote),
/// `colwidth` d'après les largeurs des colonnes de la grille que couvre la
/// cellule. Un tableau sans ligne (ou une ligne sans cellule) reçoit une
/// cellule vide, comme à la lecture.
pub(crate) fn write_table(
    txn: &mut TransactionMut,
    element: &XmlElementRef,
    table: Option<&Table>,
) {
    let empty = [empty_cell()];
    let (header_rows, header_cols, widths) = match table {
        Some(t) => (
            t.header_rows as usize,
            t.header_cols as usize,
            t.column_widths.as_slice(),
        ),
        None => (1, 0, &[][..]),
    };
    let mut rows: Vec<&[Cell]> = table
        .map(|t| {
            t.rows
                .iter()
                .map(|r| {
                    if r.is_empty() {
                        &empty[..]
                    } else {
                        r.as_slice()
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    if rows.is_empty() {
        rows.push(&empty);
    }
    let columns = grid_columns(&rows);
    for (r, row) in rows.iter().enumerate() {
        let tr = element.push_back(txn, XmlElementPrelim::empty("tableRow"));
        for (c, cell) in row.iter().enumerate() {
            let header = r < header_rows || c < header_cols;
            let tag = if header { "tableHeader" } else { "tableCell" };
            let td = tr.push_back(txn, XmlElementPrelim::empty(tag));
            write_props(txn, &td, &cell.props, cell_props());
            if let Some(value) = columns[r][c].and_then(|at| colwidth(widths, at, cell)) {
                td.insert_attribute(txn, COLWIDTH, value);
            }
            let paragraph = td.push_back(txn, XmlElementPrelim::empty("tableParagraph"));
            write_inline(txn, &paragraph, &cell.content);
        }
    }
}

/// `colwidth` d'une cellule qui commence en colonne `at` de la grille : une
/// valeur par colonne couverte, `undefined` pour une largeur automatique,
/// comme BlockNote. Aucun attribut si toutes ses colonnes sont automatiques
/// (BlockNote écrit alors `[undefined, undefined]` pour une cellule
/// fusionnée, que prosemirror-tables et BlockNote lisent de la même façon).
fn colwidth(widths: &[Option<f64>], at: usize, cell: &Cell) -> Option<Any> {
    let end = at.saturating_add(cell_span(cell)).min(MAX_WIDTH_COLUMNS);
    let covered: Vec<Option<f64>> = (at..end)
        .map(|c| widths.get(c).copied().flatten())
        .collect();
    covered.iter().any(Option::is_some).then(|| {
        let values: Vec<Any> = covered
            .into_iter()
            .map(|w| w.map_or(Any::Undefined, js_number))
            .collect();
        Any::Array(values.into())
    })
}

/// Colonne de la grille où commence chaque cellule, comme la calculent
/// prosemirror-tables et BlockNote : la première colonne libre après la
/// cellule précédente de sa ligne, en sautant celles qu'occupe une cellule
/// d'une ligne au-dessus (`rowspan`). `None` au-delà des
/// [`MAX_WIDTH_COLUMNS`] premières colonnes, qui seules portent une largeur.
fn grid_columns(rows: &[&[Cell]]) -> Vec<Vec<Option<usize>>> {
    // Pour chaque colonne, la première ligne où elle est libre.
    let mut free_from = [0usize; MAX_WIDTH_COLUMNS];
    let mut out = Vec::with_capacity(rows.len());
    for (r, row) in rows.iter().enumerate() {
        let mut column = 0usize;
        let mut starts = Vec::with_capacity(row.len());
        for cell in row.iter() {
            while column < MAX_WIDTH_COLUMNS && free_from[column] > r {
                column += 1;
            }
            if column >= MAX_WIDTH_COLUMNS {
                starts.push(None);
                continue;
            }
            starts.push(Some(column));
            let end = column.saturating_add(cell_span(cell));
            let until = r.saturating_add(cell_rowspan(cell));
            for free in &mut free_from[column..end.min(MAX_WIDTH_COLUMNS)] {
                *free = (*free).max(until);
            }
            column = end;
        }
        out.push(starts);
    }
    out
}
