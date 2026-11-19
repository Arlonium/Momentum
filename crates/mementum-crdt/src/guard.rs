//! Garde de profondeur (Ruling R17).
//!
//! Dans yrs 0.28, l'intégration d'une mise à jour est plate, mais la
//! suppression (`TransactionMut::delete`) et le ramasse-miettes
//! (`ItemContent::gc`) récursent sur la profondeur de l'arbre : la
//! suppression d'un arbre de quelques milliers de niveaux fait déborder la
//! pile et tue le processus. Une page ne garde donc jamais un arbre plus
//! profond que [`MAX_YJS_DEPTH`].
//!
//! Une mesure faite après `apply_update` arriverait trop tard dans trois cas,
//! où la récursion a lieu pendant l'application elle-même :
//! - une mise à jour qui insère un arbre profond et le supprime aussitôt ;
//! - un arbre profond inséré sous un élément déjà supprimé (le ramasse-miettes
//!   le parcourt à la validation) ;
//! - des mises à jour en attente de dépendances, qui s'intègrent toutes
//!   ensemble à l'arrivée de la dernière.
//!
//! [`Shape`] lit donc la mise à jour avant yrs, sans récursion, et refuse
//! une chaîne d'éléments imbriqués plus profonde que la borne à elle seule,
//! ou un type ou une racine hors du schéma de la page. Elle lit aussi le jeu
//! de suppressions, dont elle refuse les identifiants de client hors de
//! 53 bits et les plages qui débordent (yrs paniquerait en debug, Ruling
//! R25). Après l'application :
//! - des blocs laissés en attente (une dépendance absente) sont un refus, et
//!   le document est jeté : rien ne s'intègre plus tard en bloc. Comme yrs,
//!   on accepte en revanche un bloc arrivé avant ses prédécesseurs du même
//!   client quand ses dépendances sont là (yrs comble le trou d'un `Skip`, et
//!   son vecteur d'état s'arrête au trou, que les pairs renvoient) ;
//! - la profondeur des éléments insérés est mesurée en remontant de chacun
//!   d'eux jusqu'à la racine. La page n'ayant jamais d'arbre trop profond, la
//!   nouvelle profondeur maximale est celle d'un élément inséré.
//!
//! Pendant l'application, la récursion reste sous deux fois la borne (test
//! `twice_the_bound_in_one_update_is_refused` de `tests/depth.rs`).
//!
//! # Trous d'horloge (Ruling R131)
//!
//! Un bloc arrivé avant ses prédécesseurs du même client, yrs l'intègre en
//! laissant un trou ; un navigateur le garde en attente tant que le trou
//! n'est pas comblé, y compris dans l'état complet que le serveur lui donne.
//! Le serveur, l'API MMD et les agents liraient alors un texte qu'aucun
//! navigateur n'affiche. [`Shape::leaves_gap`] dit, sans rien appliquer, si
//! une mise à jour laisserait un tel trou, et [`Shape::awaited`] ce qu'elle
//! attend pour ne plus en laisser : l'acteur de synchronisation retient la
//! mise à jour d'un client jusque-là. `PageDoc::apply_update`, qui relit le
//! stockage, accepte toujours un tel bloc, comme yrs : ce qui est écrit doit
//! se relire, dans n'importe quel ordre (test `revision_is_canonical`).
//!
//! # Suppressions à venir (Rulings R198, R199)
//!
//! Une suppression qui vise des horloges que la page n'a pas encore, yrs la
//! garde en attente, et l'applique à l'arrivée des éléments : dix octets sans
//! bloc, `{client de Mona : [son horloge, + 1 000 000)}`, supprimaient tout ce
//! que Mona tapait ensuite, sur le serveur et chez tous, sans que rien en reste
//! dans aucune version. Elle est donc un trou comme un autre : [`Shape::awaited`]
//! rend aussi, par client, la fin de ce qu'elle supprime au-delà de ce que la
//! page aura une fois les blocs de la même mise à jour intégrés. Les plages que
//! la page garde déjà en attente (une base d'avant R198) n'attendent rien : yrs
//! comme Yjs les mettent dans l'état complet et dans l'étape 2, et chaque client
//! les renvoie dans la sienne ([`Pending`], Ruling R199).
//!
//! # Clients répétés (Ruling R207)
//!
//! yrs met bout à bout les entrées d'un même client, et écrit un bloc qui ne prolonge pas le
//! précédent par-dessus le bloc connu à sa place (`BlockStore::push`, qui croit remplacer un
//! `Skip`) : deux entrées pour le client qui a écrit la racine faisaient de la racine une plage
//! ramassée, la page se vidait chez tous puis pour de bon au compactage, et une plage plus longue
//! faisait paniquer yrs en debug. Aucun encodeur honnête n'écrit deux entrées pour un client :
//! Yjs et yrs en écrivent une, rangées par client décroissant, et leurs fusions aussi.
//! [`Shape::read`] les refuse, pour ce qui vient d'un client comme pour ce qui est relu. En
//! filet de R198, `PageDoc::apply_client_update` refuse aussi une mise à jour qui laisse une
//! suppression en attente que la page n'avait pas ([`Pending::lacks`]).
//!
//! # Métadonnées (report M7 du lot 1)
//!
//! La `Y.Map` [`META`] porte `schemaVersion`, que seul le serveur écrit, à la
//! création de la page. La mise à jour d'un client qui y touche est refusée
//! (`PageDoc::apply_client_update`) ; ce qui est relu du stockage, dont la
//! mise à jour du serveur qui pose la version, ne l'est pas
//! (`PageDoc::apply_update`), sans quoi aucune page ne se rechargerait.
//!
//! Lire la mise à jour ne suffit pas ici : l'entrée qui en écrase une autre
//! ne nomme pas son parent (elle le tient de l'entrée écrasée, son origine),
//! et une suppression ne nomme que des identifiants. [`touched`] demande donc
//! à yrs, la transaction validée, quels types elle a changés.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mementum_md::{MAX_COLOR_BYTES, MAX_DEPTH, MAX_HREF_BYTES};
use yrs::block::{
    BLOCK_GC_REF_NUMBER, BLOCK_ITEM_ANY_REF_NUMBER, BLOCK_ITEM_BINARY_REF_NUMBER,
    BLOCK_ITEM_DELETED_REF_NUMBER, BLOCK_ITEM_DOC_REF_NUMBER, BLOCK_ITEM_EMBED_REF_NUMBER,
    BLOCK_ITEM_FORMAT_REF_NUMBER, BLOCK_ITEM_JSON_REF_NUMBER, BLOCK_ITEM_STRING_REF_NUMBER,
    BLOCK_ITEM_TYPE_REF_NUMBER, BLOCK_SKIP_REF_NUMBER, ClientID, HAS_ORIGIN, HAS_PARENT_SUB,
    HAS_RIGHT_ORIGIN,
};
use yrs::branch::{BranchID, BranchPtr};
use yrs::encoding::read::{Cursor, Read};
use yrs::types::{TYPE_REFS_XML_ELEMENT, TYPE_REFS_XML_TEXT};
use yrs::{ID, IdSet, MapRef, ReadTxn, StateVector, TransactionMut, XmlOut};

use crate::attrs::{self, Expectations, Key, Value};
use crate::blocknote::BLOCK_CONTAINER;
use crate::structure::depth_in_page;
use crate::{CrdtError, FRAGMENT, META};

pub(crate) mod cut;
mod grammar;
pub(crate) mod runs;

pub(crate) use grammar::{
    MANY_VALUED, MANY_VALUED_MADE, NOT_AS_WRITTEN, RIGHT_ORIGIN_IS_ORIGIN, json_made,
};

/// Le refus d'une mise à jour où un client paraît deux fois (Ruling R207) : reconnu à la
/// relecture du journal, qui écarte l'entrée au lieu de refuser la page (Ruling R258).
pub(crate) const REPEATED_CLIENT: &str = "un client paraît deux fois dans la mise à jour";

/// Profondeur maximale d'un élément Yjs dans une page (la racine `blockGroup`
/// est à la profondeur 1).
///
/// Un bloc de niveau `n` a son `blockContainer` à la profondeur `2n`
/// (`blockGroup > blockContainer` par niveau ; une colonne coûte moins) ; son
/// contenu le plus profond, un tableau, descend jusqu'à `2n + 5` (`table >
/// tableRow > cellule > tableParagraph > texte`). Pour les [`MAX_DEPTH`]
/// niveaux du MMD, cela fait 205 ; la marge de 11 couvre le contenu d'un
/// nœud étranger. Un arbre de cette profondeur se supprime sans risque sur
/// une pile de 2 Mio, même en debug.
pub const MAX_YJS_DEPTH: usize = 2 * MAX_DEPTH + 16;

/// Imbrication maximale d'une valeur `Any` lue (yrs la décode par récursion).
const MAX_ANY_NESTING: usize = 64;

/// Plus grand identifiant de client que yrs accepte (53 bits).
const MAX_CLIENT: u64 = (1 << 53) - 1;

fn decode_error(message: impl std::fmt::Display) -> CrdtError {
    CrdtError::Decode(message.to_string())
}

/// Identifiant Yjs : client et horloge.
type Id = (u64, u32);

/// Où un élément s'insère.
enum Parent {
    /// Racine nommée ; vrai si l'élément est une entrée de map (`parentSub`).
    Named(String, bool),
    /// Type parent désigné par l'id de son élément.
    Id(Id),
    /// Même parent que cet élément voisin (origine gauche, sinon droite).
    Sibling(Id),
}

/// Un bloc de la mise à jour, dans l'ordre de son client : son id, sa
/// longueur Yjs et sa nature.
struct Struct {
    id: Id,
    len: u32,
    kind: Kind,
}

// Presque tous les blocs d'une mise à jour sont des insertions : les mettre en boîte ajouterait
// une allocation par bloc, pour n'alléger que les plages ramassées et les trous.
#[allow(clippy::large_enum_variant)]
enum Kind {
    Insert(Insert),
    /// Plage déjà ramassée (GC) : connue, sans contenu.
    Gc,
    /// Trou dans la mise à jour.
    Skip,
}

