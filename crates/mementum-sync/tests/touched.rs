//! Les pages écrites (Ruling R907) : l'acteur d'une page ne fait pour l'index qu'ajouter son
//! identifiant à un ensemble du hub, une fois son écriture au journal. Ce que l'indexeur lit par
//! [`Hub::take_touched`](mementum_sync::Hub::take_touched), réveillé par
//! [`Hub::touched_changed`](mementum_sync::Hub::touched_changed).
//!
//! Rien ici ne compte sur un minutage : ce que l'acteur a fait d'une frappe se lit une fois que le
//! serveur a répondu à ce qui la suit (`processed`), et l'absence d'un réveil se lit par un seul
//! tour de l'attente.

mod common;

use std::time::Duration;

use common::*;
use futures::FutureExt;
use mementum_crdt::META;
use mementum_schema::SCHEMA_VERSION;
use mementum_sync::{Hub, SyncConfig};
use uuid::Uuid;
use yrs::{Map, Transact};

/// Sans compaction : les seuls états encodés seraient ceux qu'on ne veut pas voir.
fn quiet() -> SyncConfig {
    SyncConfig {
        snapshot_every: u64::MAX,
        compact_after_deletions: u64::MAX,
        ..SyncConfig::default()
    }
}

/// Aucune page.
const NONE: [Uuid; 0] = [];

/// Les pages écrites depuis la dernière lecture, triées ; l'ensemble est vidé.
fn taken(hub: &Hub) -> Vec<Uuid> {
    let mut pages = hub.take_touched();
    pages.sort();
    pages
}

/// Un réveil attend-il ? Un seul tour de l'attente : s'il y en avait un, il est pris.
fn woken(hub: &Hub) -> bool {
    hub.touched_changed().now_or_never().is_some()
}

/// Les frappes de [`the_actor_only_names_the_page_it_wrote`], et combien partent avant d'attendre
/// le serveur : chaque frappe est validée sur le disque avant la suivante (jusqu'à 250 ms sur la
/// CI sous Windows, Ruling R441), et l'attente d'une rafale est bornée par [`WAIT`] : cinq frappes
/// y laissent une marge de huit.
const KEYSTROKES: usize = 60;
const BURST: usize = 5;

/// Review Focus 3 : la frappe ne paie rien pour l'index. Après des dizaines de frappes, l'ensemble
/// porte un identifiant, l'acteur n'a bâti aucune liste de blocs, encodé aucun état, et personne
/// ne lui a rien demandé.
#[tokio::test]
async fn the_actor_only_names_the_page_it_wrote() {
    let f = Fixture::new(quiet()).await;
    let id = f.ids().await[0].clone();
    let tab = f.client().await;
    assert_eq!(
        taken(&f.hub),
        NONE,
        "ni le document d'une page neuve, ni une lecture, ni une connexion ne nomment la page"
    );
    assert!(!woken(&f.hub));
    let reads = f.hub.block_reads();
    let encodings = f.hub.state_encodings();
    let calls = f.hub.calls();

    for _ in 0..KEYSTROKES / BURST {
        for _ in 0..BURST {
            tab.type_text(&id, "a");
        }
        processed(&tab).await;
    }

    assert_eq!(
        f.store.load_doc(f.page).await.unwrap().updates.len(),
        KEYSTROKES,
        "chaque frappe est au journal"
    );
    assert_eq!(
        f.hub.take_touched(),
        [f.page],
        "une entrée pour la page, pas une par frappe"
    );
    assert_eq!(f.hub.block_reads(), reads, "aucune liste de blocs bâtie");
    assert_eq!(f.hub.state_encodings(), encodings, "aucun état encodé");
    assert_eq!(f.hub.calls(), calls, "aucune demande à l'acteur");
    assert_eq!(taken(&f.hub), NONE, "l'ensemble a été vidé");
}

