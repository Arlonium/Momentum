//! L'index de recherche, côté base (lot 4, Task S1) : la projection d'une page dans
//! `blocks_index` et `page_links`, écrite par différence (Ruling R919) ; les marqueurs durables
//! des écritures qui changent l'index sans passer par un acteur (R918) ; l'intention, la
//! validation et la clôture (R909), côté base ; les rétroliens.

mod common;

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{Fixture, account, break_table, exec_sql, fresh, int, page, repair, side_pool, texts};
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_store::{
    CopiedPage, FeedLine, FeedStart, FeedTokens, IndexRow, IndexedBlock, MAX_INDEX_TX_BYTES,
    NewCopy, NewNode, NodeKind, Place, Sibling, Store, StoreError,
};
use sqlx::Row as _;
use tokio::sync::broadcast::error::TryRecvError;
use uuid::Uuid;

/// Délai maximal d'une attente de condition.
const WAIT: Duration = Duration::from_secs(10);

/// Un bloc projeté, tel que l'indexeur le donnerait : son empreinte couvre son type, son texte et
/// ses liens.
#[derive(Clone)]
struct Row {
    block_id: String,
    kind: String,
    text: String,
    hash: Vec<u8>,
    links: Vec<Uuid>,
}

impl Row {
    fn view(&self) -> IndexedBlock<'_> {
        IndexedBlock {
            block_id: &self.block_id,
            kind: &self.kind,
            text: &self.text,
            hash: &self.hash,
            links: &self.links,
        }
    }
}

fn linking(block_id: &str, text: &str, links: &[Uuid]) -> Row {
    let kind = "paragraph".to_string();
    let mut hasher = DefaultHasher::new();
    (&kind, text, links).hash(&mut hasher);
    Row {
        block_id: block_id.to_string(),
        kind,
        text: text.to_string(),
        hash: hasher.finish().to_le_bytes().to_vec(),
        links: links.to_vec(),
    }
}

fn row(block_id: &str, text: &str) -> Row {
    linking(block_id, text, &[])
}

/// Indexe `rows` pour la page `node` au `seq` donné, et rend le nombre de blocs écrits.
async fn index(store: &Store, node: Uuid, seq: i64, rows: &[Row]) -> usize {
    let views: Vec<IndexedBlock<'_>> = rows.iter().map(Row::view).collect();
    store
        .index_page(node, seq, &views, false)
        .await
        .unwrap()
        .changed
}

/// Une page de l'espace de départ.
async fn new_page(f: &Fixture, title: &str) -> Uuid {
    f.store
        .create_node(page(&f.space, None, None, title))
        .await
        .unwrap()
        .id
}

/// Les lignes de `search_outbox`, triées : `(node_id, block_id, jeton)`.
async fn outbox(db: &Path) -> Vec<(String, String, Option<String>)> {
    let pool = side_pool(db).await;
    let rows = sqlx::query(
        "SELECT node_id, block_id, token FROM search_outbox ORDER BY node_id, block_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .iter()
    .map(|r| (r.get(0), r.get(1), r.get(2)))
    .collect();
    pool.close().await;
    rows
}

/// Une ligne d'outbox sans jeton.
fn waiting(node: Uuid, block: &str) -> (String, String, Option<String>) {
    (node.to_string(), block.to_string(), None)
}

/// La ligne `search_state` de la page : `(indexed_seq, failed_seq, truncated)`.
async fn state(db: &Path, node: Uuid) -> Option<(i64, Option<i64>, i64)> {
    let pool = side_pool(db).await;
    let found = sqlx::query(
        "SELECT indexed_seq, failed_seq, truncated FROM search_state WHERE node_id = ?",
    )
    .bind(node.to_string())
    .fetch_optional(&pool)
    .await
    .unwrap()
    .map(|r| (r.get(0), r.get(1), r.get(2)));
    pool.close().await;
    found
}

/// Les lignes `blocks_index` de la page : identifiant de bloc et texte.
async fn indexed(db: &Path, node: Uuid) -> BTreeMap<String, String> {
    let pool = side_pool(db).await;
    let rows = sqlx::query("SELECT block_id, text FROM blocks_index WHERE node_id = ?")
        .bind(node.to_string())
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    pool.close().await;
    rows
}

/// Les liens de la page : `(block_id, target_id)`, triés.
async fn links(db: &Path, node: Uuid) -> Vec<(String, String)> {
    let pool = side_pool(db).await;
    let rows = sqlx::query(
        "SELECT block_id, target_id FROM page_links WHERE node_id = ? ORDER BY block_id, target_id",
    )
    .bind(node.to_string())
    .fetch_all(&pool)
    .await
    .unwrap()
    .iter()
    .map(|r| (r.get(0), r.get(1)))
    .collect();
    pool.close().await;
    rows
}

/// L'outbox vidée, comme après la clôture d'une validation.
async fn clear_outbox(db: &Path) {
    exec_sql(db, "DELETE FROM search_outbox").await;
}

/// Attend que `n` écritures soient en file ([`WAIT`] au plus) : une condition, pas une durée.
async fn queued(store: &Store, n: usize) {
    let deadline = std::time::Instant::now() + WAIT;
    while store.queued_writes() != n {
        assert!(
            std::time::Instant::now() < deadline,
            "{} écritures en file, {n} attendues",
            store.queued_writes()
        );
        tokio::task::yield_now().await;
    }
}

/// La première colonne, en texte, de `SELECT what FROM witness ORDER BY n`.
async fn witness(db: &Path) -> Vec<String> {
    texts(db, "SELECT what FROM witness ORDER BY n")
        .await
        .into_iter()
        .flatten()
        .collect()
}

/// Ruling R919 : seule la différence est écrite. Une frappe dans un bloc d'une page de 1 000 blocs
/// récrit une ligne de `blocks_index` (un déclencheur témoin compte les insertions, mises à jour et
/// suppressions de `blocks_index` et de `page_links`), et pose **une** ligne d'outbox, celle du
/// bloc : jamais celle de la page entière. Les mêmes blocs relus plus tard n'écrivent rien, et la
/// page est notée au `seq` lu. Témoin : la première indexation en écrit mille.
#[tokio::test]
async fn index_page_writes_only_what_changed() {
    let f = fresh().await;
    let node = new_page(&f, "Grande page").await;
    let mut rows: Vec<Row> = (0..1000)
        .map(|i| row(&format!("b{i:04}"), &format!("texte {i}")))
        .collect();
    assert_eq!(index(&f.store, node, 1, &rows).await, 1000);
    assert_eq!(indexed(&f.db, node).await.len(), 1000);
    clear_outbox(&f.db).await;
    exec_sql(
        &f.db,
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER w_ins AFTER INSERT ON blocks_index \
         BEGIN INSERT INTO witness (what) VALUES ('bloc ' || NEW.block_id); END; \
         CREATE TRIGGER w_upd AFTER UPDATE ON blocks_index \
         BEGIN INSERT INTO witness (what) VALUES ('bloc ' || NEW.block_id); END; \
         CREATE TRIGGER w_del AFTER DELETE ON blocks_index \
         BEGIN INSERT INTO witness (what) VALUES ('bloc ' || OLD.block_id); END; \
         CREATE TRIGGER w_link_ins AFTER INSERT ON page_links \
         BEGIN INSERT INTO witness (what) VALUES ('lien'); END; \
         CREATE TRIGGER w_link_del AFTER DELETE ON page_links \
         BEGIN INSERT INTO witness (what) VALUES ('lien'); END;",
    )
    .await;

    rows[500] = row("b0500", "texte 500, et une frappe");
    assert_eq!(index(&f.store, node, 2, &rows).await, 1);
    assert_eq!(witness(&f.db).await, ["bloc b0500"]);
    assert_eq!(outbox(&f.db).await, [waiting(node, "b0500")]);
    assert_eq!(state(&f.db, node).await, Some((2, None, 0)));
    assert_eq!(
        indexed(&f.db, node).await["b0500"],
        "texte 500, et une frappe"
    );

    // Les mêmes blocs, relus à un `seq` plus loin (une marque changée, que la projection ne voit
    // pas) : aucune ligne, seul le `seq` avance.
    assert_eq!(index(&f.store, node, 3, &rows).await, 0);
    assert_eq!(witness(&f.db).await, ["bloc b0500"]);
    assert_eq!(outbox(&f.db).await, [waiting(node, "b0500")]);
    assert_eq!(state(&f.db, node).await, Some((3, None, 0)));
}

/// Un bloc parti quitte `blocks_index` et `page_links`, et l'outbox le nomme ; un lien changé
/// remplace l'ancien.
#[tokio::test]
async fn index_page_removes_the_blocks_that_left() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    let (t1, t2, t3) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let rows = [
        linking("a", "vers une", &[t1]),
        linking("b", "vers deux", &[t2]),
        row("c", "sans lien"),
    ];
    index(&f.store, node, 1, &rows).await;
    assert_eq!(
        links(&f.db, node).await,
        [
            ("a".to_string(), t1.to_string()),
            ("b".to_string(), t2.to_string())
        ]
    );
    clear_outbox(&f.db).await;

    let rows = [linking("a", "vers trois", &[t3]), row("c", "sans lien")];
    assert_eq!(index(&f.store, node, 2, &rows).await, 2);
    let kept: Vec<String> = indexed(&f.db, node).await.into_keys().collect();
    assert_eq!(kept, ["a", "c"]);
    assert_eq!(
        links(&f.db, node).await,
        [("a".to_string(), t3.to_string())]
    );
    assert_eq!(
        outbox(&f.db).await,
        [waiting(node, "a"), waiting(node, "b")]
    );
}