/// Un élément inséré par la mise à jour.
struct Insert {
    parent: Parent,
    /// Référence de type Yjs, pour un élément qui porte un type.
    type_ref: Option<u8>,
    /// La balise d'un élément XML (`blockContainer`…), pour reconnaître un bloc recréé (Task
    /// 13b, voir le module `moves`).
    tag: Option<String>,
    /// La valeur d'une entrée `id` posée sur un type désigné par son id (l'attribut `id` d'un
    /// élément XML), quand c'est une seule chaîne : l'identifiant de bloc qu'il prend.
    block_id: Option<String>,
    /// Pour une marque de texte que la page ne garde pas (Ruling R431) : pourquoi.
    mark: Option<MarkRefusal>,
    /// Un contenu compté qui n'est pas un type : une chaîne, une valeur `Any`, du JSON, du binaire,
    /// un objet incorporé, un sous-document (Ruling R496). Il n'a sa place que dans un texte.
    untyped: bool,
    /// Une entrée de map (`parentSub`) : un attribut d'un élément, pas un enfant.
    keyed: bool,
    /// Pour une entrée de map posée sur un type désigné par son id (un attribut d'élément) : sa
    /// clé (Ruling R689).
    attr: Option<Key>,
    /// Ce que le contenu vaudrait comme valeur d'attribut (la dernière, s'il en porte
    /// plusieurs) ; `None` pour un contenu supprimé.
    value: Option<Value>,
    /// Le contenu est une valeur `Any` : la seule forme qu'un client écrit pour un attribut.
    any: bool,
    /// Le contenu est du JSON (`ContentJSON`) : yrs en rend la chaîne, Yjs `JSON.parse` de la
    /// chaîne (Ruling R702).
    json: bool,
    /// L'élément porte une origine droite : une entrée de map écrite par un client n'en a pas
    /// (Ruling R703).
    right_origin: bool,
    /// Son origine droite, telle que les octets la disent (son origine, ou à défaut son origine
    /// droite, est dans `parent` quand il ne nomme pas son parent).
    right: Option<Id>,
    /// Son origine, telle que les octets la disent (Ruling R825).
    origin: Option<Id>,
    /// L'octet d'en-tête porte la marque « entrée de map » (`parentSub`), que l'élément nomme sa
    /// clé ou non. Un client hostile la pose ou l'efface à son gré ; dans ce que yrs encode
    /// lui-même, elle dit si yrs tient l'élément pour une entrée de map (Ruling R723).
    sub_flag: bool,
    /// Le genre de son contenu, tel que l'octet d'en-tête le dit (`BLOCK_ITEM_*_REF_NUMBER`).
    content: u8,
    /// Son origine droite est son origine (Ruling R802).
    right_is_origin: bool,
}

/// Le refus d'un contenu qui n'est pas un nœud XML parmi les enfants de la racine ou d'un élément
/// (Ruling R496).
pub(crate) const UNTYPED_CHILD: &str = "la mise à jour pose un contenu qui n'est pas un nœud XML \
     parmi les enfants de la racine ou d'un élément (seul un texte porte des caractères)";

/// Une marque de texte (`ItemContent::Format`) que la page ne garde pas (Ruling R431).
pub(crate) enum MarkRefusal {
    /// Une clé hors du schéma des blocs (ses premiers caractères, pour le message).
    Unknown(String),
    /// Une valeur plus longue que sa borne.
    TooLong {
        key: &'static str,
        bytes: usize,
        bound: usize,
    },
}

impl std::fmt::Display for MarkRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarkRefusal::Unknown(key) => write!(
                f,
                "la mise à jour pose une marque de texte hors du schéma des blocs (« {key} »)"
            ),
            MarkRefusal::TooLong { key, bytes, bound } => write!(
                f,
                "la mise à jour pose une marque « {key} » trop longue ({bytes} octets, pour \
                 {bound} au plus)"
            ),
        }
    }
}

/// Ce que porte la valeur d'une marque de texte du schéma.
#[derive(Clone, Copy)]
pub(crate) enum Carried {
    /// Rien : `{}` (ou `null`, qui retire la marque).
    Nothing,
    /// Une couleur, `{"stringValue": "…"}` : [`MAX_COLOR_BYTES`] octets au plus.
    Color,
    /// Une adresse, `{"href": "…"}` : [`MAX_HREF_BYTES`] octets au plus.
    Href,
}

/// Les marques de texte qu'une page garde (Ruling R431) : celles que BlockNote 0.57 écrit, les
/// styles du registre (`mementum_schema::STYLES`) et le lien. y-prosemirror écrit chacune sous le
/// nom de son type (aucune ne se recouvre : test `marks_are_those_of_the_server` du client).
pub(crate) const MARKS: &[(&str, Carried)] = &[
    ("bold", Carried::Nothing),
    ("italic", Carried::Nothing),
    ("underline", Carried::Nothing),
    ("strike", Carried::Nothing),
    ("code", Carried::Nothing),
    ("textColor", Carried::Color),
    ("backgroundColor", Carried::Color),
    ("link", Carried::Href),
];

/// Ce qu'une valeur de marque, en JSON tel que la mise à jour l'écrit, peut prendre au-delà de ce
/// qu'elle porte : `{"stringValue":""}` (18 octets), `{"href":""}` (11), `{}`, `null`.
const MARK_VALUE_SLACK: usize = 32;

/// Octets de JSON au plus par octet de la chaîne qu'une valeur de marque porte : le pire
/// échappement (`\u0061` pour `a`, `\u0001` pour un caractère de contrôle), dont la garde se sert
/// pour borner le JSON brut avant de le lire (Ruling R537).
const WORST_ESCAPE: usize = 6;

/// Caractères d'une clé inconnue cités dans un refus.
const QUOTED_KEY_CHARS: usize = 40;

/// La marque de texte du schéma de clé `key` ([`MARKS`]), ou le refus d'une clé inconnue (Ruling
/// R431), qui n'en cite que les premiers caractères.
fn known_mark(key: &str) -> Result<(&'static str, Carried), MarkRefusal> {
    match MARKS.iter().find(|(k, _)| *k == key) {
        Some(&mark) => Ok(mark),
        None => {
            let end = key
                .char_indices()
                .nth(QUOTED_KEY_CHARS)
                .map_or(key.len(), |(i, _)| i);
            let cut = if end < key.len() { "…" } else { "" };
            Err(MarkRefusal::Unknown(format!("{}{cut}", &key[..end])))
        }
    }
}

