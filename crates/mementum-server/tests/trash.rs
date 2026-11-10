//! La corbeille : une page y part avec tout son sous-arbre et en revient avec lui, personne
//! n'écrit dans une page qui y est, on ne supprime définitivement que ce qui y est, et ce qui
//! est supprimé l'est pour de bon. Les connexions de synchro des pages touchées sont fermées.
//!
//! Les aides (application en mémoire, équipe, routes d'une page) sont dans `common/mod.rs`,
//! celles des connexions de synchro dans `common/sync.rs`.

mod common;

use axum::http::StatusCode;
use common::sync::{members, open_app, still_served, user};
use common::{
    Account, Browser, Team, TestApp, WAIT, exec_sql, outcome, page_routes, query_i64, sync_as,
    sync_url, synced_as, text, trash_list, tree_nodes,
};
use mementum_acl::Level;
use mementum_sync::testkit::Client;
use serde_json::{Value, json};
use uuid::Uuid;

async fn trash(by: &Browser<'_>, id: impl std::fmt::Display) -> common::Reply {
    by.post_empty(&format!("/api/v1/pages/{id}/trash")).await
}

async fn restore(by: &Browser<'_>, id: impl std::fmt::Display) -> common::Reply {
    by.post_empty(&format!("/api/v1/pages/{id}/restore")).await
}

async fn delete(by: &Browser<'_>, id: impl std::fmt::Display) -> common::Reply {
    by.delete(&format!("/api/v1/pages/{id}")).await
}

/// Les titres d'une liste de nœuds, dans l'ordre de la liste.
fn titles(nodes: &[Value]) -> Vec<&str> {
    nodes.iter().map(|n| text(n, "title")).collect()
}

/// Review Focus 5 : une page part à la corbeille avec ses sous-pages, et en revient avec
/// elles, chacune à sa place. Tant qu'elle y est, aucune des trois ne s'écrit, et toutes se
/// lisent encore.
#[tokio::test]
async fn trash_takes_subtree_and_restore_brings_it_back() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    alice.create(json!({ "title": "Avant" })).await;
    let page = alice.create(json!({ "title": "Page" })).await;
    let first = alice
        .create(json!({ "title": "Première", "parentId": page.id }))
        .await;
    let second = alice
        .create(json!({ "title": "Seconde", "parentId": page.id }))
        .await;
    alice.create(json!({ "title": "Après" })).await;
    alice
        .put_mmd(&first.id, "Contenu\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let before = alice.tree().await;
    assert_eq!(before.nodes.len(), 5);

    let reply = trash(&alice.browser, &page.id).await;
    reply.assert_status(StatusCode::OK);
    let trashed = reply.json();
    assert_eq!(trashed["id"], page.id);
    let at = trashed["trashedAt"]
        .as_i64()
        .expect("la date de la corbeille");
    assert!(at >= page.updated_at, "{trashed}");

    // Aucune des trois dans l'arbre ; la page seule dans la corbeille.
    let nodes = tree_nodes(&alice.browser, alice.space).await;
    assert_eq!(titles(&nodes), ["Avant", "Après"]);
    let bin = trash_list(&alice.browser, alice.space).await;
    assert_eq!(bin, std::slice::from_ref(&trashed));

    // Les sous-pages y sont aussi, par héritage : elles se lisent, et ne s'écrivent plus.
    for id in [&page.id, &first.id, &second.id] {
        let node = alice.browser.get(&format!("/api/v1/pages/{id}")).await;
        node.assert_status(StatusCode::OK);
        assert_eq!(
            node.json()["trashedAt"],
            at,
            "à la corbeille depuis la même date"
        );
        assert_eq!(alice.get_mmd(id).await.status, StatusCode::OK);
        let refused = [
            alice.put_mmd(id, "Autre\n").await,
            alice.patch(id, json!({ "title": "Autre" })).await,
            alice.patch(id, json!({ "afterId": null })).await,
            alice.post(json!({ "parentId": id })).await,
            trash(&alice.browser, id).await,
        ];
        for reply in refused {
            reply.assert_error(StatusCode::CONFLICT, "trashed");
        }
    }
    alice
        .patch(&first.id, json!({ "parentId": null }))
        .await
        .assert_error(StatusCode::CONFLICT, "trashed");
    assert_eq!(alice.get_mmd(&first.id).await.text(), "Contenu\n");
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM nodes").await,
        5
    );

    // Restaurée : les trois reviennent, chacune à sa place, telles qu'elles étaient.
    let reply = restore(&alice.browser, &page.id).await;
    reply.assert_status(StatusCode::OK);
    let restored = reply.json();
    assert_eq!(restored["id"], page.id);
    assert!(restored["trashedAt"].is_null(), "{restored}");
    assert_eq!(alice.tree().await, before);
    assert_eq!(
        trash_list(&alice.browser, alice.space).await,
        [] as [Value; 0]
    );
    alice
        .put_mmd(&first.id, "Autre\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    // Restaurer une page qui n'est pas à la corbeille est sans effet.
    restore(&alice.browser, &page.id)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(alice.tree().await, before);
}

