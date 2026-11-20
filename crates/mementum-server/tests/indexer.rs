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

/// Ce que l'index plein texte rend pour ces mots, parmi `pages` seulement : la page, le bloc
/// trouvé (aucun pour un titre), l'extrait.
fn found(t: &TestApp, words: &str, pages: &[Uuid]) -> Vec<(Uuid, Option<String>, String)> {
    let query = mementum_search::read_query(words).expect("une requête lisible");
    let allowed: mementum_search::PageSet = pages.iter().copied().collect();
    t.indexer
        .search(&query, &allowed, 0, 20)
        .unwrap()
        .pages
        .into_iter()
        .map(|hit| (hit.node_id, hit.block_id, hit.excerpt))
        .collect()
}

/// Les lignes de l'outbox, tous états confondus.
async fn outbox(t: &TestApp) -> i64 {
    query_i64(&t.db_path(), "SELECT COUNT(*) FROM search_outbox").await
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
    // Sa ligne de page entière a fait récrire ses entrées de l'index plein texte (Task S4c) :
    // il a été nourri de nouveau, et l'outbox est close.
    assert!(
        t.indexer.intents() >= 1,
        "aucun passage à l'index plein texte"
    );
    assert_eq!(found(&t, "texte", &[page]).len(), 1);
    assert_eq!(outbox(&t).await, 0);
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
}

/// Ruling R919 : une frappe dans une grande page ne récrit qu'une entrée de l'index plein texte.
/// Les écritures d'entrées sont comptées ; témoin : la première indexation en écrit une par bloc.
#[tokio::test]
async fn a_keystroke_in_a_large_page_rewrites_one_entry() {
    const BLOCKS: usize = 1_000;
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Grande page").await;
    t.indexer.settled().await;
    let before = t.indexer.entries_written();
    let lines: Vec<String> = (0..BLOCKS).map(|i| format!("Ligne {i:04}")).collect();
    write(&olga, page, &format!("{}\n", lines.join("\n\n"))).await;
    t.indexer.settled().await;
    assert_eq!(
        t.indexer.entries_written() - before,
        BLOCKS as u64,
        "témoin : la première indexation écrit une entrée par bloc"
    );

    let addr = t.listen().await;
    let client = synced_as(&olga.browser, &sync_url(addr, page)).await;
    let before = t.indexer.entries_written();
    types(&client, " et un mot").await;
    t.indexer.settled().await;
    assert_eq!(
        t.indexer.entries_written() - before,
        1,
        "une entrée pour une frappe"
    );
    let hits = found(&t, "mot", &[page]);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].2, "Ligne 0000 et un mot");
}

/// Ce que l'indexeur sait en retard (Ruling R915) : une page écrite qui attend son calme est dite
/// en retard à qui la voit, et à lui seul ; à jour (indexée, et ses lignes validées dans l'index
/// plein texte), elle ne l'est plus.
#[tokio::test]
async fn behind_tells_a_seen_page_is_late_and_nothing_of_an_unseen_one() {
    let t = default_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    let addr = t.listen().await;
    let client = synced_as(&olga.browser, &sync_url(addr, page)).await;
    t.indexer.settled().await;
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
        !behind(true).await
    })
    .await;
    assert_eq!(texts(&t.store, page).await, ["un"]);
    assert_eq!(found(&t, "un", &[page]).len(), 1);
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
    // Un second arrêt, demandé pendant le premier, attend lui aussi l'indexation en vol.
    let second = tokio::spawn({
        let indexer = indexer.clone();
        async move { indexer.stop().await }
    });
    for _ in 0..200 {
        tokio::task::yield_now().await;
    }
    assert!(!second.is_finished(), "le second arrêt n'a pas attendu");
    drop(held);
    let (_dir, cfg) = stopping.await.unwrap();
    second.await.unwrap();
    settling.await.unwrap();
    drop((store, indexer));

    let after = Store::open(&cfg.db_path()).await.unwrap();
    assert_eq!(texts(&after, page).await, ["À écrire avant de fermer"]);
    after.close().await;
}