/// Pourquoi la page ne garde pas une marque du schéma de valeur `value` (JSON), ou `None` si elle
/// la garde (Ruling R431) : une valeur plus longue que sa borne. Une valeur illisible est laissée à
/// yrs, qui la lit avec le même `serde_json`, et refuse alors la mise à jour.
///
/// La borne se juge sur la chaîne que la valeur porte, désechappée, comme `parse` et l'éditeur la
/// jugent (Ruling R537) : l'échappement JSON allonge une chaîne (`\"`, `\\`, `\u0001`), et une
/// adresse de 16 000 octets dont 1 000 guillemets, acceptée par `PUT /mmd`, disparaissait au
/// rechargement suivant. Le JSON brut est borné d'abord, avant d'être lu, à
/// [`WORST_ESCAPE`] fois la borne plus [`MARK_VALUE_SLACK`] ; une marque qui ne porte rien (le
/// gras…) garde ainsi sa borne de 32 octets.
pub(crate) fn value_refusal(
    (name, carried): (&'static str, Carried),
    value: &str,
) -> Option<MarkRefusal> {
    let (field, bound) = match carried {
        Carried::Nothing => (None, 0),
        Carried::Color => (Some("stringValue"), MAX_COLOR_BYTES),
        Carried::Href => (Some("href"), MAX_HREF_BYTES),
    };
    let raw = bound * WORST_ESCAPE + MARK_VALUE_SLACK;
    if value.len() > raw {
        return Some(MarkRefusal::TooLong {
            key: name,
            bytes: value.len(),
            bound: raw,
        });
    }
    // Ce que porte la valeur : la chaîne sous `field`, ou la valeur elle-même si c'est une chaîne
    // (`mementum-crdt` lit l'une et l'autre).
    let carried = match (field, serde_json::from_str::<serde_json::Value>(value)) {
        (Some(_), Ok(serde_json::Value::String(s))) => s.len(),
        (Some(field), Ok(serde_json::Value::Object(map))) => match map.get(field) {
            Some(serde_json::Value::String(s)) => s.len(),
            _ => 0,
        },
        _ => 0,
    };
    (carried > bound).then_some(MarkRefusal::TooLong {
        key: name,
        bytes: carried,
        bound,
    })
}

/// Au plus tant d'entrées de la page cherchées, par mise à jour, pour l'origine d'une valeur qui
/// ne nomme ni son élément ni sa clé ([`Shape::attrs`]) : la recherche suit les voisins de
/// l'origine, et une mise à jour hostile en demanderait autant qu'elle porte d'éléments. Au-delà,
/// l'attribut est jugé dans la page, après l'application.
const MAX_PAGE_LOOKUPS: usize = 64;

/// Le refus d'un contenu JSON (Ruling R702).
pub(crate) const JSON_CONTENT: &str = "la mise à jour porte un contenu JSON, qu'aucun client \
     n'écrit et que le serveur et les navigateurs ne lisent pas pareil";

/// Le refus d'une valeur qui porte une origine droite (Ruling R703).
pub(crate) const VALUE_WITH_RIGHT_ORIGIN: &str = "la mise à jour pose une valeur d'attribut qui \
     porte une origine droite : une entrée de map n'en a pas";

/// Le refus d'une entrée de map qui porte une origine droite (Ruling R723).
pub(crate) const KEYED_RIGHT_ORIGIN: &str = "la mise à jour pose une entrée de map qui porte une \
     origine droite : aucun client n'en écrit, et le serveur ne la range pas comme les navigateurs";

/// Le même refus, fait après l'application, d'après ce que yrs a fait de l'élément : la page
/// est alors à jeter, et qui relit un journal ne peut pas écarter l'entrée (Ruling R742).
pub(crate) const KEYED_RIGHT_ORIGIN_MADE: &str = "la mise à jour pose, pour le serveur, une \
     entrée de map qui porte une origine droite : aucun client n'en écrit";

/// Le refus d'un élément à cheval sur ce que la page connaît, quand la page ne sait pas le couper
/// (Ruling R784) : du JSON à plusieurs valeurs, qu'aucun client n'écrit.
pub(crate) const ASTRIDE: &str = "la mise à jour porte un contenu JSON à cheval sur ce que la page \
     connaît de son client : aucun client n'en écrit, et le serveur et les navigateurs ne le \
     couperaient pas pareil";

/// Le refus d'une mise à jour de client dont les blocs laisseraient un trou (Ruling R786).
pub(crate) const LEAVES_GAP: &str = "la mise à jour laisserait un trou dans les horloges d'un \
     client : elle attend des modifications que la page n'a pas reçues";

/// Le refus d'une entrée de map dont le contenu n'est pas une valeur (Rulings R702, R724).
pub(crate) const KEYED_NOT_A_VALUE: &str = "la mise à jour pose comme valeur d'un attribut un \
     contenu qui n'en est pas une (seule une valeur JSON en est une)";

/// Le refus d'une valeur placée par une origine qui n'est pas dans un élément (Ruling R704).
pub(crate) const VALUE_OUTSIDE_ELEMENTS: &str = "la mise à jour pose une valeur d'attribut à la \
     suite d'un contenu de texte";

/// La plus grande horloge qu'une page garde (Ruling R808) : yrs les compare en `i32`.
const MAX_CLOCK: u32 = i32::MAX as u32;

/// Le refus d'une mise à jour dont une horloge dépasse [`MAX_CLOCK`] (Ruling R808).
pub(crate) const CLOCK_OUT_OF_BOUNDS: &str = "la mise à jour porte une horloge au-delà de 2^31, \
     que la page ne sait pas ranger";

/// Le refus d'un élément qui dépend d'une horloge que la page n'a pas, dans une page qui a un trou
/// (Ruling R807).
pub(crate) const DEPENDS_ON_A_HOLE: &str = "la mise à jour dépend d'une modification que la page \
     n'a pas, chez un client dont elle garde un trou";

/// Ce qu'est, dans la page, l'origine d'une valeur qui ne nomme ni son élément ni sa clé
/// ([`Shape::attrs`]).
pub(crate) enum PageItem {
    /// L'entrée courante de cet attribut d'un élément de cette balise.
    Entry(Arc<str>, Key),
    /// Un contenu d'un texte ou de la racine : pas un attribut.
    OutsideElements,
    /// Dans un élément, sans être l'entrée courante d'un attribut (une ancienne entrée, un
    /// enfant), ou inconnu de la page.
    Unknown,
}

/// Ce que [`Shape::attrs`] dit d'une mise à jour (Ruling R689).
#[derive(Default)]
pub(crate) struct AttrFindings {
    /// Le refus du premier attribut que la page ne garde pas.
    pub(crate) refusal: Option<String>,
    /// Les éléments, nommés par la mise à jour, qui reçoivent un tel attribut : relue du
    /// stockage, la page le leur retire.
    pub(crate) unsound: Vec<(u64, u32)>,
}

/// Ce que contient une mise à jour, lu sans l'appliquer.
pub(crate) struct Shape {
    clients: Vec<(u64, Vec<Struct>)>,
    deletions: Deletions,
    /// Le jeu de suppressions porte une plage de longueur nulle (Ruling R208).
    empty_deletion: bool,
}

/// Le jeu de suppressions d'une mise à jour : par client, ses plages `[début,
/// fin)`, sans les plages vides.
type Deletions = Vec<(u64, Vec<(u32, u32)>)>;

/// Les plages qu'une page garde déjà en attente de suppression, par client,
/// triées et disjointes (Ruling R199). Une page n'en a que si elle a été relue
/// d'une base d'avant le Ruling R198 : yrs les y garde, les met dans chaque
/// état complet et chaque étape 2 qu'il encode, et chaque client qui les a
/// reçues les renvoie dans son étape 2. Ce renvoi n'apporte rien à la page :
/// il n'attend rien pour elles.
#[derive(Default)]
pub(crate) struct Pending(HashMap<u64, Vec<(u32, u32)>>);

impl Pending {
    /// Les plages en attente d'un document (`Store::pending_ds` de yrs).
    pub(crate) fn of(set: Option<&IdSet>) -> Pending {
        let Some(set) = set.filter(|set| !set.is_empty()) else {
            return Pending::default();
        };
        Pending(
            set.iter()
                .map(|(client, ranges)| {
                    let ranges = ranges.iter().map(|r| (r.start, r.end)).collect();
                    (client.get(), ranges)
                })
                .collect(),
        )
    }

    /// Vrai si `after` (les suppressions en attente d'une page, après une écriture) porte une
    /// plage non vide que celles-ci ne couvrent pas : une suppression laissée en attente que la
    /// page n'avait pas (Ruling R207). Celles d'avant peuvent avoir rétréci (leurs éléments sont
    /// arrivés), jamais grandi.
    pub(crate) fn lacks(&self, after: Option<&IdSet>) -> bool {
        after.is_some_and(|after| {
            after.iter().any(|(client, ranges)| {
                ranges.iter().any(|r| {
                    r.start < r.end && self.uncovered_end(client.get(), r.start, r.end).is_some()
                })
            })
        })
    }

    /// La fin de ce qui, dans `[start, end)`, n'est pas déjà en attente pour
    /// `client` : `None` si rien n'en reste (ou si la plage est vide).
    fn uncovered_end(&self, client: u64, start: u32, end: u32) -> Option<u32> {
        // `[e, end)` est couvert ; on recule tant que des plages en attente le prolongent.
        let mut e = end;
        if let Some(ranges) = self.0.get(&client) {
            for &(r_start, r_end) in ranges.iter().rev() {
                if e <= start {
                    return None;
                }
                if r_start >= e {
                    continue;
                }
                if r_end < e {
                    return Some(e);
                }
                e = r_start;
            }
        }
        (e > start).then_some(e)
    }
}

impl Shape {
    /// Lit les blocs d'une mise à jour v1 comme `Update::decode` de yrs, mais
    /// sans récursion (une valeur `Any` trop imbriquée est refusée).
    pub(crate) fn read(update: &[u8]) -> Result<Shape, CrdtError> {
        Shape::read_as(update, JsonCount::AsRead)
    }

    /// [`Shape::read`], pour ce que yrs vient d'encoder lui-même (un état) : un contenu JSON y
    /// porte `len` chaînes (Ruling R815).
    pub(crate) fn read_written(update: &[u8]) -> Result<Shape, CrdtError> {
        Shape::read_as(update, JsonCount::AsWritten)
    }

    fn read_as(update: &[u8], json: JsonCount) -> Result<Shape, CrdtError> {
        let mut c = Cursor::new(update);
        let n_clients: u32 = c.read_var().map_err(decode_error)?;
        let mut clients = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..n_clients {
            let n_structs: u32 = c.read_var().map_err(decode_error)?;
            let client: u64 = c.read_var().map_err(decode_error)?;
            if client > MAX_CLIENT {
                return Err(decode_error("identifiant de client hors de 53 bits"));
            }
            // Ruling R207 : yrs met bout à bout les entrées d'un même client, et écrit par-dessus
            // un bloc connu celui qui ne prolonge pas le précédent. Aucun encodeur honnête n'écrit
            // deux entrées pour un client (Yjs et yrs en écrivent une, fusions comprises).
            if !seen.insert(client) {
                return Err(decode_error(REPEATED_CLIENT));
            }
            let mut clock: u32 = c.read_var().map_err(decode_error)?;
            let mut structs = Vec::new();
            for _ in 0..n_structs {
                let id = (client, clock);
                let Some(s) = read_struct(&mut c, id, json).map_err(decode_error)? else {
                    continue;
                };
                let len = s.len;
                clock = clock
                    .checked_add(len)
                    .ok_or_else(|| decode_error("horloge hors limites"))?;
                // Ruling R808 : yrs compare les horloges en entiers signés de 32 bits
                // (`Update::integrate` : `local_clock as i32 - id.clock as i32`). Au-delà de
                // 2^31, un bloc qui laisse un trou passe pour connu ou pour contigu : il est
                // intégré sans le `Skip` qui dit le trou, le vecteur d'état le tient pour
                // comblé, et une origine dans le trou fait paniquer yrs. Aucun client n'y
                // arrive : deux milliards d'unités écrites par un onglet.
                if clock > MAX_CLOCK {
                    return Err(CrdtError::Structure(CLOCK_OUT_OF_BOUNDS.to_string()));
                }
                structs.push(s);
            }
            clients.push((client, structs));
        }
        let (deletions, empty_deletion) = read_delete_set(&mut c)?;
        Ok(Shape {
            clients,
            deletions,
            empty_deletion,
        })
    }

    /// [`Shape::read`], pour une mise à jour reçue d'un client : refusée aussi (`Decode`) si son
    /// jeu de suppressions porte une plage de longueur nulle (Ruling R208). Elle ne supprime rien,
    /// mais yrs la garde en attente ; aucun encodeur honnête n'en écrit. Ce qui est relu du
    /// stockage passe par [`Shape::read`] seul : une telle plage, écrite au journal avant cette
    /// règle, ne rend pas la page illisible.
    pub(crate) fn read_from_client(update: &[u8]) -> Result<Shape, CrdtError> {
        let shape = Shape::read(update)?;
        if shape.empty_deletion {
            return Err(decode_error(
                "plage de suppressions de longueur nulle dans la mise à jour",
            ));
        }
        Ok(shape)
    }

    /// Le jeu de suppressions de la mise à jour : par client, ses plages `[début, fin)`, sans
    /// les plages vides.
    pub(crate) fn deletions(&self) -> &[(u64, Vec<(u32, u32)>)] {
        &self.deletions
    }

    /// Les `blockContainer` que la mise à jour insère et que la page n'a pas (`state`), avec
    /// l'identifiant de bloc que leur donne l'attribut `id` écrit dans la même mise à jour : les
    /// candidats d'un bloc recréé (Task 13b, module `moves`). Un élément dont l'attribut est
    /// écrit deux fois, ou un identifiant porté par deux éléments, n'en est pas un : la
    /// correspondance ne serait pas sûre. Rangés par identifiant de bloc.
    pub(crate) fn inserted_blocks(&self, state: &StateVector) -> Vec<(ID, String)> {
        let mut containers: HashMap<Id, Option<String>> = HashMap::new();
        let mut doubtful: HashSet<Id> = HashSet::new();
        for (s, i) in self.inserts() {
            if i.type_ref == Some(TYPE_REFS_XML_ELEMENT)
                && i.tag.as_deref() == Some(BLOCK_CONTAINER)
                && s.id.1 >= state.get(&ClientID::new(s.id.0))
            {
                containers.insert(s.id, None);
            }
        }
        if containers.is_empty() {
            return Vec::new();
        }
        // Les entrées `id` de ces éléments, par l'id de leur élément Yjs : une entrée qui en
        // écrase une autre ne nomme pas son élément (elle le tient de l'entrée écrasée, son
        // origine), et l'élément devient douteux.
        let mut entries: HashMap<Id, Id> = HashMap::new();
        for (s, i) in self.inserts() {
            match (&i.block_id, &i.parent) {
                (Some(value), Parent::Id(parent)) => {
                    if let Some(slot) = containers.get_mut(parent) {
                        if slot.is_some() {
                            doubtful.insert(*parent);
                        }
                        *slot = Some(value.clone());
                        entries.insert(s.id, *parent);
                    }
                }
                (_, Parent::Sibling(origin)) => {
                    if let Some(parent) = entries.get(origin).copied() {
                        doubtful.insert(parent);
                        entries.insert((s.id.0, s.id.1 + s.len - 1), parent);
                    }
                }
                _ => {}
            }
        }
        let mut by_value: HashMap<&str, Vec<Id>> = HashMap::new();
        for (id, value) in &containers {
            if let Some(value) = value
                && !value.is_empty()
                && !doubtful.contains(id)
            {
                by_value.entry(value.as_str()).or_default().push(*id);
            }
        }
        let mut found: Vec<(ID, String)> = by_value
            .into_iter()
            .filter(|(_, ids)| ids.len() == 1)
            .map(|(value, ids)| {
                let (client, clock) = ids[0];
                (ID::new(ClientID::new(client), clock), value.to_string())
            })
            .collect();
        found.sort_by(|a, b| a.1.cmp(&b.1));
        found
    }

    /// Vrai si la mise à jour insère des éléments XML (un bloc, un nœud de
    /// contenu…) : seule une telle mise à jour peut dupliquer un bloc.
    pub(crate) fn inserts_elements(&self) -> bool {
        self.inserts()
            .any(|(_, i)| i.type_ref == Some(TYPE_REFS_XML_ELEMENT))
    }

    fn inserts(&self) -> impl Iterator<Item = (&Struct, &Insert)> {
        self.clients.iter().flat_map(|(_, structs)| {
            structs.iter().filter_map(|s| match &s.kind {
                Kind::Insert(i) => Some((s, i)),
                _ => None,
            })
        })
    }

    /// Refuse, sans rien appliquer, une mise à jour qui sort du schéma de la
    /// page ou dont les éléments imbriqués dépassent déjà la borne. Les trous
    /// et les dépendances sont laissés à yrs (voir [`Shape::deeper_than_bound`]
    /// et `PageDoc::apply_update`).
    pub(crate) fn check(&self, state: &StateVector) -> Result<(), CrdtError> {
        for (client, structs) in &self.clients {
            let floor = state.get(&ClientID::new(*client));
            for s in structs {
                let Kind::Insert(i) = &s.kind else { continue };
                // Déjà connu : yrs l'ignore.
                if s.id.1 + s.len <= floor {
                    continue;
                }
                if let Parent::Named(root, sub) = &i.parent {
                    let allowed = match root.as_str() {
                        FRAGMENT => !sub,
                        META => i.type_ref.is_none(),
                        _ => false,
                    };
                    if !allowed {
                        return Err(CrdtError::Structure(format!(
                            "la mise à jour écrit hors de la page (racine « {root} »)"
                        )));
                    }
                }
                if let Some(t) = i.type_ref
                    && t != TYPE_REFS_XML_ELEMENT
                    && t != TYPE_REFS_XML_TEXT
                {
                    return Err(CrdtError::Structure(format!(
                        "type Yjs {t} inattendu dans une page BlockNote"
                    )));
                }
            }
        }
        if self.chain_depth() > MAX_YJS_DEPTH {
            return Err(CrdtError::TooDeep);
        }
        Ok(())
    }

    /// Refuse (`Structure`), sans rien appliquer, une mise à jour de client qui porte un contenu
    /// que yrs et Yjs ne lisent pas pareil, où qu'il soit (Rulings R702, R703) :
    /// - du JSON (`ContentJSON`) : comme valeur d'attribut, yrs en rend la chaîne et Yjs
    ///   `JSON.parse` de la chaîne, un objet ; aucun client ne l'écrit (Yjs ne le produit plus
    ///   depuis sa version 13, y-prosemirror jamais) ;
    /// - une valeur `Any` qui porte une origine droite : yrs recopie la clé de map du voisin de
    ///   droite quand celui de gauche n'en a pas, Yjs prend celle de gauche ; l'élément est alors
    ///   un attribut pour yrs et un enfant sans type pour Yjs. Un client n'écrit de valeur `Any`
    ///   que pour un attribut, et une entrée de map n'a jamais d'origine droite.
    ///
    /// Ce que la page connaît déjà (yrs l'ignore) n'y compte pas.
    pub(crate) fn check_contents(&self, state: &StateVector) -> Result<(), CrdtError> {
        for (s, insert) in self.inserts() {
            if !(insert.json || (insert.any && insert.right_origin))
                || s.id.1 + s.len <= state.get(&ClientID::new(s.id.0))
            {
                continue;
            }
            return Err(CrdtError::Structure(
                if insert.json {
                    JSON_CONTENT
                } else {
                    VALUE_WITH_RIGHT_ORIGIN
                }
                .to_string(),
            ));
        }
        Ok(())
    }

    /// Vrai si la mise à jour porte un élément **à cheval** sur ce que la page connaît : son
    /// horloge de départ est sous le plancher de son client (`floor`, le vecteur d'état), sa fin
    /// le dépasse. Un tel élément est récrit avant d'être lu par la garde et par yrs
    /// ([`cut::cut`], Ruling R784), qui n'en voient que la part neuve, placée par son origine.
    ///
    /// Un élément tout entier sous le plancher n'en est pas un : yrs l'ignore (Ruling R490).
    pub(crate) fn astride(&self, floor: &StateVector) -> bool {
        self.clients.iter().any(|(client, structs)| {
            let known = floor.get(&ClientID::new(*client));
            known > 0
                && structs.iter().any(|s| {
                    matches!(s.kind, Kind::Insert(_)) && s.id.1 < known && s.id.1 + s.len > known
                })
        })
    }

    /// Les clients dont la mise à jour porte des blocs.
    pub(crate) fn clients(&self) -> impl Iterator<Item = u64> + '_ {
        self.clients.iter().map(|(client, _)| *client)
    }

    /// Les clients chez qui les blocs de la mise à jour laisseraient un trou
    /// ([`Shape::structs_leave_gap`]).
    pub(crate) fn gapped_clients(&self, state: &StateVector) -> Vec<u64> {
        let mut gapped = Vec::new();
        for (client, structs) in &self.clients {
            let mut covered = state.get(&ClientID::new(*client));
            for s in structs.iter().filter(|s| !matches!(s.kind, Kind::Skip)) {
                if s.id.1 > covered {
                    gapped.push(*client);
                    break;
                }
                covered = covered.max(s.id.1 + s.len);
            }
        }
        gapped
    }

    /// Vrai si un élément que la mise à jour intègre (au-dessus de `floor`) dépend, par son
    /// origine, son origine droite ou son parent, d'un élément que ni la page (`known`) ni la
    /// mise à jour elle-même ne portent (Ruling R787) : yrs le garderait en attente
    /// (`missing_dependency`, `update.rs`), et la page serait à jeter.
    pub(crate) fn lacks_dependency(
        &self,
        floor: &StateVector,
        known: impl Fn(u64, u32) -> bool,
    ) -> bool {
        let brought: HashMap<u64, Vec<(u32, u32)>> = self
            .clients()
            .map(|client| (client, self.ranges_of(client)))
            .collect();
        let has = |(client, clock): Id| {
            known(client, clock)
                || brought.get(&client).is_some_and(|ranges| {
                    let at = ranges.partition_point(|range| range.1 <= clock);
                    ranges.get(at).is_some_and(|range| range.0 <= clock)
                })
        };
        self.inserts().any(|(s, i)| {
            if s.id.1 + s.len <= floor.get(&ClientID::new(s.id.0)) {
                return false;
            }
            // Son parent, ou le voisin qui le lui donne (son origine, à défaut son origine droite).
            let placed_by = match i.parent {
                Parent::Id(id) | Parent::Sibling(id) => Some(id),
                Parent::Named(..) => None,
            };
            [placed_by, i.right]
                .into_iter()
                .flatten()
                .any(|id| !has(id))
        })
    }

    /// Les plages `[début, fin)` que couvrent les blocs de `client` (ses trous exceptés), triées
    /// et fondues : lu dans ce que yrs encode d'une page, ce qu'elle connaît de ce client.
    pub(crate) fn ranges_of(&self, client: u64) -> Vec<(u32, u32)> {
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        for (_, structs) in self.clients.iter().filter(|(c, _)| *c == client) {
            for s in structs.iter().filter(|s| !matches!(s.kind, Kind::Skip)) {
                match ranges.last_mut() {
                    Some(last) if last.1 == s.id.1 => last.1 += s.len,
                    _ => ranges.push((s.id.1, s.id.1 + s.len)),
                }
            }
        }
        ranges
    }

    /// Refuse (`Structure`) une mise à jour où un élément intégré (au-dessus de `floor`) porte à la
    /// fois la marque « entrée de map » et une origine droite (Ruling R723). yrs donne à un
    /// élément la clé de son voisin de gauche et, s'il n'en a pas, celle de son voisin de
    /// droite ; Yjs ne prend que celle de gauche. Entre un enfant d'une liste et une entrée de
    /// map, l'élément devient pour yrs seul l'entrée courante de la clé : yrs supprime l'enfant
    /// de gauche, et l'ancienne entrée n'est plus tenue par rien ; le parent supprimé puis
    /// ramassé, elle garde un pointeur vers un parent libéré, et l'état encodé ensuite est fait
    /// d'octets au hasard, ou le processus s'arrête (le constat R722). Un client n'écrit jamais
    /// d'entrée de map à origine droite.
    ///
    /// La marque est celle des octets lus : d'un client, elle ne prouve rien (il l'efface) ; de
    /// ce que yrs a encodé lui-même (l'encodage d'une transaction, un état, une entrée du journal
    /// depuis R491), elle dit ce que yrs a fait de l'élément.
    pub(crate) fn check_keyed(&self, floor: &StateVector) -> Result<(), CrdtError> {
        let found = self.inserts().any(|(s, i)| {
            i.sub_flag && i.right_origin && s.id.1 + s.len > floor.get(&ClientID::new(s.id.0))
        });
        if found {
            return Err(CrdtError::Structure(KEYED_RIGHT_ORIGIN.to_string()));
        }
        Ok(())
    }

    /// Vrai si la mise à jour intègre (au-dessus de `floor`) un élément placé par un voisin (son
    /// origine, à défaut son origine droite) : ses octets ne disent pas s'il sera une entrée de
    /// map, yrs le décide d'après ce voisin (règle P1 de la grammaire, module `grammar`).
    pub(crate) fn placed_by_siblings(&self, floor: &StateVector) -> bool {
        self.inserts().any(|(s, i)| {
            matches!(i.parent, Parent::Sibling(_))
                && s.id.1 + s.len > floor.get(&ClientID::new(s.id.0))
        })
    }

    /// Les éléments que la mise à jour intègre (au-dessus de `floor`) et qui ne sont pas ce
    /// qu'un client écrit pour un attribut : un contenu qui n'est pas une valeur `Any`, ou une
    /// origine droite (Rulings R702, R703). S'il se trouve que l'un d'eux est l'entrée courante
    /// d'un attribut, la page ne le garde pas : yrs et Yjs n'y lisent pas la même valeur, ou pas
    /// à la même place. Une frappe en est un (une chaîne) : la page n'y regarde que pour un
    /// élément changé qui porte des attributs.
    pub(crate) fn not_attribute_values(&self, floor: &StateVector) -> Vec<ID> {
        self.inserts()
            .filter(|(s, i)| {
                i.value.is_some()
                    && (!i.any || i.right_origin)
                    && s.id.1 + s.len > floor.get(&ClientID::new(s.id.0))
            })
            // Ruling R721 : un élément à cheval sur le plancher n'est intégré que pour sa part
            // au-dessus, et c'est elle que la page porte, à l'horloge du plancher.
            .map(|(s, _)| {
                let client = ClientID::new(s.id.0);
                ID::new(client, s.id.1.max(floor.get(&client)))
            })
            .collect()
    }

    /// Refuse (`Structure`), sans rien appliquer, une mise à jour de client qui pose une marque de
    /// texte que la page ne garde pas (Ruling R431, voir [`MARKS`]). Une marque déjà connue de la
    /// page (yrs l'ignore) n'y compte pas.
    pub(crate) fn check_marks(&self, state: &StateVector) -> Result<(), CrdtError> {
        for (client, structs) in &self.clients {
            let floor = state.get(&ClientID::new(*client));
            for s in structs {
                if let Kind::Insert(Insert {
                    mark: Some(refusal),
                    ..
                }) = &s.kind
                    && s.id.1 + s.len > floor
                {
                    return Err(CrdtError::Structure(refusal.to_string()));
                }
            }
        }
        Ok(())
    }

    /// Refuse (`Structure`), sans rien appliquer, une mise à jour de client qui pose un contenu
    /// compté sans type (une chaîne, une valeur `Any`…) parmi les enfants de la racine ou d'un
    /// élément (Ruling R496) : `children` de yrs s'arrête au premier enfant qui n'est pas un nœud
    /// XML, et le serveur lisait la page vide. Jugé ici quand le parent est nommé : la racine, ou
    /// un type par son identifiant, lu dans la mise à jour ou dans la page (`page_type_is_text`).
    /// Un élément placé par son origine (un voisin) est jugé après l'intégration
    /// (`PageDoc::try_client_update`, Ruling R497). Une entrée de map (un attribut) n'est pas un
    /// enfant ; ce que la page connaît déjà (yrs l'ignore) n'y compte pas.
    pub(crate) fn check_untyped(
        &self,
        state: &StateVector,
        page_type_is_text: impl Fn(Id) -> Option<bool>,
    ) -> Result<(), CrdtError> {
        let types: HashMap<Id, u8> = self
            .inserts()
            .filter_map(|(s, i)| i.type_ref.map(|t| (s.id, t)))
            .collect();
        for (client, structs) in &self.clients {
            let floor = state.get(&ClientID::new(*client));
            for s in structs {
                let Kind::Insert(insert) = &s.kind else {
                    continue;
                };
                if !insert.untyped || insert.keyed || s.id.1 + s.len <= floor {
                    continue;
                }
                let in_text = match &insert.parent {
                    Parent::Named(root, _) => Some(root != FRAGMENT),
                    Parent::Id(parent) => match types.get(parent) {
                        Some(&t) => Some(t == TYPE_REFS_XML_TEXT),
                        None => page_type_is_text(*parent),
                    },
                    Parent::Sibling(_) => None,
                };
                if in_text == Some(false) {
                    return Err(CrdtError::Structure(UNTYPED_CHILD.to_string()));
                }
            }
        }
        Ok(())
    }

    /// Les contenus comptés sans type que la mise à jour intègre (au-dessus de `floor`, comme
    /// [`Shape::outside_marks`]), hors entrées de map : identifiant et longueur (Ruling R498).
    pub(crate) fn untyped_items(&self, floor: &StateVector) -> Vec<(ID, u32)> {
        self.inserts()
            .filter(|(s, i)| {
                i.untyped && !i.keyed && s.id.1 + s.len > floor.get(&ClientID::new(s.id.0))
            })
            // Ruling R721 : la part intégrée d'un élément à cheval sur le plancher, et elle seule.
            .map(|(s, _)| {
                let client = ClientID::new(s.id.0);
                let from = s.id.1.max(floor.get(&client));
                (ID::new(client, from), s.id.1 + s.len - from)
            })
            .collect()
    }

    /// Vrai si chaque contenu compté sans type de la mise à jour est, d'après elle seule, dans un
    /// texte : son parent est un `XmlText` qu'elle insère, ou son origine un tel contenu (ou une
    /// marque) qu'elle porte et qui l'est. Alors aucun élément qu'elle insère n'a d'enfant qui ne
    /// soit un nœud XML, et la relecture de tout un instantané (où le premier caractère de chaque
    /// texte nomme son texte, et chaque suivant a le précédent pour origine) n'a pas à recompter
    /// les enfants de chacun de ses éléments (Ruling R497).
    pub(crate) fn untyped_in_texts(&self) -> bool {
        let inserts: Vec<(&Struct, &Insert)> = self.inserts().collect();
        let mut by_client: HashMap<u64, Vec<(u32, u32, usize)>> = HashMap::new();
        for (k, (s, _)) in inserts.iter().enumerate() {
            by_client
                .entry(s.id.0)
                .or_default()
                .push((s.id.1, s.id.1 + s.len, k));
        }
        for ranges in by_client.values_mut() {
            ranges.sort_unstable();
        }
        let find = |(client, clock): Id| -> Option<usize> {
            let ranges = by_client.get(&client)?;
            let at = ranges.partition_point(|r| r.1 <= clock);
            ranges.get(at).filter(|r| r.0 <= clock).map(|&(_, _, k)| k)
        };
        // Pour chaque élément de la mise à jour : dans un texte, d'après elle seule ?
        let mut known: Vec<Option<bool>> = vec![None; inserts.len()];
        // Ruling R854 : le départ du parcours qui a vu chaque élément ; une boucle s'arrête au
        // premier élément revu.
        let mut walked = vec![usize::MAX; inserts.len()];
        for start in 0..inserts.len() {
            let (_, insert) = inserts[start];
            if !insert.untyped || insert.keyed {
                continue;
            }
            let mut path = Vec::new();
            let mut k = start;
            let in_text = loop {
                if let Some(answer) = known[k] {
                    break answer;
                }
                let (_, insert) = inserts[k];
                if insert.type_ref.is_some() || walked[k] == start {
                    break false;
                }
                walked[k] = start;
                path.push(k);
                match &insert.parent {
                    Parent::Named(..) => break false,
                    Parent::Id(parent) => {
                        break find(*parent)
                            .is_some_and(|p| inserts[p].1.type_ref == Some(TYPE_REFS_XML_TEXT));
                    }
                    Parent::Sibling(origin) => match find(*origin) {
                        Some(next) if next != k => k = next,
                        _ => break false,
                    },
                }
            };
            for k in path {
                known[k] = Some(in_text);
            }
            if !in_text {
                return false;
            }
        }
        true
    }

    /// Les éléments XML que la mise à jour insère, retrouvés dans la page (déjà appliquée).
    pub(crate) fn inserted_elements<'a, T: ReadTxn>(
        &'a self,
        txn: &'a T,
    ) -> impl Iterator<Item = BranchPtr> + 'a {
        self.inserts()
            .filter(|(_, i)| i.type_ref == Some(TYPE_REFS_XML_ELEMENT))
            .filter_map(move |(s, _)| {
                BranchID::get_nested(txn, &ID::new(ClientID::new(s.id.0), s.id.1))
            })
    }

    /// Ce que la garde des attributs dit de la mise à jour, lue seule (Ruling R689, module
    /// `attrs`) : le refus du premier attribut qu'une page ne garde pas, et les éléments qui en
    /// reçoivent un. Un attribut est une entrée de map posée sur un élément ; la mise à jour dit
    /// lequel, et sous quelle clé, de trois façons :
    /// - l'entrée nomme son élément et sa clé (la première valeur d'un attribut) : l'élément est
    ///   dans la mise à jour, ou dans la page (`page_tag` rend sa balise, `None` pour un texte) ;
    /// - l'entrée a pour origine une entrée de la mise à jour (elle l'écrase) : mêmes élément et
    ///   clé ;
    /// - l'entrée a pour origine un élément de la page, et ne nomme rien : `page_entry` le
    ///   cherche (l'entrée courante d'un attribut d'un élément), pour une valeur `Any` seulement
    ///   (la forme qu'un client écrit, et que rien d'autre ne pose par une origine : une frappe
    ///   est une chaîne), et [`MAX_PAGE_LOOKUPS`] fois au plus par mise à jour.
    ///
    /// Ce qui n'est pas reconnu ici (l'origine est une ancienne entrée, un contenu d'une autre
    /// forme placé par une origine de la page, au-delà du plafond) est jugé dans la page, après
    /// l'application (`PageDoc`, Ruling R691). Ce que la page connaît déjà (yrs l'ignore) n'y
    /// compte pas (Ruling R490). Rien n'est parcouru pour une mise à jour sans entrée de map ni
    /// valeur `Any` placée par une origine : le chemin d'une frappe.
    pub(crate) fn attrs(
        &self,
        floor: &StateVector,
        page_tag: impl Fn(Id) -> Option<Option<Arc<str>>>,
        mut page_entry: impl FnMut(Id) -> PageItem,
    ) -> AttrFindings {
        /// Ce qu'un élément de la mise à jour est, d'après elle seule.
        #[derive(Clone, Copy)]
        enum Place {
            /// Une entrée de map de cet élément, sous cette clé.
            Entry(Id, Key),
            /// Un enfant d'une liste, ou une entrée d'une racine.
            Child,
            /// Ce qu'est cet élément de la page, son origine.
            Outside(Id),
        }
        /// La balise d'un élément jugé, et ce que ses attributs doivent être.
        type Judged<'a> = Option<(Cow<'a, str>, Expectations)>;
        let mut findings = AttrFindings::default();
        if !self
            .inserts()
            .any(|(_, i)| i.attr.is_some() || (i.any && matches!(i.parent, Parent::Sibling(_))))
        {
            return findings;
        }
        let inserts: Vec<(&Struct, &Insert)> = self.inserts().collect();
        let mut by_client: HashMap<u64, Vec<(u32, u32, usize)>> = HashMap::new();
        for (k, (s, _)) in inserts.iter().enumerate() {
            by_client
                .entry(s.id.0)
                .or_default()
                .push((s.id.1, s.id.1 + s.len, k));
        }
        for ranges in by_client.values_mut() {
            ranges.sort_unstable();
        }
        let find = |(client, clock): Id| -> Option<usize> {
            let ranges = by_client.get(&client)?;
            let at = ranges.partition_point(|r| r.1 <= clock);
            ranges.get(at).filter(|r| r.0 <= clock).map(|&(_, _, k)| k)
        };
        let covers = |s: &Struct, (client, clock): Id| {
            s.id.0 == client && s.id.1 <= clock && clock - s.id.1 < s.len
        };
        // Ce que sont les éléments placés par une origine, une fois cherché ; le chemin d'une
        // recherche, repris d'un élément à l'autre (une allocation par élément coûtait 5 ms au
        // chargement d'une page de 10 000 blocs).
        let mut places: Vec<Option<Place>> = vec![None; inserts.len()];
        let mut path: Vec<usize> = Vec::new();
        // Ruling R854 : le départ du parcours qui a vu chaque élément.
        let mut walked = vec![usize::MAX; inserts.len()];
        // Le dernier élément nommé, sa balise et ce que ses attributs doivent être (`None` : pas
        // jugé) : les attributs d'un élément se suivent dans une mise à jour.
        let mut last: Option<(Id, Judged<'_>)> = None;
        let mut floor_of: (u64, u32) = (u64::MAX, 0);
        let mut lookups = 0;
        let mut looked_up: HashSet<Id> = HashSet::new();
        for start in 0..inserts.len() {
            let (s, insert) = inserts[start];
            let Some(value) = insert.value else { continue };
            if floor_of.0 != s.id.0 {
                floor_of = (s.id.0, floor.get(&ClientID::new(s.id.0)));
            }
            if s.id.1 + s.len <= floor_of.1 {
                continue;
            }
            let place = match (&insert.parent, insert.attr) {
                // Ruling R721 : à cheval sur le plancher, l'élément n'est intégré que pour sa
                // part au-dessus, qui a pour origine ce que la page porte juste avant elle, quoi
                // que la mise à jour dise de son parent ou de son origine.
                _ if s.id.1 < floor_of.1 => Place::Outside((s.id.0, floor_of.1 - 1)),
                (Parent::Id(parent), Some(key)) => Place::Entry(*parent, key),
                (Parent::Named(..) | Parent::Id(_), _) => continue,
                (Parent::Sibling(_), _) => {
                    path.clear();
                    let mut k = start;
                    let place = loop {
                        if let Some(place) = places[k] {
                            break place;
                        }
                        let (through, at) = inserts[k];
                        let under = floor.get(&ClientID::new(through.id.0));
                        if through.id.1 < under {
                            // Une origine dans un élément à cheval sur le plancher, ou sous lui.
                            break Place::Outside((through.id.0, under - 1));
                        }
                        match (&at.parent, at.attr) {
                            (Parent::Id(parent), Some(key)) => break Place::Entry(*parent, key),
                            (Parent::Named(..) | Parent::Id(_), _) => break Place::Child,
                            (Parent::Sibling(origin), _) => {
                                path.push(k);
                                // Une origine que la page connaît déjà : c'est son élément, quoi
                                // que la mise à jour porte à cette horloge (R490, R721).
                                if origin.1 < floor.get(&ClientID::new(origin.0)) {
                                    break Place::Outside(*origin);
                                }
                                // L'origine est le plus souvent l'élément d'avant.
                                let next = match k.checked_sub(1) {
                                    Some(before) if covers(inserts[before].0, *origin) => {
                                        Some(before)
                                    }
                                    _ => find(*origin),
                                };
                                walked[k] = start;
                                match next {
                                    // Un élément revu : la chaîne boucle, mise à jour invalide.
                                    Some(next) if walked[next] != start => k = next,
                                    Some(_) => break Place::Child,
                                    None => break Place::Outside(*origin),
                                }
                            }
                        }
                    };
                    for &k in &path {
                        places[k] = Some(place);
                    }
                    place
                }
            };
            if matches!(place, Place::Child) {
                continue;
            }
            match place {
                Place::Entry(parent, key) => {
                    if last.as_ref().map(|(id, _)| *id) != Some(parent) {
                        let tag = match find(parent) {
                            Some(at) if inserts[at].0.id == parent => {
                                inserts[at].1.tag.as_deref().map(Cow::Borrowed)
                            }
                            Some(_) => None,
                            None => page_tag(parent)
                                .flatten()
                                .map(|tag| Cow::Owned(tag.to_string())),
                        };
                        let judged = tag.and_then(|tag| Some((attrs::expectations(&tag)?, tag)));
                        last = Some((parent, judged.map(|(expected, tag)| (tag, expected))));
                    }
                    if let Some((_, Some((tag, expected)))) = &last
                        && !attrs::admitted(expected, key, value)
                    {
                        findings
                            .refusal
                            .get_or_insert_with(|| attrs::refusal(tag, key));
                        findings.unsound.push(parent);
                    }
                }
                Place::Outside(origin)
                    if insert.any
                        && lookups < MAX_PAGE_LOOKUPS
                        && findings.refusal.is_none()
                        && looked_up.insert(origin) =>
                {
                    lookups += 1;
                    match page_entry(origin) {
                        PageItem::Entry(tag, key) => {
                            if !attrs::sound(&tag, key, value) {
                                findings.refusal = Some(attrs::refusal(&tag, key));
                            }
                        }
                        // Ruling R704 : aucun client n'écrit une valeur `Any` dans un texte ou
                        // à la racine ; la chercher a suivi les voisins de son origine, et la
                        // mise à jour est refusée à la première.
                        PageItem::OutsideElements => {
                            findings.refusal = Some(VALUE_OUTSIDE_ELEMENTS.to_string());
                        }
                        PageItem::Unknown => {}
                    }
                }
                Place::Outside(_) | Place::Child => {}
            }
        }
        findings
    }

    /// Les marques de texte de la mise à jour que la page ne garde pas (Ruling R431) : relue du
    /// stockage, la page les supprime (`PageDoc::apply_update`). Seulement celles que la mise à
    /// jour intègre (Ruling R490) : au-dessus de `floor`, ce que la page avait de leur client
    /// avant elle ; yrs ignore une structure à une horloge connue, et l'élément qui porte cette
    /// horloge dans la page en est un autre.
    pub(crate) fn outside_marks(&self, floor: &StateVector) -> IdSet {
        let mut set = IdSet::new();
        for (s, insert) in self.inserts() {
            if insert.mark.is_some() && s.id.1 >= floor.get(&ClientID::new(s.id.0)) {
                set.insert(ID::new(ClientID::new(s.id.0), s.id.1), s.len);
            }
        }
        set
    }

    /// Vrai si la mise à jour, donnée à une page dont le vecteur d'état est
    /// `state`, laisserait un trou dans les horloges d'un de ses clients
    /// (Ruling R131). yrs intégrerait ses blocs en laissant le trou, quand
    /// leurs dépendances sont là ; un navigateur les garde en attente tant que
    /// le trou n'est pas comblé, y compris quand il les reçoit dans l'état
    /// complet du serveur. Le serveur lirait alors un texte qu'aucun
    /// navigateur n'affiche.
    ///
    /// Pour chaque client, dans l'ordre des horloges : ce que la page couvre
    /// part de son horloge dans `state` ; un bloc (élément ou plage ramassée)
    /// qui commence au-delà est un trou ; sinon il étend ce qui est couvert.
    /// Un `Skip` n'avance rien : c'est un trou de la mise à jour elle-même.
    ///
    /// Une suppression laisse aussi un trou (Ruling R198) quand une partie de
    /// ses plages dépasse ce que la page couvre une fois les blocs de la mise à
    /// jour intégrés, sans être déjà en attente dans la page (`pending`, Ruling
    /// R199) : yrs la garderait en attente, et supprimerait à l'arrivée ce que
    /// le client visé écrirait dans la plage.
    /// Comme [`Shape::leaves_gap`], pour les blocs seuls (Ruling R388) : une suppression qui vise
    /// des horloges que la page n'a pas n'y compte pas.
    pub(crate) fn structs_leave_gap(&self, state: &StateVector) -> bool {
        self.clients.iter().any(|(client, structs)| {
            let mut covered = state.get(&ClientID::new(*client));
            structs
                .iter()
                .filter(|s| !matches!(s.kind, Kind::Skip))
                .any(|s| {
                    let gap = s.id.1 > covered;
                    covered = covered.max(s.id.1 + s.len);
                    gap
                })
        })
    }

    pub(crate) fn leaves_gap(&self, state: &StateVector, pending: &Pending) -> bool {
        let mut covered_after: HashMap<u64, u32> = HashMap::new();
        for (client, structs) in &self.clients {
            let mut covered = state.get(&ClientID::new(*client));
            for s in structs {
                if matches!(s.kind, Kind::Skip) {
                    continue;
                }
                if s.id.1 > covered {
                    return true;
                }
                // Sans débordement : `read` a vérifié chaque fin de bloc.
                covered = covered.max(s.id.1 + s.len);
            }
            let after = covered_after.entry(*client).or_insert(covered);
            *after = (*after).max(covered);
        }
        self.deletions.iter().any(|(client, ranges)| {
            let covered = covered_after
                .get(client)
                .copied()
                .unwrap_or_else(|| state.get(&ClientID::new(*client)));
            ranges.iter().any(|&(start, end)| {
                pending
                    .uncovered_end(*client, start.max(covered), end)
                    .is_some()
            })
        })
    }

    /// Ce que la mise à jour attend encore d'une page dont le vecteur d'état
    /// est `state` et les suppressions en attente `pending` : chaque client
    /// pour lequel [`Shape::leaves_gap`] voit un trou, avec l'horloge que la
    /// page doit atteindre pour qu'il n'y en ait plus. Vide si elle n'en
    /// laisserait aucun.
    ///
    /// Pour des blocs, cette horloge ne dépend que de la mise à jour : c'est le
    /// début de son dernier bloc qui ne prolonge pas le précédent (un `Skip` les
    /// sépare, ou c'est le premier). Une suite de blocs sans trou est couverte
    /// dès que son début l'est, et ces débuts croissent. Pour des suppressions
    /// (Ruling R198), c'est la fin de ce que ses plages visent au-delà de la
    /// dernière suite de blocs du même client dans la mise à jour, hormis ce que
    /// la page garde déjà en attente (Ruling R199) : en deçà, les blocs de la
    /// mise à jour l'apportent. Ce qui est en attente le reste, ou devient connu
    /// quand ses éléments arrivent ; et un vecteur d'état ne fait que croître :
    /// une page qui a atteint ces horloges ne voit plus de trou pour ce client,
    /// ni plus tard. L'appelant compare donc sans relire la mise à jour. Le test
    /// `awaited_agrees_with_leaves_gap` le vérifie contre [`Shape::leaves_gap`].
    pub(crate) fn awaited(&self, state: &StateVector, pending: &Pending) -> Vec<(u64, u32)> {
        let mut awaited = Vec::new();
        // Par client, la fin de la dernière suite de blocs de chaque entrée : ce
        // que la page aura, ses blocs intégrés.
        let mut ends: HashMap<u64, u32> = HashMap::new();
        for (client, structs) in &self.clients {
            let mut start = None;
            let mut end = None;
            for s in structs.iter().filter(|s| !matches!(s.kind, Kind::Skip)) {
                if end != Some(s.id.1) {
                    start = Some(s.id.1);
                }
                end = Some(s.id.1 + s.len);
            }
            let (Some(start), Some(end)) = (start, end) else {
                continue;
            };
            let last = ends.entry(*client).or_insert(end);
            *last = (*last).max(end);
            if start > state.get(&ClientID::new(*client)) {
                awaited.push((*client, start));
            }
        }
        for (client, ranges) in &self.deletions {
            let brought = ends.get(client).copied().unwrap_or(0);
            let needed = ranges
                .iter()
                .filter_map(|&(start, end)| pending.uncovered_end(*client, start.max(brought), end))
                .max();
            if let Some(clock) = needed
                && clock > state.get(&ClientID::new(*client))
            {
                awaited.push((*client, clock));
            }
        }
        awaited
    }

    /// Plus longue chaîne de types imbriqués entre eux dans la mise à jour
    /// (un type dont le parent n'y est pas compte pour 1).
    fn chain_depth(&self) -> usize {
        let inserts: Vec<(&Struct, &Insert)> = self.inserts().collect();
        let mut by_client: HashMap<u64, Vec<(u32, u32, usize)>> = HashMap::new();
        for (k, (s, _)) in inserts.iter().enumerate() {
            by_client
                .entry(s.id.0)
                .or_default()
                .push((s.id.1, s.id.1 + s.len, k));
        }
        for ranges in by_client.values_mut() {
            ranges.sort_unstable();
        }
        let find = |(client, clock): Id| -> Option<usize> {
            let ranges = by_client.get(&client)?;
            let at = ranges.partition_point(|r| r.1 <= clock);
            ranges.get(at).filter(|r| r.0 <= clock).map(|&(_, _, k)| k)
        };
        // Parent (index d'un type de la mise à jour) de chaque élément, résolu
        // le long des voisins sans récursion.
        let mut parent: Vec<Option<Option<usize>>> = vec![None; inserts.len()];
        // Ruling R854 : le départ du parcours qui a vu chaque élément.
        let mut walked = vec![usize::MAX; inserts.len()];
        for start in 0..inserts.len() {
            let mut path = Vec::new();
            let mut k = start;
            let resolved = loop {
                if let Some(p) = parent[k] {
                    break p;
                }
                match inserts[k].1.parent {
                    Parent::Named(..) => break None,
                    Parent::Id(id) => break find(id).filter(|&p| inserts[p].1.type_ref.is_some()),
                    // Un élément revu : la chaîne boucle, mise à jour invalide.
                    Parent::Sibling(id) => {
                        walked[k] = start;
                        match find(id) {
                            Some(s) if walked[s] != start => {
                                path.push(k);
                                k = s;
                            }
                            _ => break None,
                        }
                    }
                }
            };
            parent[k] = Some(resolved);
            for p in path {
                parent[p] = Some(resolved);
            }
        }
        // Profondeur de chaque type, le long de ses parents, sans récursion.
        let mut depth: Vec<Option<usize>> = vec![None; inserts.len()];
        let mut deepest = 0;
        let mut walked = vec![usize::MAX; inserts.len()];
        for start in 0..inserts.len() {
            if inserts[start].1.type_ref.is_none() {
                continue;
            }
            let mut path = vec![start];
            walked[start] = start;
            let mut base = 0;
            while let Some(&k) = path.last() {
                match parent[k].flatten() {
                    // Des types qui se nomment en boucle n'ont pas de fond : plus profonds que
                    // tout ce que la mise à jour pourrait imbriquer.
                    Some(p) if walked[p] == start => {
                        deepest = deepest.max(inserts.len() + 1);
                        break;
                    }
                    Some(p) if depth[p].is_none() => {
                        walked[p] = start;
                        path.push(p);
                    }
                    Some(p) => {
                        base = depth[p].unwrap_or(0);
                        break;
                    }
                    None => break,
                }
            }
            for k in path.into_iter().rev() {
                base += 1;
                depth[k] = Some(base);
            }
            deepest = deepest.max(depth[start].unwrap_or(0));
        }
        deepest
    }
}

