//! La synchro sous droits : `/sync/{id}` demande une session et le droit de lire la page ;
//! elle n'écrit qu'avec le droit de modifier, hors corbeille ; et une connexion ouverte est
//! fermée dès que les droits de sa page changent ou que la session de son compte se ferme.
//! Elle revient alors par la poignée de main, qui relit tout.
//!
//! Ce qui change pendant qu'une connexion s'ouvre est couvert aussi : les droits lus par la
//! route le sont après le chargement de la page (Ruling R89), et la session est relue une fois
//! la connexion inscrite (Ruling R88). Ces tests tiennent le verrou d'écriture de SQLite pour
//! arrêter la route au milieu de son travail ([`WriteLock`]).
//!
//! Un test de la même série est dans `src/routes/sync.rs`, parce qu'il regarde ce qu'on ne
//! voit pas d'ici : `deleted_page_actor_is_forgotten` (les acteurs vivants du hub).
//!
//! Les aides sont dans `common/mod.rs` (application, équipe, navigateurs, WebSocket avec
//! session) et dans `common/sync.rs` (ce qu'un client tape et reçoit).

mod common;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::sync::{closed, members, open_app, still_served, text, types, types_unheard, user};
use common::{
    Account, Browser, HELD, PASSWORD, Team, TestApp, WAIT, exec_sql, handshake, json_request,
    outcome, query_i64, sync_as, sync_url, synced_as,
};
use mementum_acl::{Level, Subject};
use mementum_crdt::PageDoc;
use mementum_schema::SCHEMA_VERSION;
use mementum_store::Store;
use mementum_sync::testkit::{Client, awareness_message};
use serde_json::json;
use sqlx::{Connection, Row};
use tower::ServiceExt;
use uuid::Uuid;

fn member_uri(space: Uuid, account: &Account<'_>) -> String {
    format!("/api/v1/spaces/{space}/members/{}", account.id)
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
/// Deux preuves, pour chaque changement.
///
/// 1. Une écriture qui n'est pas validée ne ferme personne. La base refuse l'écriture de la
///    route (une panne posée sur `members`) : `500`, la connexion est encore servie, et la
///    révision n'a pas bougé. Aucune attente ici : si la fermeture venait avant le commit,
///    elle serait faite, et le test le voit à coup sûr.
/// 2. Le verrou d'écriture de SQLite est tenu par une connexion annexe : l'écriture de la route
///    attend, et tant qu'elle n'est pas validée, personne n'est fermé ; quand la connexion
///    l'est, la révision a déjà bougé. Rien ne dit de l'extérieur que la route attend le
///    verrou : le test lui laisse `HELD`, comme ses voisins. Trop courte sur un poste très
///    chargé, cette attente rendrait ses deux premières affirmations vraies d'office ; elle
///    ne peut pas pour autant faire passer le test à tort, puisque la première preuve n'en
///    dépend pas.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rights_change_bumps_rev_before_the_kick() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let page = team.page(None, "Commune", &[members(Level::Edit)]).await;
    let url = sync_url(addr, page);
    let rev = async || team.store.space_rev(team.space).await.unwrap().unwrap();
    let member = member_uri(team.space, &team.tom);
    // `(nom, ce que la route écrit dans `members`, le statut quand elle passe)`.
    let changes = [
        ("le changement de rôle", "UPDATE", StatusCode::OK),
        ("le retrait du membre", "DELETE", StatusCode::NO_CONTENT),
    ];
    for (what, event, expected) in changes {
        // La requête du changement : chaque preuve envoie la sienne.
        let request = || match event {
            "UPDATE" => json_request(Method::PATCH, &member, &json!({ "role": "admin" })),
            _ => Request::delete(&member).body(Body::empty()).unwrap(),
        };
        let live = synced_as(&team.mona.browser, &url).await;
        let before = rev().await;

        // 1. L'écriture échoue dans sa transaction : rien n'est validé, personne n'est fermé.
        let failure = format!(
            "CREATE TRIGGER panne BEFORE {event} ON members \
             BEGIN SELECT RAISE(ABORT, 'panne simulée'); END"
        );
        exec_sql(&t.db_path(), &failure).await;
        let reply = team.olga.browser.send(request()).await;
        reply.assert_error(StatusCode::INTERNAL_SERVER_ERROR, "internal");
        exec_sql(&t.db_path(), "DROP TRIGGER panne").await;
        assert!(
            still_served(&live).await,
            "fermée par une écriture qui n'a pas été validée : {what}"
        );
        assert_eq!(rev().await, before, "{what}");

        // 2. L'écriture attend le verrou, puis passe.
        let lock = WriteLock::take(&t).await;
        let watch = async {
            // Le temps que la route lise les rôles et vienne attendre le verrou.
            tokio::time::sleep(HELD).await;
            assert!(!live.is_closed(), "fermée avant le commit : {what}");
            assert_eq!(rev().await, before, "{what}");
            lock.commit().await;
            assert!(closed(&live).await, "encore ouverte après {what}");
            assert!(
                rev().await > before,
                "connexion fermée avant que la révision ait bougé : {what}"
            );
        };
        let (reply, ()) = tokio::join!(team.olga.browser.send(request()), watch);
        reply.assert_status(expected);
        assert_eq!(rev().await, before + 1, "{what}");
    }
}

