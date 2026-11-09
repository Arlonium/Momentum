//! Le document de page : création, mises à jour Yjs, lecture et écriture de
//! l'arbre de blocs.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use mementum_md::{Block, normalize};
use mementum_schema::SCHEMA_VERSION;
use yrs::any::Number;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, Map, MapRef, OffsetKind, Options, Out, ReadTxn, StateVector, Transact,
    TransactionMut, Update, XmlElementPrelim, XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut,
};

use crate::blocknote::{BLOCK_GROUP, Ids, empty_paragraph, read_page, write_nodes};
use crate::guard::{Shape, missing};
use crate::structure::{Slot, check_blocks};
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
    pub(crate) meta: MapRef,
    /// Une mise à jour appliquée depuis le dernier dédoublonnage a inséré des
    /// éléments : seule une telle mise à jour peut dupliquer un bloc (Ruling R25).
    pub(crate) inserted_elements: AtomicBool,
    /// Parcours de la page faits par le dédoublonnage (pour les tests).
    pub(crate) dedupe_scans: AtomicU64,
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
            inserted_elements: AtomicBool::new(false),
            dedupe_scans: AtomicU64::new(0),
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
            page.write_all(&mut txn, &[empty_paragraph()]);
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
    ///
    /// La mise à jour est lue avant yrs (Ruling R17, voir [`crate::guard`]).
    /// Elle est refusée sans être appliquée si elle est illisible (`Decode`),
    /// si elle sort du schéma de la page (`Structure`), ou si ses éléments
    /// imbriqués dépassent à eux seuls
    /// [`MAX_YJS_DEPTH`](crate::MAX_YJS_DEPTH) (`TooDeep`). Après
    /// l'application, elle est refusée si elle a laissé des blocs en attente
    /// d'une dépendance absente (`Decode`), ou si un élément qu'elle insère
    /// dépasse la borne (`TooDeep`). Comme yrs, elle est acceptée quand elle
    /// arrive avant des mises à jour du même client dont elle ne dépend pas.
    ///
    /// Après une erreur, la mise à jour n'est ni gardée ni diffusée, et le
    /// `PageDoc` est à jeter : l'appelant le recharge depuis ses mises à jour
    /// stockées (la plupart des refus laissent le document intact, mais pas
    /// tous ; l'appelant n'a pas à les distinguer).
    pub fn apply_update(&self, update: &[u8]) -> Result<(), CrdtError> {
        let shape = Shape::read(update)?;
        let decoded = Update::decode_v1(update).map_err(|e| CrdtError::Decode(e.to_string()))?;
        let mut txn = self.doc.transact_mut();
        shape.check(&txn.state_vector())?;
        txn.apply_update(decoded)
            .map_err(|e| CrdtError::Decode(e.to_string()))?;
        if txn.store().pending_update().is_some() {
            return Err(missing());
        }
        if shape.deeper_than_bound(&txn) {
            return Err(CrdtError::TooDeep);
        }
        if shape.inserts_elements() {
            self.inserted_elements.store(true, Ordering::SeqCst);
        }
        Ok(())
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

    /// Remplace tout le contenu par `blocks` (normalisés au passage), en
    /// gardant la racine `blockGroup`. Un bloc sans id (ou dont l'id est déjà
    /// pris plus haut) reçoit un UUIDv7.
    ///
    /// BlockNote n'a pas de page sans bloc : un arbre vide s'écrit en un
    /// paragraphe vide, qui se relit `<p/>` (Ruling R16).
    ///
    /// Refusé, sans rien écrire : une structure de colonnes que BlockNote
    /// refuserait (`Structure`) ou un arbre plus profond que
    /// [`MAX_YJS_DEPTH`](crate::MAX_YJS_DEPTH) (`TooDeep`).
    pub fn write_blocks(&self, blocks: &[Block]) -> Result<Vec<u8>, CrdtError> {
        let blocks = match normalize(blocks.to_vec()) {
            b if b.is_empty() => vec![empty_paragraph()],
            b => b,
        };
        check_blocks(&blocks, Slot::Group, 2)?;
        let mut txn = self.doc.transact_mut();
        self.write_all(&mut txn, &blocks);
        Ok(txn.encode_update_v1())
    }

    /// Écrit des blocs normalisés et vérifiés à la place de tout le contenu.
    fn write_all(&self, txn: &mut TransactionMut, blocks: &[Block]) {
        match self.root_index(txn) {
            Some(i) => {
                let len = self.fragment.len(txn);
                if len > i + 1 {
                    self.fragment.remove_range(txn, i + 1, len - i - 1);
                }
                if i > 0 {
                    self.fragment.remove_range(txn, 0, i);
                }
            }
            None => {
                let len = self.fragment.len(txn);
                if len > 0 {
                    self.fragment.remove_range(txn, 0, len);
                }
            }
        }
        let root = match self.fragment.get(txn, 0) {
            Some(XmlOut::Element(root)) => root,
            _ => self
                .fragment
                .insert(txn, 0, XmlElementPrelim::empty(BLOCK_GROUP)),
        };
        let len = root.len(txn);
        if len > 0 {
            root.remove_range(txn, 0, len);
        }
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

    /// Pour `mementum-sync` seulement, qui y pose son observateur des
    /// suppressions (Ruling R40) ; jamais pour lire le contenu.
    pub fn yrs_doc(&self) -> &Doc {
        &self.doc
    }
}
