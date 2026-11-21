//! L'historique d'une page (Ruling R135), par l'API : qui lit les versions, qui restaure, à
//! chaque niveau, à la corbeille ou non ; une version ne se lit que par sa page ; ce que
//! reçoivent les connexions de synchro quand on restaure.
//!
//! Les versions de départ sont posées par le store : ce n'est pas leur coupe qu'on teste ici
//! (elle l'est dans `mementum-sync`, `tests/versions.rs`).

mod common;

use axum::http::StatusCode;
use common::sync::{members, open_app, user};
use common::{Team, TestApp, WAIT, api_token, sync_url, synced_as, uuid};
use mementum_acl::Level;
use mementum_crdt::PageDoc;
use mementum_crdt::ops::{Operation, Place, read_markdown};
use mementum_md::{Block, parse};
use mementum_store::{TokenLevel, TokenScope, VersionReason, Writer};
use mementum_sync::{Edit, OnConflict};
use serde_json::{Value, json};
use uuid::Uuid;

fn versions_uri(page: impl std::fmt::Display) -> String {
    format!("/api/v1/pages/{page}/versions")
}

fn state_uri(page: impl std::fmt::Display, version: impl std::fmt::Display) -> String {
    format!("{}/{version}/state", versions_uri(page))
}

fn restore_uri(page: impl std::fmt::Display, version: impl std::fmt::Display) -> String {
    format!("{}/{version}/restore", versions_uri(page))
}

/// L'état Yjs d'une page qui porte `mmd`, comme l'acteur en garde un.
fn state_of(mmd: &str) -> Vec<u8> {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(mmd).unwrap()).unwrap();
    page.encode_state()
}

fn blocks_of(state: &[u8]) -> Vec<Block> {
    PageDoc::from_updates([state]).unwrap().read_blocks()
}

/// Les niveaux d'accès, du plus bas au plus haut.
const LEVELS: [Level; 4] = [Level::Read, Level::Comment, Level::Edit, Level::Full];

/// Lire les versions et leur état demande « Peut modifier » (Ruling R203 ; `403` en dessous,
/// avant même de chercher la version), corbeille comprise ; restaurer aussi, et une page hors de
/// la corbeille (`409 trashed`, après le refus de niveau). Une version se lit sans l'adresse de
/// personne : son identifiant, sa date, sa raison et ses auteurs (identifiant et nom).
#[tokio::test]
async fn version_routes_follow_the_levels() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let tom = &team.tom;
    let state = state_of("Avant\n");
    for trashed in [false, true] {
        for level in LEVELS {
            let what = format!("{level:?}, corbeille : {trashed}");
            let grants = [user(&team.olga, Level::Full), user(tom, level)];
            let page = team.page(None, &what, &grants).await;
            let version = team
                .store
                .put_version(
                    page,
                    0,
                    &state,
                    &[team.mona.id, team.olga.id],
                    VersionReason::Periodic,
                )
                .await
                .unwrap();
            if trashed {
                team.trash(page).await;
            }

            let listed = tom.browser.get(&versions_uri(page)).await;
            if level < Level::Edit {
                // Un lecteur ne lit pas ce que la page portait avant qu'on la lui partage.
                listed.assert_error(StatusCode::FORBIDDEN, "forbidden");
                for version in [version.to_string(), Uuid::now_v7().to_string()] {
                    let kept = tom.browser.get(&state_uri(page, version)).await;
                    kept.assert_error(StatusCode::FORBIDDEN, "forbidden");
                }
                let restored = tom.browser.post_empty(&restore_uri(page, version)).await;
                restored.assert_error(StatusCode::FORBIDDEN, "forbidden");
                let after = team.olga.browser.get(&versions_uri(page)).await.json();
                assert_eq!(after.as_array().unwrap().len(), 1, "{what} : {after}");
                continue;
            }
            listed.assert_status(StatusCode::OK);
            let listed = listed.json();
            let mut authors = vec![
                json!({ "id": team.mona.id.to_string(), "name": "Mona" }),
                json!({ "id": team.olga.id.to_string(), "name": "Olga" }),
            ];
            authors.sort_by_key(|a| a["id"].as_str().unwrap().to_string());
            let only = &listed.as_array().unwrap()[..];
            assert_eq!(only.len(), 1, "{what} : {listed}");
            let mut keys: Vec<&str> = only[0]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                ["agentAuthors", "authors", "createdAt", "id", "reason"],
                "{what}"
            );
            assert_eq!(only[0]["id"], version.to_string(), "{what}");
            assert_eq!(only[0]["reason"], "periodic", "{what}");
            assert!(only[0]["createdAt"].is_i64(), "{what}");
            assert_eq!(only[0]["authors"], Value::from(authors), "{what}");
            assert_eq!(only[0]["agentAuthors"], json!([]), "{what}");

            let kept = tom.browser.get(&state_uri(page, version)).await;
            kept.assert_status(StatusCode::OK);
            assert_eq!(
                kept.content_type.as_deref(),
                Some("application/octet-stream"),
                "{what}"
            );
            assert_eq!(kept.body.as_ref(), state.as_slice(), "{what}");
            // Les en-têtes de toute réponse de l'API.
            assert_eq!(kept.headers["cache-control"], "no-store", "{what}");
            assert_eq!(kept.headers["x-content-type-options"], "nosniff", "{what}");

            let restored = tom.browser.post_empty(&restore_uri(page, version)).await;
            if trashed {
                restored.assert_error(StatusCode::CONFLICT, "trashed");
            } else {
                restored.assert_status(StatusCode::NO_CONTENT);
                let mmd = team.olga.get_mmd(&page.to_string()).await;
                assert_eq!(mmd.text(), "Avant\n", "{what}");
            }
            let after = tom.browser.get(&versions_uri(page)).await.json();
            let count = after.as_array().unwrap().len();
            assert_eq!(count, 1 + usize::from(!trashed), "{what} : {after}");
        }
    }
}

