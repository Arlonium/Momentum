//! Les pages sous droits : ce que chacun peut faire d'une page selon son rôle dans l'espace et
//! les accès posés sur la page ou sur ses ancêtres, ce que montre l'arbre, et ce qu'on apprend
//! d'une page qu'on ne voit pas : rien, pas même qu'elle existe.
//!
//! Les statuts attendus sont écrits ici d'après les tableaux du « Modèle des droits » du plan
//! du lot 2 (`docs/superpowers/plans/2026-11-10-p1-lot2-comptes-espaces-droits.md`), sans
//! passer par le calcul de `mementum-acl` : ses types ne servent qu'à poser des accès en base.
//! Les aides (application en mémoire, équipe, routes d'une page) sont dans `common/mod.rs`.

mod common;

use axum::http::{Method, StatusCode};
use common::{
    Account, Browser, Reply, Team, TestApp, json_request, outcome, page_routes, query_i64, text,
    trash_list, tree_nodes, uuid,
};
use mementum_acl::{Level, Role, Subject};
use mementum_crdt::PageDoc;
use mementum_schema::SCHEMA_VERSION;
use mementum_server::SignupMode;
use mementum_store::Store;
use mementum_sync::testkit::Client;
use serde_json::{Value, json};
use uuid::Uuid;

/// Une application où tout le monde peut s'inscrire : les comptes de ces tests reçoivent leur
/// rôle par le store.
async fn open_app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

fn members(level: Level) -> (Subject, Level) {
    (Subject::Members, level)
}

fn user(account: &Account<'_>, level: Level) -> (Subject, Level) {
    (Subject::User(account.id), level)
}

fn page_uri(id: Uuid) -> String {
    format!("/api/v1/pages/{id}")
}

/// Le nombre de nœuds en base, tous espaces confondus.
async fn node_count(t: &TestApp) -> i64 {
    query_i64(&t.db_path(), "SELECT COUNT(*) FROM nodes").await
}

/// Ce que `by` voit de l'arbre d'un espace : `(titre, titre du parent, niveau, section)`, par
/// titre. Un `parentId` qui nomme une page absente de l'arbre fait échouer le test.
async fn seen(by: &Account<'_>, space: Uuid) -> Vec<(String, Option<String>, String, String)> {
    let nodes = tree_nodes(&by.browser, space).await;
    let mut seen: Vec<_> = nodes
        .iter()
        .map(|node| {
            let parent = node["parentId"].as_str().map(|parent| {
                let parent = nodes
                    .iter()
                    .find(|n| n["id"] == parent)
                    .unwrap_or_else(|| panic!("parentId hors de l'arbre : {node}"));
                text(parent, "title").to_string()
            });
            (
                text(node, "title").to_string(),
                parent,
                text(node, "level").to_string(),
                text(node, "section").to_string(),
            )
        })
        .collect();
    seen.sort();
    seen
}

fn row(
    title: &str,
    parent: Option<&str>,
    level: &str,
    section: &str,
) -> (String, Option<String>, String, String) {
    (
        title.to_string(),
        parent.map(str::to_string),
        level.to_string(),
        section.to_string(),
    )
}

// ---------------------------------------------------------------------------------------------
// La matrice.

/// Qui demande.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    Owner,
    Admin,
    Member,
    Guest,
    /// Un compte qui n'a aucun rôle dans l'espace.
    Stranger,
    /// Sans session.
    Anonymous,
}

/// Un niveau, du plus faible au plus fort : l'ordre des variantes est celui du tableau.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lvl {
    Read,
    Comment,
    Edit,
    Full,
}

/// Les accès posés.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lines {
    /// `(user:<un autre>, full)` : la page privée d'un autre.
    PrivateOfAnother,
    /// `(members, niveau)`.
    Members(Lvl),
    /// `(user:<qui demande>, niveau)`.
    Named(Lvl),
}

/// Où ils sont posés.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placed {
    /// Sur la page, qui est à la racine.
    OnPage,
    /// Sur son parent : la page, sans ligne, en hérite.
    OnParent,
}

/// Ce qui est demandé : les dix routes, `PATCH` selon ce qu'il change, `POST /pages` selon
/// l'endroit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Tree,
    TrashList,
    CreateRoot,
    CreateChild,
    Get,
    Rename,
    Reorder,
    /// `PATCH` vers un autre parent, où tous ceux qui ont un rôle peuvent créer.
    MoveUnder,
    /// `PATCH { "parentId": null }`.
    MoveToRoot,
    Trash,
    Restore,
    Delete,
    GetMmd,
    PutMmd,
}

const EVERYONE: [Who; 6] = [
    Who::Owner,
    Who::Admin,
    Who::Member,
    Who::Guest,
    Who::Stranger,
    Who::Anonymous,
];

const LEVELS: [Lvl; 4] = [Lvl::Read, Lvl::Comment, Lvl::Edit, Lvl::Full];

const ACTS: [Act; 14] = [
    Act::Tree,
    Act::TrashList,
    Act::CreateRoot,
    Act::CreateChild,
    Act::Get,
    Act::Rename,
    Act::Reorder,
    Act::MoveUnder,
    Act::MoveToRoot,
    Act::Trash,
    Act::Restore,
    Act::Delete,
    Act::GetMmd,
    Act::PutMmd,
];

/// Le niveau de `who` sur une page qui porte (ou hérite de) `lines` ; `None` : il ne la voit
/// pas. Tableau « Accès posés » et paragraphe « Héritage » du modèle des droits.
fn level_of(who: Who, lines: Lines) -> Option<Lvl> {
    match (who, lines) {
        // Sans rôle dans l'espace, aucun droit, même nommé par une ligne.
        (Who::Stranger | Who::Anonymous, _) => None,
        // La page privée d'un autre : personne d'autre, pas même un propriétaire.
        (_, Lines::PrivateOfAnother) => None,
        // Un propriétaire ou un admin a `full` partout où les membres voient.
        (Who::Owner | Who::Admin, Lines::Members(_)) => Some(Lvl::Full),
        (Who::Member, Lines::Members(level)) => Some(level),
        // Une ligne `members` ne vise pas les invités.
        (Who::Guest, Lines::Members(_)) => None,
        (Who::Owner | Who::Admin | Who::Member | Who::Guest, Lines::Named(level)) => Some(level),
    }
}

