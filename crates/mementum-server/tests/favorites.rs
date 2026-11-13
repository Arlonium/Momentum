//! Les favoris d'un compte (Ruling R254) : `PUT` et `DELETE /api/v1/pages/{id}/favorite` posent
//! et retirent le favori du compte de la session, `GET /api/v1/spaces/{id}/favorites` rend ses
//! pages favorites dans l'espace, dans l'ordre où il les a ajoutées.
//!
//! Le cœur de ces tests : la liste ne nomme jamais une page que le compte ne voit plus, ni une
//! page à la corbeille, et rien ne dit qu'une telle page existe encore. Le store garde la ligne
//! (Ruling R268) ; c'est la route qui ne montre que ce que `access.rs` montre, et le favori
//! revient à sa place quand l'accès revient ou que la page est restaurée.
//!
//! Les pages et les rôles de départ sont posés par le store (`Team`, dans `common/mod.rs`) ; ce
//! qu'on teste (favoris, accès retirés et rendus, corbeille, suppression, départ de l'espace)
//! passe par les routes. Qu'un favori n'est pas poussé sur le canal de l'espace se prouve dans
//! `tests/events.rs`, où vit le client du canal (`favoriting_is_not_pushed_to_the_space`).

mod common;

use std::fmt::Display;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::sync::{members, open_app, user};
use common::{Browser, Reply, Team, TestApp, favorite_routes, outcome, query_i64, uuid};
use mementum_acl::{Level, Role};
use serde_json::json;
use uuid::Uuid;

fn favorite_uri(page: impl Display) -> String {
    format!("/api/v1/pages/{page}/favorite")
}

fn favorites_uri(space: impl Display) -> String {
    format!("/api/v1/spaces/{space}/favorites")
}

fn user_uri(page: Uuid, user: Uuid) -> String {
    format!("/api/v1/pages/{page}/access/users/{user}")
}

/// `PUT` ou `DELETE …/favorite`, sans corps : la réponse.
async fn send(by: &Browser<'_>, method: Method, page: impl Display) -> Reply {
    let request = Request::builder()
        .method(method)
        .uri(favorite_uri(page))
        .body(Body::empty())
        .unwrap();
    by.send(request).await
}

/// Aucun favori : le type d'une liste vide, que `assert_eq!` ne devine pas.
const NONE: [Uuid; 0] = [];

/// Pose le favori de `by` sur `page` : `204`, sans corps.
///
/// Les favoris sont rangés par la date où ils ont été posés, à la milliseconde, puis par
/// identifiant (le store, Ruling R268) : deux favoris posés dans la même milliseconde ne
/// garderaient pas l'ordre de leurs requêtes. Aucune main ne va si vite ; un test le pourrait,
/// sur un poste ou un build assez rapide. L'aide attend donc que l'horloge ait changé de
/// milliseconde, pour que l'ordre affirmé ne tienne pas au minutage : une condition, pas une
/// durée.
async fn add(by: &Browser<'_>, page: Uuid) {
    let reply = send(by, Method::PUT, page).await;
    reply.assert_status(StatusCode::NO_CONTENT);
    assert!(reply.body.is_empty(), "{}", reply.text());
    next_millisecond().await;
}

/// Rend la main jusqu'à ce que l'horloge ait changé de milliseconde (voir [`add`]).
async fn next_millisecond() {
    let now = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    };
    let posed = now();
    while now() == posed {
        tokio::task::yield_now().await;
    }
}

/// Retire le favori de `by` sur `page` : `204`, sans corps.
async fn remove(by: &Browser<'_>, page: Uuid) {
    let reply = send(by, Method::DELETE, page).await;
    reply.assert_status(StatusCode::NO_CONTENT);
    assert!(reply.body.is_empty(), "{}", reply.text());
}

/// `GET …/favorites` : `200`, et exactement `{ "pages": [...] }`.
async fn listed(by: &Browser<'_>, space: Uuid) -> Reply {
    let reply = by.get(&favorites_uri(space)).await;
    reply.assert_status(StatusCode::OK);
    let body = reply.json();
    let pages = body["pages"]
        .as_array()
        .unwrap_or_else(|| panic!("« pages » en liste attendu dans {body}"));
    assert_eq!(body, json!({ "pages": pages }), "rien d'autre que la liste");
    reply
}

