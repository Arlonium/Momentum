//! Durabilité : persister avant de diffuser, compacter sans rien perdre,
//! redémarrer sans rien perdre.

mod common;

use std::time::Duration;

use common::*;
use mementum_schema::SCHEMA_VERSION;
use mementum_store::Store;
use mementum_sync::{Hub, SyncConfig};

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
