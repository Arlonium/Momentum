//! Garde des attributs de bloc (Rulings R689 à R701).
//!
//! Yjs garde toute valeur JSON dans un attribut d'élément, y-prosemirror la passe telle quelle au
//! nœud de l'éditeur, et BlockNote écrit `String(valeur)` de chaque prop dans un attribut
//! `data-…` : une prop `{"toString": 1}` lève « Cannot convert object to primitive value », et la
//! page ne s'ouvre plus chez personne. Un titre crée l'élément `h` suivi de son niveau (`1e+21`
//! n'est pas un nom d'élément), et prosemirror-tables alloue une case par colonne couverte (un
//! `colspan` d'un milliard fige l'onglet), additionne les `colspan` (une chaîne les met bout à
//! bout) et joint les largeurs d'une cellule (`colwidth.join`).
//!
//! Une page ne garde donc, sur un élément que BlockNote connaît, que des attributs du type que
//! dit le registre des blocs (`mementum_schema`) :
//! - une prop de texte : une chaîne ; un booléen : un booléen ; un entier : un nombre entier que
//!   JavaScript écrit exactement (53 bits) ; un flottant : un nombre fini ;
//! - `colspan` d'une cellule : de 1 à [`MAX_WIDTH_COLUMNS`], la borne du MMD (Ruling R37) ;
//!   `rowspan` : au moins 1 ;
//! - `colwidth` d'une cellule, seule valeur qui n'est pas un scalaire : une liste d'au plus
//!   [`MAX_WIDTH_COLUMNS`] nombres finis, `null` ou `undefined` ;
//! - `id` d'un bloc, `kind`, `id` et `label` d'une mention, `latex` d'une formule : une chaîne ;
//! - un attribut hors du registre (une prop que le MMD recopie telle quelle) : une chaîne, un
//!   booléen ou un nombre fini ;
//! - `undefined` partout : c'est ce que y-prosemirror écrit d'une prop sans valeur, et l'éditeur
//!   la lit au défaut.
//!
//! Un élément d'un autre nom (un nœud étranger, Ruling R4) n'est pas jugé : aucun éditeur de ce
//! schéma ne le construit, et son aller-retour par le MMD garde ses attributs tels quels.
//!
//! La longueur d'une chaîne n'est pas bornée ici (Ruling R693).
//!
//! Le même jugement ([`sound`]) vaut pour ce qui vient d'un client (lu dans les octets de la mise
//! à jour, `guard::Shape::attrs`, puis dans la page, `PageDoc`), pour ce qui est relu du stockage
//! (l'attribut fautif est retiré), et pour ce que le serveur écrit (`write_props`, la recréation
//! d'un nœud étranger).

use std::collections::HashMap;
use std::sync::OnceLock;

use mementum_md::MAX_WIDTH_COLUMNS;
use mementum_schema::{PropSpec, PropType, blocks, cell_props};
use yrs::any::Number;
use yrs::{Any, Out};

use crate::blocknote::BLOCK_CONTAINER;

/// Plus grand entier qu'un nombre JavaScript écrit exactement (2^53 - 1).
pub(crate) const MAX_SAFE_INT: i64 = (1 << 53) - 1;

/// Ce que vaut un attribut, pour le jugement : sa nature, et sa valeur pour un nombre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Value {
    Undefined,
    Null,
    Bool,
    Int(i64),
    Float(f64),
    Str,
    /// Une liste d'au plus [`MAX_WIDTH_COLUMNS`] nombres finis, `null` ou `undefined`.
    Widths,
    /// Tout le reste : un objet, une autre liste, des octets, un type Yjs, un objet incorporé.
    Other,
}

impl Value {
    pub(crate) fn of(any: &Any) -> Value {
        match any {
            Any::Undefined => Value::Undefined,
            Any::Null => Value::Null,
            Any::Bool(_) => Value::Bool,
            Any::Number(Number::Int(i)) => Value::Int(*i),
            Any::Number(Number::Float(f)) => Value::Float(*f),
            Any::String(_) => Value::Str,
            Any::Array(values) if values.len() <= MAX_WIDTH_COLUMNS => {
                let width = |v: &Any| match v {
                    Any::Undefined | Any::Null => true,
                    Any::Number(_) => Value::of(v).is_width(),
                    _ => false,
                };
                if values.iter().all(width) {
                    Value::Widths
                } else {
                    Value::Other
                }
            }
            Any::Array(_) | Any::Map(_) | Any::Buffer(_) => Value::Other,
        }
    }

