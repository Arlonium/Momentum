//! Budgets du lot 5 (Task X2) : la frappe d'une personne pendant qu'un agent écrit, par ses
//! routes et au débit que lui laisse la porte des jetons (Ruling R1241 : 120 appels par minute
//! et par jeton, 240 par compte, sur une minute glissante, donc tout d'un coup si l'agent le
//! veut). Les agents passent par l'application en mémoire, avec un vrai jeton et les plafonds par
//! défaut ; la personne tape par une vraie connexion de synchro, et chaque frappe est
//! chronométrée jusqu'à ce qu'un second onglet l'ait reçue (son vecteur d'état atteint celui de
//! l'auteur : un contrôle qui ne relit pas la page).
//!
//! Les mesures dont le nom commence par `keystroke_seen_under_150ms` portent le budget de la
//! frappe (§5.10, < 150 ms au p95, la première frappe d'après la connexion jugée à part, Ruling
//! R488) et tournent dans le job `budgets` de la CI ; les autres (`measure_…`) n'affirment rien et
//! se lancent à la main. Toutes en release, une à la fois ([`ALONE`]) :
//!
//! `cargo test --release -p mementum-server --test agent_budgets -- --ignored --nocapture --test-threads=1`

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::*;
use mementum_server::SignupMode;
use mementum_store::{TokenLevel, TokenScope};
use mementum_sync::testkit::Client;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;
use yrs::ClientID;

/// Une mesure à la fois dans ce binaire (Ruling R341).
static ALONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// « Frappe → visible chez un autre < 150 ms » (§5.10), au p95 au plus proche rang.
const KEYSTROKE_BUDGET: Duration = Duration::from_millis(150);

/// Blocs de la grande page.
const BLOCKS: usize = 10_000;

/// La pause entre deux frappes.
const PACE: Duration = Duration::from_millis(5);

/// Appels d'un jeton par minute (R1241, la valeur par défaut que la porte applique ici).
const TOKEN_MINUTE: usize = 120;

/// Appels de tous les jetons d'un compte par minute (R1241).
const ACCOUNT_MINUTE: usize = 240;

/// Une mesure faite en debug ne dit rien.
fn release_only() {
    if cfg!(debug_assertions) {
        panic!(
            "mesure sans valeur en debug : `cargo test --release -p mementum-server --test \
             agent_budgets -- --ignored`"
        );
    }
}

async fn alone() -> tokio::sync::MutexGuard<'static, ()> {
    release_only();
    ALONE.lock().await
}

/// Le rang `p` (de 0 à 100) d'une série, au plus proche rang.
fn rank(values: &mut [Duration], p: usize) -> Duration {
    values.sort();
    let at = (values.len() * p).div_ceil(100).max(1) - 1;
    values[at.min(values.len() - 1)]
}

/// Ce qu'une série dit : nombre, p50, p95, la pire, et combien dépassent le budget de la frappe.
fn said(times: &mut [Duration]) -> String {
    let over = times.iter().filter(|t| **t > KEYSTROKE_BUDGET).count();
    format!(
        "{} mesures, p50 {:?}, p95 {:?}, la pire {:?}, {over} au-dessus de {KEYSTROKE_BUDGET:?}",
        times.len(),
        rank(times, 50),
        rank(times, 95),
        rank(times, 100),
    )
}

/// `count` paragraphes, chacun portant son rang : « Paragraphe 17 de la page de mesure… ».
fn paragraphs(count: usize) -> String {
    (1..=count)
        .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}

/// L'application des mesures : inscriptions ouvertes, plafonds du débit par défaut.
async fn app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

/// Un agent : un jeton d'API et le routeur de l'application, qu'une tâche à elle peut mener.
#[derive(Clone)]
struct Agent {
    router: axum::Router,
    token: String,
}

impl Agent {
    /// Un jeton « Peut modifier » de `user`, sur tous ses espaces.
    async fn of(t: &TestApp, user: Uuid) -> Agent {
        let (token, _) = api_token(&t.store, user, TokenLevel::Edit, TokenScope::All, false).await;
        Agent {
            router: t.router.clone(),
            token,
        }
    }

