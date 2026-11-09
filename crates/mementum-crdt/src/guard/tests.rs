//! Test différentiel de la garde contre yrs 0.28.1 :
//! - [`Shape::read`] voit les mêmes blocs que `yrs::Update::decode_v1`
//!   (clients, horloges, longueurs, trous) ;
//! - toute mise à jour que yrs intègre sans rien laisser en attente passe
//!   [`Shape::check`] et `PageDoc::apply_update`.

use std::collections::BTreeMap;

use mementum_md::{Block, Content, arb, block_text, parse};
use proptest::prelude::*;
use yrs::updates::decoder::Decode;
use yrs::{
    Doc, OffsetKind, Options, ReadTxn, Transact, Update, Xml, XmlElementPrelim, XmlFragment,
    XmlTextPrelim, merge_updates_v1,
};

use super::{Kind, Shape};
use crate::{FRAGMENT, PageDoc};

/// Ce qu'une mise à jour contient, vu par l'API publique de yrs : plages
/// insérées (éléments et GC, fusionnées), horloge du premier bloc qui n'est
/// pas un trou, et fin de la suite continue depuis l'horloge 0.
#[derive(Debug, PartialEq, Default)]
struct Seen {
    insertions: BTreeMap<u64, Vec<(u32, u32)>>,
    lower: BTreeMap<u64, u32>,
    upper: BTreeMap<u64, u32>,
}

fn merged(mut ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    ranges.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (start, end) in ranges {
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

fn seen_by_guard(update: &[u8]) -> Seen {
    let shape = Shape::read(update).expect("lu par la garde");
    let mut seen = Seen::default();
    for (client, structs) in &shape.clients {
        let ranges: Vec<(u32, u32)> = structs
            .iter()
            .filter(|s| !matches!(s.kind, Kind::Skip))
            .map(|s| (s.id.1, s.id.1 + s.len))
            .collect();
        if !ranges.is_empty() {
            seen.insertions.insert(*client, merged(ranges));
        }
        if let Some(first) = structs.iter().find(|s| !matches!(s.kind, Kind::Skip)) {
            seen.lower.insert(*client, first.id.1);
        }
        if structs.first().is_some_and(|s| s.id.1 == 0) {
            let end = structs
                .iter()
                .take_while(|s| !matches!(s.kind, Kind::Skip))
                .last()
                .map_or(0, |s| s.id.1 + s.len);
            if end != 0 {
                seen.upper.insert(*client, end);
            }
        }
    }
    seen
}

fn seen_by_yrs(update: &[u8]) -> Seen {
    let update = Update::decode_v1(update).expect("lu par yrs");
    let mut seen = Seen::default();
    for (client, ranges) in update.insertions(true).iter() {
        let ranges: Vec<(u32, u32)> = ranges.iter().map(|r| (r.start, r.end)).collect();
        seen.insertions.insert(client.get(), merged(ranges));
    }
    for (client, clock) in update.state_vector_lower().iter() {
        seen.lower.insert(client.get(), *clock);
    }
    for (client, clock) in update.state_vector().iter() {
        seen.upper.insert(client.get(), *clock);
    }
    seen
}

#[track_caller]
fn assert_same_blocks(update: &[u8]) {
    assert_eq!(seen_by_guard(update), seen_by_yrs(update));
}

/// Applique une mise à jour que yrs intègre sans attente : la garde l'accepte.
#[track_caller]
fn assert_accepted(page: &PageDoc, update: &[u8]) {
    assert_same_blocks(update);
    let state = page.yrs_doc().transact().state_vector();
    Shape::read(update)
        .and_then(|s| s.check(&state))
        .expect("acceptée par check");
    page.apply_update(update)
        .expect("acceptée par apply_update");
}

/// Mises à jour d'un vrai client BlockNote 0.57 : la fixture de la P0, et
/// celles de la Task 8 (tous les types de blocs, props hors défaut, cellules
/// fusionnées dont `colwidth` porte `undefined`). Chacune est l'état complet
/// d'un navigateur, racine écrite par le serveur comprise.
#[test]
fn blocknote_fixtures_read_like_yrs() {
    for update in [
        &include_bytes!("../../../../spec/mmd/fixtures/yjs/p0-all-blocks.bin")[..],
        &include_bytes!("../../../../spec/mmd/fixtures/yjs/all-blocks.bin")[..],
        &include_bytes!("../../../../spec/mmd/fixtures/yjs/non-default.bin")[..],
    ] {
        assert_same_blocks(update);
        let (page, _) = PageDoc::new_page();
        assert_accepted(&page, update);
    }
}

fn raw_doc() -> Doc {
    Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    })
}

