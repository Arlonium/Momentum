//! Calcul des accès d'un espace (`SpaceAccess::compute`) et cache des accès.
//!
//! Les règles sont celles du « Modèle des droits » du plan du lot 2 : niveau hérité des
//! ancêtres, ligne `members` pour les rôles `owner`, `admin` et `member`, `full` pour un
//! `owner` ou un `admin` là où une ligne `members` s'applique, section décidée sur le plus
//! haut nœud que la personne voit.

use std::sync::Arc;

use mementum_acl::{
    AccessCache, Action, Grant, Level, NodeAccess, Role, Section, SpaceAccess, Subject, TreeNode,
};
use proptest::prelude::*;
use proptest::sample::Index;
use uuid::Uuid;

const LEVELS: [Level; 4] = [Level::Read, Level::Comment, Level::Edit, Level::Full];

const ACTIONS: [Action; 11] = [
    Action::Read,
    Action::Comment,
    Action::Edit,
    Action::Rename,
    Action::CreateChild,
    Action::Reorder,
    Action::Trash,
    Action::Restore,
    Action::Move,
    Action::Share,
    Action::Delete,
];

fn id(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn root(id: Uuid) -> TreeNode {
    TreeNode {
        id,
        parent_id: None,
        trashed: false,
    }
}

fn child(id: Uuid, parent: Uuid) -> TreeNode {
    TreeNode {
        id,
        parent_id: Some(parent),
        trashed: false,
    }
}

fn grant(node: Uuid, subject: Subject, level: Level) -> (Uuid, Grant) {
    (node, Grant { subject, level })
}

/// Niveau et section d'un nœud, s'il est visible.
fn seen(access: &SpaceAccess, node: Uuid) -> Option<(Level, Section)> {
    access.node(node).map(|a| (a.level, a.section))
}

#[test]
fn level_allows_matrix() {
    // Colonnes : read, comment, edit, full.
    #[rustfmt::skip]
    let matrix: [(Action, [bool; 4]); 11] = [
        (Action::Read,        [true,  true,  true,  true]),
        (Action::Comment,     [false, true,  true,  true]),
        (Action::Edit,        [false, false, true,  true]),
        (Action::Rename,      [false, false, true,  true]),
        (Action::CreateChild, [false, false, true,  true]),
        (Action::Reorder,     [false, false, true,  true]),
        (Action::Trash,       [false, false, true,  true]),
        (Action::Restore,     [false, false, true,  true]),
        (Action::Move,        [false, false, false, true]),
        (Action::Share,       [false, false, false, true]),
        (Action::Delete,      [false, false, false, true]),
    ];
    for (action, row) in matrix {
        for (level, allowed) in LEVELS.into_iter().zip(row) {
            assert_eq!(
                level.allows(action),
                allowed,
                "{level:?} permet-il {action:?} ?"
            );
        }
    }
}

#[test]
fn text_forms_roundtrip() {
    let levels = [
        (Level::Read, "read"),
        (Level::Comment, "comment"),
        (Level::Edit, "edit"),
        (Level::Full, "full"),
    ];
    for (level, text) in levels {
        assert_eq!(level.as_str(), text);
        assert_eq!(Level::parse(text), Some(level));
    }
    let roles = [
        (Role::Owner, "owner"),
        (Role::Admin, "admin"),
        (Role::Member, "member"),
        (Role::Guest, "guest"),
    ];
    for (role, text) in roles {
        assert_eq!(role.as_str(), text);
        assert_eq!(Role::parse(text), Some(role));
    }
    let sections = [
        (Section::Space, "space"),
        (Section::Shared, "shared"),
        (Section::Private, "private"),
    ];
    for (section, text) in sections {
        assert_eq!(section.as_str(), text);
        assert_eq!(Section::parse(text), Some(section));
    }
    let user = Uuid::from_u128(0x0192_f0e1_7c3a_7d4b_9a8e_5f60_1b2c_3d4e);
    let subjects = [
        (Subject::Members, "members".to_string()),
        (
            Subject::User(user),
            "user:0192f0e1-7c3a-7d4b-9a8e-5f601b2c3d4e".to_string(),
        ),
        (
            Subject::User(Uuid::nil()),
            "user:00000000-0000-0000-0000-000000000000".to_string(),
        ),
        (
            Subject::User(Uuid::max()),
            "user:ffffffff-ffff-ffff-ffff-ffffffffffff".to_string(),
        ),
    ];
    for (subject, text) in subjects {
        assert_eq!(subject.to_string(), text);
        assert_eq!(Subject::parse(&text), Some(subject));
    }

    // Rien d'autre n'est relu : chaque valeur a une seule forme écrite.
    for text in ["", "Read", "READ", " read", "read ", "lecture"] {
        assert_eq!(Level::parse(text), None, "{text:?}");
    }
    for text in ["", "Owner", "membre", "guest "] {
        assert_eq!(Role::parse(text), None, "{text:?}");
    }
    for text in ["", "Space", "privé"] {
        assert_eq!(Section::parse(text), None, "{text:?}");
    }
    let other_spellings = [
        String::new(),
        "Members".to_string(),
        "user:".to_string(),
        "user:pas-un-uuid".to_string(),
        user.to_string(),
        format!("User:{user}"),
        format!("user:{}", user.simple()),
        format!("user:{}", user.to_string().to_uppercase()),
        format!("user:{}", user.braced()),
        format!("user:{}", user.urn()),
        format!("user:{user} "),
    ];
    for text in other_spellings {
        assert_eq!(Subject::parse(&text), None, "{text:?}");
    }
}

#[test]
fn sections_follow_the_grants() {
    let (m, g, a, b) = (id(1), id(2), id(3), id(4));
    let page = id(10);
    let nodes = [root(page)];

    // (members, full) : « Espace » pour un membre, invisible pour un invité.
    let mut grants = vec![grant(page, Subject::Members, Level::Full)];
    let member = SpaceAccess::compute(&nodes, &grants, m, Some(Role::Member));
    assert_eq!(seen(&member, page), Some((Level::Full, Section::Space)));
    let guest = SpaceAccess::compute(&nodes, &grants, g, Some(Role::Guest));
    assert_eq!(guest.node(page), None);

    // Avec en plus (user:G, read) : « Partagé » pour l'invité G ; le membre la garde en « Espace ».
    grants.push(grant(page, Subject::User(g), Level::Read));
    let guest = SpaceAccess::compute(&nodes, &grants, g, Some(Role::Guest));
    assert_eq!(seen(&guest, page), Some((Level::Read, Section::Shared)));
    let member = SpaceAccess::compute(&nodes, &grants, m, Some(Role::Member));
    assert_eq!(seen(&member, page), Some((Level::Full, Section::Space)));

    // (user:A, full) : « Privé » pour A, invisible pour un `owner` B.
    let mut grants = vec![grant(page, Subject::User(a), Level::Full)];
    let author = SpaceAccess::compute(&nodes, &grants, a, Some(Role::Member));
    assert_eq!(seen(&author, page), Some((Level::Full, Section::Private)));
    let owner = SpaceAccess::compute(&nodes, &grants, b, Some(Role::Owner));
    assert_eq!(owner.node(page), None);

    // Avec en plus (user:B, edit) : « Partagé » pour A et pour B.
    grants.push(grant(page, Subject::User(b), Level::Edit));
    let author = SpaceAccess::compute(&nodes, &grants, a, Some(Role::Member));
    assert_eq!(seen(&author, page), Some((Level::Full, Section::Shared)));
    let owner = SpaceAccess::compute(&nodes, &grants, b, Some(Role::Owner));
    assert_eq!(seen(&owner, page), Some((Level::Edit, Section::Shared)));
}

#[test]
fn admin_has_full_wherever_members_see() {
    let (admin, owner, member, author) = (id(1), id(2), id(3), id(4));
    let (team, sub, deep, wiki) = (id(10), id(11), id(12), id(20));
    let (private, opened) = (id(30), id(31));
    let nodes = [
        root(team),
        child(sub, team),
        child(deep, sub),
        root(wiki),
        root(private),
        child(opened, private),
    ];
    let grants = [
        grant(team, Subject::Members, Level::Read),
        grant(wiki, Subject::Members, Level::Edit),
        grant(private, Subject::User(author), Level::Full),
        // Une sous-page d'une page privée, ouverte aux membres.
        grant(opened, Subject::Members, Level::Comment),
    ];

    for (user, role) in [(admin, Role::Admin), (owner, Role::Owner)] {
        let access = SpaceAccess::compute(&nodes, &grants, user, Some(role));
        for page in [team, sub, deep, wiki, opened] {
            assert_eq!(
                seen(&access, page),
                Some((Level::Full, Section::Space)),
                "{role:?} sur {page}"
            );
            assert!(access.can(Action::Delete, page), "{role:?} sur {page}");
        }
        assert_eq!(access.node(private), None, "{role:?}");
        assert_eq!(access.node(opened).unwrap().visible_parent, None);
    }

    // Un simple membre a le niveau de la ligne, hérité par les sous-pages.
    let access = SpaceAccess::compute(&nodes, &grants, member, Some(Role::Member));
    let levels = [
        (team, Level::Read),
        (sub, Level::Read),
        (deep, Level::Read),
        (wiki, Level::Edit),
        (opened, Level::Comment),
    ];
    for (page, level) in levels {
        assert_eq!(seen(&access, page), Some((level, Section::Space)), "{page}");
    }
    assert!(!access.can(Action::Edit, deep));
    assert!(access.can(Action::Edit, wiki));
    assert!(!access.can(Action::Delete, wiki));
}

#[test]
fn private_page_is_invisible_to_admins() {
    let (author, admin, owner) = (id(1), id(2), id(3));
    let (page, sub) = (id(10), id(11));
    let nodes = [root(page), child(sub, page)];
    let grants = [grant(page, Subject::User(author), Level::Full)];

    for (user, role) in [(admin, Role::Admin), (owner, Role::Owner)] {
        let access = SpaceAccess::compute(&nodes, &grants, user, Some(role));
        assert_eq!(access.role(), Some(role));
        assert_eq!(access.visible().count(), 0, "{role:?}");
        for p in [page, sub] {
            assert_eq!(access.node(p), None, "{role:?} sur {p}");
            assert!(!access.can(Action::Read, p), "{role:?} sur {p}");
        }
    }

    let access = SpaceAccess::compute(&nodes, &grants, author, Some(Role::Member));
    assert_eq!(seen(&access, page), Some((Level::Full, Section::Private)));
    assert_eq!(seen(&access, sub), Some((Level::Full, Section::Private)));
}

#[test]
fn shared_subpage_shows_as_root_for_the_guest() {
    let (author, guest) = (id(1), id(2));
    let (page, shared, deep) = (id(10), id(11), id(12));
    let nodes = [root(page), child(shared, page), child(deep, shared)];
    let grants = [
        grant(page, Subject::User(author), Level::Full),
        grant(shared, Subject::User(guest), Level::Edit),
    ];

    let access = SpaceAccess::compute(&nodes, &grants, guest, Some(Role::Guest));
    assert_eq!(access.node(page), None);
    assert_eq!(
        access.node(shared).copied(),
        Some(NodeAccess {
            level: Level::Edit,
            section: Section::Shared,
            in_trash: false,
            visible_parent: None,
        })
    );
    assert_eq!(
        access.node(deep).copied(),
        Some(NodeAccess {
            level: Level::Edit,
            section: Section::Shared,
            in_trash: false,
            visible_parent: Some(shared),
        })
    );

    // L'auteur la voit sous sa page, dans la section de celle-ci (« Privé »).
    let access = SpaceAccess::compute(&nodes, &grants, author, Some(Role::Member));
    assert_eq!(
        access.node(shared).copied(),
        Some(NodeAccess {
            level: Level::Full,
            section: Section::Private,
            in_trash: false,
            visible_parent: Some(page),
        })
    );
    assert_eq!(access.node(deep).unwrap().visible_parent, Some(shared));
}

/// « Privé » demande qu'aucune ligne ne nomme un autre sujet, ni sur le nœud **ni sur ses
/// ancêtres** : la sous-page d'une page privée d'Alice, partagée à Bob (simple membre), est
/// « Partagé » chez lui, bien que sa seule ligne propre soit la sienne.
#[test]
fn subpage_shared_from_a_private_page_is_shared_for_a_member() {
    let (alice, bob) = (id(1), id(2));
    let (private, shared) = (id(10), id(11));
    let nodes = [root(private), child(shared, private)];
    let grants = [
        grant(private, Subject::User(alice), Level::Full),
        grant(shared, Subject::User(bob), Level::Edit),
    ];

    let access = SpaceAccess::compute(&nodes, &grants, bob, Some(Role::Member));
    assert_eq!(access.node(private), None);
    assert_eq!(seen(&access, shared), Some((Level::Edit, Section::Shared)));
    assert_eq!(access.node(shared).unwrap().visible_parent, None);
}

#[test]
fn trash_is_inherited() {
    let (user, guest) = (id(1), id(2));
    let (top, trashed, under, deeper, sibling) = (id(10), id(11), id(12), id(13), id(14));
    let nodes = [
        root(top),
        TreeNode {
            id: trashed,
            parent_id: Some(top),
            trashed: true,
        },
        child(under, trashed),
        child(deeper, under),
        child(sibling, top),
    ];
    let grants = [
        grant(top, Subject::Members, Level::Edit),
        grant(under, Subject::User(guest), Level::Read),
    ];

    let access = SpaceAccess::compute(&nodes, &grants, user, Some(Role::Member));
    let in_trash = [
        (top, false),
        (trashed, true),
        (under, true),
        (deeper, true),
        (sibling, false),
    ];
    for (page, expected) in in_trash {
        assert_eq!(access.node(page).unwrap().in_trash, expected, "{page}");
    }
    // `can` ne regarde que le niveau : la corbeille est l'affaire de l'appelant.
    assert!(access.can(Action::Edit, deeper));

    // Un invité qui ne voit pas la page mise à la corbeille voit ses descendants à la corbeille.
    let access = SpaceAccess::compute(&nodes, &grants, guest, Some(Role::Guest));
    assert_eq!(access.node(trashed), None);
    let under_access = access.node(under).unwrap();
    assert!(under_access.in_trash);
    assert_eq!(under_access.visible_parent, None);
    assert!(access.node(deeper).unwrap().in_trash);
}

// ---------------------------------------------------------------------------
// Propriétés, sur des arbres et des accès aléatoires.

/// Les personnes des espaces tirés : les lignes `user:` en nomment une.
const PEOPLE: usize = 4;

fn person(i: usize) -> Uuid {
    id(1_000 + i as u128)
}

fn page(i: usize) -> Uuid {
    id(2_000 + i as u128)
}

/// Un parent qui n'est pas dans l'arbre : le nœud compte comme une racine.
fn absent_parent() -> Uuid {
    id(9_999)
}

#[derive(Debug, Clone)]
struct Space {
    nodes: Vec<TreeNode>,
    grants: Vec<(Uuid, Grant)>,
}

fn level() -> impl Strategy<Value = Level> {
    prop::sample::select(LEVELS.to_vec())
}

fn role() -> impl Strategy<Value = Role> {
    prop::sample::select(vec![Role::Owner, Role::Admin, Role::Member, Role::Guest])
}

fn subject() -> impl Strategy<Value = Subject> {
    prop_oneof![
        Just(Subject::Members),
        (0..PEOPLE).prop_map(|i| Subject::User(person(i))),
    ]
}

/// Des lignes `(nœud, accès)` sur un arbre de `n` nœuds.
fn grants(n: usize) -> impl Strategy<Value = Vec<(Uuid, Grant)>> {
    prop::collection::vec((any::<Index>(), subject(), level()), 0..12).prop_map(move |raw| {
        raw.into_iter()
            .map(|(on, subject, level)| grant(page(on.index(n)), subject, level))
            .collect()
    })
}

/// Un arbre sans cycle, donné dans le désordre : le parent du nœud `i` est un nœud d'avant
/// lui, aucun, ou un nœud absent de l'arbre.
fn space() -> impl Strategy<Value = Space> {
    let node = (0u8..4, any::<Index>(), prop::bool::weighted(0.2));
    prop::collection::vec(node, 1..16)
        .prop_flat_map(|raw| {
            let n = raw.len();
            let nodes: Vec<TreeNode> = raw
                .into_iter()
                .enumerate()
                .map(|(i, (kind, parent, trashed))| TreeNode {
                    id: page(i),
                    parent_id: match kind {
                        0 => None,
                        1 => Some(absent_parent()),
                        _ if i == 0 => None,
                        _ => Some(page(parent.index(i))),
                    },
                    trashed,
                })
                .collect();
            (Just(nodes).prop_shuffle(), grants(n))
        })
        .prop_map(|(nodes, grants)| Space { nodes, grants })
}

/// Le nœud et ses ancêtres présents dans l'arbre (qui doit être sans cycle).
fn chain(nodes: &[TreeNode], id: Uuid) -> Vec<Uuid> {
    let mut out = Vec::new();
    let mut current = Some(id);
    while let Some(c) = current {
        let Some(node) = nodes.iter().find(|n| n.id == c) else {
            break;
        };
        out.push(c);
        current = node.parent_id;
    }
    out
}

/// Vrai si, en remontant les parents, on arrive à une racine (pas de parent, ou un parent
/// absent de l'arbre) plutôt que de tourner en rond.
fn reaches_root(nodes: &[TreeNode], id: Uuid) -> bool {
    let mut current = id;
    for _ in 0..=nodes.len() {
        match nodes
            .iter()
            .find(|n| n.id == current)
            .and_then(|n| n.parent_id)
        {
            Some(parent) if nodes.iter().any(|n| n.id == parent) => current = parent,
            _ => return true,
        }
    }
    false
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn child_level_is_at_least_parent_level(space in space(), who in 0..PEOPLE, role in role()) {
        let access = SpaceAccess::compute(&space.nodes, &space.grants, person(who), Some(role));
        for node in &space.nodes {
            // Le parent visible est nommé ; un parent invisible ou absent ne l'est pas.
            let parent = node.parent_id.and_then(|p| access.node(p));
            if let Some(a) = access.node(node.id) {
                let visible_parent = node.parent_id.filter(|_| parent.is_some());
                prop_assert_eq!(a.visible_parent, visible_parent, "{:?}", node);
            }
            let Some(parent) = parent else { continue };
            let child = access.node(node.id);
            prop_assert!(
                child.is_some_and(|c| c.level >= parent.level && c.section == parent.section),
                "{:?} : {:?} sous {:?}", node, child, parent
            );
        }
    }

    #[test]
    fn outsider_sees_nothing(space in space(), who in 0..PEOPLE) {
        let access = SpaceAccess::compute(&space.nodes, &space.grants, person(who), None);
        prop_assert_eq!(access.role(), None);
        prop_assert_eq!(access.visible().count(), 0);
        for node in &space.nodes {
            prop_assert!(access.node(node.id).is_none());
            for action in ACTIONS {
                prop_assert!(!access.can(action, node.id));
            }
        }
    }

    #[test]
    fn guest_sees_only_user_granted_subtrees(space in space(), who in 0..PEOPLE) {
        let user = person(who);
        let access = SpaceAccess::compute(&space.nodes, &space.grants, user, Some(Role::Guest));
        for node in &space.nodes {
            // Le plus fort des accès `user:<lui>` posés sur ce nœud ou un de ses ancêtres.
            let ancestors = chain(&space.nodes, node.id);
            let expected = space
                .grants
                .iter()
                .filter(|(on, g)| ancestors.contains(on) && g.subject == Subject::User(user))
                .map(|(_, g)| g.level)
                .max();
            let seen = access.node(node.id);
            prop_assert_eq!(seen.map(|a| a.level), expected, "{:?}", node);
            // Un invité ne voit que « Partagé ».
            if let Some(a) = seen {
                prop_assert_eq!(a.section, Section::Shared);
            }
        }
        prop_assert_eq!(
            access.visible().count(),
            space.nodes.iter().filter(|n| access.node(n.id).is_some()).count()
        );
    }

    #[test]
    fn adding_a_grant_never_lowers_anyone(
        space in space(),
        extra in (any::<Index>(), subject(), level()),
        roles in prop::array::uniform4(prop::option::of(role())),
    ) {
        let (on, subject, level) = extra;
        let mut more = space.grants.clone();
        more.push(grant(space.nodes[on.index(space.nodes.len())].id, subject, level));
        for (who, role) in roles.into_iter().enumerate() {
            let before = SpaceAccess::compute(&space.nodes, &space.grants, person(who), role);
            let after = SpaceAccess::compute(&space.nodes, &more, person(who), role);
            for (node, was) in before.visible() {
                let now = after.node(node);
                prop_assert!(
                    now.is_some_and(|n| n.level >= was.level),
                    "{:?} ({:?}) sur {} : {:?} puis {:?}", who, role, node, was, now
                );
            }
        }
    }

    #[test]
    fn compute_terminates_on_cycles(
        raw in prop::collection::vec((prop::option::of(any::<Index>()), any::<bool>()), 1..16),
        who in 0..PEOPLE,
        role in prop::option::of(role()),
        seed in any::<u64>(),
    ) {
        // Des parents quelconques, soi-même compris : des cycles, que le store interdit.
        let n = raw.len();
        let nodes: Vec<TreeNode> = raw
            .iter()
            .enumerate()
            .map(|(i, &(parent, trashed))| TreeNode {
                id: page(i),
                parent_id: parent.map(|p| page(p.index(n))),
                trashed,
            })
            .collect();
        // Des lignes sur chaque nœud, pour que tout ce qui a une racine soit vu.
        let grants: Vec<(Uuid, Grant)> = (0..n)
            .map(|i| {
                let subject = if (seed >> (i % 64)) & 1 == 0 {
                    Subject::Members
                } else {
                    Subject::User(person(who))
                };
                grant(page(i), subject, Level::Read)
            })
            .collect();
        let access = SpaceAccess::compute(&nodes, &grants, person(who), role);
        // Un nœud pris dans un cycle, ou suspendu sous un cycle, n'a pas de racine : personne ne
        // le voit. Le reste de l'arbre est calculé comme d'habitude.
        for node in &nodes {
            let seen = access.node(node.id).is_some();
            if !reaches_root(&nodes, node.id) {
                prop_assert!(!seen, "{:?}", node);
            } else if role.is_some_and(Role::is_member) {
                prop_assert!(seen, "{:?}", node);
            }
        }
        for (_, a) in access.visible() {
            if let Some(parent) = a.visible_parent {
                prop_assert!(access.node(parent).is_some());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cache.

#[test]
fn cache_misses_on_another_rev() {
    let (user, other_user, space, other_space) = (id(1), id(2), id(10), id(11));
    let cache = AccessCache::new(16);
    let access = Arc::new(SpaceAccess::compute(&[], &[], user, Some(Role::Member)));
    assert!(cache.get(user, space, 3).is_none());

    cache.put(user, space, 3, Arc::clone(&access));
    assert!(
        cache
            .get(user, space, 3)
            .is_some_and(|a| Arc::ptr_eq(&a, &access))
    );
    assert!(cache.get(user, space, 4).is_none());
    assert!(cache.get(user, space, 2).is_none());
    assert!(cache.get(other_user, space, 3).is_none());
    assert!(cache.get(user, other_space, 3).is_none());

    // Une révision plus récente remplace l'ancienne.
    let newer = Arc::new(SpaceAccess::compute(&[], &[], user, Some(Role::Admin)));
    cache.put(user, space, 4, Arc::clone(&newer));
    assert!(
        cache
            .get(user, space, 4)
            .is_some_and(|a| Arc::ptr_eq(&a, &newer))
    );
    assert!(cache.get(user, space, 3).is_none());
}

/// Les accès d'un membre à un espace de `pages` pages à la racine, toutes ouvertes aux membres :
/// il en voit `pages`.
fn access_to(pages: u128) -> Arc<SpaceAccess> {
    let nodes: Vec<TreeNode> = (0..pages).map(|i| root(id(100 + i))).collect();
    let grants: Vec<(Uuid, Grant)> = nodes
        .iter()
        .map(|n| grant(n.id, Subject::Members, Level::Read))
        .collect();
    let access = SpaceAccess::compute(&nodes, &grants, id(1), Some(Role::Member));
    assert_eq!(access.visible().count() as u128, pages);
    Arc::new(access)
}

/// Ruling R59 : le cache est borné en nœuds visibles gardés, pas en entrées.
#[test]
fn full_cache_is_emptied() {
    let space = id(10);
    let (a, b, c) = (id(1), id(2), id(3));
    let (three, two, one) = (access_to(3), access_to(2), access_to(1));
    let cache = AccessCache::new(5);
    cache.put(a, space, 1, Arc::clone(&three));
    cache.put(b, space, 1, Arc::clone(&two));
    // 3 + 2 nœuds : le plafond est atteint, pas dépassé.
    assert!(cache.get(a, space, 1).is_some() && cache.get(b, space, 1).is_some());

    // Remplacer une entrée ne compte que la nouvelle : rien n'est vidé.
    cache.put(b, space, 2, Arc::clone(&two));
    assert!(cache.get(a, space, 1).is_some() && cache.get(b, space, 2).is_some());
    cache.put(b, space, 3, Arc::clone(&one));
    cache.put(c, space, 1, Arc::clone(&one));
    // 3 + 1 + 1 nœuds.
    assert!(cache.get(a, space, 1).is_some());
    assert!(cache.get(b, space, 3).is_some() && cache.get(c, space, 1).is_some());

    // Un nœud de plus que le plafond : tout est vidé, puis la nouvelle entrée est gardée.
    cache.put(b, space, 4, Arc::clone(&two));
    assert!(cache.get(a, space, 1).is_none());
    assert!(cache.get(c, space, 1).is_none());
    assert!(cache.get(b, space, 4).is_some());
}

/// Un calcul plus gros que le plafond, à lui seul, n'est pas gardé ; il ne vide pas le cache,
/// mais l'entrée qu'il remplace, d'une révision passée, part.
#[test]
fn oversized_access_is_not_kept() {
    let space = id(10);
    let (a, b) = (id(1), id(2));
    let cache = AccessCache::new(4);
    cache.put(a, space, 1, access_to(2));
    cache.put(b, space, 1, access_to(2));
    cache.put(b, space, 2, access_to(5));
    assert!(cache.get(b, space, 2).is_none());
    assert!(cache.get(b, space, 1).is_none());
    assert!(cache.get(a, space, 1).is_some());
    // Pile au plafond : gardé.
    cache.put(b, space, 3, access_to(4));
    assert!(cache.get(b, space, 3).is_some());
}

/// Un calcul vide (un membre d'un espace sans page) compte pour un nœud : des calculs vides ne
/// font pas grossir le cache sans fin. Un cache de plafond nul ne garde rien.
#[test]
fn empty_access_counts_as_one_node() {
    let space = id(10);
    let (a, b, c) = (id(1), id(2), id(3));
    let empty = access_to(0);
    let cache = AccessCache::new(2);
    cache.put(a, space, 1, Arc::clone(&empty));
    cache.put(b, space, 1, Arc::clone(&empty));
    assert!(cache.get(a, space, 1).is_some() && cache.get(b, space, 1).is_some());
    cache.put(c, space, 1, Arc::clone(&empty));
    assert!(cache.get(a, space, 1).is_none() && cache.get(b, space, 1).is_none());
    assert!(cache.get(c, space, 1).is_some());

    let none = AccessCache::new(0);
    none.put(a, space, 1, empty);
    assert!(none.get(a, space, 1).is_none());
}
