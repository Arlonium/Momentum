//! Durabilité : persister avant de diffuser, compacter sans rien perdre (et
//! aussitôt après une grosse suppression, Ruling R40), redémarrer sans rien
//! perdre, survivre à une panne du store, donner son document à une page qui
//! n'en a pas.

mod common;

use std::time::Duration;

use common::*;
use mementum_md::parse;
use mementum_schema::SCHEMA_VERSION;
use mementum_store::Store;
use mementum_sync::{Hub, SyncConfig};
use yrs::{ReadTxn, Text, Transact, XmlFragment, XmlOut};

#[tokio::test]
async fn update_is_persisted_before_broadcast() {
    let f = Fixture::new(SyncConfig::default()).await;
    let id = f.ids().await[0].clone();
    let a = f.client().await;
    let b = f.client().await;

    let mut expected = String::new();
    for c in "Écrit, déjà 🚀".chars() {
        expected.push(c);
        a.type_text(&id, &c.to_string());
        assert!(
            b.wait_until(WAIT, |b| text(&b.blocks()) == expected).await,
            "B n'a pas reçu {expected:?}"
        );
        // Contrôlé aussitôt après la réception : le store la contient déjà.
        assert_eq!(
            text(&stored_page(&f.store, f.page).await.read_blocks()),
            expected
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compaction_keeps_concurrent_updates() {
    let f = Fixture::new(SyncConfig {
        snapshot_every: 100,
        ..SyncConfig::default()
    })
    .await;
    f.write_mmd("A\n\nB\n").await;
    let ids = f.ids().await;
    let a = f.client().await;
    let b = f.client().await;

    // 600 mises à jour chacun, envoyées en même temps sur les deux connexions.
    for _ in 0..600 {
        a.type_text(&ids[0], "a");
        b.type_text(&ids[1], "b");
        tokio::task::yield_now().await;
    }
    let expected = format!("A{}\nB{}", "a".repeat(600), "b".repeat(600));
    let long = Duration::from_secs(120);
    assert!(
        a.wait_until(long, |c| text(&c.blocks()) == expected).await,
        "A n'a pas tout reçu"
    );
    assert!(
        b.wait_until(long, |c| text(&c.blocks()) == expected).await,
        "B n'a pas tout reçu"
    );

    let live = f.hub.read_blocks(f.page).await.unwrap();
    assert_eq!(text(&live), expected);
    assert_eq!(stored_page(&f.store, f.page).await.read_blocks(), live);

    let state = f.store.load_doc(f.page).await.unwrap();
    let snapshot_seq = state.snapshot.as_ref().map_or(0, |(seq, _)| *seq);
    assert!(
        snapshot_seq >= 1000,
        "compactions faites : seq {snapshot_seq}"
    );
    assert!(state.updates.len() < 100, "journal purgé");
}

#[tokio::test]
async fn restart_keeps_everything() {
    let cfg = SyncConfig {
        snapshot_every: 7,
        ..SyncConfig::default()
    };
    let f = Fixture::new(cfg).await;
    f.write_mmd("Titre\n\n- un\n- deux\n").await;
    let ids = f.ids().await;
    let a = f.client().await;
    let b = f.client().await;
    for i in 0..20 {
        a.type_text(&ids[0], &format!(" {i}"));
    }
    let typed = a.blocks();
    assert!(b.wait_until(WAIT, |c| c.blocks() == typed).await);
    let before = b.blocks();

    // Arrêt : serveur, clients, hub, base.
    let Fixture {
        dir,
        db,
        store,
        hub,
        server,
        page,
    } = f;
    server.stop().await;
    drop((a, b, hub));
    store.close().await;

    // Nouveaux store, hub et serveur sur la même base.
    let store = Store::open(&db).await.unwrap();
    let hub = Hub::new(store.clone(), cfg);
    let server = TestServer::start(hub.clone()).await;
    assert_eq!(hub.read_blocks(page).await.unwrap(), before);
    let c = Client::connect_synced(&server.url(page, SCHEMA_VERSION)).await;
    assert_eq!(c.blocks(), before);
    assert_eq!(stored_page(&store, page).await.read_blocks(), before);

    // La page vit toujours.
    c.type_text(&ids[1], " trois");
    c.resync();
    assert!(c.wait_until(WAIT, |c| c.step2_count() >= 2).await);
    assert_eq!(hub.read_blocks(page).await.unwrap(), c.blocks());
    drop(dir);
}

#[tokio::test]
async fn store_failure_closes_everyone_and_reloads() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd(
        "Avant
",
    )
    .await;
    let id = f.ids().await[0].clone();
    let a = f.client().await;
    let b = f.client().await;

    // Panne simulée : toute écriture dans le journal échoue (Ruling R21).
    exec_sql(
        &f.db,
        "CREATE TRIGGER panne BEFORE INSERT ON doc_updates          BEGIN SELECT RAISE(ABORT, 'panne simulée'); END",
    )
    .await;
    a.type_text(&id, " perdu");
    assert!(a.wait_until(WAIT, |c| c.is_closed()).await, "A est fermé");
    assert!(b.wait_until(WAIT, |c| c.is_closed()).await, "B aussi");
    assert_eq!(text(&b.blocks()), "Avant", "B n'a rien reçu");
    assert_eq!(
        text(&f.hub.read_blocks(f.page).await.unwrap()),
        "Avant",
        "le document vivant est rechargé depuis le store"
    );

    // La panne passée, l'écriture reprend.
    exec_sql(&f.db, "DROP TRIGGER panne").await;
    let c = f.client().await;
    c.type_text(&id, " après");
    c.resync();
    assert!(c.wait_until(WAIT, |c| c.step2_count() >= 2).await);
    assert_eq!(
        text(&f.hub.read_blocks(f.page).await.unwrap()),
        "Avant après"
    );
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "Avant après"
    );
}

/// `seq` de l'instantané et nombre de mises à jour au journal.
async fn snapshot_and_journal(f: &Fixture) -> (i64, usize) {
    let state = f.store.load_doc(f.page).await.unwrap();
    (
        state.snapshot.as_ref().map_or(-1, |(seq, _)| *seq),
        state.updates.len(),
    )
}

#[tokio::test]
async fn failed_compaction_keeps_the_journal_and_retries_later() {
    let f = Fixture::new(SyncConfig {
        snapshot_every: 3,
        ..SyncConfig::default()
    })
    .await;
    f.write_mmd("Avant\n").await;
    let id = f.ids().await[0].clone();
    let a = f.client().await;
    let b = f.client().await;
    assert_eq!(snapshot_and_journal(&f).await, (0, 1));

    // Panne simulée : toute écriture d'un instantané échoue, le journal
    // s'écrit toujours. La troisième mise à jour déclenche une compaction.
    for event in ["INSERT", "UPDATE"] {
        exec_sql(
            &f.db,
            &format!(
                "CREATE TRIGGER panne_{event} BEFORE {event} ON doc_snapshots \
                 BEGIN SELECT RAISE(ABORT, 'panne simulée'); END"
            ),
        )
        .await;
    }
    a.type_text(&id, " un");
    a.type_text(&id, " deux");
    processed(&a).await;

    // La compaction a échoué : l'instantané n'a pas bougé, et le journal est
    // entier. Rien n'est perdu, ni dans le store ni pour les clients, et
    // personne n'est fermé (ce n'est pas la panne d'écriture du Ruling R21).
    assert_eq!(snapshot_and_journal(&f).await, (0, 3));
    assert!(
        b.wait_until(WAIT, |c| text(&c.blocks()) == "Avant un deux")
            .await,
        "B a reçu les mises à jour"
    );
    assert!(!a.is_closed() && !b.is_closed());
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "Avant un deux"
    );
    // Un client qui arrive reçoit tout.
    assert_eq!(text(&f.client().await.blocks()), "Avant un deux");

    // La panne passée, la compaction n'est pas retentée à chaque mise à
    // jour, mais après `snapshot_every` autres.
    for event in ["INSERT", "UPDATE"] {
        exec_sql(&f.db, &format!("DROP TRIGGER panne_{event}")).await;
    }
    a.type_text(&id, " trois");
    a.type_text(&id, " quatre");
    processed(&a).await;
    assert_eq!(snapshot_and_journal(&f).await, (0, 5));
    a.type_text(&id, " cinq");
    processed(&a).await;
    assert_eq!(
        snapshot_and_journal(&f).await,
        (6, 0),
        "compaction réussie : le journal est purgé"
    );

    let expected = "Avant un deux trois quatre cinq";
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), expected);
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        expected
    );
    assert!(b.wait_until(WAIT, |c| text(&c.blocks()) == expected).await);
}

