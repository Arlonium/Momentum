//! Synchronisation entre clients : convergence, page inconnue, déplacements
//! concurrents, client en retard, mises à jour refusées, awareness, acteurs
//! qui s'arrêtent et repartent.

mod common;

use std::time::{Duration, Instant};

use common::*;
use futures::StreamExt;
use mementum_crdt::FRAGMENT;
use mementum_md::parse;
use mementum_schema::SCHEMA_VERSION;
use mementum_sync::{SyncConfig, SyncError};
use tokio::task::JoinSet;
use uuid::Uuid;
use yrs::{Doc, OffsetKind, Options, Transact, Xml, XmlElementPrelim, XmlFragment, XmlTextPrelim};

#[tokio::test]
async fn two_clients_converge() {
    let f = Fixture::new(SyncConfig::default()).await;
    let id = f.ids().await[0].clone();
    let a = f.client().await;
    let b = f.client().await;

    // Chronomètre lancé juste avant l'envoi chez A, jamais avant la connexion.
    let start = Instant::now();
    a.type_text(&id, "x");
    assert!(
        b.wait_until(WAIT, |b| text(&b.blocks()) == "x").await,
        "B n'a pas reçu la frappe de A"
    );
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(150),
        "frappe visible chez B en {elapsed:?} (budget : 150 ms)"
    );
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "x");
}

#[tokio::test]
async fn unknown_page_is_rejected() {
    let f = Fixture::new(SyncConfig::default()).await;
    let before = row_counts(&f.db).await;
    let unknown = Uuid::now_v7();

    let (server_end, _client_end) = memory_socket(8);
    let served = f
        .hub
        .clone()
        .serve_connection(unknown, SCHEMA_VERSION, server_end)
        .await;
    assert!(matches!(served, Err(SyncError::NotFound)), "{served:?}");

    // Par le serveur de test : la connexion se ferme sans rien recevoir.
    let client = Client::connect(&f.server.url(unknown, SCHEMA_VERSION))
        .await
        .unwrap();
    assert!(client.wait_until(WAIT, |c| c.is_closed()).await);
    assert!(!client.is_synced());

    // Les autres entrées du hub aussi.
    assert!(matches!(
        f.hub.read_blocks(unknown).await,
        Err(SyncError::NotFound)
    ));
    assert!(matches!(
        f.hub.write_blocks(unknown, parse("x\n").unwrap()).await,
        Err(SyncError::NotFound)
    ));
    assert!(matches!(
        f.hub.create_page_doc(unknown).await,
        Err(SyncError::NotFound)
    ));

    assert_eq!(row_counts(&f.db).await, before, "aucune nouvelle ligne");
}

#[tokio::test]
async fn concurrent_moves_are_deduped() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("A\n\nB\n\nC\n\nD\n").await;
    let ids = f.ids().await;
    let a = f.client().await;
    let b = f.client().await;

    // Le même bloc A, déplacé chez les deux clients depuis le même état : après
    // C chez A, après D chez B. Yjs copie le bloc à chaque déplacement.
    let from_a = a
        .page()
        .unwrap()
        .move_block(&ids[0], Some(&ids[2]))
        .unwrap();
    let from_b = b
        .page()
        .unwrap()
        .move_block(&ids[0], Some(&ids[3]))
        .unwrap();
    a.apply_local(&from_a);
    b.apply_local(&from_b);

    // Le serveur garde le premier exemplaire dans l'ordre du document. B n'y
    // arrive qu'avec le déplacement de A puis le dédoublonnage du serveur.
    let deduped = |c: &Client| text(&c.blocks()) == "B\nC\nA\nD";
    assert!(
        b.wait_until(WAIT, deduped).await,
        "B : {:?}",
        text(&b.blocks())
    );
    let server = f.hub.read_blocks(f.page).await.unwrap();
    assert_eq!(text(&server), "B\nC\nA\nD");
    assert_eq!(server.iter().filter(|x| x.id == ids[0]).count(), 1);

    // A reçoit aussi le dédoublonnage par la diffusion, qu'il ait envoyé le
    // premier ou le second déplacement : son état rejoint celui de B.
    let b_state = b.state_vector();
    assert!(
        a.wait_until(WAIT, |c| c.state_vector() == b_state).await,
        "A n'a pas reçu le dédoublonnage"
    );
    assert_eq!(a.blocks(), server);
    assert_eq!(b.blocks(), server);
    assert_eq!(stored_page(&f.store, f.page).await.read_blocks(), server);
}

