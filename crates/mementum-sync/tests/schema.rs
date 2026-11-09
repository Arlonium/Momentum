//! Un client d'un autre schéma est en lecture seule : il reçoit tout, ses
//! mises à jour sont ignorées (y-prosemirror effacerait chez tout le monde les
//! blocs qu'il ne connaît pas).

mod common;

use common::*;
use mementum_schema::SCHEMA_VERSION;
use mementum_sync::SyncConfig;

async fn assert_read_only(schema: u32) {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Bonjour\n").await;
    let id = f.ids().await[0].clone();
    let other = Client::connect_synced(&f.server.url(f.page, schema)).await;
    let observer = f.client().await;
    let writer = f.client().await;
    assert_eq!(text(&other.blocks()), "Bonjour", "il reçoit tout");
    let rows = row_counts(&f.db).await;

    other.type_text(&id, " ZZZ");
    // Une connexion est servie dans l'ordre : la réponse à cette étape 1 vient
    // après le traitement de sa mise à jour.
    other.resync();
    assert!(other.wait_until(WAIT, |c| c.step2_count() >= 2).await);
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "Bonjour");
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "Bonjour"
    );

    // Les autres écrivent : il le reçoit. L'observateur, qui reçoit les
    // diffusions dans l'ordre, ne voit jamais sa mise à jour.
    writer.type_text(&id, "!");
    assert!(
        observer
            .wait_until(WAIT, |c| text(&c.blocks()) == "Bonjour!")
            .await
    );
    assert!(
        other
            .wait_until(WAIT, |c| text(&c.blocks()).contains('!'))
            .await,
        "il reçoit les mises à jour des autres"
    );
    assert!(
        text(&other.blocks()).contains("ZZZ"),
        "sa copie garde son texte"
    );
    assert!(!text(&observer.blocks()).contains("ZZZ"));
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "Bonjour!");
    assert!(!other.is_closed());
}

#[tokio::test]
async fn stale_schema_client_is_read_only() {
    assert_read_only(SCHEMA_VERSION - 1).await;
}

#[tokio::test]
async fn newer_schema_client_is_read_only() {
    assert_read_only(SCHEMA_VERSION + 1).await;
}
