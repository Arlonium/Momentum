//! Synchronisation entre clients : convergence, page inconnue, déplacements
//! concurrents, client en retard, mises à jour refusées, bornes mémoire,
//! awareness, acteurs qui s'arrêtent et repartent.

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
use yrs::sync::{Message, SyncMessage};
use yrs::updates::encoder::Encode;
use yrs::{Text, Transact, Xml, XmlElementPrelim, XmlFragment, XmlTextPrelim};

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
    // premier ou le second déplacement. Ce correctif n'est qu'une suppression,
    // et une suppression ne change pas le vecteur d'état : A a déjà celui de B
    // avec les deux déplacements, doublon compris. On attend donc le contenu.
    assert!(
        a.wait_until(WAIT, deduped).await,
        "A : {:?}",
        text(&a.blocks())
    );
    assert_eq!(a.state_vector(), b.state_vector());
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
            let doc = new_doc();
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

#[tokio::test]
async fn refused_update_is_never_served() {
    let f = Fixture::new(SyncConfig::default()).await;
    let writer = f.client().await;

    // u1 tape « a », u2 tape « b » après : u2 dépend de u1.
    let doc = detached(&writer);
    let paragraph = first_content(&doc);
    let (u1, typed) = {
        let mut txn = doc.transact_mut();
        let typed = paragraph.insert(&mut txn, 0, XmlTextPrelim::new("a"));
        (txn.encode_update_v1(), typed)
    };
    let u2 = {
        let mut txn = doc.transact_mut();
        typed.insert(&mut txn, 1, "b");
        txn.encode_update_v1()
    };

    // u2 seule : refusée après application (yrs l'a gardée en attente), le
    // client fautif est fermé et le document rechargé (Ruling R22).
    let hostile = f.client().await;
    hostile.send_update(&u2);
    assert!(hostile.wait_until(WAIT, |c| c.is_closed()).await);

    // Un nouveau venu ne reçoit pas u2, même en attente.
    let newcomer = f.client().await;
    assert!(
        newcomer.page().is_ok(),
        "rien en attente chez le nouveau venu"
    );
    assert!(!text(&newcomer.blocks()).contains('b'));

    // u1 arrive : u2, oubliée, ne s'intègre pas avec elle.
    writer.send_update(&u1);
    writer.resync();
    assert!(writer.wait_until(WAIT, |c| c.step2_count() >= 2).await);
    let live = f.hub.read_blocks(f.page).await.unwrap();
    assert_eq!(text(&live), "a");
    assert_eq!(stored_page(&f.store, f.page).await.read_blocks(), live);
    assert!(
        newcomer
            .wait_until(WAIT, |c| text(&c.blocks()) == "a")
            .await
    );
}

#[tokio::test]
async fn lagging_behind_the_broadcast_closes_the_connection() {
    let f = Fixture::new(SyncConfig {
        broadcast_capacity: 4,
        ..SyncConfig::default()
    })
    .await;
    let batcher = f.client().await;

    // Six frappes envoyées en un seul message WebSocket : l'acteur les écrit
    // et les diffuse pendant que la connexion attend sa réponse. Ses propres
    // échos débordent sa diffusion (capacité 4) : `RecvError::Lagged`.
    let doc = detached(&batcher);
    let paragraph = first_content(&doc);
    let (first, typed) = {
        let mut txn = doc.transact_mut();
        let typed = paragraph.insert(&mut txn, 0, XmlTextPrelim::new("1"));
        (txn.encode_update_v1(), typed)
    };
    let mut batch = Message::Sync(SyncMessage::Update(first)).encode_v1();
    for (i, c) in "23456".chars().enumerate() {
        let mut txn = doc.transact_mut();
        typed.insert(&mut txn, i as u32 + 1, &c.to_string());
        let update = txn.encode_update_v1();
        batch.extend(Message::Sync(SyncMessage::Update(update)).encode_v1());
    }
    batcher.send_raw(batch);
    assert!(
        batcher.wait_until(WAIT, |c| c.is_closed()).await,
        "la connexion en retard est fermée"
    );

    // Ce qu'elle a envoyé est écrit, et un nouveau venu le reçoit.
    let newcomer = f.client().await;
    assert_eq!(text(&newcomer.blocks()), "123456");
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "123456"
    );
}