/// Le statut attendu et, pour un refus, son code d'erreur.
fn expected(
    who: Who,
    lines: Lines,
    placed: Placed,
    act: Act,
) -> (StatusCode, Option<&'static str>) {
    let not_found = (StatusCode::NOT_FOUND, Some("not_found"));
    let forbidden = (StatusCode::FORBIDDEN, Some("forbidden"));
    if who == Who::Anonymous {
        return (StatusCode::UNAUTHORIZED, Some("unauthenticated"));
    }
    let has_role = who != Who::Stranger;
    let is_member = matches!(who, Who::Owner | Who::Admin | Who::Member);
    // Les routes de l'espace : un rôle suffit ; créer à la racine veut un membre.
    match act {
        Act::Tree | Act::TrashList if has_role => return (StatusCode::OK, None),
        Act::CreateRoot if is_member => return (StatusCode::CREATED, None),
        Act::CreateRoot if has_role => return forbidden,
        Act::Tree | Act::TrashList | Act::CreateRoot => return not_found,
        _ => {}
    }
    // Les routes d'une page : la voir, puis avoir le niveau (tableau « Action / Niveau »).
    let Some(level) = level_of(who, lines) else {
        return match act {
            // La page est nommée dans le corps (`parentId`) : l'erreur d'un id inconnu.
            Act::CreateChild => (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_parent")),
            _ => not_found,
        };
    };
    let (min, success) = match act {
        Act::Get | Act::GetMmd => (Lvl::Read, StatusCode::OK),
        Act::CreateChild => (Lvl::Edit, StatusCode::CREATED),
        Act::Rename | Act::Reorder | Act::Trash | Act::Restore => (Lvl::Edit, StatusCode::OK),
        Act::PutMmd => (Lvl::Edit, StatusCode::NO_CONTENT),
        Act::MoveUnder => (Lvl::Full, StatusCode::OK),
        Act::Delete => (Lvl::Full, StatusCode::NO_CONTENT),
        // Une page déjà à la racine y change seulement de place parmi les mêmes frères.
        Act::MoveToRoot => match placed {
            Placed::OnPage => (Lvl::Edit, StatusCode::OK),
            Placed::OnParent => (Lvl::Full, StatusCode::OK),
        },
        Act::Tree | Act::TrashList | Act::CreateRoot => unreachable!("traitées plus haut"),
    };
    if level < min {
        return forbidden;
    }
    // Passer à la racine, c'est y créer une page : il faut être membre.
    if act == Act::MoveToRoot && placed == Placed::OnParent && !is_member {
        return forbidden;
    }
    (success, None)
}

fn acl_level(level: Lvl) -> Level {
    match level {
        Lvl::Read => Level::Read,
        Lvl::Comment => Level::Comment,
        Lvl::Edit => Level::Edit,
        Lvl::Full => Level::Full,
    }
}

/// La page dont `act` a besoin.
enum Needs {
    /// Aucune : une route de l'espace.
    NoPage,
    /// Une page. L'action n'en change ni les accès, ni le parent, ni la corbeille : la
    /// suivante peut la reprendre.
    Page,
    /// Une page neuve : permise, l'action la déplace ou la met à la corbeille.
    FreshPage,
    /// Une page neuve, à la corbeille : on ne restaure et on ne supprime que ce qui y est.
    TrashedPage,
}

fn needs(act: Act) -> Needs {
    match act {
        Act::Tree | Act::TrashList | Act::CreateRoot => Needs::NoPage,
        Act::CreateChild | Act::Get | Act::Rename | Act::Reorder | Act::GetMmd | Act::PutMmd => {
            Needs::Page
        }
        Act::MoveUnder | Act::MoveToRoot | Act::Trash => Needs::FreshPage,
        Act::Restore | Act::Delete => Needs::TrashedPage,
    }
}

/// Envoie la requête de `act`.
async fn attempt(
    by: &Browser<'_>,
    act: Act,
    space: Uuid,
    page: Option<Uuid>,
    destination: Uuid,
) -> Reply {
    let page = || page.expect("cette action porte sur une page");
    let uri = || page_uri(page());
    match act {
        Act::Tree => by.get(&format!("/api/v1/spaces/{space}/tree")).await,
        Act::TrashList => by.get(&format!("/api/v1/spaces/{space}/trash")).await,
        Act::CreateRoot => {
            by.post("/api/v1/pages", json!({ "spaceId": space.to_string() }))
                .await
        }
        Act::CreateChild => {
            by.post("/api/v1/pages", json!({ "parentId": page().to_string() }))
                .await
        }
        Act::Get => by.get(&uri()).await,
        Act::Rename => by.patch(&uri(), json!({ "title": "Renommée" })).await,
        Act::Reorder => by.patch(&uri(), json!({ "afterId": null })).await,
        Act::MoveUnder => {
            by.patch(&uri(), json!({ "parentId": destination.to_string() }))
                .await
        }
        Act::MoveToRoot => by.patch(&uri(), json!({ "parentId": null })).await,
        Act::Trash => by.post_empty(&format!("{}/trash", uri())).await,
        Act::Restore => by.post_empty(&format!("{}/restore", uri())).await,
        Act::Delete => by.delete(&uri()).await,
        Act::GetMmd => by.get_mmd(&page().to_string()).await,
        Act::PutMmd => by.put_mmd(&page().to_string(), "Texte\n").await,
    }
}

/// Chaque sujet, sur chaque page, par chaque route : le statut est celui des tableaux. Une
/// action qui, permise, déplace la page ou la met à la corbeille a sa propre page, neuve : elle
/// ne change rien pour la suivante.
#[tokio::test]
async fn permission_matrix() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let anonymous = t.browser();
    let mut scenarios = vec![Lines::PrivateOfAnother];
    scenarios.extend(LEVELS.map(Lines::Members));
    scenarios.extend(LEVELS.map(Lines::Named));
    let mut cells = 0;
    for lines in scenarios {
        for placed in [Placed::OnPage, Placed::OnParent] {
            // Un espace par scénario, avec les mêmes rôles : les espaces restent petits.
            let space = team.another_space("Matrice").await;
            let destination = team
                .page_in(
                    space,
                    None,
                    "Destination",
                    &[members(Level::Full), user(&team.gina, Level::Full)],
                )
                .await;
            for who in EVERYONE {
                let (by, named) = match who {
                    Who::Owner => (&team.olga.browser, team.olga.id),
                    Who::Admin => (&team.adam.browser, team.adam.id),
                    Who::Member => (&team.mona.browser, team.mona.id),
                    Who::Guest => (&team.gina.browser, team.gina.id),
                    Who::Stranger => (&team.zoe.browser, team.zoe.id),
                    // Personne à nommer : la ligne nomme un compte sans rôle.
                    Who::Anonymous => (&anonymous, team.zoe.id),
                };
                let grants = match lines {
                    Lines::PrivateOfAnother => vec![user(&team.tom, Level::Full)],
                    Lines::Members(level) => vec![members(acl_level(level))],
                    Lines::Named(level) => vec![(Subject::User(named), acl_level(level))],
                };
                let new_page = async || match placed {
                    Placed::OnPage => team.page_in(space, None, "Page", &grants).await,
                    Placed::OnParent => {
                        let parent = team.page_in(space, None, "Parent", &grants).await;
                        team.page_in(space, Some(parent), "Page", &[]).await
                    }
                };
                let mut reused = None;
                for act in ACTS {
                    let page = match needs(act) {
                        Needs::NoPage => None,
                        Needs::Page => {
                            if reused.is_none() {
                                reused = Some(new_page().await);
                            }
                            reused
                        }
                        Needs::FreshPage => Some(new_page().await),
                        Needs::TrashedPage => {
                            let page = new_page().await;
                            team.trash(page).await;
                            Some(page)
                        }
                    };
                    let reply = attempt(by, act, space, page, destination).await;
                    let (status, code) = expected(who, lines, placed, act);
                    let cell = format!("{who:?}, {lines:?}, {placed:?}, {act:?}");
                    assert_eq!(reply.status, status, "{cell} : {}", reply.text());
                    if let Some(code) = code {
                        reply.assert_error(status, code);
                    }
                    cells += 1;
                }
            }
        }
    }
    assert_eq!(cells, 9 * 2 * 6 * 14, "toutes les cases ont été jouées");
}

