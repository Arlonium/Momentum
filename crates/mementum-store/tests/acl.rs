//! Accès posés sur les pages : pose, remplacement, retrait, forme texte des sujets.

mod common;

use common::{exec_sql, fresh, new_user, page, side_pool, texts};
use mementum_acl::{Grant, Level, Subject};
use mementum_store::StoreError;
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
