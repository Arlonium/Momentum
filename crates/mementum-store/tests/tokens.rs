//! Jetons d'API et journal d'activité (lot 5, Task S1 ; Rulings R1213, R1218 à R1221, R1244,
//! R1245) : un jeton ne se trouve que par son hachage, tant qu'il vit, sans rien écrire ; il ne
//! nomme que des pages de son espace ; un compte en a vingt vivants et garde ses morts
//! quatre-vingt-dix jours, cinquante au plus ; un changement de mot de passe les révoque tous ; le
//! journal garde par jeton deux parts, que les refus ne mélangent jamais aux appels réussis, et
//! s'écrit dans une voie à lui.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{Fixture, account, break_table, exec_sql, fresh, int, page, repair, texts};
use mementum_acl::Role;
use mementum_store::{
    Busy, Call, CallOutcome, CallSurface, DEAD_TOKEN_KEPT_MS, LiveToken, MAX_DEAD_TOKENS,
    MAX_KEPT_CALLS, MAX_KEPT_REFUSALS, MAX_LIVE_TOKENS, MAX_TOKEN_PAGES, NewToken, Store,
    StoreError, TokenLevel, TokenScope, writes_by,
};
use uuid::Uuid;

/// Délai maximal d'une attente de condition.
const WAIT: Duration = Duration::from_secs(10);

/// Un instant fixe, en millisecondes Unix : les jetons de ces tests vivent à l'horloge qu'on leur
/// donne.
const T0: i64 = 1_700_000_000_000;

/// Un jour, en millisecondes.
const DAY: i64 = 86_400_000;

/// Un hachage neuf : deux UUIDv7 bout à bout (le store ne voit jamais que des hachages).
fn fresh_hash() -> [u8; 32] {
    let mut hash = [0; 32];
    hash[..16].copy_from_slice(Uuid::now_v7().as_bytes());
    hash[16..].copy_from_slice(Uuid::now_v7().as_bytes());
    hash
}

/// Un jeton de `user`, nommé `name`, de cette portée, créé à `now` : niveau « modification », sans
/// corbeille ni échéance.
fn token(user: Uuid, name: &str, scope: TokenScope, now: i64) -> NewToken {
    NewToken {
        user_id: user,
        name: name.to_string(),
        hash: fresh_hash(),
        hint: "Ab9_".to_string(),
        level: TokenLevel::Edit,
        scope,
        can_trash: false,
        expires_at: None,
        now,
    }
}

/// Un appel du jeton, à `at`, de cette issue.
fn call(token: Uuid, at: i64, outcome: CallOutcome) -> Call {
    Call {
        token_id: token,
        at,
        surface: CallSurface::Mcp,
        tool: "fetch".to_string(),
        space_id: None,
        node_id: None,
        outcome,
        code: (outcome != CallOutcome::Ok).then(|| "no_match".to_string()),
        revision_before: None,
        revision_after: None,
        blocks_touched: None,
    }
}

/// Une page de l'espace de départ.
async fn new_page(f: &Fixture, title: &str) -> Uuid {
    f.store
        .create_node(page(&f.space, None, None, title))
        .await
        .unwrap()
        .id
}

/// Les lignes du journal d'activité du jeton, de cette part : `ok`, ou les refus et les erreurs.
async fn kept(db: &Path, token: Uuid, ok: bool) -> Vec<i64> {
    let part = if ok {
        "outcome = 'ok'"
    } else {
        "outcome != 'ok'"
    };
    texts(
        db,
        &format!(
            "SELECT CAST(at AS TEXT) FROM token_calls \
             WHERE token_id = '{token}' AND {part} ORDER BY id"
        ),
    )
    .await
    .into_iter()
    .map(|at| at.unwrap().parse().unwrap())
    .collect()
}

/// Attend que `n` écritures soient en file ([`WAIT`] au plus) : une condition, pas une durée.
async fn queued(store: &Store, n: usize) {
    let deadline = Instant::now() + WAIT;
    while store.queued_writes() != n {
        assert!(
            Instant::now() < deadline,
            "{} écritures en file, {n} attendues",
            store.queued_writes()
        );
        tokio::task::yield_now().await;
    }
}

/// Le jeton ne se trouve que par son hachage, et seulement tant qu'il vit : ni révoqué, ni échu (à
/// son échéance même, il ne vaut plus), ni d'un compte supprimé (ses jetons partent avec lui).
/// Trouvé, il porte son compte, son nom, son niveau, sa portée, sa corbeille et son échéance.
#[tokio::test]
async fn a_token_is_found_by_its_hash_only_while_it_lives() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    f.store
        .add_member(f.space.id, alice.id, Role::Member)
        .await
        .unwrap();
    let mut new = token(alice.id, "Claude Code", TokenScope::Space(f.space.id), T0);
    new.level = TokenLevel::Read;
    new.can_trash = true;
    new.expires_at = Some(T0 + 1_000);
    let hash = new.hash;
    let made = f.store.create_token(new).await.unwrap();
    assert_eq!(made.user_id, alice.id);
    assert_eq!(made.name, "Claude Code");
    assert_eq!(made.hint, "Ab9_");
    assert_eq!(made.level, TokenLevel::Read);
    assert_eq!(made.scope, TokenScope::Space(f.space.id));
    assert!(made.can_trash);
    assert_eq!(made.created_at, T0);
    assert_eq!(made.expires_at, Some(T0 + 1_000));
    assert_eq!(
        (made.revoked_at, made.last_used_at, made.busy_last_at),
        (None, None, None)
    );
    assert_eq!((made.busy_count, made.calls_purged), (0, 0));

    let live = LiveToken {
        id: made.id,
        user_id: alice.id,
        name: "Claude Code".to_string(),
        level: TokenLevel::Read,
        scope: TokenScope::Space(f.space.id),
        can_trash: true,
        expires_at: Some(T0 + 1_000),
    };
    assert_eq!(
        f.store.token_by_hash(hash, T0 + 999).await.unwrap(),
        Some(live)
    );
    assert!(f.store.token_is_live(made.id, T0 + 999).await.unwrap());
    // Échu, à son échéance même.
    assert_eq!(f.store.token_by_hash(hash, T0 + 1_000).await.unwrap(), None);
    assert!(!f.store.token_is_live(made.id, T0 + 1_000).await.unwrap());

    // Révoqué.
    let other = token(alice.id, "Autre", TokenScope::All, T0);
    let other_hash = other.hash;
    let other = f.store.create_token(other).await.unwrap();
    assert!(
        f.store
            .token_by_hash(other_hash, T0)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        f.store
            .revoke_token(alice.id, other.id, T0 + 5)
            .await
            .unwrap()
    );
    assert_eq!(
        f.store.token_by_hash(other_hash, T0 + 6).await.unwrap(),
        None
    );
    assert!(!f.store.token_is_live(other.id, T0 + 6).await.unwrap());

    // Un compte supprimé emporte ses jetons.
    let bob = account(&f.store, "bob").await;
    let bobs = token(bob.id, "Bob", TokenScope::All, T0);
    let bobs_hash = bobs.hash;
    let bobs = f.store.create_token(bobs).await.unwrap();
    assert!(
        f.store
            .token_by_hash(bobs_hash, T0)
            .await
            .unwrap()
            .is_some()
    );
    exec_sql(&f.db, &format!("DELETE FROM users WHERE id = '{}'", bob.id)).await;
    assert_eq!(f.store.token_by_hash(bobs_hash, T0).await.unwrap(), None);
    assert!(!f.store.token_is_live(bobs.id, T0).await.unwrap());

    // Un hachage inconnu.
    assert_eq!(f.store.token_by_hash(fresh_hash(), T0).await.unwrap(), None);
}

