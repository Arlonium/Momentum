//! Le partage d'une page : qui y a accès (`GET …/access`), l'accès général des membres, l'accès
//! d'une personne, les invitations sur la page. Toutes ces routes demandent `full` ; une page
//! garde toujours quelqu'un en `full` ; une invitation sur une page ne donne jamais le reste de
//! l'espace, ne dit rien des comptes, et ne vaut que tant que son auteur peut partager la page.
//!
//! Les rôles et les pages de départ sont posés par le store (`Team`, dans `common/mod.rs`) ; les
//! accès qu'on teste passent par les routes. Les valeurs du contrat (codes d'erreur, niveaux)
//! sont écrites ici en clair.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use axum::http::{Method, StatusCode, header};
use common::{
    Account, Browser, Logs, Reply, SESSION_COOKIE, Team, TestApp, WAIT, exec_sql, json_request,
    outcome, overtaken, page_routes, query_i64, text, tree_nodes,
};
use mementum_acl::{Level, Subject};
use mementum_schema::SCHEMA_VERSION;
use mementum_server::SignupMode;
use mementum_sync::testkit::Client;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

/// Une application où tout le monde peut s'inscrire : les comptes de ces tests reçoivent leur
/// rôle par le store, ou par une invitation.
async fn open_app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

fn members(level: Level) -> (Subject, Level) {
    (Subject::Members, level)
}

fn user(account: &Account<'_>, level: Level) -> (Subject, Level) {
    (Subject::User(account.id), level)
}

fn access_uri(page: impl std::fmt::Display) -> String {
    format!("/api/v1/pages/{page}/access")
}

fn general_uri(page: impl std::fmt::Display) -> String {
    format!("/api/v1/pages/{page}/access/general")
}

fn user_uri(page: impl std::fmt::Display, user: impl std::fmt::Display) -> String {
    format!("/api/v1/pages/{page}/access/users/{user}")
}

fn page_invitations_uri(page: impl std::fmt::Display) -> String {
    format!("/api/v1/pages/{page}/invitations")
}

/// `GET …/access` : `200`, le corps.
async fn access_of(by: &Browser<'_>, page: Uuid) -> Value {
    let reply = by.get(&access_uri(page)).await;
    reply.assert_status(StatusCode::OK);
    reply.json()
}

/// `PUT …/access/general` avec `{ "level": level }`.
async fn set_general(by: &Browser<'_>, page: Uuid, level: Value) -> Reply {
    by.put(&general_uri(page), json!({ "level": level })).await
}

/// `PUT …/access/users/{user}` avec `{ "level": level }`.
async fn set_level(
    by: &Browser<'_>,
    page: Uuid,
    user: impl std::fmt::Display,
    level: &str,
) -> Reply {
    by.put(&user_uri(page, user), json!({ "level": level }))
        .await
}

/// `DELETE …/access/users/{user}`.
async fn remove(by: &Browser<'_>, page: Uuid, user: impl std::fmt::Display) -> Reply {
    by.delete(&user_uri(page, user)).await
}

/// `POST …/invitations` : la réponse.
async fn invite(by: &Browser<'_>, page: Uuid, email: &str, level: &str) -> Reply {
    by.post(
        &page_invitations_uri(page),
        json!({ "email": email, "level": level }),
    )
    .await
}

/// Une invitation sur une page, créée : `(id, jeton)`.
async fn invited(by: &Browser<'_>, page: Uuid, email: &str, level: &str) -> (Uuid, String) {
    let reply = invite(by, page, email, level).await;
    reply.assert_status(StatusCode::CREATED);
    let body = reply.json();
    (common::uuid(&body["id"]), text(&body, "token").to_string())
}

async fn lookup(browser: &Browser<'_>, token: &str) -> Reply {
    browser
        .post("/api/v1/invitations/lookup", json!({ "token": token }))
        .await
}

async fn accept(browser: &Browser<'_>, token: &str) -> Reply {
    browser
        .post("/api/v1/invitations/accept", json!({ "token": token }))
        .await
}

/// Ce que `by` voit de l'espace : `(titre, niveau, section)` de chaque page de son arbre, par
/// titre.
async fn seen(by: &Browser<'_>, space: Uuid) -> Vec<(String, String, String)> {
    let mut pages: Vec<(String, String, String)> = tree_nodes(by, space)
        .await
        .iter()
        .map(|n| {
            (
                text(n, "title").to_string(),
                text(n, "level").to_string(),
                text(n, "section").to_string(),
            )
        })
        .collect();
    pages.sort();
    pages
}

/// `(titre, niveau, section)`, pour comparer à [`seen`].
fn page(title: &str, level: &str, section: &str) -> (String, String, String) {
    (title.to_string(), level.to_string(), section.to_string())
}

/// Les entrées d'un `PageAccess` : `(userId, niveau, hérité)`, dans l'ordre de la réponse.
fn entries(access: &Value) -> Vec<(Uuid, String, bool)> {
    access["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("« entries » en liste attendu dans {access}"))
        .iter()
        .map(|e| {
            (
                common::uuid(&e["userId"]),
                text(e, "level").to_string(),
                e["inherited"]
                    .as_bool()
                    .unwrap_or_else(|| panic!("« inherited » booléen attendu dans {e}")),
            )
        })
        .collect()
}

fn entry(account: &Account<'_>, level: &str, inherited: bool) -> (Uuid, String, bool) {
    (account.id, level.to_string(), inherited)
}

/// Le rôle de `account` dans l'espace, d'après son `GET /api/v1/me` ; `None` s'il n'y est pas.
async fn role_in(account: &Account<'_>, space: Uuid) -> Option<String> {
    let me = account.browser.me().await;
    me.assert_status(StatusCode::OK);
    me.json()["spaces"]
        .as_array()
        .expect("« spaces » en liste")
        .iter()
        .find(|s| s["id"] == space.to_string())
        .map(|s| text(s, "role").to_string())
}

/// Les cinq routes du partage, sur la page `id` (corps valides) : `(nom, réponse)`.
async fn five_routes(
    by: &Browser<'_>,
    id: &str,
    some_user: Uuid,
    other_user: Uuid,
) -> Vec<(&'static str, Reply)> {
    vec![
        ("GET access", by.get(&access_uri(id)).await),
        (
            "PUT general",
            by.put(&general_uri(id), json!({ "level": "read" })).await,
        ),
        (
            "PUT user",
            by.put(&user_uri(id, some_user), json!({ "level": "full" }))
                .await,
        ),
        ("DELETE user", by.delete(&user_uri(id, other_user)).await),
        (
            "POST invitation",
            by.post(
                &page_invitations_uri(id),
                json!({ "email": "quelqu-un@exemple.fr", "level": "read" }),
            )
            .await,
        ),
    ]
}

/// Les cinq routes demandent `full` : sous ce niveau, `403 forbidden` ; sans voir la page (pas
/// de rôle dans l'espace, invitée sans accès, admin de l'instance), la réponse d'une page qui
/// n'existe pas, avant même que le corps soit lu. Rien n'est écrit.
#[tokio::test]
async fn sharing_requires_full() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    // Mona (membre) y a `edit` par la ligne des membres ; Olga, `full`.
    let id = team
        .page(
            None,
            "Page",
            &[members(Level::Edit), user(&team.olga, Level::Full)],
        )
        .await;
    let before = team.grants(id).await;
    let (tom, olga) = (team.tom.id, team.olga.id);

    for (route, reply) in five_routes(&team.mona.browser, &id.to_string(), tom, olga).await {
        let body = reply.assert_error(StatusCode::FORBIDDEN, "forbidden");
        assert!(text(&body, "message").contains("accès complet"), "{route}");
    }
    // Un corps illisible n'y change rien : les droits d'abord.
    set_general(&team.mona.browser, id, json!("owner"))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");

    for who in [&team.zoe, &team.gina, &team.sam] {
        let unknown = Uuid::now_v7().to_string();
        let missing = five_routes(&who.browser, &unknown, tom, olga).await;
        let malformed = five_routes(&who.browser, "pas-un-id", tom, olga).await;
        let real = five_routes(&who.browser, &id.to_string(), tom, olga).await;
        for (((route, real), (_, missing)), (_, malformed)) in
            real.iter().zip(&missing).zip(&malformed)
        {
            real.assert_error(StatusCode::NOT_FOUND, "not_found");
            assert_eq!(outcome(real), outcome(missing), "{route}");
            assert_eq!(outcome(real), outcome(malformed), "{route}");
        }
        // Corps illisible : toujours la réponse d'une page qui n'existe pas.
        let bad = set_general(&who.browser, id, json!("owner")).await;
        assert_eq!(outcome(&bad), outcome(&missing[1].1));
    }

    // Sans session.
    for (_, reply) in five_routes(&t.browser(), &id.to_string(), tom, olga).await {
        reply.assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    }

    // Olga, en `full`, lit les accès.
    access_of(&team.olga.browser, id).await;
    // Rien n'a été écrit, aucune invitation créée.
    assert_eq!(team.grants(id).await, before);
    assert!(
        team.store
            .pending_invitations(team.space)
            .await
            .unwrap()
            .is_empty()
    );
}