/// Restaurer une page dont un ancêtre est à la corbeille est refusé : on restaure l'ancêtre.
#[tokio::test]
async fn restore_under_trashed_parent_is_409() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Page" })).await;
    let child = alice
        .create(json!({ "title": "Enfant", "parentId": page.id }))
        .await;
    let other = alice
        .create(json!({ "title": "Autre enfant", "parentId": page.id }))
        .await;
    // L'enfant d'abord, puis sa page.
    trash(&alice.browser, &child.id)
        .await
        .assert_status(StatusCode::OK);
    trash(&alice.browser, &page.id)
        .await
        .assert_status(StatusCode::OK);
    let bin = trash_list(&alice.browser, alice.space).await;
    assert_eq!(titles(&bin), ["Page", "Enfant"], "la plus récente d'abord");

    // Mis à la corbeille lui-même, ou seulement par héritage : c'est la page qu'on restaure.
    for id in [&child.id, &other.id] {
        let body = restore(&alice.browser, id)
            .await
            .assert_error(StatusCode::CONFLICT, "parent_trashed");
        assert!(text(&body, "message").contains("parente"), "{body}");
    }
    assert_eq!(
        titles(&trash_list(&alice.browser, alice.space).await),
        ["Page", "Enfant"]
    );
    assert_eq!(
        tree_nodes(&alice.browser, alice.space).await,
        [] as [Value; 0]
    );

    // La page restaurée, l'enfant mis à la corbeille lui-même y reste ; l'autre revient.
    restore(&alice.browser, &page.id)
        .await
        .assert_status(StatusCode::OK);
    let nodes = tree_nodes(&alice.browser, alice.space).await;
    assert_eq!(titles(&nodes), ["Page", "Autre enfant"]);
    assert_eq!(
        titles(&trash_list(&alice.browser, alice.space).await),
        ["Enfant"]
    );
    restore(&alice.browser, &child.id)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(tree_nodes(&alice.browser, alice.space).await.len(), 3);
}

