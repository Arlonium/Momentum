//! Arbre de pages : création ordonnée, déplacement, refus des cycles, corbeille, suppression,
//! persistance.

mod common;

use common::{break_table, fresh, int, new_user, page, repair, texts};
use mementum_acl::{Grant, Level, Role, Subject, chain_level};
use mementum_store::{
    AccessChange, Expected, NewNode, Node, NodeKind, Place, Sibling, Space, Store, StoreError,
};
use uuid::Uuid;

async fn titles(store: &Store, space: &Space, parent: Option<Uuid>) -> Vec<String> {
    store
        .list_nodes(space.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|n| n.parent_id == parent)
        .map(|n| n.title)
        .collect()
}

/// `'id', 'id', …` : la liste des ids pour un `IN (…)` écrit dans un test.
fn sql_list(ids: &[Uuid]) -> String {
    ids.iter()
        .map(|id| format!("'{id}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[tokio::test]
async fn create_and_list_in_order() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let c = store
        .create_node(page(space, None, Some(a.id), "C"))
        .await
        .unwrap();
    store
        .create_node(page(space, None, Some(a.id), "B"))
        .await
        .unwrap();
    // `after: None` : en tête.
    store
        .create_node(page(space, None, None, "Z"))
        .await
        .unwrap();
    let mut signed = page(space, None, Some(c.id), "D");
    signed.created_by = Some(user);
    let d = store.create_node(signed).await.unwrap();
    assert_eq!(titles(store, space, None).await, ["Z", "A", "B", "C", "D"]);
    assert!(a.created_at > 1_600_000_000_000, "millisecondes Unix");
    assert_eq!(a.space_id, space.id);
    assert_eq!(
        (a.created_by, a.updated_by, a.trashed_at),
        (None, None, None)
    );
    // L'auteur est aussi le dernier à avoir modifié la page.
    assert_eq!((d.created_by, d.updated_by), (Some(user), Some(user)));
    assert_eq!(store.get_node(d.id).await.unwrap(), Some(d));
}

#[tokio::test]
async fn create_rejects_bad_after_or_parent_without_writing() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let p = store
        .create_node(page(space, None, None, "P"))
        .await
        .unwrap();
    let q = store
        .create_node(page(space, None, Some(p.id), "Q"))
        .await
        .unwrap();
    // `after` est un enfant d'un autre parent.
    let r = store
        .create_node(page(space, Some(p.id), Some(q.id), "X"))
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    // `after` inexistant.
    let r = store
        .create_node(page(space, None, Some(Uuid::now_v7()), "X"))
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    // parent inexistant.
    let r = store
        .create_node(page(space, Some(Uuid::now_v7()), None, "X"))
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    // auteur inexistant.
    let mut ghost = page(space, None, None, "X");
    ghost.created_by = Some(Uuid::now_v7());
    assert!(matches!(
        store.create_node(ghost).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(store.list_nodes(space.id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn rename_and_get() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let r = store
        .rename_node(a.id, "  Titre tel quel ", Some(user))
        .await
        .unwrap();
    assert_eq!(r.title, "  Titre tel quel ");
    assert!(r.updated_at >= a.updated_at);
    assert_eq!(r.updated_by, Some(user));
    assert_eq!(r.created_by, None);
    assert_eq!(store.get_node(a.id).await.unwrap().unwrap(), r);
    assert!(store.get_node(Uuid::now_v7()).await.unwrap().is_none());
    assert!(matches!(
        store.rename_node(Uuid::now_v7(), "x", None).await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn move_reorders_and_reparents() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(space, None, Some(a.id), "B"))
        .await
        .unwrap();
    let c = store
        .create_node(page(space, None, Some(b.id), "C"))
        .await
        .unwrap();
    // C en tête.
    store.move_node(c.id, None, None, None).await.unwrap();
    assert_eq!(titles(store, space, None).await, ["C", "A", "B"]);
    // Après lui-même : ordre intact.
    store.move_node(a.id, None, Some(a.id), None).await.unwrap();
    assert_eq!(titles(store, space, None).await, ["C", "A", "B"]);
    // B sous A.
    let m = store.move_node(b.id, Some(a.id), None, None).await.unwrap();
    assert_eq!(m.parent_id, Some(a.id));
    assert_eq!(titles(store, space, None).await, ["C", "A"]);
    assert_eq!(titles(store, space, Some(a.id)).await, ["B"]);
    // `after` d'un autre parent : refusé sans rien écrire.
    let r = store.move_node(c.id, Some(a.id), Some(c.id), None).await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    assert_eq!(titles(store, space, None).await, ["C", "A"]);
    assert!(matches!(
        store.move_node(Uuid::now_v7(), None, None, None).await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn move_refuses_cycle() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(space, Some(a.id), None, "B"))
        .await
        .unwrap();
    let c = store
        .create_node(page(space, Some(b.id), None, "C"))
        .await
        .unwrap();
    let before = store.list_nodes(space.id).await.unwrap();

    let r = store.move_node(a.id, Some(c.id), None, None).await;
    assert!(matches!(r, Err(StoreError::Cycle)));
    let r = store.move_node(a.id, Some(a.id), None, None).await;
    assert!(matches!(r, Err(StoreError::Cycle)));
    assert_eq!(store.list_nodes(space.id).await.unwrap(), before);
}

#[tokio::test]
async fn update_node_changes_title_and_place_together() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(space, None, Some(a.id), "B"))
        .await
        .unwrap();
    let place = Place {
        parent_id: Some(a.id),
        after: None,
        from: None,
    };
    let moved = store
        .update_node(b.id, Some("B, sous A"), Some(place), Some(user), None)
        .await
        .unwrap();
    // Sous une autre page : rien à dire de la racine.
    assert_eq!(moved.at_root, None);
    assert_eq!(moved.node.title, "B, sous A");
    assert_eq!(moved.node.parent_id, Some(a.id));
    assert_eq!(moved.node.updated_by, Some(user));
    assert_eq!(
        store.get_node(b.id).await.unwrap(),
        Some(moved.node.clone())
    );
    // Rien de demandé : rien d'écrit, et le nœud tel qu'il est.
    let rev = store.space_rev(space.id).await.unwrap();
    assert_eq!(
        store
            .update_node(b.id, None, None, None, None)
            .await
            .unwrap(),
        moved
    );
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);
    assert!(matches!(
        store
            .update_node(Uuid::now_v7(), None, None, None, None)
            .await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn update_node_is_atomic() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(space, Some(a.id), None, "B"))
        .await
        .unwrap();
    let before = store.list_nodes(space.id).await.unwrap();
    let rev = store.space_rev(space.id).await.unwrap();

    // Un nouveau titre et un déplacement sous son propre enfant : tout est refusé.
    let place = Place {
        parent_id: Some(b.id),
        after: None,
        from: None,
    };
    let r = store
        .update_node(a.id, Some("Nouveau titre"), Some(place), None, None)
        .await;
    assert!(matches!(r, Err(StoreError::Cycle)));
    assert_eq!(store.get_node(a.id).await.unwrap().unwrap().title, "A");
    // De même pour un `after` qui n'est pas un frère à l'endroit visé.
    let place = Place {
        parent_id: None,
        after: Some(Sibling::Node(b.id)),
        from: None,
    };
    let r = store
        .update_node(a.id, Some("Nouveau titre"), Some(place), None, None)
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    assert_eq!(store.list_nodes(space.id).await.unwrap(), before);
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);
}

/// Ruling R82 : quand un nœud arrive à la racine, `root_grant` y est posé dans la transaction du
/// déplacement, ou relevé si son sujet y avait un accès plus faible. Celui qui déplace y a donc
/// toujours l'accès complet, et le nœud n'est jamais, entre deux écritures, sans personne pour
/// le gérer. Un accès n'est jamais abaissé, et les lignes des autres sujets ne changent pas.
///
/// Ici la chaîne ne porte aucune ligne `members` au-dessus de la page : celui qui déplace, un
/// membre, n'a l'accès complet que par les lignes à son nom (les autres cas sont plus bas, avec
/// le Ruling R94). Il le tient de la page parente, qu'il partage avec un autre : sans lui, le
/// store refuserait le déplacement (plus bas aussi).
#[tokio::test]
async fn update_node_gives_root_grant_to_a_node_arriving_at_root() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let other = new_user(db).await;
    store
        .add_member(space.id, user, Role::Member)
        .await
        .unwrap();
    let line = |user, level| Grant {
        subject: Subject::User(user),
        level,
    };
    let full = |user| line(user, Level::Full);
    let to_members = Grant {
        subject: Subject::Members,
        level: Level::Read,
    };
    let root = Place {
        parent_id: None,
        after: None,
        from: None,
    };
    let mut parent = page(space, None, None, "Parent");
    parent.grants = vec![full(other), full(user)];
    let parent = store.create_node(parent).await.unwrap();
    let rev = || async { store.space_rev(space.id).await.unwrap().unwrap() };
    // Un nœud sous `parent`, avec ces lignes propres.
    let child = |title: &'static str, grants: Vec<Grant>| async move {
        let mut child = page(space, Some(parent.id), None, title);
        child.grants = grants;
        store.create_node(child).await.unwrap()
    };
    // Les lignes du nœud, une fois passé à la racine par `user` avec `grant`.
    let lines_at_root = |title: &'static str, grants: Vec<Grant>, grant: Option<Grant>| async move {
        let node = child(title, grants).await;
        let moved = store
            .update_node(node.id, None, Some(root), Some(user), grant)
            .await
            .unwrap();
        assert_eq!(moved.node.parent_id, None, "{title}");
        // La transaction rend ce qu'elle a écrit : les lignes du nœud, et le rôle qu'elle a
        // lu pour le sujet de `grant`.
        let lines = store.node_grants(node.id).await.unwrap();
        let at_root = moved.at_root.expect("un nœud arrivé à la racine");
        assert_eq!(at_root.role, grant.map(|_| Role::Member), "{title}");
        assert_eq!(at_root.grants.len(), lines.len(), "{title}");
        for line in &lines {
            assert!(at_root.grants.contains(line), "{title} : {line:?}");
        }
        lines
    };

    // Sans ligne propre : l'accès est posé, en une seule écriture.
    let before = rev().await;
    assert_eq!(
        lines_at_root("Sans ligne", vec![], Some(full(user))).await,
        [full(user)]
    );
    assert_eq!(
        rev().await,
        before + 2,
        "une création, puis une seule transaction"
    );

    // Une ligne plus faible pour le même sujet : relevée.
    for (title, level) in [
        ("En lecture", Level::Read),
        ("En modification", Level::Edit),
    ] {
        assert_eq!(
            lines_at_root(title, vec![line(user, level)], Some(full(user))).await,
            [full(user)],
            "{title}"
        );
    }
    // Il a déjà l'accès complet : rien ne change. Un accès n'est jamais abaissé.
    assert_eq!(
        lines_at_root("Déjà complet", vec![full(user)], Some(full(user))).await,
        [full(user)]
    );
    assert_eq!(
        lines_at_root(
            "Jamais abaissé",
            vec![full(user)],
            Some(line(user, Level::Edit))
        )
        .await,
        [full(user)]
    );
    // Les lignes des autres sujets restent telles quelles ; la sienne s'y ajoute.
    let mut lines = lines_at_root(
        "Partagée",
        vec![line(other, Level::Read), to_members],
        Some(full(user)),
    )
    .await;
    lines.retain(|grant| *grant != full(user));
    assert_eq!(lines.len(), 2, "la ligne posée, et les deux autres");
    assert!(lines.contains(&line(other, Level::Read)) && lines.contains(&to_members));
    // Sans `root_grant`, rien n'est posé.
    assert_eq!(lines_at_root("Sans accès demandé", vec![], None).await, []);

    // Le nœud n'arrive pas à la racine : sous un autre parent, déjà à la racine (il y change
    // seulement de place), ou simplement renommé.
    let elsewhere = child("Sous un autre parent", vec![]).await;
    let at_root = child("Déjà à la racine", vec![]).await;
    store
        .update_node(at_root.id, None, Some(root), Some(user), None)
        .await
        .unwrap();
    let under_parent = Place {
        parent_id: Some(at_root.id),
        after: None,
        from: None,
    };
    let last = Place {
        parent_id: None,
        after: Some(Sibling::Last),
        from: None,
    };
    for (id, title, place) in [
        (elsewhere.id, None, Some(under_parent)),
        (at_root.id, None, Some(last)),
        (elsewhere.id, Some("Renommée"), None),
    ] {
        let updated = store
            .update_node(id, title, place, Some(user), Some(full(user)))
            .await
            .unwrap();
        assert_eq!(updated.at_root, None);
        assert_eq!(store.node_grants(id).await.unwrap(), []);
    }

    // Tout ou rien : si l'accès ne peut pas être écrit, le nœud ne bouge pas ; et un
    // déplacement refusé ne pose pas l'accès.
    let stuck = child("Panne", vec![]).await;
    let before = rev().await;
    break_table(db, "INSERT", "acl", "1").await;
    let r = store
        .update_node(
            stuck.id,
            Some("Déplacée"),
            Some(root),
            Some(user),
            Some(full(user)),
        )
        .await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
    repair(db).await;
    assert_eq!(store.get_node(stuck.id).await.unwrap(), Some(stuck.clone()));
    let nowhere = Place {
        parent_id: None,
        after: Some(Sibling::Node(elsewhere.id)),
        from: None,
    };
    let r = store
        .update_node(stuck.id, None, Some(nowhere), Some(user), Some(full(user)))
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    assert_eq!(store.node_grants(stuck.id).await.unwrap(), []);
    assert_eq!(store.get_node(stuck.id).await.unwrap(), Some(stuck));
    assert_eq!(rev().await, before);
}

// ---------------------------------------------------------------------------------------------
// Une page qui arrive à la racine (Rulings R94 et R82).

/// Un compte de chaque rôle dans l'espace, plus une invitée de passage, qui n'a de ligne sur
/// aucune page : c'est à elle que les tests donnent un accès, pour voir que le partage marche.
struct Cast {
    owner: Uuid,
    admin: Uuid,
    member: Uuid,
    guest: Uuid,
    reader: Uuid,
}

async fn cast(f: &common::Fixture) -> Cast {
    let with_role = async |role: Role| {
        let user = new_user(&f.db).await;
        f.store.add_member(f.space.id, user, role).await.unwrap();
        user
    };
    Cast {
        owner: with_role(Role::Owner).await,
        admin: with_role(Role::Admin).await,
        member: with_role(Role::Member).await,
        guest: with_role(Role::Guest).await,
        reader: with_role(Role::Guest).await,
    }
}

fn to_members(level: Level) -> Grant {
    Grant {
        subject: Subject::Members,
        level,
    }
}

fn named(user: Uuid, level: Level) -> Grant {
    Grant {
        subject: Subject::User(user),
        level,
    }
}

/// Une page sous `parent` (`None` : à la racine), avec ces lignes propres.
async fn with_lines(
    f: &common::Fixture,
    parent: Option<Uuid>,
    title: &str,
    grants: Vec<Grant>,
) -> Uuid {
    let mut node = page(&f.space, parent, None, title);
    node.grants = grants;
    f.store.create_node(node).await.unwrap().id
}

/// Passe la page à la racine au nom de `mover`, comme le fait la route : en fin des pages de
/// racine, avec l'accès complet demandé pour lui.
async fn to_root(f: &common::Fixture, id: Uuid, mover: Uuid) {
    let root = Place {
        parent_id: None,
        after: Some(Sibling::Last),
        from: None,
    };
    let moved = f
        .store
        .update_node(
            id,
            None,
            Some(root),
            Some(mover),
            Some(named(mover, Level::Full)),
        )
        .await
        .unwrap();
    assert_eq!(moved.node.parent_id, None);
    // La transaction rend ce qu'elle a écrit : les lignes de la page, et le rôle qu'elle a lu
    // pour qui la déplace.
    let at_root = moved.at_root.expect("une page arrivée à la racine");
    assert_lines(f, id, &at_root.grants).await;
    assert_eq!(
        at_root.role,
        f.store.role_in(f.space.id, mover).await.unwrap()
    );
}

/// Les lignes propres de la page sont exactement celles-ci, dans n'importe quel ordre.
async fn assert_lines(f: &common::Fixture, id: Uuid, expected: &[Grant]) {
    let by_subject = |lines: &[Grant]| {
        let mut lines: Vec<(String, Level)> = lines
            .iter()
            .map(|line| (line.subject.to_string(), line.level))
            .collect();
        lines.sort();
        lines
    };
    let lines = f.store.node_grants(id).await.unwrap();
    assert_eq!(by_subject(&lines), by_subject(expected));
}

/// Le niveau de `user` sur la page, d'après les lignes de sa chaîne et son rôle : ce que le
/// serveur calcule pour lui.
async fn level_of(f: &common::Fixture, id: Uuid, user: Uuid) -> Option<Level> {
    let role = f.store.role_in(f.space.id, user).await.unwrap()?;
    let chain = f.store.chain_grants(id).await.unwrap();
    chain_level(chain.iter().map(|(_, line)| line), user, role)
}

/// Après un passage à la racine : `mover` a l'accès complet à la page, et le partage y marche
/// encore. Donner un accès en commentaire ne met personne en `full` : le store le refuserait
/// (`LastFullAccess`, Ruling R70) si la page n'avait plus personne pour la gérer.
async fn assert_still_owned(f: &common::Fixture, c: &Cast, id: Uuid, mover: Uuid) {
    assert_eq!(level_of(f, id, mover).await, Some(Level::Full));
    let change = AccessChange::User(c.reader, Some(Level::Comment));
    let written = f.store.change_access(id, change, mover).await.unwrap();
    assert!(written.changed, "le partage a écrit sa ligne");
}

/// Ruling R94 : une page qui arrive à la racine garde l'accès général qu'elle tenait de ses
/// ancêtres. Une sous-page d'équipe reste une page de l'équipe, pour tous, au même niveau.
/// Déplacée par une propriétaire, elle ne reçoit aucune ligne à son nom : une propriétaire a
/// l'accès complet partout où une ligne `members` s'applique.
#[tokio::test]
async fn page_moved_to_root_keeps_its_general_access() {
    let f = fresh().await;
    let c = cast(&f).await;
    let parent = with_lines(&f, None, "Parent", vec![to_members(Level::Edit)]).await;
    let child = with_lines(&f, Some(parent), "Enfant", vec![]).await;
    let grandchild = with_lines(&f, Some(child), "Petit-enfant", vec![]).await;
    let rev = f.store.space_rev(f.space.id).await.unwrap().unwrap();

    to_root(&f, child, c.owner).await;
    assert_lines(&f, child, &[to_members(Level::Edit)]).await;
    // Rien d'autre n'est écrit : ni sur sa sous-page, ni sur son ancienne page parente.
    assert_lines(&f, grandchild, &[]).await;
    assert_lines(&f, parent, &[to_members(Level::Edit)]).await;
    assert_eq!(
        f.store.space_rev(f.space.id).await.unwrap(),
        Some(rev + 1),
        "une seule transaction"
    );
    // Chacun y a ce qu'il y avait, et sur sa sous-page aussi.
    for id in [child, grandchild] {
        assert_eq!(level_of(&f, id, c.member).await, Some(Level::Edit));
        assert_eq!(level_of(&f, id, c.admin).await, Some(Level::Full));
        assert_eq!(level_of(&f, id, c.guest).await, None);
    }
    assert_still_owned(&f, &c, child, c.owner).await;
}

/// Ruling R94 : l'accès général gardé est la plus forte des lignes `members` de l'ancienne
/// chaîne, où qu'elle soit : sur la page, sur son parent ou plus haut.
#[tokio::test]
async fn page_moved_to_root_takes_the_strongest_members_line() {
    let f = fresh().await;
    let c = cast(&f).await;
    // `(lignes du grand-parent, du parent, de la page)`, la plus forte tour à tour sur chacun.
    let chains = [
        (Level::Full, Level::Read, Level::Comment),
        (Level::Read, Level::Full, Level::Comment),
        (Level::Read, Level::Comment, Level::Full),
    ];
    for (top, middle, own) in chains {
        let top = with_lines(&f, None, "Grand-parent", vec![to_members(top)]).await;
        let middle = with_lines(&f, Some(top), "Parent", vec![to_members(middle)]).await;
        let page = with_lines(&f, Some(middle), "Page", vec![to_members(own)]).await;
        // Un membre y a l'accès complet par cette ligne : aucune ligne à son nom.
        to_root(&f, page, c.member).await;
        assert_lines(&f, page, &[to_members(Level::Full)]).await;
        assert_still_owned(&f, &c, page, c.member).await;
    }
}

/// Sans ligne `members` dans sa chaîne, une page reste privée en arrivant à la racine : aucune
/// ligne `members` n'est posée, et qui la déplace y reçoit l'accès complet (Ruling R82).
#[tokio::test]
async fn private_page_moved_to_root_stays_private() {
    let f = fresh().await;
    let c = cast(&f).await;
    let parent = with_lines(&f, None, "Privée", vec![named(c.member, Level::Full)]).await;
    let child = with_lines(&f, Some(parent), "Enfant", vec![]).await;
    to_root(&f, child, c.member).await;
    assert_lines(&f, child, &[named(c.member, Level::Full)]).await;
    // Privé veut dire privé : ni la propriétaire ni l'admin ne la voient.
    for other in [c.owner, c.admin, c.guest] {
        assert_eq!(level_of(&f, child, other).await, None);
    }
    assert_still_owned(&f, &c, child, c.member).await;
}

/// Ruling R82, au plus juste : la ligne de qui déplace n'est écrite que si, la page arrivée à
/// la racine avec son accès général, il n'y aurait pas l'accès complet autrement. Pas de ligne
/// redondante, qui lui resterait s'il quittait l'espace ou y devenait invité.
#[tokio::test]
async fn mover_line_is_written_only_when_needed() {
    let f = fresh().await;
    let c = cast(&f).await;
    let read = to_members(Level::Read);
    let full = to_members(Level::Full);
    // `(lignes de l'ancien parent, lignes de la page, qui déplace, lignes de la page après)`.
    let cases = [
        // Un membre qui tenait l'accès complet d'une ligne nommée de l'ancien parent : l'accès
        // général lui donne moins, sa ligne est écrite.
        (
            vec![read, named(c.member, Level::Full)],
            vec![],
            c.member,
            vec![read, named(c.member, Level::Full)],
        ),
        // Sa ligne propre, plus faible, est relevée, pas doublée.
        (
            vec![read, named(c.member, Level::Full)],
            vec![named(c.member, Level::Edit)],
            c.member,
            vec![read, named(c.member, Level::Full)],
        ),
        // Un membre d'une chaîne `(members, full)` : aucune ligne à son nom ; celle qu'il avait
        // déjà, plus faible, reste telle quelle.
        (vec![full], vec![], c.member, vec![full]),
        (
            vec![full],
            vec![named(c.member, Level::Read)],
            c.member,
            vec![full, named(c.member, Level::Read)],
        ),
        // Une propriétaire et un admin ont l'accès complet là où une ligne `members`
        // s'applique, à tout niveau : aucune ligne à leur nom.
        (vec![read], vec![], c.owner, vec![read]),
        (vec![read], vec![], c.admin, vec![read]),
    ];
    for (above, own, mover, after) in cases {
        let parent = with_lines(&f, None, "Parent", above).await;
        let page = with_lines(&f, Some(parent), "Page", own).await;
        to_root(&f, page, mover).await;
        assert_lines(&f, page, &after).await;
        assert_still_owned(&f, &c, page, mover).await;
    }
}

/// Les lignes nommées des anciens ancêtres ne suivent pas la page : qui la tenait d'une ligne
/// à son nom sur l'ancien parent la perd, ou retombe à l'accès général. Les lignes nommées de
/// la page elle-même restent.
#[tokio::test]
async fn named_lines_of_old_ancestors_do_not_follow() {
    let f = fresh().await;
    let c = cast(&f).await;
    let above = vec![
        to_members(Level::Read),
        named(c.guest, Level::Edit),
        named(c.member, Level::Edit),
    ];
    let parent = with_lines(&f, None, "Parent", above.clone()).await;
    let page = with_lines(&f, Some(parent), "Page", vec![]).await;
    let kept = with_lines(
        &f,
        Some(parent),
        "Avec sa ligne",
        vec![named(c.guest, Level::Read)],
    )
    .await;
    for id in [page, kept] {
        assert_eq!(level_of(&f, id, c.guest).await, Some(Level::Edit));
        assert_eq!(level_of(&f, id, c.member).await, Some(Level::Edit));
        to_root(&f, id, c.owner).await;
    }
    // L'invité ne voit plus la page ; le membre n'y a plus que l'accès général.
    assert_lines(&f, page, &[to_members(Level::Read)]).await;
    assert_eq!(level_of(&f, page, c.guest).await, None);
    assert_eq!(level_of(&f, page, c.member).await, Some(Level::Read));
    // La ligne posée sur la page elle-même reste.
    assert_lines(
        &f,
        kept,
        &[to_members(Level::Read), named(c.guest, Level::Read)],
    )
    .await;
    assert_eq!(level_of(&f, kept, c.guest).await, Some(Level::Read));
    // L'ancien parent n'a pas changé.
    assert_lines(&f, parent, &above).await;
}

/// Une page déjà à la racine y change seulement de place : aucune ligne n'est écrite, quoi
/// que demande l'appelant, et la révision de l'espace ne bouge pas.
#[tokio::test]
async fn reorder_at_root_writes_no_grant() {
    let f = fresh().await;
    let c = cast(&f).await;
    let bare = with_lines(&f, None, "Sans ligne", vec![]).await;
    let team = with_lines(&f, None, "D'équipe", vec![to_members(Level::Read)]).await;
    let shared = with_lines(&f, None, "Partagée", vec![named(c.guest, Level::Read)]).await;
    let rev = f.store.space_rev(f.space.id).await.unwrap();
    let lines = [
        (bare, vec![]),
        (team, vec![to_members(Level::Read)]),
        (shared, vec![named(c.guest, Level::Read)]),
    ];
    for (id, own) in lines {
        for after in [None, Some(Sibling::Last), Some(Sibling::Node(team))] {
            let place = Place {
                parent_id: None,
                after,
                from: None,
            };
            let updated = f
                .store
                .update_node(
                    id,
                    None,
                    Some(place),
                    Some(c.member),
                    Some(named(c.member, Level::Full)),
                )
                .await
                .unwrap();
            assert_eq!(updated.at_root, None);
            assert_lines(&f, id, &own).await;
        }
    }
    assert_eq!(f.store.space_rev(f.space.id).await.unwrap(), rev);
}

// ---------------------------------------------------------------------------------------------
// Le passage à la racine revérifie, dans sa transaction, la décision de qui déplace.
//
// L'appelant a décidé sur ce qu'il avait lu : le rôle de qui déplace (membre au moins), son
// accès complet sur la page, la page hors corbeille. Si cela ne tient plus quand la transaction
// commence, rien n'est écrit (`Stale`) : il relit et redécide. Sans cela, une ligne `full`
// serait posée pour quelqu'un qui n'a plus à l'avoir, ou que le partage ne compte plus.

/// Ce qu'un déplacement refusé ne touche pas : les nœuds de l'espace, ses lignes d'accès, sa
/// révision.
async fn written(f: &common::Fixture) -> (Vec<Node>, Vec<(Uuid, Grant)>, Option<i64>) {
    (
        f.store.list_nodes(f.space.id).await.unwrap(),
        f.store.space_grants(f.space.id).await.unwrap(),
        f.store.space_rev(f.space.id).await.unwrap(),
    )
}

/// Le passage de la page à la racine avec `root_grant` est refusé (`Stale`) sans rien écrire :
/// ni sa place, ni le titre envoyé avec, ni une ligne d'accès, ni la révision.
async fn assert_stale_at_root(f: &common::Fixture, id: Uuid, root_grant: Grant, case: &str) {
    let before = written(f).await;
    let root = Place {
        parent_id: None,
        after: Some(Sibling::Last),
        from: None,
    };
    let by = match root_grant.subject {
        Subject::User(user) => Some(user),
        Subject::Members => None,
    };
    let r = f
        .store
        .update_node(id, Some("Renommée"), Some(root), by, Some(root_grant))
        .await;
    assert!(matches!(r, Err(StoreError::Stale)), "{case} : {r:?}");
    assert_eq!(written(f).await, before, "{case}");
}

/// Un compte de plus dans l'espace, avec ce rôle.
async fn joined(f: &common::Fixture, role: Role) -> Uuid {
    let user = new_user(&f.db).await;
    f.store.add_member(f.space.id, user, role).await.unwrap();
    user
}

/// Le membre `user` quitte l'espace.
async fn leaves(f: &common::Fixture, user: Uuid) {
    let expected = Expected {
        actor: user,
        actor_role: Role::Member,
        target_role: Role::Member,
    };
    f.store
        .remove_member(f.space.id, user, expected)
        .await
        .unwrap();
}

/// Qui a quitté l'espace entre sa demande et l'écriture ne passe plus une page à la racine :
/// sans rôle, sa ligne `full` n'y donnerait aucun droit, et la page pourrait rester sans
/// personne pour la gérer (le partage ne compte pas une ligne restée à quelqu'un sans rôle).
#[tokio::test]
async fn move_to_root_is_stale_once_the_mover_has_left_the_space() {
    let f = fresh().await;
    let c = cast(&f).await;
    let full = |user| named(user, Level::Full);

    // Une page partagée entre la propriétaire et un membre ; une invitée lit sa sous-page.
    let parent = with_lines(&f, None, "Partagée", vec![full(c.owner), full(c.member)]).await;
    let page = with_lines(&f, Some(parent), "Page", vec![named(c.guest, Level::Read)]).await;
    leaves(&f, c.member).await;
    // Son départ a retiré sa ligne de la page parente.
    assert_lines(&f, parent, &[full(c.owner)]).await;
    assert_stale_at_root(&f, page, full(c.member), "partie d'une page partagée").await;
    // La page garde quelqu'un pour la gérer : la propriétaire, par la page parente.
    assert_eq!(level_of(&f, page, c.owner).await, Some(Level::Full));
    // Le refus passe avant tout le reste : un `after` qui n'est pas à la racine ne se dit
    // (`NotFound`) qu'à qui peut encore déplacer la page.
    let nowhere = Place {
        parent_id: None,
        after: Some(Sibling::Node(page)),
        from: None,
    };
    let r = f
        .store
        .update_node(
            page,
            None,
            Some(nowhere),
            Some(c.member),
            Some(full(c.member)),
        )
        .await;
    assert!(matches!(r, Err(StoreError::Stale)), "{r:?}");

    // Sa page privée, où une invitée lit la sous-page : à son départ, sa ligne y reste (elle la
    // retrouve si elle revient), et l'invitée reçoit l'accès complet à la sous-page (R70).
    let mona = joined(&f, Role::Member).await;
    let parent = with_lines(&f, None, "Privée", vec![full(mona)]).await;
    let page = with_lines(&f, Some(parent), "Page", vec![named(c.guest, Level::Read)]).await;
    leaves(&f, mona).await;
    assert_lines(&f, parent, &[full(mona)]).await;
    assert_lines(&f, page, &[full(c.guest)]).await;
    assert_stale_at_root(&f, page, full(mona), "partie de sa page privée").await;

    // Une page d'équipe en lecture, où elle tenait l'accès complet d'une ligne à son nom :
    // cette ligne est partie avec elle, et ne revient pas sur la page.
    let tess = joined(&f, Role::Member).await;
    let lines = vec![to_members(Level::Read), full(tess)];
    let parent = with_lines(&f, None, "D'équipe", lines).await;
    let page = with_lines(&f, Some(parent), "Page", vec![]).await;
    leaves(&f, tess).await;
    assert_lines(&f, parent, &[to_members(Level::Read)]).await;
    assert_stale_at_root(&f, page, full(tess), "partie d'une page d'équipe").await;

    // Un compte qui n'a jamais eu de rôle dans l'espace.
    let stranger = new_user(&f.db).await;
    assert_stale_at_root(&f, page, full(stranger), "sans rôle").await;
}

/// La racine d'un espace est aux membres : un invité n'y passe pas une page, même s'il y a
/// l'accès complet par une ligne à son nom.
#[tokio::test]
async fn move_to_root_is_stale_for_a_guest() {
    let f = fresh().await;
    let c = cast(&f).await;
    let full = |user| named(user, Level::Full);
    let parent = with_lines(&f, None, "Partagée", vec![full(c.owner), full(c.guest)]).await;
    let page = with_lines(&f, Some(parent), "Page", vec![]).await;
    assert_eq!(level_of(&f, page, c.guest).await, Some(Level::Full));
    assert_stale_at_root(&f, page, full(c.guest), "invité").await;

    // Un membre devenu invité entre-temps : sa ligne lui donne toujours l'accès complet.
    let parent = with_lines(&f, None, "Partagée", vec![full(c.owner), full(c.member)]).await;
    let page = with_lines(&f, Some(parent), "Page", vec![]).await;
    let expected = Expected {
        actor: c.owner,
        actor_role: Role::Owner,
        target_role: Role::Member,
    };
    f.store
        .set_role(f.space.id, c.member, Role::Guest, expected)
        .await
        .unwrap();
    assert_eq!(level_of(&f, page, c.member).await, Some(Level::Full));
    assert_stale_at_root(&f, page, full(c.member), "devenu invité").await;
}

/// Il faut encore, sur l'ancienne chaîne de la page, le niveau demandé pour qui déplace : sinon
/// le passage à la racine lui donnerait pour de bon un accès complet qu'il n'a plus.
#[tokio::test]
async fn move_to_root_is_stale_below_full_on_the_old_chain() {
    let f = fresh().await;
    let c = cast(&f).await;
    let full = |user| named(user, Level::Full);

    // Un admin a l'accès complet partout où une ligne `members` s'applique ; redevenu membre,
    // il n'a plus que la lecture.
    let parent = with_lines(&f, None, "D'équipe", vec![to_members(Level::Read)]).await;
    let first = with_lines(&f, Some(parent), "Avant", vec![]).await;
    let page = with_lines(&f, Some(parent), "Après", vec![]).await;
    to_root(&f, first, c.admin).await;
    let expected = Expected {
        actor: c.owner,
        actor_role: Role::Owner,
        target_role: Role::Admin,
    };
    f.store
        .set_role(f.space.id, c.admin, Role::Member, expected)
        .await
        .unwrap();
    assert_stale_at_root(&f, page, full(c.admin), "admin redevenu membre").await;

    // Un membre dont la ligne sur la page parente a baissé ; sa ligne sur la page elle-même,
    // plus faible, n'y change rien.
    let parent = with_lines(&f, None, "Partagée", vec![full(c.owner), full(c.member)]).await;
    let page = with_lines(&f, Some(parent), "Page", vec![]).await;
    let lower = with_lines(
        &f,
        Some(parent),
        "Page",
        vec![named(c.member, Level::Comment)],
    )
    .await;
    let edit = named(c.member, Level::Edit);
    f.store.set_grant(parent, edit).await.unwrap();
    for id in [page, lower] {
        assert_stale_at_root(&f, id, full(c.member), "ligne baissée").await;
    }
    // C'est le niveau demandé qui compte : celui qu'il a encore passe, sans rien lui donner
    // de plus.
    let root = Place {
        parent_id: None,
        after: Some(Sibling::Last),
        from: None,
    };
    f.store
        .update_node(page, None, Some(root), Some(c.member), Some(edit))
        .await
        .unwrap();
    assert_lines(&f, page, &[edit]).await;

    // Une page qu'il ne voit plus du tout.
    f.store
        .remove_grant(parent, Subject::User(c.member))
        .await
        .unwrap();
    assert_stale_at_root(&f, lower, full(c.member), "ligne retirée").await;
}

/// Une page à la corbeille, elle-même ou par un ancêtre, ne se déplace pas.
#[tokio::test]
async fn move_to_root_is_stale_in_the_trash() {
    let f = fresh().await;
    let c = cast(&f).await;
    let full = named(c.member, Level::Full);
    let parent = with_lines(&f, None, "D'équipe", vec![to_members(Level::Full)]).await;
    let page = with_lines(&f, Some(parent), "Page", vec![]).await;
    let child = with_lines(&f, Some(page), "Sous-page", vec![]).await;

    f.store.trash_node(page, c.owner).await.unwrap();
    assert_stale_at_root(&f, page, full, "à la corbeille").await;
    assert_stale_at_root(&f, child, full, "sous une page à la corbeille").await;
    f.store.restore_node(page).await.unwrap();
    f.store.trash_node(parent, c.owner).await.unwrap();
    assert_stale_at_root(&f, page, full, "page parente à la corbeille").await;

    // Restaurée, elle se déplace de nouveau.
    f.store.restore_node(parent).await.unwrap();
    to_root(&f, page, c.member).await;
    assert_lines(&f, page, &[to_members(Level::Full)]).await;
}

/// `root_grant` nomme qui déplace : un compte. Avec `members` pour sujet, il relèverait l'accès
/// général de la page au lieu de garder celui de sa chaîne ; il est refusé de la même façon.
#[tokio::test]
async fn move_to_root_is_stale_for_a_members_root_grant() {
    let f = fresh().await;
    for level in [Level::Read, Level::Full] {
        let parent = with_lines(&f, None, "D'équipe", vec![to_members(level)]).await;
        let page = with_lines(&f, Some(parent), "Page", vec![]).await;
        assert_stale_at_root(&f, page, to_members(Level::Full), "sujet `members`").await;
    }
}

/// `Sibling::Last` range un nœud en fin de ses frères, à la création comme au déplacement : le
/// store trouve le dernier frère dans sa transaction, sous ce parent et dans cet espace
/// seulement.
#[tokio::test]
async fn last_goes_after_the_last_sibling() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let other = store.create_space("Autre", None).await.unwrap();
    let last = |space: &Space, parent: Option<Uuid>, title: &str| {
        let mut node = page(space, parent, None, title);
        node.after = Some(Sibling::Last);
        node
    };
    // Dans un espace vide, puis après le dernier, d'où qu'il vienne.
    let a = store.create_node(last(space, None, "A")).await.unwrap();
    store.create_node(last(space, None, "B")).await.unwrap();
    store
        .create_node(page(space, None, None, "En tête"))
        .await
        .unwrap();
    let c = store.create_node(last(space, None, "C")).await.unwrap();
    assert_eq!(titles(store, space, None).await, ["En tête", "A", "B", "C"]);
    // Sous un parent, sans se mêler aux pages de la racine ni à celles d'un autre espace.
    for title in ["A1", "A2"] {
        store
            .create_node(last(space, Some(a.id), title))
            .await
            .unwrap();
    }
    store.create_node(last(&other, None, "Zzz")).await.unwrap();
    assert_eq!(titles(store, space, Some(a.id)).await, ["A1", "A2"]);
    assert_eq!(titles(store, space, None).await, ["En tête", "A", "B", "C"]);
    assert_eq!(titles(store, &other, None).await, ["Zzz"]);

    // Au déplacement : en fin des enfants du parent visé, le sien ou un autre.
    let to_end = |parent_id| Place {
        parent_id,
        after: Some(Sibling::Last),
        from: None,
    };
    store
        .update_node(a.id, None, Some(to_end(None)), None, None)
        .await
        .unwrap();
    assert_eq!(titles(store, space, None).await, ["En tête", "B", "C", "A"]);
    store
        .update_node(c.id, None, Some(to_end(Some(a.id))), None, None)
        .await
        .unwrap();
    assert_eq!(titles(store, space, Some(a.id)).await, ["A1", "A2", "C"]);
    assert_eq!(titles(store, space, None).await, ["En tête", "B", "A"]);
    // Un parent sans enfant.
    store
        .update_node(a.id, None, Some(to_end(Some(c.id))), None, None)
        .await
        .map(|_| ())
        .expect_err("a ne va pas sous son propre enfant");
    let b = store.list_nodes(space.id).await.unwrap();
    let b = b.iter().find(|n| n.title == "B").expect("B");
    store
        .update_node(b.id, None, Some(to_end(Some(c.id))), None, None)
        .await
        .unwrap();
    assert_eq!(titles(store, space, Some(c.id)).await, ["B"]);
}

