//! Les commentaires d'une page (Ruling R134) : qui peut quoi, à chaque niveau, auteur ou non,
//! à la corbeille ou non ; un fil attaché à un bloc qui disparaît ; ce que reçoivent les
//! connexions de synchro de la page ; les bornes du texte d'un message et d'un emoji.
//!
//! Les statuts attendus sont écrits ici d'après le tableau « Commentaires (R134) » du plan du
//! lot 3, sans passer par le calcul de `mementum-acl`. Les fils de départ sont posés par le
//! store : ce ne sont pas eux qu'on teste.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::sync::{members, open_app, still_served, user};
use common::{
    Browser, Reply, Team, TestApp, WAIT, assert_french, json_request, outcome, sync_url, synced_as,
    uuid,
};
use mementum_acl::Level;
use mementum_store::Authorship;
use mementum_sync::testkit::Client;
use serde_json::{Value, json};
use uuid::Uuid;

fn comments_uri(page: impl std::fmt::Display) -> String {
    format!("/api/v1/pages/{page}/comments")
}

fn thread_uri(page: impl std::fmt::Display, thread: impl std::fmt::Display) -> String {
    format!("{}/{thread}", comments_uri(page))
}

fn message_uri(
    page: impl std::fmt::Display,
    thread: impl std::fmt::Display,
    message: impl std::fmt::Display,
) -> String {
    format!("{}/messages/{message}", thread_uri(page, thread))
}

fn reaction_uri(
    page: impl std::fmt::Display,
    thread: impl std::fmt::Display,
    message: impl std::fmt::Display,
    emoji: &str,
) -> String {
    format!(
        "{}/reactions/{}",
        message_uri(page, thread, message),
        encoded(emoji)
    )
}

/// `text` encodé pour un segment de chemin : tout octet hors des caractères non réservés
/// d'une URL s'écrit `%XX`.
fn encoded(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// `PUT` sans corps.
async fn put_empty(by: &Browser<'_>, uri: &str) -> Reply {
    by.send(Request::put(uri).body(Body::empty()).unwrap())
        .await
}

/// Les trames de page reçues par `client`, hors liste des présents.
fn events(client: &Client) -> Vec<Value> {
    client
        .page_events()
        .iter()
        .map(|json| serde_json::from_str::<Value>(json).unwrap())
        .filter(|frame| frame["type"] != "roster")
        .collect()
}

// ---------------------------------------------------------------------------------------------
// La matrice.

/// Un niveau, du plus faible au plus fort : l'ordre des variantes est celui du tableau.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lvl {
    Read,
    Comment,
    Edit,
    Full,
}

const LEVELS: [Lvl; 4] = [Lvl::Read, Lvl::Comment, Lvl::Edit, Lvl::Full];

fn acl_level(level: Lvl) -> Level {
    match level {
        Lvl::Read => Level::Read,
        Lvl::Comment => Level::Comment,
        Lvl::Edit => Level::Edit,
        Lvl::Full => Level::Full,
    }
}

/// Ce qui est demandé : chaque route des commentaires, `PATCH` d'un fil dans les deux sens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    List,
    Open,
    Reply,
    Resolve,
    Reopen,
    Edit,
    Delete,
    React,
    Unreact,
}

const ACTS: [Act; 9] = [
    Act::List,
    Act::Open,
    Act::Reply,
    Act::Resolve,
    Act::Reopen,
    Act::Edit,
    Act::Delete,
    Act::React,
    Act::Unreact,
];

/// Qui a ouvert le fil, et qui a écrit le message que vise la requête : Mona (qui demande) ou
/// Tom. Le tableau R134 juge les deux à part : modifier ou supprimer un message regarde son
/// auteur, résoudre ou rouvrir regarde qui a ouvert le fil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owners {
    /// Mona a ouvert le fil ; le message est le sien, le premier.
    Mine,
    /// Tom a ouvert le fil ; le message est le sien, le premier.
    Others,
    /// Mona a ouvert le fil ; le message est une réponse de Tom.
    MyThreadTheirReply,
    /// Tom a ouvert le fil ; le message est une réponse de Mona.
    TheirThreadMyReply,
    /// Mona a ouvert le fil, puis supprimé son premier message : reste la réponse de Tom.
    MyThreadOnlyTheirReply,
    /// Tom a ouvert le fil, puis supprimé son premier message : reste la réponse de Mona.
    TheirThreadOnlyMyReply,
}

const OWNERS: [Owners; 6] = [
    Owners::Mine,
    Owners::Others,
    Owners::MyThreadTheirReply,
    Owners::TheirThreadMyReply,
    Owners::MyThreadOnlyTheirReply,
    Owners::TheirThreadOnlyMyReply,
];

