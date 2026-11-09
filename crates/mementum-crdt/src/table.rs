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

use mementum_md::{Cell, Inline, MAX_WIDTH_COLUMNS, Props, Table, cell_rowspan, cell_span};
use mementum_schema::cell_props;
use yrs::{Any, Out, ReadTxn, TransactionMut, Xml, XmlElementPrelim, XmlElementRef, XmlFragment};

use crate::blocknote::{
    MAX_SAFE, elements, js_number, read_inline, read_props, write_inline, write_props,
};

const COLWIDTH: &str = "colwidth";

fn empty_cell() -> Cell {
    Cell {
        content: vec![],
        props: Props::new(),
    }
}

/// Tableau : au moins une ligne, au moins une cellule par ligne. Les en-têtes
/// sont lus sur les cellules `tableHeader` : lignes entières en tête du
/// tableau, colonnes entières à gauche ; `normalize` les met ensuite sous
/// leur forme canonique (Ruling R19). Les largeurs sont lues comme BlockNote
/// lit `columnWidths` : les `colwidth` des cellules de la première ligne,
/// mis bout à bout.
pub(crate) fn read_table<T: ReadTxn>(txn: &T, table: &XmlElementRef) -> Table {
    let (mut rows, mut headers, mut column_widths) = (Vec::new(), Vec::new(), Vec::new());
    for row in elements(txn, table) {
        if row.tag().as_ref() != "tableRow" {
            continue;
        }
        let first = rows.is_empty();
        let (mut cells, mut header) = (Vec::new(), Vec::new());
        for cell in elements(txn, &row) {
            let tag = cell.tag().as_ref();
            if tag != "tableCell" && tag != "tableHeader" {
                continue;
            }
            header.push(tag == "tableHeader");
            let read = Cell {
                content: cell_inline(txn, &cell),
                props: read_props(txn, &cell, cell_props(), &[COLWIDTH]),
            };
            if first {
                push_widths(&mut column_widths, cell.get_attribute(txn, COLWIDTH), &read);
            }
            cells.push(read);
        }
        if cells.is_empty() {
            cells.push(empty_cell());
            header.push(false);
        }
        rows.push(cells);
        headers.push(header);
    }
    if rows.is_empty() {
        rows.push(vec![empty_cell()]);
        headers.push(vec![false]);
    }
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

/// Ajoute les largeurs d'une cellule de la première ligne, comme BlockNote :
/// les valeurs de son `colwidth`, ou sans `colwidth` une largeur automatique
/// par colonne qu'elle couvre (`colspan`). Au plus [`MAX_WIDTH_COLUMNS`] en
/// tout : `normalize` complète ensuite jusqu'à la largeur de la grille.
fn push_widths(widths: &mut Vec<Option<f64>>, colwidth: Option<Out>, cell: &Cell) {
    let room = MAX_WIDTH_COLUMNS.saturating_sub(widths.len());
    match colwidth {
        Some(Out::Any(Any::Array(values))) => {
            widths.extend(values.iter().take(room).map(width));
        }
        Some(Out::Any(value @ Any::Number(_))) => {
            widths.extend(std::iter::once(width(&value)).take(room));
        }
        _ => widths.extend(std::iter::repeat_n(None, cell_span(cell).min(room))),
    }
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
