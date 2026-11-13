//! Le document de page : création, mises à jour Yjs, lecture et écriture de
//! l'arbre de blocs.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use mementum_md::{Block, normalize};
use mementum_schema::SCHEMA_VERSION;
use yrs::any::Number;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, ClientID, Doc, Map, MapRef, OffsetKind, Options, Out, ReadTxn, StateVector, Transact,
    TransactionMut, Update, XmlElementPrelim, XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut,
};

use crate::blocknote::{BLOCK_GROUP, Ids, empty_paragraph, page_has_block, read_page, write_nodes};
use crate::guard::{Pending, REPEATED_CLIENT, Shape, missing, touched};
use crate::kept_ids::keep_ids;
use crate::structure::{Slot, check_blocks};
use crate::{CrdtError, FRAGMENT, META};

/// Clé de [`META`] qui porte la version du schéma de la page.
const SCHEMA_KEY: &str = "schemaVersion";

/// Ce qu'une mise à jour qui laisserait un trou attend de la page (Ruling
/// R131), ou qui supprimerait des horloges qu'elle n'a pas (Ruling R198) :
/// pour chaque client en cause, l'horloge que la page doit avoir atteinte. Rendu par [`PageDoc::awaits`], comparé à la page par
/// [`PageDoc::has_reached`] : qui retient une mise à jour sait quand la
/// réessayer sans la relire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Awaited(Vec<(u64, u32)>);

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

    /// Intègre une mise à jour (v1) relue du stockage, ou écrite par le
    /// serveur. Ce qui arrive d'un client passe par
    /// [`PageDoc::apply_client_update`], plus strict.
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
        let mut txn = self.doc.transact_mut();
        let (shape, decoded) = self.read(&txn, update, false)?;
        self.integrate(&mut txn, &shape, decoded)
    }

    /// Intègre une mise à jour (v1) reçue d'un client : comme
    /// [`PageDoc::apply_update`], avec les mêmes refus et la même suite à leur
    /// donner, et refusée en plus (`Structure`) si elle touche à la `Y.Map`
    /// des métadonnées [`META`], qui porte `schemaVersion` : une entrée
    /// écrite, écrasée ou supprimée (report M7 du lot 1). Seul le serveur y
    /// écrit. Refusée aussi (`Decode`) pour une plage de suppressions de
    /// longueur nulle (Ruling R208), et (`Structure`) si elle laisse une
    /// suppression en attente que la page n'avait pas (Ruling R207, filet de
    /// R198 : `PageDoc::awaits` retient d'abord ce qu'un client honnête envoie).
    ///
    /// Ce refus n'est connu qu'après l'application (voir le module `guard`) :
    /// comme après toute erreur, le `PageDoc` est à jeter.
    ///
    /// Rend la mise à jour telle que la page l'a lue, réencodée (v1) : **c'est
    /// elle qu'on écrit au journal et qu'on relaie aux autres clients, jamais
    /// les octets reçus** (Ruling R130). yrs et Yjs ne lisent pas les mêmes
    /// octets pareil : yrs laisse tomber les bits d'un entier au-delà de 32,
    /// Yjs lit l'entier en entier. Relayée telle quelle, une suppression dont
    /// l'horloge est écrite avec 2^32 de plus efface un texte sur le serveur,
    /// et rien dans les navigateurs, où l'on continue d'y écrire à perte.
    /// Réencodée, elle dit à tous ce que la page a fait.
    ///
    /// À ne pas employer pour ce qui est relu du stockage : la mise à jour du
    /// serveur qui crée la page pose `schemaVersion`, et y serait refusée.
    pub fn apply_client_update(&self, update: &[u8]) -> Result<Vec<u8>, CrdtError> {
        let mut txn = self.doc.transact_mut();
        let (shape, decoded) = self.read(&txn, update, true)?;
        // Ce que la page gardait en attente de suppression avant (d'ordinaire rien).
        let pending_before = Pending::of(txn.store().pending_ds());
        // Avant l'application, qui consomme la mise à jour lue.
        let read = decoded.encode_v1();
        self.integrate(&mut txn, &shape, decoded)?;
        // yrs ne dresse la liste des types changés qu'à la validation.
        txn.commit();
        if touched(&txn, &self.meta) {
            return Err(CrdtError::Structure(
                "la mise à jour d'un client écrit dans les métadonnées de la page".to_string(),
            ));
        }
        // Ruling R207, en filet de R198 : une suppression laissée en attente que la page
        // n'avait pas. `awaits` retient celles qu'un client honnête envoie ; si une autre forme
        // de mise à jour en laissait une, elle n'agirait que dans le document du serveur.
        if pending_before.lacks(txn.store().pending_ds()) {
            return Err(CrdtError::Structure(
                "la mise à jour laisserait une suppression en attente".to_string(),
            ));
        }
        Ok(read)
    }

    /// Vrai si la mise à jour (v1) est refusée parce qu'un client y paraît deux fois (Ruling
    /// R207). Une telle mise à jour est refusée avant que yrs la lise : la page est intacte. La
    /// relecture du journal s'en sert pour écarter l'entrée au lieu de refuser la page (Ruling
    /// R258).
    pub fn repeats_a_client(update: &[u8]) -> bool {
        matches!(Shape::read(update), Err(CrdtError::Decode(m)) if m == REPEATED_CLIENT)
    }

    /// Vrai si la mise à jour (v1), reçue d'un client, laisserait un trou
    /// dans les horloges d'un de ses clients : un bloc qui commence au-delà de
    /// ce que la page connaît de ce client, sans que la mise à jour porte ce
    /// qui manque (Ruling R131). yrs l'intégrerait en laissant le trou ; un
    /// navigateur la garde en attente tant que le trou n'est pas comblé, et le
    /// serveur lirait un texte qu'aucun navigateur n'affiche.
    ///
    /// Ou une suppression qui vise des horloges que la page n'aura pas, ses
    /// blocs intégrés, et qu'elle ne garde pas déjà en attente (Rulings R198,
    /// R199) : yrs la garderait en attente, et supprimerait à leur arrivée les
    /// éléments que le client visé écrirait dans la plage.
    ///
    /// La mise à jour est lue sans être appliquée : la page ne change pas.
    /// `Decode` si elle est illisible.
    pub fn leaves_gap(&self, update: &[u8]) -> Result<bool, CrdtError> {
        let shape = Shape::read_from_client(update)?;
        let txn = self.doc.transact();
        let pending = Pending::of(txn.store().pending_ds());
        Ok(shape.leaves_gap(&txn.state_vector(), &pending))
    }

    /// Ce qu'une mise à jour (v1) reçue d'un client attend encore de la page
    /// pour ne plus laisser de trou : `None` si elle n'en laisserait aucun
    /// ([`PageDoc::leaves_gap`]). Ce qui est rendu se compare ensuite à la
    /// page par [`PageDoc::has_reached`], sans relire la mise à jour.
    ///
    /// La mise à jour est lue sans être appliquée : la page ne change pas.
    /// `Decode` si elle est illisible.
    pub fn awaits(&self, update: &[u8]) -> Result<Option<Awaited>, CrdtError> {
        let shape = Shape::read_from_client(update)?;
        let txn = self.doc.transact();
        let pending = Pending::of(txn.store().pending_ds());
        let awaited = shape.awaited(&txn.state_vector(), &pending);
        Ok((!awaited.is_empty()).then_some(Awaited(awaited)))
    }

    /// Vrai si la page a atteint ce qu'attendait une mise à jour
    /// ([`PageDoc::awaits`]) : donnée maintenant, elle ne laisserait plus de
    /// trou. Ce qui est atteint le reste : le vecteur d'état d'une page ne
    /// fait que croître.
    pub fn has_reached(&self, awaited: &Awaited) -> bool {
        let state = self.doc.transact().state_vector();
        awaited
            .0
            .iter()
            .all(|(client, clock)| state.get(&ClientID::new(*client)) >= *clock)
    }

    /// Lit la mise à jour, par la garde puis par yrs, et la vérifie contre
    /// l'état de `txn`, sans rien appliquer (voir [`PageDoc::apply_update`]).
    fn read(
        &self,
        txn: &TransactionMut,
        update: &[u8],
        from_client: bool,
    ) -> Result<(Shape, Update), CrdtError> {
        let shape = if from_client {
            Shape::read_from_client(update)?
        } else {
            Shape::read(update)?
        };
        let decoded = Update::decode_v1(update).map_err(|e| CrdtError::Decode(e.to_string()))?;
        shape.check(&txn.state_vector())?;
        Ok((shape, decoded))
    }

    /// Applique dans `txn` une mise à jour lue par [`PageDoc::read`], et
    /// vérifie ce qui ne se sait qu'après (voir [`PageDoc::apply_update`]).
    fn integrate(
        &self,
        txn: &mut TransactionMut,
        shape: &Shape,
        decoded: Update,
    ) -> Result<(), CrdtError> {
        txn.apply_update(decoded)
            .map_err(|e| CrdtError::Decode(e.to_string()))?;
        if txn.store().pending_update().is_some() {
            return Err(missing());
        }
        if shape.deeper_than_bound(txn) {
            return Err(CrdtError::TooDeep);
        }
        if shape.inserts_elements() {
            self.inserted_elements.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    /// Vrai si le document garde des suppressions en attente : des plages
    /// qu'une mise à jour supprimait et que la page ne connaît pas encore. yrs
    /// les applique à l'arrivée des éléments, dans une autre transaction.
    ///
    /// Depuis le Ruling R198, une mise à jour de client n'en laisse plus : elle
    /// attend que la page ait ces horloges (`PageDoc::awaits`). Un document
    /// n'en a donc que s'il a été relu d'une base d'avant, ou s'il est la copie
    /// d'un client qui les a reçues du serveur (tests).
    pub fn has_pending_deletions(&self) -> bool {
        self.doc
            .transact()
            .store()
            .pending_ds()
            .is_some_and(|pending| !pending.is_empty())
    }

    /// Tout l'état du document, en une mise à jour (v1).
    pub fn encode_state(&self) -> Vec<u8> {
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    /// Vecteur d'état (v1).
    pub fn state_vector(&self) -> Vec<u8> {
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

    /// Vrai si un bloc de la page porte l'identifiant `id`, à toute profondeur : ce que dirait
    /// une recherche dans [`PageDoc::read_blocks`], sans bâtir la liste des blocs ni lire leur
    /// texte (Ruling R206). Un identifiant vide n'est jamais un bloc.
    pub fn has_block(&self, id: &str) -> bool {
        page_has_block(&self.doc.transact(), &self.fragment, id)
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
        let blocks = writable(blocks)?;
        let mut txn = self.doc.transact_mut();
        self.write_all(&mut txn, &blocks);
        Ok(txn.encode_update_v1())
    }

    /// Comme [`PageDoc::write_blocks`], pour `PUT /mmd` (Ruling R201) : avant d'écrire, un bloc
    /// sans identifiant reçoit celui du bloc courant qui lui correspond (même liste de frères,
    /// mêmes type, contenu et props, ou même place entre deux blocs gardés et même type ; voir
    /// le module `kept_ids`). Le MMD que rend `GET /mmd` ne porte pas d'ancres : sans cela, un
    /// agent qui récrivait une page telle quelle donnait un identifiant neuf à chaque bloc, et
    /// les fils de commentaires perdaient le leur. Un bloc qui porte un identifiant le garde.
    ///
    /// Comme `write_blocks`, l'écriture remplace tout le contenu : une édition faite en même
    /// temps sur le contenu remplacé est perdue (Ruling R188).
    pub fn rewrite_blocks(&self, blocks: &[Block]) -> Result<Vec<u8>, CrdtError> {
        let mut blocks = writable(blocks)?;
        // Avant la transaction d'écriture : `read_blocks` en ouvre une de lecture.
        let current = self.read_blocks();
        keep_ids(&current, &mut blocks);
        let mut txn = self.doc.transact_mut();
        self.write_all(&mut txn, &blocks);
        Ok(txn.encode_update_v1())
    }

    /// Les refus de [`PageDoc::write_blocks`], sans rien écrire : pour qui doit
    /// savoir que l'écriture passera avant de faire autre chose (l'acteur de la
    /// page, qui coupe une version avant une restauration).
    pub fn check_writable(blocks: &[Block]) -> Result<(), CrdtError> {
        writable(blocks).map(|_| ())
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

    /// Pour `mementum-sync` seulement, qui y pose son observateur des fins de
    /// transaction : le compte des suppressions (compaction, Ruling R40) et
    /// celui des changements (journal et historique, Rulings R135 et R181) ;
    /// jamais pour lire le contenu.
    pub fn yrs_doc(&self) -> &Doc {
        &self.doc
    }
}

/// Les blocs que [`PageDoc::write_blocks`] écrit : normalisés, un paragraphe vide pour un
/// arbre vide (Ruling R16), et refusés si BlockNote les refuserait (`Structure`, `TooDeep`).
fn writable(blocks: &[Block]) -> Result<Vec<Block>, CrdtError> {
    let blocks = match normalize(blocks.to_vec()) {
        b if b.is_empty() => vec![empty_paragraph()],
        b => b,
    };
    check_blocks(&blocks, Slot::Group, 2)?;
    Ok(blocks)
}
