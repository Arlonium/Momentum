//! Dupliquer une page, par l'API (Ruling R252, Task S6) : ce qui est copié (le contenu, sous des
//! identifiants de blocs neufs ; les sous-pages que la personne voit, hors corbeille ; les
//! fichiers que les blocs montrent), ce qui ne l'est pas (commentaires, versions, favoris, accès
//! nommés), où va la copie et qui la voit, qui peut dupliquer, et les bornes.
//!
//! Le droit qui change pendant la construction des documents est éprouvé dans les tests unitaires
//! de `routes/duplicate.rs`, qui tiennent la file des calculs ; la route sur une page qu'on ne
//! voit pas, dans `foreign_page_is_indistinguishable_from_missing` (`tests/permissions.rs`).

mod common;

use std::collections::HashSet;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::sync::{members, open_app, user};
use common::{Account, Browser, Reply, Team, TestApp, query_i64, upload_uri};
use mementum_acl::{Level, Subject};
use mementum_crdt::PageDoc;
use mementum_md::Block;
use mementum_server::{MAX_DUPLICATED_PAGES, MAX_TITLE_CHARS, PageNode, SignupMode};
use mementum_store::{NewNode, NodeKind, Sibling};
use serde_json::{Value, json};
use uuid::Uuid;

/// Un vrai PNG, d'un pixel.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn duplicate_uri(page: impl std::fmt::Display) -> String {
    format!("/api/v1/pages/{page}/duplicate")
}

/// `POST /api/v1/pages/{id}/duplicate`, avec la session de `by`.
async fn duplicate(by: &Browser<'_>, page: impl std::fmt::Display) -> Reply {
    by.post_empty(&duplicate_uri(page)).await
}

/// Une duplication qui doit aboutir : `201`, et la racine de la copie.
async fn duplicated(by: &Account<'_>, page: impl std::fmt::Display) -> PageNode {
    let reply = duplicate(&by.browser, page).await;
    reply.assert_status(StatusCode::CREATED);
    serde_json::from_slice(&reply.body).unwrap()
}

async fn mmd(by: &Account<'_>, page: impl std::fmt::Display) -> String {
    let reply = by.get_mmd(&page.to_string()).await;
    reply.assert_status(StatusCode::OK);
    reply.text()
}

async fn nodes(t: &TestApp) -> i64 {
    query_i64(&t.db_path(), "SELECT COUNT(*) FROM nodes").await
}

async fn count_of(t: &TestApp, table: &str, column: &str, page: impl std::fmt::Display) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {column} = '{page}'");
    query_i64(&t.db_path(), &sql).await
}

/// Les pages de l'espace « Équipe » que `by` voit, hors corbeille.
async fn tree(by: &Account<'_>, space: Uuid) -> Vec<PageNode> {
    let reply = by
        .browser
        .get(&format!("/api/v1/spaces/{space}/tree"))
        .await;
    reply.assert_status(StatusCode::OK);
    serde_json::from_value(reply.json()["nodes"].clone()).unwrap()
}

/// Les titres des enfants de `parent` dans l'arbre que voit `by`, dans l'ordre des positions.
async fn children(by: &Account<'_>, space: Uuid, parent: Option<&str>) -> Vec<String> {
    let mut nodes: Vec<PageNode> = tree(by, space)
        .await
        .into_iter()
        .filter(|node| node.parent_id.as_deref() == parent)
        .collect();
    nodes.sort_by(|a, b| a.position.cmp(&b.position));
    nodes.into_iter().map(|node| node.title).collect()
}

/// Le niveau et la section de la page, tels que l'API les écrit.
fn placed(node: &PageNode) -> (&'static str, &'static str) {
    let json = serde_json::to_value(node).unwrap();
    let known = |value: &Value, all: &[&'static str]| {
        let text = value.as_str().unwrap();
        *all.iter()
            .find(|name| **name == text)
            .unwrap_or_else(|| panic!("valeur inattendue : {text}"))
    };
    (
        known(&json["level"], &["read", "comment", "edit", "full"]),
        known(&json["section"], &["space", "private", "shared"]),
    )
}

/// La page `id` dans l'arbre de l'espace que voit `by`.
async fn listed(by: &Account<'_>, space: Uuid, id: &str) -> PageNode {
    tree(by, space)
        .await
        .into_iter()
        .find(|node| node.id == id)
        .unwrap_or_else(|| panic!("page {id} absente de l'arbre"))
}

/// `by` voit-il la page `id` ?
async fn sees(by: &Account<'_>, id: &str) -> bool {
    by.browser.get(&format!("/api/v1/pages/{id}")).await.status == StatusCode::OK
}