    /// La valeur d'un attribut telle que yrs la rend (un type Yjs n'est pas une valeur).
    pub(crate) fn of_out(out: &Out) -> Value {
        match out {
            Out::Any(any) => Value::of(any),
            _ => Value::Other,
        }
    }

    /// Vrai pour un nombre d'une liste de largeurs : fini, ou `NaN` (Ruling R705 : BlockNote
    /// écrit `[NaN, NaN]` pour toute cellule fusionnée collée sans largeurs ; le serveur et
    /// l'éditeur le lisent « sans largeur »).
    pub(crate) fn is_width(self) -> bool {
        matches!(self, Value::Float(f) if f.is_nan()) || self.is_finite()
    }

    /// Faux pour un nombre que JavaScript n'écrit pas exactement ou qui n'est pas fini.
    pub(crate) fn is_finite(self) -> bool {
        match self {
            Value::Int(i) => (-MAX_SAFE_INT..=MAX_SAFE_INT).contains(&i),
            Value::Float(f) => f.is_finite(),
            _ => true,
        }
    }
}

/// Ce qu'un attribut doit être.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expect {
    Str,
    Bool,
    /// Un nombre entier, entre ces bornes comprises.
    Int(i64, i64),
    Float,
    Widths,
    /// Un attribut hors du registre : une chaîne, un booléen ou un nombre fini.
    Scalar,
}

impl Expect {
    fn of(spec: &PropSpec) -> Expect {
        match spec.ty {
            PropType::String => Expect::Str,
            PropType::Bool => Expect::Bool,
            PropType::Int => Expect::Int(-MAX_SAFE_INT, MAX_SAFE_INT),
            PropType::Float => Expect::Float,
        }
    }

    /// Ce qui est attendu, pour le message d'un refus.
    fn name(self) -> &'static str {
        match self {
            Expect::Str => "une chaîne",
            Expect::Bool => "un booléen",
            Expect::Int(..) => "un nombre entier dans ses bornes",
            Expect::Float => "un nombre fini",
            Expect::Widths => "une liste de largeurs",
            Expect::Scalar => "une chaîne, un booléen ou un nombre fini",
        }
    }

    fn admits(self, value: Value) -> bool {
        match (self, value) {
            (_, Value::Undefined) => true,
            (Expect::Str, Value::Str) | (Expect::Bool, Value::Bool) => true,
            (Expect::Int(min, max), Value::Int(i)) => (min..=max).contains(&i),
            (Expect::Int(min, max), Value::Float(f)) => {
                // 2^53 n'est pas un entier sûr, et `MAX_SAFE_INT as f64` l'arrondit à lui.
                f.fract() == 0.0
                    && f >= min as f64
                    && f <= max as f64
                    && f.abs() < 9_007_199_254_740_992.0
            }
            (Expect::Float, Value::Int(_) | Value::Float(_)) => value.is_finite(),
            (Expect::Widths, Value::Widths) => true,
            (Expect::Scalar, Value::Str | Value::Bool) => true,
            (Expect::Scalar, Value::Int(_) | Value::Float(_)) => value.is_finite(),
            _ => false,
        }
    }
}

/// Les attributs attendus de chaque élément que BlockNote connaît, par balise.
type Table = HashMap<&'static str, Vec<(&'static str, Expect)>>;

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let of = |specs: &[PropSpec]| -> Vec<(&'static str, Expect)> {
            specs.iter().map(|s| (s.name, Expect::of(s))).collect()
        };
        let mut table = Table::new();
        for spec in blocks() {
            let mut attrs = of(spec.props);
            if !spec.wrapped {
                // Une liste de colonnes et une colonne portent leur identifiant de bloc.
                attrs.push(("id", Expect::Str));
            }
            table.insert(spec.name, attrs);
        }
        table.insert(BLOCK_CONTAINER, vec![("id", Expect::Str)]);
        for structural in ["blockGroup", "tableRow", "tableParagraph", "hardBreak"] {
            table.insert(structural, Vec::new());
        }
        let mut cell = of(cell_props());
        for (name, expect) in &mut cell {
            match *name {
                "colspan" => *expect = Expect::Int(1, MAX_WIDTH_COLUMNS as i64),
                "rowspan" => *expect = Expect::Int(1, MAX_SAFE_INT),
                _ => {}
            }
        }
        cell.push(("colwidth", Expect::Widths));
        table.insert("tableCell", cell.clone());
        table.insert("tableHeader", cell);
        table.insert(
            "mention",
            vec![
                ("kind", Expect::Str),
                ("id", Expect::Str),
                ("label", Expect::Str),
            ],
        );
        table.insert("math", vec![("latex", Expect::Str)]);
        table
    })
}

