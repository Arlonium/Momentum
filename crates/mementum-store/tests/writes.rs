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
        ..StoreOptions::default()
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

/// Un jeton de session au hasard, que la base ne connaît pas.
fn unknown_token() -> [u8; 32] {
    let mut token = [0u8; 32];
    token[..16].copy_from_slice(Uuid::now_v7().as_bytes());
    token[16..].copy_from_slice(Uuid::now_v7().as_bytes());
    token
}

/// Un anonyme ne remplit pas la voie commune (Ruling R517) : fermer une session inconnue (une
/// déconnexion à cookie bidon, `close_session` d'une connexion) ne prend aucun tour, et rend
/// `None` même pendant qu'un tour est tenu ; fermer une vraie session écrit dans la voie de son
/// compte, pas dans la voie commune. Cent déconnexions de vraies sessions d'un même compte,
/// demandées hors de toute session : une inscription, voie commune, passe après une seule.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn closing_a_session_never_fills_the_common_lane() {
    const K: usize = 100;
    let f = fresh().await;
    let olga = f.store.create_user(new_user("olga")).await.unwrap().id;
    let mut tokens = Vec::new();
    for _ in 0..K {
        let token = unknown_token();
        f.store.create_session(olga, token, i64::MAX).await.unwrap();
        tokens.push(token);
    }
    exec_sql(
        &f.db,
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER witness_out AFTER DELETE ON sessions \
         BEGIN INSERT INTO witness (what) VALUES ('sortie'); END; \
         CREATE TRIGGER witness_user AFTER INSERT ON users \
         BEGIN INSERT INTO witness (what) VALUES (NEW.name); END;",
    )
    .await;
    let held = f.store.hold_writes().await;
    // Inconnues : aucun tour, réponse tout de suite.
    for _ in 0..20 {
        let closed = writes_by(None, f.store.delete_session(unknown_token()));
        let closed = tokio::time::timeout(WAIT, closed).await;
        assert_eq!(closed.expect("sans tour").unwrap(), None);
    }
    assert_eq!(f.store.queued_writes(), 0);
    // Connues : en file, dans la voie du compte.
    let mut logouts = Vec::new();
    for (i, token) in tokens.into_iter().enumerate() {
        let store = f.store.clone();
        logouts.push(tokio::spawn(writes_by(None, async move {
            store.delete_session(token).await
        })));
        queued(&f.store, i + 1).await;
    }
    let signup = {
        let store = f.store.clone();
        tokio::spawn(writes_by(None, async move {
            let session = mementum_store::NewSession {
                token_hash: unknown_token(),
                expires_at: i64::MAX,
            };
            store
                .sign_up(new_user("neuve"), None, "Espace", session, None, 1)
                .await
        }))
    };
    queued(&f.store, K + 1).await;
    drop(held);
    signup.await.unwrap().unwrap();
    for logout in logouts {
        assert_eq!(logout.await.unwrap().unwrap(), Some(olga));
    }
    let order = texts(&f.db, "SELECT what FROM witness ORDER BY n").await;
    let at = order
        .iter()
        .position(|what| what.as_deref() == Some("neuve"))
        .unwrap();
    assert_eq!(
        at, 1,
        "sorties écrites avant l'inscription : {at} (sur {K})"
    );
}