/// Les favoris de `by` dans l'espace, dans l'ordre de la réponse.
async fn favorites(by: &Browser<'_>, space: Uuid) -> Vec<Uuid> {
    let body = listed(by, space).await.json();
    body["pages"].as_array().unwrap().iter().map(uuid).collect()
}

/// Le nombre de favoris en base, tous comptes confondus : ce que la route ne montre pas y est
/// encore.
async fn rows(t: &TestApp) -> i64 {
    query_i64(&t.db_path(), "SELECT COUNT(*) FROM favorites").await
}

/// Le favori de `page` répond à `by` exactement comme celui d'une page qui n'existe pas, pour le
/// poser comme pour le retirer : même statut, même type, même corps.
async fn assert_like_missing(by: &Browser<'_>, page: Uuid, what: &str) {
    let unknown = Uuid::now_v7().to_string();
    let routes = favorite_routes(&page.to_string());
    for ((name, request), (_, missing)) in routes.into_iter().zip(favorite_routes(&unknown)) {
        let reply = by.send(request).await;
        let body = reply.assert_error(StatusCode::NOT_FOUND, "not_found");
        assert_eq!(body["message"], "page introuvable", "{name}, {what}");
        let missing = by.send(missing).await;
        assert_eq!(outcome(&reply), outcome(&missing), "{name}, {what}");
    }
}

/// La liste des favoris de `space` répond à `by` exactement comme celle d'un espace qui
/// n'existe pas.
async fn assert_space_like_missing(by: &Browser<'_>, space: impl Display, what: &str) {
    let reply = by.get(&favorites_uri(space)).await;
    let body = reply.assert_error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(body["message"], "espace introuvable", "{what}");
    let missing = by.get(&favorites_uri(Uuid::now_v7())).await;
    assert_eq!(outcome(&reply), outcome(&missing), "{what}");
}

/// Chacun a ses favoris, rendus dans l'ordre où il les a ajoutés : ni celui où les pages ont été
/// créées, ni celui de leurs titres. Poser un favori déjà posé, ou retirer un favori absent,
/// répond `204` et n'écrit rien : ni une ligne, ni sa date, ni sa place dans la liste.
#[tokio::test]
async fn favorite_is_personal_and_listed_in_the_order_added() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let a = team.page(None, "A", &[members(Level::Read)]).await;
    let b = team.page(None, "B", &[members(Level::Read)]).await;
    let c = team.page(None, "C", &[members(Level::Read)]).await;
    let (mona, tom, olga) = (&team.mona.browser, &team.tom.browser, &team.olga.browser);
    // Sans favori : une liste vide, pas une erreur.
    assert_eq!(
        listed(mona, team.space).await.json(),
        json!({ "pages": [] })
    );

    for page in [c, a, b] {
        add(mona, page).await;
    }
    add(tom, b).await;
    assert_eq!(favorites(mona, team.space).await, [c, a, b]);
    assert_eq!(favorites(tom, team.space).await, [b]);
    assert_eq!(favorites(olga, team.space).await, NONE);
    assert_eq!(rows(&t).await, 4);

    // Déjà posé : `204`, et rien n'est écrit. Récrit, il passerait à la fin de la liste.
    let date_of = async |page: Uuid| {
        let sql = format!(
            "SELECT created_at FROM favorites WHERE user_id = '{}' AND node_id = '{page}'",
            team.mona.id
        );
        query_i64(&t.db_path(), &sql).await
    };
    let posed_at = date_of(c).await;
    add(mona, c).await;
    assert_eq!(favorites(mona, team.space).await, [c, a, b]);
    assert_eq!((rows(&t).await, date_of(c).await), (4, posed_at));

    // Retiré : celui-là seul, et le sien seul.
    remove(mona, a).await;
    assert_eq!(favorites(mona, team.space).await, [c, b]);
    assert_eq!(favorites(tom, team.space).await, [b]);
    // Déjà retiré, ou jamais posé : `204`, et rien ne change, pas même chez un autre.
    remove(mona, a).await;
    remove(tom, c).await;
    assert_eq!(favorites(mona, team.space).await, [c, b]);
    assert_eq!(favorites(tom, team.space).await, [b]);
    assert_eq!(rows(&t).await, 3);

    // Remis, c'est un favori neuf : il vient à la fin.
    add(mona, a).await;
    assert_eq!(favorites(mona, team.space).await, [c, b, a]);
}