/// Le verrou d'écriture de SQLite, tenu par une connexion annexe. Tant qu'il l'est, rien ne
/// s'écrit dans la base, et une route qui doit écrire attend (cinq secondes au plus) : c'est
/// ce qui arrête, à un endroit connu, le chargement d'une page sans document, dont l'acteur
/// écrit le sien à son premier chargement.
///
/// Ce que [`WriteLock::run`] exécute part au [`WriteLock::commit`], en une transaction, avec
/// le verrou : comme une écriture du store validée pendant que la route attend.
struct WriteLock(sqlx::SqliteConnection);

impl WriteLock {
    async fn take(t: &TestApp) -> WriteLock {
        let url = format!("sqlite:{}", t.db_path().display());
        let mut connection = sqlx::SqliteConnection::connect(&url).await.unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut connection)
            .await
            .unwrap();
        WriteLock(connection)
    }

    async fn run(&mut self, sql: &str) {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
            .execute(&mut self.0)
            .await
            .unwrap_or_else(|error| panic!("{sql} : {error}"));
    }

    async fn commit(mut self) {
        self.run("COMMIT").await;
    }
}

/// Ce qu'une écriture de droits change en base, en SQL, pour [`WriteLock::run`] : `{page}` et
/// `{who}` y sont remplacés par la page et par le sujet (`user:<id>`) de la personne. La
/// révision de l'espace est incrémentée avec, comme le fait le store dans sa transaction.
const DEMOTED_TO_READ: &str =
    "UPDATE acl SET level = 'read' WHERE node_id = '{page}' AND subject = '{who}'";
const TRASHED: &str = "UPDATE nodes SET trashed_at = 1 WHERE id = '{page}'";
const ACCESS_REMOVED: &str = "DELETE FROM acl WHERE node_id = '{page}' AND subject = '{who}'";
/// La page supprimée pour de bon : ses accès partent avec elle (`ON DELETE CASCADE`).
const DELETED: &str = "DELETE FROM nodes WHERE id = '{page}'";

/// Écrit `change` (un des modèles ci-dessus) pour cette page et cette personne, et incrémente
/// la révision de l'espace, dans la transaction du verrou.
async fn write_rights_change(
    lock: &mut WriteLock,
    change: &str,
    space: Uuid,
    page: Uuid,
    who: &Account<'_>,
) {
    let change = change
        .replace("{page}", &page.to_string())
        .replace("{who}", &Subject::User(who.id).to_string());
    lock.run(&change).await;
    lock.run(&format!(
        "UPDATE spaces SET rev = rev + 1 WHERE id = '{space}'"
    ))
    .await;
}

/// Ce que devient une connexion ouverte pendant un changement de droits.
#[derive(Clone, Copy)]
enum Served {
    Writing,
    ReadOnly,
    Closed,
}