/// Refus d'une mise à jour qui a laissé des blocs en attente.
pub(crate) fn missing() -> CrdtError {
    decode_error("mise à jour incomplète : elle dépend de modifications que la page n'a pas reçues")
}

/// Un bloc, lu comme `decode_block` de yrs ; `None` pour un élément vide
/// (yrs l'ignore, sans avancer l'horloge).
fn read_struct(
    c: &mut Cursor,
    id: Id,
    json: JsonCount,
) -> Result<Option<Struct>, yrs::encoding::read::Error> {
    use yrs::encoding::read::Error;
    let info = c.read_u8()?;
    match info {
        BLOCK_SKIP_REF_NUMBER | BLOCK_GC_REF_NUMBER => {
            let len: u32 = c.read_var()?;
            if len == 0 {
                return Err(Error::UnexpectedValue);
            }
            let kind = if info == BLOCK_GC_REF_NUMBER {
                Kind::Gc
            } else {
                Kind::Skip
            };
            Ok(Some(Struct { id, len, kind }))
        }
        _ => {
            let cant_copy_parent = info & (HAS_ORIGIN | HAS_RIGHT_ORIGIN) == 0;
            let origin = (info & HAS_ORIGIN != 0).then(|| read_id(c)).transpose()?;
            let right = (info & HAS_RIGHT_ORIGIN != 0)
                .then(|| read_id(c))
                .transpose()?;
            // Une entrée `id` posée sur un type désigné par son id : l'attribut `id` d'un élément.
            let mut id_entry = false;
            let mut attr = None;
            let parent = if cant_copy_parent {
                let named = c.read_var::<u32>()? == 1;
                if named {
                    let root = c.read_string()?.to_string();
                    let sub = info & HAS_PARENT_SUB != 0;
                    if sub {
                        c.read_string()?;
                    }
                    Parent::Named(root, sub)
                } else {
                    let p = read_id(c)?;
                    if info & HAS_PARENT_SUB != 0 {
                        let key = c.read_string()?;
                        id_entry = key == "id";
                        attr = Some(Key::of(key));
                    }
                    Parent::Id(p)
                }
            } else {
                Parent::Sibling(origin.or(right).ok_or(Error::UnexpectedValue)?)
            };
            let content = read_content(c, info, id_entry, json)?;
            if content.len == 0 {
                return Ok(None);
            }
            let untyped = matches!(
                info & 0b1111,
                BLOCK_ITEM_STRING_REF_NUMBER
                    | BLOCK_ITEM_ANY_REF_NUMBER
                    | BLOCK_ITEM_JSON_REF_NUMBER
                    | BLOCK_ITEM_BINARY_REF_NUMBER
                    | BLOCK_ITEM_EMBED_REF_NUMBER
                    | BLOCK_ITEM_DOC_REF_NUMBER
            );
            let keyed = cant_copy_parent && info & HAS_PARENT_SUB != 0;
            Ok(Some(Struct {
                id,
                len: content.len,
                kind: Kind::Insert(Insert {
                    parent,
                    type_ref: content.type_ref,
                    tag: content.tag,
                    block_id: content.string,
                    mark: content.mark,
                    untyped,
                    keyed,
                    attr,
                    value: content.value,
                    any: info & 0b1111 == BLOCK_ITEM_ANY_REF_NUMBER,
                    json: info & 0b1111 == BLOCK_ITEM_JSON_REF_NUMBER,
                    right_origin: right.is_some(),
                    right,
                    origin,
                    sub_flag: info & HAS_PARENT_SUB != 0,
                    content: info & 0b1111,
                    right_is_origin: origin.is_some() && origin == right,
                }),
            }))
        }
    }
}