// ---------------------------------------------------------------------------------------------
// Ne rien révéler.

/// Review Focus 3 : pour chaque route de page, une page d'un autre espace reçoit exactement la
/// réponse d'un id inconnu (statut, type, corps), comme une page privée d'un autre dans son
/// propre espace, une page à la corbeille qu'on ne voit pas, et un id mal formé. De même quand
/// la page est nommée dans le corps (`parentId`, `afterId`), et pour un espace.
#[tokio::test]
async fn foreign_page_is_indistinguishable_from_missing() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let zoe = &team.zoe;
    // Dans « Équipe », où Zoé n'a aucun rôle : une page des membres et sa sous-page, une page
    // dont une ligne nomme pourtant Zoé, une page à la corbeille.
    let foreign = team.page(None, "Commune", &[members(Level::Full)]).await;
    let foreign_child = team.page(Some(foreign), "Sous-page", &[]).await;
    let named = team.page(None, "Nommée", &[user(zoe, Level::Full)]).await;
    let binned = team.page(None, "Jetée", &[members(Level::Full)]).await;
    team.trash(binned).await;
    // Dans son propre espace : la page privée de Tom, qu'elle y a fait entrer.
    team.store
        .add_member(zoe.space, team.tom.id, Role::Member)
        .await
        .unwrap();
    let private = team
        .page_in(
            zoe.space,
            None,
            "Privée de Tom",
            &[user(&team.tom, Level::Full)],
        )
        .await;
    let own = zoe.create(json!({ "title": "À Zoé" })).await;
    let before = (node_count(&t).await, zoe.tree().await);

    let unknown = Uuid::now_v7().to_string();
    let hidden: Vec<(&str, String)> = vec![
        ("d'un autre espace", foreign.to_string()),
        ("sous-page d'un autre espace", foreign_child.to_string()),
        (
            "d'un autre espace, où une ligne la nomme",
            named.to_string(),
        ),
        ("d'un autre espace, à la corbeille", binned.to_string()),
        ("privée d'un autre, dans son espace", private.to_string()),
        ("id mal formé", "pas-un-uuid".to_string()),
    ];

    // L'id dans le chemin.
    let missing: Vec<_> = {
        let mut replies = Vec::new();
        for (name, request) in page_routes(&unknown) {
            let reply = zoe.browser.send(request).await;
            let body = reply.assert_error(StatusCode::NOT_FOUND, "not_found");
            assert_eq!(body["message"], "page introuvable", "{name}");
            replies.push(outcome(&reply));
        }
        replies
    };
    for (what, id) in &hidden {
        for ((name, request), missing) in page_routes(id).into_iter().zip(&missing) {
            let reply = zoe.browser.send(request).await;
            assert_eq!(&outcome(&reply), missing, "{name}, page {what}");
        }
    }

    // L'id dans le corps : le parent d'une page neuve, le nouveau parent d'une page, le frère
    // après lequel se placer. `(nom, code attendu, requête)`.
    let space = zoe.space.to_string();
    let own_uri = format!("/api/v1/pages/{}", own.id);
    let named_in_body = |id: &str| {
        let post = |body: Value| json_request(Method::POST, "/api/v1/pages", &body);
        let patch = |body: Value| json_request(Method::PATCH, &own_uri, &body);
        vec![
            (
                "POST parentId",
                "invalid_parent",
                post(json!({ "parentId": id })),
            ),
            (
                "POST spaceId et parentId",
                "invalid_parent",
                post(json!({ "spaceId": space, "parentId": id })),
            ),
            (
                "POST afterId",
                "invalid_after",
                post(json!({ "spaceId": space, "afterId": id })),
            ),
            (
                "PATCH parentId",
                "invalid_parent",
                patch(json!({ "parentId": id })),
            ),
            (
                "PATCH afterId",
                "invalid_after",
                patch(json!({ "afterId": id })),
            ),
            (
                "PATCH à la racine, afterId",
                "invalid_after",
                patch(json!({ "parentId": null, "afterId": id })),
            ),
        ]
    };
    let mut missing = Vec::new();
    for (_, code, request) in named_in_body(&unknown) {
        let reply = zoe.browser.send(request).await;
        reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, code);
        missing.push(outcome(&reply));
    }
    for (what, id) in &hidden {
        for ((name, _, request), missing) in named_in_body(id).into_iter().zip(&missing) {
            let reply = zoe.browser.send(request).await;
            assert_eq!(&outcome(&reply), missing, "{name}, page {what}");
        }
    }

    // Un espace où l'on n'a aucun rôle : comme un espace qui n'existe pas.
    for route in ["tree", "trash"] {
        let get = async |space: &str| {
            let uri = format!("/api/v1/spaces/{space}/{route}");
            zoe.browser.get(&uri).await
        };
        let missing = get(&unknown).await;
        missing.assert_error(StatusCode::NOT_FOUND, "not_found");
        for space in [team.space.to_string(), "pas-un-uuid".to_string()] {
            assert_eq!(outcome(&get(&space).await), outcome(&missing), "{route}");
        }
    }
    let create = async |space: &str| {
        let body = json!({ "spaceId": space, "title": "Intruse" });
        zoe.browser.post("/api/v1/pages", body).await
    };
    let missing = create(&unknown).await;
    missing.assert_error(StatusCode::NOT_FOUND, "not_found");
    for space in [team.space.to_string(), "pas-un-uuid".to_string()] {
        assert_eq!(outcome(&create(&space).await), outcome(&missing));
    }

    // Aucune de ces requêtes n'a rien écrit.
    assert_eq!((node_count(&t).await, zoe.tree().await), before);
    let commune = team
        .store
        .get_node(foreign)
        .await
        .unwrap()
        .expect("la page");
    assert_eq!(
        (commune.title.as_str(), commune.trashed_at),
        ("Commune", None)
    );
}

