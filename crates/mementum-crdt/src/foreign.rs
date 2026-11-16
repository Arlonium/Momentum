//! Nœuds Yjs étrangers (Ruling R4) : un type inconnu du serveur se lit en
//! bloc `unknown` à `foreign = true`, et se recrée tel quel à l'écriture.
//!
//! La prop `data` est le base64 du JSON
//! `{"attrs", "content", "children"}` du nœud, plus `"unwrapped": true` pour
//! un nœud posé directement dans un `blockGroup` (hors `blockContainer`) :
//! - `attrs` : ses attributs, clés triées, types gardés (un entier reste un
//!   entier, un flottant un flottant) ;
//! - `content` : ses nœuds XML, `{"text": [{"insert", "marks"?}]}` pour
//!   un texte (`{"embed", "marks"?}` pour un objet incorporé) et
//!   `{"tag", "attrs", "content"}` pour un élément. `marks` ne porte que les
//!   marques du schéma qui changent à cette plage, avec leur valeur neuve
//!   (`null` : retirée) ; les autres valent ce qu'elles valaient à la plage
//!   précédente (Ruling R432) ;
//! - `children` : toujours vide à la lecture ; les blocs imbriqués d'un nœud
//!   dans un `blockContainer` restent des enfants du bloc. À l'écriture, ses
//!   nœuds s'ajoutent à la suite de `content`.
//!
//! Limites : une valeur `Buffer` devient un tableau d'octets, un flottant non
//! fini et un `undefined` dans un tableau deviennent `null`, un type
//! incorporé dans un texte est perdu, une chaîne incorporée devient du texte.
//! Seules les marques du schéma sont gardées : la page n'en porte pas d'autre
//! (Ruling R431).

use std::collections::BTreeMap;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mementum_md::{Block, Content, PropValue, Props};
use mementum_schema::block;
use serde_json::{Map, Value};
use yrs::any::Number;
use yrs::types::text::YChange;
use yrs::types::{Attrs, Delta};
use yrs::{
    Any, Out, ReadTxn, Text, TransactionMut, Xml, XmlElementPrelim, XmlElementRef, XmlFragment,
    XmlOut, XmlTextPrelim, XmlTextRef,
};

use crate::blocknote::{
    BLOCK_CONTAINER, Ids, attr_string, valid_name, write_children, write_nodes,
};
use crate::guard::MARKS;

/// Éléments XML imbriqués gardés dans `data`. Avec au plus 2 niveaux JSON par
/// élément, 4 pour une plage de texte et [`MAX_VALUE_NESTING`] pour une
/// valeur, le JSON reste sous la limite de 128 niveaux de `serde_json`, qui
/// relit donc toujours ce qu'on a écrit.
const MAX_NODE_DEPTH: usize = 48;

/// Imbrication gardée d'une valeur d'attribut (au-delà : `null`).
const MAX_VALUE_NESTING: usize = 16;

/// Noms de nœuds que BlockNote connaît : un nœud étranger qui en porte un
/// n'est pas recréé (Ruling R20).
const BLOCKNOTE_NODES: &[&str] = &[
    "doc",
    "text",
    "blockGroup",
    "blockContainer",
    "tableContent",
    "tableRow",
    "tableCell",
    "tableHeader",
    "tableParagraph",
    "mention",
    "math",
    "hardBreak",
];

/// Lit un nœud étranger en bloc `unknown`. `container_id` : l'id du
/// `blockContainer` qui l'enveloppe ; sans lui, le nœud est hors enveloppe et
/// l'id du bloc est son attribut `id`.
pub(crate) fn read_foreign<T: ReadTxn>(
    txn: &T,
    element: &XmlElementRef,
    container_id: Option<String>,
) -> Block {
    let mut data = Map::new();
    data.insert("attrs".into(), attrs_json(txn, element));
    data.insert("content".into(), Value::Array(nodes_json(txn, element, 1)));
    data.insert("children".into(), Value::Array(vec![]));
    if container_id.is_none() {
        data.insert("unwrapped".into(), Value::Bool(true));
    }
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
            Out::Any(a) => Some((k.to_string(), any_json(&a, 0))),
            _ => None,
        })
        .collect();
    Value::Object(attrs.into_iter().collect())
}