/// Une version ne se lit et ne se restaure que par sa page : celle d'une autre page (que la
/// personne voit pourtant), un identifiant inconnu ou mal formé reçoivent le `404` d'une page
/// introuvable, sur chaque route ; rien n'est écrit. La liste d'une page ne porte que ses
/// versions.
#[tokio::test]
async fn version_of_another_page_is_not_found() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let here = team.page(None, "Ici", &[members(Level::Full)]).await;
    let there = team.page(None, "Là", &[members(Level::Full)]).await;
    let theirs = team
        .store
        .put_version(there, 0, &state_of("Là\n"), &[], VersionReason::Closing)
        .await
        .unwrap();
    mona.put_mmd(&here.to_string(), "Ici\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);

    for version in [
        theirs.to_string(),
        Uuid::now_v7().to_string(),
        "pas-un-uuid".into(),
    ] {
        for reply in [
            mona.browser.get(&state_uri(here, &version)).await,
            mona.browser.post_empty(&restore_uri(here, &version)).await,
        ] {
            let body = reply.assert_error(StatusCode::NOT_FOUND, "not_found");
            assert_eq!(body["message"], "page introuvable", "{version}");
        }
    }
    let listed = mona.browser.get(&versions_uri(here)).await;
    listed.assert_status(StatusCode::OK);
    assert_eq!(listed.json(), json!([]));
    assert_eq!(mona.get_mmd(&here.to_string()).await.text(), "Ici\n");
    assert_eq!(team.store.list_versions(there).await.unwrap().len(), 1);
}