    /// Un appel : le statut et le corps lu (`null` s'il n'est pas du JSON).
    async fn send(&self, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
        let request = match body {
            Some(body) => json_request(method, uri, body),
            None => Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        };
        let mut request = with_bearer(request, &self.token);
        request
            .headers_mut()
            .insert(header::HOST, "localhost".parse().unwrap());
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    /// Des opérations sur la page, qui doivent passer.
    async fn operate(&self, page: Uuid, body: Value) -> Value {
        let (status, reply) = self
            .send(
                Method::POST,
                &format!("/api/v1/pages/{page}/operations"),
                Some(&body),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        reply
    }

    /// La révision d'une lecture de la page (sa première tranche).
    async fn revision(&self, page: Uuid) -> String {
        let (status, reply) = self
            .send(Method::GET, &format!("/api/v1/pages/{page}/document"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        reply["revision"].as_str().unwrap().to_string()
    }

    /// Un lot de `pages` pages de `blocks` paragraphes, à la racine de l'espace.
    async fn create(&self, space: Uuid, round: usize, pages: usize, blocks: usize) {
        let markdown = paragraphs(blocks);
        let pages: Vec<Value> = (0..pages)
            .map(|i| json!({ "title": format!("Lot {round} page {i}"), "markdown": markdown }))
            .collect();
        let (status, reply) = self
            .send(
                Method::POST,
                &format!("/api/v1/spaces/{space}/pages/batch"),
                Some(&json!({ "pages": pages })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{reply}");
    }
}

/// Un `str_replace` du paragraphe de rang `n`, qui n'apparaît qu'une fois dans la page.
fn str_replace(n: usize) -> Value {
    json!({ "operations": [{
        "op": "str_replace",
        "old": format!("Paragraphe {n} de"),
        "new": format!("Paragraphe {n} revu de"),
    }]})
}

/// Le remplacement du bloc `anchor` par un paragraphe, sur la révision `base` (une opération
/// destructrice : elle coupe une version `before_agent` quand la tranche porte une écriture d'un
/// autre, R1234).
fn replace_block(base: &str, anchor: &str, round: usize) -> Value {
    json!({ "baseRevision": base, "operations": [{
        "op": "replace_block",
        "anchor": anchor,
        "markdown": format!("Remplacé par l'agent, tour {round}.\n"),
    }]})
}

/// Une page de `blocks` paragraphes, écrite par la session de `by`.
async fn page_of(by: &Account<'_>, title: &str, blocks: usize) -> Uuid {
    let page = by.create(json!({ "title": title })).await.id;
    by.put_mmd(&page, paragraphs(blocks))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    Uuid::parse_str(&page).unwrap()
}

/// Deux onglets de la session de `by` sur la page : celui qui tape, celui qui lit.
async fn two_tabs(t: &TestApp, by: &Account<'_>, page: Uuid) -> (Client, Client) {
    let addr = t.listen().await;
    let url = sync_url(addr, page);
    (
        synced_as(&by.browser, &url).await,
        synced_as(&by.browser, &url).await,
    )
}

/// Une frappe de `writer` dans `block`, chronométrée jusqu'à ce que `reader` l'ait reçue.
async fn keystroke(writer: &Client, reader: &Client, block: &str) -> Duration {
    let client = ClientID::new(writer.client_id());
    let start = Instant::now();
    writer.type_text(block, "x");
    let target = writer.state_vector().get(&client);
    assert!(
        reader
            .wait_until(WAIT, |c| c.state_vector().get(&client) >= target)
            .await,
        "la frappe n'est pas arrivée chez l'autre onglet"
    );
    start.elapsed()
}

/// Des frappes, une toutes les [`PACE`], jusqu'à ce que `done` le dise (au moins deux : la
/// première d'une série qui suit la connexion est jugée à part, [`judged`]).
async fn keystrokes_until(
    writer: &Client,
    reader: &Client,
    block: &str,
    done: impl Fn() -> bool,
) -> Vec<Duration> {
    let mut times = Vec::new();
    while times.len() < 2 || !done() {
        times.push(keystroke(writer, reader, block).await);
        tokio::time::sleep(PACE).await;
    }
    times
}

/// Ce que dit une série de frappes, et le dépassement du budget au p95. La première frappe d'une
/// série qui suit la connexion (`after_connection`) est jugée à part (R488) ; dans une série qui
/// en suit une autre, elle y entre comme les autres.
fn judged(
    what: &str,
    mut times: Vec<Duration>,
    after_connection: bool,
    over: &mut Vec<String>,
) -> String {
    if !after_connection {
        let p95 = rank(&mut times, 95);
        if p95 >= KEYSTROKE_BUDGET {
            over.push(format!(
                "{what} : p95 {p95:?} (budget {KEYSTROKE_BUDGET:?}), {}",
                said(&mut times)
            ));
        }
        return said(&mut times);
    }
    let first = times.remove(0);
    let p95 = rank(&mut times, 95);
    if p95 >= KEYSTROKE_BUDGET {
        over.push(format!(
            "{what} : p95 {p95:?} (budget {KEYSTROKE_BUDGET:?}), {}",
            said(&mut times)
        ));
    }
    if first >= KEYSTROKE_BUDGET {
        over.push(format!(
            "{what}, première frappe après la connexion : {first:?} (budget {KEYSTROKE_BUDGET:?})"
        ));
    }
    format!("la première {first:?}, puis {}", said(&mut times))
}

/// Les identifiants des blocs de la page, lus par un onglet.
fn ids(client: &Client) -> Vec<String> {
    client.blocks().into_iter().map(|b| b.id).collect()
}

/// Pose un déclencheur témoin qui note chaque version `before_agent` coupée, dans la transaction
/// qui l'écrit : la purge (vingt gardées au plus, R1234) ne la retire pas du compte.
async fn watch_before_agent_cuts(t: &TestApp) {
    exec_sql(
        &t.db_path(),
        "CREATE TABLE before_agent_cuts (n INTEGER PRIMARY KEY AUTOINCREMENT, node TEXT NOT NULL); \
         CREATE TRIGGER before_agent_cut AFTER INSERT ON doc_versions \
         WHEN NEW.reason = 'before_agent' \
         BEGIN INSERT INTO before_agent_cuts (node) VALUES (NEW.node_id); END;",
    )
    .await;
}

/// Les versions `before_agent` coupées dans la page depuis [`watch_before_agent_cuts`].
async fn before_agent_cuts(t: &TestApp, page: Uuid) -> i64 {
    query_i64(
        &t.db_path(),
        &format!("SELECT COUNT(*) FROM before_agent_cuts WHERE node = '{page}'"),
    )
    .await
}

/// La frappe d'une personne dans une page de 10 000 blocs **pendant qu'un agent y écrit au débit
/// de son jeton**, en deux temps, chacun avec un jeton neuf qui dépense toute sa minute
/// (R1241 : 120 appels, la fenêtre glissante les laisse partir d'un coup) :
/// 1. 120 `str_replace` de suite, chacun dans un paragraphe du milieu (une commande de l'acteur
///    chacun, 73 à 79 ms mesurés par S4 sur 10 000 blocs) ;
/// 2. 60 tours d'une lecture (la révision) puis d'un `replace_block` sur elle : la personne tape
///    sans cesse, si bien que chaque remplacement coupe une version `before_agent` (R1234), dont
///    l'état est encodé dans l'acteur puis relu et écrit hors de lui (S4, R1257).
///
/// Le verrou de la page (`page_writes`) sérialise les écritures d'agent d'une page, version
/// comprise : une frappe n'attend qu'une commande d'agent au plus.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release (job `budgets` de la CI)"]
async fn keystroke_seen_under_150ms_while_an_agent_edits_the_page_in_a_loop() {
    let _alone = alone().await;
    let t = app().await;
    let olga = t.account("Olga").await;
    let page = page_of(&olga, "Grande", BLOCKS).await;
    let (writer, reader) = two_tabs(&t, &olga, page).await;
    let blocks = ids(&writer);
    let typed = blocks[0].clone();
    let mut over = Vec::new();
    let mut lines = Vec::new();

    // 1. Des `str_replace` de suite.
    let agent = Agent::of(&t, olga.id).await;
    let done = Arc::new(AtomicBool::new(false));
    let calls = tokio::spawn({
        let (agent, done) = (agent.clone(), done.clone());
        async move {
            let mut times = Vec::with_capacity(TOKEN_MINUTE);
            for round in 0..TOKEN_MINUTE {
                let start = Instant::now();
                agent.operate(page, str_replace(BLOCKS / 2 + round)).await;
                times.push(start.elapsed());
            }
            done.store(true, Ordering::Relaxed);
            times
        }
    });
    let started = Instant::now();
    let seen = keystrokes_until(&writer, &reader, &typed, || done.load(Ordering::Relaxed)).await;
    let mut calls = calls.await.unwrap();
    lines.push(format!(
        "{TOKEN_MINUTE} str_replace d'un agent en {:?} (appel : {}) ; frappe : {}",
        started.elapsed(),
        said(&mut calls),
        judged("pendant les str_replace", seen, true, &mut over),
    ));

    // 2. Une lecture puis un `replace_block`, soixante fois.
    watch_before_agent_cuts(&t).await;
    let agent = Agent::of(&t, olga.id).await;
    let done = Arc::new(AtomicBool::new(false));
    let rounds = TOKEN_MINUTE / 2;
    let targets: Vec<String> = blocks[BLOCKS / 4..BLOCKS / 4 + rounds].to_vec();
    let calls = tokio::spawn({
        let (agent, done) = (agent.clone(), done.clone());
        async move {
            let mut times = Vec::with_capacity(rounds);
            for (round, target) in targets.iter().enumerate() {
                let base = agent.revision(page).await;
                let start = Instant::now();
                agent
                    .operate(page, replace_block(&base, target, round))
                    .await;
                times.push(start.elapsed());
            }
            done.store(true, Ordering::Relaxed);
            times
        }
    });
    let started = Instant::now();
    let seen = keystrokes_until(&writer, &reader, &typed, || done.load(Ordering::Relaxed)).await;
    let mut calls = calls.await.unwrap();
    let cuts = before_agent_cuts(&t, page).await;
    lines.push(format!(
        "{rounds} lectures et replace_block d'un agent en {:?} (replace_block : {} ; {cuts} \
         versions before_agent coupées) ; frappe : {}",
        started.elapsed(),
        said(&mut calls),
        judged("pendant les replace_block", seen, false, &mut over),
    ));
    assert_eq!(
        cuts, rounds as i64,
        "chaque remplacement devait couper une version before_agent"
    );
    println!(
        "MESURE frappe d'une personne dans une page de {BLOCKS} blocs pendant qu'un agent y écrit \
         au débit de son jeton :\n  {}",
        lines.join("\n  ")
    );
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// La frappe d'une personne pendant qu'un agent crée des pages en boucle, au débit de son jeton :
/// 120 lots de 20 pages de 50 paragraphes (2 400 pages), chacun écrit en une transaction dans la
/// voie des copies (R1243), puis relu par l'indexeur. La personne tape dans sa page, dans le même
/// espace.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release (job `budgets` de la CI)"]
async fn keystroke_seen_under_150ms_while_an_agent_creates_pages_in_a_loop() {
    let _alone = alone().await;
    const PAGES: usize = 20;
    const PARAGRAPHS: usize = 50;
    let t = app().await;
    let olga = t.account("Olga").await;
    let page = page_of(&olga, "Tapée", 1).await;
    let (writer, reader) = two_tabs(&t, &olga, page).await;
    let typed = ids(&writer)[0].clone();
    let agent = Agent::of(&t, olga.id).await;
    let done = Arc::new(AtomicBool::new(false));
    let space = olga.space;
    let calls = tokio::spawn({
        let (agent, done) = (agent.clone(), done.clone());
        async move {
            let mut times = Vec::with_capacity(TOKEN_MINUTE);
            for round in 0..TOKEN_MINUTE {
                let start = Instant::now();
                agent.create(space, round, PAGES, PARAGRAPHS).await;
                times.push(start.elapsed());
            }
            done.store(true, Ordering::Relaxed);
            times
        }
    });
    let started = Instant::now();
    let seen = keystrokes_until(&writer, &reader, &typed, || done.load(Ordering::Relaxed)).await;
    let created = started.elapsed();
    let mut calls = calls.await.unwrap();
    // Puis tant que l'indexeur relit les pages créées (la page tapée, elle, n'a jamais son
    // calme : `settled()` ne rendrait pas).
    let mut indexing = Vec::new();
    while indexing.len() < 2 || t.indexer.behind(space, |p| p != page).await.unwrap() {
        indexing.push(keystroke(&writer, &reader, &typed).await);
        tokio::time::sleep(PACE).await;
    }
    let mut over = Vec::new();
    println!(
        "MESURE frappe d'une personne pendant qu'un agent crée {} pages de {PARAGRAPHS} \
         paragraphes en {TOKEN_MINUTE} lots, en {created:?} (lot : {}) ; frappe : {} ; puis \
         pendant que l'indexeur les relit ({:?} de plus) : {}",
        PAGES * TOKEN_MINUTE,
        said(&mut calls),
        judged("pendant les créations", seen, true, &mut over),
        started.elapsed() - created,
        judged(
            "pendant l'indexation des pages créées",
            indexing,
            false,
            &mut over
        ),
    );
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// La frappe d'une personne pendant que **trois jetons de son compte** écrivent au débit du
/// compte (R1241 : 240 appels par minute, tous jetons ensemble), de front : le premier fait 80
/// `str_replace` dans la grande page où elle tape, le deuxième 40 lectures et `replace_block`
/// (avec leur version `before_agent`) dans la même page, le troisième 80 lots de 20 pages. Les
/// deux premiers se partagent les deux places du compte dans la file des écritures de la page
/// (R1242), et son verrou.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release (job `budgets` de la CI)"]
async fn keystroke_seen_under_150ms_while_three_tokens_of_an_account_write() {
    let _alone = alone().await;
    let share = ACCOUNT_MINUTE / 3;
    let t = app().await;
    let olga = t.account("Olga").await;
    let page = page_of(&olga, "Grande", BLOCKS).await;
    let (writer, reader) = two_tabs(&t, &olga, page).await;
    let blocks = ids(&writer);
    let typed = blocks[0].clone();
    watch_before_agent_cuts(&t).await;
    let (first, second, third) = (
        Agent::of(&t, olga.id).await,
        Agent::of(&t, olga.id).await,
        Agent::of(&t, olga.id).await,
    );
    let left = Arc::new(AtomicUsize::new(3));
    let space = olga.space;
    let editing = tokio::spawn({
        let left = left.clone();
        async move {
            let mut times = Vec::new();
            for round in 0..share {
                let start = Instant::now();
                first.operate(page, str_replace(BLOCKS / 2 + round)).await;
                times.push(start.elapsed());
            }
            left.fetch_sub(1, Ordering::Relaxed);
            times
        }
    });
    let targets: Vec<String> = blocks[BLOCKS / 4..BLOCKS / 4 + share / 2].to_vec();
    let replacing = tokio::spawn({
        let left = left.clone();
        async move {
            let mut times = Vec::new();
            for (round, target) in targets.iter().enumerate() {
                let base = second.revision(page).await;
                let start = Instant::now();
                second
                    .operate(page, replace_block(&base, target, round))
                    .await;
                times.push(start.elapsed());
            }
            left.fetch_sub(1, Ordering::Relaxed);
            times
        }
    });
    let creating = tokio::spawn({
        let left = left.clone();
        async move {
            let mut times = Vec::new();
            for round in 0..share {
                let start = Instant::now();
                third.create(space, round, 20, 50).await;
                times.push(start.elapsed());
            }
            left.fetch_sub(1, Ordering::Relaxed);
            times
        }
    });
    let started = Instant::now();
    let seen = keystrokes_until(&writer, &reader, &typed, || {
        left.load(Ordering::Relaxed) == 0
    })
    .await;
    let elapsed = started.elapsed();
    let (mut edits, mut replaces, mut creations) = (
        editing.await.unwrap(),
        replacing.await.unwrap(),
        creating.await.unwrap(),
    );
    let mut over = Vec::new();
    println!(
        "MESURE frappe d'une personne pendant que trois jetons de son compte écrivent au débit du \
         compte ({ACCOUNT_MINUTE} appels en {elapsed:?}) : str_replace {} ; replace_block {} ; \
         lots de 20 pages {} ; frappe : {}",
        said(&mut edits),
        said(&mut replaces),
        said(&mut creations),
        judged("pendant trois jetons du compte", seen, true, &mut over),
    );
    // La personne tape sans cesse : chaque remplacement du deuxième jeton coupe sa version.
    assert_eq!(
        before_agent_cuts(&t, page).await,
        (share / 2) as i64,
        "chaque remplacement devait couper une version before_agent"
    );
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// Ruling R1359 : les écritures d'un jeton attendent dans une voie de la file d'écriture du store.
/// Un jeton dépense sa minute d'un coup : 120 renommages menés de front (`PATCH` du titre, une
/// écriture du store chacun, hors de tout acteur) ; pendant ce temps, la session du même compte
/// renomme sa propre page, une requête après l'autre, et chacune est chronométrée. Puis la même
/// session sans rafale, pour comparer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release"]
async fn measure_session_writes_while_a_token_writes_in_a_burst() {
    let _alone = alone().await;
    let t = app().await;
    let olga = t.account("Olga").await;
    let own = olga.create(json!({ "title": "À moi" })).await.id;
    let mut renamed = Vec::new();
    for i in 0..12 {
        renamed.push(
            olga.create(json!({ "title": format!("Cible {i}") }))
                .await
                .id,
        );
    }
    let rename = async |round: usize| {
        let start = Instant::now();
        olga.patch(&own, json!({ "title": format!("À moi {round}") }))
            .await
            .assert_status(StatusCode::OK);
        start.elapsed()
    };
    let mut quiet = Vec::new();
    for round in 0..50 {
        quiet.push(rename(round).await);
    }
    let agent = Agent::of(&t, olga.id).await;
    let left = Arc::new(AtomicUsize::new(TOKEN_MINUTE));
    let started = Instant::now();
    let mut burst = Vec::new();
    for call in 0..TOKEN_MINUTE {
        let (agent, left, page) = (agent.clone(), left.clone(), renamed[call % 12].clone());
        burst.push(tokio::spawn(async move {
            let start = Instant::now();
            let (status, reply) = agent
                .send(
                    Method::PATCH,
                    &format!("/api/v1/pages/{page}"),
                    Some(&json!({ "title": format!("Renommée {call}") })),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{reply}");
            left.fetch_sub(1, Ordering::Relaxed);
            start.elapsed()
        }));
    }
    let mut during = Vec::new();
    let mut round = 50;
    while left.load(Ordering::Relaxed) > 0 || during.is_empty() {
        during.push(rename(round).await);
        round += 1;
    }
    let spent = started.elapsed();
    let mut token = Vec::new();
    for call in burst {
        token.push(call.await.unwrap());
    }
    println!(
        "MESURE renommage par la session : sans rafale {} ; pendant 120 renommages menés de \
         front par un jeton du même compte (en {spent:?} ; renommage du jeton : {}) : {}",
        said(&mut quiet),
        said(&mut token),
        said(&mut during),
    );
}

/// Sans en faire un budget (« Budgets de performance » du plan du lot 5) : la frappe qui tombe
/// pendant une écriture d'agent qui change toute la page (un `str_replace` sur tous les
/// paragraphes, `replace_all`, dix fois), et la durée de ces appels.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release"]
async fn measure_keystroke_during_an_agent_write_that_changes_the_whole_page() {
    let _alone = alone().await;
    let t = app().await;
    let olga = t.account("Olga").await;
    let page = page_of(&olga, "Grande", BLOCKS).await;
    let (writer, reader) = two_tabs(&t, &olga, page).await;
    let typed = ids(&writer)[0].clone();
    let agent = Agent::of(&t, olga.id).await;
    let done = Arc::new(AtomicBool::new(false));
    let calls = tokio::spawn({
        let (agent, done) = (agent.clone(), done.clone());
        async move {
            let mut times = Vec::new();
            for round in 0..10 {
                let (old, new) = if round % 2 == 0 {
                    ("ordinaire", "courant")
                } else {
                    ("courant", "ordinaire")
                };
                let start = Instant::now();
                agent
                    .operate(
                        page,
                        json!({ "operations": [{
                            "op": "str_replace", "old": old, "new": new, "replace_all": true,
                        }]}),
                    )
                    .await;
                times.push(start.elapsed());
            }
            done.store(true, Ordering::Relaxed);
            times
        }
    });
    let mut seen =
        keystrokes_until(&writer, &reader, &typed, || done.load(Ordering::Relaxed)).await;
    let first = seen.remove(0);
    let mut calls = calls.await.unwrap();
    println!(
        "MESURE dix str_replace d'un agent sur les {BLOCKS} paragraphes de la page (appel : {}) ; \
         frappe pendant ce temps : la première {first:?}, puis {}",
        said(&mut calls),
        said(&mut seen),
    );
}

/// Limite RO-C du plan (l'indexeur sans part par compte) : trois agents, chacun d'un compte à
/// lui, écrivent sans relâche dans leur page de 10 000 blocs au débit de leur jeton (un
/// `str_replace` toutes les 500 ms) ; une autre personne tape un mot dans sa petite page, et l'on
/// chronomètre le délai avant que la recherche le trouve, cinq fois. Le budget « trouvable en
/// moins de 5 s » vaut l'indexeur au repos (§5.10) : rien n'est affirmé.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release"]
async fn measure_findable_while_agents_edit_large_pages() {
    let _alone = alone().await;
    const PERIOD: Duration = Duration::from_millis(500);
    let t = app().await;
    let mut agents = Vec::new();
    for name in ["Ada", "Bea", "Cyd"] {
        let account = t.account(name).await;
        let page = page_of(&account, "Grande", BLOCKS).await;
        agents.push((Agent::of(&t, account.id).await, page));
    }
    let rita = t.account("Rita").await;
    let small = page_of(&rita, "Petite", 1).await;
    t.indexer.settled().await;
    let (writer, _reader) = two_tabs(&t, &rita, small).await;
    let block = ids(&writer)[0].clone();
    let stop = Arc::new(AtomicBool::new(false));
    let mut loops = Vec::new();
    for (agent, page) in agents {
        let stop = stop.clone();
        loops.push(tokio::spawn(async move {
            let mut round = 0;
            while !stop.load(Ordering::Relaxed) {
                let next = Instant::now() + PERIOD;
                agent
                    .operate(page, str_replace(1 + round % (BLOCKS - 1)))
                    .await;
                round += 1;
                tokio::time::sleep_until(next.into()).await;
            }
            round
        }));
    }
    // Les agents écrivent depuis une demi-minute : l'indexeur reprend leurs pages.
    tokio::time::sleep(Duration::from_secs(30)).await;
    let mut delays = Vec::new();
    for round in 0..5 {
        let word = format!("trouvable{}", ["aa", "bb", "cc", "dd", "ee"][round]);
        let start = Instant::now();
        writer.type_text(&block, &format!(" {word} "));
        loop {
            let query = mementum_search::read_query(&word).unwrap();
            let allowed: mementum_search::PageSet = [small].into_iter().collect();
            if !t
                .indexer
                .search(&query, &allowed, 0, 20)
                .unwrap()
                .pages
                .is_empty()
            {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(120),
                "le mot n'est jamais trouvé"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        delays.push(start.elapsed());
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    stop.store(true, Ordering::Relaxed);
    let mut written = 0;
    for task in loops {
        written += task.await.unwrap();
    }
    println!(
        "MESURE un mot tapé est trouvable, pendant que trois agents récrivent chacun un bloc de \
         sa page de {BLOCKS} blocs deux fois par seconde ({written} écritures) : {delays:?} \
         ({} relectures par l'indexeur)",
        t.indexer.reads()
    );
}

/// Ce que les relectures de M2 ont laissé à mesurer (registre du lot 5, « Pour X1 et X2 ») : le
/// coût d'un appel refusé par le débit (`429 token_busy`), selon son corps (rien, 64 Kio, qui est
/// lu et jeté, R1434, et 1,9 Mo, qui n'est pas lu), par l'application en mémoire.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release"]
async fn measure_rate_refusal_with_a_body() {
    let _alone = alone().await;
    let t = app().await;
    let olga = t.account("Olga").await;
    let agent = Agent::of(&t, olga.id).await;
    // La minute du jeton dépensée.
    for _ in 0..TOKEN_MINUTE {
        let (status, _) = agent.send(Method::GET, "/api/v1/me", None).await;
        assert_eq!(status, StatusCode::OK);
    }
    let mut lines = Vec::new();
    for (what, bytes) in [
        ("sans corps", 0usize),
        ("corps de 64 Kio", 64 << 10),
        ("corps de 1,9 Mo", 1_900_000),
    ] {
        let body = vec![b' '; bytes];
        let mut times = Vec::new();
        for _ in 0..200 {
            let request = with_bearer(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json, text/event-stream")
                    .header(header::HOST, "localhost")
                    .body(Body::from(body.clone()))
                    .unwrap(),
                &agent.token,
            );
            let start = Instant::now();
            let response = t.router.clone().oneshot(request).await.unwrap();
            let status = response.status();
            let _ = axum::body::to_bytes(response.into_body(), usize::MAX).await;
            times.push(start.elapsed());
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        }
        lines.push(format!("{what} : {}", said(&mut times)));
    }
    println!(
        "MESURE appel refusé par le débit (429 token_busy) : {}",
        lines.join(" ; ")
    );
}

/// Ruling R1600 : `calls_of` trie tout le journal du jeton (au plus mille lignes réussies et deux
/// cents refusées, la purge le borne) ; la route de l'activité, par la session, sur un journal
/// plein, dans une base qui porte les journaux pleins de cinquante autres jetons. Et la durée d'un
/// paquet du journal (R1244 : 256 lignes au plus, une instruction par ligne, une transaction).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release"]
async fn measure_activity_of_a_full_journal() {
    use mementum_store::{Call, CallOutcome, CallSurface};
    let _alone = alone().await;
    let t = app().await;
    let olga = t.account("Olga").await;
    // Vingt jetons vivants au plus par compte (`MAX_LIVE_TOKENS`) : le premier est à Olga, les
    // cinquante autres à cinq autres comptes.
    let (_, first) = api_token(&t.store, olga.id, TokenLevel::Edit, TokenScope::All, false).await;
    let mut tokens = vec![first];
    for name in ["Ada", "Bea", "Cyd", "Dan", "Eve"] {
        let other = t.account(name).await;
        for _ in 0..10 {
            let (_, id) =
                api_token(&t.store, other.id, TokenLevel::Edit, TokenScope::All, false).await;
            tokens.push(id);
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let page = Uuid::now_v7();
    let mut batches = Vec::new();
    for token in &tokens {
        let calls: Vec<Call> = (0..1_200)
            .map(|i| Call {
                token_id: *token,
                at: now - 1_000_000 + i,
                surface: CallSurface::Mcp,
                tool: "update_page".to_string(),
                space_id: Some(olga.space),
                node_id: Some(page),
                // Les deux issues mêlées, comme un agent qui se trompe une fois sur six.
                outcome: if i % 6 == 0 {
                    CallOutcome::Refused
                } else {
                    CallOutcome::Ok
                },
                code: (i % 6 == 0).then(|| "revision_conflict".to_string()),
                revision_before: Some("r_0123456789abcdef".to_string()),
                revision_after: Some("r_fedcba9876543210".to_string()),
                blocks_touched: Some(1),
            })
            .collect();
        for chunk in calls.chunks(mementum_server::agent::calls::BATCH) {
            let start = Instant::now();
            t.store.append_calls(chunk, &[]).await.unwrap();
            if chunk.len() == mementum_server::agent::calls::BATCH {
                batches.push(start.elapsed());
            }
        }
    }
    let rows = query_i64(&t.db_path(), "SELECT COUNT(*) FROM token_calls").await;
    let mut lines = vec![format!(
        "paquet de {} lignes écrit en une transaction, purge comprise : {}",
        mementum_server::agent::calls::BATCH,
        said(&mut batches)
    )];
    let token = tokens[0];
    for (what, query) in [
        ("première page, toutes issues", ""),
        ("première page, refus seuls", "?outcome=refused"),
    ] {
        let mut times = Vec::new();
        for _ in 0..200 {
            let start = Instant::now();
            let reply = olga
                .browser
                .get(&format!("/api/v1/tokens/{token}/calls{query}"))
                .await;
            times.push(start.elapsed());
            reply.assert_status(StatusCode::OK);
        }
        lines.push(format!("{what} : {}", said(&mut times)));
    }
    println!(
        "MESURE activité d'un jeton au journal plein ({rows} lignes dans la base) : {}",
        lines.join(" ; ")
    );
}

/// Ce que M2 a laissé à mesurer : le coût d'`opened_spaces` (une lecture des espaces du compte,
/// puis une restriction par espace) à chaque outil qui nomme un espace, pour un compte d'un
/// espace et pour un compte de cinquante espaces de vingt pages chacun : `workspace_info` sans
/// argument (la liste des espaces), puis avec le nom d'un espace.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release"]
async fn measure_opened_spaces() {
    use common::mcp::{Era, Mcp};
    let _alone = alone().await;
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        // Le débit n'est pas ce qu'on mesure.
        cfg.token_calls_per_min = 100_000;
        cfg.account_token_calls_per_min = 100_000;
    })
    .await;
    let mut lines = Vec::new();
    for (name, spaces) in [("Una", 1usize), ("Cinq", 50)] {
        let account = t.account(name).await;
        let mut named = String::new();
        for s in 1..spaces {
            let reply = account
                .browser
                .post("/api/v1/spaces", json!({ "name": format!("Espace {s}") }))
                .await;
            reply.assert_status(StatusCode::CREATED);
            let space = reply.json()["id"].as_str().unwrap().to_string();
            named = format!("Espace {s}");
            for p in 0..20 {
                account
                    .post(json!({ "spaceId": space, "title": format!("Page {p}") }))
                    .await
                    .assert_status(StatusCode::CREATED);
            }
        }
        if named.is_empty() {
            named = account.space.to_string();
        }
        let (token, _) = api_token(
            &t.store,
            account.id,
            TokenLevel::Edit,
            TokenScope::All,
            false,
        )
        .await;
        let mcp = Mcp::new(&t, &token, Era::V2026);
        mcp.open().await;
        for (what, args) in [
            ("workspace_info sans argument", json!({})),
            (
                "workspace_info qui nomme un espace",
                json!({ "space": named }),
            ),
        ] {
            let mut times = Vec::new();
            for _ in 0..200 {
                let start = Instant::now();
                let result = mcp.call("workspace_info", args.clone()).await;
                times.push(start.elapsed());
                assert!(!result.is_error, "{}", result.text);
            }
            lines.push(format!("{spaces} espaces, {what} : {}", said(&mut times)));
        }
    }
    println!("MESURE opened_spaces : {}", lines.join(" ; "));
}