/// Une lecture plus ancienne que ce que l'index reflète ne l'écrase jamais : ni ses blocs, ni son
/// `seq`, ni un échec ; le même `seq` relu, lui, s'écrit (une projection changée, pas la page).
#[tokio::test]
async fn an_older_read_never_overwrites_a_newer_index() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    index(&f.store, node, 5, &[row("x", "neuf")]).await;
    clear_outbox(&f.db).await;

    let older = [row("x", "ancien"), row("y", "ancien aussi")];
    assert_eq!(index(&f.store, node, 3, &older).await, 0);
    f.store.index_failed(node, 4).await.unwrap();
    assert_eq!(
        indexed(&f.db, node).await,
        BTreeMap::from([("x".to_string(), "neuf".to_string())])
    );
    assert_eq!(state(&f.db, node).await, Some((5, None, 0)));
    assert!(outbox(&f.db).await.is_empty());

    // Témoin : au même `seq`, la différence s'écrit.
    let same = [row("x", "neuf"), row("y", "ajouté")];
    assert_eq!(index(&f.store, node, 5, &same).await, 1);
    assert_eq!(indexed(&f.db, node).await.len(), 2);
}

/// Une lecture plus ancienne se juge avant de prendre un tour : elle rend sans attendre pendant
/// qu'un tour est tenu, et un échec plus ancien aussi.
#[tokio::test]
async fn an_older_read_takes_no_write_turn() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    index(&f.store, node, 5, &[row("x", "neuf")]).await;
    let held = f.store.hold_writes().await;
    let older = async {
        assert_eq!(index(&f.store, node, 3, &[row("x", "ancien")]).await, 0);
        f.store.index_failed(node, 4).await.unwrap();
    };
    tokio::time::timeout(WAIT, older)
        .await
        .expect("une lecture plus ancienne a attendu un tour d'écriture");
    drop(held);
    assert_eq!(indexed(&f.db, node).await["x"], "neuf");
}

/// Le `seq` est relu dans la transaction : un `seq` plus récent, posé pendant qu'une indexation
/// plus ancienne attendait son tour (elle avait lu l'état d'avant), l'emporte, pour les blocs
/// comme pour un échec. (Le verrou de l'indexeur, R975, fait qu'aucune autre indexation ne le
/// pose en vrai : la garde reste, en filet, contre ce qu'une autre connexion écrit.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_newer_index_written_while_waiting_its_turn_wins() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    index(&f.store, node, 1, &[row("x", "un")]).await;
    // Ce qu'une indexation plus récente aurait validé entre-temps (par une autre connexion : le
    // tour de la file tenu ne tient pas le verrou de SQLite).
    let newer = |seq: i64| {
        format!(
            "UPDATE blocks_index SET text = 'récent' WHERE node_id = '{node}'; \
             UPDATE search_state SET indexed_seq = {seq} WHERE node_id = '{node}';"
        )
    };

    let held = f.store.hold_writes().await;
    let older = tokio::spawn({
        let store = f.store.clone();
        async move { index(&store, node, 3, &[row("x", "trois"), row("y", "trois")]).await }
    });
    queued(&f.store, 1).await;
    exec_sql(&f.db, &newer(5)).await;
    drop(held);
    assert_eq!(older.await.unwrap(), 0);

    let held = f.store.hold_writes().await;
    let failed = tokio::spawn({
        let store = f.store.clone();
        async move { store.index_failed(node, 6).await }
    });
    queued(&f.store, 1).await;
    exec_sql(&f.db, &newer(7)).await;
    drop(held);
    failed.await.unwrap().unwrap();
    assert_eq!(
        indexed(&f.db, node).await,
        BTreeMap::from([("x".to_string(), "récent".to_string())])
    );
    assert_eq!(state(&f.db, node).await, Some((7, None, 0)));
}

/// Ruling R975 (relecture de S1, I1) : deux indexations de front sur une page ne laissent pas un
/// index faux noté à jour. A (au `seq` 5) retire `y` ; B (au `seq` 7, où `y` est revenu, de même
/// empreinte) a demandé à écrire pendant que A attendait son tour. Sans le verrou de l'indexeur,
/// B lisait les empreintes d'avant A, tenait `y` pour inchangé, et notait la page à 7 sans `y`.
/// Avec lui, B attend la fin de A, sans tour d'écriture, puis lit ce que A a laissé.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_indexings_of_a_page_at_once_leave_it_right() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    let both = [row("x", "un"), row("y", "deux")];
    index(&f.store, node, 1, &both).await;
    let held = f.store.hold_writes().await;
    let first = tokio::spawn({
        let store = f.store.clone();
        async move { index(&store, node, 5, &[row("x", "un")]).await }
    });
    queued(&f.store, 1).await;
    let second = tokio::spawn({
        let store = f.store.clone();
        async move { index(&store, node, 7, &both).await }
    });
    // B attend : en file d'écriture (sans le verrou), ou au verrou de l'indexeur.
    let deadline = std::time::Instant::now() + WAIT;
    while f.store.queued_writes() != 2 && f.store.index_waits() != 1 {
        assert!(
            std::time::Instant::now() < deadline,
            "la seconde n'attend nulle part"
        );
        tokio::task::yield_now().await;
    }
    let queued_then = f.store.queued_writes();
    drop(held);
    assert_eq!(first.await.unwrap(), 1);
    second.await.unwrap();
    let kept: Vec<String> = indexed(&f.db, node).await.into_keys().collect();
    assert_eq!(kept, ["x", "y"]);
    assert_eq!(state(&f.db, node).await, Some((7, None, 0)));
    assert_eq!(queued_then, 1, "la seconde attendait le verrou, sans tour");
}