/// L'arbre d'un espace ne montre à chacun que ce qu'il peut lire, hors corbeille, avec son
/// niveau et sa section (« Espace », « Partagé », « Privé »).
#[tokio::test]
async fn tree_shows_only_readable_nodes_with_sections() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let shared = team.page(None, "Commune", &[members(Level::Full)]).await;
    team.page(Some(shared), "Commune, enfant", &[]).await;
    team.page(None, "En lecture", &[members(Level::Read)]).await;
    let private = team
        .page(None, "Privée de Mona", &[user(&team.mona, Level::Full)])
        .await;
    team.page(Some(private), "Privée de Mona, enfant", &[])
        .await;
    team.page(
        None,
        "Partagée avec Gina",
        &[
            user(&team.mona, Level::Full),
            user(&team.gina, Level::Comment),
        ],
    )
    .await;
    // À la corbeille : une page des membres avec son enfant, et un enfant d'une page vivante.
    let binned = team.page(None, "Jetée", &[members(Level::Full)]).await;
    team.page(Some(binned), "Jetée, enfant", &[]).await;
    team.trash(binned).await;
    let binned_child = team.page(Some(shared), "Commune, enfant jeté", &[]).await;
    team.trash(binned_child).await;

    // Propriétaire et admin : accès complet à ce que voient les membres, et rien d'autre.
    let of_members = [
        row("Commune", None, "full", "space"),
        row("Commune, enfant", Some("Commune"), "full", "space"),
        row("En lecture", None, "full", "space"),
    ];
    assert_eq!(seen(&team.olga, team.space).await, of_members);
    assert_eq!(seen(&team.adam, team.space).await, of_members);
    // Un membre : le niveau de la ligne `members` ; ses pages privées ; ce qu'il partage.
    assert_eq!(
        seen(&team.mona, team.space).await,
        [
            row("Commune", None, "full", "space"),
            row("Commune, enfant", Some("Commune"), "full", "space"),
            row("En lecture", None, "read", "space"),
            row("Partagée avec Gina", None, "full", "shared"),
            row("Privée de Mona", None, "full", "private"),
            row(
                "Privée de Mona, enfant",
                Some("Privée de Mona"),
                "full",
                "private"
            ),
        ]
    );
    assert_eq!(
        seen(&team.tom, team.space).await,
        [
            row("Commune", None, "full", "space"),
            row("Commune, enfant", Some("Commune"), "full", "space"),
            row("En lecture", None, "read", "space"),
        ]
    );
    // Une invitée : ce qui lui est partagé, et c'est tout.
    assert_eq!(
        seen(&team.gina, team.space).await,
        [row("Partagée avec Gina", None, "comment", "shared")]
    );

    // La forme : `spaceId` et `nodes` (plus de `vaultId`) ; chaque nœud avec ses champs ; triés
    // par `(parentId, position)`, la racine d'abord.
    let uri = format!("/api/v1/spaces/{}/tree", team.space);
    let tree = team.mona.browser.get(&uri).await.json();
    let keys = |value: &Value| {
        let mut keys: Vec<String> = value
            .as_object()
            .expect("un objet")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(keys(&tree), ["nodes", "spaceId"]);
    let nodes = tree["nodes"].as_array().expect("une liste");
    for node in nodes {
        assert_eq!(
            keys(node),
            [
                "id",
                "kind",
                "level",
                "parentId",
                "position",
                "section",
                "spaceId",
                "title",
                "trashedAt",
                "updatedAt"
            ]
        );
        assert_eq!(node["spaceId"], team.space.to_string());
        assert_eq!(node["kind"], "page");
        assert!(node["trashedAt"].is_null(), "{node}");
        assert!(
            node["updatedAt"].as_i64().is_some_and(|at| at > 0),
            "{node}"
        );
    }
    let order: Vec<(Option<&str>, &str)> = nodes
        .iter()
        .map(|n| (n["parentId"].as_str(), text(n, "position")))
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);

    // Sans rôle dans l'espace (admin de l'instance compris) : l'espace n'existe pas. Sans
    // session : `401`.
    for stranger in [&team.zoe, &team.sam] {
        stranger
            .browser
            .get(&uri)
            .await
            .assert_error(StatusCode::NOT_FOUND, "not_found");
    }
    t.browser()
        .get(&uri)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    // L'arbre du lot 1, sans espace, n'existe plus.
    team.olga
        .browser
        .get("/api/v1/tree")
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
}