/// Ruling R89 : la route charge la page, **puis** lit la révision et les droits ; la connexion
/// ne tient que de cette seconde lecture. Un changement de droits validé pendant le chargement
/// est donc celui que la connexion reçoit : rétrogradée ou mise à la corbeille, elle est
/// servie en lecture seule, sans être fermée ; devenue illisible, elle est fermée sans avoir
/// rien reçu.
///
/// Le piège : la révision lue après le chargement est déjà celle d'après le changement, et la
/// relecture d'après inscription ne voit rien bouger. Si `can_write` venait du contrôle fait
/// avant la poignée de main, la connexion écrirait.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rights_changed_while_the_page_loads_are_the_ones_served() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let cases = [
        ("rien ne change", None, Served::Writing),
        (
            "rétrogradée à la lecture",
            Some(DEMOTED_TO_READ),
            Served::ReadOnly,
        ),
        ("mise à la corbeille", Some(TRASHED), Served::ReadOnly),
        ("accès retiré", Some(ACCESS_REMOVED), Served::Closed),
        // Supprimée pendant que son acteur attend pour écrire son premier document : il ne le
        // peut plus, et la connexion est fermée sans avoir rien reçu.
        ("page supprimée", Some(DELETED), Served::Closed),
    ];
    for (what, change, expected) in cases {
        // Une page que personne n'a ouverte, et qui n'a pas encore de document.
        let grants = [user(&team.olga, Level::Full), user(&team.gina, Level::Edit)];
        let page = team.page(None, what, &grants).await;
        let mut lock = WriteLock::take(&t).await;
        // La poignée de main aboutit : le premier contrôle a lu les droits d'avant. Le
        // chargement de la page attend le verrou.
        let client = sync_as(&team.gina.browser, &sync_url(addr, page))
            .await
            .unwrap_or_else(|refusal| panic!("{what} : {refusal}"));
        if let Some(change) = change {
            write_rights_change(&mut lock, change, team.space, page, &team.gina).await;
        }
        lock.commit().await;

        match expected {
            Served::Closed => {
                assert!(closed(&client).await, "{what} : encore ouverte");
                assert!(!client.is_synced(), "{what} : elle a reçu la page");
                assert_eq!(updates(&t, page).await, 0, "{what}");
            }
            Served::Writing | Served::ReadOnly => {
                assert!(
                    client.wait_until(WAIT, |c| c.is_synced()).await,
                    "{what} : la connexion n'est pas servie"
                );
                types(&client, "tapé").await;
                let written = matches!(expected, Served::Writing);
                assert_eq!(updates(&t, page).await, i64::from(written), "{what}");
                let expected = if written { "tapé\n" } else { "<p/>\n" };
                assert_eq!(mmd(&team.olga, page).await, expected, "{what}");
                assert!(!client.is_closed(), "{what}");
            }
        }
    }
}

/// La seconde lecture des droits, une fois la page chargée, peut échouer : une panne du store.
/// La connexion est alors fermée sans avoir rien reçu, plutôt que servie sur ce que le premier
/// contrôle avait lu avant la poignée de main. La panne : la révision de l'espace ne se lit
/// plus (sa colonne est renommée pendant le chargement de la page).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn store_failure_at_the_second_rights_read_closes_the_connection() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let grants = [user(&team.olga, Level::Full), user(&team.gina, Level::Edit)];
    let page = team.page(None, "Panne", &grants).await;
    let mut lock = WriteLock::take(&t).await;
    // La poignée de main aboutit ; le chargement de la page attend le verrou.
    let client = sync_as(&team.gina.browser, &sync_url(addr, page))
        .await
        .unwrap_or_else(|refusal| panic!("connexion de synchro refusée : {refusal}"));
    lock.run("ALTER TABLE spaces RENAME COLUMN rev TO rev_illisible")
        .await;
    lock.commit().await;

    assert!(closed(&client).await, "encore ouverte");
    assert!(!client.is_synced(), "elle a reçu la page");
    assert_eq!(updates(&t, page).await, 0);
    // La page a bien été chargée : le refus vient de la lecture d'après.
    let snapshots = format!("SELECT COUNT(*) FROM doc_snapshots WHERE node_id = '{page}'");
    assert_eq!(query_i64(&t.db_path(), &snapshots).await, 1);
}