/// L'accès général déplace une page entre les sections : privée, puis ouverte aux membres en
/// lecture (dans « Espace » pour un autre membre, sous-page comprise, et pas pour l'invitée),
/// puis `null` (« seulement les invités ») : elle redevient invisible pour lui.
#[tokio::test]
async fn general_access_switch_moves_the_page_between_sections() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let olga = &team.olga;
    let notes = olga
        .create(json!({ "title": "Notes", "spaceId": team.space.to_string() }))
        .await;
    olga.create(json!({ "title": "Sous-notes", "parentId": notes.id }))
        .await;
    let id = common::uuid(&json!(notes.id));
    assert!(seen(&team.mona.browser, team.space).await.is_empty());
    assert_eq!(
        seen(&olga.browser, team.space).await,
        [
            page("Notes", "full", "private"),
            page("Sous-notes", "full", "private")
        ]
    );

    // Ouverte aux membres, en lecture.
    let reply = set_general(&olga.browser, id, json!("read")).await;
    reply.assert_status(StatusCode::OK);
    let access = reply.json();
    assert_eq!(access["general"], "read");
    assert_eq!(entries(&access), [entry(olga, "full", false)]);
    assert_eq!(access["invitations"], json!([]));
    assert_eq!(access_of(&olga.browser, id).await, access);
    assert_eq!(
        seen(&team.mona.browser, team.space).await,
        [
            page("Notes", "read", "space"),
            page("Sous-notes", "read", "space")
        ]
    );
    // En lecture seulement.
    team.mona
        .get_mmd(&notes.id)
        .await
        .assert_status(StatusCode::OK);
    team.mona
        .put_mmd(&notes.id, "Texte\n")
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    // Pour Olga aussi, la page est passée dans « Espace ».
    assert_eq!(
        seen(&olga.browser, team.space).await,
        [
            page("Notes", "full", "space"),
            page("Sous-notes", "full", "space")
        ]
    );
    // L'invitée n'est pas un membre : rien.
    assert!(seen(&team.gina.browser, team.space).await.is_empty());

    // Un corps sans « level » ne retire rien : `null` se demande.
    olga.browser
        .put(&general_uri(id), json!({}))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    assert_eq!(access_of(&olga.browser, id).await["general"], "read");

    // « Seulement les invités » : la ligne des membres part.
    let reply = set_general(&olga.browser, id, Value::Null).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(reply.json()["general"], Value::Null);
    assert!(seen(&team.mona.browser, team.space).await.is_empty());
    let gone = team.mona.browser.get(&format!("/api/v1/pages/{id}")).await;
    let unknown = team
        .mona
        .browser
        .get(&format!("/api/v1/pages/{}", Uuid::now_v7()))
        .await;
    assert_eq!(outcome(&gone), outcome(&unknown));
    assert_eq!(
        seen(&olga.browser, team.space).await,
        [
            page("Notes", "full", "private"),
            page("Sous-notes", "full", "private")
        ]
    );
    assert_eq!(team.grants(id).await, [user(olga, Level::Full)]);
}

/// Review Focus 2 : B, invité sur une page, accepte. Il voit la page et ses sous-pages, et rien
/// d'autre de l'espace ; il n'en est pas membre : ni la liste des membres, ni une page à la
/// racine.
#[tokio::test]
async fn page_invitation_makes_a_guest_not_a_member() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let project = team.page(None, "Projet", &[members(Level::Full)]).await;
    let chapter = team.page(Some(project), "Chapitre", &[]).await;
    let beside = team.page(None, "À côté", &[members(Level::Read)]).await;
    let private = team
        .page(None, "Privée", &[user(&team.olga, Level::Full)])
        .await;

    // Zoé n'a aucun rôle dans l'espace : elle est B.
    let zoe = &team.zoe;
    assert_eq!(role_in(zoe, team.space).await, None);
    let (_, token) = invited(&team.olga.browser, project, "zoe@exemple.fr", "edit").await;
    let reply = accept(&zoe.browser, &token).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(
        reply.json(),
        json!({ "spaceId": team.space.to_string(), "pageId": project.to_string() })
    );

    // Invitée, pas membre.
    assert_eq!(role_in(zoe, team.space).await.as_deref(), Some("guest"));
    // La page et sa sous-page, dans « Partagé », et rien d'autre.
    assert_eq!(
        seen(&zoe.browser, team.space).await,
        [
            page("Chapitre", "edit", "shared"),
            page("Projet", "edit", "shared")
        ]
    );
    for id in [project, chapter] {
        zoe.browser
            .get(&format!("/api/v1/pages/{id}"))
            .await
            .assert_status(StatusCode::OK);
    }
    zoe.put_mmd(&chapter.to_string(), "Écrit par Zoé\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let unknown = zoe
        .browser
        .get(&format!("/api/v1/pages/{}", Uuid::now_v7()))
        .await;
    for id in [beside, private] {
        let reply = zoe.browser.get(&format!("/api/v1/pages/{id}")).await;
        assert_eq!(outcome(&reply), outcome(&unknown));
    }
    // Ni les membres, ni les invitations de l'espace, ni une page à la racine.
    zoe.browser
        .get(&format!("/api/v1/spaces/{}/members", team.space))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    zoe.browser
        .get(&format!("/api/v1/spaces/{}/invitations", team.space))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    for section in ["space", "private"] {
        zoe.post(json!({ "spaceId": team.space.to_string(), "section": section }))
            .await
            .assert_error(StatusCode::FORBIDDEN, "forbidden");
    }
    // Le partage de la page ne lui est pas ouvert non plus (`edit`).
    zoe.browser
        .get(&access_uri(project))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    // Le lien a servi.
    accept(&team.tom.browser, &token)
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
}

/// Un membre invité sur une page n'y gagne que le niveau de l'invitation, sur elle et ses
/// sous-pages : son rôle ne change pas, ni ce qu'il a ailleurs, ni ce qu'ont les autres.
#[tokio::test]
async fn invited_member_gets_only_the_extra_level() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let project = team.page(None, "Projet", &[members(Level::Read)]).await;
    team.page(Some(project), "Chapitre", &[]).await;
    team.page(None, "Autre", &[members(Level::Read)]).await;

    let (_, token) = invited(&team.olga.browser, project, "tom@exemple.fr", "edit").await;
    accept(&team.tom.browser, &token)
        .await
        .assert_status(StatusCode::OK);

    assert_eq!(
        role_in(&team.tom, team.space).await.as_deref(),
        Some("member")
    );
    assert_eq!(
        seen(&team.tom.browser, team.space).await,
        [
            page("Autre", "read", "space"),
            page("Chapitre", "edit", "space"),
            page("Projet", "edit", "space")
        ]
    );
    assert_eq!(
        seen(&team.mona.browser, team.space).await,
        [
            page("Autre", "read", "space"),
            page("Chapitre", "read", "space"),
            page("Projet", "read", "space")
        ]
    );
    let access = access_of(&team.olga.browser, project).await;
    assert_eq!(access["general"], "read");
    assert_eq!(entries(&access), [entry(&team.tom, "edit", false)]);
}

/// Retirer son dernier accès à un invité le sort de l'espace ; tant qu'il en garde un, il reste.
#[tokio::test]
async fn guest_leaves_the_space_with_his_last_grant() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let gina = &team.gina;
    let first = team
        .page(
            None,
            "Première",
            &[members(Level::Full), user(gina, Level::Read)],
        )
        .await;
    let second = team
        .page(
            None,
            "Seconde",
            &[members(Level::Full), user(gina, Level::Full)],
        )
        .await;
    assert_eq!(
        seen(&gina.browser, team.space).await,
        [
            page("Première", "read", "shared"),
            page("Seconde", "full", "shared")
        ]
    );

    let reply = remove(&team.olga.browser, first, gina.id).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(entries(&reply.json()), []);
    assert_eq!(role_in(gina, team.space).await.as_deref(), Some("guest"));
    assert_eq!(
        seen(&gina.browser, team.space).await,
        [page("Seconde", "full", "shared")]
    );
    // En `full` sur la seconde page, elle y invite quelqu'un.
    invited(&gina.browser, second, "ami@exemple.fr", "read").await;
    let invitations = &access_of(&team.olga.browser, second).await["invitations"];
    assert_eq!(invitations.as_array().map(Vec::len), Some(1));

    // Sa dernière ligne partie, elle sort de l'espace, et ses invitations en attente avec elle,
    // comme au départ d'un membre.
    let reply = remove(&team.olga.browser, second, gina.id).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(reply.json()["invitations"], json!([]));
    assert_eq!(role_in(gina, team.space).await, None);
    gina.browser
        .get(&format!("/api/v1/spaces/{}/tree", team.space))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    let listed = team
        .olga
        .browser
        .get(&format!("/api/v1/spaces/{}/members", team.space))
        .await;
    listed.assert_status(StatusCode::OK);
    assert!(!listed.text().contains(&gina.id.to_string()));
    // Un membre dont on retire une ligne, lui, reste membre (il n'en avait pas d'autre).
    let third = team
        .page(
            None,
            "Troisième",
            &[members(Level::Full), user(&team.tom, Level::Edit)],
        )
        .await;
    remove(&team.olga.browser, third, team.tom.id)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        role_in(&team.tom, team.space).await.as_deref(),
        Some("member")
    );
}