/// Une page dont la relecture panique (un défaut de yrs devant un journal hostile) est essayée
/// une fois : elle quitte l'index comme une page illisible, notée à son `seq`, c'est dit une fois
/// en `error`, et l'indexeur passe à la suite (`settled` rend). Une page échouée n'est pas dite
/// en retard : seule sa ligne d'outbox l'est, jusqu'à ce que l'index plein texte l'ait validée.
#[tokio::test]
async fn a_page_whose_reading_panics_is_tried_once_and_leaves_the_index() {
    let logs = Logs::capture();
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let (page, other) = (
        new_page(&olga, "Hostile").await,
        new_page(&olga, "Saine").await,
    );
    write(&olga, page, "Lisible\n").await;
    t.indexer.settled().await;
    assert_eq!(texts(&t.store, page).await, ["Lisible"]);
    let reads = t.indexer.reads();

    t.indexer.panic_reading(page);
    write(&olga, page, "Écrit avant la panique\n").await;
    write(&olga, other, "La suite\n").await;
    tokio::time::timeout(REAL_WAIT, t.indexer.settled())
        .await
        .expect("l'indexeur réessaie sans fin la page qui panique");
    assert_eq!(t.indexer.reads(), reads + 2, "une relecture par page");
    assert_eq!(texts(&t.store, other).await, ["La suite"]);
    assert_eq!(texts(&t.store, page).await, Vec::<String>::new());
    let seq = last_seq(&t, page).await;
    assert_eq!(state(&t, page).await, Some((seq, Some(seq))));
    let journal = logs.text();
    let said = journal
        .lines()
        .filter(|line| line.contains("ERROR") && line.contains(&page.to_string()))
        .count();
    assert_eq!(said, 1, "{journal}");

    assert!(
        !t.indexer.behind(olga.space, |_| true).await.unwrap(),
        "une page échouée n'est pas en retard sans fin"
    );
}

/// Tant que le démarrage n'est pas fini (la reprojection, puis le rattrapage), l'indexeur ne sait
/// pas ce qui est en retard : `behind` répond « en retard », à tous. Ici la version gardée de la
/// projection est illisible : le démarrage ne finit pas, et la réponse reste « en retard ».
/// Témoin : la même instance, démarrage fini, répond « à jour ».
#[tokio::test]
async fn behind_says_late_until_the_startup_is_over() {
    let logs = Logs::capture();
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let space = olga.space;
    t.indexer.settled().await;
    assert!(!t.indexer.behind(space, |_| false).await.unwrap(), "témoin");
    drop(olga);
    let (dir, cfg) = t.stop().await;

    exec_sql(
        &cfg.db_path(),
        "UPDATE instance_settings SET value = 'deux' WHERE key = 'search.projection'",
    )
    .await;
    let t = TestApp::on(dir, cfg).await;
    assert!(t.indexer.behind(space, |_| false).await.unwrap());
    until(
        "l'indexeur n'a pas lu la version de la projection",
        async || logs.text().contains("version de la projection impossible"),
    )
    .await;
    assert!(t.indexer.behind(space, |_| false).await.unwrap());
}

/// Ruling R983 : la base ne rend que les mille premières pages en retard d'un espace, sans les
/// droits. Quand elle en rend autant sans qu'aucune soit vue, la réponse est « en retard » ;
/// témoin : à 999, pour qui n'en voit aucune, elle est « à jour ».
#[tokio::test]
async fn behind_says_late_at_its_bound_whoever_looks() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    t.indexer.settled().await;
    t.indexer.stop().await;
    let other = Store::open(&t.db_path()).await.unwrap();
    for i in 0..999 {
        stored(&other, olga.space, &format!("Page {i}"), None).await;
    }
    assert!(!t.indexer.behind(olga.space, |_| false).await.unwrap());
    stored(&other, olga.space, "La millième", None).await;
    other.close().await;
    assert!(t.indexer.behind(olga.space, |_| false).await.unwrap());
}

// ---------------------------------------------------------------------------------------------
// Task S4c : l'index plein texte, nourri depuis la base par intention, validation, clôture
// (Rulings R909, R919). Les arrêts et les séparations de la base et de l'index sont dans
// `tests/index_recovery.rs`.
// ---------------------------------------------------------------------------------------------

