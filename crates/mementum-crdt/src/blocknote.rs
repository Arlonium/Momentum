//! Arbre de blocs ⇄ document Yjs au format de BlockNote 0.57 (y-prosemirror 1.x).
//!
//! Structure :
//! - `blockGroup > blockContainer[id] > <type props…>`, suivi d'un
//!   `blockGroup` pour les enfants ;
//! - `columnList[id] > column[id, width] > blockContainer…`, sans
//!   `blockContainer` autour (schéma de `@blocknote/xl-multi-column` : deux
//!   colonnes au moins, une colonne jamais vide ni contenant directement un
//!   `columnList`) ;
//! - contenu en ligne : `XmlText` dont les marques sont des attributs (simple
//!   `{}`, couleur `{"stringValue": …}`, lien `{"href": …}`), entrecoupé des
//!   éléments `mention`, `math` et `hardBreak` ;
//! - tableaux : voir [`crate::table`] ; nœuds étrangers : [`crate::foreign`].
//!
//! Les props sont des attributs typés du nœud de contenu. L'écriture donne à
//! chaque prop du registre sa valeur ou son défaut, comme BlockNote ; une prop
//! inconnue du registre est copiée telle quelle.
//!
//! La lecture garantit ce que `mementum_md::render` et `parse` supposent
//! (Rulings R10 et R16) : props du registre du type déclaré, flottants finis,
//! tableau d'au moins une ligne, imbrication bornée à [`MAX_DEPTH`],
//! colonnes au schéma de BlockNote.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mementum_md::{Block, Content, Inline, MAX_DEPTH, PropValue, Props, Styles, normalize};
use mementum_schema::{ContentKind, PropDefault, PropSpec, PropType, block};
use uuid::Uuid;
use yrs::any::Number;
use yrs::types::Attrs;
use yrs::types::text::YChange;
use yrs::{
    Any, Out, ReadTxn, Text, TransactionMut, Xml, XmlElementPrelim, XmlElementRef, XmlFragment,
    XmlFragmentRef, XmlOut, XmlTextPrelim, XmlTextRef,
};

use crate::foreign::{Foreign, read_foreign};
use crate::table::{read_table, write_table};

pub(crate) const BLOCK_GROUP: &str = "blockGroup";
pub(crate) const BLOCK_CONTAINER: &str = "blockContainer";
pub(crate) const COLUMN_LIST: &str = "columnList";
pub(crate) const COLUMN: &str = "column";

/// Plus grand entier qu'un nombre JavaScript représente exactement (2^53 - 1).
pub(crate) const MAX_SAFE: f64 = 9_007_199_254_740_991.0;

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
///
/// La récursion suit la profondeur du document, que la garde de profondeur
/// borne à `MAX_YJS_DEPTH` éléments.
pub(crate) fn read_page<T: ReadTxn>(txn: &T, fragment: &XmlFragmentRef) -> Vec<Block> {
    let mut out = Vec::new();
    for child in fragment.children(txn) {
        if let XmlOut::Element(group) = child
            && group.tag().as_ref() == BLOCK_GROUP
        {
            read_nodes(txn, &group, 1, false, &mut out);
        }
    }
    normalize(out)
}

/// Lit les blocs enfants de `parent`, de niveau `level` (1 au premier
/// niveau). `in_column` : `parent` est une colonne.
///
/// - Un bloc de niveau [`MAX_DEPTH`] n'a pas d'enfants : ses descendants le
///   suivent, au même niveau, dans l'ordre du document.
/// - Une colonne hors d'une liste de colonnes, ou une liste qui ne respecte
///   pas le schéma (moins de deux colonnes, autre chose qu'une colonne, une
///   colonne vide, directement dans une colonne, ou trop profonde pour que
///   ses blocs restent sous [`MAX_DEPTH`]), est défaite : ses blocs prennent
///   sa place, au même niveau.
fn read_nodes<T: ReadTxn>(
    txn: &T,
    parent: &XmlElementRef,
    level: usize,
    in_column: bool,
    out: &mut Vec<Block>,
) {
    for element in elements(txn, parent) {
        match element.tag().as_ref() {
            BLOCK_GROUP => {}
            COLUMN => read_nodes(txn, &element, level, in_column, out),
            COLUMN_LIST => match column_list(txn, &element, level, in_column) {
                Some(list) => out.push(list),
                None => read_nodes(txn, &element, level, in_column, out),
            },
            BLOCK_CONTAINER => {
                let (mut block, group) = read_container(txn, &element);
                match group {
                    Some(group) if level < MAX_DEPTH => {
                        read_nodes(txn, &group, level + 1, false, &mut block.children);
                        out.push(block);
                    }
                    Some(group) => {
                        out.push(block);
                        flatten(txn, &group, out);
                    }
                    None => out.push(block),
                }
            }
            // Nœud étranger hors `blockContainer`.
            _ => out.push(read_foreign(txn, &element, None)),
        }
    }
}

