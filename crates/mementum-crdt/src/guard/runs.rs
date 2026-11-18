//! Un élément que yrs et Yjs rangent selon le découpage de leurs blocs (règle N3 de la grammaire,
//! Ruling R825 ; constat I-7.1 de la re-relecture 7 de la garde des mises à jour).
//!
//! Pour placer un élément reçu, yrs (`Update::missing_dependency`) comme Yjs (`Item.getMissing`)
//! prennent pour voisin de gauche le bloc qui finit à son origine (`getItemCleanEnd`), **puis**
//! pour voisin de droite le bloc qui commence à son origine droite (`getItemCleanStart`). Quand
//! l'origine droite `(A, r)` est du client de l'origine `(A, o)`, à une horloge au plus égale
//! (`r <= o`), et que le receveur garde les unités `r - 1` à `o` dans **un même bloc**, la seconde
//! recherche coupe ce bloc en `[…, r - 1] | [r, …]` et le voisin de gauche, déjà pris, reste sur la
//! moitié qui finit à `r - 1` : l'élément est rangé juste avant `r`. Chez un receveur qui n'a pas
//! ces unités d'un seul bloc (le bloc de `o` commence à `r` ou après), le voisin de gauche finit à
//! `o`, et l'élément est rangé après `o` (la résolution des conflits part de l'unité qui suit `o`
//! et ne rencontre jamais le voisin de droite, qui est à gauche de `o`). Le découpage en blocs
//! n'est pas le même chez tous : la page vivante (où l'élément a coupé le bloc), la page relue de
//! son état (où les blocs sont ceux que l'état écrit), un onglet. La page vivante lisait « azybc »,
//! la page relue de son état « abczy », sous la même révision.
//!
//! Des unités ne tiennent en un bloc que si elles se suivent par leurs horloges, que chacune, après
//! la première, a pour origine l'unité d'avant, et que toutes ont la même origine droite
//! (`try_squash` de yrs, `Item.mergeWith` de Yjs ; ces fonctions demandent aussi qu'elles se
//! suivent dans la liste et que leurs contenus se fondent, deux conditions qu'on ne suit pas : une
//! suppression les rend vraies plus tard). On suit donc, par client, ces **suites** ([`Run`]) :
//! l'élément est dangereux si une même suite porte `r - 1` et `o`.
//!
//! **Aucun client honnête ne l'écrit en tapant** : un client insère entre un voisin et le voisin
//! qui le suit, si bien que son origine droite est à droite de son origine dans sa liste ; or des
//! unités `r` à `o` enchaînées par leurs origines mettent `o` à droite de `r` (yrs comme Yjs
//! rangent un élément à droite de son origine). **Yjs l'écrit en rétablissant** : `redoItem` suit
//! les voisins d'un élément par `redone`, et quand les deux mènent au même élément `X` de plusieurs
//! unités, il écrit l'origine `X.lastId` et l'origine droite `X.id` (la forme N2 quand `X` n'a
//! qu'une unité). La page récrit donc l'élément, sans son origine droite, comme N2 : il est rangé
//! par son origine seule, partout pareil.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use yrs::block::{ClientID, ID};
use yrs::branch::BranchID;
use yrs::{ReadTxn, StateVector};

use super::cut::Beyond;
use super::{Id, Kind, META, Parent, Shape};
use crate::{CrdtError, FRAGMENT};

/// Le type où vit une unité (règles G et P2, Rulings R827, R829).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Home {
    /// La racine du document.
    Fragment,
    /// La racine des métadonnées.
    Meta,
    /// Une autre racine : la garde la refuse.
    OtherRoot,
    /// Le type que porte cet élément.
    In(Id),
    /// Une plage ramassée : elle ne vit nulle part.
    Gone,
    /// Ses voisins manquent : yrs la tiendra pour une dépendance absente.
    Unknown,
}

/// Où une unité a été rangée : son type, et si elle y est une entrée de map (`parentSub`). Une
/// unité qui ne nomme pas son parent le tient de son origine, sinon de son origine droite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) home: Home,
    pub(crate) keyed: bool,
}

/// Ce qu'est une unité au moment de juger une mise à jour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    /// Une plage ramassée.
    Collected,
    /// Un élément, dans ce type.
    In(Home),
    /// Ni la page ni la mise à jour ne la portent : yrs la tiendra pour une dépendance absente.
    Unknown,
}

/// Un identifiant sur douze octets, ou rien. Une horloge s'arrête à 2^31 - 1 (`Shape::read`, Ruling
/// R808) : la dernière dit qu'il n'y a pas d'identifiant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, packed(4))]
struct Slot {
    client: u64,
    clock: u32,
}

impl Slot {
    const NONE: Slot = Slot {
        client: 0,
        clock: u32::MAX,
    };

