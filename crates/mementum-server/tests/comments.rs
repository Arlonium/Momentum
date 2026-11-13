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
use common::sync::{Witness, members, open_app, user};
use common::{
    Browser, Reply, Team, TestApp, WAIT, assert_french, json_request, outcome, sync_url, synced_as,
    uuid,
};
use mementum_acl::Level;
use mementum_store::{
    Authorship, MAX_MESSAGES_PER_PAGE, MAX_REACTIONS_PER_PAGE, MAX_TEXT_BYTES_PER_ACCOUNT,
    MAX_THREADS_PER_ACCOUNT,
};
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
    /// Supprimer tout le fil (Ruling R352).
    DeleteThread,
}

const ACTS: [Act; 10] = [
    Act::List,
    Act::Open,
    Act::Reply,
    Act::Resolve,
    Act::Reopen,
    Act::Edit,
    Act::Delete,
    Act::React,
    Act::Unreact,
    Act::DeleteThread,
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
        // Supprimer tout le fil, avec les messages de tous : `full`, qu'on l'ait ouvert ou non.
        Act::DeleteThread => (StatusCode::NO_CONTENT, level >= Lvl::Full),
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
        Act::DeleteThread => by.delete(&thread_uri(page, thread)).await,
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
                                ("forbidden", _) if act == Act::DeleteThread => {
                                    "il faut « Accès complet »"
                                }
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
    assert_eq!(cells, 4 * 6 * 2 * 10, "toutes les cases ont été jouées");
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
            (
                "DELETE fil",
                Request::delete(&thread_at).body(Body::empty()).unwrap(),
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

/// Sur une page à la corbeille, un fil qui n'est pas de la page, ou un message qui n'est pas du
/// fil, répond `404` avant `409 trashed` (Ruling R168) : la même réponse que sur une page
/// vivante. Le fil et le message de la page, eux, reçoivent `409 trashed`.
#[tokio::test]
async fn missing_thread_comes_before_trashed() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let page = team.page(None, "Jetée", &[members(Level::Full)]).await;
    let elsewhere = team.page(None, "Ailleurs", &[members(Level::Full)]).await;
    let store = &team.store;
    let here = store
        .create_thread(page, None, mona.id, "Ici")
        .await
        .unwrap();
    let there = store
        .create_thread(elsewhere, None, mona.id, "Là")
        .await
        .unwrap();
    team.trash(page).await;
    let (thread, message) = (here.id.to_string(), here.comments[0].id.to_string());
    let (foreign, foreign_msg) = (there.id.to_string(), there.comments[0].id.to_string());
    let unknown = Uuid::now_v7().to_string();
    // Les routes qui nomment un fil ; les quatre dernières nomment aussi un de ses messages.
    let requests = |thread: &str, message: &str| {
        let message_at = message_uri(page, thread, message);
        let reaction = format!("{message_at}/reactions/{}", encoded("👍"));
        vec![
            (
                "POST réponse",
                json_request(
                    Method::POST,
                    &format!("{}/replies", thread_uri(page, thread)),
                    &json!({ "body": "x" }),
                ),
            ),
            (
                "PATCH fil",
                json_request(
                    Method::PATCH,
                    &thread_uri(page, thread),
                    &json!({ "resolved": true }),
                ),
            ),
            (
                "DELETE fil",
                Request::delete(thread_uri(page, thread))
                    .body(Body::empty())
                    .unwrap(),
            ),
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
        ]
    };
    let cases = [
        (
            unknown.as_str(),
            message.as_str(),
            "fil de commentaires introuvable sur cette page",
        ),
        (
            foreign.as_str(),
            foreign_msg.as_str(),
            "fil de commentaires introuvable sur cette page",
        ),
    ];
    for (thread_id, message_id, why) in cases {
        for (name, request) in requests(thread_id, message_id) {
            let body = mona
                .browser
                .send(request)
                .await
                .assert_error(StatusCode::NOT_FOUND, "not_found");
            assert_eq!(body["message"], why, "{name}, fil {thread_id}");
        }
    }
    for message_id in [unknown.as_str(), foreign_msg.as_str()] {
        for (name, request) in requests(&thread, message_id).into_iter().skip(3) {
            let body = mona
                .browser
                .send(request)
                .await
                .assert_error(StatusCode::NOT_FOUND, "not_found");
            assert_eq!(
                body["message"], "message introuvable dans ce fil",
                "{name}, message {message_id}"
            );
        }
    }
    for (name, request) in requests(&thread, &message) {
        let reply = mona.browser.send(request).await;
        assert_eq!(reply.status, StatusCode::CONFLICT, "{name}");
        reply.assert_error(StatusCode::CONFLICT, "trashed");
    }
    assert_eq!(store.comment_threads(page).await.unwrap(), [here]);
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
    // Un témoin par page (Tom y écrit) : sa marque, posée après la réponse d'une route, passe
    // derrière la trame que cette route a demandée.
    let witness = Witness::join(&team.tom.browser, &sync_url(addr, page)).await;
    let witness_elsewhere = Witness::join(&team.tom.browser, &sync_url(addr, elsewhere)).await;
    let comments = json!({ "type": "comments" });
    // Après une écriture : chaque connexion de la page a reçu `count` trames, toutes `comments`,
    // et pas une de plus. La route demande la trame à l'acteur sans l'attendre : c'est la
    // marque du témoin qui dit que la trame est arrivée, ou qu'il n'y en a pas eu. Une
    // resynchronisation ne le dit pas : sa réponse passe devant une trame diffusée pendant
    // que la connexion l'attend (Ruling R342).
    let heard = async |count: usize| {
        witness.seen_by(&[&on_page[0], &on_page[1]]).await;
        for client in &on_page {
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
    // Résolu, puis résolu encore (rien ne change : rien n'est envoyé) ; rouvert de même.
    for resolved in [true, true] {
        mona.browser
            .patch(&thread_uri(page, thread), json!({ "resolved": resolved }))
            .await
            .assert_status(StatusCode::OK);
    }
    heard(3).await;
    for resolved in [false, false] {
        mona.browser
            .patch(&thread_uri(page, thread), json!({ "resolved": resolved }))
            .await
            .assert_status(StatusCode::OK);
    }
    heard(4).await;
    let edit = message_uri(page, thread, first);
    let edited = mona
        .browser
        .patch(&edit, json!({ "body": "Un, corrigé" }))
        .await;
    edited.assert_status(StatusCode::OK);
    let edited = edited.json();
    assert!(edited["editedAt"].is_number(), "{edited}");
    heard(5).await;
    // Le même texte (aux espaces de bord près) : rien ne change, ni la date de modification,
    // et rien n'est envoyé.
    let same = mona
        .browser
        .patch(&edit, json!({ "body": "  Un, corrigé\n" }))
        .await;
    same.assert_status(StatusCode::OK);
    assert_eq!(same.json(), edited);
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
    // Un fil supprimé tout entier, avec sa réponse : une trame. Supprimé de nouveau, il n'est
    // plus là : `404`, et rien n'est envoyé (Ruling R352).
    let opened = mona
        .browser
        .post(&comments_uri(page), json!({ "body": "Trois" }))
        .await;
    opened.assert_status(StatusCode::CREATED);
    let thread = uuid(&opened.json()["id"]);
    mona.browser
        .post(
            &format!("{}/replies", thread_uri(page, thread)),
            json!({ "body": "Quatre" }),
        )
        .await
        .assert_status(StatusCode::CREATED);
    heard(11).await;
    mona.browser
        .delete(&thread_uri(page, thread))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    heard(12).await;
    mona.browser
        .delete(&thread_uri(page, thread))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    heard(12).await;

    // L'autre page n'a rien reçu.
    witness_elsewhere.seen_by(&[&other]).await;
    assert_eq!(events(&other), [] as [Value; 0]);
}

/// Les identifiants des blocs de la page, lus par une connexion de synchro neuve : `(texte,
/// identifiant)`, dans l'ordre.
async fn blocks_of(
    by: &Browser<'_>,
    addr: std::net::SocketAddr,
    page: &str,
) -> Vec<(String, String)> {
    let client = synced_as(by, &sync_url(addr, page)).await;
    client
        .blocks()
        .iter()
        .map(|b| (mementum_md::block_text(b), b.id.clone()))
        .collect()
}

/// Ouvre un fil sur le bloc `block` : `201`, rend le fil.
async fn thread_on(by: &Browser<'_>, page: &str, block: &str, body: &str) -> Value {
    let opened = by
        .post(
            &comments_uri(page),
            json!({ "blockId": block, "body": body }),
        )
        .await;
    opened.assert_status(StatusCode::CREATED);
    opened.json()
}

/// La sonde de la relecture finale (S3), avec l'attente inverse. Alice ouvre un fil sur le
/// bloc « Second » ; un agent relit la page (`GET /mmd`, sans ancres) et la récrit telle quelle.
/// Chaque bloc recevait un identifiant neuf, et le fil passait sous « Bloc supprimé ». Les
/// blocs gardent leur identifiant (Ruling R201), et un fil neuf s'attache toujours au bloc.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_mmd_rewrite_keeps_threads_on_their_blocks() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({ "title": "Page" })).await.id;
    alice
        .put_mmd(&page, "Premier\n\nSecond\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let before = blocks_of(&alice.browser, addr, &page).await;
    let second = before[1].1.clone();
    thread_on(&alice.browser, &page, &second, "Sur le second").await;

    let read = alice.get_mmd(&page).await;
    read.assert_status(StatusCode::OK);
    let mmd = read.text();
    alice
        .put_mmd(&page, mmd.clone())
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let after = blocks_of(&alice.browser, addr, &page).await;
    let identical = alice.get_mmd(&page).await.text() == mmd;
    assert_eq!(
        after, before,
        "MMD lu : {mmd:?} ; bloc commenté {second} ; blocs après la récriture : {after:?} ; MMD \
         identique : {identical}"
    );
    thread_on(&alice.browser, &page, &second, "Encore sur le second").await;
}

/// Une récriture qui change la page : un bloc modifié, un inséré, un retiré, deux échangés.
/// Les blocs inchangés gardent leur identifiant, échangés compris, et leurs fils restent
/// attachés ; le bloc modifié garde le sien (il est à la même place, entre les mêmes blocs
/// inchangés, du même type) ; le bloc retiré n'est plus là, son fil se lit sous « Bloc
/// supprimé » ; le bloc inséré reçoit un identifiant neuf.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edited_mmd_keeps_threads_on_unchanged_blocks() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({ "title": "Page" })).await.id;
    alice
        .put_mmd(&page, "Un\n\nDeux\n\nTrois\n\nQuatre\n\nCinq\n\nSix\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let before = blocks_of(&alice.browser, addr, &page).await;
    let id = |text: &str| {
        before
            .iter()
            .find(|(t, _)| t == text)
            .map(|(_, id)| id.clone())
            .unwrap()
    };
    for text in ["Trois", "Quatre", "Cinq", "Six"] {
        thread_on(&alice.browser, &page, &id(text), text).await;
    }

    alice
        .put_mmd(
            &page,
            "Nouveau\n\nUn\n\nDeux bis\n\nCinq\n\nQuatre\n\nSix\n",
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let after = blocks_of(&alice.browser, addr, &page).await;
    let expected: Vec<(String, Option<String>)> = vec![
        ("Nouveau".into(), None),
        ("Un".into(), Some(id("Un"))),
        ("Deux bis".into(), Some(id("Deux"))),
        ("Cinq".into(), Some(id("Cinq"))),
        ("Quatre".into(), Some(id("Quatre"))),
        ("Six".into(), Some(id("Six"))),
    ];
    let got: Vec<(String, Option<String>)> = after
        .iter()
        .map(|(text, block)| {
            let kept = before.iter().any(|(_, old)| old == block);
            (text.clone(), kept.then(|| block.clone()))
        })
        .collect();
    assert_eq!(got, expected, "avant : {before:?} ; après : {after:?}");
    let ids: Vec<&String> = after.iter().map(|(_, id)| id).collect();
    let present = |block: &str| ids.iter().any(|id| *id == block);
    for text in ["Quatre", "Cinq", "Six"] {
        assert!(present(&id(text)), "le fil de « {text} » a perdu son bloc");
    }
    assert!(!present(&id("Trois")), "le bloc retiré est encore là");
}

/// `count` comptes de remplissage, écrits par du SQL direct (« Remplissage i »), aux
/// identifiants `01890000-0000-7000-a003-…` à partir de `base` : ils écrivent les fils et les
/// réactions de départ, sans atteindre chacun sa part (Ruling R351). Rend leurs identifiants.
async fn filler_accounts(db: &std::path::Path, base: u32, count: u32) -> Vec<String> {
    common::exec_sql(
        db,
        &format!(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < {count} - 1) \
             INSERT INTO users (id, email, name, password_hash, created_at) \
             SELECT printf('01890000-0000-7000-a003-%012d', {base} + i), \
                    printf('remplissage-%d@exemple.test', {base} + i), \
                    printf('Remplissage %d', {base} + i), 'pas-un-hachage', 1 FROM n"
        ),
    )
    .await;
    (base..base + count)
        .map(|i| format!("01890000-0000-7000-a003-{i:012}"))
        .collect()
}

/// Écrit `count` fils sur la page, résolus, par du SQL direct, le fil `i` ouvert par
/// `authors[i % authors.len()]`, chacun avec `per_thread` messages de son auteur, de `bytes` « x »
/// chacun ; identifiants `01890000-0000-7000-a001-…` (fils, à partir de `base`) et `…-a002-…`
/// (messages), valides pour la lecture des fils. Deux appels sur une même base prennent des
/// `base` écartés d'au moins `count`.
async fn fill_threads(
    db: &std::path::Path,
    page: &str,
    authors: &[String],
    (base, count): (u32, u32),
    (per_thread, bytes): (u32, u32),
) {
    let names: Vec<String> = authors.iter().map(|a| format!("\"{a}\"")).collect();
    let (names, n) = (names.join(","), authors.len());
    common::exec_sql(
        db,
        &format!(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < {count} - 1) \
             INSERT INTO comment_threads (id, node_id, created_by, created_at, resolved_at, \
               resolved_by) \
             SELECT printf('01890000-0000-7000-a001-%012d', {base} + i), '{page}', a, i, i, a \
             FROM (SELECT i, json_extract('[{names}]', printf('$[%d]', i % {n})) AS a FROM n)"
        ),
    )
    .await;
    if per_thread > 0 {
        common::exec_sql(
            db,
            &format!(
                "WITH RECURSIVE m(j) AS \
                   (SELECT 0 UNION ALL SELECT j + 1 FROM m WHERE j < {per_thread} - 1) \
                 INSERT INTO comments (id, thread_id, author_id, body, created_at) \
                 SELECT printf('01890000-0000-7000-a002-%012d', \
                          CAST(substr(t.id, 25) AS INTEGER) * 1000 + j), \
                        t.id, t.created_by, replace(hex(zeroblob({bytes})), '00', 'x'), \
                        t.created_at * 1000 + j \
                 FROM comment_threads t, m WHERE t.node_id = '{page}' \
                   AND t.id LIKE '01890000-0000-7000-a001-%'"
            ),
        )
        .await;
    }
}

/// Ruling R204 : les plafonds des commentaires, par les routes. Au-delà de 2 000 fils sur la
/// page (résolus compris), de 500 messages dans un fil ou de 32 emojis différents sur un
/// message, l'écriture est refusée par un `409` au code propre (`thread_limit`,
/// `message_limit`, `reaction_limit`), avec un message qui tutoie et dit quoi faire ; rien
/// n'est écrit, et aucune trame ne part. Les fils et les emojis de départ sont ceux de comptes
/// de remplissage, dont aucun n'atteint sa part (Ruling R351) : ce sont bien les plafonds
/// communs qui refusent.
#[tokio::test]
async fn comment_limits_are_refused_by_the_routes() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Page" })).await.id;
    let opened = alice
        .browser
        .post(&comments_uri(&page), json!({ "body": "Premier" }))
        .await;
    opened.assert_status(StatusCode::CREATED);
    let opened = opened.json();
    let (thread, message) = (
        opened["id"].as_str().unwrap().to_string(),
        opened["comments"][0]["id"].as_str().unwrap().to_string(),
    );
    let db = t.db_path();
    let fillers = filler_accounts(&db, 0, 10).await;
    // 1 999 fils de plus, résolus ; 499 messages de plus dans le fil ; 32 emojis sur le message,
    // huit par compte.
    fill_threads(&db, &page, &fillers, (0, 1999), (0, 0)).await;
    common::exec_sql(
        &db,
        &format!(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 31) \
             INSERT INTO comment_reactions (comment_id, user_id, emoji, created_at) \
             SELECT '{message}', json_extract('[\"{}\",\"{}\",\"{}\",\"{}\"]', \
                      printf('$[%d]', i / 8)), char(128512 + i), i FROM n",
            fillers[0], fillers[1], fillers[2], fillers[3]
        ),
    )
    .await;
    common::exec_sql(
        &db,
        &format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 499) \
             INSERT INTO comments (id, thread_id, author_id, body, created_at) \
             SELECT printf('01890000-0000-7000-b002-%012d', i), '{thread}', '{}', 'Réponse', i \
             FROM n",
            alice.id
        ),
    )
    .await;

    let refusals = [
        (
            alice
                .browser
                .post(&comments_uri(&page), json!({ "body": "Un fil de trop" }))
                .await,
            "thread_limit",
            "2 000",
        ),
        (
            alice
                .browser
                .post(
                    &format!("{}/replies", thread_uri(&page, &thread)),
                    json!({ "body": "Une réponse de trop" }),
                )
                .await,
            "message_limit",
            "500",
        ),
        (
            put_empty(
                &alice.browser,
                &reaction_uri(&page, &thread, &message, "🦊"),
            )
            .await,
            "reaction_limit",
            "32",
        ),
    ];
    for (reply, code, bound) in refusals {
        let body = reply.assert_error(StatusCode::CONFLICT, code);
        assert_french(&body, &[bound]);
    }
    // Un emoji déjà là se pose encore.
    put_empty(
        &alice.browser,
        &reaction_uri(&page, &thread, &message, "😀"),
    )
    .await
    .assert_status(StatusCode::OK);
}

