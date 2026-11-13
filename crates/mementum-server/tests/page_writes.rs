//! Une écriture du serveur à la fois par page (Ruling R200).
//!
//! `PUT /mmd` et la restauration d'une version contrôlent les droits deux fois, puis envoient
//! une commande à l'acteur de la page (Ruling R89). Envoyées d'un coup, des dizaines de ces
//! requêtes passaient toutes leurs contrôles dans les premières millisecondes, puis attendaient
//! dans la boîte de l'acteur : un droit retiré ensuite ne les arrêtait plus, et elles
//! remplaçaient le contenu de la page l'une après l'autre (constat S1 de la relecture finale).
//! Chaque page a donc un verrou, pris après le premier contrôle et la lecture du corps, et tenu
//! pendant « second contrôle des droits, commande à l'acteur, réponse » : chaque requête relit
//! ses droits après la précédente.
//!
//! Ce que ces tests comptent se lit dans la base, pas à l'horloge : un déclencheur témoin note,
//! dans la transaction qui écrit chaque mise à jour de Tom au journal, s'il avait encore sa
//! ligne d'accès sur la page.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::sync::{open_app, user};
use common::{Account, Team, TestApp, WAIT, exec_sql, query_i64};
use mementum_acl::Level;
use mementum_crdt::PageDoc;
use tokio::task::JoinSet;
use tower::ServiceExt;
use uuid::Uuid;

/// Requêtes envoyées d'un coup.
const BURST: usize = 24;

/// Paragraphes de chaque contenu : assez pour qu'une écriture prenne du temps dans l'acteur.
const PARAGRAPHS: usize = 200;

/// Un MMD de [`PARAGRAPHS`] paragraphes marqués de `tag`.
fn paragraphs(tag: &str) -> String {
    many_paragraphs(tag, PARAGRAPHS)
}

/// Un MMD de `n` paragraphes marqués de `tag`.
fn many_paragraphs(tag: &str, n: usize) -> String {
    let lines: Vec<String> = (0..n).map(|i| format!("{tag} {i}")).collect();
    lines.join("\n\n") + "\n"
}

/// L'état Yjs d'une page qui porte `mmd`.
fn state_of(mmd: &str) -> Vec<u8> {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&mementum_md::parse(mmd).unwrap())
        .unwrap();
    page.encode_state()
}

/// Une requête de `by`, telle qu'un navigateur l'envoie.
fn request(by: &Account<'_>, builder: axum::http::request::Builder, body: Body) -> Request<Body> {
    builder
        .header(header::HOST, "localhost")
        .header(header::COOKIE, by.browser.cookie().expect("une session"))
        .body(body)
        .unwrap()
}

/// Pose le témoin : chaque mise à jour que `who` écrit au journal de `page` y note, dans la
/// transaction qui l'écrit, si `who` avait encore sa ligne d'accès sur la page.
async fn witness(t: &TestApp, page: Uuid, who: Uuid) {
    exec_sql(
        &t.db_path(),
        &format!(
            "CREATE TABLE witness (seq INTEGER NOT NULL, granted INTEGER NOT NULL); \
             CREATE TRIGGER witness_writes AFTER INSERT ON doc_updates \
             WHEN NEW.node_id = '{page}' AND NEW.actor_id = '{who}' \
             BEGIN INSERT INTO witness VALUES (NEW.seq, \
               (SELECT COUNT(*) FROM acl WHERE node_id = NEW.node_id AND subject = 'user:{who}')); \
             END;"
        ),
    )
    .await;
}

/// Les écritures notées par le témoin, avec l'accès (`true`) ou sans.
async fn witnessed(t: &TestApp, granted: bool) -> i64 {
    let sql = format!(
        "SELECT COUNT(*) FROM witness WHERE granted = {}",
        i64::from(granted)
    );
    query_i64(&t.db_path(), &sql).await
}