    fn of(id: Option<Id>) -> Slot {
        id.map_or(Slot::NONE, |(client, clock)| Slot { client, clock })
    }

    fn get(self) -> Option<Id> {
        (self.clock != u32::MAX).then_some((self.client, self.clock))
    }
}

/// Une suite d'unités d'un client qui peuvent tenir en un seul bloc de yrs ou de Yjs : elles se
/// suivent par leurs horloges, chacune après la première a pour origine l'unité d'avant, et toutes
/// ont la même origine droite. Une page en garde une par course de frappe, et, au pire, une par
/// unité : elle tient sur 48 octets (les bornes de mémoire de `tests/linear.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    pub(crate) start: u32,
    /// Exclue.
    pub(crate) end: u32,
    right: Slot,
    /// L'origine de sa première unité (celle des suivantes est l'unité d'avant).
    origin: Slot,
    /// Le type qui la porte, pour [`Home::In`].
    parent: Slot,
    /// [`GC`], [`KEYED`], et le genre de [`Home`] au-delà (`HOME_SHIFT`) ; 0 : pas encore lu.
    marks: u32,
}

const _: () = assert!(std::mem::size_of::<Run>() == 48);

/// Une plage ramassée : ni origine ni voisins, elle ne tient avec rien.
const GC: u32 = 1;
/// Une entrée de map.
const KEYED: u32 = 2;
const HOME_SHIFT: u32 = 2;

impl Run {
    fn new(
        (start, end): (u32, u32),
        right: Option<Id>,
        origin: Option<Id>,
        gc: bool,
        place: Option<Place>,
    ) -> Run {
        let mut run = Run {
            start,
            end,
            right: Slot::of(right),
            origin: Slot::of(origin),
            parent: Slot::NONE,
            marks: if gc { GC } else { 0 },
        };
        if let Some(place) = place {
            run.set_place(place);
        }
        run
    }

    pub(crate) fn right(&self) -> Option<Id> {
        self.right.get()
    }

    pub(crate) fn origin(&self) -> Option<Id> {
        self.origin.get()
    }

    /// Une plage ramassée quand elle a été posée.
    pub(crate) fn gc(&self) -> bool {
        self.marks & GC != 0
    }

    /// Où vit la suite : écrit dans ses octets si sa première unité nomme son parent, sinon lu une
    /// fois chez ses voisins ([`Judge::place`]). Les unités d'une suite ont le même parent : chacune
    /// après la première a pour origine l'unité d'avant.
    fn place(&self) -> Option<Place> {
        let home = match self.marks >> HOME_SHIFT {
            0 => return None,
            1 => Home::Fragment,
            2 => Home::Meta,
            3 => Home::OtherRoot,
            4 => Home::In(self.parent.get()?),
            5 => Home::Gone,
            _ => Home::Unknown,
        };
        Some(Place {
            home,
            keyed: self.marks & KEYED != 0,
        })
    }

    fn set_place(&mut self, place: Place) {
        let kind = match place.home {
            Home::Fragment => 1,
            Home::Meta => 2,
            Home::OtherRoot => 3,
            Home::In(parent) => {
                self.parent = Slot::of(Some(parent));
                4
            }
            Home::Gone => 5,
            Home::Unknown => 6,
        };
        self.marks = (self.marks & GC) | (u32::from(place.keyed) * KEYED) | (kind << HOME_SHIFT);
    }

    /// L'origine de sa première unité est l'unité d'avant du même client (`client`).
    fn head(&self, client: u64) -> bool {
        self.start > 0 && self.origin() == Some((client, self.start - 1))
    }

    /// Vrai si `next` prolonge `self` (les deux de `client`) : ils tiennent en une suite.
    fn joins(&self, client: u64, next: &Run) -> bool {
        self.end == next.start
            && !self.gc()
            && !next.gc()
            && next.head(client)
            && self.right == next.right
    }

    /// L'origine de l'unité `clock` de la suite (de `client`).
    fn origin_of(&self, client: u64, clock: u32) -> Option<Id> {
        if clock == self.start {
            self.origin()
        } else {
            Some((client, clock - 1))
        }
    }
}

/// Les suites de la page, par client, triées et disjointes, fondues dès qu'elles se prolongent.
/// Elles disent, pour chaque unité qu'elles couvrent, ce que son élément portait à son écriture
/// (son origine, son origine droite), ce qu'aucun découpage ne change : yrs et Yjs gardent
/// l'origine droite aux deux moitiés d'un bloc coupé, et ne fondent que des blocs enchaînés de même
/// origine droite.
#[derive(Default)]
pub(crate) struct Runs {
    clients: HashMap<u64, Vec<Run>>,
}