/// La voie du dépôt passe en priorité (Ruling R522) : l'envoi d'un fichier tient le verrou du
/// dépôt, commun à l'instance, jusqu'à son attache. Quatre-vingts voies d'acteur ont chacune une
/// frappe en file quand une attache arrive : elle passe la première au tour suivant, au lieu
/// d'attendre une écriture de chaque voie (tous les envois de l'instance avec elle) ; et la voie
/// du dépôt ne prend jamais deux tours de suite quand d'autres attendent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_file_store_lane_goes_first_but_never_twice_in_a_row() {
    const LANES: usize = 80;
    let f = fresh().await;
    let tom = f.store.create_user(new_user("tom")).await.unwrap().id;
    let target = f
        .store
        .create_node(page(&f.space, None, None, "Fichiers"))
        .await
        .unwrap()
        .id;
    let mut pages = Vec::new();
    for i in 0..LANES {
        let node = page(&f.space, None, None, &format!("P{i}"));
        pages.push(f.store.create_node(node).await.unwrap().id);
    }
    exec_sql(
        &f.db,
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER witness_keystroke AFTER INSERT ON doc_updates \
         BEGIN INSERT INTO witness (what) VALUES ('frappe'); END; \
         CREATE TRIGGER witness_attachment AFTER INSERT ON attachments \
         BEGIN INSERT INTO witness (what) VALUES (NEW.name); END;",
    )
    .await;
    let attach = |name: &'static str, byte: u8| {
        let store = f.store.clone();
        tokio::spawn(writes_by(Some(tom), async move {
            let new = mementum_store::NewAttachment {
                id: Uuid::now_v7(),
                node_id: target,
                hash: [byte; 32],
                size: 1,
                mime: "text/plain",
                name,
                created_by: tom,
            };
            store.attach_file(new, 1).await
        }))
    };
    let held = f.store.hold_writes().await;
    let mut typing = Vec::new();
    for (i, page) in pages.iter().copied().enumerate() {
        let store = f.store.clone();
        typing.push(tokio::spawn(async move {
            store.append_updates(page, &[b"x"], None).await
        }));
        queued(&f.store, i + 1).await;
    }
    // Deux attaches en file (un cas qui ne se produit pas sous le verrou du dépôt, qui n'en laisse
    // passer qu'une à la fois) : la première passe en tête, la seconde après une autre voie.
    let first = attach("première", 1);
    queued(&f.store, LANES + 1).await;
    let second = attach("seconde", 2);
    queued(&f.store, LANES + 2).await;
    drop(held);
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    for write in typing {
        write.await.unwrap().unwrap();
    }
    let order: Vec<String> = texts(&f.db, "SELECT what FROM witness ORDER BY n")
        .await
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(order[..3], ["première", "frappe", "seconde"], "{order:?}");
}

/// Une page neuve de l'espace de départ, avec une attache de `hash`, puis supprimée pour de bon :
/// le contenu `hash` n'est plus attaché nulle part.
async fn orphan(f: &common::Fixture, by: Uuid, hash: [u8; 32]) {
    let node = f
        .store
        .create_node(page(&f.space, None, None, "Supprimée"))
        .await
        .unwrap()
        .id;
    let new = mementum_store::NewAttachment {
        id: Uuid::now_v7(),
        node_id: node,
        hash,
        size: 1,
        mime: "text/plain",
        name: "orphelin.txt",
        created_by: by,
    };
    f.store.attach_file(new, 1).await.unwrap();
    f.store.trash_node(node, by).await.unwrap();
    f.store.delete_node(node).await.unwrap();
}

/// Le ménage écrit dans la voie du dépôt (Ruling R522 ; constat C de la re-relecture 1 de la
/// robustesse) : le serveur le mène sous le verrou du dépôt, commun à l'instance, au nom du compte
/// qui supprime une page. Tom a vingt écritures en file quand son ménage demande son tour : il
/// passe le premier, au lieu d'attendre la rafale de Tom, et tous les envois de l'instance avec
/// lui. Dans la voie de Tom, il passerait après les vingt.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sweep_writes_in_the_file_store_lane() {
    const BURST: usize = 20;
    let f = fresh().await;
    let tom = f.store.create_user(new_user("tom")).await.unwrap().id;
    orphan(&f, tom, [9; 32]).await;
    exec_sql(
        &f.db,
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER witness_space AFTER INSERT ON spaces \
         BEGIN INSERT INTO witness (what) VALUES ('Tom'); END; \
         CREATE TRIGGER witness_sweep AFTER DELETE ON files \
         BEGIN INSERT INTO witness (what) VALUES ('ménage'); END;",
    )
    .await;
    let held = f.store.hold_writes().await;
    let mut burst = Vec::new();
    for i in 0..BURST {
        burst.push(create_as(&f.store, Some(tom), format!("Tom {i:02}")));
        queued(&f.store, i + 1).await;
    }
    // La lecture des candidats n'attend aucun tour : elle passe, tour tenu.
    let candidates = f.store.unattached_files().await.unwrap();
    assert_eq!(candidates, [[9; 32]]);
    let sweep = tokio::spawn(writes_by(Some(tom), {
        let store = f.store.clone();
        async move { store.forget_unattached(&candidates).await }
    }));
    queued(&f.store, BURST + 1).await;
    drop(held);
    assert_eq!(sweep.await.unwrap().unwrap(), [[9; 32]]);
    for write in burst {
        write.await.unwrap().unwrap();
    }
    let order: Vec<String> = texts(&f.db, "SELECT what FROM witness ORDER BY n")
        .await
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(order.len(), BURST + 1, "{order:?}");
    assert_eq!(order[0], "ménage", "{order:?}");
}