/// Une page de l'espace « Équipe », créée par Olga, rangée après `after` (`None` : en tête).
async fn page_after(
    team: &Team<'_>,
    parent: Option<Uuid>,
    after: Option<Uuid>,
    title: &str,
) -> Uuid {
    team.store
        .create_node(NewNode {
            space_id: team.space,
            parent_id: parent,
            after: after.map(Sibling::Node),
            kind: NodeKind::Page,
            title: title.to_string(),
            created_by: Some(team.olga.id),
            grants: Vec::new(),
            doc: None,
        })
        .await
        .unwrap()
        .id
}

/// Les blocs de la page, relus de ce que la base garde (instantané et journal).
async fn stored_blocks(team: &Team<'_>, page: impl std::fmt::Display) -> Vec<Block> {
    let page = Uuid::parse_str(&page.to_string()).unwrap();
    let state = team.store.load_doc(page).await.unwrap();
    let updates = state
        .snapshot
        .iter()
        .map(|(_, bytes)| bytes.as_slice())
        .chain(state.updates.iter().map(|(_, bytes)| bytes.as_slice()));
    PageDoc::from_updates(updates).unwrap().read_blocks()
}

/// Les identifiants de tous les blocs, à toute profondeur, et la plus grande profondeur.
fn block_ids(blocks: &[Block]) -> (Vec<String>, usize) {
    fn walk(blocks: &[Block], depth: usize, ids: &mut Vec<String>, deepest: &mut usize) {
        for block in blocks {
            ids.push(block.id.clone());
            *deepest = (*deepest).max(depth);
            walk(&block.children, depth + 1, ids, deepest);
        }
    }
    let (mut ids, mut deepest) = (Vec::new(), 0);
    walk(blocks, 1, &mut ids, &mut deepest);
    (ids, deepest)
}

/// Un contenu qui a des blocs à trois profondeurs, des marques, un tableau et des blocs à props.
const RICH: &str = "# Titre

Un paragraphe **gras** et *italique*.

- Un
  - Deux
    - Trois
- Quatre

<toggle title=\"Détails\">
Dans le pli.
</toggle>

<callout icon=\"💡\" color=\"blue\">
Une note.
</callout>

| A | B |
| --- | --- |
| 1 | 2 |
";

/// La copie porte le contenu de l'original : le même MMD. Et aucun identifiant de bloc de l'une
/// n'est dans l'autre, à aucune profondeur (preuve 5 : un bloc recollé avec l'`id` de l'original
/// serait défait par le dédoublonnage) ; ceux de la copie sont tous posés, et tous distincts.
#[tokio::test]
async fn duplicate_copies_content_with_fresh_block_ids() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team.page(None, "Riche", &[members(Level::Full)]).await;
    team.mona
        .put_mmd(&page.to_string(), RICH)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let original = mmd(&team.mona, page).await;
    assert!(
        original.contains("Trois") && original.contains("| 1 | 2 |"),
        "{original}"
    );

    let copy = duplicated(&team.mona, page).await;
    assert_ne!(copy.id, page.to_string());
    assert_eq!(mmd(&team.mona, &copy.id).await, original);
    assert_eq!(
        mmd(&team.mona, page).await,
        original,
        "l'original n'a pas bougé"
    );

    let (of_original, depth) = block_ids(&stored_blocks(&team, page).await);
    let (of_copy, copy_depth) = block_ids(&stored_blocks(&team, &copy.id).await);
    assert!(depth >= 3, "des blocs à trois profondeurs : {depth}");
    assert_eq!(copy_depth, depth);
    assert_eq!(of_copy.len(), of_original.len());
    assert!(of_copy.iter().all(|id| !id.is_empty()), "{of_copy:?}");
    let known: HashSet<&String> = of_original.iter().collect();
    let fresh: HashSet<&String> = of_copy.iter().collect();
    assert_eq!(fresh.len(), of_copy.len(), "identifiants distincts");
    assert!(
        fresh.is_disjoint(&known),
        "aucun identifiant repris : {of_copy:?} / {of_original:?}"
    );

    // La copie est une page comme une autre : elle s'écrit, et l'original ne suit pas.
    team.mona
        .put_mmd(&copy.id, "Récrite\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(mmd(&team.mona, page).await, original);
}