impl Runs {
    /// La suite qui porte l'unité `(client, clock)`, si elle est connue.
    pub(crate) fn at(&self, client: u64, clock: u32) -> Option<Run> {
        let runs = self.clients.get(&client)?;
        let at = runs.partition_point(|run| run.end <= clock);
        runs.get(at).filter(|run| run.start <= clock).copied()
    }

    /// Le début de ce qui n'est pas connu juste avant `clock` chez `client` : la fin de la
    /// dernière suite qui finit au plus à `clock`, sinon 0.
    fn unknown_from(&self, client: u64, clock: u32) -> u32 {
        self.clients.get(&client).map_or(0, |runs| {
            let at = runs.partition_point(|run| run.end <= clock);
            at.checked_sub(1).map_or(0, |before| runs[before].end)
        })
    }

    /// Prend les suites d'une mise à jour que la page vient d'intégrer. Celles d'un client que la
    /// page ne connaissait pas passent telles quelles : une relecture ne les range pas deux fois.
    pub(crate) fn absorb(&mut self, other: Runs) {
        for (client, mut list) in other.clients {
            match self.clients.entry(client) {
                Entry::Vacant(known) => {
                    list.shrink_to_fit();
                    known.insert(list);
                }
                Entry::Occupied(mut known) => {
                    for run in list {
                        Self::insert_into(known.get_mut(), client, run);
                    }
                }
            }
        }
    }

    /// Le nombre de suites connues.
    fn len(&self) -> usize {
        self.clients.values().map(Vec::len).sum()
    }

    /// Retient où vit la suite qui commence à `start`.
    fn settle(&mut self, client: u64, start: u32, place: Place) {
        if let Some(runs) = self.clients.get_mut(&client) {
            let at = runs.partition_point(|run| run.start < start);
            if let Some(run) = runs.get_mut(at).filter(|run| run.start == start) {
                run.set_place(place);
            }
        }
    }

    /// Ajoute une suite. Une suite qui en chevauche une connue n'est pas ajoutée : ce qu'on sait
    /// d'une unité ne change pas, et ce qui est connu vient de ce que la page a intégré.
    #[cfg(test)]
    fn insert(&mut self, client: u64, run: Run) {
        Self::insert_into(self.clients.entry(client).or_default(), client, run);
    }

    /// Ajoute ce que `run`, lu dans yrs, porte et que les suites connues ne couvrent pas : yrs fond
    /// des blocs que la page a vus arriver séparément (une plage ramassée à cheval sur le plancher,
    /// la part neuve d'un élément coupé et l'unité d'avant).
    fn insert_missing(&mut self, client: u64, run: Run) {
        let runs = self.clients.entry(client).or_default();
        let mut from = run.start;
        while from < run.end {
            let at = runs.partition_point(|known| known.end <= from);
            match runs.get(at) {
                Some(known) if known.start <= from => from = known.end,
                next => {
                    let to = next.map_or(run.end, |known| known.start.min(run.end));
                    let piece = Run {
                        start: from,
                        end: to,
                        origin: Slot::of(run.origin_of(client, from)),
                        ..run
                    };
                    Self::insert_into(runs, client, piece);
                    from = to;
                }
            }
        }
    }

    fn insert_into(runs: &mut Vec<Run>, client: u64, run: Run) {
        if run.start >= run.end {
            return;
        }
        let at = if runs.last().is_none_or(|last| last.start < run.start) {
            runs.len()
        } else {
            runs.partition_point(|r| r.start < run.start)
        };
        if at > 0 && runs[at - 1].end > run.start {
            return;
        }
        if runs.get(at).is_some_and(|next| next.start < run.end) {
            return;
        }
        let mut run = run;
        let mut at = at;
        if at > 0 && runs[at - 1].joins(client, &run) {
            let before = runs.remove(at - 1);
            run = Run {
                start: before.start,
                origin: before.origin,
                parent: before.parent,
                marks: before.marks,
                ..run
            };
            at -= 1;
        }
        if runs.get(at).is_some_and(|next| run.joins(client, next)) {
            let next = runs.remove(at);
            run.end = next.end;
        }
        runs.insert(at, run);
    }
}