/// Un déplacement vers la place où le nœud est déjà n'écrit rien : ni date, ni auteur, ni
/// révision. Avec un titre, seul le titre est écrit.
#[tokio::test]
async fn update_node_writes_nothing_when_the_node_is_already_there() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let parent = store
        .create_node(page(space, None, None, "Parent"))
        .await
        .unwrap();
    let node = |after: Option<Uuid>, title: &'static str| async move {
        store
            .create_node(page(space, Some(parent.id), after, title))
            .await
            .unwrap()
    };
    let a = node(None, "A").await;
    let b = node(Some(a.id), "B").await;
    let c = node(Some(b.id), "C").await;
    let before = store.list_nodes(space.id).await.unwrap();
    let rev = store.space_rev(space.id).await.unwrap();
    let under = |parent_id, after| Place {
        parent_id,
        after,
        from: None,
    };
    let here = Some(parent.id);
    let already_there = [
        // En tête, après son prédécesseur, en fin, après lui-même.
        (a.id, under(here, None)),
        (b.id, under(here, Some(Sibling::Node(a.id)))),
        (c.id, under(here, Some(Sibling::Node(b.id)))),
        (c.id, under(here, Some(Sibling::Last))),
        (b.id, under(here, Some(Sibling::Node(b.id)))),
        // Seul sous son parent : en tête et en fin à la fois.
        (parent.id, under(None, None)),
        (parent.id, under(None, Some(Sibling::Last))),
    ];
    for (id, place) in already_there {
        let node = store
            .update_node(id, None, Some(place), Some(user), None)
            .await
            .unwrap()
            .node;
        assert_eq!(Some(&node), before.iter().find(|n| n.id == id), "{place:?}");
        assert_eq!(
            store.list_nodes(space.id).await.unwrap(),
            before,
            "{place:?}"
        );
        assert_eq!(store.space_rev(space.id).await.unwrap(), rev, "{place:?}");
    }

    // Avec un titre : le titre et son auteur sont écrits, la place et la révision restent.
    let renamed = store
        .update_node(
            b.id,
            Some("B2"),
            Some(under(here, Some(Sibling::Node(a.id)))),
            Some(user),
            None,
        )
        .await
        .unwrap()
        .node;
    assert_eq!(
        (
            renamed.title.as_str(),
            &renamed.position,
            renamed.updated_by
        ),
        ("B2", &b.position, Some(user))
    );
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);
    // Une vraie place : écrite, sans toucher à la révision.
    let moved = store
        .update_node(c.id, None, Some(under(here, None)), Some(user), None)
        .await
        .unwrap()
        .node;
    assert!(moved.position < a.position);
    assert_eq!(moved.updated_by, Some(user));
    assert_eq!(titles(store, space, here).await, ["C", "A", "B2"]);
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);
}

