//! Membres d'un espace : dernier propriétaire, rôles qui ne baissent pas à l'ajout, départ d'un
//! membre et ses accès, adoption des espaces sans membre, révision.

mod common;

use common::{account, break_table, fresh, int, page, repair};
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_store::{
    InvitationGrant, Member, NewInvitation, NewNode, Node, Space, Store, StoreError,
};
use uuid::Uuid;

async fn rev(store: &Store, space: Uuid) -> i64 {
    store.space_rev(space).await.unwrap().expect("espace")
}

fn grant(subject: Subject, level: Level) -> Grant {
    Grant { subject, level }
}

/// Une page sans auteur ni document, avec ces accès.
async fn page_with(
    store: &Store,
    space: &Space,
    parent: Option<Uuid>,
    title: &str,
    grants: Vec<Grant>,
) -> Node {
    store
        .create_node(NewNode {
            grants,
            ..page(space, parent, None, title)
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn last_owner_cannot_leave_or_be_demoted() {
    let f = fresh().await;
    let store = &f.store;
    let olga = account(store, "olga").await;
    let paul = account(store, "paul").await;
    let space = store.create_space("Équipe", Some(olga.id)).await.unwrap();
    let before = rev(store, space.id).await;

    for role in [Role::Admin, Role::Member, Role::Guest] {
        let r = store.set_role(space.id, olga.id, role).await;
        assert!(matches!(r, Err(StoreError::LastOwner)), "{role:?} : {r:?}");
    }
    let r = store.remove_member(space.id, olga.id).await;
    assert!(matches!(r, Err(StoreError::LastOwner)), "{r:?}");
    assert_eq!(
        store.role_in(space.id, olga.id).await.unwrap(),
        Some(Role::Owner)
    );
    assert_eq!(rev(store, space.id).await, before, "un refus n'écrit rien");
    // Rester propriétaire n'est pas un départ.
    store
        .set_role(space.id, olga.id, Role::Owner)
        .await
        .unwrap();

    // Avec un second propriétaire, l'un ou l'autre peut partir, pas les deux.
    store
        .add_member(space.id, paul.id, Role::Owner)
        .await
        .unwrap();
    store
        .set_role(space.id, olga.id, Role::Admin)
        .await
        .unwrap();
    let r = store.set_role(space.id, paul.id, Role::Member).await;
    assert!(matches!(r, Err(StoreError::LastOwner)), "{r:?}");
    let r = store.remove_member(space.id, paul.id).await;
    assert!(matches!(r, Err(StoreError::LastOwner)), "{r:?}");
    store
        .set_role(space.id, olga.id, Role::Owner)
        .await
        .unwrap();
    store.remove_member(space.id, paul.id).await.unwrap();
    assert_eq!(store.role_in(space.id, paul.id).await.unwrap(), None);

    // Deux propriétaires qui partent en même temps : l'un des deux reste.
    store
        .add_member(space.id, paul.id, Role::Owner)
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        store.remove_member(space.id, olga.id),
        store.set_role(space.id, paul.id, Role::Member),
    );
    let refused = [&a, &b]
        .iter()
        .filter(|r| matches!(r, Err(StoreError::LastOwner)))
        .count();
    assert!(
        refused == 1 && (a.is_ok() || b.is_ok()),
        "un départ passe, l'autre est refusé : {a:?}, {b:?}"
    );
    assert_eq!(
        int(
            &f.db,
            &format!(
                "SELECT COUNT(*) FROM members WHERE space_id = '{}' AND role = 'owner'",
                space.id
            )
        )
        .await,
        1
    );

    // Hors de l'espace : `NotFound`.
    let stranger = account(store, "inconnu").await;
    for r in [
        store.set_role(space.id, stranger.id, Role::Member).await,
        store.remove_member(space.id, stranger.id).await,
        store.set_role(Uuid::now_v7(), olga.id, Role::Member).await,
        store.remove_member(Uuid::now_v7(), olga.id).await,
    ] {
        assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
    }
}

#[tokio::test]
async fn add_member_never_downgrades() {
    let f = fresh().await;
    let store = &f.store;
    let olga = account(store, "olga").await;
    let gina = account(store, "gina").await;
    let space = store.create_space("Équipe", Some(olga.id)).await.unwrap();

    let (guest, member, admin, owner) = (Role::Guest, Role::Member, Role::Admin, Role::Owner);
    for (asked, kept) in [
        (guest, guest),
        (member, member),
        (guest, member),
        (admin, admin),
        (member, admin),
        (guest, admin),
        (owner, owner),
        (admin, owner),
        (member, owner),
        (guest, owner),
    ] {
        store.add_member(space.id, gina.id, asked).await.unwrap();
        assert_eq!(
            store.role_in(space.id, gina.id).await.unwrap(),
            Some(kept),
            "ajout en {asked:?}"
        );
    }
    // `set_role`, lui, change le rôle dans les deux sens.
    store
        .set_role(space.id, gina.id, Role::Member)
        .await
        .unwrap();
    assert_eq!(
        store.members(space.id).await.unwrap(),
        [
            Member {
                user_id: olga.id,
                name: olga.name.clone(),
                email: olga.email.clone(),
                role: owner,
            },
            Member {
                user_id: gina.id,
                name: gina.name.clone(),
                email: gina.email.clone(),
                role: member,
            },
        ]
    );

    // Les espaces d'un compte, par date de création, avec son rôle dans chacun.
    let other = store.create_space("Autre", None).await.unwrap();
    store.add_member(other.id, gina.id, guest).await.unwrap();
    let space = store.get_space(space.id).await.unwrap().unwrap();
    let other = store.get_space(other.id).await.unwrap().unwrap();
    assert_eq!(
        store.spaces_of(gina.id).await.unwrap(),
        [(space.clone(), member), (other, guest)]
    );
    assert_eq!(
        store.spaces_of(olga.id).await.unwrap(),
        [(space.clone(), owner)]
    );
    assert_eq!(store.spaces_of(Uuid::now_v7()).await.unwrap(), []);

    // Un espace ou un compte inconnu : `NotFound`, rien d'écrit.
    for r in [
        store.add_member(Uuid::now_v7(), gina.id, member).await,
        store.add_member(space.id, Uuid::now_v7(), member).await,
    ] {
        assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
    }
    assert_eq!(store.members(space.id).await.unwrap().len(), 2);
    assert_eq!(store.members(Uuid::now_v7()).await.unwrap(), []);
    assert_eq!(store.role_in(space.id, Uuid::now_v7()).await.unwrap(), None);
}

#[tokio::test]
async fn remove_member_drops_shared_grants_and_keeps_private_pages() {
    let f = fresh().await;
    let (store, db) = (&f.store, &f.db);
    let olga = account(store, "olga").await;
    let alice = account(store, "alice").await;
    let space = store.create_space("Équipe", Some(olga.id)).await.unwrap();
    store
        .add_member(space.id, alice.id, Role::Member)
        .await
        .unwrap();
    let (a, o) = (Subject::User(alice.id), Subject::User(olga.id));

    // R : une page de l'espace ; R1, dessous, porte une ligne à Alice seule, mais hérite de
    // celle des membres.
    let r = page_with(
        store,
        &space,
        None,
        "R",
        vec![grant(Subject::Members, Level::Full)],
    )
    .await;
    let r1 = page_with(store, &space, Some(r.id), "R1", vec![grant(a, Level::Full)]).await;
    // P : une page privée d'Alice, et ses sous-pages.
    let p = page_with(store, &space, None, "P", vec![grant(a, Level::Full)]).await;
    let p1 = page_with(store, &space, Some(p.id), "P1", vec![]).await;
    let p2 = page_with(store, &space, Some(p.id), "P2", vec![grant(a, Level::Edit)]).await;
    // Q : partagée entre Alice et Olga ; Q1, dessous, hérite de la ligne d'Olga.
    let q = page_with(
        store,
        &space,
        None,
        "Q",
        vec![grant(a, Level::Full), grant(o, Level::Edit)],
    )
    .await;
    let q1 = page_with(store, &space, Some(q.id), "Q1", vec![grant(a, Level::Read)]).await;
    // Un autre espace, où Alice reste.
    let elsewhere = store.create_space("Ailleurs", Some(olga.id)).await.unwrap();
    store
        .add_member(elsewhere.id, alice.id, Role::Member)
        .await
        .unwrap();
    let z = page_with(
        store,
        &elsewhere,
        None,
        "Z",
        vec![grant(Subject::Members, Level::Full)],
    )
    .await;
    let z1 = page_with(
        store,
        &elsewhere,
        Some(z.id),
        "Z1",
        vec![grant(a, Level::Edit)],
    )
    .await;

    // Une panne sur le retrait des accès : rien n'est écrit, Alice est toujours membre.
    let before = rev(store, space.id).await;
    break_table(db, "DELETE", "acl", "1").await;
    let res = store.remove_member(space.id, alice.id).await;
    assert!(matches!(res, Err(StoreError::Sqlx(_))), "{res:?}");
    repair(db).await;
    assert_eq!(
        store.role_in(space.id, alice.id).await.unwrap(),
        Some(Role::Member)
    );
    assert_eq!(
        store.node_grants(r1.id).await.unwrap(),
        [grant(a, Level::Full)]
    );
    assert_eq!(rev(store, space.id).await, before);

    store.remove_member(space.id, alice.id).await.unwrap();
    assert_eq!(store.role_in(space.id, alice.id).await.unwrap(), None);
    assert_eq!(store.spaces_of(alice.id).await.unwrap().len(), 1);
    // Ses lignes partent des pages qu'elle partageait, et restent sur ses pages privées.
    for (node, expected) in [
        (&r, vec![grant(Subject::Members, Level::Full)]),
        (&r1, vec![]),
        (&p, vec![grant(a, Level::Full)]),
        (&p1, vec![]),
        (&p2, vec![grant(a, Level::Edit)]),
        (&q, vec![grant(o, Level::Edit)]),
        (&q1, vec![]),
        (&z1, vec![grant(a, Level::Edit)]),
    ] {
        assert_eq!(
            store.node_grants(node.id).await.unwrap(),
            expected,
            "{}",
            node.title
        );
    }
    // Rien n'est supprimé.
    assert_eq!(store.list_nodes(space.id).await.unwrap().len(), 7);
    assert_eq!(rev(store, space.id).await, before + 1);

    // Elle revient : ses pages privées sont là.
    store
        .add_member(space.id, alice.id, Role::Member)
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(p.id).await.unwrap(),
        [grant(a, Level::Full)]
    );
}

#[tokio::test]
async fn adopt_orphan_spaces_takes_only_memberless_spaces() {
    // `fresh` crée « Essais », sans membre.
    let f = fresh().await;
    let store = &f.store;
    let olga = account(store, "olga").await;
    let ugo = account(store, "ugo").await;
    let orphan = store.create_space("Sans personne", None).await.unwrap();
    let owned = store.create_space("À Olga", Some(olga.id)).await.unwrap();

    // Un compte inconnu : rien n'est adopté.
    let r = store.adopt_orphan_spaces(Uuid::now_v7()).await;
    assert!(matches!(r, Err(StoreError::NotFound)), "{r:?}");
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM members").await, 1);

    let adopted = store.adopt_orphan_spaces(ugo.id).await.unwrap();
    // Par date de création, à leur révision nouvelle.
    assert_eq!(
        adopted.iter().map(|s| s.id).collect::<Vec<_>>(),
        [f.space.id, orphan.id]
    );
    assert_eq!(adopted[0].rev, f.space.rev + 1);
    assert_eq!(adopted[1].rev, orphan.rev + 1);
    for space in &adopted {
        assert_eq!(
            store.get_space(space.id).await.unwrap().as_ref(),
            Some(space)
        );
        assert_eq!(
            store.role_in(space.id, ugo.id).await.unwrap(),
            Some(Role::Owner)
        );
    }
    // L'espace qui a un membre n'est pas pris.
    assert_eq!(store.role_in(owned.id, ugo.id).await.unwrap(), None);
    assert_eq!(store.members(owned.id).await.unwrap().len(), 1);
    assert_eq!(rev(store, owned.id).await, owned.rev);

    // Plus d'orphelin : rien à prendre.
    assert_eq!(store.adopt_orphan_spaces(olga.id).await.unwrap(), []);
    assert_eq!(store.spaces_of(olga.id).await.unwrap().len(), 1);
}

/// La règle de `rev` pour `members`, et pour ce que `accept_invitation`, `remove_member` et
/// `revoke_user_grant` écrivent dans `acl` : chaque écriture, un cran ; un appel qui n'écrit
/// rien, aucun.
#[tokio::test]
async fn membership_writes_bump_rev() {
    let f = fresh().await;
    let store = &f.store;
    let olga = account(store, "olga").await;
    let alice = account(store, "alice").await;
    let gina = account(store, "gina").await;
    let hugo = account(store, "hugo").await;
    let space = store.create_space("Équipe", Some(olga.id)).await.unwrap();
    let other = store.create_space("Autre", Some(olga.id)).await.unwrap();
    let page = page_with(
        store,
        &space,
        None,
        "P",
        vec![grant(Subject::Members, Level::Full)],
    )
    .await;
    let mut expected = rev(store, space.id).await;

    store
        .add_member(space.id, alice.id, Role::Member)
        .await
        .unwrap();
    expected += 1;
    assert_eq!(rev(store, space.id).await, expected, "add_member");
    store
        .add_member(space.id, alice.id, Role::Guest)
        .await
        .unwrap();
    assert_eq!(
        rev(store, space.id).await,
        expected,
        "add_member sans effet"
    );

    store
        .set_role(space.id, alice.id, Role::Admin)
        .await
        .unwrap();
    expected += 1;
    assert_eq!(rev(store, space.id).await, expected, "set_role");
    store
        .set_role(space.id, alice.id, Role::Admin)
        .await
        .unwrap();
    assert_eq!(rev(store, space.id).await, expected, "set_role sans effet");

    // Une invitation n'est pas un droit ; l'accepter en est un.
    let t = 1_800_000_000_000;
    let invite = |grant, token| NewInvitation {
        space_id: space.id,
        grant,
        email: "invite@exemple.test".to_string(),
        token_hash: token,
        created_by: olga.id,
        expires_at: t + 1,
    };
    store
        .create_invitation(invite(InvitationGrant::Space(Role::Member), [1; 32]))
        .await
        .unwrap();
    assert_eq!(rev(store, space.id).await, expected, "create_invitation");
    store.accept_invitation([1; 32], gina.id, t).await.unwrap();
    expected += 1;
    assert_eq!(
        rev(store, space.id).await,
        expected,
        "accept_invitation, espace"
    );
    // Une ligne d'accès et un invité de plus : une seule écriture.
    let to_page = InvitationGrant::Page {
        node_id: page.id,
        level: Level::Edit,
    };
    store
        .create_invitation(invite(to_page, [2; 32]))
        .await
        .unwrap();
    store.accept_invitation([2; 32], hugo.id, t).await.unwrap();
    expected += 1;
    assert_eq!(
        rev(store, space.id).await,
        expected,
        "accept_invitation, page"
    );

    // La ligne retirée et l'invité sorti : une seule écriture.
    store.revoke_user_grant(page.id, hugo.id).await.unwrap();
    expected += 1;
    assert_eq!(rev(store, space.id).await, expected, "revoke_user_grant");
    assert_eq!(store.role_in(space.id, hugo.id).await.unwrap(), None);
    store.revoke_user_grant(page.id, hugo.id).await.unwrap();
    assert_eq!(
        rev(store, space.id).await,
        expected,
        "revoke_user_grant sans effet"
    );

    store.remove_member(space.id, alice.id).await.unwrap();
    expected += 1;
    assert_eq!(rev(store, space.id).await, expected, "remove_member");

    // Les refus n'écrivent rien.
    let refused = [
        store.set_role(space.id, olga.id, Role::Member).await,
        store.remove_member(space.id, alice.id).await,
        store
            .accept_invitation([1; 32], alice.id, t)
            .await
            .map(drop),
    ];
    assert!(refused.iter().all(Result::is_err), "{refused:?}");
    assert_eq!(rev(store, space.id).await, expected, "refus");

    // L'adoption : un cran pour chaque espace adopté.
    let orphan = store.create_space("Orphelin", None).await.unwrap();
    store.adopt_orphan_spaces(olga.id).await.unwrap();
    assert_eq!(
        rev(store, orphan.id).await,
        orphan.rev + 1,
        "adopt_orphan_spaces"
    );
    assert_eq!(
        rev(store, f.space.id).await,
        f.space.rev + 1,
        "adopt_orphan_spaces"
    );

    // Rien de tout cela n'a touché l'espace d'à côté.
    assert_eq!(rev(store, other.id).await, other.rev);
    assert_eq!(rev(store, space.id).await, expected);
}
