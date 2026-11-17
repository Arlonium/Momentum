//! Le document de page : création, mises à jour Yjs, lecture et écriture de
//! l'arbre de blocs.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mementum_md::{Block, normalize};
use mementum_schema::SCHEMA_VERSION;
use yrs::any::Number;
use yrs::branch::{Branch, BranchID, BranchPtr};
use yrs::types::TypeRef;
use yrs::types::weak::LinkSource;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, Assoc, ClientID, Doc, ID, IdSet, Map, MapRef, OffsetKind, Options, Out, ReadTxn,
    StateVector, StickyIndex, Transact, TransactionMut, Update, Xml, XmlElementPrelim,
    XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut,
};

use crate::attrs::{self, Key, Value};
use crate::blocknote::{BLOCK_GROUP, Ids, empty_paragraph, page_has_block, read_page, write_nodes};
use crate::guard::runs::{Judge, Named, Run, Runs};
use crate::guard::{
    ASTRIDE, CLOCK_OUT_OF_BOUNDS, DEPENDS_ON_A_HOLE, KEYED_RIGHT_ORIGIN, KEYED_RIGHT_ORIGIN_MADE,
    LEAVES_GAP, MANY_VALUED, MANY_VALUED_MADE, NOT_AS_WRITTEN, PageItem, Pending, REPEATED_CLIENT,
    RIGHT_ORIGIN_IS_ORIGIN, Shape, UNTYPED_CHILD, cut, json_made, missing, touched,
};
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
    /// Les clients dont la page a pu garder un trou d'horloge : une entrée relue du stockage
    /// y a laissé des blocs au-delà de ce que la page connaissait d'eux (un journal d'avant le
    /// Ruling R131 ; d'un client, rien n'en laisse, Ruling R786). Pour eux seuls, ce que la page
    /// connaît au-delà du trou est demandé à yrs ([`PageDoc::known_beyond`], Ruling R785).
    holed: Mutex<HashSet<u64>>,
    /// `holed` n'est pas vide : lu à chaque mise à jour, sans verrou.
    any_holed: AtomicBool,
    /// Par client, les suites d'unités que la page a intégrées et qui peuvent tenir en un bloc
    /// de yrs (règle N3, Ruling R825) : ce que chaque unité portait à son écriture, que yrs ne
    /// rend pas public. Remplies à chaque mise à jour lue, et complétées d'après yrs pour une
    /// écriture du serveur, quand une mise à jour en demande une unité.
    runs: Mutex<Runs>,
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
            holed: Mutex::new(HashSet::new()),
            any_holed: AtomicBool::new(false),
            runs: Mutex::new(Runs::default()),
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
    ///
    /// De même, un contenu qui n'est pas un nœud XML (une chaîne, une valeur `Any`…) parmi les
    /// enfants de la racine ou d'un élément, que la garde refuse d'un client (Rulings R496, R497),
    /// est supprimé s'il est au journal (Ruling R498) : `children` de yrs s'arrêtait au premier, et
    /// le serveur lisait la page vide. Seulement ce que l'entrée a intégré (Ruling R490), et
    /// seulement si une liste qu'elle change ne se lit plus en entier : le parent de chaque contenu
    /// candidat n'est cherché qu'alors.
    ///
    /// De même encore, un contenu JSON que l'entrée intègre, où qu'il soit (Ruling R815) : d'un
    /// client, la garde le refuse (R702) ; au journal (une entrée d'avant cette règle), il est
    /// supprimé dans la transaction qui l'intègre, et le ramasse-miettes de la validation en fait
    /// un contenu supprimé. yrs 0.28.1 écrit un contenu JSON en `len` chaînes et le relit en
    /// `len + 1` : resté vivant, il rendait illisible tout état que la page encode, et la
    /// compaction suivante remplaçait le journal par un tel état.
    pub fn apply_update(&self, update: &[u8]) -> Result<(), CrdtError> {
        let (shape, before, whole, unsound) = {
            let mut txn = self.doc.transact_mut();
            // Ruling R805 : les entrées de map de plusieurs unités que l'entrée laisserait
            // vivantes, d'après ses octets ; supprimées en entier une fois intégrées.
            let mut many_valued = IdSet::new();
            let (shape, decoded, runs) = self.read(&txn, update, false, Some(&mut many_valued))?;
            let before = txn.state_vector();
            // Ruling R690 : les éléments à qui l'entrée pose un attribut que la page ne garde pas.
            let named = shape
                .attrs(&before, |id| tag_in_page(&txn, id), |_| PageItem::Unknown)
                .unsound;
            self.integrate(&mut txn, &shape, decoded)?;
            self.remember_holes(&shape, &before);
            self.remember_runs(runs);
            // Ruling R723 : ce que yrs a fait d'un élément à origine droite ne se lit que dans
            // ce qu'il encode. Une page vide n'a rien qui serve d'origine : tout est dans
            // l'entrée (un état complet, encodé par yrs), et ses octets ont suffi.
            //
            // Rulings R803, R805 : de même ce qu'il a fait d'un élément placé par un voisin
            // (entrée de map ou enfant de liste, règle P1 de la grammaire). Ruling R804 : jugé
            // sur **tout** ce que l'entrée a intégré ; l'encodage de la transaction s'arrête au
            // premier trou de chaque client, et ce qu'une entrée posait au-delà d'un trou
            // n'était pas jugé.
            let siblings = before.iter().next().is_some() && shape.placed_by_siblings(&before);
            // Ruling R815 : un contenu JSON que l'entrée intègre (d'un client, il est refusé
            // avant d'être appliqué, Ruling R702).
            let json = shape.carries_json(&before);
            let mut json_items = IdSet::new();
            if siblings || json {
                let encoded = self.integrated(&txn, &shape, &before);
                if siblings {
                    match shape.check_as_written(&encoded, &before, false, Some(&mut many_valued)) {
                        // Ce que la garde ne relit pas de ce que yrs encode n'est pas jugé ici.
                        Ok(()) | Err(CrdtError::Decode(_)) => {}
                        Err(error) => return Err(error),
                    }
                }
                // Ruling R805 : une entrée de map de plusieurs unités restée vivante (un contenu
                // placé par l'origine d'une entrée, écrit au journal avant la grammaire) est
                // supprimée en entier, dans la même transaction : coupée plus tard, ses moitiés
                // resteraient vivantes sans être l'entrée courante, et yrs lirait de la mémoire
                // libérée une fois leur parent ramassé. La relecture le refait chaque fois.
                //
                // Ruling R815 : de même un contenu JSON, où qu'il soit (un texte, une entrée de
                // map). yrs 0.28.1 l'écrit en `len` chaînes et le relit en `len + 1` : resté
                // vivant, il rendait illisible tout état que la page encode (instantané, version,
                // réponse d'étape 1), et la compaction remplaçait le journal par un tel état. Le
                // ramasse-miettes de la validation en fait un contenu supprimé, que yrs relit.
                // Seul ce que yrs a intégré comme du JSON est supprimé, lu dans ce qu'il encode ;
                // à défaut, d'après les octets de l'entrée.
                if json {
                    json_items =
                        json_made(&encoded, &before).unwrap_or_else(|_| shape.json_items(&before));
                }
            }
            delete(&mut txn, &many_valued)?;
            delete(&mut txn, &json_items)?;
            delete(&mut txn, &shape.outside_marks(&before))?;
            txn.commit();
            let named = named.into_iter().filter_map(|(client, clock)| {
                BranchID::get_nested(&txn, &ID::new(ClientID::new(client), clock))
            });
            let unsound = unsound_attrs(&txn, named.collect::<Vec<_>>(), &shape, &before);
            let whole = lists_are_whole(&txn, &shape);
            if whole && unsound.is_empty() {
                return Ok(());
            }
            (shape, before, whole, unsound)
        };
        let mut txn = self.doc.transact_mut();
        // Ruling R690 : l'attribut fautif est retiré ; l'éditeur le lit alors à son défaut.
        for (element, _, key) in unsound {
            XmlElementRef::from(element).remove_attribute(&mut txn, &key);
        }
        if whole {
            return Ok(());
        }
        let mut outside = IdSet::new();
        for (id, len) in shape.untyped_items(&before) {
            let parent = StickyIndex::from_id(id, Assoc::After)
                .get_offset(&txn)
                .map(|offset| offset.branch);
            if parent.is_some_and(|branch| !matches!(branch.type_ref(), TypeRef::XmlText)) {
                outside.insert(id, len);
            }
        }
        delete(&mut txn, &outside)
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
        let (shape, decoded, runs) =
            self.read(&txn, update, true, None)
                .map_err(|error| Refused {
                    error,
                    intact: true,
                })?;
        // Ce que la page gardait en attente de suppression avant (d'ordinaire rien).
        let pending_before = Pending::of(txn.store().pending_ds());
        let floor = txn.state_vector();
        // Avant l'application, qui supprime l'original de chaque bloc recréé et tout ce qu'il
        // porte. La lecture du texte y découpe des blocs de yrs sans rien changer au contenu :
        // l'encodage de la transaction, plus bas, n'en porte rien.
        let recreated = if shape.inserts_elements() {
            moves::capture(&mut txn, &self.fragment, &shape, &self.original_walks)
        } else {
            Vec::new()
        };
        self.integrate(&mut txn, &shape, decoded).map_err(applied)?;
        self.remember_runs(runs);
        // Ce que la transaction a intégré, encodé avant sa validation (où passe le
        // ramasse-miettes).
        let read = txn.encode_update_v1();
        // Rulings R723, R724 : une entrée de map à origine droite, ou dont le contenu n'est pas
        // une valeur, d'après ce que yrs en a fait. Jugé avant la validation, mais ce n'est pas
        // un abandon : yrs valide la transaction quand elle est lâchée, ramasse-miettes compris.
        // Le refus tient parce que tout appelant jette alors la page (l'acteur la recharge du
        // journal, où rien de cela n'est écrit) et que rien ne l'encode entre-temps.
        //
        // Ruling R803 : et un élément que yrs n'a pas rangé comme ses octets le disent (règle P1
        // de la grammaire). Seul un élément placé par un voisin peut l'être ; une frappe en est
        // un, et sa lecture ici est celle que la règle d'avant faisait déjà.
        if shape.placed_by_siblings(&floor) {
            shape
                .check_as_written(&read, &floor, true, None)
                .map_err(applied)?;
        }
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
        // Ruling R497 : une liste que la mise à jour change, ou un élément qu'elle insère, se lit
        // en entier ; sinon elle y a posé un contenu qui n'est pas un nœud XML, par l'origine
        // d'un voisin (la garde a refusé, avant d'appliquer, ce qui nomme son parent).
        if !lists_are_whole(&txn, &shape) {
            return Err(applied(CrdtError::Structure(UNTYPED_CHILD.to_string())));
        }
        // Ruling R691 : un attribut que la garde n'a pas pu juger avant d'appliquer (une valeur
        // posée par l'origine d'une ancienne entrée, ou d'une forme qu'aucun client n'écrit), et
        // que la page ne garde pas.
        if let Some((_, tag, key)) = unsound_attrs(&txn, [], &shape, &floor).first() {
            return Err(applied(CrdtError::Structure(attrs::refusal_in_page(
                tag, key,
            ))));
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
    /// garde, la profondeur de ce qu'elle insère, sa taille, et les gardes qu'une mise à jour de
    /// client passe (Ruling R536) : ses marques (R431), ses contenus sans type (R496), ses listes
    /// lues en entier (R497). Si elle n'y passe pas, elle est déjà dans la page : `CarryRefused`,
    /// et la page est à jeter.
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
        // yrs ne dresse la liste des types changés qu'à la validation (R497, `lists_are_whole`).
        txn.commit();
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

    /// L'état complet d'un document (v1) gardé ailleurs que dans le journal (une version de
    /// l'historique), tel qu'on le sert à un éditeur (Ruling R701) : relu comme une entrée du
    /// journal (un attribut que la garde refuse est retiré, Rulings R690, R702, R703), puis
    /// réencodé. `None` si le serveur ne sait pas le relire : il ne le sert pas.
    ///
    /// Toujours relu, jamais servi tel quel : ce qu'il faut retirer ne se voit pas tout dans les
    /// octets (une entrée de map que yrs et Yjs ne placent pas pareil, R703), et yrs écrit un
    /// contenu JSON sous une forme qu'il ne relit pas lui-même (un état qui en porte un est
    /// illisible pour lui, lisible pour Yjs, qui y lit un objet). Depuis le Ruling R815, une page
    /// n'en garde plus de vivant : seul un état coupé avant cette règle en porte, et il n'est pas
    /// servi.
    pub fn served_state(state: &[u8]) -> Option<Vec<u8>> {
        PageDoc::from_updates([state])
            .ok()
            .map(|page| page.encode_state())
    }

    /// `Ok` si un `PageDoc` neuf relit `state`, un état complet que la page vient d'encoder, comme
    /// le chargement relira un instantané (Ruling R816) : à vérifier avant d'écrire un état qui
    /// deviendra la seule copie d'un contenu (l'instantané d'une compaction, qui efface le journal
    /// qu'il remplace ; la version d'avant une restauration). Quelle que soit la cause (yrs 0.28.1
    /// n'écrit pas toujours ce qu'il relit, R815), un état que le serveur ne relirait pas n'est
    /// jamais écrit à la place de ce qui se relit. Le coût est celui d'un chargement de l'état :
    /// 81 ms pour une page de 10 000 blocs (3,2 Mo), en release.
    pub fn check_state(state: &[u8]) -> Result<(), CrdtError> {
        PageDoc::from_updates([state]).map(|_| ())
    }

    /// Ce qui manque, dans l'état que la page encode, à l'invariant de la classe C1 (Ruling R817) :
    /// chaque entrée de map vivante est la seule vivante de sa clé, et n'a qu'une unité. `None`
    /// si l'état le tient. Sert aux essais au hasard, qui le vérifient après chaque mise à jour
    /// plutôt que d'attendre que yrs lise de la mémoire libérée.
    #[doc(hidden)]
    pub fn unsound_map_entries(&self) -> Option<String> {
        match Shape::read_written(&self.encode_state()) {
            Ok(shape) => shape.unsound_map_entries(),
            Err(error) => Some(format!("l'état ne se relit pas : {error}")),
        }
    }

    /// Ce que la garde des attributs (Ruling R689) refuserait de cette mise à jour (v1) venue
    /// d'un client, lue sans être appliquée : `None` si elle n'y trouve rien. Sert aux tests,
    /// qui y passent tout ce que l'éditeur écrit.
    #[doc(hidden)]
    pub fn attribute_refusal(&self, update: &[u8]) -> Result<Option<String>, CrdtError> {
        let shape = Shape::read_from_client(update)?;
        let txn = self.doc.transact();
        let findings = shape.attrs(
            &txn.state_vector(),
            |id| tag_in_page(&txn, id),
            |origin| entry_in_page(&txn, origin),
        );
        if let Err(CrdtError::Structure(refusal)) = shape.check_contents(&txn.state_vector()) {
            return Ok(Some(refusal));
        }
        Ok(findings.refusal)
    }

    /// Vrai si un élément de la mise à jour (v1) porte, dans ses octets, la marque « entrée de
    /// map » et une origine droite (Ruling R723) : une entrée du journal écrite par un serveur
    /// d'avant cette garde, telle que yrs l'a encodée. [`PageDoc::apply_update`] la refuse avant
    /// de l'appliquer (la page est intacte), et la relecture du journal l'écarte au lieu de
    /// refuser la page, comme une entrée où un client paraît deux fois (Ruling R258).
    pub fn has_keyed_right_origin(update: &[u8]) -> bool {
        Shape::read(update).is_ok_and(|shape| shape.check_keyed(&StateVector::default()).is_err())
    }

    /// Vrai si `error`, rendue par [`PageDoc::apply_update`], est un refus fait **avant**
    /// d'appliquer une entrée qu'aucun client honnête n'écrit : du JSON à cheval sur ce que la
    /// page connaît (Ruling R784 : tout autre élément à cheval est coupé, pas refusé), une entrée
    /// de map à origine droite dans les octets (Ruling R723). La page est intacte : qui relit un journal écarte l'entrée et continue (Ruling
    /// R742). Le même défaut vu après l'application rend une autre erreur : la page est à jeter.
    pub fn refused_intact(error: &CrdtError) -> bool {
        matches!(error, CrdtError::Structure(why) if why == ASTRIDE
            || why == KEYED_RIGHT_ORIGIN
            || why == MANY_VALUED
            || why == DEPENDS_ON_A_HOLE
            || why == CLOCK_OUT_OF_BOUNDS
            || why == RIGHT_ORIGIN_IS_ORIGIN)
    }

    /// Vrai si `error`, rendue par [`PageDoc::apply_update`], est le refus de ce que yrs a fait
    /// d'une entrée **après** l'avoir appliquée (Rulings R723, R803, R806) : une entrée de map à
    /// origine droite, un élément rangé autrement que ses octets ne le disent, une entrée de map
    /// de plusieurs valeurs restée vivante. Aucun client n'écrit ce qu'elle porte. La page n'est
    /// plus saine : qui relit un journal la jette, et reprend sans l'entrée.
    pub fn refused_made(error: &CrdtError) -> bool {
        matches!(error, CrdtError::Structure(why) if why == KEYED_RIGHT_ORIGIN_MADE
            || why == NOT_AS_WRITTEN
            || why == MANY_VALUED_MADE)
    }

    /// Vrai si `error` est le refus d'une mise à jour qui dépend de modifications que la page n'a
    /// pas (une origine, un parent absents). La page n'est plus intacte : yrs garde la mise à
    /// jour en attente.
    pub fn lacks_a_dependency(error: &CrdtError) -> bool {
        matches!((error, missing()), (CrdtError::Decode(a), CrdtError::Decode(b)) if *a == b)
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

    /// Vrai si une entrée relue du stockage dépend, par une origine ou un parent, d'un élément
    /// que ni la page ni elle-même ne portent (Ruling R787) : appliquée, yrs la garderait en
    /// attente, et la page serait à jeter. La relecture du journal s'en sert après une entrée
    /// écartée, pour écarter sans rien appliquer ce qui dépendait d'elle (Ruling R743). L'entrée
    /// est lue comme [`PageDoc::apply_update`] la lit (coupée, Ruling R784), sans être
    /// appliquée ; `Err` si elle est refusée à la lecture.
    pub fn stored_update_lacks_dependency(&self, update: &[u8]) -> Result<bool, CrdtError> {
        let txn = self.doc.transact_mut();
        let (shape, _, _) = self.read(&txn, update, false, Some(&mut IdSet::new()))?;
        let state = txn.state_vector();
        let beyond = self.known_beyond(&txn, Some(&shape), &state)?;
        Ok(shape.lacks_dependency(&state, |client, clock| {
            clock < state.get(&ClientID::new(client))
                || beyond.get(&client).is_some_and(|ranges| {
                    ranges
                        .iter()
                        .any(|range| range.0 <= clock && clock < range.1)
                })
        }))
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
    ///
    /// Rend aussi les suites d'unités qu'elle pose (règle N3, Ruling R825), que l'appelant
    /// retient une fois la mise à jour intégrée ([`PageDoc::remember_runs`]).
    fn read(
        &self,
        txn: &TransactionMut,
        update: &[u8],
        from_client: bool,
        many_valued: Option<&mut IdSet>,
    ) -> Result<(Shape, Update, Vec<(u64, Run)>), CrdtError> {
        let read = |bytes: &[u8]| {
            if from_client {
                Shape::read_from_client(bytes)
            } else {
                Shape::read(bytes)
            }
        };
        let mut shape = read(update)?;
        // Jugé sur les octets reçus : ce que la coupe y change (un trou de la mise à jour à la
        // place d'une part connue) n'en est pas un.
        let leaves_gap = from_client && {
            let state = txn.state_vector();
            shape.structs_leave_gap(&state)
        };
        // Le plancher de chaque client : ce que la page connaît de lui **sans trou**, donc le
        // début de son premier trou si elle en a un (`get_state_vector` de yrs). Toutes les
        // gardes « au-dessus du plancher » le lisent ainsi. Une page n'a de trou que relue d'un
        // journal d'avant le Ruling R131 (`holed`) : d'un client, rien n'en laisse (plus bas).
        let state = txn.state_vector();
        // Ruling R784 : un élément à cheval sur ce que la page connaît est coupé avant que la
        // garde et yrs le lisent ; ils n'en voient que la part neuve, placée par son origine.
        let mut cut = None;
        // Rulings R801, R802 : de même ce que Yjs écrit et que yrs ne range pas sûrement : une
        // entrée de map de plusieurs valeurs (récrite en autant d'entrées d'une valeur), un
        // élément dont l'origine droite est son origine (qui la perd).
        let normalized = shape.normalized(&state);
        if shape.astride(&state) || normalized || self.any_holed.load(Ordering::Relaxed) {
            // Ruling R785 : dans une page qui a un trou, ce qu'elle connaît au-delà aussi.
            let beyond = self.known_beyond(txn, Some(&shape), &state)?;
            if (shape.astride(&state) || normalized || !beyond.is_empty())
                && let Some(bytes) = cut::cut(update, &state, &beyond)?
            {
                shape = read(&bytes)?;
                cut = Some(bytes);
            }
        }
        let beyond = self.known_beyond(txn, Some(&shape), &state)?;
        // Ruling R828 (règle I1) : un élément sans origine dont l'origine droite en a une (il se dit
        // en tête de la liste, devant une unité qui ne l'est pas). Aucun client ne l'écrit, et sa
        // place dépend de l'ordre d'arrivée : il devient une plage ramassée.
        // Ruling R834 (règle I3) : de même un élément dont l'origine de l'origine droite est à
        // droite de son origine, ou l'origine droite de l'origine à gauche de son origine droite,
        // dans une même suite : son auteur ne voyait pas ses deux voisins côte à côte.
        let mut origin_less = HashSet::new();
        let candidates = shape.origin_less_with_right(&state);
        let between = shape.between_two(&state);
        if !candidates.is_empty() || !between.is_empty() {
            let mut posed = Runs::default();
            for (client, run) in shape.runs(&state, &beyond) {
                posed.insert(client, run);
            }
            let mut page = self
                .runs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut judge = Judge {
                txn,
                floor: &state,
                beyond: &beyond,
                page: &mut page,
                update: &posed,
            };
            for (id, right) in candidates {
                if judge.origin_less_before_a_placed_unit(right)? {
                    origin_less.insert(id);
                }
            }
            for (id, origin, right) in between {
                if judge.crosses_what_it_knows(origin, right)? {
                    origin_less.insert(id);
                }
            }
        }
        // Ruling R827 (règle G) : un élément dont l'un des voisins (origine, origine droite) est
        // une plage ramassée et l'autre un élément devient une plage ramassée, comme Yjs le fait ;
        // yrs, lui, le rangeait par l'autre voisin, et sa place dépendait de l'ordre d'arrivée.
        let collected = collected_neighbours(txn, &shape, &state, &beyond, origin_less)?;
        if !collected.is_empty() {
            let bytes = cut::collect(cut.as_deref().unwrap_or(update), &collected)?;
            shape = read(&bytes)?;
            cut = Some(bytes);
        }
        // Ruling R825 (règle N3) : un élément que yrs et Yjs rangeraient selon le découpage de
        // leurs blocs (son origine droite est du client de son origine, à une horloge plus basse,
        // et une même suite peut porter l'unité d'avant l'origine droite et l'origine) perd son
        // origine droite, comme N2. Jugé après la coupe, sur ce que yrs intègre (Ruling R721).
        let candidates = shape.backward_right_origins(&state);
        if !candidates.is_empty() {
            let mut posed = Runs::default();
            for (client, run) in shape.runs(&state, &beyond) {
                posed.insert(client, run);
            }
            let mut page = self
                .runs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut judge = Judge {
                txn,
                floor: &state,
                beyond: &beyond,
                page: &mut page,
                update: &posed,
            };
            let mut flagged = HashSet::new();
            for (id, origin, right) in candidates {
                if judge.placed_by_blocks(origin, right)? {
                    flagged.insert(id);
                }
            }
            drop(page);
            if !flagged.is_empty() {
                let bytes = cut::unright(cut.as_deref().unwrap_or(update), &flagged)?;
                shape = read(&bytes)?;
                cut = Some(bytes);
            }
        }
        let runs = shape.runs(&state, &beyond);
        let update = cut.as_deref().unwrap_or(update);
        let decoded = Update::decode_v1(update).map_err(|e| CrdtError::Decode(e.to_string()))?;
        shape.check(&state)?;
        // Ruling R807 : dans une page qui a un trou, un élément qui dépend (origine, origine
        // droite, parent) d'une horloge du trou. yrs croit la connaître (il compare à la fin de
        // ce qu'il a du client), la cherche parmi ses blocs, et panique.
        if self.any_holed.load(Ordering::Relaxed) {
            let beyond = self.known_beyond(txn, None, &state)?;
            if !beyond.is_empty()
                && shape.lacks_dependency(&state, |client, clock| {
                    clock < state.get(&ClientID::new(client))
                        || beyond.get(&client).is_some_and(|ranges| {
                            ranges
                                .iter()
                                .any(|range| range.0 <= clock && clock < range.1)
                        })
                })
            {
                return Err(CrdtError::Structure(DEPENDS_ON_A_HOLE.to_string()));
            }
        }
        // Ruling R723 : une entrée de map à origine droite, quand les octets le disent.
        shape.check_keyed(&state)?;
        // Rulings R805, R806 (et les filets de R801 et R802) : la grammaire de ce qu'un client écrit, quand les octets
        // suffisent à la juger.
        shape.check_grammar(&state, from_client, many_valued)?;
        if from_client {
            // Ruling R431 : une marque de texte hors du schéma, ou trop longue.
            shape.check_marks(&state)?;
            // Ruling R496 : un contenu qui n'est pas un nœud XML parmi les enfants de la racine ou
            // d'un élément nommé.
            shape.check_untyped(&state, |(client, clock)| {
                BranchID::get_nested(txn, &ID::new(ClientID::new(client), clock))
                    .map(|branch| matches!(branch.type_ref(), TypeRef::XmlText))
            })?;
            // Rulings R702, R703 : un contenu que yrs et Yjs ne lisent pas pareil.
            shape.check_contents(&state)?;
            // Ruling R689 : un attribut d'élément qui n'est pas du type que dit le registre.
            let findings = shape.attrs(
                &state,
                |id| tag_in_page(txn, id),
                |origin| entry_in_page(txn, origin),
            );
            if let Some(refusal) = findings.refusal {
                return Err(CrdtError::Structure(refusal));
            }
            // Ruling R803 : une valeur `Any` qui ne se dit pas entrée de map.
            shape.check_values_flagged(&state)?;
            // Ruling R786 : d'un client, des blocs qui laisseraient un trou. L'acteur retient une
            // telle mise à jour avant de l'appliquer (`PageDoc::awaits`, Ruling R131) ; refusée ici
            // aussi, pour que ce qui suit ne dépende pas de l'appelant : un trou fait que la page
            // connaît des horloges au-delà du plancher, que les gardes ne regardent pas.
            if leaves_gap {
                return Err(CrdtError::Structure(LEAVES_GAP.to_string()));
            }
        }
        Ok((shape, decoded, runs))
    }

    /// Retient les suites d'unités qu'une mise à jour lue par [`PageDoc::read`] vient de poser
    /// (Ruling R825).
    fn remember_runs(&self, runs: Vec<(u64, Run)>) {
        if runs.is_empty() {
            return;
        }
        let mut page = self
            .runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (client, run) in runs {
            page.insert(client, run);
        }
    }

    /// Ce que la page connaît **au-delà de son premier trou** des clients de `shape` qui en ont
    /// un (`holed`), ou de tous les clients à trou sans `shape` : lu dans ce que yrs encode pour un destinataire qui s'arrêterait une unité
    /// avant le trou. yrs ne dit rien d'autre de ses trous ; le coût est celui d'un encodage de
    /// ce qui suit le trou et du jeu de suppressions, pour les seules mises à jour qui nomment un
    /// client à trou (un ancien onglet d'un journal d'avant le Ruling R131).
    fn known_beyond<T: ReadTxn>(
        &self,
        txn: &T,
        shape: Option<&Shape>,
        state: &StateVector,
    ) -> Result<cut::Beyond, CrdtError> {
        let mut beyond = cut::Beyond::new();
        if !self.any_holed.load(Ordering::Relaxed) {
            return Ok(beyond);
        }
        let mut holed = self
            .holed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let wanted: Vec<u64> = match shape {
            Some(shape) => shape
                .clients()
                .filter(|client| holed.contains(client))
                .collect(),
            None => holed.iter().copied().collect(),
        };
        for client in wanted {
            let id = ClientID::new(client);
            let floor = state.get(&id);
            let mut before: HashMap<ClientID, u32> =
                state.iter().map(|(c, clock)| (*c, *clock)).collect();
            if floor == 0 {
                before.remove(&id);
            } else {
                before.insert(id, floor - 1);
            }
            let encoded = txn.encode_diff_v1(&StateVector::new(before.into_iter().collect()));
            let ranges: Vec<(u32, u32)> = Shape::read(&encoded)?
                .ranges_of(client)
                .into_iter()
                .filter(|range| range.0 > floor)
                .collect();
            beyond.insert(client, ranges);
        }
        // Un trou comblé depuis : le client n'en a plus.
        holed.retain(|client| beyond.get(client).is_none_or(|ranges| !ranges.is_empty()));
        if holed.is_empty() {
            self.any_holed.store(false, Ordering::Relaxed);
        }
        beyond.retain(|_, ranges| !ranges.is_empty());
        Ok(beyond)
    }

    /// Ce que yrs a intégré de l'entrée `shape`, encodé par lui (Ruling R804). L'encodage d'une
    /// transaction part du vecteur d'état d'avant, qui s'arrête au premier trou de chaque
    /// client : il ne porte rien de ce que l'entrée a posé au-delà. Pour les clients de l'entrée
    /// chez qui la page a un trou (l'entrée vient d'en laisser un, ou la page en avait), on
    /// encode donc depuis une unité avant leur plancher, comme [`PageDoc::known_beyond`] : tout
    /// ce que la page porte d'eux au-delà y est, l'entrée comprise.
    fn integrated(&self, txn: &TransactionMut, shape: &Shape, before: &StateVector) -> Vec<u8> {
        if !self.any_holed.load(Ordering::Relaxed) {
            return txn.encode_update_v1();
        }
        let holed = self
            .holed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut from: HashMap<ClientID, u32> =
            before.iter().map(|(c, clock)| (*c, *clock)).collect();
        let mut lowered = false;
        for client in shape.clients().filter(|client| holed.contains(client)) {
            let id = ClientID::new(client);
            match before.get(&id) {
                0 => {
                    from.remove(&id);
                }
                floor => {
                    from.insert(id, floor - 1);
                }
            }
            lowered = true;
        }
        if !lowered {
            return txn.encode_update_v1();
        }
        txn.encode_diff_v1(&StateVector::new(from.into_iter().collect()))
    }

    /// Retient les clients chez qui l'entrée qu'on vient d'intégrer a laissé un trou
    /// ([`PageDoc::known_beyond`]).
    fn remember_holes(&self, shape: &Shape, before: &StateVector) {
        let gapped = shape.gapped_clients(before);
        if gapped.is_empty() {
            return;
        }
        self.holed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(gapped);
        self.any_holed.store(true, Ordering::Relaxed);
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

/// Relit le report `update`, écrit dans `txn` (validée) sur une page dont le vecteur d'état était
/// `before`, comme une entrée du journal (Ruling R452) : sa taille ([`MAX_CARRIED_BYTES`]), la
/// garde, la profondeur de ce qu'il insère ; et comme une mise à jour de client (Ruling R536) :
/// ses marques de texte (R431), ses contenus sans type parmi les enfants d'un élément (R496), et
/// chaque liste qu'il change, lue en entier (R497). `Err` dit ce qui ne passe pas.
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
    shape.check_marks(before).map_err(|e| e.to_string())?;
    shape
        .check_untyped(before, |(client, clock)| {
            BranchID::get_nested(txn, &ID::new(ClientID::new(client), clock))
                .map(|branch| matches!(branch.type_ref(), TypeRef::XmlText))
        })
        .map_err(|e| e.to_string())?;
    if !lists_are_whole(txn, &shape) {
        return Err(UNTYPED_CHILD.to_string());
    }
    // Ruling R692 : les attributs que le report pose passent la garde des attributs.
    if let Some(refusal) = shape
        .attrs(before, |id| tag_in_page(txn, id), |_| PageItem::Unknown)
        .refusal
    {
        return Err(refusal);
    }
    if let Some((_, tag, key)) = unsound_attrs(txn, [], &shape, before).first() {
        return Err(attrs::refusal_in_page(tag, key));
    }
    Ok(())
}

/// Au plus tant d'unités de la page dont les règles G et P2 demandent ce qu'elles sont, pour une
/// mise à jour (Rulings R827, R829) : chaque demande parcourt la liste de l'unité vers la gauche.
const MAX_NEIGHBOUR_LOOKUPS: usize = 16_384;

/// Ce qu'est une unité, pour les règles G et P2.
#[derive(Clone, PartialEq)]
enum Unit {
    /// Une plage ramassée.
    Collected,
    /// Un élément, dans ce parent.
    In(BranchID),
    /// Ni la page ni la mise à jour ne la portent : yrs la tiendra pour une dépendance absente.
    Unknown,
}

/// Règles G et P2 (Rulings R827, R829) : les éléments de la mise à jour (au-dessus du plancher,
/// après la coupe) que la page ramasse au lieu de les ranger, rendus pour être récrits en plages
/// ramassées ([`cut::collect`]).
///
/// - **G** : un voisin (origine, origine droite) est une plage ramassée, l'autre un élément. Yjs
///   en fait une plage ramassée (`Item.getMissing` met le parent à `null` dès que le voisin de
///   gauche ou de droite est un `GC`, et `Item.integrate` intègre alors un `GC`) ; yrs ne voit pas
///   de voisin du tout là où l'unité est ramassée, prend le parent de l'autre voisin, et range
///   l'élément : la page vivante et la page relue de son état ne le rangeaient pas pareil (l'ordre
///   d'intégration décide), ni le serveur et un navigateur.
/// - **P2** : les deux voisins sont des éléments de deux parents différents. yrs comme Yjs le
///   rangent dans le parent de gauche ; mais si ce voisin est ramassé plus tard (son parent
///   supprimé), la page relue de son état le range dans le parent de droite (yrs) ou le ramasse
///   (Yjs) : la page vivante n'était plus sa relecture.
///
/// Aucun client honnête n'écrit ni l'un ni l'autre : l'origine et l'origine droite d'un élément
/// sont dans le même parent, et une page ramasse tous les enfants d'un parent supprimé à la fois.
///
/// Un élément dont les deux voisins sont ramassés, ou le seul qu'il a, ou dont le parent nommé
/// l'est, est ramassé par yrs comme par Yjs : il n'est pas rendu, mais compte comme ramassé pour
/// ceux qui le nomment dans la mise à jour. `already` : les éléments déjà jugés à ramasser (règle
/// I1, Ruling R828), rendus avec les autres, et ramassés pour ceux qui les nomment.
///
/// Ce qu'est une unité de la page se lit par `StickyIndex::get_offset`, qui rend son parent, ou
/// `None` si yrs n'y trouve pas d'élément (yrs ne rend pas public ce qu'il garde d'une unité). Au-delà
/// de [`MAX_NEIGHBOUR_LOOKUPS`] demandes, la mise à jour est refusée.
fn collected_neighbours<T: ReadTxn>(
    txn: &T,
    shape: &Shape,
    floor: &StateVector,
    beyond: &cut::Beyond,
    already: HashSet<(u64, u32)>,
) -> Result<HashSet<(u64, u32)>, CrdtError> {
    let posed = shape.posed(floor, beyond);
    if already.is_empty()
        && posed
            .iter()
            .all(|p| p.gc || (p.origin.is_none() && p.right.is_none()))
    {
        return Ok(HashSet::new());
    }
    // Ce qu'est chaque élément posé, au fil des passes ; `None` tant qu'on ne le sait pas.
    let mut known: Vec<Option<Unit>> = posed
        .iter()
        .map(|p| (p.gc || already.contains(&p.id)).then_some(Unit::Collected))
        .collect();
    // Les unités de la mise à jour, par client, dans l'ordre : (début, fin, rang dans `posed`).
    let mut units: HashMap<u64, Vec<(u32, u32, usize)>> = HashMap::new();
    for (k, p) in posed.iter().enumerate() {
        units
            .entry(p.id.0)
            .or_default()
            .push((p.id.1, p.id.1 + p.len, k));
    }
    let in_page = |(client, clock): (u64, u32)| {
        clock < floor.get(&ClientID::new(client))
            || beyond
                .get(&client)
                .is_some_and(|ranges| ranges.iter().any(|r| r.0 <= clock && clock < r.1))
    };
    let mut page: HashMap<(u64, u32), Unit> = HashMap::new();
    let mut flagged = already;
    // Ce qu'est l'unité `id` ; `None` : un élément de la mise à jour qu'on ne sait pas encore.
    let mut resolve = |id: (u64, u32), known: &[Option<Unit>]| -> Result<Option<Unit>, CrdtError> {
        if in_page(id) {
            if let Some(unit) = page.get(&id) {
                return Ok(Some(unit.clone()));
            }
            if page.len() >= MAX_NEIGHBOUR_LOOKUPS {
                return Err(CrdtError::Structure(
                    "la mise à jour nomme trop de voisins dans la page pour être jugée".to_string(),
                ));
            }
            let unit = match StickyIndex::from_id(ID::new(ClientID::new(id.0), id.1), Assoc::After)
                .get_offset(txn)
            {
                Some(offset) => Unit::In(offset.branch.id()),
                None => Unit::Collected,
            };
            page.insert(id, unit.clone());
            return Ok(Some(unit));
        }
        Ok(
            match units.get(&id.0).and_then(|list| {
                let at = list.partition_point(|unit| unit.1 <= id.1);
                list.get(at)
                    .filter(|unit| unit.0 <= id.1)
                    .map(|unit| unit.2)
            }) {
                Some(k) => known[k].clone(),
                None => Some(Unit::Unknown),
            },
        )
    };
    // Jusqu'à ce que rien ne change : ce qu'est un élément vaut pour ceux qui le nomment.
    for _ in 0..64 {
        let mut changed = false;
        for (k, p) in posed.iter().enumerate() {
            if known[k].is_some() {
                continue;
            }
            let unit = match &p.parent {
                Named::Root(name) => Some(Unit::In(BranchID::Root(name.as_str().into()))),
                Named::Id(id) => match resolve(*id, &known)? {
                    Some(Unit::Collected) => Some(Unit::Collected),
                    Some(Unit::In(_)) => Some(Unit::In(BranchID::Nested(ID::new(
                        ClientID::new(id.0),
                        id.1,
                    )))),
                    Some(Unit::Unknown) => Some(Unit::Unknown),
                    None => None,
                },
                Named::None => {
                    // Absent (`None`), pas encore su (`Some(None)`), su.
                    let origin = p.origin.map(|id| resolve(id, &known)).transpose()?;
                    let right = p.right.map(|id| resolve(id, &known)).transpose()?;
                    if matches!(origin, Some(None)) || matches!(right, Some(None)) {
                        None
                    } else {
                        Some(match (origin.flatten(), right.flatten()) {
                            (Some(Unit::Unknown), _) | (_, Some(Unit::Unknown)) | (None, None) => {
                                Unit::Unknown
                            }
                            (Some(Unit::Collected), Some(Unit::In(_)))
                            | (Some(Unit::In(_)), Some(Unit::Collected)) => {
                                flagged.insert(p.id);
                                Unit::Collected
                            }
                            (Some(Unit::In(a)), Some(Unit::In(b))) if a != b => {
                                flagged.insert(p.id);
                                Unit::Collected
                            }
                            (Some(Unit::In(a)), _) | (None, Some(Unit::In(a))) => Unit::In(a),
                            (Some(Unit::Collected), _) | (None, Some(Unit::Collected)) => {
                                Unit::Collected
                            }
                        })
                    }
                }
            };
            if unit.is_some() {
                known[k] = unit;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(flagged)
}

/// La balise d'un type de la page, pour la garde des attributs : `Some(None)` s'il n'est pas un
/// élément, `None` si la page n'a pas ce type.
fn tag_in_page<T: ReadTxn>(txn: &T, (client, clock): (u64, u32)) -> Option<Option<Arc<str>>> {
    BranchID::get_nested(txn, &ID::new(ClientID::new(client), clock)).map(|branch| {
        match branch.type_ref() {
            TypeRef::XmlElement(tag) => Some(tag.clone()),
            _ => None,
        }
    })
}

/// L'attribut dont `origin`, un élément Yjs de la page, est l'entrée courante : la balise de son
/// élément XML et sa clé (Ruling R689). Une entrée qui en écrase une autre ne nomme ni l'un ni
/// l'autre : elle les tient de l'entrée écrasée, son origine. yrs ne rend public ni le parent ni
/// la clé d'un élément Yjs ; le parent se lit par une position relative (`StickyIndex`), et
/// l'entrée courante de chaque clé se compare par le lien faible que yrs en donne (`Map::link`,
/// fonction `weak` de yrs : un lien n'est jamais écrit, seule sa source est comparée). `None` pour
/// une ancienne entrée, un enfant d'une liste, ou un élément que la page n'a pas.
fn entry_in_page<T: ReadTxn>(txn: &T, (client, clock): (u64, u32)) -> PageItem {
    let id = ID::new(ClientID::new(client), clock);
    let Some(offset) = StickyIndex::from_id(id, Assoc::After).get_offset(txn) else {
        return PageItem::Unknown;
    };
    let branch = offset.branch;
    let TypeRef::XmlElement(tag) = branch.type_ref() else {
        return PageItem::OutsideElements;
    };
    let wanted = LinkSource::new(
        StickyIndex::from_id(id, Assoc::Before),
        StickyIndex::from_id(id, Assoc::After),
    );
    let map = MapRef::from(branch);
    XmlElementRef::from(branch)
        .attributes(txn)
        .find(|(key, _)| {
            map.link(txn, key)
                .is_some_and(|link| **link.source() == wanted)
        })
        .map_or(PageItem::Unknown, |(key, _)| {
            PageItem::Entry(tag.clone(), Key::of(key))
        })
}

/// Les attributs que la page ne garde pas (Ruling R689, module `attrs`) parmi ceux des éléments
/// que la transaction (validée) a changés eux-mêmes, et de `more` : l'élément, sa balise, la clé.
/// C'est le jugement de ce que les clients liront (Ruling R691) : la valeur courante de chaque
/// attribut, quelle que soit la forme de l'entrée qui la porte. Une frappe change un texte, pas
/// un élément : rien n'est lu pour elle.
///
/// Un attribut dont l'entrée courante est un élément de la mise à jour qui n'est pas ce qu'un
/// client écrit pour un attribut (`Shape::not_attribute_values` : un contenu qui n'est pas une
/// valeur `Any`, une origine droite) n'est pas gardé non plus, quoi que yrs y lise (Rulings R702,
/// R703) : yrs rend une chaîne là où Yjs lit un objet, un caractère, ou un enfant. yrs ne dit
/// pas quel élément porte un attribut : l'entrée courante de chaque clé est comparée par son
/// lien faible (Ruling R697).
fn unsound_attrs(
    txn: &TransactionMut,
    more: impl IntoIterator<Item = BranchPtr>,
    shape: &Shape,
    floor: &StateVector,
) -> Vec<(BranchPtr, Arc<str>, String)> {
    let mut seen = HashSet::new();
    let mut unsound = Vec::new();
    // Les suspects, par la forme écrite de leur lien (`LinkSource` n'a ni ordre ni empreinte, et
    // yrs ne rend pas l'identifiant d'un lien) : les chercher un à un pour chaque attribut
    // coûtait le produit des deux, et la relecture d'une entrée aux blocs fautifs nombreux
    // croissait comme son carré (Ruling R744).
    let mut suspects: Option<HashSet<String>> = None;
    for branch in changed_themselves(txn).chain(more) {
        let TypeRef::XmlElement(tag) = branch.type_ref() else {
            continue;
        };
        if !attrs::judged(tag) || !seen.insert(branch) {
            continue;
        }
        let map = MapRef::from(branch);
        for (key, value) in XmlElementRef::from(branch).attributes(txn) {
            let suspects = suspects.get_or_insert_with(|| {
                shape
                    .not_attribute_values(floor)
                    .into_iter()
                    .map(|id| {
                        format!(
                            "{:?}",
                            LinkSource::new(
                                StickyIndex::from_id(id, Assoc::Before),
                                StickyIndex::from_id(id, Assoc::After),
                            )
                        )
                    })
                    .collect()
            });
            let suspect = !suspects.is_empty()
                && map
                    .link(txn, key)
                    .is_some_and(|link| suspects.contains(&format!("{:?}", **link.source())));
            if suspect || !attrs::sound(tag, Key::of(key), Value::of_out(&value)) {
                unsound.push((branch, tag.clone(), key.to_string()));
            }
        }
    }
    unsound
}

/// Les types que la transaction (validée) a changés eux-mêmes : yrs donne les types changés
/// suivis chacun de ses ancêtres jusqu'à la racine (`changed_parent_types`) ; un type est changé
/// lui-même s'il n'est pas le parent de celui qui le précède.
fn changed_themselves<'a>(txn: &'a TransactionMut) -> impl Iterator<Item = BranchPtr> + 'a {
    let changed = txn.changed_parent_types();
    changed
        .iter()
        .enumerate()
        .filter(move |&(i, branch)| i == 0 || parent_of(changed[i - 1]) != Some(*branch))
        .map(|(_, branch)| *branch)
}

/// Supprime de la page les éléments de `ids` (une mise à jour sans bloc, qui ne porte que ces
/// suppressions : aucun client, puis le jeu de suppressions). Ils sont dans la page : rien ne reste
/// en attente.
fn delete(txn: &mut TransactionMut, ids: &IdSet) -> Result<(), CrdtError> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut deletions = vec![0];
    deletions.extend(ids.encode_v1());
    let deletions = Update::decode_v1(&deletions).map_err(|e| CrdtError::Decode(e.to_string()))?;
    txn.apply_update(deletions)
        .map_err(|e| CrdtError::Decode(e.to_string()))
}

/// Vrai si chaque liste que la transaction (validée) a changée elle-même, et chaque élément que la
/// mise à jour insère, se lit en entier : autant d'enfants (`children`, qui s'arrête au premier
/// qui n'est pas un nœud XML) que de longueur (Ruling R497).
///
/// yrs donne les types changés suivis chacun de ses ancêtres jusqu'à la racine
/// (`changed_parent_types`) : un type est changé lui-même s'il n'est pas le parent de celui qui le
/// précède. Seuls ceux-là sont recomptés ; recompter les ancêtres faisait recompter le `blockGroup`
/// racine à chaque frappe (frappe vue par un autre à 5,2 ms au p95 sur 10 000 blocs, et la
/// relecture de 499 frappes à 216 ms). Une liste ne change que si un enfant y entre ou en sort :
/// elle est alors elle-même changée.
fn lists_are_whole(txn: &TransactionMut, shape: &Shape) -> bool {
    let mut seen = HashSet::new();
    changed_themselves(txn)
        .chain(
            (!shape.untyped_in_texts())
                .then(|| shape.inserted_elements(txn))
                .into_iter()
                .flatten(),
        )
        .filter(|branch| seen.insert(*branch))
        .all(|branch| match branch.type_ref() {
            TypeRef::XmlElement(_) => {
                let element = XmlElementRef::from(branch);
                element.children(txn).count() as u32 == element.len(txn)
            }
            TypeRef::XmlFragment => {
                let fragment = XmlFragmentRef::from(branch);
                fragment.children(txn).count() as u32 == fragment.len(txn)
            }
            _ => true,
        })
}

/// Le type parent d'un type (`None` pour une racine, ou un parent qui n'est pas XML).
fn parent_of(branch: BranchPtr) -> Option<BranchPtr> {
    let parent = XmlElementRef::from(branch).parent()?;
    let parent: &Branch = match &parent {
        XmlOut::Element(e) => e.as_ref(),
        XmlOut::Fragment(f) => f.as_ref(),
        XmlOut::Text(t) => t.as_ref(),
    };
    Some(BranchPtr::from(parent))
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
        txn.commit();
        assert_eq!(verify_carried(&update, &before, &txn), Ok(()));
        drop(txn);

        // Trop lourde (C1e).
        let mut txn = page.doc.transact_mut();
        let before = txn.state_vector();
        text.insert(&mut txn, 0, &"x".repeat(MAX_CARRIED_BYTES + 1));
        let update = txn.encode_update_v1();
        txn.commit();
        assert!(verify_carried(&update, &before, &txn).is_err());
        drop(txn);

        // Hors de la page (C1d) : une racine que la garde refuse.
        let other = page.doc.get_or_insert_xml_fragment("autre");
        let mut txn = page.doc.transact_mut();
        let before = txn.state_vector();
        other.push_back(&mut txn, XmlTextPrelim::new("hors de la page"));
        let update = txn.encode_update_v1();
        txn.commit();
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
        txn.commit();
        assert_eq!(
            verify_carried(&update, &before, &txn),
            Err(CrdtError::TooDeep.to_string())
        );
    }

    /// Ruling R536 : la relecture d'un report refuse aussi ce que la garde refuse d'un client :
    /// une marque hors du schéma ou trop longue (R431), un contenu sans type dont le parent est
    /// nommé (R496), ou posé par l'origine d'un voisin parmi les enfants d'un élément (R497) ; elle
    /// laisse passer une marque du schéma.
    #[test]
    fn verify_carried_refuses_what_a_client_update_would_refuse() {
        use yrs::{Text, XmlTextPrelim};

        let page = page(7);
        page.fill(&mut page.doc.transact_mut(), &[empty_paragraph()]);
        let text = {
            let mut txn = page.doc.transact_mut();
            let root = page.root(&txn).unwrap();
            let text = root.push_back(&mut txn, XmlTextPrelim::new(""));
            text.insert(&mut txn, 0, "texte");
            text
        };
        let format = |key: &str, value: Any| {
            let mut txn = page.doc.transact_mut();
            let before = txn.state_vector();
            let mut attrs = yrs::types::Attrs::new();
            attrs.insert(key.into(), value);
            text.format(&mut txn, 0, 2, attrs);
            let update = txn.encode_update_v1();
            txn.commit();
            verify_carried(&update, &before, &txn)
        };
        let href = |n: usize| {
            Any::from(HashMap::from([(
                "href".to_string(),
                Any::from("a".repeat(n)),
            )]))
        };
        assert_eq!(
            format("bold", Any::from(HashMap::<String, Any>::new())),
            Ok(())
        );
        assert_eq!(format("link", href(100)), Ok(()));
        assert!(format("x", Any::Bool(true)).is_err(), "hors du schéma");
        assert!(format("link", href(20_000)).is_err(), "trop longue");

        // Une chaîne d'un client neuf, faite à la main : par l'origine (après le premier élément
        // du fragment, la racine), ou parent nommé (en tête du fragment).
        let untyped = |client: u64, origin: Option<(u64, u32)>| {
            let mut bytes = vec![1, 1];
            let var = |out: &mut Vec<u8>, mut v: u64| {
                while v > 0x7F {
                    out.push(0x80 | (v & 0x7F) as u8);
                    v >>= 7;
                }
                out.push(v as u8);
            };
            var(&mut bytes, client);
            var(&mut bytes, 0);
            match origin {
                None => {
                    bytes.push(4);
                    var(&mut bytes, 1);
                    var(&mut bytes, FRAGMENT.len() as u64);
                    bytes.extend_from_slice(FRAGMENT.as_bytes());
                }
                Some((client, clock)) => {
                    bytes.push(4 | 0x80);
                    var(&mut bytes, client);
                    var(&mut bytes, clock as u64);
                }
            }
            var(&mut bytes, 1);
            bytes.push(b'x');
            bytes.push(0);
            let mut txn = page.doc.transact_mut();
            let before = txn.state_vector();
            txn.apply_update(Update::decode_v1(&bytes).unwrap())
                .unwrap();
            let update = txn.encode_update_v1();
            txn.commit();
            let verdict = verify_carried(&update, &before, &txn);
            drop(txn);
            verdict
        };
        let first = {
            let txn = page.doc.transact();
            let Some(XmlOut::Element(first)) = page.fragment.get(&txn, 0) else {
                panic!("la racine");
            };
            let yrs::branch::BranchID::Nested(id) =
                AsRef::<yrs::branch::Branch>::as_ref(&first).id()
            else {
                panic!("un élément");
            };
            (id.client.get(), id.clock)
        };
        assert!(
            untyped(4242, Some(first)).is_err(),
            "après la racine, par l'origine"
        );
        assert!(untyped(4243, None).is_err(), "en tête du fragment");
    }

    /// Ruling R692 : la relecture d'un report refuse aussi un attribut que la garde des attributs
    /// refuserait d'un client, sur un élément de la page (lu dans la page) comme sur un élément
    /// que le report insère (lu dans la mise à jour) ; elle laisse passer un attribut du registre.
    #[test]
    fn verify_carried_refuses_an_attribute_of_another_type() {
        let page = page(7);
        page.fill(&mut page.doc.transact_mut(), &[empty_paragraph()]);
        let object = || {
            Any::from(HashMap::from([(
                "toString".to_string(),
                Any::Number(Number::Int(1)),
            )]))
        };
        let paragraph = |txn: &TransactionMut| -> XmlElementRef {
            let root = page.root(txn).unwrap();
            let Some(XmlOut::Element(container)) = root.get(txn, 0) else {
                panic!("un bloc");
            };
            let Some(XmlOut::Element(paragraph)) = container.get(txn, 0) else {
                panic!("un paragraphe");
            };
            paragraph
        };
        let verdict = |write: &dyn Fn(&mut TransactionMut)| {
            let mut txn = page.doc.transact_mut();
            let before = txn.state_vector();
            write(&mut txn);
            let update = txn.encode_update_v1();
            txn.commit();
            verify_carried(&update, &before, &txn)
        };
        assert_eq!(
            verdict(&|txn| {
                paragraph(txn).insert_attribute(txn, "backgroundColor", "red");
            }),
            Ok(())
        );
        let over = verdict(&|txn| {
            paragraph(txn).insert_attribute(txn, "backgroundColor", object());
        });
        assert!(
            over.as_ref()
                .is_err_and(|why| why.contains("backgroundColor")),
            "{over:?}"
        );
        let fresh = verdict(&|txn| {
            let root = page.root(txn).unwrap();
            let container = root.push_back(txn, XmlElementPrelim::empty("blockContainer"));
            let heading = container.push_back(txn, XmlElementPrelim::empty("heading"));
            heading.insert_attribute(txn, "level", object());
        });
        assert!(
            fresh.as_ref().is_err_and(|why| why.contains("level")),
            "{fresh:?}"
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