#[tokio::test]
async fn lagged_client_is_disconnected() {
    let f = Fixture::new(SyncConfig {
        broadcast_capacity: 4,
        ..SyncConfig::default()
    })
    .await;
    let id = f.ids().await[0].clone();
    let writer = f.client().await;

    // Un client qui ne lit plus : on ne lit que le premier message du serveur
    // (son étape 1, signe que la connexion est inscrite), puis plus rien.
    let (server_end, mut stalled) = memory_socket(0);
    let serving = tokio::spawn(
        f.hub
            .clone()
            .serve_connection(f.page, SCHEMA_VERSION, server_end),
    );
    let hello = tokio::time::timeout(WAIT, stalled.next()).await.unwrap();
    assert!(hello.is_some(), "le serveur envoie son étape 1");

    for _ in 0..50 {
        writer.type_text(&id, "x");
    }
    let served = tokio::time::timeout(WAIT, serving)
        .await
        .expect("le client en retard est déconnecté")
        .unwrap();
    assert!(served.is_ok(), "{served:?}");

    // Un client qui se reconnecte reçoit l'état complet.
    let full = "x".repeat(50);
    let again = f.client().await;
    assert!(
        again.wait_until(WAIT, |c| text(&c.blocks()) == full).await,
        "{:?}",
        text(&again.blocks())
    );
    assert!(!writer.is_closed());
}

/// Plus profond que tout ce qu'une pile de 2 Mio libère sans déborder.
const DEEP: usize = 4_000;

/// Mise à jour d'un client hostile : `DEEP` blocs imbriqués. Construite sur un
/// fil à grosse pile, car yrs récurse en libérant un arbre profond (c'est le
/// client qui en paie le prix, pas le serveur).
fn deep_update() -> Vec<u8> {
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(|| {
            let doc = Doc::with_options(Options {
                offset_kind: OffsetKind::Utf16,
                ..Options::default()
            });
            let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
            let mut txn = doc.transact_mut();
            let mut group = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
            for i in 0..DEEP {
                let container =
                    group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
                container.insert_attribute(&mut txn, "id", format!("deep{i}"));
                let paragraph = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
                paragraph.push_back(&mut txn, XmlTextPrelim::new("x"));
                group = container.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
            }
            txn.encode_update_v1()
        })
        .unwrap()
        .join()
        .unwrap()
}

// Runtime tokio par défaut, pile par défaut : le serveur ne doit pas déborder.
#[tokio::test]
async fn deep_update_is_rejected() {
    let update = deep_update();
    let f = Fixture::new(SyncConfig::default()).await;
    let id = f.ids().await[0].clone();
    let a = f.client().await;
    let b = f.client().await;
    let hostile = f.client().await;
    let before = f.store.load_doc(f.page).await.unwrap();

    hostile.send_update(&update);
    assert!(
        hostile.wait_until(WAIT, |c| c.is_closed()).await,
        "le client fautif est déconnecté"
    );
    assert_eq!(
        f.store.load_doc(f.page).await.unwrap(),
        before,
        "sa mise à jour n'est pas écrite"
    );

    // Les autres continuent d'écrire et de recevoir.
    a.type_text(&id, "a");
    assert!(b.wait_until(WAIT, |c| text(&c.blocks()) == "a").await);
    b.type_text(&id, "b");
    assert!(a.wait_until(WAIT, |c| text(&c.blocks()) == "ab").await);
    assert!(!a.is_closed() && !b.is_closed());
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "ab");
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "ab"
    );
}

