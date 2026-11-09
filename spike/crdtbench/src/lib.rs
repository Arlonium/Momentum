//! Même page de blocs modélisée avec yrs (structure BlockNote) et avec Loro (arbre déplaçable).

/// Petit générateur xorshift64* déterministe.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn below(&mut self, n: usize) -> usize {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) % n.max(1) as u64) as usize
    }
}

/// Éditions d'un pair : `edits` modifications de texte, puis déplacement des blocs `moves`.
pub struct Workload {
    pub edits: usize,
    pub moves_a: Vec<usize>,
    pub moves_b: Vec<usize>,
}

/// Ligne de 80 caractères ASCII pour le bloc `i`.
pub fn line(i: usize) -> String {
    let mut s = format!("Bloc {i:05} : une ligne de texte de quatre-vingts caracteres environ, pour mesurer.");
    s.truncate(80);
    s
}

pub mod y {
    use std::collections::HashMap;

    use yrs::updates::decoder::Decode;
    use yrs::{
        Doc, GetString, OffsetKind, Options, ReadTxn, StateVector, Text, Transact, Update, Xml, XmlElementPrelim, XmlElementRef,
        XmlFragment, XmlOut, XmlTextPrelim, XmlTextRef,
    };

    use super::{line, Rng};

    const FRAGMENT: &str = "document-store";

    pub fn new_doc(client: u64) -> Doc {
        Doc::with_options(Options { client_id: yrs::ClientID::new(client + 1), offset_kind: OffsetKind::Utf16, ..Default::default() })
    }

    /// Le fragment doit être obtenu AVANT d'ouvrir la transaction : avec la feature `sync`,
    /// `get_or_insert_xml_fragment` attend le verrou d'écriture (interblocage sinon).
    fn group(frag: &yrs::XmlFragmentRef, txn: &impl ReadTxn) -> XmlElementRef {
        match frag.get(txn, 0) {
            Some(XmlOut::Element(g)) => g,
            _ => panic!("pas de blockGroup"),
        }
    }

    fn id_of(el: &XmlElementRef, txn: &impl ReadTxn) -> String {
        el.get_attribute(txn, "id").map(|o| o.to_string(txn)).unwrap_or_default()
    }

    fn text_of(container: &XmlElementRef, txn: &impl ReadTxn) -> XmlTextRef {
        let Some(XmlOut::Element(p)) = container.get(txn, 0) else { panic!("pas de paragraphe") };
        let Some(XmlOut::Text(t)) = p.get(txn, 0) else { panic!("pas de texte") };
        t
    }

    fn push_block(group: &XmlElementRef, txn: &mut yrs::TransactionMut, at: u32, id: &str, text: &str) {
        let c = group.insert(txn, at, XmlElementPrelim::empty("blockContainer"));
        c.insert_attribute(txn, "id", id);
        let p = c.push_back(txn, XmlElementPrelim::empty("paragraph"));
        p.push_back(txn, XmlTextPrelim::new(text));
    }

    /// Page de `n` paragraphes au format BlockNote.
    pub fn built(n: usize) -> Doc {
        let doc = new_doc(0);
        let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let g = frag.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
        for i in 0..n {
            push_block(&g, &mut txn, i as u32, &format!("b{i}"), &line(i));
        }
        drop(txn);
        doc
    }

    pub fn snapshot(doc: &Doc) -> Vec<u8> {
        doc.transact().encode_state_as_update_v1(&StateVector::default())
    }

    pub fn load(bytes: &[u8], client: u64) -> Doc {
        let doc = new_doc(client);
        doc.get_or_insert_xml_fragment(FRAGMENT);
        doc.transact_mut().apply_update(Update::decode_v1(bytes).unwrap()).unwrap();
        doc
    }

    /// `edits` modifications de texte aléatoires, puis déplacement (suppression + réinsertion) des blocs `moves`.
    pub fn edit_random(doc: &Doc, rng: &mut Rng, edits: usize, moves: &[usize]) {
        let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let g = group(&frag, &txn);
        let containers: HashMap<String, XmlElementRef> = g
            .children(&txn)
            .filter_map(|c| match c {
                XmlOut::Element(e) => Some((id_of(&e, &txn), e)),
                _ => None,
            })
            .collect();
        let ids: Vec<String> = containers.keys().cloned().collect();
        for _ in 0..edits {
            let t = text_of(&containers[&ids[rng.below(ids.len())]], &txn);
            let len = t.len(&txn) as usize;
            if len < 4 || rng.below(10) < 7 {
                let at = rng.below(len + 1) as u32;
                t.insert(&mut txn, at, "xy");
            } else {
                let at = rng.below(len - 3) as u32;
                t.remove_range(&mut txn, at, 1 + rng.below(3) as u32);
            }
        }
        for &k in moves {
            let id = format!("b{k}");
            let Some(idx) = g.children(&txn).position(|c| matches!(&c, XmlOut::Element(e) if id_of(e, &txn) == id)) else { continue };
            let Some(XmlOut::Element(c)) = g.get(&txn, idx as u32) else { continue };
            let text = text_of(&c, &txn).get_string(&txn);
            g.remove_range(&mut txn, idx as u32, 1);
            let to = rng.below(g.len(&txn) as usize + 1) as u32;
            push_block(&g, &mut txn, to, &id, &text);
        }
    }

