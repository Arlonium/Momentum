//! Les rétroliens d'une page (lot 4, Task S6) : `GET /api/v1/pages/{id}/backlinks` rend les pages
//! qui mènent à elle (un lien de page, une mention de page, un lien du texte vers son adresse),
//! celles que le compte voit, de l'espace de la page, hors corbeille.
//!
//! Le chemin entier : les pages sont écrites par `PUT /mmd`, l'indexeur les projette dans
//! `page_links`, la route les lit. Les pages et les rôles de départ sont posés par le store
//! (`Team`) ; ce qu'on teste (accès donnés, corbeille, liens retirés) passe par les routes. Les
//! bornes (deux cents pages vues, vingt mille parcourues) et ce que des sources non vues ne doivent
//! jamais cacher se prouvent sur la route elle-même, dans `src/routes/backlinks.rs`, où elles se
//! règlent ; que les liens d'un autre espace ne sont même pas lus, dans le store
//! (`sources_of_another_space_are_not_even_read`, `crates/mementum-store/tests/search.rs`). Le
//! `404` d'une page qu'on ne voit pas est celui de toutes les routes de page
//! (`foreign_page_is_indistinguishable_from_missing`, par `page_routes`).

mod common;
#[path = "../../mementum-sync/tests/common/limit.rs"]
mod limit;

use std::time::{Duration, Instant};

use axum::http::{StatusCode, header};
use common::sync::{members, user};
use common::{Account, Browser, Reply, Team, TestApp, uuid};
use mementum_acl::Level;
use mementum_server::SignupMode;
use serde_json::json;
use uuid::Uuid;

/// Le calme qu'attend l'indexeur avant de relire une page écrite : court, pour que `settled`
/// rende vite.
const QUICK: Duration = Duration::from_millis(20);

/// Le temps de réponse plancher d'une lecture qui passe (`BACKLINKS_FLOOR`, Ruling R1022).
const FLOOR: Duration = Duration::from_millis(75);

/// Une application aux inscriptions ouvertes, dont l'indexeur n'attend que [`QUICK`] de calme.
async fn quick_app() -> TestApp {
    TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.index_after = QUICK;
    })
    .await
}

fn uri(page: Uuid) -> String {
    format!("/api/v1/pages/{page}/backlinks")
}

/// `GET …/backlinks`, par `by`.
async fn backlinks(by: &Browser<'_>, page: Uuid) -> Reply {
    by.get(&uri(page)).await
}

