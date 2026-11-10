//! La synchro sous droits : `/sync/{id}` demande une session et le droit de lire la page ;
//! elle n'écrit qu'avec le droit de modifier, hors corbeille ; et une connexion ouverte est
//! fermée dès que les droits de sa page changent ou que la session de son compte se ferme.
//! Elle revient alors par la poignée de main, qui relit tout.
//!
//! Deux tests de la même série sont dans `src/routes/sync.rs`, parce qu'ils regardent ce
//! qu'on ne voit pas d'ici : `revoked_access_closes_live_connection` (`rights_changed`, que la
//! route de partage appellera) et `deleted_page_actor_is_forgotten` (les acteurs vivants).
//!
//! Les aides (application, équipe, navigateurs, WebSocket avec session) sont dans
//! `common/mod.rs`.

mod common;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::{
    Account, Browser, PASSWORD, Team, TestApp, WAIT, exec_sql, handshake, json_request, outcome,
    query_i64, sync_as, sync_url, synced_as,
};
use mementum_acl::{Level, Subject};
use mementum_crdt::PageDoc;
use mementum_schema::SCHEMA_VERSION;
use mementum_server::SignupMode;
use mementum_store::Store;
use mementum_sync::testkit::Client;
use serde_json::json;
use sqlx::{Connection, Row};
use tower::ServiceExt;
use uuid::Uuid;

/// Délai dans lequel une connexion doit être fermée quand ses droits changent (Review
/// Focus 1).
const CLOSES_WITHIN: Duration = Duration::from_secs(2);

/// Une application où tout le monde peut s'inscrire : les comptes de ces tests reçoivent leur
/// rôle par le store.
async fn open_app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

fn members(level: Level) -> (Subject, Level) {
    (Subject::Members, level)
}

fn user(account: &Account<'_>, level: Level) -> (Subject, Level) {
    (Subject::User(account.id), level)
}

fn member_uri(space: Uuid, account: &Account<'_>) -> String {
    format!("/api/v1/spaces/{space}/members/{}", account.id)
}

/// Le texte de chaque bloc de premier niveau du document du client, une ligne par bloc.
fn text(client: &Client) -> String {
    client
        .blocks()
        .iter()
        .map(mementum_md::block_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tape `typed` à la fin du premier bloc, puis attend que le serveur ait traité tout ce que
/// la connexion a envoyé : il traite ses messages dans l'ordre, et répond ensuite à la
/// resynchronisation.
async fn types(client: &Client, typed: &str) {
    let block = client.blocks()[0].id.clone();
    let start = client.step2_count();
    client.type_text(&block, typed);
    client.resync();
    assert!(
        client.wait_until(WAIT, |c| c.step2_count() > start).await,
        "le serveur n'a pas traité « {typed} »"
    );
}

/// Tape `typed` à la fin du premier bloc sans rien attendre : pour une connexion que le
/// serveur vient de fermer, et qui ne le sait peut-être pas encore.
fn types_unheard(client: &Client, typed: &str) {
    let block = client.blocks()[0].id.clone();
    client.type_text(&block, typed);
}

/// Le serveur ferme la connexion, en moins de deux secondes.
async fn closed(client: &Client) -> bool {
    client.wait_until(CLOSES_WITHIN, |c| c.is_closed()).await
}

/// La connexion est encore servie : une resynchronisation reçoit sa réponse.
async fn still_served(client: &Client) -> bool {
    let start = client.step2_count();
    client.resync();
    client.wait_until(WAIT, |c| c.step2_count() > start).await && !client.is_closed()
}

/// Le statut du refus d'une connexion de synchro demandée avec la session de `by` (sans
/// cookie s'il n'en porte pas).
async fn refused(by: &Browser<'_>, url: &str) -> u16 {
    match handshake(url, None, by.cookie().as_deref()).await {
        Ok(()) => panic!("connexion de synchro acceptée : {url}"),
        Err(status) => status,
    }
}

/// `GET /sync/{id}` sans poignée de main WebSocket, avec la session de `by` : la réponse
/// entière, pour comparer deux refus.
async fn plain_get(by: &Browser<'_>, id: impl std::fmt::Display) -> common::Reply {
    by.get(&format!("/sync/{id}?schema={SCHEMA_VERSION}")).await
}

/// Le MMD de la page, lu par `by`.
async fn mmd(by: &Account<'_>, page: Uuid) -> String {
    let reply = by.get_mmd(&page.to_string()).await;
    reply.assert_status(StatusCode::OK);
    reply.text()
}

/// Le nombre de mises à jour au journal de la page.
async fn updates(t: &TestApp, page: Uuid) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM doc_updates WHERE node_id = '{page}'");
    query_i64(&t.db_path(), &sql).await
}

/// Le compte (`actor_id`) de chaque mise à jour au journal de la page, dans l'ordre des
/// `seq`.
async fn actors(db: &Path, page: Uuid) -> Vec<Option<Uuid>> {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    let actors = sqlx::query("SELECT actor_id FROM doc_updates WHERE node_id = ? ORDER BY seq")
        .bind(page.to_string())
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|row| {
            row.get::<Option<String>, _>(0)
                .map(|id| Uuid::parse_str(&id).unwrap())
        })
        .collect();
    pool.close().await;
    actors
}

