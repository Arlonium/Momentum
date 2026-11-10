//! Journal Yjs et instantanés.

mod common;

use common::{Fixture, break_table, fresh, int, new_user, page, repair, texts};
use mementum_store::StoreError;
use uuid::Uuid;

async fn fresh_page() -> (Fixture, Uuid) {
    let f = fresh().await;
    let n = f
        .store
        .create_node(page(&f.space, None, None, "P"))
        .await
        .unwrap();
    (f, n.id)
}

#[tokio::test]
async fn append_and_load() {
    let (f, id) = fresh_page().await;
    let store = &f.store;
    assert_eq!(store.append_updates(id, &[b"a"], None).await.unwrap(), 1);
    assert_eq!(store.append_updates(id, &[b"b"], None).await.unwrap(), 2);
    let doc = store.load_doc(id).await.unwrap();
    assert!(doc.snapshot.is_none());
    assert_eq!(doc.updates, vec![(1, b"a".to_vec()), (2, b"b".to_vec())]);
}

#[tokio::test]
async fn append_to_unknown_page_is_not_found() {
    let (f, _id) = fresh_page().await;
    let store = &f.store;
    assert!(matches!(
        store.append_updates(Uuid::now_v7(), &[b"a"], None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store
            .append_updates(Uuid::now_v7(), &[b"a", b"b"], None)
            .await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM doc_updates").await, 0);
}

/// Sans mise à jour, rien n'est écrit, mais la page et le compte sont vérifiés comme pour une
/// écriture.
#[tokio::test]
async fn empty_append_checks_page_and_actor() {
    let (f, id) = fresh_page().await;
    let store = &f.store;
    let actor = new_user(&f.db).await;
    assert_eq!(store.append_updates(id, &[], None).await.unwrap(), 0);
    assert_eq!(store.append_updates(id, &[], Some(actor)).await.unwrap(), 0);
    assert!(matches!(
        store.append_updates(Uuid::now_v7(), &[], None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.append_updates(Uuid::now_v7(), &[], Some(actor)).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.append_updates(id, &[], Some(Uuid::now_v7())).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM doc_updates").await, 0);
}

#[tokio::test]
async fn snapshot_never_goes_backwards() {
    let (f, id) = fresh_page().await;
    let store = &f.store;
    for i in 1..=12u8 {
        store.append_updates(id, &[&[i]], None).await.unwrap();
    }
    store.put_snapshot(id, 10, b"dix").await.unwrap();
    store.put_snapshot(id, 5, b"cinq").await.unwrap();
    let doc = store.load_doc(id).await.unwrap();
    assert_eq!(doc.snapshot, Some((10, b"dix".to_vec())));
    assert_eq!(doc.updates, vec![(11, vec![11]), (12, vec![12])]);
}

#[tokio::test]
async fn seq_continues_after_snapshot() {
    let (f, id) = fresh_page().await;
    let store = &f.store;
    for i in 1..=10u8 {
        store.append_updates(id, &[&[i]], None).await.unwrap();
    }
    store.put_snapshot(id, 10, b"dix").await.unwrap();
    assert_eq!(
        store.append_updates(id, &[b"onze"], None).await.unwrap(),
        11
    );
    let doc = store.load_doc(id).await.unwrap();
    assert_eq!(doc.snapshot, Some((10, b"dix".to_vec())));
    assert_eq!(doc.updates, vec![(11, b"onze".to_vec())]);
}

#[tokio::test]
async fn concurrent_appends_get_distinct_seqs() {
    let (f, id) = fresh_page().await;
    let mut tasks = Vec::new();
    for i in 0..20u8 {
        let s = f.store.clone();
        tasks.push(tokio::spawn(async move {
            // Un appel sur deux écrit deux mises à jour d'un coup.
            if i % 2 == 0 {
                s.append_updates(id, &[&[i]], None).await.unwrap()
            } else {
                s.append_updates(id, &[&[i], &[i]], None).await.unwrap()
            }
        }));
    }
    let mut returned = Vec::new();
    for t in tasks {
        returned.push(t.await.unwrap());
    }
    let doc = f.store.load_doc(id).await.unwrap();
    let seqs: Vec<i64> = doc.updates.iter().map(|(seq, _)| *seq).collect();
    assert_eq!(seqs, (1..=30).collect::<Vec<i64>>());
    // Les deux mises à jour d'un même appel se suivent, et l'appel rend le `seq` de la seconde.
    for (i, last) in returned.into_iter().enumerate() {
        let mine: Vec<i64> = doc
            .updates
            .iter()
            .filter(|(_, update)| update[0] as usize == i)
            .map(|(seq, _)| *seq)
            .collect();
        if i % 2 == 0 {
            assert_eq!(mine, [last]);
        } else {
            assert_eq!(mine, [last - 1, last]);
        }
    }
}

#[tokio::test]
async fn append_updates_returns_last_seq() {
    let (f, id) = fresh_page().await;
    let store = &f.store;
    // Sans mise à jour : rien d'écrit, et le `seq` où en est la page.
    assert_eq!(store.append_updates(id, &[], None).await.unwrap(), 0);
    assert_eq!(
        store
            .append_updates(id, &[b"a", b"bc", b"def"], None)
            .await
            .unwrap(),
        3
    );
    assert_eq!(store.append_updates(id, &[b"g"], None).await.unwrap(), 4);
    assert_eq!(store.append_updates(id, &[], None).await.unwrap(), 4);
    let doc = store.load_doc(id).await.unwrap();
    assert_eq!(
        doc.updates,
        vec![
            (1, b"a".to_vec()),
            (2, b"bc".to_vec()),
            (3, b"def".to_vec()),
            (4, b"g".to_vec()),
        ]
    );
    // Après une compaction, la suite repart de l'instantané.
    store.put_snapshot(id, 4, b"s").await.unwrap();
    assert_eq!(store.append_updates(id, &[], None).await.unwrap(), 4);
    assert_eq!(
        store.append_updates(id, &[b"h", b"i"], None).await.unwrap(),
        6
    );
}

#[tokio::test]
async fn append_updates_is_atomic() {
    let (f, id) = fresh_page().await;
    let store = &f.store;
    store.append_updates(id, &[b"avant"], None).await.unwrap();
    let before = store.load_doc(id).await.unwrap();

    // La panne tombe sur la deuxième mise à jour de l'appel : la première est déjà écrite dans
    // la transaction.
    break_table(&f.db, "INSERT", "doc_updates", "NEW.update_blob = x'02'").await;
    let r = store.append_updates(id, &[&[1], &[2], &[3]], None).await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
    assert_eq!(store.load_doc(id).await.unwrap(), before);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM doc_updates").await, 1);

    // La panne passée, les mêmes mises à jour s'écrivent à la suite, sans trou dans les `seq`.
    repair(&f.db).await;
    assert_eq!(
        store
            .append_updates(id, &[&[1], &[2], &[3]], None)
            .await
            .unwrap(),
        4
    );
    let doc = store.load_doc(id).await.unwrap();
    assert_eq!(
        doc.updates,
        vec![
            (1, b"avant".to_vec()),
            (2, vec![1]),
            (3, vec![2]),
            (4, vec![3]),
        ]
    );
}

#[tokio::test]
async fn append_updates_records_actor() {
    let (f, id) = fresh_page().await;
    let store = &f.store;
    let actor = new_user(&f.db).await;
    store
        .append_updates(id, &[b"a", b"b"], Some(actor))
        .await
        .unwrap();
    store
        .append_updates(id, &[b"c"], Some(actor))
        .await
        .unwrap();
    // Sans acteur connu : une écriture du serveur.
    store.append_updates(id, &[b"d"], None).await.unwrap();
    assert_eq!(
        texts(&f.db, "SELECT actor_id FROM doc_updates ORDER BY seq").await,
        [
            Some(actor.to_string()),
            Some(actor.to_string()),
            Some(actor.to_string()),
            None,
        ]
    );
    // Un acteur qui n'est pas un compte : rien n'est écrit.
    assert!(matches!(
        store
            .append_updates(id, &[b"f"], Some(Uuid::now_v7()))
            .await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(store.load_doc(id).await.unwrap().updates.len(), 4);
}
