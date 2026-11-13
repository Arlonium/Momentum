//! Inscription (`Store::sign_up`) : le compte et ce qui vient avec lui (espaces sans membre
//! repris par le premier compte, invitation acceptée, premier espace, session) s'écrivent en
//! une seule transaction. Une panne au milieu ne laisse ni compte sans espace ou sans session,
//! ni adresse prise.

mod common;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use common::tmp::TestDir;
use common::{break_table, fresh, int, page, repair};
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_store::{
    Expected, InvitationGrant, NewInvitation, NewSession, NewUser, Store, StoreError,
};
use uuid::Uuid;

const DAY: i64 = 86_400_000;

/// La durée d'une session du serveur : trente jours.
const TTL: i64 = 30 * DAY;

/// Une session neuve, d'un jeton jamais vu, qui court trente jours.
fn session() -> NewSession {
    let mut token_hash = [0; 32];
    token_hash[..16].copy_from_slice(Uuid::now_v7().as_bytes());
    NewSession {
        token_hash,
        expires_at: now() + TTL,
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn new_user(tag: &str) -> NewUser {
    NewUser {
        email: format!("{tag}@exemple.test"),
        name: tag.to_string(),
        password_hash: "pas-un-hachage".to_string(),
    }
}

/// Le nom du premier espace d'un compte, comme le serveur le donne.
fn space_of(tag: &str) -> String {
    format!("Espace de {tag}")
}

/// Une base neuve, sans aucun espace.
async fn empty() -> (TestDir, PathBuf, Store) {
    let dir = TestDir::new();
    let db = dir.path().join("m.db");
    let store = Store::open(&db).await.unwrap();
    (dir, db, store)
}

/// Une invitation de `by` dans `space`, sur l'espace ou sur une page.
async fn invite(
    store: &Store,
    space: Uuid,
    grant: InvitationGrant,
    by: Uuid,
    token: u8,
    expires_at: i64,
) -> ([u8; 32], Uuid) {
    let hash = [token; 32];
    let invitation = store
        .create_invitation(NewInvitation {
            space_id: space,
            grant,
            email: "invite@exemple.test".to_string(),
            token_hash: hash,
            created_by: by,
            expires_at,
        })
        .await
        .unwrap();
    (hash, invitation.id)
}

/// Le premier compte reprend les espaces sans membre (ceux d'une base du lot 1), et n'en reçoit
/// pas un de plus.
#[tokio::test]
async fn first_sign_up_adopts_orphan_spaces_and_gets_no_other() {
    let f = fresh().await;
    let other = f.store.create_space("Second", None).await.unwrap();

    let signed = f
        .store
        .sign_up(
            new_user("alice"),
            None,
            &space_of("alice"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    assert!(signed.user.instance_admin);
    assert_eq!(signed.user.email, "alice@exemple.test");
    // Les deux espaces, du plus ancien au plus récent, à leur révision nouvelle.
    let adopted: Vec<(Uuid, i64)> = signed.adopted.iter().map(|s| (s.id, s.rev)).collect();
    assert_eq!(
        adopted,
        [(f.space.id, f.space.rev + 1), (other.id, other.rev + 1)]
    );
    assert_eq!(signed.space, None);
    assert_eq!(signed.accepted, None);
    assert!(!signed.invitation_withdrawn);

    let spaces = f.store.spaces_of(signed.user.id).await.unwrap();
    let roles: Vec<(Uuid, Role)> = spaces.iter().map(|(s, r)| (s.id, *r)).collect();
    assert_eq!(roles, [(f.space.id, Role::Owner), (other.id, Role::Owner)]);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM spaces").await, 2);
    // Le compte se relit tel que rendu.
    assert_eq!(
        f.store.get_user(signed.user.id).await.unwrap(),
        Some(signed.user)
    );
}

/// Sans espace à reprendre ni invitation, un compte reçoit le sien, dont il est `owner`. Seul
/// le premier compte adopte.
#[tokio::test]
async fn sign_up_without_a_space_creates_one() {
    let (_dir, db, store) = empty().await;
    let alice = store
        .sign_up(
            new_user("alice"),
            None,
            &space_of("alice"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    assert!(alice.user.instance_admin);
    assert!(alice.adopted.is_empty());
    let space = alice.space.expect("un espace à elle");
    assert_eq!(space.name, "Espace de alice");
    // Une écriture dans `members` : la révision a bougé.
    assert_eq!(space.rev, 1);
    assert_eq!(store.space_rev(space.id).await.unwrap(), Some(1));
    assert_eq!(
        store.role_in(space.id, alice.user.id).await.unwrap(),
        Some(Role::Owner)
    );

    // Un espace sans membre apparu ensuite n'est pas pour le compte suivant.
    let orphan = store.create_space("Orphelin", None).await.unwrap();
    let bob = store
        .sign_up(
            new_user("bob"),
            None,
            &space_of("bob"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    assert!(!bob.user.instance_admin);
    assert!(bob.adopted.is_empty());
    let bob_space = bob.space.expect("un espace à lui");
    assert_eq!(bob_space.name, "Espace de bob");
    assert_ne!(bob_space.id, space.id);
    let spaces = store.spaces_of(bob.user.id).await.unwrap();
    assert_eq!(spaces.len(), 1);
    assert_eq!((spaces[0].0.id, spaces[0].1), (bob_space.id, Role::Owner));
    assert!(store.members(orphan.id).await.unwrap().is_empty());
    assert_eq!(int(&db, "SELECT COUNT(*) FROM spaces").await, 3);
}

/// Avec le jeton d'une invitation qui vaut : elle est acceptée dans la même transaction, et le
/// compte ne reçoit pas d'espace à lui.
#[tokio::test]
async fn sign_up_accepts_its_invitation() {
    let (_dir, db, store) = empty().await;
    let alice = store
        .sign_up(
            new_user("alice"),
            None,
            &space_of("alice"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    let space = alice.space.unwrap();
    let rev = store.space_rev(space.id).await.unwrap().unwrap();

    // Dans l'espace.
    let (token, id) = invite(
        &store,
        space.id,
        InvitationGrant::Space(Role::Member),
        alice.user.id,
        1,
        now() + 7 * DAY,
    )
    .await;
    let bob = store
        .sign_up(
            new_user("bob"),
            Some(token),
            &space_of("bob"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    assert!(!bob.user.instance_admin);
    assert_eq!(bob.accepted.as_ref().map(|i| i.id), Some(id));
    assert!(!bob.invitation_withdrawn);
    assert_eq!(bob.space, None);
    assert!(bob.adopted.is_empty());
    assert_eq!(
        store.role_in(space.id, bob.user.id).await.unwrap(),
        Some(Role::Member)
    );
    assert_eq!(store.spaces_of(bob.user.id).await.unwrap().len(), 1);
    assert_eq!(store.space_rev(space.id).await.unwrap(), Some(rev + 1));
    // Elle ne sert plus.
    assert_eq!(store.invitation_by_token(token, now()).await.unwrap(), None);
    assert_eq!(
        int(
            &db,
            "SELECT COUNT(*) FROM invitations WHERE accepted_at IS NOT NULL"
        )
        .await,
        1
    );

    // Sur une page : le compte entre comme invité, avec son accès sur la page.
    let node = store
        .create_node(page(&space, None, None, "Page"))
        .await
        .unwrap();
    let (token, _) = invite(
        &store,
        space.id,
        InvitationGrant::Page {
            node_id: node.id,
            level: Level::Edit,
        },
        alice.user.id,
        2,
        now() + 7 * DAY,
    )
    .await;
    let carol = store
        .sign_up(
            new_user("carol"),
            Some(token),
            &space_of("carol"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    assert!(carol.accepted.is_some());
    assert_eq!(carol.space, None);
    assert_eq!(
        store.role_in(space.id, carol.user.id).await.unwrap(),
        Some(Role::Guest)
    );
    assert_eq!(
        store.node_grants(node.id).await.unwrap(),
        [Grant {
            subject: Subject::User(carol.user.id),
            level: Level::Edit,
        }]
    );
    assert_eq!(int(&db, "SELECT COUNT(*) FROM spaces").await, 1);
}

/// Une invitation qui ne vaut plus à l'instant de l'écriture (inconnue, échue, révoquée, ou
/// dont l'auteur a perdu son rôle, Ruling R69) : le compte est créé quand même, sans espace ni
/// droit. C'est à l'appelant de vérifier le jeton avant, s'il veut refuser l'inscription.
#[tokio::test]
async fn sign_up_with_a_withdrawn_invitation_makes_an_account_without_space() {
    let (_dir, db, store) = empty().await;
    let alice = store
        .sign_up(
            new_user("alice"),
            None,
            &space_of("alice"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    let dan = store
        .sign_up(
            new_user("dan"),
            None,
            &space_of("dan"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    let space = alice.space.unwrap();
    store
        .add_member(space.id, dan.user.id, Role::Admin)
        .await
        .unwrap();
    let member = InvitationGrant::Space(Role::Member);

    let (expired, _) = invite(&store, space.id, member, alice.user.id, 1, now() - 1).await;
    let (revoked, id) = invite(&store, space.id, member, alice.user.id, 2, now() + DAY).await;
    store.revoke_invitation(id).await.unwrap();
    // Son auteur n'est plus admin de l'espace.
    let (orphaned, _) = invite(&store, space.id, member, dan.user.id, 3, now() + DAY).await;
    let seen = Expected {
        actor: alice.user.id,
        actor_role: Role::Owner,
        target_role: Role::Admin,
    };
    store
        .set_role(space.id, dan.user.id, Role::Member, seen)
        .await
        .unwrap();
    let unknown = [9; 32];

    let rev = store.space_rev(space.id).await.unwrap();
    let spaces = int(&db, "SELECT COUNT(*) FROM spaces").await;
    for (i, token) in [expired, revoked, orphaned, unknown]
        .into_iter()
        .enumerate()
    {
        let tag = format!("bob{i}");
        let bob = store
            .sign_up(
                new_user(&tag),
                Some(token),
                &space_of(&tag),
                session(),
                None,
                now(),
            )
            .await
            .unwrap();
        assert!(bob.invitation_withdrawn, "cas {i}");
        assert_eq!(bob.accepted, None, "cas {i}");
        assert_eq!(bob.space, None, "cas {i}");
        assert!(bob.adopted.is_empty(), "cas {i}");
        assert!(
            store.spaces_of(bob.user.id).await.unwrap().is_empty(),
            "cas {i}"
        );
        assert!(store.get_user(bob.user.id).await.unwrap().is_some());
    }
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);
    assert_eq!(int(&db, "SELECT COUNT(*) FROM spaces").await, spaces);
}

/// Une panne à n'importe quelle étape après la création du compte défait tout : aucun compte,
/// l'adresse reste libre, et l'inscription relancée fait ce qu'elle devait.
#[tokio::test]
async fn sign_up_is_one_transaction() {
    let f = fresh().await;
    let users = "SELECT COUNT(*) FROM users";
    let members = "SELECT COUNT(*) FROM members";

    // Premier compte : la panne tombe à l'adoption de l'espace sans membre.
    break_table(&f.db, "INSERT", "members", "1").await;
    let failed = f
        .store
        .sign_up(
            new_user("alice"),
            None,
            &space_of("alice"),
            session(),
            None,
            now(),
        )
        .await;
    assert!(matches!(failed, Err(StoreError::Sqlx(_))), "{failed:?}");
    assert_eq!(int(&f.db, users).await, 0);
    assert_eq!(int(&f.db, members).await, 0);
    assert_eq!(
        f.store.user_by_email("alice@exemple.test").await.unwrap(),
        None
    );
    repair(&f.db).await;
    // Relancée : elle est encore le premier compte, et l'espace lui revient.
    let alice = f
        .store
        .sign_up(
            new_user("alice"),
            None,
            &space_of("alice"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    assert!(alice.user.instance_admin);
    assert_eq!(alice.adopted.len(), 1);
    assert_eq!(alice.adopted[0].id, f.space.id);

    // Compte suivant, sans invitation : la panne tombe à la création de son espace, puis à
    // son entrée dans cet espace.
    for (event, table) in [("INSERT", "spaces"), ("INSERT", "members")] {
        break_table(&f.db, event, table, "1").await;
        let failed = f
            .store
            .sign_up(
                new_user("bob"),
                None,
                &space_of("bob"),
                session(),
                None,
                now(),
            )
            .await;
        assert!(matches!(failed, Err(StoreError::Sqlx(_))), "{failed:?}");
        assert_eq!(int(&f.db, users).await, 1, "panne sur {table}");
        assert_eq!(int(&f.db, "SELECT COUNT(*) FROM spaces").await, 1);
        assert_eq!(
            f.store.user_by_email("bob@exemple.test").await.unwrap(),
            None
        );
        repair(&f.db).await;
    }
    let bob = f
        .store
        .sign_up(
            new_user("bob"),
            None,
            &space_of("bob"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    assert!(bob.space.is_some());

    // Avec une invitation : la panne tombe au moment de la marquer acceptée, après l'entrée
    // dans l'espace.
    let (token, _) = invite(
        &f.store,
        f.space.id,
        InvitationGrant::Space(Role::Member),
        alice.user.id,
        4,
        now() + DAY,
    )
    .await;
    let rev = f.store.space_rev(f.space.id).await.unwrap();
    break_table(&f.db, "UPDATE", "invitations", "1").await;
    let failed = f
        .store
        .sign_up(
            new_user("carol"),
            Some(token),
            &space_of("carol"),
            session(),
            None,
            now(),
        )
        .await;
    assert!(matches!(failed, Err(StoreError::Sqlx(_))), "{failed:?}");
    assert_eq!(int(&f.db, users).await, 2);
    assert_eq!(f.store.members(f.space.id).await.unwrap().len(), 1);
    assert_eq!(f.store.space_rev(f.space.id).await.unwrap(), rev);
    // L'invitation n'a pas servi : elle vaut encore.
    assert!(
        f.store
            .invitation_by_token(token, now())
            .await
            .unwrap()
            .is_some()
    );
    repair(&f.db).await;
    let carol = f
        .store
        .sign_up(
            new_user("carol"),
            Some(token),
            &space_of("carol"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    assert!(carol.accepted.is_some());
    assert_eq!(
        f.store.role_in(f.space.id, carol.user.id).await.unwrap(),
        Some(Role::Member)
    );

    // Une adresse déjà prise : rien n'est écrit non plus.
    let spaces = int(&f.db, "SELECT COUNT(*) FROM spaces").await;
    let taken = f
        .store
        .sign_up(
            new_user(" Alice"),
            None,
            "Espace en trop",
            session(),
            None,
            now(),
        )
        .await;
    assert!(matches!(taken, Err(StoreError::EmailTaken)), "{taken:?}");
    assert_eq!(int(&f.db, users).await, 3);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM spaces").await, spaces);
}

/// Le compte naît avec sa session : le jeton donné à l'inscription ouvre le compte, jusqu'à
/// l'échéance donnée.
#[tokio::test]
async fn sign_up_opens_its_session() {
    let (_dir, _db, store) = empty().await;
    let opened = session();
    let alice = store
        .sign_up(
            new_user("alice"),
            None,
            &space_of("alice"),
            opened,
            None,
            now(),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .session_user(opened.token_hash, now(), TTL)
            .await
            .unwrap(),
        Some(alice.user.clone())
    );
    assert_eq!(
        store
            .session_user(opened.token_hash, opened.expires_at, TTL)
            .await
            .unwrap(),
        None,
        "échue à son échéance"
    );
    // L'inscription rend les espaces du compte, lus dans sa transaction.
    assert_eq!(alice.spaces, store.spaces_of(alice.user.id).await.unwrap());
    assert_eq!(alice.replaced, None);
}

/// Ruling R265 : la session que portait le navigateur (`replaces`) est supprimée dans la
/// transaction de l'inscription, qui rend son compte, pour que l'appelant ferme ses connexions.
/// Si elle ne peut pas l'être, rien n'est écrit, et elle vit encore. Un jeton inconnu ne
/// remplace rien.
#[tokio::test]
async fn sign_up_closes_the_session_it_replaces() {
    let (_dir, db, store) = empty().await;
    let carried = session();
    let alice = store
        .sign_up(
            new_user("alice"),
            None,
            &space_of("alice"),
            carried,
            None,
            now(),
        )
        .await
        .unwrap();
    let alice_is_served = async || {
        store
            .session_user(carried.token_hash, now(), TTL)
            .await
            .unwrap()
            .map(|u| u.id)
    };

    break_table(&db, "DELETE", "sessions", "1").await;
    let failed = store
        .sign_up(
            new_user("bob"),
            None,
            &space_of("bob"),
            session(),
            Some(carried.token_hash),
            now(),
        )
        .await;
    assert!(matches!(failed, Err(StoreError::Sqlx(_))), "{failed:?}");
    assert_eq!(int(&db, "SELECT COUNT(*) FROM users").await, 1);
    assert_eq!(alice_is_served().await, Some(alice.user.id));
    repair(&db).await;

    let bob = store
        .sign_up(
            new_user("bob"),
            None,
            &space_of("bob"),
            session(),
            Some(carried.token_hash),
            now(),
        )
        .await
        .unwrap();
    assert_eq!(bob.replaced, Some(alice.user.id));
    assert_eq!(alice_is_served().await, None);
    let carol = store
        .sign_up(
            new_user("carol"),
            None,
            &space_of("carol"),
            session(),
            Some([7; 32]),
            now(),
        )
        .await
        .unwrap();
    assert_eq!(carol.replaced, None);
}

/// Report m1 du lot 2 : la session s'ouvre dans la transaction qui crée le compte. Si elle ne
/// peut pas s'ouvrir, rien n'est écrit : ni compte (l'adresse reste libre), ni espace, et
/// l'invitation donnée attend toujours. Avant, l'inscription répondait par une erreur pour un
/// compte pourtant créé, sans session, adresse prise et invitation consommée : qui recommençait
/// se voyait répondre que l'adresse était prise.
#[tokio::test]
async fn sign_up_that_cannot_open_its_session_creates_nothing() {
    let (_dir, db, store) = empty().await;
    let alice = store
        .sign_up(
            new_user("alice"),
            None,
            &space_of("alice"),
            session(),
            None,
            now(),
        )
        .await
        .unwrap();
    let space = alice.space.unwrap();
    let (token, _) = invite(
        &store,
        space.id,
        InvitationGrant::Space(Role::Member),
        alice.user.id,
        1,
        now() + DAY,
    )
    .await;
    let rev = store.space_rev(space.id).await.unwrap();
    let users = "SELECT COUNT(*) FROM users";
    let spaces = "SELECT COUNT(*) FROM spaces";

    // Une connexion à côté du store pose une panne sur l'écriture des sessions.
    break_table(&db, "INSERT", "sessions", "1").await;
    // Avec une invitation, puis sans (le compte recevrait un espace à lui).
    for (tag, invitation) in [("bob", Some(token)), ("carol", None)] {
        let failed = store
            .sign_up(
                new_user(tag),
                invitation,
                &space_of(tag),
                session(),
                None,
                now(),
            )
            .await;
        assert!(matches!(failed, Err(StoreError::Sqlx(_))), "{failed:?}");
        assert_eq!(int(&db, users).await, 1, "{tag}");
        assert_eq!(
            store
                .user_by_email(&format!("{tag}@exemple.test"))
                .await
                .unwrap(),
            None,
            "{tag} : l'adresse reste libre"
        );
        assert_eq!(int(&db, spaces).await, 1, "{tag}");
        assert_eq!(int(&db, "SELECT COUNT(*) FROM sessions").await, 1, "{tag}");
        assert_eq!(store.members(space.id).await.unwrap().len(), 1, "{tag}");
        assert_eq!(store.space_rev(space.id).await.unwrap(), rev, "{tag}");
        assert!(
            store
                .invitation_by_token(token, now())
                .await
                .unwrap()
                .is_some(),
            "{tag} : l'invitation attend toujours"
        );
    }

    // La panne levée, l'inscription relancée fait ce qu'elle devait.
    repair(&db).await;
    let opened = session();
    let bob = store
        .sign_up(
            new_user("bob"),
            Some(token),
            &space_of("bob"),
            opened,
            None,
            now(),
        )
        .await
        .unwrap();
    assert!(bob.accepted.is_some());
    assert_eq!(
        store
            .session_user(opened.token_hash, now(), TTL)
            .await
            .unwrap()
            .map(|u| u.id),
        Some(bob.user.id)
    );
}

/// Des inscriptions de front sur une base sans compte : une seule voit la base vide. Elle seule
/// est admin de l'instance et reprend l'espace sans membre ; chacune des autres reçoit le sien.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_sign_ups_make_one_first_account() {
    let f = fresh().await;
    let mut set = tokio::task::JoinSet::new();
    for i in 0..8 {
        let store = f.store.clone();
        set.spawn(async move {
            let tag = format!("c{i}");
            store
                .sign_up(
                    new_user(&tag),
                    None,
                    &space_of(&tag),
                    session(),
                    None,
                    now(),
                )
                .await
        });
    }
    let signed: Vec<_> = set
        .join_all()
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    let (first, others): (Vec<_>, Vec<_>) = signed.iter().partition(|s| s.user.instance_admin);
    assert_eq!(first.len(), 1, "un seul premier compte");
    assert_eq!(first[0].adopted.len(), 1);
    assert_eq!(first[0].adopted[0].id, f.space.id);
    assert_eq!(first[0].space, None);
    for other in &others {
        assert!(other.adopted.is_empty());
        assert!(other.space.is_some());
    }
    assert_eq!(f.store.members(f.space.id).await.unwrap().len(), 1);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM users").await, 8);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM spaces").await, 8);
}
