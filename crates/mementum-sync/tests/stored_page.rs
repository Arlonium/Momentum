//! La lecture seule d'une page gardée (Ruling R907) : [`mementum_sync::stored_page`] relit ce que
//! le store garde d'une page, comme l'acteur le relit en se chargeant, sans passer par lui et sans
//! rien écrire. C'est ce que lit l'indexeur.

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::raw::{read_var, write_var};
use common::{
    Client, Fixture, WAIT, detached, live_becomes, new_page, processed, row_counts, text,
};
use mementum_crdt::{META, PageDoc};
use mementum_schema::SCHEMA_VERSION;
use mementum_sync::{SyncConfig, stored_page};
use sqlx::Row;
use uuid::Uuid;
use yrs::{Map, Transact};

/// Le plus grand `seq` que la base garde de la page, instantané ou journal, lu par une connexion
/// à part.
async fn last_seq(db: &Path, page: Uuid) -> i64 {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    let seq = sqlx::query(
        "SELECT MAX(seq) FROM (SELECT seq FROM doc_updates WHERE node_id = ?1 \
         UNION ALL SELECT seq FROM doc_snapshots WHERE node_id = ?1)",
    )
    .bind(page.to_string())
    .fetch_one(&pool)
    .await
    .unwrap()
    .get(0);
    pool.close().await;
    seq
}

/// La page gardée est la page que l'acteur sert : mêmes blocs, et le dernier `seq` de la base.
/// La page gardée est lue d'abord : l'acteur, s'il n'est pas vivant, est lancé par la seconde
/// lecture.
async fn agree(f: &Fixture, what: &str) {
    let (stored, seq) = stored_page(&f.store, f.page)
        .await
        .unwrap_or_else(|error| panic!("{what} : {error}"))
        .unwrap_or_else(|| panic!("{what} : la page a un document"));
    let served = f.hub.read_blocks(f.page).await.unwrap();
    assert_eq!(stored.read_blocks(), served, "{what}");
    assert_eq!(seq, last_seq(&f.db, f.page).await, "{what}");
}

/// Après chaque étape d'une scène qui passe par tout ce qu'un acteur écrit (frappes de deux
/// onglets, suppression, écriture du serveur, restauration, mise à jour refusée, compactions,
/// arrêt de l'acteur et sa version de fin), la page gardée est celle que l'acteur sert.
#[tokio::test]
async fn stored_page_reads_what_the_actor_serves() {
    let f = Fixture::new(SyncConfig {
        // Des compactions en route : l'instantané et la fin du journal se lisent ensemble.
        snapshot_every: 3,
        // Une version avant chaque écriture qui en suit une autre : de quoi restaurer.
        version_every: Duration::ZERO,
        idle_close: Duration::from_millis(100),
        ..SyncConfig::default()
    })
    .await;
    agree(&f, "page neuve").await;

    f.hub
        .write_blocks(f.page, mementum_md::parse("Un\n\nDeux\n").unwrap(), None)
        .await
        .unwrap();
    agree(&f, "écriture du serveur").await;

    let tab = f.client().await;
    let ids: Vec<String> = tab.blocks().into_iter().map(|block| block.id).collect();
    for _ in 0..4 {
        tab.type_text(&ids[0], "a");
    }
    processed(&tab).await;
    agree(&f, "frappes, une compaction peut-être en route").await;
    f.hub.compacted(f.page).await.unwrap();
    agree(&f, "frappes, compaction faite").await;
    assert_eq!(
        text(&f.hub.read_blocks(f.page).await.unwrap()),
        "Unaaaa\nDeux"
    );

    let other = f.client().await;
    other.type_text(&ids[1], "b");
    other.type_text(&ids[1], "c");
    processed(&other).await;
    agree(&f, "frappes d'un second onglet").await;

    assert!(
        tab.wait_until(WAIT, |c| text(&c.blocks()) == "Unaaaa\nDeuxbc")
            .await
    );
    let deletion = tab.page().unwrap().replace_text(&ids[0], 0, 2, "").unwrap();
    tab.send_update(&deletion);
    processed(&tab).await;
    agree(&f, "suppression").await;

    f.hub
        .write_blocks(f.page, mementum_md::parse("Trois\n").unwrap(), None)
        .await
        .unwrap();
    agree(&f, "page récrite").await;

    let versions = f.store.list_versions(f.page).await.unwrap();
    let oldest = versions.last().expect("des versions").id;
    f.hub.restore_version(f.page, oldest, None).await.unwrap();
    agree(&f, "restauration").await;
    assert_ne!(text(&f.hub.read_blocks(f.page).await.unwrap()), "Trois");

    let hostile = Client::connect_synced(&f.server.url(f.page, SCHEMA_VERSION)).await;
    let doc = detached(&hostile);
    let meta = doc.get_or_insert_map(META);
    let update = {
        let mut txn = doc.transact_mut();
        meta.insert(&mut txn, "schemaVersion", 999);
        txn.encode_update_v1()
    };
    hostile.send_update(&update);
    assert!(hostile.wait_until(WAIT, |c| c.is_closed()).await);
    agree(&f, "mise à jour refusée").await;

    drop((tab, other, hostile));
    assert!(live_becomes(&f.hub, &[]).await, "l'acteur s'arrête");
    agree(&f, "acteur arrêté").await;
}

