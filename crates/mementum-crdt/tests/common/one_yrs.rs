// Un yrs, vu par ce qu'il rend (Ruling R875). Ce fichier est inclus deux fois par `include!`, une
// fois par crate : `y` est le yrs du serveur (`vendor/yrs`, corrigé) dans un module, celui de
// crates.io (0.28.1, d'origine) dans l'autre. Le même code mène donc les deux, et tout ce qu'il
// rend est fait d'octets et de chaînes, que l'appelant compare.
//
// Ce qu'on voit d'un document ([`Seen`]) : l'état encodé (v1 et v2 : chaque bloc y est écrit tel
// que yrs le garde, avec ses bornes, ses origines, son parent, sa marque d'entrée de map et son
// contenu ; puis le jeu de suppressions), le vecteur d'état, et ce qu'on **lit** de chaque type
// racine en le parcourant (texte et marques plage par plage, listes, entrées de map, arbre XML et
// attributs, texte cité par un lien) : la lecture suit les voisins de gauche à droite et les
// entrées courantes des maps, que l'encodage ne dit pas.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use y::types::Attrs;
use y::types::text::YChange;
use y::undo::{Options as UndoOptions, UndoManager};
use y::updates::decoder::Decode;
use y::updates::encoder::Encode;
use y::{
    Any, Array, ClientID, Doc, GetString, Map, OffsetKind, Options, Out, Quotable, ReadTxn,
    StateVector, Text, TextRef, Transact, Update, WeakRef, WriteTxn, Xml, XmlFragment, XmlOut,
    XmlTextPrelim, XmlTextRef,
};

/// L'état v1, l'état v2, le vecteur d'état, la lecture.
pub type Seen = (Vec<u8>, Vec<u8>, Vec<u8>, String);

pub fn doc(client: u64, skip_gc: bool) -> Doc {
    Doc::with_options(Options {
        client_id: ClientID::new(client),
        offset_kind: OffsetKind::Utf16,
        skip_gc,
        ..Options::default()
    })
}

/// Une valeur, les clés d'un objet dans l'ordre.
fn any(value: &Any) -> String {
    match value {
        Any::Array(items) => {
            let items: Vec<String> = items.iter().map(any).collect();
            format!("[{}]", items.join(","))
        }
        Any::Map(map) => format!(
            "{{{}}}",
            sorted(map.iter().map(|(k, v)| (k.as_str(), any(v))))
        ),
        other => format!("{other:?}"),
    }
}

fn sorted<'a>(entries: impl Iterator<Item = (&'a str, String)>) -> String {
    let mut entries: Vec<String> = entries.map(|(k, v)| format!("{k:?}:{v}")).collect();
    entries.sort_unstable();
    entries.join(",")
}

fn attrs(attrs: &Attrs) -> String {
    sorted(attrs.iter().map(|(k, v)| (&**k, any(v))))
}

/// Un texte, plage par plage, avec ses marques ; et ses attributs.
fn text<T: ReadTxn>(txn: &T, text: &XmlTextRef, depth: usize) -> String {
    let chunks: Vec<String> = Text::diff(text, txn, YChange::identity)
        .iter()
        .map(|chunk| {
            let marks = chunk.attributes.as_deref().map(attrs).unwrap_or_default();
            format!("{}({marks})", out(txn, &chunk.insert, depth + 1))
        })
        .collect();
    let own = sorted(
        text.attributes(txn)
            .map(|(k, v)| (k, out(txn, &v, depth + 1))),
    );
    format!("texte{{{own}}}[{}]", chunks.join(""))
}

fn children<T: ReadTxn, X: XmlFragment>(txn: &T, parent: &X, depth: usize) -> String {
    let children: Vec<String> = (0..parent.len(txn))
        .map(|at| match parent.get(txn, at) {
            None => "∅".to_string(),
            Some(XmlOut::Element(e)) => out(txn, &Out::YXmlElement(e), depth + 1),
            Some(XmlOut::Fragment(f)) => out(txn, &Out::YXmlFragment(f), depth + 1),
            Some(XmlOut::Text(t)) => out(txn, &Out::YXmlText(t), depth + 1),
        })
        .collect();
    format!("{}:[{}]", parent.len(txn), children.join(","))
}

