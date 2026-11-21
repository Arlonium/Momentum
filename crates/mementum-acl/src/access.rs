//! Ce qu'une personne voit d'un espace, et à quel niveau : [`SpaceAccess::compute`].

use std::collections::HashMap;

use uuid::Uuid;

use crate::{Action, Barred, Grant, Level, Role, Section, Subject};

/// Le rang d'une page sans parent visible, dans [`SpaceAccess::parents`].
const NO_PARENT: u32 = u32::MAX;

/// Un nœud de l'arbre d'un espace, réduit à ce dont le calcul des accès a besoin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    /// Le nœud lui-même est à la corbeille (`trashed_at` posé).
    pub trashed: bool,
}

/// Les accès d'une personne à un nœud qu'elle voit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeAccess {
    /// Le plus fort des niveaux que lui donnent ce nœud et ses ancêtres.
    pub level: Level,
    /// La section de son plus haut ancêtre visible (la sienne s'il n'en a pas).
    pub section: Section,
    /// La page ou un de ses ancêtres est à la corbeille.
    pub in_trash: bool,
    /// Le parent s'il est visible. `None` : racine, ou parent que la personne ne voit pas.
    pub visible_parent: Option<Uuid>,
    /// Les actions refusées quel que soit le niveau (l'interdit d'un jeton d'API, Ruling R1216) ;
    /// vide pour une session.
    pub barred: Barred,
    /// Le niveau du compte, quand un jeton d'API l'a ramené plus bas (`level` est alors celui du
    /// jeton) : de quoi dire, dans un refus, que c'est le jeton qui borne et non le compte (recette
    /// du lot 5, F5, Ruling R1549). `None` pour une session, et quand le jeton ne borne pas. Ne
    /// permet rien.
    pub capped_from: Option<Level>,
}

impl NodeAccess {
    /// Le niveau permet-il `action`, et l'interdit la laisse-t-il ? Ne regarde pas la corbeille.
    pub fn allows(&self, action: Action) -> bool {
        self.level.allows(action) && !self.barred.contains(action)
    }

    /// Ces accès vus par un jeton d'API (Ruling R1217) : le niveau ramené sous `cap`, `barred`
    /// ajouté à l'interdit, et « Partagé » pour un jeton à racines. La seule règle par page de la
    /// restriction, pour [`SpaceAccess::narrowed`], [`SpaceAccess::narrowed_node`] et une page
    /// qu'un jeton vient de créer ; le parent visible, lui, dépend de la portée.
    pub fn narrowed(self, cap: Level, barred: Barred, has_roots: bool) -> NodeAccess {
        NodeAccess {
            level: self.level.min(cap),
            capped_from: if cap < self.level {
                Some(self.capped_from.unwrap_or(self.level))
            } else {
                self.capped_from
            },
            section: if has_roots {
                Section::Shared
            } else {
                self.section
            },
            barred: self.barred.union(barred),
            ..self
        }
    }
}

/// Ce qu'une personne voit d'un espace. Calculé une fois par révision de l'espace (voir
/// [`AccessCache`](crate::AccessCache)), puis lu à chaque appel.
#[derive(Debug, Clone)]
pub struct SpaceAccess {
    role: Option<Role>,
    /// Les nœuds visibles, triés par id (recherche dichotomique).
    visible: Vec<(Uuid, NodeAccess)>,
    /// Pour chaque nœud de `visible`, au même rang, le rang de son parent visible
    /// ([`NO_PARENT`] sans lui) : 4 octets par page, pour remonter les parents visibles par des
    /// lectures d'un tableau, sans recherche (Rulings R1217, R1575).
    parents: Vec<u32>,
}

