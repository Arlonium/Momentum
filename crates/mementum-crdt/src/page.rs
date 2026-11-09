//! Le document de page : création, mises à jour Yjs, lecture et écriture de
//! l'arbre de blocs.

use mementum_md::Block;
use mementum_schema::SCHEMA_VERSION;
use yrs::any::Number;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, Map, MapRef, OffsetKind, Options, Out, ReadTxn, StateVector, Transact,
    TransactionMut, Update, XmlElementPrelim, XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut,
};

use crate::blocknote::{BLOCK_GROUP, Ids, empty_paragraph, read_page, write_nodes};
use crate::{CrdtError, FRAGMENT, META};

/// Clé de [`META`] qui porte la version du schéma de la page.
const SCHEMA_KEY: &str = "schemaVersion";

/// Document Yjs d'une page, au format de BlockNote.
///
/// Toutes les positions comptent en UTF-16, comme Yjs dans le navigateur
/// (`OffsetKind::Utf16`). Chaque écriture renvoie la mise à jour Yjs (v1)
/// qu'elle a produite, à diffuser aux autres clients.
pub struct PageDoc {
    pub(crate) doc: Doc,
    pub(crate) fragment: XmlFragmentRef,
    meta: MapRef,
}

// L'acteur de synchronisation garde le document d'une tâche tokio à l'autre.
const _: fn() = || {
    fn shareable<T: Send + Sync>() {}
    shareable::<PageDoc>();
};

impl PageDoc {
    fn empty() -> PageDoc {
        let doc = Doc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        // Avant toute transaction : avec la feature `sync`, l'appel attendrait
        // le verrou d'une transaction ouverte.
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let meta = doc.get_or_insert_map(META);
        PageDoc {
            doc,
            fragment,
            meta,
        }
    }

    /// Page neuve : la racine BlockNote (`blockGroup > blockContainer >
    /// paragraph`) et `schemaVersion`. Renvoie aussi la mise à jour qui
    /// contient tout cela : c'est le serveur qui écrit la racine, jamais deux
    /// clients chacun la leur.
    pub fn new_page() -> (PageDoc, Vec<u8>) {
        let page = PageDoc::empty();
        let update = {
            let mut txn = page.doc.transact_mut();
            page.write_all(&mut txn, &[]);
            page.meta.insert(
                &mut txn,
                SCHEMA_KEY,
                Any::Number(Number::Int(i64::from(SCHEMA_VERSION))),
            );
            txn.encode_update_v1()
        };
        (page, update)
    }

    /// Document rebâti à partir de ses mises à jour (v1), dans n'importe quel ordre.
    pub fn from_updates<'a>(
        updates: impl IntoIterator<Item = &'a [u8]>,
    ) -> Result<PageDoc, CrdtError> {
        let page = PageDoc::empty();
        for update in updates {
            page.apply_update(update)?;
        }
        Ok(page)
    }

    /// Intègre une mise à jour (v1) reçue d'un client ou du stockage.
    pub fn apply_update(&self, update: &[u8]) -> Result<(), CrdtError> {
        let update = Update::decode_v1(update).map_err(|e| CrdtError::Decode(e.to_string()))?;
        self.doc
            .transact_mut()
            .apply_update(update)
            .map_err(|e| CrdtError::Decode(e.to_string()))
    }

    /// Tout l'état du document, en une mise à jour (v1).
    pub fn encode_state(&self) -> Vec<u8> {
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    /// Vecteur d'état (v1).
    pub fn state_vector(&self) -> Vec<u8> {
        use yrs::updates::encoder::Encode;
        self.doc.transact().state_vector().encode_v1()
    }

    /// Ce qui manque à un client dont on connaît le vecteur d'état (v1).
    pub fn encode_diff(&self, remote_state_vector: &[u8]) -> Result<Vec<u8>, CrdtError> {
        let sv = StateVector::decode_v1(remote_state_vector)
            .map_err(|e| CrdtError::Decode(e.to_string()))?;
        Ok(self.doc.transact().encode_state_as_update_v1(&sv))
    }

    /// Version du schéma écrite dans la page, si elle est lisible.
    pub fn schema_version(&self) -> Option<u32> {
        let txn = self.doc.transact();
        match self.meta.get(&txn, SCHEMA_KEY) {
            Some(Out::Any(Any::Number(n))) => n.as_i64().and_then(|v| u32::try_from(v).ok()),
            _ => None,
        }
    }

    /// L'arbre de blocs de la page, normalisé, que `mementum_md::render`
    /// écrit toujours en MMD relisible (Ruling R10).
    pub fn read_blocks(&self) -> Vec<Block> {
        read_page(&self.doc.transact(), &self.fragment)
    }

    /// Remplace tout le contenu par `blocks`, en gardant la racine
    /// `blockGroup`. Un bloc sans id (ou dont l'id est déjà pris plus haut)
    /// reçoit un UUIDv7. Un arbre vide donne un paragraphe vide.
    pub fn write_blocks(&self, blocks: &[Block]) -> Vec<u8> {
        let mut txn = self.doc.transact_mut();
        self.write_all(&mut txn, blocks);
        txn.encode_update_v1()
    }

    fn write_all(&self, txn: &mut TransactionMut, blocks: &[Block]) {
        let root = match self.root_index(txn) {
            Some(i) => {
                let len = self.fragment.len(txn);
                if len > i + 1 {
                    self.fragment.remove_range(txn, i + 1, len - i - 1);
                }
                if i > 0 {
                    self.fragment.remove_range(txn, 0, i);
                }
                self.root(txn).expect("racine gardée")
            }
            None => {
                let len = self.fragment.len(txn);
                if len > 0 {
                    self.fragment.remove_range(txn, 0, len);
                }
                self.fragment
                    .insert(txn, 0, XmlElementPrelim::empty(BLOCK_GROUP))
            }
        };
        let len = root.len(txn);
        if len > 0 {
            root.remove_range(txn, 0, len);
        }
        let fallback = [empty_paragraph()];
        let blocks = if blocks.is_empty() { &fallback } else { blocks };
        write_nodes(txn, &root, 0, blocks, &mut Ids::default());
    }

    fn root_index<T: ReadTxn>(&self, txn: &T) -> Option<u32> {
        self.fragment
            .children(txn)
            .position(|c| matches!(c, XmlOut::Element(e) if e.tag().as_ref() == BLOCK_GROUP))
            .map(|i| i as u32)
    }

    /// La racine `blockGroup` (la première, s'il y en a plusieurs).
    pub(crate) fn root<T: ReadTxn>(&self, txn: &T) -> Option<XmlElementRef> {
        let index = self.root_index(txn)?;
        match self.fragment.get(txn, index) {
            Some(XmlOut::Element(e)) => Some(e),
            _ => None,
        }
    }

    /// Pour `mementum-sync` seulement (`Awareness`) ; jamais pour lire le contenu.
    pub fn yrs_doc(&self) -> &Doc {
        &self.doc
    }
}
