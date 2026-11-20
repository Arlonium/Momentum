//! L'indexeur (lot 4, Task S4b) : une page écrite a sa projection à jour dans `blocks_index`,
//! `page_links` et `search_state`, sans que la frappe le paie et sans passer par un acteur.
//!
//! Aucun test n'attend une durée : `IndexerHandle::settled` rend quand l'indexeur n'a plus rien à
//! projeter ; les tests qui disent « deux secondes plus tard » arrêtent l'horloge de tokio
//! ([`StoppedClock`]) et bornent leurs attentes en temps réel ([`until`]).

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::clock::StoppedClock;
use common::sync::types;
use common::{Account, Logs, TestApp, exec_sql, query_i64, sync_url, synced_as};
use mementum_crdt::PageDoc;
use mementum_md::parse;
use mementum_server::SignupMode;
use mementum_store::{NewNode, NodeKind, Store, VersionReason};
use serde_json::json;
use uuid::Uuid;

/// Délai maximal, en temps réel, d'une attente de condition.
const REAL_WAIT: Duration = Duration::from_secs(20);

/// Le calme attendu avant d'indexer, dans les tests qui laissent tourner l'horloge.
const QUICK: Duration = Duration::from_millis(20);

/// Une application dont l'indexeur n'attend que [`QUICK`] de calme.
async fn quick_app() -> TestApp {
    TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.index_after = QUICK;
    })
    .await
}

/// Une application aux délais par défaut (2 s de calme, 15 s au plus) : pour l'horloge arrêtée.
async fn default_app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

/// Attend `ready` en temps réel ([`REAL_WAIT`] au plus), en cédant la main : sous une horloge
/// arrêtée, aucun délai de tokio ne finit. La borne peut faire échouer le test à tort sur un
/// poste très chargé, jamais le faire passer.
async fn until(what: &str, mut ready: impl AsyncFnMut() -> bool) {
    let deadline = std::time::Instant::now() + REAL_WAIT;
    while !ready().await {
        assert!(std::time::Instant::now() < deadline, "jamais vu : {what}");
        tokio::task::yield_now().await;
    }
}

/// Une page neuve du compte, à ce titre.
async fn new_page(by: &Account<'_>, title: &str) -> Uuid {
    by.create(json!({ "title": title }))
        .await
        .id
        .parse()
        .unwrap()
}

/// Récrit la page par `PUT /mmd`.
async fn write(by: &Account<'_>, page: Uuid, mmd: &str) {
    by.put_mmd(&page.to_string(), mmd.to_string())
        .await
        .assert_status(StatusCode::NO_CONTENT);
}

/// Les textes que `blocks_index` garde pour la page, rangés.
async fn texts(store: &Store, page: Uuid) -> Vec<String> {
    let mut texts: Vec<String> = store
        .indexed_blocks(page)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.text)
        .collect();
    texts.sort();
    texts
}

/// `(indexed_seq, failed_seq)` de la page ; `None` sans ligne.
async fn state(t: &TestApp, page: Uuid) -> Option<(i64, Option<i64>)> {
    let found = format!("SELECT COUNT(*) FROM search_state WHERE node_id = '{page}'");
    if query_i64(&t.db_path(), &found).await == 0 {
        return None;
    }
    let indexed = format!("SELECT indexed_seq FROM search_state WHERE node_id = '{page}'");
    let failed =
        format!("SELECT COALESCE(failed_seq, -9) FROM search_state WHERE node_id = '{page}'");
    let failed = query_i64(&t.db_path(), &failed).await;
    Some((
        query_i64(&t.db_path(), &indexed).await,
        (failed != -9).then_some(failed),
    ))
}

/// Le dernier `seq` que la base garde pour la page.
async fn last_seq(t: &TestApp, page: Uuid) -> i64 {
    let sql = format!(
        "SELECT MAX(COALESCE((SELECT MAX(seq) FROM doc_updates WHERE node_id = '{page}'), 0), \
                    COALESCE((SELECT seq FROM doc_snapshots WHERE node_id = '{page}'), 0))"
    );
    query_i64(&t.db_path(), &sql).await
}

/// L'état Yjs d'une page qui porte `mmd`.
fn state_of(mmd: &str) -> Vec<u8> {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(mmd).unwrap()).unwrap();
    page.encode_state()
}

