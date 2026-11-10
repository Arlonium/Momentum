//! Espaces, membres et invitations d'espace : qui peut quoi selon son rôle, le dernier
//! propriétaire, les invitations par lien (consultation, acceptation, échéance, révocation), le
//! jeton montré une seule fois, et des droits relus dès que `spaces.rev` bouge.
//!
//! Quelqu'un qui n'a pas de rôle dans un espace n'en apprend rien : il reçoit la réponse d'un
//! espace qui n'existe pas, admin de l'instance compris. Les valeurs du contrat (durée d'une
//! invitation, codes d'erreur) sont écrites ici en clair, pas reprises du serveur.

mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::{Browser, Reply, TestApp, WAIT, all_logs, assert_french, exec_sql, query_i64};
use mementum_acl::Role;
use mementum_schema::SCHEMA_VERSION;
use mementum_server::SignupMode;
use mementum_store::{Expected, Store};
use mementum_sync::testkit::Client;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Sept jours, en millisecondes : la durée d'une invitation.
const WEEK_MS: i64 = 604_800_000;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Une application où tout le monde peut s'inscrire : les comptes de ces tests entrent dans
/// les espaces par des invitations, pas par l'inscription.
async fn open_app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

fn uuid(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap_or_else(|| panic!("un id : {value}")))
        .unwrap_or_else(|e| panic!("{value} n'est pas un UUID : {e}"))
}

/// Un compte et le navigateur qui porte sa session.
struct Person<'a> {
    browser: Browser<'a>,
    id: Uuid,
}

async fn person<'a>(t: &'a TestApp, email: &str, name: &str) -> Person<'a> {
    let (browser, me) = t.signed_up(email, name).await;
    Person {
        id: uuid(&me["user"]["id"]),
        browser,
    }
}

/// `POST /api/v1/spaces` : l'id de l'espace créé.
async fn create_space(by: &Person<'_>, name: &str) -> Uuid {
    let reply = by
        .browser
        .post("/api/v1/spaces", json!({ "name": name }))
        .await;
    reply.assert_status(StatusCode::CREATED);
    uuid(&reply.json()["id"])
}

/// `POST /api/v1/spaces/{id}/invitations` : la réponse.
async fn invite(by: &Browser<'_>, space: Uuid, email: &str, role: &str) -> Reply {
    by.post(
        &format!("/api/v1/spaces/{space}/invitations"),
        json!({ "email": email, "role": role }),
    )
    .await
}

/// Une invitation créée : `(id, jeton)`.
async fn invited(by: &Browser<'_>, space: Uuid, email: &str, role: &str) -> (Uuid, String) {
    let reply = invite(by, space, email, role).await;
    reply.assert_status(StatusCode::CREATED);
    let body = reply.json();
    let token = body["token"].as_str().expect("token").to_string();
    (uuid(&body["id"]), token)
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

/// `by` invite `who` au rôle `role`, et `who` accepte.
async fn join(by: &Person<'_>, who: &Person<'_>, space: Uuid, role: &str) {
    let (_, token) = invited(&by.browser, space, "invite@exemple.fr", role).await;
    accept(&who.browser, &token)
        .await
        .assert_status(StatusCode::OK);
}

fn members_uri(space: Uuid) -> String {
    format!("/api/v1/spaces/{space}/members")
}

fn member_uri(space: Uuid, user: Uuid) -> String {
    format!("/api/v1/spaces/{space}/members/{user}")
}

fn invitations_uri(space: Uuid) -> String {
    format!("/api/v1/spaces/{space}/invitations")
}

/// Le rôle de chacun dans l'espace, lu par `GET …/members` : `(userId, rôle)`.
async fn roles(by: &Browser<'_>, space: Uuid) -> Vec<(Uuid, String)> {
    let reply = by.get(&members_uri(space)).await;
    reply.assert_status(StatusCode::OK);
    reply
        .json()
        .as_array()
        .expect("une liste")
        .iter()
        .map(|m| (uuid(&m["userId"]), m["role"].as_str().unwrap().to_string()))
        .collect()
}

async fn role_of(by: &Browser<'_>, space: Uuid, user: Uuid) -> Option<String> {
    roles(by, space)
        .await
        .into_iter()
        .find(|(id, _)| *id == user)
        .map(|(_, role)| role)
}

/// L'espace « Équipe » d'Olga (propriétaire), avec Adam (admin), Mona et Tom (membres) et Gina
/// (invitée). Sam, le premier compte, administre l'instance mais n'a aucun rôle dans l'espace.
struct Team<'a> {
    sam: Person<'a>,
    olga: Person<'a>,
    adam: Person<'a>,
    mona: Person<'a>,
    tom: Person<'a>,
    gina: Person<'a>,
    space: Uuid,
}