/// Une page plus grande que la borne d'une transaction s'écrit en plusieurs, dans la voie de
/// l'index, et seule la dernière note son `seq` : une écriture d'une autre voie, servie entre deux
/// tranches, lit une partie des blocs et une page encore jamais indexée.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_page_over_the_transaction_bound_is_written_in_slices_and_marked_at_the_last() {
    let f = fresh().await;
    let node = new_page(&f, "Lourde").await;
    // Six blocs de 60 Kio : quatre tiennent sous la borne de 256 Kio, le cinquième la passe.
    let text = "a".repeat(60 * 1024);
    let rows: Vec<Row> = (0..6).map(|i| row(&format!("b{i}"), &text)).collect();
    assert!(6 * text.len() > MAX_INDEX_TX_BYTES);
    exec_sql(
        &f.db,
        &format!(
            "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, rows INTEGER, seq INTEGER); \
             CREATE TRIGGER probe AFTER INSERT ON spaces BEGIN \
             INSERT INTO witness (rows, seq) VALUES ( \
                 (SELECT COUNT(*) FROM blocks_index WHERE node_id = '{node}'), \
                 (SELECT indexed_seq FROM search_state WHERE node_id = '{node}')); END;"
        ),
    )
    .await;
    let held = f.store.hold_writes().await;
    let indexing = tokio::spawn({
        let store = f.store.clone();
        async move { index(&store, node, 7, &rows).await }
    });
    queued(&f.store, 1).await;
    let probe = tokio::spawn({
        let store = f.store.clone();
        async move { store.create_space("Sonde", None).await }
    });
    queued(&f.store, 2).await;
    drop(held);
    assert_eq!(indexing.await.unwrap(), 6);
    probe.await.unwrap().unwrap();

    let pool = side_pool(&f.db).await;
    let seen: Vec<(i64, i64)> = sqlx::query("SELECT rows, seq FROM witness ORDER BY n")
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    pool.close().await;
    assert_eq!(seen, [(4, -1)], "entre deux tranches");
    assert_eq!(indexed(&f.db, node).await.len(), 6);
    assert_eq!(state(&f.db, node).await, Some((7, None, 0)));
}

/// Le `seq` est relu dans **chaque** tranche, pas seulement la première (relecture de S1, I3 b) :
/// un `seq` plus récent posé entre deux tranches (par une écriture d'une autre voie, servie entre
/// elles) arrête l'écriture ; les tranches d'après ne s'écrivent pas, et le `seq` posé reste.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_newer_seq_set_between_two_slices_stops_the_writing() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    exec_sql(
        &f.db,
        &format!(
            "CREATE TRIGGER newer AFTER INSERT ON spaces BEGIN \
             UPDATE search_state SET indexed_seq = 9 WHERE node_id = '{node}'; END;"
        ),
    )
    .await;
    // Quatre blocs de 100 octets, deux par tranche sous une borne de 400 octets.
    let rows: Vec<Row> = (0..4)
        .map(|i| row(&format!("b{i}"), &"t".repeat(100)))
        .collect();
    let held = f.store.hold_writes().await;
    let indexing = tokio::spawn({
        let store = f.store.clone();
        async move {
            let views: Vec<IndexedBlock<'_>> = rows.iter().map(Row::view).collect();
            store.index_page_bounded(node, 7, &views, false, 400).await
        }
    });
    queued(&f.store, 1).await;
    let probe = tokio::spawn({
        let store = f.store.clone();
        async move { store.create_space("Sonde", None).await }
    });
    queued(&f.store, 2).await;
    drop(held);
    assert_eq!(indexing.await.unwrap().unwrap().changed, 2);
    probe.await.unwrap().unwrap();
    let kept: Vec<String> = indexed(&f.db, node).await.into_keys().collect();
    assert_eq!(kept, ["b0", "b1"]);
    assert_eq!(state(&f.db, node).await, Some((9, None, 0)));
}

/// Une page plus grande que la borne, quand c'est par une suppression : les blocs partis
/// comptent, et la dernière tranche note le `seq`.
#[tokio::test]
async fn removals_are_sliced_too() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    let rows: Vec<Row> = (0..40)
        .map(|i| row(&format!("b{i:02}"), &format!("bloc {i}")))
        .collect();
    index(&f.store, node, 1, &rows).await;
    let views: Vec<IndexedBlock<'_>> = rows[..1].iter().map(Row::view).collect();
    let written = f
        .store
        .index_page_bounded(node, 2, &views, true, 512)
        .await
        .unwrap();
    assert_eq!(written.changed, 39);
    assert_eq!(indexed(&f.db, node).await.len(), 1);
    assert_eq!(state(&f.db, node).await, Some((2, None, 1)));
}

/// Des lignes qui ne se rangent pas sous une clé de bloc (vide : la page entière, dans l'outbox ;
/// répétée) sont refusées, sans rien écrire.
#[tokio::test]
async fn rows_with_an_empty_or_repeated_key_are_refused() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    for rows in [
        vec![row("", "sans clé")],
        vec![row("a", "un"), row("a", "deux")],
    ] {
        let views: Vec<IndexedBlock<'_>> = rows.iter().map(Row::view).collect();
        let refused = f.store.index_page(node, 1, &views, false).await;
        assert!(matches!(refused, Err(StoreError::Sqlx(_))), "{refused:?}");
    }
    assert!(indexed(&f.db, node).await.is_empty());
    assert_eq!(state(&f.db, node).await, Some((-1, None, 0)));
}

/// Ruling R918 : une page créée naît jamais indexée (`indexed_seq = -1`), avec sa ligne d'outbox
/// de page entière (son titre), dans la transaction qui la crée, qu'elle ait un document ou non
/// (R961) ; un dossier n'est pas cherché, et ne laisse rien. [`Store::never_indexed`] les lit.
#[tokio::test]
async fn a_created_page_is_born_never_indexed() {
    let f = fresh().await;
    let with_doc = f
        .store
        .create_node(NewNode {
            doc: Some(b"etat".to_vec()),
            ..page(&f.space, None, None, "Avec document")
        })
        .await
        .unwrap()
        .id;
    let without_doc = new_page(&f, "Sans document").await;
    let folder = f
        .store
        .create_node(NewNode {
            kind: NodeKind::Folder,
            ..page(&f.space, None, None, "Dossier")
        })
        .await
        .unwrap()
        .id;
    for node in [with_doc, without_doc] {
        assert_eq!(state(&f.db, node).await, Some((-1, None, 0)));
    }
    assert_eq!(state(&f.db, folder).await, None);
    assert_eq!(
        outbox(&f.db).await,
        [waiting(with_doc, ""), waiting(without_doc, "")]
    );
    assert_eq!(
        f.store.never_indexed(10).await.unwrap(),
        [with_doc, without_doc]
    );
    assert_eq!(f.store.never_indexed(1).await.unwrap(), [with_doc]);
    // Indexée, elle n'est plus « jamais indexée ».
    index(&f.store, with_doc, 0, &[row("a", "texte")]).await;
    assert_eq!(f.store.never_indexed(10).await.unwrap(), [without_doc]);
}

/// Ruling R918 : chaque page d'une copie naît jamais indexée, avec sa ligne d'outbox de page
/// entière, dans la transaction de la copie.
#[tokio::test]
async fn a_duplicated_page_and_its_subpages_are_born_never_indexed() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    f.store
        .add_member(f.space.id, olga, Role::Member)
        .await
        .unwrap();
    let source = f
        .store
        .create_node(NewNode {
            grants: vec![Grant {
                subject: Subject::Members,
                level: Level::Full,
            }],
            ..page(&f.space, None, None, "Source")
        })
        .await
        .unwrap()
        .id;
    clear_outbox(&f.db).await;
    let pages = vec![
        CopiedPage {
            id: Uuid::now_v7(),
            parent: None,
            title: "Copie".to_string(),
            position: None,
            doc: vec![0],
            attachments: Vec::new(),
        },
        CopiedPage {
            id: Uuid::now_v7(),
            parent: Some(0),
            title: "Sous-page".to_string(),
            position: None,
            doc: vec![0],
            attachments: Vec::new(),
        },
    ];
    let (root, child) = (pages[0].id, pages[1].id);
    let copy = NewCopy {
        space_id: f.space.id,
        source,
        parent_id: None,
        after: Some(Sibling::Last),
        grants: vec![Grant {
            subject: Subject::User(olga),
            level: Level::Full,
        }],
        created_by: olga,
        pages,
    };
    f.store.duplicate_pages(&copy, 1).await.unwrap();
    for node in [root, child] {
        assert_eq!(state(&f.db, node).await, Some((-1, None, 0)));
    }
    let mut born = vec![waiting(root, ""), waiting(child, "")];
    born.sort();
    assert_eq!(outbox(&f.db).await, born);
    let never = f.store.never_indexed(10).await.unwrap();
    assert!(never.contains(&root) && never.contains(&child), "{never:?}");
}

