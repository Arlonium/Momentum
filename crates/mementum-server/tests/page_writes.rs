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
    burst_then(t, requests, async {
        team.olga
            .browser
            .delete(&format!(
                "/api/v1/pages/{page}/access/users/{}",
                team.tom.id
            ))
            .await
            .assert_status(StatusCode::OK);
    })
    .await
}

/// Envoie `requests` d'un coup ; dès qu'une a répondu `204`, fait `then` ; puis attend toutes
/// les réponses. Rend leurs statuts.
async fn burst_then(
    t: &TestApp,
    requests: Vec<Request<Body>>,
    then: impl std::future::Future<Output = ()>,
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
    then.await;
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

/// Pose le témoin d'une session : chaque mise à jour que `who` écrit au journal de `page` y note,
/// dans la transaction qui l'écrit, si la session `session` (son hachage) est encore en base.
async fn session_witness(t: &TestApp, page: Uuid, who: Uuid, session: [u8; 32]) {
    let hex: String = session.iter().map(|b| format!("{b:02x}")).collect();
    exec_sql(
        &t.db_path(),
        &format!(
            "CREATE TABLE witness (seq INTEGER NOT NULL, granted INTEGER NOT NULL); \
             CREATE TRIGGER witness_writes AFTER INSERT ON doc_updates \
             WHEN NEW.node_id = '{page}' AND NEW.actor_id = '{who}' \
             BEGIN INSERT INTO witness VALUES (NEW.seq, \
               (SELECT COUNT(*) FROM sessions WHERE token_hash = x'{hex}')); \
             END;"
        ),
    )
    .await;
}

/// Comment la session de Tom prend fin, pendant que ses écritures attendent.
#[derive(Debug, Clone, Copy)]
enum Ending {
    /// Il se déconnecte.
    LogOut,
    /// La titulaire du compte, d'un autre navigateur, change son mot de passe : toutes les
    /// sessions du compte sont fermées.
    PasswordChanged,
}

/// Constat I1 de la relecture de la piste S (Ruling R209), avec l'attente inverse. Tom envoie
/// d'un coup 24 `PUT /mmd`, ou 24 restaurations, puis sa session prend fin : déconnexion, ou
/// changement du mot de passe par la titulaire du compte. Chaque requête prenait le verrou à son
/// tour, relisait des droits que le compte a toujours, et écrivait : 23 écritures sur 24 après la
/// déconnexion. La session est relue sous le verrou : au plus une écriture sans elle, les autres
/// répondent `401`.
#[tokio::test]
async fn writes_queued_before_the_session_ends_stop_with_it() {
    for restore in [false, true] {
        for ending in [Ending::LogOut, Ending::PasswordChanged] {
            let what = format!(
                "{} ; {ending:?}",
                if restore { "restaurations" } else { "PUT /mmd" }
            );
            let t = open_app().await;
            let team = Team::new(&t).await;
            let grants = [user(&team.olga, Level::Full), user(&team.tom, Level::Edit)];
            let page = team.page(None, &what, &grants).await;
            let cookie = team.tom.browser.session().expect("une session");
            let session = mementum_server::auth::token_hash(&cookie).unwrap();
            session_witness(&t, page, team.tom.id, session).await;
            let requests: Vec<Request<Body>> = if restore {
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
                (0..BURST)
                    .map(|i| {
                        let uri =
                            format!("/api/v1/pages/{page}/versions/{}/restore", versions[i % 2]);
                        request(&team.tom, Request::post(uri), Body::empty())
                    })
                    .collect()
            } else {
                let contents = [paragraphs("Alpha"), paragraphs("Bêta")];
                (0..BURST)
                    .map(|i| {
                        request(
                            &team.tom,
                            Request::put(format!("/api/v1/pages/{page}/mmd"))
                                .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
                            Body::from(contents[i % 2].clone()),
                        )
                    })
                    .collect()
            };
            let statuses = burst_then(&t, requests, async {
                match ending {
                    Ending::LogOut => {
                        team.tom
                            .browser
                            .log_out()
                            .await
                            .assert_status(StatusCode::NO_CONTENT);
                    }
                    Ending::PasswordChanged => {
                        let owner = t.browser();
                        owner
                            .log_in("tom@exemple.fr", common::PASSWORD)
                            .await
                            .assert_status(StatusCode::OK);
                        owner
                            .change_password(common::PASSWORD, "un-autre-mot-de-passe")
                            .await
                            .assert_status(StatusCode::NO_CONTENT);
                    }
                }
            })
            .await;
            let written = statuses
                .iter()
                .filter(|s| **s == StatusCode::NO_CONTENT)
                .count();
            let refused = statuses
                .iter()
                .filter(|s| **s == StatusCode::UNAUTHORIZED)
                .count();
            let (with, without) = (witnessed(&t, true).await, witnessed(&t, false).await);
            assert!(
                without <= 1 && written + refused == statuses.len() && refused > 0,
                "{what} : {written} réponses 204, {refused} 401, sur {} ; écritures au journal \
                 avec la session : {with}, sans elle : {without} ; statuts : {statuses:?}",
                statuses.len()
            );
        }
    }
}

/// Constat I3 de la relecture de la piste S (Ruling R259) : l'écriture complète d'une page est
/// quadratique en nombre de blocs frères (30 000 paragraphes d'un caractère, 90 Ko : 50 s
/// d'acteur en release). Un `PUT /mmd` de plus de `MAX_MMD_BLOCKS` blocs (tous niveaux, lignes de
/// tableau comprises) est refusé avant le verrou et avant que la page soit chargée : `422
/// too_many_blocks`, avec un message qui dit la borne ; rien n'est écrit.
#[tokio::test]
async fn too_many_blocks_are_refused_before_anything_is_written() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Trop", &[user(&team.olga, Level::Full)])
        .await;
    assert_eq!(mementum_server::MAX_MMD_BLOCKS, 12_000);
    let lines: Vec<&str> = vec!["a"; mementum_server::MAX_MMD_BLOCKS + 1];
    let reply = team
        .olga
        .put_mmd(&page.to_string(), lines.join("\n\n") + "\n")
        .await;
    let body = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "too_many_blocks");
    common::assert_french(&body, &["12 001", "12 000"]);
    let sql = format!("SELECT COUNT(*) FROM doc_updates WHERE node_id = '{page}'");
    assert_eq!(query_i64(&t.db_path(), &sql).await, 0, "rien d'écrit");
    let sql = format!("SELECT COUNT(*) FROM doc_snapshots WHERE node_id = '{page}'");
    assert_eq!(
        query_i64(&t.db_path(), &sql).await,
        0,
        "la page n'est pas chargée"
    );
}