/// Un trou interne (bloc `Skip`) que yrs comble sans attente : accepté, comme
/// par yrs ; le bloc qui manquait, arrivé ensuite, le remplit.
#[test]
fn internal_skip_is_accepted_like_yrs() {
    let doc = raw_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let paragraph =
        |txn: &mut yrs::TransactionMut, at: Option<u32>, root: &yrs::XmlElementRef, text: &str| {
            let container = match at {
                Some(i) => root.insert(txn, i, XmlElementPrelim::empty("blockContainer")),
                None => root.push_back(txn, XmlElementPrelim::empty("blockContainer")),
            };
            container.insert_attribute(txn, "id", text);
            let p = container.push_back(txn, XmlElementPrelim::empty("paragraph"));
            p.push_back(txn, XmlTextPrelim::new(text));
        };
    let (first, root) = {
        let mut txn = doc.transact_mut();
        let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        paragraph(&mut txn, None, &root, "a");
        (txn.encode_update_v1(), root)
    };
    let second = {
        let mut txn = doc.transact_mut();
        paragraph(&mut txn, None, &root, "b");
        txn.encode_update_v1()
    };
    // En tête : il ne dépend que du premier bloc, pas du deuxième.
    let third = {
        let mut txn = doc.transact_mut();
        paragraph(&mut txn, Some(0), &root, "c");
        txn.encode_update_v1()
    };
    // Après « b » : il dépend du deuxième.
    let fourth = {
        let mut txn = doc.transact_mut();
        paragraph(&mut txn, None, &root, "d");
        txn.encode_update_v1()
    };

    let gapped = merge_updates_v1([&first, &third]).unwrap();
    let shape = Shape::read(&gapped).unwrap();
    assert!(
        shape
            .clients
            .iter()
            .any(|(_, structs)| structs.iter().any(|s| matches!(s.kind, Kind::Skip))),
        "la fusion porte un trou"
    );
    let (page, _) = PageDoc::new_page();
    assert_accepted(&page, &gapped);
    assert_accepted(&page, &second);
    // La racine du client suit ou précède celle de la page selon les ids de
    // client : seul l'ordre de ses propres blocs compte.
    let mut texts: Vec<String> = page.read_blocks().iter().map(block_text).collect();
    texts.retain(|t| !t.is_empty());
    assert_eq!(texts, ["c", "a", "b"]);

    // Avec une dépendance absente, des blocs resteraient en attente : refus.
    let waiting = merge_updates_v1([&first, &fourth]).unwrap();
    assert_same_blocks(&waiting);
    let (page, _) = PageDoc::new_page();
    assert!(matches!(
        page.apply_update(&waiting),
        Err(crate::CrdtError::Decode(_))
    ));
}

/// Premier bloc de la page dont le texte n'est pas vide.
fn first_text(blocks: &[Block]) -> Option<(String, usize)> {
    blocks.iter().find_map(|b| match &b.content {
        Content::Inline(_) if !block_text(b).is_empty() => {
            Some((b.id.clone(), block_text(b).chars().count()))
        }
        _ => first_text(&b.children),
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// Mises à jour produites par la page (écriture, éditions, déplacement,
    /// état, diff entre deux pairs) : mêmes blocs que yrs, toutes acceptées.
    #[test]
    fn page_updates_read_like_yrs(t in arb::tree()) {
        let (page, created) = PageDoc::new_page();
        let peer = PageDoc::from_updates([created.as_slice()]).unwrap();
        let receiver = PageDoc::from_updates([created.as_slice()]).unwrap();
        let mut updates = vec![page.write_blocks(&t).expect("écriture")];
        let blocks = page.read_blocks();
        if let Some((id, n)) = first_text(&blocks) {
            updates.push(page.replace_text(&id, n / 2, n - n / 2, "é🚀").unwrap());
        }
        let first = blocks[0].id.clone();
        updates.push(page.insert_blocks_after(Some(&first), &parse("Nouveau\n").unwrap()).unwrap());
        if let Some(last) = blocks.last().filter(|b| b.id != first)
            && let Ok(moved) = page.move_block(&last.id, None)
        {
            updates.push(moved);
        }
        for update in &updates {
            assert_accepted(&receiver, update);
        }
        prop_assert_eq!(receiver.read_blocks(), page.read_blocks());

        let state = page.encode_state();
        let (fresh, _) = PageDoc::new_page();
        assert_accepted(&fresh, &state);

        let diff = page.encode_diff(&peer.state_vector()).unwrap();
        assert_accepted(&peer, &diff);
        prop_assert_eq!(peer.read_blocks(), page.read_blocks());
    }
}
