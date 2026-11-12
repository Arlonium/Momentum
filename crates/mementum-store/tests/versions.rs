//! Versions d'une page (Ruling R135) : ce que le store garde de chacune, les cent dernières, et
//! ce qu'il relit du journal pour la tranche pas encore versionnée.

mod common;

use common::{Fixture, account, break_table, fresh, int, new_user, page, repair, texts};
use mementum_store::{MAX_BEFORE_RESTORE, MAX_VERSIONS, StoreError, Unversioned, VersionReason};
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

/// Une version garde l'état donné tel quel, son `seq`, sa raison et ses auteurs, triés et sans
/// doublon, nommés par ce que le store sait d'eux : un compte qui n'est membre de rien reste
/// nommé ; un identifiant que le store ne connaît pas reçoit un nom vide.
#[tokio::test]
async fn version_keeps_its_state_and_its_authors() {
    let (f, id) = fresh_page().await;
    let alice = account(&f.store, "alice").await;
    let bob = account(&f.store, "bob").await;
    let stranger = Uuid::now_v7();
    let authors = [bob.id, alice.id, bob.id, stranger];

    let version = f
        .store
        .put_version(id, 7, b"etat", &authors, VersionReason::Periodic)
        .await
        .unwrap();

    let listed = f.store.list_versions(id).await.unwrap();
    assert_eq!(listed.len(), 1);
    let kept = &listed[0];
    assert_eq!(kept.id, version);
    assert_eq!(kept.seq, 7);
    assert_eq!(kept.reason, VersionReason::Periodic);
    assert!(kept.created_at > 0);
    let mut expected = vec![
        (alice.id, "alice".to_string()),
        (bob.id, "bob".to_string()),
        (stranger, String::new()),
    ];
    expected.sort();
    let names: Vec<_> = kept
        .authors
        .iter()
        .map(|a| (a.id, a.name.clone()))
        .collect();
    assert_eq!(names, expected);
    assert_eq!(
        f.store.get_version(id, version).await.unwrap(),
        Some(b"etat".to_vec())
    );
    // En base, des identifiants triés, en JSON.
    let ids: Vec<String> = expected.iter().map(|(id, _)| format!("\"{id}\"")).collect();
    assert_eq!(
        texts(&f.db, "SELECT authors FROM doc_versions").await,
        [Some(format!("[{}]", ids.join(",")))]
    );
}

/// Les trois raisons se relisent ; la plus récente vient d'abord ; sans auteur, la liste est
/// vide.
#[tokio::test]
async fn versions_are_listed_most_recent_first() {
    let (f, id) = fresh_page().await;
    let mut put = Vec::new();
    for (seq, reason) in [
        (1, VersionReason::Periodic),
        (2, VersionReason::BeforeRestore),
        (3, VersionReason::Closing),
    ] {
        put.push(
            f.store
                .put_version(id, seq, b"e", &[], reason)
                .await
                .unwrap(),
        );
    }
    let listed = f.store.list_versions(id).await.unwrap();
    let read: Vec<_> = listed.iter().map(|v| (v.id, v.seq, v.reason)).collect();
    assert_eq!(
        read,
        [
            (put[2], 3, VersionReason::Closing),
            (put[1], 2, VersionReason::BeforeRestore),
            (put[0], 1, VersionReason::Periodic),
        ]
    );
    assert!(listed.iter().all(|v| v.authors.is_empty()));
    assert_eq!(
        texts(&f.db, "SELECT reason FROM doc_versions ORDER BY seq").await,
        [
            Some("periodic".to_string()),
            Some("before_restore".to_string()),
            Some("closing".to_string()),
        ]
    );
}