/// Jeu de suppressions, lu comme `IdSet::decode` de yrs (v1) : `varUint(n)`,
/// puis `n` fois `varUint(client) varUint(plages)` et, par plage,
/// `varUint(horloge) varUint(longueur)`. yrs ne refuse un client hors de
/// 53 bits que par un `debug_assert!`, et calcule `horloge + longueur` par une
/// addition qui déborde en debug : les deux paniquent en debug (Ruling R25).
///
/// Rend les plages `[horloge, horloge + longueur)`, sans les plages vides, qui
/// ne suppriment rien, et s'il y en avait une (Ruling R208). Rien n'est réservé
/// d'après un compte lu : ce qui est alloué reste borné par les octets reçus.
fn read_delete_set(c: &mut Cursor) -> Result<(Deletions, bool), CrdtError> {
    let n_clients: u32 = c.read_var().map_err(decode_error)?;
    let mut deletions = Vec::new();
    let mut empty = false;
    for _ in 0..n_clients {
        let client: u64 = c.read_var().map_err(decode_error)?;
        if client > MAX_CLIENT {
            return Err(decode_error(
                "identifiant de client hors de 53 bits dans les suppressions",
            ));
        }
        let n_ranges: u32 = c.read_var().map_err(decode_error)?;
        let mut ranges = Vec::new();
        for _ in 0..n_ranges {
            let clock: u32 = c.read_var().map_err(decode_error)?;
            let len: u32 = c.read_var().map_err(decode_error)?;
            let Some(end) = clock.checked_add(len) else {
                return Err(decode_error("plage de suppressions hors limites"));
            };
            if len > 0 {
                ranges.push((clock, end));
            } else {
                empty = true;
            }
        }
        if !ranges.is_empty() {
            deletions.push((client, ranges));
        }
    }
    Ok((deletions, empty))
}

