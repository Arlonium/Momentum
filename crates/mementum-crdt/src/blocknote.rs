//! Arbre de blocs ⇄ document Yjs au format de BlockNote 0.57 (y-prosemirror 1.x).
//!
//! Structure :
//! - `blockGroup > blockContainer[id] > <type props…>`, suivi d'un
//!   `blockGroup` pour les enfants ;
//! - `columnList[id] > column[id, width] > blockContainer…`, sans
//!   `blockContainer` autour ;
//! - contenu en ligne : `XmlText` dont les marques sont des attributs (simple
//!   `{}`, couleur `{"stringValue": …}`, lien `{"href": …}`), entrecoupé des
//!   éléments `mention`, `math` et `hardBreak` ;
//! - tableau : `table > tableRow > tableHeader | tableCell > tableParagraph`.
//!   Une cellule d'en-tête est un `tableHeader` ; la largeur d'une colonne est
//!   l'attribut `colwidth` (`[largeur]`) de ses cellules.
//!
//! Les props sont des attributs typés du nœud de contenu. L'écriture donne à
//! chaque prop du registre sa valeur ou son défaut, comme BlockNote ; une prop
//! inconnue du registre est copiée telle quelle.
//!
//! La lecture garantit les invariants que `mementum_md::render` suppose
//! (Ruling R10) : props du registre du type déclaré, flottants finis, tableau
//! d'au moins une ligne, imbrication bornée à [`MAX_DEPTH`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mementum_md::{
    Block, Cell, Content, Inline, MAX_DEPTH, PropValue, Props, Styles, Table, normalize,
};
use mementum_schema::{ContentKind, PropDefault, PropSpec, PropType, block, cell_props};
use serde_json::{Map, Value};
use uuid::Uuid;
use yrs::any::Number;
use yrs::types::Attrs;
use yrs::types::text::YChange;
use yrs::{
    Any, Out, ReadTxn, Text, TransactionMut, Xml, XmlElementPrelim, XmlElementRef, XmlFragment,
    XmlFragmentRef, XmlOut, XmlTextPrelim, XmlTextRef,
};

pub(crate) const BLOCK_GROUP: &str = "blockGroup";
pub(crate) const BLOCK_CONTAINER: &str = "blockContainer";
pub(crate) const COLUMN_LIST: &str = "columnList";
pub(crate) const COLUMN: &str = "column";

/// Indices écrits sur `table` quand les types des cellules ne suffisent pas
/// à retrouver `Table.header_rows` / `header_cols` (voir [`derive_headers`]).
/// BlockNote les ignore, et y-prosemirror les efface dès qu'il réécrit le
/// tableau : ils ne servent que s'ils concordent avec les cellules.
const HEADER_ROWS: &str = "headerRows";
const HEADER_COLS: &str = "headerCols";
const COLWIDTH: &str = "colwidth";

/// Plus grand entier qu'un nombre JavaScript représente exactement (2^53 - 1).
const MAX_SAFE: f64 = 9_007_199_254_740_991.0;

/// Clés que le MMD réserve à la forme d'un bloc : une prop inconnue du
/// registre qui porterait l'un de ces noms ne se relirait pas.
const RESERVED: &[&str] = &[
    "id",
    "anchor",
    "title",
    "type",
    "data",
    "header-rows",
    "header-cols",
    "widths",
];

// ===================================================================== lecture

/// Lit la page : les blocs de chaque `blockGroup` racine, normalisés.
pub(crate) fn read_page<T: ReadTxn>(txn: &T, fragment: &XmlFragmentRef) -> Vec<Block> {
    let mut out = Vec::new();
    for child in fragment.children(txn) {
        if let XmlOut::Element(group) = child
            && group.tag().as_ref() == BLOCK_GROUP
        {
            read_nodes(txn, &group, 1, &mut out);
        }
    }
    normalize(out)
}

/// Un bloc lu sans ses enfants, et l'élément qui porte ses enfants.
struct Node {
    block: Block,
    children_of: Option<XmlElementRef>,
}

/// Lit les blocs enfants de `parent`, de niveau `level` (1 au premier niveau).
/// Un bloc de niveau [`MAX_DEPTH`] n'a pas d'enfants : ses descendants le
/// suivent, au même niveau, dans l'ordre du document.
fn read_nodes<T: ReadTxn>(txn: &T, parent: &XmlElementRef, level: usize, out: &mut Vec<Block>) {
    for element in elements(txn, parent) {
        let Some(Node {
            mut block,
            children_of,
        }) = read_node(txn, &element)
        else {
            continue;
        };
        match children_of {
            Some(source) if level < MAX_DEPTH => {
                read_nodes(txn, &source, level + 1, &mut block.children);
                out.push(block);
            }
            Some(source) => {
                out.push(block);
                flatten(txn, &source, out);
            }
            None => out.push(block),
        }
    }
}