/// Les sous-pages viennent avec la copie, à leur place et dans leur ordre, avec leur contenu ;
/// pas celles qui sont à la corbeille, ni ce qu'elles portent.
#[tokio::test]
async fn duplicate_copies_the_subtree_but_not_what_is_trashed() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let root = team.page(None, "Projet", &[members(Level::Full)]).await;
    let first = page_after(&team, Some(root), None, "Première").await;
    let second = page_after(&team, Some(root), Some(first), "Deuxième").await;
    let third = page_after(&team, Some(root), Some(second), "Troisième").await;
    let under_first = page_after(&team, Some(first), None, "Sous la première").await;
    let trashed = page_after(&team, Some(root), Some(third), "Jetée").await;
    let _under_trashed = page_after(&team, Some(trashed), None, "Sous la jetée").await;
    let trashed_deep = page_after(&team, Some(second), None, "Jetée, plus bas").await;
    team.trash(trashed).await;
    team.trash(trashed_deep).await;
    for (page, text) in [(root, "racine"), (second, "deux"), (under_first, "dessous")] {
        team.mona
            .put_mmd(&page.to_string(), format!("Contenu {text}\n"))
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
    let before = nodes(&t).await;

    let copy = duplicated(&team.mona, root).await;
    assert_eq!(copy.title, "Projet (copie)");
    assert_eq!(nodes(&t).await, before + 5, "cinq pages copiées, pas huit");
    let mona = &team.mona;
    assert_eq!(
        children(mona, team.space, Some(&copy.id)).await,
        ["Première", "Deuxième", "Troisième"]
    );
    let all = tree(mona, team.space).await;
    let child = |parent: &str, title: &str| {
        all.iter()
            .find(|n| n.parent_id.as_deref() == Some(parent) && n.title == title)
            .unwrap_or_else(|| panic!("« {title} » sous {parent}"))
            .clone()
    };
    let (first_copy, second_copy) = (child(&copy.id, "Première"), child(&copy.id, "Deuxième"));
    // Chaque sous-page garde la position de son original parmi ses sœurs.
    let root_id = root.to_string();
    for title in ["Première", "Deuxième", "Troisième"] {
        assert_eq!(
            child(&copy.id, title).position,
            child(&root_id, title).position,
            "{title}"
        );
    }
    assert_eq!(
        children(mona, team.space, Some(&first_copy.id)).await,
        ["Sous la première"]
    );
    assert!(
        children(mona, team.space, Some(&second_copy.id))
            .await
            .is_empty()
    );
    let under_copy = child(&first_copy.id, "Sous la première");
    assert_eq!(mmd(mona, &copy.id).await, "Contenu racine\n");
    assert_eq!(mmd(mona, &second_copy.id).await, "Contenu deux\n");
    assert_eq!(mmd(mona, &under_copy.id).await, "Contenu dessous\n");
    assert_eq!(mmd(mona, &first_copy.id).await, "<p/>\n");
    // Rien de la copie n'est à la corbeille, et les originaux n'ont pas bougé.
    assert_eq!(
        children(mona, team.space, Some(&root.to_string())).await,
        ["Première", "Deuxième", "Troisième"]
    );
    let in_trash = "SELECT COUNT(*) FROM nodes WHERE trashed_at IS NOT NULL";
    assert_eq!(query_i64(&t.db_path(), in_trash).await, 2);
}

/// La copie d'une sous-page, pour qui peut créer sous la page parente : juste après la page, sous
/// la même page parente, sans accès propre ; elle en hérite, et ceux qui voient la page parente
/// la voient, au niveau qu'ils y ont.
#[tokio::test]
async fn duplicate_of_a_subpage_stays_beside_it_and_inherits() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let grants = [members(Level::Edit), user(&team.gina, Level::Read)];
    let parent = team.page(None, "Projet", &grants).await;
    let page = page_after(&team, Some(parent), None, "Notes").await;
    let _next = page_after(&team, Some(parent), Some(page), "Suite").await;
    // Un accès nommé sur la page : il ne suit pas la copie.
    team.store
        .set_grant(
            page,
            mementum_acl::Grant {
                subject: Subject::User(team.gina.id),
                level: Level::Full,
            },
        )
        .await
        .unwrap();

    let copy = duplicated(&team.mona, page).await;
    assert_eq!(copy.title, "Notes (copie)");
    assert_eq!(copy.parent_id, Some(parent.to_string()));
    assert_eq!(copy.space_id, team.space.to_string());
    assert_eq!(placed(&copy), ("edit", "space"));
    assert_eq!(copy.trashed_at, None);
    let parent_id = parent.to_string();
    assert_eq!(
        children(&team.mona, team.space, Some(&parent_id)).await,
        ["Notes", "Notes (copie)", "Suite"]
    );
    let copy_id = Uuid::parse_str(&copy.id).unwrap();
    assert!(team.grants(copy_id).await.is_empty(), "aucun accès propre");
    // La réponse dit ce que l'arbre dit.
    assert_eq!(listed(&team.mona, team.space, &copy.id).await, copy);
    // Tom (membre) et Olga (propriétaire) la voient comme la page parente ; Gina, invitée en
    // lecture sur la page parente, la lit, sans l'accès complet qu'elle a sur l'original.
    for (who, level) in [
        (&team.tom, "edit"),
        (&team.olga, "full"),
        (&team.gina, "read"),
    ] {
        let seen = tree(who, team.space).await;
        let node = seen.iter().find(|n| n.id == copy.id).expect("la copie");
        assert_eq!(placed(node).0, level);
    }
    assert!(!sees(&team.zoe, &copy.id).await);
}