/// Lire une page gardée n'écrit rien : une page sans document reste sans document (l'acteur, lui,
/// lui écrit le sien, Ruling R28), et aucun tour de la file d'écriture n'est attendu. Le tour est
/// tenu par le test pendant les lectures : une lecture qui écrirait attendrait sans fin.
#[tokio::test]
async fn stored_page_writes_nothing() {
    let f = Fixture::new(SyncConfig::default()).await;
    let bare = new_page(&f.store).await;
    let unknown = Uuid::now_v7();
    let rows = row_counts(&f.db).await;

    let turn = f.store.hold_writes().await;
    let (store, known) = (f.store.clone(), f.page);
    let reads = tokio::spawn(async move {
        (
            stored_page(&store, bare).await,
            stored_page(&store, unknown).await,
            stored_page(&store, known).await,
        )
    });
    let (of_bare, of_unknown, of_known) = tokio::time::timeout(WAIT, reads)
        .await
        .expect("une lecture seule n'attend aucun tour d'écriture")
        .unwrap();
    assert_eq!(f.store.queued_writes(), 0, "aucune écriture en file");
    drop(turn);
    assert!(
        matches!(of_bare, Ok(None)),
        "une page sans document n'a rien à lire"
    );
    assert!(matches!(of_unknown, Ok(None)), "une page inconnue non plus");
    let Ok(Some((page, seq))) = of_known else {
        panic!("la page qui a son document se lit");
    };
    assert_eq!((page.read_blocks().len(), seq), (1, 0));
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
    assert!(
        f.store.load_doc(bare).await.unwrap().snapshot.is_none(),
        "toujours sans document"
    );

    // Le témoin : l'acteur écrit le document de la page qui n'en avait pas.
    f.hub.load_page(bare).await.unwrap();
    assert_ne!(row_counts(&f.db).await, rows);
    let Ok(Some((page, seq))) = stored_page(&f.store, bare).await else {
        panic!("la page a maintenant son document");
    };
    assert_eq!((page.read_blocks().len(), seq), (1, 0));
}

/// Le journal d'une page, bâti à la main, dont la troisième entrée est écartée à la relecture
/// (Ruling R258) : la création, « Un », une entrée où le client de la racine paraît deux fois (un
/// `Skip` à son horloge courante, puis une plage ramassée `[0, 1)`), et « deux ». Rend les
/// entrées, et le rang de celle qui est écartée.
fn journal_with_a_repeated_client() -> (Vec<Vec<u8>>, usize) {
    let (created, creation) = PageDoc::new_page();
    let paragraph = created.read_blocks()[0].id.clone();
    let first = PageDoc::from_updates([creation.as_slice()]).unwrap();
    let one = first.replace_text(&paragraph, 0, 0, "Un").unwrap();
    // Le vecteur d'état de la page neuve : un client, celui de la racine, et son horloge.
    let state = created.state_vector();
    let mut at = 0;
    assert_eq!(read_var(&state, &mut at), 1);
    let root = read_var(&state, &mut at) as u64;
    let clock = read_var(&state, &mut at) as u64;
    let mut hostile = vec![2, 1];
    write_var(&mut hostile, root);
    write_var(&mut hostile, clock);
    hostile.extend([10, 1, 1]);
    write_var(&mut hostile, root);
    hostile.extend([0, 0, 1, 0]);
    assert!(PageDoc::repeats_a_client(&hostile));
    let second = PageDoc::from_updates([creation.as_slice(), one.as_slice()]).unwrap();
    let two = second.replace_text(&paragraph, 2, 0, " deux").unwrap();
    (vec![creation, one, hostile, two], 2)
}