/// Sans session, `/sync` ne dit rien d'une page : `401`, qu'elle existe ou non. Le contrôle
/// de l'`Origin` passe avant celui de la session.
#[tokio::test]
async fn sync_requires_session() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({ "title": "Page" })).await;
    let url = sync_url(addr, &page.id);
    let nobody = t.browser();

    assert_eq!(refused(&nobody, &url).await, 401);
    // L'erreur JSON habituelle, la même pour une page qui n'existe pas.
    let reply = plain_get(&nobody, &page.id).await;
    reply.assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    for other in [Uuid::now_v7().to_string(), "pas-un-uuid".to_string()] {
        assert_eq!(
            outcome(&plain_get(&nobody, &other).await),
            outcome(&reply),
            "{other}"
        );
        assert_eq!(refused(&nobody, &sync_url(addr, &other)).await, 401);
    }

    // Un cookie qui n'est pas un jeton, un jeton qui n'ouvre aucune session, le jeton d'une
    // session fermée.
    let garbled = t.browser();
    garbled.set_session(Some("pas-un-jeton"));
    let unknown = t.browser();
    unknown.set_session(Some(&"A".repeat(43)));
    let second = t.browser();
    second
        .log_in("alice@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let logged_out = t.browser();
    logged_out.set_session(second.session().as_deref());
    assert_eq!(
        handshake(&url, None, logged_out.cookie().as_deref()).await,
        Ok(())
    );
    second.log_out().await.assert_status(StatusCode::NO_CONTENT);
    for (browser, what) in [
        (&garbled, "cookie illisible"),
        (&unknown, "jeton inconnu"),
        (&logged_out, "session fermée"),
    ] {
        assert_eq!(refused(browser, &url).await, 401, "{what}");
        assert_eq!(
            outcome(&plain_get(browser, &page.id).await),
            outcome(&reply),
            "{what}"
        );
    }

    // L'`Origin` d'abord : celle d'un autre site est refusée, avec ou sans session.
    for cookie in [None, alice.browser.cookie()] {
        assert_eq!(
            handshake(&url, Some("http://evil.example"), cookie.as_deref()).await,
            Err(403)
        );
    }

    // Avec sa session, la page s'ouvre.
    let client = synced_as(&alice.browser, &url).await;
    assert_eq!(client.blocks().len(), 1, "la racine écrite par le serveur");
}

/// Review Focus 3 : une page qu'on ne lit pas répond comme une page qui n'existe pas, même
/// statut, même corps : page d'un autre espace, page privée d'un autre membre du même espace,
/// id inconnu.
#[tokio::test]
async fn sync_on_unreadable_page_is_404() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let foreign = team.zoe.create(json!({ "title": "Chez Zoé" })).await;
    let private = team
        .page(None, "Privée de Tom", &[user(&team.tom, Level::Full)])
        .await;
    let readable = team.page(None, "Commune", &[members(Level::Read)]).await;
    let snapshots = async |page: Uuid| {
        let sql = format!("SELECT COUNT(*) FROM doc_snapshots WHERE node_id = '{page}'");
        query_i64(&t.db_path(), &sql).await
    };

    let mona = &team.mona.browser;
    let missing = plain_get(mona, Uuid::now_v7()).await;
    missing.assert_error(StatusCode::NOT_FOUND, "not_found");
    for (what, id) in [
        ("page d'un autre espace", foreign.id.clone()),
        ("page privée d'un autre membre", private.to_string()),
        ("id inconnu", Uuid::now_v7().to_string()),
        ("id illisible", "pas-un-uuid".to_string()),
    ] {
        assert_eq!(refused(mona, &sync_url(addr, &id)).await, 404, "{what}");
        assert_eq!(
            outcome(&plain_get(mona, &id).await),
            outcome(&missing),
            "{what}"
        );
    }
    // La propriétaire de l'espace ne lit pas la page privée d'un membre ; l'admin de
    // l'instance, qui n'a aucun rôle dans l'espace, non plus.
    for (who, account) in [("Olga", &team.olga), ("Sam", &team.sam)] {
        let url = sync_url(addr, private);
        assert_eq!(refused(&account.browser, &url).await, 404, "{who}");
        assert_eq!(
            outcome(&plain_get(&account.browser, private).await),
            outcome(&missing),
            "{who}"
        );
    }
    // Aucun de ces refus n'a lancé l'acteur de la page : il aurait écrit son document.
    assert_eq!(snapshots(private).await, 0);

    // Qui lit la page l'ouvre.
    let tom = synced_as(&team.tom.browser, &sync_url(addr, private)).await;
    assert_eq!(tom.blocks().len(), 1);
    assert_eq!(snapshots(private).await, 1);
    synced_as(mona, &sync_url(addr, readable)).await;
    synced_as(&team.zoe.browser, &sync_url(addr, &foreign.id)).await;
}