/// Un tableau MMD de `rows` lignes sur `cols` colonnes, sans en-tête.
fn table(rows: usize, cols: usize) -> String {
    let row = format!(
        "|{}
",
        " a |".repeat(cols)
    );
    let mut mmd = row.clone();
    mmd.push_str(&format!(
        "|{}
",
        " --- |".repeat(cols)
    ));
    for _ in 1..rows {
        mmd.push_str(&row);
    }
    mmd
}

/// Constat I3 de la re-relecture de la piste S (Ruling R368) : sous le plafond de blocs, un
/// paragraphe de 20 000 plages en gras (120 Ko), un de 40 000 sauts de ligne (240 Ko) et un
/// tableau de 100 lignes sur 4 000 colonnes (1,6 Mo) occupaient l'acteur 91 s, 112 à 145 s et
/// 20 à 25 s : chaque liste que l'écriture bâtit par insertions successives coûte le carré de sa
/// longueur. Un `PUT /mmd` dont le coût estimé dépasse le budget est refusé avant le verrou et
/// avant que la page soit chargée : `422 too_long_to_write`, avec un message qui dit ce qui est
/// trop long et quoi faire ; rien n'est écrit.
#[tokio::test]
async fn mmd_too_long_to_write_is_refused_before_anything_is_written() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Trop long", &[user(&team.olga, Level::Full)])
        .await;
    let entries = [
        (
            "**a**b".repeat(20_000)
                + "
",
            vec![
                "un même bloc",
                "40 000 morceaux",
                "découpe-le en plusieurs blocs",
            ],
        ),
        (
            "a<br/>".repeat(40_000)
                + "
",
            vec!["un même bloc", "80 000 morceaux", "sauts de ligne"],
        ),
        (
            table(100, 4_000),
            vec![
                "une même ligne de tableau",
                "4 000 cellules",
                "tableaux moins larges",
            ],
        ),
    ];
    for (mmd, words) in entries {
        let reply = team.olga.put_mmd(&page.to_string(), mmd).await;
        let body = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "too_long_to_write");
        let mut words = words;
        words.extend(["trop long à écrire", "8 s au plus", "rien n'a été écrit"]);
        common::assert_french(&body, &words);
    }
    let sql = format!("SELECT COUNT(*) FROM doc_updates WHERE node_id = '{page}'");
    assert_eq!(query_i64(&t.db_path(), &sql).await, 0, "rien d'écrit");
    let sql = format!("SELECT COUNT(*) FROM doc_snapshots WHERE node_id = '{page}'");
    assert_eq!(
        query_i64(&t.db_path(), &sql).await,
        0,
        "la page n'est pas chargée"
    );
}

