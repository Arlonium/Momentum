//! Arbre de pages : création ordonnée, déplacement, refus des cycles, corbeille, suppression,
//! persistance.

mod common;

use common::{break_table, fresh, int, new_user, page, repair, texts};
use mementum_acl::{Grant, Level, Subject};
use mementum_store::{NewNode, NodeKind, Place, Sibling, Space, Store, StoreError};
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
    assert_eq!(moved.title, "B, sous A");
    assert_eq!(moved.parent_id, Some(a.id));
    assert_eq!(moved.updated_by, Some(user));
    assert_eq!(store.get_node(b.id).await.unwrap(), Some(moved.clone()));
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
#[tokio::test]
async fn update_node_gives_root_grant_to_a_node_arriving_at_root() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let other = new_user(db).await;
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
    parent.grants = vec![Grant {
        subject: Subject::Members,
        level: Level::Full,
    }];
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
        assert_eq!(moved.parent_id, None, "{title}");
        store.node_grants(node.id).await.unwrap()
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
        store
            .update_node(id, title, place, Some(user), Some(full(user)))
            .await
            .unwrap();
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
            .unwrap();
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
        .unwrap();
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
        .unwrap();
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
        .unwrap();
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
        .unwrap();
    assert_eq!(to_root.parent_id, None);
    let stays = store
        .update_node(node.id, None, Some(decided_under(None, None)), None, None)
        .await
        .unwrap();
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