/// Les noms d'attributs que la table connaît, sous une balise ou une autre.
fn names() -> &'static HashMap<&'static str, &'static str> {
    static NAMES: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        table()
            .values()
            .flat_map(|attrs| attrs.iter().map(|(name, _)| (*name, *name)))
            .collect()
    })
}

/// La clé d'un attribut lue dans une mise à jour, sans la copier : celle d'un attribut que la
/// table connaît (sous une balise ou une autre), ou une autre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    Listed(&'static str),
    Unlisted,
}

impl Key {
    pub(crate) fn of(key: &str) -> Key {
        names()
            .get(key)
            .map_or(Key::Unlisted, |name| Key::Listed(name))
    }
}

/// Vrai si un élément de cette balise est jugé : BlockNote le connaît.
pub(crate) fn judged(tag: &str) -> bool {
    table().contains_key(tag)
}

/// Ce que les attributs d'un élément jugé doivent être, par nom.
pub(crate) type Expectations = &'static [(&'static str, Expect)];

/// Ce que les attributs d'un élément `tag` doivent être ; `None` si l'élément n'est pas jugé.
pub(crate) fn expectations(tag: &str) -> Option<Expectations> {
    table().get(tag).map(Vec::as_slice)
}

/// Ce qu'un attribut `key` doit être, parmi ceux d'un élément jugé.
fn expected(attrs: Expectations, key: Key) -> Expect {
    match key {
        Key::Listed(name) => attrs
            .iter()
            .find(|(n, _)| std::ptr::eq(*n, name) || *n == name)
            .map_or(Expect::Scalar, |(_, expect)| *expect),
        Key::Unlisted => Expect::Scalar,
    }
}

/// Vrai si un élément dont les attributs doivent être `attrs` garde `key` de valeur `value`.
pub(crate) fn admitted(attrs: Expectations, key: Key, value: Value) -> bool {
    expected(attrs, key).admits(value)
}

/// Vrai si la page garde l'attribut `key` de valeur `value` sur un élément `tag`.
pub(crate) fn sound(tag: &str, key: Key, value: Value) -> bool {
    expectations(tag).is_none_or(|attrs| admitted(attrs, key, value))
}

/// Le refus d'un attribut que la page ne garde pas (Ruling R689).
pub(crate) fn refusal(tag: &str, key: Key) -> String {
    let wanted = expectations(tag).map_or("d'un autre type", |attrs| expected(attrs, key).name());
    match key {
        Key::Listed(name) => format!(
            "la mise à jour pose sur un élément « {tag} » un attribut « {name} » qui n'est pas \
             {wanted}"
        ),
        Key::Unlisted => format!(
            "la mise à jour pose sur un élément « {tag} » un attribut hors du registre des blocs \
             qui n'est pas {wanted}"
        ),
    }
}