/// Au niveau `read` (ou `comment`, qui le vaut au lot 2), une connexion reçoit ce que les
/// autres écrivent ; ce qu'elle envoie n'est ni écrit ni diffusé.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reader_connection_is_read_only() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    for level in [Level::Read, Level::Comment] {
        let grants = [members(Level::Edit), user(&team.gina, level)];
        let page = team.page(None, "À lire", &grants).await;
        let url = sync_url(addr, page);
        let writer = synced_as(&team.mona.browser, &url).await;
        let reader = synced_as(&team.gina.browser, &url).await;
        types(&writer, "Bonjour").await;
        assert!(
            reader.wait_until(WAIT, |c| text(c) == "Bonjour").await,
            "{level:?} : il reçoit ce que les autres écrivent"
        );
        let written = updates(&t, page).await;

        // Ce qu'il envoie n'est pas écrit…
        types(&reader, " ZZZ").await;
        assert_eq!(updates(&t, page).await, written, "{level:?} : rien d'écrit");
        assert_eq!(mmd(&team.mona, page).await, "Bonjour\n");
        // … ni diffusé : l'autre connexion reçoit les diffusions dans l'ordre, et celle qui
        // suit ne lui a pas apporté son texte.
        types(&writer, " !").await;
        assert!(
            reader.wait_until(WAIT, |c| text(c).contains('!')).await,
            "{level:?} : il reçoit toujours"
        );
        assert_eq!(text(&writer), "Bonjour !");
        assert_eq!(mmd(&team.mona, page).await, "Bonjour !\n");
        assert!(!reader.is_closed(), "{level:?} : il reste connecté");
    }
}

/// Ce qui est écrit porte son auteur : le compte de la connexion pour une frappe, celui de
/// la session pour `PUT /mmd`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_are_attributed_to_the_account() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let grants = [members(Level::Edit), user(&team.gina, Level::Read)];
    let page = team.page(None, "Attribuée", &grants).await;
    let url = sync_url(addr, page);

    let mona = synced_as(&team.mona.browser, &url).await;
    types(&mona, "de Mona").await;
    team.tom
        .put_mmd(&page.to_string(), "de Tom\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let tom = synced_as(&team.tom.browser, &url).await;
    types(&tom, " encore").await;
    // Une lectrice n'écrit rien : rien ne lui est attribué.
    let gina = synced_as(&team.gina.browser, &url).await;
    types(&gina, " jamais").await;

    assert_eq!(
        actors(&t.db_path(), page).await,
        [Some(team.mona.id), Some(team.tom.id), Some(team.tom.id)]
    );
    assert_eq!(mmd(&team.olga, page).await, "de Tom encore\n");
}

