//! L'index plein texte et la base ne se séparent jamais sans que le démarrage le voie (lot 4,
//! Task S4c ; plan du lot, « L'intention, la validation, la clôture », Ruling R909 ; Review
//! Focus 5 : aucun extrait d'un texte que la base n'a pas).
//!
//! Les arrêts : l'indexeur est arrêté à un point nommé (`halt_before_the_commit`,
//! `halt_before_the_close`), comme un processus tué là, jamais par un minutage. Les séparations :
//! le serveur est arrêté et sa base fermée avant toute copie (on ne restaure ni ne récrit une
//! base sous un serveur qui tourne), puis relancé sur le même dossier. Aucun test n'attend une
//! durée : `IndexerHandle::settled` rend quand l'indexeur n'a plus rien à projeter ni à valider.

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::http::StatusCode;
use common::sync::types;
use common::tmp::TestDir;
use common::{Account, TestApp, exec_sql, lot1_database, query_i64, sync_url, synced_as};
use mementum_crdt::PageDoc;
use mementum_md::parse;
use mementum_server::{Config, SignupMode};
use mementum_store::{NewNode, NodeKind, Store};
use serde_json::json;
use uuid::Uuid;

/// Délai maximal, en temps réel, d'une attente de condition.
const REAL_WAIT: Duration = Duration::from_secs(20);

/// Le calme attendu avant d'indexer.
const QUICK: Duration = Duration::from_millis(20);

async fn quick_app() -> TestApp {
    TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.index_after = QUICK;
    })
    .await
}

/// Attend `ready` ([`REAL_WAIT`] au plus) : la borne peut faire échouer le test à tort sur un
/// poste très chargé, jamais le faire passer.
async fn until(what: &str, mut ready: impl AsyncFnMut() -> bool) {
    let deadline = std::time::Instant::now() + REAL_WAIT;
    while !ready().await {
        assert!(std::time::Instant::now() < deadline, "jamais vu : {what}");
        tokio::task::yield_now().await;
    }
}

async fn new_page(by: &Account<'_>, title: &str) -> Uuid {
    by.create(json!({ "title": title }))
        .await
        .id
        .parse()
        .unwrap()
}

async fn write(by: &Account<'_>, page: Uuid, mmd: &str) {
    by.put_mmd(&page.to_string(), mmd.to_string())
        .await
        .assert_status(StatusCode::NO_CONTENT);
}

/// Les pages que l'index plein texte rend pour ces mots, parmi `pages` seulement.
fn found(t: &TestApp, words: &str, pages: &[Uuid]) -> Vec<Uuid> {
    let query = mementum_search::read_query(words).expect("une requête lisible");
    let allowed: mementum_search::PageSet = pages.iter().copied().collect();
    t.indexer
        .search(&query, &allowed, 0, 20)
        .unwrap()
        .pages
        .into_iter()
        .map(|hit| hit.node_id)
        .collect()
}

async fn outbox(t: &TestApp) -> i64 {
    query_i64(&t.db_path(), "SELECT COUNT(*) FROM search_outbox").await
}

/// Les fichiers d'une base : elle-même, son journal, l'index de son journal.
fn database_files(db: &Path) -> [PathBuf; 3] {
    let named = |suffix: &str| {
        let mut name = db.file_name().unwrap().to_os_string();
        name.push(suffix);
        db.with_file_name(name)
    };
    [db.to_path_buf(), named("-wal"), named("-shm")]
}

/// Copie une base fermée dans `to`.
fn copy_database(cfg: &Config, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for file in database_files(&cfg.db_path()) {
        if file.exists() {
            std::fs::copy(&file, to.join(file.file_name().unwrap())).unwrap();
        }
    }
}

/// Remet en place la copie de `from`, à la place de la base (fermée) : rien d'autre du dossier
/// des données n'est touché, `search/` reste tel quel.
fn put_database_back(from: &Path, cfg: &Config) {
    for file in database_files(&cfg.db_path()) {
        if file.exists() {
            std::fs::remove_file(&file).unwrap();
        }
        let copy = from.join(file.file_name().unwrap());
        if copy.exists() {
            std::fs::copy(&copy, &file).unwrap();
        }
    }
}