/// `Place::from` : le parent que l'appelant a lu avant de décider. Si le nœud n'est plus sous
/// ce parent, le déplacement est refusé (`Stale`) sans rien écrire : les droits vérifiés pour
/// une place sous l'ancien parent ne valent pas pour un changement de parent.
#[tokio::test]
async fn update_node_refuses_a_place_decided_under_another_parent() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let old = store
        .create_node(page(space, None, None, "Ancien parent"))
        .await
        .unwrap();
    let new = store
        .create_node(page(space, None, None, "Nouveau parent"))
        .await
        .unwrap();
    let node = store
        .create_node(page(space, Some(old.id), None, "Page"))
        .await
        .unwrap();
    // Lue sous `old`, la page passe sous `new` avant que l'appelant écrive.
    let moved = store
        .move_node(node.id, Some(new.id), None, None)
        .await
        .unwrap();
    let rev = store.space_rev(space.id).await.unwrap();
    let decided_under = |from: Option<Uuid>, parent_id: Option<Uuid>| Place {
        parent_id,
        after: None,
        from: Some(from),
    };
    // Une simple place sous l'ancien parent la ramènerait sous lui ; de même vers la racine, ou
    // lue à la racine.
    for place in [
        decided_under(Some(old.id), Some(old.id)),
        decided_under(Some(old.id), None),
        decided_under(None, None),
    ] {
        let r = store
            .update_node(node.id, Some("Renommée"), Some(place), None, None)
            .await;
        assert!(matches!(r, Err(StoreError::Stale)), "{place:?} : {r:?}");
    }
    assert_eq!(store.get_node(node.id).await.unwrap(), Some(moved));
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);

    // Décidé sous le parent qu'elle a : le déplacement passe.
    let back = store
        .update_node(
            node.id,
            None,
            Some(decided_under(Some(new.id), Some(old.id))),
            None,
            None,
        )
        .await
        .unwrap()
        .node;
    assert_eq!(back.parent_id, Some(old.id));
    let to_root = store
        .update_node(
            node.id,
            None,
            Some(decided_under(Some(old.id), None)),
            None,
            None,
        )
        .await
        .unwrap()
        .node;
    assert_eq!(to_root.parent_id, None);
    let stays = store
        .update_node(node.id, None, Some(decided_under(None, None)), None, None)
        .await
        .unwrap()
        .node;
    assert_eq!(stays.parent_id, None);
}

