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

use std::collections::HashMap;

use yrs::block::ClientID;
use yrs::{ReadTxn, StateVector};

use super::cut::Beyond;
use super::{Id, Kind, Shape};
use crate::CrdtError;

/// Une suite d'unités d'un client qui peuvent tenir en un seul bloc de yrs ou de Yjs : elles se
/// suivent par leurs horloges, chacune après la première a pour origine l'unité d'avant, et toutes
/// ont la même origine droite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    pub(crate) start: u32,
    /// Exclue.
    pub(crate) end: u32,
    pub(crate) right: Option<Id>,
    /// L'origine de sa première unité (celle des suivantes est l'unité d'avant).
    pub(crate) origin: Option<Id>,
    /// Une plage ramassée : ni origine ni voisins, elle ne tient avec rien.
    pub(crate) gc: bool,
}

impl Run {
    /// L'origine de sa première unité est l'unité d'avant du même client (`client`).
    fn head(&self, client: u64) -> bool {
        self.start > 0 && self.origin == Some((client, self.start - 1))
    }

    /// Vrai si `next` prolonge `self` (les deux de `client`) : ils tiennent en une suite.
    fn joins(&self, client: u64, next: &Run) -> bool {
        self.end == next.start
            && !self.gc
            && !next.gc
            && next.head(client)
            && self.right == next.right
    }