/// Un mot tapé dans une page arrive dans `blocks_index`, la page notée au `seq` lu ; un lien vers
/// une page de l'instance arrive dans `page_links`. Témoin : avant la frappe, la page (un
/// paragraphe vide) n'a aucune ligne.
#[tokio::test]
async fn a_typed_word_reaches_the_index_tables() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let (page, target) = (
        new_page(&olga, "Notes").await,
        new_page(&olga, "Cible").await,
    );
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, Vec::<String>::new());
    assert_eq!(state(&t, page).await, Some((0, None)));

    let addr = t.listen().await;
    let client = synced_as(&olga.browser, &sync_url(addr, page)).await;
    types(&client, "bonjour").await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["bonjour"]);
    assert_eq!(
        state(&t, page).await,
        Some((last_seq(&t, page).await, None))
    );

    write(&olga, page, &format!("Voir [la cible](/p/{target}).\n")).await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["Voir la cible."]);
    let links = format!(
        "SELECT COUNT(*) FROM page_links WHERE node_id = '{page}' AND target_id = '{target}'"
    );
    assert_eq!(query_i64(&t.db_path(), &links).await, 1);
}

/// Review Focus 6 : un texte effacé quitte l'index à la réindexation de sa page. Témoin : il y
/// était.
#[tokio::test]
async fn erased_text_leaves_the_index() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    write(&olga, page, "Le code est 4812.\n\nReste.\n").await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["Le code est 4812.", "Reste."]);

    write(&olga, page, "Reste.\n").await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["Reste."]);
    let kept = "SELECT COUNT(*) FROM blocks_index WHERE text LIKE '%4812%'";
    assert_eq!(query_i64(&t.db_path(), kept).await, 0);
}

/// Une page écrite par `PUT /mmd` est indexée, et une page restaurée à une version aussi : les
/// deux écritures du serveur nomment la page à l'indexeur.
#[tokio::test]
async fn a_page_written_by_put_mmd_or_restored_is_indexed() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    write(&olga, page, "Après\n").await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["Après"]);

    let version = t
        .store
        .put_version(
            page,
            0,
            &state_of("Avant\n"),
            &[olga.id],
            VersionReason::Periodic,
        )
        .await
        .unwrap();
    olga.browser
        .post_empty(&format!("/api/v1/pages/{page}/versions/{version}/restore"))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["Avant"]);
    assert_eq!(
        state(&t, page).await,
        Some((last_seq(&t, page).await, None))
    );
}

/// Ruling R918 : une page créée et une page dupliquée sont indexées par leur marqueur (leur ligne
/// à `-1`), sans qu'aucun acteur soit demandé ni qu'aucune liste de blocs soit lue par lui.
#[tokio::test]
async fn a_created_page_and_a_duplicated_page_are_indexed_without_any_actor() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Modèle").await;
    write(&olga, page, "Texte du modèle\n").await;
    t.indexer.settled().await;

    let created = new_page(&olga, "Neuve").await;
    let copy = olga
        .browser
        .post_empty(&format!("/api/v1/pages/{page}/duplicate"))
        .await;
    copy.assert_status(StatusCode::CREATED);
    let copy: Uuid = copy.json()["id"].as_str().unwrap().parse().unwrap();
    assert_ne!(copy, page);
    t.indexer.settled().await;
    assert_eq!(state(&t, created).await, Some((0, None)));
    assert_eq!(state(&t, copy).await, Some((0, None)));
    assert_eq!(texts(&t.store, copy).await, ["Texte du modèle"]);

    // Une page née par un autre `Store`, que rien n'annonce et qu'aucun acteur ne connaît :
    // seule sa ligne à `-1` la désigne. Les comptes du hub sont relevés avant que l'indexeur
    // relise ses marqueurs (la duplication, elle, lit le modèle par son acteur).
    let other = Store::open(&t.db_path()).await.unwrap();
    let born = other
        .create_node(NewNode {
            space_id: olga.space,
            parent_id: None,
            after: None,
            kind: NodeKind::Page,
            title: "Née ailleurs".to_string(),
            created_by: Some(olga.id),
            grants: Vec::new(),
            doc: Some(state_of("Née sans acteur\n")),
        })
        .await
        .unwrap()
        .id;
    other.close().await;
    let before = (t.hub.calls(), t.hub.block_reads(), t.hub.state_encodings());
    assert_eq!(state(&t, born).await, Some((-1, None)));
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, born).await, ["Née sans acteur"]);
    assert_eq!(
        (t.hub.calls(), t.hub.block_reads(), t.hub.state_encodings()),
        before,
        "l'indexeur ne demande rien à un acteur"
    );
}