/// Descendants de `source`, sans enfants, dans l'ordre du document (sans récursion).
fn flatten<T: ReadTxn>(txn: &T, source: &XmlElementRef, out: &mut Vec<Block>) {
    let mut stack = vec![elements(txn, source).into_iter()];
    while let Some(top) = stack.last_mut() {
        let Some(element) = top.next() else {
            stack.pop();
            continue;
        };
        if let Some(node) = read_node(txn, &element) {
            out.push(node.block);
            if let Some(source) = node.children_of {
                stack.push(elements(txn, &source).into_iter());
            }
        }
    }
}

fn read_node<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> Option<Node> {
    match element.tag().as_ref() {
        BLOCK_CONTAINER => Some(read_container(txn, element)),
        tag @ (COLUMN_LIST | COLUMN) => {
            let spec = block(tag).expect("colonnes dans le registre");
            let block = Block {
                id: attr_string(txn, element, "id"),
                kind: tag.to_string(),
                props: read_props(txn, element, spec.props, &["id"]),
                content: Content::None,
                children: vec![],
            };
            Some(Node {
                block,
                children_of: Some(element.clone()),
            })
        }
        BLOCK_GROUP => None,
        // Nœud étranger hors `blockContainer` : ses enfants vont dans `data`.
        _ => Some(Node {
            block: foreign(txn, element, None),
            children_of: None,
        }),
    }
}

fn read_container<T: ReadTxn>(txn: &T, container: &XmlElementRef) -> Node {
    let id = attr_string(txn, container, "id");
    let (mut content, mut group) = (None, None);
    for element in elements(txn, container) {
        if element.tag().as_ref() == BLOCK_GROUP {
            group.get_or_insert(element);
        } else if content.is_none() {
            content = Some(element);
        }
    }
    let block = match content {
        // Sans nœud de contenu : un paragraphe vide, pour garder les enfants.
        None => Block {
            id,
            kind: "paragraph".into(),
            props: Props::new(),
            content: Content::Inline(vec![]),
            children: vec![],
        },
        Some(node) => match block(node.tag()) {
            Some(spec) if spec.wrapped => {
                let skip: &[&str] = match spec.content {
                    ContentKind::Table => &[HEADER_ROWS, HEADER_COLS],
                    _ => &[],
                };
                Block {
                    id,
                    kind: spec.name.to_string(),
                    props: read_props(txn, &node, spec.props, skip),
                    content: match spec.content {
                        ContentKind::Inline => Content::Inline(read_inline(txn, &node)),
                        ContentKind::Table => Content::Table(read_table(txn, &node)),
                        ContentKind::None => Content::None,
                    },
                    children: vec![],
                }
            }
            _ => foreign(txn, &node, Some(id)),
        },
    };
    Node {
        block,
        children_of: group,
    }
}

/// Props d'un élément : celles du registre converties au type déclaré (sinon
/// absentes, donc au défaut), les autres copiées si le MMD sait les écrire.
fn read_props<T: ReadTxn>(
    txn: &T,
    element: &XmlElementRef,
    specs: &[PropSpec],
    skip: &[&str],
) -> Props {
    let mut props = Props::new();
    for (name, value) in element.attributes(txn) {
        let Out::Any(value) = value else { continue };
        if skip.contains(&name) {
            continue;
        }
        let read = match specs.iter().find(|p| p.name == name) {
            Some(spec) => typed(&value, spec.ty),
            None if bag_key(name, specs) => untyped(&value),
            None => None,
        };
        if let Some(v) = read {
            props.insert(name.to_string(), v);
        }
    }
    props
}

/// Valeur d'une prop du registre, convertie si c'est sans perte (`"2"` → 2).
fn typed(value: &Any, ty: PropType) -> Option<PropValue> {
    match (ty, value) {
        (PropType::String, Any::String(s)) => Some(PropValue::Str(s.to_string())),
        (PropType::String, Any::Bool(b)) => Some(PropValue::Str(b.to_string())),
        (PropType::String, Any::Number(Number::Int(i))) => Some(PropValue::Str(i.to_string())),
        (PropType::String, Any::Number(Number::Float(f))) if f.is_finite() => {
            Some(PropValue::Str(f.to_string()))
        }
        (PropType::Int, Any::Number(n)) => n.as_i64().map(PropValue::Int),
        (PropType::Int, Any::String(s)) => s.parse().ok().map(PropValue::Int),
        (PropType::Float, Any::Number(n)) => n.as_f64().and_then(finite).map(PropValue::Float),
        (PropType::Float, Any::String(s)) => s.parse().ok().and_then(finite).map(PropValue::Float),
        (PropType::Bool, Any::Bool(b)) => Some(PropValue::Bool(*b)),
        (PropType::Bool, Any::String(s)) => match s.as_ref() {
            "true" => Some(PropValue::Bool(true)),
            "false" => Some(PropValue::Bool(false)),
            _ => None,
        },
        _ => None,
    }
}