/// Review Focus 5 : deux connexions ouvertes sur une sous-page ; le parent part à la
/// corbeille. Elles sont fermées, reviennent, et plus personne n'écrit ; après la
/// restauration, elles sont fermées de nouveau, reviennent, et l'écriture repasse.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trashed_page_is_read_only_for_everyone() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let parent = team.page(None, "Parent", &[members(Level::Full)]).await;
    let child = team.page(Some(parent), "Enfant", &[]).await;
    let url = sync_url(addr, child);
    let mona = synced_as(&team.mona.browser, &url).await;
    let tom = synced_as(&team.tom.browser, &url).await;
    types(&mona, "avant").await;
    assert!(tom.wait_until(WAIT, |c| text(c) == "avant").await);

    team.olga
        .browser
        .post_empty(&format!("/api/v1/pages/{parent}/trash"))
        .await
        .assert_status(StatusCode::OK);
    // Au retour de la route, plus rien de ce qu'elles envoient n'est traité.
    types_unheard(&mona, " trop tard");
    assert!(closed(&mona).await, "Mona est fermée");
    assert!(closed(&tom).await, "Tom aussi");

    // Elles reviennent : la page se lit encore, et ne s'écrit plus, pas même par la
    // propriétaire de l'espace.
    let written = updates(&t, child).await;
    let mona = synced_as(&team.mona.browser, &url).await;
    let tom = synced_as(&team.tom.browser, &url).await;
    let olga = synced_as(&team.olga.browser, &url).await;
    assert_eq!(text(&mona), "avant");
    for (client, typed) in [(&mona, " M"), (&tom, " T"), (&olga, " O")] {
        types(client, typed).await;
    }
    assert_eq!(updates(&t, child).await, written, "plus rien n'est écrit");
    assert_eq!(mmd(&team.olga, child).await, "avant\n");
    // Rien n'est diffusé non plus : chacune n'a que son propre texte.
    assert!(still_served(&tom).await);
    assert_eq!(text(&tom), "avant T");

    team.olga
        .browser
        .post_empty(&format!("/api/v1/pages/{parent}/restore"))
        .await
        .assert_status(StatusCode::OK);
    for (client, who) in [(&mona, "Mona"), (&tom, "Tom"), (&olga, "Olga")] {
        assert!(closed(client).await, "{who} est fermée à la restauration");
    }
    let mona = synced_as(&team.mona.browser, &url).await;
    let tom = synced_as(&team.tom.browser, &url).await;
    types(&mona, " après").await;
    assert!(tom.wait_until(WAIT, |c| text(c) == "avant après").await);
    assert_eq!(updates(&t, child).await, written + 1);
    assert_eq!(mmd(&team.olga, child).await, "avant après\n");
}

/// Une page déplacée sous un parent que B ne voit pas : B ne la tenait que de son ancien
/// parent. Sa connexion est fermée, et sa reconnexion reçoit `404`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn moved_page_follows_its_new_parent() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let shared = team.page(None, "Commune", &[members(Level::Edit)]).await;
    let page = team.page(Some(shared), "Page", &[]).await;
    let hidden = team
        .page(None, "Privée d'Olga", &[user(&team.olga, Level::Full)])
        .await;
    let url = sync_url(addr, page);
    let mona = synced_as(&team.mona.browser, &url).await;
    let olga = synced_as(&team.olga.browser, &url).await;
    types(&mona, "avant").await;

    team.olga
        .patched(&page.to_string(), json!({ "parentId": hidden.to_string() }))
        .await;
    types_unheard(&mona, " trop tard");
    assert!(closed(&mona).await, "Mona est fermée");
    assert_eq!(refused(&team.mona.browser, &url).await, 404);
    assert_eq!(
        outcome(&plain_get(&team.mona.browser, page).await),
        outcome(&plain_get(&team.mona.browser, Uuid::now_v7()).await)
    );

    // Olga, fermée avec toute la page, la rouvre sous son nouveau parent ; rien de ce que
    // Mona a tapé ensuite n'y est.
    assert!(closed(&olga).await);
    let olga = synced_as(&team.olga.browser, &url).await;
    assert_eq!(text(&olga), "avant");
    types(&olga, " après").await;
    assert_eq!(mmd(&team.olga, page).await, "avant après\n");
}

/// Un membre retiré de l'espace : ses connexions sont fermées, et il ne revient pas.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removed_member_is_kicked() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let page = team.page(None, "Commune", &[members(Level::Edit)]).await;
    let url = sync_url(addr, page);
    let tom = synced_as(&team.tom.browser, &url).await;
    let mona = synced_as(&team.mona.browser, &url).await;
    types(&tom, "avant").await;

    team.olga
        .browser
        .delete(&member_uri(team.space, &team.tom))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    types_unheard(&tom, " trop tard");
    assert!(closed(&tom).await, "Tom est fermé");
    assert_eq!(refused(&team.tom.browser, &url).await, 404);

    // Les droits de tout l'espace ont pu changer : Mona est fermée aussi, revient, et écrit.
    assert!(closed(&mona).await);
    let mona = synced_as(&team.mona.browser, &url).await;
    assert_eq!(text(&mona), "avant");
    types(&mona, " après").await;
    assert_eq!(mmd(&team.olga, page).await, "avant après\n");
}

