//! Journal Yjs et instantanés.

use mementum_store::{NewNode, NodeKind, Store, StoreError};
use uuid::Uuid;

async fn fresh_page() -> (tempfile::TempDir, Store, Uuid) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("m.db")).await.unwrap();
    let space = store.default_space().await.unwrap();
    let n = store
        .create_node(NewNode {
            vault_id: space.vault_id,
            parent_id: None,
            after: None,
            kind: NodeKind::Page,
            title: "P".into(),
        })
        .await
        .unwrap();
    (dir, store, n.id)
}

#[tokio::test]
async fn append_and_load() {
    let (_d, store, id) = fresh_page().await;
    assert_eq!(store.append_update(id, b"a").await.unwrap(), 1);
    assert_eq!(store.append_update(id, b"b").await.unwrap(), 2);
    let doc = store.load_doc(id).await.unwrap();
    assert!(doc.snapshot.is_none());
    assert_eq!(doc.updates, vec![(1, b"a".to_vec()), (2, b"b".to_vec())]);
}

#[tokio::test]
async fn append_to_unknown_page_is_not_found() {
    let (_d, store, _id) = fresh_page().await;
    assert!(matches!(
        store.append_update(Uuid::now_v7(), b"a").await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn snapshot_never_goes_backwards() {
    let (_d, store, id) = fresh_page().await;
    for i in 1..=12u8 {
        store.append_update(id, &[i]).await.unwrap();
    }
    store.put_snapshot(id, 10, b"dix").await.unwrap();
    store.put_snapshot(id, 5, b"cinq").await.unwrap();
    let doc = store.load_doc(id).await.unwrap();
    assert_eq!(doc.snapshot, Some((10, b"dix".to_vec())));
    assert_eq!(doc.updates, vec![(11, vec![11]), (12, vec![12])]);
}

#[tokio::test]
async fn seq_continues_after_snapshot() {
    let (_d, store, id) = fresh_page().await;
    for i in 1..=10u8 {
        store.append_update(id, &[i]).await.unwrap();
    }
    store.put_snapshot(id, 10, b"dix").await.unwrap();
    assert_eq!(store.append_update(id, b"onze").await.unwrap(), 11);
    let doc = store.load_doc(id).await.unwrap();
    assert_eq!(doc.snapshot, Some((10, b"dix".to_vec())));
    assert_eq!(doc.updates, vec![(11, b"onze".to_vec())]);
}

#[tokio::test]
async fn concurrent_appends_get_distinct_seqs() {
    let (_d, store, id) = fresh_page().await;
    let mut tasks = Vec::new();
    for i in 0..20u8 {
        let s = store.clone();
        tasks.push(tokio::spawn(async move {
            s.append_update(id, &[i]).await.unwrap()
        }));
    }
    let mut seqs = Vec::new();
    for t in tasks {
        seqs.push(t.await.unwrap());
    }
    seqs.sort();
    assert_eq!(seqs, (1..=20).collect::<Vec<i64>>());
}
