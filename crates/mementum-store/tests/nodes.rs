//! Arbre de pages : création ordonnée, déplacement, refus des cycles, persistance.

use mementum_store::{NewNode, NodeKind, Space, Store, StoreError};
use uuid::Uuid;

async fn fresh() -> (tempfile::TempDir, Store, Space) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("sous-dossier").join("m.db"))
        .await
        .unwrap();
    let space = store.default_space().await.unwrap();
    (dir, store, space)
}

fn new(space: &Space, parent: Option<Uuid>, after: Option<Uuid>, title: &str) -> NewNode {
    NewNode {
        vault_id: space.vault_id,
        parent_id: parent,
        after,
        kind: NodeKind::Page,
        title: title.to_string(),
    }
}

async fn titles(store: &Store, space: &Space, parent: Option<Uuid>) -> Vec<String> {
    store
        .list_nodes(space.vault_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|n| n.parent_id == parent)
        .map(|n| n.title)
        .collect()
}

#[tokio::test]
async fn default_space_is_stable() {
    let (_d, store, space) = fresh().await;
    assert_eq!(store.default_space().await.unwrap(), space);
}

#[tokio::test]
async fn create_and_list_in_order() {
    let (_d, store, space) = fresh().await;
    let a = store
        .create_node(new(&space, None, None, "A"))
        .await
        .unwrap();
    let c = store
        .create_node(new(&space, None, Some(a.id), "C"))
        .await
        .unwrap();
    store
        .create_node(new(&space, None, Some(a.id), "B"))
        .await
        .unwrap();
    // `after: None` : en tête.
    store
        .create_node(new(&space, None, None, "Z"))
        .await
        .unwrap();
    store
        .create_node(new(&space, None, Some(c.id), "D"))
        .await
        .unwrap();
    assert_eq!(
        titles(&store, &space, None).await,
        ["Z", "A", "B", "C", "D"]
    );
    assert!(a.created_at > 1_600_000_000_000, "millisecondes Unix");
}

#[tokio::test]
async fn create_rejects_bad_after_or_parent_without_writing() {
    let (_d, store, space) = fresh().await;
    let p = store
        .create_node(new(&space, None, None, "P"))
        .await
        .unwrap();
    let q = store
        .create_node(new(&space, None, Some(p.id), "Q"))
        .await
        .unwrap();
    // `after` est un enfant d'un autre parent.
    let r = store
        .create_node(new(&space, Some(p.id), Some(q.id), "X"))
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    // `after` inexistant.
    let r = store
        .create_node(new(&space, None, Some(Uuid::now_v7()), "X"))
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    // parent inexistant.
    let r = store
        .create_node(new(&space, Some(Uuid::now_v7()), None, "X"))
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    assert_eq!(store.list_nodes(space.vault_id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn rename_and_get() {
    let (_d, store, space) = fresh().await;
    let a = store
        .create_node(new(&space, None, None, "A"))
        .await
        .unwrap();
    let r = store.rename_node(a.id, "  Titre tel quel ").await.unwrap();
    assert_eq!(r.title, "  Titre tel quel ");
    assert!(r.updated_at >= a.updated_at);
    assert_eq!(store.get_node(a.id).await.unwrap().unwrap(), r);
    assert!(store.get_node(Uuid::now_v7()).await.unwrap().is_none());
    assert!(matches!(
        store.rename_node(Uuid::now_v7(), "x").await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn move_reorders_and_reparents() {
    let (_d, store, space) = fresh().await;
    let a = store
        .create_node(new(&space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(new(&space, None, Some(a.id), "B"))
        .await
        .unwrap();
    let c = store
        .create_node(new(&space, None, Some(b.id), "C"))
        .await
        .unwrap();
    // C en tête.
    store.move_node(c.id, None, None).await.unwrap();
    assert_eq!(titles(&store, &space, None).await, ["C", "A", "B"]);
    // Après lui-même : ordre intact.
    store.move_node(a.id, None, Some(a.id)).await.unwrap();
    assert_eq!(titles(&store, &space, None).await, ["C", "A", "B"]);
    // B sous A.
    let m = store.move_node(b.id, Some(a.id), None).await.unwrap();
    assert_eq!(m.parent_id, Some(a.id));
    assert_eq!(titles(&store, &space, None).await, ["C", "A"]);
    assert_eq!(titles(&store, &space, Some(a.id)).await, ["B"]);
    // `after` d'un autre parent : refusé sans rien écrire.
    let r = store.move_node(c.id, Some(a.id), Some(c.id)).await;
    assert!(matches!(r, Err(StoreError::NotFound)));
    assert_eq!(titles(&store, &space, None).await, ["C", "A"]);
    assert!(matches!(
        store.move_node(Uuid::now_v7(), None, None).await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn move_refuses_cycle() {
    let (_d, store, space) = fresh().await;
    let a = store
        .create_node(new(&space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(new(&space, Some(a.id), None, "B"))
        .await
        .unwrap();
    let c = store
        .create_node(new(&space, Some(b.id), None, "C"))
        .await
        .unwrap();
    let before = store.list_nodes(space.vault_id).await.unwrap();

    let r = store.move_node(a.id, Some(c.id), None).await;
    assert!(matches!(r, Err(StoreError::Cycle)));
    let r = store.move_node(a.id, Some(a.id), None).await;
    assert!(matches!(r, Err(StoreError::Cycle)));
    assert_eq!(store.list_nodes(space.vault_id).await.unwrap(), before);
}

#[tokio::test]
async fn reopen_keeps_everything() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.db");
    let store = Store::open(&path).await.unwrap();
    let space = store.default_space().await.unwrap();
    let a = store
        .create_node(new(&space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(new(&space, None, Some(a.id), "B"))
        .await
        .unwrap();
    store.append_update(b.id, b"u1").await.unwrap();
    store.put_snapshot(b.id, 1, b"s").await.unwrap();
    store.append_update(b.id, b"u2").await.unwrap();
    let nodes = store.list_nodes(space.vault_id).await.unwrap();
    store.close().await;

    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.default_space().await.unwrap(), space);
    assert_eq!(store.list_nodes(space.vault_id).await.unwrap(), nodes);
    let doc = store.load_doc(b.id).await.unwrap();
    assert_eq!(doc.snapshot, Some((1, b"s".to_vec())));
    assert_eq!(doc.updates, vec![(2, b"u2".to_vec())]);
}

#[tokio::test]
async fn pragmas_apply_to_every_connection() {
    let (_d, store, _space) = fresh().await;
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