/// La copie d'une page de racine d'« Espace » : à la racine, en dernier, dans le « Privé » de qui
/// duplique, avec `(user:<lui>, full)` pour toute ligne. Les autres membres ne la voient pas,
/// propriétaire compris.
#[tokio::test]
async fn duplicate_of_a_root_page_is_private_to_who_duplicates() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team.page(None, "D'équipe", &[members(Level::Edit)]).await;
    let _other = page_after(&team, None, Some(page), "Autre").await;
    let under = page_after(&team, Some(page), None, "Sous-page").await;

    let copy = duplicated(&team.mona, page).await;
    assert_eq!(copy.title, "D'équipe (copie)");
    assert_eq!(copy.parent_id, None);
    assert_eq!(placed(&copy), ("full", "private"));
    let copy_id = Uuid::parse_str(&copy.id).unwrap();
    assert_eq!(
        team.grants(copy_id).await,
        [(Subject::User(team.mona.id), Level::Full)]
    );
    assert_eq!(listed(&team.mona, team.space, &copy.id).await, copy);
    // En dernier à la racine : après toutes les pages de l'espace, vues ou non.
    let all = team.store.list_nodes(team.space).await.unwrap();
    let last = all
        .iter()
        .filter(|node| node.parent_id.is_none())
        .max_by(|a, b| a.position.cmp(&b.position))
        .unwrap();
    assert_eq!(last.id.to_string(), copy.id);
    // La sous-page est venue, sans accès propre.
    let subpages = children(&team.mona, team.space, Some(&copy.id)).await;
    assert_eq!(subpages, ["Sous-page"]);
    let acl =
        "SELECT COUNT(*) FROM acl a JOIN nodes n ON n.id = a.node_id WHERE n.parent_id = '{copy}'"
            .replace("{copy}", &copy.id);
    assert_eq!(query_i64(&t.db_path(), &acl).await, 0);
    // Personne d'autre ne la voit, ni elle ni sa sous-page.
    for other in [&team.olga, &team.adam, &team.tom, &team.gina, &team.zoe] {
        assert!(!sees(other, &copy.id).await);
        let seen = tree_or_none(other, team.space).await;
        assert!(
            seen.iter()
                .all(|n| n.id != copy.id && n.parent_id.as_deref() != Some(&copy.id)),
            "la copie est privée"
        );
    }
    // L'original et sa sous-page n'ont pas bougé.
    assert_eq!(team.grants(page).await, [(Subject::Members, Level::Edit)]);
    assert!(sees(&team.tom, &under.to_string()).await);
}

/// L'arbre de l'espace tel que `by` le voit ; vide pour qui n'y a aucun rôle (`404`).
async fn tree_or_none(by: &Account<'_>, space: Uuid) -> Vec<PageNode> {
    let reply = by
        .browser
        .get(&format!("/api/v1/spaces/{space}/tree"))
        .await;
    if reply.status == StatusCode::NOT_FOUND {
        return Vec::new();
    }
    reply.assert_status(StatusCode::OK);
    serde_json::from_value(reply.json()["nodes"].clone()).unwrap()
}

/// Un membre qui lit une sous-page sans voir sa page parente : la copie va à la racine, dans son
/// « Privé » ; rien n'est créé sous la page parente invisible, et la réponse ne la nomme pas.
#[tokio::test]
async fn member_who_does_not_see_the_parent_gets_a_private_root_copy() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let parent = team
        .page(None, "Secrète", &[user(&team.olga, Level::Full)])
        .await;
    let page = team
        .page(Some(parent), "Partagée", &[user(&team.mona, Level::Edit)])
        .await;
    assert!(!sees(&team.mona, &parent.to_string()).await, "précondition");

    let reply = duplicate(&team.mona.browser, page).await;
    reply.assert_status(StatusCode::CREATED);
    assert!(
        !reply.text().contains(&parent.to_string()),
        "la réponse ne nomme pas la page parente : {}",
        reply.text()
    );
    let copy: PageNode = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(copy.parent_id, None);
    assert_eq!(placed(&copy), ("full", "private"));
    assert_eq!(
        count_of(&t, "nodes", "parent_id", parent).await,
        1,
        "rien de plus sous elle"
    );
    let stored = team
        .store
        .get_node(Uuid::parse_str(&copy.id).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.parent_id, None, "à la racine, en base aussi");
    assert!(
        !sees(&team.olga, &copy.id).await,
        "privée, même pour la propriétaire"
    );
}