/// Restaurer dit aux connexions de la page qu'il y a une version de plus (trame `versions`),
/// et le contenu restauré leur arrive par la synchro, avec les identifiants des blocs de la
/// version. La version `before_restore` porte ce qui a été remplacé, au nom de qui l'avait
/// écrit.
#[tokio::test]
async fn restore_tells_the_page() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let (olga, mona) = (&team.olga, &team.mona);
    let page = team.page(None, "Restaurée", &[members(Level::Full)]).await;
    let state = state_of("Un\n\nDeux\n");
    let version = team
        .store
        .put_version(page, 0, &state, &[], VersionReason::Periodic)
        .await
        .unwrap();
    mona.put_mmd(&page.to_string(), "Remplacé\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let reader = synced_as(&mona.browser, &sync_url(addr, page)).await;

    olga.browser
        .post_empty(&restore_uri(page, version))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let expected = blocks_of(&state);
    assert!(
        reader.wait_until(WAIT, |c| c.blocks() == expected).await,
        "le contenu restauré n'est pas arrivé : {:?}",
        reader.blocks()
    );
    assert!(
        reader
            .wait_until(WAIT, |c| c
                .page_events()
                .iter()
                .any(|frame| frame == r#"{"type":"versions"}"#))
            .await,
        "trame `versions` attendue : {:?}",
        reader.page_events()
    );

    let listed = mona.browser.get(&versions_uri(page)).await.json();
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert_eq!(listed[0]["reason"], "before_restore");
    assert_eq!(
        listed[0]["authors"],
        json!([{ "id": mona.id.to_string(), "name": "Mona" }])
    );
    let replaced = mona
        .browser
        .get(&state_uri(page, uuid(&listed[0]["id"])))
        .await;
    assert_eq!(
        mementum_md::block_text(&blocks_of(&replaced.body)[0]),
        "Remplacé"
    );
    assert_eq!(listed[1]["id"], version.to_string());
}

/// Le réglage de l'intervalle des versions arrive au hub du serveur : avec une seconde, une
/// écriture qui suit la précédente de plus d'une seconde coupe une version.
#[tokio::test]
async fn version_interval_setting_reaches_the_hub() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = mementum_server::SignupMode::Open;
        cfg.version_every = std::time::Duration::from_secs(1);
    })
    .await;
    let team = Team::new(&t).await;
    let page = team.page(None, "Réglée", &[members(Level::Full)]).await;
    let id = page.to_string();
    team.mona
        .put_mmd(&id, "Un\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    team.mona
        .put_mmd(&id, "Deux\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let listed = team.mona.browser.get(&versions_uri(page)).await.json();
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["reason"], "periodic");
}

/// Ruling R186 : sur une page à la corbeille, une version qui n'est pas de cette page, inconnue
/// ou mal nommée reçoit le `404` d'une page introuvable avant le `409 trashed`, comme le fil
/// d'un commentaire (R168) ; une version de la page reçoit `409`. Sous le niveau, `403` passe
/// avant les deux.
#[tokio::test]
async fn missing_version_comes_before_trashed() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, gina) = (&team.mona, &team.gina);
    let binned = team
        .page(
            None,
            "Jetée",
            &[members(Level::Full), user(gina, Level::Read)],
        )
        .await;
    let elsewhere = team.page(None, "Ailleurs", &[members(Level::Full)]).await;
    let ours = team
        .store
        .put_version(binned, 0, &state_of("Ici\n"), &[], VersionReason::Closing)
        .await
        .unwrap();
    let theirs = team
        .store
        .put_version(elsewhere, 0, &state_of("Là\n"), &[], VersionReason::Closing)
        .await
        .unwrap();
    team.trash(binned).await;

    for version in [
        theirs.to_string(),
        Uuid::now_v7().to_string(),
        "pas-un-uuid".into(),
    ] {
        let reply = mona
            .browser
            .post_empty(&restore_uri(binned, &version))
            .await;
        let body = reply.assert_error(StatusCode::NOT_FOUND, "not_found");
        assert_eq!(body["message"], "page introuvable", "{version}");
        // Une lectrice : le niveau d'abord.
        let reply = gina
            .browser
            .post_empty(&restore_uri(binned, &version))
            .await;
        reply.assert_error(StatusCode::FORBIDDEN, "forbidden");
    }
    let reply = mona.browser.post_empty(&restore_uri(binned, ours)).await;
    reply.assert_error(StatusCode::CONFLICT, "trashed");
}