/// Ruling R70 : après tout changement, la page garde quelqu'un en `full`, à elle ou hérité ;
/// sinon `409 last_full_access`, et rien n'est écrit. Une ligne `members` compte (les admins y
/// ont `full`), un `full` hérité aussi ; une ligne restée à quelqu'un sans rôle, non.
#[tokio::test]
async fn last_full_access_cannot_be_removed() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let olga = &team.olga.browser;
    let refused = async |reply: Reply| {
        let body = reply.assert_error(StatusCode::CONFLICT, "last_full_access");
        assert!(text(&body, "message").contains("accès complet"), "{body}");
    };

    // La page privée d'Olga, partagée en lecture avec Mona.
    let shared = team
        .page(
            None,
            "Partagée",
            &[user(&team.olga, Level::Full), user(&team.mona, Level::Read)],
        )
        .await;
    let before = team.grants(shared).await;
    refused(remove(olga, shared, team.olga.id).await).await;
    for level in ["edit", "comment", "read"] {
        refused(set_level(olga, shared, team.olga.id, level).await).await;
    }
    assert_eq!(team.grants(shared).await, before);

    // Une page des membres : la ligne `members` est le seul `full`. Elle peut baisser, elle
    // compte encore. Elle peut partir aussi, depuis R122 (c'était `409 last_full_access`,
    // sans issue) : qui la retire reçoit sa ligne, et la page garde quelqu'un en `full`.
    let team_page = team.page(None, "Équipe", &[members(Level::Full)]).await;
    let reply = set_general(olga, team_page, json!("read")).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(reply.json()["general"], "read");
    assert_eq!(team.grants(team_page).await, [members(Level::Read)]);
    let reply = set_general(olga, team_page, Value::Null).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(entries(&reply.json()), [entry(&team.olga, "full", false)]);
    assert_eq!(
        team.grants(team_page).await,
        [user(&team.olga, Level::Full)]
    );
    // Cette ligne-là est désormais la seule : elle ne part pas.
    refused(remove(olga, team_page, team.olga.id).await).await;

    // Sam n'a pas de rôle dans l'espace : sa ligne ne compte pas.
    let dormant = team
        .page(
            None,
            "Dormante",
            &[user(&team.olga, Level::Full), user(&team.sam, Level::Full)],
        )
        .await;
    let before = team.grants(dormant).await;
    refused(remove(olga, dormant, team.olga.id).await).await;
    refused(set_level(olga, dormant, team.olga.id, "edit").await).await;
    assert_eq!(team.grants(dormant).await, before);

    // Un `full` hérité compte : Olga l'a sur le parent.
    let parent = team
        .page(None, "Parent", &[user(&team.olga, Level::Full)])
        .await;
    let child = team
        .page(Some(parent), "Enfant", &[user(&team.mona, Level::Full)])
        .await;
    let reply = remove(olga, child, team.mona.id).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(entries(&reply.json()), [entry(&team.olga, "full", true)]);
    // Sur le parent, la seule ligne en `full` ne part pas.
    refused(remove(olga, parent, team.olga.id).await).await;

    // Une ligne `members` sur un ancêtre compte, à tout niveau.
    let open = team.page(None, "Ouverte", &[members(Level::Read)]).await;
    let under = team
        .page(Some(open), "Dessous", &[user(&team.mona, Level::Full)])
        .await;
    remove(olga, under, team.mona.id)
        .await
        .assert_status(StatusCode::OK);
    assert!(team.grants(under).await.is_empty());
}

/// On peut se retirer à soi-même son accès, même le dernier moyen de voir la page, tant que
/// quelqu'un d'autre y garde `full` : le changement est fait (`204`, on ne voit plus les accès),
/// et la page sort de son arbre.
#[tokio::test]
async fn removing_oneself_is_allowed() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let both = [user(&team.olga, Level::Full), user(mona, Level::Full)];
    let left = team.page(None, "Quittée", &both).await;
    let lowered = team.page(None, "Baissée", &both).await;

    let reply = remove(&mona.browser, left, mona.id).await;
    reply.assert_status(StatusCode::NO_CONTENT);
    assert!(reply.body.is_empty());
    assert_eq!(team.grants(left).await, [user(&team.olga, Level::Full)]);
    mona.browser
        .get(&format!("/api/v1/pages/{left}"))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");

    // Se baisser en lecture : on voit encore la page, plus ses accès.
    set_level(&mona.browser, lowered, mona.id, "read")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        seen(&mona.browser, team.space).await,
        [page("Baissée", "read", "shared")]
    );
    mona.browser
        .get(&access_uri(lowered))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");

    // Olga, seule en `full` désormais, ne se retire pas.
    remove(&team.olga.browser, lowered, team.olga.id)
        .await
        .assert_error(StatusCode::CONFLICT, "last_full_access");
}

/// Ce qu'on hérite d'un ancêtre se voit sur la sous-page (`inherited: true`, au niveau hérité),
/// et ne s'y retire pas : `404`, comme pour un compte sans ligne. Un compte qui a une ligne sur la
/// page et un niveau hérité plus fort n'apparaît qu'une fois, au niveau effectif, hérité.
#[tokio::test]
async fn inherited_entries_cannot_be_deleted_from_the_child() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let olga = &team.olga.browser;
    let parent = team
        .page(
            None,
            "Parent",
            &[
                user(&team.olga, Level::Full),
                user(&team.mona, Level::Edit),
                user(&team.tom, Level::Read),
            ],
        )
        .await;
    let child = team
        .page(
            Some(parent),
            "Enfant",
            &[
                user(&team.mona, Level::Read),
                user(&team.tom, Level::Edit),
                user(&team.gina, Level::Comment),
            ],
        )
        .await;

    let access = access_of(olga, child).await;
    assert_eq!(access["general"], Value::Null);
    // Dans l'ordre d'arrivée dans l'espace : Olga, Mona, Tom, Gina.
    assert_eq!(
        entries(&access),
        [
            entry(&team.olga, "full", true),
            entry(&team.mona, "edit", true),
            entry(&team.tom, "edit", false),
            entry(&team.gina, "comment", false),
        ]
    );
    let olga_entry = &access["entries"][0];
    assert_eq!(olga_entry["name"], "Olga");
    assert_eq!(olga_entry["email"], "olga@exemple.fr");
    assert_eq!(
        entries(&access_of(olga, parent).await),
        [
            entry(&team.olga, "full", false),
            entry(&team.mona, "edit", false),
            entry(&team.tom, "read", false),
        ]
    );

    // Olga n'a pas de ligne sur l'enfant : `404`, comme un compte inconnu ou mal écrit.
    let before = team.grants(child).await;
    let inherited = remove(olga, child, team.olga.id).await;
    inherited.assert_error(StatusCode::NOT_FOUND, "not_found");
    for other in [Uuid::now_v7().to_string(), "pas-un-id".to_string()] {
        assert_eq!(
            outcome(&remove(olga, child, &other).await),
            outcome(&inherited)
        );
    }
    assert_eq!(team.grants(child).await, before);

    // Mona a sa ligne : elle part, et Mona garde ce qu'elle hérite.
    let reply = remove(olga, child, team.mona.id).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(
        entries(&reply.json()),
        [
            entry(&team.olga, "full", true),
            entry(&team.mona, "edit", true),
            entry(&team.tom, "edit", false),
            entry(&team.gina, "comment", false),
        ]
    );
}