impl SpaceAccess {
    /// Calcule les accès de `user`, de rôle `role` dans l'espace. `None` : pas de ligne dans
    /// `members`, donc aucun droit (admin d'instance compris).
    ///
    /// `nodes` est tout l'arbre de l'espace, corbeille comprise, dans n'importe quel ordre ;
    /// `grants`, ses lignes d'accès `(nœud, accès)`. Règles (« Modèle des droits ») :
    /// - le niveau sur un nœud est le plus fort de ceux que donnent ce nœud et ses ancêtres :
    ///   `user:<user>` donne son niveau ; `members` donne le sien à un `member`, et `full` à un
    ///   `owner` ou un `admin` ; rien à un invité ;
    /// - la section se décide sur le plus haut nœud visible, et ses descendants la reprennent :
    ///   « Espace » si la personne est membre et qu'une ligne `members` est posée sur ce nœud
    ///   ou un de ses ancêtres ; « Privé » si aucune ligne de cette chaîne ne nomme un autre
    ///   sujet que `user:<user>`, et jamais pour un invité (« un invité ne voit que
    ///   Partagé ») ; « Partagé » sinon ;
    /// - un parent absent de `nodes` vaut la racine. Un nœud pris dans un cycle (que le store
    ///   interdit), ou suspendu sous un cycle, n'a pas de racine : personne ne le voit.
    ///
    /// Un seul parcours de haut en bas, sans récursion, où chaque nœud passe au plus une fois.
    pub fn compute(
        nodes: &[TreeNode],
        grants: &[(Uuid, Grant)],
        user: Uuid,
        role: Option<Role>,
    ) -> SpaceAccess {
        let Some(role) = role else {
            return SpaceAccess {
                role: None,
                visible: Vec::new(),
                parents: Vec::new(),
            };
        };

        let mut lines: HashMap<Uuid, Lines> = HashMap::new();
        for &(node, grant) in grants {
            lines.entry(node).or_default().add(grant, user, role);
        }

        // Le premier nœud de chaque id : un doublon (que le store interdit) est ignoré.
        let mut first: HashMap<Uuid, usize> = HashMap::with_capacity(nodes.len());
        for (i, node) in nodes.iter().enumerate() {
            first.entry(node.id).or_insert(i);
        }
        // Chaque nœud est rangé une fois, sous son parent ou parmi les racines : le parcours
        // ne passe donc qu'une fois par nœud, et n'atteint jamais un cycle.
        let mut children: HashMap<Uuid, Vec<&TreeNode>> = HashMap::new();
        let mut stack: Vec<(&TreeNode, Chain)> = Vec::new();
        for (i, node) in nodes.iter().enumerate() {
            if first[&node.id] != i {
                continue;
            }
            match node.parent_id.filter(|p| first.contains_key(p)) {
                Some(parent) => children.entry(parent).or_default().push(node),
                None => stack.push((node, Chain::default())),
            }
        }

        let mut visible = Vec::new();
        while let Some((node, above)) = stack.pop() {
            let own = lines.get(&node.id).copied().unwrap_or_default();
            let mut chain = Chain {
                level: above.level.max(own.level),
                section: above.section,
                members_line: above.members_line || own.members_line,
                other_subject: above.other_subject || own.other_subject,
                in_trash: above.in_trash || node.trashed,
            };
            if let Some(level) = chain.level {
                let parent_visible = above.level.is_some();
                // Le plus haut nœud visible décide de la section ; ses descendants la reprennent.
                let section = match above.section {
                    Some(section) => section,
                    None => chain.top_section(role),
                };
                chain.section = Some(section);
                visible.push((
                    node.id,
                    NodeAccess {
                        level,
                        section,
                        in_trash: chain.in_trash,
                        visible_parent: node.parent_id.filter(|_| parent_visible),
                        barred: Barred::NONE,
                        capped_from: None,
                    },
                ));
            }
            if let Some(kids) = children.get(&node.id) {
                stack.extend(kids.iter().map(|&kid| (kid, chain)));
            }
        }
        visible.sort_unstable_by_key(|&(id, _)| id);
        // Gardé en cache : pas de place réservée en trop.
        visible.shrink_to_fit();
        let parents = ranks_of_parents(&visible);
        SpaceAccess {
            role: Some(role),
            visible,
            parents,
        }
    }