/// Envoie `requests` d'un coup ; dès qu'une a répondu `204`, Olga retire l'accès de Tom à la
/// page ; puis attend toutes les réponses. Rend leurs statuts.
async fn burst_then_revoke(
    t: &TestApp,
    team: &Team<'_>,
    page: Uuid,
    requests: Vec<Request<Body>>,
) -> Vec<StatusCode> {
    let written = Arc::new(AtomicUsize::new(0));
    let mut burst = JoinSet::new();
    for req in requests {
        let (router, written) = (t.router.clone(), written.clone());
        burst.spawn(async move {
            let status = router.oneshot(req).await.unwrap().status();
            if status == StatusCode::NO_CONTENT {
                written.fetch_add(1, Ordering::SeqCst);
            }
            status
        });
    }
    let deadline = tokio::time::Instant::now() + WAIT;
    while written.load(Ordering::SeqCst) == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "aucune écriture n'a abouti"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    team.olga
        .browser
        .delete(&format!(
            "/api/v1/pages/{page}/access/users/{}",
            team.tom.id
        ))
        .await
        .assert_status(StatusCode::OK);
    let mut statuses = Vec::new();
    while let Some(status) = burst.join_next().await {
        statuses.push(status.unwrap());
    }
    statuses
}

/// Ce qu'on attend d'une rafale, une fois l'accès retiré : au plus une écriture faite sans
/// l'accès (celle qui avait passé son second contrôle juste avant le retrait), et toutes les
/// autres réponses sont un `204` (avant le retrait) ou un `404` (après), jamais autre chose.
async fn assert_the_burst_stopped(t: &TestApp, what: &str, statuses: &[StatusCode]) {
    let written = statuses
        .iter()
        .filter(|s| **s == StatusCode::NO_CONTENT)
        .count();
    let refused = statuses
        .iter()
        .filter(|s| **s == StatusCode::NOT_FOUND)
        .count();
    let (with, without) = (witnessed(t, true).await, witnessed(t, false).await);
    assert!(
        without <= 1 && written + refused == statuses.len() && refused > 0,
        "{what} : {written} réponses 204, {refused} 404, sur {} ; écritures au journal avec \
         l'accès : {with}, sans l'accès : {without} ; statuts : {statuses:?}",
        statuses.len()
    );
}

/// Le constat S1 pour la restauration, avec l'attente inverse. Tom (« Peut modifier ») envoie
/// d'un coup des restaurations de deux versions en alternance ; Olga retire son accès dès que la
/// première a abouti. Après le retrait, au plus une restauration est encore écrite ; les autres
/// répondent `404`.
#[tokio::test]
async fn restores_queued_before_a_revocation_stop_with_it() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let grants = [user(&team.olga, Level::Full), user(&team.tom, Level::Edit)];
    let page = team.page(None, "Restaurations", &grants).await;
    let mut versions = Vec::new();
    for tag in ["Alpha", "Bêta"] {
        let state = state_of(&paragraphs(tag));
        let version = team
            .store
            .put_version(page, 0, &state, &[], mementum_store::VersionReason::Closing)
            .await
            .unwrap();
        versions.push(version);
    }
    witness(&t, page, team.tom.id).await;

    let requests = (0..BURST)
        .map(|i| {
            let uri = format!("/api/v1/pages/{page}/versions/{}/restore", versions[i % 2]);
            request(&team.tom, Request::post(uri), Body::empty())
        })
        .collect();
    let statuses = burst_then_revoke(&t, &team, page, requests).await;
    assert_the_burst_stopped(&t, "restaurations", &statuses).await;
}

/// Le même constat pour `PUT /mmd` (antérieur au lot) : des récritures envoyées d'un coup, en
/// alternance ; après le retrait, au plus une est encore écrite.
#[tokio::test]
async fn mmd_writes_queued_before_a_revocation_stop_with_it() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let grants = [user(&team.olga, Level::Full), user(&team.tom, Level::Edit)];
    let page = team.page(None, "Récritures", &grants).await;
    witness(&t, page, team.tom.id).await;
    let contents = [paragraphs("Alpha"), paragraphs("Bêta")];

    let requests = (0..BURST)
        .map(|i| {
            request(
                &team.tom,
                Request::put(format!("/api/v1/pages/{page}/mmd"))
                    .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
                Body::from(contents[i % 2].clone()),
            )
        })
        .collect();
    let statuses = burst_then_revoke(&t, &team, page, requests).await;
    assert_the_burst_stopped(&t, "PUT /mmd", &statuses).await;
}

