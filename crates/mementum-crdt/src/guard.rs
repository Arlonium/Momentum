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

use std::collections::HashMap;

use mementum_md::MAX_DEPTH;
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
use yrs::{ID, MapRef, ReadTxn, StateVector, TransactionMut, XmlOut};

use crate::structure::depth_in_page;
use crate::{CrdtError, FRAGMENT, META};

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
}

/// Ce que contient une mise à jour, lu sans l'appliquer.
pub(crate) struct Shape {
    clients: Vec<(u64, Vec<Struct>)>,
}

impl Shape {
    /// Lit les blocs d'une mise à jour v1 comme `Update::decode` de yrs, mais
    /// sans récursion (une valeur `Any` trop imbriquée est refusée).
    pub(crate) fn read(update: &[u8]) -> Result<Shape, CrdtError> {
        let mut c = Cursor::new(update);
        let n_clients: u32 = c.read_var().map_err(decode_error)?;
        let mut clients = Vec::new();
        for _ in 0..n_clients {
            let n_structs: u32 = c.read_var().map_err(decode_error)?;
            let client: u64 = c.read_var().map_err(decode_error)?;
            if client > MAX_CLIENT {
                return Err(decode_error("identifiant de client hors de 53 bits"));
            }
            let mut clock: u32 = c.read_var().map_err(decode_error)?;
            let mut structs = Vec::new();
            for _ in 0..n_structs {
                let id = (client, clock);
                let Some(s) = read_struct(&mut c, id).map_err(decode_error)? else {
                    continue;
                };
                let len = s.len;
                clock = clock
                    .checked_add(len)
                    .ok_or_else(|| decode_error("horloge hors limites"))?;
                structs.push(s);
            }
            clients.push((client, structs));
        }
        read_delete_set(&mut c)?;
        Ok(Shape { clients })
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
    pub(crate) fn leaves_gap(&self, state: &StateVector) -> bool {
        self.clients.iter().any(|(client, structs)| {
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
            false
        })
    }

    /// Ce que la mise à jour attend encore d'une page dont le vecteur d'état
    /// est `state` : chaque client pour lequel [`Shape::leaves_gap`] voit un
    /// trou, avec l'horloge que la page doit atteindre pour qu'il n'y en ait
    /// plus. Vide si elle n'en laisserait aucun.
    ///
    /// Cette horloge ne dépend que de la mise à jour : c'est le début de son
    /// dernier bloc qui ne prolonge pas le précédent (un `Skip` les sépare, ou
    /// c'est le premier). Une suite de blocs sans trou est couverte dès que son
    /// début l'est, et ces débuts croissent. Comme un vecteur d'état ne fait
    /// que croître, une page qui a atteint cette horloge ne voit plus de trou
    /// pour ce client, ni plus tard : l'appelant compare sans relire la mise à
    /// jour. Le test `awaited_agrees_with_leaves_gap` le vérifie contre
    /// [`Shape::leaves_gap`].
    pub(crate) fn awaited(&self, state: &StateVector) -> Vec<(u64, u32)> {
        self.clients
            .iter()
            .filter_map(|(client, structs)| {
                let mut start = None;
                let mut end = None;
                for s in structs.iter().filter(|s| !matches!(s.kind, Kind::Skip)) {
                    if end != Some(s.id.1) {
                        start = Some(s.id.1);
                    }
                    end = Some(s.id.1 + s.len);
                }
                let clock = start?;
                (clock > state.get(&ClientID::new(*client))).then_some((*client, clock))
            })
            .collect()
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
                    // Une chaîne plus longue que la mise à jour boucle : mise à jour invalide.
                    Parent::Sibling(id) => match find(id) {
                        Some(s) if s != k && path.len() < inserts.len() => {
                            path.push(k);
                            k = s;
                        }
                        _ => break None,
                    },
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
        for start in 0..inserts.len() {
            if inserts[start].1.type_ref.is_none() {
                continue;
            }
            let mut path = vec![start];
            let mut base = 0;
            while let Some(&k) = path.last() {
                match parent[k].flatten() {
                    Some(p) if depth[p].is_none() && path.len() <= inserts.len() => path.push(p),
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
fn read_struct(c: &mut Cursor, id: Id) -> Result<Option<Struct>, yrs::encoding::read::Error> {
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
                        c.read_string()?;
                    }
                    Parent::Id(p)
                }
            } else {
                Parent::Sibling(origin.or(right).ok_or(Error::UnexpectedValue)?)
            };
            let (len, type_ref) = read_content(c, info)?;
            if len == 0 {
                return Ok(None);
            }
            Ok(Some(Struct {
                id,
                len,
                kind: Kind::Insert(Insert { parent, type_ref }),
            }))
        }
    }
}

/// Jeu de suppressions, lu comme `IdSet::decode` de yrs (v1) : `varUint(n)`,
/// puis `n` fois `varUint(client) varUint(plages)` et, par plage,
/// `varUint(horloge) varUint(longueur)`. yrs ne refuse un client hors de
/// 53 bits que par un `debug_assert!`, et calcule `horloge + longueur` par une
/// addition qui déborde en debug : les deux paniquent en debug (Ruling R25).
fn read_delete_set(c: &mut Cursor) -> Result<(), CrdtError> {
    let n_clients: u32 = c.read_var().map_err(decode_error)?;
    for _ in 0..n_clients {
        let client: u64 = c.read_var().map_err(decode_error)?;
        if client > MAX_CLIENT {
            return Err(decode_error(
                "identifiant de client hors de 53 bits dans les suppressions",
            ));
        }
        let n_ranges: u32 = c.read_var().map_err(decode_error)?;
        for _ in 0..n_ranges {
            let clock: u32 = c.read_var().map_err(decode_error)?;
            let len: u32 = c.read_var().map_err(decode_error)?;
            if clock.checked_add(len).is_none() {
                return Err(decode_error("plage de suppressions hors limites"));
            }
        }
    }
    Ok(())
}

fn read_id(c: &mut Cursor) -> Result<Id, yrs::encoding::read::Error> {
    let client: u64 = c.read_var()?;
    if client > MAX_CLIENT {
        return Err(yrs::encoding::read::Error::UnexpectedValue);
    }
    Ok((client, c.read_var()?))
}

/// Contenu d'un élément, lu comme `ItemContent::decode` de yrs : sa longueur
/// Yjs et, pour un type, sa référence.
fn read_content(c: &mut Cursor, info: u8) -> Result<(u32, Option<u8>), yrs::encoding::read::Error> {
    use yrs::encoding::read::Error;
    Ok(match info & 0b1111 {
        BLOCK_ITEM_DELETED_REF_NUMBER => (c.read_var()?, None),
        BLOCK_ITEM_JSON_REF_NUMBER => {
            // yrs lit `len + 1` chaînes (et aucune si `len` dépasse i32).
            let remaining = c.read_var::<u32>()? as i32;
            let count = if remaining >= 0 {
                remaining as u32 + 1
            } else {
                0
            };
            for _ in 0..count {
                c.read_string()?;
            }
            (count, None)
        }
        BLOCK_ITEM_BINARY_REF_NUMBER => {
            c.read_buf()?;
            (1, None)
        }
        BLOCK_ITEM_STRING_REF_NUMBER => (c.read_string()?.encode_utf16().count() as u32, None),
        BLOCK_ITEM_EMBED_REF_NUMBER => {
            c.read_string()?;
            (1, None)
        }
        BLOCK_ITEM_FORMAT_REF_NUMBER => {
            c.read_string()?;
            c.read_string()?;
            (1, None)
        }
        BLOCK_ITEM_TYPE_REF_NUMBER => {
            let type_ref = c.read_u8()?;
            match type_ref {
                TYPE_REFS_XML_ELEMENT => {
                    c.read_string()?;
                }
                0 | 1 | 2 | 4 | 5 | 6 | 9 | 15 => {}
                _ => return Err(Error::UnexpectedValue),
            }
            (1, Some(type_ref))
        }
        BLOCK_ITEM_ANY_REF_NUMBER => {
            let len: u32 = c.read_var()?;
            for _ in 0..len {
                skip_any(c)?;
            }
            (len, None)
        }
        BLOCK_ITEM_DOC_REF_NUMBER => {
            c.read_string()?;
            skip_any(c)?;
            (1, None)
        }
        _ => return Err(Error::UnexpectedValue),
    })
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