/// Une membre passée invitée garde ce que lui donne sa propre ligne, et perd ce qu'elle ne
/// tenait que de la ligne des membres : l'écriture ici, la page entière là.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn demoted_member_loses_write_where_he_only_had_members_access() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let grants = [members(Level::Edit), user(&team.mona, Level::Read)];
    let own_line = team.page(None, "Avec sa ligne", &grants).await;
    let members_only = team
        .page(None, "Par les membres", &[members(Level::Edit)])
        .await;
    let (own_url, members_url) = (sync_url(addr, own_line), sync_url(addr, members_only));
    let here = synced_as(&team.mona.browser, &own_url).await;
    let there = synced_as(&team.mona.browser, &members_url).await;
    types(&here, "avant").await;
    types(&there, "avant").await;

    team.olga
        .browser
        .patch(
            &member_uri(team.space, &team.mona),
            json!({ "role": "guest" }),
        )
        .await
        .assert_status(StatusCode::OK);
    types_unheard(&here, " trop tard");
    types_unheard(&there, " trop tard");
    assert!(closed(&here).await && closed(&there).await);

    // Sa propre ligne lui laisse la lecture : elle revient, reçoit, et n'écrit plus.
    let written = updates(&t, own_line).await;
    let here = synced_as(&team.mona.browser, &own_url).await;
    assert_eq!(text(&here), "avant");
    types(&here, " perdu").await;
    assert_eq!(updates(&t, own_line).await, written, "rien d'écrit");
    let tom = synced_as(&team.tom.browser, &own_url).await;
    types(&tom, " !").await;
    assert!(here.wait_until(WAIT, |c| text(c).contains('!')).await);
    assert_eq!(text(&tom), "avant !");
    assert_eq!(mmd(&team.olga, own_line).await, "avant !\n");

    // Là où elle n'avait que la ligne des membres, la page n'existe plus pour elle.
    assert_eq!(refused(&team.mona.browser, &members_url).await, 404);
    assert_eq!(mmd(&team.olga, members_only).await, "avant\n");
}

/// Ruling R73 : une connexion ouverte ne survit pas à sa session. Après une déconnexion,
/// puis après un changement de mot de passe, les connexions du compte sont fermées, celles
/// des autres comptes non, et l'ancien cookie ne rouvre rien.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_and_password_change_close_the_account_connections() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let one = sync_url(addr, team.page(None, "Une", &[members(Level::Edit)]).await);
    let two = sync_url(addr, team.page(None, "Deux", &[members(Level::Edit)]).await);
    let other = synced_as(&team.tom.browser, &one).await;

    // Déconnexion : Mona a deux pages ouvertes, et une troisième connexion sur un autre
    // appareil, avec sa propre session.
    let phone = t.browser();
    phone
        .log_in("mona@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let first = synced_as(&team.mona.browser, &one).await;
    let second = synced_as(&team.mona.browser, &two).await;
    let on_phone = synced_as(&phone, &two).await;
    types(&first, "avant").await;
    let old = t.browser();
    old.set_session(team.mona.browser.session().as_deref());

    team.mona
        .browser
        .log_out()
        .await
        .assert_status(StatusCode::NO_CONTENT);
    types_unheard(&first, " trop tard");
    for (client, which) in [
        (&first, "la première page"),
        (&second, "la seconde page"),
        (&on_phone, "l'autre appareil"),
    ] {
        assert!(closed(client).await, "encore ouverte : {which}");
    }
    assert!(still_served(&other).await, "Tom n'est pas touché");
    // Sans session, et avec le cookie de la session fermée : `401`.
    assert_eq!(refused(&team.mona.browser, &one).await, 401);
    assert_eq!(refused(&old, &one).await, 401);
    // L'autre appareil a gardé sa session : il revient.
    let on_phone = synced_as(&phone, &one).await;
    assert_eq!(text(&on_phone), "avant");

    // Changement de mot de passe : toutes les sessions d'Adam sont fermées, et celle qui
    // l'a demandé repart avec un cookie neuf.
    let first = synced_as(&team.adam.browser, &one).await;
    let second = synced_as(&team.adam.browser, &two).await;
    let old = t.browser();
    old.set_session(team.adam.browser.session().as_deref());
    team.adam
        .browser
        .change_password(PASSWORD, "un-autre-mot-de-passe-de-test")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    types_unheard(&first, " trop tard");
    assert!(closed(&first).await, "encore ouverte : la première page");
    assert!(closed(&second).await, "encore ouverte : la seconde page");
    assert!(still_served(&other).await, "Tom n'est pas touché");
    assert!(still_served(&on_phone).await, "Mona non plus");
    assert_eq!(refused(&old, &one).await, 401);
    assert_eq!(refused(&t.browser(), &one).await, 401);
    // Avec le cookie neuf, il revient ; rien de ce qui a été tapé trop tard n'est là.
    let back = synced_as(&team.adam.browser, &one).await;
    assert_eq!(text(&back), "avant");
}