impl Owners {
    /// Mona a ouvert le fil.
    fn opened(self) -> bool {
        matches!(
            self,
            Owners::Mine | Owners::MyThreadTheirReply | Owners::MyThreadOnlyTheirReply
        )
    }

    /// Le message visé est de Mona.
    fn wrote(self) -> bool {
        matches!(
            self,
            Owners::Mine | Owners::TheirThreadMyReply | Owners::TheirThreadOnlyMyReply
        )
    }

    /// Le premier message, celui de qui a ouvert le fil, a été supprimé.
    fn first_deleted(self) -> bool {
        matches!(
            self,
            Owners::MyThreadOnlyTheirReply | Owners::TheirThreadOnlyMyReply
        )
    }
}

/// Ce que reçoit Mona, de niveau `level` sur la page, selon qui a ouvert le fil et écrit le
/// message (`owners`) : le tableau « Commentaires (R134) ». Un refus de niveau ou d'auteur
/// passe avant la corbeille (`403`, puis `409 trashed`), et la lecture passe toujours.
fn expected(
    level: Lvl,
    owners: Owners,
    trashed: bool,
    act: Act,
) -> (StatusCode, Option<&'static str>) {
    let commenter = level >= Lvl::Comment;
    let (opened, wrote) = (owners.opened(), owners.wrote());
    let (success, allowed) = match act {
        Act::List => return (StatusCode::OK, None),
        Act::Open | Act::Reply => (StatusCode::CREATED, commenter),
        Act::React | Act::Unreact => (StatusCode::OK, commenter),
        // Modifier son message : `comment`, et en être l'auteur.
        Act::Edit => (StatusCode::OK, commenter && wrote),
        // Supprimer un message : en être l'auteur avec `comment`, ou `full`.
        Act::Delete => (
            StatusCode::NO_CONTENT,
            commenter && (wrote || level >= Lvl::Full),
        ),
        // Résoudre ou rouvrir un fil : l'avoir ouvert avec `comment`, ou `edit`.
        Act::Resolve | Act::Reopen => (StatusCode::OK, commenter && (opened || level >= Lvl::Edit)),
    };
    if !allowed {
        return (StatusCode::FORBIDDEN, Some("forbidden"));
    }
    if trashed {
        return (StatusCode::CONFLICT, Some("trashed"));
    }
    (success, None)
}

/// Envoie la requête de `act`, sur le fil `thread` et son message `message`.
async fn attempt(by: &Browser<'_>, act: Act, page: Uuid, thread: Uuid, message: Uuid) -> Reply {
    match act {
        Act::List => by.get(&comments_uri(page)).await,
        Act::Open => {
            by.post(&comments_uri(page), json!({ "body": "Nouveau fil" }))
                .await
        }
        Act::Reply => {
            let uri = format!("{}/replies", thread_uri(page, thread));
            by.post(&uri, json!({ "body": "Réponse" })).await
        }
        Act::Resolve => {
            by.patch(&thread_uri(page, thread), json!({ "resolved": true }))
                .await
        }
        Act::Reopen => {
            by.patch(&thread_uri(page, thread), json!({ "resolved": false }))
                .await
        }
        Act::Edit => {
            let uri = message_uri(page, thread, message);
            by.patch(&uri, json!({ "body": "Modifié" })).await
        }
        Act::Delete => by.delete(&message_uri(page, thread, message)).await,
        Act::React => put_empty(by, &reaction_uri(page, thread, message, "🎉")).await,
        Act::Unreact => by.delete(&reaction_uri(page, thread, message, "👍")).await,
    }
}

