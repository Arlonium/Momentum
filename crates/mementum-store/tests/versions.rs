//! Versions d'une page (Ruling R135) : ce que le store garde de chacune, les cent dernières, et
//! ce qu'il relit du journal pour la tranche pas encore versionnée.

mod common;

use common::{Fixture, account, break_table, exec_sql, fresh, int, new_user, page, repair, texts};
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
            versioned: false,
        }
    );

    // Une version au `seq` 4 couvre tout ; une écriture de plus ouvre une tranche neuve.
    store
        .put_version(id, 4, b"v", &[alice, bob], VersionReason::Periodic)
        .await
        .unwrap();
    let covered = Unversioned {
        versioned: true,
        ..Unversioned::default()
    };
    assert_eq!(store.unversioned(id).await.unwrap(), covered);
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
            versioned: true,
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
            versioned: true,
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

/// Les versions se rangent d'abord par `seq`, qui ne fait que croître pour une page : une
/// horloge système qui recule (les versions déjà gardées datées plus tard que la neuve) ne fait
/// ni effacer par la purge la version qu'on vient d'écrire, ni la ranger après les autres.
#[tokio::test]
async fn version_just_written_survives_a_clock_that_went_back() {
    let (f, id) = fresh_page().await;
    for seq in 0..MAX_VERSIONS as i64 {
        f.store
            .put_version(id, seq, b"avant", &[], VersionReason::Periodic)
            .await
            .unwrap();
    }
    // L'horloge recule : tout ce qui est gardé paraît écrit dans une heure.
    exec_sql(
        &f.db,
        "UPDATE doc_versions SET created_at = created_at + 3600000",
    )
    .await;
    let newest = f
        .store
        .put_version(
            id,
            MAX_VERSIONS as i64,
            b"neuve",
            &[],
            VersionReason::Closing,
        )
        .await
        .unwrap();
    let listed = f.store.list_versions(id).await.unwrap();
    assert_eq!(listed.len(), MAX_VERSIONS);
    assert_eq!(listed[0].id, newest, "la plus récente d'abord");
    assert_eq!(
        listed[MAX_VERSIONS - 1].seq,
        1,
        "la plus ancienne (seq 0) est partie"
    );
    assert_eq!(
        f.store.get_version(id, newest).await.unwrap(),
        Some(b"neuve".to_vec())
    );
}

/// Ce que la purge retirerait à la version suivante : rien sous les plafonds ; la plus ancienne
/// de toutes à cent ; la plus ancienne `before_restore` à vingt de cette sorte.
#[tokio::test]
async fn next_purged_names_what_the_next_version_would_push_out() {
    let (f, id) = fresh_page().await;
    let mut ordinary = Vec::new();
    for seq in 0..(MAX_VERSIONS - MAX_BEFORE_RESTORE) as i64 {
        ordinary.push(
            f.store
                .put_version(id, seq, b"o", &[], VersionReason::Periodic)
                .await
                .unwrap(),
        );
    }
    assert!(f.store.next_purged(id).await.unwrap().is_empty());
    let mut before = Vec::new();
    for seq in 0..MAX_BEFORE_RESTORE as i64 {
        let seq = (MAX_VERSIONS - MAX_BEFORE_RESTORE) as i64 + seq;
        before.push(
            f.store
                .put_version(id, seq, b"r", &[], VersionReason::BeforeRestore)
                .await
                .unwrap(),
        );
    }
    assert_eq!(
        f.store.next_purged(id).await.unwrap(),
        [before[0], ordinary[0]]
    );
    assert!(
        f.store
            .next_purged(Uuid::now_v7())
            .await
            .unwrap()
            .is_empty()
    );
}

/// L'état d'une version est sa dernière colonne (Ruling R191) : SQLite lit les colonnes d'une
/// ligne dans l'ordre, et une colonne placée après lui faisait lire tout l'état à chaque liste
/// des versions et à chaque purge. Mesuré en release sur une page de 10 000 blocs (3 Mo par
/// version) : la cent-unième version prenait 343 ms, purge comprise, au lieu de 29 ms.
#[tokio::test]
async fn state_is_the_last_column_of_a_version() {
    let (f, _) = fresh_page().await;
    let columns = texts(
        &f.db,
        "SELECT name FROM pragma_table_info('doc_versions') ORDER BY cid",
    )
    .await;
    assert_eq!(
        columns.last(),
        Some(&Some("state_blob".to_string())),
        "{columns:?}"
    );
}

/// Une restauration écrit au journal le contenu d'une version gardée : dans la même
/// transaction, la version retient ce `seq` comme le dernier où le contenu de la page valait
/// elle (Ruling R197). La tranche pas encore versionnée ne compte donc que ce qui suit, et une
/// version restaurée ne change pas de place dans la liste (rangée par le `seq` de sa coupe).
#[tokio::test]
async fn restore_write_is_covered_by_its_version() {
    let (f, id) = fresh_page().await;
    let alice = new_user(&f.db).await;
    let store = &f.store;
    store
        .append_updates(id, &[b"a"], Some(alice))
        .await
        .unwrap();
    let older = store
        .put_version(id, 1, b"ancienne", &[alice], VersionReason::Closing)
        .await
        .unwrap();
    store
        .append_updates(id, &[b"b"], Some(alice))
        .await
        .unwrap();
    let newer = store
        .put_version(id, 2, b"recente", &[alice], VersionReason::Closing)
        .await
        .unwrap();

    let seq = store
        .append_restore(id, b"restauree", Some(alice), older)
        .await
        .unwrap();
    assert_eq!(seq, 3);
    let covered = Unversioned {
        versioned: true,
        ..Unversioned::default()
    };
    assert_eq!(store.unversioned(id).await.unwrap(), covered, "rien après");
    let order: Vec<Uuid> = store
        .list_versions(id)
        .await
        .unwrap()
        .iter()
        .map(|v| v.id)
        .collect();
    assert_eq!(order, [newer, older], "la liste ne bouge pas");

    // Une écriture de plus ouvre une tranche, que la version restaurée ne couvre pas.
    store
        .append_updates(id, &[b"c"], Some(alice))
        .await
        .unwrap();
    let fourth = int(&f.db, "SELECT created_at FROM doc_updates WHERE seq = 4").await;
    assert_eq!(
        store.unversioned(id).await.unwrap(),
        Unversioned {
            since: Some(fourth),
            authors: vec![alice],
            versioned: true,
        }
    );
}

/// L'écriture d'une restauration au journal et la borne de sa version vont ensemble, ou pas du
/// tout : une version qui n'est pas de la page, ou une borne que la base refuse, n'écrivent rien
/// au journal.
#[tokio::test]
async fn restore_write_and_its_bound_are_all_or_nothing() {
    let (f, id) = fresh_page().await;
    let other = f
        .store
        .create_node(page(&f.space, None, None, "Autre"))
        .await
        .unwrap()
        .id;
    let theirs = f
        .store
        .put_version(other, 0, b"e", &[], VersionReason::Closing)
        .await
        .unwrap();
    let ours = f
        .store
        .put_version(id, 0, b"e", &[], VersionReason::Closing)
        .await
        .unwrap();
    for version in [theirs, Uuid::now_v7()] {
        assert!(matches!(
            f.store.append_restore(id, b"r", None, version).await,
            Err(StoreError::NotFound)
        ));
    }
    break_table(&f.db, "UPDATE", "doc_versions", "1").await;
    assert!(f.store.append_restore(id, b"r", None, ours).await.is_err());
    repair(&f.db).await;
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM doc_updates").await, 0);
}