/// Ruling R517, Review Focus de la Task S1 : chercher un jeton n'écrit rien, connu ou non (un
/// anonyme qui essaie des jetons ne remplit aucune voie de la file). Tour d'écriture tenu, la
/// lecture d'un hachage inconnu, d'un jeton vivant, d'un jeton révoqué, et ce qu'en lisent les
/// autres lectures, aboutissent sans attendre, et rien n'entre en file.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unknown_hash_writes_nothing() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let live = token(alice.id, "Vivant", TokenScope::All, T0);
    let live_hash = live.hash;
    let live = f.store.create_token(live).await.unwrap();
    let dead = token(alice.id, "Révoqué", TokenScope::All, T0);
    let dead_hash = dead.hash;
    let dead = f.store.create_token(dead).await.unwrap();
    f.store.revoke_token(alice.id, dead.id, T0).await.unwrap();

    let held = f.store.hold_writes().await;
    let reads = tokio::spawn({
        let store = f.store.clone();
        async move {
            let mut found = Vec::new();
            for hash in [fresh_hash(), live_hash, dead_hash] {
                found.push(store.token_by_hash(hash, T0).await.unwrap().map(|t| t.id));
            }
            let lives = (
                store.token_is_live(live.id, T0).await.unwrap(),
                store.token_is_live(Uuid::now_v7(), T0).await.unwrap(),
            );
            let listed = store.tokens_of(alice.id, T0).await.unwrap().len();
            (found, lives, listed)
        }
    });
    let (found, lives, listed) = tokio::time::timeout(WAIT, reads)
        .await
        .expect("une lecture de jeton n'attend aucun tour d'écriture")
        .unwrap();
    assert_eq!(found, [None, Some(live.id), None]);
    assert_eq!(lives, (true, false));
    assert_eq!(listed, 2);
    assert_eq!(f.store.queued_writes(), 0);
    drop(held);
}

/// Un jeton de portée « pages » ne nomme que des pages de son espace (Ruling R1219), chacune une
/// fois : une page d'un autre espace, ou inconnue, fait refuser le jeton entier, sans rien écrire.
/// Un jeton d'un espace, ou de pages, demande un rôle dans l'espace. Ni zéro page, ni plus de
/// cinquante : la route le dit, et le store ne l'écrit pas.
#[tokio::test]
async fn a_token_names_only_pages_of_its_space() {
    let f = fresh().await;
    let elsewhere = f.store.create_space("Ailleurs", None).await.unwrap();
    let closed = f.store.create_space("Fermé", None).await.unwrap();
    let alice = account(&f.store, "alice").await;
    for space in [f.space.id, elsewhere.id] {
        f.store
            .add_member(space, alice.id, Role::Member)
            .await
            .unwrap();
    }
    let here = new_page(&f, "Ici").await;
    let second = new_page(&f, "Ici aussi").await;
    let there = f
        .store
        .create_node(page(&elsewhere, None, None, "Là-bas"))
        .await
        .unwrap()
        .id;
    let shut = f
        .store
        .create_node(page(&closed, None, None, "Fermée"))
        .await
        .unwrap()
        .id;

    for scope in [
        TokenScope::Pages {
            space: f.space.id,
            pages: vec![here, there],
        },
        TokenScope::Pages {
            space: f.space.id,
            pages: vec![here, Uuid::now_v7()],
        },
        TokenScope::Space(closed.id),
        TokenScope::Pages {
            space: closed.id,
            pages: vec![shut],
        },
    ] {
        let refused = f
            .store
            .create_token(token(alice.id, "x", scope.clone(), T0))
            .await;
        assert!(matches!(refused, Err(StoreError::NotFound)), "{scope:?}");
    }
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_tokens").await, 0);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_token_pages").await, 0);

    // Les pages de l'espace, chacune une fois.
    let new = token(
        alice.id,
        "Pages",
        TokenScope::Pages {
            space: f.space.id,
            pages: vec![second, here, here],
        },
        T0,
    );
    let hash = new.hash;
    let made = f.store.create_token(new).await.unwrap();
    let mut pages = vec![here, second];
    pages.sort();
    let scope = TokenScope::Pages {
        space: f.space.id,
        pages,
    };
    assert_eq!(made.scope, scope);
    assert_eq!(
        f.store
            .token_by_hash(hash, T0)
            .await
            .unwrap()
            .unwrap()
            .scope,
        scope
    );

    // Ni aucune page, ni plus de cinquante, même toutes de l'espace : un appelant mal écrit.
    let mut many = Vec::new();
    for i in 0..=MAX_TOKEN_PAGES {
        many.push(new_page(&f, &format!("P{i}")).await);
    }
    for pages in [Vec::new(), many.clone()] {
        let refused = f
            .store
            .create_token(token(
                alice.id,
                "x",
                TokenScope::Pages {
                    space: f.space.id,
                    pages,
                },
                T0,
            ))
            .await;
        assert!(
            matches!(&refused, Err(StoreError::Sqlx(_))),
            "un appelant mal écrit, pas une page introuvable : {refused:?}"
        );
    }
    many.pop();
    f.store
        .create_token(token(
            alice.id,
            "Cinquante",
            TokenScope::Pages {
                space: f.space.id,
                pages: many,
            },
            T0,
        ))
        .await
        .unwrap();
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_tokens").await, 2);
}