fn read_id(c: &mut Cursor) -> Result<Id, yrs::encoding::read::Error> {
    let client: u64 = c.read_var()?;
    if client > MAX_CLIENT {
        return Err(yrs::encoding::read::Error::UnexpectedValue);
    }
    Ok((client, c.read_var()?))
}

/// Combien de chaînes lire d'un contenu JSON (`ContentJSON`) dont l'en-tête dit `len` (Ruling
/// R815). yrs 0.28.1 n'écrit pas ce qu'il lit : `ItemContent::encode` écrit `len` chaînes, comme
/// Yjs, et `ItemContent::decode` en lit `len + 1`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum JsonCount {
    /// Comme yrs lit une mise à jour : `len + 1` chaînes. Pour ce qu'on donne à yrs, qui
    /// l'intégrera ainsi.
    AsRead,
    /// Comme yrs (et Yjs) l'écrit : `len` chaînes. Pour ce que yrs vient d'encoder lui-même.
    AsWritten,
}

/// Ce que [`read_content`] retient d'un contenu.
struct Content {
    /// Longueur Yjs.
    len: u32,
    /// Pour un type, sa référence.
    type_ref: Option<u8>,
    /// Pour un élément XML, sa balise.
    tag: Option<String>,
    /// Pour un contenu `Any` d'une seule chaîne, quand on l'a demandée.
    string: Option<String>,
    /// Pour une marque de texte que la page ne garde pas, pourquoi (Ruling R431).
    mark: Option<MarkRefusal>,
    /// Ce que le contenu vaudrait comme valeur d'attribut (Ruling R689) ; `None` s'il est
    /// supprimé.
    value: Option<Value>,
}