/// Une recopie s'écrit dans la voie de son compte, pas dans celle du dépôt (Ruling R522) : elle
/// ne se fait pas sous le verrou du dépôt, et dans la voie du dépôt, qui passe en priorité, les
/// recopies d'un compte passeraient devant toutes les autres voies, et devant les attaches des
/// envois. Tom a dix recopies en file quand une attache arrive : elle passe la première. Dans la
/// voie du dépôt, elle attendrait les dix.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn copies_wait_in_the_lane_of_their_account() {
    const COPIES: usize = 10;
    let f = fresh().await;
    let tom = f.store.create_user(new_user("tom")).await.unwrap().id;
    let mut pages = Vec::new();
    for title in ["Source", "Cible"] {
        let node = page(&f.space, None, None, title);
        pages.push(f.store.create_node(node).await.unwrap().id);
    }
    let (source, target) = (pages[0], pages[1]);
    let attachment = |hash: [u8; 32], name: &'static str| mementum_store::NewAttachment {
        id: Uuid::now_v7(),
        node_id: target,
        hash,
        size: 1,
        mime: "text/plain",
        name,
        created_by: tom,
    };
    let original = f
        .store
        .attach_file(
            mementum_store::NewAttachment {
                node_id: source,
                ..attachment([1; 32], "recopie")
            },
            1,
        )
        .await
        .unwrap();
    exec_sql(
        &f.db,
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER witness_attachment AFTER INSERT ON attachments \
         BEGIN INSERT INTO witness (what) VALUES (NEW.name); END;",
    )
    .await;
    let held = f.store.hold_writes().await;
    let mut copies = Vec::new();
    for i in 0..COPIES {
        let store = f.store.clone();
        let from = original.id;
        copies.push(tokio::spawn(writes_by(Some(tom), async move {
            store
                .copy_attachment(source, from, Uuid::now_v7(), target, tom, 1)
                .await
        })));
        queued(&f.store, i + 1).await;
    }
    let attach = tokio::spawn(writes_by(Some(tom), {
        let store = f.store.clone();
        let new = attachment([2; 32], "envoi");
        async move { store.attach_file(new, 1).await }
    }));
    queued(&f.store, COPIES + 1).await;
    drop(held);
    attach.await.unwrap().unwrap();
    for copy in copies {
        assert!(copy.await.unwrap().unwrap().is_some());
    }
    let order: Vec<String> = texts(&f.db, "SELECT what FROM witness ORDER BY n")
        .await
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(order.len(), COPIES + 1, "{order:?}");
    assert_eq!(order[0], "envoi", "{order:?}");
}

/// Les copies de pages ont une voie à elles, commune à tous les comptes (Ruling R607) : entre
/// deux copies, la file sert une écriture de chaque autre voie qui attend. Six comptes ont chacun
/// une copie en file quand Olga demande trois écritures : elles passent une copie sur deux, et la
/// dernière n'attend que trois copies. Dans la voie de chaque compte, les six copies passeraient
/// toutes devant sa première écriture ; une frappe les attendrait de même.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn page_copies_share_one_lane_whoever_asks() {
    use mementum_acl::{Grant, Level, Role, Subject};
    use mementum_store::{CopiedPage, NewCopy, NewNode, Sibling};

    const COPIES: usize = 6;
    let f = fresh().await;
    let olga = f.store.create_user(new_user("olga")).await.unwrap().id;
    let full = Grant {
        subject: Subject::Members,
        level: Level::Full,
    };
    let source = f
        .store
        .create_node(NewNode {
            grants: vec![full],
            ..page(&f.space, None, None, "Source")
        })
        .await
        .unwrap()
        .id;
    let mut accounts = Vec::new();
    for i in 0..COPIES {
        let id = f
            .store
            .create_user(new_user(&format!("compte{i}")))
            .await
            .unwrap()
            .id;
        f.store
            .add_member(f.space.id, id, Role::Member)
            .await
            .unwrap();
        accounts.push(id);
    }
    exec_sql(
        &f.db,
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER witness_node AFTER INSERT ON nodes \
         BEGIN INSERT INTO witness (what) VALUES (NEW.title); END;",
    )
    .await;
    let held = f.store.hold_writes().await;
    let mut writes = Vec::new();
    for (i, by) in accounts.iter().copied().enumerate() {
        let store = f.store.clone();
        let copy = NewCopy {
            space_id: f.space.id,
            source,
            parent_id: None,
            after: Some(Sibling::Last),
            grants: Vec::new(),
            created_by: by,
            pages: vec![CopiedPage {
                id: Uuid::now_v7(),
                parent: None,
                title: "copie".to_string(),
                position: None,
                doc: vec![0],
                attachments: Vec::new(),
            }],
        };
        writes.push(tokio::spawn(writes_by(Some(by), async move {
            store.duplicate_pages(&copy, 1).await.map(|_| ())
        })));
        queued(&f.store, i + 1).await;
    }
    for i in 0..3 {
        let store = f.store.clone();
        let node = page(&f.space, None, None, "olga");
        writes.push(tokio::spawn(writes_by(Some(olga), async move {
            store.create_node(node).await.map(|_| ())
        })));
        queued(&f.store, COPIES + i + 1).await;
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
    assert_eq!(order.len(), COPIES + 3, "{order:?}");
    assert_eq!(
        order[..6],
        ["copie", "olga", "copie", "olga", "copie", "olga"],
        "{order:?}"
    );
}