/// Un compte a au plus vingt jetons vivants (Ruling R1220) : le vingt-et-unième est refusé, sans
/// rien écrire ; un jeton révoqué, ou échu, ne compte plus ; ceux d'un autre compte n'y comptent
/// pas.
#[tokio::test]
async fn the_twenty_first_token_is_refused() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let mut ids = Vec::new();
    for i in 0..MAX_LIVE_TOKENS {
        let made = f
            .store
            .create_token(token(
                alice.id,
                &format!("j{i}"),
                TokenScope::All,
                T0 + i as i64,
            ))
            .await
            .unwrap();
        ids.push(made.id);
    }
    assert!(matches!(
        f.store
            .create_token(token(alice.id, "de trop", TokenScope::All, T0 + 100))
            .await,
        Err(StoreError::TokenLimit)
    ));
    assert_eq!(
        int(&f.db, "SELECT COUNT(*) FROM api_tokens").await,
        MAX_LIVE_TOKENS as i64
    );
    let bob = account(&f.store, "bob").await;
    f.store
        .create_token(token(bob.id, "Bob", TokenScope::All, T0 + 100))
        .await
        .unwrap();

    // Révoqué : il ne compte plus.
    assert!(
        f.store
            .revoke_token(alice.id, ids[0], T0 + 200)
            .await
            .unwrap()
    );
    f.store
        .create_token(token(alice.id, "à sa place", TokenScope::All, T0 + 300))
        .await
        .unwrap();

    // Échu : il ne compte plus, dès son échéance.
    assert!(
        f.store
            .revoke_token(alice.id, ids[1], T0 + 400)
            .await
            .unwrap()
    );
    let mut expiring = token(alice.id, "échéance", TokenScope::All, T0 + 500);
    expiring.expires_at = Some(T0 + 1_000);
    f.store.create_token(expiring).await.unwrap();
    assert!(matches!(
        f.store
            .create_token(token(alice.id, "trop tôt", TokenScope::All, T0 + 999))
            .await,
        Err(StoreError::TokenLimit)
    ));
    f.store
        .create_token(token(alice.id, "à l'échéance", TokenScope::All, T0 + 1_000))
        .await
        .unwrap();
}

/// Un jeton mort (révoqué ou échu) est gardé, avec son journal, quatre-vingt-dix jours après sa
/// mort, et un compte en garde cinquante au plus (Ruling R1220) : au-delà, le plus anciennement
/// mort part, avec ses lignes, dans la transaction qui crée ou révoque un jeton. Passé
/// quatre-vingt-dix jours, un jeton mort n'est plus lu, même avant que la purge le retire.
#[tokio::test]
async fn dead_tokens_are_kept_ninety_days_and_fifty_at_most() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let names = async |now: i64| -> Vec<String> {
        f.store
            .tokens_of(alice.id, now)
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect()
    };
    let old = f
        .store
        .create_token(token(alice.id, "ancien", TokenScope::All, T0))
        .await
        .unwrap();
    f.store
        .append_calls(&[call(old.id, T0, CallOutcome::Ok)], &[])
        .await
        .unwrap();
    f.store.revoke_token(alice.id, old.id, T0).await.unwrap();
    let recent = f
        .store
        .create_token(token(alice.id, "récent", TokenScope::All, T0))
        .await
        .unwrap();
    f.store
        .revoke_token(alice.id, recent.id, T0 + DAY)
        .await
        .unwrap();

    // Encore lu la veille de ses quatre-vingt-dix jours ; plus le jour même.
    assert_eq!(
        names(T0 + DEAD_TOKEN_KEPT_MS - 1).await,
        ["récent", "ancien"]
    );
    assert_eq!(names(T0 + DEAD_TOKEN_KEPT_MS).await, ["récent"]);
    assert!(matches!(
        f.store
            .calls_of(alice.id, old.id, None, None, 10, T0 + DEAD_TOKEN_KEPT_MS)
            .await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        f.store
            .calls_of(
                alice.id,
                old.id,
                None,
                None,
                10,
                T0 + DEAD_TOKEN_KEPT_MS - 1
            )
            .await
            .unwrap()
            .len(),
        1
    );
    // Toujours en base : c'est la création d'un jeton qui le retire, avec ses lignes.
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_tokens").await, 2);
    f.store
        .create_token(token(
            alice.id,
            "neuf",
            TokenScope::All,
            T0 + DEAD_TOKEN_KEPT_MS,
        ))
        .await
        .unwrap();
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_tokens").await, 2);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM token_calls").await, 0);
    assert_eq!(names(T0 + DEAD_TOKEN_KEPT_MS).await, ["neuf", "récent"]);

    // Cinquante morts au plus : chaque révocation au-delà retire le plus anciennement mort.
    let start = T0 + 2 * DEAD_TOKEN_KEPT_MS;
    let total = MAX_DEAD_TOKENS + 2;
    for i in 0..total {
        let now = start + 10 * i as i64;
        let made = f
            .store
            .create_token(token(alice.id, &format!("m{i}"), TokenScope::All, now))
            .await
            .unwrap();
        f.store
            .revoke_token(alice.id, made.id, now + 1)
            .await
            .unwrap();
    }
    let dead: Vec<String> = texts(
        &f.db,
        "SELECT name FROM api_tokens WHERE revoked_at IS NOT NULL ORDER BY revoked_at",
    )
    .await
    .into_iter()
    .flatten()
    .collect();
    let expected: Vec<String> = (2..total).map(|i| format!("m{i}")).collect();
    assert_eq!(dead, expected);
    assert_eq!(names(start + 10 * total as i64).await[0], "neuf");
}