/// Contenu d'un élément, lu comme `ItemContent::decode` de yrs : sa longueur
/// Yjs et, pour un type, sa référence (et sa balise pour un élément XML). Avec
/// `want_string`, un contenu `Any` fait d'une seule chaîne est rendu aussi. Pour une marque de
/// texte que la page ne garde pas, pourquoi (Ruling R431). Un contenu JSON est lu comme `json`
/// le dit : comme yrs le lit, ou comme il l'écrit (Ruling R815).
fn read_content(
    c: &mut Cursor,
    info: u8,
    want_string: bool,
    json: JsonCount,
) -> Result<Content, yrs::encoding::read::Error> {
    use yrs::encoding::read::Error;
    let mut content = Content {
        len: 0,
        type_ref: None,
        tag: None,
        string: None,
        mark: None,
        // Un type Yjs, du JSON, des octets, un objet incorporé : pas une valeur d'attribut.
        value: Some(Value::Other),
    };
    content.len = match info & 0b1111 {
        BLOCK_ITEM_DELETED_REF_NUMBER => {
            content.value = None;
            c.read_var()?
        }
        BLOCK_ITEM_JSON_REF_NUMBER => {
            // yrs lit `len + 1` chaînes (et aucune si `len` dépasse i32) ; il en écrit `len`.
            let remaining = c.read_var::<u32>()? as i32;
            let count = match json {
                _ if remaining < 0 => 0,
                JsonCount::AsRead => remaining as u32 + 1,
                JsonCount::AsWritten => remaining as u32,
            };
            for _ in 0..count {
                c.read_string()?;
            }
            count
        }
        BLOCK_ITEM_BINARY_REF_NUMBER => {
            c.read_buf()?;
            1
        }
        BLOCK_ITEM_STRING_REF_NUMBER => {
            // Comme valeur d'attribut, yrs rend toute la chaîne et Yjs son dernier caractère
            // (Ruling R702) : pas une valeur.
            c.read_string()?.encode_utf16().count() as u32
        }
        BLOCK_ITEM_EMBED_REF_NUMBER => {
            c.read_string()?;
            1
        }
        BLOCK_ITEM_FORMAT_REF_NUMBER => {
            let mark = known_mark(c.read_string()?);
            let value = c.read_string()?;
            content.mark = match mark {
                Ok(mark) => value_refusal(mark, value),
                Err(refusal) => Some(refusal),
            };
            1
        }
        BLOCK_ITEM_TYPE_REF_NUMBER => {
            let type_ref = c.read_u8()?;
            match type_ref {
                TYPE_REFS_XML_ELEMENT => {
                    content.tag = Some(c.read_string()?.to_string());
                }
                0 | 1 | 2 | 4 | 5 | 6 | 9 | 15 => {}
                _ => return Err(Error::UnexpectedValue),
            }
            content.type_ref = Some(type_ref);
            1
        }
        BLOCK_ITEM_ANY_REF_NUMBER => {
            let len: u32 = c.read_var()?;
            // Une seule chaîne (marque `119` de lib0), quand on la demande : la valeur d'un
            // attribut `id`. Sinon, comme yrs, chaque valeur est sautée.
            if want_string && len == 1 && c.buf.get(c.next) == Some(&119) {
                c.read_u8()?;
                content.string = Some(c.read_string()?.to_string());
                content.value = Some(Value::Str);
            } else {
                // La valeur d'une entrée de map est la dernière de son contenu.
                for _ in 0..len {
                    let start = c.next;
                    skip_any(c)?;
                    content.value = Some(summary(&c.buf[start..c.next]));
                }
            }
            len
        }
        BLOCK_ITEM_DOC_REF_NUMBER => {
            c.read_string()?;
            skip_any(c)?;
            1
        }
        _ => return Err(Error::UnexpectedValue),
    };
    Ok(content)
}