/// Une forme de MMD, de taille `n`.
type Shape = dyn Fn(usize) -> String;

/// Le plus grand `n` pour lequel `shape(n)` passe les deux bornes de `PUT /mmd` (blocs et coût
/// estimé), entre 1 et `cap`.
fn largest_accepted(shape: &Shape, cap: usize) -> usize {
    let accepted = |n: usize| {
        let blocks = mementum_md::parse(&shape(n)).unwrap();
        let count = {
            let mut count = 0;
            let mut stack: Vec<&mementum_md::Block> = blocks.iter().collect();
            while let Some(block) = stack.pop() {
                count += 1;
                if let mementum_md::Content::Table(table) = &block.content {
                    count += table.rows.len();
                }
                stack.extend(block.children.iter());
            }
            count
        };
        count <= mementum_server::MAX_MMD_BLOCKS
            && u128::from(mementum_crdt::write_cost(&blocks).nanos)
                <= mementum_server::MMD_WRITE_BUDGET.as_nanos()
    };
    let (mut low, mut high) = (1, cap);
    if accepted(high) {
        return high;
    }
    assert!(accepted(low));
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if accepted(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    low
}

/// Mesure, à lancer en release (Ruling R368) : la pire écriture que les bornes de `PUT /mmd`
/// acceptent occupe l'acteur moins de dix secondes. Pour chaque forme, le plus grand MMD
/// accepté, cherché sur l'estimation elle-même, est écrit deux fois de suite sur une page neuve
/// d'un serveur neuf (la seconde récrit la première, et retire autant qu'elle écrit) ; chaque
/// requête est chronométrée, acteur compris. Des poids trop bas laisseraient passer de plus grands MMD, plus
/// lents : la mesure ne peut pas passer à tort.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure, à lancer en --release"]
async fn measure_worst_accepted_writes() {
    // Un `assert!` sur `cfg!` est refusé par clippy (`assertions_on_constants`).
    if cfg!(debug_assertions) {
        panic!("mesure sans valeur en debug : lancer `cargo test --release`");
    }
    let rich = "**du texte** ordinaire et **du texte** ordinaire et **du texte** fin";
    let shapes: Vec<(&str, Box<Shape>)> = vec![
        (
            "plages grasses et maigres d'un paragraphe",
            Box::new(|n| {
                "**a**b".repeat(n)
                    + "
"
            }),
        ),
        (
            "lettres et sauts de ligne d'un paragraphe",
            Box::new(|n| {
                "a<br/>".repeat(n)
                    + "
"
            }),
        ),
        ("cellules d'une ligne", Box::new(|n| table(1, n))),
        ("lignes de 100 cellules", Box::new(|n| table(n, 100))),
        (
            "paragraphes de six plages",
            Box::new(move |n| {
                vec![rich; n].join(
                    "

",
                ) + "
"
            }),
        ),
        (
            "un paragraphe de plages et une ligne de cellules",
            Box::new(|n| {
                "**a**b".repeat(n)
                    + "

" + &table(1, 2 * n)
            }),
        ),
    ];
    // `MEMENTUM_I3_SHAPE` : ne mesurer que les formes dont le nom contient ce texte.
    let only = std::env::var("MEMENTUM_I3_SHAPE").unwrap_or_default();
    let mut worst = Duration::ZERO;
    for (what, shape) in shapes.into_iter().filter(|(what, _)| what.contains(&only)) {
        let n = largest_accepted(shape.as_ref(), 200_000);
        let mmd = shape(n);
        // Un serveur neuf par forme : les acteurs des pages d'avant (leur compactage, leur
        // mémoire) ne pèsent pas sur la mesure.
        let t = open_app().await;
        let team = Team::new(&t).await;
        let page = team
            .page(None, what, &[user(&team.olga, Level::Full)])
            .await;
        let mut times = Vec::new();
        for _ in 0..2 {
            let start = std::time::Instant::now();
            let reply = team.olga.put_mmd(&page.to_string(), mmd.clone()).await;
            assert_eq!(reply.status, StatusCode::NO_CONTENT, "{what}, n = {n}");
            times.push(start.elapsed());
        }
        println!(
            "MESURE pire écriture acceptée, {what} (n = {n}, {} octets) : {times:?}",
            mmd.len()
        );
        worst = worst.max(*times.iter().max().unwrap());
    }
    println!("MESURE pire écriture acceptée, toutes formes : {worst:?}");
    assert!(worst < Duration::from_secs(10), "{worst:?}");
}

/// Constat N2 de la re-relecture de la piste S (Ruling R365), avec l'attente inverse. Pendant
/// qu'Olga récrit une longue page, Tom demande d'un coup 20 fils sur un de ses blocs : chaque
/// requête attend l'acteur de la page, occupé ; Tom se déconnecte pendant ce temps. Avant, la
/// route relisait ses droits une fois la page chargée, jamais la session : 20 fils écrits sans
/// elle. Elle relit maintenant la session aussi (comme R209) : aucun fil écrit sans session, et
/// chaque requête répond `401`. Le minutage peut faire échouer ce test à tort (une déconnexion
/// qui arriverait après les écritures), jamais le faire passer à tort : le témoin compte, dans
/// la transaction de chaque fil écrit, si la session était encore en base.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn threads_queued_behind_the_actor_stop_with_the_session() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (olga, tom) = (&team.olga, &team.tom);
    let doc = PageDoc::new_page().0;
    doc.write_blocks(&mementum_md::parse("Bloc ^bloc-fixe\n").unwrap())
        .unwrap();
    let page = team
        .store
        .create_node(mementum_store::NewNode {
            space_id: team.space,
            parent_id: None,
            after: None,
            kind: mementum_store::NodeKind::Page,
            title: "Longue".to_string(),
            created_by: Some(olga.id),
            grants: [user(olga, Level::Full), user(tom, Level::Comment)]
                .iter()
                .map(|&(subject, level)| mementum_acl::Grant { subject, level })
                .collect(),
            doc: Some(doc.encode_state()),
        })
        .await
        .unwrap()
        .id;
    let cookie = tom.browser.session().expect("une session");
    let session = mementum_server::auth::token_hash(&cookie).unwrap();
    let hex: String = session.iter().map(|b| format!("{b:02x}")).collect();
    exec_sql(
        &t.db_path(),
        &format!(
            "CREATE TABLE witness (granted INTEGER NOT NULL); \
             CREATE TRIGGER witness_threads AFTER INSERT ON comment_threads \
             WHEN NEW.node_id = '{page}' AND NEW.created_by = '{}' \
             BEGIN INSERT INTO witness VALUES \
               ((SELECT COUNT(*) FROM sessions WHERE token_hash = x'{hex}')); \
             END;",
            tom.id
        ),
    )
    .await;

    // La longue écriture d'Olga occupe l'acteur ; elle garde le bloc (son ancre).
    let long = format!("Bloc ^bloc-fixe\n\n{}", many_paragraphs("Longue", 11_000));
    let write = request(
        olga,
        Request::put(format!("/api/v1/pages/{page}/mmd"))
            .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
        Body::from(long),
    );
    let router = t.router.clone();
    let olga_write = tokio::spawn(async move { router.oneshot(write).await.unwrap().status() });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut burst = JoinSet::new();
    for i in 0..20 {
        let body = serde_json::json!({ "blockId": "bloc-fixe", "body": format!("Fil {i}") });
        let req = request(
            tom,
            Request::post(format!("/api/v1/pages/{page}/comments"))
                .header(header::CONTENT_TYPE, "application/json"),
            Body::from(body.to_string()),
        );
        let router = t.router.clone();
        burst.spawn(async move { router.oneshot(req).await.unwrap().status() });
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !olga_write.is_finished(),
        "l'écriture d'Olga était trop courte"
    );
    tom.browser
        .log_out()
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let mut statuses = Vec::new();
    while let Some(status) = burst.join_next().await {
        statuses.push(status.unwrap());
    }
    assert_eq!(olga_write.await.unwrap(), StatusCode::NO_CONTENT);
    let without = query_i64(
        &t.db_path(),
        "SELECT COUNT(*) FROM witness WHERE granted = 0",
    )
    .await;
    let with = query_i64(
        &t.db_path(),
        "SELECT COUNT(*) FROM witness WHERE granted = 1",
    )
    .await;
    assert!(
        without == 0 && statuses.iter().all(|s| *s == StatusCode::UNAUTHORIZED),
        "fils écrits sans la session : {without}, avec : {with} ; statuts : {statuses:?}"
    );
}