/// Un membre qui lit une sous-page sous une page parente qu'il lit sans pouvoir y créer : la
/// copie va à la racine, dans son « Privé », et rien n'est créé sous la page parente.
#[tokio::test]
async fn member_who_cannot_create_under_the_parent_gets_a_private_root_copy() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    for level in [Level::Read, Level::Comment] {
        let parent = team.page(None, "En lecture", &[members(level)]).await;
        let page = page_after(&team, Some(parent), None, "Notes").await;

        let copy = duplicated(&team.mona, page).await;
        assert_eq!(copy.parent_id, None, "{level:?}");
        assert_eq!(placed(&copy), ("full", "private"));
        assert_eq!(
            count_of(&t, "nodes", "parent_id", parent).await,
            1,
            "{level:?}"
        );
        assert!(!sees(&team.tom, &copy.id).await, "{level:?}");
        assert_eq!(listed(&team.mona, team.space, &copy.id).await, copy);
    }
}

/// Une invitée ne duplique que là où elle peut créer : une page partagée avec elle à la racine,
/// ou sous une page parente qu'elle ne peut pas modifier, `403`, qui le dit, et rien n'est écrit.
/// Témoin : sous une page parente qu'elle peut modifier, la copie se pose à côté.
#[tokio::test]
async fn guest_cannot_duplicate_where_he_cannot_create() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let gina = &team.gina;
    let at_root = team
        .page(None, "À la racine", &[user(gina, Level::Full)])
        .await;
    let read_parent = team.page(None, "Lue", &[user(gina, Level::Read)]).await;
    let under_read = team
        .page(Some(read_parent), "Sous la lue", &[user(gina, Level::Full)])
        .await;
    let hidden_parent = team.page(None, "Cachée", &[]).await;
    let under_hidden = team
        .page(
            Some(hidden_parent),
            "Sous la cachée",
            &[user(gina, Level::Full)],
        )
        .await;
    let acl = "SELECT COUNT(*) FROM acl";
    let before = (nodes(&t).await, query_i64(&t.db_path(), acl).await);
    for (page, what) in [
        (at_root, "à la racine"),
        (under_read, "sous une page lue"),
        (under_hidden, "sous une page cachée"),
    ] {
        let refusal = duplicate(&gina.browser, page)
            .await
            .assert_error(StatusCode::FORBIDDEN, "forbidden");
        let message = refusal["message"].as_str().unwrap();
        assert!(
            message.contains("réservé aux membres de l'espace"),
            "{what} : {message}"
        );
    }
    let after = (nodes(&t).await, query_i64(&t.db_path(), acl).await);
    assert_eq!(after, before, "rien d'écrit");

    // Témoin : sous une page parente qu'elle peut modifier.
    let edited_parent = team
        .page(None, "Modifiable", &[user(gina, Level::Edit)])
        .await;
    let under_edited = page_after(&team, Some(edited_parent), None, "Notes").await;
    let copy = duplicated(gina, under_edited).await;
    assert_eq!(copy.parent_id, Some(edited_parent.to_string()));
    assert_eq!(placed(&copy), ("edit", "shared"));
    let edited = edited_parent.to_string();
    assert_eq!(
        children(gina, team.space, Some(&edited)).await,
        ["Notes", "Notes (copie)"]
    );
    assert!(
        team.grants(Uuid::parse_str(&copy.id).unwrap())
            .await
            .is_empty()
    );
}