#[tokio::test]
async fn oversized_message_closes_its_author() {
    let f = Fixture::new(SyncConfig {
        max_message_bytes: 4096,
        ..SyncConfig::default()
    })
    .await;
    let id = f.ids().await[0].clone();
    let a = f.client().await;
    let b = f.client().await;

    // Un message valide (4 097 demandes d'awareness), mais trop gros : la
    // taille est contrôlée avant tout décodage.
    let (server_end, mut author) = memory_socket(8);
    let serving = tokio::spawn(
        f.hub
            .clone()
            .serve_connection(f.page, SCHEMA_VERSION, server_end),
    );
    assert!(
        tokio::time::timeout(WAIT, author.next())
            .await
            .unwrap()
            .is_some()
    );
    futures::SinkExt::send(&mut author, vec![3; 4097])
        .await
        .unwrap();
    let served = tokio::time::timeout(WAIT, serving)
        .await
        .expect("l'auteur du message trop gros est fermé")
        .unwrap();
    assert!(served.is_ok(), "{served:?}");

    // Par le réseau aussi.
    let big = f.client().await;
    big.send_raw(vec![3; 5000]);
    assert!(big.wait_until(WAIT, |c| c.is_closed()).await);

    // Les autres continuent.
    a.type_text(&id, "ok");
    assert!(b.wait_until(WAIT, |c| text(&c.blocks()) == "ok").await);
    assert!(!a.is_closed() && !b.is_closed());
}

#[tokio::test]
async fn queue_over_max_bytes_closes_the_connection() {
    // Le nombre de messages en retard n'est pas en cause (1024) : les octets.
    let f = Fixture::new(SyncConfig {
        max_queue_bytes: 8 * 1024,
        ..SyncConfig::default()
    })
    .await;
    let id = f.ids().await[0].clone();
    let writer = f.client().await;

    let (server_end, mut stalled) = memory_socket(0);
    let serving = tokio::spawn(
        f.hub
            .clone()
            .serve_connection(f.page, SCHEMA_VERSION, server_end),
    );
    assert!(
        tokio::time::timeout(WAIT, stalled.next())
            .await
            .unwrap()
            .is_some()
    );

    let chunk = "x".repeat(1024);
    for _ in 0..20 {
        writer.type_text(&id, &chunk);
    }
    let served = tokio::time::timeout(WAIT, serving)
        .await
        .expect("le client dont la file dépasse max_queue_bytes est fermé")
        .unwrap();
    assert!(served.is_ok(), "{served:?}");
    assert!(!writer.is_closed());
    // Tout est écrit. (Un nouveau venu ne tiendrait pas dans une file de
    // 8 Kio : l'état complet de la page en fait 20.)
    writer.resync();
    assert!(writer.wait_until(WAIT, |c| c.step2_count() >= 2).await);
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        chunk.repeat(20)
    );
}

