//! API HTTP du serveur : pages, MMD, synchronisation, garde-fous réseau. Les
//! aides (application en mémoire, compte de test, journaux, SQL direct) sont
//! dans `common/mod.rs`.
//!
//! Chaque test de pages se donne un compte (`TestApp::account`) et travaille
//! dans l'espace que son inscription lui a donné, dont il est propriétaire :
//! ce que chacun peut faire selon ses droits est dans `permissions.rs`, la
//! corbeille dans `trash.rs`.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{Logs, TestApp, WAIT, assert_french, exec_sql, handshake, query_i64};
use mementum_schema::SCHEMA_VERSION;
use mementum_server::Info;
use mementum_store::Store;
use mementum_sync::testkit::Client;
use serde_json::json;
use uuid::Uuid;

fn titles(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/mmd/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} : {e}", path.display()))
}

#[tokio::test]
async fn health_and_info() {
    let t = TestApp::new().await;
    assert!(t.cfg.data_dir.is_dir(), "le dossier des données est créé");
    assert!(t.db_path().starts_with(&t.cfg.data_dir));

    let health = t.get("/health").await;
    assert_eq!(health.status, StatusCode::OK);
    assert_eq!(health.text(), "ok");

    let reply = t.get("/api/v1/info").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        reply.json(),
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "schemaVersion": SCHEMA_VERSION,
            // Inscriptions sur invitation par défaut (Ruling R50) ; aucun
            // compte n'existe encore.
            "signup": "invite",
            "setup": true,
        })
    );
    let info: Info = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(info.schema_version, SCHEMA_VERSION);
}

