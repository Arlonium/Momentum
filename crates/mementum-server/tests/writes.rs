//! La file d'écriture du store, vue des routes et de la synchro (Rulings R510 à R516) : un compte
//! qui écrit sans relâche ne fait échouer l'écriture de personne, et ne fait attendre la frappe
//! d'un autre que le temps d'une ou deux de ses écritures, pas de toutes celles qu'il a en file.
//!
//! Ce que ces tests comptent se lit dans la base, pas à l'horloge : un déclencheur témoin note,
//! dans la transaction qui l'écrit, chaque favori posé ou retiré et chaque frappe écrite au
//! journal de la page, dans l'ordre où elles sont validées (un seul écrivain à la fois).

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::sync::{Loops, SLOW_DISK, open_app_with, text, types_unheard, user};
use common::{TestApp, WAIT, exec_sql, json_request, query_i64, sync_url, synced_as};
use mementum_acl::Level;
use uuid::Uuid;

/// Boucles d'écritures du lecteur.
const LOOPS: usize = 8;

/// Frappes de Mona mesurées.
const KEYSTROKES: usize = 10;

/// Favoris du lecteur validés, au plus, entre le moment où une frappe (ou une requête d'un autre
/// compte) part et celui où elle est écrite. Ses huit boucles ont presque toujours sept écritures
/// en file : servies dans l'ordre d'arrivée, la frappe les attendrait toutes (vu : huit à chaque
/// frappe, la file mutée en une seule voie). La file sert les voies tour à tour : la frappe, sur
/// celle de sa page, attend l'écriture en cours et au plus une autre du lecteur (vu : deux à
/// chaque frappe) ; le reste est le trajet jusqu'à la file (la WebSocket, l'acteur, la route),
/// pendant lequel le lecteur continue, à 50 ms par écriture sur ce disque lent.
const READER_WRITES_AT_MOST: i64 = 4;

/// Pose le témoin : chaque favori posé ou retiré, chaque mise à jour écrite au journal de `page`
/// (`frappe`), et chaque page créée au titre « Neuve … » (`page`), y prend un numéro, dans la
/// transaction qui l'écrit.
async fn witness(t: &TestApp, page: Uuid) {
    exec_sql(
        &t.db_path(),
        &format!(
            "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
             CREATE TRIGGER witness_favorite_in AFTER INSERT ON favorites \
             BEGIN INSERT INTO witness (what) VALUES ('favori'); END; \
             CREATE TRIGGER witness_favorite_out AFTER DELETE ON favorites \
             BEGIN INSERT INTO witness (what) VALUES ('favori'); END; \
             CREATE TRIGGER witness_keystroke AFTER INSERT ON doc_updates \
             WHEN NEW.node_id = '{page}' \
             BEGIN INSERT INTO witness (what) VALUES ('frappe'); END; \
             CREATE TRIGGER witness_page AFTER INSERT ON nodes \
             WHEN NEW.title LIKE 'Neuve %' \
             BEGIN INSERT INTO witness (what) VALUES ('page'); END;"
        ),
    )
    .await;
}

/// Le dernier numéro du témoin.
async fn last_witnessed(t: &TestApp) -> i64 {
    query_i64(&t.db_path(), "SELECT COALESCE(MAX(n), 0) FROM witness").await
}

/// Les favoris validés après le numéro `before` du témoin, et avant la première écriture `what`
/// qui le suit.
async fn reader_writes_before(t: &TestApp, before: i64, what: &str) -> i64 {
    query_i64(
        &t.db_path(),
        &format!(
            "SELECT COUNT(*) FROM witness WHERE what = 'favori' AND n > {before} \
             AND n < (SELECT MIN(n) FROM witness WHERE what = '{what}' AND n > {before})"
        ),
    )
    .await
}