/// Écrit `mmd` dans la page par un autre `Store`, serveur arrêté : rien ne l'annonce à personne.
async fn write_offline(cfg: &Config, page: Uuid, mmd: &str) {
    let other = Store::open(&cfg.db_path()).await.unwrap();
    let blocks = parse(mmd).unwrap();
    let updates = match mementum_sync::stored_page(&other, page).await.unwrap() {
        Some((doc, _)) => vec![doc.write_blocks(&blocks).unwrap()],
        None => {
            let (doc, creation) = PageDoc::new_page();
            vec![creation, doc.write_blocks(&blocks).unwrap()]
        }
    };
    let updates: Vec<&[u8]> = updates.iter().map(Vec::as_slice).collect();
    other.append_updates(page, &updates, None).await.unwrap();
    other.close().await;
}

/// Un arrêt entre l'intention et la validation : au démarrage, le jeton de l'index est encore
/// celui de la dernière validation close, et l'intention restée posée n'a jamais été validée.
/// Elle est défaite, rien n'est reconstruit, et ses lignes passent par une intention neuve.
#[tokio::test]
async fn a_stop_before_the_commit_undoes_the_intent() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    t.indexer.settled().await;

    t.indexer.halt_before_the_commit();
    write(&olga, page, "Tapé avant la panne\n").await;
    until(
        "l'indexeur n'est pas arrivé à sa validation",
        async || t.indexer.halted(),
    )
    .await;
    let posed = t.store.feed_tokens().await.unwrap();
    assert!(posed.intent.is_some(), "l'intention est posée");
    drop(olga);
    let (dir, cfg) = t.stop().await;

    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(found(&t, "panne", &[page]), [page]);
    assert_eq!(
        t.indexer.rebuilds(),
        0,
        "l'index est resté celui de la base"
    );
    assert_eq!(
        t.indexer.intents(),
        1,
        "l'intention défaite, ses lignes prises par une neuve"
    );
    let tokens = t.store.feed_tokens().await.unwrap();
    assert_eq!(tokens.intent, None);
    assert_ne!(tokens.committed, posed.intent);
    assert_ne!(tokens.committed, posed.committed);
    assert_eq!(outbox(&t).await, 0);
}

/// Un arrêt entre la validation et la clôture : au démarrage, le jeton de l'index est celui de
/// l'intention posée. Elle est rejouée puis close, sous le même jeton, sans intention neuve ni
/// reconstruction.
#[tokio::test]
async fn a_stop_between_the_commit_and_the_close_replays_the_intent() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    t.indexer.settled().await;

    t.indexer.halt_before_the_close();
    write(&olga, page, "Validé avant la panne\n").await;
    until("l'indexeur n'est pas arrivé à sa clôture", async || {
        t.indexer.halted()
    })
    .await;
    let posed = t.store.feed_tokens().await.unwrap();
    assert!(posed.intent.is_some(), "l'intention est posée");
    assert!(outbox(&t).await > 0, "ses lignes ne sont pas closes");
    drop(olga);
    let (dir, cfg) = t.stop().await;

    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(found(&t, "panne", &[page]), [page]);
    assert_eq!(t.indexer.rebuilds(), 0);
    assert_eq!(t.indexer.intents(), 0, "rejouée, pas remplacée");
    let tokens = t.store.feed_tokens().await.unwrap();
    assert_eq!(tokens.intent, None);
    assert_eq!(
        tokens.committed, posed.intent,
        "close sous son propre jeton"
    );
    assert_eq!(outbox(&t).await, 0);
}

/// Une copie de la base prise entre l'intention et la clôture, puis remise en place : le jeton
/// de l'index est celui de son intention, mais la base remise en place peut porter un autre
/// texte que celui que l'index a validé. Les lignes de l'intention sont donc **rejouées depuis la
/// base**, jamais tenues pour faites.
///
/// La scène : l'indexeur est arrêté entre la validation et la clôture, le serveur aussi ; puis
/// le texte de la ligne, dans `blocks_index`, est changé à la main, la ligne gardant le jeton de
/// l'intention : c'est l'état d'une copie prise avant que l'index valide un texte plus récent.
#[tokio::test]
async fn a_copy_taken_between_the_intent_and_the_close_replays_from_the_database() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    t.indexer.settled().await;

    t.indexer.halt_before_the_close();
    write(&olga, page, "Le texte que valide le moteur\n").await;
    until("l'indexeur n'est pas arrivé à sa clôture", async || {
        t.indexer.halted()
    })
    .await;
    let posed = t.store.feed_tokens().await.unwrap();
    drop(olga);
    let (dir, cfg) = t.stop().await;
    exec_sql(
        &cfg.db_path(),
        &format!(
            "UPDATE blocks_index SET text = 'Celui de la base remise en place' \
             WHERE node_id = '{page}'"
        ),
    )
    .await;
    let taken = format!(
        "SELECT COUNT(*) FROM search_outbox WHERE node_id = '{page}' AND token IS NOT NULL"
    );
    assert!(
        query_i64(&cfg.db_path(), &taken).await > 0,
        "la ligne porte encore le jeton de l'intention"
    );

    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert!(
        found(&t, "moteur", &[page]).is_empty(),
        "un texte que la base n'a pas"
    );
    assert_eq!(found(&t, "remise", &[page]), [page]);
    assert_eq!(t.indexer.rebuilds(), 0);
    assert_eq!(t.indexer.intents(), 0);
    let tokens = t.store.feed_tokens().await.unwrap();
    assert_eq!((tokens.intent, tokens.committed), (None, posed.intent));
}

