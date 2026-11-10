//! Espaces : création, lecture, renommage, révision, cloisonnement des nœuds.

mod common;

use common::{fresh, int, new_user, page, texts};
use mementum_acl::{Grant, Level, Subject};
use mementum_store::{NewNode, NodeKind, Place, Store, StoreError};
use uuid::Uuid;

async fn rev(store: &Store, space: Uuid) -> i64 {
    store.space_rev(space).await.unwrap().expect("espace")
}

#[tokio::test]
async fn space_create_get_rename() {
    let f = fresh().await;
    let space = &f.space;
    assert_eq!(space.name, "Essais");
    assert_eq!(space.rev, 0);
    assert!(space.created_at > 1_600_000_000_000, "millisecondes Unix");
    assert_eq!(
        f.store.get_space(space.id).await.unwrap().as_ref(),
        Some(space)
    );
    assert_eq!(f.store.space_rev(space.id).await.unwrap(), Some(0));

    // Le nom est enregistré tel quel : ses règles sont celles de la route HTTP.
    let renamed = f.store.rename_space(space.id, "  Perso ").await.unwrap();
    assert_eq!(renamed.id, space.id);
    assert_eq!(renamed.name, "  Perso ");
    assert_eq!(
        f.store.get_space(space.id).await.unwrap(),
        Some(renamed.clone())
    );
    // Un nom ne change ni la structure ni les droits : la révision ne bouge pas.
    assert_eq!(renamed.rev, 0);

    let unknown = Uuid::now_v7();
    assert_eq!(f.store.get_space(unknown).await.unwrap(), None);
    assert_eq!(f.store.space_rev(unknown).await.unwrap(), None);
    assert!(matches!(
        f.store.rename_space(unknown, "x").await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn space_owner_becomes_member() {
    let f = fresh().await;
    let owner = new_user(&f.db).await;
    let space = f.store.create_space("Pro", Some(owner)).await.unwrap();
    assert_eq!(
        texts(
            &f.db,
            &format!(
                "SELECT user_id || ' ' || role FROM members WHERE space_id = '{}'",
                space.id
            )
        )
        .await,
        [Some(format!("{owner} owner"))]
    );
    assert_eq!(
        texts(
            &f.db,
            &format!("SELECT created_by FROM spaces WHERE id = '{}'", space.id)
        )
        .await,
        [Some(owner.to_string())]
    );
    // Sans compte, pas d'espace à son nom, et rien d'écrit.
    assert!(matches!(
        f.store.create_space("Fantôme", Some(Uuid::now_v7())).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM spaces").await, 2);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM members").await, 1);
}

/// La règle de `rev` : chaque écriture qui touche à la structure ou aux droits d'un espace
/// incrémente sa révision, d'un cran, et la sienne seulement.
#[tokio::test]
async fn every_structural_write_bumps_rev() {
    let f = fresh().await;
    let (store, space) = (&f.store, f.space.id);
    let user = new_user(&f.db).await;
    let other = store.create_space("Autre", None).await.unwrap();
    let mut expected = 0;
    assert_eq!(rev(store, space).await, expected);

    // `members` : le propriétaire d'un espace neuf.
    let owned = store.create_space("À moi", Some(user)).await.unwrap();
    assert_eq!(owned.rev, 1, "create_space avec un propriétaire");
    assert_eq!(rev(store, owned.id).await, 1);

    let a = store
        .create_node(page(&f.space, None, None, "A"))
        .await
        .unwrap();
    expected += 1;
    assert_eq!(rev(store, space).await, expected, "create_node");

    let b = store
        .create_node(NewNode {
            space_id: space,
            parent_id: Some(a.id),
            after: None,
            kind: NodeKind::Page,
            title: "B".into(),
            created_by: Some(user),
            grants: vec![Grant {
                subject: Subject::User(user),
                level: Level::Read,
            }],
            doc: Some(b"doc".to_vec()),
        })
        .await
        .unwrap();
    expected += 1;
    assert_eq!(
        rev(store, space).await,
        expected,
        "create_node avec accès et document : un seul cran"
    );

    store.rename_node(a.id, "A2", None).await.unwrap();
    expected += 1;
    assert_eq!(rev(store, space).await, expected, "rename_node");

    store.move_node(b.id, None, Some(a.id), None).await.unwrap();
    expected += 1;
    assert_eq!(rev(store, space).await, expected, "move_node");

    let place = Place {
        parent_id: Some(a.id),
        after: None,
    };
    store
        .update_node(b.id, Some("B2"), Some(place), Some(user), None)
        .await
        .unwrap();
    expected += 1;
    assert_eq!(rev(store, space).await, expected, "update_node");

    store.trash_node(a.id, user).await.unwrap();
    expected += 1;
    assert_eq!(rev(store, space).await, expected, "trash_node");

    store.restore_node(a.id).await.unwrap();
    expected += 1;
    assert_eq!(rev(store, space).await, expected, "restore_node");

    let grant = Grant {
        subject: Subject::Members,
        level: Level::Edit,
    };
    store.set_grant(a.id, grant).await.unwrap();
    expected += 1;
    assert_eq!(rev(store, space).await, expected, "set_grant");

    store.remove_grant(a.id, Subject::Members).await.unwrap();
    expected += 1;
    assert_eq!(rev(store, space).await, expected, "remove_grant");

    store.trash_node(a.id, user).await.unwrap();
    expected += 1;
    store.delete_node(a.id).await.unwrap();
    expected += 1;
    assert_eq!(rev(store, space).await, expected, "delete_node");

    // Le contenu d'une page ne compte pas : une frappe ne vide pas le cache des accès.
    let c = store
        .create_node(page(&f.space, None, None, "C"))
        .await
        .unwrap();
    expected += 1;
    store.append_updates(c.id, &[b"u1"], None).await.unwrap();
    store
        .append_updates(c.id, &[b"u2", b"u3"], Some(user))
        .await
        .unwrap();
    store.put_snapshot(c.id, 3, b"s").await.unwrap();
    assert_eq!(rev(store, space).await, expected, "contenu");

    // Un refus n'écrit rien, donc ne compte pas non plus.
    assert!(matches!(
        store.move_node(c.id, Some(c.id), None, None).await,
        Err(StoreError::Cycle)
    ));
    assert!(matches!(
        store.delete_node(c.id).await,
        Err(StoreError::NotTrashed)
    ));
    assert_eq!(rev(store, space).await, expected, "refus");

    // Rien de tout cela n'a touché les autres espaces.
    assert_eq!(rev(store, other.id).await, 0);
    assert_eq!(rev(store, owned.id).await, 1);
}

#[tokio::test]
async fn nodes_do_not_cross_spaces() {
    let f = fresh().await;
    let (store, here) = (&f.store, &f.space);
    let there = store.create_space("Ailleurs", None).await.unwrap();
    let mine = store
        .create_node(page(here, None, None, "Ici"))
        .await
        .unwrap();
    let theirs = store
        .create_node(page(&there, None, None, "Là-bas"))
        .await
        .unwrap();

    // Création : un parent, puis un `after`, d'un autre espace.
    let r = store
        .create_node(page(here, Some(theirs.id), None, "X"))
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    let r = store
        .create_node(page(here, None, Some(theirs.id), "X"))
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    // Un espace inconnu.
    let mut nowhere = page(here, None, None, "X");
    nowhere.space_id = Uuid::now_v7();
    assert!(matches!(
        store.create_node(nowhere).await,
        Err(StoreError::NotFound)
    ));

    // Déplacement : de même, par `move_node` comme par `update_node`.
    let r = store.move_node(mine.id, Some(theirs.id), None, None).await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    let r = store.move_node(mine.id, None, Some(theirs.id), None).await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    let place = Place {
        parent_id: Some(theirs.id),
        after: None,
    };
    let r = store
        .update_node(mine.id, None, Some(place), None, None)
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));

    // Rien n'a bougé, d'un côté comme de l'autre.
    assert_eq!(store.list_nodes(here.id).await.unwrap(), [mine]);
    assert_eq!(store.list_nodes(there.id).await.unwrap(), [theirs]);
    assert_eq!(store.space_rev(here.id).await.unwrap(), Some(1));
    assert_eq!(store.space_rev(there.id).await.unwrap(), Some(1));
}