/// Ruling R71, conditions 3 et 4 : un retrait de membre et un changement de rôle incrémentent
/// `spaces.rev` dans leur transaction, et ne ferment les connexions qu'après le commit.
///
/// Le verrou d'écriture de SQLite est tenu par une connexion annexe : l'écriture de la route
/// attend, et tant qu'elle n'est pas validée, personne n'est fermé ; quand la connexion l'est,
/// la révision a déjà bougé.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rights_change_bumps_rev_before_the_kick() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let page = team.page(None, "Commune", &[members(Level::Edit)]).await;
    let url = sync_url(addr, page);
    let rev = async || team.store.space_rev(team.space).await.unwrap().unwrap();
    let member = member_uri(team.space, &team.tom);
    let changes = [
        (
            "le changement de rôle",
            json_request(Method::PATCH, &member, &json!({ "role": "admin" })),
            StatusCode::OK,
        ),
        (
            "le retrait du membre",
            Request::delete(&member).body(Body::empty()).unwrap(),
            StatusCode::NO_CONTENT,
        ),
    ];
    for (what, request, expected) in changes {
        let live = synced_as(&team.mona.browser, &url).await;
        let before = rev().await;
        let mut lock =
            sqlx::SqliteConnection::connect(&format!("sqlite:{}", t.db_path().display()))
                .await
                .unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut lock)
            .await
            .unwrap();
        let watch = async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert!(!live.is_closed(), "fermée avant le commit : {what}");
            assert_eq!(rev().await, before, "{what}");
            sqlx::query("ROLLBACK").execute(&mut lock).await.unwrap();
            assert!(closed(&live).await, "encore ouverte après {what}");
            assert!(
                rev().await > before,
                "connexion fermée avant que la révision ait bougé : {what}"
            );
        };
        let (reply, ()) = tokio::join!(team.olga.browser.send(request), watch);
        reply.assert_status(expected);
        assert_eq!(rev().await, before + 1, "{what}");
    }
}

/// Ruling R90 : se connecter par-dessus une session la ferme, avec les connexions de synchro
/// du compte qu'elle portait. Celles du compte qui se connecte, et des autres, ne sont pas
/// touchées ; une connexion ratée ne ferme rien.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_over_a_session_closes_the_previous_account_connections() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let page = team.page(None, "Commune", &[members(Level::Edit)]).await;
    let url = sync_url(addr, page);
    // Un navigateur partagé : Mona y est connectée, la page ouverte. Elle l'est aussi chez
    // elle ; Tom et Adam ont la page ouverte ailleurs.
    let shared = t.browser();
    shared
        .log_in("mona@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let mona = synced_as(&shared, &url).await;
    let mona_home = synced_as(&team.mona.browser, &url).await;
    let tom_elsewhere = synced_as(&team.tom.browser, &url).await;
    let adam = synced_as(&team.adam.browser, &url).await;
    types(&mona, "avant").await;
    let replaced = t.browser();
    replaced.set_session(shared.session().as_deref());

    // Un mot de passe faux par-dessus la session : rien n'est fermé, elle vaut toujours.
    shared
        .log_in("tom@exemple.fr", "pas-le-mot-de-passe-de-test")
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    assert!(still_served(&mona).await, "fermée par une connexion ratée");
    shared.me().await.assert_status(StatusCode::OK);

    // Tom se connecte par-dessus.
    shared
        .log_in("tom@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    types_unheard(&mona, " trop tard");
    assert!(
        closed(&mona).await,
        "la connexion de Mona est encore ouverte"
    );
    assert_eq!(refused(&replaced, &url).await, 401);
    assert!(still_served(&tom_elsewhere).await, "Tom est fermé");
    assert!(still_served(&adam).await, "Adam est fermé");
    // Toutes les connexions du compte remplacé sont fermées (Ruling R73) ; son autre
    // appareil, qui a gardé sa session, revient.
    assert!(closed(&mona_home).await);
    let mona_home = synced_as(&team.mona.browser, &url).await;
    assert_eq!(text(&mona_home), "avant");

    // Le navigateur porte maintenant la session de Tom : ce qu'il écrit est de Tom.
    let tom = synced_as(&shared, &url).await;
    types(&tom, " puis Tom").await;
    assert_eq!(
        actors(&t.db_path(), page).await,
        [Some(team.mona.id), Some(team.tom.id)]
    );
}

/// Ruling R90 : la déconnexion ferme les connexions du compte que la session supprimée
/// portait, sans dépendre d'une lecture faite avant : même quand la session est échue.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logout_with_an_expired_session_still_closes_its_connections() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let page = team.page(None, "Commune", &[members(Level::Edit)]).await;
    let url = sync_url(addr, page);
    let mona = synced_as(&team.mona.browser, &url).await;
    let tom = synced_as(&team.tom.browser, &url).await;

    // La session de Mona arrive à échéance, la page ouverte. Sa connexion lui survit (elle
    // n'est relue qu'à l'ouverture).
    let expire = format!(
        "UPDATE sessions SET expires_at = 1 WHERE user_id = '{}'",
        team.mona.id
    );
    exec_sql(&t.db_path(), &expire).await;
    team.mona
        .browser
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert!(still_served(&mona).await);

    let cookie = team.mona.browser.cookie();
    team.mona
        .browser
        .log_out()
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert!(
        closed(&mona).await,
        "la connexion de Mona est encore ouverte"
    );
    assert!(still_served(&tom).await, "Tom est fermé");
    let left = format!(
        "SELECT COUNT(*) FROM sessions WHERE user_id = '{}'",
        team.mona.id
    );
    assert_eq!(query_i64(&t.db_path(), &left).await, 0);
    assert_eq!(handshake(&url, None, cookie.as_deref()).await, Err(401));
}