/// Rulings R350 et R351 : les plafonds de la page (10 000 messages, 4 Mio de texte : `409
/// page_comment_limit`) et la part d'un compte (200 fils ouverts sur la page : `409
/// thread_limit` ; 8 emojis posés sur un message : `409 reaction_limit`), par les routes, avec un
/// message qui tutoie, dit la borne, dit quand c'est sa part, et ce qu'il faut faire. Ils viennent
/// après tous les refus de droit et le corps, dans l'ordre de R168 : une lectrice reçoit `403`,
/// un corps vide `422`, un fil inconnu `404`, une page à la corbeille `409 trashed`, même sur une
/// page pleine. Rien n'est écrit.
#[tokio::test]
async fn page_and_account_limits_are_refused_by_the_routes() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, tom, gina) = (&team.mona, &team.tom, &team.gina);
    let db = t.db_path();
    let filler = filler_accounts(&db, 0, 1).await;
    let grants = [members(Level::Full), user(gina, Level::Read)];
    let messages_of = async |page: Uuid| {
        common::query_i64(
            &db,
            &format!(
                "SELECT COUNT(*) FROM comments c JOIN comment_threads t ON t.id = c.thread_id \
                 WHERE t.node_id = '{page}'"
            ),
        )
        .await
    };

    // 10 000 messages : 25 fils de 400.
    let crowded = team.page(None, "Pleine", &grants).await;
    fill_threads(&db, &crowded.to_string(), &filler, (0, 25), (400, 7)).await;
    let filled = "01890000-0000-7000-a001-000000000000";
    let reply_uri = format!("{}/replies", thread_uri(crowded, filled));
    let refused = mona
        .browser
        .post(&comments_uri(crowded), json!({ "body": "Un fil de trop" }))
        .await;
    let page_full = refused.assert_error(StatusCode::CONFLICT, "page_comment_limit");
    assert_french(
        &page_full,
        &["10 000", "accès complet", "rien n'a été écrit"],
    );
    let refused = mona
        .browser
        .post(&reply_uri, json!({ "body": "Une réponse de trop" }))
        .await;
    assert_eq!(
        refused.assert_error(StatusCode::CONFLICT, "page_comment_limit"),
        page_full
    );
    // Avant le plafond : le niveau, le fil, le corps.
    gina.browser
        .post(&comments_uri(crowded), json!({ "body": "Lectrice" }))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    let unknown = Uuid::now_v7();
    mona.browser
        .post(
            &format!("{}/replies", thread_uri(crowded, unknown)),
            json!({ "body": "Ailleurs" }),
        )
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    mona.browser
        .post(&reply_uri, json!({ "body": "  " }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_body");
    assert_eq!(messages_of(crowded).await, 10_000);

    // 4 190 000 octets : 419 messages de 10 000 « x ». Il reste 4 304 octets.
    let heavy = team.page(None, "Lourde", &grants).await;
    fill_threads(&db, &heavy.to_string(), &filler, (100, 1), (419, 10_000)).await;
    let refused = mona
        .browser
        .post(&comments_uri(heavy), json!({ "body": "é".repeat(2_153) }))
        .await;
    let text_full = refused.assert_error(StatusCode::CONFLICT, "page_comment_limit");
    assert_french(&text_full, &["4 Mio", "raccourcis", "rien n'a été écrit"]);
    assert_ne!(text_full["message"], page_full["message"]);
    let opened = mona
        .browser
        .post(&comments_uri(heavy), json!({ "body": "é".repeat(2_150) }))
        .await;
    opened.assert_status(StatusCode::CREATED);
    let opened = opened.json();
    let (thread, message) = (uuid(&opened["id"]), uuid(&opened["comments"][0]["id"]));
    let refused = mona
        .browser
        .patch(
            &message_uri(heavy, thread, message),
            json!({ "body": "é".repeat(2_153) }),
        )
        .await;
    assert_eq!(
        refused.assert_error(StatusCode::CONFLICT, "page_comment_limit"),
        text_full
    );
    mona.browser
        .patch(
            &message_uri(heavy, thread, message),
            json!({ "body": "Court" }),
        )
        .await
        .assert_status(StatusCode::OK);

    // Mona a ouvert 200 fils sur une page : sa part.
    let own = team.page(None, "Partagée", &grants).await;
    fill_threads(
        &db,
        &own.to_string(),
        &[mona.id.to_string()],
        (200, 200),
        (1, 3),
    )
    .await;
    let refused = mona
        .browser
        .post(&comments_uri(own), json!({ "body": "Un de trop" }))
        .await;
    let her_part = refused.assert_error(StatusCode::CONFLICT, "thread_limit");
    assert_french(
        &her_part,
        &[
            "tu as déjà ouvert 200 fils",
            "ta part",
            "rien n'a été écrit",
        ],
    );
    let opened = tom
        .browser
        .post(&comments_uri(own), json!({ "body": "La part de Tom" }))
        .await;
    opened.assert_status(StatusCode::CREATED);
    let opened = opened.json();
    let (thread, message) = (uuid(&opened["id"]), uuid(&opened["comments"][0]["id"]));
    mona.browser
        .post(
            &format!("{}/replies", thread_uri(own, thread)),
            json!({ "body": "Répondre n'ouvre pas de fil" }),
        )
        .await
        .assert_status(StatusCode::CREATED);

    // Mona pose 8 emojis sur le message de Tom : sa part.
    for i in 0..8 {
        let emoji = char::from_u32(0x1F600 + i).unwrap().to_string();
        put_empty(&mona.browser, &reaction_uri(own, thread, message, &emoji))
            .await
            .assert_status(StatusCode::OK);
    }
    let refused = put_empty(&mona.browser, &reaction_uri(own, thread, message, "🦊")).await;
    let her_emojis = refused.assert_error(StatusCode::CONFLICT, "reaction_limit");
    assert_french(
        &her_emojis,
        &["tu as déjà posé 8 emojis", "ta part", "rien n'a été écrit"],
    );
    put_empty(&tom.browser, &reaction_uri(own, thread, message, "🦊"))
        .await
        .assert_status(StatusCode::OK);

    // Le message d'un plafond ne porte jamais deux espaces de suite.
    for body in [&page_full, &text_full, &her_part, &her_emojis] {
        let message = body["message"].as_str().unwrap();
        assert!(!message.contains("  "), "« {message} »");
    }

    // À la corbeille, c'est la corbeille qui refuse, avant le plafond.
    team.trash(crowded).await;
    mona.browser
        .post(&comments_uri(crowded), json!({ "body": "Jetée" }))
        .await
        .assert_error(StatusCode::CONFLICT, "trashed");
    assert_eq!(messages_of(crowded).await, 10_000);
}

/// La sonde (b) de la relecture de S4, inversée (Ruling R351) : une invitée qui peut commenter
/// ouvre des fils tant qu'elle peut ; elle s'arrête à sa part, 200, avec un message qui le dit,
/// et la page garde ses fils pour les autres : Olga en ouvre encore.
#[tokio::test]
async fn one_account_does_not_fill_the_threads_of_a_page() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (olga, gina) = (&team.olga, &team.gina);
    let page = team
        .page(
            None,
            "Commentée",
            &[members(Level::Full), user(gina, Level::Comment)],
        )
        .await;
    for i in 0..MAX_THREADS_PER_ACCOUNT {
        gina.browser
            .post(&comments_uri(page), json!({ "body": format!("Fil {i}") }))
            .await
            .assert_status(StatusCode::CREATED);
    }
    let refused = gina
        .browser
        .post(&comments_uri(page), json!({ "body": "Un de plus" }))
        .await;
    let body = refused.assert_error(StatusCode::CONFLICT, "thread_limit");
    assert_french(&body, &["200", "ta part"]);
    olga.browser
        .post(&comments_uri(page), json!({ "body": "Olga ouvre encore" }))
        .await
        .assert_status(StatusCode::CREATED);
    let threads = gina.browser.get(&comments_uri(page)).await.json();
    assert_eq!(threads.as_array().map(Vec::len), Some(201));

    // Olga supprime un fil de Gina, réponses comprises, en une requête (Ruling R352) ; avant,
    // elle supprimait message par message.
    let hers = uuid(&threads[0]["id"]);
    for i in 0..3 {
        gina.browser
            .post(
                &format!("{}/replies", thread_uri(page, hers)),
                json!({ "body": format!("Réponse {i}") }),
            )
            .await
            .assert_status(StatusCode::CREATED);
    }
    olga.browser
        .delete(&thread_uri(page, hers))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let threads = gina.browser.get(&comments_uri(page)).await.json();
    assert_eq!(threads.as_array().map(Vec::len), Some(200));
    assert!(
        threads
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["id"] != hers.to_string())
    );
}