/// Une page dont on ne voit pas le parent est à la racine de ce qu'on voit : `parentId` est
/// `null`, et l'id du parent n'apparaît dans aucune réponse.
#[tokio::test]
async fn hidden_parent_is_not_leaked() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, gina) = (&team.mona, &team.gina);
    let hidden = team.page(None, "Cachée", &[user(mona, Level::Full)]).await;
    let shown = team
        .page(Some(hidden), "Montrée", &[user(gina, Level::Edit)])
        .await;
    let uri = page_uri(shown);

    let nodes = tree_nodes(&gina.browser, team.space).await;
    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert_eq!(nodes[0]["id"], shown.to_string());
    assert!(nodes[0]["parentId"].is_null());
    assert_eq!(
        (text(&nodes[0], "level"), text(&nodes[0], "section")),
        ("edit", "shared")
    );

    // Partout où la page est rendue.
    let tree_uri = format!("/api/v1/spaces/{}/tree", team.space);
    let trash_uri = format!("/api/v1/spaces/{}/trash", team.space);
    let child = json!({ "parentId": shown.to_string(), "title": "Sous-page de Gina" });
    let replies = [
        ("arbre", gina.browser.get(&tree_uri).await),
        ("page", gina.browser.get(&uri).await),
        (
            "titre",
            gina.browser
                .patch(&uri, json!({ "title": "Montrée à Gina" }))
                .await,
        ),
        (
            "place",
            gina.browser.patch(&uri, json!({ "afterId": null })).await,
        ),
        ("sous-page", gina.browser.post("/api/v1/pages", child).await),
        (
            "corbeille",
            gina.browser.post_empty(&format!("{uri}/trash")).await,
        ),
        ("liste de la corbeille", gina.browser.get(&trash_uri).await),
        (
            "restauration",
            gina.browser.post_empty(&format!("{uri}/restore")).await,
        ),
    ];
    for (name, reply) in &replies {
        assert!(reply.status.is_success(), "{name} : {}", reply.text());
        assert!(
            !reply.text().contains(&hidden.to_string()),
            "{name} : l'id du parent caché est dans {}",
            reply.text()
        );
    }
    for name in ["page", "titre", "place", "corbeille", "restauration"] {
        let (_, reply) = replies.iter().find(|(n, _)| *n == name).unwrap();
        let node = reply.json();
        assert_eq!(node["id"], shown.to_string(), "{name}");
        assert!(node["parentId"].is_null(), "{name} : {node}");
    }

    // Mona, elle, voit le parent.
    let node = mona.browser.get(&uri).await.json();
    assert_eq!(node["parentId"], hidden.to_string());

    // Gina ne peut ni le lire, ni le nommer : comme un id inconnu.
    let unknown = Uuid::now_v7();
    assert_eq!(
        outcome(&gina.browser.get(&page_uri(hidden)).await),
        outcome(&gina.browser.get(&page_uri(unknown)).await)
    );
    let named = gina
        .browser
        .patch(&uri, json!({ "parentId": hidden.to_string() }))
        .await;
    named.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_parent");
    let missing = gina
        .browser
        .patch(&uri, json!({ "parentId": unknown.to_string() }))
        .await;
    assert_eq!(outcome(&named), outcome(&missing));
}

/// Une page privée n'est vue que de son auteur : ni le propriétaire, ni un admin de l'espace,
/// ni l'admin de l'instance (sans rôle, puis admin de l'espace) ne la lisent, ne l'écrivent,
/// ni n'apprennent qu'elle existe.
#[tokio::test]
async fn private_page_is_invisible_to_admin_and_instance_admin() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let space = team.space.to_string();
    // Sans `section` : privée.
    let page = mona
        .browser
        .post(
            "/api/v1/pages",
            json!({ "spaceId": space, "title": "Journal" }),
        )
        .await;
    page.assert_status(StatusCode::CREATED);
    let page = uuid(&page.json()["id"]);
    let child = mona
        .browser
        .post(
            "/api/v1/pages",
            json!({ "parentId": page.to_string(), "title": "Lundi" }),
        )
        .await;
    child.assert_status(StatusCode::CREATED);
    let child = uuid(&child.json()["id"]);
    mona.put_mmd(&child.to_string(), "Secret\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(
        seen(mona, team.space).await,
        [
            row("Journal", None, "full", "private"),
            row("Lundi", Some("Journal"), "full", "private"),
        ]
    );

    let unknown = Uuid::now_v7().to_string();
    let hidden_from = async |who: &Account<'_>, name: &str| {
        for id in [page, child] {
            let missing = page_routes(&unknown);
            for ((route, request), (_, missing)) in
                page_routes(&id.to_string()).into_iter().zip(missing)
            {
                let reply = who.browser.send(request).await;
                reply.assert_error(StatusCode::NOT_FOUND, "not_found");
                let missing = who.browser.send(missing).await;
                assert_eq!(outcome(&reply), outcome(&missing), "{name}, {route}");
            }
        }
    };
    for (who, name) in [
        (&team.olga, "Olga"),
        (&team.adam, "Adam"),
        (&team.tom, "Tom"),
    ] {
        hidden_from(who, name).await;
        assert_eq!(seen(who, team.space).await, [], "{name}");
        assert_eq!(trash_list(&who.browser, team.space).await, [] as [Value; 0]);
    }
    // L'admin de l'instance, sans rôle dans l'espace.
    hidden_from(&team.sam, "Sam").await;
    team.sam
        .browser
        .get(&format!("/api/v1/spaces/{space}/tree"))
        .await
        .assert_error(StatusCode::NOT_FOUND, "not_found");
    // Devenu admin de l'espace : il voit l'espace, toujours pas la page.
    team.store
        .add_member(team.space, team.sam.id, Role::Admin)
        .await
        .unwrap();
    hidden_from(&team.sam, "Sam, admin de l'espace").await;
    assert_eq!(seen(&team.sam, team.space).await, []);

    // À la corbeille non plus, pas même dans la liste.
    mona.browser
        .post_empty(&format!("{}/trash", page_uri(page)))
        .await
        .assert_status(StatusCode::OK);
    for (who, name) in [(&team.olga, "Olga"), (&team.sam, "Sam")] {
        hidden_from(who, name).await;
        assert_eq!(trash_list(&who.browser, team.space).await, [] as [Value; 0]);
    }
    assert_eq!(trash_list(&mona.browser, team.space).await.len(), 1);
    // Rien n'a été écrit par les autres.
    assert_eq!(mona.get_mmd(&child.to_string()).await.text(), "Secret\n");
    let journal = team.store.get_node(page).await.unwrap().expect("la page");
    assert_eq!(journal.title, "Journal");
}