/// Dupliquer demande « Peut lire », à tout niveau, pour un membre ; sans session, `401` ; une
/// page à la corbeille, elle-même ou par sa page parente, `409 trashed`, après le `403` d'une
/// invitée qui n'a nulle part où poser la copie ; rien n'est écrit par un refus.
#[tokio::test]
async fn duplicate_routes_follow_the_levels() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let levels = [Level::Read, Level::Comment, Level::Edit, Level::Full];
    for level in levels {
        // Une page de racine, puis une sous-page, que Tom a à ce niveau.
        let grants = [user(&team.olga, Level::Full), user(&team.tom, level)];
        let root = team.page(None, "Racine", &grants).await;
        let copy = duplicated(&team.tom, root).await;
        assert_eq!(copy.parent_id, None, "{level:?}");
        assert_eq!(placed(&copy).0, "full", "{level:?}");

        let parent = team.page(None, "Parente", &grants).await;
        let page = page_after(&team, Some(parent), None, "Fille").await;
        let copy = duplicated(&team.tom, page).await;
        let beside = level >= Level::Edit;
        assert_eq!(copy.parent_id.is_some(), beside, "{level:?}");

        // À la corbeille : elle-même, puis par sa page parente.
        for trashed in [page, parent] {
            let before = nodes(&t).await;
            team.trash(trashed).await;
            duplicate(&team.tom.browser, page)
                .await
                .assert_error(StatusCode::CONFLICT, "trashed");
            assert_eq!(nodes(&t).await, before, "{level:?}");
            team.store.restore_node(trashed).await.unwrap();
        }
    }
    // Une invitée, sur une page à la corbeille qu'elle ne pourrait pas poser : `403` d'abord.
    let shared = team
        .page(None, "Partagée", &[user(&team.gina, Level::Full)])
        .await;
    team.trash(shared).await;
    duplicate(&team.gina.browser, shared)
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    // Sans session, et pour qui n'a aucun rôle dans l'espace.
    let page = team.page(None, "Ouverte", &[members(Level::Full)]).await;
    let before = nodes(&t).await;
    duplicate(&t.browser(), page)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    duplicate(&team.zoe.browser, page)
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    duplicate(&team.mona.browser, "pas-un-id")
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(nodes(&t).await, before);
    // Un `GET` n'est pas la route.
    let get = team.mona.browser.get(&duplicate_uri(page)).await;
    assert_eq!(get.status, StatusCode::METHOD_NOT_ALLOWED);
}

async fn uploaded(by: &Browser<'_>, page: Uuid, name: &str, bytes: &[u8]) -> Value {
    let reply = by
        .send(
            Request::post(upload_uri(page, name))
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .body(Body::from(bytes.to_vec()))
                .unwrap(),
        )
        .await;
    reply.assert_status(StatusCode::CREATED);
    reply.json()
}

/// La prop `url` des blocs de la page qui en ont une, dans l'ordre du document.
async fn urls(team: &Team<'_>, page: impl std::fmt::Display) -> Vec<String> {
    stored_blocks(team, page)
        .await
        .iter()
        .filter_map(|block| match block.props.get("url") {
            Some(mementum_md::PropValue::Str(url)) => Some(url.clone()),
            _ => None,
        })
        .collect()
}

/// Preuve 3 : la copie garde ses fichiers. L'adresse d'un bloc qui montrait un fichier de la page
/// nomme la copie et une attache à elle (une seule pour deux blocs du même fichier) ; l'original
/// supprimé pour de bon, et le ménage passé, le fichier de la copie se lit encore. Une adresse
/// externe, celle d'un fichier d'une troisième page, celle d'un fichier que la page n'a pas, et
/// celle d'un bloc qui n'est pas un bloc de fichier (un signet) sont restées telles quelles.
#[tokio::test]
async fn duplicated_page_keeps_its_files_after_the_original_is_deleted() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let page = team.page(None, "Illustrée", &[members(Level::Full)]).await;
    let third = team.page(None, "Troisième", &[members(Level::Full)]).await;
    let own = uploaded(&mona.browser, page, "photo.png", PNG).await;
    let other = b"un autre contenu";
    let elsewhere = uploaded(&mona.browser, third, "ailleurs.bin", other).await;
    // Un fichier de la page que plus aucun bloc ne montre : il ne suit pas la copie.
    uploaded(&mona.browser, page, "oubli.bin", b"sans bloc").await;
    let (own_url, elsewhere_url) = (
        own["url"].as_str().unwrap(),
        elsewhere["url"].as_str().unwrap(),
    );
    let external = "https://exemple.fr/logo.png";
    let missing = format!("/api/v1/pages/{page}/files/{}", Uuid::now_v7());
    let content = format!(
        "![photo]({own_url})\n\n![ailleurs]({elsewhere_url})\n\n![externe]({external})\n\n\
         <file src=\"{own_url}\" name=\"photo.png\"/>\n\n![absent]({missing})\n\n\
         <bookmark url=\"{own_url}\"/>\n"
    );
    mona.put_mmd(&page.to_string(), content)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        urls(&team, page).await,
        [
            own_url,
            elsewhere_url,
            external,
            own_url,
            missing.as_str(),
            own_url
        ],
        "précondition"
    );

    let copy = duplicated(mona, page).await;
    let copied = urls(&team, &copy.id).await;
    assert_eq!(copied.len(), 6);
    let prefix = format!("/api/v1/pages/{}/files/", copy.id);
    let file = copied[0]
        .strip_prefix(&prefix)
        .unwrap_or_else(|| panic!("{}", copied[0]));
    assert!(Uuid::parse_str(file).is_ok(), "{file}");
    assert_ne!(file, own["id"].as_str().unwrap(), "une attache à elle");
    assert_eq!(copied[3], copied[0], "le même fichier, la même attache");
    assert_eq!(&copied[1..3], [elsewhere_url, external]);
    assert_eq!(copied[4], missing);
    assert_eq!(
        copied[5], own_url,
        "un signet n'est pas un bloc de fichier : son adresse reste"
    );
    assert_eq!(count_of(&t, "attachments", "node_id", &copy.id).await, 1);
    assert_eq!(count_of(&t, "attachments", "node_id", page).await, 2);
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM files").await,
        3
    );
    let read = async |url: &str| mona.browser.get(url).await;
    assert_eq!(read(&copied[0]).await.body.as_ref(), PNG);

    // L'original supprimé pour de bon : le ménage passe, le fichier de la copie reste.
    mona.browser
        .post_empty(&format!("/api/v1/pages/{page}/trash"))
        .await
        .assert_status(StatusCode::OK);
    mona.browser
        .delete(&format!("/api/v1/pages/{page}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM files").await,
        2,
        "témoin : le ménage est passé, le fichier sans bloc est parti"
    );
    let served = read(&copied[0]).await;
    served.assert_status(StatusCode::OK);
    assert_eq!(served.body.as_ref(), PNG);
    read(own_url)
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    // Le fichier de la troisième page se lit toujours par elle.
    assert_eq!(read(elsewhere_url).await.body.as_ref(), other);
}

