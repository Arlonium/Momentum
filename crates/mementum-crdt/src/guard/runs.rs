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

thread_local! {
    /// Les pas faits par [`Judge::descends_from`] sur ce fil : ce qui prouve, sans minutage, qu'un
    /// ancêtre se trouve en un nombre de pas logarithmique (Ruling R856).
    pub(crate) static ANCESTOR_STEPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

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
    /// Ce qu'on a lu de la place des suites dans l'arbre des origines, par première unité : rempli
    /// à la demande ([`Judge::climb`]). `None` : ses origines bouclent, ou manquent.
    climbs: HashMap<Id, Option<Climb>>,
}

/// La place d'une suite dans l'arbre des origines : le parent d'une unité est son origine. Avec un
/// saut vers un ancêtre (Myers, 1983), on trouve un ancêtre à une profondeur donnée en un nombre de
/// pas logarithmique, quelle que soit la façon dont les unités se découpent en suites.
#[derive(Clone, Copy)]
struct Climb {
    /// Le nombre d'unités de la racine à la première unité de la suite, elle comprise.
    depth: u32,
    /// Le nombre de suites de la racine à celle-ci, elle comprise.
    level: u32,
    /// Un ancêtre de la première unité (elle-même à la racine), sa profondeur, et le niveau de sa
    /// suite.
    jump: Id,
    jump_depth: u32,
    jump_level: u32,
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
        self.climbs.extend(other.climbs);
        for (client, mut list) in other.clients {
            match self.clients.entry(client) {
                Entry::Vacant(known) => {
                    list.shrink_to_fit();
                    known.insert(list);
                }
                Entry::Occupied(mut known) => Self::merge(known.get_mut(), client, list),
            }
        }
    }

    /// Pose `run` après les suites de `client`, fondue avec la dernière si elle la prolonge. Une
    /// suite qui ne vient pas après elles (des origines qui bouclent : yrs gardera la mise à jour en
    /// attente) n'est pas posée : les suites restent triées.
    fn pose(&mut self, client: u64, run: Run) {
        let list = self.clients.entry(client).or_default();
        match list.last_mut() {
            Some(last) if last.end > run.start => {}
            Some(last) if last.joins(client, &run) => last.end = run.end,
            _ => list.push(run),
        }
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

    /// Range `incoming`, par horloges croissantes, parmi les suites connues d'un client, en une
    /// passe (Ruling R855) : ce qu'on sait d'une unité ne change pas, seul ce que les suites connues
    /// ne couvrent pas est ajouté. yrs fond des blocs que la page a vus arriver séparément (une
    /// plage ramassée à cheval sur le plancher, la part neuve d'un élément coupé et l'unité d'avant) :
    /// une suite lue dans yrs peut chevaucher ce qui est connu.
    fn merge(known: &mut Vec<Run>, client: u64, incoming: Vec<Run>) {
        let push = |out: &mut Vec<Run>, run: Run| match out.last_mut() {
            Some(last) if last.joins(client, &run) => last.end = run.end,
            _ => out.push(run),
        };
        // Le cas ordinaire : tout arrive après ce qui est connu.
        if incoming
            .first()
            .is_none_or(|first| known.last().is_none_or(|last| last.end <= first.start))
        {
            for run in incoming {
                push(known, run);
            }
            return;
        }
        let mut out = Vec::with_capacity(known.len() + incoming.len());
        let mut old = std::mem::take(known).into_iter().peekable();
        for run in incoming {
            let mut from = run.start;
            while from < run.end {
                while let Some(before) = old.next_if(|before| before.end <= from) {
                    push(&mut out, before);
                }
                match old.peek() {
                    Some(covering) if covering.start <= from => from = covering.end,
                    next => {
                        let to = next.map_or(run.end, |next| next.start.min(run.end));
                        let piece = Run {
                            start: from,
                            end: to,
                            origin: Slot::of(run.origin_of(client, from)),
                            ..run
                        };
                        push(&mut out, piece);
                        from = to;
                    }
                }
            }
        }
        for after in old {
            push(&mut out, after);
        }
        *known = out;
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
            for s in structs.iter().filter(|s| s.is_posed(known, holes)) {
                let Some(run) = s.run(known, s.right()) else {
                    continue;
                };
                match list.last_mut() {
                    Some(last) if last.joins(*client, &run) => last.end = run.end,
                    _ => list.push(run),
                }
            }
            if !list.is_empty() {
                list.shrink_to_fit();
                out.clients.insert(*client, list);
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

    /// Vrai si la mise à jour intègre (au-dessus de `floor`) un élément qui a une origine et une
    /// origine droite, hors des entrées de map : les règles G, P2, N3 et V n'ont à juger que ceux-là.
    pub(crate) fn has_item_between_two(&self, floor: &StateVector) -> bool {
        self.inserts().any(|(s, i)| {
            !i.sub_flag
                && i.origin.is_some()
                && i.right.is_some()
                && s.id.1 + s.len > floor.get(&ClientID::new(s.id.0))
        })
    }
}

impl super::Struct {
    /// Vrai si la page intégrera ce bloc : au-dessus du plancher `known` de son client, hors de ce
    /// qu'elle connaît au-delà d'un trou (`holes`).
    fn is_posed(&self, known: u32, holes: Option<&Vec<(u32, u32)>>) -> bool {
        self.id.1 + self.len > known
            && !matches!(self.kind, Kind::Skip)
            && !holes
                .is_some_and(|ranges| ranges.iter().any(|r| r.0 <= self.id.1 && self.id.1 < r.1))
    }

    /// L'origine droite de l'élément, s'il en est un.
    fn right(&self) -> Option<Id> {
        match &self.kind {
            Kind::Insert(i) => i.right,
            _ => None,
        }
    }

    /// Les voisins que nomme l'élément : il dépend d'eux, et de ce dont ils dépendent.
    fn neighbours(&self) -> [Option<Id>; 2] {
        match &self.kind {
            Kind::Insert(i) => [i.origin, i.right],
            _ => [None, None],
        }
    }

    /// Le voisin de gauche et le voisin de droite d'un élément qui a les deux, hors des entrées de
    /// map : les candidats des règles N3 et V (Rulings R825, R856).
    fn between(&self) -> Option<(Id, Id)> {
        match &self.kind {
            Kind::Insert(i) if !i.sub_flag => Some((i.origin?, i.right?)),
            _ => None,
        }
    }

    /// La suite que pose ce bloc au-dessus du plancher `known` de son client, avec `right` pour
    /// origine droite. Une plage ramassée à cheval sur le plancher n'est pas coupée (Ruling R784 ne
    /// coupe que les éléments) : sa part connue n'est pas ramassée pour autant.
    fn run(&self, known: u32, right: Option<Id>) -> Option<Run> {
        let span = (self.id.1.max(known), self.id.1 + self.len);
        match &self.kind {
            Kind::Insert(i) => Some(Run::new(span, right, i.origin, false, i.place())),
            Kind::Gc => Some(Run::new(span, None, None, true, Some(GONE))),
            Kind::Skip => None,
        }
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
    /// Ce que la mise à jour pose et que la page intégrera : les éléments et les plages ramassées
    /// au-dessus du plancher de leur client, hors de ce que la page connaît au-delà d'un trou. Par
    /// client, dans l'ordre des horloges.
    fn posed(&self, floor: &StateVector, beyond: &Beyond) -> Vec<&super::Struct> {
        let mut out = Vec::new();
        for (client, structs) in &self.clients {
            let known = floor.get(&ClientID::new(*client));
            let holes = beyond.get(client);
            out.extend(structs.iter().filter(|s| s.is_posed(known, holes)));
        }
        out
    }
}

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
        let list = read.clients.remove(&client).unwrap_or_default();
        Runs::merge(self.page.clients.entry(client).or_default(), client, list);
        Ok(self.page.at(client, clock))
    }

    /// Où vit l'unité `id`, de la page ou de la mise à jour. yrs range un élément qui ne nomme pas
    /// son parent chez son origine, sinon chez son origine droite : on remonte ces voisins, et ce
    /// qu'on trouve est retenu pour chaque suite traversée. Chaque suite n'est donc remontée
    /// qu'une fois, sans parcourir la liste où vit l'unité (`StickyIndex::get_offset` le faisait,
    /// pour chaque unité nommée). Un élément intégré ne dépend que de ce qui l'a été avant lui ; une
    /// mise à jour hostile peut boucler sur elle-même : le parcours s'arrête à la première suite
    /// revue (Ruling R854), et yrs gardera la mise à jour en attente.
    fn place(&mut self, id: Id) -> Result<Place, CrdtError> {
        let mut path: HashSet<Id> = HashSet::new();
        let mut at = id;
        let place = loop {
            let Some(run) = self.at(at.0, at.1)? else {
                break UNKNOWN;
            };
            if let Some(place) = run.place() {
                break place;
            }
            if !path.insert((at.0, run.start)) {
                break UNKNOWN;
            }
            match run.origin().or(run.right()) {
                Some(next) => at = next,
                None => break UNKNOWN,
            }
        };
        // Une unité de la page dont un voisin manque ne le restera pas : un trou d'horloge peut se
        // combler. Seul ce qu'on sait se retient pour elle.
        for (client, start) in path {
            if !self.in_page(client, start) {
                self.update.settle(client, start, place);
            } else if place.home != Home::Unknown {
                self.page.settle(client, start, place);
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
    /// les éléments déjà jugés à ramasser (règle I1), rendus avec les autres, et ramassés
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
        if self.collected(right)? {
            return Ok(false);
        }
        Ok(self
            .at(right.0, right.1)?
            .is_some_and(|run| run.origin_of(right.0, right.1).is_some()))
    }

    /// Vrai si l'unité `id` est une plage ramassée. Une unité de la page se juge par ce qu'elle est
    /// aujourd'hui, jamais par ce que sa suite a été posée : la page vivante, qui se souvient d'un
    /// élément, et la page relue de son état, qui n'y lit qu'une plage ramassée, jugent ainsi pareil
    /// (Ruling R858).
    fn collected(&mut self, id: Id) -> Result<bool, CrdtError> {
        if self.in_page(id.0, id.1) {
            return Ok(self.page_unit(id)? == Unit::Collected);
        }
        Ok(self.update.at(id.0, id.1).is_some_and(|run| run.gc()))
    }

    /// Règles N3 et V (Rulings R825, R856) : les éléments de la mise à jour qui perdent leur origine
    /// droite, jugés **une fois chacun**, dans l'ordre où yrs les intègre, sur ce qui a déjà été
    /// récrit (Ruling R861). Un élément qui perd son origine droite peut tenir en une suite avec ce
    /// qui le précède, et un élément qui le nomme a alors la forme N3 : le juger sur la mise à jour
    /// reçue le laissait rangé selon le découpage des blocs. Ce qu'un élément nomme (l'unité d'avant
    /// de son client, son origine, son origine droite) est jugé avant lui, d'un client à l'autre ;
    /// rien de ce qui vient après ne change un verdict, et il n'y a donc pas de point fixe à chercher.
    ///
    /// Les suites de la mise à jour (`update`) sont refaites au fil de la passe, telles que yrs les
    /// intégrera.
    pub(crate) fn rights_to_drop(&mut self, shape: &Shape) -> Result<HashSet<Id>, CrdtError> {
        let posed = shape.posed(self.floor, self.beyond);
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
        *self.update = Runs::default();
        let mut flagged = HashSet::new();
        let mut waiting = vec![true; posed.len()];
        let mut stack = Vec::new();
        for root in 0..posed.len() {
            if !waiting[root] {
                continue;
            }
            waiting[root] = false;
            stack.push(root);
            // En profondeur, sans récursion. Une boucle (ce qu'on attend attend déjà) n'attend
            // personne : yrs gardera la mise à jour en attente, et la page la refusera.
            while let Some(&k) = stack.last() {
                let s = posed[k];
                let before = k.checked_sub(1).filter(|&j| posed[j].id.0 == s.id.0);
                let named = s.neighbours();
                let next = before
                    .into_iter()
                    .chain(
                        named
                            .into_iter()
                            .flatten()
                            .filter(|id| !self.in_page(id.0, id.1))
                            .filter_map(rank),
                    )
                    .find(|&j| waiting[j]);
                if let Some(j) = next {
                    waiting[j] = false;
                    stack.push(j);
                    continue;
                }
                stack.pop();
                let mut right = s.right();
                if let Some((origin, neighbour)) = s.between()
                    && self.loses_its_right_origin(origin, neighbour)?
                {
                    flagged.insert(s.id);
                    right = None;
                }
                let known = self.floor.get(&ClientID::new(s.id.0));
                if let Some(run) = s.run(known, right) {
                    self.update.pose(s.id.0, run);
                }
            }
        }
        Ok(flagged)
    }

    /// Vrai si un élément d'origine `origin` et d'origine droite `right` perd son origine droite :
    /// il sera rangé par son origine seule, partout pareil. La règle N3 (Ruling R825) ou la règle V
    /// (Ruling R856) le demandent, sauf à côté d'une entrée de map, où le refus de R723 répond.
    fn loses_its_right_origin(&mut self, origin: Id, right: Id) -> Result<bool, CrdtError> {
        if self.collected(origin)? || self.collected(right)? {
            return Ok(false);
        }
        let placed = origin.0 == right.0
            && right.1 < origin.1
            && right.1 > 0
            && self.placed_by_blocks(origin, right)?;
        Ok((placed || self.right_does_not_follow(origin, right)?)
            && !self.keyed(origin)?
            && !self.keyed(right)?)
    }

    /// Règle V (Ruling R856) : vrai si `right` ne peut pas être ce qui suit `origin` dans la liste
    /// que voyait l'auteur de l'élément. Une liste se lit comme l'arbre de ses origines, parcouru en
    /// profondeur : une unité est suivie de son premier enfant, ou, si elle n'en a pas, du prochain
    /// enfant d'un de ses ancêtres. Ce qui suit `origin` a donc pour origine `origin` lui-même ou un
    /// de ses ancêtres ; sinon, l'auteur ne voyait pas ces deux voisins côte à côte, et yrs comme
    /// Yjs rangent l'élément selon l'ordre où arrivent ceux qui sont entre eux.
    ///
    /// L'origine droite est retirée, l'élément gardé : `redoItem` écrit cette forme quand il suit
    /// séparément les deux voisins d'un élément rétabli (trois scènes sur 20 000 du tirage penché
    /// vers les annulations), et ramasser l'élément y perdrait le texte rétabli.
    fn right_does_not_follow(&mut self, origin: Id, right: Id) -> Result<bool, CrdtError> {
        let Some(run) = self.at(right.0, right.1)? else {
            return Ok(false);
        };
        match run.origin_of(right.0, right.1) {
            None => Ok(false),
            Some(parent) if parent == origin => Ok(false),
            Some(parent) => Ok(!self.descends_from(origin, parent)?),
        }
    }

    /// Vrai si `ancestor` est `id` ou un de ses ancêtres dans l'arbre des origines. Ce qui est
    /// répondu ne dépend que des unités, pas de leur découpage en suites : la page vivante et la
    /// page relue de son état répondent pareil.
    fn descends_from(&mut self, id: Id, ancestor: Id) -> Result<bool, CrdtError> {
        let Some(wanted) = self.depth(ancestor)? else {
            return Ok(false);
        };
        let mut at = id;
        loop {
            ANCESTOR_STEPS.with(|steps| steps.set(steps.get() + 1));
            let Some(run) = self.at(at.0, at.1)? else {
                return Ok(false);
            };
            let head = (at.0, run.start);
            let Some(climb) = self.climb(head)? else {
                return Ok(false);
            };
            if climb.depth + (at.1 - run.start) < wanted {
                return Ok(false);
            }
            if climb.depth <= wanted {
                return Ok(ancestor == (at.0, run.start + (wanted - climb.depth)));
            }
            at = if climb.jump != head && climb.jump_depth >= wanted {
                climb.jump
            } else {
                match run.origin() {
                    Some(parent) if !run.gc() => parent,
                    _ => return Ok(false),
                }
            };
        }
    }

    /// La profondeur de l'unité `id` dans l'arbre des origines ; `None` si ses origines bouclent.
    fn depth(&mut self, id: Id) -> Result<Option<u32>, CrdtError> {
        let Some(run) = self.at(id.0, id.1)? else {
            return Ok(None);
        };
        let climb = self.climb((id.0, run.start))?;
        Ok(climb.map(|climb| climb.depth + (id.1 - run.start)))
    }

    /// Ce qu'on sait de la suite qui commence à `head`, de la page ou de la mise à jour. Une suite
    /// sans origine est à la racine : rien n'est retenu pour elle (une page peut en porter une par
    /// lettre).
    fn climbed(&mut self, head: Id) -> Result<Option<Option<Climb>>, CrdtError> {
        let Some(run) = self.at(head.0, head.1)? else {
            return Ok(Some(None));
        };
        if run.gc() || run.origin_of(head.0, head.1).is_none() {
            return Ok(Some(Some(Climb {
                depth: 1,
                level: 1,
                jump: head,
                jump_depth: 1,
                jump_level: 1,
            })));
        }
        let runs = if self.in_page(head.0, head.1) {
            &*self.page
        } else {
            &*self.update
        };
        Ok(runs.climbs.get(&head).copied())
    }

    /// Retient la place d'une suite. Pour une suite de la page, seulement ce qu'on sait : un voisin
    /// qui manque peut arriver, quand un trou d'horloge se comble.
    fn remember_climb(&mut self, head: Id, climb: Option<Climb>) {
        if !self.in_page(head.0, head.1) {
            self.update.climbs.insert(head, climb);
        } else if climb.is_some() {
            self.page.climbs.insert(head, climb);
        }
    }

    /// La place de la suite qui commence à `head` dans l'arbre des origines, lue une fois, en
    /// remontant ses origines sans récursion. Des origines qui bouclent n'ont pas de place : yrs
    /// gardera la mise à jour en attente.
    fn climb(&mut self, head: Id) -> Result<Option<Climb>, CrdtError> {
        if let Some(known) = self.climbed(head)? {
            return Ok(known);
        }
        let mut stack = vec![head];
        let mut walked: HashSet<Id> = HashSet::from([head]);
        while let Some(&at) = stack.last() {
            if self.climbed(at)?.is_some() {
                stack.pop();
                continue;
            }
            // `climbed` a répondu pour une suite sans origine : celle-ci en a une.
            let parent = self
                .at(at.0, at.1)?
                .and_then(|run| run.origin_of(at.0, at.1));
            let Some((parent, parent_run)) = (match parent {
                Some(parent) => self.at(parent.0, parent.1)?.map(|run| (parent, run)),
                None => None,
            }) else {
                self.remember_climb(at, None);
                stack.pop();
                continue;
            };
            let parent_head = (parent.0, parent_run.start);
            match self.climbed(parent_head)? {
                None if walked.insert(parent_head) => {
                    stack.push(parent_head);
                    continue;
                }
                // Revue sans réponse : les origines bouclent.
                None | Some(None) => self.remember_climb(at, None),
                Some(Some(above)) => {
                    let climb = self.climb_under(parent, parent_run.start, above)?;
                    self.remember_climb(at, Some(climb));
                }
            }
            stack.pop();
        }
        Ok(self.climbed(head)?.flatten())
    }

    /// La place d'une suite dont l'origine est `parent`, dans la suite qui commence à `start` et
    /// dont la place est `above` : un saut à double portée quand les deux sauts d'au-dessus sont de
    /// même longueur, sinon l'origine elle-même.
    fn climb_under(&mut self, parent: Id, start: u32, above: Climb) -> Result<Climb, CrdtError> {
        let depth = above.depth + (parent.1 - start) + 1;
        let level = above.level + 1;
        let far = match self.at(above.jump.0, above.jump.1)? {
            Some(run) => self.climbed((above.jump.0, run.start))?.flatten(),
            None => None,
        };
        Ok(match far {
            Some(far) if above.level - above.jump_level == above.jump_level - far.jump_level => {
                Climb {
                    depth,
                    level,
                    jump: far.jump,
                    jump_depth: far.jump_depth,
                    jump_level: far.jump_level,
                }
            }
            _ => Climb {
                depth,
                level,
                jump: parent,
                jump_depth: depth - 1,
                jump_level: above.level,
            },
        })
    }

    /// Vrai si un élément d'origine `(A, o)` et d'origine droite `(A, r)`, `0 < r < o`, est rangé
    /// selon le découpage des blocs : une même suite peut porter les unités `r - 1` à `o`.
    fn placed_by_blocks(&mut self, origin: Id, right: Id) -> Result<bool, CrdtError> {
        let (client, wanted) = (origin.0, right.1 - 1);
        let mut clock = origin.1;
        // Deux suites qui se prolongent sont fondues : au plus une par source (la page, puis la
        // mise à jour), et ce parcours ne descend qu'une fois d'une horloge à une plus basse.
        loop {
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
        let insert = |runs: &mut Runs, run: Run| {
            Runs::merge(runs.clients.entry(7).or_default(), 7, vec![run]);
        };
        insert(&mut runs, run(3, 5, Some((1, 0)), true));
        insert(&mut runs, run(0, 3, Some((1, 0)), false));
        insert(&mut runs, run(5, 6, Some((1, 0)), true));
        assert_eq!(runs.at(7, 4), Some(run(0, 6, Some((1, 0)), false)));
        // Une autre origine droite, ou une origine qui n'est pas l'unité d'avant : une autre suite.
        insert(&mut runs, run(6, 8, Some((1, 1)), true));
        insert(&mut runs, run(8, 9, Some((1, 1)), false));
        assert_eq!(runs.at(7, 6), Some(run(6, 8, Some((1, 1)), true)));
        assert_eq!(runs.at(7, 8), Some(run(8, 9, Some((1, 1)), false)));
        // De ce qui chevauche une suite connue, seule la part qu'elle ne couvre pas est ajoutée.
        insert(&mut runs, run(4, 12, None, false));
        assert_eq!(runs.at(7, 4), Some(run(0, 6, Some((1, 0)), false)));
        assert_eq!(
            runs.at(7, 10).map(|run| (run.start, run.end)),
            Some((9, 12))
        );
        assert_eq!(runs.at(7, 10).and_then(|run| run.origin()), Some((7, 8)));
        assert_eq!(runs.unknown_from(7, 10), 9);
        assert_eq!(runs.unknown_from(8, 10), 0);
    }
}