/// Ce que `by` lit des rétroliens de `page` : `200`, et chaque page qui y mène, `(pageId, blockId,
/// excerpt)`, dans l'ordre de la réponse.
async fn listed(by: &Browser<'_>, page: Uuid) -> Vec<(Uuid, Option<String>, String)> {
    let reply = backlinks(by, page).await;
    reply.assert_status(StatusCode::OK);
    let body = reply.json();
    let pages = body["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("« pages » en liste attendu : {body}"));
    pages
        .iter()
        .map(|p| {
            let mut fields: Vec<&str> = p.as_object().unwrap().keys().map(String::as_str).collect();
            fields.sort_unstable();
            assert_eq!(fields, ["blockId", "excerpt", "pageId"], "{p}");
            (
                uuid(&p["pageId"]),
                p["blockId"].as_str().map(str::to_string),
                p["excerpt"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// Les seules pages des rétroliens de `page` que `by` lit, dans l'ordre de la réponse.
async fn sources(by: &Browser<'_>, page: Uuid) -> Vec<Uuid> {
    listed(by, page)
        .await
        .into_iter()
        .map(|(id, ..)| id)
        .collect()
}

/// Aucune page : le type d'une liste vide, que `assert_eq!` ne devine pas.
const NONE: [Uuid; 0] = [];

/// Récrit la page par `PUT /mmd`, au nom de `by`.
async fn write(by: &Account<'_>, page: Uuid, mmd: &str) {
    by.put_mmd(&page.to_string(), mmd.to_string())
        .await
        .assert_status(StatusCode::NO_CONTENT);
}

/// La clé du bloc de `page` dont le texte indexé est `text`, telle que `blocks_index` la garde.
async fn block_of(t: &TestApp, page: Uuid, text: &str) -> String {
    t.store
        .indexed_blocks(page)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.text == text)
        .unwrap_or_else(|| panic!("aucun bloc « {text} » indexé dans {page}"))
        .block_id
}

/// Un lien de page, une mention de page et un lien du texte vers l'adresse d'une page sont trois
/// rétroliens : chacun rendu avec le bloc qui mène et son texte, dans l'ordre des identifiants des
/// pages. Témoin : une page qui ne mène qu'ailleurs n'y est pas.
#[tokio::test]
async fn a_page_link_a_mention_and_a_link_in_a_text_are_backlinks() {
    let t = quick_app().await;
    let team = Team::new(&t).await;
    let olga = &team.olga;
    let target = team.page(None, "Cible", &[members(Level::Full)]).await;
    let by_link = team
        .page(None, "Par un lien de page", &[members(Level::Full)])
        .await;
    let by_mention = team
        .page(None, "Par une mention", &[members(Level::Full)])
        .await;
    let by_text = team
        .page(None, "Par le texte", &[members(Level::Full)])
        .await;
    let other = team.page(None, "Ailleurs", &[members(Level::Full)]).await;
    write(
        olga,
        by_link,
        &format!("<page id=\"{target}\">Cible</page>\n"),
    )
    .await;
    write(
        olga,
        by_mention,
        &format!("Voir <mention type=\"page\" id=\"{target}\">Cible</mention> demain.\n"),
    )
    .await;
    write(olga, by_text, &format!("Lire [la cible](/p/{target}).\n")).await;
    write(olga, other, &format!("Lire [l'autre](/p/{by_link}).\n")).await;
    t.indexer.settled().await;

    let expected = vec![
        (
            by_link,
            Some(block_of(&t, by_link, "Cible").await),
            "Cible".to_string(),
        ),
        (
            by_mention,
            Some(block_of(&t, by_mention, "Voir Cible demain.").await),
            "Voir Cible demain.".to_string(),
        ),
        (
            by_text,
            Some(block_of(&t, by_text, "Lire la cible.").await),
            "Lire la cible.".to_string(),
        ),
    ];
    assert!(by_link < by_mention && by_mention < by_text);
    assert_eq!(listed(&team.mona.browser, target).await, expected);
    // Témoin : le lien de « Ailleurs » mène à sa propre cible.
    assert_eq!(sources(&team.mona.browser, by_link).await, [other]);
}

/// Review Focus 1 : une page qui mène à la cible, mais que le lecteur ne voit pas (privée d'un
/// autre), n'est pas rendue, et la réponse est celle d'une page sans rétrolien. Témoin : l'accès
/// donné (par la route du partage), elle paraît.
#[tokio::test]
async fn a_backlink_from_an_unseen_page_is_not_listed() {
    let t = quick_app().await;
    let team = Team::new(&t).await;
    let target = team.page(None, "Cible", &[members(Level::Read)]).await;
    let alone = team
        .page(None, "Sans rétrolien", &[members(Level::Read)])
        .await;
    let hidden = team
        .page(None, "Privée de Tom", &[user(&team.tom, Level::Full)])
        .await;
    write(
        &team.tom,
        hidden,
        &format!("Vers [la cible](/p/{target}).\n"),
    )
    .await;
    t.indexer.settled().await;
    let mona = &team.mona.browser;
    assert_eq!(sources(&team.tom.browser, target).await, [hidden]);

    let reply = backlinks(mona, target).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(reply.body, backlinks(mona, alone).await.body);
    assert_eq!(reply.json(), json!({ "pages": [] }));

    team.tom
        .browser
        .put(
            &format!("/api/v1/pages/{hidden}/access/users/{}", team.mona.id),
            json!({ "level": "read" }),
        )
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(sources(mona, target).await, [hidden]);
}

/// Une page qui mène à la cible et qu'on met à la corbeille n'est plus rendue, ni sa sous-page
/// (à la corbeille par elle) ; restaurées, elles reviennent. Rien n'est réindexé entre-temps :
/// c'est la route qui juge, à chaque requête.
#[tokio::test]
async fn a_backlink_from_a_trashed_page_is_not_listed() {
    let t = quick_app().await;
    let team = Team::new(&t).await;
    let olga = &team.olga;
    let target = team.page(None, "Cible", &[members(Level::Full)]).await;
    let source = team.page(None, "Source", &[members(Level::Full)]).await;
    let child = team.page(Some(source), "Sous-page", &[]).await;
    write(olga, source, &format!("Vers [la cible](/p/{target}).\n")).await;
    write(olga, child, &format!("Vers [la cible](/p/{target}).\n")).await;
    t.indexer.settled().await;
    let mona = &team.mona.browser;
    let both = {
        let mut both = vec![source, child];
        both.sort();
        both
    };
    assert_eq!(sources(mona, target).await, both);

    t.indexer.stop().await;
    mona.post_empty(&format!("/api/v1/pages/{source}/trash"))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(sources(mona, target).await, NONE);
    mona.post_empty(&format!("/api/v1/pages/{source}/restore"))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(sources(mona, target).await, both);
}

/// Une page d'un autre espace qui mène à la cible n'est pas rendue, même à qui la voit (L-e) :
/// les rétroliens ne viennent que de l'espace de la page. Témoin : la même page, dans l'espace
/// de la cible, l'est.
#[tokio::test]
async fn a_backlink_from_another_space_is_not_listed() {
    let t = quick_app().await;
    let team = Team::new(&t).await;
    let olga = &team.olga;
    let target = team.page(None, "Cible", &[members(Level::Full)]).await;
    let elsewhere = team.another_space("Ailleurs").await;
    let far = team
        .page_in(elsewhere, None, "Loin", &[members(Level::Full)])
        .await;
    let near = team.page(None, "Près", &[members(Level::Full)]).await;
    for source in [far, near] {
        write(olga, source, &format!("Vers [la cible](/p/{target}).\n")).await;
    }
    t.indexer.settled().await;
    // Mona voit les deux pages, chacune dans son espace.
    for source in [far, near] {
        team.mona
            .browser
            .get(&format!("/api/v1/pages/{source}"))
            .await
            .assert_status(StatusCode::OK);
    }
    assert_eq!(sources(&team.mona.browser, target).await, [near]);
}

/// Une page qui mène deux fois à la cible n'est rendue qu'une fois, avec le premier de ses blocs
/// qui y mène dans l'ordre des identifiants de bloc (Ruling R963), quel que soit l'ordre du
/// document.
#[tokio::test]
async fn a_page_linking_twice_is_listed_once() {
    let t = quick_app().await;
    let team = Team::new(&t).await;
    let target = team.page(None, "Cible", &[members(Level::Full)]).await;
    let source = team.page(None, "Source", &[members(Level::Full)]).await;
    let mmd = format!(
        "Premier [lien](/p/{target}).\n\nAutre chose.\n\nSecond <mention type=\"page\" \
         id=\"{target}\">Cible</mention>.\n"
    );
    write(&team.olga, source, &mmd).await;
    t.indexer.settled().await;
    let first = block_of(&t, source, "Premier lien.").await;
    let second = block_of(&t, source, "Second Cible.").await;
    let (block, text) = if first < second {
        (first, "Premier lien.")
    } else {
        (second, "Second Cible.")
    };
    assert_eq!(
        listed(&team.mona.browser, target).await,
        [(source, Some(block), text.to_string())]
    );
}

/// Un lien retiré de sa page, la page réindexée, n'est plus un rétrolien. Témoin : il l'était.
#[tokio::test]
async fn a_link_removed_leaves_the_backlinks() {
    let t = quick_app().await;
    let team = Team::new(&t).await;
    let target = team.page(None, "Cible", &[members(Level::Full)]).await;
    let source = team.page(None, "Source", &[members(Level::Full)]).await;
    write(
        &team.olga,
        source,
        &format!("Vers [la cible](/p/{target}).\n"),
    )
    .await;
    t.indexer.settled().await;
    let mona = &team.mona.browser;
    assert_eq!(sources(mona, target).await, [source]);

    write(&team.olga, source, "Plus de lien.\n").await;
    t.indexer.settled().await;
    assert_eq!(sources(mona, target).await, NONE);
}

/// Une page qui mène à elle-même n'est pas son propre rétrolien. Témoin : une autre page qui y
/// mène l'est.
#[tokio::test]
async fn a_page_does_not_backlink_itself() {
    let t = quick_app().await;
    let team = Team::new(&t).await;
    let target = team.page(None, "Cible", &[members(Level::Full)]).await;
    let other = team.page(None, "Autre", &[members(Level::Full)]).await;
    for page in [target, other] {
        write(
            &team.olga,
            page,
            &format!("Vers [la cible](/p/{target}).\n"),
        )
        .await;
    }
    t.indexer.settled().await;
    assert_eq!(sources(&team.mona.browser, target).await, [other]);
}

/// Lire la page suffit (« Peut lire ») : un lecteur reçoit ses rétroliens, sous les en-têtes de
/// toute l'API (`no-store`, `nosniff`). Un invité aussi, pour une page partagée avec lui, et il n'y
/// lit que les pages qu'il voit.
#[tokio::test]
async fn a_reader_gets_the_backlinks() {
    let t = quick_app().await;
    let team = Team::new(&t).await;
    let target = team
        .page(
            None,
            "Cible",
            &[members(Level::Read), user(&team.gina, Level::Read)],
        )
        .await;
    let shared = team
        .page(
            None,
            "Partagée",
            &[members(Level::Read), user(&team.gina, Level::Read)],
        )
        .await;
    let members_only = team
        .page(None, "Des membres", &[members(Level::Read)])
        .await;
    for source in [shared, members_only] {
        write(
            &team.olga,
            source,
            &format!("Vers [la cible](/p/{target}).\n"),
        )
        .await;
    }
    t.indexer.settled().await;

    let started = Instant::now();
    let reply = backlinks(&team.mona.browser, target).await;
    // Le plancher de la route (Ruling R1022) : une borne basse, que la lenteur du poste ne peut
    // pas faire tenir à tort tant que la lecture, sans lui, prend moins de 75 ms ; la preuve sans
    // minutage est dans `src/routes/backlinks.rs` (`an_answer_never_leaves_before_the_floor`).
    assert!(started.elapsed() >= FLOOR, "{:?}", started.elapsed());
    reply.assert_status(StatusCode::OK);
    assert_eq!(
        reply.headers[header::CACHE_CONTROL],
        "no-store",
        "{:?}",
        reply.headers
    );
    assert_eq!(reply.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(reply.content_type.as_deref(), Some("application/json"));
    let mut both = vec![shared, members_only];
    both.sort();
    assert_eq!(sources(&team.mona.browser, target).await, both);
    assert_eq!(sources(&team.gina.browser, target).await, [shared]);
}

/// Une page à la corbeille se lit encore : ses rétroliens aussi (ce sont les sources à la
/// corbeille qui ne sont pas rendues).
#[tokio::test]
async fn a_reader_gets_the_backlinks_in_the_trash_too() {
    let t = quick_app().await;
    let team = Team::new(&t).await;
    let target = team.page(None, "Cible", &[members(Level::Read)]).await;
    let source = team.page(None, "Source", &[members(Level::Read)]).await;
    write(
        &team.olga,
        source,
        &format!("Vers [la cible](/p/{target}).\n"),
    )
    .await;
    t.indexer.settled().await;
    team.trash(target).await;
    let mona = &team.mona.browser;
    mona.get(&format!("/api/v1/pages/{target}"))
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(sources(mona, target).await, [source]);
}

/// Les limites connues (Ruling R340) : ce que le test dit voulu n'est pas tenu, et il l'affirme.
/// Le jour où la limite est levée, il échoue : le sortir de ce module, et lui faire affirmer le
/// comportement voulu.
mod known_limits {
    use super::limit::known_limit;
    use super::*;

    /// Voulu : un lien écrit sous l'adresse publique entière du serveur (`MEMENTUM_PUBLIC_URL`)
    /// cesse d'être un rétrolien quand l'adresse publique change, et un lien écrit sous la
    /// nouvelle en devient un. Aujourd'hui (Ruling R998) : l'origine publique n'est pas gardée
    /// avec la version de la projection ; la changer ne fait rien reprojeter. Le lien sous
    /// l'ancienne adresse reste un rétrolien après le redémarrage, jusqu'à la prochaine écriture
    /// de son bloc. Les liens relatifs (`/p/<id>`), que l'application écrit, n'en dépendent pas.
    #[tokio::test]
    async fn a_link_under_the_old_public_address_stays_a_backlink() {
        let t = TestApp::with_config(|cfg| {
            cfg.signup = SignupMode::Open;
            cfg.index_after = QUICK;
            cfg.public_url = Some("https://ancienne.exemple.fr".to_string());
        })
        .await;
        let olga = t.account("Olga").await;
        let page = |title: &str| {
            let olga = &olga;
            let title = title.to_string();
            async move {
                let node = olga.create(json!({ "title": title })).await;
                Uuid::parse_str(&node.id).unwrap()
            }
        };
        let target = page("Cible").await;
        let source = page("Source").await;
        write(
            &olga,
            source,
            &format!("Vers [la cible](https://ancienne.exemple.fr/p/{target}).\n"),
        )
        .await;
        t.indexer.settled().await;
        assert_eq!(sources(&olga.browser, target).await, [source]);
        let session = olga.browser.session();
        drop(olga);

        let (dir, mut cfg) = t.stop().await;
        cfg.public_url = Some("https://nouvelle.exemple.fr".to_string());
        let t = TestApp::on(dir, cfg).await;
        t.indexer.settled().await;
        let browser = t.browser();
        browser.set_session(session.as_deref());
        let mut unmet = Vec::new();
        let after = sources(&browser, target).await;
        if after != NONE {
            unmet.push(format!(
                "rétroliens sous la nouvelle adresse publique : {} page(s)",
                after.len()
            ));
        }
        let loss = vec!["rétroliens sous la nouvelle adresse publique : 1 page(s)".to_string()];
        known_limit(
            "R998 : l'origine publique n'est pas gardée avec la version de la projection ; la \
             changer ne reprojette rien. La lever demande de la garder dans le store et de la \
             comparer au démarrage de l'indexeur ; d'ici là, `mementum reindex` répare",
            &unmet,
            &loss,
        );
    }
}
