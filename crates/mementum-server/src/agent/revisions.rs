//! Les révisions servies aux agents, les empreintes de leurs blocs, et la dernière lecture de
//! chaque page (lot 5, Task S7, Rulings R1226, R1386, R1403).
//!
//! Le serveur est sans état pour les agents, sauf ceci : pour chaque révision d'une page qu'il a
//! servie (une lecture, `GET …/document` ou `GET …/mmd`, ou une écriture, dont il rend la révision
//! écrite), il garde l'empreinte de chaque bloc (`BlockHashes` : identifiant, contenu, place, 24
//! octets par bloc). C'est ce qui permet `on_conflict: "rebase"` (une écriture sur une révision
//! que la page a dépassée s'applique si les blocs qu'elle touche n'ont pas changé, Ruling R1231)
//! et `since` (une lecture en tranches dit ce qui a changé depuis la première).
//!
//! **La dernière lecture d'une page** (ses blocs, leurs empreintes, l'index des ancres) est gardée
//! aussi, sous le compte des écritures de la page (`Hub::page_generation`) au moment où elle a été
//! faite : tant qu'il n'a pas bougé, une tranche suivante d'une même lecture se sert d'elle, sans
//! repasser par l'acteur de la page (tour de correction de S7, R1403 : une lecture complète de
//! 100 000 blocs coûtait plus de deux cents passages de 220 ms dans l'acteur).
//!
//! **Borné en octets** ([`MAX_KEPT_BYTES`], 16 Mio, empreintes et lectures ensemble ; le poids
//! d'une lecture est estimé) : au-delà, ce qui a été le moins récemment servi part d'abord. Une
//! révision qui n'y est plus, ou d'avant un redémarrage, se juge comme inconnue : `409
//! revision_conflict` dès que la page a changé, `changed: null` pour `since`. Les empreintes sont
//! sous la clé du processus (`Hub::hash_key`) : rien ne survit à un redémarrage, et rien n'a à y
//! survivre.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use mementum_crdt::anchors::{Anchors, BlockHashes};
use mementum_md::{Block, Content};
use mementum_sync::PageRead;
use uuid::Uuid;

/// Octets gardés au plus (Ruling R1226) : environ 70 révisions d'une page de 10 000 blocs, ou des
/// milliers de petites pages ; une lecture gardée en pèse davantage (ses blocs).
pub const MAX_KEPT_BYTES: usize = 16 << 20;

/// Ce qui est gardé, par clé.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    /// Les empreintes d'une révision servie d'une page.
    Hashes(Uuid, String),
    /// La dernière lecture d'une page.
    Read(Uuid),
}

/// Une lecture gardée : le compte des écritures de la page quand elle a été faite, la lecture et
/// l'index de ses ancres.
#[derive(Clone)]
struct KeptRead {
    generation: u64,
    read: Arc<PageRead>,
    anchors: Arc<Anchors>,
}

#[derive(Clone)]
enum Kept {
    Hashes(Arc<BlockHashes>),
    Read(KeptRead),
}

/// Les révisions servies, et les dernières lectures (voir le module).
pub(crate) struct Revisions {
    max_bytes: usize,
    kept: Mutex<Store>,
}

#[derive(Default)]
struct Store {
    /// La clé → ce qui est gardé, son poids, et le rang de son dernier usage.
    entries: HashMap<Key, (Kept, usize, u64)>,
    /// Le rang du dernier usage → la clé : le moins récemment servi d'abord.
    order: BTreeMap<u64, Key>,
    bytes: usize,
    next: u64,
}

impl Store {
    /// Range `kept` sous `key`, ou le rend le plus récent s'il y est déjà.
    fn put(&mut self, key: Key, kept: Kept, size: usize, max: usize) {
        let tick = self.next;
        self.next += 1;
        if let Some((_, old_size, used)) = self.entries.remove(&key) {
            self.order.remove(&used);
            self.bytes -= old_size;
        }
        self.bytes += size;
        self.order.insert(tick, key.clone());
        self.entries.insert(key, (kept, size, tick));
        while self.bytes > max {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            if let Some((_, size, _)) = self.entries.remove(&oldest) {
                self.bytes -= size;
            }
        }
    }