/// La corbeille d'un espace liste les pages mises à la corbeille (pas leurs descendants) que
/// la personne peut restaurer, la plus récente d'abord.
#[tokio::test]
async fn trash_lists_restorable_pages_most_recent_first() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let live = team.page(None, "Vivante", &[members(Level::Full)]).await;
    // `(titre, parent, accès, date de mise à la corbeille)`.
    let pages = [
        ("Des membres", None, vec![members(Level::Full)], 1000),
        ("En lecture", None, vec![members(Level::Read)], 3000),
        (
            "Privée de Mona",
            None,
            vec![user(&team.mona, Level::Full)],
            2000,
        ),
        ("Avec une sous-page", None, vec![members(Level::Edit)], 4000),
        ("Enfant d'une page vivante", Some(live), vec![], 500),
    ];
    for (title, parent, grants, at) in pages {
        let page = team.page(parent, title, &grants).await;
        if title == "Avec une sous-page" {
            team.page(Some(page), "Sous-page", &[]).await;
        }
        team.trash(page).await;
        let sql = format!("UPDATE nodes SET trashed_at = {at} WHERE id = '{page}'");
        exec_sql(&t.db_path(), &sql).await;
    }
    let listed = async |who: &Account<'_>| {
        let bin = trash_list(&who.browser, team.space).await;
        for node in &bin {
            assert!(node["trashedAt"].as_i64().is_some(), "{node}");
            assert_eq!(node["spaceId"], team.space.to_string());
        }
        bin.iter()
            .map(|n| text(n, "title").to_string())
            .collect::<Vec<_>>()
    };
    // Propriétaire et admin : tout ce que voient les membres.
    for who in [&team.olga, &team.adam] {
        assert_eq!(
            listed(who).await,
            [
                "Avec une sous-page",
                "En lecture",
                "Des membres",
                "Enfant d'une page vivante"
            ]
        );
    }
    // Un membre : ce qu'il peut restaurer (`edit`), sa page privée comprise.
    assert_eq!(
        listed(&team.mona).await,
        [
            "Avec une sous-page",
            "Privée de Mona",
            "Des membres",
            "Enfant d'une page vivante"
        ]
    );
    assert_eq!(
        listed(&team.tom).await,
        [
            "Avec une sous-page",
            "Des membres",
            "Enfant d'une page vivante"
        ]
    );
    // Une invitée ne voit rien de tout cela ; sans rôle, l'espace n'existe pas.
    assert_eq!(listed(&team.gina).await, [] as [&str; 0]);
    let uri = format!("/api/v1/spaces/{}/trash", team.space);
    team.zoe
        .browser
        .get(&uri)
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    t.browser()
        .get(&uri)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
}

/// L'ordre des refus : une page qu'on ne voit pas répond `404` même à la corbeille ; un niveau
/// insuffisant sur une page qu'on voit, `403`, avant `409 trashed`, qui ne va qu'à ceux qui
/// pourraient l'écrire.
#[tokio::test]
async fn trashed_page_refusals_come_in_order() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(
            None,
            "Jetée",
            &[members(Level::Read), user(&team.tom, Level::Edit)],
        )
        .await;
    team.trash(page).await;
    let id = page.to_string();
    let uri = format!("/api/v1/pages/{id}");

    // Qui ne la voit pas (une invitée, un compte hors de l'espace) : comme un id inconnu.
    let unknown = Uuid::now_v7().to_string();
    for who in [&team.gina, &team.zoe] {
        for ((name, request), (_, missing)) in
            page_routes(&id).into_iter().zip(page_routes(&unknown))
        {
            let reply = who.browser.send(request).await;
            reply.assert_error(StatusCode::NOT_FOUND, "not_found");
            let missing = who.browser.send(missing).await;
            assert_eq!(outcome(&reply), outcome(&missing), "{name}");
        }
    }

    // Qui la lit sans pouvoir l'écrire (Mona, `read`) : `403`, jamais `409`.
    let mona = &team.mona;
    assert_eq!(mona.browser.get(&uri).await.status, StatusCode::OK);
    assert_eq!(mona.get_mmd(&id).await.status, StatusCode::OK);
    let refused = [
        mona.put_mmd(&id, "Autre\n").await,
        mona.patch(&id, json!({ "title": "Autre" })).await,
        mona.patch(&id, json!({ "afterId": null })).await,
        mona.post(json!({ "parentId": id })).await,
        trash(&mona.browser, &id).await,
        restore(&mona.browser, &id).await,
        delete(&mona.browser, &id).await,
    ];
    for reply in refused {
        reply.assert_error(StatusCode::FORBIDDEN, "forbidden");
    }

    // Qui pourrait l'écrire (Tom, `edit`) : `409 trashed` ; au-dessus de son niveau, `403`.
    let tom = &team.tom;
    let refused = [
        tom.put_mmd(&id, "Autre\n").await,
        tom.patch(&id, json!({ "title": "Autre" })).await,
        tom.patch(&id, json!({ "afterId": null })).await,
        tom.post(json!({ "parentId": id })).await,
        trash(&tom.browser, &id).await,
    ];
    for reply in refused {
        reply.assert_error(StatusCode::CONFLICT, "trashed");
    }
    delete(&tom.browser, &id)
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    // Rien n'a été écrit ; il peut la restaurer.
    let node = team.store.get_node(page).await.unwrap().expect("la page");
    assert_eq!(node.title, "Jetée");
    restore(&tom.browser, &id)
        .await
        .assert_status(StatusCode::OK);
}