#[tokio::test]
async fn pages_crud() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let tree = alice.tree().await;
    assert!(tree.nodes.is_empty());
    assert_eq!(tree.space_id, alice.space.to_string());
    let uri = format!("/api/v1/spaces/{}/tree", alice.space);
    let raw = alice.browser.get(&uri).await.json();
    assert!(raw.get("spaceId").is_some() && raw.get("nodes").is_some());

    // Créer : sans `afterId`, en fin des frères (Ruling R23).
    let a = alice.create(json!({ "title": "A" })).await;
    assert_eq!(a.kind, "page");
    assert_eq!(a.parent_id, None);
    assert_eq!(a.title, "A");
    assert!(a.updated_at > 0);
    Uuid::parse_str(&a.id).expect("id en UUID");
    let b = alice.create(json!({ "title": "B" })).await;
    assert_eq!(alice.children(None).await, titles(&["A", "B"]));
    // `"afterId": null` : en tête ; un id : après lui.
    let c = alice.create(json!({ "title": "C", "afterId": null })).await;
    assert_eq!(alice.children(None).await, titles(&["C", "A", "B"]));
    let d = alice.create(json!({ "title": "D", "afterId": a.id })).await;
    assert_eq!(alice.children(None).await, titles(&["C", "A", "D", "B"]));
    // La réponse JSON a la forme attendue (camelCase).
    let raw = alice.post(json!({ "title": "brut" })).await.json();
    for key in [
        "id",
        "spaceId",
        "parentId",
        "kind",
        "title",
        "position",
        "level",
        "section",
        "updatedAt",
        "trashedAt",
    ] {
        assert!(raw.get(key).is_some(), "{key} absent de {raw}");
    }

    // Le document est créé avec le nœud : une page neuve se lit en MMD.
    let mmd = alice.get_mmd(&a.id).await;
    assert_eq!(mmd.status, StatusCode::OK);

    // Renommer : la place ne change pas.
    let a2 = alice.patched(&a.id, json!({ "title": "A2" })).await;
    assert_eq!(a2.title, "A2");
    assert_eq!(a2.position, a.position);
    assert_eq!(a2.parent_id, None);
    assert!(a2.updated_at >= a.updated_at);
    assert_eq!(alice.node(&a.id).await.title, "A2");

    // `parentId` seul : en fin des enfants du nouveau parent.
    let e = alice
        .create(json!({ "title": "E", "parentId": a.id }))
        .await;
    assert_eq!(e.parent_id.as_deref(), Some(a.id.as_str()));
    let moved = alice.patched(&b.id, json!({ "parentId": a.id })).await;
    assert_eq!(moved.parent_id.as_deref(), Some(a.id.as_str()));
    assert_eq!(alice.children(Some(&a.id)).await, titles(&["E", "B"]));
    assert_eq!(
        alice.children(None).await,
        titles(&["C", "A2", "D", "brut"])
    );

    // `afterId` seul : réordonnancement chez le parent actuel.
    alice.patched(&b.id, json!({ "afterId": null })).await;
    assert_eq!(alice.children(Some(&a.id)).await, titles(&["B", "E"]));
    alice.patched(&b.id, json!({ "afterId": e.id })).await;
    assert_eq!(alice.children(Some(&a.id)).await, titles(&["E", "B"]));

    // `"parentId": null` : à la racine, en fin.
    let root = alice.patched(&e.id, json!({ "parentId": null })).await;
    assert_eq!(root.parent_id, None);
    assert_eq!(
        alice.children(None).await,
        titles(&["C", "A2", "D", "brut", "E"])
    );
    assert_eq!(alice.children(Some(&a.id)).await, titles(&["B"]));

    // Les deux : on suit les deux.
    alice
        .patched(&e.id, json!({ "parentId": a.id, "afterId": null }))
        .await;
    assert_eq!(alice.children(Some(&a.id)).await, titles(&["E", "B"]));
    alice
        .patched(&c.id, json!({ "parentId": a.id, "afterId": e.id }))
        .await;
    assert_eq!(alice.children(Some(&a.id)).await, titles(&["E", "C", "B"]));

    // Ni `parentId` ni `afterId` : pas de déplacement.
    let before = alice.node(&e.id).await;
    let same = alice.patched(&e.id, json!({})).await;
    assert_eq!(
        (&same.parent_id, &same.position),
        (&before.parent_id, &before.position)
    );
    let renamed = alice.patched(&e.id, json!({ "title": "E2" })).await;
    assert_eq!(
        (&renamed.parent_id, &renamed.position),
        (&before.parent_id, &before.position)
    );

    // Un `afterId` qui n'est pas un frère : 422, rien ne bouge.
    let snapshot = alice.tree().await.nodes;
    alice
        .patch(&e.id, json!({ "afterId": d.id }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_after");
    alice
        .patch(&e.id, json!({ "parentId": null, "afterId": b.id }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_after");
    alice
        .patch(&e.id, json!({ "afterId": "pas-un-uuid" }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_after");
    alice
        .post(json!({ "title": "X", "afterId": e.id }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_after");
    // Un parent inconnu : 422 aussi.
    let unknown = Uuid::now_v7().to_string();
    alice
        .post(json!({ "title": "X", "parentId": unknown }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_parent");
    alice
        .patch(&e.id, json!({ "parentId": unknown }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_parent");
    assert_eq!(alice.tree().await.nodes, snapshot, "aucun refus n'écrit");

    // Une page inconnue : 404.
    alice
        .patch(&unknown, json!({ "title": "X" }))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    alice
        .patch("pas-un-uuid", json!({ "title": "X" }))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(alice.tree().await.nodes, snapshot);
}

#[tokio::test]
async fn move_cycle_is_409() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let a = alice.create(json!({ "title": "A" })).await;
    let b = alice
        .create(json!({ "title": "B", "parentId": a.id }))
        .await;
    let c = alice
        .create(json!({ "title": "C", "parentId": b.id }))
        .await;
    let before = alice.tree().await.nodes;

    // Sous sa petite-fille, sous sa fille, sous elle-même : refusé.
    for parent in [&c.id, &b.id, &a.id] {
        let body = alice
            .patch(&a.id, json!({ "parentId": parent }))
            .await
            .assert_error(StatusCode::CONFLICT, "cycle");
        assert!(body.get("line").is_none());
    }
    // Le refus n'écrit rien, pas même le titre envoyé avec.
    alice
        .patch(&a.id, json!({ "title": "Renommée", "parentId": c.id }))
        .await
        .assert_error(StatusCode::CONFLICT, "cycle");
    assert_eq!(alice.tree().await.nodes, before);

    // Le déplacement inverse est permis.
    let moved = alice.patched(&c.id, json!({ "parentId": null })).await;
    assert_eq!(moved.parent_id, None);
}

#[tokio::test]
async fn mmd_get_put() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Spécification" })).await;
    let mmd = fixture("p0-all-blocks.mmd");

    let put = alice.put_mmd(&page.id, mmd.clone()).await;
    assert_eq!(put.status, StatusCode::NO_CONTENT, "{}", put.text());
    assert!(put.body.is_empty());

    let got = alice.get_mmd(&page.id).await;
    assert_eq!(got.status, StatusCode::OK);
    assert_eq!(
        got.content_type.as_deref(),
        Some("text/markdown; charset=utf-8")
    );
    assert_eq!(got.text(), mmd, "le MMD relu est exactement celui écrit");

    // Une seconde écriture remplace la première.
    let put = alice.put_mmd(&page.id, "Bonjour Café 🚀\n").await;
    assert_eq!(put.status, StatusCode::NO_CONTENT);
    assert_eq!(alice.get_mmd(&page.id).await.text(), "Bonjour Café 🚀\n");
}

#[tokio::test]
async fn invalid_mmd_is_422() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({})).await;
    assert_eq!(
        alice.put_mmd(&page.id, "Avant\n").await.status,
        StatusCode::NO_CONTENT
    );

    let body = alice
        .put_mmd(&page.id, "* x\n")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_mmd");
    assert_eq!(body["line"], 1, "{body}");

    let body = alice
        .put_mmd(&page.id, "Un\n\n## Deux\n\n##### Trop profond\n")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_mmd");
    assert_eq!(body["line"], 5, "{body}");

    // Un corps qui n'est pas de l'UTF-8 : la ligne de l'octet fautif.
    let body = alice
        .put_mmd(&page.id, Body::from(b"Un\n\nDeux \xff\n".to_vec()))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_mmd");
    assert_eq!(body["line"], 3, "{body}");

    // Rien n'a été écrit.
    assert_eq!(alice.get_mmd(&page.id).await.text(), "Avant\n");
}

/// Ruling R33 : un bloc étranger écrit par MMD (`foreign=true`) devient le bloc
/// custom `unknown` de BlockNote, `originalType` et `data` gardés. Recréé en
/// élément étranger, y-prosemirror le supprimerait chez tous dès qu'un
/// navigateur ouvre la page.
#[tokio::test]
async fn foreign_mmd_block_is_written_as_unknown() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({})).await;
    let data = "eyJhdHRycyI6eyJhIjoiMSJ9LCJjb250ZW50IjpbXSwiY2hpbGRyZW4iOltdfQ==";
    let block = |id: &str, foreign: &str| {
        format!("<block type=\"futureWidget\" id=\"{id}\" data=\"{data}\"{foreign}/>")
    };
    let put = alice
        .put_mmd(
            &page.id,
            format!(
                "Avant\n\nParent\n  {}\n\n{}\n\nAprès\n",
                block("b_future02", " foreign=true"),
                block("b_future01", " foreign=true"),
            ),
        )
        .await;
    assert_eq!(put.status, StatusCode::NO_CONTENT, "{}", put.text());
    // Relu sans `foreign`, au premier niveau comme en enfant.
    assert_eq!(
        alice.get_mmd(&page.id).await.text(),
        format!(
            "Avant\n\nParent\n  {}\n\n{}\n\nAprès\n",
            block("b_future02", ""),
            block("b_future01", ""),
        )
    );
}

/// Avant R33, ce bloc étranger de 60 niveaux sous 78 blocs imbriqués dépassait
/// `MAX_YJS_DEPTH` (`422 invalid_blocks`). Devenu un bloc `unknown`, son `data`
/// reste opaque : il est écrit tel quel. `PUT /mmd` ne produit plus
/// `invalid_blocks` pour une structure que BlockNote refuserait (la lecture
/// MMD borne l'imbrication et refuse les colonnes invalides) ; la traduction
/// de `TooDeep` et de `Structure` en `invalid_blocks` reste testée dans
/// `error.rs`. Ce code sert aussi au refus d'une prop hors registre (R41,
/// `unregistered_prop_is_422`).
#[tokio::test]
async fn deep_foreign_mmd_block_stays_opaque() {
    use base64::Engine;
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({})).await;
    let mut node = json!({ "tag": "inner" });
    for _ in 1..60 {
        node = json!({ "tag": "inner", "content": [node] });
    }
    let data =
        base64::engine::general_purpose::STANDARD.encode(json!({ "content": [node] }).to_string());
    let mut mmd = String::new();
    for level in 0..78 {
        mmd.push_str(&format!(
            "{}Niveau {level}
",
            "  ".repeat(level)
        ));
    }
    mmd.push_str(&format!(
        "{}<block type=\"futureWidget\" id=\"f\" data=\"{data}\" foreign=true/>
",
        "  ".repeat(78)
    ));
    mementum_md::parse(&mmd).expect("MMD lisible");
    let put = alice.put_mmd(&page.id, mmd.clone()).await;
    assert_eq!(put.status, StatusCode::NO_CONTENT, "{}", put.text());
    assert_eq!(
        alice.get_mmd(&page.id).await.text(),
        mmd.replace(" foreign=true", "")
    );
}

/// Ruling R41 : `PUT /mmd` refuse une prop hors registre, sur un bloc comme
/// sur une cellule de tableau. Écrite en attribut Yjs, y-prosemirror la
/// retirerait du document partagé dès qu'un navigateur modifie le bloc, sans
/// bruit. Le message nomme la prop et le type du bloc.
#[tokio::test]
async fn unregistered_prop_is_422() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({})).await;
    assert_eq!(
        alice.put_mmd(&page.id, "Avant\n").await.status,
        StatusCode::NO_CONTENT
    );
    let refused = async |mmd: &str, expected: &[&str]| {
        assert!(mementum_md::parse(mmd).is_ok(), "MMD lisible : {mmd:?}");
        let body = alice
            .put_mmd(&page.id, mmd.to_string())
            .await
            .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_blocks");
        assert_french(&body, expected);
        assert!(body.get("line").is_none(), "{body}");
    };

    // Sur un bloc, au premier niveau comme en enfant.
    refused("Texte {foo=1}\n", &["« foo »", "« paragraph »", "registre"]).await;
    refused(
        "Parent\n  - Enfant {bar=\"x\"}\n",
        &["« bar »", "« bulletListItem »"],
    )
    .await;
    // Une prop du registre, mais d'un autre type de bloc.
    refused("Texte {toggle=true}\n", &["« toggle »", "« paragraph »"]).await;
    // Sur une balise et sur un bloc `unknown`.
    refused("<toc foo=1/>\n", &["« foo »", "« toc »"]).await;
    refused(
        "<block type=\"futureWidget\" id=\"b1\" data=\"e30=\" foo=1/>\n",
        &["« foo »", "« unknown »"],
    )
    .await;
    // Sur une cellule de tableau, et dans la liste `{…}` du tableau.
    refused(
        "| a | B {foo=1} |\n| --- | --- |\n| c | d |\n",
        &["« foo »", "cellule", "« table »"],
    )
    .await;
    refused(
        "| a | b |\n| --- | --- |\n| c | d |\n{foo=1}\n",
        &["« foo »", "« table »"],
    )
    .await;
    // Dans un tableau enfant d'un autre bloc.
    refused(
        "Parent\n  | a {bar=true} |\n  | --- |\n",
        &["« bar »", "cellule"],
    )
    .await;

    // Rien n'a été écrit.
    assert_eq!(alice.get_mmd(&page.id).await.text(), "Avant\n");

    // Les props du registre passent toujours, bloc et cellule.
    let valid = concat!(
        "Texte {align=\"center\" bg=\"yellow\"}\n\n",
        "## Titre {toggle=true}\n\n",
        "| A {bg=\"yellow\" colspan=2} |\n| --- |\n| c {align=\"right\"} | d |\n",
        "{color=\"blue\" header-rows=0 widths=\"120,\"}\n",
    );
    let put = alice.put_mmd(&page.id, valid).await;
    assert_eq!(put.status, StatusCode::NO_CONTENT, "{}", put.text());
    assert_eq!(alice.get_mmd(&page.id).await.text(), valid);
}

/// Ruling R41 : aucun MMD légitime n'est refusé. Toutes les fixtures passent,
/// sauf `p1-edge-cases.mmd`, refusée pour sa seule ligne qui porte exprès
/// deux props inconnues (le format MMD, lui, les lit et les rend toujours).
#[tokio::test]
async fn fixtures_pass_except_the_unregistered_prop_line() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({})).await;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/mmd/fixtures");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".mmd"))
        .collect();
    names.sort();
    assert!(names.len() >= 4, "fixtures MMD attendues : {names:?}");
    let bag = "Texte {bar=\"x\" foo=1}\n\n";
    for name in &names {
        let mmd = fixture(name);
        let put = alice.put_mmd(&page.id, mmd.clone()).await;
        if name != "p1-edge-cases.mmd" {
            assert!(!mmd.contains(bag), "{name}");
            assert_eq!(
                put.status,
                StatusCode::NO_CONTENT,
                "{name} : {}",
                put.text()
            );
            // Relue telle quelle, sauf `foreign` (R33).
            assert_eq!(
                alice.get_mmd(&page.id).await.text(),
                mmd.replace(" foreign=true/>", "/>"),
                "{name}"
            );
            continue;
        }
        let body = put.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_blocks");
        assert_french(&body, &["« bar »", "« paragraph »"]);
        // Sans cette ligne, la fixture passe.
        assert_eq!(mmd.matches(bag).count(), 1, "{name}");
        let rest = mmd.replace(bag, "");
        let put = alice.put_mmd(&page.id, rest.clone()).await;
        assert_eq!(
            put.status,
            StatusCode::NO_CONTENT,
            "{name} : {}",
            put.text()
        );
        assert_eq!(
            alice.get_mmd(&page.id).await.text(),
            rest.replace(" foreign=true/>", "/>"),
            "{name}"
        );
    }
}