/// Une page garde ses cent dernières versions : la cent-unième efface la plus ancienne, dans la
/// transaction qui l'écrit. Celles d'une autre page n'y comptent pas.
#[tokio::test]
async fn only_the_last_hundred_versions_of_a_page_are_kept() {
    let (f, id) = fresh_page().await;
    let other = f
        .store
        .create_node(page(&f.space, None, None, "Autre"))
        .await
        .unwrap()
        .id;
    let theirs = f
        .store
        .put_version(other, 1, b"autre", &[], VersionReason::Closing)
        .await
        .unwrap();
    let mut put = Vec::new();
    for seq in 0..MAX_VERSIONS as i64 + 5 {
        let state = format!("etat {seq}");
        put.push(
            f.store
                .put_version(id, seq, state.as_bytes(), &[], VersionReason::Periodic)
                .await
                .unwrap(),
        );
    }
    let listed = f.store.list_versions(id).await.unwrap();
    assert_eq!(listed.len(), MAX_VERSIONS);
    let kept: Vec<Uuid> = listed.iter().map(|v| v.id).collect();
    let mut newest: Vec<Uuid> = put[5..].to_vec();
    newest.reverse();
    assert_eq!(kept, newest, "les cinq plus anciennes sont parties");
    for gone in &put[..5] {
        assert_eq!(f.store.get_version(id, *gone).await.unwrap(), None);
    }
    assert_eq!(
        f.store.get_version(other, theirs).await.unwrap(),
        Some(b"autre".to_vec())
    );
    assert_eq!(
        int(&f.db, "SELECT COUNT(*) FROM doc_versions").await,
        MAX_VERSIONS as i64 + 1
    );

    // Une version que la base refuse n'efface rien : la purge est dans la même transaction.
    break_table(&f.db, "INSERT", "doc_versions", "1").await;
    assert!(
        f.store
            .put_version(id, 999, b"refusee", &[], VersionReason::Periodic)
            .await
            .is_err()
    );
    repair(&f.db).await;
    let after: Vec<Uuid> = f
        .store
        .list_versions(id)
        .await
        .unwrap()
        .iter()
        .map(|v| v.id)
        .collect();
    assert_eq!(after, newest);
}