/// Un mot tapé dans une page devient trouvable, avec son bloc et son extrait, et le titre d'une
/// page l'est dès sa création. Rien n'est rendu hors de l'ensemble de pages donné. Une fois
/// l'indexeur au repos, l'outbox est vide, l'intention close, et le jeton de la base est celui
/// d'une validation. Témoin : avant la frappe, le mot ne trouve rien.
#[tokio::test]
async fn a_typed_word_becomes_findable() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Carnet").await;
    t.indexer.settled().await;
    assert!(found(&t, "bonjour", &[page]).is_empty(), "témoin");
    assert_eq!(
        found(&t, "carnet", &[page]),
        [(page, None, String::new())],
        "le titre d'une page créée"
    );

    let addr = t.listen().await;
    let client = synced_as(&olga.browser, &sync_url(addr, page)).await;
    types(&client, "bonjour tout le monde").await;
    t.indexer.settled().await;
    let hits = found(&t, "bonjour", &[page]);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].0, page);
    assert!(hits[0].1.is_some(), "le bloc trouvé");
    assert_eq!(hits[0].2, "bonjour tout le monde");
    assert!(
        found(&t, "bonjour", &[]).is_empty(),
        "hors des pages permises"
    );
    assert!(found(&t, "bonjour", &[Uuid::now_v7()]).is_empty());

    assert_eq!(outbox(&t).await, 0, "l'outbox est close");
    let tokens = t.store.feed_tokens().await.unwrap();
    assert!(tokens.committed.is_some(), "aucune validation close");
    assert_eq!(tokens.intent, None);
}

/// Ruling R902 : le titre est sur chaque entrée de sa page. Renommée, la page est trouvée par son
/// nouveau titre et plus par l'ancien, seul comme avec un mot d'un de ses blocs, et toutes ses
/// entrées ont été récrites (comptées). Témoin : avant, l'ancien titre la trouve.
#[tokio::test]
async fn a_rename_is_found_by_the_new_title_and_no_longer_by_the_old() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Ancien intitulé").await;
    write(&olga, page, "Alpha\n\nBravo\n\nCharlie\n").await;
    t.indexer.settled().await;
    assert_eq!(found(&t, "ancien", &[page]).len(), 1, "témoin");
    let with_block = found(&t, "ancien bravo", &[page]);
    assert_eq!(with_block.len(), 1, "témoin : {with_block:?}");
    assert_eq!(with_block[0].2, "Bravo");

    let before = t.indexer.entries_written();
    olga.patched(&page.to_string(), json!({ "title": "Nouveau nom" }))
        .await;
    t.indexer.settled().await;
    assert!(found(&t, "ancien", &[page]).is_empty());
    assert!(found(&t, "ancien bravo", &[page]).is_empty());
    assert_eq!(found(&t, "nouveau", &[page]).len(), 1);
    let with_block = found(&t, "nouveau bravo", &[page]);
    assert_eq!(with_block.len(), 1, "{with_block:?}");
    assert_eq!(with_block[0].2, "Bravo");
    assert_eq!(
        t.indexer.entries_written() - before,
        4,
        "l'entrée de titre et celle de chaque bloc"
    );
    assert_eq!(outbox(&t).await, 0);
}

/// Une page supprimée pour de bon quitte l'index plein texte, par la ligne d'outbox de page
/// entière que sa suppression pose (Ruling R918), qui survit à la page. Témoin : à la corbeille,
/// elle y est encore (décision L4 : c'est la requête qui écarte la corbeille, pas l'index).
#[tokio::test]
async fn a_deleted_page_leaves_the_text_index() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Éphémère").await;
    write(&olga, page, "Un contenu voué à partir\n").await;
    t.indexer.settled().await;
    assert_eq!(found(&t, "voué", &[page]).len(), 1);

    t.store.trash_node(page, olga.id).await.unwrap();
    t.indexer.settled().await;
    assert_eq!(found(&t, "voué", &[page]).len(), 1, "témoin : la corbeille");
    assert_eq!(found(&t, "éphémère", &[page]).len(), 1);

    t.store.delete_node(page).await.unwrap();
    t.indexer.settled().await;
    assert!(found(&t, "voué", &[page]).is_empty());
    assert!(found(&t, "éphémère", &[page]).is_empty());
    assert_eq!(outbox(&t).await, 0);
}