/// Le témoin du compte des états encodés : une compaction en encode un.
#[tokio::test]
async fn the_count_of_encodings_sees_a_compaction() {
    let f = Fixture::new(SyncConfig {
        snapshot_every: 2,
        ..quiet()
    })
    .await;
    let id = f.ids().await[0].clone();
    let tab = f.client().await;
    let encodings = f.hub.state_encodings();
    tab.type_text(&id, "a");
    tab.type_text(&id, "b");
    processed(&tab).await;
    f.hub.compacted(f.page).await.unwrap();
    assert_eq!(f.hub.state_encodings(), encodings + 1);
    assert_eq!(
        f.store.load_doc(f.page).await.unwrap().snapshot.unwrap().0,
        2,
        "la compaction a eu lieu"
    );
}

/// Les deux autres chemins par où un acteur écrit : `PUT /mmd` et une restauration.
#[tokio::test]
async fn a_write_of_the_server_and_a_restore_name_the_page() {
    // Une version avant chaque écriture qui en suit une autre : de quoi restaurer.
    let f = Fixture::new(quiet().with_version_every(Duration::ZERO)).await;
    f.write_mmd("Un\n").await;
    assert_eq!(taken(&f.hub), [f.page], "une écriture du serveur");
    f.write_mmd("Deux\n").await;
    assert_eq!(taken(&f.hub), [f.page]);

    let versions = f.store.list_versions(f.page).await.unwrap();
    assert_eq!(versions.len(), 1, "la version de « Un » : {versions:?}");
    f.hub
        .restore_version(f.page, versions[0].id, None)
        .await
        .unwrap();
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "Un");
    assert_eq!(taken(&f.hub), [f.page], "une restauration");

    // Restaurer ce que la page a déjà n'écrit rien (Ruling R180) : rien n'est nommé.
    f.hub
        .restore_version(f.page, versions[0].id, None)
        .await
        .unwrap();
    assert_eq!(taken(&f.hub), NONE);
}

/// Une mise à jour refusée (illisible, ou qui écrit dans les métadonnées de la page) ferme sa
/// connexion et n'est pas au journal : la page n'est pas nommée.
#[tokio::test]
async fn a_refused_update_names_nothing() {
    let f = Fixture::new(quiet()).await;
    let id = f.ids().await[0].clone();
    let journal = f.store.load_doc(f.page).await.unwrap();

    let garbled = f.client().await;
    garbled.send_update(&[0xFF, 0xFF, 0xFF, 0xFF, 0x07]);
    assert!(garbled.wait_until(WAIT, |c| c.is_closed()).await);

    let hostile = f.client().await;
    let doc = detached(&hostile);
    let meta = doc.get_or_insert_map(META);
    let update = {
        let mut txn = doc.transact_mut();
        meta.insert(&mut txn, "schemaVersion", 999);
        txn.encode_update_v1()
    };
    hostile.send_update(&update);
    assert!(hostile.wait_until(WAIT, |c| c.is_closed()).await);

    assert_eq!(f.store.load_doc(f.page).await.unwrap(), journal);
    assert_eq!(taken(&f.hub), NONE);
    assert!(!woken(&f.hub));

    // Le témoin : une frappe honnête, sur la même page, la nomme.
    let honest = f.client().await;
    honest.type_text(&id, "a");
    processed(&honest).await;
    assert_eq!(taken(&f.hub), [f.page]);
}

/// Ruling R181 : l'étape 2 d'un onglet qui revient sans rien de neuf n'est ni écrite ni relayée.
/// Elle ne nomme pas la page non plus : chaque onglet qui revient ferait sinon réindexer sa page.
#[tokio::test]
async fn an_update_that_changes_nothing_names_nothing() {
    let f = Fixture::new(quiet()).await;
    f.write_mmd("Bonjour\n").await;
    // Elle remplace la première : la page a des suppressions, que l'étape 2 porte.
    f.write_mmd("Bonjour tout le monde\n").await;
    let id = f.ids().await[0].clone();
    let tab = f.client().await;
    assert_eq!(taken(&f.hub), [f.page]);
    assert!(woken(&f.hub), "le réveil de ces deux écritures, pris ici");
    let journal = f.store.load_doc(f.page).await.unwrap();

    let page = tab.page().unwrap();
    let step2 = page.encode_diff(&page.state_vector()).unwrap();
    assert!(step2.len() > 2, "elle porte des suppressions : {step2:?}");
    tab.send_update(&step2);
    processed(&tab).await;
    assert_eq!(f.store.load_doc(f.page).await.unwrap(), journal);
    assert_eq!(taken(&f.hub), NONE);
    assert!(!woken(&f.hub));

    // Le témoin : une suppression seule change la page.
    let deletion = page.replace_text(&id, 0, 8, "").unwrap();
    tab.send_update(&deletion);
    processed(&tab).await;
    assert_eq!(taken(&f.hub), [f.page]);
}