/// Une version ne se lit que par sa page : celle d'une autre page, ou un identifiant inconnu,
/// ne rend rien. Une page inconnue n'a aucune version, et n'en reçoit pas.
#[tokio::test]
async fn version_is_read_only_through_its_page() {
    let (f, id) = fresh_page().await;
    let other = f
        .store
        .create_node(page(&f.space, None, None, "Autre"))
        .await
        .unwrap()
        .id;
    let version = f
        .store
        .put_version(id, 1, b"e", &[], VersionReason::Closing)
        .await
        .unwrap();
    assert_eq!(f.store.get_version(other, version).await.unwrap(), None);
    assert_eq!(f.store.get_version(id, Uuid::now_v7()).await.unwrap(), None);
    assert!(f.store.list_versions(other).await.unwrap().is_empty());
    let unknown = Uuid::now_v7();
    assert!(f.store.list_versions(unknown).await.unwrap().is_empty());
    assert!(matches!(
        f.store
            .put_version(unknown, 1, b"e", &[], VersionReason::Closing)
            .await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM doc_versions").await, 1);
}

/// Ce que le journal garde de la tranche pas encore versionnée : la plus ancienne écriture
/// au-delà du `seq` de la dernière version, et les comptes qui l'ont faite. Une compaction qui a
/// purgé une partie de la tranche laisse la date de son instantané (après les écritures
/// purgées), et les auteurs de ce qui reste.
#[tokio::test]
async fn unversioned_is_read_from_what_the_journal_keeps() {
    let (f, id) = fresh_page().await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    let store = &f.store;
    assert_eq!(store.unversioned(id).await.unwrap(), Unversioned::default());

    store.append_updates(id, &[b"a"], Some(bob)).await.unwrap();
    store.append_updates(id, &[b"b"], None).await.unwrap();
    store
        .append_updates(id, &[b"c"], Some(alice))
        .await
        .unwrap();
    store.append_updates(id, &[b"d"], Some(bob)).await.unwrap();
    let first = int(&f.db, "SELECT created_at FROM doc_updates WHERE seq = 1").await;
    let mut both = vec![alice, bob];
    both.sort();
    assert_eq!(
        store.unversioned(id).await.unwrap(),
        Unversioned {
            since: Some(first),
            authors: both,
        }
    );

    // Une version au `seq` 4 couvre tout ; une écriture de plus ouvre une tranche neuve.
    store
        .put_version(id, 4, b"v", &[alice, bob], VersionReason::Periodic)
        .await
        .unwrap();
    assert_eq!(store.unversioned(id).await.unwrap(), Unversioned::default());
    store
        .append_updates(id, &[b"e"], Some(alice))
        .await
        .unwrap();
    let fifth = int(&f.db, "SELECT created_at FROM doc_updates WHERE seq = 5").await;
    assert_eq!(
        store.unversioned(id).await.unwrap(),
        Unversioned {
            since: Some(fifth),
            authors: vec![alice],
        }
    );

    // La compaction purge le journal jusqu'au `seq` 6 : il ne reste rien d'Alice, ni de Bob.
    store.append_updates(id, &[b"f"], Some(bob)).await.unwrap();
    store.put_snapshot(id, 6, b"instantane").await.unwrap();
    store.append_updates(id, &[b"g"], None).await.unwrap();
    let snapshot = int(&f.db, "SELECT created_at FROM doc_snapshots").await;
    assert_eq!(
        store.unversioned(id).await.unwrap(),
        Unversioned {
            since: Some(snapshot),
            authors: Vec::new(),
        }
    );
}

/// Une page garde au plus vingt versions `before_restore` : la purge retire d'abord les plus
/// anciennes de cette sorte, puis applique le plafond de cent. Cent restaurations après cent
/// versions ordinaires n'en évincent donc que vingt : il en reste quatre-vingts, et les vingt
/// `before_restore` les plus récentes.
#[tokio::test]
async fn at_most_twenty_versions_before_a_restore_are_kept() {
    let (f, id) = fresh_page().await;
    let mut ordinary = Vec::new();
    for seq in 0..MAX_VERSIONS as i64 {
        let reason = if seq % 2 == 0 {
            VersionReason::Periodic
        } else {
            VersionReason::Closing
        };
        ordinary.push(
            f.store
                .put_version(id, seq, b"o", &[], reason)
                .await
                .unwrap(),
        );
    }
    let mut before = Vec::new();
    for seq in 0..MAX_VERSIONS as i64 {
        let seq = MAX_VERSIONS as i64 + seq;
        before.push(
            f.store
                .put_version(id, seq, b"r", &[], VersionReason::BeforeRestore)
                .await
                .unwrap(),
        );
    }
    let listed = f.store.list_versions(id).await.unwrap();
    let kept = |reason: VersionReason| -> Vec<Uuid> {
        listed
            .iter()
            .filter(|v| v.reason == reason)
            .map(|v| v.id)
            .collect()
    };
    let mut newest_before: Vec<Uuid> = before[before.len() - MAX_BEFORE_RESTORE..].to_vec();
    newest_before.reverse();
    assert_eq!(kept(VersionReason::BeforeRestore), newest_before);
    let ordinary_kept = listed.len() - MAX_BEFORE_RESTORE;
    assert_eq!(ordinary_kept, MAX_VERSIONS - MAX_BEFORE_RESTORE);
    let mut newest_ordinary: Vec<Uuid> = ordinary[MAX_BEFORE_RESTORE..].to_vec();
    newest_ordinary.reverse();
    let ordinary_ids: Vec<Uuid> = listed
        .iter()
        .filter(|v| v.reason != VersionReason::BeforeRestore)
        .map(|v| v.id)
        .collect();
    assert_eq!(
        ordinary_ids, newest_ordinary,
        "les vingt plus anciennes sont parties"
    );
}