    /// Ce calcul restreint à un jeton d'API (Ruling R1217) : ne restent que les pages qu'il voit
    /// déjà et qui sont dans la portée, à un niveau qui ne dépasse pas `cap`, avec `barred` en
    /// plus de leur interdit. La restriction ne peut que retirer : elle ne lit aucune ligne
    /// d'accès, et ne rend jamais une page que ce calcul ne rend pas, ni un niveau plus haut.
    ///
    /// `roots` (triées par identifiant) : les pages de la portée, chacune avec ses sous-pages.
    /// Une page est dans la portée si la chaîne de ses **parents visibles** atteint l'une d'elles
    /// (Ruling R1574) : une page que la personne voit sous une racine qu'elle ne voit plus en est
    /// sortie. `None` : toute la portée de l'espace. Le parent visible d'une page de la portée
    /// reste nommé s'il est lui-même dans la portée, sinon il devient `None` (une racine de la
    /// portée sous une page hors d'elle) ; avec `roots`, toute page est en « Partagé » et le
    /// rôle devient `guest` ; sans, la section reste, et le rôle vaut au plus `member`.
    ///
    /// En proportion des pages vues, par les rangs des parents ([`SpaceAccess::parents`]).
    pub fn narrowed(&self, cap: Level, barred: Barred, roots: Option<&[Uuid]>) -> SpaceAccess {
        debug_assert!(roots.is_none_or(|roots| roots.is_sorted()));
        let in_scope = match roots {
            None => vec![true; self.visible.len()],
            Some(roots) => self.scope_of_all(roots),
        };
        let mut new_rank = vec![NO_PARENT; self.visible.len()];
        let mut visible = Vec::with_capacity(in_scope.iter().filter(|&&kept| kept).count());
        for (i, &(id, _)) in self.visible.iter().enumerate() {
            if in_scope[i] {
                new_rank[i] = rank(visible.len());
                visible.push((id, self.visible[i].1));
            }
        }
        let mut parents = Vec::with_capacity(visible.len());
        for (i, &(_, access)) in self.visible.iter().enumerate() {
            if !in_scope[i] {
                continue;
            }
            let parent = self.parents[i];
            let parent_in_scope = parent != NO_PARENT && in_scope[parent as usize];
            let kept = rank_index(new_rank[i]);
            visible[kept].1 = restrict(access, cap, barred, roots.is_some(), parent_in_scope);
            parents.push(if parent_in_scope {
                new_rank[parent as usize]
            } else {
                NO_PARENT
            });
        }
        SpaceAccess {
            role: self.role.map(|role| narrowed_role(role, roots.is_some())),
            visible,
            parents,
        }
    }

    /// Ce que [`SpaceAccess::narrowed`] donnerait à la seule page `id`, sans restreindre tout
    /// l'espace (Ruling R1256) : la page est jugée par la remontée de ses parents visibles
    /// jusqu'à une page de `roots`, par leurs rangs, puis son niveau est ramené sous `cap` et
    /// `barred` posé. `None` : la personne ne la voit pas, ou elle est hors de la portée. En
    /// proportion de la profondeur de la page (Ruling R1575).
    pub fn narrowed_node(
        &self,
        id: Uuid,
        cap: Level,
        barred: Barred,
        roots: Option<&[Uuid]>,
    ) -> Option<NodeAccess> {
        debug_assert!(roots.is_none_or(|roots| roots.is_sorted()));
        let i = self.rank_of(id)?;
        let access = self.visible[i].1;
        let (in_scope, parent_in_scope) = match roots {
            // Toute page vue est dans la portée, son parent visible aussi.
            None => (true, access.visible_parent.is_some()),
            Some(roots) => {
                let above = self.reaches_a_root(self.parents[i], roots);
                (above || roots.binary_search(&id).is_ok(), above)
            }
        };
        in_scope.then(|| restrict(access, cap, barred, roots.is_some(), parent_in_scope))
    }

