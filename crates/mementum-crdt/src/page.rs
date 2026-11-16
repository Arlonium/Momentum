//! Le document de page : création, mises à jour Yjs, lecture et écriture de
//! l'arbre de blocs.

use std::collections::HashMap;
use std::sync::Mutex;
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
use crate::moves::{self, Carried, MAX_CARRIED_BYTES, Recreated};
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

/// Ce que [`PageDoc::apply_client_update`] a fait d'une mise à jour d'un client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// Ce que la page a intégré de la mise à jour, encodé par elle (v1) : c'est ce qu'on écrit au
    /// journal et qu'on relaie aux autres clients, jamais les octets reçus (Rulings R130, R491).
    pub read: Vec<u8>,
    /// L'écriture du serveur qui reporte sur les blocs que la mise à jour recrée ce que son
    /// auteur n'en avait pas vu (Task 13b, module `moves`), déjà appliquée à la page : à écrire
    /// au journal avec `read`, dans la même écriture, puis à diffuser à toutes les connexions,
    /// l'expéditeur compris. `None` s'il n'y a rien à reporter.
    pub carried: Option<Vec<u8>>,
    /// Pourquoi le report n'a pas été écrit, s'il a dépassé ses bornes (Ruling R451) : pour le
    /// journal du serveur.
    pub withheld: Option<String>,
}

/// Au plus tant de copies reportées dont la page se souvient pour le dédoublonnage (Ruling
/// R453) ; au-delà, elle les oublie toutes.
const MAX_REMEMBERED_COPIES: usize = 1_024;

/// Le refus d'une mise à jour de client ([`PageDoc::try_client_update`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub error: CrdtError,
    /// Refusée avant d'être appliquée : le document est intact (Ruling R492).
    pub intact: bool,
}

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
    /// Parcours de la page faits pour trouver l'original d'un bloc recréé (pour les tests).
    pub(crate) original_walks: AtomicU64,
    /// Les copies qui ont reçu un report, par identifiant de bloc : le dédoublonnage garde
    /// celle-là quand le bloc est en double (Ruling R453). Oubliées au rechargement.
    pub(crate) carried_copies: Mutex<HashMap<String, yrs::ID>>,
}

// L'acteur de synchronisation garde le document d'une tâche tokio à l'autre.
const _: fn() = || {
    fn shareable<T: Send + Sync>() {}
    shareable::<PageDoc>();
};