/// Une page supprimée pour de bon quitte les jetons qui la nommaient (cascade), sans les retirer :
/// un jeton dont toutes les pages sont parties existe encore, et ne nomme plus rien.
#[tokio::test]
async fn a_deleted_page_leaves_its_tokens() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    f.store
        .add_member(f.space.id, alice.id, Role::Member)
        .await
        .unwrap();
    let (first, second) = (new_page(&f, "Une").await, new_page(&f, "Deux").await);
    let new = token(
        alice.id,
        "Pages",
        TokenScope::Pages {
            space: f.space.id,
            pages: vec![first, second],
        },
        T0,
    );
    let hash = new.hash;
    f.store.create_token(new).await.unwrap();
    let scope_now = async || {
        f.store
            .token_by_hash(hash, T0)
            .await
            .unwrap()
            .unwrap()
            .scope
    };

    f.store.trash_node(first, alice.id).await.unwrap();
    f.store.delete_node(first).await.unwrap();
    assert_eq!(
        scope_now().await,
        TokenScope::Pages {
            space: f.space.id,
            pages: vec![second],
        }
    );
    f.store.trash_node(second, alice.id).await.unwrap();
    f.store.delete_node(second).await.unwrap();
    let empty = TokenScope::Pages {
        space: f.space.id,
        pages: Vec::new(),
    };
    assert_eq!(scope_now().await, empty);
    assert_eq!(
        f.store.tokens_of(alice.id, T0).await.unwrap()[0].scope,
        empty
    );
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_token_pages").await, 0);
}

/// Ruling R1221 : changer de mot de passe révoque tous les jetons du compte, dans la transaction
/// du changement. Une révocation que la base refuse (un déclencheur de panne sur `api_tokens`) fait
/// échouer tout le changement : ni le mot de passe, ni les sessions, ni les jetons ne bougent. Le
/// changement passé, tous les jetons du compte sont révoqués, pas ceux d'un autre ; la commande
/// (`set_password`) fait de même.
#[tokio::test]
async fn changing_the_password_revokes_every_token_in_the_same_transaction() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let bob = account(&f.store, "bob").await;
    let mut hashes = Vec::new();
    for name in ["un", "deux"] {
        let new = token(alice.id, name, TokenScope::All, T0);
        hashes.push(new.hash);
        f.store.create_token(new).await.unwrap();
    }
    let bobs = token(bob.id, "Bob", TokenScope::All, T0);
    let bobs_hash = bobs.hash;
    f.store.create_token(bobs).await.unwrap();
    let far = i64::MAX;
    let session = [7; 32];
    f.store
        .create_session(alice.id, session, far)
        .await
        .unwrap();

    break_table(&f.db, "UPDATE", "api_tokens", "1").await;
    assert!(
        f.store
            .change_password(alice.id, "pas-un-hachage", "neuf", [8; 32], far)
            .await
            .is_err()
    );
    assert!(f.store.set_password(alice.id, "neuf").await.is_err());
    repair(&f.db).await;
    let user = f.store.get_user(alice.id).await.unwrap().unwrap();
    assert_eq!(user.password_hash.as_deref(), Some("pas-un-hachage"));
    assert!(f.store.session_is_live(session, 0).await.unwrap());
    for hash in &hashes {
        assert!(f.store.token_by_hash(*hash, T0).await.unwrap().is_some());
    }

    assert!(
        f.store
            .change_password(alice.id, "pas-un-hachage", "neuf", [8; 32], far)
            .await
            .unwrap()
            .is_some()
    );
    for hash in &hashes {
        assert_eq!(f.store.token_by_hash(*hash, T0).await.unwrap(), None);
    }
    assert!(
        f.store
            .tokens_of(alice.id, T0)
            .await
            .unwrap()
            .iter()
            .all(|t| t.revoked_at.is_some())
    );
    assert!(
        f.store
            .token_by_hash(bobs_hash, T0)
            .await
            .unwrap()
            .is_some()
    );

    let again = token(alice.id, "trois", TokenScope::All, T0);
    let again_hash = again.hash;
    f.store.create_token(again).await.unwrap();
    f.store.set_password(alice.id, "encore").await.unwrap();
    assert_eq!(f.store.token_by_hash(again_hash, T0).await.unwrap(), None);
    assert!(
        f.store
            .token_by_hash(bobs_hash, T0)
            .await
            .unwrap()
            .is_some()
    );
}

/// Révoquer un jeton déjà révoqué n'écrit rien, et ne prend aucun tour : la révocation lit
/// d'abord (Ruling R270). Le jeton d'un autre compte, ou un jeton inconnu, est introuvable, sans
/// rien écrire non plus. Un déclencheur témoin compte les mises à jour de `api_tokens`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revoking_twice_writes_once() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let bob = account(&f.store, "bob").await;
    let made = f
        .store
        .create_token(token(alice.id, "Jeton", TokenScope::All, T0))
        .await
        .unwrap();
    exec_sql(
        &f.db,
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER witness_revoke AFTER UPDATE ON api_tokens \
         BEGIN INSERT INTO witness (what) VALUES (NEW.revoked_at); END;",
    )
    .await;
    assert!(
        f.store
            .revoke_token(alice.id, made.id, T0 + 1)
            .await
            .unwrap()
    );

    let held = f.store.hold_writes().await;
    let again = tokio::spawn({
        let store = f.store.clone();
        async move {
            (
                store.revoke_token(alice.id, made.id, T0 + 2).await,
                store.revoke_token(bob.id, made.id, T0 + 2).await,
                store.revoke_token(alice.id, Uuid::now_v7(), T0 + 2).await,
            )
        }
    });
    let (twice, theirs, unknown) = tokio::time::timeout(WAIT, again)
        .await
        .expect("rien à écrire : aucun tour attendu")
        .unwrap();
    assert!(!twice.unwrap());
    assert!(matches!(theirs, Err(StoreError::NotFound)));
    assert!(matches!(unknown, Err(StoreError::NotFound)));
    assert_eq!(f.store.queued_writes(), 0);
    drop(held);
    assert_eq!(
        texts(&f.db, "SELECT what FROM witness ORDER BY n").await,
        [Some((T0 + 1).to_string())]
    );
}