/// Supprimer définitivement demande `full`, et que la page soit à la corbeille (elle-même, ou
/// par un ancêtre).
#[tokio::test]
async fn delete_requires_trash_and_full() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (olga, mona, tom) = (&team.olga, &team.mona, &team.tom);
    let project = team.page(None, "Projet", &[members(Level::Edit)]).await;
    let page = team.page(Some(project), "Page", &[]).await;

    // Pas à la corbeille : `403` sous le niveau, sinon `409 not_trashed`.
    delete(&tom.browser, page)
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    let body = delete(&olga.browser, page)
        .await
        .assert_error(StatusCode::CONFLICT, "not_trashed");
    assert!(text(&body, "message").contains("corbeille"), "{body}");
    // À la corbeille (`edit` suffit pour l'y mettre) : toujours `403` sous le niveau.
    trash(&tom.browser, page)
        .await
        .assert_status(StatusCode::OK);
    for who in [tom, mona] {
        delete(&who.browser, page)
            .await
            .assert_error(StatusCode::FORBIDDEN, "forbidden");
    }
    assert!(team.store.get_node(page).await.unwrap().is_some());
    delete(&olga.browser, page)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert!(team.store.get_node(page).await.unwrap().is_none());
    assert!(team.store.get_node(project).await.unwrap().is_some());

    // Une sous-page d'une page à la corbeille y est aussi : elle se supprime, seule.
    let parent = team.page(None, "Parent", &[members(Level::Full)]).await;
    let child = team.page(Some(parent), "Enfant", &[]).await;
    team.trash(parent).await;
    let reply = delete(&mona.browser, child).await;
    reply.assert_status(StatusCode::NO_CONTENT);
    assert!(reply.body.is_empty());
    assert!(team.store.get_node(child).await.unwrap().is_none());
    assert_eq!(
        titles(&trash_list(&mona.browser, team.space).await),
        ["Parent"]
    );
}

/// Une page supprimée définitivement l'est avec son sous-arbre : toutes les routes répondent
/// comme pour un id inconnu, il n'en reste rien en base, et ses connexions de synchro sont
/// fermées.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleted_page_is_gone_for_good() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({ "title": "Page" })).await;
    let child = alice
        .create(json!({ "title": "Enfant", "parentId": page.id }))
        .await;
    let kept = alice.create(json!({ "title": "Gardée" })).await;
    for id in [&page.id, &child.id, &kept.id] {
        alice
            .put_mmd(id, "Contenu\n")
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
    let rows = async |id: &str| {
        let mut rows = 0;
        for (table, key) in [
            ("nodes", "id"),
            ("acl", "node_id"),
            ("doc_snapshots", "node_id"),
            ("doc_updates", "node_id"),
        ] {
            let sql = format!("SELECT COUNT(*) FROM {table} WHERE {key} = '{id}'");
            rows += query_i64(&t.db_path(), &sql).await;
        }
        rows
    };
    // Nœud, accès, instantané, journal ; la sous-page n'a pas d'accès propre.
    assert_eq!(
        (
            rows(&page.id).await,
            rows(&child.id).await,
            rows(&kept.id).await
        ),
        (4, 3, 4)
    );
    let url = |id: &str| sync_url(addr, id);
    let live = synced_as(&alice.browser, &url(&child.id)).await;

    trash(&alice.browser, &page.id)
        .await
        .assert_status(StatusCode::OK);
    let live_again = synced_as(&alice.browser, &url(&child.id)).await;
    let reply = delete(&alice.browser, &page.id).await;
    reply.assert_status(StatusCode::NO_CONTENT);
    assert!(reply.body.is_empty());

    // Les connexions ouvertes sur le sous-arbre sont fermées, et ne se rouvrent pas.
    for client in [&live, &live_again] {
        assert!(
            client.wait_until(WAIT, |c| c.is_closed()).await,
            "connexion encore ouverte sur une page supprimée"
        );
    }
    let refused = sync_as(&alice.browser, &url(&child.id))
        .await
        .err()
        .expect("connexion refusée");
    assert!(refused.contains("404"), "{refused}");

    // `404` partout, comme pour un id inconnu, pour la page et pour sa sous-page.
    let unknown = Uuid::now_v7().to_string();
    for id in [&page.id, &child.id] {
        for ((name, request), (_, missing)) in
            page_routes(id).into_iter().zip(page_routes(&unknown))
        {
            let reply = alice.browser.send(request).await;
            reply.assert_error(StatusCode::NOT_FOUND, "not_found");
            let missing = alice.browser.send(missing).await;
            assert_eq!(outcome(&reply), outcome(&missing), "{name}");
        }
        assert_eq!(rows(id).await, 0, "plus rien en base");
    }
    assert_eq!(
        titles(&tree_nodes(&alice.browser, alice.space).await),
        ["Gardée"]
    );
    assert_eq!(
        trash_list(&alice.browser, alice.space).await,
        [] as [Value; 0]
    );
    // Le reste n'a pas bougé.
    assert_eq!(rows(&kept.id).await, 4);
    assert_eq!(alice.get_mmd(&kept.id).await.text(), "Contenu\n");
}