impl Shape {
    /// Les suites que la mise à jour pose et que la page intégrera : les éléments et les plages
    /// ramassées au-dessus du plancher de leur client (`floor`), hors de ce que la page connaît
    /// au-delà d'un trou (`beyond`). Après la coupe (Ruling R784), aucun élément n'est à cheval :
    /// chacun est connu en entier, ou neuf en entier. Par client, dans l'ordre des horloges, fondues
    /// dès qu'elles se prolongent.
    pub(crate) fn runs(&self, floor: &StateVector, beyond: &Beyond) -> Runs {
        let mut out = Runs::default();
        for (client, structs) in &self.clients {
            let known = floor.get(&ClientID::new(*client));
            let holes = beyond.get(client);
            let mut list: Vec<Run> = Vec::new();
            for s in structs {
                let end = s.id.1 + s.len;
                if end <= known
                    || holes
                        .is_some_and(|ranges| ranges.iter().any(|r| r.0 <= s.id.1 && s.id.1 < r.1))
                {
                    continue;
                }
                // Une plage ramassée à cheval sur le plancher n'est pas coupée (Ruling R784 ne coupe
                // que les éléments) : sa part connue n'est pas ramassée pour autant.
                let span = (s.id.1.max(known), end);
                let run = match &s.kind {
                    Kind::Insert(i) => Run::new(span, i.right, i.origin, false, i.place()),
                    Kind::Gc => Run::new(span, None, None, true, Some(GONE)),
                    Kind::Skip => continue,
                };
                match list.last_mut() {
                    Some(last) if last.joins(*client, &run) => last.end = run.end,
                    _ => list.push(run),
                }
            }
            if !list.is_empty() {
                out.clients.entry(*client).or_default().extend(list);
            }
        }
        out
    }

    /// Les éléments que la mise à jour intègre (au-dessus de `floor`) sans origine mais avec une
    /// origine droite, hors des entrées de map : les candidats de la règle I1 (Ruling R828). Leur
    /// identifiant et leur origine droite.
    pub(crate) fn origin_less_with_right(&self, floor: &StateVector) -> Vec<(Id, Id)> {
        self.inserts()
            .filter(|(s, i)| {
                i.origin.is_none()
                    && !i.sub_flag
                    && s.id.1 + s.len > floor.get(&ClientID::new(s.id.0))
            })
            .filter_map(|(s, i)| i.right.map(|right| (s.id, right)))
            .collect()
    }

    /// Les éléments que la mise à jour intègre (au-dessus de `floor`) qui ont une origine et une
    /// origine droite, hors des entrées de map : les candidats de la règle I3 (Ruling R834). Leur
    /// identifiant, leur origine et leur origine droite.
    pub(crate) fn between_two(&self, floor: &StateVector) -> Vec<(Id, Id, Id)> {
        self.inserts()
            .filter(|(s, i)| !i.sub_flag && s.id.1 + s.len > floor.get(&ClientID::new(s.id.0)))
            .filter_map(|(s, i)| Some((s.id, i.origin?, i.right?)))
            .collect()
    }

    /// Les éléments que la mise à jour intègre (au-dessus de `floor`) dont l'origine droite est
    /// du client de leur origine, à une horloge plus basse (l'égalité est la forme N2, récrite à
    /// part), et pas la toute première unité du client : les candidats de la règle N3. Leur
    /// identifiant, leur origine et leur origine droite.
    pub(crate) fn backward_right_origins(&self, floor: &StateVector) -> Vec<(Id, Id, Id)> {
        let mut found = Vec::new();
        for (s, i) in self.inserts() {
            if s.id.1 + s.len <= floor.get(&ClientID::new(s.id.0)) {
                continue;
            }
            if let (Some(origin), Some(right)) = (i.origin, i.right)
                && origin.0 == right.0
                && right.1 < origin.1
                && right.1 > 0
            {
                found.push((s.id, origin, right));
            }
        }
        found
    }
}

impl super::Insert {
    /// Où l'élément se dit rangé, s'il nomme son parent.
    fn place(&self) -> Option<Place> {
        let home = match &self.parent {
            Parent::Named(root, _) if root == FRAGMENT => Home::Fragment,
            Parent::Named(root, _) if root == META => Home::Meta,
            Parent::Named(..) => Home::OtherRoot,
            Parent::Id(id) => Home::In(*id),
            Parent::Sibling(_) => return None,
        };
        let keyed = self.keyed || matches!(self.parent, Parent::Named(_, true));
        Some(Place { home, keyed })
    }
}

impl Shape {
    /// Ce que la mise à jour pose et que la page intégrera (comme [`Shape::runs`]) : les éléments
    /// et les plages ramassées au-dessus du plancher de leur client, hors de ce que la page connaît
    /// au-delà d'un trou. Par client, dans l'ordre des horloges.
    fn posed(&self, floor: &StateVector, beyond: &Beyond) -> Vec<&super::Struct> {
        let mut out = Vec::new();
        for (client, structs) in &self.clients {
            let known = floor.get(&ClientID::new(*client));
            let holes = beyond.get(client);
            out.extend(structs.iter().filter(|s| {
                s.id.1 + s.len > known
                    && !matches!(s.kind, Kind::Skip)
                    && !holes
                        .is_some_and(|ranges| ranges.iter().any(|r| r.0 <= s.id.1 && s.id.1 < r.1))
            }));
        }
        out
    }
}

