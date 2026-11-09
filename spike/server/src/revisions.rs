//! Révisions lues par les agents : pour chacune, alias `bN` → id de bloc et empreinte de chaque bloc.

use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use spike_mmd::{render, Block, RenderOpts};
use yrs::updates::encoder::Encode;
use yrs::ReadTxn;

/// Nombre de révisions gardées par page.
pub const KEEP: usize = 100;

#[derive(Debug, Default)]
pub struct Snapshot {
    pub aliases: HashMap<String, String>,
    pub hashes: HashMap<String, u64>,
}

#[derive(Default)]
pub struct RevisionLog {
    entries: Mutex<VecDeque<(String, Arc<Snapshot>)>>,
}

impl RevisionLog {
    pub fn record(&self, revision: String, snapshot: Snapshot) -> Arc<Snapshot> {
        let snap = Arc::new(snapshot);
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|(r, _)| *r != revision);
        entries.push_back((revision, snap.clone()));
        while entries.len() > KEEP {
            entries.pop_front();
        }
        snap
    }

    pub fn get(&self, revision: &str) -> Option<Arc<Snapshot>> {
        self.entries.lock().unwrap().iter().find(|(r, _)| r == revision).map(|(_, s)| s.clone())
    }
}

/// Révision = 16 premiers caractères hex du SHA-256 de l'instantané Yjs (vecteur d'état + suppressions).
pub fn revision_of<T: ReadTxn>(txn: &T) -> String {
    let digest = Sha256::digest(txn.snapshot().encode_v1());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// MMD d'un bloc seul, sans ses enfants.
pub fn block_markdown(b: &Block) -> String {
    let alone = Block { children: vec![], ..b.clone() };
    render(&[alone], RenderOpts { anchors: false }).markdown
}

/// Empreinte du contenu propre d'un bloc (type, props, texte, marques) ; les enfants n'y entrent pas.
pub fn block_hash(b: &Block) -> u64 {
    let mut h = DefaultHasher::new();
    block_markdown(b).hash(&mut h);
    h.finish()
}

pub fn flatten(blocks: &[Block], out: &mut Vec<Block>) {
    for b in blocks {
        out.push(b.clone());
        flatten(&b.children, out);
    }
}

pub fn snapshot_of(blocks: &[Block], anchors: &[(String, String)]) -> Snapshot {
    let mut all = Vec::new();
    flatten(blocks, &mut all);
    Snapshot {
        aliases: anchors.iter().cloned().collect(),
        hashes: all.iter().map(|b| (b.id.clone(), block_hash(b))).collect(),
    }
}