/// Ruling R89, pour `PUT /mmd` : les droits sont contrôlés avant la lecture du corps, puis de
/// nouveau une fois la page chargée, juste avant d'écrire. Un droit retiré pendant le
/// chargement n'écrit plus rien, et la réponse est celle du droit qui manque.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn put_mmd_checks_rights_again_once_the_page_is_loaded() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let snapshots = async |page: Uuid| {
        let sql = format!("SELECT COUNT(*) FROM doc_snapshots WHERE node_id = '{page}'");
        query_i64(&t.db_path(), &sql).await
    };
    let cases = [
        ("rien ne change", None, None),
        (
            "rétrogradée à la lecture",
            Some(DEMOTED_TO_READ),
            Some((StatusCode::FORBIDDEN, "forbidden")),
        ),
        (
            "mise à la corbeille",
            Some(TRASHED),
            Some((StatusCode::CONFLICT, "trashed")),
        ),
        (
            "accès retiré",
            Some(ACCESS_REMOVED),
            Some((StatusCode::NOT_FOUND, "not_found")),
        ),
    ];
    for (what, change, refusal) in cases {
        let grants = [user(&team.olga, Level::Full), user(&team.gina, Level::Edit)];
        let page = team.page(None, what, &grants).await;
        let id = page.to_string();
        let mut lock = WriteLock::take(&t).await;
        let commit = async {
            // Le temps que la route contrôle les droits, lise le corps et demande la page,
            // dont le chargement attend le verrou. Rien ne dit de l'extérieur qu'elle y est :
            // l'attente est fixe, et la plus large que le verrou permet (`HELD`, la moitié des
            // 5 s qu'une écriture l'attend). Trop courte sur un poste chargé, elle ferait
            // échouer le test à tort (le refus viendrait du premier contrôle, et la page ne
            // serait pas chargée : l'assertion sur l'instantané le verrait), jamais passer.
            tokio::time::sleep(HELD).await;
            if let Some(change) = change {
                write_rights_change(&mut lock, change, team.space, page, &team.gina).await;
            }
            lock.commit().await;
        };
        let (reply, ()) = tokio::join!(team.gina.put_mmd(&id, "Écrit\n"), commit);

        // La page a été chargée : un refus vient donc du second contrôle, pas du premier.
        assert_eq!(snapshots(page).await, 1, "{what}");
        match refusal {
            None => {
                reply.assert_status(StatusCode::NO_CONTENT);
                assert_eq!(mmd(&team.olga, page).await, "Écrit\n", "{what}");
            }
            Some((status, code)) => {
                reply.assert_error(status, code);
                assert_eq!(updates(&t, page).await, 0, "{what} : rien d'écrit");
                assert_eq!(mmd(&team.olga, page).await, "<p/>\n", "{what}");
            }
        }
    }
}

