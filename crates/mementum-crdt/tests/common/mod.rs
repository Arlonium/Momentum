//! Outils partagés des tests de fixtures : l'arbre XML d'un document Yjs,
//! écrit de façon stable pour comparer ce qu'écrit le serveur à ce
//! qu'écrit BlockNote.

use std::collections::BTreeMap;

use mementum_crdt::FRAGMENT;
use yrs::any::Number;
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, OffsetKind, Options, Out, ReadTxn, Text, Transact, Update, Xml, XmlElementRef,
    XmlFragment, XmlOut,
};

/// Valeur yrs écrite de façon stable (clés triées, type des nombres visible).
fn canonical(value: &Any) -> String {
    match value {
        Any::Number(Number::Int(i)) => format!("{i}i"),
        Any::Number(Number::Float(f)) => format!("{f}f"),
        Any::String(s) => format!("{s:?}"),
        Any::Map(m) => {
            let sorted: BTreeMap<_, _> = m.iter().map(|(k, v)| (k, canonical(v))).collect();
            format!("{sorted:?}")
        }
        Any::Array(a) => format!("{:?}", a.iter().map(canonical).collect::<Vec<_>>()),
        other => format!("{other:?}"),
    }
}

/// L'arbre XML d'un document, une ligne par nœud ou plage de texte. Les
/// attributs pour lesquels `skip(balise, clé, valeur)` est vrai sont omis.
pub fn dump(state: &[u8], skip: &dyn Fn(&str, &str, &Any) -> bool) -> Vec<String> {
    fn walk<T: ReadTxn>(
        txn: &T,
        element: &XmlElementRef,
        depth: usize,
        skip: &dyn Fn(&str, &str, &Any) -> bool,
        out: &mut Vec<String>,
    ) {
        let tag = element.tag().to_string();
        let attrs: BTreeMap<String, String> = element
            .attributes(txn)
            .filter_map(|(k, v)| match v {
                Out::Any(a) if !skip(&tag, k, &a) => Some((k.to_string(), canonical(&a))),
                _ => None,
            })
            .collect();
        out.push(format!("{}<{tag} {attrs:?}>", "  ".repeat(depth)));
        for child in element.children(txn) {
            match child {
                XmlOut::Element(e) => walk(txn, &e, depth + 1, skip, out),
                XmlOut::Text(t) => {
                    for run in t.diff(txn, YChange::identity) {
                        let marks: BTreeMap<String, String> = run
                            .attributes
                            .iter()
                            .flat_map(|a| a.iter())
                            .map(|(k, v)| (k.to_string(), canonical(v)))
                            .collect();
                        out.push(format!(
                            "{}{:?} {marks:?}",
                            "  ".repeat(depth + 1),
                            run.insert
                        ));
                    }
                }
                XmlOut::Fragment(_) => {}
            }
        }
    }
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(state).unwrap())
        .unwrap();
    let txn = doc.transact();
    let mut out = Vec::new();
    for child in fragment.children(&txn) {
        if let XmlOut::Element(e) = child {
            walk(&txn, &e, 0, skip, &mut out);
        }
    }
    out
}