/// Mettre à la corbeille, restaurer et changer de parent ferment les connexions de synchro du
/// sous-arbre de la page (elles se rouvrent en relisant leurs droits) ; renommer et changer de
/// place parmi les mêmes frères n'en ferment aucune, et les autres pages ne sont pas touchées.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trash_restore_and_move_close_live_connections_of_the_subtree() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({ "title": "Page" })).await;
    let child = alice
        .create(json!({ "title": "Enfant", "parentId": page.id }))
        .await;
    let elsewhere = alice.create(json!({ "title": "Ailleurs" })).await;
    let url = |id: &str| sync_url(addr, id);
    // Une connexion sur la page, une sur sa sous-page.
    let connect = async || {
        [
            synced_as(&alice.browser, &url(&page.id)).await,
            synced_as(&alice.browser, &url(&child.id)).await,
        ]
    };
    let closed = async |clients: &[Client; 2], after: &str| {
        for client in clients {
            assert!(
                client.wait_until(WAIT, |c| c.is_closed()).await,
                "connexion encore ouverte après {after}"
            );
        }
    };
    let bystander = synced_as(&alice.browser, &url(&elsewhere.id)).await;

    // Renommer, changer de place : personne n'est fermé.
    let live = connect().await;
    alice
        .patched(&page.id, json!({ "title": "Renommée" }))
        .await;
    alice.patched(&page.id, json!({ "afterId": null })).await;
    alice.patched(&child.id, json!({ "afterId": null })).await;
    for client in &live {
        assert!(still_served(client).await, "fermée par un renommage");
    }

    trash(&alice.browser, &page.id)
        .await
        .assert_status(StatusCode::OK);
    closed(&live, "la mise à la corbeille").await;

    let live = connect().await;
    restore(&alice.browser, &page.id)
        .await
        .assert_status(StatusCode::OK);
    closed(&live, "la restauration").await;

    let live = connect().await;
    alice
        .patched(&page.id, json!({ "parentId": elsewhere.id }))
        .await;
    closed(&live, "le passage sous une autre page").await;

    let live = connect().await;
    alice.patched(&page.id, json!({ "parentId": null })).await;
    closed(&live, "le passage à la racine").await;

    // Une sous-page seule : sa page n'est pas touchée.
    let live = connect().await;
    trash(&alice.browser, &child.id)
        .await
        .assert_status(StatusCode::OK);
    assert!(live[1].wait_until(WAIT, |c| c.is_closed()).await);
    assert!(
        still_served(&live[0]).await,
        "la page n'a pas changé de droits"
    );

    // La page d'à côté n'a jamais été fermée (elle l'aurait été quand « Page » est passée
    // sous elle, si le sous-arbre fermé était celui de la destination).
    assert!(still_served(&bystander).await);
}