impl PageDoc {
    fn empty() -> PageDoc {
        PageDoc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        })
    }

    fn with_options(options: Options) -> PageDoc {
        let doc = Doc::with_options(options);
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
            original_walks: AtomicU64::new(0),
            carried_copies: Mutex::new(HashMap::new()),
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
            page.fill(&mut txn, &[empty_paragraph()]);
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
    ///
    /// Une marque de texte que la garde refuserait d'un client (Ruling R431 : une clé hors du
    /// schéma des blocs, une valeur trop longue), écrite au journal avant cette règle, est
    /// supprimée du document dans la même transaction : le serveur ne garde jamais une marque
    /// qu'il refuserait d'un client, et ce qu'il donne aux clients (l'état complet, ce qui leur
    /// manque) porte cette suppression, qu'ils appliquent à leur tour. La relecture du journal
    /// la refait chaque fois, de la même façon, dans n'importe quel ordre.
    ///
    /// Seules les marques que la mise à jour a intégrées sont supprimées (Ruling R490) : celles
    /// dont l'horloge dépasse ce que la page avait de leur client avant elle. Une structure à une
    /// horloge que la page connaît déjà, yrs l'ignore ; supprimer son identifiant retirait
    /// l'élément que la page porte à cette horloge (une lettre d'un autre client).
    pub fn apply_update(&self, update: &[u8]) -> Result<(), CrdtError> {
        let mut txn = self.doc.transact_mut();
        let (shape, decoded) = self.read(&txn, update, false)?;
        let before = txn.state_vector();
        self.integrate(&mut txn, &shape, decoded)?;
        let outside = shape.outside_marks(&before);
        if !outside.is_empty() {
            // Une mise à jour sans bloc, qui ne porte que ces suppressions : aucun client (`0`),
            // puis le jeu de suppressions. Les marques sont dans la page (intégrées à l'instant,
            // ou déjà là) : rien ne reste en attente.
            let mut deletions = vec![0];
            deletions.extend(outside.encode_v1());
            let deletions =
                Update::decode_v1(&deletions).map_err(|e| CrdtError::Decode(e.to_string()))?;
            txn.apply_update(deletions)
                .map_err(|e| CrdtError::Decode(e.to_string()))?;
        }
        Ok(())
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
    /// Refusée enfin (`Structure`, Ruling R431) si elle pose une marque de texte
    /// hors du schéma des blocs, ou d'une valeur plus longue que sa borne : un
    /// client du schéma de la page n'en écrit pas (un client d'un autre schéma est
    /// en lecture seule), et y-prosemirror supprimerait chez tous le texte qui la
    /// porte.
    ///
    /// Certains de ces refus ne sont connus qu'après l'application (voir le module `guard`) :
    /// comme après toute erreur, le `PageDoc` est à jeter. [`PageDoc::try_client_update`] dit
    /// lesquels laissent le document intact.
    ///
    /// Rend ce que la page a intégré de la mise à jour, encodé par elle (v1) : **c'est ce qu'on
    /// écrit au journal et qu'on relaie aux autres clients, jamais les octets reçus** (Ruling
    /// R130). yrs et Yjs ne lisent pas les mêmes octets pareil : yrs laisse tomber les bits d'un
    /// entier au-delà de 32, Yjs lit l'entier en entier. Relayée telle quelle, une suppression
    /// dont l'horloge est écrite avec 2^32 de plus efface un texte sur le serveur, et rien dans les
    /// navigateurs, où l'on continue d'y écrire à perte. Encodée par la page, elle dit à tous ce
    /// que la page a fait, et seulement cela (Ruling R491) : une structure à une horloge que la
    /// page connaît déjà, yrs l'ignore ; réencodée de la mise à jour lue, elle allait au journal,
    /// et la relecture lisait une autre page que la page vivante.
    ///
    /// À ne pas employer pour ce qui est relu du stockage : la mise à jour du
    /// serveur qui crée la page pose `schemaVersion`, et y serait refusée.
    ///
    /// Une mise à jour qui recrée un bloc (elle insère un `blockContainer` à l'`id` d'un bloc
    /// vivant, dont elle supprime l'élément : un déplacement, Ruling R220) fait reporter sur la
    /// copie ce que son auteur n'avait pas vu de l'original (Task 13b, Ruling R221, module
    /// `moves`) : relevé avant l'application, écrit après, dans une transaction du serveur, et
    /// rendu dans [`Applied::carried`]. Rien de cela ne s'exécute pour une mise à jour qui
    /// n'insère aucun élément : le chemin d'une frappe ne change pas.
    ///
    /// Le report est retenu avant d'écrire s'il dépasse ses bornes, estimées par le haut
    /// ([`Applied::withheld`], Rulings R451, R458). Écrit, il est relu comme une entrée du journal
    /// (la garde, la profondeur, la taille, Ruling R452) : s'il n'y passe pas, ce qui ne devrait
    /// jamais arriver, l'erreur est [`CrdtError::CarryRefused`], et comme après toute erreur la
    /// page est à jeter (R459).
    pub fn apply_client_update(&self, update: &[u8]) -> Result<Applied, CrdtError> {
        self.try_client_update(update)
            .map_err(|refused| refused.error)
    }

    /// [`PageDoc::apply_client_update`], dont le refus dit aussi si la mise à jour a été refusée
    /// avant d'être appliquée (`intact`, Ruling R492) : illisible, hors du schéma, trop profonde à
    /// elle seule, une marque refusée. Le document est alors intact, et l'appelant n'a pas à le
    /// recharger. Sinon (une dépendance absente, un élément trop profond une fois intégré, les
    /// métadonnées touchées, une suppression laissée en attente, un report refusé), il est à
    /// jeter.
    pub fn try_client_update(&self, update: &[u8]) -> Result<Applied, Refused> {
        let applied = |error| Refused {
            error,
            intact: false,
        };
        let mut txn = self.doc.transact_mut();
        let (shape, decoded) = self.read(&txn, update, true).map_err(|error| Refused {
            error,
            intact: true,
        })?;
        // Ce que la page gardait en attente de suppression avant (d'ordinaire rien).
        let pending_before = Pending::of(txn.store().pending_ds());
        // Avant l'application, qui supprime l'original de chaque bloc recréé et tout ce qu'il
        // porte. La lecture du texte y découpe des blocs de yrs sans rien changer au contenu :
        // l'encodage de la transaction, plus bas, n'en porte rien.
        let recreated = if shape.inserts_elements() {
            moves::capture(&mut txn, &self.fragment, &shape, &self.original_walks)
        } else {
            Vec::new()
        };
        self.integrate(&mut txn, &shape, decoded).map_err(applied)?;
        // Ce que la transaction a intégré, encodé avant sa validation (où passe le
        // ramasse-miettes).
        let read = txn.encode_update_v1();
        // yrs ne dresse la liste des types changés qu'à la validation.
        txn.commit();
        if touched(&txn, &self.meta) {
            return Err(applied(CrdtError::Structure(
                "la mise à jour d'un client écrit dans les métadonnées de la page".to_string(),
            )));
        }
        // Ruling R207, en filet de R198 : une suppression laissée en attente que la page
        // n'avait pas. `awaits` retient celles qu'un client honnête envoie ; si une autre forme
        // de mise à jour en laissait une, elle n'agirait que dans le document du serveur.
        if pending_before.lacks(txn.store().pending_ds()) {
            return Err(applied(CrdtError::Structure(
                "la mise à jour laisserait une suppression en attente".to_string(),
            )));
        }
        drop(txn);
        let (carried, withheld) = if recreated.is_empty() {
            (None, None)
        } else {
            self.carry(&recreated).map_err(applied)?
        };
        Ok(Applied {
            read,
            carried,
            withheld,
        })
    }

    /// Écrit dans les copies ce que [`moves::capture`] a relevé, et rend la mise à jour (v1) de
    /// cette écriture, s'il y a eu quelque chose à écrire, ou pourquoi rien ne l'a été.
    ///
    /// L'écriture est relue comme une entrée du journal avant d'être rendue (Ruling R452) : la
    /// garde, la profondeur de ce qu'elle insère, sa taille. Si elle n'y passe pas, elle est déjà
    /// dans la page : `CarryRefused`, et la page est à jeter.
    fn carry(
        &self,
        recreated: &[Recreated],
    ) -> Result<(Option<Vec<u8>>, Option<String>), CrdtError> {
        let mut txn = self.doc.transact_mut();
        let before = txn.state_vector();
        let (written, bound) = match moves::carry(&mut txn, recreated) {
            Carried::Nothing => return Ok((None, None)),
            Carried::Withheld(why) => return Ok((None, Some(why))),
            Carried::Written(written, bound) => (written, bound),
        };
        let update = txn.encode_update_v1();
        // Ruling R458 : l'estimation est une borne haute ; la dépasser est un défaut, que les
        // tests voient.
        debug_assert!(
            update.len() <= bound,
            "report de {} octets, au-delà de sa borne haute ({bound})",
            update.len()
        );
        if let Err(why) = verify_carried(&update, &before, &txn) {
            return Err(CrdtError::CarryRefused(why));
        }
        drop(txn);
        let mut copies = self
            .carried_copies
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if copies.len() + written.len() > MAX_REMEMBERED_COPIES {
            copies.clear();
        }
        copies.extend(written);
        Ok((Some(update), None))
    }

    /// Nombre de parcours de la page faits pour trouver l'original d'un bloc recréé, au-delà de
    /// 65 536 horloges supprimées (Ruling R372). Sert aux tests.
    #[doc(hidden)]
    pub fn original_walks(&self) -> u64 {
        self.original_walks.load(Ordering::SeqCst)
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

    /// Vrai si les blocs d'une entrée relue du stockage laisseraient un trou dans les horloges
    /// d'un de leurs clients (Rulings R366, R388) : lue comme le stockage se lit (une plage de
    /// suppression vide y est admise, Ruling R208). La relecture du journal s'en sert après une
    /// entrée écartée (Ruling R258) : une entrée qui suivrait, chez son client, des blocs de
    /// l'entrée écartée laisserait un trou, que yrs intègre et qu'un navigateur garde en attente.
    ///
    /// Ses suppressions n'y comptent pas (Ruling R388) : une écriture complète du serveur, faite
    /// quand l'entrée écartée était dans la page, supprime ses éléments ; écartée pour cela, elle
    /// ramenait la page à un ancien contenu. yrs garde ces suppressions en attente, sans effet sur
    /// ce que la page porte, comme Yjs.
    pub fn stored_update_leaves_gap(&self, update: &[u8]) -> Result<bool, CrdtError> {
        let shape = Shape::read(update)?;
        let txn = self.doc.transact();
        Ok(shape.structs_leave_gap(&txn.state_vector()))
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
        let state = txn.state_vector();
        shape.check(&state)?;
        if from_client {
            // Ruling R431 : une marque de texte hors du schéma, ou trop longue.
            shape.check_marks(&state)?;
        }
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
        self.write_all(&blocks)
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
        self.write_all(&blocks)
    }

    /// Les refus de [`PageDoc::write_blocks`], sans rien écrire : pour qui doit
    /// savoir que l'écriture passera avant de faire autre chose (l'acteur de la
    /// page, qui coupe une version avant une restauration).
    pub fn check_writable(blocks: &[Block]) -> Result<(), CrdtError> {
        writable(blocks).map(|_| ())
    }

    /// Écrit des blocs normalisés et vérifiés à la place de tout le contenu, et rend la mise à
    /// jour (v1) qui fait les deux.
    ///
    /// Le retrait du contenu courant et l'écriture du nouveau se font dans deux transactions,
    /// dont les mises à jour sont mises bout à bout en une (Ruling R367). En une seule
    /// transaction, le ramasse-miettes de yrs, qui passe à sa validation sur ce qu'elle a
    /// supprimé, coûtait en proportion du contenu retiré multiplié par le contenu écrit du même
    /// client : récrire une page de 10 000 paragraphes de six plages occupait l'acteur 10 à 11 s,
    /// contre 0,8 à 1,4 s pour la première écriture. Les suppressions ne prennent pas d'horloge :
    /// les éléments écrits ont les mêmes identifiants et les mêmes origines qu'en une transaction,
    /// et la mise à jour est la même, octet pour octet (tests de ce module).
    ///
    /// Si l'une des deux transactions a écrit dans la part de l'autre (une écriture qui
    /// supprimerait, en récrivant un attribut déjà posé), `HalfWritten` (Ruling R389) : le retrait
    /// est fait, le document est à recharger. Aucune écriture du serveur n'en fait aujourd'hui
    /// (la prop `colwidth` d'une cellule, que l'écriture posait deux fois, est sautée) ; c'était
    /// une assertion, qui arrêtait l'acteur.
    fn write_all(&self, blocks: &[Block]) -> Result<Vec<u8>, CrdtError> {
        let (cleared, wrote_nothing) = {
            let mut txn = self.doc.transact_mut();
            self.clear(&mut txn);
            (txn.encode_update_v1(), txn.insert_set().is_empty())
        };
        let (filled, removed_nothing) = {
            let mut txn = self.doc.transact_mut();
            self.fill(&mut txn, blocks);
            (txn.encode_update_v1(), txn.delete_set().is_empty())
        };
        // Une mise à jour v1 est la suite des éléments écrits, puis l'ensemble des suppressions.
        // `fill` ne supprime rien et `clear` n'écrit rien : chacune de ces deux parties vides
        // tient en un octet nul. Les éléments de l'une suivis des suppressions de l'autre sont,
        // octet pour octet, la mise à jour qu'aurait encodée une seule transaction ; la fusion
        // de yrs (`merge_updates_v1`), qui relit et récrit tout, coûtait 3 s sur un tableau de
        // 300 000 cellules.
        if !(wrote_nothing
            && removed_nothing
            && filled.last() == Some(&0)
            && cleared.first() == Some(&0))
        {
            return Err(CrdtError::HalfWritten);
        }
        let mut update = filled;
        update.pop();
        update.extend_from_slice(&cleared[1..]);
        Ok(update)
    }

    /// Retire tout le contenu, sauf la première racine `blockGroup`, vidée.
    fn clear(&self, txn: &mut TransactionMut) {
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
        if let Some(XmlOut::Element(root)) = self.fragment.get(txn, 0) {
            let len = root.len(txn);
            if len > 0 {
                root.remove_range(txn, 0, len);
            }
        }
    }

    /// Écrit des blocs dans la racine vide laissée par [`PageDoc::clear`] (ou dans une page
    /// neuve), qu'il crée s'il n'y en a pas.
    fn fill(&self, txn: &mut TransactionMut, blocks: &[Block]) {
        let root = match self.fragment.get(txn, 0) {
            Some(XmlOut::Element(root)) => root,
            _ => self
                .fragment
                .insert(txn, 0, XmlElementPrelim::empty(BLOCK_GROUP)),
        };
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

/// Relit le report `update`, écrit dans `txn` sur une page dont le vecteur d'état était `before`,
/// comme une entrée du journal (Ruling R452) : sa taille ([`MAX_CARRIED_BYTES`]), la garde, la
/// profondeur de ce qu'il insère. `Err` dit ce qui ne passe pas.
fn verify_carried(update: &[u8], before: &StateVector, txn: &TransactionMut) -> Result<(), String> {
    if update.len() > MAX_CARRIED_BYTES {
        return Err(format!(
            "{} octets, au-delà de {MAX_CARRIED_BYTES}",
            update.len()
        ));
    }
    let shape = Shape::read(update).map_err(|e| e.to_string())?;
    shape.check(before).map_err(|e| e.to_string())?;
    if shape.deeper_than_bound(txn) {
        return Err(CrdtError::TooDeep.to_string());
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use mementum_md::{Content, Inline, Styles, arb};
    use proptest::prelude::*;

    use super::*;

    /// Une page vide, à l'identifiant de client fixé : deux telles pages qui reçoivent les mêmes
    /// écritures encodent les mêmes octets.
    fn page(client: u64) -> PageDoc {
        PageDoc::with_options(Options {
            client_id: ClientID::new(client),
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        })
    }

    /// L'écriture complète d'avant le Ruling R367 : retrait et écriture dans une transaction.
    fn write_in_one_transaction(page: &PageDoc, blocks: &[Block]) -> Vec<u8> {
        let mut txn = page.doc.transact_mut();
        page.clear(&mut txn);
        page.fill(&mut txn, blocks);
        txn.encode_update_v1()
    }

    /// Des blocs dont l'écriture ne dépend que d'eux : un identifiant pour chacun (sinon
    /// `write_nodes` en tire un neuf), et des plages qui n'ouvrent jamais deux marques à la fois
    /// (voir `one_mark`). yrs ouvre les marques d'une plage dans l'ordre d'une `HashMap` propre à
    /// chaque appel : deux écritures de ces plages ne rangent pas leurs marques dans le même
    /// ordre, en une transaction comme en deux.
    fn deterministic(blocks: &mut [Block], next: &mut usize) {
        for block in blocks {
            *next += 1;
            block.id = format!("bloc-{next}");
            match &mut block.content {
                Content::Inline(content) => one_mark(content),
                Content::Table(table) => {
                    for cell in table.rows.iter_mut().flatten() {
                        one_mark(&mut cell.content);
                    }
                }
                Content::None => {}
            }
            deterministic(&mut block.children, next);
        }
    }

    /// Un identifiant pour chaque bloc, rien d'autre.
    fn numbered(mut blocks: Vec<Block>) -> Vec<Block> {
        let mut next = 0;
        let mut stack: Vec<&mut Block> = blocks.iter_mut().collect();
        while let Some(block) = stack.pop() {
            next += 1;
            block.id = format!("bloc-{next}");
            stack.extend(block.children.iter_mut());
        }
        blocks
    }

    /// Les liens défaits en leurs plages, et au plus une marque par plage, le gras ou
    /// l'italique : même avec une marque par plage, des liens qui se suivent laissent derrière
    /// eux des marques qui en rétablissent d'autres, et la plage suivante en ouvre alors deux.
    fn one_mark(content: &mut Vec<Inline>) {
        let mut out = Vec::with_capacity(content.len());
        for inline in std::mem::take(content) {
            match inline {
                Inline::Link { mut children, .. } => {
                    one_mark(&mut children);
                    out.extend(children);
                }
                Inline::Text { text, styles } => out.push(Inline::Text {
                    text,
                    styles: Styles {
                        bold: styles.bold,
                        italic: styles.italic && !styles.bold,
                        ..Styles::default()
                    },
                }),
                other => out.push(other),
            }
        }
        *content = out;
    }

    /// Ruling R452 (mutations C1c à C1e de la re-relecture) : la relecture d'un report refuse
    /// une mise à jour trop lourde, une qui écrit hors de la page, et une qui insère plus profond
    /// que la borne ; elle laisse passer une écriture ordinaire.
    #[test]
    fn verify_carried_refuses_what_a_stored_entry_would_refuse() {
        use yrs::{Text, XmlTextPrelim};

        // Une écriture ordinaire.
        let page = page(7);
        page.fill(&mut page.doc.transact_mut(), &[empty_paragraph()]);
        let before = page.doc.transact().state_vector();
        let mut txn = page.doc.transact_mut();
        let root = page.root(&txn).unwrap();
        let text = root.push_back(&mut txn, XmlTextPrelim::new(""));
        text.insert(&mut txn, 0, "texte");
        let update = txn.encode_update_v1();
        assert_eq!(verify_carried(&update, &before, &txn), Ok(()));
        drop(txn);

        // Trop lourde (C1e).
        let mut txn = page.doc.transact_mut();
        let before = txn.state_vector();
        text.insert(&mut txn, 0, &"x".repeat(MAX_CARRIED_BYTES + 1));
        let update = txn.encode_update_v1();
        assert!(verify_carried(&update, &before, &txn).is_err());
        drop(txn);

        // Hors de la page (C1d) : une racine que la garde refuse.
        let other = page.doc.get_or_insert_xml_fragment("autre");
        let mut txn = page.doc.transact_mut();
        let before = txn.state_vector();
        other.push_back(&mut txn, XmlTextPrelim::new("hors de la page"));
        let update = txn.encode_update_v1();
        assert!(verify_carried(&update, &before, &txn).is_err());
        drop(txn);

        // Plus profond que la borne (C1c).
        let mut txn = page.doc.transact_mut();
        let mut parent = page.root(&txn).unwrap();
        for _ in 1..crate::MAX_YJS_DEPTH {
            parent = parent.push_back(&mut txn, XmlElementPrelim::empty(BLOCK_GROUP));
        }
        drop(txn);
        let mut txn = page.doc.transact_mut();
        let before = txn.state_vector();
        parent.push_back(&mut txn, XmlTextPrelim::new("trop profond"));
        let update = txn.encode_update_v1();
        assert_eq!(
            verify_carried(&update, &before, &txn),
            Err(CrdtError::TooDeep.to_string())
        );
    }

    /// Ruling R389 : la prop `colwidth` d'une cellule, dans un tableau à largeurs, était posée
    /// deux fois par l'écriture, et l'assertion de `write_all` arrêtait l'acteur. Elle est sautée :
    /// la largeur vient de celles du tableau. Une écriture qui écrirait encore dans la part du
    /// retrait (un attribut posé deux fois, ici l'identifiant d'une colonne) rend `HalfWritten`
    /// au lieu de paniquer.
    #[test]
    fn a_cell_colwidth_is_skipped_and_a_half_write_is_an_error() {
        let mut blocks =
            mementum_md::parse("| a | b |\n| --- | --- |\n{widths=\"120,\"}\n").unwrap();
        let Content::Table(table) = &mut blocks[0].content else {
            panic!("un tableau");
        };
        table.rows[0][0]
            .props
            .insert("colwidth".to_string(), mementum_md::PropValue::Int(7));
        let (page, _) = PageDoc::new_page();
        page.write_blocks(&blocks).unwrap();
        let Content::Table(read) = &page.read_blocks()[0].content else {
            panic!("un tableau relu");
        };
        assert_eq!(read.column_widths, [Some(120.0), None]);
        assert!(!read.rows[0][0].props.contains_key("colwidth"));

        let mut columns = mementum_md::parse(
            "<columns>\n<column>\nUn\n</column>\n<column>\nDeux\n</column>\n</columns>\n",
        )
        .unwrap();
        columns[0].children[0].props.insert(
            "id".to_string(),
            mementum_md::PropValue::Str("x".to_string()),
        );
        assert!(matches!(
            page.write_blocks(&columns),
            Err(CrdtError::HalfWritten)
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Ruling R367 : en deux transactions, l'écriture complète rend, octet pour octet, la mise
        /// à jour d'une seule transaction, et laisse la page dans le même état.
        #[test]
        fn split_write_is_the_single_transaction_byte_for_byte(
            before in arb::tree(),
            after in arb::tree(),
        ) {
            let (Ok(mut before), Ok(mut after)) = (writable(&before), writable(&after)) else {
                return Ok(());
            };
            deterministic(&mut before, &mut 0);
            deterministic(&mut after, &mut 0);
            let (split, single) = (page(7), page(7));
            for page in [&split, &single] {
                write_in_one_transaction(page, &before);
            }
            prop_assert!(split.encode_state() == single.encode_state(), "première écriture");
            let written = split.write_all(&after).unwrap();
            prop_assert!(written == write_in_one_transaction(&single, &after), "mise à jour");
            prop_assert!(split.encode_state() == single.encode_state(), "état");
            prop_assert_eq!(split.read_blocks(), single.read_blocks());
        }

        /// Sur des blocs quelconques : la même page lue, et la mise à jour rendue, appliquée à une
        /// copie de la page d'avant, la mène au même contenu (ses éléments et ses suppressions y
        /// sont). Les octets ne se comparent pas : yrs range les marques d'une plage qui en ouvre
        /// plusieurs dans l'ordre d'une `HashMap`, et deux écritures en une transaction de ces
        /// mêmes blocs n'ont déjà pas les mêmes horloges.
        #[test]
        fn split_write_reads_as_the_single_transaction(
            before in arb::tree(),
            after in arb::tree(),
        ) {
            let (split, single) = (page(7), page(7));
            let before = numbered(writable(&before).unwrap());
            for page in [&split, &single] {
                write_in_one_transaction(page, &before);
            }
            let replica = PageDoc::from_updates([split.encode_state().as_slice()]).unwrap();
            let after = numbered(writable(&after).unwrap());
            let written = split.write_all(&after).unwrap();
            write_in_one_transaction(&single, &after);
            prop_assert_eq!(split.read_blocks(), single.read_blocks());
            replica.apply_update(&written).unwrap();
            prop_assert_eq!(replica.read_blocks(), split.read_blocks());
        }
    }
}