    /// Pour chaque nœud de `visible`, au même rang : son parent visible, lu par les rangs. Pour
    /// les tests : il vaut toujours `visible_parent`.
    pub fn parents_by_rank(&self) -> impl Iterator<Item = (Uuid, Option<Uuid>)> + '_ {
        self.visible
            .iter()
            .zip(&self.parents)
            .map(|(&(id, _), &parent)| {
                let parent = (parent != NO_PARENT).then(|| self.visible[parent as usize].0);
                (id, parent)
            })
    }

    /// Le rang de `id` dans `visible`.
    fn rank_of(&self, id: Uuid) -> Option<usize> {
        self.visible
            .binary_search_by_key(&id, |&(node, _)| node)
            .ok()
    }

    /// La chaîne des parents visibles qui part du rang `from` (lui compris) atteint-elle une
    /// page de `roots` ? Les parents visibles forment une forêt (un parent est visité avant ses
    /// enfants dans `compute`) ; la borne ne sert qu'à ne jamais tourner sans fin.
    fn reaches_a_root(&self, mut from: u32, roots: &[Uuid]) -> bool {
        for _ in 0..=self.visible.len() {
            if from == NO_PARENT {
                return false;
            }
            let at = from as usize;
            if roots.binary_search(&self.visible[at].0).is_ok() {
                return true;
            }
            from = self.parents[at];
        }
        false
    }

    /// Pour chaque nœud de `visible`, au même rang : est-il dans la portée de `roots` ? Une
    /// remontée par nœud, chaque nœud jugé une fois (les chemins remontés sont notés).
    fn scope_of_all(&self, roots: &[Uuid]) -> Vec<bool> {
        const UNKNOWN: u8 = 0;
        const IN: u8 = 1;
        const OUT: u8 = 2;
        let mut state = vec![UNKNOWN; self.visible.len()];
        let mut path = Vec::new();
        for start in 0..self.visible.len() {
            let mut at = start;
            let verdict = loop {
                if state[at] != UNKNOWN {
                    break state[at];
                }
                if roots.binary_search(&self.visible[at].0).is_ok() {
                    state[at] = IN;
                    break IN;
                }
                path.push(at);
                let parent = self.parents[at];
                if parent == NO_PARENT || path.len() > self.visible.len() {
                    break OUT;
                }
                at = parent as usize;
            };
            for node in path.drain(..) {
                state[node] = verdict;
            }
        }
        state.into_iter().map(|s| s == IN).collect()
    }

    /// Le rôle de la personne dans l'espace.
    pub fn role(&self) -> Option<Role> {
        self.role
    }

    /// Ses accès au nœud `id`. `None` : elle ne le voit pas (ou il n'existe pas).
    pub fn node(&self, id: Uuid) -> Option<&NodeAccess> {
        self.visible
            .binary_search_by_key(&id, |&(node, _)| node)
            .ok()
            .map(|i| &self.visible[i].1)
    }

    /// Son niveau sur `id` permet-il `action`, et l'interdit d'un jeton la laisse-t-il ? Ne
    /// regarde pas la corbeille : l'appelant refuse lui-même d'écrire dans une page `in_trash`.
    pub fn can(&self, action: Action, id: Uuid) -> bool {
        self.node(id).is_some_and(|a| a.allows(action))
    }

    /// Les nœuds qu'elle voit, corbeille comprise, par id croissant.
    pub fn visible(&self) -> impl Iterator<Item = (Uuid, &NodeAccess)> + '_ {
        self.visible.iter().map(|(id, access)| (*id, access))
    }

    /// Le nombre de nœuds qu'elle voit : ce que pèse le calcul dans le cache.
    pub(crate) fn visible_count(&self) -> usize {
        self.visible.len()
    }
}

/// Le rang d'un nœud de `visible`, en 32 bits : un espace a moins de quatre milliards de pages.
fn rank(index: usize) -> u32 {
    u32::try_from(index)
        .ok()
        .filter(|&r| r != NO_PARENT)
        .expect("moins de 2^32 - 1 pages visibles")
}

fn rank_index(rank: u32) -> usize {
    rank as usize
}

/// Les rangs des parents visibles de `visible` (triés par id), au même rang.
fn ranks_of_parents(visible: &[(Uuid, NodeAccess)]) -> Vec<u32> {
    let mut parents: Vec<u32> = visible
        .iter()
        .map(|(_, access)| {
            access
                .visible_parent
                .and_then(|parent| visible.binary_search_by_key(&parent, |&(id, _)| id).ok())
                .map_or(NO_PARENT, rank)
        })
        .collect();
    parents.shrink_to_fit();
    parents
}

/// Les accès d'une page de la portée d'un jeton : le niveau ramené sous `cap`, l'interdit
/// ajouté, « Partagé » pour un jeton à racines, et le parent visible gardé s'il est dans la
/// portée.
fn restrict(
    access: NodeAccess,
    cap: Level,
    barred: Barred,
    has_roots: bool,
    parent_in_scope: bool,
) -> NodeAccess {
    NodeAccess {
        visible_parent: access.visible_parent.filter(|_| parent_in_scope),
        ..access.narrowed(cap, barred, has_roots)
    }
}

/// Le rôle d'un compte vu par son jeton : `guest` pour un jeton à racines (ce que voit un
/// invité, Ruling R57), au plus `member` sinon.
fn narrowed_role(role: Role, has_roots: bool) -> Role {
    if has_roots {
        Role::Guest
    } else if role.rank() > Role::Member.rank() {
        Role::Member
    } else {
        role
    }
}

