//! La route de recherche (lot 4, Task S5) : `POST /api/v1/spaces/{id}/search`.
//!
//! Ce que ces tests tiennent : une page qu'on ne voit pas ne se trouve, ne se devine ni ne se
//! compte (Review Focus 1) ; un droit retiré vaut à la recherche suivante, l'indexeur figé
//! (Review Focus 2) ; les bornes d'une recherche et l'ordre des refus ; le débit par compte
//! (R910) ; le temps de réponse plancher (R932).
//!
//! Aucun test n'attend une durée. L'index à jour s'attend par `t.indexer.settled()` ; un calcul
//! se retient à son début par [`Gate`] ; le plancher se prouve sous l'horloge arrêtée
//! ([`StoppedClock`]), les attentes bornées en temps réel ([`until`]).

mod common;
#[path = "../../mementum-sync/tests/common/limit.rs"]
mod limit;

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use common::clock::StoppedClock;
use common::{Account, Browser, Reply, Team, TestApp, all_logs, json_request, outcome};
use mementum_acl::{Level, Subject};
use mementum_server::{MAX_SEARCHES, SEARCH_FLOOR, SignupMode};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tower::ServiceExt;
use uuid::Uuid;

/// Délai maximal, en temps réel, d'une attente de condition.
const REAL_WAIT: Duration = Duration::from_secs(30);

/// Le calme attendu avant d'indexer une page écrite.
const QUICK: Duration = Duration::from_millis(20);

/// La réponse d'une recherche qui ne trouve rien, index à jour.
fn nothing() -> Value {
    json!({ "results": [], "more": false, "indexing": false })
}

/// Une application aux inscriptions ouvertes, dont l'indexeur n'attend que [`QUICK`] de calme.
async fn app() -> TestApp {
    TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.index_after = QUICK;
    })
    .await
}

/// Attend `ready` en temps réel ([`REAL_WAIT`] au plus), en cédant la main : sous une horloge
/// arrêtée, aucun délai de tokio ne finit. La borne peut faire échouer le test à tort sur un
/// poste très chargé, jamais le faire passer.
async fn until(what: &str, ready: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + REAL_WAIT;
    while !ready() {
        assert!(std::time::Instant::now() < deadline, "jamais vu : {what}");
        tokio::task::yield_now().await;
    }
}

/// Cède la main assez pour que toute tâche prête ait tourné.
async fn settle() {
    for _ in 0..500 {
        tokio::task::yield_now().await;
    }
}

fn members(level: Level) -> (Subject, Level) {
    (Subject::Members, level)
}

fn user(account: &Account<'_>, level: Level) -> (Subject, Level) {
    (Subject::User(account.id), level)
}

/// Récrit la page par `PUT /mmd`.
async fn write(by: &Account<'_>, page: Uuid, mmd: &str) {
    by.put_mmd(&page.to_string(), mmd.to_string())
        .await
        .assert_status(StatusCode::NO_CONTENT);
}

/// Un MMD de `n` paragraphes de ce texte.
fn blocks(text: &str, n: usize) -> String {
    let mut mmd = vec![text; n].join("\n\n");
    mmd.push('\n');
    mmd
}

fn uri(space: Uuid) -> String {
    format!("/api/v1/spaces/{space}/search")
}

/// `POST …/search` avec `{ "q": q }`.
async fn search(by: &Account<'_>, space: Uuid, q: &str) -> Reply {
    by.browser.post(&uri(space), json!({ "q": q })).await
}

/// Ce qu'une recherche qui passe répond.
async fn found(by: &Account<'_>, space: Uuid, q: &str) -> Value {
    let reply = search(by, space, q).await;
    reply.assert_status(StatusCode::OK);
    reply.json()
}

/// Les pages d'une réponse, dans son ordre.
fn pages(results: &Value) -> Vec<Uuid> {
    results["results"]
        .as_array()
        .unwrap_or_else(|| panic!("« results » en liste attendu dans {results}"))
        .iter()
        .map(|hit| hit["pageId"].as_str().unwrap().parse().unwrap())
        .collect()
}

/// Les pages qu'une recherche rend à `by`, rangées.
async fn pages_found(by: &Account<'_>, space: Uuid, q: &str) -> Vec<Uuid> {
    let mut pages = pages(&found(by, space, q).await);
    pages.sort();
    pages
}

fn sorted(mut pages: Vec<Uuid>) -> Vec<Uuid> {
    pages.sort();
    pages
}

/// Une recherche à envoyer hors d'un [`Browser`], dans une tâche : la session de `by`, `Host`.
fn request(by: &Account<'_>, space: Uuid, body: Value) -> Request<Body> {
    let mut request = json_request(Method::POST, &uri(space), &body);
    with_session(&mut request, &by.browser);
    request
}

fn with_session(request: &mut Request<Body>, by: &Browser<'_>) {
    request
        .headers_mut()
        .insert(header::HOST, "localhost".parse().unwrap());
    if let Some(cookie) = by.cookie() {
        request
            .headers_mut()
            .insert(header::COOKIE, cookie.parse().unwrap());
    }
}

type Answer = (StatusCode, HeaderMap, Value);

/// Envoie la requête dans une tâche : son statut, ses en-têtes, son corps en JSON.
fn send(t: &TestApp, request: Request<Body>) -> JoinHandle<Answer> {
    let router = t.router.clone();
    tokio::spawn(async move {
        let response = router.oneshot(request).await.unwrap();
        let (status, headers) = (response.status(), response.headers().clone());
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, headers, body)
    })
}

/// Une recherche de `by`, envoyée dans une tâche.
fn asking(t: &TestApp, by: &Account<'_>, space: Uuid, q: &str) -> JoinHandle<Answer> {
    send(t, request(by, space, json!({ "q": q })))
}

/// Retient chaque calcul de recherche à son début, sur son fil, jusqu'à un [`Gate::pass`], et
/// note l'ordre des comptes servis. Lâchée, elle laisse tout passer.
struct Gate {
    permits: mpsc::Sender<()>,
    order: Arc<Mutex<Vec<Uuid>>>,
}

impl Gate {
    fn on(t: &TestApp) -> Gate {
        let (permits, taken) = mpsc::channel::<()>();
        let taken = Mutex::new(taken);
        let order = Arc::new(Mutex::new(Vec::new()));
        let served = order.clone();
        t.searches.before_each_computation(move |account| {
            served.lock().unwrap().push(account);
            // Une émettrice lâchée rend une erreur : le calcul passe.
            let _ = taken.lock().unwrap().recv_timeout(REAL_WAIT);
        });
        Gate { permits, order }
    }

    /// Laisse passer un calcul : celui qui attend, ou le prochain.
    fn pass(&self) {
        self.permits.send(()).unwrap();
    }