/// Ruling R918 : le marqueur d'un renommage est écrit dans la transaction du renommage. La
/// transaction abandonnée (le marqueur refusé, ou la version de l'espace refusée) : le titre ne
/// change pas, et aucune ligne n'est posée. Validée (le témoin) : une ligne de page entière, une
/// seule même après un second renommage, et qui perd le jeton d'une intention qui l'avait prise.
/// Un titre récrit à l'identique ne marque rien.
#[tokio::test]
async fn a_rename_puts_the_page_in_the_outbox_in_its_transaction() {
    let f = fresh().await;
    let node = new_page(&f, "Ancien").await;
    clear_outbox(&f.db).await;

    for (table, event) in [("search_outbox", "INSERT"), ("spaces", "UPDATE")] {
        break_table(&f.db, event, table, "1").await;
        assert!(f.store.rename_node(node, "Neuf", None).await.is_err());
        repair(&f.db).await;
        let title = f.store.get_node(node).await.unwrap().unwrap().title;
        assert_eq!(title, "Ancien", "panne sur {table}");
        assert!(outbox(&f.db).await.is_empty(), "panne sur {table}");
    }

    f.store.rename_node(node, "Neuf", None).await.unwrap();
    assert_eq!(outbox(&f.db).await, [waiting(node, "")]);
    let FeedStart::Begun(feed) = f.store.begin_feed(10).await.unwrap() else {
        panic!("une intention");
    };
    assert_eq!(
        outbox(&f.db).await[0].2.as_deref(),
        Some(feed.token.as_str())
    );
    f.store.rename_node(node, "Plus neuf", None).await.unwrap();
    assert_eq!(outbox(&f.db).await, [waiting(node, "")]);

    f.store.undo_feed().await.unwrap();
    clear_outbox(&f.db).await;
    f.store.rename_node(node, "Plus neuf", None).await.unwrap();
    assert!(outbox(&f.db).await.is_empty());
}

/// Ruling R918 : un déplacement sans titre neuf ne marque rien (le fil d'Ariane est calculé à la
/// requête, R903), pas plus qu'un titre récrit à l'identique avec un déplacement ; un dossier
/// renommé non plus (les dossiers ne sont pas cherchés).
#[tokio::test]
async fn a_move_without_a_new_title_marks_nothing() {
    let f = fresh().await;
    let moved = new_page(&f, "Déplacée").await;
    let parent = new_page(&f, "Parente").await;
    let folder = f
        .store
        .create_node(NewNode {
            kind: NodeKind::Folder,
            ..page(&f.space, None, None, "Dossier")
        })
        .await
        .unwrap()
        .id;
    clear_outbox(&f.db).await;
    f.store
        .move_node(moved, Some(parent), None, None)
        .await
        .unwrap();
    let place = Place {
        parent_id: None,
        after: None,
        from: None,
    };
    f.store
        .update_node(moved, Some("Déplacée"), Some(place), None, None)
        .await
        .unwrap();
    f.store
        .rename_node(folder, "Dossier renommé", None)
        .await
        .unwrap();
    assert!(outbox(&f.db).await.is_empty());
    // Témoin : le même appel avec un titre neuf marque la page.
    f.store
        .update_node(moved, Some("Renommée"), Some(place), None, None)
        .await
        .unwrap();
    assert_eq!(outbox(&f.db).await, [waiting(moved, "")]);
}

/// Une page supprimée pour de bon emporte ses lignes des trois tables (et celles de son
/// sous-arbre), par la cascade ; ses lignes de bloc quittent l'outbox, et une ligne de page entière
/// y est posée pour elle et pour chaque sous-page, dans la transaction de la suppression, pour que
/// leurs entrées quittent l'index plein texte. Les liens d'une autre page vers elle restent.
#[tokio::test]
async fn the_index_leaves_with_its_page() {
    let f = fresh().await;
    let by = account(&f.store, "olga").await.id;
    let doomed = new_page(&f, "Supprimée").await;
    let child = f
        .store
        .create_node(page(&f.space, Some(doomed), None, "Sous-page"))
        .await
        .unwrap()
        .id;
    let other = new_page(&f, "Autre").await;
    index(
        &f.store,
        doomed,
        1,
        &[linking("d", "vers l'autre", &[other])],
    )
    .await;
    index(&f.store, child, 1, &[row("c", "texte")]).await;
    index(&f.store, other, 1, &[linking("o", "vers elle", &[doomed])]).await;
    clear_outbox(&f.db).await;
    // Des lignes de bloc en attente pour les deux pages qui partent, une pour l'autre.
    index(&f.store, doomed, 2, &[linking("d", "changé", &[other])]).await;
    index(&f.store, child, 2, &[row("c", "changé")]).await;
    index(&f.store, other, 2, &[linking("o", "changé", &[doomed])]).await;

    f.store.trash_node(doomed, by).await.unwrap();
    f.store.delete_node(doomed).await.unwrap();
    for node in [doomed, child] {
        for table in ["blocks_index", "page_links", "search_state"] {
            let count = int(
                &f.db,
                &format!("SELECT COUNT(*) FROM {table} WHERE node_id = '{node}'"),
            )
            .await;
            assert_eq!(count, 0, "{table}");
        }
    }
    let mut expected = vec![waiting(doomed, ""), waiting(child, ""), waiting(other, "o")];
    expected.sort();
    assert_eq!(outbox(&f.db).await, expected);
    assert_eq!(
        links(&f.db, other).await,
        [("o".to_string(), doomed.to_string())]
    );
    assert_eq!(state(&f.db, other).await, Some((2, None, 0)));
}

/// Ruling R918 : rien n'écrit un titre, ni ne crée ou ne supprime un nœud, sans son marqueur. Dans
/// le code de tous les crates de l'atelier (lignes de commentaire et modules de tests retirés),
/// `INSERT INTO nodes`, `DELETE FROM nodes` et un `UPDATE nodes SET` qui touche `title` ne
/// paraissent qu'aux places d'une liste fermée, chacune dans une fonction qui appelle son
/// marqueur. Une écriture neuve du titre, d'où qu'elle vienne, est une place de plus : ce test
/// rougit, et la place neuve prend son marqueur avant d'entrer dans la liste.
#[test]
fn only_listed_places_write_a_title_or_a_node() {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }
    /// Le nom de la fonction que déclare la ligne de code `code`, s'il y en a une.
    fn declared(code: &str) -> Option<String> {
        let at = code
            .find("fn ")
            .filter(|&at| at == 0 || code[..at].ends_with(' ') || code[..at].ends_with('('))?;
        let name: String = code[at + 3..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        (!name.is_empty()).then_some(name)
    }
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(crates).unwrap() {
        let dir = entry.unwrap().path();
        if dir.join("src").is_dir() {
            walk(&dir.join("src"), &mut sources);
        }
    }
    assert!(sources.len() > 40, "les sources de l'atelier : {sources:?}");
    // `(fichier : fonction : genre)` de chaque place, et le texte de chaque fonction.
    let mut places = Vec::new();
    let mut bodies: BTreeMap<String, String> = BTreeMap::new();
    for path in &sources {
        let relative = path.strip_prefix(crates).unwrap();
        let relative = relative.to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        // Le module de tests d'un fichier, en fin de fichier, n'écrit dans aucune base servie.
        let end = lines
            .windows(2)
            .position(|w| {
                w[0].trim() == "#[cfg(test)]" && w[1].trim_start().starts_with("mod tests")
            })
            .unwrap_or(lines.len());
        let mut function = String::new();
        for line in &lines[..end] {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if let Some(name) = declared(line.trim_start()) {
                function = format!("{relative} : {name}");
            }
            // Une chaîne continuée perd sa barre de fin de ligne ; espaces réduits.
            let body = bodies.entry(function.clone()).or_default();
            body.push(' ');
            body.push_str(&line.trim().trim_end_matches('\\').to_lowercase());
        }
    }
    for (function, body) in &bodies {
        let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
        // Écrits en morceaux, par principe : ce test n'est pas lu, mais ses motifs resteraient
        // ceux d'une requête.
        for (pattern, kind) in [
            (["into ", "nodes"].concat(), "création"),
            (["delete from ", "nodes"].concat(), "suppression"),
            (["update ", "nodes"].concat(), "titre"),
        ] {
            for (at, _) in flat.match_indices(&pattern) {
                let rest = &flat[at + pattern.len()..];
                if rest.chars().next().is_some_and(is_ident) {
                    continue;
                }
                // Un `UPDATE` compte s'il touche `title`, dans sa chaîne SQL (jusqu'au guillemet
                // qui la ferme).
                let sql = &rest[..rest.find('"').unwrap_or(rest.len())];
                if kind == "titre" && !sql.contains("title") {
                    continue;
                }
                places.push(format!("{function} : {kind}"));
            }
        }
    }
    places.sort();
    assert_eq!(
        places,
        [
            "mementum-store/src/duplicate.rs : duplicate_pages : création",
            "mementum-store/src/nodes.rs : create_node : création",
            "mementum-store/src/nodes.rs : delete_node : suppression",
            "mementum-store/src/nodes.rs : update_node : titre",
        ]
    );
    for (function, marker) in [
        (
            "mementum-store/src/duplicate.rs : duplicate_pages",
            "mark_born(",
        ),
        ("mementum-store/src/nodes.rs : create_node", "mark_born("),
        ("mementum-store/src/nodes.rs : delete_node", "mark_deleted("),
        ("mementum-store/src/nodes.rs : update_node", "mark_renamed("),
    ] {
        assert!(
            bodies[function].contains(marker),
            "{function} sans {marker}"
        );
    }
}

