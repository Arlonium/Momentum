//! Tableaux au format Yjs de BlockNote 0.57 :
//! `table > tableRow > tableHeader | tableCell > tableParagraph`.
//!
//! BlockNote n'écrit ni `headerRows` ni `columnWidths` dans Yjs (Ruling R15) :
//! une cellule d'en-tête est un `tableHeader`, et la largeur d'une colonne est
//! l'attribut `colwidth` (`[largeur]`) de ses cellules. Les props de cellule
//! sont des attributs du nœud de cellule, défauts compris.

use mementum_md::{Cell, Inline, Props, Table};
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
/// leur forme canonique (Ruling R19).
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
            if first {
                column_widths.push(colwidth(txn, &cell));
            }
            header.push(tag == "tableHeader");
            cells.push(Cell {
                content: cell_inline(txn, &cell),
                props: read_props(txn, &cell, cell_props(), &[COLWIDTH]),
            });
        }
        if cells.is_empty() {
            cells.push(empty_cell());
            header.push(false);
            if first {
                column_widths.push(None);
            }
        }
        rows.push(cells);
        headers.push(header);
    }
    if rows.is_empty() {
        rows.push(vec![empty_cell()]);
        headers.push(vec![false]);
        column_widths = vec![None];
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

/// Largeur d'une colonne : première valeur de `colwidth`, finie et dans les
/// entiers sûrs (au-delà, `parse` ne relirait pas la liste `widths`).
fn colwidth<T: ReadTxn>(txn: &T, cell: &XmlElementRef) -> Option<f64> {
    let width = match cell.get_attribute(txn, COLWIDTH) {
        Some(Out::Any(Any::Array(values))) => values.first().cloned(),
        Some(Out::Any(value @ Any::Number(_))) => Some(value),
        _ => None,
    };
    match width {
        Some(Any::Number(n)) => n.as_f64().filter(|w| w.is_finite() && w.abs() <= MAX_SAFE),
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
/// colonne d'en-tête, `colwidth` d'après les largeurs. Un tableau sans ligne
/// (ou une ligne sans cellule) reçoit une cellule vide, comme à la lecture.
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
    for (r, row) in rows.iter().enumerate() {
        let tr = element.push_back(txn, XmlElementPrelim::empty("tableRow"));
        for (c, cell) in row.iter().enumerate() {
            let header = r < header_rows || c < header_cols;
            let tag = if header { "tableHeader" } else { "tableCell" };
            let td = tr.push_back(txn, XmlElementPrelim::empty(tag));
            write_props(txn, &td, &cell.props, cell_props());
            if let Some(Some(width)) = widths.get(c) {
                td.insert_attribute(txn, COLWIDTH, Any::Array(vec![js_number(*width)].into()));
            }
            let paragraph = td.push_back(txn, XmlElementPrelim::empty("tableParagraph"));
            write_inline(txn, &paragraph, &cell.content);
        }
    }
}