/// Une page qu'on ne sait plus relire quitte l'index plein texte : plus aucun résultat ni
/// extrait d'elle, ni par son texte ni par son titre, par la ligne d'outbox de page entière que
/// pose `Store::index_failed`. Témoin : elle était trouvée.
#[tokio::test]
async fn a_page_that_can_no_longer_be_read_leaves_the_text_index() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Abîmée").await;
    write(&olga, page, "Encore lisible\n").await;
    t.indexer.settled().await;
    assert_eq!(found(&t, "lisible", &[page]).len(), 1, "témoin");
    assert_eq!(found(&t, "abîmée", &[page]).len(), 1, "témoin");
    drop(olga);
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
    assert_eq!(state(&t, page).await, Some((seq, Some(seq))));
    assert!(found(&t, "lisible", &[page]).is_empty());
    assert!(found(&t, "abîmée", &[page]).is_empty());
    assert_eq!(outbox(&t).await, 0);
    assert_eq!(t.indexer.rebuilds(), 0, "l'index était celui de la base");
}

/// Une clôture refusée sans que le processus s'arrête (plan, « L'intention, la validation, la
/// clôture ») : l'index a validé, la base tient encore l'intention. Aucune intention neuve ne
/// commence ; le passage suivant rejoue celle-là sous le même jeton, puis clôt. Le mot écrit
/// pendant le refus est trouvé après, par l'intention qui suit. Les intentions sont comptées.
#[tokio::test]
async fn a_refused_close_is_replayed_and_closed_again_without_a_restart() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    t.indexer.settled().await;
    let intents = t.indexer.intents();

    // Une clôture refusée, puis plus rien n'est écrit : la même intention est rejouée et close.
    t.indexer.refuse_closes(1);
    write(&olga, page, "Premier jet\n").await;
    until("la clôture n'a pas été refusée", async || {
        t.indexer.close_refusals() == 1
    })
    .await;
    // Le jeton refusé : encore l'intention posée, ou déjà clos si le nouvel essai est passé.
    let posed = t.store.feed_tokens().await.unwrap();
    let refused = posed.intent.or(posed.committed);
    t.indexer.settled().await;
    assert_eq!(found(&t, "premier", &[page]).len(), 1);
    let closed = t.store.feed_tokens().await.unwrap();
    assert_eq!(closed.intent, None);
    assert_eq!(
        closed.committed, refused,
        "c'est le jeton refusé qui est clos"
    );
    assert_eq!(
        t.indexer.intents(),
        intents + 1,
        "aucune intention neuve : celle qui était posée est rejouée"
    );
    assert_eq!(outbox(&t).await, 0);

    // Une clôture refusée, puis un mot écrit pendant le refus : sa ligne perd son jeton, attend
    // l'intention suivante, et il est trouvé après.
    t.indexer.refuse_closes(1);
    write(&olga, page, "Second jet\n").await;
    until("la seconde clôture n'a pas été refusée", async || {
        t.indexer.close_refusals() == 2
    })
    .await;
    write(&olga, page, "Troisième jet\n").await;
    t.indexer.settled().await;
    assert!(found(&t, "premier", &[page]).is_empty());
    assert!(found(&t, "second", &[page]).is_empty());
    assert_eq!(found(&t, "troisième", &[page]).len(), 1);
    assert_eq!(t.store.feed_tokens().await.unwrap().intent, None);
    assert_eq!(
        t.indexer.intents(),
        intents + 3,
        "l'intention du second jet, rejouée, puis celle du troisième"
    );
    assert_eq!(t.indexer.close_refusals(), 2);
    assert_eq!(outbox(&t).await, 0);
    assert_eq!(t.indexer.rebuilds(), 0);
}