/// `trashed_since` : la date du nœud s'il est à la corbeille, sinon celle de son plus proche
/// ancêtre qui y est.
#[tokio::test]
async fn trashed_since_is_the_date_of_the_nearest_trashed_node() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let node = |parent: Option<Uuid>, title: &'static str| async move {
        store
            .create_node(page(space, parent, None, title))
            .await
            .unwrap()
            .id
    };
    let top = node(None, "Haut").await;
    let middle = node(Some(top), "Milieu").await;
    let low = node(Some(middle), "Bas").await;
    let leaf = node(Some(low), "Feuille").await;
    let aside = node(None, "À côté").await;
    for id in [top, middle, low, leaf, aside] {
        assert_eq!(store.trashed_since(id).await.unwrap(), None);
    }
    assert_eq!(store.trashed_since(Uuid::now_v7()).await.unwrap(), None);

    // Le haut et le milieu à la corbeille, à des dates qu'on distingue.
    for (id, at) in [(top, 1000), (middle, 2000)] {
        store.trash_node(id, user).await.unwrap();
        let sql = format!("UPDATE nodes SET trashed_at = {at} WHERE id = '{id}'");
        common::exec_sql(db, &sql).await;
    }
    assert_eq!(store.trashed_since(top).await.unwrap(), Some(1000));
    // Sa propre date, puis celle du plus proche ancêtre, pas celle du plus haut.
    assert_eq!(store.trashed_since(middle).await.unwrap(), Some(2000));
    assert_eq!(store.trashed_since(low).await.unwrap(), Some(2000));
    assert_eq!(store.trashed_since(leaf).await.unwrap(), Some(2000));
    assert_eq!(store.trashed_since(aside).await.unwrap(), None);
    // Le milieu restauré (par SQL : le store le refuserait sous un parent à la corbeille).
    let sql = format!("UPDATE nodes SET trashed_at = NULL WHERE id = '{middle}'");
    common::exec_sql(db, &sql).await;
    assert_eq!(store.trashed_since(leaf).await.unwrap(), Some(1000));
}