/// Huit boucles d'écritures d'un simple lecteur (Tom pose et retire des favoris, sans relâche,
/// chaque boucle sur sa page) : aucune écriture de personne n'échoue, ni les siennes, ni les
/// frappes de Mona, ni les pages qu'Olga crée ; et chaque frappe de Mona, comme chaque page
/// d'Olga, est écrite après au plus [`READER_WRITES_AT_MOST`] favoris de Tom validés depuis
/// qu'elle est partie. Pour Olga, c'est la voie de son compte, nommé par sa session : dans une
/// voie commune à toutes les requêtes, elle attendrait toutes celles de Tom.
///
/// Sur un disque lent (`SLOW_DISK`) : sans la file, neuf écritures se disputaient le verrou de
/// SQLite, et l'une d'elles l'attendait plus que le délai d'attente de sa connexion (`database
/// is locked` ; pour une frappe, l'acteur recharge la page et ferme toutes ses connexions).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eight_write_loops_of_a_reader_fail_nobody_and_let_typing_through() {
    let t = open_app_with(SLOW_DISK).await;
    let team = common::Team::new(&t).await;
    let addr = t.listen().await;
    let grants = [
        user(&team.olga, Level::Full),
        user(&team.mona, Level::Edit),
        user(&team.tom, Level::Read),
    ];
    let page = team.page(None, "Commune", &grants).await;
    let mut favorites = Vec::new();
    for i in 0..LOOPS {
        let read = [user(&team.tom, Level::Read)];
        favorites.push(team.page(None, &format!("Lue {i}"), &read).await);
    }
    team.olga
        .put_mmd(&page.to_string(), "Bonjour\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let url = sync_url(addr, page);
    let mona = synced_as(&team.mona.browser, &url).await;
    let olga = synced_as(&team.olga.browser, &url).await;
    witness(&t, page).await;

    // Chaque boucle pose puis retire le favori de sa page : chaque requête écrit.
    let reader = Loops::start(
        &t,
        &team.tom.browser,
        LOOPS,
        StatusCode::NO_CONTENT,
        move |k, round| {
            let method = if round % 2 == 0 {
                Method::PUT
            } else {
                Method::DELETE
            };
            let uri = format!("/api/v1/pages/{}/favorite", favorites[k]);
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap()
        },
    );
    // Le lecteur a toutes ses boucles en route avant la première frappe.
    let deadline = tokio::time::Instant::now() + WAIT;
    while reader.done() < 2 * LOOPS && !reader.failed() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "le lecteur n'écrit pas"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    let (mut waited, mut olga_waited) = (Vec::new(), Vec::new());
    let mut typed = String::from("Bonjour");
    for i in 0..KEYSTROKES {
        let before = last_witnessed(&t).await;
        types_unheard(&mona, "x");
        typed.push('x');
        assert!(
            olga.wait_until(WAIT, |c| text(c) == typed).await,
            "frappe {i} : Olga ne l'a pas reçue ({:?}), connexion de Mona fermée : {}",
            text(&olga),
            mona.is_closed()
        );
        waited.push(reader_writes_before(&t, before, "frappe").await);
        // Et Olga écrit ailleurs, par une route.
        let before = last_witnessed(&t).await;
        team.olga
            .browser
            .post(
                "/api/v1/pages",
                serde_json::json!({ "spaceId": team.space.to_string(), "title": format!("Neuve {i}") }),
            )
            .await
            .assert_status(StatusCode::CREATED);
        olga_waited.push(reader_writes_before(&t, before, "page").await);
    }
    let done = reader.done();
    let refused = reader.stop().await;
    assert!(refused.is_empty(), "favoris refusés : {refused:?}");
    assert!(!mona.is_closed() && !olga.is_closed());
    assert!(
        waited.iter().all(|&n| n <= READER_WRITES_AT_MOST),
        "favoris de Tom validés pendant chaque frappe de Mona : {waited:?} (au plus \
         {READER_WRITES_AT_MOST}) ; {done} favoris en tout"
    );
    assert!(
        olga_waited.iter().all(|&n| n <= READER_WRITES_AT_MOST),
        "favoris de Tom validés pendant chaque création d'Olga : {olga_waited:?} (au plus \
         {READER_WRITES_AT_MOST}) ; {done} favoris en tout"
    );
}

/// Une requête du routeur, avec l'hôte et, s'il y en a un, ce cookie : son statut, si elle répond
/// avant [`WAIT`] ; `None` sinon.
async fn answer(
    t: &TestApp,
    mut request: Request<Body>,
    cookie: Option<&str>,
) -> Option<StatusCode> {
    use tower::ServiceExt;
    let headers = request.headers_mut();
    headers.insert(header::HOST, "localhost".parse().unwrap());
    if let Some(cookie) = cookie {
        headers.insert(header::COOKIE, cookie.parse().unwrap());
    }
    let reply = tokio::time::timeout(WAIT, t.router.clone().oneshot(request)).await;
    reply.ok().map(|reply| reply.unwrap().status())
}

/// Ce qu'un anonyme peut demander n'écrit rien (Ruling R517) : une connexion annexe tient le
/// verrou d'écriture de SQLite, et le délai d'attente du store est d'une minute ; une requête qui
/// écrirait ne répondrait pas, toutes répondent. Une déconnexion à cookie bidon (avant, elle
/// prenait un tour de la voie commune, et cent par seconde y faisaient attendre toute
/// inscription), sans cookie, une connexion au mauvais mot de passe, une inscription à une
/// adresse prise, `/me` à cookie bidon.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn anonymous_requests_write_nothing() {
    use base64::Engine;
    let t = open_app_with(mementum_store::StoreOptions {
        busy_timeout: std::time::Duration::from_secs(60),
        write_delay: std::time::Duration::ZERO,
    })
    .await;
    let _olga = t.signed_up("olga@exemple.fr", "Olga").await;
    let bogus = format!(
        "{}={}",
        common::SESSION_COOKIE,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([7u8; 32])
    );
    let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", t.db_path().display()))
        .await
        .unwrap();
    let mut lock = side.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let logout = || {
        Request::post("/api/v1/auth/logout")
            .body(Body::empty())
            .unwrap()
    };
    let login = json_request(
        Method::POST,
        "/api/v1/auth/login",
        &serde_json::json!({ "email": "olga@exemple.fr", "password": "faux-faux-faux" }),
    );
    let signup = json_request(
        Method::POST,
        "/api/v1/auth/signup",
        &serde_json::json!({ "email": "olga@exemple.fr", "name": "X", "password": common::PASSWORD }),
    );
    let me = Request::get("/api/v1/me").body(Body::empty()).unwrap();
    let answers = [
        (
            "déconnexion à cookie bidon",
            answer(&t, logout(), Some(&bogus)).await,
        ),
        ("déconnexion sans cookie", answer(&t, logout(), None).await),
        ("mauvais mot de passe", answer(&t, login, None).await),
        ("adresse prise", answer(&t, signup, None).await),
        ("/me à cookie bidon", answer(&t, me, Some(&bogus)).await),
    ];
    sqlx::query("ROLLBACK").execute(&mut *lock).await.unwrap();
    let expected = [
        StatusCode::NO_CONTENT,
        StatusCode::NO_CONTENT,
        StatusCode::UNAUTHORIZED,
        StatusCode::CONFLICT,
        StatusCode::UNAUTHORIZED,
    ];
    for ((what, status), expected) in answers.into_iter().zip(expected) {
        assert_eq!(
            status,
            Some(expected),
            "{what} (None : elle attend d'écrire)"
        );
    }
}