#[tokio::test]
async fn page_without_document_gets_one_on_first_load() {
    // Un nœud dont `create_page_doc` n'a pas abouti (Ruling R28).
    let dir = tmp::TestDir::new();
    let store = Store::open(&dir.path().join("mementum.db")).await.unwrap();
    let page = new_page(&store).await;
    let hub = Hub::new(store.clone(), SyncConfig::default());
    let server = TestServer::start(hub.clone()).await;

    // Deux clients ouvrent la page ensemble : une seule racine, celle du
    // serveur, écrite avant toute mise à jour d'un client.
    let url = server.url(page, SCHEMA_VERSION);
    let (a, b) = tokio::join!(Client::connect_synced(&url), Client::connect_synced(&url));
    assert_eq!(a.blocks().len(), 1);
    assert_eq!(a.blocks(), b.blocks());
    let state = store.load_doc(page).await.unwrap();
    assert_eq!(state.snapshot.as_ref().map(|(seq, _)| *seq), Some(0));
    assert!(state.updates.is_empty());

    // `create_page_doc` après coup : sans effet.
    hub.create_page_doc(page).await.unwrap();
    assert_eq!(store.load_doc(page).await.unwrap(), state);

    let id = a.blocks()[0].id.clone();
    a.type_text(&id, "x");
    assert!(b.wait_until(WAIT, |c| text(&c.blocks()) == "x").await);
    assert_eq!(stored_page(&store, page).await.read_blocks(), b.blocks());
}