/// L'indexation n'avance ni la révision ni la version d'un espace, et n'annonce rien sur
/// `Store::changes` (règles de `rev` et de `lsn`). Témoin : un renommage qui suit est annoncé.
#[tokio::test]
async fn indexing_bumps_neither_rev_nor_lsn_and_announces_nothing() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    let (rev, lsn) = (
        f.store.space_rev(f.space.id).await.unwrap(),
        f.store.space_lsn(f.space.id).await.unwrap(),
    );
    let mut changes = f.store.changes();
    index(&f.store, node, 1, &[linking("a", "texte", &[node])]).await;
    f.store.index_failed(node, 2).await.unwrap();
    index(&f.store, node, 3, &[row("a", "texte")]).await;
    assert_eq!(f.store.outbox_everything(None, 10).await.unwrap(), None);
    let FeedStart::Begun(feed) = f.store.begin_feed(10).await.unwrap() else {
        panic!("une intention");
    };
    f.store.finish_feed(&feed.token).await.unwrap();
    f.store.outbox_everything(None, 10).await.unwrap();
    assert!(matches!(
        f.store.begin_feed(10).await.unwrap(),
        FeedStart::Begun(_)
    ));
    f.store.undo_feed().await.unwrap();
    assert_eq!(changes.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(f.store.space_rev(f.space.id).await.unwrap(), rev);
    assert_eq!(f.store.space_lsn(f.space.id).await.unwrap(), lsn);

    f.store.rename_node(node, "Renommée", None).await.unwrap();
    assert_eq!(changes.try_recv(), Ok(f.space.id));
}

/// Les lectures de l'index ne prennent aucun tour d'écriture : elles rendent pendant qu'un tour
/// est tenu, le rattrapage du démarrage compris ; et un échec déjà noté au même `seq` ne
/// redemande pas de tour.
#[tokio::test]
async fn stale_pages_takes_no_write_turn() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    index(&f.store, node, 1, &[linking("a", "texte", &[node])]).await;
    f.store.index_failed(node, 2).await.unwrap();
    clear_outbox(&f.db).await;
    let held = f.store.hold_writes().await;
    let store = &f.store;
    let reads = async {
        store.stale_pages(None, 10).await.unwrap();
        store.never_indexed(10).await.unwrap();
        store.behind_in_space(f.space.id, 10).await.unwrap();
        store.feed_tokens().await.unwrap();
        store.intent_lines().await.unwrap();
        store.indexed_blocks(node).await.unwrap();
        store.indexed_block(node, "a").await.unwrap();
        store
            .backlink_sources(node, f.space.id, None, 10)
            .await
            .unwrap();
        store
            .backlink_excerpts(node, f.space.id, &[node], 10)
            .await
            .unwrap();
        // Rien à passer à l'index plein texte, rien à défaire : aucun tour non plus.
        assert!(matches!(
            store.begin_feed(10).await.unwrap(),
            FeedStart::Nothing
        ));
        store.undo_feed().await.unwrap();
        store.index_failed(node, 2).await.unwrap();
    };
    tokio::time::timeout(WAIT, reads)
        .await
        .expect("une lecture a attendu un tour d'écriture");
    assert_eq!(f.store.queued_writes(), 0);
    drop(held);
}

/// Le rattrapage du démarrage : une page dont le dernier `seq` gardé (journal ou instantané)
/// dépasse ce que l'index reflète, même écrite par un autre `Store` (une commande, un test), qui
/// n'annonce rien à celui-ci ; une page sans ligne (aucune ne l'est depuis la migration `0005`,
/// R964 : le filet reste, pour une ligne écrite à la main). Pas un dossier,
/// pas une page à jour, pas une page dont la lecture a échoué à son dernier `seq`. Par lots,
/// dans l'ordre des identifiants, au plus `limit` pages lues par appel.
#[tokio::test]
async fn stale_pages_names_a_page_written_by_another_store() {
    let f = fresh().await;
    let written = f
        .store
        .create_node(NewNode {
            doc: Some(b"etat".to_vec()),
            ..page(&f.space, None, None, "Écrite ailleurs")
        })
        .await
        .unwrap()
        .id;
    let fresh_page = new_page(&f, "À jour").await;
    let failed = new_page(&f, "Illisible").await;
    let old = new_page(&f, "D'avant le lot").await;
    f.store
        .create_node(NewNode {
            kind: NodeKind::Folder,
            ..page(&f.space, None, None, "Dossier")
        })
        .await
        .unwrap();
    for node in [written, fresh_page, old] {
        index(&f.store, node, 0, &[row("a", "texte")]).await;
    }
    f.store
        .append_updates(failed, &[b"un", b"deux"], None)
        .await
        .unwrap();
    f.store.index_failed(failed, 2).await.unwrap();
    exec_sql(
        &f.db,
        &format!("DELETE FROM search_state WHERE node_id = '{old}'"),
    )
    .await;
    assert_eq!(f.store.stale_pages(None, 10).await.unwrap().pages, [old]);

    let other = Store::open(&f.db).await.unwrap();
    other
        .append_updates(written, &[b"ailleurs"], None)
        .await
        .unwrap();
    other.close().await;
    let all = f.store.stale_pages(None, 10).await.unwrap();
    assert_eq!(all.pages, [written, old]);
    assert_eq!(all.next, None);

    // Par lots : chaque appel lit au plus `limit` pages, et dit où reprendre.
    let mut seen = Vec::new();
    let mut after = None;
    let mut calls = 0;
    loop {
        let batch = f.store.stale_pages(after, 2).await.unwrap();
        calls += 1;
        seen.extend(batch.pages);
        match batch.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(seen, [written, old]);
    assert_eq!(calls, 3, "quatre pages, deux par lot, puis la fin");
}

/// Une page dont la lecture échoue quitte l'index : ses lignes de `blocks_index` et de
/// `page_links` sont retirées, ses lignes de bloc quittent l'outbox, sa ligne de page entière y est
/// posée, et elle est notée à ce `seq` : ni jamais indexée, ni à rattraper, tant que son `seq` ne
/// change pas (R962). Indexée de nouveau, l'échec est oublié.
#[tokio::test]
async fn a_failed_read_leaves_the_index_until_its_seq_changes() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    f.store
        .append_updates(node, &[b"un", b"deux"], None)
        .await
        .unwrap();
    index(&f.store, node, 1, &[linking("a", "texte", &[node])]).await;
    clear_outbox(&f.db).await;
    index(&f.store, node, 2, &[linking("a", "changé", &[node])]).await;

    f.store.index_failed(node, 2).await.unwrap();
    assert!(indexed(&f.db, node).await.is_empty());
    assert!(links(&f.db, node).await.is_empty());
    assert_eq!(outbox(&f.db).await, [waiting(node, "")]);
    assert_eq!(state(&f.db, node).await, Some((2, Some(2), 0)));
    assert!(
        f.store
            .stale_pages(None, 10)
            .await
            .unwrap()
            .pages
            .is_empty()
    );
    assert!(f.store.never_indexed(10).await.unwrap().is_empty());

    f.store
        .append_updates(node, &[b"trois"], None)
        .await
        .unwrap();
    assert_eq!(f.store.stale_pages(None, 10).await.unwrap().pages, [node]);
    index(&f.store, node, 3, &[row("a", "relu")]).await;
    assert_eq!(state(&f.db, node).await, Some((3, None, 0)));
}

