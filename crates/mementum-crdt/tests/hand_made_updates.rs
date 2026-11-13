//! Mises à jour écrites à la main par un client hostile (constat C1 de la relecture de la piste
//! S, Ruling R207). Un client répété dans la partie « blocs » d'une mise à jour passait la garde,
//! et yrs écrivait par-dessus un bloc connu : la racine de la page devenait une plage ramassée,
//! la page se vidait chez tous, puis pour de bon au compactage ; avec une plage plus longue que
//! le bloc visé, yrs paniquait en debug et rendait en release un état qu'il ne relisait plus.
//! Aucun encodeur honnête n'écrit deux entrées pour un client (Yjs et yrs écrivent une entrée par
//! client, rangées par client décroissant, fusions comprises) : la garde les refuse.

use std::panic::{AssertUnwindSafe, catch_unwind};

use mementum_crdt::{CrdtError, PageDoc};
use mementum_md::{block_text, parse};
use proptest::prelude::*;
use yrs::StateVector;
use yrs::updates::decoder::Decode;

/// Écrit un entier variable de lib0.
fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Un bloc sans contenu : une plage ramassée (`gc`) ou un `Skip`, et sa longueur.
#[derive(Debug, Clone, Copy)]
enum Filler {
    Gc(u32),
    Skip(u32),
}

/// Une mise à jour écrite à la main : des entrées `(client, horloge, blocs)`, écrites dans
/// l'ordre et telles quelles (un client peut s'y répéter), puis des suppressions `(client,
/// horloge, longueur)`.
fn hand_made(entries: &[(u64, u32, Vec<Filler>)], deletions: &[(u64, u32, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    var(&mut out, entries.len() as u64);
    for (client, clock, blocks) in entries {
        var(&mut out, blocks.len() as u64);
        var(&mut out, *client);
        var(&mut out, u64::from(*clock));
        for block in blocks {
            let (info, len) = match block {
                Filler::Gc(len) => (0, len),
                Filler::Skip(len) => (10, len),
            };
            out.push(info);
            var(&mut out, u64::from(*len));
        }
    }
    var(&mut out, deletions.len() as u64);
    for (client, clock, len) in deletions {
        var(&mut out, *client);
        var(&mut out, 1);
        var(&mut out, u64::from(*clock));
        var(&mut out, u64::from(*len));
    }
    out
}

/// L'état d'une page qui porte deux paragraphes, écrite par un client, puis éditée par un autre.
fn base_state() -> Vec<u8> {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse("Un\n\nDeux\n").unwrap()).unwrap();
    let editor = PageDoc::from_updates([page.encode_state().as_slice()]).unwrap();
    let id = editor.read_blocks()[0].id.clone();
    let edit = editor.replace_text(&id, 2, 0, " et demi").unwrap();
    page.apply_client_update(&edit).unwrap();
    page.encode_state()
}

/// Les clients de la page et leur horloge, du plus haut au plus bas.
fn clients(state: &[u8]) -> Vec<(u64, u32)> {
    let page = PageDoc::from_updates([state]).unwrap();
    let sv = StateVector::decode_v1(&page.state_vector()).unwrap();
    let mut clients: Vec<(u64, u32)> = sv.iter().map(|(c, k)| (c.get(), *k)).collect();
    clients.sort_unstable_by_key(|&(client, _)| std::cmp::Reverse(client));
    clients
}

/// Le client qui a écrit la racine de la page (son bloc à l'horloge 0 est le `blockGroup`
/// racine), et son horloge : dans [`base_state`], c'est celui de la page, le plus avancé (il a
/// écrit la racine puis les deux paragraphes ; l'éditeur, « et demi » seulement).
fn root_writer(state: &[u8]) -> (u64, u32) {
    clients(state)
        .into_iter()
        .max_by_key(|&(_, clock)| clock)
        .unwrap()
}

/// La sonde de la relecture, avec l'attente inverse : deux entrées pour le client qui a écrit la
/// racine, un `Skip` d'une unité à son horloge courante, puis une plage ramassée `[0, 1)`, que la
/// page connaît déjà. La garde la refuse (`Decode`), d'un client comme du stockage, et la page
/// reste entière ; de même avec une plage longue (`[0, 1 000 000)`), qui faisait paniquer yrs en
/// debug et rendait en release un état illisible.
#[test]
fn repeated_client_is_refused_and_the_page_stays_whole() {
    let state = base_state();
    let (client, clock) = root_writer(&state);
    for gc in [1, 1_000_000] {
        let forged = hand_made(
            &[
                (client, clock, vec![Filler::Skip(1)]),
                (client, 0, vec![Filler::Gc(gc)]),
            ],
            &[],
        );
        let page = PageDoc::from_updates([state.as_slice()]).unwrap();
        let before = (page.encode_state(), page.read_blocks());
        let refused = |r: Result<(), CrdtError>| matches!(r, Err(CrdtError::Decode(_)));
        assert!(
            matches!(page.awaits(&forged), Err(CrdtError::Decode(_))),
            "plage {gc} : awaits"
        );
        assert!(
            refused(page.apply_client_update(&forged).map(|_| ())),
            "plage {gc} : apply_client_update"
        );
        assert!(
            refused(page.apply_update(&forged)),
            "plage {gc} : apply_update"
        );
        assert_eq!(
            (page.encode_state(), page.read_blocks()),
            before,
            "plage {gc}"
        );
        let reread = PageDoc::from_updates([page.encode_state().as_slice()]).unwrap();
        let texts: Vec<String> = reread.read_blocks().iter().map(block_text).collect();
        assert_eq!(texts, ["Un et demi", "Deux"], "plage {gc}");
    }
}

/// Ruling R208 (m4) : une plage de suppression de longueur nulle ne supprime rien, mais yrs la
/// garde en attente. Aucun encodeur honnête n'en écrit : venue d'un client, elle est refusée ;
/// relue du stockage, elle passe (une base d'avant ne devient pas illisible pour elle).
#[test]
fn empty_deletion_range_is_refused_from_a_client_only() {
    let state = base_state();
    let (client, clock) = clients(&state)[0];
    let empty = hand_made(&[], &[(client, clock + 2, 0)]);
    let page = PageDoc::from_updates([state.as_slice()]).unwrap();
    assert!(matches!(page.awaits(&empty), Err(CrdtError::Decode(_))));
    assert!(matches!(
        page.apply_client_update(&empty),
        Err(CrdtError::Decode(_))
    ));
    assert!(!page.has_pending_deletions());
    let stored = PageDoc::from_updates([state.as_slice()]).unwrap();
    assert_eq!(stored.apply_update(&empty), Ok(()));
}

/// Un état gardé en base ou écrit par un navigateur porte une entrée par client : il se relit
/// toujours (les fixtures écrites par Chromium, l'état complet et une fusion de yrs).
#[test]
fn honest_encodings_carry_one_entry_per_client() {
    let state = base_state();
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse("Autre\n").unwrap()).unwrap();
    let merged = yrs::merge_updates_v1([state.as_slice(), page.encode_state().as_slice()]).unwrap();
    for (what, bytes) in [
        ("état complet", state.as_slice()),
        ("fusion de yrs", merged.as_slice()),
        (
            "fixture all-blocks",
            include_bytes!("../../../spec/mmd/fixtures/yjs/all-blocks.bin").as_slice(),
        ),
        (
            "fixture p0-all-blocks",
            include_bytes!("../../../spec/mmd/fixtures/yjs/p0-all-blocks.bin").as_slice(),
        ),
        (
            "fixture non-default",
            include_bytes!("../../../spec/mmd/fixtures/yjs/non-default.bin").as_slice(),
        ),
    ] {
        PageDoc::from_updates([bytes]).unwrap_or_else(|e| panic!("{what} : {e}"));
    }
}