/// `n` paragraphes en MMD, sans ancres : chaque écriture crée des blocs neufs.
fn paragraphs(n: usize, word: &str) -> String {
    (1..=n)
        .map(|i| format!("{word} {i}, un paragraphe ordinaire."))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}

/// La page est compactée (journal vide), et relue depuis SQLite, c'est le même
/// document que le document vivant et que celui de `client`, à la même révision.
async fn assert_compacted_and_identical(f: &Fixture, client: &Client) {
    let live = f.hub.read_blocks(f.page).await.unwrap();
    assert!(client.wait_until(WAIT, |c| c.blocks() == live).await);
    let state = f.store.load_doc(f.page).await.unwrap();
    assert!(
        state.updates.is_empty(),
        "journal compacté, il reste {} mises à jour",
        state.updates.len()
    );
    let stored = stored_page(&f.store, f.page).await;
    assert_eq!(stored.read_blocks(), live);
    assert_eq!(stored.revision(), client.page().unwrap().revision());
}

/// Ruling R40 : une grosse suppression est compactée aussitôt écrite, qu'elle
/// vienne du serveur (`PUT /mmd`) ou d'un client ; le chargement ne rejoue
/// donc plus le ramasse-miettes de yrs. La page relue depuis SQLite est la même,
/// à la même révision.
#[tokio::test]
async fn big_deletion_is_compacted_at_once() {
    // (a) Réécriture complète par le serveur.
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd(&paragraphs(200, "Avant")).await;
    let reader = f.client().await;
    // Écrire une page neuve ne supprime que son paragraphe vide : pas de compaction.
    assert_eq!(f.store.load_doc(f.page).await.unwrap().updates.len(), 1);
    f.write_mmd(&paragraphs(200, "Après")).await;
    assert_compacted_and_identical(&f, &reader).await;

    // (b) Un client remplace tout le contenu, en une mise à jour.
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd(&paragraphs(200, "Avant")).await;
    let writer = f.client().await;
    let update = writer
        .page()
        .unwrap()
        .write_blocks(&parse(&paragraphs(200, "Collé")).unwrap())
        .unwrap();
    writer.apply_local(&update);
    processed(&writer).await;
    assert_compacted_and_identical(&f, &writer).await;
}