/// `subtree_ids` rend le nœud d'abord, puis ses descendants niveau par niveau, quel que soit
/// l'ordre dans lequel ils ont été créés.
#[tokio::test]
async fn subtree_ids_come_level_by_level() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let node = |parent: Option<Uuid>, title: &'static str| async move {
        store
            .create_node(page(space, parent, None, title))
            .await
            .unwrap()
            .id
    };
    // Créés en profondeur d'abord : P, C1, G (sous C1), GG (sous G), puis C2 et son enfant H.
    let p = node(None, "P").await;
    let c1 = node(Some(p), "C1").await;
    let g = node(Some(c1), "G").await;
    let gg = node(Some(g), "GG").await;
    let c2 = node(Some(p), "C2").await;
    let h = node(Some(c2), "H").await;
    // Une page à côté, qui n'en est pas.
    node(None, "Ailleurs").await;

    let ids = store.subtree_ids(p).await.unwrap();
    let level = |id: &Uuid| match *id {
        id if id == p => 0,
        id if id == c1 || id == c2 => 1,
        id if id == g || id == h => 2,
        id if id == gg => 3,
        other => panic!("{other} n'est pas dans le sous-arbre"),
    };
    let levels: Vec<u32> = ids.iter().map(level).collect();
    assert_eq!(levels, [0, 1, 1, 2, 2, 3], "{ids:?}");
    // Le même ordre à chaque appel.
    assert_eq!(store.subtree_ids(p).await.unwrap(), ids);
    assert_eq!(store.subtree_ids(g).await.unwrap(), [g, gg]);
}