/// Valeur d'une prop inconnue du registre, de son propre type.
fn untyped(value: &Any) -> Option<PropValue> {
    match value {
        Any::String(s) => Some(PropValue::Str(s.to_string())),
        Any::Bool(b) => Some(PropValue::Bool(*b)),
        Any::Number(Number::Int(i)) => Some(PropValue::Int(*i)),
        Any::Number(Number::Float(f)) => finite(*f).map(PropValue::Float),
        _ => None,
    }
}

fn finite(f: f64) -> Option<f64> {
    f.is_finite().then_some(f)
}

/// Vrai si une prop inconnue du registre de ce nom s'écrit et se relit en MMD.
fn bag_key(name: &str, specs: &[PropSpec]) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !RESERVED.contains(&name)
        && !specs.iter().any(|p| p.mmd == name)
}

/// Contenu en ligne d'un élément. Un élément en ligne inconnu est ignoré.
fn read_inline<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> Vec<Inline> {
    let mut out = Vec::new();
    for child in element.children(txn) {
        match child {
            XmlOut::Text(text) => read_runs(txn, &text, &mut out),
            XmlOut::Element(e) => match e.tag().as_ref() {
                "mention" => out.push(Inline::Mention {
                    kind: attr_string(txn, &e, "kind"),
                    id: attr_string(txn, &e, "id"),
                    label: attr_string(txn, &e, "label"),
                }),
                "math" => out.push(Inline::Math {
                    latex: attr_string(txn, &e, "latex"),
                }),
                "hardBreak" => out.push(Inline::HardBreak),
                _ => {}
            },
            XmlOut::Fragment(_) => {}
        }
    }
    out
}

/// Plages d'un `XmlText` (texte brut de `diff`, jamais `get_string`). Les
/// plages voisines d'un même lien forment un seul lien.
fn read_runs<T: ReadTxn>(txn: &T, text: &XmlTextRef, out: &mut Vec<Inline>) {
    for run in text.diff(txn, YChange::identity) {
        let Out::Any(Any::String(s)) = &run.insert else {
            continue;
        };
        let (styles, link) = styles_of(run.attributes.as_deref());
        let text = Inline::Text {
            text: s.to_string(),
            styles,
        };
        match link {
            None => out.push(text),
            Some(href) => match out.last_mut() {
                Some(Inline::Link { href: h, children }) if *h == href => children.push(text),
                _ => out.push(Inline::Link {
                    href,
                    children: vec![text],
                }),
            },
        }
    }
}

fn styles_of(attrs: Option<&Attrs>) -> (Styles, Option<String>) {
    let mut styles = Styles::default();
    let mut link = None;
    for (key, value) in attrs.into_iter().flatten() {
        let on = !matches!(value, Any::Null | Any::Undefined | Any::Bool(false));
        match key.as_ref() {
            "bold" => styles.bold = on,
            "italic" => styles.italic = on,
            "underline" => styles.underline = on,
            "strike" => styles.strike = on,
            "code" => styles.code = on,
            "textColor" => styles.text_color = map_string(value, "stringValue"),
            "backgroundColor" => styles.background_color = map_string(value, "stringValue"),
            "link" => link = map_string(value, "href"),
            _ => {}
        }
    }
    (styles, link)
}

/// `{key: "…"}` → `"…"` (une chaîne nue est aussi acceptée).
fn map_string(value: &Any, key: &str) -> Option<String> {
    match value {
        Any::Map(m) => match m.get(key) {
            Some(Any::String(s)) => Some(s.to_string()),
            _ => None,
        },
        Any::String(s) => Some(s.to_string()),
        _ => None,
    }
}

fn empty_cell() -> Cell {
    Cell {
        content: vec![],
        props: Props::new(),
    }
}

/// Tableau : au moins une ligne, au moins une cellule par ligne.
fn read_table<T: ReadTxn>(txn: &T, table: &XmlElementRef) -> Table {
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
    let (header_rows, header_cols) = header_hints(txn, table, &headers);
    Table {
        header_rows,
        header_cols,
        column_widths,
        rows,
    }
}