    /// Échange les mises à jour manquantes dans les deux sens.
    pub fn sync(a: &Doc, b: &Doc) {
        let ua = a.transact().encode_state_as_update_v1(&b.transact().state_vector());
        let ub = b.transact().encode_state_as_update_v1(&a.transact().state_vector());
        b.transact_mut().apply_update(Update::decode_v1(&ua).unwrap()).unwrap();
        a.transact_mut().apply_update(Update::decode_v1(&ub).unwrap()).unwrap();
    }

    /// (id, texte) de chaque bloc, dans l'ordre.
    pub fn block_texts(doc: &Doc) -> Vec<(String, String)> {
        let frag = doc.get_or_insert_xml_fragment(FRAGMENT);
        let txn = doc.transact();
        let g = group(&frag, &txn);
        g.children(&txn)
            .filter_map(|c| match c {
                XmlOut::Element(e) => Some((id_of(&e, &txn), text_of(&e, &txn).get_string(&txn))),
                _ => None,
            })
            .collect()
    }
}

pub mod l {
    use std::borrow::Cow;
    use std::collections::HashMap;

    use loro::{ExportMode, LoroDoc, LoroText, LoroValue, TreeID, ValueOrContainer};

    use super::{line, Rng};

    const TREE: &str = "blocks";

    pub fn new_doc(peer: u64) -> LoroDoc {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).unwrap();
        doc
    }

    fn id_of(doc: &LoroDoc, node: TreeID) -> String {
        match doc.get_tree(TREE).get_meta(node).unwrap().get("id") {
            Some(ValueOrContainer::Value(LoroValue::String(s))) => s.to_string(),
            _ => String::new(),
        }
    }

    fn text_of(doc: &LoroDoc, node: TreeID) -> LoroText {
        match doc.get_tree(TREE).get_meta(node).unwrap().get("text") {
            Some(ValueOrContainer::Container(loro::Container::Text(t))) => t,
            _ => panic!("bloc sans texte"),
        }
    }

    /// Page de `n` paragraphes : un nœud d'arbre par bloc, avec `id`, `type` et un `LoroText`.
    pub fn built(n: usize) -> LoroDoc {
        let doc = new_doc(1_000);
        let tree = doc.get_tree(TREE);
        tree.enable_fractional_index(0);
        for i in 0..n {
            let node = tree.create(None).unwrap();
            let meta = tree.get_meta(node).unwrap();
            meta.insert("id", format!("b{i}")).unwrap();
            meta.insert("type", "paragraph").unwrap();
            meta.insert_container("text", LoroText::new()).unwrap().insert(0, &line(i)).unwrap();
        }
        doc.commit();
        doc
    }

    pub fn snapshot(doc: &LoroDoc) -> Vec<u8> {
        doc.export(ExportMode::Snapshot).unwrap()
    }

    pub fn load(bytes: &[u8], peer: u64) -> LoroDoc {
        let doc = new_doc(peer);
        doc.import(bytes).unwrap();
        doc
    }

    pub fn edit_random(doc: &LoroDoc, rng: &mut Rng, edits: usize, moves: &[usize]) {
        let tree = doc.get_tree(TREE);
        let nodes: HashMap<String, TreeID> = tree.roots().into_iter().map(|n| (id_of(doc, n), n)).collect();
        let ids: Vec<String> = nodes.keys().cloned().collect();
        for _ in 0..edits {
            let t = text_of(doc, nodes[&ids[rng.below(ids.len())]]);
            let len = t.len_unicode();
            if len < 4 || rng.below(10) < 7 {
                t.insert(rng.below(len + 1), "xy").unwrap();
            } else {
                t.delete(rng.below(len - 3), 1 + rng.below(3)).unwrap();
            }
        }
        for &k in moves {
            if let Some(&node) = nodes.get(&format!("b{k}")) {
                let to = rng.below(tree.roots().len());
                tree.mov_to(node, None, to).unwrap();
            }
        }
        doc.commit();
    }

    pub fn sync(a: &LoroDoc, b: &LoroDoc) {
        let ua = a.export(ExportMode::Updates { from: Cow::Owned(b.oplog_vv()) }).unwrap();
        let ub = b.export(ExportMode::Updates { from: Cow::Owned(a.oplog_vv()) }).unwrap();
        b.import(&ua).unwrap();
        a.import(&ub).unwrap();
    }

    pub fn block_texts(doc: &LoroDoc) -> Vec<(String, String)> {
        doc.get_tree(TREE).roots().into_iter().map(|n| (id_of(doc, n), text_of(doc, n).to_string())).collect()
    }
}