    /// Ce qui est gardé sous `key`, rendu le plus récent.
    fn get(&mut self, key: &Key) -> Option<Kept> {
        let tick = self.next;
        let (kept, _, used) = self.entries.get_mut(key)?;
        let found = kept.clone();
        let old = std::mem::replace(used, tick);
        self.next += 1;
        self.order.remove(&old);
        self.order.insert(tick, key.clone());
        Some(found)
    }
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

    fn kept(&self) -> std::sync::MutexGuard<'_, Store> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// La révision `revision` de `page` vient d'être servie, avec les empreintes de ses blocs.
    pub(crate) fn served(&self, page: Uuid, revision: &str, hashes: Arc<BlockHashes>) {
        let size = hashes.bytes() + revision.len();
        if size > self.max_bytes {
            return;
        }
        self.kept().put(
            Key::Hashes(page, revision.to_string()),
            Kept::Hashes(hashes),
            size,
            self.max_bytes,
        );
    }

    /// Les empreintes de la révision `revision` de `page`, si elles sont encore gardées ; leur
    /// usage les rend les plus récentes.
    pub(crate) fn hashes(&self, page: Uuid, revision: &str) -> Option<Arc<BlockHashes>> {
        match self.kept().get(&Key::Hashes(page, revision.to_string()))? {
            Kept::Hashes(hashes) => Some(hashes),
            Kept::Read(_) => None,
        }
    }

    /// La dernière lecture de `page`, si elle a été faite sous ce compte des écritures de la page
    /// (`Hub::page_generation`, lu **avant** de lire la page) : la page n'a pas été écrite depuis.
    pub(crate) fn kept_read(
        &self,
        page: Uuid,
        generation: u64,
    ) -> Option<(Arc<PageRead>, Arc<Anchors>)> {
        match self.kept().get(&Key::Read(page))? {
            Kept::Read(kept) if kept.generation == generation => Some((kept.read, kept.anchors)),
            _ => None,
        }
    }

    /// Garde la lecture `read` de `page`, faite sous le compte d'écritures `generation`, l'index
    /// de ses ancres, et son poids (`read_bytes`, calculé hors des fils du serveur).
    pub(crate) fn keep_read(
        &self,
        page: Uuid,
        generation: u64,
        read: Arc<PageRead>,
        anchors: Arc<Anchors>,
        size: usize,
    ) {
        if size > self.max_bytes {
            return;
        }
        self.kept().put(
            Key::Read(page),
            Kept::Read(KeptRead {
                generation,
                read,
                anchors,
            }),
            size,
            self.max_bytes,
        );
    }

    /// Octets gardés (pour les tests).
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.kept().bytes
    }
}

/// Le poids d'une lecture gardée, estimé : ses empreintes, l'index des ancres (le SHA-256 et
/// l'identifiant de chaque bloc), et ses blocs (identifiant, type, texte et une part fixe par bloc
/// et par cellule de tableau).
pub(crate) fn read_bytes(read: &PageRead) -> usize {
    fn blocks(list: &[Block]) -> usize {
        list.iter()
            .map(|b| {
                let content = match &b.content {
                    Content::Table(table) => table
                        .rows
                        .iter()
                        .flatten()
                        .map(|cell| 96 + format!("{:?}", cell.content).len())
                        .sum(),
                    _ => 2 * mementum_md::block_text(b).len(),
                };
                160 + 2 * b.id.len()
                    + b.kind.len()
                    + content
                    + 48 * b.props.len()
                    + blocks(&b.children)
            })
            .sum()
    }
    read.hashes.bytes() + read.revision.len() + blocks(&read.blocks)
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

    /// Une lecture gardée ne sert que sous le compte d'écritures où elle a été faite (R1403).
    #[test]
    fn a_kept_read_serves_only_while_the_page_is_not_written() {
        let blocks = parse("Un\n\nDeux\n").unwrap();
        let read = Arc::new(PageRead {
            hashes: Arc::new(hashes(&blocks, &HashKey::new())),
            revision: "r_1".to_string(),
            blocks: blocks.clone(),
        });
        let revisions = Revisions::default();
        let page = Uuid::now_v7();
        let size = read_bytes(&read);
        revisions.keep_read(page, 3, read, Arc::new(Anchors::of(&blocks)), size);
        assert!(revisions.kept_read(page, 3).is_some());
        assert!(revisions.kept_read(page, 4).is_none());
        assert!(revisions.kept_read(Uuid::now_v7(), 3).is_none());
    }
}
