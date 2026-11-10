//! Un client d'un autre schéma est en lecture seule, même avec le droit
//! d'écrire : il reçoit tout, ses mises à jour sont ignorées (y-prosemirror
//! effacerait chez tout le monde les blocs qu'il ne connaît pas).

mod common;

use common::*;
use mementum_schema::SCHEMA_VERSION;
use mementum_sync::ConnInfo;

#[tokio::test]
async fn stale_schema_client_is_read_only() {
    assert_read_only(ConnInfo {
        client_schema: SCHEMA_VERSION - 1,
        ..writing_conn()
    })
    .await;
}

#[tokio::test]
async fn newer_schema_client_is_read_only() {
    assert_read_only(ConnInfo {
        client_schema: SCHEMA_VERSION + 1,
        ..writing_conn()
    })
    .await;
}