/// Ruling R41 : le format MMD ne change pas. Une prop inconnue déjà présente
/// dans Yjs (écrite par un client) est rendue telle quelle par `GET /mmd` ;
/// seule l'écriture par `PUT /mmd` la refuse, même pour ce texte relu.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unregistered_prop_in_yjs_is_still_read() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({})).await;
    let url = format!("ws://{addr}/sync/{}?schema={SCHEMA_VERSION}", page.id);
    let client = Client::connect_synced(&url).await;

    let mmd = "Texte {bar=\"x\" foo=1}\n\n| a | B {baz=true} |\n| --- | --- |\n";
    let blocks = mementum_md::parse(mmd).expect("MMD lisible");
    let update = client
        .page()
        .expect("document local lisible")
        .write_blocks(&blocks)
        .expect("écriture locale");
    let start = client.step2_count();
    client.apply_local(&update);
    client.resync();
    assert!(client.wait_until(WAIT, |c| c.step2_count() > start).await);

    assert_eq!(alice.get_mmd(&page.id).await.text(), mmd);
    alice
        .put_mmd(&page.id, mmd)
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_blocks");
    assert_eq!(alice.get_mmd(&page.id).await.text(), mmd);
}

#[tokio::test]
async fn unknown_page_is_404_and_not_created() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let id = Uuid::now_v7();

    alice
        .get_mmd(&id.to_string())
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    alice
        .put_mmd(&id.to_string(), "Texte\n")
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    alice
        .get_mmd("pas-un-uuid")
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");

    // `/sync` : 404 avant toute poignée de main WebSocket.
    t.get(&format!("/sync/{id}?schema={SCHEMA_VERSION}"))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    let url = format!("ws://{addr}/sync/{id}?schema={SCHEMA_VERSION}");
    let refused = Client::connect(&url)
        .await
        .err()
        .expect("connexion refusée");
    assert!(refused.contains("404"), "{refused}");
    let refused = Client::connect(&format!("ws://{addr}/sync/pas-un-uuid?schema=1"))
        .await
        .err()
        .expect("connexion refusée");
    assert!(refused.contains("404"), "{refused}");

    // Rien n'a été créé : ni nœud, ni document.
    assert!(alice.tree().await.nodes.is_empty());
    let store = Store::open(&t.db_path()).await.unwrap();
    assert!(store.get_node(id).await.unwrap().is_none());
    let doc = store.load_doc(id).await.unwrap();
    assert!(doc.snapshot.is_none() && doc.updates.is_empty());
    store.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_route_serves_existing_page() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({ "title": "En direct" })).await;
    let url = format!("ws://{addr}/sync/{}?schema={SCHEMA_VERSION}", page.id);
    let client = Client::connect_synced(&url).await;
    assert_eq!(client.blocks().len(), 1, "la racine écrite par le serveur");
    // Sans poignée de main WebSocket : une erreur JSON, pas un 404.
    t.get(&format!("/sync/{}?schema={SCHEMA_VERSION}", page.id))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_request");

    // Une écriture MMD est diffusée aux connexions ouvertes.
    assert_eq!(
        alice.put_mmd(&page.id, "Bonjour\n").await.status,
        StatusCode::NO_CONTENT
    );
    assert!(
        client
            .wait_until(WAIT, |c| {
                c.blocks().first().map(mementum_md::block_text).as_deref() == Some("Bonjour")
            })
            .await,
        "le client n'a pas reçu l'écriture"
    );

    // Une frappe du client se relit en MMD.
    // L'étape 2 qui répond à `resync` vient après le traitement de la frappe.
    let block = client.blocks()[0].id.clone();
    let start = client.step2_count();
    client.type_text(&block, " à tous");
    client.resync();
    assert!(client.wait_until(WAIT, |c| c.step2_count() > start).await);
    assert_eq!(alice.get_mmd(&page.id).await.text(), "Bonjour à tous\n");
}