/// Vrai si un bloc que [`read_page`] lirait porte l'identifiant `id` (constat S7, Ruling
/// R206) : le même parcours que [`read_nodes`], [`column_list`] et [`flatten`], qui ne lit que
/// les identifiants et la forme de l'arbre, sans le texte ni les props d'aucun bloc. Un fil de
/// commentaires sur un bloc le demande à chaque ouverture : bâtir la liste des blocs d'une page
/// de 10 000 blocs occupait son acteur environ 39 ms, pendant lesquelles ses frappes attendaient.
pub(crate) fn page_has_block<T: ReadTxn>(txn: &T, fragment: &XmlFragmentRef, id: &str) -> bool {
    !id.is_empty()
        && fragment.children(txn).any(|child| {
            matches!(child, XmlOut::Element(group)
                if group.tag().as_ref() == BLOCK_GROUP && nodes_have(txn, &group, 1, false, id))
        })
}

/// [`read_nodes`], pour les identifiants seulement.
fn nodes_have<T: ReadTxn>(
    txn: &T,
    parent: &XmlElementRef,
    level: usize,
    in_column: bool,
    id: &str,
) -> bool {
    elements(txn, parent)
        .iter()
        .any(|element| match element.tag().as_ref() {
            BLOCK_GROUP => false,
            COLUMN => nodes_have(txn, element, level, in_column, id),
            COLUMN_LIST if is_column_list(txn, element, level, in_column) => {
                id_is(txn, element, id)
                    || elements(txn, element).iter().any(|column| {
                        id_is(txn, column, id) || nodes_have(txn, column, level + 2, true, id)
                    })
            }
            COLUMN_LIST => nodes_have(txn, element, level, in_column, id),
            BLOCK_CONTAINER => {
                id_is(txn, element, id)
                    || container_group(txn, element).is_some_and(|group| {
                        if level < MAX_DEPTH {
                            nodes_have(txn, &group, level + 1, false, id)
                        } else {
                            flattened_have(txn, &group, id)
                        }
                    })
            }
            // Nœud étranger hors `blockContainer` : son propre identifiant.
            _ => id_is(txn, element, id),
        })
}

/// [`column_list`] rendrait-il une liste ? Ses conditions, sans lire ses blocs.
fn is_column_list<T: ReadTxn>(
    txn: &T,
    list: &XmlElementRef,
    level: usize,
    in_column: bool,
) -> bool {
    if in_column || level + 2 > MAX_DEPTH {
        return false;
    }
    let columns = elements(txn, list);
    columns.len() >= 2
        && columns.iter().all(|column| {
            column.tag().as_ref() == COLUMN && yields_blocks(txn, column, level + 2, true)
        })
}

/// [`read_nodes`] rendrait-il au moins un bloc pour les enfants de `parent` ?
fn yields_blocks<T: ReadTxn>(
    txn: &T,
    parent: &XmlElementRef,
    level: usize,
    in_column: bool,
) -> bool {
    elements(txn, parent)
        .iter()
        .any(|element| match element.tag().as_ref() {
            BLOCK_GROUP => false,
            // Une liste valide est un bloc ; défaite, elle rend ce que rendent ses colonnes.
            COLUMN | COLUMN_LIST => {
                (element.tag().as_ref() == COLUMN_LIST
                    && is_column_list(txn, element, level, in_column))
                    || yields_blocks(txn, element, level, in_column)
            }
            _ => true,
        })
}

/// Le `blockGroup` qui porte les enfants d'un `blockContainer`, comme [`read_container`] le
/// trouve.
fn container_group<T: ReadTxn>(txn: &T, container: &XmlElementRef) -> Option<XmlElementRef> {
    container.children(txn).find_map(|child| match child {
        XmlOut::Element(element) if element.tag().as_ref() == BLOCK_GROUP => Some(element),
        _ => None,
    })
}

/// L'attribut `id` de l'élément vaut-il `id`, lu comme [`attr_string`] le lit, sans allouer pour
/// le cas ordinaire (une chaîne) ?
fn id_is<T: ReadTxn>(txn: &T, element: &XmlElementRef, id: &str) -> bool {
    match element.get_attribute(txn, "id") {
        Some(Out::Any(Any::String(s))) => s.as_ref() == id,
        None => false,
        Some(_) => attr_string(txn, element, "id") == id,
    }
}

/// [`flatten`], pour les identifiants seulement.
fn flattened_have<T: ReadTxn>(txn: &T, source: &XmlElementRef, id: &str) -> bool {
    let mut stack = vec![elements(txn, source).into_iter()];
    while let Some(top) = stack.last_mut() {
        let Some(element) = top.next() else {
            stack.pop();
            continue;
        };
        match element.tag().as_ref() {
            BLOCK_GROUP => {}
            COLUMN | COLUMN_LIST => stack.push(elements(txn, &element).into_iter()),
            BLOCK_CONTAINER => {
                if id_is(txn, &element, id) {
                    return true;
                }
                if let Some(group) = container_group(txn, &element) {
                    stack.push(elements(txn, &group).into_iter());
                }
            }
            _ => {
                if id_is(txn, &element, id) {
                    return true;
                }
            }
        }
    }
    false
}