/// Un favori que le compte ne voit plus n'est plus listé, et son identifiant ne sort plus ;
/// l'accès rendu, il revient, à sa place et non à la fin. Par trois chemins : la ligne d'une
/// membre retirée, celle d'une invitée, l'accès général fermé. Tant qu'on ne voit pas la page,
/// son favori ne se pose ni ne se retire : la réponse est celle d'une page qui n'existe pas. La
/// ligne, elle, reste en base.
#[tokio::test]
async fn favorite_one_no_longer_sees_is_not_listed_and_comes_back_with_the_access() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let open = team.page(None, "Ouverte", &[members(Level::Read)]).await;
    // La page d'Olga, partagée avec Mona et Gina par une ligne à leur nom.
    let shared = team
        .page(
            None,
            "Partagée",
            &[
                user(&team.olga, Level::Full),
                user(&team.mona, Level::Read),
                user(&team.gina, Level::Read),
            ],
        )
        .await;
    let last = team
        .page(
            None,
            "Dernière",
            &[members(Level::Read), user(&team.gina, Level::Comment)],
        )
        .await;
    let (mona, gina, olga) = (&team.mona.browser, &team.gina.browser, &team.olga.browser);
    for page in [open, shared, last] {
        add(mona, page).await;
    }
    // Une invitée a ses favoris parmi les pages qu'elle voit.
    for page in [last, shared] {
        add(gina, page).await;
    }
    assert_eq!(favorites(mona, team.space).await, [open, shared, last]);
    assert_eq!(favorites(gina, team.space).await, [last, shared]);
    let all = rows(&t).await;
    assert_eq!(all, 5);

    // 1. Olga retire la ligne de Mona.
    let reply = olga.delete(&user_uri(shared, team.mona.id)).await;
    assert!(reply.status.is_success(), "{}", reply.text());
    let reply = listed(mona, team.space).await;
    assert_eq!(
        reply.json(),
        json!({ "pages": [open.to_string(), last.to_string()] })
    );
    assert!(!reply.text().contains(&shared.to_string()));
    assert_like_missing(mona, shared, "ligne retirée").await;
    // Rien n'a été écrit ni retiré : le favori attend en base, et Gina n'a rien perdu.
    assert_eq!(rows(&t).await, all);
    assert_eq!(favorites(gina, team.space).await, [last, shared]);
    // L'accès rendu, à un autre niveau : la page revient entre les deux autres.
    let reply = olga
        .put(
            &user_uri(shared, team.mona.id),
            json!({ "level": "comment" }),
        )
        .await;
    assert!(reply.status.is_success(), "{}", reply.text());
    assert_eq!(favorites(mona, team.space).await, [open, shared, last]);

    // 2. La ligne d'une invitée, qui garde une autre page et reste donc dans l'espace.
    let reply = olga.delete(&user_uri(last, team.gina.id)).await;
    assert!(reply.status.is_success(), "{}", reply.text());
    let reply = listed(gina, team.space).await;
    assert_eq!(reply.json(), json!({ "pages": [shared.to_string()] }));
    assert!(!reply.text().contains(&last.to_string()));
    assert_like_missing(gina, last, "ligne d'une invitée retirée").await;
    let reply = olga
        .put(&user_uri(last, team.gina.id), json!({ "level": "read" }))
        .await;
    assert!(reply.status.is_success(), "{}", reply.text());
    assert_eq!(favorites(gina, team.space).await, [last, shared]);

    // 3. L'accès général fermé : la page n'est plus aux membres.
    let general = format!("/api/v1/pages/{open}/access/general");
    let reply = olga.put(&general, json!({ "level": null })).await;
    assert!(reply.status.is_success(), "{}", reply.text());
    let reply = listed(mona, team.space).await;
    assert_eq!(
        reply.json(),
        json!({ "pages": [shared.to_string(), last.to_string()] })
    );
    assert!(!reply.text().contains(&open.to_string()));
    assert_like_missing(mona, open, "accès général fermé").await;
    let reply = olga.put(&general, json!({ "level": "read" })).await;
    assert!(reply.status.is_success(), "{}", reply.text());
    assert_eq!(favorites(mona, team.space).await, [open, shared, last]);
    assert_eq!(rows(&t).await, all);
}