#[tokio::test]
async fn title_rules() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    // Titre absent, nul ou blanc : « Sans titre ».
    assert_eq!(alice.create(json!({})).await.title, "Sans titre");
    assert_eq!(
        alice.create(json!({ "title": null })).await.title,
        "Sans titre"
    );
    assert_eq!(
        alice.create(json!({ "title": " \t\n " })).await.title,
        "Sans titre"
    );
    // Espaces de bord retirées.
    assert_eq!(
        alice.create(json!({ "title": "  Bonjour  " })).await.title,
        "Bonjour"
    );
    // 500 caractères (et non 500 octets ni 500 unités UTF-16) : accepté.
    let max = "é".repeat(500);
    assert_eq!(alice.create(json!({ "title": max })).await.title, max);
    assert_eq!(
        alice
            .create(json!({ "title": format!("  {max}  ") }))
            .await
            .title,
        max
    );
    let rockets = "🚀".repeat(500);
    assert_eq!(
        alice.create(json!({ "title": rockets })).await.title,
        rockets
    );

    // 501 caractères : refusé, rien n'est créé.
    let count = alice.tree().await.nodes.len();
    alice
        .post(json!({ "title": "é".repeat(501) }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "title_too_long");
    assert_eq!(alice.tree().await.nodes.len(), count);

    // Les mêmes règles au renommage.
    let page = alice.create(json!({ "title": "Page" })).await;
    let other = alice.create(json!({ "title": "Autre" })).await;
    assert_eq!(
        alice
            .patched(&page.id, json!({ "title": "   " }))
            .await
            .title,
        "Sans titre"
    );
    assert_eq!(
        alice
            .patched(&page.id, json!({ "title": " Nouveau " }))
            .await
            .title,
        "Nouveau"
    );
    let before = alice.node(&page.id).await;
    alice
        .patch(&page.id, json!({ "title": "a".repeat(501) }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "title_too_long");
    // Refusé avant tout déplacement demandé avec.
    alice
        .patch(
            &page.id,
            json!({ "title": "a".repeat(501), "parentId": other.id }),
        )
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "title_too_long");
    assert_eq!(alice.node(&page.id).await, before);
    // `"title": null` au renommage : le titre ne change pas.
    assert_eq!(
        alice
            .patched(&page.id, json!({ "title": null }))
            .await
            .title,
        "Nouveau"
    );
}