/// Le centile `percent` de durées triées, en millisecondes.
fn centile(sorted: &[Duration], percent: usize) -> f64 {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank - 1].as_secs_f64() * 1000.0
}

/// L'attente d'un navigateur avant de revenir après `closures` fermetures de suite sans
/// synchro (y-websocket 3.1.0) : 200 ms après la première, le double après chaque suivante,
/// 2,5 s au plus.
fn browser_backoff(closures: usize) -> Duration {
    (1..=closures as u32)
        .map(|n| Duration::from_millis((100 * 2u64.pow(n)).min(2_500)))
        .sum()
}

/// Mesure, pas un test (elle n'affirme rien, elle affiche des lignes `MESURE`) : ce que
/// coûte le contrôle de la révision à l'inscription (Ruling R71) quand l'espace est écrit
/// pendant qu'une page s'ouvre. `spaces.rev` bouge pour toute écriture qui change l'arbre ou
/// les droits de l'espace, pas seulement les droits de cette page : une connexion qui
/// s'inscrit à ce moment est fermée, et revient. L'écriture mesurée est une création de page,
/// qui incrémente toujours `rev`.
///
/// `cargo test --release -p mementum-server --test sync_rights -- --ignored --nocapture`
///
/// Des pages de 1 000 blocs que personne n'a encore ouvertes (leur acteur n'est pas lancé),
/// ouvertes l'une après l'autre jusqu'à leur première synchro, vingt par série : dans un
/// espace au repos, puis pendant qu'une tâche y crée des pages sans s'arrêter, puis pendant
/// qu'elle en crée dix par seconde. Le client de test revient aussitôt ; un navigateur attend
/// d'abord ([`browser_backoff`]), ce que la seconde suite de chiffres ajoute.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, à lancer en --release avec --nocapture"]
async fn measure_page_open_while_the_space_is_written() {
    const PAGES: usize = 20;
    const SERIES: usize = 3;
    const BLOCKS: usize = 1_000;
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let store = Store::open(&t.db_path()).await.unwrap();
    let mut content = (1..=BLOCKS)
        .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
        .collect::<Vec<_>>()
        .join("\n\n");
    content.push('\n');
    let blocks = mementum_md::parse(&content).unwrap();
    // Le contenu est écrit par le store, à la suite de l'instantané de la page neuve : aucun
    // acteur n'est lancé.
    let mut cold = Vec::new();
    for i in 0..SERIES * PAGES {
        let page = alice
            .create(json!({ "title": format!("Froide {i}") }))
            .await;
        let id = Uuid::parse_str(&page.id).unwrap();
        let (_, snapshot) = store.load_doc(id).await.unwrap().snapshot.unwrap();
        let update = PageDoc::from_updates([snapshot.as_slice()])
            .unwrap()
            .write_blocks(&blocks)
            .unwrap();
        store.append_updates(id, &[&update], None).await.unwrap();
        cold.push(id);
    }
    store.close().await;

    // Ouvre la page jusqu'à sa première synchro : la durée, et le nombre de connexions
    // fermées avant d'avoir rien reçu.
    let open = async |id: Uuid| {
        let url = sync_url(addr, id);
        let start = Instant::now();
        let mut closed_at_join = 0;
        loop {
            let client = sync_as(&alice.browser, &url)
                .await
                .expect("poignée de main");
            assert!(
                client
                    .wait_until(WAIT, |c| c.is_synced() || c.is_closed())
                    .await
            );
            if client.is_synced() {
                assert_eq!(client.blocks().len(), BLOCKS);
                return (start.elapsed(), closed_at_join);
            }
            closed_at_join += 1;
        }
    };
    let report = |what: &str, opened: &[(Duration, usize)]| {
        let mut times: Vec<Duration> = opened.iter().map(|(time, _)| *time).collect();
        times.sort();
        let closed: usize = opened.iter().map(|(_, closed)| closed).sum();
        let reopened = opened.iter().filter(|(_, closed)| *closed > 0).count();
        let worst = opened.iter().map(|(_, closed)| *closed).max().unwrap_or(0);
        let mut in_browser: Vec<Duration> = opened
            .iter()
            .map(|(time, closed)| *time + browser_backoff(*closed))
            .collect();
        in_browser.sort();
        println!(
            "MESURE ouverture d'une page de {BLOCKS} blocs jamais ouverte, {what} : {} \
             ouvertures, {closed} connexions fermées à l'inscription ({reopened} ouvertures \
             touchées, {worst} fermetures de suite au pire) ; jusqu'à la première synchro : p50 \
             {:.1} ms, p95 {:.1} ms, max {:.1} ms ; avec l'attente d'un navigateur avant de \
             revenir : p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms",
            opened.len(),
            centile(&times, 50),
            centile(&times, 95),
            centile(&times, 100),
            centile(&in_browser, 50),
            centile(&in_browser, 95),
            centile(&in_browser, 100),
        );
    };

    let mut at_rest = Vec::new();
    for id in &cold[..PAGES] {
        at_rest.push(open(*id).await);
    }
    report("espace au repos", &at_rest);

    // Une tâche crée des pages dans le même espace, l'une après l'autre : sans pause, puis
    // dix par seconde.
    let series = [
        ("espace écrit sans arrêt", None),
        (
            "dix pages créées par seconde",
            Some(Duration::from_millis(100)),
        ),
    ];
    for (n, (what, pause)) in series.into_iter().enumerate() {
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let router = t.router.clone();
            let cookie = alice.browser.cookie().unwrap();
            let body = json!({ "spaceId": alice.space.to_string(), "title": "Bruit" }).to_string();
            let stop = stop.clone();
            tokio::spawn(async move {
                let mut created = 0u32;
                while !stop.load(Ordering::Relaxed) {
                    let request = Request::post("/api/v1/pages")
                        .header(header::HOST, "localhost")
                        .header(header::COOKIE, &cookie)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body.clone()))
                        .unwrap();
                    let response = router.clone().oneshot(request).await.unwrap();
                    assert_eq!(response.status(), StatusCode::CREATED);
                    created += 1;
                    if let Some(pause) = pause {
                        tokio::time::sleep(pause).await;
                    }
                }
                created
            })
        };
        let started = Instant::now();
        let mut opened = Vec::new();
        for id in &cold[(n + 1) * PAGES..(n + 2) * PAGES] {
            opened.push(open(*id).await);
            if let Some(pause) = pause {
                // Les ouvertures s'étalent sur plusieurs écritures.
                tokio::time::sleep(pause / 3).await;
            }
        }
        let elapsed = started.elapsed();
        stop.store(true, Ordering::Relaxed);
        let created = writer.await.unwrap();
        report(what, &opened);
        println!(
            "MESURE pendant ces ouvertures ({what}) : {created} pages créées en {:.0} ms, soit \
             une écriture qui incrémente `rev` toutes les {:.1} ms",
            elapsed.as_secs_f64() * 1000.0,
            elapsed.as_secs_f64() * 1000.0 / f64::from(created.max(1)),
        );
    }
}
