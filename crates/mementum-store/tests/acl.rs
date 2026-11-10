//! Accès posés sur les pages : pose, remplacement, retrait, forme texte des sujets, et le
//! changement d'un accès par le partage d'une page (`change_access`).

mod common;

use common::{account, exec_sql, fresh, new_user, page, side_pool, texts};
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_store::{AccessChange, AccessWritten, Space, Store, StoreError};
use uuid::Uuid;

fn grant(subject: Subject, level: Level) -> Grant {
    Grant { subject, level }
}

#[tokio::test]
async fn set_grant_replaces_level() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let user = Subject::User(Uuid::now_v7());
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    assert_eq!(store.node_grants(a.id).await.unwrap(), []);

    store
        .set_grant(a.id, grant(user, Level::Read))
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [grant(user, Level::Read)]
    );
    // Un second niveau pour le même sujet remplace le premier : une seule ligne.
    store
        .set_grant(a.id, grant(user, Level::Full))
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [grant(user, Level::Full)]
    );
    // Et l'on peut l'abaisser.
    store
        .set_grant(a.id, grant(user, Level::Comment))
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [grant(user, Level::Comment)]
    );
    // Un autre sujet a sa propre ligne.
    store
        .set_grant(a.id, grant(Subject::Members, Level::Edit))
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [
            grant(Subject::Members, Level::Edit),
            grant(user, Level::Comment)
        ]
    );

    assert!(matches!(
        store
            .set_grant(Uuid::now_v7(), grant(user, Level::Read))
            .await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn remove_grant_drops_only_that_line() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let user = Subject::User(Uuid::now_v7());
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(user, Level::Edit))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::Members, Level::Read))
        .await
        .unwrap();

    store.remove_grant(a.id, user).await.unwrap();
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [grant(Subject::Members, Level::Read)]
    );
    // Retirer une ligne qui n'existe pas n'écrit rien.
    let rev = store.space_rev(space.id).await.unwrap();
    store.remove_grant(a.id, user).await.unwrap();
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);
    assert!(matches!(
        store.remove_grant(Uuid::now_v7(), user).await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn space_grants_stay_in_their_space() {
    let f = fresh().await;
    let (store, here) = (&f.store, &f.space);
    let there = store.create_space("Ailleurs", None).await.unwrap();
    let user = Subject::User(Uuid::now_v7());
    let a = store
        .create_node(page(here, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(here, Some(a.id), None, "B"))
        .await
        .unwrap();
    let z = store
        .create_node(page(&there, None, None, "Z"))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::Members, Level::Full))
        .await
        .unwrap();
    store
        .set_grant(b.id, grant(user, Level::Read))
        .await
        .unwrap();
    store
        .set_grant(z.id, grant(user, Level::Full))
        .await
        .unwrap();

    let mut found = store.space_grants(here.id).await.unwrap();
    found.sort_by_key(|(node, _)| *node);
    assert_eq!(
        found,
        [
            (a.id, grant(Subject::Members, Level::Full)),
            (b.id, grant(user, Level::Read)),
        ]
    );
    assert_eq!(
        store.space_grants(there.id).await.unwrap(),
        [(z.id, grant(user, Level::Full))]
    );
    assert_eq!(store.space_grants(Uuid::now_v7()).await.unwrap(), []);
}

#[tokio::test]
async fn subject_text_roundtrip() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    // Un compte réel, ou non : `acl.subject` est un texte, pas une clé étrangère.
    let id = new_user(&f.db).await;
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::User(id), Level::Comment))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::Members, Level::Edit))
        .await
        .unwrap();

    // En base : la forme texte de `mementum-acl`, et elle seule.
    assert_eq!(
        texts(
            &f.db,
            "SELECT subject || ' ' || level FROM acl ORDER BY subject"
        )
        .await,
        [
            Some("members edit".to_string()),
            Some(format!("user:{} comment", id.hyphenated())),
        ]
    );
    // Relue telle quelle.
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [
            grant(Subject::Members, Level::Edit),
            grant(Subject::User(id), Level::Comment),
        ]
    );
    // Et c'est par ce texte qu'une ligne se retire.
    store.remove_grant(a.id, Subject::User(id)).await.unwrap();
    store.remove_grant(a.id, Subject::Members).await.unwrap();
    assert_eq!(texts(&f.db, "SELECT subject FROM acl").await, []);
}