/// La copie ne reprend ni les fils de commentaires, ni les versions, ni les favoris, ni les accès
/// nommés de l'original ; lui les garde tous.
#[tokio::test]
async fn duplicate_copies_neither_comments_nor_versions_nor_favorites_nor_named_accesses() {
    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        // Une version à chaque écriture qui en suit une autre.
        cfg.version_every = Duration::ZERO;
    })
    .await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let grants = [
        members(Level::Edit),
        user(&team.gina, Level::Comment),
        user(&team.tom, Level::Full),
    ];
    let parent = team.page(None, "Projet", &[members(Level::Edit)]).await;
    let page = team.page(Some(parent), "Suivie", &grants).await;
    let id = page.to_string();
    for text in ["Un\n", "Deux\n", "Trois\n"] {
        mona.put_mmd(&id, text)
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
    mona.browser
        .post(
            &format!("/api/v1/pages/{id}/comments"),
            json!({ "body": "Un fil" }),
        )
        .await
        .assert_status(StatusCode::CREATED);
    mona.browser
        .send(
            Request::put(format!("/api/v1/pages/{id}/favorite"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let of_original = async || {
        (
            count_of(&t, "comment_threads", "node_id", page).await,
            count_of(&t, "doc_versions", "node_id", page).await,
            count_of(&t, "favorites", "node_id", page).await,
            count_of(&t, "acl", "node_id", page).await,
        )
    };
    let kept = of_original().await;
    assert!(
        kept.0 == 1 && kept.1 >= 1 && kept.2 == 1 && kept.3 == 3,
        "précondition : {kept:?}"
    );

    let copy = duplicated(mona, page).await;
    assert_eq!(mmd(mona, &copy.id).await, "Trois\n");
    for table in [
        "comment_threads",
        "doc_versions",
        "favorites",
        "acl",
        "doc_updates",
    ] {
        assert_eq!(count_of(&t, table, "node_id", &copy.id).await, 0, "{table}");
    }
    assert_eq!(count_of(&t, "doc_snapshots", "node_id", &copy.id).await, 1);
    assert_eq!(of_original().await, kept);
    let listed = mona
        .browser
        .get(&format!("/api/v1/pages/{}/comments", copy.id))
        .await;
    assert_eq!(listed.json(), json!([]));
    let favorites = mona
        .browser
        .get(&format!("/api/v1/spaces/{}/favorites", team.space))
        .await;
    assert!(!favorites.text().contains(&copy.id), "{}", favorites.text());
    // Gina, nommée sur l'original, ne voit pas la copie : l'accès nommé n'a pas suivi.
    assert!(sees(&team.gina, &id).await, "témoin");
    assert!(!sees(&team.gina, &copy.id).await);
}

/// Cent pages se copient ; cent une, `422 subtree_too_large`, qui dit quoi faire, et rien n'est
/// écrit. Une sous-page à la corbeille ne compte pas.
#[tokio::test]
async fn subtree_of_101_pages_is_refused() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let root = team.page(None, "Grande", &[members(Level::Full)]).await;
    let mut last = None;
    for i in 1..MAX_DUPLICATED_PAGES {
        // Une page sur dix sous la précédente : un sous-arbre, pas une liste.
        let parent = if i % 10 == 0 {
            last.unwrap_or(root)
        } else {
            root
        };
        last = Some(page_after(&team, Some(parent), None, &format!("Page {i}")).await);
    }
    let before = nodes(&t).await;
    let copy = duplicated(&team.mona, root).await;
    assert_eq!(nodes(&t).await, before + MAX_DUPLICATED_PAGES as i64);

    // Une de plus, à la corbeille : toujours cent. Restaurée : cent une.
    let extra = page_after(&team, last, None, "De trop").await;
    team.trash(extra).await;
    let before = nodes(&t).await;
    duplicated(&team.mona, root).await;
    assert_eq!(nodes(&t).await, before + MAX_DUPLICATED_PAGES as i64);
    team.store.restore_node(extra).await.unwrap();
    let before = (
        nodes(&t).await,
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM doc_snapshots").await,
    );
    let refusal = duplicate(&team.mona.browser, root)
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "subtree_too_large");
    let message = refusal["message"].as_str().unwrap();
    assert!(
        message.contains("plus de 100 pages") && message.contains("duplique"),
        "{message}"
    );
    let after = (
        nodes(&t).await,
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM doc_snapshots").await,
    );
    assert_eq!(after, before, "rien d'écrit");
    // La copie de cent pages, elle, se duplique encore.
    duplicated(&team.mona, &copy.id).await;
}

/// Le titre de la copie tient dans la borne des titres : celui d'une page de 500 caractères est
/// coupé, et la copie en a 500, qui finissent par « (copie) ». Un titre court est gardé entier.
#[tokio::test]
async fn long_title_is_cut_to_fit() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let long: String = "é".repeat(MAX_TITLE_CHARS);
    let page = team.page(None, &long, &[members(Level::Full)]).await;
    let copy = duplicated(&team.mona, page).await;
    assert_eq!(copy.title.chars().count(), MAX_TITLE_CHARS);
    assert!(copy.title.ends_with(" (copie)"), "{}", copy.title);
    assert!(copy.title.starts_with("ééé"));
    // La copie d'une copie : coupée de même.
    let again = duplicated(&team.mona, &copy.id).await;
    assert_eq!(again.title.chars().count(), MAX_TITLE_CHARS);
    assert!(again.title.ends_with("é (copie)"), "{}", again.title);

    let short = team.page(None, "Court", &[members(Level::Full)]).await;
    assert_eq!(duplicated(&team.mona, short).await.title, "Court (copie)");
}

