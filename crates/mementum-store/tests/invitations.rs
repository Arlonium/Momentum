//! Invitations : usage unique, échéance, révocation ; invitation d'espace et invitation de page,
//! qui ne baissent jamais un rôle ni un niveau ; l'invité qui perd son dernier accès sort de
//! l'espace.

mod common;

use std::slice;
use std::time::{SystemTime, UNIX_EPOCH};

use common::{account, break_table, fresh, int, page, repair, side_pool};
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_store::{
    Invitation, InvitationGrant, NewInvitation, NewNode, Node, Space, Store, StoreError,
};
use sqlx::SqlitePool;
use uuid::Uuid;

const DAY: i64 = 86_400_000;

/// L'heure du poste, en millisecondes Unix : `pending_invitations` lit celle du serveur.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

async fn rev(store: &Store, space: Uuid) -> i64 {
    store.space_rev(space).await.unwrap().expect("espace")
}

fn grant(subject: Subject, level: Level) -> Grant {
    Grant { subject, level }
}

/// Une invitation de `by`, sur l'espace ou sur une page, qui expire dans une semaine.
fn invitation(space: &Space, grant: InvitationGrant, by: Uuid, token: u8) -> NewInvitation {
    NewInvitation {
        space_id: space.id,
        grant,
        email: "invite@exemple.test".to_string(),
        token_hash: [token; 32],
        created_by: by,
        expires_at: now() + 7 * DAY,
    }
}

fn to_space(role: Role) -> InvitationGrant {
    InvitationGrant::Space(role)
}

fn to_page(node: &Node, level: Level) -> InvitationGrant {
    InvitationGrant::Page {
        node_id: node.id,
        level,
    }
}

/// Crée l'invitation et l'accepte pour `user`.
async fn invite_and_accept(
    store: &Store,
    i: NewInvitation,
    user: Uuid,
) -> Result<Invitation, StoreError> {
    let token = i.token_hash;
    store.create_invitation(i).await.unwrap();
    store.accept_invitation(token, user, now()).await
}

/// Une page de l'espace, ouverte aux membres.
async fn space_page(store: &Store, space: &Space, parent: Option<Uuid>, title: &str) -> Node {
    let grants = match parent {
        None => vec![grant(Subject::Members, Level::Full)],
        Some(_) => vec![],
    };
    store
        .create_node(NewNode {
            grants,
            ..page(space, parent, None, title)
        })
        .await
        .unwrap()
}

/// Les accès que `user` a dans l'espace : `(nœud, niveau)`.
async fn lines_of(store: &Store, space: &Space, user: Uuid) -> Vec<(Uuid, Level)> {
    store
        .space_grants(space.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|(_, g)| g.subject == Subject::User(user))
        .map(|(node, g)| (node, g.level))
        .collect()
}

