//! Test différentiel de la garde contre yrs 0.28.1 :
//! - [`Shape::read`] voit les mêmes blocs que `yrs::Update::decode_v1`
//!   (clients, horloges, longueurs, trous) ;
//! - toute mise à jour que yrs intègre sans rien laisser en attente passe
//!   [`Shape::check`] et `PageDoc::apply_update`.
//!
//! Et les métadonnées de la page (report M7 du lot 1) : un client n'y touche
//! pas, le stockage se recharge comme avant.

use std::collections::BTreeMap;

use mementum_md::{Block, Content, arb, block_text, parse};
use mementum_schema::SCHEMA_VERSION;
use proptest::prelude::*;
use yrs::updates::decoder::Decode;
use yrs::{
    Doc, Map, MapRef, OffsetKind, Options, ReadTxn, Transact, TransactionMut, Update, Xml,
    XmlElementPrelim, XmlFragment, XmlTextPrelim, merge_updates_v1,
};

use super::{Insert, Kind, Parent, Pending, Shape, Struct};
use crate::{CrdtError, FRAGMENT, META, PageDoc};

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

/// Mise à jour vide : l'état d'un client qui n'a encore rien reçu.
const NOTHING: &[u8] = &[0, 0];

/// Clé de la `Y.Map` des métadonnées qui porte la version du schéma.
const SCHEMA_KEY: &str = "schemaVersion";

/// La mise à jour que produit `edit` sur les métadonnées, chez un client
/// (un `Y.Doc` brut, comme un navigateur) qui a reçu `state`.
fn meta_edit(state: &[u8], edit: impl FnOnce(&mut TransactionMut, &MapRef)) -> Vec<u8> {
    let doc = raw_doc();
    // Avant toute transaction (interblocage de yrs avec la feature `sync`).
    let meta = doc.get_or_insert_map(META);
    doc.transact_mut()
        .apply_update(Update::decode_v1(state).unwrap())
        .unwrap();
    let mut txn = doc.transact_mut();
    edit(&mut txn, &meta);
    txn.encode_update_v1()
}

/// Report M7 du lot 1 : la mise à jour d'un client qui touche à la `Y.Map`
/// des métadonnées est refusée, sous toutes ses formes. Une entrée qui en
/// écrase une autre ne nomme pas son parent (elle le tient de l'entrée
/// écrasée), et une suppression ne nomme que des identifiants : aucune des
/// deux ne se reconnaît à la seule lecture de la mise à jour.
#[test]
fn client_cannot_overwrite_schema_version() {
    type Edit = fn(&mut TransactionMut, &MapRef);
    let (_, created) = PageDoc::new_page();
    let cases: [(&str, &[u8], Edit); 5] = [
        ("version écrasée", &created, |txn, meta| {
            meta.insert(txn, SCHEMA_KEY, 999);
        }),
        ("version supprimée", &created, |txn, meta| {
            meta.remove(txn, SCHEMA_KEY);
        }),
        ("autre clé", &created, |txn, meta| {
            meta.insert(txn, "auteur", "x");
        }),
        // Écriture concurrente de celle du serveur : laquelle gagne dépend
        // des identifiants de client.
        (
            "version écrite sans avoir reçu la page",
            NOTHING,
            |txn, meta| {
                meta.insert(txn, SCHEMA_KEY, 999);
            },
        ),
        // Un type que le schéma de la page admet ailleurs.
        (
            "élément posé à la place de la version",
            &created,
            |txn, meta| {
                meta.insert(txn, SCHEMA_KEY, XmlElementPrelim::empty("paragraph"));
            },
        ),
    ];
    for (what, state, edit) in cases {
        let update = meta_edit(state, edit);
        let page = PageDoc::from_updates([created.as_slice()]).unwrap();
        let refused = page.apply_client_update(&update);
        assert!(
            matches!(refused, Err(CrdtError::Structure(_))),
            "{what} : {refused:?}"
        );
    }

    // Le contenu, lui, passe. L'état complet d'un client aussi, tel qu'il le
    // renvoie à la reconnexion : il porte l'entrée du serveur, que la page
    // connaît déjà.
    let page = PageDoc::from_updates([created.as_slice()]).unwrap();
    let client = PageDoc::from_updates([created.as_slice()]).unwrap();
    let typed = client.write_blocks(&parse("Bonjour\n").unwrap()).unwrap();
    page.apply_client_update(&typed).expect("le contenu passe");
    page.apply_client_update(&client.encode_state())
        .expect("l'état complet d'un client passe");
    assert_eq!(page.read_blocks(), client.read_blocks());
    assert_eq!(page.schema_version(), Some(SCHEMA_VERSION));
}