#[tokio::test]
async fn errors_are_json() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let post_raw = |body: &'static str| {
        Request::post("/api/v1/pages")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap()
    };
    // JSON illisible : la ligne et la colonne.
    let body = alice
        .browser
        .send(post_raw("{\n  \"title\": "))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_json");
    assert_french(&body, &["JSON illisible", "ligne 2"]);
    let body = alice
        .browser
        .send(post_raw("{} {}"))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_json");
    assert_french(&body, &["JSON illisible"]);
    // Champ inconnu, nommé.
    let body = alice
        .post(json!({ "parentID": null }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    assert_french(&body, &["champ inconnu", "parentID"]);
    // Type invalide, avec le champ.
    let body = alice
        .post(json!({ "title": 3 }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    assert_french(&body, &["type invalide", "title"]);
    let body = alice
        .patch(&Uuid::now_v7().to_string(), json!({}))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    assert_french(&body, &["page introuvable"]);
    // Un corps qui n'est pas un objet. (Un tableau, lui, passe : serde lit
    // une structure depuis une séquence, champs par défaut.)
    let body = alice
        .browser
        .send(post_raw("3"))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    assert_french(&body, &["objet JSON"]);
    // Type de contenu absent.
    let body = alice
        .browser
        .send(
            Request::post("/api/v1/pages")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .assert_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid_json");
    assert_french(&body, &["Content-Type"]);
    // Route et méthode inconnues ; l'arbre sans espace du lot 1 n'est plus une route.
    for uri in ["/api/v1/inconnue", "/api/v1/tree"] {
        let body = alice
            .browser
            .get(uri)
            .await
            .assert_error(StatusCode::NOT_FOUND, "not_found");
        assert_french(&body, &["route inconnue"]);
    }
    let tree = format!("/api/v1/spaces/{}/tree", alice.space);
    alice
        .browser
        .send(Request::delete(&tree).body(Body::empty()).unwrap())
        .await
        .assert_error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    // Sans session : `401`, avant toute lecture du corps.
    for body in ["{\n  \"title\": ", "3", "{}"] {
        t.send(post_raw(body))
            .await
            .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
    assert!(alice.tree().await.nodes.is_empty());
}

#[tokio::test]
async fn payload_too_large_is_413() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({})).await;
    let before = alice.get_mmd(&page.id).await.text();
    // Plus de 2 Mio : refusé avant toute lecture du MMD.
    let mmd = "a".repeat(mementum_server::MAX_BODY_BYTES + 1);
    let body = alice
        .put_mmd(&page.id, mmd)
        .await
        .assert_error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");
    assert_french(&body, &["trop gros", "2097152"]);
    let big = json!({ "title": "a".repeat(mementum_server::MAX_BODY_BYTES) });
    alice
        .post(big)
        .await
        .assert_error(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large");
    assert_eq!(alice.get_mmd(&page.id).await.text(), before);
    assert_eq!(alice.tree().await.nodes.len(), 1);
}

/// Ruling R23 : les combinaisons de `parentId` et `afterId`.
#[tokio::test]
async fn positions_combinations() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let r1 = alice.create(json!({ "title": "R1" })).await;
    // `POST` avec `"parentId": null` : à la racine, en fin.
    let r2 = alice
        .create(json!({ "title": "R2", "parentId": null }))
        .await;
    assert_eq!(r2.parent_id, None);
    assert_eq!(alice.children(None).await, titles(&["R1", "R2"]));

    // Sous un parent qui a déjà des enfants, sans `afterId` : en fin.
    let c1 = alice
        .create(json!({ "title": "C1", "parentId": r1.id }))
        .await;
    alice
        .create(json!({ "title": "C2", "parentId": r1.id }))
        .await;
    assert_eq!(alice.children(Some(&r1.id)).await, titles(&["C1", "C2"]));
    // `parentId` avec `"afterId": null` : en tête du parent.
    alice
        .create(json!({ "title": "C0", "parentId": r1.id, "afterId": null }))
        .await;
    assert_eq!(
        alice.children(Some(&r1.id)).await,
        titles(&["C0", "C1", "C2"])
    );
    // `parentId` avec un id comme `afterId` : après lui.
    alice
        .create(json!({ "title": "C1b", "parentId": r1.id, "afterId": c1.id }))
        .await;
    assert_eq!(
        alice.children(Some(&r1.id)).await,
        titles(&["C0", "C1", "C1b", "C2"])
    );

    // `PATCH { "parentId": null, "afterId": null }` : à la racine, en tête.
    let moved = alice
        .patched(&c1.id, json!({ "parentId": null, "afterId": null }))
        .await;
    assert_eq!(moved.parent_id, None);
    assert_eq!(alice.children(None).await, titles(&["C1", "R1", "R2"]));
    // `PATCH { "parentId": null, "afterId": <frère à la racine> }`.
    alice
        .patched(&c1.id, json!({ "parentId": null, "afterId": r1.id }))
        .await;
    assert_eq!(alice.children(None).await, titles(&["R1", "C1", "R2"]));
    assert_eq!(
        alice.children(Some(&r1.id)).await,
        titles(&["C0", "C1b", "C2"])
    );
    // Depuis un sous-niveau, après un frère de la racine.
    let c2 = alice.node_titled("C2").await;
    alice
        .patched(&c2.id, json!({ "parentId": null, "afterId": r2.id }))
        .await;
    assert_eq!(
        alice.children(None).await,
        titles(&["R1", "C1", "R2", "C2"])
    );
}

/// Le nœud, ses accès et son document sont écrits en une transaction : si le
/// document ne peut pas l'être, rien n'est créé (`500`), et le serveur le
/// journalise. (Au lot 1, la page était créée sans document, Ruling R32 ;
/// l'acteur d'une page restée sans document lui écrit toujours le sien à son
/// premier chargement, R28.)
#[tokio::test]
async fn page_doc_failure_creates_nothing() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    exec_sql(
        &t.db_path(),
        "CREATE TRIGGER panne BEFORE INSERT ON doc_snapshots \
         BEGIN SELECT RAISE(ABORT, 'panne simulée'); END",
    )
    .await;
    let logs = Logs::capture();
    for body in [
        json!({ "title": "Sans document" }),
        json!({ "title": "Sans document", "section": "space" }),
    ] {
        let body = alice
            .post(body)
            .await
            .assert_error(StatusCode::INTERNAL_SERVER_ERROR, "internal");
        // Le détail va au journal, pas dans la réponse.
        assert_eq!(body["message"], "erreur interne du serveur");
    }
    let text = logs.text();
    assert!(
        text.contains("ERROR") && text.contains("panne simulée"),
        "erreur attendue au journal : {text}"
    );
    drop(logs);
    for table in ["nodes", "acl", "doc_snapshots"] {
        let rows = query_i64(&t.db_path(), &format!("SELECT COUNT(*) FROM {table}")).await;
        assert_eq!(rows, 0, "{table}");
    }
    assert!(alice.tree().await.nodes.is_empty());

    // La panne passée, la page se crée, avec la racine écrite par le serveur.
    exec_sql(&t.db_path(), "DROP TRIGGER panne").await;
    let page = alice.create(json!({ "title": "Avec document" })).await;
    let got = alice.get_mmd(&page.id).await;
    assert_eq!(got.status, StatusCode::OK, "{}", got.text());
    assert_eq!(got.text(), "<p/>\n");
}

/// Ruling R31 : sur la boucle locale, l'en-tête `Host` doit nommer la boucle
/// locale (DNS rebinding).
#[tokio::test]
async fn host_is_checked_on_loopback() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let request = |uri: &str, host: Option<&str>| {
        let mut builder = Request::get(uri);
        if let Some(host) = host {
            builder = builder.header(header::HOST, host);
        }
        builder.body(Body::empty()).unwrap()
    };
    for host in [
        "localhost",
        "localhost:5173",
        "LocalHost:8080",
        "127.0.0.1",
        "127.0.0.1:8080",
        "[::1]",
        "[::1]:8080",
    ] {
        let reply = t.send_raw(request("/health", Some(host))).await;
        assert_eq!(reply.status, StatusCode::OK, "Host: {host}");
    }
    for host in [
        "evil.example",
        "evil.example:8080",
        "localhost.evil.example",
        "127.0.0.1.evil.example:8080",
        "127.0.0.2",
        "[::2]:8080",
        "::1",
        "localhost:http",
        "",
    ] {
        for uri in ["/health", "/api/v1/me", "/api/v1/inconnue"] {
            let body = t
                .send_raw(request(uri, Some(host)))
                .await
                .assert_error(StatusCode::MISDIRECTED_REQUEST, "bad_host");
            assert_french(&body, &["Host"]);
        }
    }
    // Sans en-tête `Host` : refusé aussi.
    t.send_raw(request("/health", None))
        .await
        .assert_error(StatusCode::MISDIRECTED_REQUEST, "bad_host");
    // Une écriture refusée n'écrit rien, même avec une session.
    let body = json!({ "spaceId": alice.space.to_string() });
    alice
        .browser
        .send(
            Request::post("/api/v1/pages")
                .header(header::HOST, "evil.example")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .assert_error(StatusCode::MISDIRECTED_REQUEST, "bad_host");
    assert!(alice.tree().await.nodes.is_empty());
}

/// Ruling R31 : hors de la boucle locale, le contrôle de `Host` est désactivé
/// et le démarrage le journalise : ce sont les comptes qui protègent les
/// pages (R51), dont aucune route ne répond sans session ; tant que `/sync`
/// ne la vérifie pas, il le dit aussi. L'`Origin` de `/sync` doit alors être
/// locale ou celle du serveur lui-même.
#[tokio::test]
async fn non_loopback_bind_warns_and_skips_host_check() {
    let logs = Logs::capture();
    let t = TestApp::with_bind("0.0.0.0:0").await;
    let text = logs.text();
    let warning = text
        .lines()
        .find(|line| line.contains("boucle locale"))
        .unwrap_or_else(|| panic!("avertissement attendu : {text}"));
    for part in [
        "WARN",
        "Host",
        "comptes qui protègent les pages",
        "sans session",
        "/sync",
        "pas encore",
    ] {
        assert!(warning.contains(part), "« {part} » absent de : {warning}");
    }
    // Sur la boucle locale, rien de tel.
    drop(logs);
    let logs = Logs::capture();
    TestApp::new().await;
    assert!(!logs.text().contains("boucle locale"), "{}", logs.text());
    drop(logs);
    let reply = t
        .send_raw(
            Request::get("/health")
                .header(header::HOST, "192.168.1.10:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);

    let alice = t.account("Alice").await;
    let page = alice.create(json!({})).await;
    let sync = |origin: &str| {
        Request::get(format!("/sync/{}?schema={SCHEMA_VERSION}", page.id))
            .header(header::HOST, "192.168.1.10:8080")
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .unwrap()
    };
    // Même origine que le serveur : l'Origin passe (puis, sans poignée de
    // main en mémoire, la requête n'est pas une WebSocket).
    t.send_raw(sync("http://192.168.1.10:8080"))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_request");
    t.send_raw(sync("http://localhost:5173"))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_request");
    t.send_raw(sync("http://evil.example"))
        .await
        .assert_error(StatusCode::FORBIDDEN, "bad_origin");
}

/// Ruling R67 : quand le démarrage migre une base, le store en écrit d'abord
/// une copie, et c'est le serveur qui le journalise, avec son chemin et sa
/// taille.
#[tokio::test]
async fn startup_logs_the_backup_made_before_migrating() {
    let logs = Logs::capture();
    let (t, lot1) = TestApp::on_lot1_database().await;
    let text = logs.text();
    drop(logs);
    let backup = t.cfg.data_dir.join("mementum.db.avant-0002.bak");
    assert!(backup.is_file(), "copie attendue : {}", backup.display());
    let bytes = std::fs::metadata(&backup).unwrap().len();
    let line = text
        .lines()
        .find(|line| line.contains("mementum.db.avant-0002.bak"))
        .unwrap_or_else(|| panic!("la copie n'est pas nommée dans le journal : {text}"));
    assert!(line.contains("INFO"), "{line}");
    assert!(line.contains("migrée"), "{line}");
    assert!(line.contains(&format!("bytes={bytes}")), "{line}");
    assert!(line.contains("kept=false"), "{line}");
    // La base migrée sert bien les pages du lot 1, à son premier compte.
    let alice = t.account("Alice").await;
    assert_eq!(alice.space, lot1.vault);
    let mut pages: Vec<String> = alice.tree().await.nodes.into_iter().map(|n| n.id).collect();
    pages.sort();
    let mut expected = [lot1.root.to_string(), lot1.child.to_string()];
    expected.sort();
    assert_eq!(pages, expected);

    // Au démarrage suivant, plus rien à migrer : rien à dire.
    let logs = Logs::capture();
    let _router = mementum_server::app(&t.cfg).await.unwrap();
    assert!(!logs.text().contains(".bak"), "{}", logs.text());
}

/// Ruling R31 : une `Origin` présente doit nommer la boucle locale ; sans
/// `Origin` (client hors navigateur), la connexion passe.
#[tokio::test]
async fn sync_origin_is_checked() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({})).await;
    let url = format!("ws://{addr}/sync/{}?schema={SCHEMA_VERSION}", page.id);

    assert_eq!(handshake(&url, None).await, Ok(()));
    assert_eq!(handshake(&url, Some("http://localhost:5174")).await, Ok(()));
    assert_eq!(handshake(&url, Some("http://127.0.0.1:5173")).await, Ok(()));
    assert_eq!(handshake(&url, Some("https://[::1]")).await, Ok(()));
    for origin in [
        "http://evil.example",
        "http://localhost.evil.example:5173",
        "null",
        "file://",
    ] {
        assert_eq!(handshake(&url, Some(origin)).await, Err(403), "{origin}");
    }
    let client = Client::connect_synced(&url).await;
    assert_eq!(client.blocks().len(), 1);

    // La réponse 403 est l'erreur JSON habituelle.
    let body = t
        .send(
            Request::get(format!("/sync/{}?schema={SCHEMA_VERSION}", page.id))
                .header(header::ORIGIN, "http://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .assert_error(StatusCode::FORBIDDEN, "bad_origin");
    assert_french(&body, &["Origin"]);
}

/// Un client qui ne donne pas (ou mal) son schéma est en lecture seule, et le
/// serveur le journalise.
#[tokio::test]
async fn sync_without_schema_is_logged_read_only() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let page = alice.create(json!({})).await;
    for query in ["", "?schema=abc"] {
        let logs = Logs::capture();
        let url = format!("ws://{addr}/sync/{}{query}", page.id);
        let client = Client::connect_synced(&url).await;
        let text = logs.text();
        assert!(
            text.contains("lecture seule") && text.contains(&page.id),
            "journal attendu pour « {query} » : {text}"
        );
        drop(client);
    }
    // Avec le bon schéma, rien de tel.
    let logs = Logs::capture();
    let client = Client::connect_synced(&format!(
        "ws://{addr}/sync/{}?schema={SCHEMA_VERSION}",
        page.id
    ))
    .await;
    assert!(!logs.text().contains("lecture seule"), "{}", logs.text());
    drop(client);
}

#[test]
fn bindings_are_committed() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/protocol/src/generated");
    let expected: BTreeMap<String, String> =
        mementum_server::types::bindings().into_iter().collect();
    if std::env::var("UPDATE_BINDINGS").as_deref() == Ok("1") {
        std::fs::create_dir_all(&dir).unwrap();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !expected.contains_key(&name) {
                std::fs::remove_file(&path).unwrap();
            }
        }
        for (name, content) in &expected {
            std::fs::write(dir.join(name), content).unwrap();
        }
    }
    let committed: BTreeMap<String, String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| {
            panic!(
                "{} illisible ({e}) : lancer UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed",
                dir.display()
            )
        })
        .map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            (name, std::fs::read_to_string(&path).unwrap())
        })
        .collect();
    assert_eq!(
        committed, expected,
        "packages/protocol/src/generated périmé : lancer UPDATE_BINDINGS=1 cargo test -p mementum-server bindings_are_committed"
    );
    for content in expected.values() {
        assert!(!content.contains('\r'), "fichiers générés en LF");
    }
}