/// Un client qui coupe chaque requête peu après l'avoir envoyée ne relâche pas le verrou avant
/// l'écriture : ce qui suit l'obtention du verrou va au bout dans sa propre tâche, verrou tenu.
/// Sinon, la requête suivante passait aussitôt ses contrôles, et la file se reformait dans la
/// boîte de l'acteur. Les requêtes coupées pendant qu'elles attendent le verrou ne le gardent
/// pas, et n'écrivent rien. Une écriture d'Olga, qui prend le même verrou, sert de borne : quand
/// elle a répondu, tout ce qui l'avait avant elle est écrit.
#[tokio::test]
async fn requests_cut_by_the_client_keep_the_lock_until_written() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let grants = [user(&team.olga, Level::Full), user(&team.tom, Level::Edit)];
    let page = team.page(None, "Coupées", &grants).await;
    witness(&t, page, team.tom.id).await;
    // Une écriture bien plus longue que l'écart entre deux coupures : sans le verrou tenu par
    // la tâche, chaque coupure laisserait la suivante passer ses contrôles pendant l'écriture.
    let contents = [
        many_paragraphs("Alpha", 10 * PARAGRAPHS),
        many_paragraphs("Bêta", 10 * PARAGRAPHS),
    ];

    let mut burst = JoinSet::new();
    for i in 0..BURST {
        let req = request(
            &team.tom,
            Request::put(format!("/api/v1/pages/{page}/mmd"))
                .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
            Body::from(contents[i % 2].clone()),
        );
        let router = t.router.clone();
        // Chacune coupée un peu plus tard que la précédente.
        let cut_after = Duration::from_millis(5 * (i as u64 + 1));
        burst.spawn(async move { tokio::time::timeout(cut_after, router.oneshot(req)).await });
    }
    let deadline = tokio::time::Instant::now() + WAIT;
    while witnessed(&t, true).await == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "aucune écriture n'a abouti"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    team.olga
        .browser
        .delete(&format!(
            "/api/v1/pages/{page}/access/users/{}",
            team.tom.id
        ))
        .await
        .assert_status(StatusCode::OK);
    while burst.join_next().await.is_some() {}
    team.olga
        .put_mmd(&page.to_string(), "Olga\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let (with, without) = (witnessed(&t, true).await, witnessed(&t, false).await);
    assert!(
        without <= 1,
        "écritures de Tom avec l'accès : {with}, sans l'accès : {without}"
    );
}

/// Le verrou est par page : pendant qu'une page écrit une rafale de restaurations, une écriture
/// sur une autre page aboutit sans attendre la fin de la rafale.
#[tokio::test]
async fn two_pages_never_wait_for_each_other() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let grants = [user(&team.olga, Level::Full)];
    let busy = team.page(None, "Occupée", &grants).await;
    let other = team.page(None, "Autre", &grants).await;
    let mut versions = Vec::new();
    for tag in ["Alpha", "Bêta"] {
        let state = state_of(&paragraphs(tag));
        let version = team
            .store
            .put_version(busy, 0, &state, &[], mementum_store::VersionReason::Closing)
            .await
            .unwrap();
        versions.push(version);
    }

    let done = Arc::new(AtomicUsize::new(0));
    let mut burst = JoinSet::new();
    for i in 0..BURST {
        let uri = format!("/api/v1/pages/{busy}/versions/{}/restore", versions[i % 2]);
        let req = request(&team.olga, Request::post(uri), Body::empty());
        let (router, done) = (t.router.clone(), done.clone());
        burst.spawn(async move {
            let status = router.oneshot(req).await.unwrap().status();
            done.fetch_add(1, Ordering::SeqCst);
            status
        });
    }
    // La rafale est en route : une restauration a abouti.
    let deadline = tokio::time::Instant::now() + WAIT;
    while done.load(Ordering::SeqCst) == 0 {
        assert!(tokio::time::Instant::now() < deadline, "rafale bloquée");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    team.olga
        .put_mmd(&other.to_string(), "Ailleurs\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let finished = done.load(Ordering::SeqCst);
    assert!(
        finished < BURST,
        "l'écriture de l'autre page a attendu la fin de la rafale ({finished} sur {BURST})"
    );
    while let Some(status) = burst.join_next().await {
        assert_eq!(status.unwrap(), StatusCode::NO_CONTENT);
    }
}
