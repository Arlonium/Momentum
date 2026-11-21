//! Tests de `PUT /mmd` et de la restauration d'une version qui ont besoin de l'état du serveur
//! (le hub, la file des lectures, la file des écritures), en appelant la route elle-même :
//! Rulings R430 (récrit), R436, R494 et R535.

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
            axum::http::HeaderMap::new(),
            Ok(Bytes::from(mmd)),
        )
        .await
        // Lot 5, Task S7 : la réponse porte l'`ETag` de l'état écrit ; ces tests n'en lisent que
        // le statut.
        .map(|response| response.status())
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

/// Une version de `page` qui porte `mmd`, gardée par le store ; rend son identifiant.
async fn version(state: &AppState, page: Uuid, mmd: &str) -> Uuid {
    let (doc, _) = mementum_crdt::PageDoc::new_page();
    doc.write_blocks(&mementum_md::parse(mmd).unwrap()).unwrap();
    state
        .store
        .put_version(
            page,
            0,
            &doc.encode_state(),
            &[],
            mementum_store::VersionReason::Closing,
        )
        .await
        .unwrap()
}

/// La restauration de `version` par `who` sur `page`, dans sa propre tâche.
fn restore(
    state: &AppState,
    who: &SessionUser,
    page: Uuid,
    version: Uuid,
) -> JoinHandle<Result<StatusCode, ApiError>> {
    let (state, who) = (
        state.clone(),
        SessionUser {
            user: who.user.clone(),
            session: who.session,
        },
    );
    tokio::spawn(async move {
        crate::routes::versions::restore(
            State(state),
            who,
            Ok(Path((page.to_string(), version.to_string()))),
        )
        .await
    })
}

/// L'équipe d'un test de file : une page de l'espace d'Olga, où chacun de `editors` peut modifier.
async fn team(state: &AppState, editors: &[&str]) -> (Uuid, Uuid, Vec<SessionUser>) {
    let olga = account(state, "olga").await;
    let space = state
        .store
        .create_space("Équipe", Some(olga.id))
        .await
        .unwrap();
    let mut users = Vec::new();
    for name in editors {
        let user = account(state, name).await;
        state
            .store
            .add_member(space.id, user.id, Role::Member)
            .await
            .unwrap();
        users.push(user);
    }
    let refs: Vec<&AuthUser> = users.iter().collect();
    let page = page(state, space.id, &olga, &refs).await;
    let mut sessions = Vec::new();
    for user in users {
        sessions.push(with_session(state, user).await);
    }
    (page, space.id, sessions)
}

/// Le statut de la réponse d'une requête.
async fn status(request: JoinHandle<Result<StatusCode, ApiError>>) -> StatusCode {
    match request.await.unwrap() {
        Ok(status) => status,
        Err(error) => error.into_response().status(),
    }
}

/// `request` est refusée par `429 page_busy` sans prendre de place dans la file de `page`. La
/// file est tenue pendant ce temps (une écriture retenue à son tour dans le store) : une requête
/// qui y prendrait une place attendrait sans fin, et c'est sa place qui la dénonce.
async fn refused_at_once(
    state: &AppState,
    page: Uuid,
    request: JoinHandle<Result<StatusCode, ApiError>>,
    what: &str,
) {
    let before = state.page_writes.places(page);
    until(what, async || {
        request.is_finished() || state.page_writes.places(page) > before
    })
    .await;
    assert_eq!(state.page_writes.places(page), before, "{what} : une place");
    assert_eq!(
        status(request).await,
        StatusCode::TOO_MANY_REQUESTS,
        "{what}"
    );
}

/// Ruling R494, constat N2 de la re-relecture du tour 5 : un `PUT /mmd` refusé à sa lecture ne
/// tient jamais de place dans la file de la page. Quatre rédacteurs envoient chacun deux `PUT
/// /mmd` lents à lire, puis refusés (trop longs à écrire) ; pendant qu'ils sont lus ou attendent de
/// l'être, Mona écrit. Avant, la place se prenait avant la lecture : les huit tenaient la file
/// pleine, et Mona recevait `429 page_busy` sans que rien ne fût écrit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_mmd_never_holds_a_place() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(&dir).await;
    let (page, _, users) = team(&state, &["tom", "ugo", "vera", "wes", "mona"]).await;
    let (readers, mona) = users.split_at(4);
    let slow = "a*b*".repeat(150_000) + "\n";
    let mut burst = Vec::new();
    for reader in readers {
        for _ in 0..2 {
            burst.push(put(&state, reader, page, slow.clone()));
        }
    }
    until("la rafale n'est pas en lecture", async || {
        state.mmd_reads.counts() == (MAX_MMD_READS, burst.len() - MAX_MMD_READS)
    })
    .await;
    let reply = status(put(&state, &mona[0], page, "Mona\n".to_string())).await;
    assert_eq!(reply, StatusCode::NO_CONTENT, "Mona n'a pas pu écrire");
    for request in burst {
        assert_eq!(status(request).await, StatusCode::UNPROCESSABLE_ENTITY);
    }
}