#[tokio::test]
async fn trash_then_restore_roundtrip() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(space, Some(a.id), None, "B"))
        .await
        .unwrap();

    let trashed = store.trash_node(a.id, user).await.unwrap();
    let at = trashed.trashed_at.expect("à la corbeille");
    assert!(at >= a.created_at);
    // Seule la corbeille a changé.
    assert_eq!(
        trashed,
        mementum_store::Node {
            trashed_at: Some(at),
            ..a.clone()
        }
    );
    assert_eq!(store.get_node(a.id).await.unwrap(), Some(trashed.clone()));
    assert_eq!(
        texts(
            db,
            &format!("SELECT trashed_by FROM nodes WHERE id = '{}'", a.id)
        )
        .await,
        [Some(user.to_string())]
    );
    // La sous-page n'est à la corbeille que par héritage, et la liste garde tout.
    assert_eq!(store.get_node(b.id).await.unwrap(), Some(b.clone()));
    assert_eq!(store.list_nodes(space.id).await.unwrap().len(), 2);

    // Une seconde mise à la corbeille ne change rien, pas même la date.
    let rev = store.space_rev(space.id).await.unwrap();
    assert_eq!(store.trash_node(a.id, user).await.unwrap(), trashed);
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);

    let restored = store.restore_node(a.id).await.unwrap();
    assert_eq!(restored, a);
    assert_eq!(store.get_node(a.id).await.unwrap(), Some(a.clone()));
    assert_eq!(
        texts(
            db,
            &format!("SELECT trashed_by FROM nodes WHERE id = '{}'", a.id)
        )
        .await,
        [None]
    );
    // Restaurer une page qui n'est pas à la corbeille est sans effet.
    let rev = store.space_rev(space.id).await.unwrap();
    assert_eq!(store.restore_node(a.id).await.unwrap(), a);
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);

    let unknown = Uuid::now_v7();
    assert!(matches!(
        store.trash_node(unknown, user).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.restore_node(unknown).await,
        Err(StoreError::NotFound)
    ));
    // Un compte inconnu ne met rien à la corbeille.
    assert!(matches!(
        store.trash_node(a.id, Uuid::now_v7()).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(store.get_node(a.id).await.unwrap(), Some(a));
}

#[tokio::test]
async fn restore_under_trashed_parent_is_refused() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(space, Some(a.id), None, "B"))
        .await
        .unwrap();
    let c = store
        .create_node(page(space, Some(b.id), None, "C"))
        .await
        .unwrap();
    // C est mise à la corbeille, puis sa grand-mère A.
    let c_trashed = store.trash_node(c.id, user).await.unwrap();
    store.trash_node(a.id, user).await.unwrap();
    let before = store.list_nodes(space.id).await.unwrap();
    let rev = store.space_rev(space.id).await.unwrap();

    // C y est elle-même ; B n'y est que par héritage : refusé dans les deux cas.
    assert!(matches!(
        store.restore_node(c.id).await,
        Err(StoreError::ParentTrashed)
    ));
    assert!(matches!(
        store.restore_node(b.id).await,
        Err(StoreError::ParentTrashed)
    ));
    assert_eq!(store.list_nodes(space.id).await.unwrap(), before);
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);

    // On restaure l'ancêtre ; C, mise à la corbeille pour elle-même, y reste, et en sort ensuite.
    assert_eq!(store.restore_node(a.id).await.unwrap(), a);
    assert_eq!(store.get_node(c.id).await.unwrap(), Some(c_trashed));
    assert_eq!(store.restore_node(c.id).await.unwrap(), c);
}