/// Un client de la page, ou un client qu'elle ne connaît pas.
fn pick(clients: &[(u64, u32)], index: usize) -> (u64, u32) {
    clients.get(index).copied().unwrap_or((4242, 0))
}

fn at(base: u32, offset: i32) -> u32 {
    base.saturating_add_signed(offset)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 4096, ..ProptestConfig::default() })]

    /// La sonde différentielle de la relecture (m5) : pour toute mise à jour faite à la main de
    /// plages ramassées, de `Skip` et de suppressions, autour des horloges de la page, que la
    /// garde ne refuse pas et que `awaits` ne retient pas, `apply_client_update` ne panique pas,
    /// réussit, et ne laisse aucune suppression en attente : la règle de R198 dit ce que fait
    /// yrs. Les clients peuvent se répéter : la garde les refuse alors (R207).
    #[test]
    fn what_the_guard_lets_through_leaves_nothing_pending(
        entries in prop::collection::vec(
            (0usize..3, -3i32..4, prop::collection::vec((any::<bool>(), 1u32..5), 1..4)),
            0..4,
        ),
        deletions in prop::collection::vec((0usize..3, -6i32..5, 0u32..6), 0..4),
    ) {
        let state = base_state_cached();
        let known = clients(&state);
        let entries: Vec<(u64, u32, Vec<Filler>)> = entries
            .into_iter()
            .map(|(who, offset, blocks)| {
                let (client, clock) = pick(&known, who);
                let blocks = blocks
                    .into_iter()
                    .map(|(gc, len)| if gc { Filler::Gc(len) } else { Filler::Skip(len) })
                    .collect();
                (client, at(clock, offset), blocks)
            })
            .collect();
        let deletions: Vec<(u64, u32, u32)> = deletions
            .into_iter()
            .map(|(who, offset, len)| {
                let (client, clock) = pick(&known, who);
                (client, at(clock, offset), len)
            })
            .collect();
        let update = hand_made(&entries, &deletions);
        let page = PageDoc::from_updates([state.as_slice()]).unwrap();
        if let Ok(None) = page.awaits(&update) {
            let applied = catch_unwind(AssertUnwindSafe(|| page.apply_client_update(&update)));
            prop_assert!(applied.is_ok(), "yrs a paniqué : {entries:?} {deletions:?}");
            let applied = applied.unwrap();
            prop_assert!(applied.is_ok(), "{applied:?} : {entries:?} {deletions:?}");
            prop_assert!(
                !page.has_pending_deletions(),
                "suppression laissée en attente : {entries:?} {deletions:?}"
            );
        }
    }
}

/// [`base_state`], bâti une fois pour toutes les entrées du test de propriétés.
fn base_state_cached() -> Vec<u8> {
    static STATE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    STATE.get_or_init(base_state).clone()
}