/// Ruling R494 : un compte n'a que deux places dans la file d'une page. Tom écrit longuement, une
/// deuxième écriture attend derrière : sa troisième est refusée sur-le-champ (`429 page_busy`),
/// pendant que Mona prend une place et écrit à son tour.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_account_holds_at_most_two_places_on_a_page() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(&dir).await;
    let (page, _, users) = team(&state, &["tom", "mona"]).await;
    let (tom, mona) = (&users[0], &users[1]);
    state.hub.load_page(page).await.unwrap();
    let base = state.hub.calls();
    // Ruling R886 : la première écriture attend son tour d'écriture dans le store, que le test
    // tient jusqu'à ce que Mona ait sa place ; sa durée ne décide de rien.
    let held = state.store.hold_writes().await;
    let first = put(&state, tom, page, paragraphs("Alpha", 4_000));
    until("l'écriture longue n'arrive pas à l'acteur", async || {
        state.hub.calls() >= base + 2
    })
    .await;
    until("l'écriture longue n'attend pas son tour", async || {
        state.store.queued_writes() == 1
    })
    .await;
    let second = put(&state, tom, page, paragraphs("Bêta", 10));
    until("la deuxième écriture n'a pas de place", async || {
        state.page_writes.places(page) == 2
    })
    .await;
    let third = put(&state, tom, page, "Tom\n".to_string());
    refused_at_once(&state, page, third, "la troisième de Tom").await;
    let monas = put(&state, mona, page, "Mona\n".to_string());
    until("Mona n'a pas de place", async || {
        state.page_writes.places(page) == 3
    })
    .await;
    drop(held);
    for request in [first, second, monas] {
        assert_eq!(status(request).await, StatusCode::NO_CONTENT);
    }
}

/// Ce qui arrête la file des écritures de Tom.
#[derive(Debug, Clone, Copy)]
enum Ending {
    /// Olga lui retire l'accès à la page.
    Revoked,
    /// Sa session prend fin (déconnexion, mot de passe changé).
    SessionEnded,
}

/// Les écritures de Tom dans la file : des `PUT /mmd`, ou des restaurations de version.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Mmd,
    Restore,
}

