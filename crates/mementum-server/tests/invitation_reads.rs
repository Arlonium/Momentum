//! Ce que coûte la liste des invitations d'un espace (`GET /api/v1/spaces/{id}/invitations`),
//! qui porte le titre de la page de chaque invitation de page (report M11 du lot 2) : le titre
//! se lit une fois pour toutes les pages, pas une fois par invitation (Ruling R262).
//!
//! Les lectures se comptent dans le journal des requêtes de sqlx (cible `sqlx::query`, une
//! ligne par instruction, son SQL en entier), écrit par le fil de SQLite : il faut l'abonné de
//! tout le processus ([`all_logs`]). Ce fichier n'a qu'un test, pour que les instructions d'un
//! autre ne s'y mêlent pas.

mod common;

use std::fmt::Write;

use axum::http::StatusCode;
use common::{Team, TestApp, all_logs, exec_sql};
use mementum_acl::{Level, Subject};
use mementum_server::SignupMode;
use uuid::Uuid;

/// Les pages de l'espace et leurs invitations : cinquante pages ouvertes aux membres, mille
/// invitations de page réparties sur elles.
const PAGES: usize = 50;
const INVITATIONS: usize = 1_000;

#[tokio::test]
async fn listing_page_invitations_reads_titles_once() {
    // Avant toute requête : les journaux de tous les fils, au niveau TRACE.
    let logs = all_logs();
    let t = TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await;
    let team = Team::new(&t).await;
    let mut pages = Vec::new();
    for i in 0..PAGES {
        let title = format!("Page {i}");
        pages.push(
            team.page(None, &title, &[(Subject::Members, Level::Full)])
                .await,
        );
    }
    // Les invitations, en une instruction : par le store, une transaction chacune.
    let mut sql = String::from(
        "INSERT INTO invitations (id, space_id, node_id, email, grant_level, token_hash, \
         created_by, created_at, expires_at) VALUES ",
    );
    for i in 0..INVITATIONS {
        let separator = if i == 0 { "" } else { ", " };
        write!(
            sql,
            "{separator}('{}', '{}', '{}', 'x{i}@exemple.fr', 'read', randomblob(32), '{}', {i}, {})",
            Uuid::now_v7(),
            team.space,
            pages[i % PAGES],
            team.olga.id,
            i64::MAX,
        )
        .unwrap();
    }
    exec_sql(&t.db_path(), &sql).await;

    let uri = format!("/api/v1/spaces/{}/invitations", team.space);
    let start = logs.text().len();
    let reply = team.olga.browser.get(&uri).await;
    let journal = logs.text()[start..].to_string();
    reply.assert_status(StatusCode::OK);
    let listed = reply.json();
    assert_eq!(listed.as_array().map(Vec::len), Some(INVITATIONS));
    assert_eq!(listed[1]["pageTitle"], "Page 1");

    // Les instructions qui lisent `nodes` pendant la requête : le titre, et ce que le calcul
    // des droits relit de l'espace. Au moins une (le journal de sqlx est bien lu), au plus une
    // par page.
    let reads = journal.matches("FROM nodes").count();
    assert!(
        (1..=PAGES).contains(&reads),
        "{reads} lectures de `nodes` pour {INVITATIONS} invitations sur {PAGES} pages"
    );
}
