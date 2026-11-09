//! Révision canonique d'une page, pour les agents.
//!
//! Elle doit changer à toute modification, suppression pure comprise (une
//! suppression n'avance pas le vecteur d'état), et ne pas dépendre de
//! l'ordre d'arrivée des mises à jour. On hache donc un encodage canonique de
//! l'instantané Yjs : vecteur d'état trié par client, puis ensemble des
//! suppressions trié et fusionné. `Snapshot::encode` ne convient pas : son
//! ordre vient d'une `HashMap`.

use std::ops::Range;

use sha2::{Digest, Sha256};
use yrs::{ReadTxn, Transact};

use crate::PageDoc;

impl PageDoc {
    /// `r_` suivi de 16 caractères hexadécimaux.
    pub fn revision(&self) -> String {
        let snapshot = self.doc.transact().snapshot();
        let mut state: Vec<(u64, u32)> = snapshot
            .state_map
            .iter()
            .map(|(client, clock)| (client.get(), *clock))
            .collect();
        state.sort_unstable();
        let mut deleted: Vec<(u64, Vec<Range<u32>>)> = snapshot
            .delete_set
            .iter()
            .map(|(client, ranges)| (client.get(), merged(ranges.iter().cloned().collect())))
            .filter(|(_, ranges)| !ranges.is_empty())
            .collect();
        deleted.sort_unstable_by_key(|(client, _)| *client);

        let mut hash = Sha256::new();
        hash.update((state.len() as u64).to_be_bytes());
        for (client, clock) in state {
            hash.update(client.to_be_bytes());
            hash.update(clock.to_be_bytes());
        }
        hash.update((deleted.len() as u64).to_be_bytes());
        for (client, ranges) in deleted {
            hash.update(client.to_be_bytes());
            hash.update((ranges.len() as u64).to_be_bytes());
            for range in ranges {
                hash.update(range.start.to_be_bytes());
                hash.update(range.end.to_be_bytes());
            }
        }
        let digest = hash.finalize();
        let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
        format!("r_{hex}")
    }
}

/// Plages triées, celles qui se chevauchent ou se touchent fusionnées : le
/// découpage des blocs Yjs varie d'un client à l'autre.
fn merged(mut ranges: Vec<Range<u32>>) -> Vec<Range<u32>> {
    ranges.retain(|r| r.start < r.end);
    ranges.sort_unstable_by_key(|r| (r.start, r.end));
    let mut out: Vec<Range<u32>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match out.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => out.push(range),
        }
    }
    out
}