// ---------------------------------------------------------------------------------------------
// Créer et déplacer.

/// Une page créée à la racine reçoit les accès de sa section : `(members, full)` dans
/// « Espace », `(user:<auteur>, full)` dans « Privé » (le défaut). Une sous-page n'en reçoit
/// aucun : elle hérite.
#[tokio::test]
async fn root_page_gets_the_access_of_its_section() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let mona = &team.mona;
    let space = team.space.to_string();
    let post = async |body: Value| mona.browser.post("/api/v1/pages", body).await;
    let created = async |body: Value| {
        let reply = post(body).await;
        reply.assert_status(StatusCode::CREATED);
        reply.json()
    };

    let private = created(json!({ "spaceId": space, "title": "Privée" })).await;
    assert_eq!(private["spaceId"], space);
    assert!(private["parentId"].is_null());
    assert!(private["trashedAt"].is_null());
    assert_eq!(
        (text(&private, "level"), text(&private, "section")),
        ("full", "private")
    );
    assert_eq!(
        team.grants(uuid(&private["id"])).await,
        [user(mona, Level::Full)]
    );
    let explicit = created(json!({ "spaceId": space, "section": "private" })).await;
    assert_eq!(
        team.grants(uuid(&explicit["id"])).await,
        [user(mona, Level::Full)]
    );

    let shared = created(json!({ "spaceId": space, "section": "space", "title": "Commune" })).await;
    assert_eq!(
        (text(&shared, "level"), text(&shared, "section")),
        ("full", "space")
    );
    let shared_id = uuid(&shared["id"]);
    assert_eq!(team.grants(shared_id).await, [members(Level::Full)]);
    // Tous les membres y ont l'accès complet, comme dans Notion.
    let for_tom = team.tom.browser.get(&page_uri(shared_id)).await.json();
    assert_eq!(
        (text(&for_tom, "level"), text(&for_tom, "section")),
        ("full", "space")
    );

    // Une sous-page hérite ; `spaceId` y est facultatif.
    let parent = shared_id.to_string();
    for body in [
        json!({ "parentId": parent, "title": "Sous-page" }),
        json!({ "parentId": parent, "spaceId": space }),
    ] {
        let child = created(body).await;
        assert_eq!(child["parentId"], parent);
        assert_eq!(child["spaceId"], space);
        assert_eq!(
            (text(&child, "level"), text(&child, "section")),
            ("full", "space")
        );
        assert_eq!(team.grants(uuid(&child["id"])).await, []);
    }

    // Les refus n'écrivent rien.
    let count = node_count(&t).await;
    let body = post(json!({ "title": "Sans espace" }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    assert!(text(&body, "message").contains("spaceId"), "{body}");
    let body = post(json!({ "parentId": parent, "section": "space" }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    assert!(text(&body, "message").contains("section"), "{body}");
    // « Partagé » ne se choisit pas : c'est ce que devient une page qu'on partage.
    let body = post(json!({ "spaceId": space, "section": "shared" }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_json");
    assert!(text(&body, "message").contains("section"), "{body}");
    // Le parent est d'un autre espace que celui qu'on nomme.
    post(json!({ "spaceId": mona.space.to_string(), "parentId": parent }))
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_parent");
    assert_eq!(node_count(&t).await, count);
}

/// Un invité ne crée rien à la racine d'un espace ; il crée une sous-page là où il peut
/// écrire.
#[tokio::test]
async fn guest_cannot_create_root_page() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let gina = &team.gina;
    let space = team.space.to_string();
    let shared = team
        .page(None, "Partagée", &[user(gina, Level::Edit)])
        .await;
    let read_only = team
        .page(None, "En lecture", &[user(gina, Level::Comment)])
        .await;
    let count = node_count(&t).await;
    for body in [
        json!({ "spaceId": space }),
        json!({ "spaceId": space, "section": "space" }),
        json!({ "spaceId": space, "section": "private", "title": "À Gina" }),
    ] {
        gina.browser
            .post("/api/v1/pages", body)
            .await
            .assert_error(StatusCode::FORBIDDEN, "forbidden");
    }
    // Sous une page qu'elle lit sans pouvoir l'écrire : non plus.
    gina.browser
        .post(
            "/api/v1/pages",
            json!({ "parentId": read_only.to_string() }),
        )
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(node_count(&t).await, count);

    // Sous une page partagée avec elle en écriture : oui, et la sous-page hérite.
    let child = gina
        .browser
        .post(
            "/api/v1/pages",
            json!({ "parentId": shared.to_string(), "title": "De Gina" }),
        )
        .await;
    child.assert_status(StatusCode::CREATED);
    let child = child.json();
    assert_eq!(child["parentId"], shared.to_string());
    assert_eq!(
        (text(&child, "level"), text(&child, "section")),
        ("edit", "shared")
    );
    assert_eq!(team.grants(uuid(&child["id"])).await, []);
}

/// Changer de place parmi les mêmes frères demande `edit` ; changer de parent demande `full`
/// sur la page, et de pouvoir créer sous le nouveau parent.
#[tokio::test]
async fn move_to_another_parent_requires_full() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (olga, mona, tom) = (&team.olga, &team.mona, &team.tom);
    let project = team.page(None, "Projet", &[members(Level::Edit)]).await;
    let page = team.page(Some(project), "Page", &[]).await;
    // Créée après, donc en tête : l'ordre est « Sœur », « Page ».
    let sibling = team.page(Some(project), "Sœur", &[]).await;
    let elsewhere = team.page(None, "Ailleurs", &[members(Level::Full)]).await;
    let id = page.to_string();

    // Avec `edit` : renommer, et changer de place sous le même parent, nommé ou non.
    assert_eq!(
        tom.patched(&id, json!({ "title": "Page de Tom" }))
            .await
            .title,
        "Page de Tom"
    );
    tom.patched(&id, json!({ "afterId": null })).await;
    let titles = |nodes: Vec<Value>| -> Vec<String> {
        let mut children: Vec<&Value> = nodes
            .iter()
            .filter(|n| n["parentId"] == project.to_string())
            .collect();
        children.sort_by_key(|n| text(n, "position").to_string());
        children
            .iter()
            .map(|n| text(n, "title").to_string())
            .collect()
    };
    assert_eq!(
        titles(tree_nodes(&tom.browser, team.space).await),
        ["Page de Tom", "Sœur"]
    );
    tom.patched(
        &id,
        json!({ "parentId": project.to_string(), "afterId": sibling.to_string() }),
    )
    .await;
    assert_eq!(
        titles(tree_nodes(&tom.browser, team.space).await),
        ["Sœur", "Page de Tom"]
    );

    // Changer de parent : non, sous une autre page comme à la racine, et rien ne bouge, pas
    // même le titre envoyé avec.
    let before = team.store.get_node(page).await.unwrap();
    for body in [
        json!({ "parentId": elsewhere.to_string() }),
        json!({ "parentId": elsewhere.to_string(), "afterId": null }),
        json!({ "parentId": null }),
        json!({ "title": "Déplacée", "parentId": elsewhere.to_string() }),
    ] {
        tom.patch(&id, body)
            .await
            .assert_error(StatusCode::FORBIDDEN, "forbidden");
    }
    assert_eq!(team.store.get_node(page).await.unwrap(), before);

    // Avec `full` (la propriétaire, sur une page des membres) : oui.
    let moved = olga
        .patched(&id, json!({ "parentId": elsewhere.to_string() }))
        .await;
    assert_eq!(moved.parent_id, Some(elsewhere.to_string()));

    // `full` sur la page ne suffit pas : il faut pouvoir créer sous la destination.
    let own = team.page(None, "À Mona", &[user(mona, Level::Full)]).await;
    let read_only = team.page(None, "En lecture", &[members(Level::Read)]).await;
    let private = team
        .page(None, "Privée de Tom", &[user(tom, Level::Full)])
        .await;
    let binned = team.page(None, "Jetée", &[members(Level::Full)]).await;
    team.trash(binned).await;
    let own_id = own.to_string();
    mona.patch(&own_id, json!({ "parentId": read_only.to_string() }))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    // Une destination à la corbeille ne reçoit plus rien.
    mona.patch(&own_id, json!({ "parentId": binned.to_string() }))
        .await
        .assert_error(StatusCode::CONFLICT, "trashed");
    // Une destination qu'elle ne voit pas : l'erreur d'un id inconnu.
    let hidden = mona
        .patch(&own_id, json!({ "parentId": private.to_string() }))
        .await;
    hidden.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_parent");
    let missing = mona
        .patch(&own_id, json!({ "parentId": Uuid::now_v7().to_string() }))
        .await;
    assert_eq!(outcome(&hidden), outcome(&missing));
    let still = team.store.get_node(own).await.unwrap().expect("la page");
    assert_eq!(still.parent_id, None);

    let moved = mona
        .patched(&own_id, json!({ "parentId": elsewhere.to_string() }))
        .await;
    assert_eq!(moved.parent_id, Some(elsewhere.to_string()));
}

/// Une page passée à la racine sans accès propre reçoit `(user:<qui la déplace>, full)` : elle
/// ne devient pas invisible de tous. Une page qui porte ses propres accès les garde.
#[tokio::test]
async fn page_moved_to_root_stays_visible_to_its_mover() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (olga, mona, tom) = (&team.olga, &team.mona, &team.tom);
    let shared = team.page(None, "Commune", &[members(Level::Full)]).await;
    let child = team.page(Some(shared), "Enfant", &[]).await;
    let grandchild = team.page(Some(child), "Petit-enfant", &[]).await;

    let reply = mona
        .patch(&child.to_string(), json!({ "parentId": null }))
        .await;
    reply.assert_status(StatusCode::OK);
    let moved = reply.json();
    assert!(moved["parentId"].is_null());
    assert_eq!(
        (text(&moved, "level"), text(&moved, "section")),
        ("full", "private")
    );
    assert_eq!(team.grants(child).await, [user(mona, Level::Full)]);
    assert_eq!(team.grants(grandchild).await, []);
    assert_eq!(
        seen(mona, team.space).await,
        [
            row("Commune", None, "full", "space"),
            row("Enfant", None, "full", "private"),
            row("Petit-enfant", Some("Enfant"), "full", "private"),
        ]
    );
    let node = team.store.get_node(child).await.unwrap().expect("la page");
    assert_eq!(node.updated_by, Some(mona.id));
    // Devenue la page privée de Mona, sa sous-page avec elle : la propriétaire ne les voit
    // plus.
    assert_eq!(
        seen(olga, team.space).await,
        [row("Commune", None, "full", "space")]
    );
    for id in [child, grandchild] {
        olga.browser
            .get(&page_uri(id))
            .await
            .assert_error(StatusCode::NOT_FOUND, "not_found");
    }

    // Une page qui porte ses propres accès les garde, et rien ne s'y ajoute.
    let lined = team
        .page(Some(shared), "En lecture", &[members(Level::Read)])
        .await;
    let moved = olga
        .patched(&lined.to_string(), json!({ "parentId": null }))
        .await;
    assert_eq!(moved.parent_id, None);
    assert_eq!(team.grants(lined).await, [members(Level::Read)]);
    let for_tom = tom.browser.get(&page_uri(lined)).await.json();
    assert_eq!(
        (text(&for_tom, "level"), text(&for_tom, "section")),
        ("read", "space")
    );

    // Une page dont les accès propres ne nomment pas qui la déplace : à la racine, il ne la
    // verrait plus. Le déplacement est refusé, et rien ne change.
    let invited = team
        .page(
            Some(shared),
            "Partagée avec Gina",
            &[user(&team.gina, Level::Read)],
        )
        .await;
    let before = team.store.get_node(invited).await.unwrap();
    mona.patch(&invited.to_string(), json!({ "parentId": null }))
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(team.store.get_node(invited).await.unwrap(), before);
    assert_eq!(team.grants(invited).await, [user(&team.gina, Level::Read)]);
}

/// `PATCH` change le titre et la place en une seule écriture : un déplacement refusé (`409
/// cycle`) n'écrit rien, pas même le titre envoyé avec.
#[tokio::test]
async fn patch_title_and_cycle_changes_nothing() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let a = alice.create(json!({ "title": "A" })).await;
    let b = alice
        .create(json!({ "title": "B", "parentId": a.id }))
        .await;
    let c = alice
        .create(json!({ "title": "C", "parentId": b.id }))
        .await;
    let store = Store::open(&t.db_path()).await.unwrap();
    let stored = Uuid::parse_str(&a.id).unwrap();
    let before = (
        alice.tree().await,
        store.get_node(stored).await.unwrap(),
        store.space_rev(alice.space).await.unwrap(),
    );

    // Sous sa petite-fille, sous sa fille, sous elle-même.
    for parent in [&c.id, &b.id, &a.id] {
        alice
            .patch(&a.id, json!({ "title": "Renommée", "parentId": parent }))
            .await
            .assert_error(StatusCode::CONFLICT, "cycle");
        alice
            .patch(
                &a.id,
                json!({ "title": "Renommée", "parentId": parent, "afterId": null }),
            )
            .await
            .assert_error(StatusCode::CONFLICT, "cycle");
    }
    let after = (
        alice.tree().await,
        store.get_node(stored).await.unwrap(),
        store.space_rev(alice.space).await.unwrap(),
    );
    assert_eq!(after, before);
    store.close().await;

    // Les deux ensemble, quand rien ne s'y oppose : une seule écriture.
    let moved = alice
        .patched(
            &c.id,
            json!({ "title": "C, à la racine", "parentId": null }),
        )
        .await;
    assert_eq!(
        (moved.title.as_str(), &moved.parent_id),
        ("C, à la racine", &None)
    );
}

/// Le document d'une page neuve est écrit avec son nœud, dans la même transaction :
/// l'instantané de `seq` 0 est en base avant toute connexion, et la première synchro rend son
/// paragraphe vide sans qu'un second instantané soit écrit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_page_doc_is_written_with_the_node() {
    let t = TestApp::new().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Neuve" })).await;
    let id = Uuid::parse_str(&page.id).unwrap();

    // Avant toute connexion : l'instantané de `seq` 0, sans journal, daté comme le nœud.
    let store = Store::open(&t.db_path()).await.unwrap();
    let doc = store.load_doc(id).await.unwrap();
    let (seq, state) = doc.snapshot.clone().expect("l'instantané de la page neuve");
    assert_eq!(seq, 0);
    assert_eq!(doc.updates, []);
    let written_at = async |table: &str, key: &str| {
        let sql = format!("SELECT created_at FROM {table} WHERE {key} = '{id}'");
        query_i64(&t.db_path(), &sql).await
    };
    let snapshot_at = written_at("doc_snapshots", "node_id").await;
    assert_eq!(snapshot_at, written_at("nodes", "id").await);
    // C'est le document que l'acteur de la page écrirait : un paragraphe vide, au schéma
    // courant.
    let stored = PageDoc::from_updates([state.as_slice()]).expect("un document lisible");
    assert_eq!(stored.schema_version(), Some(SCHEMA_VERSION));
    let blocks = stored.read_blocks();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].kind, "paragraph");
    assert_eq!(mementum_md::block_text(&blocks[0]), "");

    // La première synchro rend ce paragraphe-là, et n'écrit rien.
    let addr = t.listen().await;
    let url = format!("ws://{addr}/sync/{id}?schema={SCHEMA_VERSION}");
    let client = Client::connect_synced(&url).await;
    let synced = client.blocks();
    assert_eq!(synced.len(), 1);
    assert_eq!(
        synced[0].id, blocks[0].id,
        "le même bloc, pas une seconde racine"
    );
    assert_eq!(alice.get_mmd(&page.id).await.text(), "<p/>\n");
    assert_eq!(store.load_doc(id).await.unwrap(), doc);
    assert_eq!(written_at("doc_snapshots", "node_id").await, snapshot_at);
    store.close().await;
}