/// Une page à la corbeille n'est pas listée, qu'elle y soit elle-même ou par une page parente ;
/// restaurée, elle revient à sa place. La ligne reste en base tout du long.
#[tokio::test]
async fn favorite_in_the_trash_is_not_listed_and_comes_back_restored() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let first = team.page(None, "Première", &[members(Level::Edit)]).await;
    let binned = team.page(None, "Jetée", &[members(Level::Edit)]).await;
    let parent = team.page(None, "Parente", &[members(Level::Edit)]).await;
    let child = team.page(Some(parent), "Sous-page", &[]).await;
    let last = team.page(None, "Dernière", &[members(Level::Edit)]).await;
    let mona = &team.mona.browser;
    for page in [first, binned, child, last] {
        add(mona, page).await;
    }
    assert_eq!(
        favorites(mona, team.space).await,
        [first, binned, child, last]
    );
    let moved = async |page: Uuid, to: &str| {
        let reply = mona.post_empty(&format!("/api/v1/pages/{page}/{to}")).await;
        reply.assert_status(StatusCode::OK);
    };

    // Elle-même à la corbeille.
    moved(binned, "trash").await;
    let reply = listed(mona, team.space).await;
    assert_eq!(
        reply.json(),
        json!({ "pages": [first.to_string(), child.to_string(), last.to_string()] })
    );
    assert!(!reply.text().contains(&binned.to_string()));
    // Par sa page parente, qui n'est pas un favori.
    moved(parent, "trash").await;
    let reply = listed(mona, team.space).await;
    assert_eq!(
        reply.json(),
        json!({ "pages": [first.to_string(), last.to_string()] })
    );
    assert!(!reply.text().contains(&child.to_string()));
    // Mona lit toujours ces pages, à la corbeille : ce n'est pas l'accès qui les cache.
    for page in [binned, child] {
        let reply = mona.get(&format!("/api/v1/pages/{page}")).await;
        reply.assert_status(StatusCode::OK);
    }
    assert_eq!(rows(&t).await, 4);

    // Restaurées, dans l'autre ordre : chacune revient à sa place, pas à la fin.
    moved(parent, "restore").await;
    assert_eq!(favorites(mona, team.space).await, [first, child, last]);
    moved(binned, "restore").await;
    assert_eq!(
        favorites(mona, team.space).await,
        [first, binned, child, last]
    );
}

/// La liste est celle d'un espace : un favori d'un autre espace du compte n'y figure pas, et
/// son identifiant n'y sort pas. Un espace où l'on n'a aucun rôle répond comme un espace qui
/// n'existe pas.
#[tokio::test]
async fn favorites_of_another_space_are_not_listed() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let other = team.another_space("Autre").await;
    let here = team.page(None, "Ici", &[members(Level::Read)]).await;
    let there = team
        .page_in(other, None, "Là-bas", &[members(Level::Read)])
        .await;
    // Dans l'espace que son inscription a donné à Mona.
    let own = team.mona.create(json!({ "title": "Chez Mona" })).await;
    let own = Uuid::parse_str(&own.id).unwrap();
    let mona = &team.mona.browser;
    for page in [there, own, here] {
        add(mona, page).await;
    }

    let ids: [(Uuid, Uuid); 3] = [(team.space, here), (other, there), (team.mona.space, own)];
    for (space, page) in ids {
        let reply = listed(mona, space).await;
        assert_eq!(reply.json(), json!({ "pages": [page.to_string()] }));
        for (_, elsewhere) in ids.iter().filter(|(_, elsewhere)| *elsewhere != page) {
            assert!(
                !reply.text().contains(&elsewhere.to_string()),
                "{}",
                reply.text()
            );
        }
    }

    // Zoé n'a de rôle ni dans « Équipe » ni dans « Autre », ni l'admin de l'instance.
    for (who, by) in [("Zoé", &team.zoe.browser), ("Sam", &team.sam.browser)] {
        for space in [team.space.to_string(), other.to_string()] {
            assert_space_like_missing(by, space, who).await;
        }
        assert_space_like_missing(by, "pas-un-uuid", who).await;
    }
}