/// Lignes et colonnes d'en-tête : les indices du `table` s'ils concordent
/// avec les cellules, sinon ce qu'en déduit BlockNote.
fn header_hints<T: ReadTxn>(txn: &T, table: &XmlElementRef, headers: &[Vec<bool>]) -> (u32, u32) {
    let derived = derive_headers(headers);
    let hint = |name| match table.get_attribute(txn, name) {
        Some(Out::Any(Any::Number(n))) => n.as_i64().and_then(|i| u32::try_from(i).ok()),
        _ => None,
    };
    let (rows, cols) = (hint(HEADER_ROWS), hint(HEADER_COLS));
    if rows.is_none() && cols.is_none() {
        return derived;
    }
    let (rows, cols) = (rows.unwrap_or(derived.0), cols.unwrap_or(derived.1));
    let ncols = headers.iter().map(Vec::len).max().unwrap_or(0);
    let fits = rows as usize <= headers.len()
        && cols as usize <= ncols
        && headers.iter().enumerate().all(|(r, row)| {
            row.iter()
                .enumerate()
                .all(|(c, &h)| h == (r < rows as usize || c < cols as usize))
        });
    if fits { (rows, cols) } else { derived }
}

/// Comme BlockNote : une ligne d'en-tête n'a que des `tableHeader`, une
/// colonne d'en-tête aussi (sur toutes les lignes).
fn derive_headers(headers: &[Vec<bool>]) -> (u32, u32) {
    let rows = headers.iter().filter(|r| r.iter().all(|&h| h)).count();
    let ncols = headers.first().map_or(0, Vec::len);
    let cols = (0..ncols)
        .filter(|&c| headers.iter().all(|r| r.get(c).copied().unwrap_or(false)))
        .count();
    (rows as u32, cols as u32)
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

/// Nœud étranger lu en bloc `unknown` (Ruling R4) : `data` est le base64 du
/// JSON `{"attrs", "content", "children"}` de tout son sous-arbre.
/// - Dans un `blockContainer` (`id` donné), `content` porte ses nœuds XML et
///   `children` est vide : les blocs imbriqués restent des enfants du bloc.
/// - Hors `blockContainer`, `children` porte ses nœuds XML et `content` est
///   vide ; l'id du bloc est son attribut `id`.
fn foreign<T: ReadTxn>(txn: &T, element: &XmlElementRef, container_id: Option<String>) -> Block {
    let nodes = Value::Array(nodes_json(txn, element, 1));
    let empty = Value::Array(vec![]);
    let (content, children) = match container_id {
        Some(_) => (nodes, empty),
        None => (empty, nodes),
    };
    let mut data = Map::new();
    data.insert("attrs".into(), attrs_json(txn, element));
    data.insert("content".into(), content);
    data.insert("children".into(), children);
    let data = STANDARD.encode(Value::Object(data).to_string());
    let props = Props::from([
        (
            "originalType".to_string(),
            PropValue::Str(element.tag().to_string()),
        ),
        ("data".to_string(), PropValue::Str(data)),
        ("foreign".to_string(), PropValue::Bool(true)),
    ]);
    Block {
        id: container_id.unwrap_or_else(|| attr_string(txn, element, "id")),
        kind: "unknown".into(),
        props,
        content: Content::None,
        children: vec![],
    }
}

/// Attributs d'un élément en JSON, clés triées ; `undefined` est omis.
fn attrs_json<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> Value {
    let attrs: BTreeMap<String, Value> = element
        .attributes(txn)
        .filter_map(|(k, v)| match v {
            Out::Any(Any::Undefined) => None,
            Out::Any(a) => Some((k.to_string(), any_json(&a))),
            _ => None,
        })
        .collect();
    Value::Object(attrs.into_iter().collect())
}

/// Nœuds XML enfants en JSON : `{"text": [{"insert", "attributes"}…]}` ou
/// `{"tag", "attrs", "content"}`. Au-delà de [`MAX_DEPTH`] niveaux, plus rien.
fn nodes_json<T: ReadTxn>(txn: &T, element: &XmlElementRef, depth: usize) -> Vec<Value> {
    if depth > MAX_DEPTH {
        return vec![];
    }
    let mut out = Vec::new();
    for child in element.children(txn) {
        match child {
            XmlOut::Text(text) => {
                let runs: Vec<Value> = text
                    .diff(txn, YChange::identity)
                    .into_iter()
                    .filter_map(|run| {
                        let Out::Any(Any::String(s)) = &run.insert else {
                            return None;
                        };
                        let mut item = Map::new();
                        item.insert("insert".into(), Value::String(s.to_string()));
                        if let Some(attrs) = run.attributes.filter(|a| !a.is_empty()) {
                            let sorted: BTreeMap<String, Value> = attrs
                                .iter()
                                .map(|(k, v)| (k.to_string(), any_json(v)))
                                .collect();
                            item.insert(
                                "attributes".into(),
                                Value::Object(sorted.into_iter().collect()),
                            );
                        }
                        Some(Value::Object(item))
                    })
                    .collect();
                let mut node = Map::new();
                node.insert("text".into(), Value::Array(runs));
                out.push(Value::Object(node));
            }
            XmlOut::Element(e) => {
                let mut node = Map::new();
                node.insert("tag".into(), Value::String(e.tag().to_string()));
                node.insert("attrs".into(), attrs_json(txn, &e));
                node.insert(
                    "content".into(),
                    Value::Array(nodes_json(txn, &e, depth + 1)),
                );
                out.push(Value::Object(node));
            }
            XmlOut::Fragment(_) => {}
        }
    }
    out
}

/// Valeur yrs en JSON, types gardés (entier, flottant, chaîne…), clés triées.
fn any_json(value: &Any) -> Value {
    match value {
        Any::Null | Any::Undefined => Value::Null,
        Any::Bool(b) => Value::Bool(*b),
        Any::Number(Number::Int(i)) => Value::from(*i),
        Any::Number(Number::Float(f)) => {
            serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number)
        }
        Any::String(s) => Value::String(s.to_string()),
        Any::Buffer(bytes) => Value::Array(bytes.iter().map(|b| Value::from(*b)).collect()),
        Any::Array(values) => Value::Array(values.iter().map(any_json).collect()),
        Any::Map(entries) => {
            let sorted: BTreeMap<&String, Value> =
                entries.iter().map(|(k, v)| (k, any_json(v))).collect();
            Value::Object(sorted.into_iter().map(|(k, v)| (k.clone(), v)).collect())
        }
    }
}