/// Ruling R907 : indexer une page écrite ne demande rien à son acteur : ni demande, ni liste de
/// blocs, ni état encodé. Témoin : la page a bien été indexée entre les deux relevés.
#[tokio::test]
async fn the_indexer_never_asks_an_actor() {
    let t = default_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    let addr = t.listen().await;
    let client = synced_as(&olga.browser, &sync_url(addr, page)).await;
    t.indexer.settled().await;
    // L'horloge arrêtée : rien n'est indexé avant que les comptes soient relevés.
    let _clock = StoppedClock::start();
    types(&client, "un").await;
    until("la page écrite n'attend pas son calme", async || {
        t.indexer.waits_for_calm(page)
    })
    .await;
    let before = (t.hub.calls(), t.hub.block_reads(), t.hub.state_encodings());
    tokio::time::advance(Duration::from_millis(2_100)).await;
    until("la page n'est pas indexée", async || {
        texts(&t.store, page).await == ["un"]
    })
    .await;
    assert_eq!(
        (t.hub.calls(), t.hub.block_reads(), t.hub.state_encodings()),
        before
    );
}

/// Décision L4 : la corbeille reste indexée. Une page jetée garde ses lignes, et une page jetée
/// avant sa première indexation est indexée quand même : restaurée, elle se retrouve sans
/// attendre.
#[tokio::test]
async fn a_trashed_page_stays_indexed() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    write(&olga, page, "Gardé\n").await;
    t.indexer.settled().await;
    let trash = |id: Uuid| {
        let uri = format!("/api/v1/pages/{id}/trash");
        async move {
            olga.browser
                .post_empty(&uri)
                .await
                .assert_status(StatusCode::OK);
        }
    };
    trash(page).await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["Gardé"]);

    // Une page écrite par un autre `Store` (rien n'est annoncé), jetée avant d'être indexée.
    let other = Store::open(&t.db_path()).await.unwrap();
    let late = other
        .create_node(NewNode {
            space_id: olga.space,
            parent_id: None,
            after: None,
            kind: NodeKind::Page,
            title: "Jetée".to_string(),
            created_by: Some(olga.id),
            grants: Vec::new(),
            doc: Some(state_of("Jeté tôt\n")),
        })
        .await
        .unwrap()
        .id;
    other.trash_node(late, olga.id).await.unwrap();
    other.close().await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, late).await, ["Jeté tôt"]);
}

/// Ruling R915 : une instance existante est indexée d'elle-même après son démarrage. Une base
/// d'avant ce lot (ici celle du lot 1, que l'ouverture migre : chaque page y est notée jamais
/// indexée) ; puis le rattrapage : ce qu'un autre `Store` a écrit dans une page déjà indexée,
/// serveur arrêté, sans rien annoncer, est indexé au démarrage suivant.
#[tokio::test]
async fn an_existing_instance_is_indexed_after_startup() {
    let (t, lot1) = TestApp::on_lot1_database().await;
    t.indexer.settled().await;
    for page in [lot1.root, lot1.child] {
        let (indexed, failed) = state(&t, page).await.expect("une ligne par page");
        assert!(indexed >= 0 && failed.is_none(), "{page} : {indexed}");
    }
    let (dir, mut cfg) = t.stop().await;

    // Serveur arrêté : la page reçoit un texte par un autre `Store`.
    let other = Store::open(&cfg.db_path()).await.unwrap();
    let blocks = parse("Écrit serveur arrêté\n").unwrap();
    let updates = match mementum_sync::stored_page(&other, lot1.root).await.unwrap() {
        Some((doc, _)) => vec![doc.write_blocks(&blocks).unwrap()],
        // Une page du lot 1 restée sans document : il est écrit avec son texte.
        None => {
            let (doc, creation) = PageDoc::new_page();
            vec![creation, doc.write_blocks(&blocks).unwrap()]
        }
    };
    let updates: Vec<&[u8]> = updates.iter().map(Vec::as_slice).collect();
    other
        .append_updates(lot1.root, &updates, None)
        .await
        .unwrap();
    other.close().await;

    cfg.index_after = QUICK;
    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, lot1.root).await, ["Écrit serveur arrêté"]);
    assert_eq!(
        state(&t, lot1.root).await,
        Some((last_seq(&t, lot1.root).await, None))
    );
}