/// Écrit une invitation en SQL direct, sans passer par le store.
async fn insert_invitation(
    pool: &SqlitePool,
    (space, node): (Uuid, Option<Uuid>),
    (role, level): (Option<&str>, Option<&str>),
    by: Uuid,
    token: u8,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO invitations (id, space_id, node_id, email, grant_role, grant_level, \
                                  token_hash, created_by, created_at, expires_at) \
         VALUES (?, ?, ?, 'x@exemple.test', ?, ?, ?, ?, 1, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(space.to_string())
    .bind(node.map(|n| n.to_string()))
    .bind(role)
    .bind(level)
    .bind(&[token; 32][..])
    .bind(by.to_string())
    .bind(i64::MAX)
    .execute(pool)
    .await
    .map(drop)
}

/// Le schéma refuse un rôle ou un niveau inconnu dans une invitation, comme dans `members` et
/// `acl` : sinon une valeur fausse n'échouerait qu'à l'acceptation.
#[tokio::test]
async fn invitation_grant_values_are_checked() {
    let f = fresh().await;
    let store = &f.store;
    let olga = account(store, "olga").await;
    let p = space_page(store, &f.space, None, "P").await;
    let (to_space, to_page) = ((f.space.id, None), (f.space.id, Some(p.id)));
    let pool = side_pool(&f.db).await;

    for (target, grant) in [
        (to_space, (Some("superadmin"), None)),
        (to_space, (Some("Member"), None)),
        (to_space, (Some(""), None)),
        (to_page, (None, Some("admin"))),
        (to_page, (None, Some("Edit"))),
    ] {
        let r = insert_invitation(&pool, target, grant, olga.id, 1).await;
        assert!(
            matches!(&r, Err(sqlx::Error::Database(e)) if e.is_check_violation()),
            "{grant:?} : {r:?}"
        );
    }
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM invitations").await, 0);

    // Les formes que le store écrit passent, et se relisent.
    insert_invitation(&pool, to_space, (Some("member"), None), olga.id, 1)
        .await
        .unwrap();
    insert_invitation(&pool, to_page, (None, Some("edit")), olga.id, 2)
        .await
        .unwrap();
    pool.close().await;
    let grants: Vec<_> = store
        .pending_invitations(f.space.id)
        .await
        .unwrap()
        .into_iter()
        .map(|i| i.grant)
        .collect();
    assert_eq!(
        grants,
        [
            InvitationGrant::Space(Role::Member),
            InvitationGrant::Page {
                node_id: p.id,
                level: Level::Edit
            }
        ]
    );
}

#[tokio::test]
async fn invitation_is_single_use_and_expires() {
    let f = fresh().await;
    let store = &f.store;
    let olga = account(store, "olga").await;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let space = store.create_space("Équipe", Some(olga.id)).await.unwrap();
    let p = space_page(store, &space, None, "P").await;
    let t = now();

    // Acceptée une fois, puis plus jamais.
    let mut new = invitation(&space, to_space(Role::Member), olga.id, 1);
    new.email = "  Alice@Exemple.TEST ".to_string();
    // Le hachage du jeton ne sort pas par `Debug` (une invitation écrite dans un journal).
    let shown = format!("{new:?}");
    assert!(!shown.contains("1, 1, 1"), "{shown}");
    let inv = store.create_invitation(new.clone()).await.unwrap();
    assert_eq!(inv.email, "alice@exemple.test", "rognée, en minuscules");
    assert_eq!(
        (inv.space_id, inv.grant, inv.created_by, inv.expires_at),
        (space.id, to_space(Role::Member), olga.id, new.expires_at)
    );
    assert!(inv.created_at >= t);
    assert_eq!(
        store
            .invitation_by_token([1; 32], t)
            .await
            .unwrap()
            .as_ref(),
        Some(&inv)
    );
    assert_eq!(
        store.pending_invitations(space.id).await.unwrap(),
        slice::from_ref(&inv)
    );
    assert_eq!(
        store.accept_invitation([1; 32], alice.id, t).await.unwrap(),
        inv
    );
    assert_eq!(
        store.role_in(space.id, alice.id).await.unwrap(),
        Some(Role::Member)
    );
    for who in [bob.id, alice.id] {
        let r = store.accept_invitation([1; 32], who, t).await;
        assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
    }
    assert_eq!(store.invitation_by_token([1; 32], t).await.unwrap(), None);
    assert_eq!(store.pending_invitations(space.id).await.unwrap(), []);

    // Échue : valable jusqu'à son échéance, exclue.
    let mut new = invitation(&space, to_space(Role::Admin), olga.id, 2);
    new.expires_at = t - 1;
    let expired = store.create_invitation(new).await.unwrap();
    assert_eq!(
        store.invitation_by_token([2; 32], t - 2).await.unwrap(),
        Some(expired)
    );
    assert_eq!(
        store.invitation_by_token([2; 32], t - 1).await.unwrap(),
        None
    );
    for at in [t - 1, t] {
        let r = store.accept_invitation([2; 32], bob.id, at).await;
        assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
    }
    assert_eq!(store.pending_invitations(space.id).await.unwrap(), []);

    // Révoquée.
    let revoked = store
        .create_invitation(invitation(&space, to_page(&p, Level::Edit), olga.id, 3))
        .await
        .unwrap();
    assert_eq!(
        store.pending_invitations(space.id).await.unwrap(),
        slice::from_ref(&revoked)
    );
    store.revoke_invitation(revoked.id).await.unwrap();
    assert_eq!(store.invitation_by_token([3; 32], t).await.unwrap(), None);
    let r = store.accept_invitation([3; 32], bob.id, t).await;
    assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
    assert_eq!(store.pending_invitations(space.id).await.unwrap(), []);
    // Une seconde fois, ou une invitation déjà acceptée (c'est un droit, qui se retire
    // autrement) : `NotFound`.
    for id in [revoked.id, inv.id, Uuid::now_v7()] {
        let r = store.revoke_invitation(id).await;
        assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
    }

    // Un jeton inconnu.
    let r = store.accept_invitation([4; 32], bob.id, t).await;
    assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
    assert_eq!(store.invitation_by_token([4; 32], t).await.unwrap(), None);

    // Bob n'a rien reçu.
    assert_eq!(store.role_in(space.id, bob.id).await.unwrap(), None);
    assert_eq!(lines_of(store, &space, bob.id).await, []);
}

#[tokio::test]
async fn page_invitation_adds_a_guest_with_one_grant() {
    let f = fresh().await;
    let (store, db) = (&f.store, &f.db);
    let olga = account(store, "olga").await;
    let gina = account(store, "gina").await;
    let space = store.create_space("Équipe", Some(olga.id)).await.unwrap();
    let r = space_page(store, &space, None, "R").await;
    let r1 = space_page(store, &space, Some(r.id), "R1").await;
    space_page(store, &space, None, "Q").await;
    let elsewhere = store.create_space("Ailleurs", Some(olga.id)).await.unwrap();
    let z = space_page(store, &elsewhere, None, "Z").await;

    // Une page d'un autre espace, une page, un espace ou un auteur inconnus : `NotFound`.
    let mut unknown_space = invitation(&space, to_space(Role::Member), olga.id, 9);
    unknown_space.space_id = Uuid::now_v7();
    let mut unknown_page = invitation(&space, to_page(&r1, Level::Read), olga.id, 9);
    unknown_page.grant = InvitationGrant::Page {
        node_id: Uuid::now_v7(),
        level: Level::Read,
    };
    for bad in [
        invitation(&space, to_page(&z, Level::Read), olga.id, 9),
        unknown_page,
        unknown_space,
        invitation(&space, to_page(&r1, Level::Read), Uuid::now_v7(), 9),
    ] {
        let res = store.create_invitation(bad).await;
        assert!(matches!(res, Err(StoreError::NotFound)), "{res:?}");
    }
    assert_eq!(int(db, "SELECT COUNT(*) FROM invitations").await, 0);

    let inv = store
        .create_invitation(invitation(&space, to_page(&r1, Level::Edit), olga.id, 1))
        .await
        .unwrap();
    assert_eq!(inv.grant, to_page(&r1, Level::Edit));

    // Une panne sur la dernière écriture (l'invitation marquée acceptée) : rien ne reste.
    let before = rev(store, space.id).await;
    break_table(db, "UPDATE", "invitations", "1").await;
    let res = store.accept_invitation([1; 32], gina.id, now()).await;
    assert!(matches!(res, Err(StoreError::Sqlx(_))), "{res:?}");
    repair(db).await;
    assert_eq!(store.role_in(space.id, gina.id).await.unwrap(), None);
    assert_eq!(lines_of(store, &space, gina.id).await, []);
    assert_eq!(rev(store, space.id).await, before);
    assert_eq!(
        store.pending_invitations(space.id).await.unwrap(),
        slice::from_ref(&inv)
    );

    assert_eq!(
        store
            .accept_invitation([1; 32], gina.id, now())
            .await
            .unwrap(),
        inv
    );
    // Invitée dans l'espace, avec une seule ligne : celle de la page, au niveau donné.
    assert_eq!(
        store.role_in(space.id, gina.id).await.unwrap(),
        Some(Role::Guest)
    );
    assert_eq!(
        lines_of(store, &space, gina.id).await,
        [(r1.id, Level::Edit)]
    );
    let space_now = store.get_space(space.id).await.unwrap().unwrap();
    assert_eq!(
        store.spaces_of(gina.id).await.unwrap(),
        [(space_now, Role::Guest)]
    );
    assert_eq!(rev(store, space.id).await, before + 1);
    // Rien ailleurs.
    assert_eq!(store.role_in(elsewhere.id, gina.id).await.unwrap(), None);
    assert_eq!(lines_of(store, &elsewhere, gina.id).await, []);
}

#[tokio::test]
async fn page_invitation_keeps_a_member_a_member() {
    let f = fresh().await;
    let store = &f.store;
    let olga = account(store, "olga").await;
    let adam = account(store, "adam").await;
    let mona = account(store, "mona").await;
    let space = store.create_space("Équipe", Some(olga.id)).await.unwrap();
    store
        .add_member(space.id, adam.id, Role::Admin)
        .await
        .unwrap();
    store
        .add_member(space.id, mona.id, Role::Member)
        .await
        .unwrap();
    let p = space_page(store, &space, None, "P").await;

    for (who, role, level, token) in [
        (&mona, Role::Member, Level::Read, 1),
        (&adam, Role::Admin, Level::Comment, 2),
        (&olga, Role::Owner, Level::Edit, 3),
    ] {
        invite_and_accept(
            store,
            invitation(&space, to_page(&p, level), olga.id, token),
            who.id,
        )
        .await
        .unwrap();
        assert_eq!(
            store.role_in(space.id, who.id).await.unwrap(),
            Some(role),
            "{}",
            who.name
        );
        assert_eq!(lines_of(store, &space, who.id).await, [(p.id, level)]);
    }
}

#[tokio::test]
async fn accept_never_lowers_level_or_role() {
    let f = fresh().await;
    let store = &f.store;
    let olga = account(store, "olga").await;
    let adam = account(store, "adam").await;
    let gina = account(store, "gina").await;
    let space = store.create_space("Équipe", Some(olga.id)).await.unwrap();
    store
        .add_member(space.id, adam.id, Role::Admin)
        .await
        .unwrap();
    store
        .add_member(space.id, gina.id, Role::Guest)
        .await
        .unwrap();
    let p = space_page(store, &space, None, "P").await;
    let q = space_page(store, &space, None, "Q").await;
    let a = Subject::User(adam.id);
    store.set_grant(p.id, grant(a, Level::Full)).await.unwrap();
    store.set_grant(q.id, grant(a, Level::Read)).await.unwrap();

    // Rôles : rien ne baisse, sans rien écrire ; un invité devient membre.
    let before = rev(store, space.id).await;
    for (who, asked, token) in [(&adam, Role::Member, 1), (&olga, Role::Admin, 2)] {
        invite_and_accept(
            store,
            invitation(&space, to_space(asked), olga.id, token),
            who.id,
        )
        .await
        .unwrap();
    }
    assert_eq!(
        store.role_in(space.id, adam.id).await.unwrap(),
        Some(Role::Admin)
    );
    assert_eq!(
        store.role_in(space.id, olga.id).await.unwrap(),
        Some(Role::Owner)
    );
    assert_eq!(rev(store, space.id).await, before, "rien d'écrit");
    invite_and_accept(
        store,
        invitation(&space, to_space(Role::Member), olga.id, 3),
        gina.id,
    )
    .await
    .unwrap();
    assert_eq!(
        store.role_in(space.id, gina.id).await.unwrap(),
        Some(Role::Member)
    );

    // Niveaux : `full` reste `full` ; `read` monte à `edit`.
    let before = rev(store, space.id).await;
    invite_and_accept(
        store,
        invitation(&space, to_page(&p, Level::Read), olga.id, 4),
        adam.id,
    )
    .await
    .unwrap();
    assert_eq!(
        lines_of(store, &space, adam.id).await,
        [(p.id, Level::Full), (q.id, Level::Read)]
    );
    assert_eq!(rev(store, space.id).await, before, "rien d'écrit");
    invite_and_accept(
        store,
        invitation(&space, to_page(&q, Level::Edit), olga.id, 5),
        adam.id,
    )
    .await
    .unwrap();
    assert_eq!(
        lines_of(store, &space, adam.id).await,
        [(p.id, Level::Full), (q.id, Level::Edit)]
    );
    assert_eq!(rev(store, space.id).await, before + 1);

    // Même sans effet, l'invitation est consommée.
    for token in [1, 2, 4] {
        let r = store.accept_invitation([token; 32], adam.id, now()).await;
        assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
    }
}

#[tokio::test]
async fn guest_leaves_with_last_grant() {
    let f = fresh().await;
    let store = &f.store;
    let olga = account(store, "olga").await;
    let mona = account(store, "mona").await;
    let gina = account(store, "gina").await;
    let space = store.create_space("Équipe", Some(olga.id)).await.unwrap();
    store
        .add_member(space.id, mona.id, Role::Member)
        .await
        .unwrap();
    let p = space_page(store, &space, None, "P").await;
    let p1 = space_page(store, &space, Some(p.id), "P1").await;
    let q = space_page(store, &space, None, "Q").await;
    // Gina est invitée sur deux pages, et dans un autre espace.
    for (node, token) in [(&p1, 1), (&q, 2)] {
        invite_and_accept(
            store,
            invitation(&space, to_page(node, Level::Read), olga.id, token),
            gina.id,
        )
        .await
        .unwrap();
    }
    let elsewhere = store.create_space("Ailleurs", Some(olga.id)).await.unwrap();
    let z = space_page(store, &elsewhere, None, "Z").await;
    invite_and_accept(
        store,
        invitation(&elsewhere, to_page(&z, Level::Read), olga.id, 3),
        gina.id,
    )
    .await
    .unwrap();
    assert_eq!(
        store.role_in(space.id, gina.id).await.unwrap(),
        Some(Role::Guest)
    );

    // Une ligne de moins : elle reste, par l'autre.
    store.revoke_user_grant(p1.id, gina.id).await.unwrap();
    assert_eq!(
        lines_of(store, &space, gina.id).await,
        [(q.id, Level::Read)]
    );
    assert_eq!(
        store.role_in(space.id, gina.id).await.unwrap(),
        Some(Role::Guest)
    );
    // Retirer une ligne qui n'existe pas n'écrit rien.
    let before = rev(store, space.id).await;
    store.revoke_user_grant(p1.id, gina.id).await.unwrap();
    assert_eq!(rev(store, space.id).await, before);

    // La dernière : elle sort de l'espace, pas de l'autre, où sa ligne ne compte pas ici.
    store.revoke_user_grant(q.id, gina.id).await.unwrap();
    assert_eq!(store.role_in(space.id, gina.id).await.unwrap(), None);
    assert_eq!(lines_of(store, &space, gina.id).await, []);
    assert_eq!(
        store.role_in(elsewhere.id, gina.id).await.unwrap(),
        Some(Role::Guest)
    );
    assert_eq!(store.spaces_of(gina.id).await.unwrap().len(), 1);

    // Un membre qui perd sa dernière ligne reste membre.
    let m = Subject::User(mona.id);
    store.set_grant(p1.id, grant(m, Level::Edit)).await.unwrap();
    store.revoke_user_grant(p1.id, mona.id).await.unwrap();
    assert_eq!(lines_of(store, &space, mona.id).await, []);
    assert_eq!(
        store.role_in(space.id, mona.id).await.unwrap(),
        Some(Role::Member)
    );

    // Un nœud inconnu : `NotFound`.
    let r = store.revoke_user_grant(Uuid::now_v7(), mona.id).await;
    assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
}