/// Ruling R1244 : le journal garde par jeton deux parts, les mille derniers appels réussis et les
/// deux cents derniers refusés (une erreur compte avec eux) ; un refus ne fait jamais partir une
/// ligne réussie, ni l'inverse. Le journal se lit du plus récent au plus ancien, par pages.
#[tokio::test]
async fn refused_calls_never_push_out_a_successful_one() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let made = f
        .store
        .create_token(token(alice.id, "Jeton", TokenScope::All, T0))
        .await
        .unwrap();
    let ok: Vec<Call> = (0..MAX_KEPT_CALLS as i64)
        .map(|i| call(made.id, T0 + i, CallOutcome::Ok))
        .collect();
    f.store.append_calls(&ok, &[]).await.unwrap();
    let refused: Vec<Call> = (0..500)
        .map(|i| call(made.id, T0 + 10_000 + i, CallOutcome::Refused))
        .collect();
    f.store.append_calls(&refused, &[]).await.unwrap();
    let all_ok: Vec<i64> = (0..MAX_KEPT_CALLS as i64).map(|i| T0 + i).collect();
    assert_eq!(kept(&f.db, made.id, true).await, all_ok);
    let newest_refused: Vec<i64> = (500 - MAX_KEPT_REFUSALS as i64..500)
        .map(|i| T0 + 10_000 + i)
        .collect();
    assert_eq!(kept(&f.db, made.id, false).await, newest_refused);

    // Une erreur compte avec les refus.
    f.store
        .append_calls(&[call(made.id, T0 + 20_000, CallOutcome::Error)], &[])
        .await
        .unwrap();
    assert_eq!(kept(&f.db, made.id, true).await, all_ok);
    let refusals = kept(&f.db, made.id, false).await;
    assert_eq!(refusals.len(), MAX_KEPT_REFUSALS);
    assert_eq!(refusals[0], newest_refused[1]);
    assert_eq!(refusals.last(), Some(&(T0 + 20_000)));

    // Des appels réussis de plus ne font partir que les plus anciens réussis.
    let more: Vec<Call> = (0..10)
        .map(|i| call(made.id, T0 + 30_000 + i, CallOutcome::Ok))
        .collect();
    f.store.append_calls(&more, &[]).await.unwrap();
    let ok_now = kept(&f.db, made.id, true).await;
    assert_eq!(ok_now.len(), MAX_KEPT_CALLS);
    assert_eq!(ok_now[0], T0 + 10);
    assert_eq!(kept(&f.db, made.id, false).await, refusals);

    // Lu du plus récent au plus ancien, par pages.
    let first = f
        .store
        .calls_of(alice.id, made.id, None, None, 3, T0)
        .await
        .unwrap();
    let ats: Vec<i64> = first.iter().map(|row| row.call.at).collect();
    assert_eq!(ats, [T0 + 30_009, T0 + 30_008, T0 + 30_007]);
    assert_eq!(first[0].call, more[9]);
    let next = f
        .store
        .calls_of(alice.id, made.id, Some(first[2].id), None, 2, T0)
        .await
        .unwrap();
    let ats: Vec<i64> = next.iter().map(|row| row.call.at).collect();
    assert_eq!(ats, [T0 + 30_006, T0 + 30_005]);
    // Le journal d'un jeton d'un autre compte ne se lit pas.
    let bob = account(&f.store, "bob").await;
    assert!(matches!(
        f.store.calls_of(bob.id, made.id, None, None, 3, T0).await,
        Err(StoreError::NotFound)
    ));
}

/// Ruling R1244 : ce que la purge retire est compté, part par part, et la date de la dernière
/// utilisation suit le plus récent des appels écrits.
#[tokio::test]
async fn purged_calls_are_counted() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let made = f
        .store
        .create_token(token(alice.id, "Jeton", TokenScope::All, T0))
        .await
        .unwrap();
    let state = async || f.store.tokens_of(alice.id, T0).await.unwrap()[0].clone();
    let refused: Vec<Call> = (0..MAX_KEPT_REFUSALS as i64 + 5)
        .map(|i| call(made.id, T0 + i, CallOutcome::Refused))
        .collect();
    f.store.append_calls(&refused, &[]).await.unwrap();
    assert_eq!(state().await.calls_purged, 5);
    assert_eq!(state().await.last_used_at, Some(T0 + 204));
    let ok: Vec<Call> = (0..MAX_KEPT_CALLS as i64 + 7)
        .map(|i| call(made.id, T0 + 1_000 + i, CallOutcome::Ok))
        .collect();
    f.store.append_calls(&ok, &[]).await.unwrap();
    assert_eq!(state().await.calls_purged, 12);
    // Les sept plus anciens partent (T0 + 1 000 à T0 + 1 006).
    let newest = T0 + 1_000 + MAX_KEPT_CALLS as i64 + 6;
    let after_ok: Vec<i64> = (T0 + 1_007..=newest).collect();
    assert_eq!(kept(&f.db, made.id, true).await, after_ok);
    // Des appels écrits dans le désordre : la dernière utilisation ne recule pas, et la purge
    // suit l'ordre d'écriture, pas les dates (R1269) : l'appel daté T0 + 1, écrit le dernier,
    // reste, et c'est le plus anciennement écrit (T0 + 1 007) qui part.
    f.store
        .append_calls(&[call(made.id, T0 + 1, CallOutcome::Ok)], &[])
        .await
        .unwrap();
    let now = state().await;
    assert_eq!(now.calls_purged, 13);
    assert_eq!(now.last_used_at, Some(newest));
    let mut last_ok: Vec<i64> = (T0 + 1_008..=newest).collect();
    last_ok.push(T0 + 1);
    assert_eq!(kept(&f.db, made.id, true).await, last_ok);
    assert_eq!(
        int(&f.db, "SELECT COUNT(*) FROM token_calls").await,
        (MAX_KEPT_CALLS + MAX_KEPT_REFUSALS) as i64
    );
}