/// Ruling R352 : `DELETE …/comments/{threadId}`, pour qui a l'accès complet, supprime tout le fil,
/// avec les messages de tous et leurs réactions, en une requête ; le reste de la page n'est pas
/// touché. Qui a moins, « Peut modifier » ou l'auteur du fil avec « Peut commenter », reçoit
/// `403`, qui dit le niveau qu'il faut, avant que le fil soit cherché (Ruling R168) : un fil
/// inconnu lui répond le même `403`.
#[tokio::test]
async fn deleting_a_thread_takes_everyone_s_messages() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (olga, mona, tom) = (&team.olga, &team.mona, &team.tom);
    let page = team
        .page(
            None,
            "Commentée",
            &[
                user(olga, Level::Full),
                user(mona, Level::Edit),
                user(tom, Level::Comment),
            ],
        )
        .await;
    let opened = tom
        .browser
        .post(&comments_uri(page), json!({ "body": "Le fil de Tom" }))
        .await;
    opened.assert_status(StatusCode::CREATED);
    let thread = uuid(&opened.json()["id"]);
    let replied = mona
        .browser
        .post(
            &format!("{}/replies", thread_uri(page, thread)),
            json!({ "body": "La réponse de Mona" }),
        )
        .await;
    replied.assert_status(StatusCode::CREATED);
    put_empty(
        &tom.browser,
        &reaction_uri(page, thread, uuid(&replied.json()["id"]), "👍"),
    )
    .await
    .assert_status(StatusCode::OK);
    let kept = mona
        .browser
        .post(&comments_uri(page), json!({ "body": "Le fil de Mona" }))
        .await;
    kept.assert_status(StatusCode::CREATED);
    let kept = kept.json();
    let before = olga.browser.get(&comments_uri(page)).await.json();

    // Tom, qui l'a ouvert, puis Mona.
    for (browser, held) in [
        (&tom.browser, "Peut commenter"),
        (&mona.browser, "Peut modifier"),
    ] {
        for target in [thread, Uuid::now_v7()] {
            let body = browser
                .delete(&thread_uri(page, target))
                .await
                .assert_error(StatusCode::FORBIDDEN, "forbidden");
            assert_french(&body, &["il faut « Accès complet »", held]);
        }
    }
    assert_eq!(olga.browser.get(&comments_uri(page)).await.json(), before);

    olga.browser
        .delete(&thread_uri(page, thread))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let after = olga.browser.get(&comments_uri(page)).await.json();
    assert_eq!(after, json!([kept]));
    let db = t.db_path();
    for (table, sql) in [
        (
            "comments",
            format!("SELECT COUNT(*) FROM comments WHERE thread_id = '{thread}'"),
        ),
        (
            "comment_reactions",
            "SELECT COUNT(*) FROM comment_reactions".to_string(),
        ),
    ] {
        assert_eq!(common::query_i64(&db, &sql).await, 0, "{table}");
    }
}