    /// L'origine de l'unité `clock` de la suite (de `client`).
    fn origin_of(&self, client: u64, clock: u32) -> Option<Id> {
        if clock == self.start {
            self.origin
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

    /// Ajoute une suite. Une suite qui en chevauche une connue n'est pas ajoutée : ce qu'on sait
    /// d'une unité ne change pas, et ce qui est connu vient de ce que la page a intégré.
    pub(crate) fn insert(&mut self, client: u64, run: Run) {
        if run.start >= run.end {
            return;
        }
        let runs = self.clients.entry(client).or_default();
        let at = runs.partition_point(|r| r.start < run.start);
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
    pub(crate) fn runs(&self, floor: &StateVector, beyond: &Beyond) -> Vec<(u64, Run)> {
        let mut out: Vec<(u64, Run)> = Vec::new();
        for (client, structs) in &self.clients {
            let known = floor.get(&ClientID::new(*client));
            let holes = beyond.get(client);
            for s in structs {
                let end = s.id.1 + s.len;
                if end <= known
                    || holes
                        .is_some_and(|ranges| ranges.iter().any(|r| r.0 <= s.id.1 && s.id.1 < r.1))
                {
                    continue;
                }
                let run = match &s.kind {
                    Kind::Insert(i) => Run {
                        start: s.id.1,
                        end,
                        right: i.right,
                        origin: i.origin,
                        gc: false,
                    },
                    Kind::Gc => Run {
                        start: s.id.1,
                        end,
                        right: None,
                        origin: None,
                        gc: true,
                    },
                    Kind::Skip => continue,
                };
                match out.last_mut() {
                    Some((c, last)) if *c == *client && last.joins(*client, &run) => {
                        last.end = run.end;
                    }
                    _ => out.push((*client, run)),
                }
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

/// Le parent qu'un élément nomme dans ses octets.
pub(crate) enum Named {
    /// Une racine, par son nom.
    Root(String),
    /// Un type, par l'identifiant de son élément.
    Id(Id),
    /// Aucun : ses voisins le lui donnent.
    None,
}

/// Un élément ou une plage ramassée que la mise à jour pose et que la page intégrera, avec ce qui
/// le place : son origine, son origine droite, le parent qu'il nomme (règles G et P2, Rulings
/// R827, R829).
pub(crate) struct Posed {
    pub(crate) id: Id,
    pub(crate) len: u32,
    /// Une plage ramassée (`GC`).
    pub(crate) gc: bool,
    pub(crate) origin: Option<Id>,
    pub(crate) right: Option<Id>,
    pub(crate) parent: Named,
}

impl Shape {
    /// Ce que la mise à jour pose et que la page intégrera (comme [`Shape::runs`]) : les éléments
    /// et les plages ramassées au-dessus du plancher de leur client, hors de ce que la page connaît
    /// au-delà d'un trou. Par client, dans l'ordre des horloges.
    pub(crate) fn posed(&self, floor: &StateVector, beyond: &Beyond) -> Vec<Posed> {
        let mut out = Vec::new();
        for (client, structs) in &self.clients {
            let known = floor.get(&ClientID::new(*client));
            let holes = beyond.get(client);
            for s in structs {
                if s.id.1 + s.len <= known
                    || holes
                        .is_some_and(|ranges| ranges.iter().any(|r| r.0 <= s.id.1 && s.id.1 < r.1))
                {
                    continue;
                }
                match &s.kind {
                    Kind::Insert(i) => out.push(Posed {
                        id: s.id,
                        len: s.len,
                        gc: false,
                        origin: i.origin,
                        right: i.right,
                        parent: match &i.parent {
                            super::Parent::Id(p) => Named::Id(*p),
                            super::Parent::Named(root, _) => Named::Root(root.clone()),
                            super::Parent::Sibling(_) => Named::None,
                        },
                    }),
                    Kind::Gc => out.push(Posed {
                        id: s.id,
                        len: s.len,
                        gc: true,
                        origin: None,
                        right: None,
                        parent: Named::None,
                    }),
                    Kind::Skip => {}
                }
            }
        }
        out
    }
}

/// Au plus tant de suites parcourues pour juger un élément ; au-delà, il est tenu pour dangereux
/// (récrit). Les suites d'une même source sont fondues dès qu'elles se prolongent : un élément
/// n'en traverse d'ordinaire que deux (la page, puis la mise à jour).
const MAX_STEPS: usize = 64;

/// Ce qu'on sait des unités d'un client, pour juger les candidats d'une mise à jour : les suites
/// de la page (`page`), qu'on complète d'après yrs pour ce qu'elles ne connaissent pas encore (une
/// écriture du serveur), et celles que la mise à jour pose (`update`).
pub(crate) struct Judge<'a, T: ReadTxn> {
    pub(crate) txn: &'a T,
    pub(crate) floor: &'a StateVector,
    pub(crate) beyond: &'a Beyond,
    pub(crate) page: &'a mut Runs,
    pub(crate) update: &'a Runs,
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
        for (c, run) in shape.runs(&StateVector::default(), &Beyond::new()) {
            if c == client {
                self.page.insert(c, run);
            }
        }
        Ok(self.page.at(client, clock))
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
            .is_some_and(|run| !run.gc && run.origin_of(right.0, right.1).is_some()))
    }

    /// Règle I3 (Ruling R834) : vrai si un élément d'origine `origin` et d'origine droite `right`
    /// se dit posé entre deux voisins que son auteur ne voyait pas côte à côte. Son auteur connaît
    /// `right`, donc l'origine de `right` ; et `origin`, donc l'origine droite d'`origin`. L'origine
    /// de `right` doit être `origin` ou à sa gauche, l'origine droite d'`origin` être `right` ou à
    /// sa droite. Deux unités d'une même suite sont rangées dans l'ordre de leurs horloges (chacune
    /// a pour origine celle d'avant) : l'origine de `right` dans la suite d'`origin`, plus loin
    /// qu'elle, est à sa droite ; l'origine droite d'`origin` dans la suite de `right`, avant elle,
    /// est à sa gauche. Aucun client ne l'écrit en tapant, et yrs comme Yjs rangent un tel élément
    /// selon l'ordre où arrivent ceux qui sont entre ses voisins.
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
        if at_origin.gc || at_right.gc {
            return Ok(false);
        }
        if let Some(before) = at_right.origin_of(right.0, right.1)
            && before.0 == origin.0
            && origin.1 < before.1
            && self.chained(origin.0, origin.1, before.1)?
        {
            return Ok(true);
        }
        match at_origin.right {
            Some(after) if after.0 == right.0 && after.1 < right.1 => {
                self.chained(right.0, after.1, right.1)
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
            if run.gc {
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
            if run.gc {
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
            if before.gc || before.right != run.right {
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
        Run {
            start,
            end,
            right,
            origin: if head {
                Some((7, start.wrapping_sub(1)))
            } else {
                None
            },
            gc: false,
        }
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
