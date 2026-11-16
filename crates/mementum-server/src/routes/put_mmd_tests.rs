//! Tests de `PUT /mmd` qui ont besoin de l'état du serveur (le hub, la file des lectures), en
//! appelant la route elle-même : Rulings R430 (récrit) et R436.

use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_store::{NewNode, NodeKind};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::AppState;
use crate::access::tests::{account, state};
use crate::auth::{AuthUser, SessionUser};
use crate::error::ApiError;
use crate::mmd_reads::MAX_MMD_READS;
use crate::routes::pages::put_mmd;

async fn with_session(state: &AppState, user: AuthUser) -> SessionUser {
    let (_, session) = crate::auth::new_token();
    state
        .store
        .create_session(user.id, session, i64::MAX)
        .await
        .unwrap();
    SessionUser { user, session }
}

/// Une instruction SQL (ou plusieurs, séparées par `;;`) sur la base, par une connexion à part ;
/// rend la première colonne de la première ligne de la dernière, s'il y en a une.
async fn sql(db: &std::path::Path, statement: &str) -> i64 {
    use sqlx::Row;
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    let mut value = 0;
    for part in statement.split(";;") {
        let rows = sqlx::query(sqlx::AssertSqlSafe(part.to_string()))
            .fetch_all(&pool)
            .await
            .unwrap();
        if let Some(row) = rows.first() {
            value = row.try_get(0).unwrap_or(0);
        }
    }
    pool.close().await;
    value
}

fn paragraphs(tag: &str, n: usize) -> String {
    let lines: Vec<String> = (0..n).map(|i| format!("{tag} {i}")).collect();
    lines.join("\n\n") + "\n"
}

