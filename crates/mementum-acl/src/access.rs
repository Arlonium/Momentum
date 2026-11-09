//! Ce qu'une personne voit d'un espace, et à quel niveau : [`SpaceAccess::compute`].

use std::collections::HashMap;

use uuid::Uuid;

use crate::{Action, Grant, Level, Role, Section, Subject};

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
}

/// Ce qu'une personne voit d'un espace. Calculé une fois par révision de l'espace (voir
/// [`AccessCache`](crate::AccessCache)), puis lu à chaque appel.
#[derive(Debug, Clone)]
pub struct SpaceAccess {
    role: Option<Role>,
    /// Les nœuds visibles, triés par id (recherche dichotomique).
    visible: Vec<(Uuid, NodeAccess)>,
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
                    },
                ));
            }
            if let Some(kids) = children.get(&node.id) {
                stack.extend(kids.iter().map(|&kid| (kid, chain)));
            }
        }
        visible.sort_unstable_by_key(|&(id, _)| id);
        SpaceAccess {
            role: Some(role),
            visible,
        }
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

    /// Son niveau sur `id` permet-il `action` ? Ne regarde pas la corbeille : l'appelant
    /// refuse lui-même d'écrire dans une page `in_trash`.
    pub fn can(&self, action: Action, id: Uuid) -> bool {
        self.node(id).is_some_and(|a| a.level.allows(action))
    }

    /// Les nœuds qu'elle voit, corbeille comprise, par id croissant.
    pub fn visible(&self) -> impl Iterator<Item = (Uuid, &NodeAccess)> + '_ {
        self.visible.iter().map(|(id, access)| (*id, access))
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
        let level = match grant.subject {
            Subject::User(id) if id == user => Some(grant.level),
            Subject::User(_) => None,
            Subject::Members => {
                self.members_line = true;
                members_level(role, grant.level)
            }
        };
        self.other_subject |= grant.subject != Subject::User(user);
        self.level = self.level.max(level);
    }
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