/// Ruling R1244 : un appel refusé par le débit (`429 token_busy`) n'écrit aucune ligne, seulement
/// un compte et la date du dernier ; il ne compte pas comme une utilisation. Rien à écrire : aucun
/// tour pris.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_busy_call_writes_no_line() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let made = f
        .store
        .create_token(token(alice.id, "Jeton", TokenScope::All, T0))
        .await
        .unwrap();
    let busy = |count, at| Busy {
        token_id: made.id,
        count,
        at,
    };
    f.store.append_calls(&[], &[busy(3, T0 + 5)]).await.unwrap();
    f.store.append_calls(&[], &[busy(2, T0 + 4)]).await.unwrap();
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM token_calls").await, 0);
    let state = &f.store.tokens_of(alice.id, T0).await.unwrap()[0];
    assert_eq!(state.busy_count, 5);
    assert_eq!(state.busy_last_at, Some(T0 + 5));
    assert_eq!(state.last_used_at, None);

    let held = f.store.hold_writes().await;
    let nothing = tokio::spawn({
        let store = f.store.clone();
        async move { store.append_calls(&[], &[]).await }
    });
    tokio::time::timeout(WAIT, nothing)
        .await
        .expect("rien à écrire : aucun tour attendu")
        .unwrap()
        .unwrap();
    assert_eq!(f.store.queued_writes(), 0);
    drop(held);
}

/// Le journal d'activité a une voie à lui dans la file (`Lane::Calls`, Ruling R1244), sans
/// priorité, quel que soit le compte de la tâche qui l'écrit : entre deux paquets, la file sert une
/// écriture de chaque autre voie qui attend. Trois paquets, chacun au nom d'un autre compte, puis
/// une écriture de la voie commune, puis trois d'Olga : l'ordre est journal, commune, Olga,
/// journal, Olga, journal, Olga. Dans la voie de chaque compte, les trois paquets passeraient
/// d'affilée.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn calls_are_written_in_their_own_lane() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let made = f
        .store
        .create_token(token(alice.id, "Jeton", TokenScope::All, T0))
        .await
        .unwrap();
    let olga = Uuid::now_v7();
    exec_sql(
        &f.db,
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER witness_calls AFTER INSERT ON token_calls \
         BEGIN INSERT INTO witness (what) VALUES ('journal'); END; \
         CREATE TRIGGER witness_space AFTER INSERT ON spaces \
         BEGIN INSERT INTO witness (what) VALUES (NEW.name); END;",
    )
    .await;
    let held = f.store.hold_writes().await;
    let mut writes = Vec::new();
    for i in 0..3 {
        let store = f.store.clone();
        let batch = [call(made.id, T0 + i, CallOutcome::Ok)];
        writes.push(tokio::spawn(writes_by(Some(Uuid::now_v7()), async move {
            store.append_calls(&batch, &[]).await
        })));
        queued(&f.store, i as usize + 1).await;
    }
    writes.push(tokio::spawn({
        let store = f.store.clone();
        writes_by(None, async move {
            store.create_space("commune", None).await.map(|_| ())
        })
    }));
    queued(&f.store, 4).await;
    for i in 0..3 {
        writes.push(tokio::spawn({
            let store = f.store.clone();
            writes_by(Some(olga), async move {
                store.create_space("olga", None).await.map(|_| ())
            })
        }));
        queued(&f.store, i + 5).await;
    }
    drop(held);
    for write in writes {
        write.await.unwrap().unwrap();
    }
    let order: Vec<String> = texts(&f.db, "SELECT what FROM witness ORDER BY n")
        .await
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(
        order,
        [
            "journal", "commune", "olga", "journal", "olga", "journal", "olga"
        ],
        "{order:?}"
    );
}

/// Un paquet du journal qui nomme un jeton disparu (purgé depuis, ou un identifiant inconnu)
/// n'échoue pas (relecture de S1) : ses lignes et ses comptes sont laissés, ceux des autres jetons
/// s'écrivent, et la dernière utilisation du jeton vivant est posée.
#[tokio::test]
async fn a_batch_naming_a_vanished_token_writes_the_others() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let purged = f
        .store
        .create_token(token(alice.id, "Parti", TokenScope::All, T0))
        .await
        .unwrap();
    f.store.revoke_token(alice.id, purged.id, T0).await.unwrap();
    let later = T0 + DEAD_TOKEN_KEPT_MS;
    let live = f
        .store
        .create_token(token(alice.id, "Vivant", TokenScope::All, later))
        .await
        .unwrap();
    assert_eq!(
        int(
            &f.db,
            &format!("SELECT COUNT(*) FROM api_tokens WHERE id = '{}'", purged.id)
        )
        .await,
        0,
        "la création d'un jeton a purgé le jeton mort"
    );
    let unknown = Uuid::now_v7();
    f.store
        .append_calls(
            &[
                call(purged.id, later + 1, CallOutcome::Ok),
                call(live.id, later + 2, CallOutcome::Ok),
                call(unknown, later + 3, CallOutcome::Refused),
            ],
            &[Busy {
                token_id: unknown,
                count: 4,
                at: later + 4,
            }],
        )
        .await
        .unwrap();
    assert_eq!(
        texts(&f.db, "SELECT token_id FROM token_calls").await,
        [Some(live.id.to_string())]
    );
    let state = &f.store.tokens_of(alice.id, later).await.unwrap()[0];
    assert_eq!(state.id, live.id);
    assert_eq!(state.last_used_at, Some(later + 2));
}