/// Le refus ne vaut que pour ce qui arrive d'un client. Ce qui est en base
/// (dont la mise à jour du serveur qui pose `schemaVersion`) repasse par la
/// garde à chaque chargement : s'il y était refusé, toute page existante
/// deviendrait illisible.
#[test]
fn stored_pages_still_load_after_the_guard_tightens() {
    let (new_page, created) = PageDoc::new_page();
    let snapshot = new_page.encode_state();
    let stored: [(&str, &[u8]); 5] = [
        (
            "fixture p0-all-blocks",
            include_bytes!("../../../../spec/mmd/fixtures/yjs/p0-all-blocks.bin"),
        ),
        (
            "fixture all-blocks",
            include_bytes!("../../../../spec/mmd/fixtures/yjs/all-blocks.bin"),
        ),
        (
            "fixture non-default",
            include_bytes!("../../../../spec/mmd/fixtures/yjs/non-default.bin"),
        ),
        ("page neuve, mise à jour du serveur", &created),
        ("page neuve, instantané", &snapshot),
    ];
    let mut with_version = 0;
    for (what, state) in stored {
        // En instantané (`from_updates`), puis au journal d'une page déjà là.
        let loaded = PageDoc::from_updates([state]).unwrap_or_else(|e| panic!("{what} : {e}"));
        assert!(!loaded.read_blocks().is_empty(), "{what}");
        let (page, _) = PageDoc::new_page();
        assert_eq!(page.apply_update(state), Ok(()), "{what}");

        // Le piège : la même mise à jour, venue d'un client, est refusée dès
        // qu'elle pose la version.
        if loaded.schema_version().is_some() {
            with_version += 1;
            let (fresh, _) = PageDoc::new_page();
            assert!(
                matches!(
                    fresh.apply_client_update(state),
                    Err(CrdtError::Structure(_))
                ),
                "{what}"
            );
        }
    }
    assert!(
        with_version >= 2,
        "la page neuve porte sa version, en mise à jour comme en instantané"
    );
    assert_eq!(new_page.schema_version(), Some(SCHEMA_VERSION));

    // Une version récrite par le serveur (une migration du schéma, plus
    // tard) se recharge du journal ; et l'état complet d'un client qui l'a
    // reçue, où l'ancienne entrée figure parmi les suppressions, passe.
    let migrated = meta_edit(&created, |txn, meta| {
        meta.insert(txn, SCHEMA_KEY, i64::from(SCHEMA_VERSION) + 1);
    });
    let page = PageDoc::from_updates([created.as_slice(), migrated.as_slice()]).unwrap();
    assert_eq!(page.schema_version(), Some(SCHEMA_VERSION + 1));
    let client = PageDoc::from_updates([page.encode_state().as_slice()]).unwrap();
    client.write_blocks(&parse("Bonjour\n").unwrap()).unwrap();
    page.apply_client_update(&client.encode_state())
        .expect("l'état complet d'un client passe");
    assert_eq!(page.read_blocks(), client.read_blocks());
    assert_eq!(page.schema_version(), Some(SCHEMA_VERSION + 1));
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

/// Une entrée de mise à jour : le client, l'horloge du premier bloc, puis chaque bloc (0 :
/// élément, 1 : plage ramassée, 2 : `Skip`) et sa longueur.
type Entry = (u64, u32, Vec<(u8, u32)>);

/// Une plage supprimée : le client, l'horloge et la longueur.
type Deleted = (u64, u32, u32);

/// Une mise à jour lue, faite à la main, les horloges se suivant comme dans [`Shape::read`],
/// avec ses plages supprimées (un client peut s'y répéter).
fn shape_of(entries: &[Entry], deleted: &[Deleted]) -> Shape {
    let deletions = deleted
        .iter()
        .map(|&(client, clock, len)| (client, vec![(clock, clock + len)]))
        .collect();
    let clients = entries
        .iter()
        .map(|(client, start, blocks)| {
            let mut clock = *start;
            let structs = blocks
                .iter()
                .map(|&(kind, len)| {
                    let kind = match kind {
                        0 => Kind::Insert(Insert {
                            parent: Parent::Named(FRAGMENT.to_string(), false),
                            type_ref: None,
                        }),
                        1 => Kind::Gc,
                        _ => Kind::Skip,
                    };
                    let s = Struct {
                        id: (*client, clock),
                        len,
                        kind,
                    };
                    clock += len;
                    s
                })
                .collect();
            (*client, structs)
        })
        .collect();
    Shape {
        clients,
        deletions,
        empty_deletion: false,
    }
}

/// Les plages qu'une page garde en attente, rangées comme yrs les range (triées, fusionnées).
fn pending_of(ranges: &[Deleted]) -> Pending {
    let mut set = yrs::IdSet::new();
    for &(client, clock, len) in ranges {
        set.insert(yrs::ID::new(yrs::block::ClientID::new(client), clock), len);
    }
    Pending::of(Some(&set))
}

/// Un vecteur d'état pour les clients 1, 2 et 3.
fn state_of(clocks: &[u32]) -> yrs::StateVector {
    let mut state = yrs::StateVector::default();
    for (client, clock) in (1..).zip(clocks) {
        state.set_max(yrs::block::ClientID::new(client), *clock);
    }
    state
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// Rulings R131, R198 et R199 : ce qu'une mise à jour attend d'une page
    /// ([`Shape::awaited`]), comparé à un vecteur d'état atteint plus tard, dit la même chose que
    /// [`Shape::leaves_gap`] relu sur ce vecteur : l'appelant peut réessayer une mise à jour
    /// retenue sans la relire. Les entrées d'un même client peuvent se répéter, comme dans une
    /// mise à jour écrite à la main ; ses plages supprimées tombent avant, dans, ou au-delà de ses
    /// blocs, et en partie dans ce que la page garde déjà en attente.
    #[test]
    fn awaited_agrees_with_leaves_gap(
        entries in prop::collection::vec(
            (1u64..4, 0u32..20, prop::collection::vec((0u8..3, 1u32..5), 0..6)),
            0..4,
        ),
        deleted in prop::collection::vec((1u64..4, 0u32..40, 1u32..8), 0..4),
        pending in prop::collection::vec((1u64..4, 0u32..40, 1u32..8), 0..3),
        now in prop::collection::vec(0u32..40, 3),
        later in prop::collection::vec(0u32..20, 3),
    ) {
        let shape = shape_of(&entries, &deleted);
        let pending = pending_of(&pending);
        let state = state_of(&now);
        let awaited = shape.awaited(&state, &pending);
        prop_assert_eq!(awaited.is_empty(), !shape.leaves_gap(&state, &pending));
        let grown: Vec<u32> = now.iter().zip(&later).map(|(n, l)| n + l).collect();
        let grown = state_of(&grown);
        let reached = awaited
            .iter()
            .all(|(client, clock)| grown.get(&yrs::block::ClientID::new(*client)) >= *clock);
        prop_assert_eq!(reached, !shape.leaves_gap(&grown, &pending));
    }
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

/// Ruling R207, le filet de R198 : une plage non vide en attente après l'écriture, que celles
/// d'avant ne couvrent pas, est une suppression laissée en attente que la page n'avait pas ; une
/// plage qui a rétréci ne l'est pas, ni une plage vide.
#[test]
fn pending_lacks_only_what_is_new() {
    let set = |ranges: &[Deleted]| {
        let mut set = yrs::IdSet::new();
        for &(client, clock, len) in ranges {
            set.insert(yrs::ID::new(yrs::block::ClientID::new(client), clock), len);
        }
        set
    };
    let none = Pending::of(None);
    assert!(!none.lacks(None));
    assert!(none.lacks(Some(&set(&[(1, 5, 3)]))));
    assert!(!none.lacks(Some(&set(&[(1, 5, 0)]))), "une plage vide");
    let before = pending_of(&[(1, 0, 10)]);
    assert!(!before.lacks(Some(&set(&[(1, 5, 3)]))), "rétrécie");
    assert!(!before.lacks(Some(&set(&[(1, 0, 10)]))), "la même");
    assert!(before.lacks(Some(&set(&[(1, 8, 4)]))), "plus loin");
    assert!(before.lacks(Some(&set(&[(2, 0, 1)]))), "un autre client");
}
