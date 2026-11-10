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

/// Fermer une session rend son compte, qu'elle soit échue ou non : qui la ferme doit aussi
/// fermer ce qu'elle avait ouvert. Un jeton inconnu ne rend rien.
#[tokio::test]
async fn closed_session_names_its_account() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    store
        .create_session(alice.id, [1; 32], T0 + TTL)
        .await
        .unwrap();
    // Celle de Bob est déjà échue.
    store.create_session(bob.id, [2; 32], T0 - 1).await.unwrap();
    assert_eq!(store.session_user([2; 32], T0, TTL).await.unwrap(), None);

    assert_eq!(store.delete_session([1; 32]).await.unwrap(), Some(alice.id));
    assert_eq!(store.delete_session([2; 32]).await.unwrap(), Some(bob.id));
    // Déjà fermées, ou jamais ouverte.
    assert_eq!(store.delete_session([1; 32]).await.unwrap(), None);
    assert_eq!(store.delete_session([9; 32]).await.unwrap(), None);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM sessions").await, 0);
}

/// `session_is_live` dit si une session vaut encore, sans rien écrire : son échéance ne glisse
/// pas, même là où `session_user` la ferait glisser.
#[tokio::test]
async fn session_liveness_is_read_without_sliding() {
    let f = fresh().await;
    let (store, db) = (&f.store, &f.db);
    let alice = account(store, "alice").await;
    let token = [7; 32];
    store
        .create_session(alice.id, token, T0 + TTL)
        .await
        .unwrap();

    assert!(store.session_is_live(token, T0).await.unwrap());
    // Une milliseconde avant l'échéance : vivante, et l'échéance n'a pas bougé.
    assert!(store.session_is_live(token, T0 + TTL - 1).await.unwrap());
    assert_eq!(expiry(db, token).await, T0 + TTL);
    // Échue dès l'échéance, comme pour `session_user` ; inconnue ; fermée.
    assert!(!store.session_is_live(token, T0 + TTL).await.unwrap());
    assert!(!store.session_is_live([8; 32], T0).await.unwrap());
    store.delete_session(token).await.unwrap();
    assert!(!store.session_is_live(token, T0).await.unwrap());
}

/// Le hachage du compte, lu en base.
async fn password_hash(store: &mementum_store::Store, user: Uuid) -> Option<String> {
    store.get_user(user).await.unwrap().unwrap().password_hash
}

/// Ruling R120 : une session ne naît que si le hachage que l'appelant vient de vérifier est
/// encore celui du compte. `set_password` le change, et supprime les sessions : une connexion
/// vérifiée avant lui n'ouvre plus rien après. C'est lui que lancent la route de changement
/// comme la commande `mementum user set-password` : les deux sont couvertes.
#[tokio::test]
async fn session_is_only_opened_for_the_verified_password_hash() {
    let f = fresh().await;
    let (store, db) = (&f.store, &f.db);
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let expires = T0 + TTL;
    let verified = alice.password_hash.clone().unwrap();

    // Le hachage vérifié est celui du compte : la session est ouverte.
    assert!(
        store
            .create_session_verified(alice.id, &verified, [1; 32], expires)
            .await
            .unwrap()
    );
    let opened = store.session_user([1; 32], T0, TTL).await.unwrap().unwrap();
    assert_eq!(opened.id, alice.id);
    assert_eq!(expiry(db, [1; 32]).await, expires);

    // Le mot de passe change : le hachage vérifié avant ne vaut plus, et rien n'est écrit.
    store
        .set_password(alice.id, "nouveau-hachage")
        .await
        .unwrap();
    assert!(
        !store
            .create_session_verified(alice.id, &verified, [2; 32], expires)
            .await
            .unwrap()
    );
    assert_eq!(int(db, "SELECT COUNT(*) FROM sessions").await, 0);
    // Celui d'après le changement, si.
    assert!(
        store
            .create_session_verified(alice.id, "nouveau-hachage", [3; 32], expires)
            .await
            .unwrap()
    );

    // Le hachage est celui du compte nommé, pas celui d'un autre : Bob porte encore le hachage
    // qu'Alice avait, et c'est sa session à lui que ce hachage ouvre.
    assert_eq!(
        password_hash(store, bob.id).await.as_deref(),
        Some(verified.as_str())
    );
    assert!(
        store
            .create_session_verified(bob.id, &verified, [4; 32], expires)
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .session_user([4; 32], T0, TTL)
            .await
            .unwrap()
            .unwrap()
            .id,
        bob.id
    );
    // Un compte inconnu, un compte sans mot de passe (Ruling R60) : rien, sans erreur.
    assert!(
        !store
            .create_session_verified(Uuid::now_v7(), &verified, [5; 32], expires)
            .await
            .unwrap()
    );
    common::exec_sql(
        db,
        &format!(
            "UPDATE users SET password_hash = NULL WHERE id = '{}'",
            bob.id
        ),
    )
    .await;
    for hash in [verified.as_str(), ""] {
        assert!(
            !store
                .create_session_verified(bob.id, hash, [6; 32], expires)
                .await
                .unwrap()
        );
    }
    assert_eq!(int(db, "SELECT COUNT(*) FROM sessions").await, 2);
}

/// Ruling R120, pour le changement de mot de passe, qui rouvre une session : le hachage n'est
/// remplacé que s'il est encore celui que l'appelant a vérifié. Alors les sessions du compte
/// sont supprimées et la sienne est ouverte, dans la même transaction. Sinon rien ne change :
/// ni le hachage, ni les sessions.
#[tokio::test]
async fn password_is_only_changed_from_the_verified_hash() {
    let f = fresh().await;
    let (store, db) = (&f.store, &f.db);
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let expires = T0 + TTL;
    let verified = alice.password_hash.clone().unwrap();
    for (user, token) in [(alice.id, [1; 32]), (alice.id, [2; 32]), (bob.id, [3; 32])] {
        store.create_session(user, token, expires).await.unwrap();
    }

    assert!(
        store
            .change_password(alice.id, &verified, "premier-changement", [4; 32], expires)
            .await
            .unwrap()
    );
    assert_eq!(
        password_hash(store, alice.id).await.as_deref(),
        Some("premier-changement")
    );
    // Les sessions d'avant sont mortes, la neuve vaut, celle de Bob aussi.
    for dead in [[1; 32], [2; 32]] {
        assert_eq!(store.session_user(dead, T0, TTL).await.unwrap(), None);
    }
    for (live, user) in [([4; 32], alice.id), ([3; 32], bob.id)] {
        assert_eq!(
            store.session_user(live, T0, TTL).await.unwrap().unwrap().id,
            user
        );
    }

    // Une demande vérifiée contre le hachage d'avant, arrivée après : rien ne change.
    assert!(
        !store
            .change_password(alice.id, &verified, "second-changement", [5; 32], expires)
            .await
            .unwrap()
    );
    assert_eq!(
        password_hash(store, alice.id).await.as_deref(),
        Some("premier-changement")
    );
    assert!(store.session_is_live([4; 32], T0).await.unwrap());
    assert!(!store.session_is_live([5; 32], T0).await.unwrap());
    // Un compte inconnu : rien non plus, sans erreur.
    assert!(
        !store
            .change_password(Uuid::now_v7(), &verified, "x", [6; 32], expires)
            .await
            .unwrap()
    );
    // Le hachage de Bob, le même texte que celui d'Alice avant, n'a pas bougé.
    assert_eq!(
        password_hash(store, bob.id).await.as_deref(),
        Some(verified.as_str())
    );
    assert_eq!(int(db, "SELECT COUNT(*) FROM sessions").await, 2);
}