/// Au plus tant de suites parcourues pour juger un élément ; au-delà, il est tenu pour dangereux
/// (récrit). Les suites d'une même source sont fondues dès qu'elles se prolongent : un élément
/// n'en traverse d'ordinaire que deux (la page, puis la mise à jour).
const MAX_STEPS: usize = 64;

/// Une unité dont les voisins manquent.
const UNKNOWN: Place = Place {
    home: Home::Unknown,
    keyed: false,
};

/// Une plage ramassée.
const GONE: Place = Place {
    home: Home::Gone,
    keyed: false,
};

/// Ce qu'on sait des unités d'un client, pour juger les candidats d'une mise à jour : les suites
/// de la page (`page`), qu'on complète d'après yrs pour ce qu'elles ne connaissent pas encore (une
/// écriture du serveur), et celles que la mise à jour pose (`update`).
pub(crate) struct Judge<'a, T: ReadTxn> {
    pub(crate) txn: &'a T,
    pub(crate) floor: &'a StateVector,
    pub(crate) beyond: &'a Beyond,
    pub(crate) page: &'a mut Runs,
    pub(crate) update: &'a mut Runs,
}

/// Où en est le jugement d'un élément posé par la mise à jour ([`Judge::collected_neighbours`]).
#[derive(Clone, Copy)]
enum Seen {
    New,
    Visiting,
    Done(Unit),
}