#[tokio::test]
async fn delete_requires_trash() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(space, Some(a.id), None, "B"))
        .await
        .unwrap();
    store.append_updates(b.id, &[b"u"], None).await.unwrap();
    let before = store.list_nodes(space.id).await.unwrap();

    assert!(matches!(
        store.delete_node(a.id).await,
        Err(StoreError::NotTrashed)
    ));
    assert!(matches!(
        store.delete_node(b.id).await,
        Err(StoreError::NotTrashed)
    ));
    assert!(matches!(
        store.delete_node(Uuid::now_v7()).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(store.list_nodes(space.id).await.unwrap(), before);
    assert_eq!(store.load_doc(b.id).await.unwrap().updates.len(), 1);

    // À la corbeille par héritage, B se supprime ; sa mère reste, à la corbeille.
    let trashed = store.trash_node(a.id, user).await.unwrap();
    assert_eq!(store.delete_node(b.id).await.unwrap(), [b.id]);
    assert_eq!(store.list_nodes(space.id).await.unwrap(), [trashed]);
    // Restaurée, elle ne se supprime plus.
    store.restore_node(a.id).await.unwrap();
    assert!(matches!(
        store.delete_node(a.id).await,
        Err(StoreError::NotTrashed)
    ));
}

#[tokio::test]
async fn delete_removes_subtree_docs_and_grants() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let grant = |subject, level| Grant { subject, level };
    let with = |parent: Option<Uuid>, title: &str, grants: Vec<Grant>| NewNode {
        space_id: space.id,
        parent_id: parent,
        after: None,
        kind: NodeKind::Page,
        title: title.to_string(),
        created_by: Some(user),
        grants,
        doc: Some(title.as_bytes().to_vec()),
    };
    // Le sous-arbre à supprimer : P, ses enfants C1 et C2, et G sous C1.
    let p = store
        .create_node(with(None, "P", vec![grant(Subject::Members, Level::Full)]))
        .await
        .unwrap();
    let c1 = store
        .create_node(with(
            Some(p.id),
            "C1",
            vec![grant(Subject::User(user), Level::Edit)],
        ))
        .await
        .unwrap();
    let c2 = store
        .create_node(with(Some(p.id), "C2", vec![]))
        .await
        .unwrap();
    let g = store
        .create_node(with(
            Some(c1.id),
            "G",
            vec![grant(Subject::User(user), Level::Read)],
        ))
        .await
        .unwrap();
    // Ce qui doit rester : une page sœur, avec son document et ses accès.
    let s = store
        .create_node(with(None, "S", vec![grant(Subject::Members, Level::Read)]))
        .await
        .unwrap();
    for id in [p.id, c1.id, g.id, s.id] {
        store.append_updates(id, &[b"u1"], None).await.unwrap();
        store
            .append_updates(id, &[b"u2", b"u3"], Some(user))
            .await
            .unwrap();
    }
    // Une invitation sur une page du sous-arbre, et une sur l'espace.
    for (node, token) in [(Some(g.id), "01"), (None, "02")] {
        let (role, level) = match node {
            Some(_) => ("NULL", "'read'"),
            None => ("'member'", "NULL"),
        };
        common::exec_sql(
            db,
            &format!(
                "INSERT INTO invitations (id, space_id, node_id, email, grant_role, grant_level, \
                     token_hash, created_by, created_at, expires_at) \
                 VALUES ('{id}', '{space}', {node}, 'a@exemple.test', {role}, {level}, x'{token}', \
                     '{user}', 1, 2)",
                id = Uuid::now_v7(),
                space = space.id,
                node = node.map_or("NULL".to_string(), |n| format!("'{n}'")),
            ),
        )
        .await;
    }

    let mut subtree = vec![p.id, c1.id, c2.id, g.id];
    subtree.sort();
    let mut found = store.subtree_ids(p.id).await.unwrap();
    assert_eq!(found[0], p.id, "le nœud d'abord");
    found.sort();
    assert_eq!(found, subtree);
    assert_eq!(store.subtree_ids(c1.id).await.unwrap(), [c1.id, g.id]);
    assert_eq!(store.subtree_ids(s.id).await.unwrap(), [s.id]);
    assert!(matches!(
        store.subtree_ids(Uuid::now_v7()).await,
        Err(StoreError::NotFound)
    ));

    store.trash_node(p.id, user).await.unwrap();
    let mut deleted = store.delete_node(p.id).await.unwrap();
    deleted.sort();
    assert_eq!(deleted, subtree);

    // Plus aucune ligne pour les ids rendus.
    let ids = sql_list(&deleted);
    for (table, column) in [
        ("nodes", "id"),
        ("doc_updates", "node_id"),
        ("doc_snapshots", "node_id"),
        ("acl", "node_id"),
        ("invitations", "node_id"),
    ] {
        assert_eq!(
            int(
                db,
                &format!("SELECT COUNT(*) FROM {table} WHERE {column} IN ({ids})")
            )
            .await,
            0,
            "{table}"
        );
    }
    for id in &deleted {
        assert_eq!(store.get_node(*id).await.unwrap(), None);
        assert_eq!(store.node_grants(*id).await.unwrap(), []);
        let doc = store.load_doc(*id).await.unwrap();
        assert!(doc.snapshot.is_none() && doc.updates.is_empty());
    }
    assert!(matches!(
        store.delete_node(p.id).await,
        Err(StoreError::NotFound)
    ));

    // La page sœur et l'invitation d'espace n'ont rien perdu.
    assert_eq!(
        store.list_nodes(space.id).await.unwrap(),
        std::slice::from_ref(&s)
    );
    let doc = store.load_doc(s.id).await.unwrap();
    assert_eq!(doc.snapshot, Some((0, b"S".to_vec())));
    assert_eq!(doc.updates.len(), 3);
    assert_eq!(
        store.space_grants(space.id).await.unwrap(),
        [(s.id, grant(Subject::Members, Level::Read))]
    );
    assert_eq!(int(db, "SELECT COUNT(*) FROM invitations").await, 1);
    assert_eq!(
        int(db, "SELECT COUNT(*) FROM pragma_foreign_key_check").await,
        0
    );
}