    /// Les comptes dont le calcul a commencé, dans l'ordre.
    fn order(&self) -> Vec<Uuid> {
        self.order.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------------------------
// Droits.

/// Review Focus 1 : pour qui ne voit pas la page, un mot que seule elle porte (dans son titre ou
/// dans son texte) reçoit la réponse d'un mot que personne n'a écrit, octet pour octet,
/// `indexing` compris. Témoin : sa propriétaire la trouve par les deux.
#[tokio::test]
async fn a_word_only_an_unseen_page_holds_answers_like_a_word_nobody_wrote() {
    let t = app().await;
    let team = Team::new(&t).await;
    let secret = team
        .page(None, "Secret licorne", &[user(&team.olga, Level::Full)])
        .await;
    write(&team.olga, secret, "Un texte sur le narval.\n").await;
    t.indexer.settled().await;

    assert_eq!(
        pages_found(&team.olga, team.space, "licorne").await,
        [secret]
    );
    assert_eq!(
        pages_found(&team.olga, team.space, "narval").await,
        [secret]
    );
    for who in [&team.mona, &team.tom, &team.gina] {
        let nobody = search(who, team.space, "zzzrien").await;
        nobody.assert_status(StatusCode::OK);
        assert_eq!(nobody.json(), nothing());
        for word in ["licorne", "narval", "secret licorne", "narv"] {
            let reply = search(who, team.space, word).await;
            assert_eq!(outcome(&reply), outcome(&nobody), "{word}");
            assert_eq!(reply.body, nobody.body, "{word}");
        }
    }
}

/// Un invité ne trouve que les pages où il est invité ; un membre, celles des membres aussi.
#[tokio::test]
async fn a_guest_finds_only_the_pages_he_was_invited_on() {
    let t = app().await;
    let team = Team::new(&t).await;
    let invited = team
        .page(
            None,
            "Girafe commune",
            &[members(Level::Read), user(&team.gina, Level::Read)],
        )
        .await;
    let closed = team
        .page(None, "Girafe des membres", &[members(Level::Read)])
        .await;
    t.indexer.settled().await;
    assert_eq!(
        pages_found(&team.gina, team.space, "girafe").await,
        [invited]
    );
    assert_eq!(
        pages_found(&team.mona, team.space, "girafe").await,
        sorted(vec![invited, closed])
    );
}

/// Une page privée n'est trouvée que par qui y est nommé, par son titre comme par son texte.
#[tokio::test]
async fn a_private_page_is_found_by_its_owner_only() {
    let t = app().await;
    let team = Team::new(&t).await;
    let private = team
        .page(None, "Carnet okapi", &[user(&team.olga, Level::Full)])
        .await;
    write(&team.olga, private, "Le pangolin dort.\n").await;
    t.indexer.settled().await;
    for word in ["okapi", "pangolin"] {
        assert_eq!(pages_found(&team.olga, team.space, word).await, [private]);
        for who in [&team.mona, &team.tom, &team.gina] {
            assert_eq!(found(who, team.space, word).await, nothing(), "{word}");
        }
    }
}

/// Review Focus 2 : un accès retiré vaut à la recherche suivante, sans l'indexeur (figé).
#[tokio::test]
async fn withdrawn_access_hides_the_page_at_the_next_query() {
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(
            None,
            "Dossier tapir",
            &[user(&team.olga, Level::Full), user(&team.mona, Level::Read)],
        )
        .await;
    t.indexer.settled().await;
    t.indexer.freeze();
    assert_eq!(pages_found(&team.mona, team.space, "tapir").await, [page]);
    let removed = team
        .olga
        .browser
        .delete(&format!(
            "/api/v1/pages/{page}/access/users/{}",
            team.mona.id
        ))
        .await;
    assert!(removed.status.is_success(), "{}", removed.text());
    assert_eq!(found(&team.mona, team.space, "tapir").await, nothing());
    assert_eq!(pages_found(&team.olga, team.space, "tapir").await, [page]);
}

/// Une page fermée aux membres n'est plus trouvée par eux à la recherche suivante.
#[tokio::test]
async fn a_page_closed_to_members_is_hidden_at_the_next_query() {
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Note lamantin", &[members(Level::Read)])
        .await;
    t.indexer.settled().await;
    t.indexer.freeze();
    assert_eq!(
        pages_found(&team.mona, team.space, "lamantin").await,
        [page]
    );
    let closed = team
        .olga
        .browser
        .put(
            &format!("/api/v1/pages/{page}/access/general"),
            json!({ "level": null }),
        )
        .await;
    assert!(closed.status.is_success(), "{}", closed.text());
    assert_eq!(found(&team.mona, team.space, "lamantin").await, nothing());
    assert_eq!(
        pages_found(&team.olga, team.space, "lamantin").await,
        [page]
    );
}

/// Un membre retiré de l'espace reçoit, à sa recherche suivante, la réponse d'un espace inconnu.
#[tokio::test]
async fn a_member_removed_from_the_space_gets_404() {
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Note axolotl", &[members(Level::Read)])
        .await;
    t.indexer.settled().await;
    t.indexer.freeze();
    assert_eq!(pages_found(&team.mona, team.space, "axolotl").await, [page]);
    let removed = team
        .olga
        .browser
        .delete(&format!(
            "/api/v1/spaces/{}/members/{}",
            team.space, team.mona.id
        ))
        .await;
    assert!(removed.status.is_success(), "{}", removed.text());
    let reply = search(&team.mona, team.space, "axolotl").await;
    reply.assert_error(StatusCode::NOT_FOUND, "not_found");
    let unknown = search(&team.mona, Uuid::now_v7(), "axolotl").await;
    assert_eq!(outcome(&reply), outcome(&unknown));
}

/// La corbeille ne sort jamais (L4) : une page jetée n'est plus trouvée, ni par son titre ni par
/// son texte, par personne ; restaurée, elle revient. L'indexeur est figé : elle reste indexée.
#[tokio::test]
async fn a_trashed_page_is_not_found_and_comes_back_restored() {
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Note ocelot", &[members(Level::Full)])
        .await;
    write(&team.olga, page, "Le quokka sourit.\n").await;
    t.indexer.settled().await;
    t.indexer.freeze();
    for word in ["ocelot", "quokka"] {
        assert_eq!(pages_found(&team.mona, team.space, word).await, [page]);
    }
    let trashed = team
        .olga
        .browser
        .post_empty(&format!("/api/v1/pages/{page}/trash"))
        .await;
    assert!(trashed.status.is_success(), "{}", trashed.text());
    for who in [&team.olga, &team.mona] {
        for word in ["ocelot", "quokka"] {
            assert_eq!(found(who, team.space, word).await, nothing(), "{word}");
        }
    }
    let restored = team
        .olga
        .browser
        .post_empty(&format!("/api/v1/pages/{page}/restore"))
        .await;
    assert!(restored.status.is_success(), "{}", restored.text());
    for word in ["ocelot", "quokka"] {
        assert_eq!(pages_found(&team.mona, team.space, word).await, [page]);
    }
}

/// La sous-page d'une page jetée n'est pas trouvée, bien qu'elle ne soit pas jetée elle-même.
#[tokio::test]
async fn a_subpage_of_a_trashed_page_is_not_found() {
    let t = app().await;
    let team = Team::new(&t).await;
    let parent = team.page(None, "Parent", &[members(Level::Full)]).await;
    let child = team.page(Some(parent), "Enfant ornithorynque", &[]).await;
    t.indexer.settled().await;
    t.indexer.freeze();
    assert_eq!(
        pages_found(&team.mona, team.space, "ornithorynque").await,
        [child]
    );
    team.trash(parent).await;
    for who in [&team.olga, &team.mona] {
        assert_eq!(found(who, team.space, "ornithorynque").await, nothing());
    }
}

/// Le fil d'Ariane ne nomme que les ancêtres que le compte voit (R903) : pour la propriétaire,
/// toute la chaîne, du plus haut au plus proche ; pour une invitée sur la page du milieu, à
/// partir d'elle ; pour une invitée sur la seule sous-page, rien.
#[tokio::test]
async fn the_breadcrumb_names_only_seen_ancestors() {
    let t = app().await;
    let team = Team::new(&t).await;
    let top = team
        .page(None, "Haut", &[user(&team.olga, Level::Full)])
        .await;
    let middle = team
        .page(Some(top), "Milieu", &[user(&team.gina, Level::Read)])
        .await;
    let leaf = team.page(Some(middle), "Feuille wombat", &[]).await;
    let aside = team.page(Some(top), "Côté", &[]).await;
    let twig = team
        .page(
            Some(aside),
            "Brindille dugong",
            &[user(&team.gina, Level::Read)],
        )
        .await;
    t.indexer.settled().await;

    let path = |results: &Value| results["results"][0]["path"].clone();
    let olga = found(&team.olga, team.space, "wombat").await;
    assert_eq!(pages(&olga), [leaf]);
    assert_eq!(
        path(&olga),
        json!([
            { "id": top.to_string(), "title": "Haut" },
            { "id": middle.to_string(), "title": "Milieu" },
        ])
    );
    let gina = found(&team.gina, team.space, "wombat").await;
    assert_eq!(pages(&gina), [leaf]);
    assert_eq!(
        path(&gina),
        json!([{ "id": middle.to_string(), "title": "Milieu" }])
    );
    let gina = found(&team.gina, team.space, "dugong").await;
    assert_eq!(pages(&gina), [twig]);
    assert_eq!(path(&gina), json!([]));
    // Les titres des ancêtres ne sont pas un texte cherché (écart E2).
    assert_eq!(
        found(&team.olga, team.space, "milieu wombat").await,
        nothing()
    );
}

/// Le titre d'un résultat est celui de l'instant (R904), lu dans la base : la page est renommée,
/// l'indexeur figé.
#[tokio::test]
async fn the_title_is_the_current_one() {
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Ancien titre", &[members(Level::Full)])
        .await;
    write(&team.olga, page, "Le suricate veille.\n").await;
    t.indexer.settled().await;
    t.indexer.freeze();
    team.olga
        .patched(&page.to_string(), json!({ "title": "Nouveau titre" }))
        .await;
    let results = found(&team.mona, team.space, "suricate").await;
    assert_eq!(pages(&results), [page]);
    assert_eq!(results["results"][0]["title"], "Nouveau titre");
}

/// `indexing` ne dit rien d'une page qu'on ne voit pas : une page des membres attend
/// l'indexeur (figé) ; un membre lit `indexing` vrai, l'invitée faux. Témoin : avant elle, faux
/// pour les deux.
#[tokio::test]
async fn indexing_says_nothing_of_unseen_pages() {
    let t = app().await;
    let team = Team::new(&t).await;
    team.page(None, "Déjà là", &[members(Level::Read)]).await;
    t.indexer.settled().await;
    t.indexer.freeze();
    for who in [&team.mona, &team.gina] {
        assert_eq!(found(who, team.space, "zzzrien").await, nothing());
    }
    // Créée par l'application : l'indexeur, au repos, ne l'apprend que par son annonce (R1055),
    // et la dit en retard dès son tour suivant.
    team.olga
        .post(json!({
            "spaceId": team.space.to_string(), "section": "space", "title": "Tardive",
        }))
        .await
        .assert_status(StatusCode::CREATED);
    let late = json!({ "results": [], "more": false, "indexing": true });
    let deadline = std::time::Instant::now() + REAL_WAIT;
    while found(&team.mona, team.space, "zzzrien").await != late {
        assert!(
            std::time::Instant::now() < deadline,
            "jamais dite en retard"
        );
    }
    assert_eq!(found(&team.gina, team.space, "zzzrien").await, nothing());
}

/// R916, de bout en bout : deux pages de Mona, dosées (les deux mots cherchés, en nombres et en
/// longueurs différents), gardent leur ordre et leur réponse entière, octet pour octet, quoi
/// qu'écrivent ensuite une page qu'elle ne voit pas dans son espace et une page d'un autre
/// espace : des centaines de blocs qui portent ces mots. Témoin : ces blocs sont bien indexés.
#[tokio::test]
async fn two_dosed_blocks_keep_their_order_whatever_others_write() {
    let t = app().await;
    let team = Team::new(&t).await;
    let first = team.page(None, "Première", &[members(Level::Full)]).await;
    let second = team.page(None, "Seconde", &[members(Level::Full)]).await;
    write(&team.mona, first, "tigre tigre tigre lion\n").await;
    write(
        &team.mona,
        second,
        "tigre lion lion lion et bien d'autres bêtes encore ici\n",
    )
    .await;
    t.indexer.settled().await;
    let before = search(&team.mona, team.space, "tigre lion").await;
    before.assert_status(StatusCode::OK);
    assert_eq!(
        sorted(pages(&before.json())),
        sorted(vec![first, second]),
        "{}",
        before.text()
    );

    let hidden = team
        .page(None, "Cachée", &[user(&team.olga, Level::Full)])
        .await;
    write(
        &team.olga,
        hidden,
        &blocks("tigre tigre tigre tigre lion", 300),
    )
    .await;
    let elsewhere: Uuid = team
        .zoe
        .create(json!({ "title": "Ailleurs" }))
        .await
        .id
        .parse()
        .unwrap();
    write(&team.zoe, elsewhere, &blocks("lion", 300)).await;
    t.indexer.settled().await;
    assert_eq!(pages_found(&team.olga, team.space, "tigre").await.len(), 3);
    assert_eq!(
        pages_found(&team.zoe, team.zoe.space, "lion").await,
        [elsewhere]
    );

    let after = search(&team.mona, team.space, "tigre lion").await;
    assert_eq!(after.text(), before.text());
}

/// Une page trouvée par son titre n'a ni bloc ni extrait ; trouvée par son texte, elle porte le
/// bloc, un extrait et les places du mot en unités UTF-16.
#[tokio::test]
async fn a_hit_names_its_block_and_marks_the_words() {
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Titre capybara", &[members(Level::Full)])
        .await;
    write(&team.olga, page, "Été 😀 le fennec court.\n").await;
    t.indexer.settled().await;
    let by_title = found(&team.mona, team.space, "capybara").await;
    assert_eq!(
        by_title["results"],
        json!([{
            "pageId": page.to_string(),
            "title": "Titre capybara",
            "path": [],
            "blockId": null,
            "excerpt": "",
            "marks": [],
        }])
    );
    let by_text = found(&team.mona, team.space, "fennec").await;
    let hit = &by_text["results"][0];
    assert_eq!(hit["pageId"], page.to_string());
    assert!(hit["blockId"].is_string(), "{hit}");
    assert_eq!(hit["excerpt"], "Été 😀 le fennec court.");
    // « Été 😀 le » : trois lettres, une espace, deux unités pour l'emoji, une espace, « le »,
    // une espace.
    assert_eq!(hit["marks"], json!([[10, 16]]));
}

// ---------------------------------------------------------------------------------------------
// Bornes, ordre des refus.

/// Une recherche au-delà de ses bornes : `422 invalid_query`, un message qui tutoie, dit la
/// borne, et ne recopie rien de la recherche. Témoin : à la borne, elle passe.
#[tokio::test]
async fn a_query_over_the_bounds_is_422() {
    let t = app().await;
    let team = Team::new(&t).await;
    let at_the_bound = "a".repeat(200);
    search(&team.mona, team.space, &at_the_bound)
        .await
        .assert_status(StatusCode::OK);
    let long = "xyzzy".repeat(41);
    let many = "xyzzyun deux trois quatre cinq six sept huit neuf";
    for (q, bound) in [(long.as_str(), "200"), (many, "8")] {
        let reply = search(&team.mona, team.space, q).await;
        let body = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_query");
        let message = body["message"].as_str().unwrap();
        assert!(message.contains(bound), "{message}");
        assert!(message.starts_with("Ta recherche"), "{message}");
        assert!(!message.contains("xyzzy"), "{message}");
    }
    let eight = "un deux trois quatre cinq six sept huit";
    search(&team.mona, team.space, eight)
        .await
        .assert_status(StatusCode::OK);
}

/// Un corps `http-body` sans fin, par morceaux d'un Kio, qui compte les morceaux demandés.
struct Endless(Arc<AtomicUsize>);

impl http_body::Body for Endless {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Infallible>>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(Some(Ok(http_body::Frame::data(Bytes::from(vec![
            b' ';
            1024
        ])))))
    }
}

/// Le corps d'une recherche est borné à 4 Kio : au-delà, `413`, sans que le reste soit lu (un
/// corps sans fin est refusé après quelques morceaux). Sans rôle dans l'espace, le même corps
/// vaut `404`, et n'est pas lu plus loin non plus.
#[tokio::test]
async fn a_body_over_its_bound_is_refused_without_being_read_whole() {
    let t = app().await;
    let team = Team::new(&t).await;
    for (who, status) in [
        (&team.mona, StatusCode::PAYLOAD_TOO_LARGE),
        (&team.sam, StatusCode::NOT_FOUND),
    ] {
        let read = Arc::new(AtomicUsize::new(0));
        let mut request = Request::post(uri(team.space))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::new(Endless(read.clone())))
            .unwrap();
        with_session(&mut request, &who.browser);
        let (answered, _, _) = send(&t, request).await.unwrap();
        assert_eq!(answered, status);
        let read = read.load(Ordering::SeqCst);
        assert!((4..=6).contains(&read), "{read} morceaux d'un Kio lus");
    }
    assert_eq!(mementum_server::MAX_SEARCH_BODY_BYTES, 4096);
}