/// Pose, par du SQL direct, `per` réactions de `user` (toujours les mêmes emojis) sur chacun des
/// `messages` premiers messages de la page, rangés par identifiant.
async fn fill_reactions(db: &std::path::Path, page: &str, user: &str, messages: u32, per: u32) {
    common::exec_sql(
        db,
        &format!(
            "WITH RECURSIVE e(k) AS (SELECT 0 UNION ALL SELECT k + 1 FROM e WHERE k < {per} - 1) \
             INSERT INTO comment_reactions (comment_id, user_id, emoji, created_at) \
             SELECT m.id, '{user}', char(128512 + k), 1 FROM \
               (SELECT c.id FROM comments c JOIN comment_threads t ON t.id = c.thread_id \
                WHERE t.node_id = '{page}' ORDER BY c.id LIMIT {messages}) m, e"
        ),
    )
    .await;
}

/// La sonde (b) de la re-relecture du tour 1, inversée (Ruling R358) : une invitée qui peut
/// commenter prend toute sa part d'une page (le texte, par des messages de 10 000 caractères ;
/// les messages ; les réactions), et Olga y écrit encore à chaque fois. Avant, Gina fermait les
/// commentaires de la page à tous en 105 messages d'un fil.
#[tokio::test]
async fn one_account_does_not_close_the_comments_of_a_page() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (olga, gina) = (&team.olga, &team.gina);
    let db = t.db_path();
    let grants = [members(Level::Full), user(gina, Level::Comment)];
    let her_part = |body: &Value, what: &str| {
        assert_french(body, &[what, "c'est ta part", "rien n'a été écrit"]);
    };

    // Le texte : 104 messages de 10 000 « x », puis le refus de sa part.
    let heavy = team.page(None, "Lourde", &grants).await;
    let long = "x".repeat(10_000);
    let opened = gina
        .browser
        .post(&comments_uri(heavy), json!({ "body": long }))
        .await;
    opened.assert_status(StatusCode::CREATED);
    let replies = format!("{}/replies", thread_uri(heavy, uuid(&opened.json()["id"])));
    let mut written = 1;
    let refused = loop {
        let reply = gina.browser.post(&replies, json!({ "body": long })).await;
        if reply.status != StatusCode::CREATED {
            break reply;
        }
        written += 1;
        assert!(
            written <= 110,
            "la part de texte d'un compte n'est pas plafonnée"
        );
    };
    her_part(
        &refused.assert_error(StatusCode::CONFLICT, "page_comment_limit"),
        "1 Mio",
    );
    assert_eq!(written, MAX_TEXT_BYTES_PER_ACCOUNT as usize / 10_000);
    olga.browser
        .post(&comments_uri(heavy), json!({ "body": long }))
        .await
        .assert_status(StatusCode::CREATED);

    // Les messages : 2 000, en cinq fils de Gina.
    let crowded = team.page(None, "Pleine", &grants).await;
    let gina_id = gina.id.to_string();
    fill_threads(
        &db,
        &crowded.to_string(),
        std::slice::from_ref(&gina_id),
        (0, 5),
        (400, 1),
    )
    .await;
    let refused = gina
        .browser
        .post(&comments_uri(crowded), json!({ "body": "Un de trop" }))
        .await;
    her_part(
        &refused.assert_error(StatusCode::CONFLICT, "page_comment_limit"),
        "2 000 messages",
    );
    let opened = olga
        .browser
        .post(
            &comments_uri(crowded),
            json!({ "body": "Olga écrit encore" }),
        )
        .await;
    opened.assert_status(StatusCode::CREATED);
    let opened = opened.json();
    let (thread, message) = (uuid(&opened["id"]), uuid(&opened["comments"][0]["id"]));

    // Les réactions : 2 000, sur ses propres messages.
    fill_reactions(&db, &crowded.to_string(), &gina_id, 250, 8).await;
    let refused = put_empty(&gina.browser, &reaction_uri(crowded, thread, message, "👍")).await;
    her_part(
        &refused.assert_error(StatusCode::CONFLICT, "reaction_limit"),
        "2 000 réactions",
    );
    put_empty(&olga.browser, &reaction_uri(crowded, thread, message, "👍"))
        .await
        .assert_status(StatusCode::OK);
}