/// Une écriture que le store refuse n'est pas au journal : la page n'est nommée qu'après
/// l'écriture, jamais avant.
#[tokio::test]
async fn a_write_the_store_refuses_names_nothing() {
    let f = Fixture::new(quiet()).await;
    exec_sql(
        &f.db,
        "CREATE TRIGGER refus BEFORE INSERT ON doc_updates \
         BEGIN SELECT RAISE(ABORT, 'refus du test'); END",
    )
    .await;
    let refused = f
        .hub
        .write_blocks(f.page, mementum_md::parse("Refusé\n").unwrap(), None)
        .await;
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(taken(&f.hub), NONE);
    assert!(!woken(&f.hub));

    exec_sql(&f.db, "DROP TRIGGER refus").await;
    f.write_mmd("Écrit\n").await;
    assert_eq!(taken(&f.hub), [f.page]);
}

/// L'ensemble grandit avec les pages écrites, pas avec les écritures.
#[tokio::test]
async fn the_set_is_bounded_by_the_pages_not_by_the_keystrokes() {
    let f = Fixture::new(quiet()).await;
    let other = new_page(&f.store).await;
    f.hub.create_page_doc(other).await.unwrap();
    let silent = new_page(&f.store).await;
    f.hub.create_page_doc(silent).await.unwrap();
    let first = f.client().await;
    let second = Client::connect_synced(&f.server.url(other, SCHEMA_VERSION)).await;
    let (first_id, second_id) = (first.blocks()[0].id.clone(), second.blocks()[0].id.clone());

    for _ in 0..3 {
        for _ in 0..BURST {
            first.type_text(&first_id, "a");
            second.type_text(&second_id, "b");
        }
        processed(&first).await;
        processed(&second).await;
    }
    let pages = f.hub.take_touched();
    assert_eq!(pages.len(), 2, "une entrée par page écrite : {pages:?}");
    let mut expected = [f.page, other];
    expected.sort();
    let mut sorted = pages;
    sorted.sort();
    assert_eq!(sorted, expected, "la page où rien n'est écrit n'y est pas");
}

/// La notification : sans contenu, quand l'ensemble passe de vide à non vide ; gardée si
/// personne ne l'attendait ; une seule tant que l'ensemble n'a pas été vidé.
#[tokio::test]
async fn the_wake_up_comes_when_the_set_stops_being_empty() {
    let f = Fixture::new(quiet()).await;
    assert!(!woken(&f.hub), "rien d'écrit, rien à lire");

    // Quelqu'un attend, puis la page est écrite.
    let hub = f.hub.clone();
    let waiting = tokio::spawn(async move { hub.touched_changed().await });
    f.write_mmd("Un\n").await;
    tokio::time::timeout(WAIT, waiting)
        .await
        .expect("le réveil de qui attendait")
        .unwrap();

    // L'ensemble n'est pas vide : une écriture de plus ne réveille pas de nouveau.
    f.write_mmd("Deux\n").await;
    assert!(!woken(&f.hub), "un seul réveil par ensemble à lire");
    assert_eq!(taken(&f.hub), [f.page]);

    // Personne n'attend : le réveil est gardé pour l'appel suivant.
    f.write_mmd("Trois\n").await;
    tokio::time::timeout(WAIT, f.hub.touched_changed())
        .await
        .expect("le réveil gardé");
    assert_eq!(taken(&f.hub), [f.page]);
    assert!(!woken(&f.hub));
}