/// Les lignes posées sur un nœud, résumées pour la personne du calcul.
#[derive(Debug, Clone, Copy, Default)]
struct Lines {
    /// Le plus fort niveau qu'elles lui donnent.
    level: Option<Level>,
    /// Une ligne `members` est posée.
    members_line: bool,
    /// Une ligne nomme un autre sujet que `user:<elle>`.
    other_subject: bool,
}

impl Lines {
    fn add(&mut self, grant: Grant, user: Uuid, role: Role) {
        self.members_line |= grant.subject == Subject::Members;
        self.other_subject |= grant.subject != Subject::User(user);
        self.level = self.level.max(line_level(grant, user, role));
    }
}

/// Le niveau qu'une ligne donne à `user`, de rôle `role` : la sienne, son niveau ; `members`,
/// celui de [`members_level`] ; celle d'un autre, rien. La seule règle par ligne, pour
/// [`SpaceAccess::compute`] comme pour [`chain_level`].
fn line_level(grant: Grant, user: Uuid, role: Role) -> Option<Level> {
    match grant.subject {
        Subject::User(id) if id == user => Some(grant.level),
        Subject::User(_) => None,
        Subject::Members => members_level(role, grant.level),
    }
}

/// Le niveau de `user`, de rôle `role`, sur une page dont `chain` porte les lignes : celles de
/// la page et de tous ses ancêtres jusqu'à la racine, dans n'importe quel ordre. C'est le
/// niveau que [`SpaceAccess::compute`] donne à la page (test `chain_level_matches_compute`),
/// sans calculer tout l'espace : le store s'en sert dans la transaction d'un changement
/// d'accès. `None` : la personne ne voit pas la page.
///
/// À l'appelant de s'assurer que la chaîne monte jusqu'à une racine : un nœud pris dans un
/// cycle de parents, ou suspendu sous un cycle, n'est vu par personne. La corbeille ne change
/// pas le niveau.
pub fn chain_level<'a>(
    chain: impl IntoIterator<Item = &'a Grant>,
    user: Uuid,
    role: Role,
) -> Option<Level> {
    chain
        .into_iter()
        .filter_map(|&grant| line_level(grant, user, role))
        .max()
}

/// Ce qu'une ligne `(members, level)` donne à quelqu'un de rôle `role`.
fn members_level(role: Role, level: Level) -> Option<Level> {
    match role {
        // Un `owner` ou un `admin` a `full` partout où les membres voient.
        Role::Owner | Role::Admin => Some(Level::Full),
        Role::Member => Some(level),
        Role::Guest => None,
    }
}

/// Ce qu'un nœud reçoit de lui-même et de ses ancêtres.
#[derive(Debug, Clone, Copy, Default)]
struct Chain {
    /// Le plus fort niveau de la personne sur la chaîne. `None` : elle ne voit pas le nœud.
    level: Option<Level>,
    /// La section du plus haut nœud visible de la chaîne : `Some` dès que `level` l'est.
    section: Option<Section>,
    /// Une ligne `members` est posée sur la chaîne.
    members_line: bool,
    /// Une ligne de la chaîne nomme un autre sujet que `user:<la personne>`.
    other_subject: bool,
    in_trash: bool,
}

impl Chain {
    /// La section d'un plus haut nœud visible, d'après les lignes de sa chaîne.
    fn top_section(&self, role: Role) -> Section {
        if role.is_member() && self.members_line {
            Section::Space
        } else if role.is_member() && !self.other_subject {
            Section::Private
        } else {
            Section::Shared
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gardé en cache, un calcul ne réserve pas de place en trop : son poids dans le cache (ses
    /// nœuds visibles) est bien ce qu'il occupe.
    #[test]
    fn visible_nodes_take_no_spare_room() {
        let user = Uuid::from_u128(1);
        let nodes: Vec<TreeNode> = (0..100)
            .map(|i| TreeNode {
                id: Uuid::from_u128(1_000 + i),
                parent_id: None,
                trashed: false,
            })
            .collect();
        // Un nœud sur trois visible : le calcul en écarte, sa liste ne doit pas garder leur place.
        let grants: Vec<(Uuid, Grant)> = nodes
            .iter()
            .step_by(3)
            .map(|n| {
                let grant = Grant {
                    subject: Subject::User(user),
                    level: Level::Read,
                };
                (n.id, grant)
            })
            .collect();
        let access = SpaceAccess::compute(&nodes, &grants, user, Some(Role::Member));
        assert_eq!(access.visible_count(), 34);
        assert_eq!(access.visible.capacity(), access.visible.len());
    }
}