/// Le refus, quand on ne sait que la balise (la page après l'application).
pub(crate) fn refusal_in_page(tag: &str, key: &str) -> String {
    refusal(tag, Key::of(key))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn listed(key: &str) -> Key {
        let key = Key::of(key);
        assert!(matches!(key, Key::Listed(_)));
        key
    }

    #[test]
    fn registry_types_are_enforced_on_known_elements() {
        let object = Value::of(&Any::Map(Arc::new(HashMap::from([(
            "toString".to_string(),
            Any::Number(Number::Int(1)),
        )]))));
        assert_eq!(object, Value::Other);
        for (tag, key) in [
            ("paragraph", "backgroundColor"),
            ("heading", "level"),
            ("image", "url"),
            ("checkListItem", "checked"),
            ("image", "previewWidth"),
            ("blockContainer", "id"),
            ("column", "width"),
            ("tableCell", "colwidth"),
            ("mention", "label"),
        ] {
            assert!(!sound(tag, listed(key), object), "{tag}.{key}");
            assert!(!sound(tag, listed(key), Value::Null), "{tag}.{key} null");
            assert!(sound(tag, listed(key), Value::Undefined), "{tag}.{key}");
        }
        assert!(sound("heading", listed("level"), Value::Int(3)));
        assert!(sound("heading", listed("level"), Value::Float(3.0)));
        assert!(!sound("heading", listed("level"), Value::Float(1e21)));
        assert!(!sound("heading", listed("level"), Value::Float(2.5)));
        assert!(!sound("heading", listed("level"), Value::Str));
        assert!(!sound("heading", listed("level"), Value::Int(i64::MAX)));
        // 2^53 n'est pas un entier sûr (m3 de la relecture).
        assert!(!sound(
            "heading",
            listed("level"),
            Value::Float(9_007_199_254_740_992.0)
        ));
        assert!(sound(
            "heading",
            listed("level"),
            Value::Float(9_007_199_254_740_991.0)
        ));
        assert!(sound("image", listed("previewWidth"), Value::Float(1.5)));
        assert!(sound("image", listed("previewWidth"), Value::Int(320)));
        assert!(!sound(
            "image",
            listed("previewWidth"),
            Value::Float(f64::NAN)
        ));
        assert!(!sound("image", listed("previewWidth"), Value::Str));
        assert!(sound("checkListItem", listed("checked"), Value::Bool));
        assert!(!sound("checkListItem", listed("checked"), Value::Str));
        assert!(!sound("paragraph", listed("textAlignment"), Value::Bool));
    }

    #[test]
    fn spans_and_widths_have_their_bounds() {
        for cell in ["tableCell", "tableHeader"] {
            assert!(sound(cell, listed("colspan"), Value::Int(1)));
            assert!(sound(cell, listed("colspan"), Value::Int(100)));
            assert!(!sound(cell, listed("colspan"), Value::Int(101)));
            assert!(!sound(cell, listed("colspan"), Value::Int(0)));
            assert!(!sound(cell, listed("colspan"), Value::Float(1e9)));
            assert!(sound(cell, listed("rowspan"), Value::Int(100_000)));
            assert!(!sound(cell, listed("rowspan"), Value::Int(0)));
            assert!(sound(cell, listed("colwidth"), Value::Widths));
            assert!(!sound(cell, listed("colwidth"), Value::Str));
            assert!(!sound(cell, listed("colwidth"), Value::Int(120)));
        }
        let list = |values: Vec<Any>| Value::of(&Any::Array(values.into()));
        let n = |i: i64| Any::Number(Number::Int(i));
        assert_eq!(list(vec![n(120), Any::Undefined, Any::Null]), Value::Widths);
        assert_eq!(list(vec![]), Value::Widths);
        assert_eq!(list(vec![Any::from("120")]), Value::Other);
        // `NaN` est une largeur absente (R705) ; l'infini n'en est pas une.
        assert_eq!(
            list(vec![Any::Number(Number::Float(f64::NAN)), n(120)]),
            Value::Widths
        );
        assert_eq!(
            list(vec![Any::Number(Number::Float(f64::INFINITY))]),
            Value::Other
        );
        assert_eq!(list(vec![list_any(vec![n(1)])]), Value::Other);
        assert_eq!(list(vec![n(1); MAX_WIDTH_COLUMNS]), Value::Widths);
        assert_eq!(list(vec![n(1); MAX_WIDTH_COLUMNS + 1]), Value::Other);
        // Une liste n'est une valeur que pour `colwidth`.
        assert!(!sound(
            "paragraph",
            listed("backgroundColor"),
            Value::Widths
        ));
        assert!(!sound("paragraph", Key::Unlisted, Value::Widths));
    }

    fn list_any(values: Vec<Any>) -> Any {
        Any::Array(values.into())
    }

    #[test]
    fn an_attribute_outside_the_registry_is_a_scalar() {
        for value in [
            Value::Str,
            Value::Bool,
            Value::Int(7),
            Value::Float(0.5),
            Value::Undefined,
        ] {
            assert!(sound("paragraph", Key::Unlisted, value), "{value:?}");
            // Connu sous une autre balise : hors du registre pour celle-ci.
            assert!(sound("paragraph", listed("level"), value), "{value:?}");
        }
        for value in [
            Value::Other,
            Value::Null,
            Value::Float(f64::INFINITY),
            Value::Int(i64::MIN),
        ] {
            assert!(!sound("paragraph", Key::Unlisted, value), "{value:?}");
        }
    }

    #[test]
    fn a_foreign_element_is_not_judged() {
        assert!(!judged("schemaPlusRecent"));
        assert!(sound("schemaPlusRecent", Key::Unlisted, Value::Other));
        assert!(sound("schemaPlusRecent", listed("level"), Value::Other));
        for tag in [
            "blockGroup",
            "blockContainer",
            "tableRow",
            "tableCell",
            "tableHeader",
            "tableParagraph",
            "mention",
            "math",
            "hardBreak",
            "columnList",
            "column",
            "paragraph",
            "unknown",
        ] {
            assert!(judged(tag), "{tag}");
        }
    }

    #[test]
    fn a_refusal_names_the_element_and_the_attribute() {
        let why = refusal("heading", listed("level"));
        assert!(why.contains("heading") && why.contains("level"), "{why}");
        let why = refusal_in_page("paragraph", "inconnu");
        assert!(why.contains("hors du registre"), "{why}");
    }
}