/// Écrit ce journal pour une page neuve de la base de `f`, sans passer par un acteur. Rend la
/// page et le dernier `seq` écrit.
async fn page_with_a_repeated_client(f: &Fixture) -> (Uuid, i64) {
    let page = new_page(&f.store).await;
    let (journal, _) = journal_with_a_repeated_client();
    let entries: Vec<&[u8]> = journal.iter().map(Vec::as_slice).collect();
    let seq = f.store.append_updates(page, &entries, None).await.unwrap();
    (page, seq)
}

/// Une entrée que l'acteur écarte en se chargeant est écartée de même : la page gardée ne porte
/// rien que l'acteur ne servirait pas.
#[tokio::test]
async fn stored_page_sets_aside_what_load_sets_aside() {
    let f = Fixture::new(SyncConfig::default()).await;
    // Le témoin : rejoué sans rien écarter, ce journal ne se relit pas.
    let (journal, _) = journal_with_a_repeated_client();
    assert!(
        PageDoc::from_updates(journal.iter().map(Vec::as_slice)).is_err(),
        "l'entrée hostile est refusée à qui ne l'écarte pas"
    );

    let (page, last) = page_with_a_repeated_client(&f).await;
    let Ok(Some((stored, seq))) = stored_page(&f.store, page).await else {
        panic!("la page se lit sans l'entrée écartée");
    };
    assert_eq!(text(&stored.read_blocks()), "Un deux");
    assert_eq!(
        seq, last,
        "le dernier seq de la base, entrée écartée ou non"
    );
    assert_eq!(seq, last_seq(&f.db, page).await);
    assert_eq!(
        stored.read_blocks(),
        f.hub.read_blocks(page).await.unwrap(),
        "ce que l'acteur sert"
    );
}

/// Ce qu'un test voit du journal du fil courant : le niveau et le message de chaque ligne.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<(tracing::Level, String)>>>);

impl Lines {
    fn at(&self, level: tracing::Level) -> Vec<String> {
        let lines = self.0.lock().unwrap();
        lines
            .iter()
            .filter(|(at, _)| *at == level)
            .map(|(_, message)| message.clone())
            .collect()
    }
}

struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl tracing::Subscriber for Lines {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut message = Message(String::new());
        event.record(&mut message);
        self.0
            .lock()
            .unwrap()
            .push((*event.metadata().level(), message.0));
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// L'acteur a déjà dit, en `error`, chaque entrée qu'il écarte ; l'indexeur relit les mêmes pages
/// après lui, et ne le redit pas : la lecture seule l'écrit en `debug`.
///
/// Le second abonné, inerte, est là pour que `tracing` décide de l'intérêt d'un point de journal
/// sur tous les abonnés, et pas d'après le fil d'un autre test qui l'aurait atteint le premier,
/// sans abonné.
#[tokio::test]
async fn stored_page_logs_no_error() {
    let _never_alone = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let lines = Lines::default();
    let _guard = tracing::subscriber::set_default(lines.clone());
    let f = Fixture::new(SyncConfig::default()).await;
    let (page, _) = page_with_a_repeated_client(&f).await;

    assert!(matches!(stored_page(&f.store, page).await, Ok(Some(_))));
    assert_eq!(lines.at(tracing::Level::ERROR), Vec::<String>::new());
    let said = lines.at(tracing::Level::DEBUG);
    assert!(
        said.iter().any(|line| line.contains("écartée")),
        "l'entrée écartée est dite, en `debug` : {said:?}"
    );

    // Le témoin : l'acteur qui se charge, sur ce même fil, l'écrit en `error`.
    f.hub.load_page(page).await.unwrap();
    let errors = lines.at(tracing::Level::ERROR);
    assert!(
        errors.len() == 1 && errors[0].contains("écartée"),
        "{errors:?}"
    );
}