/// Une liste de colonnes, si elle respecte le schéma de BlockNote.
fn column_list<T: ReadTxn>(
    txn: &T,
    list: &XmlElementRef,
    level: usize,
    in_column: bool,
) -> Option<Block> {
    if in_column || level + 2 > MAX_DEPTH {
        return None;
    }
    let spec = block(COLUMN).expect("colonne dans le registre");
    let mut columns = Vec::new();
    for column in elements(txn, list) {
        if column.tag().as_ref() != COLUMN {
            return None;
        }
        let mut children = Vec::new();
        read_nodes(txn, &column, level + 2, true, &mut children);
        if children.is_empty() {
            return None;
        }
        columns.push(Block {
            id: attr_string(txn, &column, "id"),
            kind: COLUMN.into(),
            props: read_props(txn, &column, spec.props, &["id"]),
            content: Content::None,
            children,
        });
    }
    if columns.len() < 2 {
        return None;
    }
    let spec = block(COLUMN_LIST).expect("liste de colonnes dans le registre");
    Some(Block {
        id: attr_string(txn, list, "id"),
        kind: COLUMN_LIST.into(),
        props: read_props(txn, list, spec.props, &["id"]),
        content: Content::None,
        children: columns,
    })
}

/// Descendants de `source`, sans enfants, dans l'ordre du document (sans
/// récursion). Les colonnes ne s'écrivent pas : seuls leurs blocs restent.
fn flatten<T: ReadTxn>(txn: &T, source: &XmlElementRef, out: &mut Vec<Block>) {
    let mut stack = vec![elements(txn, source).into_iter()];
    while let Some(top) = stack.last_mut() {
        let Some(element) = top.next() else {
            stack.pop();
            continue;
        };
        match element.tag().as_ref() {
            BLOCK_GROUP => {}
            COLUMN | COLUMN_LIST => stack.push(elements(txn, &element).into_iter()),
            BLOCK_CONTAINER => {
                let (block, group) = read_container(txn, &element);
                out.push(block);
                if let Some(group) = group {
                    stack.push(elements(txn, &group).into_iter());
                }
            }
            _ => out.push(read_foreign(txn, &element, None)),
        }
    }
}

/// Un bloc sans ses enfants, et le `blockGroup` qui les porte.
fn read_container<T: ReadTxn>(
    txn: &T,
    container: &XmlElementRef,
) -> (Block, Option<XmlElementRef>) {
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
            ..empty_paragraph()
        },
        Some(node) => match block(node.tag()) {
            Some(spec) if spec.wrapped => Block {
                id,
                kind: spec.name.to_string(),
                props: read_props(txn, &node, spec.props, &[]),
                content: match spec.content {
                    ContentKind::Inline => Content::Inline(read_inline(txn, &node)),
                    ContentKind::Table => Content::Table(read_table(txn, &node)),
                    ContentKind::None => Content::None,
                },
                children: vec![],
            },
            _ => read_foreign(txn, &node, Some(id)),
        },
    };
    (block, group)
}

/// Props d'un élément : celles du registre converties au type déclaré (sinon
/// absentes, donc au défaut), les autres copiées si le MMD sait les écrire.
pub(crate) fn read_props<T: ReadTxn>(
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
    valid_name(name) && !RESERVED.contains(&name) && !specs.iter().any(|p| p.mmd == name)
}

/// Identifiant `[A-Za-z][A-Za-z0-9_-]*`.
pub(crate) fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Contenu en ligne d'un élément. Un élément en ligne inconnu est ignoré.
/// Dans un bloc de code, `normalize` le ramène ensuite au texte brut.
pub(crate) fn read_inline<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> Vec<Inline> {
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

    pub(crate) fn take(&mut self, id: &str) -> String {
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

/// Écrit `blocks` (déjà normalisés et vérifiés) dans `parent`, à partir de
/// l'index `at`. Renvoie le nombre de nœuds insérés dans `parent`.
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
pub(crate) fn write_children(
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
pub(crate) fn write_props(
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
pub(crate) fn js_number(f: f64) -> Any {
    if f.fract() == 0.0 && f.abs() <= MAX_SAFE {
        Any::Number(Number::Int(f as i64))
    } else {
        Any::Number(Number::Float(f))
    }
}

/// Contenu en ligne : un `XmlText` par suite de textes, un élément par
/// mention, formule ou saut de ligne.
pub(crate) fn write_inline(txn: &mut TransactionMut, element: &XmlElementRef, content: &[Inline]) {
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

/// Ids de tous les blocs de la page.
pub(crate) fn block_ids<T: ReadTxn>(txn: &T, fragment: &XmlFragmentRef) -> HashSet<String> {
    fragment
        .successors(txn)
        .filter_map(|node| match node {
            XmlOut::Element(e) if is_block(&e) => Some(attr_string(txn, &e, "id")),
            _ => None,
        })
        .filter(|id| !id.is_empty())
        .collect()
}

/// Vrai pour un élément qui porte l'id d'un bloc.
pub(crate) fn is_block(element: &XmlElementRef) -> bool {
    matches!(
        element.tag().as_ref(),
        BLOCK_CONTAINER | COLUMN_LIST | COLUMN
    )
}