impl<T: ReadTxn> Judge<'_, T> {
    /// Vrai si la page connaît l'unité `(client, clock)` (yrs l'a : sous le plancher, ou au-delà
    /// d'un trou).
    fn in_page(&self, client: u64, clock: u32) -> bool {
        clock < self.floor.get(&ClientID::new(client))
            || self
                .beyond
                .get(&client)
                .is_some_and(|ranges| ranges.iter().any(|r| r.0 <= clock && clock < r.1))
    }

    /// La suite qui porte l'unité `(client, clock)`, de la page ou de la mise à jour.
    fn at(&mut self, client: u64, clock: u32) -> Result<Option<Run>, CrdtError> {
        if !self.in_page(client, clock) {
            return Ok(self.update.at(client, clock));
        }
        if let Some(run) = self.page.at(client, clock) {
            return Ok(Some(run));
        }
        // Une unité de la page que ses suites ne connaissent pas : une écriture du serveur, qui
        // ne passe pas par la lecture des mises à jour. On lit ce que yrs en garde, depuis la fin
        // de ce qui est connu juste avant elle : l'encodage de ses blocs dit, pour chacun, son
        // origine et son origine droite.
        let from = self.page.unknown_from(client, clock);
        let id = ClientID::new(client);
        let mut since: HashMap<ClientID, u32> =
            self.floor.iter().map(|(c, clock)| (*c, *clock)).collect();
        if from == 0 {
            since.remove(&id);
        } else {
            since.insert(id, from);
        }
        let encoded = self
            .txn
            .encode_diff_v1(&StateVector::new(since.into_iter().collect()));
        let shape = Shape::read_written(&encoded)?;
        let mut read = shape.runs(&StateVector::default(), &Beyond::new());
        for run in read.clients.remove(&client).unwrap_or_default() {
            self.page.insert_missing(client, run);
        }
        Ok(self.page.at(client, clock))
    }

    /// Où vit l'unité `id`, de la page ou de la mise à jour. yrs range un élément qui ne nomme pas
    /// son parent chez son origine, sinon chez son origine droite : on remonte ces voisins, et ce
    /// qu'on trouve est retenu pour chaque suite traversée. Chaque suite n'est donc remontée
    /// qu'une fois, sans parcourir la liste où vit l'unité (`StickyIndex::get_offset` le faisait,
    /// pour chaque unité nommée).
    fn place(&mut self, id: Id) -> Result<Place, CrdtError> {
        let mut path: Vec<Id> = Vec::new();
        let mut at = id;
        let place = loop {
            let Some(run) = self.at(at.0, at.1)? else {
                break UNKNOWN;
            };
            if let Some(place) = run.place() {
                break place;
            }
            // Un élément intégré ne dépend que de ce qui l'a été avant lui ; une mise à jour
            // hostile peut boucler sur elle-même, et yrs la gardera en attente.
            if path.len() % 4096 == 4095 && path.len() > self.page.len() + self.update.len() {
                break UNKNOWN;
            }
            path.push((at.0, run.start));
            match run.origin().or(run.right()) {
                Some(next) => at = next,
                None => break UNKNOWN,
            }
        };
        for (client, start) in path {
            if self.in_page(client, start) {
                self.page.settle(client, start, place);
            } else {
                self.update.settle(client, start, place);
            }
        }
        Ok(place)
    }

    /// Vrai si yrs tient l'unité `id` pour une entrée de map. Un élément à origine droite posé à
    /// côté d'une telle unité n'est jamais récrit : yrs lui en donne la clé, et il est refusé
    /// d'après ce que yrs en fait (Ruling R723).
    pub(crate) fn keyed(&mut self, id: Id) -> Result<bool, CrdtError> {
        let place = self.place(id)?;
        // Une entrée ramassée ne donne plus de clé à personne : yrs n'y voit pas de voisin.
        Ok(place.keyed && self.lives(place.home))
    }

    /// Faux si rien ne peut vivre dans `home` : une plage ramassée, un type de la page qui n'en
    /// est plus un (supprimé, le ramasse-miettes a ramassé tout ce qu'il portait).
    fn lives(&self, home: Home) -> bool {
        match home {
            Home::Gone | Home::Unknown => false,
            Home::In(parent) => {
                !self.in_page(parent.0, parent.1)
                    || BranchID::get_nested(self.txn, &ID::new(ClientID::new(parent.0), parent.1))
                        .is_some()
            }
            _ => true,
        }
    }

    /// Ce qu'est l'unité `id` de la page : une plage ramassée si sa suite l'est, ou si le type où
    /// elle vit n'en est plus un (supprimé, le ramasse-miettes a ramassé tout ce qu'il portait).
    pub(crate) fn page_unit(&mut self, id: Id) -> Result<Unit, CrdtError> {
        let home = self.place(id)?.home;
        Ok(match home {
            Home::Unknown => Unit::Unknown,
            home if self.lives(home) => Unit::In(home),
            _ => Unit::Collected,
        })
    }

    /// Règles G et P2 (Rulings R827, R829) : les éléments de la mise à jour (au-dessus du plancher,
    /// après la coupe) que la page ramasse au lieu de les ranger, rendus pour être récrits en
    /// plages ramassées ([`super::cut::collect`]).
    ///
    /// - **G** : un voisin (origine, origine droite) est une plage ramassée, l'autre un élément. Yjs
    ///   en fait une plage ramassée (`Item.getMissing` met le parent à `null` dès que le voisin de
    ///   gauche ou de droite est un `GC`, et `Item.integrate` intègre alors un `GC`) ; yrs ne voit
    ///   pas de voisin du tout là où l'unité est ramassée, prend le parent de l'autre voisin, et
    ///   range l'élément : la page vivante et la page relue de son état ne le rangeaient pas pareil
    ///   (l'ordre d'intégration décide), ni le serveur et un navigateur.
    /// - **P2** : les deux voisins sont des éléments de deux parents différents. yrs comme Yjs le
    ///   rangent dans le parent de gauche ; mais si ce voisin est ramassé plus tard (son parent
    ///   supprimé), la page relue de son état le range dans le parent de droite (yrs) ou le ramasse
    ///   (Yjs) : la page vivante n'était plus sa relecture.
    ///
    /// Aucun client honnête n'écrit ni l'un ni l'autre : l'origine et l'origine droite d'un élément
    /// sont dans le même parent, et une page ramasse tous les enfants d'un parent supprimé à la fois.
    ///
    /// Seuls les éléments à deux voisins sont jugés, avec ce qu'ils nomment dans la mise à jour : un
    /// élément qui n'a qu'un voisin suit le sort de ce voisin chez yrs comme chez Yjs. `already` :
    /// les éléments déjà jugés à ramasser (règles I1 et I3), rendus avec les autres, et ramassés
    /// pour ceux qui les nomment.
    pub(crate) fn collected_neighbours(
        &mut self,
        shape: &Shape,
        already: HashSet<Id>,
    ) -> Result<HashSet<Id>, CrdtError> {
        let posed = shape.posed(self.floor, self.beyond);
        let mut flagged = already;
        // Où commencent et finissent, dans `posed`, les unités de chaque client.
        let mut spans: HashMap<u64, (usize, usize)> = HashMap::new();
        for (k, s) in posed.iter().enumerate() {
            spans.entry(s.id.0).or_insert((k, k)).1 = k + 1;
        }
        let rank = |id: Id| {
            let &(from, to) = spans.get(&id.0)?;
            let list = &posed[from..to];
            let at = list.partition_point(|s| s.id.1 + s.len <= id.1);
            list.get(at).filter(|s| s.id.1 <= id.1).map(|_| from + at)
        };
        let mut seen: Vec<Seen> = posed
            .iter()
            .map(|s| {
                if matches!(s.kind, Kind::Gc) || flagged.contains(&s.id) {
                    Seen::Done(Unit::Collected)
                } else {
                    Seen::New
                }
            })
            .collect();
        // En profondeur, sans récursion : un élément est jugé une fois ses voisins jugés.
        let mut stack = Vec::new();
        for root in 0..posed.len() {
            let two = matches!(&posed[root].kind, Kind::Insert(i) if i.origin.is_some() && i.right.is_some());
            if !two || !matches!(seen[root], Seen::New) {
                continue;
            }
            stack.push(root);
            while let Some(&k) = stack.last() {
                seen[k] = Seen::Visiting;
                let Kind::Insert(p) = &posed[k].kind else {
                    stack.pop();
                    continue;
                };
                let place = p.place();
                let named = match place {
                    Some(Place {
                        home: Home::In(parent),
                        ..
                    }) => [Some(parent), None],
                    Some(_) => [None, None],
                    None => [p.origin, p.right],
                };
                let waits = named
                    .into_iter()
                    .flatten()
                    .filter(|id| !self.in_page(id.0, id.1))
                    .filter_map(rank)
                    .find(|j| matches!(seen[*j], Seen::New));
                if let Some(j) = waits {
                    stack.push(j);
                    continue;
                }
                stack.pop();
                let unit_of = |judge: &mut Self, id: Id| -> Result<Unit, CrdtError> {
                    if judge.in_page(id.0, id.1) {
                        return judge.page_unit(id);
                    }
                    Ok(match rank(id).map(|j| seen[j]) {
                        Some(Seen::Done(unit)) => unit,
                        _ => Unit::Unknown,
                    })
                };
                let unit = match place {
                    Some(Place {
                        home: Home::In(parent),
                        ..
                    }) if self.in_page(parent.0, parent.1) => {
                        if self.lives(Home::In(parent)) {
                            Unit::In(Home::In(parent))
                        } else {
                            Unit::Collected
                        }
                    }
                    Some(Place {
                        home: Home::In(parent),
                        ..
                    }) => match (unit_of(self, parent)?, rank(parent).map(|j| &posed[j].kind)) {
                        // yrs ne range rien dans ce qui n'est pas un type.
                        (Unit::In(_), Some(Kind::Insert(named))) if named.type_ref.is_some() => {
                            Unit::In(Home::In(parent))
                        }
                        (Unit::Unknown, _) => Unit::Unknown,
                        _ => Unit::Collected,
                    },
                    Some(place) => Unit::In(place.home),
                    None => {
                        let origin = p.origin.map(|id| unit_of(self, id)).transpose()?;
                        let right = p.right.map(|id| unit_of(self, id)).transpose()?;
                        match (origin, right) {
                            (Some(Unit::Unknown), _) | (_, Some(Unit::Unknown)) | (None, None) => {
                                Unit::Unknown
                            }
                            (Some(a), Some(b)) if a != b => {
                                // Deux voisins qui ne vivent pas au même endroit. À côté d'une
                                // entrée de map, c'est la règle de R723 qui décide.
                                let keyed = match (p.origin, p.right) {
                                    (Some(o), Some(r)) => {
                                        (matches!(a, Unit::In(_)) && self.place(o)?.keyed)
                                            || (matches!(b, Unit::In(_)) && self.place(r)?.keyed)
                                    }
                                    _ => false,
                                };
                                if keyed {
                                    a
                                } else {
                                    flagged.insert(posed[k].id);
                                    Unit::Collected
                                }
                            }
                            (Some(unit), _) | (None, Some(unit)) => unit,
                        }
                    }
                };
                seen[k] = Seen::Done(unit);
            }
        }
        Ok(flagged)
    }

    /// Règle I1 (Ruling R828) : vrai si l'unité `right`, l'origine droite d'un élément qui n'a pas
    /// d'origine, en a une. Un tel élément se dit posé en tête de la liste, devant `right` ; or
    /// `right` a son origine à sa gauche, et l'auteur de l'élément la connaît (il connaît `right`,
    /// donc ce dont `right` dépend) : aucun client ne l'écrit, et yrs comme Yjs le rangent selon
    /// l'ordre où arrivent les éléments placés entre la tête et `right`.
    pub(crate) fn origin_less_before_a_placed_unit(
        &mut self,
        right: Id,
    ) -> Result<bool, CrdtError> {
        Ok(self
            .at(right.0, right.1)?
            .is_some_and(|run| !run.gc() && run.origin_of(right.0, right.1).is_some()))
    }

    /// Règle I3 (Ruling R834) : vrai si un élément d'origine `origin` et d'origine droite `right`
    /// se dit posé entre deux voisins que son auteur ne voyait pas côte à côte. Son auteur connaît
    /// `right`, donc l'origine de `right`, qui doit être `origin` ou à sa gauche. Deux unités d'une
    /// même chaîne sont rangées dans l'ordre de leurs horloges (chacune a pour origine celle
    /// d'avant, et un élément est toujours à droite de son origine) : l'origine de `right` dans la
    /// chaîne d'`origin`, plus loin qu'elle, est à sa droite, et `right` plus à droite encore.
    /// Aucun client ne l'écrit, et yrs comme Yjs rangent un tel élément selon l'ordre où arrivent
    /// ceux qui sont entre ses voisins.
    ///
    /// La forme inverse (l'origine droite d'`origin` dans la chaîne de `right`, avant elle) n'est
    /// pas une règle : l'origine droite d'une unité n'est pas toujours à sa droite. `redoItem` écrit
    /// une origine droite qui est, dans la suite de l'origine, avant elle (N3 ne la récrit que si
    /// ces unités peuvent tenir en un bloc), et un élément posé ensuite juste après une telle unité
    /// est honnête (scènes 8172 et 13429 du tirage penché vers les annulations, `redo-crossing.json`).
    pub(crate) fn crosses_what_it_knows(
        &mut self,
        origin: Id,
        right: Id,
    ) -> Result<bool, CrdtError> {
        let (Some(at_origin), Some(at_right)) =
            (self.at(origin.0, origin.1)?, self.at(right.0, right.1)?)
        else {
            return Ok(false);
        };
        if at_origin.gc() || at_right.gc() {
            return Ok(false);
        }
        match at_right.origin_of(right.0, right.1) {
            Some(before) if before.0 == origin.0 && origin.1 < before.1 => {
                self.chained(origin.0, origin.1, before.1)
            }
            _ => Ok(false),
        }
    }

    /// Vrai si chaque unité de `client` dans `]from, to]` a pour origine l'unité d'avant : elles
    /// sont rangées dans l'ordre de leurs horloges, `to` à droite de `from`. Lu d'une suite à l'autre
    /// (la page, la mise à jour), si bien que la réponse ne dépend pas de leur découpage : la page
    /// vivante et la page relue de son état jugent pareil.
    fn chained(&mut self, client: u64, from: u32, to: u32) -> Result<bool, CrdtError> {
        let mut clock = to;
        for _ in 0..MAX_STEPS {
            let Some(run) = self.at(client, clock)? else {
                return Ok(false);
            };
            if run.gc() {
                return Ok(false);
            }
            if run.start <= from {
                return Ok(true);
            }
            if !run.head(client) {
                return Ok(false);
            }
            clock = run.start - 1;
        }
        // Au-delà, tenu pour enchaîné : l'élément est ramassé, forme que la relecture de l'état
        // retrouve telle quelle (une réponse « non » au-delà de la borne ne serait pas celle de la
        // relecture, qui parcourt moins de suites).
        Ok(true)
    }

    /// Vrai si un élément d'origine `(A, o)` et d'origine droite `(A, r)`, `0 < r < o`, est rangé
    /// selon le découpage des blocs : une même suite peut porter les unités `r - 1` à `o`.
    pub(crate) fn placed_by_blocks(&mut self, origin: Id, right: Id) -> Result<bool, CrdtError> {
        let (client, wanted) = (origin.0, right.1 - 1);
        let mut clock = origin.1;
        for _ in 0..MAX_STEPS {
            let Some(run) = self.at(client, clock)? else {
                return Ok(false);
            };
            if run.gc() {
                return Ok(false);
            }
            if run.start <= wanted {
                return Ok(true);
            }
            if !run.head(client) {
                return Ok(false);
            }
            let Some(before) = self.at(client, run.start - 1)? else {
                return Ok(false);
            };
            if before.gc() || before.right() != run.right() {
                return Ok(false);
            }
            clock = run.start - 1;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(start: u32, end: u32, right: Option<Id>, head: bool) -> Run {
        let origin = head.then(|| (7, start.wrapping_sub(1)));
        Run::new((start, end), right, origin, false, None)
    }

    #[test]
    fn runs_join_when_they_follow_on_with_the_same_right_origin() {
        let mut runs = Runs::default();
        runs.insert(7, run(3, 5, Some((1, 0)), true));
        runs.insert(7, run(0, 3, Some((1, 0)), false));
        runs.insert(7, run(5, 6, Some((1, 0)), true));
        assert_eq!(runs.at(7, 4), Some(run(0, 6, Some((1, 0)), false)));
        // Une autre origine droite, ou une origine qui n'est pas l'unité d'avant : une autre suite.
        runs.insert(7, run(6, 8, Some((1, 1)), true));
        runs.insert(7, run(8, 9, Some((1, 1)), false));
        assert_eq!(runs.at(7, 6), Some(run(6, 8, Some((1, 1)), true)));
        assert_eq!(runs.at(7, 8), Some(run(8, 9, Some((1, 1)), false)));
        // Ce qui chevauche une suite connue n'est pas ajouté.
        runs.insert(7, run(4, 12, None, false));
        assert_eq!(runs.at(7, 10), None);
        assert_eq!(runs.unknown_from(7, 10), 9);
        assert_eq!(runs.unknown_from(8, 10), 0);
    }
}