/// Ce que vaut, pour la garde des attributs (Ruling R689), une valeur `Any` que [`skip_any`] a
/// lue : la nature d'un scalaire, son nombre, et, pour une liste, si c'est une liste de largeurs
/// (`attrs::Value::Widths`). Rien n'est alloué, et une liste n'est lue qu'un niveau.
fn summary(bytes: &[u8]) -> Value {
    fn number(c: &mut Cursor, tag: u8) -> Option<Value> {
        Some(match tag {
            125 => Value::Int(c.read_var::<i64>().ok()?),
            124 => Value::Float(f64::from(c.read_f32().ok()?)),
            123 => Value::Float(c.read_f64().ok()?),
            _ => Value::Int(c.read_i64().ok()?),
        })
    }
    let mut c = Cursor::new(bytes);
    let Ok(tag) = c.read_u8() else {
        return Value::Other;
    };
    match tag {
        127 => Value::Undefined,
        126 => Value::Null,
        122..=125 => number(&mut c, tag).unwrap_or(Value::Other),
        120 | 121 => Value::Bool,
        119 => Value::Str,
        117 => {
            let Ok(len) = c.read_var::<u64>() else {
                return Value::Other;
            };
            if len > mementum_md::MAX_WIDTH_COLUMNS as u64 {
                return Value::Other;
            }
            for _ in 0..len {
                let width = match c.read_u8() {
                    Ok(126 | 127) => true,
                    Ok(tag @ 122..=125) => number(&mut c, tag).is_some_and(Value::is_width),
                    _ => false,
                };
                if !width {
                    return Value::Other;
                }
            }
            Value::Widths
        }
        _ => Value::Other,
    }
}

/// Saute une valeur `Any` (encodage lib0) sans récursion.
fn skip_any(c: &mut Cursor) -> Result<(), yrs::encoding::read::Error> {
    use yrs::encoding::read::Error;
    // Valeurs restant à lire dans chaque tableau ou map ouvert.
    let mut open: Vec<(u64, bool)> = Vec::new();
    loop {
        match c.read_u8()? {
            116 => {
                c.read_buf()?;
            }
            tag @ (117 | 118) => {
                if open.len() >= MAX_ANY_NESTING {
                    return Err(Error::UnexpectedValue);
                }
                open.push((c.read_var()?, tag == 118));
            }
            119 => {
                c.read_string()?;
            }
            120 | 121 | 126 | 127 => {}
            122 | 123 => {
                c.read_exact(8)?;
            }
            124 => {
                c.read_exact(4)?;
            }
            125 => {
                c.read_var::<i64>()?;
            }
            _ => return Err(Error::UnexpectedValue),
        }
        loop {
            match open.last_mut() {
                None => return Ok(()),
                Some((0, _)) => {
                    open.pop();
                }
                Some((remaining, map)) => {
                    *remaining -= 1;
                    if *map {
                        c.read_string()?;
                    }
                    break;
                }
            }
        }
    }
}

// ============================================================ page entière

impl Shape {
    /// Vrai si un élément inséré par la mise à jour (déjà appliquée) est plus
    /// profond que la borne. Chaque élément est retrouvé par son id, puis on
    /// remonte jusqu'à la racine : O(éléments insérés × profondeur).
    pub(crate) fn deeper_than_bound<T: ReadTxn>(&self, txn: &T) -> bool {
        self.inserts()
            .filter(|(_, i)| i.type_ref.is_some())
            .filter_map(|(s, _)| {
                let id = ID::new(ClientID::new(s.id.0), s.id.1);
                BranchID::get_nested(txn, &id).and_then(|b| XmlOut::try_from(b).ok())
            })
            .any(|node| depth_in_page(&node) > MAX_YJS_DEPTH)
    }
}

// ============================================================ métadonnées

/// Vrai si la transaction, déjà validée (`commit`), a changé `map` : une
/// entrée écrite, écrasée ou supprimée, ou un type posé dedans qui a changé.
///
/// yrs dresse à la validation la liste des types changés, chacun avec ses
/// ancêtres ; une suppression sans effet (l'entrée l'était déjà) n'y met
/// rien. Les pointeurs sont comparés, jamais suivis.
pub(crate) fn touched(txn: &TransactionMut, map: &MapRef) -> bool {
    let map = BranchPtr::from(map.as_ref());
    txn.changed_parent_types().contains(&map)
}

#[cfg(test)]
mod tests;