/// Ce que l'index a de retard dans un espace : une page jamais indexée (à `-1` : créée, dupliquée,
/// ou d'une instance d'avant ce lot, que la migration note ainsi, R964), une page qui a une ligne
/// d'outbox (un titre changé, un bloc pas encore validé). Pas une page à jour, pas un dossier, pas
/// une page d'un autre espace. Dans l'ordre des identifiants, au plus `limit`.
#[tokio::test]
async fn behind_in_space_names_unindexed_and_pending_pages_of_the_space() {
    let f = fresh().await;
    let born = new_page(&f, "Neuve").await;
    let current = new_page(&f, "À jour").await;
    let renamed = new_page(&f, "Renommée").await;
    let old = new_page(&f, "D'avant le lot").await;
    f.store
        .create_node(NewNode {
            kind: NodeKind::Folder,
            ..page(&f.space, None, None, "Dossier")
        })
        .await
        .unwrap();
    let elsewhere = f.store.create_space("Ailleurs", None).await.unwrap();
    let far = f
        .store
        .create_node(page(&elsewhere, None, None, "Loin"))
        .await
        .unwrap()
        .id;
    for node in [current, renamed, old] {
        index(&f.store, node, 0, &[row("a", "texte")]).await;
    }
    clear_outbox(&f.db).await;
    // Ce que la migration `0005` pose pour une page d'avant ce lot.
    exec_sql(
        &f.db,
        &format!("UPDATE search_state SET indexed_seq = -1 WHERE node_id = '{old}'"),
    )
    .await;
    f.store
        .rename_node(renamed, "Renommée encore", None)
        .await
        .unwrap();
    assert_eq!(
        f.store.behind_in_space(f.space.id, 10).await.unwrap(),
        [born, renamed, old]
    );
    assert_eq!(
        f.store.behind_in_space(f.space.id, 1).await.unwrap(),
        [born]
    );
    assert_eq!(
        f.store.behind_in_space(elsewhere.id, 10).await.unwrap(),
        [far]
    );
}

/// Ce que `blocks_index` garde se relit, pour nourrir l'index plein texte : tous les blocs d'une
/// page, dans l'ordre de leur clé, ou un seul.
#[tokio::test]
async fn indexed_blocks_read_back() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    index(&f.store, node, 1, &[row("b", "deux"), row("a", "un")]).await;
    let all = f.store.indexed_blocks(node).await.unwrap();
    let expected = |id: &str, text: &str| IndexRow {
        block_id: id.to_string(),
        kind: "paragraph".to_string(),
        text: text.to_string(),
    };
    assert_eq!(all, [expected("a", "un"), expected("b", "deux")]);
    assert_eq!(
        f.store.indexed_block(node, "b").await.unwrap(),
        Some(expected("b", "deux"))
    );
    assert_eq!(f.store.indexed_block(node, "z").await.unwrap(), None);
}

/// Ruling R977 : la version de la projection (`PROJECTION_VERSION` de `mementum-search`, R960)
/// est gardée dans la base, à côté des jetons : l'indexeur reprojette tout quand elle diffère de
/// la sienne. `None` tant qu'aucune n'est écrite (une instance neuve, ou d'avant ce lot). L'écrire
/// ne touche pas aux jetons, n'avance ni `rev` ni `lsn`, et ne prend pas de tour si elle est déjà
/// celle-là ; une valeur illisible est une erreur, jamais une version prise pour une autre.
#[tokio::test]
async fn the_projection_version_is_kept_in_the_base() {
    let f = fresh().await;
    assert_eq!(f.store.projection_version().await.unwrap(), None);
    let mut changes = f.store.changes();
    f.store.set_projection_version(1).await.unwrap();
    assert_eq!(f.store.projection_version().await.unwrap(), Some(1));
    f.store.set_projection_version(u32::MAX).await.unwrap();
    assert_eq!(f.store.projection_version().await.unwrap(), Some(u32::MAX));
    assert_eq!(changes.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(f.store.feed_tokens().await.unwrap(), FeedTokens::default());

    let held = f.store.hold_writes().await;
    tokio::time::timeout(WAIT, f.store.set_projection_version(u32::MAX))
        .await
        .expect("la même version a attendu un tour d'écriture")
        .unwrap();
    drop(held);

    exec_sql(
        &f.db,
        "UPDATE instance_settings SET value = 'deux' WHERE key = 'search.projection'",
    )
    .await;
    assert!(f.store.projection_version().await.is_err());
}

/// Tout reconstruire passe par tranches de pages, chacune sa transaction dans la voie de l'index :
/// une écriture d'une autre voie passe entre deux tranches (ordre lu dans la base). Chaque page,
/// et elles seules (pas un dossier), reçoit sa ligne de page entière ; l'appelant reprend à
/// l'identifiant rendu, jusqu'à `None`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn outbox_everything_goes_by_slices() {
    let f = fresh().await;
    let mut pages = Vec::new();
    for i in 0..6 {
        pages.push(new_page(&f, &format!("Page {i}")).await);
    }
    f.store
        .create_node(NewNode {
            kind: NodeKind::Folder,
            ..page(&f.space, None, None, "Dossier")
        })
        .await
        .unwrap();
    clear_outbox(&f.db).await;
    exec_sql(
        &f.db,
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER w_out AFTER INSERT ON search_outbox \
         BEGIN INSERT INTO witness (what) VALUES ('tranche'); END; \
         CREATE TRIGGER w_space AFTER INSERT ON spaces \
         BEGIN INSERT INTO witness (what) VALUES (NEW.name); END;",
    )
    .await;
    let held = f.store.hold_writes().await;
    let rebuild = tokio::spawn({
        let store = f.store.clone();
        async move {
            let (mut after, mut slices) = (None, 0);
            loop {
                slices += 1;
                match store.outbox_everything(after, 2).await.unwrap() {
                    Some(next) => after = Some(next),
                    None => return slices,
                }
            }
        }
    });
    queued(&f.store, 1).await;
    let other = tokio::spawn({
        let store = f.store.clone();
        async move { store.create_space("Autre", None).await }
    });
    queued(&f.store, 2).await;
    drop(held);
    assert_eq!(
        rebuild.await.unwrap(),
        4,
        "trois tranches pleines, puis la fin"
    );
    other.await.unwrap().unwrap();
    let order = witness(&f.db).await;
    assert_eq!(order[..3], ["tranche", "tranche", "Autre"], "{order:?}");
    assert_eq!(order.len(), 7, "{order:?}");
    let all: Vec<_> = pages.iter().map(|p| waiting(*p, "")).collect();
    assert_eq!(outbox(&f.db).await, all);
}