/// `read` suffit pour poser et retirer un favori, pour un membre comme pour une invitée, et à
/// la corbeille aussi : jamais `403`, jamais `409 trashed`. Qui ne voit pas la page reçoit la
/// réponse d'une page qui n'existe pas, et rien n'est écrit ; sans session, `401` avant tout.
/// La liste demande un rôle dans l'espace, celui d'invitée compris.
#[tokio::test]
async fn favorite_routes_follow_the_levels() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let levels = [Level::Read, Level::Comment, Level::Edit, Level::Full];
    let mut pages = Vec::new();
    for (who, name) in [(&team.tom, "un membre"), (&team.gina, "une invitée")] {
        // Une page par niveau, vivante puis à la corbeille, où `who` n'a que sa ligne.
        let mut posed = Vec::new();
        let mut alive = Vec::new();
        for level in levels {
            for trashed in [false, true] {
                let grants = [user(&team.olga, Level::Full), user(who, level)];
                let page = team.page(None, "Page", &grants).await;
                if trashed {
                    team.trash(page).await;
                } else {
                    alive.push(page);
                }
                let before = rows(&t).await;
                let reply = send(&who.browser, Method::PUT, page).await;
                assert_eq!(
                    reply.status,
                    StatusCode::NO_CONTENT,
                    "PUT par {name}, {level:?}, corbeille : {trashed} : {}",
                    reply.text()
                );
                // Posé pour de bon, à la corbeille aussi.
                assert_eq!(rows(&t).await, before + 1, "{name}, {level:?}, {trashed}");
                next_millisecond().await;
                posed.push(page);
            }
        }
        // La liste ne montre que les pages vivantes.
        assert_eq!(favorites(&who.browser, team.space).await, alive, "{name}");
        for page in &posed {
            let before = rows(&t).await;
            let reply = send(&who.browser, Method::DELETE, page).await;
            assert_eq!(
                reply.status,
                StatusCode::NO_CONTENT,
                "DELETE par {name}, page {page} : {}",
                reply.text()
            );
            assert_eq!(rows(&t).await, before - 1, "{name}, page {page}");
        }
        assert_eq!(favorites(&who.browser, team.space).await, NONE, "{name}");
        pages.extend(posed);
    }
    assert_eq!(rows(&t).await, 0);

    // Qui ne voit pas ces pages (Mona est membre de l'espace, sans ligne sur elles) : la réponse
    // d'une page inconnue, et rien n'est écrit.
    for page in &pages {
        assert_like_missing(&team.mona.browser, *page, "membre sans accès").await;
    }
    assert_eq!(rows(&t).await, 0);

    // Sans session : `401`, que la page ou l'espace existe ou non, bien nommé ou non.
    let nobody = t.browser();
    let unknown = Uuid::now_v7().to_string();
    for id in [pages[0].to_string(), unknown.clone(), "pas-un-uuid".into()] {
        for method in [Method::PUT, Method::DELETE] {
            let reply = send(&nobody, method, &id).await;
            reply.assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
        }
    }
    for space in [team.space.to_string(), unknown, "pas-un-uuid".into()] {
        let reply = nobody.get(&favorites_uri(space)).await;
        reply.assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
    assert_eq!(rows(&t).await, 0);

    // La liste : tout rôle la lit, et n'y lit que les siens.
    let common = team.page(None, "Commune", &[members(Level::Read)]).await;
    add(&team.adam.browser, common).await;
    for (who, mine) in [
        (&team.olga, vec![]),
        (&team.adam, vec![common]),
        (&team.mona, vec![]),
        (&team.gina, vec![]),
    ] {
        assert_eq!(favorites(&who.browser, team.space).await, mine);
    }
    // Sans rôle, admin de l'instance compris : un espace introuvable.
    for (who, by) in [("Sam", &team.sam.browser), ("Zoé", &team.zoe.browser)] {
        assert_space_like_missing(by, team.space, who).await;
    }
}