/// Les écritures de l'index de recherche ont une voie à elles (Ruling R908), sans priorité, quel
/// que soit le compte de la tâche qui les mène : entre deux transactions de l'index, la file sert
/// une écriture de chaque autre voie qui attend. Trois transactions de l'index (une indexation de
/// page, puis deux tranches d'une reconstruction ; deux indexations ne sont jamais en file
/// ensemble, elles passent une à une par le verrou de l'indexeur, R975), chacune au nom d'un autre
/// compte, puis une écriture de la voie commune, puis trois d'Olga : l'ordre est index, commune,
/// Olga, index, Olga, index, Olga. Dans la voie de chaque compte, les trois passeraient d'affilée ;
/// dans la voie commune, l'écriture commune les attendrait toutes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn index_writes_share_one_lane() {
    use mementum_store::IndexedBlock;

    const PAGES: usize = 3;
    let f = fresh().await;
    let olga = Uuid::now_v7();
    let mut pages = Vec::new();
    for i in 0..PAGES {
        let node = page(&f.space, None, None, &format!("P{i}"));
        pages.push(f.store.create_node(node).await.unwrap().id);
    }
    // Les lignes d'outbox des pages créées : les tranches de la reconstruction les récrivent.
    exec_sql(
        &f.db,
        "DELETE FROM search_outbox; \
         CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER witness_index AFTER INSERT ON blocks_index \
         BEGIN INSERT INTO witness (what) VALUES ('index'); END; \
         CREATE TRIGGER witness_rebuild AFTER INSERT ON search_outbox WHEN NEW.block_id = '' \
         BEGIN INSERT INTO witness (what) VALUES ('index'); END; \
         CREATE TRIGGER witness_space AFTER INSERT ON spaces \
         BEGIN INSERT INTO witness (what) VALUES (NEW.name); END;",
    )
    .await;
    let held = f.store.hold_writes().await;
    let mut writes = Vec::new();
    writes.push(tokio::spawn(writes_by(Some(Uuid::now_v7()), {
        let store = f.store.clone();
        let node = pages[0];
        async move {
            let links: [Uuid; 0] = [];
            let row = IndexedBlock {
                block_id: "a",
                kind: "paragraph",
                text: "texte",
                hash: &[1],
                links: &links,
            };
            store.index_page(node, 1, &[row], false).await.map(|_| ())
        }
    })));
    queued(&f.store, 1).await;
    for (i, after) in [None, Some(pages[0])].into_iter().enumerate() {
        let store = f.store.clone();
        writes.push(tokio::spawn(writes_by(Some(Uuid::now_v7()), async move {
            store.outbox_everything(after, 1).await.map(|_| ())
        })));
        queued(&f.store, i + 2).await;
    }
    writes.push(tokio::spawn({
        let store = f.store.clone();
        writes_by(None, async move {
            store.create_space("commune", None).await.map(|_| ())
        })
    }));
    queued(&f.store, PAGES + 1).await;
    for i in 0..3 {
        writes.push(tokio::spawn({
            let store = f.store.clone();
            writes_by(Some(olga), async move {
                store.create_space("olga", None).await.map(|_| ())
            })
        }));
        queued(&f.store, PAGES + i + 2).await;
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
        ["index", "commune", "olga", "index", "olga", "index", "olga"],
        "{order:?}"
    );
}