/// Ruling R1271 (relecture de S1) : le changement de mot de passe purge aussi les jetons morts du
/// compte, dans sa transaction : un jeton mort depuis plus de quatre-vingt-dix jours part, avec son
/// journal. Le changement date sa révocation à l'horloge du système, bien après `T0` (2023).
#[tokio::test]
async fn changing_the_password_purges_the_dead_tokens() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let dead = async || {
        let old = f
            .store
            .create_token(token(alice.id, "Ancien", TokenScope::All, T0))
            .await
            .unwrap();
        f.store
            .append_calls(&[call(old.id, T0, CallOutcome::Ok)], &[])
            .await
            .unwrap();
        f.store.revoke_token(alice.id, old.id, T0).await.unwrap();
        assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_tokens").await, 1);
        assert_eq!(int(&f.db, "SELECT COUNT(*) FROM token_calls").await, 1);
    };
    dead().await;
    assert!(
        f.store
            .change_password(alice.id, "pas-un-hachage", "neuf", [8; 32], i64::MAX)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_tokens").await, 0);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM token_calls").await, 0);

    dead().await;
    f.store.set_password(alice.id, "encore").await.unwrap();
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_tokens").await, 0);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM token_calls").await, 0);
}

/// Ruling R1270 (relecture de S1) : un jeton mort depuis quatre-vingt-dix jours ou plus ne se
/// révoque plus : introuvable, comme un jeton inconnu, sans rien écrire, même échu sans avoir été
/// révoqué. La veille, il se révoquait encore.
#[tokio::test]
async fn a_token_dead_for_ninety_days_cannot_be_revoked() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let mut expiring = token(alice.id, "Échu", TokenScope::All, T0);
    expiring.expires_at = Some(T0 + 1_000);
    let made = f.store.create_token(expiring).await.unwrap();
    let forgotten = T0 + 1_000 + DEAD_TOKEN_KEPT_MS;
    assert!(matches!(
        f.store.revoke_token(alice.id, made.id, forgotten).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        int(
            &f.db,
            "SELECT COUNT(*) FROM api_tokens WHERE revoked_at IS NOT NULL"
        )
        .await,
        0
    );
    assert!(
        f.store
            .revoke_token(alice.id, made.id, forgotten - 1)
            .await
            .unwrap()
    );
}

/// Ruling R1273 (relecture de S1) : révoquer un jeton échu, pas encore révoqué, lui pose sa date
/// de révocation (`true`) ; une seconde fois, rien (`false`). Sa mort reste son échéance : il ne se
/// lit plus quatre-vingt-dix jours après elle, même si sa révocation est plus récente.
#[tokio::test]
async fn revoking_an_expired_token_dates_its_revocation() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let mut expiring = token(alice.id, "Échu", TokenScope::All, T0);
    expiring.expires_at = Some(T0 + 1_000);
    let made = f.store.create_token(expiring).await.unwrap();
    assert!(
        f.store
            .revoke_token(alice.id, made.id, T0 + 5_000)
            .await
            .unwrap()
    );
    let listed = f.store.tokens_of(alice.id, T0 + 5_000).await.unwrap();
    assert_eq!(listed[0].revoked_at, Some(T0 + 5_000));
    assert_eq!(listed[0].expires_at, Some(T0 + 1_000));
    assert!(
        !f.store
            .revoke_token(alice.id, made.id, T0 + 6_000)
            .await
            .unwrap()
    );
    let after = T0 + 1_000 + DEAD_TOKEN_KEPT_MS;
    assert_eq!(
        f.store.tokens_of(alice.id, after - 1).await.unwrap().len(),
        1
    );
    assert!(f.store.tokens_of(alice.id, after).await.unwrap().is_empty());
}

/// La liste des jetons d'un compte (relecture de S1) : les vivants d'abord, du plus récemment
/// créé au plus anciennement, puis les morts, du plus récemment mort au plus anciennement, quelle
/// que soit leur création.
#[tokio::test]
async fn tokens_are_listed_living_first_and_newest_first() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let mut ids = Vec::new();
    for (i, name) in ["a", "b", "c", "d"].into_iter().enumerate() {
        let made = f
            .store
            .create_token(token(alice.id, name, TokenScope::All, T0 + 5 * i as i64))
            .await
            .unwrap();
        ids.push(made.id);
    }
    // « d », créé le dernier, meurt avant « a ».
    f.store
        .revoke_token(alice.id, ids[3], T0 + 20)
        .await
        .unwrap();
    f.store
        .revoke_token(alice.id, ids[0], T0 + 30)
        .await
        .unwrap();
    let names: Vec<String> = f
        .store
        .tokens_of(alice.id, T0 + 40)
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, ["c", "b", "a", "d"]);
}

// ---------------------------------------------------------------------------
// Ce que les routes des jetons demandent au store (Task S6).

/// Une création vérifiée (le mot de passe redemandé à la création, décision L2) n'écrit que si le
/// hachage du compte est encore celui que la route vient de vérifier (Ruling R120) : un changement
/// de mot de passe passé entre la vérification et l'écriture la laisse sans effet, sans rien
/// écrire ; vérifiée contre le hachage courant, elle crée ; un compte sans mot de passe n'en crée
/// aucun.
#[tokio::test]
async fn a_token_is_created_only_while_the_verified_password_holds() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let first = token(alice.id, "un", TokenScope::All, T0);
    let made = f
        .store
        .create_token_verified(first, "pas-un-hachage")
        .await
        .unwrap();
    assert_eq!(made.map(|t| t.name).as_deref(), Some("un"));

    // Le mot de passe change pendant qu'une création vérifiée contre l'ancien attend.
    f.store.set_password(alice.id, "neuf").await.unwrap();
    let stale = token(alice.id, "deux", TokenScope::All, T0);
    let stale_hash = stale.hash;
    assert_eq!(
        f.store
            .create_token_verified(stale, "pas-un-hachage")
            .await
            .unwrap(),
        None
    );
    assert_eq!(f.store.token_by_hash(stale_hash, T0).await.unwrap(), None);
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_tokens").await, 1);

    // Vérifiée contre le hachage courant : elle crée.
    let current = token(alice.id, "trois", TokenScope::All, T0);
    let current_hash = current.hash;
    assert!(
        f.store
            .create_token_verified(current, "neuf")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        f.store
            .token_by_hash(current_hash, T0)
            .await
            .unwrap()
            .is_some()
    );

    // Un compte sans mot de passe : aucun hachage vérifié ne vaut.
    exec_sql(
        &f.db,
        &format!(
            "UPDATE users SET password_hash = NULL WHERE id = '{}'",
            alice.id
        ),
    )
    .await;
    let none = token(alice.id, "quatre", TokenScope::All, T0);
    assert_eq!(
        f.store.create_token_verified(none, "neuf").await.unwrap(),
        None
    );
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM api_tokens").await, 2);
}

