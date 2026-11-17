//! La grammaire d'une mise à jour (garde des mises à jour, tour 4 ; Rulings R801 à R806).
//!
//! yrs 0.28.1 n'est pas sûr en mémoire devant un élément qu'il range sous une clé de map d'une
//! façon que Yjs n'écrit pas : l'entrée n'est plus tenue par rien, son parent est supprimé puis
//! ramassé, et l'état encodé ensuite lit de la mémoire libérée. Trois tours ont refusé ces formes
//! une par une ; chaque relecture en trouvait une autre. Ce module dit à la place **ce qu'un
//! client écrit** pour une entrée de map et pour un enfant de liste, d'après le code de Yjs 13, et
//! la garde refuse ce qui en sort : avant d'appliquer quand les octets le disent
//! ([`Shape::check_grammar`], [`Shape::check_values_flagged`]), après quand seul yrs sait ce
//! qu'il en a fait ([`Shape::check_as_written`]). Deux formes que Yjs écrit vraiment, et que yrs
//! ne range pas sûrement, ne sont pas refusées mais **récrites** avant d'être lues (N1, N2 :
//! [`super::cut::cut`]).
//!
//! Ce que Yjs écrit (`Item.write`, `structs/Item.js`) : l'octet d'en-tête porte le genre du
//! contenu, la marque « origine », la marque « origine droite » et la marque « entrée de map »
//! (`parentSub !== null`), **que l'élément nomme sa clé ou non**. La clé et le parent ne sont
//! écrits que sans origine ni origine droite.
//!
//! | Règle | Ce qu'un client écrit | Source dans Yjs |
//! | --- | --- | --- |
//! | E1 | une entrée de map porte une valeur `Any`, ou un contenu supprimé | `typeMapSet` (`types/AbstractType.js`) : `ContentAny([value])` pour toute valeur JSON ; `Item.gc` : `ContentDeleted` |
//! | E2 | une entrée de map n'a pas d'origine droite | `typeMapSet` : `new Item(id, left, left && left.lastId, null, null, parent, key, content)` |
//! | E4 | une valeur `Any` est une entrée de map | `ContentAny` ne naît que de `typeMapSet` et de `Y.Array` ; une page n'a pas de `Y.Array` |
//! | P1 | yrs range l'élément comme ses octets le disent : entrée de map si, et seulement si, il en porte la marque | `Item.write` : `parentSub === null ? 0 : BIT6` |
//! | N1 | une entrée de map `Any` **peut porter plusieurs valeurs**, vivante ou non : récrite en autant d'entrées d'une valeur, chacune d'origine la précédente | `Item.mergeWith` fond les entrées supprimées d'une clé que l'`UndoManager` garde (`keepItem`) ; `redoItem` (`structs/Item.js`) recopie le contenu fondu (`item.content.copy()`) dans une entrée vivante |
//! | N2 | l'origine droite d'un élément **peut être son origine** : l'origine droite est retirée | `redoItem` : `left` et `right` suivis par `redone` jusqu'au même élément |
//! | E3 | (filet de N1, et règle du journal) aucune entrée de map de plusieurs unités ne reste vivante : refusée d'un client, supprimée en entier à la relecture | ce que N1 laisse : une chaîne ou un autre contenu de plusieurs unités sous une clé, qu'aucun client n'écrit (E1) |
//! | L1 | (filet de N2) l'origine droite d'un élément n'est pas son origine | ce que N2 laisse : rien |
//!
//! N1 et N2 ont été trouvées par l'essai au hasard côté Yjs (`yjsAtRandom.test.ts`) : la
//! lecture du code de Yjs avait d'abord donné « une valeur par entrée » et « l'origine droite
//! suit l'origine », que Yjs dément après une annulation. **Une règle neuve se prouve contre
//! cet essai avant d'être posée.**
//!
//! Une forme neuve que le client écrirait (un `Y.Array`, une `Y.Map` imbriquée, un type comme
//! valeur d'attribut) demande d'élargir cette grammaire : sinon l'onglet est refusé.

use std::collections::HashMap;