/// Le hachage de la session que porte ce navigateur, en hexadécimal, comme le rend
/// `hex(token_hash)` en SQL.
fn session_hex(browser: &Browser<'_>) -> String {
    let token = browser.session().expect("une session");
    let hash = mementum_server::auth::token_hash(&token).expect("un jeton de session");
    hash.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// Ruling R88 : la course elle-même, forcée. La session est fermée entre le contrôle de la
/// route et l'inscription de la connexion, sans `kick_user` (il passerait avant l'inscription,
/// sans trouver la connexion) : elle est fermée quand même, sans avoir rien reçu.
///
/// C'est la session de la connexion qui est relue, pas « une session du compte » : le compte en
/// a une autre, qui vaut toujours.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_closed_while_the_connection_opens_closes_it() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let cases = [
        ("rien ne change", None),
        (
            "session supprimée",
            Some("DELETE FROM sessions WHERE hex(token_hash) = '{session}'"),
        ),
        (
            "session échue",
            Some("UPDATE sessions SET expires_at = 1 WHERE hex(token_hash) = '{session}'"),
        ),
    ];
    for (what, change) in cases {
        let page = team.page(None, what, &[members(Level::Edit)]).await;
        let url = sync_url(addr, page);
        // Un second appareil de Mona, avec sa propre session.
        let phone = t.browser();
        phone
            .log_in("mona@exemple.fr", PASSWORD)
            .await
            .assert_status(StatusCode::OK);
        let session = session_hex(&phone);
        let mut lock = WriteLock::take(&t).await;
        // La poignée de main aboutit : la route a lu la session. Le chargement de la page
        // attend le verrou.
        let client = sync_as(&phone, &url)
            .await
            .unwrap_or_else(|refusal| panic!("{what} : {refusal}"));
        if let Some(change) = change {
            lock.run(&change.replace("{session}", &session)).await;
        }
        lock.commit().await;

        let Some(_) = change else {
            assert!(
                client.wait_until(WAIT, |c| c.is_synced()).await,
                "{what} : la connexion n'est pas servie"
            );
            types(&client, "tapé").await;
            assert_eq!(updates(&t, page).await, 1, "{what}");
            continue;
        };
        assert!(closed(&client).await, "{what} : encore ouverte");
        assert!(!client.is_synced(), "{what} : elle a reçu la page");
        assert_eq!(updates(&t, page).await, 0, "{what}");
        assert_eq!(refused(&phone, &url).await, 401, "{what}");
        // L'autre session du compte vaut toujours : elle ouvre la page.
        let home = synced_as(&team.mona.browser, &url).await;
        assert!(still_served(&home).await, "{what}");
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

/// Ruling R90, pour l'inscription : créer un compte par-dessus une session la ferme, avec les
/// connexions de synchro du compte qu'elle portait, comme le fait la connexion. Sur un
/// navigateur partagé, les pages ouvertes par la personne d'avant ne restent pas ouvertes sous
/// le cookie de la suivante. Une inscription refusée ne ferme rien.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signup_over_a_session_closes_the_previous_account_connections() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let page = team.page(None, "Commune", &[members(Level::Edit)]).await;
    let url = sync_url(addr, page);
    let sessions_of = async |account: &Account<'_>| {
        let sql = format!(
            "SELECT COUNT(*) FROM sessions WHERE user_id = '{}'",
            account.id
        );
        query_i64(&t.db_path(), &sql).await
    };
    // Un navigateur partagé : Mona y est connectée, la page ouverte. Elle l'est aussi chez
    // elle ; Tom a la page ouverte ailleurs.
    let shared = t.browser();
    shared
        .log_in("mona@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let mona = synced_as(&shared, &url).await;
    let mona_home = synced_as(&team.mona.browser, &url).await;
    let tom = synced_as(&team.tom.browser, &url).await;
    types(&mona, "avant").await;
    let replaced = t.browser();
    replaced.set_session(shared.session().as_deref());
    assert_eq!(sessions_of(&team.mona).await, 2);

    // Des inscriptions refusées par-dessus la session : une adresse prise, une invitation qui
    // n'existe pas. Rien n'est fermé, et la session vaut toujours.
    shared
        .sign_up("tom@exemple.fr", "Tom bis", PASSWORD, None)
        .await
        .assert_error(StatusCode::CONFLICT, "email_taken");
    let no_such_invitation = "A".repeat(43);
    shared
        .sign_up(
            "nina@exemple.fr",
            "Nina",
            PASSWORD,
            Some(&no_such_invitation),
        )
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    assert!(
        still_served(&mona).await,
        "fermée par une inscription ratée"
    );
    let me = shared.me().await;
    me.assert_status(StatusCode::OK);
    assert_eq!(me.json()["user"]["email"], "mona@exemple.fr");
    assert_eq!(sessions_of(&team.mona).await, 2);

    // Nina s'inscrit par-dessus.
    shared
        .sign_up("nina@exemple.fr", "Nina", PASSWORD, None)
        .await
        .assert_status(StatusCode::CREATED);
    types_unheard(&mona, " trop tard");
    assert!(
        closed(&mona).await,
        "la connexion de Mona est encore ouverte"
    );
    // La session remplacée ne vaut plus rien, pour personne.
    assert_eq!(refused(&replaced, &url).await, 401);
    replaced
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert_eq!(sessions_of(&team.mona).await, 1);
    assert!(still_served(&tom).await, "Tom est fermé");
    // Toutes les connexions du compte remplacé sont fermées (Ruling R73) ; son autre appareil,
    // qui a gardé sa session, revient, et rien de « trop tard » n'a été écrit.
    assert!(closed(&mona_home).await);
    let mona_home = synced_as(&team.mona.browser, &url).await;
    assert_eq!(text(&mona_home), "avant");

    // Le navigateur porte maintenant la session de Nina, qui n'a aucun rôle dans l'espace : la
    // page n'existe pas pour elle.
    let me = shared.me().await;
    me.assert_status(StatusCode::OK);
    assert_eq!(me.json()["user"]["email"], "nina@exemple.fr");
    assert_eq!(refused(&shared, &url).await, 404);
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

/// Rulings R101, R118 et R136, par la vraie route : une connexion de synchro porte le compte de
/// sa session, son nom, et son droit d'écrire. La présence qu'annonce un compte qui écrit est à
/// lui : Tom ne remplace ni n'efface l'état de Mona, et renvoyer au serveur ce qu'il reçoit des
/// autres, comme le fait y-websocket, ne le ferme pas. Le nom d'un état est celui du compte, lu
/// avec la session : Tom a beau s'annoncer « Mona », les autres voient « Tom ». Gina, invitée en
/// lecture seule sur la page, voit les autres ; rien de ce qu'elle annonce n'est gardé ni
/// relayé. (Le détail des règles est testé dans `mementum-sync`, `tests/presence.rs` et
/// `tests/wire.rs` ; ici, que la route donne bien au hub le compte, le nom et le droit de chaque
/// connexion.)
///
/// Les états se distinguent par leur couleur : le nom, que le serveur écrit, ne change plus
/// quand le client en annonce un autre (avant R136, ce test les distinguait par le nom).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn presence_belongs_to_the_account_of_the_session() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let grants = [members(Level::Edit), user(&team.gina, Level::Read)];
    let page = team.page(None, "Commune", &grants).await;
    let url = sync_url(addr, page);
    let mona = synced_as(&team.mona.browser, &url).await;
    let tom = synced_as(&team.tom.browser, &url).await;
    let gina = synced_as(&team.gina.browser, &url).await;
    let (mona_id, tom_id, gina_id) = (mona.client_id(), tom.client_id(), gina.client_id());
    let state =
        |name: &str, colour: &str| format!(r#"{{"user":{{"name":"{name}","color":"{colour}"}}}}"#);
    let sees = async |seen_by: &Client, client: u64, state: &str| {
        let arrived =
            |c: &Client| c.awareness_states().get(&client).map(String::as_str) == Some(state);
        seen_by.wait_until(WAIT, arrived).await
    };

    // Ceux qui écrivent s'annoncent, et chacun les voit, la lectrice comprise.
    mona.set_awareness(&state("Mona", "#000001"));
    tom.set_awareness(&state("Tom", "#000002"));
    for seen_by in [&tom, &gina] {
        assert!(sees(seen_by, mona_id, &state("Mona", "#000001")).await);
    }
    for seen_by in [&mona, &gina] {
        assert!(sees(seen_by, tom_id, &state("Tom", "#000002")).await);
    }

    // Tom renvoie l'état de Mona tel qu'il l'a reçu ; puis le remplace ; puis le retire. Gina
    // fait de même, et s'annonce.
    for sender in [&tom, &gina] {
        sender.send_raw(awareness_message(&[(
            mona_id,
            1,
            &state("Mona", "#000001"),
        )]));
        sender.send_raw(awareness_message(&[(
            mona_id,
            500,
            &state("Mona ?", "#0000ff"),
        )]));
        sender.send_raw(awareness_message(&[(mona_id, 501, "null")]));
    }
    gina.set_awareness(&state("Gina", "#000004"));
    assert!(still_served(&tom).await, "fermé pour un état renvoyé");
    assert!(still_served(&gina).await, "fermée pour un état annoncé");
    // Barrière : l'état suivant de Tom, envoyé après, arrive chez les autres après tout ce
    // que le serveur aurait relayé d'eux avant. Il s'y dit « Mona » : les autres lisent le nom
    // de son compte.
    tom.set_awareness(&state("Mona", "#000003"));
    for seen_by in [&mona, &gina] {
        assert!(
            sees(seen_by, tom_id, &state("Tom", "#000003")).await,
            "le nom annoncé par Tom est relayé"
        );
    }
    assert!(sees(&gina, mona_id, &state("Mona", "#000001")).await);
    assert!(
        !mona.awareness_states().contains_key(&gina_id),
        "la présence d'une lectrice est relayée"
    );
    // Le serveur n'en a rien gardé : qui arrive reçoit l'état de Mona, pas celui de Gina, et
    // l'état suivant de Mona passe partout.
    let adam = synced_as(&team.adam.browser, &url).await;
    assert!(sees(&adam, mona_id, &state("Mona", "#000001")).await);
    assert!(sees(&adam, tom_id, &state("Tom", "#000003")).await);
    assert!(!adam.awareness_states().contains_key(&gina_id));
    // La liste des présents (trame de page, Ruling R136), par la même route : une ligne par
    // compte, dans l'ordre des comptes, le nom du compte, la lectrice sans le droit d'écrire,
    // et rien d'autre (pas d'adresse).
    let roster = serde_json::json!({ "type": "roster", "users": [
        { "id": team.adam.id.to_string(), "name": "Adam", "canWrite": true },
        { "id": team.mona.id.to_string(), "name": "Mona", "canWrite": true },
        { "id": team.tom.id.to_string(), "name": "Tom", "canWrite": true },
        { "id": team.gina.id.to_string(), "name": "Gina", "canWrite": false },
    ] });
    let last_roster = |c: &Client| {
        c.page_events()
            .last()
            .map(|json| serde_json::from_str::<serde_json::Value>(json).unwrap())
    };
    for seen_by in [&gina, &adam] {
        assert!(
            seen_by
                .wait_until(WAIT, |c| last_roster(c).as_ref() == Some(&roster))
                .await,
            "{:?}",
            seen_by.page_events()
        );
    }
    mona.set_awareness(&state("Mona", "#000005"));
    for seen_by in [&tom, &gina, &adam] {
        assert!(sees(seen_by, mona_id, &state("Mona", "#000005")).await);
    }
    assert!(!gina.is_closed());
}

/// Délai dans lequel une connexion doit être fermée quand son accès est retiré (Review
/// Focus 1).
const CLOSES_WITHIN: Duration = Duration::from_secs(2);

/// Review Focus 1 : B écrit sur une page partagée avec lui, puis son accès est retiré par la
/// route de partage. Sa connexion se ferme en moins de deux secondes, sa reconnexion reçoit
/// `404`, et ce qu'il envoie ensuite n'est pas dans le MMD.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoked_access_closes_live_connection() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    // Une page d'Olga, partagée avec Gina en modification ; sa sous-page en hérite.
    let grants = [user(&team.olga, Level::Full), user(&team.gina, Level::Edit)];
    let shared = team.page(None, "Partagée", &grants).await;
    let child = team.page(Some(shared), "Sous-page", &[]).await;
    let (shared_url, child_url) = (sync_url(addr, shared), sync_url(addr, child));
    let olga = synced_as(&team.olga.browser, &shared_url).await;
    let gina = synced_as(&team.gina.browser, &shared_url).await;
    let gina_child = synced_as(&team.gina.browser, &child_url).await;
    types(&gina, "de Gina").await;
    assert!(olga.wait_until(WAIT, |c| text(c) == "de Gina").await);

    let revoked = Instant::now();
    team.olga
        .browser
        .delete(&format!(
            "/api/v1/pages/{shared}/access/users/{}",
            team.gina.id
        ))
        .await
        .assert_status(StatusCode::OK);
    // Au retour de la route, plus rien de ce qu'elle envoie n'est traité.
    types_unheard(&gina, " en trop");
    for (client, which) in [(&gina, "la page"), (&gina_child, "la sous-page")] {
        assert!(
            client.wait_until(CLOSES_WITHIN, |c| c.is_closed()).await,
            "encore ouverte : {which}"
        );
    }
    let took = revoked.elapsed();
    assert!(took < CLOSES_WITHIN, "fermées en {took:?}");

    // Elle ne revient pas : la réponse d'une page qui n'existe pas.
    for url in [&shared_url, &child_url] {
        assert_eq!(refused(&team.gina.browser, url).await, 404);
    }
    assert_eq!(
        outcome(&plain_get(&team.gina.browser, shared).await),
        outcome(&plain_get(&team.gina.browser, Uuid::now_v7()).await)
    );
    assert_eq!(mmd(&team.olga, shared).await, "de Gina\n");

    // Olga, fermée avec toute la page, revient et écrit.
    assert!(closed(&olga).await);
    let olga = synced_as(&team.olga.browser, &shared_url).await;
    assert_eq!(text(&olga), "de Gina");
    types(&olga, ", puis d'Olga").await;
    assert_eq!(mmd(&team.olga, shared).await, "de Gina, puis d'Olga\n");
}

/// Le centile `percent` de durées triées, en millisecondes.
fn centile(sorted: &[Duration], percent: usize) -> f64 {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank - 1].as_secs_f64() * 1000.0
}

/// L'attente d'un navigateur avant de revenir après `closures` fermetures de suite sans
/// synchro (y-websocket 3.1.0) : 200 ms après la première, le double après chaque suivante,
/// 2,5 s au plus.
///
/// Recopié à la main de y-websocket 3.1.0, `src/y-websocket.js`, lignes 184 à 207
/// (`closeWebsocketConnection` : `wsUnsuccessfulReconnects++`, puis `min(2^n × 100,
/// maxBackoffTime)`) et ligne 324 (`maxBackoffTime = 2500`). À relire quand la version de
/// y-websocket d'`apps/web` change.
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