/// Rulings R200, R209, R494 et R535 : une écriture longue de Tom est dans l'acteur, une deuxième
/// attend le verrou ; ses six suivantes sont refusées sur-le-champ (sa part est prise). Puis son
/// accès est retiré, ou sa session prend fin : la deuxième relit ses droits et sa session sous le
/// verrou, et n'écrit pas (`404`, `401`) ; seule la première est au journal. Ces tests remplacent
/// les rafales des tests d'intégration de R200 et R209, que la part de deux places par compte
/// réduit à une requête qui attend (pour `PUT /mmd` depuis R494, pour la restauration depuis
/// R535) : sans voir l'acteur, ils ne savaient plus faire tomber le retrait pendant qu'elle
/// attend.
///
/// Sans minutage (Ruling R886) : le test tient le tour d'écriture du store, si bien que la
/// première reste dans l'acteur, verrou de la page tenu, tant que la fin n'est pas posée ; la fin
/// s'écrit par une connexion à part, hors de la file.
async fn queued_writes_stop_with(kind: Kind, ending: Ending) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("m.db");
    let state = state(&dir).await;
    let (page, space, users) = team(&state, &["tom"]).await;
    let tom = &users[0];
    let tom_id = tom.user.id;
    sql(
        &db,
        &format!(
            "CREATE TABLE witness (seq INTEGER NOT NULL);; \
             CREATE TRIGGER witness_writes AFTER INSERT ON doc_updates \
             WHEN NEW.node_id = '{page}' AND NEW.actor_id = '{tom_id}' \
             BEGIN INSERT INTO witness VALUES (NEW.seq); END"
        ),
    )
    .await;
    // Pour les restaurations : une version longue, et une courte.
    let versions = match kind {
        Kind::Mmd => None,
        Kind::Restore => Some((
            version(&state, page, &paragraphs("Alpha", 4_000)).await,
            version(&state, page, "Tom\n").await,
        )),
    };
    let send = |long: bool, mmd: String| match versions {
        Some((alpha, short)) => restore(&state, tom, page, if long { alpha } else { short }),
        None => put(&state, tom, page, mmd),
    };
    state.hub.load_page(page).await.unwrap();
    let base = state.hub.calls();
    // Ruling R886 : la première écriture est retenue à son tour d'écriture dans le store, que le
    // test tient, jusqu'à ce que la fin soit posée. Sa durée ne décide donc de rien : avant,
    // elle devait durer plus que tout ce que le test fait jusqu'au retrait.
    let held = state.store.hold_writes().await;
    let first = send(true, paragraphs("Alpha", 4_000));
    until("l'écriture longue n'arrive pas à l'acteur", async || {
        state.hub.calls() >= base + 2
    })
    .await;
    until("l'écriture longue n'attend pas son tour", async || {
        state.store.queued_writes() == 1
    })
    .await;
    let second = send(false, "Tom 1\n".to_string());
    until("la deuxième écriture n'a pas de place", async || {
        state.page_writes.places(page) == 2
    })
    .await;
    for i in 0..6 {
        let extra = send(false, format!("Tom {}\n", i + 2));
        let what = format!("{kind:?}, {ending:?} : la requête {i} de trop");
        refused_at_once(&state, page, extra, &what).await;
    }
    match ending {
        Ending::Revoked => {
            sql(
                &db,
                &format!(
                    "DELETE FROM acl WHERE node_id = '{page}' AND subject = 'user:{tom_id}';; \
                     UPDATE spaces SET rev = rev + 1, lsn = lsn + 1 WHERE id = '{space}'"
                ),
            )
            .await;
        }
        // Par la base, comme `mementum user set-password` d'un autre processus : la file du
        // store, dont le test tient le tour, ferait attendre `Store::delete_session`.
        Ending::SessionEnded => {
            let gone = sql(
                &db,
                &format!("DELETE FROM sessions WHERE user_id = '{tom_id}' RETURNING 1"),
            )
            .await;
            assert_eq!(gone, 1, "{kind:?} : la session de Tom n'est pas partie");
        }
    }
    // La fin est posée, et rien de Tom n'est encore au journal : la première tient toujours le
    // verrou de la page, la deuxième n'a pas pu relire ses droits.
    assert_eq!(
        (
            sql(&db, "SELECT COUNT(*) FROM witness").await,
            state.page_writes.places(page)
        ),
        (0, 2),
        "{kind:?}, {ending:?} : la première a écrit avant la fin"
    );
    drop(held);
    assert_eq!(
        status(first).await,
        StatusCode::NO_CONTENT,
        "{kind:?}, {ending:?} : la première"
    );
    let expected = match ending {
        Ending::Revoked => StatusCode::NOT_FOUND,
        Ending::SessionEnded => StatusCode::UNAUTHORIZED,
    };
    assert_eq!(
        status(second).await,
        expected,
        "{kind:?}, {ending:?} : la deuxième"
    );
    let written = sql(&db, "SELECT COUNT(*) FROM witness").await;
    assert_eq!(
        written, 1,
        "{kind:?}, {ending:?} : écritures de Tom au journal"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mmd_writes_queued_before_a_revocation_stop_with_it() {
    queued_writes_stop_with(Kind::Mmd, Ending::Revoked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mmd_writes_queued_before_the_session_ends_stop_with_it() {
    queued_writes_stop_with(Kind::Mmd, Ending::SessionEnded).await;
}

/// Pour la restauration : `restores_queued_before_a_revocation_stop_with_it` et les
/// restaurations de `writes_queued_before_the_session_ends_stop_with_it`, tests d'intégration
/// de R200 et R209 (`tests/page_writes.rs`), passés ici avec R535.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restores_queued_before_a_revocation_stop_with_it() {
    queued_writes_stop_with(Kind::Restore, Ending::Revoked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restores_queued_before_the_session_ends_stop_with_it() {
    queued_writes_stop_with(Kind::Restore, Ending::SessionEnded).await;
}

/// Ruling R535, constat M2 de la re-relecture du tour 6 : une restauration prend sa place dans la
/// file de la page sur la part du compte, comme un `PUT /mmd` (R494). Douze boucles de
/// restaurations sans effet (R180) de Tom tenaient la file pleine : Mona, rédactrice, n'écrivait
/// plus (aucune écriture et 93 `429` en dix secondes). Tom tient ses deux places (deux écritures
/// en attente) : sa restauration est refusée sur-le-champ, `429 page_busy` ; celle de Mona passe,
/// et son `PUT /mmd` aussi.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restore_takes_a_place_of_the_account() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(&dir).await;
    let (page, _, users) = team(&state, &["tom", "mona"]).await;
    let (tom, mona) = (&users[0], &users[1]);
    let version = version(&state, page, "Version\n").await;
    // Les deux places de Tom, prises (deux de ses écritures en attente).
    let held: Vec<_> = (0..2)
        .map(|_| state.page_writes.reserve(page, tom.user.id).unwrap())
        .collect();
    let refused = status(restore(&state, tom, page, version)).await;
    assert_eq!(
        refused,
        StatusCode::TOO_MANY_REQUESTS,
        "la restauration de Tom"
    );
    let monas = status(restore(&state, mona, page, version)).await;
    assert_eq!(monas, StatusCode::NO_CONTENT, "celle de Mona");
    let monas = status(put(&state, mona, page, "Mona\n".to_string())).await;
    assert_eq!(monas, StatusCode::NO_CONTENT, "le PUT /mmd de Mona");
    drop(held);
    let again = status(restore(&state, tom, page, version)).await;
    assert_eq!(again, StatusCode::NO_CONTENT, "Tom, ses places rendues");
}

/// Ruling R200 : le verrou est par page. Pendant que la file d'une page est tenue (son verrou
/// pris), un `PUT /mmd` et une restauration sur une autre page aboutissent, et une restauration
/// sur la page tenue attend. Remplace le test d'intégration du même nom (`tests/page_writes.rs`),
/// qui envoyait une rafale de vingt-quatre restaurations d'un même compte : depuis R535, vingt-deux
/// y reçoivent `429`. Si le verrou était commun aux pages, l'écriture de l'autre page attendrait
/// sans fin : le test échoue au bout d'une minute plutôt que de se bloquer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_pages_never_wait_for_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(&dir).await;
    let (busy, space, users) = team(&state, &["tom"]).await;
    let tom = &users[0];
    let other = page(&state, space, &tom.user, &[]).await;
    let (busy_version, other_version) = (
        version(&state, busy, "Occupée\n").await,
        version(&state, other, "Autre\n").await,
    );
    let held = state
        .page_writes
        .reserve(busy, Uuid::now_v7())
        .unwrap()
        .lock()
        .await;
    let waiting = restore(&state, tom, busy, busy_version);
    until(
        "la restauration de la page tenue n'a pas de place",
        async || state.page_writes.places(busy) == 2,
    )
    .await;
    let bound = Duration::from_secs(60);
    let elsewhere =
        tokio::time::timeout(bound, status(put(&state, tom, other, "Ailleurs\n".into())))
            .await
            .expect("le PUT /mmd de l'autre page a attendu la page tenue");
    assert_eq!(elsewhere, StatusCode::NO_CONTENT);
    let elsewhere = tokio::time::timeout(bound, status(restore(&state, tom, other, other_version)))
        .await
        .expect("la restauration de l'autre page a attendu la page tenue");
    assert_eq!(elsewhere, StatusCode::NO_CONTENT);
    assert!(!waiting.is_finished(), "la page tenue n'a pas attendu");
    drop(held);
    assert_eq!(status(waiting).await, StatusCode::NO_CONTENT);
}

/// Lot 5, correction groupée (mutation S7-04 de la relecture finale, Ruling R1387) : `If-Match`
/// se juge dans la commande qui écrit, verrou de la page tenu, jamais avant. Le verrou de la page
/// tenu, Tom envoie un `PUT /mmd` qui nomme la révision courante, et attend le verrou ; une
/// écriture passe alors à côté du verrou (par le hub) ; le verrou rendu, le `PUT /mmd` répond
/// `409 revision_conflict` et l'écriture d'entre-temps reste. Jugée avant le verrou, la
/// révision valait encore : le `PUT /mmd` écrasait l'écriture d'entre-temps, en `204`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn if_match_is_judged_once_the_lock_of_the_page_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(&dir).await;
    let (page, _, users) = team(&state, &["tom"]).await;
    let tom = &users[0];
    assert_eq!(
        status(put(&state, tom, page, "Un\n".into())).await,
        StatusCode::NO_CONTENT
    );
    let read = state.hub.read_page(page).await.unwrap();
    let held = state
        .page_writes
        .reserve(page, Uuid::now_v7())
        .unwrap()
        .lock()
        .await;
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::IF_MATCH,
        format!("\"{}\"", read.revision).parse().unwrap(),
    );
    let waiting = {
        let (state, who) = (
            state.clone(),
            SessionUser {
                user: tom.user.clone(),
                session: tom.session,
            },
        );
        tokio::spawn(async move {
            put_mmd(
                State(state),
                who,
                Ok(Path(page.to_string())),
                headers,
                Ok(Bytes::from("Écrasé\n")),
            )
            .await
            .map(|response| response.status())
        })
    };
    until("le PUT /mmd attend le verrou de la page", async || {
        state.page_writes.places(page) == 2
    })
    .await;
    let between = mementum_md::parse("Entre-temps\n").unwrap();
    state
        .hub
        .write_blocks_at(page, between, Some(tom.user.id), None)
        .await
        .unwrap();
    drop(held);
    assert_eq!(status(waiting).await, StatusCode::CONFLICT);
    let blocks = state.hub.read_blocks(page).await.unwrap();
    let shown = format!("{blocks:?}");
    assert!(
        shown.contains("Entre-temps") && !shown.contains("Écrasé"),
        "l'écriture d'entre-temps reste : {shown}"
    );
}