#[tokio::test]
async fn create_node_writes_grants_and_doc_atomically() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let grants = vec![
        Grant {
            subject: Subject::User(user),
            level: Level::Full,
        },
        Grant {
            subject: Subject::Members,
            level: Level::Read,
        },
    ];
    let new = NewNode {
        space_id: space.id,
        parent_id: None,
        after: None,
        kind: NodeKind::Page,
        title: "Avec accès et document".into(),
        created_by: Some(user),
        grants: grants.clone(),
        doc: Some(vec![0x00, 0xff, 0x01]),
    };

    // La panne tombe sur la seconde ligne d'accès : le nœud, l'instantané et la première ligne
    // sont déjà écrits dans la transaction.
    break_table(db, "INSERT", "acl", "NEW.subject = 'members'").await;
    let r = store.create_node(new.clone()).await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
    assert_eq!(store.list_nodes(space.id).await.unwrap(), []);
    for table in ["nodes", "doc_snapshots", "acl"] {
        assert_eq!(
            int(db, &format!("SELECT COUNT(*) FROM {table}")).await,
            0,
            "{table}"
        );
    }
    assert_eq!(store.space_rev(space.id).await.unwrap(), Some(0));

    // La panne passée, tout est écrit ensemble.
    repair(db).await;
    let node = store.create_node(new).await.unwrap();
    assert_eq!(
        store.list_nodes(space.id).await.unwrap(),
        std::slice::from_ref(&node)
    );
    let mut found = store.node_grants(node.id).await.unwrap();
    found.sort_by_key(|g| g.level);
    assert_eq!(found, [grants[1], grants[0]]);
    let doc = store.load_doc(node.id).await.unwrap();
    assert_eq!(doc.snapshot, Some((0, vec![0x00, 0xff, 0x01])));
    assert!(doc.updates.is_empty());
    // La première mise à jour suit l'instantané de `seq` 0.
    assert_eq!(
        store.append_updates(node.id, &[b"u"], None).await.unwrap(),
        1
    );
    assert_eq!(store.space_rev(space.id).await.unwrap(), Some(1));
}

#[tokio::test]
async fn reopen_keeps_everything() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.db");
    let store = Store::open(&path).await.unwrap();
    let space = store.create_space("Essais", None).await.unwrap();
    let a = store
        .create_node(page(&space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(&space, None, Some(a.id), "B"))
        .await
        .unwrap();
    store.append_updates(b.id, &[b"u1"], None).await.unwrap();
    store.put_snapshot(b.id, 1, b"s").await.unwrap();
    store.append_updates(b.id, &[b"u2"], None).await.unwrap();
    let nodes = store.list_nodes(space.id).await.unwrap();
    let space = store.get_space(space.id).await.unwrap().unwrap();
    store.close().await;

    let store = Store::open(&path).await.unwrap();
    assert_eq!(
        store.get_space(space.id).await.unwrap(),
        Some(space.clone())
    );
    assert_eq!(store.list_nodes(space.id).await.unwrap(), nodes);
    let doc = store.load_doc(b.id).await.unwrap();
    assert_eq!(doc.snapshot, Some((1, b"s".to_vec())));
    assert_eq!(doc.updates, vec![(2, b"u2".to_vec())]);
}

#[tokio::test]
async fn pragmas_apply_to_every_connection() {
    let f = fresh().await;
    let store = &f.store;
    // Quatre connexions du pool tenues à la fois, donc distinctes : chacune
    // doit avoir les pragmas.
    let all = store.pragmas(4).await.unwrap();
    assert_eq!(all.len(), 4);
    for (journal, sync, fk, busy) in all {
        assert_eq!(journal, "wal");
        assert_eq!(sync, 2, "FULL");
        assert_eq!(fk, 1);
        assert_eq!(busy, 5000);
    }
}