use yrs::block::{
    BLOCK_GC_REF_NUMBER, BLOCK_ITEM_ANY_REF_NUMBER, BLOCK_ITEM_DELETED_REF_NUMBER,
    BLOCK_ITEM_JSON_REF_NUMBER, BLOCK_SKIP_REF_NUMBER, ClientID, HAS_ORIGIN, HAS_PARENT_SUB,
    HAS_RIGHT_ORIGIN,
};
use yrs::encoding::read::{Cursor, Read};
use yrs::{ID, IdSet, StateVector};

use super::{
    Id, Insert, JsonCount, KEYED_NOT_A_VALUE, KEYED_RIGHT_ORIGIN_MADE, Kind, Shape, Struct,
    decode_error, read_content, read_id,
};
use crate::CrdtError;

/// E3 : une entrée de map à plusieurs valeurs, que la mise à jour ne supprime pas en entier.
pub(crate) const MANY_VALUED: &str = "la mise à jour pose une entrée de map de plusieurs valeurs \
     sans la supprimer en entier, ce qu'aucun client n'écrit";

/// E3, vue dans ce que yrs a fait de la mise à jour.
pub(crate) const MANY_VALUED_MADE: &str = "la mise à jour pose, pour le serveur, une entrée de \
     map de plusieurs valeurs sans la supprimer en entier, ce qu'aucun client n'écrit";

/// E4 : une valeur `Any` qui ne porte pas la marque d'une entrée de map.
pub(crate) const VALUE_WITHOUT_FLAG: &str = "la mise à jour pose une valeur d'attribut qui ne se \
     dit pas entrée de map, ce qu'aucun client n'écrit";

/// L1 : un élément dont l'origine droite est son origine.
pub(crate) const RIGHT_ORIGIN_IS_ORIGIN: &str = "la mise à jour pose un élément dont l'origine \
     droite est son origine, ce qu'aucun client n'écrit";

/// P1 : yrs a rangé un élément autrement que ses octets ne le disent.
pub(crate) const NOT_AS_WRITTEN: &str = "la mise à jour pose un élément que la page range \
     autrement que ses octets ne le disent (entrée de map ou enfant de liste), ce qu'aucun \
     client n'écrit";

/// Les plages `[début, fin)` d'un client, triées et fondues.
type Ranges = HashMap<u64, Vec<(u32, u32)>>;

impl Shape {
    /// Refuse (`Structure`), sans rien appliquer, ce que les octets d'une mise à jour disent hors
    /// de la grammaire, pour ce qu'elle intègre (au-dessus de `floor`) : les règles E3 et L1,
    /// pour un client comme pour une entrée relue, et E1 pour un client (`from_client`), quand
    /// l'élément porte la marque d'une entrée de map. E3 vaut pour tout contenu de plusieurs
    /// unités qui n'est pas supprimé (une chaîne, du JSON) : relue du journal, une telle entrée
    /// n'est pas refusée pour son genre, mais elle se coupe comme une valeur `Any`.
    /// Avec `many_valued` (la relecture du journal), une entrée qui manque à E3 n'est pas
    /// refusée mais relevée : la page la supprime en entier une fois intégrée (Ruling R805).
    ///
    /// Rien n'est lu de la page, et rien n'est alloué pour une mise à jour qui ne porte pas
    /// d'entrée de map de plusieurs valeurs : le chemin d'une frappe est une boucle sur ses
    /// éléments.
    pub(crate) fn check_grammar(
        &self,
        floor: &StateVector,
        from_client: bool,
        mut many_valued: Option<&mut IdSet>,
    ) -> Result<(), CrdtError> {
        let mut deleted: Option<Ranges> = None;
        for (s, i) in self.inserts() {
            if s.id.1 + s.len <= floor.get(&ClientID::new(s.id.0)) {
                continue;
            }
            if i.right_is_origin {
                return Err(CrdtError::Structure(RIGHT_ORIGIN_IS_ORIGIN.to_string()));
            }
            if !i.sub_flag {
                continue;
            }
            if from_client
                && i.content != BLOCK_ITEM_ANY_REF_NUMBER
                && i.content != BLOCK_ITEM_DELETED_REF_NUMBER
            {
                return Err(CrdtError::Structure(KEYED_NOT_A_VALUE.to_string()));
            }
            if i.content != BLOCK_ITEM_DELETED_REF_NUMBER && s.len > 1 {
                let deleted = deleted.get_or_insert_with(|| self.deleted_ranges());
                if !covers(deleted.get(&s.id.0), s.id.1, s.id.1 + s.len) {
                    match many_valued.as_deref_mut() {
                        Some(found) => {
                            // Sa part au-dessus du plancher : c'est elle que la page intègre.
                            let from = s.id.1.max(floor.get(&ClientID::new(s.id.0)));
                            found.insert(
                                ID::new(ClientID::new(s.id.0), from),
                                s.id.1 + s.len - from,
                            );
                        }
                        None => return Err(CrdtError::Structure(MANY_VALUED.to_string())),
                    }
                }
            }
        }
        Ok(())
    }