/// Une ligne que `mementum-acl` ne sait pas relire est une erreur, jamais une ligne sautée :
/// un accès ignoré en silence fausserait le calcul des droits.
#[tokio::test]
async fn unreadable_grant_is_an_error() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(space, None, None, "B"))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::Members, Level::Read))
        .await
        .unwrap();

    // Un sujet hors de la forme canonique (uuid en majuscules) : le schéma l'admet.
    let shouted = Uuid::now_v7().hyphenated().to_string().to_uppercase();
    exec_sql(
        &f.db,
        &format!(
            "INSERT INTO acl (node_id, subject, level) VALUES ('{}', 'user:{shouted}', 'edit')",
            a.id
        ),
    )
    .await;
    let r = store.node_grants(a.id).await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
    let r = store.space_grants(space.id).await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
    // La page d'à côté se lit toujours.
    assert_eq!(store.node_grants(b.id).await.unwrap(), []);
    exec_sql(&f.db, "DELETE FROM acl WHERE subject LIKE 'user:%'").await;
    assert_eq!(store.space_grants(space.id).await.unwrap().len(), 1);

    // Un niveau inconnu : le schéma le refuse (`CHECK`) ; il faut le forcer pour l'écrire.
    let unknown_level = "INSERT INTO acl (node_id, subject, level) VALUES (?, 'members', 'admin')";
    let pool = side_pool(&f.db).await;
    let mut conn = pool.acquire().await.unwrap();
    let r = sqlx::query(unknown_level)
        .bind(b.id.to_string())
        .execute(&mut *conn)
        .await;
    assert!(r.is_err(), "niveau inconnu admis par le schéma");
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(unknown_level)
        .bind(b.id.to_string())
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);
    pool.close().await;
    let r = store.node_grants(b.id).await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
    let r = store.space_grants(space.id).await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
}

/// Un espace où Olga est propriétaire, Mona membre et Gina invitée ; Sam a un compte, sans rôle.
struct Team {
    olga: Uuid,
    mona: Uuid,
    gina: Uuid,
    sam: Uuid,
}

async fn team(store: &Store, space: Uuid) -> Team {
    let team = Team {
        olga: account(store, "olga").await.id,
        mona: account(store, "mona").await.id,
        gina: account(store, "gina").await.id,
        sam: account(store, "sam").await.id,
    };
    for (who, role) in [
        (team.olga, Role::Owner),
        (team.mona, Role::Member),
        (team.gina, Role::Guest),
    ] {
        store.add_member(space, who, role).await.unwrap();
    }
    team
}

/// Une page de l'espace, sous `parent`, avec ces lignes.
async fn page_with(
    store: &Store,
    space: &Space,
    parent: Option<Uuid>,
    grants: &[(Subject, Level)],
) -> Uuid {
    let mut new = page(space, parent, None, "P");
    new.grants = grants.iter().map(|&(s, l)| grant(s, l)).collect();
    store.create_node(new).await.unwrap().id
}

/// Ruling R70, dans la transaction de l'écriture : un changement qui laisserait la page sans
/// personne en `full` est refusé, sans rien écrire. Une ligne `members` compte, un `full`
/// hérité aussi ; une ligne restée à quelqu'un sans rôle, non.
#[tokio::test]
async fn change_access_keeps_someone_in_full() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let (olga, mona, sam) = (
        Subject::User(t.olga),
        Subject::User(t.mona),
        Subject::User(t.sam),
    );
    let rev = async || store.space_rev(space.id).await.unwrap().unwrap();
    let refused = async |node: Uuid, change: AccessChange| {
        let before = (store.node_grants(node).await.unwrap(), rev().await);
        let r = store.change_access(node, change, rev().await).await;
        assert!(matches!(r, Err(StoreError::LastFullAccess)), "{r:?}");
        assert_eq!(
            (store.node_grants(node).await.unwrap(), rev().await),
            before
        );
    };

    // La seule ligne `full` ne part pas, et ne baisse pas.
    let shared = page_with(
        store,
        space,
        None,
        &[(olga, Level::Full), (mona, Level::Read)],
    )
    .await;
    refused(shared, AccessChange::User(t.olga, None)).await;
    refused(shared, AccessChange::User(t.olga, Some(Level::Edit))).await;
    // Sam n'a pas de rôle : sa ligne `full` ne compte pas.
    let dormant = page_with(
        store,
        space,
        None,
        &[(olga, Level::Full), (sam, Level::Full)],
    )
    .await;
    refused(dormant, AccessChange::User(t.olga, None)).await;
    // La ligne `members` compte, à tout niveau ; seule, elle ne part pas.
    let open = page_with(store, space, None, &[(Subject::Members, Level::Full)]).await;
    refused(open, AccessChange::Members(None)).await;
    store
        .change_access(open, AccessChange::Members(Some(Level::Read)), rev().await)
        .await
        .unwrap();
    // Sous elle, une page peut perdre sa seule ligne `full`.
    let under = page_with(store, space, Some(open), &[(mona, Level::Full)]).await;
    let written = store
        .change_access(under, AccessChange::User(t.mona, None), rev().await)
        .await
        .unwrap();
    assert!(written.changed && !written.left_space);
    // Un `full` hérité compte aussi.
    let parent = page_with(store, space, None, &[(olga, Level::Full)]).await;
    let child = page_with(store, space, Some(parent), &[(mona, Level::Full)]).await;
    store
        .change_access(
            child,
            AccessChange::User(t.mona, Some(Level::Read)),
            rev().await,
        )
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(child).await.unwrap(),
        [grant(mona, Level::Read)]
    );
    refused(parent, AccessChange::User(t.olga, None)).await;
    // Un autre `full` sur la page : Olga peut partir.
    store
        .change_access(
            shared,
            AccessChange::User(t.mona, Some(Level::Full)),
            rev().await,
        )
        .await
        .unwrap();
    store
        .change_access(shared, AccessChange::User(t.olga, None), rev().await)
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(shared).await.unwrap(),
        [grant(mona, Level::Full)]
    );
}