/// Ce qu'une lecture des fils d'une page rend au plus quand elle est au pire cas accepté : le
/// texte des messages (4 Mio), ce qui les entoure (auteurs, dates, identifiants) et 20 000
/// réactions. Tant que la lecture n'est pas paginée, c'est le prix de la première synchro de
/// chaque onglet (mesuré : `measure_comments_of_a_full_page`, `tests/measure.rs`).
const FULL_PAGE_READ: usize = 12 * 1024 * 1024;

/// La sonde (a) de la relecture de S4 et celle de la re-relecture, inversées (Rulings R350,
/// R357) : une page pleine de tout ce qu'elle accepte (10 000 messages, 4 Mio de texte, 20 000
/// réactions, chaque compte dans sa part) refuse un message et une réaction de plus, et la lecture
/// de ses fils reste sous [`FULL_PAGE_READ`]. Avant, à 1 % du plafond des fils, une lecture
/// rendait 192 Mio ; avec les réactions sans plafond, chaque compte y ajoutait 14 Mio.
#[tokio::test]
async fn reading_a_full_page_stays_under_a_bound() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let gina = &team.gina;
    let db = t.db_path();
    let grants = [members(Level::Full), user(gina, Level::Comment)];
    let full = team.page(None, "Pleine", &grants).await;
    // 25 fils de 400 messages de 419 octets, cinq comptes : 2 000 messages et 838 000 octets
    // chacun, 4 190 000 en tout. Puis dix comptes posent chacun 2 000 réactions.
    let writers = filler_accounts(&db, 0, 5).await;
    fill_threads(&db, &full.to_string(), &writers, (0, 25), (400, 419)).await;
    for filler in filler_accounts(&db, 5, 10).await {
        fill_reactions(&db, &full.to_string(), &filler, 250, 8).await;
    }
    let reactions = common::query_i64(&db, "SELECT COUNT(*) FROM comment_reactions").await;
    assert_eq!(reactions, i64::from(MAX_REACTIONS_PER_PAGE));
    gina.browser
        .post(&comments_uri(full), json!({ "body": "Un de trop" }))
        .await
        .assert_error(StatusCode::CONFLICT, "page_comment_limit");
    let (thread, message) = (
        "01890000-0000-7000-a001-000000000000",
        "01890000-0000-7000-a002-000000000000",
    );
    let refused = put_empty(&gina.browser, &reaction_uri(full, thread, message, "🦊")).await;
    let body = refused.assert_error(StatusCode::CONFLICT, "reaction_limit");
    assert_french(&body, &["20 000 réactions"]);

    let read = gina.browser.get(&comments_uri(full)).await;
    read.assert_status(StatusCode::OK);
    let threads = read.json();
    let threads = threads.as_array().unwrap();
    let count = |key: &str| -> usize {
        threads
            .iter()
            .flat_map(|t| t["comments"].as_array().unwrap())
            .map(|c| match key {
                "comments" => 1,
                _ => c["reactions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r["users"].as_array().unwrap().len())
                    .sum(),
            })
            .sum()
    };
    assert_eq!(count("comments"), MAX_MESSAGES_PER_PAGE as usize);
    assert_eq!(count("reactions"), MAX_REACTIONS_PER_PAGE as usize);
    assert!(
        read.body.len() < FULL_PAGE_READ,
        "{} octets",
        read.body.len()
    );
}