/// L'ordre des refus : le même corps illisible vaut `404` à qui n'a aucun rôle dans l'espace, la
/// réponse d'un espace inconnu, et un refus du corps (`400`, `422`) à qui en a un.
#[tokio::test]
async fn an_unreadable_body_in_a_space_without_role_is_404_not_422() {
    let t = app().await;
    let team = Team::new(&t).await;
    let post = async |by: &Account<'_>, space: Uuid, body: &'static str| {
        let request = Request::post(uri(space))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        by.browser.send(request).await
    };
    let long: &'static str =
        Box::leak(format!(r#"{{"q":"{}"}}"#, "a".repeat(201)).into_boxed_str());
    let bodies = [
        ("{", StatusCode::BAD_REQUEST, "invalid_json"),
        ("[]", StatusCode::UNPROCESSABLE_ENTITY, "invalid_json"),
        (
            r#"{"inconnu":1}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_json",
        ),
        (
            r#"{"q":"mot","after":-1}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_json",
        ),
        (
            r#"{"q":"mot","after":1.5}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_json",
        ),
        (
            r#"{"q":"mot","after":100}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_query",
        ),
        (long, StatusCode::UNPROCESSABLE_ENTITY, "invalid_query"),
    ];
    let unknown = Uuid::now_v7();
    for (body, status, code) in bodies {
        for stranger in [&team.sam, &team.zoe] {
            let reply = post(stranger, team.space, body).await;
            reply.assert_error(StatusCode::NOT_FOUND, "not_found");
            let missing = post(stranger, unknown, body).await;
            assert_eq!(outcome(&reply), outcome(&missing), "{body}");
        }
        for member in [&team.olga, &team.gina] {
            post(member, team.space, body)
                .await
                .assert_error(status, code);
        }
    }
    // Sans session, `401` avant tout.
    let nobody = t.browser();
    let reply = nobody.post(&uri(team.space), json!({ "q": "mot" })).await;
    reply.assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
}

/// Lit, d'une réponse HTTP/1 reçue en entier (en-têtes et corps annoncé), son statut.
fn status_line(received: &[u8]) -> Option<u16> {
    let end = received.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&received[..end]);
    let length: usize = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .map_or(Some(0), |(_, value)| value.trim().parse().ok())?;
    (received.len() >= end + 4 + length).then_some(())?;
    head.split(' ').nth(1)?.parse().ok()
}

/// Envoie toute la requête (tête puis corps) avant de lire quoi que ce soit, comme un client
/// HTTP/1 qui n'attend pas, puis lit la réponse.
async fn push_then_read(
    addr: std::net::SocketAddr,
    cookie: Option<String>,
    path: &str,
    body: &[u8],
    extra: &str,
) -> Vec<u8> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let cookie = cookie
        .map(|c| format!("Cookie: {c}\r\n"))
        .unwrap_or_default();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n{cookie}\
         Content-Length: {}\r\n{extra}\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    let _ = stream.write_all(body).await;
    let mut received = Vec::new();
    let mut buffer = [0u8; 8192];
    let deadline = tokio::time::Instant::now() + common::WAIT;
    while status_line(&received).is_none() {
        match tokio::time::timeout_at(deadline, stream.read(&mut buffer)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(n)) => received.extend_from_slice(&buffer[..n]),
        }
    }
    received
}

