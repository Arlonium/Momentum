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

/// Un tour tenu trop longtemps se journalise (Ruling R518) : l'écriture qui l'attend, une fois
/// le seuil passé (`StoreOptions::slow_write`), puis le tour, en tombant. Sans cela, un tour tenu
/// à tort gèlerait toutes les écritures sans rien dire.
#[tokio::test]
async fn a_turn_held_too_long_is_logged() {
    let logs = common::Logs::capture();
    let dir = common::tmp::TestDir::new();
    let options = mementum_store::StoreOptions {
        slow_write: std::time::Duration::from_millis(50),
        ..Default::default()
    };
    let store = mementum_store::Store::open_with(&dir.path().join("m.db"), options)
        .await
        .unwrap();
    let held = store.hold_writes().await;
    let write = tokio::spawn({
        let store = store.clone();
        async move { store.create_space("Après", None).await }
    });
    let deadline = tokio::time::Instant::now() + WAIT;
    while !logs
        .text()
        .contains("une écriture attend son tour depuis plus de")
    {
        assert!(tokio::time::Instant::now() < deadline, "{}", logs.text());
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    drop(held);
    write.await.unwrap().unwrap();
    assert!(
        logs.text().contains("tour d'écriture tenu"),
        "{}",
        logs.text()
    );
    store.close().await;
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
        ..Default::default()
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

/// Boucles de favoris de Tom pendant les envois de fichiers.
const FILE_LOOPS: usize = 16;

/// Favoris de Tom validés, au plus, pendant l'envoi d'Olga : l'attache de Tom attend une écriture
/// de Tom, le trajet de l'envoi d'Olga jusqu'au verrou du dépôt en laisse passer une ou deux, et
/// son attache une de plus (vu : quatre). Quand l'attache attendait dans la voie de l'auteur, elle
/// attendait toutes ses boucles (vu : dix-huit).
const FILE_READER_WRITES_AT_MOST: i64 = 8;

/// Le verrou du dépôt des fichiers, commun à l'instance, n'attend jamais la rafale d'un compte
/// (Ruling R522) : Tom a seize boucles de favoris en file quand il envoie un fichier ; son envoi
/// pose le contenu sous le verrou du dépôt, puis attend son tour pour l'attacher. Olga envoie
/// ensuite un fichier : elle attend le verrou, donc l'attache de Tom. Si l'attache attendait
/// dans la voie de Tom, derrière ses favoris en file, tous les envois de l'instance les
/// attendraient ; dans la voie du dépôt, l'attache passe après une écriture de Tom. Le compte se
/// lit dans la base : les favoris de Tom validés entre le départ de l'envoi d'Olga et son attache.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_file_store_lock_never_waits_for_an_account_burst() {
    use sha2::{Digest, Sha256};
    let t = open_app_with(SLOW_DISK).await;
    let team = common::Team::new(&t).await;
    let grants = [user(&team.olga, Level::Edit), user(&team.tom, Level::Edit)];
    let page = team.page(None, "Fichiers", &grants).await;
    let mut favorites = Vec::new();
    for i in 0..FILE_LOOPS {
        let read = [user(&team.tom, Level::Read)];
        favorites.push(team.page(None, &format!("Lue {i}"), &read).await);
    }
    exec_sql(
        &t.db_path(),
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER witness_favorite_in AFTER INSERT ON favorites \
         BEGIN INSERT INTO witness (what) VALUES ('favori'); END; \
         CREATE TRIGGER witness_favorite_out AFTER DELETE ON favorites \
         BEGIN INSERT INTO witness (what) VALUES ('favori'); END; \
         CREATE TRIGGER witness_attachment AFTER INSERT ON attachments \
         BEGIN INSERT INTO witness (what) VALUES (NEW.name); END;",
    )
    .await;
    let reader = Loops::start(
        &t,
        &team.tom.browser,
        FILE_LOOPS,
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
    let deadline = tokio::time::Instant::now() + WAIT;
    while reader.done() < 2 * FILE_LOOPS && !reader.failed() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "le lecteur n'écrit pas"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let upload = |name: &str, bytes: &'static [u8]| {
        Request::post(common::upload_uri(page, name))
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from(bytes))
            .unwrap()
    };
    // L'envoi de Tom : son contenu posé dans le dépôt, il tient le verrou jusqu'à l'attache.
    const TOM: &[u8] = b"le fichier de Tom";
    let tom = {
        let request = upload("tom.txt", TOM);
        let tom = team.tom.browser.cookie().unwrap();
        let t = &t;
        async move { answer(t, request, Some(&tom)).await }
    };
    let hex: String = Sha256::digest(TOM)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let posed = t.cfg.data_dir.join("files").join(&hex[..2]).join(&hex[2..]);
    let olga = async {
        let deadline = tokio::time::Instant::now() + WAIT;
        while !posed.is_file() {
            assert!(tokio::time::Instant::now() < deadline, "le contenu de Tom");
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        let before = last_witnessed(&t).await;
        let cookie = team.olga.browser.cookie().unwrap();
        let status = answer(&t, upload("olga.txt", b"le fichier d'Olga"), Some(&cookie)).await;
        (before, status)
    };
    let (tom, (before, olga)) = tokio::join!(tom, olga);
    assert_eq!(tom, Some(StatusCode::CREATED));
    assert_eq!(olga, Some(StatusCode::CREATED));
    let waited = reader_writes_before(&t, before, "olga.txt").await;
    let refused = reader.stop().await;
    assert!(refused.is_empty(), "favoris refusés : {refused:?}");
    assert!(
        waited <= FILE_READER_WRITES_AT_MOST,
        "favoris de Tom validés pendant l'envoi d'Olga : {waited} (au plus \
         {FILE_READER_WRITES_AT_MOST})"
    );
}

/// Un `409 account_changed` (Ruling R540) ne prend pas même un tour de la file : une connexion
/// annexe tient le verrou d'écriture de SQLite, et le délai d'attente du store est d'une minute ;
/// une requête qui prendrait un tour, ou écrirait, ne répondrait pas. Une création de page, puis
/// un favori, nommant un autre compte que celui de la session, répondent tous deux `409`, verrou
/// tenu. La session, neuve, ne glisse pas (`session_user`, la seule écriture admise avant le
/// contrôle). La même création, nommant le bon compte, aboutit une fois le verrou rendu.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn account_changed_takes_no_turn() {
    let t = open_app_with(mementum_store::StoreOptions {
        busy_timeout: std::time::Duration::from_secs(60),
        ..Default::default()
    })
    .await;
    let olga = t.account("Olga").await;
    let page = olga
        .create(serde_json::json!({ "spaceId": olga.space.to_string(), "title": "Lue" }))
        .await
        .id;
    let cookie = olga.browser.cookie().unwrap();
    let naming = |account: Uuid, mut request: Request<Body>| {
        let value = account.to_string().parse().unwrap();
        request.headers_mut().insert("x-mementum-account", value);
        request
    };
    let create = || {
        json_request(
            Method::POST,
            "/api/v1/pages",
            &serde_json::json!({ "spaceId": olga.space.to_string(), "title": "Neuve" }),
        )
    };
    let favorite = || {
        Request::put(format!("/api/v1/pages/{page}/favorite"))
            .body(Body::empty())
            .unwrap()
    };
    let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", t.db_path().display()))
        .await
        .unwrap();
    let mut lock = side.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let other = Uuid::now_v7();
    let changed = [
        answer(&t, naming(other, create()), Some(&cookie)).await,
        answer(&t, naming(other, favorite()), Some(&cookie)).await,
    ];
    let witness = tokio::spawn({
        let (router, cookie) = (t.router.clone(), cookie.clone());
        let mut request = naming(olga.id, create());
        request
            .headers_mut()
            .insert(header::HOST, "localhost".parse().unwrap());
        request
            .headers_mut()
            .insert(header::COOKIE, cookie.parse().unwrap());
        async move {
            use tower::ServiceExt;
            router.oneshot(request).await.unwrap().status()
        }
    });
    sqlx::query("ROLLBACK").execute(&mut *lock).await.unwrap();
    assert_eq!(changed, [Some(StatusCode::CONFLICT); 2]);
    assert_eq!(witness.await.unwrap(), StatusCode::CREATED);
}