    /// Vrai si la mise à jour intègre (au-dessus de `floor`) ce que Yjs écrit et que la page
    /// récrit avant de le lire ([`super::cut::cut`]) : une entrée de map `Any` de plusieurs
    /// valeurs (N1, Ruling R801), un élément dont l'origine droite est son origine (N2, Ruling
    /// R802).
    pub(crate) fn normalized(&self, floor: &StateVector) -> bool {
        self.inserts().any(|(s, i)| {
            ((i.sub_flag && i.content == BLOCK_ITEM_ANY_REF_NUMBER && s.len > 1)
                || i.right_is_origin)
                && s.id.1 + s.len > floor.get(&ClientID::new(s.id.0))
        })
    }

    /// Refuse (`Structure`), sans rien appliquer, une mise à jour de client qui intègre
    /// (au-dessus de `floor`) une valeur `Any` sans la marque d'une entrée de map (règle E4).
    pub(crate) fn check_values_flagged(&self, floor: &StateVector) -> Result<(), CrdtError> {
        let found = self.inserts().any(|(s, i)| {
            i.content == BLOCK_ITEM_ANY_REF_NUMBER
                && !i.sub_flag
                && s.id.1 + s.len > floor.get(&ClientID::new(s.id.0))
        });
        if found {
            return Err(CrdtError::Structure(VALUE_WITHOUT_FLAG.to_string()));
        }
        Ok(())
    }

    /// Les plages que la mise à jour supprime, par client, triées et fondues.
    fn deleted_ranges(&self) -> Ranges {
        let mut all = Ranges::new();
        for (client, ranges) in self.deletions() {
            all.entry(*client).or_default().extend(ranges.iter());
        }
        for ranges in all.values_mut() {
            ranges.sort_unstable();
            let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
            for &(start, end) in ranges.iter() {
                match merged.last_mut() {
                    Some(last) if start <= last.1 => last.1 = last.1.max(end),
                    _ => merged.push((start, end)),
                }
            }
            *ranges = merged;
        }
        all
    }