fn out<T: ReadTxn>(txn: &T, value: &Out, depth: usize) -> String {
    if depth > 300 {
        return "…".to_string();
    }
    match value {
        Out::Any(value) => any(value),
        Out::YText(t) => text(txn, t.as_ref(), depth),
        Out::YXmlText(t) => text(txn, t, depth),
        Out::YArray(a) => {
            let items: Vec<String> = a.iter(txn).map(|v| out(txn, &v, depth + 1)).collect();
            format!("liste[{}]", items.join(","))
        }
        Out::YMap(m) => format!(
            "map{{{}}}",
            sorted(m.iter(txn).map(|(k, v)| (k, out(txn, &v, depth + 1))))
        ),
        Out::YXmlElement(e) => format!(
            "<{} {}>{}",
            e.tag(),
            sorted(e.attributes(txn).map(|(k, v)| (k, out(txn, &v, depth + 1)))),
            children(txn, e, depth)
        ),
        Out::YXmlFragment(f) => format!("fragment{}", children(txn, f, depth)),
        Out::YWeakLink(link) => {
            let link: WeakRef<TextRef> = WeakRef::from(link.clone());
            format!("lien[{}]", link.get_string(txn))
        }
        Out::YDoc(_) => "document".to_string(),
        Out::UndefinedRef(_) => "sans type".to_string(),
    }
}

/// Ce qu'on lit de chaque type racine, dans l'ordre de leurs noms.
fn reading<T: ReadTxn>(txn: &T) -> String {
    let mut roots: Vec<(String, String)> = txn
        .root_refs()
        .map(|(name, value)| (name.to_string(), out(txn, &value, 0)))
        .collect();
    roots.sort_unstable();
    let roots: Vec<String> = roots
        .into_iter()
        .map(|(k, v)| format!("{k} = {v}"))
        .collect();
    roots.join("\n")
}

pub fn seen(doc: &Doc) -> Seen {
    let txn = doc.transact();
    // Les blocs et le jeu de suppressions du magasin, sans ce qui attend : yrs 0.28.1 panique en
    // y mêlant une mise à jour en attente qu'il ne relit pas (`encode_state_as_update_v1`).
    let waiting = (
        txn.store().pending_update().is_some(),
        txn.store().pending_ds().is_some(),
    );
    (
        txn.encode_diff_v1(&StateVector::default()),
        txn.encode_diff_v2(&StateVector::default()),
        txn.state_vector().encode_v1(),
        format!(
            "{}
en attente = {waiting:?}",
            reading(&txn)
        ),
    )
}

/// Applique une mise à jour, et dit ce que yrs en a répondu.
pub fn apply(doc: &Doc, update: &[u8]) -> String {
    match Update::decode_v1(update) {
        Err(error) => format!("illisible : {error}"),
        Ok(update) => match doc.transact_mut().apply_update(update) {
            Err(error) => format!("refusée : {error}"),
            Ok(()) => "appliquée".to_string(),
        },
    }
}