/// R505 : chaque refus arrive à un client qui envoie toute sa requête, corps compris, avant de
/// lire, sur une vraie connexion : le corps, petit et borné, est reçu avant de refuser.
#[tokio::test]
async fn a_refusal_reaches_a_client_that_keeps_sending() {
    let t = app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let path = uri(team.space);
    let valid = format!(r#"{{"q":"{}"}}"#, "é".repeat(200)).into_bytes();
    let garbage = vec![b'x'; 4000];
    let long = format!(r#"{{"q":"{}"}}"#, "a".repeat(2000)).into_bytes();
    let other = format!("X-Mementum-Account: {}\r\n", team.olga.id);
    for (what, cookie, body, extra, status) in [
        ("sans session", None, &valid, "", 401u16),
        (
            "un autre compte nommé",
            team.mona.browser.cookie(),
            &valid,
            other.as_str(),
            409,
        ),
        ("sans rôle", team.sam.browser.cookie(), &garbage, "", 404),
        (
            "corps illisible",
            team.mona.browser.cookie(),
            &garbage,
            "",
            400,
        ),
        (
            "recherche trop longue",
            team.mona.browser.cookie(),
            &long,
            "",
            422,
        ),
    ] {
        let received = push_then_read(addr, cookie, &path, body, extra).await;
        assert_eq!(
            status_line(&received),
            Some(status),
            "{what} : reçu {:?}",
            String::from_utf8_lossy(&received)
        );
    }
}

/// Une recherche sans aucun mot répond vide sans rien calculer : ni place, ni ensemble bâti.
/// Témoin : une recherche d'un mot calcule.
#[tokio::test]
async fn an_empty_query_answers_empty_without_computing() {
    let t = app().await;
    let team = Team::new(&t).await;
    t.indexer.settled().await;
    for q in ["", "   ", "… ! ?", "\u{301}"] {
        assert_eq!(found(&team.mona, team.space, q).await, nothing(), "{q:?}");
    }
    assert_eq!(
        (
            t.searches.started(),
            t.searches.sets_built(),
            t.searches.computed()
        ),
        (0, 0, 0)
    );
    assert_eq!(found(&team.mona, team.space, "mot").await, nothing());
    assert_eq!(
        (
            t.searches.started(),
            t.searches.sets_built(),
            t.searches.computed()
        ),
        (1, 1, 1)
    );
}

/// `n` pages des membres au titre « Zèbre <rang> ».
async fn zebras(team: &Team<'_>, n: usize) -> Vec<Uuid> {
    let mut pages = Vec::with_capacity(n);
    for i in 0..n {
        pages.push(
            team.page(None, &format!("Zèbre {i}"), &[members(Level::Read)])
                .await,
        );
    }
    pages
}

/// `POST …/search` avec `after`.
async fn found_after(by: &Account<'_>, space: Uuid, q: &str, after: u32) -> Value {
    let reply = by
        .browser
        .post(&uri(space), json!({ "q": q, "after": after }))
        .await;
    reply.assert_status(StatusCode::OK);
    reply.json()
}

/// Une recherche rend cent pages au plus, vingt par réponse (R911) : cent cinq pages portent le
/// mot ; la première réponse en rend vingt et dit qu'il en reste ; la cinquième rend les
/// dernières des cent et dit qu'il n'en reste pas ; au-delà, `422 invalid_query`.
#[tokio::test]
async fn paging_stops_at_the_bound() {
    let t = app().await;
    let team = Team::new(&t).await;
    let all = zebras(&team, 105).await;
    t.indexer.settled().await;
    let mut seen = Vec::new();
    for after in [0, 20, 40, 60, 80] {
        let answer = found_after(&team.mona, team.space, "zèbre", after).await;
        assert_eq!(pages(&answer).len(), 20, "après {after}");
        assert_eq!(answer["more"], after < 80, "après {after}");
        seen.extend(pages(&answer));
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 100);
    assert!(seen.iter().all(|page| all.contains(page)));
    let last = found_after(&team.mona, team.space, "zèbre", 99).await;
    assert_eq!(pages(&last).len(), 1);
    assert_eq!(last["more"], false);
    for after in [100, 101, u32::MAX] {
        let reply = team
            .mona
            .browser
            .post(&uri(team.space), json!({ "q": "zèbre", "after": after }))
            .await;
        let body = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_query");
        assert!(body["message"].as_str().unwrap().contains("cent"), "{body}");
    }
}

/// Deux réponses qui se suivent ne rendent jamais la même page, et aucune ne manque : quarante-
/// cinq pages portent le mot, lues par `after` 0, 20 puis 40. La première réponse, redemandée,
/// est la même.
#[tokio::test]
async fn paging_never_repeats_a_page() {
    let t = app().await;
    let team = Team::new(&t).await;
    let all = sorted(zebras(&team, 45).await);
    t.indexer.settled().await;
    let first = found_after(&team.mona, team.space, "zèbre", 0).await;
    let second = found_after(&team.mona, team.space, "zèbre", 20).await;
    let third = found_after(&team.mona, team.space, "zèbre", 40).await;
    assert_eq!(
        [&first, &second, &third].map(|answer| (pages(answer).len(), answer["more"].clone())),
        [(20, json!(true)), (20, json!(true)), (5, json!(false))]
    );
    let mut seen: Vec<Uuid> = [&first, &second, &third]
        .into_iter()
        .flat_map(pages)
        .collect();
    seen.sort();
    assert_eq!(seen, all, "chaque page une fois");
    assert_eq!(found(&team.mona, team.space, "zèbre").await, first);
}

/// La recherche d'une personne n'est jamais journalisée, à aucun niveau, sur aucun fil (R917) :
/// ni quand elle passe, ni refusée. Témoin : le journal de tout le processus voit bien les
/// lectures de la base. Et elle ne voyage pas dans une adresse : un `GET` répond `405`, et un `q`
/// dans l'adresse d'un `POST` n'est pas lu.
#[tokio::test]
async fn the_query_is_never_logged_and_never_in_an_address() {
    let logs = all_logs();
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Mot xyzzytémoin", &[members(Level::Read)])
        .await;
    t.indexer.settled().await;
    // Rien d'autre que la recherche ne mettrait ce mot au journal (le titre n'y va pas).
    assert!(!logs.text().to_lowercase().contains("xyzzyt"));
    assert_eq!(
        pages_found(&team.mona, team.space, "xyzzytémoin").await,
        [page]
    );
    assert_eq!(
        pages_found(&team.mona, team.space, "XyzzyTém").await,
        [page]
    );
    let many = "xyzzytémoin deux trois quatre cinq six sept huit neuf";
    search(&team.mona, team.space, many)
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_query");
    search(&team.sam, team.space, "xyzzytémoin")
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");

    let get = team
        .mona
        .browser
        .get(&format!("{}?q=xyzzyt%C3%A9moin", uri(team.space)))
        .await;
    get.assert_error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    let in_the_address = team
        .mona
        .browser
        .post(
            &format!("{}?q=xyzzyt%C3%A9moin", uri(team.space)),
            json!({ "q": "" }),
        )
        .await;
    in_the_address.assert_status(StatusCode::OK);
    assert_eq!(in_the_address.json(), nothing());

    let text = logs.text().to_lowercase();
    assert!(text.contains("sqlx::query"), "le journal ne voit rien");
    assert!(!text.contains("xyzzyt"), "la recherche est au journal");
}

/// Toute réponse de la route, refus compris, porte les en-têtes de l'API.
#[tokio::test]
async fn search_answers_carry_the_api_headers() {
    let t = app().await;
    let team = Team::new(&t).await;
    let long = "a".repeat(201);
    for (who, q) in [
        (&team.mona, "mot"),
        (&team.mona, long.as_str()),
        (&team.sam, "mot"),
    ] {
        let reply = search(who, team.space, q).await;
        assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(reply.headers["x-content-type-options"], "nosniff");
    }
}

// ---------------------------------------------------------------------------------------------
// Débit.

/// R910 : un compte a une recherche admise et une qui attend. Une troisième prend la place de
/// celle qui attendait, refusée (`429 search_busy`, `Retry-After: 1`) sans avoir rien calculé.
#[tokio::test]
async fn a_newer_search_takes_the_place_of_the_one_waiting() {
    let t = app().await;
    let team = Team::new(&t).await;
    t.indexer.settled().await;
    let gate = Gate::on(&t);
    let first = asking(&t, &team.mona, team.space, "un");
    until("le premier calcul", || t.searches.started() == 1).await;
    let second = asking(&t, &team.mona, team.space, "deux");
    until("la deuxième attend", || t.searches.counts() == (1, 1)).await;
    let third = asking(&t, &team.mona, team.space, "trois");
    let (status, headers, body) = second.await.unwrap();
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"], "search_busy");
    assert_eq!(headers[header::RETRY_AFTER], "1");
    assert!(body["message"].as_str().unwrap().contains("ton compte"));
    assert_eq!(t.searches.counts(), (1, 1));
    assert_eq!(t.searches.started(), 1, "la refusée n'a rien calculé");
    drop(gate);
    assert_eq!(first.await.unwrap().0, StatusCode::OK);
    assert_eq!(third.await.unwrap().0, StatusCode::OK);
    assert_eq!(t.searches.started(), 2);
}

/// Une rafale de recherches d'un compte ne retarde pas un autre : une seule place de l'instance
/// est libre, Mona la prend et relance dix fois (neuf refusées, une qui attend), puis Tom
/// cherche. Au calcul de Mona fini, c'est Tom qui est servi ; la recherche de Mona qui attendait
/// ne l'est qu'après lui. L'ordre de service se lit au début des calculs, sans minutage.
#[tokio::test]
async fn a_burst_of_searches_of_one_account_does_not_delay_another() {
    let t = app().await;
    let team = Team::new(&t).await;
    t.indexer.settled().await;
    let held: Vec<_> = (1..MAX_SEARCHES).map(|_| t.searches.hold()).collect();
    let gate = Gate::on(&t);
    let first = asking(&t, &team.mona, team.space, "un");
    until("le calcul de Mona", || t.searches.started() == 1).await;
    let mut waiting: Option<JoinHandle<Answer>> = None;
    for i in 0..10 {
        let next = asking(&t, &team.mona, team.space, &format!("mot{i}"));
        match waiting.replace(next) {
            Some(refused) => assert_eq!(refused.await.unwrap().0, StatusCode::TOO_MANY_REQUESTS),
            None => until("la rafale attend", || t.searches.counts().1 == 1).await,
        }
    }
    let tom = asking(&t, &team.tom, team.space, "autre");
    until("Tom attend", || t.searches.counts() == (MAX_SEARCHES, 2)).await;
    assert_eq!(t.searches.started(), 1);

    gate.pass();
    assert_eq!(first.await.unwrap().0, StatusCode::OK);
    // La première réponse de Mona est partie : sa recherche suivante est prête, mais Tom tient
    // la place.
    until("le calcul de Tom", || t.searches.started() == 2).await;
    settle().await;
    assert_eq!(gate.order(), [team.mona.id, team.tom.id]);
    assert_eq!(t.searches.counts(), (MAX_SEARCHES, 1));
    gate.pass();
    assert_eq!(tom.await.unwrap().0, StatusCode::OK);
    until("le second calcul de Mona", || t.searches.started() == 3).await;
    gate.pass();
    assert_eq!(waiting.unwrap().await.unwrap().0, StatusCode::OK);
    assert_eq!(gate.order(), [team.mona.id, team.tom.id, team.mona.id]);
    drop(held);
}

/// R630 : une recherche abandonnée garde ses places jusqu'à la fin de son calcul. La requête de
/// Mona est lâchée pendant que son calcul tourne ; sa recherche suivante attend, et ne commence
/// qu'une fois ce calcul fini.
#[tokio::test]
async fn an_abandoned_search_keeps_its_place_until_its_computation_ends() {
    let t = app().await;
    let team = Team::new(&t).await;
    t.indexer.settled().await;
    let gate = Gate::on(&t);
    let first = asking(&t, &team.mona, team.space, "un");
    until("le premier calcul", || t.searches.started() == 1).await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    settle().await;
    assert_eq!(t.searches.counts(), (1, 0), "le calcul tient sa place");
    let second = asking(&t, &team.mona, team.space, "deux");
    until("la suivante attend", || t.searches.counts() == (1, 1)).await;
    settle().await;
    assert_eq!(
        t.searches.started(),
        1,
        "rien ne commence avant la fin du calcul"
    );
    assert_eq!(t.searches.computed(), 0);
    gate.pass();
    until("le second calcul", || t.searches.started() == 2).await;
    assert_eq!(t.searches.computed(), 1);
    gate.pass();
    assert_eq!(second.await.unwrap().0, StatusCode::OK);
}

/// Les droits dont l'ensemble permis est bâti sont lus une fois la place prise, pas avant
/// l'attente (R89) : une recherche de Mona attend derrière sa précédente ; son accès à la page
/// est retiré pendant ce temps ; servie ensuite, elle ne la trouve pas. Témoin : la précédente,
/// partie avant le retrait, la trouvait.
#[tokio::test]
async fn a_right_withdrawn_while_a_search_waits_is_not_served() {
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(
            None,
            "Dossier tamanoir",
            &[user(&team.olga, Level::Full), user(&team.mona, Level::Read)],
        )
        .await;
    t.indexer.settled().await;
    t.indexer.freeze();
    let gate = Gate::on(&t);
    let first = asking(&t, &team.mona, team.space, "tamanoir");
    until("le premier calcul", || t.searches.started() == 1).await;
    let second = asking(&t, &team.mona, team.space, "tamanoir");
    until("la seconde attend", || t.searches.counts() == (1, 1)).await;
    let removed = team
        .olga
        .browser
        .delete(&format!(
            "/api/v1/pages/{page}/access/users/{}",
            team.mona.id
        ))
        .await;
    assert!(removed.status.is_success(), "{}", removed.text());
    drop(gate);
    let (status, _, body) = first.await.unwrap();
    assert_eq!((status, pages(&body)), (StatusCode::OK, vec![page]));
    let (status, _, body) = second.await.unwrap();
    assert_eq!((status, body), (StatusCode::OK, nothing()));
}

/// I7 : l'ensemble des pages permises est bâti sous la garde du débit, hors du moteur. Les quatre
/// places tenues, mille recherches d'un compte arrivent : aucun ensemble n'est bâti, aucun calcul
/// ne commence (une seule attend, les autres sont refusées). Puis, les places rendues, le calcul
/// de celle qui attendait tourne pendant que le moteur du test, à un seul fil, sert une tâche
/// neuve : il n'est pas sur ce fil.
#[tokio::test]
async fn the_allowed_set_is_built_under_the_guard_off_the_engine() {
    let t = app().await;
    let team = Team::new(&t).await;
    team.page(None, "Mot fossa", &[members(Level::Read)]).await;
    t.indexer.settled().await;
    let held: Vec<_> = (0..MAX_SEARCHES).map(|_| t.searches.hold()).collect();
    let mut waiting: Option<JoinHandle<Answer>> = None;
    for _ in 0..1000 {
        let next = asking(&t, &team.mona, team.space, "fossa");
        match waiting.replace(next) {
            Some(refused) => assert_eq!(refused.await.unwrap().0, StatusCode::TOO_MANY_REQUESTS),
            None => until("la première attend", || t.searches.counts().1 == 1).await,
        }
    }
    until("la dernière attend", || {
        t.searches.counts() == (MAX_SEARCHES, 1)
    })
    .await;
    settle().await;
    assert_eq!((t.searches.started(), t.searches.sets_built()), (0, 0));

    // Le calcul ne continue qu'une fois une tâche neuve du moteur du test servie : sur le fil du
    // moteur, il l'attendrait en vain.
    let engine = tokio::runtime::Handle::current();
    let free = Arc::new(AtomicBool::new(false));
    let served = free.clone();
    t.searches.before_each_computation(move |_| {
        let (ran, seen) = mpsc::channel::<()>();
        engine.spawn(async move {
            let _ = ran.send(());
        });
        served.store(
            seen.recv_timeout(Duration::from_secs(10)).is_ok(),
            Ordering::SeqCst,
        );
    });
    drop(held);
    let (status, _, body) = waiting.unwrap().await.unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pages(&body).len(), 1);
    assert!(free.load(Ordering::SeqCst), "le moteur est resté libre");
    assert_eq!((t.searches.started(), t.searches.sets_built()), (1, 1));
}

// ---------------------------------------------------------------------------------------------
// Ce que `indexing` coûte (R1055).

/// L'indexeur au repos, une recherche ne lit pas dans la base ce qui est en retard : il le sait.
#[tokio::test]
async fn an_idle_indexer_is_not_asked_the_database_what_is_late() {
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Note gerboise", &[members(Level::Read)])
        .await;
    t.indexer.settled().await;
    let reads = t.indexer.behind_reads();
    for who in [&team.olga, &team.mona, &team.gina] {
        assert_eq!(found(who, team.space, "zzzrien").await, nothing());
    }
    assert_eq!(
        pages_found(&team.mona, team.space, "gerboise").await,
        [page]
    );
    assert!(!t.indexer.behind(team.space, |_| true).await.unwrap());
    assert_eq!(t.indexer.behind_reads(), reads, "aucune lecture au repos");
}

/// L'indexeur pas au repos (figé, une page renommée attend l'index plein texte), les pages en
/// retard d'un espace sont lues dans la base une fois, puis gardées une seconde : soixante
/// demandes de suite n'en font pas une lecture de plus, et le filtre de chaque appelant
/// s'applique à ce qui est gardé. La seconde passée, elles sont relues, une fois.
#[tokio::test]
async fn late_pages_are_read_from_the_database_once_a_second_at_most() {
    let t = app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Premier nom", &[members(Level::Full)])
        .await;
    let other = team.page(None, "Autre", &[members(Level::Read)]).await;
    t.indexer.settled().await;
    t.indexer.freeze();
    team.olga
        .patched(&page.to_string(), json!({ "title": "Second nom" }))
        .await;
    let behind =
        async |seen: &dyn Fn(Uuid) -> bool| t.indexer.behind(team.space, seen).await.unwrap();
    let deadline = std::time::Instant::now() + REAL_WAIT;
    // Tant que l'indexeur se croit au repos (l'annonce du renommage n'est pas encore lue), rien
    // n'est lu. Un filtre qui ne voit rien passe ce que l'indexeur tient en mémoire, et va à la
    // base.
    while t.indexer.behind_reads() == 0 {
        assert!(std::time::Instant::now() < deadline, "jamais lues");
        assert!(!behind(&|_| false).await);
        tokio::task::yield_now().await;
    }
    let reads = t.indexer.behind_reads();
    assert_eq!(reads, 1);
    for _ in 0..20 {
        assert!(behind(&|id| id == page).await);
        assert!(!behind(&|_| false).await);
        assert!(!behind(&|id| id == Uuid::nil()).await);
    }
    assert_eq!(t.indexer.behind_reads(), reads, "gardées, pas relues");
    // Mona la voit en retard par la route, sans lecture de plus.
    assert_eq!(
        found(&team.mona, team.space, "zzzrien").await["indexing"],
        true
    );
    assert_eq!(t.indexer.behind_reads(), reads);

    while t.indexer.behind_reads() == reads {
        assert!(std::time::Instant::now() < deadline, "jamais relues");
        assert!(!behind(&|_| false).await);
        tokio::task::yield_now().await;
    }
    assert_eq!(t.indexer.behind_reads(), reads + 1);
    // Plus d'une seconde après leur indexation, les deux pages ne sont plus tenues en mémoire :
    // ce qui suit se lit dans ce qui est gardé, filtré pour chaque appelant.
    assert!(behind(&|id| id == page).await, "la page renommée");
    assert!(!behind(&|id| id == other).await, "pas l'autre");
    assert_eq!(t.indexer.behind_reads(), reads + 1);
}

// ---------------------------------------------------------------------------------------------
// Plancher (R932), sous l'horloge arrêtée.

/// Avance l'horloge arrêtée au-delà du plancher : le minuteur de tokio arrondit une échéance à
/// la milliseconde suivante.
async fn pass_the_floor() {
    tokio::time::advance(SEARCH_FLOOR + Duration::from_millis(2)).await;
}

/// La réponse d'une recherche calculée ne part pas avant le plancher : un mot que rien ne porte
/// (Tom) et un mot que cent blocs d'une page non vue portent (Mona), cherchés au même instant,
/// ont fini leur calcul et ne répondent ni l'un ni l'autre une milliseconde avant le plancher ;
/// ils répondent tous deux au plancher, la même chose.
#[tokio::test]
async fn an_answer_never_leaves_before_the_floor() {
    let t = app().await;
    let team = Team::new(&t).await;
    let hidden = team
        .page(None, "Cachée", &[user(&team.olga, Level::Full)])
        .await;
    write(&team.olga, hidden, &blocks("licorne", 100)).await;
    t.indexer.settled().await;
    assert_eq!(
        pages_found(&team.olga, team.space, "licorne").await,
        [hidden]
    );
    let ready = t.searches.answers_ready();

    let clock = StoppedClock::start();
    let asked = [
        asking(&t, &team.mona, team.space, "licorne"),
        asking(&t, &team.tom, team.space, "zzzrien"),
    ];
    // La barrière : les deux réponses sont prêtes, il ne leur reste que le plancher.
    until("les deux réponses prêtes", || {
        t.searches.answers_ready() == ready + 2
    })
    .await;
    assert!(asked.iter().all(|a| !a.is_finished()), "avant tout");
    tokio::time::advance(SEARCH_FLOOR - Duration::from_millis(1)).await;
    // Un minuteur échu à tort réveillerait sa tâche : lui laisser le temps de tourner.
    settle().await;
    assert!(
        asked.iter().all(|a| !a.is_finished()),
        "une milliseconde avant le plancher"
    );
    tokio::time::advance(Duration::from_millis(3)).await;
    until("les deux réponses au plancher", || {
        asked.iter().all(|a| a.is_finished())
    })
    .await;
    for answer in asked {
        let (status, _, body) = answer.await.unwrap();
        assert_eq!((status, body), (StatusCode::OK, nothing()));
    }
    assert_eq!(SEARCH_FLOOR, Duration::from_millis(50));
    drop(clock);
}

/// La place du compte est tenue jusqu'au départ de la réponse : une seconde recherche de Mona,
/// envoyée pendant que la première attend son plancher (son calcul fini), ne commence pas son
/// calcul ; elle le commence une fois le plancher passé.
#[tokio::test]
async fn the_account_keeps_its_place_until_the_answer_leaves() {
    let t = app().await;
    let team = Team::new(&t).await;
    t.indexer.settled().await;
    let clock = StoppedClock::start();
    let first = asking(&t, &team.mona, team.space, "un");
    until("la première réponse prête", || {
        t.searches.answers_ready() == 1
    })
    .await;
    let second = asking(&t, &team.mona, team.space, "deux");
    until("la seconde attend", || t.searches.counts() == (1, 1)).await;
    assert_eq!(
        t.searches.started(),
        1,
        "pas avant le départ de la première"
    );
    assert!(!first.is_finished());
    pass_the_floor().await;
    until("la première réponse", || first.is_finished()).await;
    until("la seconde réponse prête", || {
        t.searches.answers_ready() == 2
    })
    .await;
    assert!(!second.is_finished(), "la seconde a son plancher à elle");
    pass_the_floor().await;
    assert_eq!(second.await.unwrap().0, StatusCode::OK);
    assert_eq!(first.await.unwrap().0, StatusCode::OK);
    drop(clock);
}

/// R1054 : la place de l'instance est tenue, elle aussi, jusqu'au départ de la réponse. Une seule
/// est libre ; Mona a fini son calcul et attend son plancher ; Tom, un autre compte, n'entre pas
/// (sans cela, l'heure où son calcul commence dirait la durée de celui de Mona). Il entre une
/// fois le plancher de Mona passé. `abandoned` : Mona abandonne sa requête pendant l'attente, et
/// rien ne change.
async fn the_instance_place_waits_for_the_floor(abandoned: bool) {
    let t = app().await;
    let team = Team::new(&t).await;
    t.indexer.settled().await;
    let held: Vec<_> = (1..MAX_SEARCHES).map(|_| t.searches.hold()).collect();
    let clock = StoppedClock::start();
    let start = tokio::time::Instant::now();
    let mona = asking(&t, &team.mona, team.space, "un");
    until("la réponse de Mona prête", || {
        t.searches.answers_ready() == 1
    })
    .await;
    if abandoned {
        mona.abort();
        settle().await;
    }
    let tom = asking(&t, &team.tom, team.space, "deux");
    until("Tom attend", || t.searches.counts() == (MAX_SEARCHES, 1)).await;
    settle().await;
    assert_eq!(t.searches.started(), 1, "Tom n'a pas commencé");
    assert_eq!(
        tokio::time::Instant::now(),
        start,
        "l'horloge n'a pas bougé"
    );
    pass_the_floor().await;
    until("la réponse de Tom prête", || {
        t.searches.answers_ready() == 2
    })
    .await;
    assert_eq!(t.searches.started(), 2);
    if !abandoned {
        assert_eq!(mona.await.unwrap().0, StatusCode::OK);
    }
    pass_the_floor().await;
    assert_eq!(tom.await.unwrap().0, StatusCode::OK);
    drop(clock);
    drop(held);
}

#[tokio::test]
async fn the_instance_keeps_its_place_until_the_answer_leaves() {
    the_instance_place_waits_for_the_floor(false).await;
}

#[tokio::test]
async fn an_abandoned_search_keeps_the_place_of_the_instance_until_the_floor() {
    the_instance_place_waits_for_the_floor(true).await;
}

/// Une recherche sans mot, un refus et un `429` ne lisent pas l'index et n'attendent pas le
/// plancher : ils répondent sans que l'horloge bouge.
#[tokio::test]
async fn an_empty_query_and_a_refusal_do_not_wait_for_the_floor() {
    let t = app().await;
    let team = Team::new(&t).await;
    t.indexer.settled().await;
    let gate = Gate::on(&t);
    let clock = StoppedClock::start();
    let start = tokio::time::Instant::now();
    assert_eq!(found(&team.mona, team.space, "  ").await, nothing());
    search(&team.sam, team.space, "mot")
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    search(&team.mona, team.space, &"a".repeat(201))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_query");
    // Ni la recherche vide, ni l'étranger, ni la recherche trop longue n'ont rien calculé.
    assert_eq!((t.searches.started(), t.searches.sets_built()), (0, 0));
    let first = asking(&t, &team.mona, team.space, "un");
    until("le premier calcul", || t.searches.started() == 1).await;
    let second = asking(&t, &team.mona, team.space, "deux");
    until("la deuxième attend", || t.searches.counts() == (1, 1)).await;
    let third = asking(&t, &team.mona, team.space, "trois");
    assert_eq!(second.await.unwrap().0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        tokio::time::Instant::now(),
        start,
        "l'horloge n'a pas bougé"
    );

    drop(gate);
    until("la première réponse prête", || {
        t.searches.answers_ready() == 1
    })
    .await;
    pass_the_floor().await;
    assert_eq!(first.await.unwrap().0, StatusCode::OK);
    until("la dernière réponse prête", || {
        t.searches.answers_ready() == 2
    })
    .await;
    pass_the_floor().await;
    assert_eq!(third.await.unwrap().0, StatusCode::OK);
    drop(clock);
}

/// Les limites connues de la recherche devant les droits (Task X1). Chaque test relève les
/// exigences manquées et les donne à `known_limit` avec la perte dite : vert tant que cette perte
/// se produit, rouge le jour où elle est levée (le sortir alors de ce module, et lui faire
/// affirmer le comportement voulu) et pour toute autre issue.
mod known_limits {
    use super::limit::known_limit;
    use super::*;

    /// Voulu : un texte retiré d'une page avant d'y inviter quelqu'un n'est jamais trouvé par ce
    /// nouveau lecteur. Aujourd'hui (limite L-a du plan, R906) : l'index suit l'écriture après un
    /// temps de calme (2 à 15 s) ; d'ici là, la recherche du nouveau lecteur rend la page pour le
    /// mot retiré, avec l'extrait qui le porte. L'indexeur est figé pour tenir cette fenêtre
    /// ouverte ; `erased_text_leaves_the_index` (S4b) prouve qu'elle se referme.
    #[tokio::test]
    async fn a_text_removed_just_before_inviting_is_found_by_the_new_reader() {
        let t = app().await;
        let team = Team::new(&t).await;
        let page = team
            .page(None, "Dossier", &[user(&team.olga, Level::Full)])
            .await;
        write(&team.olga, page, "Le tatou reste entre nous.\n").await;
        t.indexer.settled().await;
        // Préconditions : sa propriétaire la trouve, Mona non.
        assert_eq!(pages_found(&team.olga, team.space, "tatou").await, [page]);
        assert_eq!(found(&team.mona, team.space, "tatou").await, nothing());

        t.indexer.freeze();
        write(&team.olga, page, "Un texte pour tous.\n").await;
        team.olga
            .browser
            .put(
                &format!("/api/v1/pages/{page}/access/users/{}", team.mona.id),
                json!({ "level": "read" }),
            )
            .await
            .assert_status(StatusCode::OK);
        // Ce que Mona lit de la page ne porte plus le mot.
        let read = team
            .mona
            .browser
            .get(&format!("/api/v1/pages/{page}/mmd"))
            .await;
        read.assert_status(StatusCode::OK);
        assert_eq!(read.text(), "Un texte pour tous.\n");

        let answer = found(&team.mona, team.space, "tatou").await;
        let mut unmet = Vec::new();
        if pages(&answer) == [page] {
            unmet.push("la page est trouvée par le mot retiré".to_string());
        }
        if answer.to_string().contains("tatou reste entre nous") {
            unmet.push("l'extrait porte le texte retiré".to_string());
        }
        let loss = vec![
            "la page est trouvée par le mot retiré".to_string(),
            "l'extrait porte le texte retiré".to_string(),
        ];
        known_limit(
            "L-a : l'index suit une écriture après un temps de calme, et les droits sont jugés \
             à la requête sur la page, pas sur le texte indexé. La lever demande de réindexer \
             une page avant qu'un accès neuf y soit servi, ou de vérifier un extrait contre le \
             document courant",
            &unmet,
            &loss,
        );
    }
}
