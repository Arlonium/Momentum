//! L'historique d'une page (Ruling R135), par l'API : qui lit les versions, qui restaure, à
//! chaque niveau, à la corbeille ou non ; une version ne se lit que par sa page ; ce que
//! reçoivent les connexions de synchro quand on restaure.
//!
//! Les versions de départ sont posées par le store : ce n'est pas leur coupe qu'on teste ici
//! (elle l'est dans `mementum-sync`, `tests/versions.rs`).

mod common;

use axum::http::StatusCode;
use common::sync::{members, open_app, user};
use common::{Team, TestApp, WAIT, sync_url, synced_as, uuid};
use mementum_acl::Level;
use mementum_crdt::PageDoc;
use mementum_md::{Block, parse};
use mementum_store::VersionReason;
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

/// Lire les versions et leur état se peut dès « Peut lire », corbeille comprise ; restaurer
/// demande « Peut modifier » (`403` en dessous), et une page hors de la corbeille (`409
/// trashed`, après le refus de niveau). Une version se lit sans l'adresse de personne : son
/// identifiant, sa date, sa raison et ses auteurs (identifiant et nom).
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
            assert_eq!(keys, ["authors", "createdAt", "id", "reason"], "{what}");
            assert_eq!(only[0]["id"], version.to_string(), "{what}");
            assert_eq!(only[0]["reason"], "periodic", "{what}");
            assert!(only[0]["createdAt"].is_i64(), "{what}");
            assert_eq!(only[0]["authors"], Value::from(authors), "{what}");

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
            match (level >= Level::Edit, trashed) {
                (false, _) => {
                    restored.assert_error(StatusCode::FORBIDDEN, "forbidden");
                }
                (true, true) => {
                    restored.assert_error(StatusCode::CONFLICT, "trashed");
                }
                (true, false) => {
                    restored.assert_status(StatusCode::NO_CONTENT);
                    let mmd = team.olga.get_mmd(&page.to_string()).await;
                    assert_eq!(mmd.text(), "Avant\n", "{what}");
                }
            }
            let after = tom.browser.get(&versions_uri(page)).await.json();
            let count = after.as_array().unwrap().len();
            let restored = level >= Level::Edit && !trashed;
            assert_eq!(count, 1 + usize::from(restored), "{what} : {after}");
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
