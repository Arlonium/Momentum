//! La file d'écriture du store (Rulings R510 à R516) : SQLite n'a qu'un écrivain à la fois, et
//! son verrou d'écriture n'est pas une file. Qui l'attend le redemande par intervalles, et
//! échoue (`database is locked`) au bout du délai d'attente de sa connexion ; qui le rend puis
//! le redemande aussitôt le reprend. Les écritures du store attendent donc leur tour dans une
//! file à lui, en mémoire, sans délai, et une seule connexion écrit.
//!
//! Ces tests posent un disque lent ([`StoreOptions::write_delay`]) et un délai d'attente court
//! ([`StoreOptions::busy_timeout`]) : ce qui ne se voyait que par moments sur la CI sous Windows
//! (Ruling R485) devient sûr, et rapide.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::tmp::TestDir;
use common::{exec_sql, fresh, int, page, texts};
use mementum_store::{NewUser, Space, Store, StoreError, StoreOptions, writes_by};
use uuid::Uuid;

/// Délai maximal d'une attente de condition.
const WAIT: Duration = Duration::from_secs(10);

/// Un compte de test (le store ne voit que des hachages).
fn new_user(tag: &str) -> NewUser {
    NewUser {
        email: format!("{tag}@exemple.test"),
        name: tag.to_string(),
        password_hash: "pas-un-hachage".into(),
    }
}

/// Le délai d'attente d'une connexion sur le verrou, dans ces tests.
const SHORT_WAIT: Duration = Duration::from_millis(100);

/// Ce que dure chaque validation, verrou tenu : un disque lent.
const SLOW: Duration = Duration::from_millis(50);

/// Un store sur un dossier neuf, avec un disque lent et un délai d'attente court.
async fn slow_store(dir: &TestDir) -> Store {
    let options = StoreOptions {
        busy_timeout: SHORT_WAIT,
        write_delay: SLOW,
    };
    Store::open_with(&dir.path().join("m.db"), options)
        .await
        .unwrap()
}

/// Dix écritures lancées ensemble, chacune tenant le verrou 50 ms : la dernière servie attend
/// au moins neuf validations, 450 ms, bien plus que le délai d'attente d'une connexion (100 ms,
/// et jusqu'à 250 ms sous Windows, où chaque pause de SQLite dure au moins 15 ms). Quel que soit
/// l'ordre où le verrou les sert, l'une d'elles attend donc plus que ce délai : sans file,
/// elle échoue (`database is locked`). Avec la file, aucune n'attend le verrou de SQLite :
/// toutes aboutissent, à leur tour.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_wait_their_turn_beyond_the_busy_timeout() {
    const WRITERS: usize = 10;
    let dir = TestDir::new();
    let store = slow_store(&dir).await;
    let writes: Vec<_> = (0..WRITERS)
        .map(|i| {
            let store = store.clone();
            tokio::spawn(async move { store.create_space(&format!("Espace {i}"), None).await })
        })
        .collect();
    let mut failed = Vec::new();
    for write in writes {
        if let Err(error) = write.await.unwrap() {
            failed.push(error.to_string());
        }
    }
    assert!(failed.is_empty(), "écritures échouées : {failed:?}");
    let db = dir.path().join("m.db");
    assert_eq!(
        int(&db, "SELECT COUNT(*) FROM spaces").await,
        WRITERS as i64
    );
    store.close().await;
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

/// Le nom des espaces créés, dans l'ordre de leur écriture (`rowid` croît à chaque insertion).
async fn written(db: &Path) -> Vec<String> {
    texts(db, "SELECT name FROM spaces ORDER BY rowid")
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// Lance la création de l'espace `name`, ses écritures attribuées à `account`.
fn create_as(
    store: &Store,
    account: Option<Uuid>,
    name: String,
) -> tokio::task::JoinHandle<Result<Space, StoreError>> {
    let store = store.clone();
    tokio::spawn(writes_by(account, async move {
        store.create_space(&name, None).await
    }))
}

/// L'équité entre comptes : Tom a cent écritures en file quand Mona en demande une. Mona passe
/// juste après la première de Tom (la ronde : Tom, Mona, Tom…), pas après les cent ; et celles
/// de Tom gardent leur ordre d'arrivée.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_account_waits_one_write_of_another_not_its_whole_burst() {
    let f = fresh().await;
    let (tom, mona) = (Uuid::now_v7(), Uuid::now_v7());
    let held = f.store.hold_writes().await;
    let mut burst = Vec::new();
    for i in 0..100 {
        burst.push(create_as(&f.store, Some(tom), format!("Tom {i:03}")));
        // Une à une : l'ordre d'arrivée dans la file est celui des noms.
        queued(&f.store, i + 1).await;
    }
    let alone = create_as(&f.store, Some(mona), "Mona".into());
    queued(&f.store, 101).await;
    drop(held);
    alone.await.unwrap().unwrap();
    for write in burst {
        write.await.unwrap().unwrap();
    }
    let mut expected = vec!["Essais".to_string(), "Tom 000".into(), "Mona".into()];
    expected.extend((1..100).map(|i| format!("Tom {i:03}")));
    assert_eq!(written(&f.db).await, expected);
}