/// Le journal se lit par issue (l'écran « Activité », Task C2) : les seules lignes de l'issue
/// demandée, de la plus récente à la plus ancienne, avant le curseur donné ; sans issue, toutes.
#[tokio::test]
async fn the_journal_is_read_by_outcome() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let made = f
        .store
        .create_token(token(alice.id, "un", TokenScope::All, T0))
        .await
        .unwrap();
    let outcomes = [
        CallOutcome::Ok,
        CallOutcome::Refused,
        CallOutcome::Error,
        CallOutcome::Ok,
        CallOutcome::Refused,
    ];
    let calls: Vec<Call> = outcomes
        .iter()
        .enumerate()
        .map(|(i, &outcome)| call(made.id, T0 + 1 + i as i64, outcome))
        .collect();
    f.store.append_calls(&calls, &[]).await.unwrap();
    let read = |before: Option<i64>, outcome: Option<CallOutcome>, limit: usize| {
        let store = f.store.clone();
        async move {
            store
                .calls_of(alice.id, made.id, before, outcome, limit, T0)
                .await
                .unwrap()
                .into_iter()
                .map(|row| (row.id, row.call.at))
                .collect::<Vec<_>>()
        }
    };
    let ats = |rows: &[(i64, i64)]| rows.iter().map(|&(_, at)| at).collect::<Vec<_>>();
    assert_eq!(
        ats(&read(None, None, 10).await),
        [T0 + 5, T0 + 4, T0 + 3, T0 + 2, T0 + 1]
    );
    let ok = read(None, Some(CallOutcome::Ok), 10).await;
    assert_eq!(ats(&ok), [T0 + 4, T0 + 1]);
    assert_eq!(
        ats(&read(None, Some(CallOutcome::Refused), 10).await),
        [T0 + 5, T0 + 2]
    );
    assert_eq!(
        ats(&read(None, Some(CallOutcome::Error), 10).await),
        [T0 + 3]
    );
    // Le curseur vaut avec l'issue : avant la plus récente des réussies, la suivante seule.
    assert_eq!(
        ats(&read(Some(ok[0].0), Some(CallOutcome::Ok), 1).await),
        [T0 + 1]
    );
    assert_eq!(read(Some(ok[1].0), Some(CallOutcome::Ok), 10).await, []);
}

/// Un changement de mot de passe dit combien de jetons il a retirés (Task S6 : la route et la
/// commande le disent) : ceux qui vivaient, pas un jeton déjà révoqué ni un jeton échu, qui
/// reçoit pourtant sa date de révocation.
#[tokio::test]
async fn changing_the_password_counts_the_tokens_that_lived() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    // L'horloge du changement est celle du système : les jetons naissent à elle, sans quoi la
    // purge (morts depuis quatre-vingt-dix jours) retirerait l'échu et le révoqué.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    for name in ["un", "deux"] {
        f.store
            .create_token(token(alice.id, name, TokenScope::All, now))
            .await
            .unwrap();
    }
    let mut expired = token(alice.id, "échu", TokenScope::All, now - 10);
    expired.expires_at = Some(now - 1);
    let expired = f.store.create_token(expired).await.unwrap();
    let revoked = f
        .store
        .create_token(token(alice.id, "révoqué", TokenScope::All, now))
        .await
        .unwrap();
    assert!(
        f.store
            .revoke_token(alice.id, revoked.id, now)
            .await
            .unwrap()
    );

    assert_eq!(
        f.store
            .change_password(alice.id, "pas-un-hachage", "neuf", [8; 32], i64::MAX)
            .await
            .unwrap(),
        Some(2)
    );
    let tokens = f.store.tokens_of(alice.id, now).await.unwrap();
    assert_eq!(tokens.len(), 4, "{tokens:?}");
    assert!(tokens.iter().all(|t| t.revoked_at.is_some()), "{tokens:?}");
    assert!(tokens.iter().any(|t| t.id == expired.id));

    // La commande : plus rien de vivant, puis un jeton neuf.
    assert_eq!(f.store.set_password(alice.id, "encore").await.unwrap(), 0);
    f.store
        .create_token(token(alice.id, "trois", TokenScope::All, now))
        .await
        .unwrap();
    assert_eq!(f.store.set_password(alice.id, "toujours").await.unwrap(), 1);
}

/// Ruling R1607 (R1381 récrit, tour de correction de S7) : une écriture au journal au nom d'un
/// jeton n'est admise que si le jeton a une ligne vivante ; un jeton inconnu (sans ligne) est
/// refusé comme un jeton révoqué, rien d'écrit. Témoin : le jeton vivant écrit.
#[tokio::test]
async fn the_journal_refuses_a_token_without_a_row() {
    let f = fresh().await;
    let alice = account(&f.store, "alice").await;
    let node = new_page(&f, "Journal").await;
    let writer = |token: Uuid| mementum_store::Writer {
        account: Some(alice.id),
        token: Some(token),
    };
    let unknown = f
        .store
        .append_updates_by(
            node,
            &[b"une mise a jour".as_slice()],
            &writer(Uuid::now_v7()),
        )
        .await;
    assert!(
        matches!(unknown, Err(StoreError::TokenRevoked)),
        "{unknown:?}"
    );
    let entries = || int(&f.db, "SELECT COUNT(*) FROM doc_updates");
    assert_eq!(entries().await, 0);
    let live = common::new_token(&f.db, alice.id).await;
    f.store
        .append_updates_by(node, &[b"une mise a jour".as_slice()], &writer(live))
        .await
        .unwrap();
    assert_eq!(entries().await, 1);
}