/// Une copie dont les documents pèsent plus que la borne de l'instance est refusée (`422
/// subtree_too_large`, Ruling R592), et rien n'est écrit. La borne est ici celle d'une instance
/// de test : la moitié de ce que pèse la copie des mêmes pages sur une instance ordinaire, qui,
/// elle, la fait (témoin).
#[tokio::test]
async fn subtree_heavier_than_the_bound_is_refused() {
    let content = (0..50)
        .map(|i| format!("Paragraphe {i}, pour peser un peu.\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let build = async |team: &Team<'_>| {
        let root = team.page(None, "Lourde", &[members(Level::Full)]).await;
        let child = page_after(team, Some(root), None, "Sous-page").await;
        for page in [root, child] {
            team.mona
                .put_mmd(&page.to_string(), content.clone())
                .await
                .assert_status(StatusCode::NO_CONTENT);
        }
        root
    };
    let weight = "SELECT SUM(LENGTH(state_blob)) FROM doc_snapshots";

    // Témoin : sur une instance ordinaire, la copie se fait ; elle pèse `copied` octets.
    let t = open_app().await;
    let team = Team::new(&t).await;
    let root = build(&team).await;
    let before = query_i64(&t.db_path(), weight).await;
    duplicated(&team.mona, root).await;
    let copied = query_i64(&t.db_path(), weight).await - before;
    assert!(
        copied > 4_000,
        "deux documents de cinquante paragraphes : {copied}"
    );

    let t = TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.max_duplicated_bytes = (copied / 2) as usize;
    })
    .await;
    let team = Team::new(&t).await;
    let root = build(&team).await;
    let written = async || (nodes(&t).await, query_i64(&t.db_path(), weight).await);
    let before = written().await;
    let refusal = duplicate(&team.mona.browser, root)
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "subtree_too_large");
    let message = refusal["message"].as_str().unwrap();
    assert!(
        message.contains(&format!("plus de {} octets", copied / 2)),
        "{message}"
    );
    assert!(message.contains("duplique-les par morceaux"), "{message}");
    assert_eq!(written().await, before, "rien d'écrit");
}