/// Attend que `ok` soit vrai, une minute au plus.
async fn until(what: &str, mut ok: impl AsyncFnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !ok().await {
        assert!(Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// Une page de l'espace d'Olga où chacun de `editors` peut modifier.
async fn page(state: &AppState, space: Uuid, owner: &AuthUser, editors: &[&AuthUser]) -> Uuid {
    let mut grants = vec![Grant {
        subject: Subject::User(owner.id),
        level: Level::Full,
    }];
    grants.extend(editors.iter().map(|user| Grant {
        subject: Subject::User(user.id),
        level: Level::Edit,
    }));
    state
        .store
        .create_node(NewNode {
            space_id: space,
            parent_id: None,
            after: None,
            kind: NodeKind::Page,
            title: "Page".to_string(),
            created_by: Some(owner.id),
            grants,
            doc: None,
        })
        .await
        .unwrap()
        .id
}

/// `PUT /mmd` de `who` sur `page`, dans sa propre tâche (que le test peut couper).
fn put(
    state: &AppState,
    who: &SessionUser,
    page: Uuid,
    mmd: String,
) -> JoinHandle<Result<StatusCode, ApiError>> {
    let (state, who) = (
        state.clone(),
        SessionUser {
            user: who.user.clone(),
            session: who.session,
        },
    );
    tokio::spawn(async move {
        put_mmd(
            State(state),
            who,
            Ok(Path(page.to_string())),
            Ok(Bytes::from(mmd)),
        )
        .await
    })
}

/// Ruling R430 (récrit), constat I3 de la re-relecture du tour 4 : une requête coupée par le
/// client une fois qu'elle tient le verrou de la page ne le rend qu'une fois son écriture faite.
/// Une écriture longue de Tom part ; dès que le hub la voit dans l'acteur, elle est coupée ; puis
/// dix requêtes d'une ligne, chacune coupée dès que le hub voit son chargement de la page (il
/// part juste après le verrou), jusqu'à la première qui ne l'obtient pas avant que l'écriture
/// longue soit au journal. L'accès de Tom est alors retiré ; Olga écrit en dernier, et sa réponse
/// dit que tout ce qui la précédait est écrit. Sous la mutation « verrou tenu par la requête »,
/// chaque requête coupée rend le verrou aussitôt, passe ses contrôles pendant l'écriture longue,
/// et écrit après le retrait : dix écritures sans l'accès, à chaque passage. Sans minutage ni
/// crochet dans le code : `Hub::calls` suffit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requests_cut_while_holding_the_lock_write_after_the_revocation_at_most_once() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("m.db");
    let state = state(&dir).await;
    let olga = account(&state, "olga").await;
    let tom = account(&state, "tom").await;
    let space = state
        .store
        .create_space("Équipe", Some(olga.id))
        .await
        .unwrap();
    state
        .store
        .add_member(space.id, tom.id, Role::Member)
        .await
        .unwrap();
    let page = page(&state, space.id, &olga, &[&tom]).await;
    let tom_id = tom.id;
    // Le témoin : chaque mise à jour que Tom écrit au journal de la page y note, dans la
    // transaction qui l'écrit, s'il avait encore sa ligne d'accès.
    sql(
        &db,
        &format!(
            "CREATE TABLE witness (seq INTEGER NOT NULL, granted INTEGER NOT NULL);; \
             CREATE TRIGGER witness_writes AFTER INSERT ON doc_updates \
             WHEN NEW.node_id = '{page}' AND NEW.actor_id = '{tom_id}' \
             BEGIN INSERT INTO witness VALUES (NEW.seq, \
               (SELECT COUNT(*) FROM acl WHERE node_id = NEW.node_id \
                AND subject = 'user:{tom_id}')); \
             END"
        ),
    )
    .await;
    let tom = with_session(&state, tom).await;
    let olga = with_session(&state, olga).await;
    state.hub.load_page(page).await.unwrap();
    let written = |granted: i64| {
        let db = db.clone();
        async move {
            sql(
                &db,
                &format!("SELECT COUNT(*) FROM witness WHERE granted = {granted}"),
            )
            .await
        }
    };

    // L'écriture longue, dans l'acteur (son chargement de la page et son écriture demandés).
    let base = state.hub.calls();
    let first = put(&state, &tom, page, paragraphs("Alpha", 4_000));
    until("l'écriture longue n'arrive pas à l'acteur", async || {
        state.hub.calls() >= base + 2
    })
    .await;
    first.abort();
    // Les suivantes : chacune coupée dès qu'elle tient le verrou.
    let (mut held, mut waiting) = (0, Vec::new());
    for i in 0..10 {
        let before = state.hub.calls();
        let request = put(&state, &tom, page, format!("Tom {i}\n"));
        let mut got_it = false;
        until("ni verrou ni écriture", async || {
            if state.hub.calls() > before {
                got_it = true;
                return true;
            }
            written(1).await >= 1
        })
        .await;
        if got_it {
            held += 1;
            request.abort();
        } else {
            waiting.push(request);
            break;
        }
    }
    until("l'écriture longue n'est pas écrite", async || {
        written(1).await >= 1
    })
    .await;
    // Le retrait de l'accès de Tom, comme le ferait la route (révision avancée : les caches).
    sql(
        &db,
        &format!(
            "DELETE FROM acl WHERE node_id = '{page}' AND subject = 'user:{tom_id}';; \
             UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = '{}'",
            space.id
        ),
    )
    .await;
    // Olga écrit après tout le monde : sa réponse dit que tout ce qui précède est écrit.
    let last = put(&state, &olga, page, "Olga\n".to_string());
    assert_eq!(last.await.unwrap().unwrap(), StatusCode::NO_CONTENT);
    for request in waiting {
        let _ = request.await;
    }
    let (with, without) = (written(1).await, written(0).await);
    assert!(
        without <= 1,
        "écritures sans l'accès : {without} (avec : {with}, coupées verrou tenu : {held})"
    );
}

/// Ruling R436, constat I2 de la re-relecture du tour 4 : pendant que Tom a trente-deux `PUT
/// /mmd` lents dans la file des lectures (huit par page, sur quatre pages : la file d'une page
/// en admet huit, R435), le `PUT /mmd` d'une ligne de Mona passe au premier tour de lecture qui
/// lui revient, pas après les trente-deux. La preuve se lit dans ce qui reste de la rafale quand
/// Mona a sa réponse, pas à l'horloge : en file d'arrivée, toute la rafale serait lue avant elle.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_account_does_not_hold_back_another_ones_mmd() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(&dir).await;
    let olga = account(&state, "olga").await;
    let tom = account(&state, "tom").await;
    let mona = account(&state, "mona").await;
    let space = state
        .store
        .create_space("Équipe", Some(olga.id))
        .await
        .unwrap();
    for user in [&tom, &mona] {
        state
            .store
            .add_member(space.id, user.id, Role::Member)
            .await
            .unwrap();
    }
    let mut toms_pages = Vec::new();
    for _ in 0..4 {
        toms_pages.push(page(&state, space.id, &olga, &[&tom]).await);
    }
    let monas_page = page(&state, space.id, &olga, &[&mona]).await;
    let tom = with_session(&state, tom).await;
    let mona = with_session(&state, mona).await;
    // Long à lire, puis refusé (trop long à écrire) : rien ne va au verrou ni à l'acteur.
    let slow = "a*b*".repeat(150_000) + "\n";
    let mut burst = Vec::new();
    for page in &toms_pages {
        for _ in 0..crate::page_writes::MAX_PAGE_WRITES {
            burst.push(put(&state, &tom, *page, slow.clone()));
        }
    }
    let queued = burst.len() - MAX_MMD_READS;
    until("la rafale de Tom n'est pas en file", async || {
        state.mmd_reads.counts().1 >= queued
    })
    .await;
    let reply = put(&state, &mona, monas_page, "Mona\n".to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply, StatusCode::NO_CONTENT);
    let left = burst.iter().filter(|r| !r.is_finished()).count();
    assert!(
        left >= burst.len() / 2,
        "Mona a eu sa réponse quand {} des {} lectures de Tom étaient faites",
        burst.len() - left,
        burst.len()
    );
    for request in burst {
        let refused = request.await.unwrap().unwrap_err().into_response();
        assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