#[tokio::test]
async fn oversized_awareness_is_ignored() {
    let f = Fixture::new(SyncConfig::default()).await;
    let a = f.client().await;
    let b = f.client().await;
    let w = f.client().await;
    let a_id = a.client_id();

    // Un état de plus de 64 Kio : ni gardé, ni relayé, sans fermer la connexion.
    a.set_awareness(&format!(r#"{{"pad":"{}"}}"#, "x".repeat(70 * 1024)));
    a.resync();
    assert!(a.wait_until(WAIT, |c| c.step2_count() >= 2).await);
    assert!(!a.is_closed());

    // Les diffusions arrivent dans l'ordre : l'état de W arrive après celui
    // de A, s'il avait été relayé.
    w.set_awareness(r#"{"user":"W"}"#);
    let w_id = w.client_id();
    assert!(
        b.wait_until(WAIT, |c| c.awareness_states().contains_key(&w_id))
            .await
    );
    assert!(!b.awareness_states().contains_key(&a_id), "non relayé");
    let newcomer = f.client().await;
    assert!(
        newcomer
            .wait_until(WAIT, |c| c.awareness_states().contains_key(&w_id))
            .await
    );
    assert!(
        !newcomer.awareness_states().contains_key(&a_id),
        "non gardé"
    );
}

/// Genre d'un message y-websocket du serveur (un seul par message WebSocket).
fn kind(message: &[u8]) -> &'static str {
    use yrs::sync::MessageReader;
    use yrs::updates::decoder::DecoderV1;
    let mut decoder = DecoderV1::from(message);
    let read: Vec<_> = MessageReader::new(&mut decoder)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        read.len(),
        1,
        "un message y-websocket par message WebSocket"
    );
    match &read[0] {
        Message::Sync(SyncMessage::SyncStep1(_)) => "étape 1",
        Message::Sync(SyncMessage::SyncStep2(_)) => "étape 2",
        Message::Sync(SyncMessage::Update(_)) => "mise à jour",
        Message::Awareness(_) => "awareness",
        _ => "autre",
    }
}

/// Une connexion en mémoire sur la page, avec un état d'awareness connu :
/// son accueil (étape 1, awareness) est lu, plus rien ne doit arriver.
async fn quiet_connection(
    f: &Fixture,
    what: &str,
) -> (
    Client,
    tokio::task::JoinHandle<Result<(), SyncError>>,
    MemorySocket,
) {
    let other = f.client().await;
    other.set_awareness(r#"{"user":"autre"}"#);
    other.resync();
    assert!(other.wait_until(WAIT, |c| c.step2_count() >= 2).await);
    let (server_end, mut client_end) = memory_socket(64);
    let serving = tokio::spawn(
        f.hub
            .clone()
            .serve_connection(f.page, SCHEMA_VERSION, server_end),
    );
    for expected in ["étape 1", "awareness"] {
        let hello = tokio::time::timeout(WAIT, client_end.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(kind(&hello), expected, "accueil ({what})");
    }
    (other, serving, client_end)
}

async fn next_kind(socket: &mut MemorySocket) -> &'static str {
    let message = tokio::time::timeout(WAIT, socket.next())
        .await
        .expect("une réponse")
        .expect("connexion ouverte");
    kind(&message)
}

#[tokio::test]
async fn many_sync_steps_in_one_message_get_one_answer() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (_other, _serving, mut socket) = quiet_connection(&f, "étapes 1").await;

    // 10 000 étapes 1 (vecteur d'état vide) dans un seul message, puis une
    // demande d'awareness seule, qui sert de barrière.
    let step1 = [0u8, 0, 1, 0];
    let batch: Vec<u8> = step1.iter().copied().cycle().take(4 * 10_000).collect();
    futures::SinkExt::send(&mut socket, batch).await.unwrap();
    futures::SinkExt::send(&mut socket, vec![3]).await.unwrap();

    // Une seule étape 2, puis la réponse à la barrière : rien d'autre.
    assert_eq!(next_kind(&mut socket).await, "étape 2");
    assert_eq!(next_kind(&mut socket).await, "awareness");
}

#[tokio::test]
async fn many_awareness_queries_in_one_message_get_one_answer() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (_other, _serving, mut socket) = quiet_connection(&f, "awareness").await;

    // 10 000 demandes d'awareness dans un seul message, puis une étape 1
    // seule, qui sert de barrière.
    futures::SinkExt::send(&mut socket, vec![3; 10_000])
        .await
        .unwrap();
    futures::SinkExt::send(&mut socket, vec![0, 0, 1, 0])
        .await
        .unwrap();

    assert_eq!(next_kind(&mut socket).await, "awareness");
    assert_eq!(next_kind(&mut socket).await, "étape 2");
}

#[tokio::test]
async fn replies_beyond_max_queue_bytes_close_the_connection() {
    // L'état de la page dépasse à lui seul la borne : l'étape 2 passe seule,
    // la réponse d'awareness qui la suit dans le même lot ferme la connexion.
    let f = Fixture::new(SyncConfig {
        max_queue_bytes: 256,
        ..SyncConfig::default()
    })
    .await;
    f.write_mmd(&format!("{}\n", "x".repeat(1024))).await;
    let (_other, serving, mut socket) = quiet_connection(&f, "borne").await;

    let mut batch = vec![0u8, 0, 1, 0];
    batch.push(3);
    futures::SinkExt::send(&mut socket, batch).await.unwrap();
    let served = tokio::time::timeout(WAIT, serving)
        .await
        .expect("la connexion est fermée")
        .unwrap();
    assert!(served.is_ok(), "{served:?}");
}