/// Ruling R47 : une invitation sur une page ne dit pas si l'adresse a déjà un compte. La réponse
/// a le même statut et la même forme pour un membre, un compte hors de l'espace et une adresse
/// inconnue ; la liste des invitations et la consultation du lien aussi.
#[tokio::test]
async fn page_invitation_answer_does_not_reveal_accounts() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let project = team.page(None, "Projet", &[members(Level::Full)]).await;
    /// Statut, type de contenu, et chaque champ avec la sorte de sa valeur.
    fn shape(reply: &Reply) -> (StatusCode, Option<String>, BTreeMap<String, String>) {
        let fields = reply
            .json()
            .as_object()
            .expect("un objet")
            .iter()
            .map(|(key, value)| {
                let kind = match value {
                    Value::String(s) => format!("texte de {} caractères", s.len()),
                    other => format!("{other:?}"),
                };
                (key.clone(), kind)
            })
            .collect();
        (reply.status, reply.content_type.clone(), fields)
    }

    let emails = ["mona@exemple.fr", "zoe@exemple.fr", "personne@exemple.fr"];
    let mut tokens = Vec::new();
    let mut shapes = Vec::new();
    for email in emails {
        let reply = invite(&team.olga.browser, project, email, "read").await;
        reply.assert_status(StatusCode::CREATED);
        // `id` (36 caractères) et `token` (43), rien d'autre.
        let mut fields = shape(&reply).2.into_keys().collect::<Vec<_>>();
        fields.sort();
        assert_eq!(fields, ["id", "token"]);
        tokens.push(text(&reply.json(), "token").to_string());
        shapes.push(shape(&reply));
    }
    assert!(shapes.windows(2).all(|w| w[0] == w[1]), "{shapes:?}");

    // La liste des invitations de la page : les trois, pareilles.
    let access = access_of(&team.olga.browser, project).await;
    let listed = access["invitations"].as_array().expect("une liste");
    assert_eq!(listed.len(), 3);
    for (invitation, email) in listed.iter().zip(emails) {
        assert_eq!(invitation["email"], email);
        assert_eq!(invitation["level"], "read");
        assert_eq!(invitation["role"], Value::Null);
        assert_eq!(invitation["pageId"], project.to_string());
        assert!(invitation.get("token").is_none());
    }
    // La consultation du lien, sans session : même forme.
    let looked: Vec<_> = {
        let mut all = Vec::new();
        for token in &tokens {
            let reply = lookup(&t.browser(), token).await;
            reply.assert_status(StatusCode::OK);
            all.push(shape(&reply).2.into_keys().collect::<Vec<_>>());
        }
        all
    };
    assert!(looked.windows(2).all(|w| w[0] == w[1]), "{looked:?}");

    // Une adresse mal formée, elle, est refusée.
    invite(&team.olga.browser, project, "pas une adresse", "read")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_email");
}

/// Ruling R69 : une invitation de page ne vaut que tant que son auteur peut partager la page.
/// Mona perd `full` : son lien ne se consulte plus, ne s'accepte plus et ne sert plus à
/// s'inscrire, comme un lien inconnu. Elle le retrouve : le lien revaut.
#[tokio::test]
async fn page_invitation_dies_when_its_author_loses_full() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let project = team
        .page(
            None,
            "Projet",
            &[user(&team.olga, Level::Full), user(&team.mona, Level::Full)],
        )
        .await;
    let (_, token) = invited(&team.mona.browser, project, "zoe@exemple.fr", "read").await;
    lookup(&t.browser(), &token)
        .await
        .assert_status(StatusCode::OK);

    set_level(&team.olga.browser, project, team.mona.id, "edit")
        .await
        .assert_status(StatusCode::OK);

    let unknown = lookup(&t.browser(), &"A".repeat(43)).await;
    unknown.assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    let looked = lookup(&t.browser(), &token).await;
    assert_eq!(outcome(&looked), outcome(&unknown));
    accept(&team.zoe.browser, &token)
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    let newcomer = t.browser();
    newcomer
        .sign_up(
            "nouvelle@exemple.fr",
            "Nouvelle",
            common::PASSWORD,
            Some(&token),
        )
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    assert!(newcomer.session().is_none());
    newcomer
        .log_in("nouvelle@exemple.fr", common::PASSWORD)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    assert_eq!(role_in(&team.zoe, team.space).await, None);

    // Mona retrouve `full` : son lien revaut.
    set_level(&team.olga.browser, project, team.mona.id, "full")
        .await
        .assert_status(StatusCode::OK);
    accept(&team.zoe.browser, &token)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        seen(&team.zoe.browser, team.space).await,
        [page("Projet", "read", "shared")]
    );
}

/// Une invitation de page se révoque par qui peut partager la page, membre ou non ; pour qui ne
/// le peut pas, admin compris, comme une invitation qui n'existe pas.
#[tokio::test]
async fn page_invitation_is_revoked_by_who_can_share() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    // La page de Mona (membre), partagée en lecture avec Tom ; Adam (admin) ne la voit pas.
    let page_id = team
        .page(
            None,
            "De Mona",
            &[user(&team.mona, Level::Full), user(&team.tom, Level::Read)],
        )
        .await;
    let (id, _) = invited(&team.mona.browser, page_id, "x@exemple.fr", "edit").await;
    assert_eq!(
        access_of(&team.mona.browser, page_id).await["invitations"][0]["id"],
        id.to_string()
    );
    let missing = team
        .adam
        .browser
        .delete(&format!("/api/v1/invitations/{}", Uuid::now_v7()))
        .await;
    missing.assert_error(StatusCode::NOT_FOUND, "not_found");
    for who in [&team.adam, &team.tom, &team.olga, &team.zoe] {
        let reply = who
            .browser
            .delete(&format!("/api/v1/invitations/{id}"))
            .await;
        assert_eq!(outcome(&reply), outcome(&missing));
    }
    team.mona
        .browser
        .delete(&format!("/api/v1/invitations/{id}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        access_of(&team.mona.browser, page_id).await["invitations"],
        json!([])
    );
    // Une invitation d'espace, elle, reste aux admins.
    let space_invitation = team
        .olga
        .browser
        .post(
            &format!("/api/v1/spaces/{}/invitations", team.space),
            json!({ "email": "y@exemple.fr", "role": "member" }),
        )
        .await;
    space_invitation.assert_status(StatusCode::CREATED);
    let space_id = text(&space_invitation.json(), "id").to_string();
    let reply = team
        .mona
        .browser
        .delete(&format!("/api/v1/invitations/{space_id}"))
        .await;
    assert_eq!(outcome(&reply), outcome(&missing));
}

/// On ne donne un accès qu'à un compte qui a un rôle dans l'espace, membre ou invité, à l'un
/// des quatre niveaux. Un compte sans rôle répond comme un compte inconnu : on n'apprend pas
/// qu'il existe.
#[tokio::test]
async fn access_is_given_only_to_people_of_the_space() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let olga = &team.olga.browser;
    let project = team.page(None, "Projet", &[members(Level::Full)]).await;
    let before = team.grants(project).await;

    // Sam existe (il administre l'instance), sans rôle dans l'espace.
    let outsider = set_level(olga, project, team.sam.id, "read").await;
    outsider.assert_error(StatusCode::NOT_FOUND, "not_found");
    for other in [Uuid::now_v7().to_string(), "pas-un-id".to_string()] {
        assert_eq!(
            outcome(&set_level(olga, project, &other, "read").await),
            outcome(&outsider)
        );
    }
    assert_eq!(team.grants(project).await, before);

    // L'invitée, oui : elle voit la page, dans « Partagé ».
    let reply = set_level(olga, project, team.gina.id, "comment").await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(
        entries(&reply.json()),
        [entry(&team.gina, "comment", false)]
    );
    assert_eq!(
        seen(&team.gina.browser, team.space).await,
        [page("Projet", "comment", "shared")]
    );
    // Les quatre niveaux, et eux seuls.
    for level in ["read", "comment", "edit", "full"] {
        let reply = set_level(olga, project, team.tom.id, level).await;
        reply.assert_status(StatusCode::OK);
        assert!(
            entries(&reply.json()).contains(&entry(&team.tom, level, false)),
            "{level}"
        );
    }
    for bad in [json!({ "level": "owner" }), json!({})] {
        olga.put(&user_uri(project, team.tom.id), bad)
            .await
            .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    }
    // `null` ne retire pas l'accès : c'est `DELETE`. (serde_json le lit comme une erreur de
    // syntaxe, `400`.)
    let null = olga
        .put(&user_uri(project, team.tom.id), json!({ "level": null }))
        .await;
    assert!(null.status.is_client_error(), "{}", null.text());
    assert_eq!(null.json()["error"], "invalid_json");
    assert!(entries(&access_of(olga, project).await).contains(&entry(&team.tom, "full", false)));
    invite(olga, project, "x@exemple.fr", "owner")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
}