/// Une validation de l'index plein texte refusée défait l'intention, et le passage est tenté de
/// nouveau, sur un index rouvert : le mot est trouvé, par une seconde intention.
#[tokio::test]
async fn a_refused_commit_undoes_the_intent_and_is_tried_again() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    write(&olga, page, "Avant le refus\n").await;
    t.indexer.settled().await;
    let (intents, committed) = (
        t.indexer.intents(),
        t.store.feed_tokens().await.unwrap().committed,
    );

    t.indexer.refuse_commits(1);
    write(&olga, page, "Texte à valider\n").await;
    until("la validation n'a pas été refusée", async || {
        t.indexer.commit_refusals() == 1
    })
    .await;
    assert_eq!(
        t.store.feed_tokens().await.unwrap().committed,
        committed,
        "rien n'est clos par une validation refusée"
    );
    t.indexer.settled().await;
    assert_eq!(found(&t, "valider", &[page]).len(), 1);
    assert!(found(&t, "refus", &[page]).is_empty());
    assert_eq!(
        t.indexer.intents(),
        intents + 2,
        "l'intention défaite, puis une neuve"
    );
    let tokens = t.store.feed_tokens().await.unwrap();
    assert_eq!(tokens.intent, None);
    assert_ne!(tokens.committed, committed);
    assert_eq!(outbox(&t).await, 0);
    assert_eq!(
        t.indexer.rebuilds(),
        0,
        "l'index rouvert est celui de la base"
    );
}

// ---------------------------------------------------------------------------------------------
// Mesures (`#[ignore]`, en release, une à la fois : `cargo test --release -p mementum-server
// --test indexer -- --ignored --nocapture --test-threads=1`). Aucune n'est un critère de refus :
// leurs lignes `MESURE` vont au registre.
// ---------------------------------------------------------------------------------------------

/// Les mesures de ce binaire passent une à la fois (Ruling R341).
static ALONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Le rang `p` (de 0 à 100) d'une série, au plus proche rang.
fn rank(values: &mut [Duration], p: usize) -> Duration {
    values.sort();
    let at = (values.len() * p).div_ceil(100).max(1) - 1;
    values[at.min(values.len() - 1)]
}

/// Une page de l'espace, écrite par ce store, avec ce document.
async fn stored(store: &Store, space: Uuid, title: &str, doc: Option<Vec<u8>>) -> Uuid {
    store
        .create_node(NewNode {
            space_id: space,
            parent_id: None,
            after: None,
            kind: NodeKind::Page,
            title: title.to_string(),
            created_by: None,
            grants: Vec::new(),
            doc,
        })
        .await
        .unwrap()
        .id
}

/// Le premier sel de l'en-tête du WAL : il change à chaque fois que le journal repart du début,
/// c'est-à-dire après un point de contrôle complet.
fn wal_salt(db: &std::path::Path) -> Option<[u8; 4]> {
    use std::io::Read;
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    let mut header = [0u8; 20];
    std::fs::File::open(wal)
        .ok()?
        .read_exact(&mut header)
        .ok()?;
    Some([header[16], header[17], header[18], header[19]])
}