/// Une page qu'on ne sait plus relire quitte l'index (ses lignes d'avant sont retirées), est
/// notée à son `seq`, dite une fois au journal, et n'est pas relue tant que son `seq` n'a pas
/// changé : ni au réveil suivant, ni au démarrage suivant.
#[tokio::test]
async fn a_page_that_cannot_be_read_is_tried_once_and_leaves_the_index() {
    let logs = Logs::capture();
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    write(&olga, page, "Lisible\n").await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["Lisible"]);
    let (dir, cfg) = t.stop().await;

    // Son état gardé devient illisible, et son `seq` avance.
    let seq = {
        let sql = format!(
            "SELECT MAX(COALESCE((SELECT MAX(seq) FROM doc_updates WHERE node_id = '{page}'), 0), \
                        COALESCE((SELECT seq FROM doc_snapshots WHERE node_id = '{page}'), 0))"
        );
        query_i64(&cfg.db_path(), &sql).await + 1
    };
    exec_sql(
        &cfg.db_path(),
        &format!(
            "DELETE FROM doc_updates WHERE node_id = '{page}'; \
             INSERT OR REPLACE INTO doc_snapshots (node_id, seq, state_blob, created_at) \
             VALUES ('{page}', {seq}, x'FFFFFFFFFFFFFFFFFFFF', 0)"
        ),
    )
    .await;

    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, Vec::<String>::new());
    assert_eq!(state(&t, page).await, Some((seq, Some(seq))));
    assert_eq!(
        t.indexer.reads(),
        1,
        "seule la page écrite depuis est relue"
    );
    t.indexer.settled().await;
    assert_eq!(
        t.indexer.reads(),
        1,
        "pas de nouvel essai au réveil suivant"
    );
    let journal = logs.text();
    let said = journal
        .lines()
        .filter(|line| line.contains("ERROR") && line.contains("impossible à relire"))
        .count();
    assert_eq!(said, 1, "{journal}");

    let (dir, cfg) = t.stop().await;
    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(
        t.indexer.reads(),
        0,
        "pas de nouvel essai au démarrage suivant"
    );
    assert_eq!(state(&t, page).await, Some((seq, Some(seq))));
}

/// Ruling R960 : une base indexée sous une autre version de la projection est indexée de
/// nouveau. Une ligne gardée sous l'ancienne forme est récrite, chaque page reçoit sa ligne
/// d'outbox de page entière, et la version du binaire est notée. Témoin : sous la même version,
/// la même ligne trafiquée reste telle quelle (la page est à jour, rien ne la fait relire).
#[tokio::test]
async fn a_database_indexed_under_another_projection_version_is_indexed_again() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    write(&olga, page, "Texte\n").await;
    t.indexer.settled().await;
    assert_eq!(
        t.store.projection_version().await.unwrap(),
        Some(mementum_search::PROJECTION_VERSION)
    );
    let (dir, cfg) = t.stop().await;
    let old_form = format!(
        "UPDATE blocks_index SET text = 'ancienne forme', hash = x'00' WHERE node_id = '{page}'; \
         DELETE FROM search_outbox"
    );

    exec_sql(&cfg.db_path(), &old_form).await;
    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["ancienne forme"], "témoin");
    let (dir, cfg) = t.stop().await;

    exec_sql(&cfg.db_path(), &old_form).await;
    exec_sql(
        &cfg.db_path(),
        "UPDATE instance_settings SET value = '0' WHERE key = 'search.projection'",
    )
    .await;
    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["Texte"]);
    assert_eq!(
        t.store.projection_version().await.unwrap(),
        Some(mementum_search::PROJECTION_VERSION)
    );
    let whole =
        format!("SELECT COUNT(*) FROM search_outbox WHERE node_id = '{page}' AND block_id = ''");
    assert_eq!(query_i64(&t.db_path(), &whole).await, 1);
}