    /// Refuse (`Structure`) ce que yrs a fait d'une mise à jour, lu dans `made`, l'encodage par
    /// yrs de ce qu'il a intégré : yrs y écrit la marque « entrée de map » de chaque élément
    /// d'après la clé qu'il lui a donnée (celle de son voisin de gauche, à défaut de droite :
    /// `integrate_item`), quoi que les octets reçus (`self`) aient dit. Pour ce qui est au-dessus
    /// de `floor` :
    /// - une entrée de map à origine droite (E2, Ruling R723) ;
    /// - une entrée de map de plusieurs unités, qui n'est pas un contenu supprimé, que la mise à
    ///   jour ne supprime pas en entier (E3) : refusée, ou relevée dans `many_valued` s'il est
    ///   donné (la relecture du journal, qui la supprime en entier) ;
    /// - d'un client (`from_client`) : une entrée de map dont le contenu n'est ni une valeur
    ///   `Any` ni un contenu supprimé (E1, Rulings R702, R724), et un élément que yrs n'a pas
    ///   rangé comme ses octets le disent (P1) : devenu entrée de map sans en porter la marque,
    ///   ou l'inverse.
    ///
    /// Ce que yrs a rangé parmi les plages ramassées (un parent inconnu ou ramassé) n'est rien.
    /// E3 et P1 ne jugent que ce que la mise à jour a écrit : dans une page à trou, `made` porte
    /// aussi ce que la page connaissait au-delà (`PageDoc::integrated`).
    ///
    /// `made` est lu sans rien garder de ses contenus : une frappe y coûte la lecture de
    /// quelques octets. `Decode` si la garde ne le relit pas.
    pub(crate) fn check_as_written(
        &self,
        made: &[u8],
        floor: &StateVector,
        from_client: bool,
        mut many_valued: Option<&mut IdSet>,
    ) -> Result<(), CrdtError> {
        let mut deleted: Option<Ranges> = None;
        let mut c = Cursor::new(made);
        let n_clients: u32 = c.read_var().map_err(decode_error)?;
        for _ in 0..n_clients {
            let n_structs: u32 = c.read_var().map_err(decode_error)?;
            let client: u64 = c.read_var().map_err(decode_error)?;
            let mut clock: u32 = c.read_var().map_err(decode_error)?;
            let known = floor.get(&ClientID::new(client));
            let written = self
                .clients
                .iter()
                .find(|(c, _)| *c == client)
                .map(|(_, structs)| structs.as_slice());
            for _ in 0..n_structs {
                let info = c.read_u8().map_err(decode_error)?;
                if matches!(info, BLOCK_SKIP_REF_NUMBER | BLOCK_GC_REF_NUMBER) {
                    let len: u32 = c.read_var().map_err(decode_error)?;
                    clock = clock
                        .checked_add(len)
                        .ok_or_else(|| decode_error("horloge"))?;
                    continue;
                }
                let origin = info & HAS_ORIGIN != 0;
                let right = info & HAS_RIGHT_ORIGIN != 0;
                let sub = info & HAS_PARENT_SUB != 0;
                if origin {
                    read_id(&mut c).map_err(decode_error)?;
                }
                if right {
                    read_id(&mut c).map_err(decode_error)?;
                }
                if !origin && !right {
                    if c.read_var::<u32>().map_err(decode_error)? == 1 {
                        c.read_string().map_err(decode_error)?;
                    } else {
                        read_id(&mut c).map_err(decode_error)?;
                    }
                    if sub {
                        c.read_string().map_err(decode_error)?;
                    }
                }
                // Ce que yrs a encodé : un contenu JSON y porte `len` chaînes (Ruling R815).
                let len = read_content(&mut c, info, false, JsonCount::AsWritten)
                    .map_err(decode_error)?
                    .len;
                let start = clock;
                clock = clock
                    .checked_add(len)
                    .ok_or_else(|| decode_error("horloge"))?;
                if len == 0 || clock <= known {
                    continue;
                }
                let content = info & 0b1111;
                if sub && right {
                    return Err(CrdtError::Structure(KEYED_RIGHT_ORIGIN_MADE.to_string()));
                }
                if from_client
                    && sub
                    && content != BLOCK_ITEM_ANY_REF_NUMBER
                    && content != BLOCK_ITEM_DELETED_REF_NUMBER
                {
                    return Err(CrdtError::Structure(KEYED_NOT_A_VALUE.to_string()));
                }
                let Some((w, written_len)) =
                    written.and_then(|written| insert_at(written, (client, start)))
                else {
                    continue;
                };
                if from_client && w.sub_flag != sub {
                    return Err(CrdtError::Structure(NOT_AS_WRITTEN.to_string()));
                }
                // E3 juge l'élément tel que la mise à jour l'a écrit, pas le morceau que yrs en
                // garde : un élément de la même mise à jour dont l'origine tombe au milieu d'un
                // autre le coupe dans la transaction, et chaque morceau d'une entrée de map de
                // plusieurs unités serait passé pour une entrée d'une unité (graine 776 de l'essai
                // élargi, voie du journal : deux entrées vivantes pour une clé, Ruling R818).
                if sub && content != BLOCK_ITEM_DELETED_REF_NUMBER && written_len > 1 {
                    let deleted = deleted.get_or_insert_with(|| self.deleted_ranges());
                    if !covers(deleted.get(&client), start, clock) {
                        match many_valued.as_deref_mut() {
                            Some(found) => {
                                found.insert(ID::new(ClientID::new(client), start), len);
                            }
                            None => {
                                return Err(CrdtError::Structure(MANY_VALUED_MADE.to_string()));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl Shape {
    /// Vrai si la mise à jour intègre (au-dessus de `floor`) un contenu JSON (Ruling R815).
    pub(crate) fn carries_json(&self, floor: &StateVector) -> bool {
        self.inserts()
            .any(|(s, i)| i.json && s.id.1 + s.len > floor.get(&ClientID::new(s.id.0)))
    }

    /// Les contenus JSON que la mise à jour intègre, d'après ses octets : pour chacun, sa part
    /// au-dessus de `floor` (Ruling R815). Dans une page qui a un trou, une part que la page
    /// connaît au-delà du trou y est aussi : [`json_made`], qui lit ce que yrs a intégré, est la
    /// lecture exacte ; celle-ci ne sert que si la garde ne relit pas ce que yrs a encodé.
    pub(crate) fn json_items(&self, floor: &StateVector) -> IdSet {
        let mut found = IdSet::new();
        for (s, i) in self.inserts() {
            let client = ClientID::new(s.id.0);
            let from = s.id.1.max(floor.get(&client));
            if i.json && s.id.1 + s.len > from {
                found.insert(ID::new(client, from), s.id.1 + s.len - from);
            }
        }
        found
    }
}

impl Shape {
    /// Ce qui manque, dans un état que yrs a encodé (`self`, lu par [`Shape::read_written`]), à
    /// l'invariant de la classe C1 (Ruling R817) : **chaque entrée de map vivante est la seule
    /// vivante de sa clé, et n'a qu'une unité.** yrs, comme Yjs, supprime l'entrée de gauche quand
    /// une entrée devient la courante de sa clé ; une entrée de plusieurs unités coupée laissait
    /// sa moitié gauche vivante hors de la map (C1), et la page lisait plus tard de la mémoire
    /// libérée. `None` si l'état le tient.
    ///
    /// Les entrées d'une même clé se trouvent sans lire leur clé : chacune, hors la première, a
    /// pour origine la précédente (`typeMapSet`), et une entrée supprimée garde la sienne. Une
    /// entrée dont l'origine est ramassée (son parent l'est) n'est rattachée à rien.
    pub(crate) fn unsound_map_entries(&self) -> Option<String> {
        let deleted = self.deleted_ranges();
        // Les entrées de map, par client, dans l'ordre de leurs horloges : (début, fin, vivante,
        // origine).
        let mut entries: Vec<(Id, u32, bool, Option<Id>)> = Vec::new();
        for (s, i) in self.inserts() {
            if !i.sub_flag {
                continue;
            }
            let end = s.id.1 + s.len;
            let live = i.content != BLOCK_ITEM_DELETED_REF_NUMBER
                && !covers(deleted.get(&s.id.0), s.id.1, end);
            if live && s.len > 1 {
                return Some(format!(
                    "une entrée de map vivante de {} unités ({}, {})",
                    s.len, s.id.0, s.id.1
                ));
            }
            let origin = match i.parent {
                super::Parent::Sibling(id) => Some(id),
                _ => None,
            };
            entries.push((s.id, end, live, origin));
        }
        entries.sort_unstable_by_key(|e| e.0);
        let at = |(client, clock): Id| {
            let first = entries.partition_point(|e| (e.0.0, e.1) <= (client, clock));
            entries
                .get(first)
                .filter(|e| e.0.0 == client && e.0.1 <= clock)
                .map(|_| first)
        };
        // Chaque entrée rattachée à celle d'avant elle dans sa clé : une forêt, une clé par arbre.
        let mut chain: Vec<usize> = (0..entries.len()).collect();
        fn root(chain: &mut [usize], mut k: usize) -> usize {
            while chain[k] != k {
                chain[k] = chain[chain[k]];
                k = chain[k];
            }
            k
        }
        for (k, entry) in entries.iter().enumerate() {
            if let Some(left) = entry.3.and_then(at) {
                let (a, b) = (root(&mut chain, k), root(&mut chain, left));
                chain[a] = b;
            }
        }
        let mut live_in: HashMap<usize, Id> = HashMap::new();
        for (k, entry) in entries.iter().enumerate() {
            if !entry.2 {
                continue;
            }
            let r = root(&mut chain, k);
            if let Some(other) = live_in.insert(r, entry.0) {
                return Some(format!(
                    "deux entrées de map vivantes pour une même clé : {other:?} et {:?}",
                    entry.0
                ));
            }
        }
        None
    }
}

/// Les contenus JSON que yrs a intégrés, lus dans `made`, ce qu'il en a encodé (l'encodage de la
/// transaction, `PageDoc::integrated`), au-dessus de `floor` (Ruling R815). yrs 0.28.1 écrit un tel
/// contenu en `len` chaînes et le relit en `len + 1` : resté vivant dans une page, il rend illisible
/// tout état qu'elle encode. Seuls les éléments que yrs garde comme du JSON y sont : un contenu
/// qu'il a rangé parmi les plages ramassées (un parent inconnu ou ramassé) n'en est plus un.
pub(crate) fn json_made(made: &[u8], floor: &StateVector) -> Result<IdSet, CrdtError> {
    let mut found = IdSet::new();
    let mut c = Cursor::new(made);
    let n_clients: u32 = c.read_var().map_err(decode_error)?;
    for _ in 0..n_clients {
        let n_structs: u32 = c.read_var().map_err(decode_error)?;
        let client: u64 = c.read_var().map_err(decode_error)?;
        let mut clock: u32 = c.read_var().map_err(decode_error)?;
        let known = floor.get(&ClientID::new(client));
        for _ in 0..n_structs {
            let info = c.read_u8().map_err(decode_error)?;
            if matches!(info, BLOCK_SKIP_REF_NUMBER | BLOCK_GC_REF_NUMBER) {
                let len: u32 = c.read_var().map_err(decode_error)?;
                clock = clock
                    .checked_add(len)
                    .ok_or_else(|| decode_error("horloge"))?;
                continue;
            }
            let origin = info & HAS_ORIGIN != 0;
            let right = info & HAS_RIGHT_ORIGIN != 0;
            if origin {
                read_id(&mut c).map_err(decode_error)?;
            }
            if right {
                read_id(&mut c).map_err(decode_error)?;
            }
            if !origin && !right {
                if c.read_var::<u32>().map_err(decode_error)? == 1 {
                    c.read_string().map_err(decode_error)?;
                } else {
                    read_id(&mut c).map_err(decode_error)?;
                }
                if info & HAS_PARENT_SUB != 0 {
                    c.read_string().map_err(decode_error)?;
                }
            }
            let len = read_content(&mut c, info, false, JsonCount::AsWritten)
                .map_err(decode_error)?
                .len;
            let start = clock;
            clock = clock
                .checked_add(len)
                .ok_or_else(|| decode_error("horloge"))?;
            if info & 0b1111 == BLOCK_ITEM_JSON_REF_NUMBER && clock > known {
                let from = start.max(known);
                found.insert(ID::new(ClientID::new(client), from), clock - from);
            }
        }
    }
    Ok(found)
}

/// L'élément de `structs` (ceux d'un client, dans l'ordre de leurs horloges) qui couvre `id`, et
/// sa longueur telle que la mise à jour l'écrit.
fn insert_at(structs: &[Struct], id: Id) -> Option<(&Insert, u32)> {
    let at = structs.partition_point(|s| s.id.1 + s.len <= id.1);
    match structs.get(at) {
        Some(Struct {
            id: start,
            len,
            kind: Kind::Insert(insert),
        }) if start.1 <= id.1 => Some((insert, *len)),
        _ => None,
    }
}

/// Vrai si `ranges` (triées, fondues) couvrent `[start, end)`.
fn covers(ranges: Option<&Vec<(u32, u32)>>, start: u32, end: u32) -> bool {
    ranges.is_some_and(|ranges| {
        let at = ranges.partition_point(|range| range.1 <= start);
        ranges
            .get(at)
            .is_some_and(|range| range.0 <= start && end <= range.1)
    })
}