/// Efface les `n` premiers caractères du premier bloc, côté `client`.
fn erase_start(client: &Client, n: u32) {
    let doc = detached(client);
    let paragraph = first_content(&doc);
    let before = doc.transact().state_vector();
    {
        let mut txn = doc.transact_mut();
        let Some(XmlOut::Text(text)) = paragraph.get(&txn, 0) else {
            panic!("le premier bloc n'a pas de texte");
        };
        text.remove_range(&mut txn, 0, n);
    }
    let update = doc.transact().encode_state_as_update_v1(&before);
    client.apply_local(&update);
}

/// Les petites suppressions restent au journal ; leur cumul déclenche la
/// compaction quand il atteint `compact_after_deletions`.
#[tokio::test]
async fn small_deletions_compact_once_their_sum_is_large() {
    let f = Fixture::new(SyncConfig {
        compact_after_deletions: 50,
        ..SyncConfig::default()
    })
    .await;
    // 80 caractères ; l'écriture ne supprime que le paragraphe vide de la page neuve.
    f.write_mmd(&format!("{}\n", "abcdefghij".repeat(8))).await;
    let client = f.client().await;
    let journal = || async { f.store.load_doc(f.page).await.unwrap().updates.len() };
    assert_eq!(journal().await, 1);

    erase_start(&client, 30);
    processed(&client).await;
    assert_eq!(journal().await, 2, "30 caractères effacés : sous le seuil");

    erase_start(&client, 30);
    processed(&client).await;
    assert_eq!(
        journal().await,
        0,
        "60 caractères effacés en tout : compaction"
    );
    assert_compacted_and_identical(&f, &client).await;
    assert_eq!(text(&client.blocks()), "abcdefghij".repeat(2));
}

/// Une grosse suppression restée au journal (panne entre l'écriture et la
/// compaction, ou base écrite avant R40) est compactée au chargement suivant :
/// elle ne coûte qu'une fois.
#[tokio::test]
async fn journal_with_big_deletion_is_compacted_on_load() {
    let f = Fixture::new(SyncConfig {
        compact_after_deletions: u64::MAX,
        ..SyncConfig::default()
    })
    .await;
    f.write_mmd(&paragraphs(200, "Avant")).await;
    f.write_mmd(&paragraphs(200, "Après")).await;
    assert_eq!(f.store.load_doc(f.page).await.unwrap().updates.len(), 2);
    let expected = f.hub.read_blocks(f.page).await.unwrap();

    // Le hub s'arrête, puis un autre, au réglage par défaut, rouvre la page.
    let Fixture {
        dir,
        store,
        hub,
        server,
        page,
        ..
    } = f;
    server.stop().await;
    drop(hub);
    let hub = Hub::new(store.clone(), SyncConfig::default());
    assert_eq!(hub.read_blocks(page).await.unwrap(), expected);
    assert!(store.load_doc(page).await.unwrap().updates.is_empty());
    assert_eq!(stored_page(&store, page).await.read_blocks(), expected);
    drop(dir);
}