/// Ruling R906 : une page écrite sans relâche (jamais deux secondes de calme) est indexée quand
/// même, quinze secondes au plus après sa première écriture non indexée, et pas avant : une
/// indexation par tranche de quinze secondes, quel que soit le nombre de frappes.
#[tokio::test]
async fn a_page_written_without_pause_is_indexed_at_most_every_bound() {
    let t = default_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    let addr = t.listen().await;
    let client = synced_as(&olga.browser, &sync_url(addr, page)).await;
    t.indexer.settled().await;
    let _clock = StoppedClock::start();
    let reads = t.indexer.reads();

    for cycle in 1..=2u64 {
        // Huit frappes à 1,9 s d'écart : 13,3 s après la première, jamais 2 s de calme.
        types(&client, "a").await;
        until("la page écrite n'attend pas son calme", async || {
            t.indexer.waits_for_calm(page)
        })
        .await;
        for _ in 0..7 {
            tokio::time::advance(Duration::from_millis(1_900)).await;
            types(&client, "a").await;
        }
        assert!(t.indexer.waits_for_calm(page), "la page attend encore");
        assert_eq!(t.indexer.reads(), reads + cycle - 1, "pas avant la borne");
        // La borne passée, sans une frappe de plus.
        tokio::time::advance(Duration::from_millis(1_900)).await;
        until("la page n'est pas indexée après sa borne", async || {
            state(&t, page).await == Some((last_seq(&t, page).await, None))
        })
        .await;
        assert_eq!(
            t.indexer.reads(),
            reads + cycle,
            "une indexation par tranche de quinze secondes"
        );
        assert_eq!(
            texts(&t.store, page).await,
            ["a".repeat(8 * cycle as usize)]
        );
    }
}

/// Les pages passent tour à tour : une page écrite sans relâche n'empêche pas une autre d'être
/// indexée après son propre calme. Témoin : la première, elle, attend encore.
#[tokio::test]
async fn pages_take_turns() {
    let t = default_app().await;
    let olga = t.account("Olga").await;
    let (busy, calm) = (
        new_page(&olga, "Écrite sans relâche").await,
        new_page(&olga, "Écrite une fois").await,
    );
    let addr = t.listen().await;
    let on_busy = synced_as(&olga.browser, &sync_url(addr, busy)).await;
    let on_calm = synced_as(&olga.browser, &sync_url(addr, calm)).await;
    t.indexer.settled().await;
    let _clock = StoppedClock::start();
    let reads = t.indexer.reads();

    types(&on_busy, "a").await;
    types(&on_calm, "une fois").await;
    until("les deux pages n'attendent pas leur calme", async || {
        t.indexer.waits_for_calm(busy) && t.indexer.waits_for_calm(calm)
    })
    .await;
    tokio::time::advance(Duration::from_millis(1_500)).await;
    types(&on_busy, "a").await;
    tokio::time::advance(Duration::from_millis(1_000)).await;
    until("la page calme n'est pas indexée", async || {
        texts(&t.store, calm).await == ["une fois"]
    })
    .await;
    assert_eq!(texts(&t.store, busy).await, Vec::<String>::new());
    assert!(
        t.indexer.waits_for_calm(busy),
        "la page écrite sans relâche attend"
    );
    assert_eq!(t.indexer.reads(), reads + 1);
}

/// Ruling R919 : une frappe dans une grande page ne récrit qu'une ligne. Un déclencheur témoin
/// compte les lignes insérées, mises à jour et retirées de `blocks_index` et de `page_links`.
/// Témoin : la première indexation de la page en écrit mille.
#[tokio::test]
async fn a_keystroke_in_a_large_page_writes_one_row() {
    const BLOCKS: usize = 1_000;
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Grande page").await;
    exec_sql(
        &t.db_path(),
        "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
         CREATE TRIGGER w_bi AFTER INSERT ON blocks_index \
         BEGIN INSERT INTO witness (what) VALUES ('bloc écrit'); END; \
         CREATE TRIGGER w_bu AFTER UPDATE ON blocks_index \
         BEGIN INSERT INTO witness (what) VALUES ('bloc récrit'); END; \
         CREATE TRIGGER w_bd AFTER DELETE ON blocks_index \
         BEGIN INSERT INTO witness (what) VALUES ('bloc retiré'); END; \
         CREATE TRIGGER w_li AFTER INSERT ON page_links \
         BEGIN INSERT INTO witness (what) VALUES ('lien écrit'); END; \
         CREATE TRIGGER w_ld AFTER DELETE ON page_links \
         BEGIN INSERT INTO witness (what) VALUES ('lien retiré'); END",
    )
    .await;
    let written = async || query_i64(&t.db_path(), "SELECT COUNT(*) FROM witness").await;
    let lines: Vec<String> = (0..BLOCKS).map(|i| format!("Ligne {i:04}")).collect();
    write(&olga, page, &format!("{}\n", lines.join("\n\n"))).await;
    t.indexer.settled().await;
    assert_eq!(
        written().await,
        BLOCKS as i64,
        "témoin : la première indexation"
    );

    let addr = t.listen().await;
    let client = synced_as(&olga.browser, &sync_url(addr, page)).await;
    types(&client, " et un mot").await;
    t.indexer.settled().await;
    assert_eq!(
        written().await,
        BLOCKS as i64 + 1,
        "une ligne pour une frappe"
    );
    let first = &texts(&t.store, page).await[0];
    assert_eq!(first, "Ligne 0000 et un mot");
    let lines =
        format!("SELECT COUNT(*) FROM search_outbox WHERE node_id = '{page}' AND block_id != ''");
    assert_eq!(query_i64(&t.db_path(), &lines).await, BLOCKS as i64);
}