/// Chaque niveau, chaque façon d'avoir ouvert le fil et écrit le message (`Owners`), à la
/// corbeille ou non, sur chaque route : le statut est celui du tableau, un refus n'écrit rien,
/// une écriture permise change ce qu'on relit. Chaque case a sa propre page, avec un fil neuf :
/// elle ne change rien pour la suivante.
#[tokio::test]
async fn comment_routes_follow_the_levels() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, tom) = (&team.mona, &team.tom);
    let store = &team.store;
    let mut cells = 0;
    for level in LEVELS {
        for owners in OWNERS {
            for trashed in [false, true] {
                for act in ACTS {
                    let opener = if owners.opened() { mona.id } else { tom.id };
                    let writer = if owners.wrote() { mona.id } else { tom.id };
                    let grants = [user(mona, acl_level(level)), user(tom, Level::Full)];
                    let page = team.page(None, "Commentée", &grants).await;
                    let thread = store
                        .create_thread(page, None, opener, "Ouverture")
                        .await
                        .unwrap();
                    let first = thread.comments[0].id;
                    let message = if writer == opener && !owners.first_deleted() {
                        first
                    } else {
                        store
                            .reply_to_thread(page, thread.id, writer, "Réponse")
                            .await
                            .unwrap()
                            .id
                    };
                    if owners.first_deleted() {
                        store
                            .delete_comment(page, thread.id, first, Authorship::Any)
                            .await
                            .unwrap();
                    }
                    if act == Act::Reopen {
                        store
                            .resolve_thread(page, thread.id, tom.id, true, Authorship::Any)
                            .await
                            .unwrap();
                    }
                    if act == Act::Unreact {
                        store
                            .add_reaction(page, thread.id, message, mona.id, "👍")
                            .await
                            .unwrap();
                    }
                    if trashed {
                        team.trash(page).await;
                    }
                    let before = store.comment_threads(page).await.unwrap();

                    let reply = attempt(&mona.browser, act, page, thread.id, message).await;
                    let after = store.comment_threads(page).await.unwrap();
                    let (status, code) = expected(level, owners, trashed, act);
                    let cell = format!(
                        "{level:?}, {owners:?}, {}, {act:?}",
                        if trashed {
                            "à la corbeille"
                        } else {
                            "vivante"
                        }
                    );
                    assert_eq!(reply.status, status, "{cell} : {}", reply.text());
                    match code {
                        Some(code) => {
                            let body = reply.assert_error(status, code);
                            // Le refus de niveau nomme le niveau qu'il faut ; celui de l'auteur
                            // dit à qui est le message ou le fil.
                            let why = match (code, level) {
                                ("forbidden", Lvl::Read) => "il faut « Peut commenter »",
                                ("forbidden", _) => "n'est pas le tien",
                                _ => "corbeille",
                            };
                            assert_french(&body, &[why]);
                            assert_eq!(after, before, "{cell} : rien n'est écrit");
                        }
                        None if act == Act::List => {
                            assert_eq!(after, before, "{cell}");
                            let threads = reply.json();
                            assert_eq!(threads[0]["id"], thread.id.to_string(), "{cell}");
                        }
                        None => assert_ne!(after, before, "{cell} : rien n'a changé"),
                    }
                    cells += 1;
                }
            }
        }
    }
    assert_eq!(cells, 4 * 6 * 2 * 9, "toutes les cases ont été jouées");
}

// ---------------------------------------------------------------------------------------------
// Ce que les routes rendent.

/// Les fils se relisent du plus ancien au plus récent, résolus compris, chaque message avec son
/// auteur (identifiant et nom, jamais l'adresse) et ses réactions. Les réponses des écritures
/// ont la forme de ce qu'on relit.
#[tokio::test]
async fn threads_read_back_with_their_authors_and_no_address() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, tom) = (&team.mona, &team.tom);
    let page = team.page(None, "Commentée", &[members(Level::Full)]).await;

    let opened = mona
        .browser
        .post(&comments_uri(page), json!({ "body": "  Premier fil  " }))
        .await;
    opened.assert_status(StatusCode::CREATED);
    let opened = opened.json();
    let thread = uuid(&opened["id"]);
    let first = uuid(&opened["comments"][0]["id"]);
    let replied = tom
        .browser
        .post(
            &format!("{}/replies", thread_uri(page, thread)),
            json!({ "body": "Réponse\nsur deux lignes" }),
        )
        .await;
    replied.assert_status(StatusCode::CREATED);
    let replied = replied.json();
    let reacted = put_empty(&tom.browser, &reaction_uri(page, thread, first, "👍")).await;
    reacted.assert_status(StatusCode::OK);
    let second = tom
        .browser
        .post(
            &comments_uri(page),
            json!({ "body": "Second fil", "blockId": null }),
        )
        .await;
    second.assert_status(StatusCode::CREATED);
    let resolved = mona
        .browser
        .patch(&thread_uri(page, thread), json!({ "resolved": true }))
        .await;
    resolved.assert_status(StatusCode::OK);

    let read = mona.browser.get(&comments_uri(page)).await;
    read.assert_status(StatusCode::OK);
    let threads = read.json();
    let author = |account: &common::Account<'_>, name: &str| json!({ "id": account.id.to_string(), "name": name });
    let resolved_at = threads[0]["resolvedAt"].clone();
    assert!(resolved_at.is_number(), "{threads}");
    assert_eq!(
        threads[0],
        json!({
            "id": thread.to_string(),
            "blockId": null,
            "openedBy": author(mona, "Mona"),
            "createdAt": opened["createdAt"],
            "resolvedAt": resolved_at,
            "resolvedBy": author(mona, "Mona"),
            "comments": [
                {
                    "id": first.to_string(),
                    "author": author(mona, "Mona"),
                    "body": "Premier fil",
                    "createdAt": opened["comments"][0]["createdAt"],
                    "editedAt": null,
                    "reactions": [{ "emoji": "👍", "users": [author(tom, "Tom")] }],
                },
                replied,
            ],
        })
    );
    assert_eq!(threads[1], second.json());
    assert_eq!(threads.as_array().map(Vec::len), Some(2));
    assert_eq!(reacted.json(), threads[0]["comments"][0]);
    assert_eq!(resolved.json(), threads[0]);
    // Aucune adresse, de personne.
    let text = read.text();
    for account in [mona, tom] {
        let email = team
            .store
            .get_user(account.id)
            .await
            .unwrap()
            .unwrap()
            .email;
        assert!(!text.contains(&email), "{text}");
    }
    assert!(!text.contains("email"), "{text}");

    // On ne retire que sa propre réaction : celle de Tom reste quand Mona retire « la sienne ».
    let unreacted = mona
        .browser
        .delete(&reaction_uri(page, thread, first, "👍"))
        .await;
    unreacted.assert_status(StatusCode::OK);
    assert_eq!(unreacted.json(), threads[0]["comments"][0]);
}

