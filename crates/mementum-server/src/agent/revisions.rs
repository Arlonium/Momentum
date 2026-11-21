//! Les révisions servies aux agents, et les empreintes de leurs blocs (lot 5, Task S7, Ruling
//! R1226).
//!
//! Le serveur est sans état pour les agents, sauf ceci : pour chaque révision d'une page qu'il a
//! servie (une lecture, `GET …/document` ou `GET …/mmd`, ou une écriture, dont il rend la révision
//! écrite), il garde l'empreinte de chaque bloc (`BlockHashes` : identifiant, contenu, place, 24
//! octets par bloc). C'est ce qui permet `on_conflict: "rebase"` (une écriture sur une révision
//! que la page a dépassée s'applique si les blocs qu'elle touche n'ont pas changé, Ruling R1231)
//! et `since` (une lecture en tranches dit ce qui a changé depuis la première).
//!
//! **Borné en octets** ([`MAX_KEPT_BYTES`], 16 Mio) : au-delà, la révision la moins récemment lue
//! ou écrite part d'abord. Une révision qui n'y est plus, ou d'avant un redémarrage, se juge comme
//! inconnue : `409 revision_conflict` dès que la page a changé, `changed: null` pour `since`.
//! Les empreintes sont sous la clé du processus (`Hub::hash_key`) : rien ne survit à un
//! redémarrage, et rien n'a à y survivre.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use mementum_crdt::anchors::BlockHashes;
use uuid::Uuid;

/// Octets d'empreintes gardés au plus (Ruling R1226) : environ 70 révisions d'une page de
/// 10 000 blocs, ou des milliers de petites pages.
pub const MAX_KEPT_BYTES: usize = 16 << 20;

/// Les révisions servies, et les empreintes de leurs blocs (voir le module).
pub(crate) struct Revisions {
    max_bytes: usize,
    kept: Mutex<Kept>,
}

#[derive(Default)]
struct Kept {
    /// `(page, révision)` → les empreintes, et le rang de leur dernier usage.
    entries: HashMap<(Uuid, String), (Arc<BlockHashes>, u64)>,
    /// Le rang du dernier usage → la révision : la moins récemment servie d'abord.
    order: BTreeMap<u64, (Uuid, String)>,
    bytes: usize,
    next: u64,
}

impl Default for Revisions {
    fn default() -> Self {
        Revisions::new(MAX_KEPT_BYTES)
    }
}

impl Revisions {
    pub(crate) fn new(max_bytes: usize) -> Revisions {
        Revisions {
            max_bytes,
            kept: Mutex::default(),
        }
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// La révision `revision` de `page` vient d'être servie, avec les empreintes de ses blocs.
    pub(crate) fn served(&self, page: Uuid, revision: &str, hashes: Arc<BlockHashes>) {
        let size = hashes.bytes() + revision.len();
        if size > self.max_bytes {
            return;
        }
        let mut kept = self.kept();
        let tick = kept.next;
        kept.next += 1;
        let key = (page, revision.to_string());
        if let Some((_, used)) = kept.entries.get(&key).map(|(h, used)| (h.clone(), *used)) {
            kept.order.remove(&used);
            kept.order.insert(tick, key.clone());
            if let Some(entry) = kept.entries.get_mut(&key) {
                entry.1 = tick;
            }
            return;
        }
        kept.bytes += size;
        kept.order.insert(tick, key.clone());
        kept.entries.insert(key, (hashes, tick));
        while kept.bytes > self.max_bytes {
            let Some((_, oldest)) = kept.order.pop_first() else {
                break;
            };
            if let Some((gone, _)) = kept.entries.remove(&oldest) {
                kept.bytes -= gone.bytes() + oldest.1.len();
            }
        }
    }

    /// Les empreintes de la révision `revision` de `page`, si elles sont encore gardées ; leur
    /// usage les rend les plus récentes.
    pub(crate) fn hashes(&self, page: Uuid, revision: &str) -> Option<Arc<BlockHashes>> {
        let mut kept = self.kept();
        let tick = kept.next;
        let key = (page, revision.to_string());
        let (hashes, used) = kept
            .entries
            .get(&key)
            .map(|(hashes, used)| (hashes.clone(), *used))?;
        kept.next += 1;
        kept.order.remove(&used);
        kept.order.insert(tick, key.clone());
        if let Some(entry) = kept.entries.get_mut(&key) {
            entry.1 = tick;
        }
        Some(hashes)
    }

    /// Octets gardés (pour les tests).
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.kept().bytes
    }
}

#[cfg(test)]
mod tests {
    use mementum_crdt::anchors::{HashKey, hashes};
    use mementum_md::parse;

    use super::*;

    fn some_hashes(text: &str) -> Arc<BlockHashes> {
        let blocks = parse(text).unwrap();
        Arc::new(hashes(&blocks, &HashKey::new()))
    }

    /// Le cache garde au plus sa borne d'octets, et rend d'abord la révision la moins récemment
    /// servie ; une révision relue redevient la plus récente.
    #[test]
    fn the_least_recently_served_revision_goes_first() {
        let one = some_hashes("Un\n\nDeux\n\nTrois\n");
        let size = one.bytes() + "r_1".len();
        let revisions = Revisions::new(size * 2 + size / 2);
        let page = Uuid::now_v7();
        revisions.served(page, "r_1", one.clone());
        revisions.served(page, "r_2", one.clone());
        assert!(revisions.hashes(page, "r_1").is_some());
        revisions.served(page, "r_3", one.clone());
        assert!(revisions.hashes(page, "r_2").is_none(), "r_2 devait partir");
        assert!(revisions.hashes(page, "r_1").is_some());
        assert!(revisions.hashes(page, "r_3").is_some());
        assert!(revisions.bytes() <= size * 2 + size / 2);
    }
}