async fn team(t: &TestApp) -> Team<'_> {
    let sam = person(t, "sam@exemple.fr", "Sam").await;
    let olga = person(t, "olga@exemple.fr", "Olga").await;
    let space = create_space(&olga, "Équipe").await;
    let adam = person(t, "adam@exemple.fr", "Adam").await;
    let mona = person(t, "mona@exemple.fr", "Mona").await;
    let tom = person(t, "tom@exemple.fr", "Tom").await;
    let gina = person(t, "gina@exemple.fr", "Gina").await;
    join(&olga, &adam, space, "admin").await;
    join(&olga, &mona, space, "member").await;
    join(&olga, &tom, space, "member").await;
    // Aucune route de ce lot ne fait un invité (les invitations de page viennent ensuite).
    let store = Store::open(&t.db_path()).await.unwrap();
    store.add_member(space, gina.id, Role::Guest).await.unwrap();
    store.close().await;
    Team {
        sam,
        olga,
        adam,
        mona,
        tom,
        gina,
        space,
    }
}

/// Deux espaces d'un même compte ne partagent rien : ni membres, ni invitations, ni nom.
#[tokio::test]
async fn two_spaces_of_one_account_are_separate() {
    let t = open_app().await;
    let (alice_browser, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    let alice = Person {
        id: uuid(&me["user"]["id"]),
        browser: alice_browser,
    };
    let perso = uuid(&me["spaces"][0]["id"]);
    let reply = alice
        .browser
        .post("/api/v1/spaces", json!({ "name": "Pro" }))
        .await;
    reply.assert_status(StatusCode::CREATED);
    let pro = uuid(&reply.json()["id"]);
    assert_eq!(
        reply.json(),
        json!({ "id": pro.to_string(), "name": "Pro", "role": "owner" })
    );
    assert_ne!(pro, perso);
    assert_eq!(
        alice.browser.me().await.json()["spaces"],
        json!([
            { "id": perso.to_string(), "name": "Espace de Alice", "role": "owner" },
            { "id": pro.to_string(), "name": "Pro", "role": "owner" },
        ])
    );

    // Bob entre dans « Pro », pas dans l'autre.
    let bob = person(&t, "bob@exemple.fr", "Bob").await;
    join(&alice, &bob, pro, "member").await;
    assert_eq!(
        roles(&bob.browser, pro).await,
        [
            (alice.id, "owner".to_string()),
            (bob.id, "member".to_string())
        ]
    );
    assert_eq!(
        roles(&alice.browser, perso).await,
        [(alice.id, "owner".to_string())]
    );
    let foreign = bob.browser.get(&members_uri(perso)).await;
    foreign.assert_error(StatusCode::NOT_FOUND, "not_found");
    let missing = bob.browser.get(&members_uri(Uuid::now_v7())).await;
    assert_eq!(
        foreign.body, missing.body,
        "comme un espace qui n'existe pas"
    );
    let bob_spaces = bob.browser.me().await.json()["spaces"].clone();
    assert!(
        bob_spaces
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["id"] != perso.to_string()),
        "{bob_spaces}"
    );

    // Une invitation de l'un n'est pas dans la liste de l'autre.
    let (in_perso, _) = invited(&alice.browser, perso, "carol@exemple.fr", "member").await;
    let listed = |space| {
        let browser = &alice.browser;
        async move {
            let reply = browser.get(&invitations_uri(space)).await;
            reply.assert_status(StatusCode::OK);
            reply
                .json()
                .as_array()
                .unwrap()
                .iter()
                .map(|i| uuid(&i["id"]))
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(listed(perso).await, [in_perso]);
    assert!(listed(pro).await.is_empty());

    // Renommer l'un ne renomme pas l'autre.
    let reply = alice
        .browser
        .patch(&format!("/api/v1/spaces/{pro}"), json!({ "name": "Pro 2" }))
        .await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(
        reply.json(),
        json!({ "id": pro.to_string(), "name": "Pro 2", "role": "owner" })
    );
    let names: Vec<Value> = alice.browser.me().await.json()["spaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].clone())
        .collect();
    assert_eq!(names, [json!("Espace de Alice"), json!("Pro 2")]);
}

/// Nom d'espace rogné, de 1 à 100 caractères, à la création comme au renommage.
#[tokio::test]
async fn space_name_is_trimmed_and_bounded() {
    let t = open_app().await;
    let alice = person(&t, "alice@exemple.fr", "Alice").await;
    let reply = alice
        .browser
        .post("/api/v1/spaces", json!({ "name": "  Pro \n" }))
        .await;
    reply.assert_status(StatusCode::CREATED);
    assert_eq!(reply.json()["name"], "Pro");
    let pro = uuid(&reply.json()["id"]);
    let longest = "é".repeat(100);
    let reply = alice
        .browser
        .post("/api/v1/spaces", json!({ "name": longest }))
        .await;
    reply.assert_status(StatusCode::CREATED);
    assert_eq!(reply.json()["name"], longest);

    let spaces = query_i64(&t.db_path(), "SELECT COUNT(*) FROM spaces").await;
    for bad in ["", "   ", &"é".repeat(101)] {
        let body = alice
            .browser
            .post("/api/v1/spaces", json!({ "name": bad }))
            .await
            .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_name");
        assert_french(&body, &["100"]);
        alice
            .browser
            .patch(&format!("/api/v1/spaces/{pro}"), json!({ "name": bad }))
            .await
            .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_name");
    }
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM spaces").await,
        spaces,
        "rien de créé"
    );
    // Le nom refusé n'a rien changé.
    let me = alice.browser.me().await.json();
    assert!(
        me["spaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == pro.to_string() && s["name"] == "Pro")
    );
    // Pour qui n'a pas de rôle dans l'espace, un corps invalide ne change rien : `404`.
    let bob = person(&t, "bob@exemple.fr", "Bob").await;
    bob.browser
        .patch(&format!("/api/v1/spaces/{pro}"), json!({ "name": "" }))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
}

/// Statuts attendus de chaque route, dans l'ordre : propriétaire, admin, membre, invité,
/// compte étranger à l'espace (l'admin de l'instance), sans session. Tableau des rôles du
/// « Modèle des droits » : voir les membres dès `member` ; renommer, inviter, retirer dès
/// `admin` ; changer le rôle d'un membre entre `admin` et `member` dès `admin`.
const MATRIX: [(&str, [u16; 6]); 10] = [
    ("POST /api/v1/spaces", [201, 201, 201, 201, 201, 401]),
    ("PATCH /api/v1/spaces/{id}", [200, 200, 403, 403, 404, 401]),
    (
        "GET /api/v1/spaces/{id}/members",
        [200, 200, 200, 403, 404, 401],
    ),
    (
        "PATCH /api/v1/spaces/{id}/members/{tom}",
        [200, 200, 403, 403, 404, 401],
    ),
    (
        "DELETE /api/v1/spaces/{id}/members/{tom}",
        [204, 204, 403, 403, 404, 401],
    ),
    (
        "POST /api/v1/spaces/{id}/invitations",
        [201, 201, 403, 403, 404, 401],
    ),
    (
        "GET /api/v1/spaces/{id}/invitations",
        [200, 200, 403, 403, 404, 401],
    ),
    (
        "DELETE /api/v1/invitations/{invitation}",
        [204, 204, 403, 403, 404, 401],
    ),
    // Sans session : le jeton suffit.
    (
        "POST /api/v1/invitations/lookup",
        [200, 200, 200, 200, 200, 200],
    ),
    // En dernier : qui accepte entre dans l'espace (l'invitée et l'étranger y deviennent
    // membres).
    (
        "POST /api/v1/invitations/accept",
        [200, 200, 200, 200, 200, 401],
    ),
];

/// Une requête du tableau [`MATRIX`] : `route` sur `space`, Tom pour cible, et l'invitation
/// `(id, jeton)`.
async fn call(
    browser: &Browser<'_>,
    route: &str,
    space: Uuid,
    tom: Uuid,
    (invitation, token): (Uuid, &str),
) -> Reply {
    let at = format!("/api/v1/spaces/{space}");
    match route {
        "POST /api/v1/spaces" => {
            browser
                .post("/api/v1/spaces", json!({ "name": "Ailleurs" }))
                .await
        }
        "PATCH /api/v1/spaces/{id}" => browser.patch(&at, json!({ "name": "Équipe" })).await,
        "GET /api/v1/spaces/{id}/members" => browser.get(&members_uri(space)).await,
        "PATCH /api/v1/spaces/{id}/members/{tom}" => {
            browser
                .patch(&member_uri(space, tom), json!({ "role": "admin" }))
                .await
        }
        "DELETE /api/v1/spaces/{id}/members/{tom}" => browser.delete(&member_uri(space, tom)).await,
        "POST /api/v1/spaces/{id}/invitations" => {
            invite(browser, space, "nouveau@exemple.fr", "member").await
        }
        "GET /api/v1/spaces/{id}/invitations" => browser.get(&invitations_uri(space)).await,
        "DELETE /api/v1/invitations/{invitation}" => {
            browser
                .delete(&format!("/api/v1/invitations/{invitation}"))
                .await
        }
        "POST /api/v1/invitations/lookup" => lookup(browser, token).await,
        "POST /api/v1/invitations/accept" => accept(browser, token).await,
        other => unreachable!("route hors du tableau : {other}"),
    }
}

#[tokio::test]
async fn space_routes_by_role() {
    let t = open_app().await;
    let team = team(&t).await;
    let anonymous = t.browser();
    let who: [(&str, &Browser); 6] = [
        ("le propriétaire", &team.olga.browser),
        ("l'admin", &team.adam.browser),
        ("un membre", &team.mona.browser),
        ("l'invitée", &team.gina.browser),
        (
            "l'admin de l'instance, étranger à l'espace",
            &team.sam.browser,
        ),
        ("un navigateur sans session", &anonymous),
    ];
    let store = Store::open(&t.db_path()).await.unwrap();
    let (space, tom) = (team.space, team.tom.id);
    let unknown_space = Uuid::now_v7();

    for (route, expected) in MATRIX {
        for ((name, browser), status) in who.iter().zip(expected) {
            // Avant chaque appel : Tom est membre, et une invitation neuve d'Olga attend.
            store.add_member(space, tom, Role::Member).await.unwrap();
            let current = store.role_in(space, tom).await.unwrap().unwrap();
            let by_olga = Expected {
                actor: team.olga.id,
                actor_role: Role::Owner,
                target_role: current,
            };
            store
                .set_role(space, tom, Role::Member, by_olga)
                .await
                .unwrap();
            let (invitation, token) =
                invited(&team.olga.browser, space, "invite@exemple.fr", "member").await;

            let reply = call(browser, route, space, tom, (invitation, &token)).await;
            assert_eq!(
                reply.status.as_u16(),
                status,
                "{route} par {name} : {}",
                reply.text()
            );
            match status {
                401 => {
                    reply.assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
                }
                403 => {
                    reply.assert_error(StatusCode::FORBIDDEN, "forbidden");
                }
                404 => {
                    reply.assert_error(StatusCode::NOT_FOUND, "not_found");
                    // Comme un espace, ou une invitation, qui n'existe pas : même statut, même
                    // corps.
                    let missing =
                        call(browser, route, unknown_space, tom, (Uuid::now_v7(), &token)).await;
                    assert_eq!(
                        (missing.status, &missing.body),
                        (reply.status, &reply.body),
                        "{route} par {name}"
                    );
                }
                _ => {}
            }
        }
    }
    store.close().await;
}

/// Un admin ne change ni ne retire un propriétaire, et ne fait passer un rôle qu'entre `admin`
/// et `member` ; un propriétaire peut tout.
#[tokio::test]
async fn admin_cannot_touch_owners() {
    let t = open_app().await;
    let Team {
        olga,
        adam,
        mona,
        gina,
        space,
        ..
    } = team(&t).await;

    for role in ["owner", "admin", "member", "guest"] {
        adam.browser
            .patch(&member_uri(space, olga.id), json!({ "role": role }))
            .await
            .assert_error(StatusCode::FORBIDDEN, "forbidden");
    }
    adam.browser
        .delete(&member_uri(space, olga.id))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    // Ni propriétaire, ni invité ; un invité ne devient pas membre par un admin.
    for role in ["owner", "guest"] {
        adam.browser
            .patch(&member_uri(space, mona.id), json!({ "role": role }))
            .await
            .assert_error(StatusCode::FORBIDDEN, "forbidden");
    }
    adam.browser
        .patch(&member_uri(space, gina.id), json!({ "role": "member" }))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(
        role_of(&olga.browser, space, olga.id).await.as_deref(),
        Some("owner")
    );
    assert_eq!(
        role_of(&olga.browser, space, mona.id).await.as_deref(),
        Some("member")
    );
    assert_eq!(
        role_of(&olga.browser, space, gina.id).await.as_deref(),
        Some("guest")
    );

    // Entre `admin` et `member`, dans les deux sens : la réponse est le membre.
    let reply = adam
        .browser
        .patch(&member_uri(space, mona.id), json!({ "role": "admin" }))
        .await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(
        reply.json(),
        json!({
            "userId": mona.id.to_string(),
            "name": "Mona",
            "email": "mona@exemple.fr",
            "role": "admin",
        })
    );
    adam.browser
        .patch(&member_uri(space, mona.id), json!({ "role": "member" }))
        .await
        .assert_status(StatusCode::OK);
    // Un membre inconnu de l'espace : `404`.
    adam.browser
        .patch(
            &member_uri(space, Uuid::now_v7()),
            json!({ "role": "admin" }),
        )
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");

    // Le propriétaire, lui, fait d'Adam un propriétaire, qui peut alors la rétrograder.
    olga.browser
        .patch(&member_uri(space, adam.id), json!({ "role": "owner" }))
        .await
        .assert_status(StatusCode::OK);
    adam.browser
        .patch(&member_uri(space, olga.id), json!({ "role": "admin" }))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(
        role_of(&adam.browser, space, olga.id).await.as_deref(),
        Some("admin")
    );
    // Et un propriétaire fait d'un invité un membre.
    adam.browser
        .patch(&member_uri(space, gina.id), json!({ "role": "member" }))
        .await
        .assert_status(StatusCode::OK);
}

/// Un espace garde au moins un propriétaire : le dernier ne part pas, et ne change pas de rôle.
#[tokio::test]
async fn last_owner_cannot_leave_or_be_demoted() {
    let t = open_app().await;
    let Team {
        olga, adam, space, ..
    } = team(&t).await;

    let body = olga
        .browser
        .delete(&member_uri(space, olga.id))
        .await
        .assert_error(StatusCode::CONFLICT, "last_owner");
    assert_french(&body, &["propriétaire"]);
    for role in ["admin", "member", "guest"] {
        olga.browser
            .patch(&member_uri(space, olga.id), json!({ "role": role }))
            .await
            .assert_error(StatusCode::CONFLICT, "last_owner");
    }
    assert_eq!(
        role_of(&olga.browser, space, olga.id).await.as_deref(),
        Some("owner")
    );

    // Avec un second propriétaire, elle peut céder sa place ; c'est alors lui le dernier.
    olga.browser
        .patch(&member_uri(space, adam.id), json!({ "role": "owner" }))
        .await
        .assert_status(StatusCode::OK);
    olga.browser
        .patch(&member_uri(space, olga.id), json!({ "role": "admin" }))
        .await
        .assert_status(StatusCode::OK);
    adam.browser
        .delete(&member_uri(space, adam.id))
        .await
        .assert_error(StatusCode::CONFLICT, "last_owner");
    adam.browser
        .patch(&member_uri(space, adam.id), json!({ "role": "member" }))
        .await
        .assert_error(StatusCode::CONFLICT, "last_owner");
    // Admin, elle ne touche plus au propriétaire.
    olga.browser
        .delete(&member_uri(space, adam.id))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
}

/// Chacun peut quitter un espace, invité compris : il n'y voit plus rien, comme un étranger.
#[tokio::test]
async fn member_can_leave() {
    let t = open_app().await;
    let Team {
        olga,
        mona,
        tom,
        gina,
        space,
        ..
    } = team(&t).await;

    // Un membre ne retire personne d'autre que lui.
    mona.browser
        .delete(&member_uri(space, tom.id))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    mona.browser
        .delete(&member_uri(space, mona.id))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    mona.browser
        .get(&members_uri(space))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    mona.browser
        .delete(&member_uri(space, mona.id))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    let spaces = mona.browser.me().await.json()["spaces"].clone();
    assert!(
        spaces
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["id"] != space.to_string()),
        "{spaces}"
    );

    gina.browser
        .delete(&member_uri(space, gina.id))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let left: Vec<Uuid> = roles(&olga.browser, space)
        .await
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert!(!left.contains(&mona.id) && !left.contains(&gina.id));
    assert!(left.contains(&tom.id));
}

/// Inviter, consulter le lien sans session, accepter : la personne est membre ; le lien ne sert
/// plus. L'inscription accepte aussi une invitation, par le même chemin.
#[tokio::test]
async fn space_invitation_roundtrip() {
    let t = open_app().await;
    let olga = person(&t, "olga@exemple.fr", "Olga").await;
    let space = create_space(&olga, "Équipe").await;

    let reply = invite(&olga.browser, space, "  Bob@Exemple.FR ", "member").await;
    reply.assert_status(StatusCode::CREATED);
    let created = reply.json();
    let fields: Vec<&String> = created.as_object().unwrap().keys().collect();
    assert_eq!(fields, ["id", "token"]);
    let id = uuid(&created["id"]);
    let token = created["token"].as_str().unwrap().to_string();
    // 32 octets en base64url, sans remplissage.
    assert_eq!(token.len(), 43);
    assert_eq!(URL_SAFE_NO_PAD.decode(&token).unwrap().len(), 32);

    // Sans session.
    let anonymous = t.browser();
    let reply = lookup(&anonymous, &token).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(
        reply.json(),
        json!({
            "spaceName": "Équipe",
            "pageTitle": null,
            "invitedBy": "Olga",
            "email": "bob@exemple.fr",
        })
    );
    // En attente, sans le jeton.
    let pending = olga.browser.get(&invitations_uri(space)).await;
    pending.assert_status(StatusCode::OK);
    let pending = pending.json();
    assert_eq!(pending.as_array().unwrap().len(), 1);
    let expires_at = pending[0]["expiresAt"].as_i64().expect("expiresAt");
    assert_eq!(
        pending,
        json!([{
            "id": id.to_string(),
            "email": "bob@exemple.fr",
            "role": "member",
            "level": null,
            "pageId": null,
            "expiresAt": expires_at,
        }])
    );
    // Il faut une session pour accepter.
    accept(&anonymous, &token)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");

    // Bob, avec un compte à une autre adresse : c'est le jeton qui fait foi.
    let bob = person(&t, "robert@exemple.fr", "Bob").await;
    let reply = accept(&bob.browser, &token).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(
        reply.json(),
        json!({ "spaceId": space.to_string(), "pageId": null })
    );
    assert_eq!(
        role_of(&olga.browser, space, bob.id).await.as_deref(),
        Some("member")
    );
    let spaces = bob.browser.me().await.json()["spaces"].clone();
    assert!(
        spaces
            .as_array()
            .unwrap()
            .contains(&json!({ "id": space.to_string(), "name": "Équipe", "role": "member" })),
        "{spaces}"
    );

    // Le lien a servi : ni une seconde acceptation, ni une autre personne, ni une consultation.
    let carol = person(&t, "carol@exemple.fr", "Carol").await;
    let mut bodies = Vec::new();
    for reply in [
        accept(&bob.browser, &token).await,
        accept(&carol.browser, &token).await,
        lookup(&anonymous, &token).await,
    ] {
        reply.assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
        bodies.push(reply.body);
    }
    bodies.dedup();
    assert_eq!(bodies.len(), 1);
    assert_eq!(role_of(&olga.browser, space, carol.id).await, None);
    assert_eq!(
        olga.browser.get(&invitations_uri(space)).await.json(),
        json!([])
    );

    // À l'inscription : le compte entre dans l'espace au rôle de l'invitation, sans espace à
    // lui.
    let (_, token) = invited(&olga.browser, space, "dora@exemple.fr", "admin").await;
    let dora = t.browser();
    let reply = dora
        .sign_up("dora@exemple.fr", "Dora", common::PASSWORD, Some(&token))
        .await;
    reply.assert_status(StatusCode::CREATED);
    assert_eq!(
        reply.json()["spaces"],
        json!([{ "id": space.to_string(), "name": "Équipe", "role": "admin" }])
    );
    accept(&dora, &token)
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");

    // Ce qu'on n'invite pas : une adresse mal formée, un rôle autre que `admin` ou `member`.
    invite(&olga.browser, space, "pas une adresse", "member")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_email");
    for role in ["owner", "guest", "Admin", ""] {
        let body = invite(&olga.browser, space, "eve@exemple.fr", role)
            .await
            .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
        assert_french(&body, &["role"]);
    }
    // Un jeton mal formé : la même réponse qu'un jeton inconnu.
    for bad in ["", "pas-un-jeton", &"A".repeat(43)] {
        lookup(&anonymous, bad)
            .await
            .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
        accept(&carol.browser, bad)
            .await
            .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    }
}

/// Une invitation dure 7 jours ; échue, elle ne se consulte plus, ne s'accepte plus et quitte
/// la liste des invitations en attente.
#[tokio::test]
async fn invitation_expires_after_7_days() {
    let t = open_app().await;
    let olga = person(&t, "olga@exemple.fr", "Olga").await;
    let space = create_space(&olga, "Équipe").await;
    let before = now_ms();
    let (id, token) = invited(&olga.browser, space, "bob@exemple.fr", "member").await;
    let after = now_ms();

    // L'échéance est posée par le serveur, la date de création par le store, un instant après.
    let lifetime = query_i64(
        &t.db_path(),
        "SELECT expires_at - created_at FROM invitations",
    )
    .await;
    assert!(
        (WEEK_MS - 5_000..=WEEK_MS).contains(&lifetime),
        "{lifetime}"
    );
    let pending = olga.browser.get(&invitations_uri(space)).await.json();
    let expires_at = pending[0]["expiresAt"].as_i64().unwrap();
    assert!(
        (before + WEEK_MS..=after + WEEK_MS).contains(&expires_at),
        "{expires_at}"
    );
    let anonymous = t.browser();
    lookup(&anonymous, &token)
        .await
        .assert_status(StatusCode::OK);

    // L'échéance avancée à l'instant passé.
    exec_sql(
        &t.db_path(),
        &format!(
            "UPDATE invitations SET expires_at = {} WHERE id = '{id}'",
            now_ms() - 1
        ),
    )
    .await;
    lookup(&anonymous, &token)
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    let bob = person(&t, "bob@exemple.fr", "Bob").await;
    accept(&bob.browser, &token)
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    t.browser()
        .sign_up("carol@exemple.fr", "Carol", common::PASSWORD, Some(&token))
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    assert_eq!(role_of(&olga.browser, space, bob.id).await, None);
    assert_eq!(
        olga.browser.get(&invitations_uri(space)).await.json(),
        json!([])
    );
    // Échue, elle n'existe plus : la révoquer répond `404`.
    olga.browser
        .delete(&format!("/api/v1/invitations/{id}"))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
}

/// Une invitation révoquée ne vaut plus rien, et sa révocation ne se refait pas.
#[tokio::test]
async fn revoked_invitation_is_404() {
    let t = open_app().await;
    let olga = person(&t, "olga@exemple.fr", "Olga").await;
    let space = create_space(&olga, "Équipe").await;
    let (id, token) = invited(&olga.browser, space, "bob@exemple.fr", "member").await;

    olga.browser
        .delete(&format!("/api/v1/invitations/{id}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let anonymous = t.browser();
    lookup(&anonymous, &token)
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    let bob = person(&t, "bob@exemple.fr", "Bob").await;
    accept(&bob.browser, &token)
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    t.browser()
        .sign_up("carol@exemple.fr", "Carol", common::PASSWORD, Some(&token))
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    assert_eq!(role_of(&olga.browser, space, bob.id).await, None);
    assert_eq!(
        olga.browser.get(&invitations_uri(space)).await.json(),
        json!([])
    );

    // Révoquée, acceptée, inconnue, mal écrite : la même réponse.
    let (accepted, accepted_token) =
        invited(&olga.browser, space, "dan@exemple.fr", "member").await;
    accept(&bob.browser, &accepted_token)
        .await
        .assert_status(StatusCode::OK);
    let mut bodies = Vec::new();
    for target in [
        id.to_string(),
        accepted.to_string(),
        Uuid::now_v7().to_string(),
        "pas-un-uuid".to_string(),
    ] {
        let reply = olga
            .browser
            .delete(&format!("/api/v1/invitations/{target}"))
            .await;
        reply.assert_error(StatusCode::NOT_FOUND, "not_found");
        bodies.push(reply.body);
    }
    bodies.dedup();
    assert_eq!(bodies.len(), 1);
    // L'invitation acceptée reste un droit : Bob est toujours membre.
    assert_eq!(
        role_of(&olga.browser, space, bob.id).await.as_deref(),
        Some("member")
    );
}

/// Accepter une invitation ne baisse jamais un rôle ; elle peut en monter un.
#[tokio::test]
async fn accept_never_downgrades_a_role() {
    let t = open_app().await;
    let Team {
        olga,
        adam,
        mona,
        gina,
        space,
        ..
    } = team(&t).await;

    for (who, asked, kept) in [
        (&adam, "member", "admin"),
        (&olga, "admin", "owner"),
        (&olga, "member", "owner"),
        (&mona, "member", "member"),
        (&mona, "admin", "admin"),
        (&gina, "member", "member"),
    ] {
        let (_, token) = invited(&olga.browser, space, "x@exemple.fr", asked).await;
        let reply = accept(&who.browser, &token).await;
        reply.assert_status(StatusCode::OK);
        assert_eq!(
            reply.json(),
            json!({ "spaceId": space.to_string(), "pageId": null })
        );
        assert_eq!(
            role_of(&olga.browser, space, who.id).await.as_deref(),
            Some(kept),
            "invitation {asked}"
        );
        // Consommée, même sans effet.
        accept(&who.browser, &token)
            .await
            .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    }
}

/// Le jeton d'une invitation n'est montré qu'une fois, dans la réponse qui la crée : ni dans
/// la liste, ni dans une autre réponse, ni dans les journaux, ni en clair dans la base.
#[tokio::test]
async fn invitation_token_is_shown_once() {
    // Avant toute requête : les journaux de tous les fils, au niveau TRACE.
    let logs = all_logs();
    let t = open_app().await;
    let olga = person(&t, "olga@exemple.fr", "Olga").await;
    let space = create_space(&olga, "Équipe").await;
    let (id, token) = invited(&olga.browser, space, "bob@exemple.fr", "member").await;
    let bytes = URL_SAFE_NO_PAD.decode(&token).unwrap();
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();

    let anonymous = t.browser();
    let bob = person(&t, "bob@exemple.fr", "Bob").await;
    let list = olga.browser.get(&invitations_uri(space)).await;
    list.assert_status(StatusCode::OK);
    assert!(list.text().contains(&id.to_string()));
    let replies = [
        list,
        lookup(&anonymous, &token).await,
        accept(&bob.browser, &token).await,
        accept(&bob.browser, &token).await,
        olga.browser.get(&invitations_uri(space)).await,
    ];
    for reply in &replies {
        let text = format!("{:?}\n{}", reply.headers, reply.text());
        for secret in [token.as_str(), &hash, &hash.to_lowercase()] {
            assert!(!text.contains(secret), "« {secret} » dans : {text}");
        }
    }

    // Les journaux, lus pour de bon : la création et l'acceptation y sont, le jeton non.
    let text = logs.text();
    for expected in ["invitation créée", "invitation acceptée", &id.to_string()] {
        assert!(
            text.contains(expected),
            "« {expected} » absent des journaux"
        );
    }
    for secret in [token.as_str(), &hash, &hash.to_lowercase()] {
        assert!(!text.contains(secret), "« {secret} » dans les journaux");
    }

    // La base garde le SHA-256 des 32 octets, jamais le jeton.
    assert_eq!(
        query_i64(
            &t.db_path(),
            &format!("SELECT COUNT(*) FROM invitations WHERE hex(token_hash) = '{hash}'")
        )
        .await,
        1
    );
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&t.cfg.data_dir).unwrap() {
        files.extend(std::fs::read(entry.unwrap().path()).unwrap());
    }
    assert!(
        !files
            .windows(token.len())
            .any(|window| window == token.as_bytes()),
        "jeton en clair dans les fichiers de la base"
    );
}

/// Les droits se relisent dès que `spaces.rev` bouge : un membre retiré perd l'espace à
/// l'appel suivant, un admin rétrogradé ses droits d'admin, sans attendre que le cache se vide.
#[tokio::test]
async fn access_cache_follows_rev() {
    let t = open_app().await;
    let Team {
        olga,
        adam,
        mona,
        space,
        ..
    } = team(&t).await;

    // Calculés une première fois, donc en cache.
    mona.browser
        .get(&members_uri(space))
        .await
        .assert_status(StatusCode::OK);
    adam.browser
        .get(&invitations_uri(space))
        .await
        .assert_status(StatusCode::OK);

    olga.browser
        .delete(&member_uri(space, mona.id))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    mona.browser
        .get(&members_uri(space))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");

    olga.browser
        .patch(&member_uri(space, adam.id), json!({ "role": "member" }))
        .await
        .assert_status(StatusCode::OK);
    adam.browser
        .get(&invitations_uri(space))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    adam.browser
        .get(&members_uri(space))
        .await
        .assert_status(StatusCode::OK);

    // Une écriture faite hors des routes (par le store) compte aussi : elle incrémente `rev`.
    let store = Store::open(&t.db_path()).await.unwrap();
    let by_olga = Expected {
        actor: olga.id,
        actor_role: Role::Owner,
        target_role: Role::Member,
    };
    store.remove_member(space, adam.id, by_olga).await.unwrap();
    store.close().await;
    adam.browser
        .get(&members_uri(space))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
}

/// Retirer un membre ou changer un rôle ferme les connexions de synchro des pages de cet
/// espace (elles se rouvriront en relisant leurs droits), et seulement celles-là.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_changes_close_live_connections_of_the_space_only() {
    let t = open_app().await;
    let (olga_browser, me) = t.signed_up("olga@exemple.fr", "Olga").await;
    let olga = Person {
        id: uuid(&me["user"]["id"]),
        browser: olga_browser,
    };
    let home = uuid(&me["spaces"][0]["id"]);
    let elsewhere = create_space(&olga, "Ailleurs").await;
    let mona = person(&t, "mona@exemple.fr", "Mona").await;
    let tom = person(&t, "tom@exemple.fr", "Tom").await;
    join(&olga, &mona, elsewhere, "member").await;
    join(&olga, &tom, home, "member").await;

    let addr = t.listen().await;
    let page = olga
        .browser
        .post(
            "/api/v1/pages",
            json!({ "spaceId": home.to_string(), "section": "space", "title": "En direct" }),
        )
        .await;
    page.assert_status(StatusCode::CREATED);
    let page = uuid(&page.json()["id"]).to_string();
    let url = format!("ws://{addr}/sync/{page}?schema={SCHEMA_VERSION}");
    let live = Client::connect_synced(&url).await;

    // Dans l'autre espace : la connexion reste ouverte, et ce qu'elle envoie arrive.
    olga.browser
        .patch(&member_uri(elsewhere, mona.id), json!({ "role": "admin" }))
        .await
        .assert_status(StatusCode::OK);
    olga.browser
        .delete(&member_uri(elsewhere, mona.id))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let block = live.blocks()[0].id.clone();
    let start = live.step2_count();
    live.type_text(&block, "toujours là");
    live.resync();
    assert!(live.wait_until(WAIT, |c| c.step2_count() > start).await);
    assert!(!live.is_closed());
    assert_eq!(olga.browser.get_mmd(&page).await.text(), "toujours là\n");

    // Dans son espace : un changement de rôle la ferme.
    olga.browser
        .patch(&member_uri(home, tom.id), json!({ "role": "admin" }))
        .await
        .assert_status(StatusCode::OK);
    assert!(
        live.wait_until(WAIT, |c| c.is_closed()).await,
        "connexion fermée après le changement de rôle"
    );
    // Un retrait aussi.
    let again = Client::connect_synced(&url).await;
    olga.browser
        .delete(&member_uri(home, tom.id))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert!(
        again.wait_until(WAIT, |c| c.is_closed()).await,
        "connexion fermée après le retrait"
    );
}