/// Un fil dit qui l'a ouvert (`openedBy`, identifiant et nom), dans la réponse qui le crée,
/// dans celle qui le résout et à la lecture ; il le dit encore quand le premier message, celui
/// de qui l'a ouvert, a été supprimé : c'est toujours elle qui peut le résoudre avec `comment`.
#[tokio::test]
async fn thread_says_who_opened_it() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, tom) = (&team.mona, &team.tom);
    let page = team
        .page(
            None,
            "Commentée",
            &[user(mona, Level::Comment), user(tom, Level::Comment)],
        )
        .await;
    let opener = json!({ "id": mona.id.to_string(), "name": "Mona" });

    let opened = mona
        .browser
        .post(&comments_uri(page), json!({ "body": "Ouvert par Mona" }))
        .await;
    opened.assert_status(StatusCode::CREATED);
    let opened = opened.json();
    assert_eq!(opened["openedBy"], opener);
    let (thread, first) = (uuid(&opened["id"]), uuid(&opened["comments"][0]["id"]));
    tom.browser
        .post(
            &format!("{}/replies", thread_uri(page, thread)),
            json!({ "body": "Réponse de Tom" }),
        )
        .await
        .assert_status(StatusCode::CREATED);
    mona.browser
        .delete(&message_uri(page, thread, first))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let read = mona.browser.get(&comments_uri(page)).await.json();
    assert_eq!(read[0]["openedBy"], opener);
    assert_eq!(read[0]["comments"][0]["author"]["name"], "Tom");
    // Tom, seul auteur qui reste, ne résout pas le fil de Mona ; elle, oui.
    let uri = thread_uri(page, thread);
    tom.browser
        .patch(&uri, json!({ "resolved": true }))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    let resolved = mona.browser.patch(&uri, json!({ "resolved": true })).await;
    resolved.assert_status(StatusCode::OK);
    assert_eq!(resolved.json()["openedBy"], opener);
    assert_eq!(resolved.json()["resolvedBy"], opener);
}

/// Un fil attaché à un bloc lui survit : le bloc supprimé par `PUT /mmd`, le fil se lit
/// toujours, avec son `blockId` ; un fil neuf ne peut plus s'y attacher.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_survives_its_block() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({ "title": "Page" })).await.id;
    alice
        .put_mmd(&page, "Premier\n\nSecond\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let client = synced_as(&alice.browser, &sync_url(addr, &page)).await;
    let second = client.blocks()[1].id.clone();

    let opened = alice
        .browser
        .post(
            &comments_uri(&page),
            json!({ "blockId": second, "body": "Sur le second" }),
        )
        .await;
    opened.assert_status(StatusCode::CREATED);
    let opened = opened.json();
    assert_eq!(opened["blockId"], second);

    alice
        .put_mmd(&page, "Premier\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert!(
        client
            .wait_until(WAIT, |c| c.blocks().iter().all(|b| b.id != second))
            .await,
        "le bloc est toujours dans le document"
    );
    let read = alice.browser.get(&comments_uri(&page)).await;
    read.assert_status(StatusCode::OK);
    assert_eq!(read.json(), json!([opened]));
    alice
        .browser
        .post(
            &comments_uri(&page),
            json!({ "blockId": second, "body": "Trop tard" }),
        )
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_block");
}