/// Le scénario de la relecture finale (S5), par le vrai chemin : Olga écrit un secret dans une
/// page privée, puis le récrit ; une version le garde. Elle partage la page en lecture avec
/// Gina, une invitée : Gina ne lit ni la liste des versions ni leur état (`403`, Ruling R203).
/// Olga, elle, les lit.
#[tokio::test]
async fn a_reader_does_not_read_what_was_erased_before_sharing() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = mementum_server::SignupMode::Open;
        cfg.version_every = std::time::Duration::from_secs(1);
    })
    .await;
    let team = Team::new(&t).await;
    let (olga, gina) = (&team.olga, &team.gina);
    let page = team.page(None, "Privée", &[user(olga, Level::Full)]).await;
    let id = page.to_string();
    olga.put_mmd(&id, "Mot de passe du coffre : hunter2\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    olga.put_mmd(&id, "Rien à voir\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let listed = olga.browser.get(&versions_uri(page)).await;
    listed.assert_status(StatusCode::OK);
    let version = uuid(&listed.json()[0]["id"]);
    let kept = olga.browser.get(&state_uri(page, version)).await;
    kept.assert_status(StatusCode::OK);
    assert_eq!(
        mementum_md::block_text(&blocks_of(&kept.body)[0]),
        "Mot de passe du coffre : hunter2"
    );

    team.store
        .set_grant(
            page,
            mementum_acl::Grant {
                subject: mementum_acl::Subject::User(gina.id),
                level: Level::Read,
            },
        )
        .await
        .unwrap();
    gina.get_mmd(&id).await.assert_status(StatusCode::OK);
    gina.browser
        .get(&versions_uri(page))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    gina.browser
        .get(&state_uri(page, version))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
}

/// Garde des attributs (Ruling R701) : une version coupée avant la garde, dont l'état porte un
/// attribut de bloc qui empêche un éditeur de se monter (`backgroundColor = {"toString": 1}`),
/// est servie sans lui ; une version saine est servie telle qu'elle a été gardée, octet pour
/// octet.
#[tokio::test]
async fn a_version_kept_before_the_attribute_guard_is_served_without_the_attribute() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let olga = &team.olga;
    let page = team.page(None, "Historique", &[members(Level::Full)]).await;
    let sound = state_of("Un\n");
    // L'état sain, où la valeur « default » de la couleur de fond (une chaîne de lib0 : 119, sa
    // longueur, ses octets) devient l'objet `{"toString": 1}` (118 : un objet d'une entrée ; 125 :
    // un entier) : ce qu'un client à soi écrivait avant la garde.
    let default: Vec<u8> = [&[119, 7][..], b"default"].concat();
    let at = sound
        .windows(default.len())
        .position(|w| w == default)
        .expect("la couleur de fond du paragraphe");
    let object: Vec<u8> = [&[118, 1, 8][..], b"toString", &[125, 1]].concat();
    let hostile = [&sound[..at], &object, &sound[at + default.len()..]].concat();
    assert!(
        hostile.windows(8).any(|w| w == b"toString"),
        "l'état forgé porte la valeur"
    );
    let mut served = Vec::new();
    for state in [&sound, &hostile] {
        let version = team
            .store
            .put_version(page, 0, state, &[], VersionReason::Periodic)
            .await
            .unwrap();
        let kept = olga.browser.get(&state_uri(page, version)).await;
        kept.assert_status(StatusCode::OK);
        served.push(kept.body.to_vec());
    }
    assert!(
        blocks_of(&served[0]) == blocks_of(&sound),
        "une version saine est servie avec ses blocs"
    );
    assert!(
        !served[1].windows(8).any(|w| w == b"toString"),
        "la valeur est servie"
    );
    assert_eq!(blocks_of(&served[1]), blocks_of(&sound), "le reste est là");
}