/// Step 0 de la Task S4b : le tour d'écriture que tient une transaction de l'index, sur
/// `Store::index_page` seul, selon le poids de ses lignes changées, et ce que la frappe d'une
/// autre page (une écriture de son journal, validée sur le disque) attend pendant ce temps. Le
/// nombre de fois où le WAL est reparti du début dit les points de contrôle déclenchés.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release"]
async fn measure_the_write_turn_held_by_an_index_transaction() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let _alone = ALONE.lock().await;
    const ROW_TEXT: usize = 900;
    // Une frappe : deux octets au journal de sa page, validés sur le disque.
    const KEYSTROKE: &[u8] = &[0, 0];
    for size in [64usize << 10, 256 << 10, 1 << 20, 4 << 20] {
        let dir = common::tmp::TestDir::new();
        let db = dir.path().join("m.db");
        let store = Store::open(&db).await.unwrap();
        let space = store.create_space("Espace", None).await.unwrap().id;
        let page = stored(&store, space, "Indexée", None).await;
        let typed = stored(&store, space, "Tapée", None).await;
        let rows = size / 1024;
        let rounds = ((32usize << 20) / size).clamp(16, 128);

        // La frappe seule, d'abord.
        let mut alone = Vec::new();
        for _ in 0..100 {
            let start = std::time::Instant::now();
            store
                .append_updates(typed, &[KEYSTROKE], None)
                .await
                .unwrap();
            alone.push(start.elapsed());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let typing = tokio::spawn({
            let (store, stop) = (store.clone(), stop.clone());
            async move {
                let mut waits = Vec::new();
                while !stop.load(Ordering::Relaxed) {
                    let start = std::time::Instant::now();
                    store
                        .append_updates(typed, &[KEYSTROKE], None)
                        .await
                        .unwrap();
                    waits.push(start.elapsed());
                }
                waits
            }
        });
        let (mut held, mut restarts, mut salt) = (Vec::new(), 0, wal_salt(&db));
        for round in 0..rounds {
            let texts: Vec<(String, String, [u8; 8])> = (0..rows)
                .map(|i| {
                    let text = format!("{round:06} {i:06} {}", "x".repeat(ROW_TEXT));
                    (format!("bloc-{i:06}"), text, (round as u64).to_le_bytes())
                })
                .collect();
            let views: Vec<mementum_store::IndexedBlock<'_>> = texts
                .iter()
                .map(|(block_id, text, hash)| mementum_store::IndexedBlock {
                    block_id,
                    kind: "paragraph",
                    text,
                    hash,
                    links: &[],
                })
                .collect();
            let start = std::time::Instant::now();
            let written = store
                .index_page_bounded(page, round as i64 + 1, &views, false, usize::MAX)
                .await
                .unwrap();
            held.push(start.elapsed());
            assert_eq!(written.changed, rows);
            let now = wal_salt(&db);
            if now != salt {
                restarts += 1;
                salt = now;
            }
        }
        stop.store(true, Ordering::Relaxed);
        let mut during = typing.await.unwrap();
        println!(
            "MESURE transaction de l'index de {} Kio ({rows} lignes de {ROW_TEXT} octets, {rounds} \
             transactions) : tenue p50 {:?}, p95 {:?}, max {:?} ; frappe d'une autre page seule \
             p95 {:?}, pendant ({} frappes) p50 {:?}, p95 {:?}, max {:?} ; le WAL est reparti du \
             début {restarts} fois",
            size >> 10,
            rank(&mut held, 50),
            rank(&mut held, 95),
            rank(&mut held, 100),
            rank(&mut alone, 95),
            during.len(),
            rank(&mut during, 50),
            rank(&mut during, 95),
            rank(&mut during, 100),
        );
        store.close().await;
    }
}

/// Un MMD de `blocks` paragraphes d'une phrase.
fn paragraphs(blocks: usize, word: &str) -> String {
    let lines: Vec<String> = (0..blocks)
        .map(|i| format!("Ligne {i:05} du {word}, une phrase ordinaire d'une page."))
        .collect();
    format!("{}\n", lines.join("\n\n"))
}

/// Step 3 : indexer une page de 10 000 blocs, phase par phase (les appels mêmes de l'indexeur) :
/// relire la page gardée, lire ses blocs et les projeter, écrire les tables ; puis la même page
/// après une frappe (un bloc changé).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release"]
async fn measure_indexing_a_10000_block_page() {
    let _alone = ALONE.lock().await;
    const BLOCKS: usize = 10_000;
    let dir = common::tmp::TestDir::new();
    let store = Store::open(&dir.path().join("m.db")).await.unwrap();
    let space = store.create_space("Espace", None).await.unwrap().id;
    let page = stored(
        &store,
        space,
        "Grande",
        Some(state_of(&paragraphs(BLOCKS, "plan"))),
    )
    .await;

    for pass in ["première indexation", "après une frappe"] {
        let start = std::time::Instant::now();
        let (doc, seq) = mementum_sync::stored_page(&store, page)
            .await
            .unwrap()
            .unwrap();
        let read = start.elapsed();
        let start = std::time::Instant::now();
        let (projection, doc) = tokio::task::spawn_blocking(move || {
            (mementum_search::project(&doc.read_blocks(), None), doc)
        })
        .await
        .unwrap();
        let projected = start.elapsed();
        let rows: Vec<mementum_store::IndexedBlock<'_>> = projection
            .rows
            .iter()
            .map(|row| mementum_store::IndexedBlock {
                block_id: &row.block_id,
                kind: row.kind,
                text: &row.text,
                hash: &row.hash,
                links: &row.links,
            })
            .collect();
        let start = std::time::Instant::now();
        let written = store
            .index_page(page, seq, &rows, projection.truncated)
            .await
            .unwrap();
        println!(
            "MESURE indexer une page de {BLOCKS} blocs, {pass} : relire {read:?}, lire les blocs \
             et projeter {projected:?}, écrire {:?} ({} lignes écrites, {} lignes projetées)",
            start.elapsed(),
            written.changed,
            rows.len(),
        );
        // La frappe : un mot dans le premier bloc, écrit au journal.
        let first = doc.read_blocks()[0].id.clone();
        let update = doc.replace_text(&first, 0, 0, "Mot ").unwrap();
        store.append_updates(page, &[&update], None).await.unwrap();
    }
    store.close().await;
}