/// Inverse de [`any_json`] : `2` reste un entier, `2.0` un flottant.
fn json_any(value: &Value) -> Any {
    match value {
        Value::Null => Any::Null,
        Value::Bool(b) => Any::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Any::Number(Number::Int(i)),
            None => Any::Number(Number::Float(n.as_f64().unwrap_or(f64::NAN))),
        },
        Value::String(s) => Any::from(s.as_str()),
        Value::Array(values) => Any::Array(values.iter().map(json_any).collect::<Vec<_>>().into()),
        Value::Object(entries) => Any::Map(Arc::new(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), json_any(v)))
                .collect(),
        )),
    }
}

// =================================================================== écriture

/// Attribution des ids : un id donné est gardé à sa première occurrence ; un
/// id vide ou déjà pris reçoit un UUIDv7.
#[derive(Default)]
pub(crate) struct Ids {
    used: HashSet<String>,
}

impl Ids {
    /// Ids déjà présents dans le document, à ne pas réutiliser.
    pub(crate) fn taken(used: HashSet<String>) -> Self {
        Ids { used }
    }

    fn take(&mut self, id: &str) -> String {
        if !id.is_empty() && self.used.insert(id.to_string()) {
            return id.to_string();
        }
        loop {
            let fresh = Uuid::now_v7().to_string();
            if self.used.insert(fresh.clone()) {
                return fresh;
            }
        }
    }
}

/// Paragraphe vide : BlockNote n'a pas de page sans bloc.
pub(crate) fn empty_paragraph() -> Block {
    Block {
        id: String::new(),
        kind: "paragraph".into(),
        props: Props::new(),
        content: Content::Inline(vec![]),
        children: vec![],
    }
}

/// Écrit `blocks` dans `parent`, à partir de l'index `at`. Renvoie le nombre
/// de nœuds insérés dans `parent`.
pub(crate) fn write_nodes(
    txn: &mut TransactionMut,
    parent: &XmlElementRef,
    at: u32,
    blocks: &[Block],
    ids: &mut Ids,
) -> u32 {
    let mut index = at;
    for b in blocks {
        index += write_node(txn, parent, index, b, ids);
    }
    index - at
}

fn write_node(
    txn: &mut TransactionMut,
    parent: &XmlElementRef,
    at: u32,
    b: &Block,
    ids: &mut Ids,
) -> u32 {
    if let kind @ (COLUMN_LIST | COLUMN) = b.kind.as_str() {
        let spec = block(kind).expect("colonnes dans le registre");
        let element = parent.insert(txn, at, XmlElementPrelim::empty(kind));
        element.insert_attribute(txn, "id", ids.take(&b.id));
        write_props(txn, &element, &b.props, spec.props);
        write_nodes(txn, &element, 0, &b.children, ids);
        return 1;
    }
    if let Some(node) = Foreign::of(b) {
        return node.write(txn, parent, at, b, ids);
    }
    let container = parent.insert(txn, at, XmlElementPrelim::empty(BLOCK_CONTAINER));
    container.insert_attribute(txn, "id", ids.take(&b.id));
    write_content(txn, &container, b);
    write_children(txn, &container, &b.children, ids);
    1
}