#[tokio::test]
async fn hostile_messages_close_only_their_connection() {
    let f = Fixture::new(SyncConfig::default()).await;
    let id = f.ids().await[0].clone();
    let a = f.client().await;
    let b = f.client().await;
    let before = f.store.load_doc(f.page).await.unwrap();

    let huge_client = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F];
    let mut sv_huge_client = vec![0, 0, 11, 1];
    sv_huge_client.extend(huge_client);
    sv_huge_client.push(1);
    let mut awareness_huge_client = vec![1, 14, 1];
    awareness_huge_client.extend(huge_client);
    awareness_huge_client.extend([1, 2, b'{', b'}']);
    let hostile: Vec<(&str, Vec<u8>)> = vec![
        (
            "étape 1 : 4 milliards de clients annoncés",
            vec![0, 0, 5, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F],
        ),
        ("étape 1 : client hors de 53 bits", sv_huge_client),
        ("sous-type de synchro inconnu", vec![0, 9, 0]),
        ("longueur au-delà du message", vec![0, 2, 50, 1, 2]),
        ("awareness : client hors de 53 bits", awareness_huge_client),
        ("awareness illisible", vec![1, 3, 0xFF, 0xFF, 0x0F]),
        ("mise à jour illisible", vec![0, 2, 3, 1, 2, 3]),
    ];
    for (what, message) in hostile {
        let client = f.client().await;
        client.send_raw(message);
        assert!(
            client.wait_until(WAIT, |c| c.is_closed()).await,
            "{what} : connexion fermée"
        );
    }
    assert_eq!(f.store.load_doc(f.page).await.unwrap(), before);

    a.type_text(&id, "ok");
    assert!(b.wait_until(WAIT, |c| text(&c.blocks()) == "ok").await);
    assert!(!a.is_closed() && !b.is_closed());
}

#[tokio::test]
async fn awareness_is_relayed_and_kept_for_newcomers() {
    let f = Fixture::new(SyncConfig::default()).await;
    let a = f.client().await;
    let b = f.client().await;
    let a_id = a.client_id();
    let state = r#"{"user":{"name":"A"}}"#;

    a.set_awareness(state);
    assert!(
        b.wait_until(WAIT, |c| c
            .awareness_states()
            .get(&a_id)
            .map(String::as_str)
            == Some(state))
            .await,
        "relayée aux autres"
    );
    let newcomer = f.client().await;
    assert!(
        newcomer
            .wait_until(WAIT, |c| c.awareness_states().contains_key(&a_id))
            .await,
        "gardée pour les nouveaux venus"
    );

    drop(a);
    assert!(
        b.wait_until(WAIT, |c| !c.awareness_states().contains_key(&a_id))
            .await,
        "retirée au départ du client"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn churning_actors_never_diverge() {
    // Un acteur s'arrête dès qu'il n'a ni connexion ni demande.
    let f = Fixture::new(SyncConfig {
        idle_close: Duration::ZERO,
        ..SyncConfig::default()
    })
    .await;

    // Lire ses écritures, alors que les acteurs s'arrêtent et repartent.
    let noise = {
        let hub = f.hub.clone();
        let page = f.page;
        tokio::spawn(async move {
            for _ in 0..200 {
                hub.read_blocks(page).await.unwrap();
            }
        })
    };
    for i in 0..30 {
        let mmd = format!("écriture {i}\n");
        f.write_mmd(&mmd).await;
        let read = f.hub.read_blocks(f.page).await.unwrap();
        assert_eq!(text(&read), format!("écriture {i}"));
    }
    noise.await.unwrap();

    // Écritures concurrentes.
    let mut tasks = JoinSet::new();
    for t in 0..4 {
        let hub = f.hub.clone();
        let page = f.page;
        tasks.spawn(async move {
            for i in 0..15 {
                let blocks = parse(&format!("t{t} {i}\n")).unwrap();
                hub.write_blocks(page, blocks).await.unwrap();
            }
        });
    }
    while let Some(done) = tasks.join_next().await {
        done.unwrap();
    }

    // Clients qui vont et viennent : chacun attend que le serveur ait traité
    // sa frappe (réponse à une étape 1 de plus) avant de partir.
    let id = f.ids().await[0].clone();
    for _ in 0..2 {
        let url = f.url();
        let id = id.clone();
        tasks.spawn(async move {
            for _ in 0..5 {
                let client = Client::connect_synced(&url).await;
                client.type_text(&id, "+");
                client.resync();
                assert!(client.wait_until(WAIT, |c| c.step2_count() >= 2).await);
            }
        });
    }
    while let Some(done) = tasks.join_next().await {
        done.unwrap();
    }

    let live = f.hub.read_blocks(f.page).await.unwrap();
    assert_eq!(live.len(), 1);
    assert!(text(&live).ends_with("++++++++++"), "{:?}", text(&live));
    assert_eq!(stored_page(&f.store, f.page).await.read_blocks(), live);
}