/// Ce que l'indexeur sait en retard (Ruling R915) : une page écrite qui attend son calme est dite
/// en retard à qui la voit, et à lui seul ; à jour, elle ne l'est plus. (L'outbox est vidée à la
/// main : rien ne la clôt avant la Task S4c.)
#[tokio::test]
async fn behind_tells_a_seen_page_is_late_and_nothing_of_an_unseen_one() {
    let t = default_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    let addr = t.listen().await;
    let client = synced_as(&olga.browser, &sync_url(addr, page)).await;
    t.indexer.settled().await;
    exec_sql(&t.db_path(), "DELETE FROM search_outbox").await;
    let behind = async |seen: bool| {
        t.indexer
            .behind(olga.space, |id| seen && id == page)
            .await
            .unwrap()
    };
    assert!(!behind(true).await, "à jour");

    let _clock = StoppedClock::start();
    types(&client, "un").await;
    until("la page écrite n'est pas dite en retard", async || {
        behind(true).await
    })
    .await;
    assert!(!behind(false).await, "rien d'une page qu'on ne voit pas");
    tokio::time::advance(Duration::from_millis(2_100)).await;
    until("la page indexée est encore dite en retard", async || {
        exec_sql(&t.db_path(), "DELETE FROM search_outbox").await;
        !behind(true).await
    })
    .await;
    assert_eq!(texts(&t.store, page).await, ["un"]);
}

/// À l'arrêt, l'indexeur finit l'indexation en cours avant que la base ferme : une écriture de
/// l'index en file au moment de l'arrêt est validée, pas coupée. Le tour d'écriture est tenu
/// pendant que l'indexation attend, et l'arrêt est demandé pendant ce temps.
#[tokio::test]
async fn the_indexer_stops_before_the_store_closes() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    t.indexer.settled().await;
    let (store, indexer, db) = (t.store.clone(), t.indexer.clone(), t.db_path());
    let (space, by) = (olga.space, olga.id);
    drop(olga);

    let held = store.hold_writes().await;
    // Une page créée par un autre `Store`, dans une tâche à elle (cette tâche-ci tient un tour) :
    // le tour tenu ne retient pas son écriture.
    let page = tokio::spawn(async move {
        let other = Store::open(&db).await.unwrap();
        let page = other
            .create_node(NewNode {
                space_id: space,
                parent_id: None,
                after: None,
                kind: NodeKind::Page,
                title: "Créée pendant le tour".to_string(),
                created_by: Some(by),
                grants: Vec::new(),
                doc: Some(state_of("À écrire avant de fermer\n")),
            })
            .await
            .unwrap()
            .id;
        other.close().await;
        page
    })
    .await
    .unwrap();
    // L'indexeur relit ses marqueurs, et son écriture attend le tour.
    let settling = tokio::spawn({
        let indexer = indexer.clone();
        async move { indexer.settled().await }
    });
    until("l'écriture de l'index n'est pas en file", async || {
        store.queued_writes() == 1
    })
    .await;

    let stopping = tokio::spawn(t.stop());
    until("l'arrêt n'a pas atteint l'indexeur", async || {
        indexer.stopping()
    })
    .await;
    drop(held);
    let (_dir, cfg) = stopping.await.unwrap();
    settling.await.unwrap();
    drop((store, indexer));

    let after = Store::open(&cfg.db_path()).await.unwrap();
    assert_eq!(texts(&after, page).await, ["À écrire avant de fermer"]);
    after.close().await;
}