/// `blockGroup` des enfants d'un `blockContainer`, s'il y en a.
fn write_children(
    txn: &mut TransactionMut,
    container: &XmlElementRef,
    children: &[Block],
    ids: &mut Ids,
) {
    if !children.is_empty() {
        let group = container.push_back(txn, XmlElementPrelim::empty(BLOCK_GROUP));
        write_nodes(txn, &group, 0, children, ids);
    }
}

/// Nœud de contenu d'un bloc. Un type hors registre s'écrit en bloc
/// `unknown` dont `originalType` est le type, comme le fait `render`.
fn write_content(txn: &mut TransactionMut, container: &XmlElementRef, b: &Block) {
    let (spec, props) = match block(&b.kind) {
        Some(spec) => (spec, b.props.clone()),
        None => {
            let mut props = b.props.clone();
            props
                .entry("originalType".into())
                .or_insert_with(|| PropValue::Str(b.kind.clone()));
            (block("unknown").expect("unknown dans le registre"), props)
        }
    };
    let node = container.push_back(txn, XmlElementPrelim::empty(spec.name));
    write_props(txn, &node, &props, spec.props);
    match (spec.content, &b.content) {
        (ContentKind::Inline, Content::Inline(content)) => write_inline(txn, &node, content),
        (ContentKind::Table, Content::Table(table)) => write_table(txn, &node, Some(table)),
        (ContentKind::Table, _) => write_table(txn, &node, None),
        _ => {}
    }
}

/// Chaque prop du registre (sa valeur, son défaut, ou `undefined` sans
/// défaut, comme BlockNote), puis les props inconnues du registre.
fn write_props(
    txn: &mut TransactionMut,
    element: &XmlElementRef,
    props: &Props,
    specs: &[PropSpec],
) {
    for spec in specs {
        let value = match props.get(spec.name) {
            Some(v) => prop_any(v, Some(spec.ty)),
            None => spec.default.map_or(Any::Undefined, default_any),
        };
        element.insert_attribute(txn, spec.name, value);
    }
    for (name, value) in props {
        if !specs.iter().any(|s| s.name == name) {
            element.insert_attribute(txn, name.as_str(), prop_any(value, None));
        }
    }
}

fn prop_any(value: &PropValue, ty: Option<PropType>) -> Any {
    match value {
        PropValue::Str(s) => Any::from(s.as_str()),
        PropValue::Int(i) => Any::Number(Number::Int(*i)),
        // Un flottant du registre s'écrit comme le fait JavaScript (320.0 → 320) ;
        // celui d'une prop inconnue garde son type, faute de registre pour le relire.
        PropValue::Float(f) if ty == Some(PropType::Float) => js_number(*f),
        PropValue::Float(f) => Any::Number(Number::Float(*f)),
        PropValue::Bool(b) => Any::Bool(*b),
    }
}

fn default_any(default: PropDefault) -> Any {
    match default {
        PropDefault::Str(s) => Any::from(s),
        PropDefault::Int(i) => Any::Number(Number::Int(i)),
        PropDefault::Float(f) => js_number(f),
        PropDefault::Bool(b) => Any::Bool(b),
    }
}

/// Nombre au format JavaScript : entier s'il l'est exactement, sinon flottant.
fn js_number(f: f64) -> Any {
    if f.fract() == 0.0 && f.abs() <= MAX_SAFE {
        Any::Number(Number::Int(f as i64))
    } else {
        Any::Number(Number::Float(f))
    }
}

/// Contenu en ligne : un `XmlText` par suite de textes, un élément par
/// mention, formule ou saut de ligne.
fn write_inline(txn: &mut TransactionMut, element: &XmlElementRef, content: &[Inline]) {
    let mut text = None;
    for inline in content {
        write_inline_item(txn, element, &mut text, inline, None);
    }
}