/// Un document neuf qui reçoit `updates` une à une : la réponse de yrs et ce qu'on voit du
/// document après chacune. Ses types racines sont ceux de `roots` : `t` pour un texte, `x` pour
/// un fragment XML, `m` pour une map.
pub fn replay(
    client: u64,
    skip_gc: bool,
    roots: &[(&str, char)],
    updates: &[Vec<u8>],
) -> Vec<(String, Seen)> {
    let doc = doc(client, skip_gc);
    for (name, kind) in roots {
        match kind {
            't' => drop(doc.get_or_insert_text(*name)),
            'x' => drop(doc.get_or_insert_xml_fragment(*name)),
            _ => drop(doc.get_or_insert_map(*name)),
        }
    }
    updates
        .iter()
        .map(|update| (apply(&doc, update), seen(&doc)))
        .collect()
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

const WORDS: [&str; 8] = ["a", "b", "xy", "été", "😀", "l", "mn", " "];
const KEYS: [&str; 3] = ["k", "l", "m"];

fn value(rng: &mut Rng) -> Any {
    match rng.below(5) {
        0 => Any::from(rng.below(9) as f64),
        1 => Any::from(true),
        2 => Any::from(WORDS[rng.below(8) as usize]),
        3 => Any::Null,
        _ => Any::from(1.5),
    }
}

/// Un geste dans une transaction ouverte, par l'API de yrs.
fn gesture(rng: &mut Rng, txn: &mut y::TransactionMut) {
    let text = txn.get_or_insert_text("t");
    let array = txn.get_or_insert_array("a");
    let map = txn.get_or_insert_map("m");
    let fragment = txn.get_or_insert_xml_fragment("x");
    let len = text.len(txn);
    let items = array.len(txn);
    match rng.below(16) {
        // La frappe : une lettre à la fin, puis une autre, chacune un élément jusqu'à la
        // validation.
        0..=3 => {
            for _ in 0..1 + rng.below(5) {
                let at = text.len(txn);
                text.insert(txn, at, WORDS[rng.below(8) as usize]);
            }
        }
        4 => {
            let at = rng.below(u64::from(len) + 1) as u32;
            text.insert(txn, at, WORDS[rng.below(8) as usize]);
        }
        5 if len > 0 => {
            let at = rng.below(u64::from(len)) as u32;
            let count = 1 + rng.below(u64::from(len - at).min(3)) as u32;
            text.remove_range(txn, at, count);
        }
        6 if len > 0 => {
            let at = rng.below(u64::from(len)) as u32;
            let count = 1 + rng.below(u64::from(len - at).min(4)) as u32;
            let attrs: Attrs = HashMap::from([(Arc::from("bold"), value(rng))]);
            text.format(txn, at, count, attrs);
        }
        7 => {
            let at = rng.below(u64::from(len) + 1) as u32;
            text.insert_embed(txn, at, value(rng));
        }
        // Des valeurs une à une à la fin d'une liste : autant d'éléments `Any`.
        8 | 9 => {
            for _ in 0..1 + rng.below(5) {
                array.push_back(txn, value(rng));
            }
        }
        10 => {
            let at = rng.below(u64::from(items) + 1) as u32;
            let values: Vec<Any> = (0..1 + rng.below(3)).map(|_| value(rng)).collect();
            array.insert_range(txn, at, values);
        }
        11 if items > 0 => {
            let at = rng.below(u64::from(items)) as u32;
            let count = 1 + rng.below(u64::from(items - at).min(3)) as u32;
            array.remove_range(txn, at, count);
        }
        12 => {
            map.insert(txn, KEYS[rng.below(3) as usize], value(rng));
        }
        13 => {
            map.remove(txn, KEYS[rng.below(3) as usize]);
        }
        // Un lien vers une plage du texte : ses éléments sont cités (`linked`), et ne se fondent
        // plus.
        14 if len > 1 => {
            let from = rng.below(u64::from(len - 1)) as u32;
            let to = from + rng.below(u64::from(len - 1 - from).min(3)) as u32;
            if let Ok(link) = text.quote(txn, from..=to) {
                map.insert(txn, "lien", link);
            }
        }
        _ => {
            let at = fragment.len(txn);
            let inner = fragment.insert(txn, at, XmlTextPrelim::new("p"));
            for _ in 0..rng.below(4) {
                let end = Text::len(&inner, txn);
                Text::insert(&inner, txn, end, WORDS[rng.below(8) as usize]);
            }
        }
    }
}

/// Un geste d'une seule unité du premier document, que son gestionnaire d'annulation retient :
/// une lettre ou une valeur de plus, ou de moins.
fn kept_gesture(rng: &mut Rng, txn: &mut y::TransactionMut) {
    let text = txn.get_or_insert_text("t");
    let array = txn.get_or_insert_array("a");
    let len = text.len(txn);
    let items = array.len(txn);
    match rng.below(8) {
        0 | 1 => text.insert(txn, len, "a"),
        2 | 3 if len > 0 => text.remove_range(txn, len - 1, 1),
        4 if len > 0 => text.remove_range(txn, rng.below(u64::from(len)) as u32, 1),
        5 => {
            array.push_back(txn, value(rng));
        }
        6 if items > 0 => array.remove_range(txn, items - 1, 1),
        _ => text.insert(txn, rng.below(u64::from(len) + 1) as u32, "b"),
    }
}

/// Une scène tirée au hasard, par l'API de yrs : trois documents, des transactions de un à cinq
/// gestes, chacune envoyée aux autres **telle qu'elle est avant sa validation** (ses éléments ne
/// sont pas encore fondus : qui la reçoit les fond), tout de suite ou plus tard, des échanges
/// d'états, et, pour le premier document, des gestes d'une unité qu'il annule et rétablit. Rend
/// ce qu'on voit des trois documents après chaque pas.
///
/// Le gestionnaire d'annulation ne retient que ces gestes d'une unité : yrs rétablit les éléments
/// d'un pas dans l'ordre d'une table de hachage (`UndoManager::try_process`), si bien qu'un pas de
/// plusieurs éléments ne donne pas deux fois les mêmes horloges.
pub fn scene(seed: u64, steps: usize) -> Vec<(String, Vec<Seen>)> {
    let mut rng = Rng(seed);
    let skip_gc = rng.chance(25);
    // Le premier client est tantôt le plus petit, tantôt le plus grand.
    let clients: [u64; 3] = if rng.chance(50) { [3, 7, 5] } else { [9, 4, 6] };
    let docs: Vec<Doc> = clients.iter().map(|c| doc(*c, skip_gc)).collect();
    for doc in &docs {
        doc.get_or_insert_text("t");
        doc.get_or_insert_array("a");
        doc.get_or_insert_map("m");
        doc.get_or_insert_xml_fragment("x");
    }
    let kept = Arc::new(AtomicBool::new(false));
    let capture = Arc::clone(&kept);
    let mut manager: UndoManager<()> = UndoManager::with_options(UndoOptions {
        capture_timeout_millis: 0,
        capture_transaction: Some(Arc::new(move |_| capture.load(Ordering::Relaxed))),
        ..UndoOptions::default()
    });
    manager.expand_scope(&docs[0], &docs[0].get_or_insert_text("t"));
    manager.expand_scope(&docs[0], &docs[0].get_or_insert_array("a"));
    // Ce qui attend d'être reçu : (destinataire, mise à jour).
    let mut mail: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut out = Vec::new();
    for _ in 0..steps {
        let who = rng.below(3) as usize;
        let what = match rng.below(24) {
            0..=7 => {
                let update = {
                    let mut txn = docs[who].transact_mut();
                    for _ in 0..1 + rng.below(5) {
                        gesture(&mut rng, &mut txn);
                    }
                    txn.encode_update_v1()
                };
                for other in 0..3 {
                    if other != who {
                        mail.push((other, update.clone()));
                    }
                }
                "transaction".to_string()
            }
            8..=11 => {
                kept.store(true, Ordering::Relaxed);
                let update = {
                    let mut txn = docs[0].transact_mut();
                    kept_gesture(&mut rng, &mut txn);
                    txn.encode_update_v1()
                };
                kept.store(false, Ordering::Relaxed);
                mail.push((1, update.clone()));
                mail.push((2, update));
                "geste retenu".to_string()
            }
            12..=16 if !mail.is_empty() => {
                // Dans l'ordre, ou non : ce qui arrive trop tôt attend chez yrs.
                let at = if rng.chance(70) {
                    0
                } else {
                    rng.below(mail.len() as u64) as usize
                };
                let (to, update) = mail.remove(at);
                format!("reçu : {}", apply(&docs[to], &update))
            }
            17..=19 => {
                kept.store(true, Ordering::Relaxed);
                let done = manager.undo_blocking();
                kept.store(false, Ordering::Relaxed);
                format!("annuler : {done}")
            }
            20 | 21 => {
                kept.store(true, Ordering::Relaxed);
                let done = manager.redo_blocking();
                kept.store(false, Ordering::Relaxed);
                format!("rétablir : {done}")
            }
            _ => {
                // Un échange d'états : ce qui manque à l'un, encodé par l'autre.
                let to = (who + 1 + rng.below(2) as usize) % 3;
                let vector = docs[to].transact().state_vector();
                let diff = docs[who].transact().encode_diff_v1(&vector);
                format!("état : {}", apply(&docs[to], &diff))
            }
        };
        out.push((what, docs.iter().map(seen).collect()));
    }
    out
}