/// Ruling R85 : l'accès général (ce que voient les membres de l'espace) ne se pose ni ne se
/// retire par un invité, même en `full` : `403`, après le refus de niveau et avant
/// `409 trashed`, et rien n'est écrit. Le reste de ce que donne `full` lui reste (R86).
#[tokio::test]
async fn guest_with_full_cannot_change_general_access() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let gina = &team.gina;
    // Une page partagée avec elle, sans accès général ; une page d'équipe où elle a sa ligne.
    let shared = team
        .page(
            None,
            "Partagée",
            &[user(&team.olga, Level::Full), user(gina, Level::Full)],
        )
        .await;
    let team_page = team
        .page(
            None,
            "Équipe",
            &[members(Level::Full), user(gina, Level::Full)],
        )
        .await;
    let guest_refusal = async |id: Uuid, level: Value| {
        let before = team.grants(id).await;
        let reply = set_general(&gina.browser, id, level.clone()).await;
        let body = reply.assert_error(StatusCode::FORBIDDEN, "forbidden");
        assert!(
            text(&body, "message").contains("invité"),
            "{level} : {body}"
        );
        assert_eq!(team.grants(id).await, before, "{level}");
    };
    // Poser, dans les deux sens de niveau, et retirer.
    for level in [json!("read"), json!("full")] {
        guest_refusal(shared, level).await;
    }
    for level in [Value::Null, json!("read")] {
        guest_refusal(team_page, level).await;
    }
    // Mona ne voit toujours pas la page partagée ; l'équipe garde la sienne.
    assert_eq!(
        seen(&team.mona.browser, team.space).await,
        [page("Équipe", "full", "space")]
    );

    // R86 : le reste de `full` lui reste. Elle lit les accès, en donne, invite.
    access_of(&gina.browser, shared).await;
    set_level(&gina.browser, shared, team.tom.id, "read")
        .await
        .assert_status(StatusCode::OK);
    invited(&gina.browser, shared, "ami@exemple.fr", "full").await;

    // L'ordre des refus : sous `full`, le refus de niveau ; à la corbeille, R85 d'abord.
    let read_only = team
        .page(
            None,
            "Lue",
            &[user(&team.olga, Level::Full), user(gina, Level::Edit)],
        )
        .await;
    let body = set_general(&gina.browser, read_only, json!("read"))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    assert!(text(&body, "message").contains("accès complet"), "{body}");
    team.trash(shared).await;
    guest_refusal(shared, json!("read")).await;
    set_general(&team.olga.browser, shared, json!("read"))
        .await
        .assert_error(StatusCode::CONFLICT, "trashed");
}

/// Ruling R87 : un changement d'accès ne dépend que de ce sur quoi il repose, vérifié dans sa
/// transaction. Mona, qui crée des sous-pages sans arrêt ailleurs dans l'espace (chacune fait
/// bouger `spaces.rev`), ne le bloque pas : l'ancien contrôle, sur la révision de tout
/// l'espace, finissait en `409 access_changed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sharing_is_not_blocked_by_writes_elsewhere_in_the_space() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let project = team.page(None, "Projet", &[members(Level::Full)]).await;
    let busy = team.page(None, "Agitée", &[members(Level::Edit)]).await;
    let stop = Arc::new(AtomicBool::new(false));
    let created = Arc::new(AtomicUsize::new(0));
    let cookie = team.mona.browser.cookie().expect("la session de Mona");
    let writers: Vec<_> = (0..2)
        .map(|_| {
            let (router, cookie, stop) = (t.router.clone(), cookie.clone(), stop.clone());
            let created = created.clone();
            tokio::spawn(async move {
                while !stop.load(Ordering::Relaxed) {
                    let body = json!({ "parentId": busy.to_string(), "title": "Encore" });
                    let mut request = json_request(Method::POST, "/api/v1/pages", &body);
                    let headers = request.headers_mut();
                    headers.insert(header::HOST, "localhost".parse().unwrap());
                    headers.insert(header::COOKIE, cookie.parse().unwrap());
                    let reply = router.clone().oneshot(request).await.unwrap();
                    assert_eq!(reply.status(), StatusCode::CREATED);
                    created.fetch_add(1, Ordering::Relaxed);
                }
            })
        })
        .collect();

    // Au moins trente changements, et autant qu'il en faut pour que dix pages aient été créées
    // pendant qu'ils passaient. Un nombre fixe de changements ne suffit pas : sur un poste
    // chargé, trente changements passent avant dix créations (vu : sept), et le test ne
    // prouverait plus que les écritures de Mona étaient là.
    let olga = &team.olga.browser;
    let mut steps = 0;
    let mut last = "";
    while steps < 30 || created.load(Ordering::Relaxed) < 10 {
        assert!(
            steps < 5_000,
            "les créations n'avancent pas : {} en {steps} changements",
            created.load(Ordering::Relaxed)
        );
        last = if steps % 2 == 0 { "read" } else { "edit" };
        let reply = set_level(olga, project, team.tom.id, last).await;
        assert_eq!(
            reply.status,
            StatusCode::OK,
            "étape {steps} : {}",
            reply.text()
        );
        steps += 1;
    }
    stop.store(true, Ordering::Relaxed);
    for writer in writers {
        writer.await.unwrap();
    }
    assert!(entries(&access_of(olga, project).await).contains(&entry(&team.tom, last, false)));
}

/// Ruling R93 : l'accès général qu'une page parente donne est le plancher de la sous-page
/// (`inheritedGeneral`) ; `general` est le niveau effectif. Sous le plancher, ou `null` sans
/// ligne propre : `409 inherited_access`, rien d'écrit. Au plancher : la ligne propre part, il
/// n'en reste jamais de redondante. Au-dessus : elle est posée ; `null` la retire et revient au
/// plancher. Sans plancher : `null` retire la ligne propre, ou ne fait rien. Chaque cas, avec ce
/// qui reste en base et ce que rend `PageAccess`.
#[tokio::test]
async fn inherited_general_access_is_a_floor_on_the_child() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let olga = &team.olga.browser;
    let parent = team.page(None, "Parent", &[members(Level::Edit)]).await;
    let child = team.page(Some(parent), "Enfant", &[]).await;
    // `(general, inheritedGeneral)` d'un `PageAccess`.
    let floor = |access: &Value| {
        (
            access["general"].clone(),
            access["inheritedGeneral"].clone(),
        )
    };
    let edit_floor = (json!("edit"), json!("edit"));
    // Un `PUT …/general` qui passe : `200`, et le `PageAccess` rendu.
    let set = async |id: Uuid, level: Value| {
        let reply = set_general(olga, id, level.clone()).await;
        assert_eq!(reply.status, StatusCode::OK, "{level} : {}", reply.text());
        reply.json()
    };

    assert_eq!(floor(&access_of(olga, child).await), edit_floor);
    assert_eq!(
        floor(&access_of(olga, parent).await),
        (json!("edit"), Value::Null)
    );

    // Sous le plancher, ou `null` sans ligne propre : refusé, rien d'écrit.
    let refused = async |level: Value| {
        let before = team.grants(child).await;
        let reply = set_general(olga, child, level.clone()).await;
        let body = reply.assert_error(StatusCode::CONFLICT, "inherited_access");
        assert!(
            text(&body, "message").contains("parente"),
            "{level} : {body}"
        );
        assert_eq!(team.grants(child).await, before, "{level}");
    };
    for level in [Value::Null, json!("read"), json!("comment")] {
        refused(level).await;
    }
    // Au plancher, sans ligne propre : rien à écrire.
    assert_eq!(floor(&set(child, json!("edit")).await), edit_floor);
    assert!(team.grants(child).await.is_empty());

    // Au-dessus : la ligne est posée ; c'est elle qu'on voit, sur le plancher hérité.
    let access = set(child, json!("full")).await;
    assert_eq!(floor(&access), (json!("full"), json!("edit")));
    assert_eq!(team.grants(child).await, [members(Level::Full)]);
    assert_eq!(
        seen(&team.mona.browser, team.space).await,
        [
            page("Enfant", "full", "space"),
            page("Parent", "edit", "space")
        ]
    );
    // Ligne propre plus haute, puis `null` : retour au plancher, sans ligne.
    assert_eq!(floor(&set(child, Value::Null).await), edit_floor);
    assert!(team.grants(child).await.is_empty());
    assert_eq!(
        seen(&team.mona.browser, team.space).await,
        [
            page("Enfant", "edit", "space"),
            page("Parent", "edit", "space")
        ]
    );
    // Ligne propre plus haute, puis le niveau du plancher : pas de ligne redondante.
    set(child, json!("full")).await;
    assert_eq!(floor(&set(child, json!("edit")).await), edit_floor);
    assert!(team.grants(child).await.is_empty());
    // Sous le plancher, même avec une ligne propre : refusé, elle reste.
    set(child, json!("full")).await;
    refused(json!("read")).await;
    assert_eq!(team.grants(child).await, [members(Level::Full)]);

    // Sans plancher (une page privée d'Olga) : `null` sans ligne ne fait rien ; un niveau se
    // pose ; `null` le retire.
    let alone = team
        .page(None, "Seule", &[user(&team.olga, Level::Full)])
        .await;
    assert_eq!(
        floor(&set(alone, Value::Null).await),
        (Value::Null, Value::Null)
    );
    assert_eq!(team.grants(alone).await, [user(&team.olga, Level::Full)]);
    assert_eq!(
        floor(&set(alone, json!("read")).await),
        (json!("read"), Value::Null)
    );
    assert_eq!(
        floor(&set(alone, Value::Null).await),
        (Value::Null, Value::Null)
    );
    assert_eq!(team.grants(alone).await, [user(&team.olga, Level::Full)]);
}