/// Step 3 : la première indexation d'une instance existante par l'indexeur du serveur, et la
/// frappe d'un autre pendant ce temps (vue par un second onglet), comparée à la même frappe une
/// fois l'indexeur au repos. L'échelle est dite par la ligne `MESURE`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure, en release"]
async fn keystroke_seen_while_10000_pages_are_first_indexed() {
    let _alone = ALONE.lock().await;
    const PAGES: usize = 10_000;
    const BLOCKS: usize = 10;
    const KEYSTROKES: usize = 200;
    // Une instance avec un compte, arrêtée, puis remplie par un autre `Store`.
    let t = default_app().await;
    let olga = t.account("Olga").await;
    let typed = new_page(&olga, "Tapée").await;
    let (space, session) = (olga.space, olga.browser.session());
    t.indexer.settled().await;
    drop(olga);
    let (dir, cfg) = t.stop().await;
    let other = Store::open(&cfg.db_path()).await.unwrap();
    let doc = state_of(&paragraphs(BLOCKS, "rattrapage"));
    let filling = std::time::Instant::now();
    for i in 0..PAGES {
        stored(&other, space, &format!("Page {i}"), Some(doc.clone())).await;
    }
    other.close().await;
    println!(
        "MESURE {PAGES} pages de {BLOCKS} blocs créées en {:?}",
        filling.elapsed()
    );

    let t = TestApp::on(dir, cfg).await;
    let started = std::time::Instant::now();
    let browser = t.browser();
    browser.set_session(session.as_deref());
    let addr = t.listen().await;
    let writer = synced_as(&browser, &sync_url(addr, typed)).await;
    let reader = synced_as(&browser, &sync_url(addr, typed)).await;
    let block = writer.blocks()[0].id.clone();
    let settling = tokio::spawn({
        let indexer = t.indexer.clone();
        async move {
            indexer.settled().await;
            started.elapsed()
        }
    });
    let mut typed_so_far = 0;
    let mut keystrokes = async |count: usize| {
        let mut seen = Vec::new();
        for _ in 0..count {
            typed_so_far += 1;
            let expected = typed_so_far;
            let start = std::time::Instant::now();
            writer.type_text(&block, "x");
            assert!(
                reader
                    .wait_until(common::WAIT, |c| {
                        mementum_md::block_text(&c.blocks()[0]).len() == expected
                    })
                    .await,
                "la frappe n'est pas arrivée chez l'autre onglet"
            );
            seen.push(start.elapsed());
        }
        seen
    };
    let mut during = keystrokes(KEYSTROKES).await;
    let still_indexing = !settling.is_finished();
    let indexed_in = settling.await.unwrap();
    // L'indexeur a fini son rattrapage ; la page tapée attend encore son calme.
    t.indexer.settled().await;
    let mut after = keystrokes(KEYSTROKES).await;
    let reads = t.indexer.reads();
    println!(
        "MESURE première indexation de {PAGES} pages de {BLOCKS} blocs par le serveur : {indexed_in:?} \
         ({reads} pages relues) ; frappe vue par un autre onglet pendant ({KEYSTROKES} frappes, \
         indexation encore en cours à la dernière : {still_indexing}) p50 {:?}, p95 {:?}, max {:?} ; \
         indexeur au repos p50 {:?}, p95 {:?}, max {:?}",
        rank(&mut during, 50),
        rank(&mut during, 95),
        rank(&mut during, 100),
        rank(&mut after, 50),
        rank(&mut after, 95),
        rank(&mut after, 100),
    );
}