/// Un `blockId` qui ne nomme aucun bloc de la page (inconnu, vide, ou d'une autre page) :
/// `422 invalid_block`, et rien n'est écrit. Un bloc imbriqué compte ; sans `blockId`, ou
/// `null`, c'est un commentaire de la page.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_block_is_refused() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({ "title": "Page" })).await.id;
    let other = alice.create(json!({ "title": "Autre" })).await.id;
    alice
        .put_mmd(&page, "- Parent\n  - Enfant\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    alice
        .put_mmd(&other, "Ailleurs\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let here = synced_as(&alice.browser, &sync_url(addr, &page))
        .await
        .blocks();
    let elsewhere = synced_as(&alice.browser, &sync_url(addr, &other))
        .await
        .blocks()[0]
        .id
        .clone();
    let nested = here[0].children[0].id.clone();
    let open = async |body: Value| alice.browser.post(&comments_uri(&page), body).await;

    for (block, why) in [
        ("inexistant", "inconnu"),
        ("", "vide"),
        (elsewhere.as_str(), "d'une autre page"),
    ] {
        let reply = open(json!({ "blockId": block, "body": "x" })).await;
        let body = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_block");
        assert_french(&body, &["bloc"]);
        assert!(!body.to_string().contains(&elsewhere), "{why} : {body}");
    }
    let read = alice.browser.get(&comments_uri(&page)).await;
    assert_eq!(read.json(), json!([]));

    let on_nested = open(json!({ "blockId": nested, "body": "Sur l'enfant" })).await;
    on_nested.assert_status(StatusCode::CREATED);
    assert_eq!(on_nested.json()["blockId"], nested);
    for body in [
        json!({ "body": "Sans bloc" }),
        json!({ "blockId": null, "body": "Nul" }),
    ] {
        let reply = open(body).await;
        reply.assert_status(StatusCode::CREATED);
        assert_eq!(reply.json()["blockId"], Value::Null);
    }
}

/// Le texte d'un message : de 1 à 10 000 caractères une fois les espaces de bord retirés (des
/// caractères, pas des octets), sans caractère de contrôle autre que le saut de ligne. La même
/// règle pour un fil neuf, une réponse et une modification ; un refus n'écrit rien.
#[tokio::test]
async fn body_bounds() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Page" })).await.id;
    let open = async |body: &str| {
        alice
            .browser
            .post(&comments_uri(&page), json!({ "body": body }))
            .await
    };

    let longest = "👍".repeat(10_000);
    for (body, kept) in [
        ("x", "x"),
        ("  bord  ", "bord"),
        ("\n\nlignes\nmultiples\n", "lignes\nmultiples"),
        (longest.as_str(), longest.as_str()),
    ] {
        let reply = open(body).await;
        reply.assert_status(StatusCode::CREATED);
        assert_eq!(reply.json()["comments"][0]["body"], kept);
    }
    let padded = format!("  {longest}\n");
    open(&padded).await.assert_status(StatusCode::CREATED);
    let count = alice.browser.get(&comments_uri(&page)).await.json();
    let written = count.as_array().map(Vec::len);

    let too_long = format!("{longest}a");
    for (body, why) in [
        ("", "vide"),
        ("   \n\t ", "blanc"),
        (too_long.as_str(), "10 000"),
        ("a\tb", "contrôle"),
        ("a\rb", "contrôle"),
        ("a\u{0}b", "contrôle"),
        ("a\u{7f}b", "contrôle"),
        ("a\u{85}b", "contrôle"),
    ] {
        let reply = open(body).await;
        let error = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_body");
        let expected = if why == "blanc" { "vide" } else { why };
        assert_french(&error, &[expected]);
    }
    let after = alice.browser.get(&comments_uri(&page)).await.json();
    assert_eq!(after.as_array().map(Vec::len), written);

    // Une réponse et une modification suivent la même règle.
    let thread = uuid(&after[0]["id"]);
    let message = uuid(&after[0]["comments"][0]["id"]);
    let replies = format!("{}/replies", thread_uri(&page, thread));
    let edit = message_uri(&page, thread, message);
    for uri in [&replies, &edit] {
        let method = if uri == &replies {
            Method::POST
        } else {
            Method::PATCH
        };
        for (body, status) in [
            (too_long.as_str(), StatusCode::UNPROCESSABLE_ENTITY),
            ("a\tb", StatusCode::UNPROCESSABLE_ENTITY),
            ("   ", StatusCode::UNPROCESSABLE_ENTITY),
            (longest.as_str(), StatusCode::OK),
        ] {
            let request = json_request(method.clone(), uri, &json!({ "body": body }));
            let reply = alice.browser.send(request).await;
            match status {
                StatusCode::OK => {
                    let expected = if method == Method::POST {
                        StatusCode::CREATED
                    } else {
                        StatusCode::OK
                    };
                    reply.assert_status(expected);
                }
                _ => {
                    reply.assert_error(status, "invalid_body");
                }
            }
        }
    }
}

