//! Arbre de pages : création ordonnée, déplacement, refus des cycles, corbeille, suppression,
//! persistance.

mod common;

use common::{break_table, fresh, int, new_user, page, repair, texts};
use mementum_acl::{Grant, Level, Subject};
use mementum_store::{NewNode, NodeKind, Place, Space, Store, StoreError};
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
    };
    let r = store
        .update_node(a.id, Some("Nouveau titre"), Some(place), None, None)
        .await;
    assert!(matches!(r, Err(StoreError::Cycle)));
    assert_eq!(store.get_node(a.id).await.unwrap().unwrap().title, "A");
    // De même pour un `after` qui n'est pas un frère à l'endroit visé.
    let place = Place {
        parent_id: None,
        after: Some(b.id),
    };
    let r = store
        .update_node(a.id, Some("Nouveau titre"), Some(place), None, None)
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    assert_eq!(store.list_nodes(space.id).await.unwrap(), before);
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);
}

/// Ruling R66 : un nœud qui arrive à la racine sans aucune ligne d'accès propre reçoit
/// `root_grant` dans la transaction du déplacement. Il n'est donc jamais, entre deux écritures,
/// visible de personne.
#[tokio::test]
async fn update_node_gives_root_grant_to_a_node_arriving_at_root_without_lines() {
    let f = fresh().await;
    let (store, space, db) = (&f.store, &f.space, &f.db);
    let user = new_user(db).await;
    let other = new_user(db).await;
    let full = |user| Grant {
        subject: Subject::User(user),
        level: Level::Full,
    };
    let root = Place {
        parent_id: None,
        after: None,
    };
    let mut parent = page(space, None, None, "Parent");
    parent.grants = vec![Grant {
        subject: Subject::Members,
        level: Level::Full,
    }];
    let parent = store.create_node(parent).await.unwrap();
    let rev = || async { store.space_rev(space.id).await.unwrap().unwrap() };

    // Sans ligne propre : le déplacement à la racine pose l'accès, en une seule écriture.
    let bare = store
        .create_node(page(space, Some(parent.id), None, "Sans ligne"))
        .await
        .unwrap();
    let before = rev().await;
    let moved = store
        .update_node(bare.id, None, Some(root), Some(user), Some(full(user)))
        .await
        .unwrap();
    assert_eq!(moved.parent_id, None);
    assert_eq!(store.node_grants(bare.id).await.unwrap(), [full(user)]);
    assert_eq!(rev().await, before + 1, "une seule transaction");

    // Avec une ligne propre, même pour quelqu'un d'autre : rien n'est posé.
    let mut shared = page(space, Some(parent.id), None, "Partagée");
    shared.grants = vec![Grant {
        subject: Subject::User(other),
        level: Level::Read,
    }];
    let shared = store.create_node(shared).await.unwrap();
    let lines = store.node_grants(shared.id).await.unwrap();
    store
        .update_node(shared.id, None, Some(root), Some(user), Some(full(user)))
        .await
        .unwrap();
    assert_eq!(store.node_grants(shared.id).await.unwrap(), lines);

    // Sans `root_grant`, rien n'est posé non plus.
    let silent = store
        .create_node(page(space, Some(parent.id), None, "Sans accès demandé"))
        .await
        .unwrap();
    store
        .update_node(silent.id, None, Some(root), Some(user), None)
        .await
        .unwrap();
    assert_eq!(store.node_grants(silent.id).await.unwrap(), []);

    // Le nœud n'arrive pas à la racine : sous un autre parent, ou déjà à la racine (il change
    // seulement de place), ou simplement renommé.
    let elsewhere = store
        .create_node(page(space, Some(parent.id), None, "Sous un autre parent"))
        .await
        .unwrap();
    let under_bare = Place {
        parent_id: Some(bare.id),
        after: None,
    };
    store
        .update_node(
            elsewhere.id,
            None,
            Some(under_bare),
            Some(user),
            Some(full(user)),
        )
        .await
        .unwrap();
    assert_eq!(store.node_grants(elsewhere.id).await.unwrap(), []);
    store
        .update_node(silent.id, None, Some(root), Some(user), Some(full(user)))
        .await
        .unwrap();
    assert_eq!(store.node_grants(silent.id).await.unwrap(), []);
    store
        .update_node(
            elsewhere.id,
            Some("Renommée"),
            None,
            Some(user),
            Some(full(user)),
        )
        .await
        .unwrap();
    assert_eq!(store.node_grants(elsewhere.id).await.unwrap(), []);

    // Tout ou rien : si l'accès ne peut pas être écrit, le nœud ne bouge pas ; et un
    // déplacement refusé ne pose pas l'accès.
    let stuck = store
        .create_node(page(space, Some(parent.id), None, "Panne"))
        .await
        .unwrap();
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
        after: Some(elsewhere.id),
    };
    let r = store
        .update_node(stuck.id, None, Some(nowhere), Some(user), Some(full(user)))
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    assert_eq!(store.node_grants(stuck.id).await.unwrap(), []);
    assert_eq!(store.get_node(stuck.id).await.unwrap(), Some(stuck));
    assert_eq!(rev().await, before);
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
    store.append_update(b.id, b"u").await.unwrap();
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
        store.append_update(id, b"u1").await.unwrap();
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
    assert_eq!(store.append_update(node.id, b"u").await.unwrap(), 1);
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
    store.append_update(b.id, b"u1").await.unwrap();
    store.put_snapshot(b.id, 1, b"s").await.unwrap();
    store.append_update(b.id, b"u2").await.unwrap();
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