/// Un renommage validé dans la base, puis le serveur arrêté avant toute validation de l'index :
/// relancé, l'ancien titre ne trouve plus la page, le nouveau la trouve, sans aucune écriture de
/// plus. C'est le marqueur durable du renommage (Ruling R918) qui le porte, pas une annonce.
#[tokio::test]
async fn a_rename_before_a_crash_is_caught_up() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Titre original").await;
    t.indexer.settled().await;
    assert_eq!(found(&t, "original", &[page]), [page], "témoin");

    // L'index plein texte ne valide plus rien : comme un serveur tué juste après le renommage.
    t.indexer.stop().await;
    olga.patched(&page.to_string(), json!({ "title": "Titre retouché" }))
        .await;
    drop(olga);
    let (dir, cfg) = t.stop().await;

    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert!(found(&t, "original", &[page]).is_empty());
    assert_eq!(found(&t, "retouché", &[page]), [page]);
    assert_eq!(t.indexer.rebuilds(), 0);
}

/// Une frappe validée dans la base, puis le serveur arrêté avant qu'elle soit indexée : rien de
/// durable ne la nomme (le hub ne garde ses pages écrites qu'en mémoire), et c'est le rattrapage
/// du démarrage qui la retrouve. Relancé, le mot est trouvé.
#[tokio::test]
async fn a_keystroke_before_a_crash_is_caught_up() {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let page = new_page(&olga, "Notes").await;
    t.indexer.settled().await;

    t.indexer.stop().await;
    let addr = t.listen().await;
    let client = synced_as(&olga.browser, &sync_url(addr, page)).await;
    types(&client, "frappe orpheline").await;
    drop((client, olga));
    let (dir, cfg) = t.stop().await;

    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(found(&t, "orpheline", &[page]), [page]);
    assert_eq!(t.indexer.rebuilds(), 0);
}

/// Review Focus 5 : une copie de la base, puis `commits` validations de l'index, puis la copie
/// remise en place, `search/` gardé. Le jeton de l'index n'est ni celui de la dernière validation
/// close de la base remise en place, ni celui de son intention : au démarrage tout est
/// reconstruit, et le mot écrit après la copie n'est plus trouvé. Témoin : avant la remise en
/// place, il l'est.
async fn a_copy_put_back_rebuilds(commits: u64) {
    let t = quick_app().await;
    let olga = t.account("Olga").await;
    let (page, draft) = (
        new_page(&olga, "Notes").await,
        new_page(&olga, "Brouillon").await,
    );
    write(&olga, page, "Le texte de la copie\n").await;
    t.indexer.settled().await;
    drop(olga);
    let (dir, cfg) = t.stop().await;
    let copy = dir.path().join("copie");
    copy_database(&cfg, &copy);

    // Un mot écrit après la copie : une seule validation de l'index le porte.
    write_offline(&cfg, page, "Un mot neuf\n").await;
    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(t.indexer.intents(), 1, "une seule validation");
    // Puis les autres, une par renommage.
    for i in 1..commits {
        t.store
            .rename_node(draft, &format!("Brouillon {i}"), None)
            .await
            .unwrap();
        t.indexer.settled().await;
    }
    assert_eq!(t.indexer.intents(), commits);
    assert_eq!(found(&t, "neuf", &[page]), [page], "témoin");
    assert_eq!(t.indexer.rebuilds(), 0, "témoin");
    let (dir, cfg) = t.stop().await;

    put_database_back(&copy, &cfg);
    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(
        t.indexer.rebuilds(),
        1,
        "l'index n'est pas celui de la base"
    );
    assert!(
        found(&t, "neuf", &[page]).is_empty(),
        "un texte que la base n'a pas"
    );
    assert_eq!(found(&t, "copie", &[page]), [page]);
    assert_eq!(found(&t, "brouillon", &[draft]), [draft]);
    assert_eq!(outbox(&t).await, 0);
    let tokens = t.store.feed_tokens().await.unwrap();
    assert_eq!(tokens.intent, None);
    assert!(tokens.committed.is_some());
}