/// L'écriture n'a lieu que si l'espace est encore à la révision sur laquelle l'appelant a
/// décidé ; sinon `Stale`, et rien n'est écrit. Une ligne déjà ainsi n'écrit rien non plus, et
/// la révision ne bouge pas ; une page inconnue est `NotFound`.
#[tokio::test]
async fn change_access_writes_only_on_the_rev_it_was_decided_on() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let node = page_with(store, space, None, &[(Subject::Members, Level::Full)]).await;
    let rev = store.space_rev(space.id).await.unwrap().unwrap();
    for stale in [rev - 1, rev + 1] {
        let r = store
            .change_access(node, AccessChange::User(t.mona, Some(Level::Edit)), stale)
            .await;
        assert!(matches!(r, Err(StoreError::Stale)), "{r:?}");
    }
    assert_eq!(
        store.node_grants(node).await.unwrap(),
        [grant(Subject::Members, Level::Full)]
    );
    assert_eq!(store.space_rev(space.id).await.unwrap(), Some(rev));

    // Sur la bonne révision : écrit, un cran.
    let written = store
        .change_access(node, AccessChange::User(t.mona, Some(Level::Edit)), rev)
        .await
        .unwrap();
    assert_eq!(
        written,
        AccessWritten {
            changed: true,
            left_space: false
        }
    );
    assert_eq!(store.space_rev(space.id).await.unwrap(), Some(rev + 1));
    // À l'identique : rien d'écrit.
    for change in [
        AccessChange::User(t.mona, Some(Level::Edit)),
        AccessChange::User(t.olga, None),
        AccessChange::Members(Some(Level::Full)),
    ] {
        let written = store.change_access(node, change, rev + 1).await.unwrap();
        assert_eq!(written, AccessWritten::default(), "{change:?}");
    }
    assert_eq!(store.space_rev(space.id).await.unwrap(), Some(rev + 1));
    let r = store
        .change_access(Uuid::now_v7(), AccessChange::Members(None), rev + 1)
        .await;
    assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
}

/// Retirer à un invité sa dernière ligne de l'espace l'en fait sortir ; tant qu'il en garde
/// une, il reste. Un membre reste membre.
#[tokio::test]
async fn change_access_lets_a_bare_guest_leave() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let (gina, mona) = (Subject::User(t.gina), Subject::User(t.mona));
    let first = page_with(
        store,
        space,
        None,
        &[
            (Subject::Members, Level::Full),
            (gina, Level::Read),
            (mona, Level::Edit),
        ],
    )
    .await;
    let second = page_with(
        store,
        space,
        None,
        &[(Subject::Members, Level::Full), (gina, Level::Edit)],
    )
    .await;
    let rev = async || store.space_rev(space.id).await.unwrap().unwrap();

    let written = store
        .change_access(first, AccessChange::User(t.gina, None), rev().await)
        .await
        .unwrap();
    assert!(written.changed && !written.left_space);
    assert_eq!(
        store.role_in(space.id, t.gina).await.unwrap(),
        Some(Role::Guest)
    );
    let written = store
        .change_access(second, AccessChange::User(t.gina, None), rev().await)
        .await
        .unwrap();
    assert!(written.changed && written.left_space);
    assert_eq!(store.role_in(space.id, t.gina).await.unwrap(), None);
    // Un membre sans plus aucune ligne reste membre.
    let written = store
        .change_access(first, AccessChange::User(t.mona, None), rev().await)
        .await
        .unwrap();
    assert!(written.changed && !written.left_space);
    assert_eq!(
        store.role_in(space.id, t.mona).await.unwrap(),
        Some(Role::Member)
    );
}
