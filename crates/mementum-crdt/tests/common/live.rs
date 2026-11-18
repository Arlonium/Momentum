//! La page relue de son propre état est-elle la page vivante (constat I-7.1 de la re-relecture 7
//! de la garde des mises à jour) ? C'est ce que font la compaction (l'instantané qui remplace le
//! journal, relu au chargement) et l'étape 2 d'un onglet qui ouvre la page (l'état entier). Les
//! essais relisaient l'état par un yrs neuf pour savoir **s'il se relit**, et rechargeaient la page
//! **de son journal** : aucun ne comparait la page relue de son état à la page vivante, et un
//! élément que yrs comme Yjs rangent selon le découpage de leurs blocs (Ruling R825) y passait,
//! sous la même révision.
//!
//! Inclus par son chemin dans les binaires de test qui s'en servent.

use std::collections::BTreeMap;

use mementum_crdt::{FRAGMENT, META, PageDoc};
use yrs::any::Number;
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, Map, OffsetKind, Options, Out, ReadTxn, Text, Transact, Update, Xml, XmlElementRef,
    XmlFragment, XmlOut,
};

/// Une valeur écrite de façon stable (clés triées, type des nombres visible).
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

/// Ce que vaut une valeur lue d'un document, sans rien qui dépende de l'adresse d'un type.
fn shown(value: &Out) -> String {
    match value {
        Out::Any(a) => canonical(a),
        Out::YXmlElement(e) => format!("<{}>", e.tag()),
        Out::YXmlText(_) => "texte".to_string(),
        Out::YXmlFragment(_) => "fragment".to_string(),
        Out::YText(_) => "Y.Text".to_string(),
        Out::YArray(_) => "Y.Array".to_string(),
        Out::YMap(_) => "Y.Map".to_string(),
        Out::YDoc(_) => "sous-document".to_string(),
        _ => "autre".to_string(),
    }
}

/// L'arbre de la page **tel que son document le porte**, sans le réencoder : une ligne par
/// élément (sa balise et ses attributs, triés), par plage de texte (son contenu et ses marques,
/// triées), dans l'ordre du document ; puis les métadonnées. (Ne jamais passer par `get_string`,
/// dont l'ordre des attributs n'est pas stable.)
pub fn tree(doc: &Doc) -> Vec<String> {
    fn walk<T: ReadTxn>(txn: &T, element: &XmlElementRef, depth: usize, out: &mut Vec<String>) {
        let attrs: BTreeMap<String, String> = element
            .attributes(txn)
            .map(|(k, v)| (k.to_string(), shown(&v)))
            .collect();
        out.push(format!(
            "{}<{} {attrs:?}>",
            "  ".repeat(depth),
            element.tag()
        ));
        children(txn, element.children(txn), depth + 1, out);
    }
    fn children<T: ReadTxn>(
        txn: &T,
        nodes: impl Iterator<Item = XmlOut>,
        depth: usize,
        out: &mut Vec<String>,
    ) {
        for child in nodes {
            match child {
                XmlOut::Element(e) => walk(txn, &e, depth, out),
                XmlOut::Text(t) => {
                    out.push(format!("{}texte", "  ".repeat(depth)));
                    for run in t.diff(txn, YChange::identity) {
                        let marks: BTreeMap<String, String> = run
                            .attributes
                            .iter()
                            .flat_map(|a| a.iter())
                            .map(|(k, v)| (k.to_string(), canonical(v)))
                            .collect();
                        out.push(format!(
                            "{}{} {marks:?}",
                            "  ".repeat(depth + 1),
                            shown(&run.insert)
                        ));
                    }
                }
                XmlOut::Fragment(_) => out.push(format!("{}fragment", "  ".repeat(depth))),
            }
        }
    }
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let meta = doc.get_or_insert_map(META);
    let txn = doc.transact();
    let mut out = Vec::new();
    children(&txn, fragment.children(&txn), 0, &mut out);
    let meta: BTreeMap<String, String> = meta
        .iter(&txn)
        .map(|(k, v)| (k.to_string(), shown(&v)))
        .collect();
    out.push(format!("métadonnées {meta:?}"));
    out
}