/// L'acteur d'une page a sa voie : sa frappe, demandée pendant une rafale d'un compte, passe
/// après une écriture de la rafale, même quand la frappe est écrite au nom de ce compte. Sur la
/// voie du compte, elle attendrait toute la rafale, et l'acteur avec elle, donc la frappe de
/// tous les autres sur la page.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_page_actor_waits_one_write_of_a_burst() {
    let f = fresh().await;
    let tom = f.store.create_user(new_user("tom")).await.unwrap().id;
    let page = f
        .store
        .create_node(page(&f.space, None, None, "Page"))
        .await
        .unwrap()
        .id;
    exec_sql(
        &f.db,
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER witness_space AFTER INSERT ON spaces \
         BEGIN INSERT INTO witness (what) VALUES (NEW.name); END; \
         CREATE TRIGGER witness_keystroke AFTER INSERT ON doc_updates \
         BEGIN INSERT INTO witness (what) VALUES ('frappe'); END;",
    )
    .await;
    let held = f.store.hold_writes().await;
    let mut burst = Vec::new();
    for i in 0..20 {
        burst.push(create_as(&f.store, Some(tom), format!("Tom {i:02}")));
        queued(&f.store, i + 1).await;
    }
    let keystroke = tokio::spawn({
        let store = f.store.clone();
        writes_by(Some(tom), async move {
            store.append_updates(page, &[b"x"], Some(tom)).await
        })
    });
    queued(&f.store, 21).await;
    drop(held);
    keystroke.await.unwrap().unwrap();
    for write in burst {
        write.await.unwrap().unwrap();
    }
    let order = texts(&f.db, "SELECT what FROM witness ORDER BY n").await;
    assert_eq!(
        order[..3],
        [
            Some("Tom 00".into()),
            Some("frappe".into()),
            Some("Tom 01".into())
        ]
    );
}

/// Une écriture qui attend son tour ne tient aucune connexion : quarante écritures en file, plus
/// que le pool des lectures n'a de connexions, laissent lire.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_waiting_their_turn_leave_reads_alone() {
    let f = fresh().await;
    let held = f.store.hold_writes().await;
    let burst: Vec<_> = (0..40)
        .map(|i| create_as(&f.store, None, format!("Espace {i}")))
        .collect();
    queued(&f.store, 40).await;
    for _ in 0..20 {
        assert!(f.store.get_space(f.space.id).await.unwrap().is_some());
    }
    drop(held);
    for write in burst {
        write.await.unwrap().unwrap();
    }
    assert_eq!(written(&f.db).await.len(), 41);
}

/// Une écriture abandonnée pendant qu'elle attend (une requête dont le client coupe) sort de la
/// file sans rien écrire, et ne retient personne.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_abandoned_in_the_queue_writes_nothing_and_holds_no_one() {
    let f = fresh().await;
    let held = f.store.hold_writes().await;
    let abandoned = create_as(&f.store, None, "Abandonné".into());
    let kept = create_as(&f.store, None, "Gardé".into());
    queued(&f.store, 2).await;
    abandoned.abort();
    assert!(abandoned.await.unwrap_err().is_cancelled());
    queued(&f.store, 1).await;
    drop(held);
    kept.await.unwrap().unwrap();
    assert_eq!(written(&f.db).await, ["Essais", "Gardé"]);
    assert_eq!(f.store.queued_writes(), 0);
}

/// Aucune écriture hors de la file : chaque connexion du pool des lectures refuse d'écrire
/// (`query_only`), et une seule connexion écrit. Une écriture du store qui passerait par le pool
/// des lectures échouerait donc à chaque passage de la suite, au lieu de contourner la file.
#[tokio::test]
async fn no_write_bypasses_the_queue() {
    let f = fresh().await;
    let all = f.store.pragmas(10).await.unwrap();
    let (reads, writes) = all.split_at(10);
    assert!(reads.iter().all(|p| p.4 == 1), "{reads:?}");
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].4, 0, "la connexion qui écrit");
}