/// Une page à la corbeille garde ses accès : on les lit, on ne les change pas (`409 trashed`).
#[tokio::test]
async fn trashed_page_access_is_read_only() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let project = team.page(None, "Projet", &[members(Level::Full)]).await;
    team.trash(project).await;
    let olga = &team.olga.browser;
    assert_eq!(access_of(olga, project).await["general"], "full");
    for (route, reply) in five_routes(olga, &project.to_string(), team.tom.id, team.tom.id)
        .await
        .into_iter()
        .skip(1)
    {
        let body = reply.assert_error(StatusCode::CONFLICT, "trashed");
        assert!(text(&body, "message").contains("corbeille"), "{route}");
    }
    assert_eq!(team.grants(project).await, [members(Level::Full)]);
}

// ---------------------------------------------------------------------------------------------
// Les refus de la transaction (Ruling R87), tels qu'ils sortent par HTTP.
//
// Avant le corps, la route répond par le cache des accès ; c'est la transaction du store qui
// fait foi. Trois de ses refus ne se voient qu'en course : la page devenue invisible, mise à la
// corbeille, ou l'appelant devenu invité, entre le contrôle de la route et l'écriture. Pour que
// le refus vienne de la transaction sans dépendre d'un minutage, ces tests font ce que le store
// ne fait jamais : ils écrivent en base sans incrémenter la révision de l'espace. Le cache garde
// les droits d'avant, le contrôle de la route passe, et la transaction, qui relit tout, refuse.

/// Une page qu'Olga partage avec Mona, toutes deux en accès complet, sans accès général. Les
/// accès de Mona sont en cache : elle en lit les accès.
async fn shared_with_mona(team: &Team<'_>) -> Uuid {
    let lines = [user(&team.olga, Level::Full), user(&team.mona, Level::Full)];
    let page = team.page(None, "D'Olga et Mona", &lines).await;
    access_of(&team.mona.browser, page).await;
    page
}

/// Ce qu'un changement d'accès refusé ne touche pas : les lignes de la page et la révision de
/// l'espace.
async fn untouched(t: &TestApp, team: &Team<'_>, page: Uuid) -> (Vec<(Subject, Level)>, i64) {
    let rev = format!("SELECT rev FROM spaces WHERE id = '{}'", team.space);
    (team.grants(page).await, query_i64(&t.db_path(), &rev).await)
}

/// Les trois routes qui changent un accès, appelées par `by` sur `page` : l'accès général,
/// l'accès de Tom, le retrait de celui d'Olga.
async fn three_changes(
    by: &Browser<'_>,
    page: Uuid,
    team: &Team<'_>,
) -> Vec<(&'static str, Reply)> {
    vec![
        ("PUT general", set_general(by, page, json!("read")).await),
        ("PUT users", set_level(by, page, team.tom.id, "read").await),
        ("DELETE users", remove(by, page, team.olga.id).await),
    ]
}

/// `Unseen` : Mona perd son accès à la page entre le contrôle de la route et l'écriture. La
/// transaction ne la lui montre plus, et la réponse est exactement celle d'une page qui
/// n'existe pas : même statut, même type, même corps. Rien n'est écrit.
#[tokio::test]
async fn sharer_who_lost_the_page_gets_the_404_of_an_unknown_page() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona.browser;
    let page = shared_with_mona(&team).await;
    let gone = format!(
        "DELETE FROM acl WHERE node_id = '{page}' AND subject = '{}'",
        Subject::User(team.mona.id)
    );
    exec_sql(&t.db_path(), &gone).await;
    // Le cache n'en sait rien : il lui laisse encore lire les accès. Le refus qui suit vient
    // donc de la transaction.
    access_of(mona, page).await;
    let before = untouched(&t, &team, page).await;

    let unknown = three_changes(mona, Uuid::now_v7(), &team).await;
    let refused = three_changes(mona, page, &team).await;
    for ((route, reply), (_, unknown)) in refused.iter().zip(&unknown) {
        reply.assert_error(StatusCode::NOT_FOUND, "not_found");
        assert_eq!(outcome(reply), outcome(unknown), "{route}");
    }
    assert_eq!(untouched(&t, &team, page).await, before);
}

/// `Trashed` : la page part à la corbeille entre le contrôle de la route et l'écriture :
/// `409 trashed`, rien n'est écrit.
#[tokio::test]
async fn sharing_overtaken_by_the_trash_answers_trashed() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = shared_with_mona(&team).await;
    let trashed = format!("UPDATE nodes SET trashed_at = 1 WHERE id = '{page}'");
    exec_sql(&t.db_path(), &trashed).await;
    // Le cache n'en sait rien : la page est encore dans l'arbre de Mona, d'où il exclut la
    // corbeille. Le contrôle de la route passe donc, et le refus qui suit vient de la
    // transaction.
    let tree = tree_nodes(&team.mona.browser, team.space).await;
    assert!(
        tree.iter().any(|node| text(node, "id") == page.to_string()),
        "le cache des accès sait la page à la corbeille : {tree:?}"
    );
    let before = untouched(&t, &team, page).await;

    for (route, reply) in three_changes(&team.mona.browser, page, &team).await {
        let body = reply.assert_error(StatusCode::CONFLICT, "trashed");
        assert!(text(&body, "message").contains("corbeille"), "{route}");
    }
    assert_eq!(untouched(&t, &team, page).await, before);
}

/// `GuestGeneral` (Ruling R85) : Mona, membre, devient invitée entre le contrôle de la route et
/// l'écriture. Sa ligne lui garde l'accès complet, mais l'accès général n'est plus à elle :
/// `403 forbidden`, avec le message de R85, et rien n'est écrit. Le reste de ce que donne
/// l'accès complet lui reste (R86).
#[tokio::test]
async fn sharer_demoted_to_guest_cannot_change_general_access() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona.browser;
    let page = shared_with_mona(&team).await;
    let demoted = format!(
        "UPDATE members SET role = 'guest' WHERE space_id = '{}' AND user_id = '{}'",
        team.space, team.mona.id
    );
    exec_sql(&t.db_path(), &demoted).await;
    // Le cache n'en sait rien : Mona lit encore les membres de l'espace, ce qu'une invitée ne
    // peut pas. Le contrôle de la route passe donc, et le refus qui suit vient de la
    // transaction.
    mona.get(&format!("/api/v1/spaces/{}/members", team.space))
        .await
        .assert_status(StatusCode::OK);
    let before = untouched(&t, &team, page).await;

    for level in [json!("read"), Value::Null] {
        let reply = set_general(mona, page, level.clone()).await;
        let body = reply.assert_error(StatusCode::FORBIDDEN, "forbidden");
        let message = text(&body, "message");
        assert!(
            message.contains("invité") && message.contains("accès général"),
            "{level} : {body}"
        );
    }
    assert_eq!(untouched(&t, &team, page).await, before);
    // Invitée en accès complet, elle donne encore un accès à quelqu'un.
    set_level(mona, page, team.tom.id, "read")
        .await
        .assert_status(StatusCode::OK);
}