/// Une page supprimée pour de bon emporte ses favoris (la clé étrangère de la migration `0004`,
/// Ruling R268) : elle n'est plus listée, pour personne, son favori répond comme celui d'une
/// page inconnue, et aucune ligne ne reste. Les autres favoris gardent leur ordre.
#[tokio::test]
async fn favorite_of_a_deleted_page_is_gone() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let a = team.page(None, "A", &[members(Level::Read)]).await;
    let gone = team.page(None, "Supprimée", &[members(Level::Read)]).await;
    let child = team.page(Some(gone), "Sous-page", &[]).await;
    let c = team.page(None, "C", &[members(Level::Read)]).await;
    let (mona, tom, olga) = (&team.mona.browser, &team.tom.browser, &team.olga.browser);
    for page in [a, gone, child, c] {
        add(mona, page).await;
    }
    add(tom, gone).await;
    assert_eq!(rows(&t).await, 5);

    // La propriétaire la met à la corbeille, puis la supprime, avec sa sous-page.
    let reply = olga
        .post_empty(&format!("/api/v1/pages/{gone}/trash"))
        .await;
    reply.assert_status(StatusCode::OK);
    let reply = olga.delete(&format!("/api/v1/pages/{gone}")).await;
    reply.assert_status(StatusCode::NO_CONTENT);

    assert_eq!(favorites(mona, team.space).await, [a, c]);
    assert_eq!(favorites(tom, team.space).await, NONE);
    for page in [gone, child] {
        assert_like_missing(mona, page, "page supprimée").await;
    }
    assert_eq!(rows(&t).await, 2);
}

/// Ruling R280 : les favoris de qui n'a plus de rôle dans l'espace restent en base, sans que
/// rien ne les montre : la liste répond comme celle d'un espace qui n'existe pas, et le favori
/// comme celui d'une page inconnue. S'il revient dans l'espace, il les retrouve, dans leur
/// ordre, parmi les pages qu'il voit. Pour une membre qui part d'elle-même, et pour une invitée
/// qui sort de l'espace avec son dernier accès.
#[tokio::test]
async fn favorites_of_who_left_the_space_come_back_with_the_role() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let a = team.page(None, "A", &[members(Level::Read)]).await;
    let b = team.page(None, "B", &[members(Level::Read)]).await;
    let only = team
        .page(
            None,
            "Pour Gina",
            &[user(&team.olga, Level::Full), user(&team.gina, Level::Read)],
        )
        .await;
    let (mona, gina, olga) = (&team.mona.browser, &team.gina.browser, &team.olga.browser);
    for page in [b, a] {
        add(mona, page).await;
    }
    add(gina, only).await;
    assert_eq!(favorites(mona, team.space).await, [b, a]);
    assert_eq!(favorites(gina, team.space).await, [only]);

    // Mona quitte l'espace ; Gina en sort, son seul accès retiré.
    let reply = mona
        .delete(&format!(
            "/api/v1/spaces/{}/members/{}",
            team.space, team.mona.id
        ))
        .await;
    reply.assert_status(StatusCode::NO_CONTENT);
    let reply = olga.delete(&user_uri(only, team.gina.id)).await;
    assert!(reply.status.is_success(), "{}", reply.text());
    assert_eq!(
        team.store.role_in(team.space, team.gina.id).await.unwrap(),
        None
    );
    for (who, by, pages) in [("Mona", mona, vec![a, b]), ("Gina", gina, vec![only])] {
        assert_space_like_missing(by, team.space, who).await;
        for page in pages {
            assert_like_missing(by, page, who).await;
        }
    }
    // Les lignes attendent en base.
    assert_eq!(rows(&t).await, 3);

    // De retour : les mêmes favoris, dans leur ordre.
    team.store
        .add_member(team.space, team.mona.id, Role::Member)
        .await
        .unwrap();
    assert_eq!(favorites(mona, team.space).await, [b, a]);
    // Gina, de nouveau invitée, ne retrouve le sien qu'avec l'accès à la page.
    team.store
        .add_member(team.space, team.gina.id, Role::Guest)
        .await
        .unwrap();
    assert_eq!(favorites(gina, team.space).await, NONE);
    let reply = olga
        .put(&user_uri(only, team.gina.id), json!({ "level": "read" }))
        .await;
    assert!(reply.status.is_success(), "{}", reply.text());
    assert_eq!(favorites(gina, team.space).await, [only]);
    assert_eq!(rows(&t).await, 3);
}