/// Nœuds XML enfants en JSON ; au-delà de [`MAX_NODE_DEPTH`], plus rien.
fn nodes_json<T: ReadTxn>(txn: &T, element: &XmlElementRef, depth: usize) -> Vec<Value> {
    if depth > MAX_NODE_DEPTH {
        return vec![];
    }
    let mut out = Vec::new();
    for child in element.children(txn) {
        match child {
            XmlOut::Text(text) => {
                let mut node = Map::new();
                node.insert("text".into(), Value::Array(runs_json(txn, &text)));
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

/// Les plages d'un texte en JSON (Ruling R432) : `{"insert"}` ou `{"embed"}`, et `"marks"` à la
/// plage où une marque change, avec sa valeur neuve (`null` : retirée) ; une marque qui ne change
/// pas n'est pas récrite, et [`write_json_nodes`] la reporte sur la plage suivante. Écrire toutes
/// les marques de chaque plage coûtait, pour une adresse longue sur des plages alternées, le produit
/// de sa longueur par leur nombre. Seules les marques du schéma sont suivies : la page n'en garde
/// pas d'autre (Ruling R431).
fn runs_json<T: ReadTxn>(txn: &T, text: &XmlTextRef) -> Vec<Value> {
    // La valeur de chaque marque du schéma à la plage précédente.
    let mut previous: Vec<Option<Any>> = vec![None; MARKS.len()];
    text.diff(txn, YChange::identity)
        .into_iter()
        .filter_map(|run| {
            let mut item = Map::new();
            match run.insert {
                Out::Any(Any::String(s)) => {
                    item.insert("insert".into(), Value::String(s.to_string()))
                }
                Out::Any(other) => item.insert("embed".into(), any_json(&other, 0)),
                _ => return None,
            };
            let mut marks = Map::new();
            for ((name, _), before) in MARKS.iter().zip(previous.iter_mut()) {
                let now = run.attributes.as_deref().and_then(|a| a.get(*name));
                if !same_value(before.as_ref(), now) {
                    marks.insert(
                        name.to_string(),
                        now.map_or(Value::Null, |v| any_json(v, 0)),
                    );
                    *before = now.cloned();
                }
            }
            if !marks.is_empty() {
                item.insert("marks".into(), Value::Object(marks));
            }
            Some(Value::Object(item))
        })
        .collect()
}

/// Vrai si deux valeurs de marque sont les mêmes : le même objet du document d'abord (une marque
/// posée sur plusieurs plages), sans comparer son contenu.
fn same_value(a: Option<&Any>, b: Option<&Any>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Any::Map(a)), Some(Any::Map(b))) => Arc::ptr_eq(a, b) || a == b,
        (Some(Any::String(a)), Some(Any::String(b))) => Arc::ptr_eq(a, b) || a == b,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Valeur yrs en JSON, types gardés (entier, flottant, chaîne…), clés triées.
fn any_json(value: &Any, nesting: usize) -> Value {
    if nesting > MAX_VALUE_NESTING {
        return Value::Null;
    }
    match value {
        Any::Null | Any::Undefined => Value::Null,
        Any::Bool(b) => Value::Bool(*b),
        Any::Number(Number::Int(i)) => Value::from(*i),
        Any::Number(Number::Float(f)) => {
            serde_json::Number::from_f64(*f).map_or(Value::Null, Value::Number)
        }
        Any::String(s) => Value::String(s.to_string()),
        Any::Buffer(bytes) => Value::Array(bytes.iter().map(|b| Value::from(*b)).collect()),
        Any::Array(values) => {
            Value::Array(values.iter().map(|v| any_json(v, nesting + 1)).collect())
        }
        Any::Map(entries) => {
            let sorted: BTreeMap<&String, Value> = entries
                .iter()
                .map(|(k, v)| (k, any_json(v, nesting + 1)))
                .collect();
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

/// Nœud étranger à recréer depuis la prop `data` d'un bloc `unknown`.
pub(crate) struct Foreign {
    tag: String,
    attrs: Map<String, Value>,
    nodes: Vec<Value>,
    unwrapped: bool,
}

impl Foreign {
    /// Le nœud d'origine d'un bloc `unknown` à `foreign = true`, si son nom
    /// n'est ni un nom de nœud de BlockNote ni un nom invalide, si son `data`
    /// se décode, et si aucune autre prop ne s'y ajoute. Sinon le bloc
    /// s'écrit en bloc `unknown` de BlockNote, props en attributs : rien ne se
    /// perd, mais le nœud d'origine n'est pas recréé.
    pub(crate) fn of(b: &Block) -> Option<Foreign> {
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
        if tag.len() > 64
            || !valid_name(tag)
            || block(tag).is_some()
            || BLOCKNOTE_NODES.contains(&tag.as_str())
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
        let mut nodes = list("content")?;
        nodes.extend(list("children")?);
        let attrs = match data.remove("attrs") {
            None => Map::new(),
            Some(Value::Object(m)) => m,
            Some(_) => return None,
        };
        let unwrapped = match data.remove("unwrapped") {
            None => false,
            Some(Value::Bool(u)) => u,
            Some(_) => return None,
        };
        Some(Foreign {
            tag: tag.clone(),
            attrs,
            nodes,
            unwrapped,
        })
    }

    /// Dans un `blockContainer` (id du bloc), enfants du bloc dans son
    /// `blockGroup` ; hors enveloppe, directement dans `parent`, son id passé
    /// au contrôle des doublons, et les enfants du bloc à sa suite.
    pub(crate) fn write(
        mut self,
        txn: &mut TransactionMut,
        parent: &XmlElementRef,
        at: u32,
        b: &Block,
        ids: &mut Ids,
    ) -> u32 {
        if self.unwrapped {
            let own = match self.attrs.get("id") {
                Some(Value::String(id)) => id.clone(),
                _ => String::new(),
            };
            let wanted = if b.id.is_empty() { own } else { b.id.clone() };
            if !wanted.is_empty() {
                self.attrs
                    .insert("id".into(), Value::String(ids.take(&wanted)));
            }
            let node = parent.insert(txn, at, XmlElementPrelim::empty(self.tag.as_str()));
            write_json_attrs(txn, &node, &self.attrs);
            write_json_nodes(txn, &node, &self.nodes);
            1 + write_nodes(txn, parent, at + 1, &b.children, ids)
        } else {
            let container = parent.insert(txn, at, XmlElementPrelim::empty(BLOCK_CONTAINER));
            container.insert_attribute(txn, "id", ids.take(&b.id));
            let node = container.push_back(txn, XmlElementPrelim::empty(self.tag.as_str()));
            write_json_attrs(txn, &node, &self.attrs);
            write_json_nodes(txn, &node, &self.nodes);
            write_children(txn, &container, &b.children, ids);
            1
        }
    }

    /// Profondeur XML de ce que [`Foreign::write`] crée sous le nœud.
    pub(crate) fn content_depth(&self) -> usize {
        json_depth(&self.nodes)
    }

    pub(crate) fn unwrapped(&self) -> bool {
        self.unwrapped
    }
}

/// Profondeur XML des nœuds décrits (un texte compte pour 1), sans récursion.
fn json_depth(nodes: &[Value]) -> usize {
    let mut deepest = 0;
    let mut stack: Vec<(&[Value], usize)> = vec![(nodes, 1)];
    while let Some((nodes, depth)) = stack.pop() {
        for node in nodes {
            deepest = deepest.max(depth);
            if let Some(Value::Array(content)) = node.get("content") {
                stack.push((content, depth + 1));
            }
        }
    }
    deepest
}

fn write_json_attrs(txn: &mut TransactionMut, element: &XmlElementRef, attrs: &Map<String, Value>) {
    for (key, value) in attrs {
        element.insert_attribute(txn, key.as_str(), json_any(value));
    }
}

/// Recrée des nœuds XML décrits par [`nodes_json`] (profondeur bornée par la
/// limite de récursion de `serde_json`, qui a lu ce JSON).
fn write_json_nodes(txn: &mut TransactionMut, element: &XmlElementRef, nodes: &[Value]) {
    for node in nodes {
        let Value::Object(node) = node else { continue };
        if let Some(Value::Array(runs)) = node.get("text") {
            let text = element.push_back(txn, XmlTextPrelim::new(""));
            // Les marques en vigueur, reportées d'une plage à l'autre (Ruling R432), puis tout le
            // texte d'une passe : insérer plage par plage à la fin reparcourait le texte à chaque
            // plage.
            let mut current = Attrs::new();
            let mut delta = Vec::with_capacity(runs.len());
            for run in runs {
                if let Some(Value::Object(marks)) = run.get("marks") {
                    for (key, value) in marks {
                        let Some((name, _)) = MARKS.iter().find(|(n, _)| *n == key.as_str()) else {
                            continue;
                        };
                        match value {
                            Value::Null => current.remove(*name),
                            value => current.insert(Arc::from(*name), json_any(value)),
                        };
                    }
                }
                let value = match (run.get("insert"), run.get("embed")) {
                    (Some(Value::String(s)), _) if !s.is_empty() => Any::from(s.as_str()),
                    (None, Some(embed)) => json_any(embed),
                    _ => continue,
                };
                delta.push(Delta::Inserted(value, Some(Box::new(current.clone()))));
            }
            text.apply_delta(txn, delta);
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