/// Le journal d'un changement d'accès dit ce qui a été écrit, pas ce qui était demandé. Au
/// plancher qu'une page parente donne, la ligne `members` de la sous-page est retirée : le
/// journal dit qu'elle n'a plus de ligne, pas le niveau demandé. Jamais d'adresse ni de nom.
#[tokio::test]
async fn sharing_log_says_what_was_written() {
    /// La ligne que le serveur journalise pour un changement d'accès de `page`, pendant
    /// `change`, qui doit réussir : une seule, au niveau `info`, sans adresse ni nom.
    async fn logged(change: impl Future<Output = Reply>, page: Uuid) -> String {
        let logs = Logs::capture();
        change.await.assert_status(StatusCode::OK);
        let journal = logs.text();
        let page = page.to_string();
        let mut lines = journal
            .lines()
            .filter(|line| line.contains(" INFO ") && line.contains(&page));
        let line = lines
            .next()
            .unwrap_or_else(|| panic!("journal : {journal}"));
        assert_eq!(lines.next(), None, "journal : {journal}");
        for personal in ["exemple.fr", "Olga", "Tom", "Mona"] {
            assert!(!line.contains(personal), "{line}");
        }
        line.to_string()
    }
    let t = open_app().await;
    let team = Team::new(&t).await;
    let olga = &team.olga.browser;
    let parent = team.page(None, "Parent", &[members(Level::Edit)]).await;
    let child = team.page(Some(parent), "Enfant", &[]).await;

    // Au-dessus du plancher : la ligne est posée, à ce niveau.
    let line = logged(set_general(olga, child, json!("full")), child).await;
    assert!(line.contains(r#"line="full""#), "{line}");
    // Au plancher : la ligne propre part. Le niveau demandé (`edit`) n'est pas écrit.
    let line = logged(set_general(olga, child, json!("edit")), child).await;
    assert!(line.contains(r#"line="aucune""#), "{line}");
    assert!(!line.contains("edit"), "{line}");
    assert!(team.grants(child).await.is_empty());

    // L'accès d'une personne : posé, puis retiré.
    let tom = team.tom.id;
    let line = logged(set_level(olga, child, tom, "read"), child).await;
    assert!(
        line.contains(r#"line="read""#) && line.contains(&tom.to_string()),
        "{line}"
    );
    let line = logged(remove(olga, child, tom), child).await;
    assert!(
        line.contains(r#"line="aucune""#) && line.contains(&tom.to_string()),
        "{line}"
    );

    // Ruling R122 : Mona retire l'accès général d'une page d'équipe, et sa ligne est posée. Le
    // journal dit les deux écritures : la ligne des membres partie, celle de qui demande
    // posée, pour le compte dont l'id est déjà là. Rien de plus.
    let team_page = team.page(None, "Équipe", &[members(Level::Full)]).await;
    let mona = &team.mona;
    let line = logged(
        set_general(&mona.browser, team_page, Value::Null),
        team_page,
    )
    .await;
    assert!(
        line.contains(r#"line="aucune""#)
            && line.contains("actor_line_posed=true")
            && line.contains(&format!("user_id={}", mona.id)),
        "{line}"
    );
    // Quand rien n'est posé pour qui demande, le journal le dit aussi.
    let line = logged(
        set_general(&mona.browser, team_page, json!("read")),
        team_page,
    )
    .await;
    assert!(
        line.contains(r#"line="read""#) && line.contains("actor_line_posed=false"),
        "{line}"
    );
}

/// Review Focus 1 : chaque changement d'accès ferme les connexions de synchro de la page et de
/// ses sous-pages (elles se rouvrent en relisant leurs droits) ; une invitation, une lecture des
/// accès ou un changement qui ne change rien n'en ferme aucune, et les autres pages ne sont
/// jamais touchées.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sharing_change_kicks_live_connections() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let project = team.page(None, "Projet", &[members(Level::Full)]).await;
    let chapter = team.page(Some(project), "Chapitre", &[]).await;
    let elsewhere = team.page(None, "Ailleurs", &[members(Level::Full)]).await;
    let session = team.mona.browser.session().expect("la session de Mona");
    let cookie = format!("{SESSION_COOKIE}={session}");
    let connect = async |id: Uuid| {
        let url = format!("ws://{addr}/sync/{id}?schema={SCHEMA_VERSION}");
        let client = Client::connect_with_headers(&url, &[("cookie", &cookie)])
            .await
            .expect("connexion de Mona");
        assert!(client.wait_until(WAIT, |c| c.is_synced()).await);
        client
    };
    let still_served = async |client: &Client| {
        let start = client.step2_count();
        client.resync();
        client.wait_until(WAIT, |c| c.step2_count() > start).await && !client.is_closed()
    };
    let olga = &team.olga.browser;
    let bystander = connect(elsewhere).await;

    // Chaque changement doit fermer les connexions rouvertes après le précédent.
    let mut live = [connect(project).await, connect(chapter).await];
    for step in 0..3 {
        let (what, reply) = match step {
            0 => (
                "l'accès général",
                set_general(olga, project, json!("edit")).await,
            ),
            1 => (
                "l'accès de Tom",
                set_level(olga, project, team.tom.id, "read").await,
            ),
            _ => (
                "le retrait de Tom",
                remove(olga, project, team.tom.id).await,
            ),
        };
        reply.assert_status(StatusCode::OK);
        for client in &live {
            assert!(
                client.wait_until(WAIT, |c| c.is_closed()).await,
                "connexion encore ouverte après {what}"
            );
        }
        live = [connect(project).await, connect(chapter).await];
    }

    // Ni une invitation, ni une lecture, ni un changement à l'identique.
    invited(olga, project, "x@exemple.fr", "read").await;
    access_of(olga, project).await;
    set_general(olga, project, json!("edit"))
        .await
        .assert_status(StatusCode::OK);
    for client in &live {
        assert!(
            still_served(client).await,
            "connexion fermée sans changement"
        );
    }
    assert!(still_served(&bystander).await, "page d'à côté fermée");
}

// ---------------------------------------------------------------------------------------------
// Ruling R122 : qui restreint l'accès général d'une page y garde l'accès complet. Quand le
// changement (la ligne `members` retirée, ou baissée) lui ferait perdre `full`, sa ligne
// `(user:<lui>, full)` est posée dans la même transaction : la réponse est `200`, avec les
// accès de la page, où il figure, et non plus `409 last_full_access` ou `204`.

/// `PUT …/access/general` par `by`, qui doit passer et lui laisser l'accès complet : `200`, et
/// le `PageAccess` rendu, qui est aussi celui que relit `GET …/access`.
async fn restricted(by: &Browser<'_>, page: Uuid, level: Value) -> Value {
    let reply = set_general(by, page, level.clone()).await;
    assert_eq!(reply.status, StatusCode::OK, "{level} : {}", reply.text());
    let access = reply.json();
    assert_eq!(access_of(by, page).await, access, "{level}");
    access
}

/// Les accès posés sur la page, par sujet : pour comparer à [`Team::grants`], qui les rend dans
/// cet ordre.
fn by_subject(mut lines: Vec<(Subject, Level)>) -> Vec<(Subject, Level)> {
    lines.sort_by_key(|(subject, _)| subject.to_string());
    lines
}

/// Cas (a) de la relecture finale : une page née dans « Espace » ne porte que la ligne des
/// membres, son auteur n'y est pas nommé. Qui coche « Seulement les invités » (un membre
/// simple, un admin, la propriétaire) n'est plus refusé par `409 last_full_access` : il reçoit
/// sa ligne, et la page est à lui.
#[tokio::test]
async fn general_access_removed_from_a_team_page_keeps_the_requester_in_full() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    for who in [&team.mona, &team.adam, &team.olga] {
        let id = team.page(None, "Équipe", &[members(Level::Full)]).await;
        let access = restricted(&who.browser, id, Value::Null).await;
        assert_eq!(access["general"], Value::Null);
        assert_eq!(access["inheritedGeneral"], Value::Null);
        assert_eq!(entries(&access), [entry(who, "full", false)]);
        assert_eq!(team.grants(id).await, [user(who, Level::Full)]);
    }
}

/// Cas (b) : une invitée est nommée en accès complet. Le retrait de l'accès général répondait
/// `204`, et qui le demandait perdait la page sans un mot. Il y reste, à côté d'elle.
#[tokio::test]
async fn general_access_removed_keeps_the_requester_beside_a_named_guest() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, gina) = (&team.mona, &team.gina);
    let lines = [members(Level::Full), user(gina, Level::Full)];
    let id = team.page(None, "Projet", &lines).await;

    let access = restricted(&mona.browser, id, Value::Null).await;
    assert_eq!(access["general"], Value::Null);
    // Dans l'ordre d'arrivée dans l'espace : Mona, puis Gina.
    assert_eq!(
        entries(&access),
        [entry(mona, "full", false), entry(gina, "full", false)]
    );
    assert_eq!(
        team.grants(id).await,
        by_subject(vec![user(mona, Level::Full), user(gina, Level::Full)])
    );
    // L'invitée n'y a rien perdu.
    assert_eq!(access_of(&gina.browser, id).await, access);
}

/// Baisser l'accès général, c'est le restreindre aussi : un membre simple qui le passe de
/// l'accès complet à un niveau plus bas répondait `204`, et ne pouvait plus revenir en arrière.
/// Il garde l'accès complet, par sa ligne.
#[tokio::test]
async fn general_access_lowered_keeps_the_requester_in_full() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    for (name, level) in [
        ("edit", Level::Edit),
        ("comment", Level::Comment),
        ("read", Level::Read),
    ] {
        let id = team.page(None, "Équipe", &[members(Level::Full)]).await;
        let access = restricted(&mona.browser, id, json!(name)).await;
        assert_eq!(access["general"], name);
        assert_eq!(entries(&access), [entry(mona, "full", false)], "{name}");
        assert_eq!(
            team.grants(id).await,
            [members(level), user(mona, Level::Full)],
            "{name}"
        );
        // Elle peut donc le relever.
        restricted(&mona.browser, id, json!("full")).await;
    }
}

/// Rien n'est écrit pour qui garde l'accès complet autrement : déjà nommé sur la page, ou sur
/// une page parente ; propriétaire ou admin là où une ligne des membres continue de
/// s'appliquer (la sienne, baissée, ou celle d'une page parente).
#[tokio::test]
async fn no_redundant_line_when_the_requester_keeps_full() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;

    // Déjà nommée en accès complet sur la page.
    let lines = [members(Level::Full), user(mona, Level::Full)];
    let named = team.page(None, "Nommée", &lines).await;
    let access = restricted(&mona.browser, named, Value::Null).await;
    assert_eq!(entries(&access), [entry(mona, "full", false)]);
    assert_eq!(team.grants(named).await, [user(mona, Level::Full)]);
    // Nommée sur la page parente : la sous-page n'a plus aucune ligne.
    let parent = team.page(None, "Parent", &[user(mona, Level::Full)]).await;
    let child = team
        .page(Some(parent), "Enfant", &[members(Level::Edit)])
        .await;
    let access = restricted(&mona.browser, child, Value::Null).await;
    assert_eq!(entries(&access), [entry(mona, "full", true)]);
    assert!(team.grants(child).await.is_empty());

    // Propriétaire, admin : la ligne des membres reste, plus bas ; ils y gardent l'accès
    // complet, sans être nommés.
    for who in [&team.olga, &team.adam] {
        let id = team.page(None, "Équipe", &[members(Level::Full)]).await;
        let access = restricted(&who.browser, id, json!("read")).await;
        assert_eq!(access["general"], "read");
        assert_eq!(entries(&access), []);
        assert_eq!(team.grants(id).await, [members(Level::Read)]);
    }
    // De même sous le plancher d'une page parente : la ligne propre part, le plancher reste.
    let floor = team.page(None, "Plancher", &[members(Level::Read)]).await;
    let under = team
        .page(Some(floor), "Dessous", &[members(Level::Full)])
        .await;
    let access = restricted(&team.olga.browser, under, Value::Null).await;
    assert_eq!(access["general"], "read");
    assert_eq!(access["inheritedGeneral"], "read");
    assert_eq!(entries(&access), []);
    assert!(team.grants(under).await.is_empty());
}

/// Sous le plancher d'une page parente (R93), retirer la ligne propre de la sous-page fait
/// retomber un membre simple au niveau du plancher : sa ligne est posée aussi. La sous-page
/// reste dans « Espace », ouverte aux membres au niveau du plancher.
#[tokio::test]
async fn general_access_removed_under_an_inherited_floor_keeps_the_requester_in_full() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let parent = team.page(None, "Parent", &[members(Level::Edit)]).await;
    let child = team
        .page(Some(parent), "Enfant", &[members(Level::Full)])
        .await;

    let access = restricted(&mona.browser, child, Value::Null).await;
    assert_eq!(access["general"], "edit");
    assert_eq!(access["inheritedGeneral"], "edit");
    assert_eq!(entries(&access), [entry(mona, "full", false)]);
    assert_eq!(team.grants(child).await, [user(mona, Level::Full)]);
    assert_eq!(team.grants(parent).await, [members(Level::Edit)]);
    assert_eq!(
        seen(&mona.browser, team.space).await,
        [
            page("Enfant", "full", "space"),
            page("Parent", "edit", "space")
        ]
    );
    assert_eq!(
        seen(&team.tom.browser, team.space).await,
        [
            page("Enfant", "edit", "space"),
            page("Parent", "edit", "space")
        ]
    );
}

/// Les sections après coup (`GET …/tree`). La page restreinte passe dans « Privé » pour qui l'a
/// restreinte, s'il y est seul, sous-pages comprises ; dans « Partagé » s'il reste d'autres
/// personnes nommées ; baissée seulement, elle reste dans « Espace ». Un autre membre, un admin
/// et la propriétaire ne voient plus la page restreinte, et reçoivent pour elle, sur chaque
/// route, exactement la réponse d'une page inconnue : statut, type, corps.
#[tokio::test]
async fn restricted_team_page_leaves_the_space_section() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, gina) = (&team.mona, &team.gina);
    let alone = team.page(None, "Seule", &[members(Level::Full)]).await;
    let sub = team.page(Some(alone), "Sous-page", &[]).await;
    let lines = [members(Level::Full), user(gina, Level::Full)];
    let shared = team.page(None, "À deux", &lines).await;
    let lowered = team.page(None, "Baissée", &[members(Level::Full)]).await;
    assert_eq!(
        seen(&team.tom.browser, team.space).await,
        [
            page("Baissée", "full", "space"),
            page("Seule", "full", "space"),
            page("Sous-page", "full", "space"),
            page("À deux", "full", "space"),
        ]
    );

    restricted(&mona.browser, alone, Value::Null).await;
    restricted(&mona.browser, shared, Value::Null).await;
    restricted(&mona.browser, lowered, json!("read")).await;

    assert_eq!(
        seen(&mona.browser, team.space).await,
        [
            page("Baissée", "full", "space"),
            page("Seule", "full", "private"),
            page("Sous-page", "full", "private"),
            page("À deux", "full", "shared"),
        ]
    );
    assert_eq!(
        seen(&gina.browser, team.space).await,
        [page("À deux", "full", "shared")]
    );
    // Les autres membres ne gardent que la page baissée ; les admins y ont l'accès complet.
    assert_eq!(
        seen(&team.tom.browser, team.space).await,
        [page("Baissée", "read", "space")]
    );
    for who in [&team.adam, &team.olga] {
        assert_eq!(
            seen(&who.browser, team.space).await,
            [page("Baissée", "full", "space")]
        );
    }

    // Pour eux, la page restreinte n'existe plus, sur aucune route.
    let unknown = Uuid::now_v7().to_string();
    for who in [&team.tom, &team.adam, &team.olga] {
        let mut missing = Vec::new();
        for (route, request) in page_routes(&unknown) {
            let reply = who.browser.send(request).await;
            let body = reply.assert_error(StatusCode::NOT_FOUND, "not_found");
            assert_eq!(body["message"], "page introuvable", "{route}");
            missing.push(outcome(&reply));
        }
        for id in [alone, sub, shared] {
            for ((route, request), missing) in
                page_routes(&id.to_string()).into_iter().zip(&missing)
            {
                let reply = who.browser.send(request).await;
                assert_eq!(&outcome(&reply), missing, "{route}, page {id}");
            }
        }
    }
    // Rien de cela n'a rien écrit.
    assert_eq!(team.grants(alone).await, [user(mona, Level::Full)]);
    assert!(team.grants(sub).await.is_empty());
}