/// `None` si la page relue de son propre état (`PageDoc::from_updates([état])`, ce que relit le
/// chargement d'un instantané) a les blocs, l'arbre et la révision de la page vivante ; sinon,
/// ce qui diffère. Un état que la page ne relit pas est aussi un écart.
pub fn reloaded_from_state_differs(page: &PageDoc) -> Option<String> {
    let state = page.encode_state();
    let again = match PageDoc::from_updates([state.as_slice()]) {
        Ok(again) => again,
        Err(error) => return Some(format!("la page ne se relit pas de son état : {error}")),
    };
    if again.read_blocks() != page.read_blocks() {
        return Some("la page relue de son état n'a pas les blocs de la page vivante".to_string());
    }
    let (live, read) = (tree(page.yrs_doc()), tree(again.yrs_doc()));
    if live != read {
        let at = live.iter().zip(&read).position(|(a, b)| a != b);
        return Some(format!(
            "la page relue de son état n'a pas l'arbre de la page vivante (première ligne qui \
             diffère : {at:?} ; vivante {:?}, relue {:?})",
            at.and_then(|n| live.get(n)),
            at.and_then(|n| read.get(n))
        ));
    }
    if again.revision() != page.revision() {
        return Some(
            "la page relue de son état n'a pas la révision de la page vivante".to_string(),
        );
    }
    None
}

/// `None` si **un lecteur sans garde**, un yrs neuf à qui la page sert son état, lit l'arbre de la
/// page vivante ; sinon, ce qui diffère (Ruling R862). La page relue de son état
/// ([`reloaded_from_state_differs`]) repasse par les règles de la garde : elle récrit à la relecture
/// ce qui a été mal récrit à l'arrivée, et ne voit pas l'écart que voit un navigateur.
#[allow(dead_code)]
pub fn bare_reader_differs(page: &PageDoc) -> Option<String> {
    let bare = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let update = match Update::decode_v1(&page.encode_state()) {
        Ok(update) => update,
        Err(error) => return Some(format!("l'état servi ne se lit pas : {error}")),
    };
    if let Err(error) = bare.transact_mut().apply_update(update) {
        return Some(format!("l'état servi ne s'intègre pas : {error}"));
    }
    if bare.transact().store().pending_update().is_some() {
        return Some("l'état servi laisse des éléments en attente".to_string());
    }
    let (live, read) = (tree(page.yrs_doc()), tree(&bare));
    if live == read {
        return None;
    }
    let at = live.iter().zip(&read).position(|(a, b)| a != b);
    Some(format!(
        "un yrs sans garde ne lit pas l'arbre de la page vivante (première ligne qui diffère : \
         {at:?} ; vivante {:?}, sans garde {:?})",
        at.and_then(|n| live.get(n)),
        at.and_then(|n| read.get(n))
    ))
}

/// Le plan d'un document : ses balises et le texte brut de ses textes, ce que yrs et Yjs écrivent
/// pareil (`apps/web/src/editor/servedStates.test.ts` fait le même relevé dans un vrai Yjs).
#[allow(dead_code)]
pub fn outline(doc: &Doc) -> Vec<String> {
    fn walk<T: ReadTxn>(txn: &T, nodes: impl Iterator<Item = XmlOut>, out: &mut Vec<String>) {
        for node in nodes {
            match node {
                XmlOut::Element(element) => {
                    out.push(format!("<{}>", element.tag()));
                    walk(txn, element.children(txn), out);
                    out.push(format!("</{}>", element.tag()));
                }
                XmlOut::Text(text) => {
                    let runs = text.diff(txn, YChange::identity);
                    let letters: String = runs
                        .iter()
                        .map(|run| match &run.insert {
                            Out::Any(Any::String(letters)) => letters.to_string(),
                            _ => "?".to_string(),
                        })
                        .collect();
                    out.push(format!("texte {letters}"));
                }
                XmlOut::Fragment(_) => out.push("fragment".to_string()),
            }
        }
    }
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    let mut out = Vec::new();
    walk(&txn, fragment.children(&txn), &mut out);
    out
}