/// Ruling R980 (Task S4b) : tout reprojeter passe par tranches de pages. Chaque page, et elles
/// seules (pas un dossier), redevient jamais indexée, un échec de lecture oublié, et reçoit sa
/// ligne d'outbox de page entière ; ses lignes d'index restent (la page reste trouvable sous
/// l'ancienne forme), et rien n'est annoncé. L'appelant reprend à l'identifiant rendu, jusqu'à
/// `None`.
#[tokio::test]
async fn reprojecting_everything_goes_by_slices_and_marks_every_page_never_indexed() {
    let f = fresh().await;
    let mut pages = Vec::new();
    for i in 0..5 {
        pages.push(new_page(&f, &format!("Page {i}")).await);
    }
    let folder = f
        .store
        .create_node(NewNode {
            kind: NodeKind::Folder,
            ..page(&f.space, None, None, "Dossier")
        })
        .await
        .unwrap()
        .id;
    for node in &pages[..4] {
        assert_eq!(index(&f.store, *node, 3, &[row("b", "texte")]).await, 1);
    }
    f.store.index_failed(pages[4], 2).await.unwrap();
    clear_outbox(&f.db).await;
    assert_eq!(f.store.never_indexed(10).await.unwrap(), Vec::<Uuid>::new());
    let mut changes = f.store.changes();

    let (mut after, mut slices) = (None::<Uuid>, 0);
    loop {
        slices += 1;
        match f.store.reproject_everything(after, 2).await.unwrap() {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(slices, 3, "deux tranches pleines, puis la fin");
    let mut sorted = pages.clone();
    sorted.sort();
    assert_eq!(f.store.never_indexed(10).await.unwrap(), sorted);
    for node in &pages {
        assert_eq!(state(&f.db, *node).await, Some((-1, None, 0)));
    }
    assert_eq!(state(&f.db, folder).await, None);
    let all: Vec<_> = sorted.iter().map(|p| waiting(*p, "")).collect();
    assert_eq!(outbox(&f.db).await, all);
    assert_eq!(indexed(&f.db, pages[0]).await.len(), 1);
    assert_eq!(changes.try_recv(), Err(TryRecvError::Empty));
}

/// Ruling R909, R939 : la clôture retire les lignes de son intention, sauf celle qui a bougé
/// depuis (un bloc changé de nouveau : il a perdu son jeton), qui reste pour la suivante. Le
/// jeton devient celui de la dernière validation close.
#[tokio::test]
async fn finish_feed_keeps_a_line_changed_meanwhile() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    clear_outbox(&f.db).await;
    index(&f.store, node, 1, &[row("x", "un"), row("y", "deux")]).await;
    let FeedStart::Begun(feed) = f.store.begin_feed(10).await.unwrap() else {
        panic!("une intention");
    };
    let line = |block: &str| FeedLine {
        node_id: node,
        block_id: Some(block.to_string()),
    };
    assert_eq!(feed.lines, [line("x"), line("y")]);
    assert_eq!(
        f.store.feed_tokens().await.unwrap(),
        FeedTokens {
            committed: None,
            intent: Some(feed.token.clone())
        }
    );
    assert_eq!(f.store.intent_lines().await.unwrap(), Some(feed.clone()));

    index(
        &f.store,
        node,
        2,
        &[row("x", "un, changé"), row("y", "deux")],
    )
    .await;
    f.store.finish_feed(&feed.token).await.unwrap();
    assert_eq!(outbox(&f.db).await, [waiting(node, "x")]);
    assert_eq!(
        f.store.feed_tokens().await.unwrap(),
        FeedTokens {
            committed: Some(feed.token.clone()),
            intent: None
        }
    );
    assert_eq!(f.store.intent_lines().await.unwrap(), None);
    // Un jeton qui n'est pas celui de l'intention posée ne clôt rien.
    assert!(matches!(
        f.store.finish_feed(&feed.token).await,
        Err(StoreError::Stale)
    ));
    assert_eq!(outbox(&f.db).await, [waiting(node, "x")]);
}

/// Ruling R939 (relecture de S1, I3 a) : une suppression pendant une intention posée retire le
/// jeton de la ligne de page entière qu'elle prend : la clôture la laisse, sans jeton, pour que les
/// entrées de la page quittent l'index plein texte à l'intention suivante. Les lignes de bloc de la
/// page, prises elles aussi, partent avec la suppression.
#[tokio::test]
async fn a_deletion_during_an_intent_keeps_its_line_past_the_closing() {
    let f = fresh().await;
    let by = account(&f.store, "olga").await.id;
    let node = new_page(&f, "Supprimée").await;
    index(&f.store, node, 1, &[row("x", "un")]).await;
    let FeedStart::Begun(feed) = f.store.begin_feed(10).await.unwrap() else {
        panic!("une intention");
    };
    assert_eq!(feed.lines.len(), 2, "la page entière et son bloc");
    f.store.trash_node(node, by).await.unwrap();
    f.store.delete_node(node).await.unwrap();
    f.store.finish_feed(&feed.token).await.unwrap();
    assert_eq!(outbox(&f.db).await, [waiting(node, "")]);
}

/// Ruling R939 (relecture de S1, I3 a) : une reconstruction pendant une intention posée retire le
/// jeton de chaque ligne de page entière qu'elle récrit : la clôture les laisse, sans jeton.
#[tokio::test]
async fn a_rebuild_during_an_intent_keeps_its_lines_past_the_closing() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    let FeedStart::Begun(feed) = f.store.begin_feed(10).await.unwrap() else {
        panic!("une intention");
    };
    assert_eq!(feed.lines.len(), 1);
    assert_eq!(f.store.outbox_everything(None, 10).await.unwrap(), None);
    f.store.finish_feed(&feed.token).await.unwrap();
    assert_eq!(outbox(&f.db).await, [waiting(node, "")]);
}

/// Défaire une intention rend ses lignes, sans jeton, et retire l'intention ; le jeton de la
/// dernière validation close ne bouge pas. Une intention neuve reprend les mêmes lignes.
#[tokio::test]
async fn undo_feed_gives_the_lines_back() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    let FeedStart::Begun(first) = f.store.begin_feed(10).await.unwrap() else {
        panic!("une intention");
    };
    f.store.finish_feed(&first.token).await.unwrap();
    index(&f.store, node, 1, &[row("x", "un")]).await;
    f.store.rename_node(node, "Renommée", None).await.unwrap();
    let FeedStart::Begun(feed) = f.store.begin_feed(10).await.unwrap() else {
        panic!("une intention");
    };
    let page_line = FeedLine {
        node_id: node,
        block_id: None,
    };
    let block_line = FeedLine {
        node_id: node,
        block_id: Some("x".to_string()),
    };
    // La ligne de page entière d'abord.
    assert_eq!(feed.lines, [page_line.clone(), block_line.clone()]);
    f.store.undo_feed().await.unwrap();
    assert_eq!(outbox(&f.db).await, [waiting(node, ""), waiting(node, "x")]);
    assert_eq!(
        f.store.feed_tokens().await.unwrap(),
        FeedTokens {
            committed: Some(first.token.clone()),
            intent: None
        }
    );
    let FeedStart::Begun(again) = f.store.begin_feed(10).await.unwrap() else {
        panic!("une intention");
    };
    assert_ne!(again.token, feed.token);
    assert_eq!(again.lines, [page_line, block_line]);
    // Une intention prend au plus `limit` lignes.
    f.store.undo_feed().await.unwrap();
    let FeedStart::Begun(one) = f.store.begin_feed(1).await.unwrap() else {
        panic!("une intention");
    };
    assert_eq!(one.lines.len(), 1);
}

/// Ruling R909 : tant qu'une intention est posée, pas encore close ni défaite, aucune autre ne
/// commence ; close ou défaite, la suivante le peut.
#[tokio::test]
async fn a_second_feed_cannot_begin_before_the_first_is_finished_or_undone() {
    let f = fresh().await;
    let node = new_page(&f, "Page").await;
    let FeedStart::Begun(first) = f.store.begin_feed(10).await.unwrap() else {
        panic!("une intention");
    };
    f.store.rename_node(node, "Renommée", None).await.unwrap();
    match f.store.begin_feed(10).await.unwrap() {
        FeedStart::Pending(token) => assert_eq!(token, first.token),
        other => panic!("{other:?}"),
    }
    f.store.undo_feed().await.unwrap();
    let FeedStart::Begun(second) = f.store.begin_feed(10).await.unwrap() else {
        panic!("une intention");
    };
    assert!(matches!(
        f.store.begin_feed(10).await.unwrap(),
        FeedStart::Pending(_)
    ));
    f.store.finish_feed(&second.token).await.unwrap();
    assert!(matches!(
        f.store.begin_feed(10).await.unwrap(),
        FeedStart::Nothing
    ));
}