/// Lot 5, Task C2 : une version dit ceux de ses auteurs qui ont écrit par un jeton d'API
/// (`agentAuthors`, leurs identifiants, dans l'ordre de `authors`), et rien de plus : un compte que
/// la version garde parmi ses auteurs d'agent sans le garder parmi ses auteurs n'y paraît pas, et
/// une version sans écriture d'agent rend une liste vide.
#[tokio::test]
async fn agent_authors_are_a_subset_of_the_authors() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, olga, gina) = (&team.mona, &team.olga, &team.gina);
    let page = team.page(None, "Écrite", &[members(Level::Full)]).await;
    let state = state_of("Un\n");
    let ordinary = team
        .store
        .put_version_by(page, 0, &state, &[mona.id], &[], VersionReason::Periodic)
        .await
        .unwrap();
    // Gina ne compte que parmi les auteurs d'agent : la route ne la rend pas.
    let by_agent = team
        .store
        .put_version_by(
            page,
            1,
            &state,
            &[olga.id, mona.id],
            &[mona.id, gina.id, olga.id],
            VersionReason::BeforeAgent,
        )
        .await
        .unwrap();

    let listed = olga.browser.get(&versions_uri(page)).await.json();
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert_eq!(listed[0]["id"], by_agent.to_string());
    assert_eq!(listed[0]["reason"], "before_agent");
    let authors: Vec<&str> = listed[0]["authors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|author| author["id"].as_str().unwrap())
        .collect();
    let agents: Vec<&str> = listed[0]["agentAuthors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    let mut expected = vec![mona.id.to_string(), olga.id.to_string()];
    expected.sort();
    assert_eq!(authors, expected, "{listed:?}");
    assert_eq!(
        agents, expected,
        "les auteurs, dans leur ordre : {listed:?}"
    );
    assert!(
        !agents.contains(&gina.id.to_string().as_str()),
        "{listed:?}"
    );
    assert_eq!(listed[1]["id"], ordinary.to_string());
    assert_eq!(listed[1]["agentAuthors"], json!([]));
}

/// Lot 5, Task C2 : l'historique ne nomme jamais un jeton. Une page écrite par le jeton de Tom
/// (par l'acteur, comme une route d'agent) garde à son arrêt une version dont Tom est l'auteur, par
/// un agent ; la liste des versions ne porte ni le nom du jeton, ni son identifiant, ni sa fin.
#[tokio::test]
async fn the_token_name_is_never_in_a_version() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let tom = &team.tom;
    let page = team
        .page(None, "Par un agent", &[user(tom, Level::Edit)])
        .await;
    let (text, token) = api_token(&t.store, tom.id, TokenLevel::Edit, TokenScope::All, false).await;
    let read = t.hub.read_page(page).await.unwrap();
    t.hub
        .edit_page(
            page,
            Edit {
                ops: vec![Operation::Insert {
                    at: Place::End,
                    blocks: read_markdown(0, "Écrit par un agent\n").unwrap(),
                }],
                base: Some((read.revision.clone(), Some(read.hashes.clone()))),
                on_conflict: OnConflict::Rebase,
                writer: Writer {
                    account: Some(tom.id),
                    token: Some(token),
                },
                commented: None,
                alone: false,
            },
        )
        .await
        .unwrap();
    // L'acteur s'arrête : la tranche, la première de la page, garde sa version de fin.
    t.hub.shutdown().await;

    let listed = tom.browser.get(&versions_uri(page)).await;
    listed.assert_status(StatusCode::OK);
    let json = listed.json();
    let versions = json.as_array().unwrap();
    assert_eq!(versions.len(), 1, "{json}");
    assert_eq!(versions[0]["agentAuthors"], json!([tom.id.to_string()]));
    assert_eq!(
        versions[0]["authors"],
        json!([{ "id": tom.id.to_string(), "name": "Tom" }])
    );
    let body = String::from_utf8(listed.body.to_vec()).unwrap();
    let hint = &text[text.len() - 4..];
    for secret in ["agent de test", &token.to_string(), hint, &text] {
        assert!(!body.contains(secret), "« {secret} » dans {body}");
    }
}