/// La décision se prend dans la transaction qui écrit (R87). La demande de Mona passe le
/// contrôle de la route, puis attend le verrou d'écriture de SQLite, que le test tient ; un
/// changement croisé est écrit et validé sous ce verrou ; la transaction de Mona vient après
/// lui, et décide sur ce qu'il a laissé. C'est le verrou qui force cet ordre : l'attente de
/// [`overtaken`] ne sert qu'à laisser la route faire son contrôle sur la base d'avant.
#[tokio::test]
async fn crossed_changes_never_leave_the_page_without_full_access() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, gina) = (&team.mona, &team.gina);
    let line_removed = |page: Uuid, who: &Account<'_>| {
        let who = Subject::User(who.id);
        vec![format!(
            "DELETE FROM acl WHERE node_id = '{page}' AND subject = '{who}'"
        )]
    };

    // Mona est nommée : à sa demande, rien n'était à poser pour elle. Olga lui retire sa ligne
    // entre-temps (la ligne des membres le permet encore). Décidé sur la base d'avant, le
    // retrait de l'accès général laissait la page sans personne.
    let lines = [members(Level::Full), user(mona, Level::Full)];
    let unnamed = team.page(None, "Dénommée", &lines).await;
    let request = set_general(&mona.browser, unnamed, Value::Null);
    let taken = overtaken(&t, team.space, request, &line_removed(unnamed, mona)).await;
    taken.reply.assert_status(StatusCode::OK);
    assert_eq!(entries(&taken.reply.json()), [entry(mona, "full", false)]);
    assert_eq!(team.grants(unnamed).await, [user(mona, Level::Full)]);

    // L'invitée est la seule nommée en accès complet ; sa ligne part entre-temps. Sans celle de
    // Mona, la page n'avait plus personne.
    let lines = [members(Level::Full), user(gina, Level::Full)];
    let guest_gone = team.page(None, "Sans l'invitée", &lines).await;
    let request = set_general(&mona.browser, guest_gone, Value::Null);
    let taken = overtaken(&t, team.space, request, &line_removed(guest_gone, gina)).await;
    taken.reply.assert_status(StatusCode::OK);
    assert_eq!(entries(&taken.reply.json()), [entry(mona, "full", false)]);
    assert_eq!(team.grants(guest_gone).await, [user(mona, Level::Full)]);
}