/// Les sources d'un rétrolien : distinctes (une page qui mène trois fois à la cible compte une
/// fois, et la borne compte des pages, pas des liens), de l'espace de la cible, hors la cible
/// elle-même, dans l'ordre des identifiants, reprises après `after`.
#[tokio::test]
async fn backlink_sources_are_distinct_of_the_space_and_ordered() {
    let f = fresh().await;
    let many = new_page(&f, "Trois liens").await;
    let one = new_page(&f, "Un lien").await;
    let target = new_page(&f, "Cible").await;
    let unrelated = new_page(&f, "Vers ailleurs").await;
    let elsewhere = f.store.create_space("Ailleurs", None).await.unwrap();
    let far = f
        .store
        .create_node(page(&elsewhere, None, None, "Loin"))
        .await
        .unwrap()
        .id;
    let rows = [
        linking("a", "un", &[target]),
        linking("b", "deux", &[target]),
        linking("c", "trois", &[target, one]),
    ];
    index(&f.store, many, 1, &rows).await;
    index(&f.store, one, 1, &[linking("a", "un", &[target])]).await;
    index(&f.store, target, 1, &[linking("a", "soi", &[target])]).await;
    index(&f.store, unrelated, 1, &[linking("a", "x", &[many])]).await;
    index(&f.store, far, 1, &[linking("a", "loin", &[target])]).await;
    assert!(many < one);

    let sources = |after, limit| f.store.backlink_sources(target, f.space.id, after, limit);
    assert_eq!(sources(None, 10).await.unwrap(), [many, one]);
    assert_eq!(sources(None, 2).await.unwrap(), [many, one]);
    assert_eq!(sources(None, 1).await.unwrap(), [many]);
    assert_eq!(sources(Some(many), 10).await.unwrap(), [one]);
    assert!(sources(Some(one), 10).await.unwrap().is_empty());
    assert_eq!(
        f.store
            .backlink_sources(target, elsewhere.id, None, 10)
            .await
            .unwrap(),
        [far]
    );
}

/// L'extrait d'un rétrolien est coupé par la base, entre deux caractères : un bloc de 64 Kio
/// rend la longueur demandée, pas son texte entier. Pour chaque source, le premier bloc qui mène à
/// la cible, dans l'ordre des clés (R963) ; une source qui n'y mène pas n'est pas rendue.
#[tokio::test]
async fn backlink_excerpts_are_cut_by_the_database() {
    let f = fresh().await;
    let target = new_page(&f, "Cible").await;
    let long = new_page(&f, "Longue").await;
    let short = new_page(&f, "Courte").await;
    let none = new_page(&f, "Sans lien").await;
    let text = "é".repeat(32 * 1024);
    assert_eq!(text.len(), 64 * 1024);
    let rows = [
        linking("a-long", &text, &[target]),
        linking("z-court", "court", &[target]),
    ];
    index(&f.store, long, 1, &rows).await;
    index(&f.store, short, 1, &[linking("s", "salut", &[target])]).await;
    index(&f.store, none, 1, &[row("n", "rien")]).await;

    let found = f
        .store
        .backlink_excerpts(target, f.space.id, &[long, short, none], 240)
        .await
        .unwrap();
    assert_eq!(found.len(), 2);
    let by_page: BTreeMap<Uuid, (String, String)> = found
        .into_iter()
        .map(|b| (b.node_id, (b.block_id, b.excerpt)))
        .collect();
    assert_eq!(by_page[&long], ("a-long".to_string(), "é".repeat(240)));
    assert_eq!(by_page[&short], ("s".to_string(), "salut".to_string()));
}

/// `n` pages neuves de `space`, pas encore indexées.
async fn new_pages(f: &Fixture, space: &mementum_store::Space, n: usize) -> Vec<Uuid> {
    let mut pages = Vec::with_capacity(n);
    for i in 0..n {
        let node = page(space, None, None, &format!("Source {i}"));
        pages.push(f.store.create_node(node).await.unwrap().id);
    }
    pages
}

/// Chacune de ces pages mène à `target` par un bloc.
async fn link_to(f: &Fixture, sources: &[Uuid], target: Uuid) {
    for &source in sources {
        index(&f.store, source, 1, &[linking("a", "vers la cible", &[target])]).await;
    }
}

/// Task S6 (relecture de S1, m3) : des liens posés en masse depuis un autre espace vers l'UUID
/// d'une page ne sont **même pas lus** quand on lit ses rétroliens, ni par les sources ni par les
/// extraits. Leur coût se compte en pas de la machine virtuelle de SQLite
/// (`Store::backlink_read_steps`) : il ne bouge pas quand une page d'un autre espace qui mène à la
/// cible devient cent, aux identifiants avant et après la seule source. Témoin : cinquante pages
/// du même espace le font
/// grandir, et elles sont rendues. Avec la requête de S1 (tous les liens vers la cible lus par
/// l'index, l'espace jugé à la jointure sur `nodes`), les liens d'ailleurs étaient lus, puis
/// écartés.
#[tokio::test]
async fn sources_of_another_space_are_not_even_read() {
    let f = fresh().await;
    let target = new_page(&f, "Cible").await;
    let elsewhere = f.store.create_space("Ailleurs", None).await.unwrap();
    // Des identifiants avant celui de la source, et d'autres après.
    let before = new_pages(&f, &elsewhere, 50).await;
    let seen = new_page(&f, "Vue").await;
    let after = new_pages(&f, &elsewhere, 50).await;
    assert!(before.iter().all(|id| *id < seen) && after.iter().all(|id| *id > seen));
    // Un lien d'ailleurs dès le départ : la dernière recherche dans l'index s'arrête alors sur une
    // entrée hors de la plage, et non au bout de l'index, ce qui coûte un pas de plus, une fois,
    // quel que soit le nombre de liens d'ailleurs (vu en écrivant ce test).
    link_to(&f, &[seen, after[0]], target).await;
    let read = async || {
        f.store
            .backlink_read_steps(target, f.space.id, 500, 240)
            .await
            .unwrap()
    };
    let (sources, found, alone) = read().await;
    assert_eq!(sources, [seen]);
    assert_eq!(found.len(), 1);

    link_to(&f, &before, target).await;
    link_to(&f, &after[1..], target).await;
    let (sources, found, crowded) = read().await;
    assert_eq!(sources, [seen]);
    assert_eq!(found.len(), 1);
    assert_eq!(crowded, alone, "des liens d'un autre espace ont été lus");

    // Témoin : le compte voit ce qui est lu.
    let nearby = new_pages(&f, &f.space, 50).await;
    link_to(&f, &nearby, target).await;
    let (sources, found, here) = read().await;
    assert_eq!(sources.len(), 51);
    assert_eq!(found.len(), 51);
    assert!(here.sources > alone.sources, "{here:?} / {alone:?}");
    assert!(here.excerpts > alone.excerpts, "{here:?} / {alone:?}");
}

/// Task S6 : une page qui mène trois cents fois à la cible (un lien par bloc) coûte aux deux
/// lectures d'un rétrolien ce que coûte une page qui y mène une fois : les sources se lisent une
/// par page (on saute de l'une à la suivante dans l'index), l'extrait par le premier bloc de chaque
/// page, sans lire les autres. Sans cela, dix mille liens par page (la borne de la projection)
/// faisaient lire à chaque appel dix mille entrées par source.
#[tokio::test]
async fn a_source_linking_many_times_is_read_once() {
    let f = fresh().await;
    let target = new_page(&f, "Cible").await;
    let source = new_page(&f, "Source").await;
    let first = linking("a", "le premier", &[target]);
    index(&f.store, source, 1, std::slice::from_ref(&first)).await;
    let read = async || {
        f.store
            .backlink_read_steps(target, f.space.id, 500, 240)
            .await
            .unwrap()
    };
    let (sources, found, once) = read().await;
    assert_eq!(sources, [source]);
    assert_eq!(found[0].block_id, "a");

    let mut rows = vec![first];
    rows.extend((0..300).map(|i| linking(&format!("b{i:03}"), "encore", &[target])));
    index(&f.store, source, 2, &rows).await;
    let (sources, found, many) = read().await;
    assert_eq!(sources, [source]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].block_id, "a");
    assert_eq!(many, once, "les liens d'une même source ont été lus un à un");
}