fn write_inline_item(
    txn: &mut TransactionMut,
    element: &XmlElementRef,
    current: &mut Option<XmlTextRef>,
    inline: &Inline,
    link: Option<&str>,
) {
    let mut push = |txn: &mut TransactionMut, tag: &str, attrs: &[(&str, &str)]| {
        *current = None;
        let e = element.push_back(txn, XmlElementPrelim::empty(tag));
        for (k, v) in attrs {
            e.insert_attribute(txn, *k, *v);
        }
    };
    match inline {
        Inline::Text { text, styles } => {
            if text.is_empty() {
                return;
            }
            let t = match current {
                Some(t) => t.clone(),
                None => current
                    .insert(element.push_back(txn, XmlTextPrelim::new("")))
                    .clone(),
            };
            let at = t.len(txn);
            // Attributs explicites, même vides : le texte n'hérite pas des marques voisines.
            t.insert_with_attributes(txn, at, text, marks(styles, link));
        }
        Inline::Link { href, children } => {
            for child in children {
                write_inline_item(txn, element, current, child, Some(href));
            }
        }
        Inline::Mention { kind, id, label } => {
            push(
                txn,
                "mention",
                &[("kind", kind), ("id", id), ("label", label)],
            );
        }
        Inline::Math { latex } => push(txn, "math", &[("latex", latex)]),
        Inline::HardBreak => push(txn, "hardBreak", &[]),
    }
}

fn marks(styles: &Styles, link: Option<&str>) -> Attrs {
    let one = |key: &str, value: &str| {
        Any::Map(Arc::new(HashMap::from([(
            key.to_string(),
            Any::from(value),
        )])))
    };
    let mut attrs = Attrs::new();
    for (on, key) in [
        (styles.bold, "bold"),
        (styles.italic, "italic"),
        (styles.underline, "underline"),
        (styles.strike, "strike"),
        (styles.code, "code"),
    ] {
        if on {
            attrs.insert(key.into(), Any::Map(Arc::new(HashMap::new())));
        }
    }
    if let Some(color) = &styles.text_color {
        attrs.insert("textColor".into(), one("stringValue", color));
    }
    if let Some(color) = &styles.background_color {
        attrs.insert("backgroundColor".into(), one("stringValue", color));
    }
    if let Some(href) = link {
        attrs.insert("link".into(), one("href", href));
    }
    attrs
}