#[tokio::test]
async fn a_copy_then_one_commit_then_the_copy_put_back_rebuilds() {
    a_copy_put_back_rebuilds(1).await;
}

#[tokio::test]
async fn a_copy_then_a_hundred_commits_then_the_copy_put_back_rebuilds() {
    a_copy_put_back_rebuilds(100).await;
}

/// Le dossier `search/` d'une autre instance, posé à la place de celui de celle-ci : son jeton
/// n'est pas de cette base. Il est vidé et reconstruit : rien de l'autre instance n'est trouvé,
/// même en nommant ses pages, et les pages d'ici le sont.
#[tokio::test]
async fn a_directory_of_another_instance_is_rebuilt() {
    let (a, b) = (quick_app().await, quick_app().await);
    let (ana, ben) = (a.account("Ana").await, b.account("Ben").await);
    let (theirs, ours) = (
        new_page(&ana, "Chez eux").await,
        new_page(&ben, "Chez nous").await,
    );
    write(&ana, theirs, "Un secret voisin\n").await;
    write(&ben, ours, "Un texte local\n").await;
    a.indexer.settled().await;
    b.indexer.settled().await;
    assert_eq!(found(&a, "voisin", &[theirs]), [theirs], "témoin");
    drop((ana, ben));
    let (_a_dir, a_cfg) = a.stop().await;
    let (b_dir, b_cfg) = b.stop().await;

    let (from, to) = (a_cfg.data_dir.join("search"), b_cfg.data_dir.join("search"));
    std::fs::remove_dir_all(&to).unwrap();
    std::fs::create_dir_all(&to).unwrap();
    for entry in std::fs::read_dir(&from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }

    let b = TestApp::on(b_dir, b_cfg).await;
    b.indexer.settled().await;
    assert_eq!(b.indexer.rebuilds(), 1);
    assert!(found(&b, "voisin", &[theirs, ours]).is_empty());
    assert!(found(&b, "eux", &[theirs, ours]).is_empty());
    assert_eq!(found(&b, "local", &[theirs, ours]), [ours]);
    assert_eq!(found(&b, "nous", &[theirs, ours]), [ours]);
}

/// Une copie d'avant la migration `0005` remise en place, `search/` gardé : migrée de nouveau,
/// la base n'a aucun jeton, l'index en porte un. Tout est reconstruit : la page née après la
/// copie n'est plus trouvée, celles de la copie le sont.
#[tokio::test]
async fn a_backup_put_back_before_0005_rebuilds_after_migrating_again() {
    let dir = TestDir::new();
    let cfg = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        data_dir: dir.path().join("donnees"),
        index_after: QUICK,
        ..Config::default()
    };
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    let lot1 = lot1_database(&cfg.db_path()).await;
    let backup = dir.path().join("sauvegarde");
    copy_database(&cfg, &backup);

    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(found(&t, "racine", &[lot1.root]), [lot1.root], "témoin");
    let late = t
        .store
        .create_node(NewNode {
            space_id: lot1.vault,
            parent_id: None,
            after: None,
            kind: NodeKind::Page,
            title: "Née après la sauvegarde".to_string(),
            created_by: None,
            grants: Vec::new(),
            doc: None,
        })
        .await
        .unwrap()
        .id;
    t.indexer.settled().await;
    assert_eq!(found(&t, "sauvegarde", &[late]), [late], "témoin");
    assert!(t.store.feed_tokens().await.unwrap().committed.is_some());
    let (dir, cfg) = t.stop().await;

    put_database_back(&backup, &cfg);
    let t = TestApp::on(dir, cfg).await;
    t.indexer.settled().await;
    assert_eq!(t.indexer.rebuilds(), 1);
    assert!(found(&t, "sauvegarde", &[late, lot1.root]).is_empty());
    assert_eq!(found(&t, "racine", &[lot1.root]), [lot1.root]);
    assert_eq!(
        found(&t, "sous-page", &[lot1.root, lot1.child]),
        [lot1.child]
    );
    assert_eq!(outbox(&t).await, 0);
}