/// Un emoji : de 1 à 16 octets, sans espace ni caractère de contrôle, pour poser comme pour
/// retirer une réaction ; un refus n'écrit rien.
#[tokio::test]
async fn emoji_bounds() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Page" })).await.id;
    let opened = alice
        .browser
        .post(&comments_uri(&page), json!({ "body": "Un" }))
        .await
        .json();
    let (thread, message) = (uuid(&opened["id"]), uuid(&opened["comments"][0]["id"]));
    let uri = |emoji: &str| reaction_uri(&page, thread, message, emoji);

    // 16 octets : quatre emojis de quatre octets.
    let sixteen = "👍".repeat(4);
    for emoji in ["👍", "a", "+1", sixteen.as_str()] {
        let reply = put_empty(&alice.browser, &uri(emoji)).await;
        reply.assert_status(StatusCode::OK);
        let reactions = reply.json()["reactions"].clone();
        assert!(
            reactions
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["emoji"] == emoji),
            "{emoji} : {reactions}"
        );
    }
    let seventeen = format!("{sixteen}a");
    // Une famille écrite en emojis joints (25 octets) dépasse la borne.
    let family = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
    for (emoji, why) in [
        (seventeen.as_str(), "16 octets"),
        (family, "16 octets"),
        (" ", "espace"),
        ("a b", "espace"),
        ("\u{a0}", "espace"),
        ("\u{0}", "contrôle"),
        ("\t", "contrôle"),
        ("\u{7f}", "contrôle"),
    ] {
        for refused in [
            put_empty(&alice.browser, &uri(emoji)).await,
            alice.browser.delete(&uri(emoji)).await,
        ] {
            let body = refused.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_emoji");
            assert_french(&body, &[why]);
        }
    }
    // Des octets qui ne sont pas de l'UTF-8 : axum refuse le chemin entier, et la route répond
    // comme pour tout chemin illisible (Ruling R172).
    let raw = format!(
        "{}/messages/{message}/reactions/%FF",
        thread_uri(&page, thread)
    );
    let body = put_empty(&alice.browser, &raw)
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(body["message"], "page introuvable");
    let read = alice.browser.get(&comments_uri(&page)).await.json();
    assert_eq!(
        read[0]["comments"][0]["reactions"].as_array().map(Vec::len),
        Some(4)
    );
}