/// Tableau : `tableHeader` pour une cellule d'une ligne ou d'une colonne
/// d'en-tête, `colwidth` d'après les largeurs. Un tableau sans ligne (ou une
/// ligne sans cellule) reçoit une cellule vide, comme à la lecture.
fn write_table(txn: &mut TransactionMut, element: &XmlElementRef, table: Option<&Table>) {
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
    let is_header = |r: usize, c: usize| r < header_rows || c < header_cols;
    let headers: Vec<Vec<bool>> = rows
        .iter()
        .enumerate()
        .map(|(r, row)| (0..row.len()).map(|c| is_header(r, c)).collect())
        .collect();
    let (hr, hc) = (header_rows as u32, header_cols as u32);
    if derive_headers(&headers) != (hr, hc) {
        element.insert_attribute(txn, HEADER_ROWS, Any::Number(Number::Int(hr.into())));
        element.insert_attribute(txn, HEADER_COLS, Any::Number(Number::Int(hc.into())));
    }
    for (r, row) in rows.iter().enumerate() {
        let tr = element.push_back(txn, XmlElementPrelim::empty("tableRow"));
        for (c, cell) in row.iter().enumerate() {
            let tag = if is_header(r, c) {
                "tableHeader"
            } else {
                "tableCell"
            };
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

/// Nœud étranger à recréer depuis la prop `data` d'un bloc `unknown`.
struct Foreign {
    tag: String,
    attrs: Map<String, Value>,
    content: Vec<Value>,
    children: Vec<Value>,
}

impl Foreign {
    /// Le nœud d'origine d'un bloc `unknown` à `foreign = true`, si son
    /// `data` se décode et qu'aucune autre prop ne s'y ajoute. Sinon le bloc
    /// s'écrit en bloc `unknown` de BlockNote, props en attributs : rien ne
    /// se perd, mais le nœud d'origine n'est pas recréé.
    fn of(b: &Block) -> Option<Foreign> {
        if b.kind != "unknown"
            || b.props.get("foreign") != Some(&PropValue::Bool(true))
            || b.props
                .keys()
                .any(|k| !matches!(k.as_str(), "originalType" | "data" | "foreign"))
        {
            return None;
        }
        let Some(PropValue::Str(tag)) = b.props.get("originalType") else {
            return None;
        };
        // Jamais un type connu ni une enveloppe : seul un nœud étranger se recrée.
        if tag.is_empty()
            || block(tag).is_some()
            || matches!(tag.as_str(), BLOCK_GROUP | BLOCK_CONTAINER)
        {
            return None;
        }
        let Some(PropValue::Str(data)) = b.props.get("data") else {
            return None;
        };
        let bytes = STANDARD.decode(data).ok()?;
        let Value::Object(mut data) = serde_json::from_slice(&bytes).ok()? else {
            return None;
        };
        let mut list = |key: &str| match data.remove(key) {
            None => Some(vec![]),
            Some(Value::Array(a)) => Some(a),
            Some(_) => None,
        };
        let (content, children) = (list("content")?, list("children")?);
        let attrs = match data.remove("attrs") {
            None => Map::new(),
            Some(Value::Object(m)) => m,
            Some(_) => return None,
        };
        Some(Foreign {
            tag: tag.clone(),
            attrs,
            content,
            children,
        })
    }

    /// Sans `children` : dans un `blockContainer` (id du bloc), enfants du
    /// bloc dans son `blockGroup`. Avec : directement dans `parent`, les
    /// enfants du bloc à sa suite.
    fn write(
        self,
        txn: &mut TransactionMut,
        parent: &XmlElementRef,
        at: u32,
        b: &Block,
        ids: &mut Ids,
    ) -> u32 {
        if self.children.is_empty() {
            let container = parent.insert(txn, at, XmlElementPrelim::empty(BLOCK_CONTAINER));
            container.insert_attribute(txn, "id", ids.take(&b.id));
            let node = container.push_back(txn, XmlElementPrelim::empty(self.tag.as_str()));
            write_json_attrs(txn, &node, &self.attrs);
            write_json_nodes(txn, &node, &self.content);
            write_children(txn, &container, &b.children, ids);
            1
        } else {
            let node = parent.insert(txn, at, XmlElementPrelim::empty(self.tag.as_str()));
            write_json_attrs(txn, &node, &self.attrs);
            write_json_nodes(txn, &node, &self.content);
            write_json_nodes(txn, &node, &self.children);
            1 + write_nodes(txn, parent, at + 1, &b.children, ids)
        }
    }
}

fn write_json_attrs(txn: &mut TransactionMut, element: &XmlElementRef, attrs: &Map<String, Value>) {
    for (key, value) in attrs {
        element.insert_attribute(txn, key.as_str(), json_any(value));
    }
}

/// Recrée des nœuds XML décrits par [`nodes_json`] (profondeur bornée par
/// la limite de récursion de `serde_json`).
fn write_json_nodes(txn: &mut TransactionMut, element: &XmlElementRef, nodes: &[Value]) {
    for node in nodes {
        let Value::Object(node) = node else { continue };
        if let Some(Value::Array(runs)) = node.get("text") {
            let text = element.push_back(txn, XmlTextPrelim::new(""));
            for run in runs {
                let Some(Value::String(s)) = run.get("insert") else {
                    continue;
                };
                if s.is_empty() {
                    continue;
                }
                let attrs: Attrs = match run.get("attributes") {
                    Some(Value::Object(m)) => m
                        .iter()
                        .map(|(k, v)| (Arc::from(k.as_str()), json_any(v)))
                        .collect(),
                    _ => Attrs::new(),
                };
                let at = text.len(txn);
                text.insert_with_attributes(txn, at, s, attrs);
            }
        } else if let Some(Value::String(tag)) = node.get("tag")
            && !tag.is_empty()
        {
            let child = element.push_back(txn, XmlElementPrelim::empty(tag.as_str()));
            if let Some(Value::Object(attrs)) = node.get("attrs") {
                write_json_attrs(txn, &child, attrs);
            }
            if let Some(Value::Array(content)) = node.get("content") {
                write_json_nodes(txn, &child, content);
            }
        }
    }
}

// ============================================================ outils communs

/// Éléments enfants (les textes sont ignorés).
pub(crate) fn elements<T: ReadTxn>(txn: &T, parent: &XmlElementRef) -> Vec<XmlElementRef> {
    parent
        .children(txn)
        .filter_map(|c| match c {
            XmlOut::Element(e) => Some(e),
            _ => None,
        })
        .collect()
}

/// Attribut en chaîne (un nombre ou un booléen est écrit en texte).
pub(crate) fn attr_string<T: ReadTxn>(txn: &T, element: &XmlElementRef, name: &str) -> String {
    match element.get_attribute(txn, name) {
        Some(Out::Any(Any::String(s))) => s.to_string(),
        Some(Out::Any(Any::Number(Number::Int(i)))) => i.to_string(),
        Some(Out::Any(Any::Number(Number::Float(f)))) if f.is_finite() => f.to_string(),
        Some(Out::Any(Any::Bool(b))) => b.to_string(),
        _ => String::new(),
    }
}

/// Vrai pour un élément qui porte l'id d'un bloc.
pub(crate) fn is_block(element: &XmlElementRef) -> bool {
    matches!(
        element.tag().as_ref(),
        BLOCK_CONTAINER | COLUMN_LIST | COLUMN
    )
}