/// Les écritures portent leur auteur : qui a créé, qui a modifié en dernier, qui a écrit le
/// contenu, qui a mis à la corbeille.
#[tokio::test]
async fn writes_carry_their_author() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let (mona, tom) = (&team.mona, &team.tom);
    let space = team.space.to_string();
    let reply = mona
        .browser
        .post(
            "/api/v1/pages",
            json!({ "spaceId": space, "section": "space" }),
        )
        .await;
    reply.assert_status(StatusCode::CREATED);
    let page = uuid(&reply.json()["id"]);
    let id = page.to_string();
    let count = async |table: &str, condition: String| {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE {condition}");
        query_i64(&t.db_path(), &sql).await
    };
    let node = team.store.get_node(page).await.unwrap().expect("la page");
    assert_eq!(
        (node.created_by, node.updated_by),
        (Some(mona.id), Some(mona.id))
    );

    // Tom la renomme, puis la déplace.
    tom.patched(&id, json!({ "title": "Renommée par Tom" }))
        .await;
    let node = team.store.get_node(page).await.unwrap().expect("la page");
    assert_eq!(
        (node.created_by, node.updated_by),
        (Some(mona.id), Some(tom.id))
    );
    mona.patched(&id, json!({ "afterId": null })).await;
    let node = team.store.get_node(page).await.unwrap().expect("la page");
    assert_eq!(node.updated_by, Some(mona.id));

    // Tom en écrit le contenu.
    tom.put_mmd(&id, "Bonjour\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let by_tom = format!("node_id = '{page}' AND actor_id = '{}'", tom.id);
    assert_eq!(count("doc_updates", by_tom).await, 1);
    let by_others = format!(
        "node_id = '{page}' AND (actor_id IS NULL OR actor_id != '{}')",
        tom.id
    );
    assert_eq!(count("doc_updates", by_others).await, 0);

    // Tom la met à la corbeille ; restaurée, plus personne ne l'y a mise.
    tom.browser
        .post_empty(&format!("{}/trash", page_uri(page)))
        .await
        .assert_status(StatusCode::OK);
    let trashed_by_tom = format!("id = '{page}' AND trashed_by = '{}'", tom.id);
    assert_eq!(count("nodes", trashed_by_tom).await, 1);
    mona.browser
        .post_empty(&format!("{}/restore", page_uri(page)))
        .await
        .assert_status(StatusCode::OK);
    let untouched = format!("id = '{page}' AND trashed_by IS NULL AND trashed_at IS NULL");
    assert_eq!(count("nodes", untouched).await, 1);
}