/// Un corps qui ne dit pas exactement ce qu'il faut : `422 invalid_json`, champ manquant ou en
/// trop, sur chaque route qui en lit un ; rien n'est écrit.
#[tokio::test]
async fn comment_bodies_must_be_exact() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Page" })).await.id;
    let opened = alice
        .browser
        .post(&comments_uri(&page), json!({ "body": "Un" }))
        .await
        .json();
    let (thread, message) = (uuid(&opened["id"]), uuid(&opened["comments"][0]["id"]));
    let replies = format!("{}/replies", thread_uri(&page, thread));
    let cases = [
        (Method::POST, comments_uri(&page), json!({}), "manquant"),
        (
            Method::POST,
            comments_uri(&page),
            json!({ "body": "x", "resolved": true }),
            "inconnu",
        ),
        (Method::POST, replies.clone(), json!({}), "manquant"),
        (
            Method::POST,
            replies,
            json!({ "body": "x", "blockId": null }),
            "inconnu",
        ),
        (
            Method::PATCH,
            thread_uri(&page, thread),
            json!({}),
            "manquant",
        ),
        (
            Method::PATCH,
            thread_uri(&page, thread),
            json!({ "resolved": "oui" }),
            "type invalide",
        ),
        (
            Method::PATCH,
            thread_uri(&page, thread),
            json!({ "resolved": true, "body": "x" }),
            "inconnu",
        ),
        (
            Method::PATCH,
            message_uri(&page, thread, message),
            json!({}),
            "manquant",
        ),
        (
            Method::PATCH,
            message_uri(&page, thread, message),
            json!({ "body": "x", "author": "moi" }),
            "inconnu",
        ),
    ];
    for (method, uri, body, why) in cases {
        let reply = alice
            .browser
            .send(json_request(method.clone(), &uri, &body))
            .await;
        let error = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
        assert_french(&error, &[why]);
    }
    // Un corps vide, sans type de contenu.
    let empty = alice
        .browser
        .send(
            Request::patch(thread_uri(&page, thread))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(
        empty.status,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "{}",
        empty.text()
    );
    let empty = alice
        .browser
        .send(
            Request::patch(message_uri(&page, thread, message))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    empty.assert_error(StatusCode::BAD_REQUEST, "invalid_json");
    let read = alice.browser.get(&comments_uri(&page)).await.json();
    assert_eq!(read, json!([opened]));
}

/// Un fil ou un message qui n'est pas de cette page, ou qui n'existe pas, ou mal nommé : le
/// même `404 not_found`, sur chaque route qui le nomme. Qui voit une page n'apprend rien des
/// fils d'une autre.
#[tokio::test]
async fn thread_of_another_page_is_not_found() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let page = team.page(None, "Ici", &[members(Level::Full)]).await;
    // Une page que Mona ne voit pas, et une qu'elle voit.
    let hidden = team
        .page(None, "Cachée", &[user(&team.tom, Level::Full)])
        .await;
    let seen = team.page(None, "Ailleurs", &[members(Level::Full)]).await;
    let store = &team.store;
    let here = store
        .create_thread(page, None, mona.id, "Ici")
        .await
        .unwrap();
    let mut foreign = Vec::new();
    for node in [hidden, seen] {
        let thread = store
            .create_thread(node, None, team.tom.id, "Là")
            .await
            .unwrap();
        foreign.push((thread.id, thread.comments[0].id));
    }
    let here_msg = here.comments[0].id;
    // Les routes qui nomment un fil, sans message.
    let thread_routes = |thread: &str| {
        let thread_at = format!("{}/{thread}", comments_uri(page));
        vec![
            (
                "POST réponse",
                json_request(
                    Method::POST,
                    &format!("{thread_at}/replies"),
                    &json!({ "body": "x" }),
                ),
            ),
            (
                "PATCH fil",
                json_request(Method::PATCH, &thread_at, &json!({ "resolved": true })),
            ),
        ]
    };
    // Celles qui nomment un fil et un de ses messages.
    let message_routes = |thread: &str, message: &str| {
        let message_at = message_uri(page, thread, message);
        let reaction = format!("{message_at}/reactions/{}", encoded("👍"));
        vec![
            (
                "PATCH message",
                json_request(Method::PATCH, &message_at, &json!({ "body": "x" })),
            ),
            (
                "DELETE message",
                Request::delete(&message_at).body(Body::empty()).unwrap(),
            ),
            (
                "PUT réaction",
                Request::put(&reaction).body(Body::empty()).unwrap(),
            ),
            (
                "DELETE réaction",
                Request::delete(&reaction).body(Body::empty()).unwrap(),
            ),
        ]
    };
    // Les réponses de référence : un fil inconnu, un message inconnu d'un fil de la page.
    let unknown = Uuid::now_v7().to_string();
    let reference = async |routes: Vec<(&'static str, Request<Body>)>| {
        let mut replies = Vec::new();
        for (_, request) in routes {
            let reply = mona.browser.send(request).await;
            reply.assert_error(StatusCode::NOT_FOUND, "not_found");
            replies.push(outcome(&reply));
        }
        replies
    };
    let no_thread = reference(thread_routes(&unknown)).await;
    let no_thread_for_message = reference(message_routes(&unknown, &here_msg.to_string())).await;
    let no_message = reference(message_routes(&here.id.to_string(), &unknown)).await;
    assert_ne!(no_thread_for_message, no_message, "un fil, puis un message");

    let compare = async |routes: Vec<(&'static str, Request<Body>)>,
                         missing: &[(StatusCode, Option<String>, String)],
                         case: &str| {
        for ((name, request), missing) in routes.into_iter().zip(missing) {
            let reply = mona.browser.send(request).await;
            assert_eq!(&outcome(&reply), missing, "{name}, {case}");
        }
    };
    let mut threads = vec!["pas-un-uuid".to_string()];
    threads.extend(foreign.iter().map(|(thread, _)| thread.to_string()));
    for thread in &threads {
        let case = format!("fil {thread}");
        compare(thread_routes(thread), &no_thread, &case).await;
        compare(
            message_routes(thread, &here_msg.to_string()),
            &no_thread_for_message,
            &case,
        )
        .await;
    }
    for (thread, message) in &foreign {
        let case = format!("fil {thread}, message {message}");
        let routes = message_routes(&thread.to_string(), &message.to_string());
        compare(routes, &no_thread_for_message, &case).await;
    }
    let mut messages = vec!["pas-un-uuid".to_string()];
    messages.extend(foreign.iter().map(|(_, message)| message.to_string()));
    for message in &messages {
        let case = format!("fil de la page, message {message}");
        let routes = message_routes(&here.id.to_string(), message);
        compare(routes, &no_message, &case).await;
    }
    // Rien n'a été écrit, ni ici ni ailleurs.
    for (node, count) in [(page, 1), (hidden, 1), (seen, 1)] {
        let threads = store.comment_threads(node).await.unwrap();
        assert_eq!(threads.len(), count);
        assert_eq!(threads[0].comments.len(), 1);
        assert!(threads[0].comments[0].reactions.is_empty());
        assert_eq!(threads[0].resolved_at, None);
    }
}

// ---------------------------------------------------------------------------------------------
// Ce que reçoivent les connexions de synchro.

/// Chaque écriture de commentaire validée envoie la trame `{"type":"comments"}` à toutes les
/// connexions de synchro de la page, lecteurs compris, et à celles d'aucune autre. Une
/// écriture qui ne change rien (une réaction déjà posée) ou refusée n'envoie rien.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn comment_write_tells_the_page() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let (mona, gina) = (&team.mona, &team.gina);
    let page = team
        .page(
            None,
            "Commentée",
            &[members(Level::Full), user(gina, Level::Read)],
        )
        .await;
    let elsewhere = team.page(None, "Ailleurs", &[members(Level::Full)]).await;
    let on_page = [
        synced_as(&mona.browser, &sync_url(addr, page)).await,
        synced_as(&gina.browser, &sync_url(addr, page)).await,
    ];
    let other = synced_as(&mona.browser, &sync_url(addr, elsewhere)).await;
    let comments = json!({ "type": "comments" });
    // Après une écriture : chaque connexion de la page a reçu `count` trames, toutes `comments`,
    // et pas une de plus (la resynchronisation passe derrière tout ce que l'acteur a diffusé
    // avant elle).
    let heard = async |count: usize| {
        for client in &on_page {
            assert!(still_served(client).await, "connexion fermée");
            let frames = events(client);
            assert_eq!(frames.len(), count, "{frames:?}");
            assert!(frames.iter().all(|frame| frame == &comments), "{frames:?}");
        }
    };

    let opened = mona
        .browser
        .post(&comments_uri(page), json!({ "body": "Un" }))
        .await;
    opened.assert_status(StatusCode::CREATED);
    let opened = opened.json();
    let (thread, first) = (uuid(&opened["id"]), uuid(&opened["comments"][0]["id"]));
    heard(1).await;
    let replied = mona
        .browser
        .post(
            &format!("{}/replies", thread_uri(page, thread)),
            json!({ "body": "Deux" }),
        )
        .await;
    replied.assert_status(StatusCode::CREATED);
    let second = uuid(&replied.json()["id"]);
    heard(2).await;
    for resolved in [true, false] {
        mona.browser
            .patch(&thread_uri(page, thread), json!({ "resolved": resolved }))
            .await
            .assert_status(StatusCode::OK);
    }
    heard(4).await;
    mona.browser
        .patch(
            &message_uri(page, thread, first),
            json!({ "body": "Un, corrigé" }),
        )
        .await
        .assert_status(StatusCode::OK);
    heard(5).await;
    let reaction = reaction_uri(page, thread, first, "👍");
    put_empty(&mona.browser, &reaction)
        .await
        .assert_status(StatusCode::OK);
    heard(6).await;
    // Déjà posée, rien ne change : rien n'est envoyé. Refusée non plus.
    put_empty(&mona.browser, &reaction)
        .await
        .assert_status(StatusCode::OK);
    mona.browser
        .post(&comments_uri(page), json!({ "body": "" }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_body");
    gina.browser
        .post(&comments_uri(page), json!({ "body": "Lectrice" }))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    heard(6).await;
    mona.browser
        .delete(&reaction)
        .await
        .assert_status(StatusCode::OK);
    heard(7).await;
    for message in [second, first] {
        mona.browser
            .delete(&message_uri(page, thread, message))
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
    heard(9).await;

    // L'autre page n'a rien reçu.
    assert!(still_served(&other).await);
    assert_eq!(events(&other), [] as [Value; 0]);
}
