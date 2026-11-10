//! Comptes et sessions : premier compte admin d'instance, adresse e-mail unique, sessions qui
//! expirent et glissent, changement de mot de passe.

mod common;

use common::{account, fresh, int};
use mementum_store::{NewUser, StoreError};
use uuid::Uuid;

const DAY: i64 = 86_400_000;
const TTL: i64 = 30 * DAY;
/// Un instant fixe, en millisecondes Unix : les sessions prennent l'heure de l'appelant.
const T0: i64 = 1_800_000_000_000;

fn new_user(email: &str) -> NewUser {
    NewUser {
        email: email.to_string(),
        name: "Compte de test".to_string(),
        password_hash: "pas-un-hachage".to_string(),
    }
}

/// L'échéance d'une session, lue en base.
async fn expiry(db: &std::path::Path, token: [u8; 32]) -> i64 {
    let hex: String = token.iter().map(|b| format!("{b:02X}")).collect();
    int(
        db,
        &format!("SELECT expires_at FROM sessions WHERE hex(token_hash) = '{hex}'"),
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn first_user_is_instance_admin_only() {
    let f = fresh().await;
    assert_eq!(f.store.count_users().await.unwrap(), 0);

    // Des créations concurrentes, sur une base sans compte : une seule voit la base vide.
    let mut set = tokio::task::JoinSet::new();
    for i in 0..8 {
        let store = f.store.clone();
        set.spawn(async move {
            store
                .create_user(new_user(&format!("c{i}@exemple.test")))
                .await
        });
    }
    let created: Vec<_> = set
        .join_all()
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    let admins: Vec<_> = created.iter().filter(|u| u.instance_admin).collect();
    assert_eq!(admins.len(), 1, "un seul admin d'instance : {created:?}");
    assert_eq!(f.store.count_users().await.unwrap(), 8);
    assert_eq!(
        int(&f.db, "SELECT COUNT(*) FROM users WHERE instance_admin = 1").await,
        1
    );
    // Ce qui est rendu est ce qui est écrit.
    for user in &created {
        assert_eq!(
            f.store.get_user(user.id).await.unwrap().as_ref(),
            Some(user)
        );
    }

    // Les comptes suivants ne le sont pas non plus.
    let later = account(&f.store, "plus-tard").await;
    assert!(!later.instance_admin);
    assert_eq!(f.store.count_users().await.unwrap(), 9);
}

#[tokio::test]
async fn email_is_unique_ignoring_case_and_spaces() {
    let f = fresh().await;
    let store = &f.store;
    let alice = store
        .create_user(NewUser {
            email: "  Alice.Martin@Exemple.FR \t".to_string(),
            name: "  Alice ".to_string(),
            password_hash: "pas-un-hachage".to_string(),
        })
        .await
        .unwrap();
    // L'adresse est gardée rognée et en minuscules ; le nom, tel quel (ses règles sont celles
    // de la route HTTP).
    assert_eq!(alice.email, "alice.martin@exemple.fr");
    assert_eq!(alice.name, "  Alice ");
    assert_eq!(alice.password_hash.as_deref(), Some("pas-un-hachage"));
    assert!(alice.instance_admin, "le premier compte");
    assert!(alice.created_at > 1_600_000_000_000, "millisecondes Unix");

    for taken in [
        "alice.martin@exemple.fr",
        "ALICE.MARTIN@EXEMPLE.FR",
        " alice.martin@exemple.fr",
        "Alice.Martin@exemple.fr\n",
    ] {
        let r = store.create_user(new_user(taken)).await;
        assert!(
            matches!(r, Err(StoreError::EmailTaken)),
            "{taken:?} : {r:?}"
        );
    }
    assert_eq!(store.count_users().await.unwrap(), 1);

    // La recherche normalise de même.
    for query in ["alice.martin@exemple.fr", "  ALICE.Martin@exemple.FR  "] {
        assert_eq!(
            store.user_by_email(query).await.unwrap().as_ref(),
            Some(&alice),
            "{query:?}"
        );
    }
    assert_eq!(store.user_by_email("bob@exemple.fr").await.unwrap(), None);
    assert_eq!(store.get_user(alice.id).await.unwrap(), Some(alice));
    assert_eq!(store.get_user(Uuid::now_v7()).await.unwrap(), None);
}

#[tokio::test]
async fn set_password_deletes_sessions() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let expires = T0 + TTL;
    store
        .create_session(alice.id, [1; 32], expires)
        .await
        .unwrap();
    store
        .create_session(alice.id, [2; 32], expires)
        .await
        .unwrap();
    store
        .create_session(bob.id, [3; 32], expires)
        .await
        .unwrap();

    store
        .set_password(alice.id, "nouveau-hachage")
        .await
        .unwrap();
    assert_eq!(
        store
            .get_user(alice.id)
            .await
            .unwrap()
            .unwrap()
            .password_hash
            .as_deref(),
        Some("nouveau-hachage")
    );
    // Toutes ses sessions sont mortes, celles des autres non.
    assert_eq!(store.session_user([1; 32], T0, TTL).await.unwrap(), None);
    assert_eq!(store.session_user([2; 32], T0, TTL).await.unwrap(), None);
    let still = store.session_user([3; 32], T0, TTL).await.unwrap().unwrap();
    assert_eq!(still.id, bob.id);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM sessions").await, 1);

    // Un compte inconnu : rien n'est écrit.
    assert!(matches!(
        store.set_password(Uuid::now_v7(), "x").await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.create_session(Uuid::now_v7(), [4; 32], expires).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM sessions").await, 1);

    // Le hachage ne sort pas par `Debug` (un compte écrit dans un journal).
    let shown = format!("{:?}", store.get_user(alice.id).await.unwrap());
    assert!(!shown.contains("nouveau-hachage"), "{shown}");
}

#[tokio::test]
async fn session_expires_and_slides() {
    let f = fresh().await;
    let (store, db) = (&f.store, &f.db);
    let alice = account(store, "alice").await;
    let token = [7; 32];
    store
        .create_session(alice.id, token, T0 + TTL)
        .await
        .unwrap();
    assert_eq!(expiry(db, token).await, T0 + TTL);

    // Tant qu'il reste plus de `TTL` moins un jour, l'échéance ne bouge pas (rien n'est écrit).
    let user = store.session_user(token, T0, TTL).await.unwrap();
    assert_eq!(user.as_ref(), Some(&alice));
    store.session_user(token, T0 + DAY - 1, TTL).await.unwrap();
    store.session_user(token, T0 + DAY, TTL).await.unwrap();
    assert_eq!(expiry(db, token).await, T0 + TTL);

    // Au-delà, elle repart à `now + TTL`.
    let now = T0 + DAY + 1;
    assert_eq!(
        store.session_user(token, now, TTL).await.unwrap().as_ref(),
        Some(&alice)
    );
    assert_eq!(expiry(db, token).await, now + TTL);

    // Échue : `None`, dès l'échéance, et rien n'est écrit.
    let end = now + TTL;
    assert_eq!(store.session_user(token, end, TTL).await.unwrap(), None);
    assert_eq!(
        store.session_user(token, end + DAY, TTL).await.unwrap(),
        None
    );
    assert_eq!(expiry(db, token).await, end);
    // Une milliseconde avant, elle vit encore (et glisse).
    assert_eq!(
        store
            .session_user(token, end - 1, TTL)
            .await
            .unwrap()
            .as_ref(),
        Some(&alice)
    );
    let end = end - 1 + TTL;
    assert_eq!(expiry(db, token).await, end);
    // Un jeton inconnu : `None`.
    assert_eq!(store.session_user([8; 32], T0, TTL).await.unwrap(), None);

    // La purge retire les sessions échues, et elles seules.
    store
        .create_session(alice.id, [9; 32], end + TTL)
        .await
        .unwrap();
    assert_eq!(store.purge_sessions(end).await.unwrap(), 1);
    assert_eq!(store.purge_sessions(end).await.unwrap(), 0);
    assert!(
        store
            .session_user([9; 32], end, TTL)
            .await
            .unwrap()
            .is_some()
    );

    // La déconnexion retire la session ; sans effet pour un jeton inconnu.
    store.delete_session([9; 32]).await.unwrap();
    store.delete_session([9; 32]).await.unwrap();
    assert_eq!(store.session_user([9; 32], end, TTL).await.unwrap(), None);
    assert_eq!(int(db, "SELECT COUNT(*) FROM sessions").await, 0);
}
