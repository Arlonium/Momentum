//! Synchronisation entre clients : convergence, page inconnue, déplacements
//! concurrents, client en retard, mises à jour refusées, bornes mémoire,
//! awareness, acteurs qui s'arrêtent et repartent ; et les droits : connexion
//! sans le droit d'écrire, connexions fermées à la demande (`kick`), pages
//! oubliées (`forget`), écritures attribuées et atomiques.

mod common;

use std::time::{Duration, Instant};

use common::*;
use futures::StreamExt;
use mementum_crdt::{FRAGMENT, META};
use mementum_md::parse;
use mementum_schema::SCHEMA_VERSION;
use mementum_sync::{ConnInfo, Hub, SyncConfig, SyncError};
use tokio::task::JoinSet;
use uuid::Uuid;
use yrs::sync::{Message, SyncMessage};
use yrs::updates::encoder::Encode;
use yrs::{Map, Text, Transact, Xml, XmlElementPrelim, XmlFragment, XmlTextPrelim};

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
        .serve_connection(unknown, writing_conn(), server_end)
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
        f.hub
            .write_blocks(unknown, parse("x\n").unwrap(), None)
            .await,
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
    // premier ou le second déplacement. Deux conditions, attendues ensemble :
    // - le vecteur d'état de B, que A n'atteint qu'en recevant le déplacement
    //   de B. Le contenu seul ne suffit pas : « B C A D » est déjà ce que vaut
    //   le document de A après son propre déplacement, avant qu'il ait rien
    //   reçu, et l'attente rendrait vrai tout de suite ;
    // - le contenu. Le vecteur d'état seul ne suffit pas non plus : le
    //   correctif n'est qu'une suppression, qui ne le change pas, et A a celui
    //   de B dès les deux déplacements, doublon compris.
    let b_state = b.state_vector();
    assert!(
        a.wait_until(WAIT, |c| c.state_vector() == b_state && deduped(c))
            .await,
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
            .serve_connection(f.page, writing_conn(), server_end),
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
                hub.write_blocks(page, blocks, None).await.unwrap();
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
            .serve_connection(f.page, writing_conn(), server_end),
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
            .serve_connection(f.page, writing_conn(), server_end),
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
            .serve_connection(f.page, writing_conn(), server_end),
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

// ---------------------------------------------------------------------------
// Droits : lecture seule, fermeture à la demande, écritures attribuées
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cannot_write_connection_is_ignored() {
    // Le schéma courant, mais sans le droit d'écrire.
    assert_read_only(ConnInfo {
        can_write: false,
        ..writing_conn()
    })
    .await;
}

#[tokio::test]
async fn kick_closes_every_connection_of_the_page() {
    let f = Fixture::new(SyncConfig::default()).await;
    let elsewhere = new_page(&f.store).await;
    let id = f.ids().await[0].clone();
    let a = f.client().await;
    let b = f.client().await;
    let c = Client::connect_synced(&f.server.url(elsewhere, SCHEMA_VERSION)).await;
    a.type_text(&id, "avant");
    assert!(b.wait_until(WAIT, |c| text(&c.blocks()) == "avant").await);
    let rows = row_counts(&f.db).await;
    let mut pages = vec![f.page, elsewhere];
    pages.sort();
    assert_eq!(live(&f.hub).await, pages);

    f.hub.kick(&[f.page]).await;
    // Au retour, l'acteur ne traite plus rien de ces connexions : ce que A
    // tape avant d'avoir vu la fermeture n'arrive pas au serveur.
    a.type_text(&id, " après");
    assert!(a.wait_until(WAIT, |c| c.is_closed()).await, "A est fermé");
    assert!(b.wait_until(WAIT, |c| c.is_closed()).await, "B aussi");
    assert_eq!(text(&b.blocks()), "avant");
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");

    // La connexion d'une autre page n'est pas touchée.
    let there = c.blocks()[0].id.clone();
    c.type_text(&there, "ailleurs");
    processed(&c).await;
    assert!(!c.is_closed());
    assert_eq!(
        text(&f.hub.read_blocks(elsewhere).await.unwrap()),
        "ailleurs"
    );

    // L'acteur est resté, avec son document : une reconnexion retrouve tout
    // le contenu.
    assert_eq!(live(&f.hub).await, pages);
    let again = f.client().await;
    assert_eq!(text(&again.blocks()), "avant");
    // Et A, revenu avec son document comme le fait y-websocket, apporte ce
    // qu'il avait tapé pendant la fermeture.
    again.apply_local(&a.page().unwrap().encode_state());
    processed(&again).await;
    assert_eq!(
        text(&f.hub.read_blocks(f.page).await.unwrap()),
        "avant après"
    );
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "avant après"
    );
}

#[tokio::test]
async fn kick_on_a_page_without_actor_is_a_no_op() {
    let f = Fixture::new(SyncConfig::default()).await;
    let never_opened = new_page(&f.store).await;
    let rows = row_counts(&f.db).await;

    // Une page que personne n'a ouverte, une page inconnue, aucune page :
    // ni erreur, ni attente, ni acteur lancé (il écrirait le document de la
    // page, Ruling R28).
    let kicked = tokio::time::timeout(WAIT, async {
        f.hub.kick(&[never_opened, Uuid::now_v7()]).await;
        f.hub.kick(&[]).await;
    })
    .await;
    assert!(kicked.is_ok(), "kick rend la main");
    assert_eq!(live(&f.hub).await, [f.page], "aucun acteur lancé");
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");

    // Un acteur sans connexion : rien à fermer, il reste là avec son document.
    f.write_mmd("Bonjour\n").await;
    f.hub.kick(&[f.page]).await;
    assert_eq!(live(&f.hub).await, [f.page]);
    assert_eq!(text(&f.client().await.blocks()), "Bonjour");
}

#[tokio::test]
async fn kick_does_not_wait_for_a_busy_actor() {
    use sqlx::Connection;
    let f = Fixture::new(SyncConfig::default()).await;
    let a = f.client().await;

    // L'acteur est pris : une connexion annexe tient le verrou d'écriture de
    // SQLite, et l'écriture qu'on lui demande attend ce verrou (5 s au plus).
    let mut lock = sqlx::SqliteConnection::connect(&format!("sqlite:{}", f.db.display()))
        .await
        .unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut lock)
        .await
        .unwrap();
    let mut writing = Box::pin(f.hub.write_blocks(f.page, parse("Écrit\n").unwrap(), None));
    // Une fois tentée, la demande d'écriture est dans la boîte de l'acteur,
    // devant celle de `kick`.
    assert!(futures::poll!(&mut writing).is_pending());

    f.hub.kick(&[f.page]).await;
    assert!(
        futures::poll!(&mut writing).is_pending(),
        "kick a rendu la main sans attendre l'acteur"
    );
    assert!(!a.is_closed());

    // Le verrou rendu, l'acteur finit son écriture, puis ferme les
    // connexions : la demande était restée dans sa boîte.
    sqlx::query("ROLLBACK").execute(&mut lock).await.unwrap();
    writing.await.unwrap();
    assert!(a.wait_until(WAIT, |c| c.is_closed()).await, "A est fermé");
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "Écrit"
    );
}

#[tokio::test]
async fn forget_stops_the_actor_without_writing() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Avant\n").await;
    let id = f.ids().await[0].clone();
    let a = f.client().await;
    let b = f.client().await;
    a.type_text(&id, " suite");
    assert!(
        b.wait_until(WAIT, |c| text(&c.blocks()) == "Avant suite")
            .await
    );
    // Un journal de deux mises à jour, que rien n'a compacté : un instantané
    // pris à l'arrêt se verrait.
    let before = f.store.load_doc(f.page).await.unwrap();
    assert_eq!(before.updates.len(), 2);
    let rows = row_counts(&f.db).await;

    // Avec une page inconnue, ignorée.
    f.hub.forget(&[f.page, Uuid::now_v7()]).await;
    assert!(live(&f.hub).await.is_empty(), "au retour, plus d'acteur");
    a.type_text(&id, " perdu");
    assert!(a.wait_until(WAIT, |c| c.is_closed()).await, "A est fermé");
    assert!(b.wait_until(WAIT, |c| c.is_closed()).await, "B aussi");
    assert_eq!(
        f.store.load_doc(f.page).await.unwrap(),
        before,
        "ni instantané ni mise à jour"
    );
    assert_eq!(row_counts(&f.db).await, rows);
    assert!(live(&f.hub).await.is_empty(), "personne ne l'a relancé");

    // Rien n'était perdu : la page, toujours en base, se rouvre entière.
    let c = f.client().await;
    assert_eq!(text(&c.blocks()), "Avant suite");

    // Comme la suppression définitive : la page quitte la base, puis le hub
    // l'oublie. Rien n'est écrit pour elle, et qui revient ne la trouve plus.
    let user = new_user(&f.db).await;
    f.store.trash_node(f.page, user).await.unwrap();
    f.store.delete_node(f.page).await.unwrap();
    let rows = row_counts(&f.db).await;
    f.hub.forget(&[f.page]).await;
    assert!(c.wait_until(WAIT, |c| c.is_closed()).await);
    assert!(live(&f.hub).await.is_empty());
    assert!(matches!(
        f.hub.read_blocks(f.page).await,
        Err(SyncError::NotFound)
    ));
    let back = Client::connect(&f.url()).await.unwrap();
    assert!(back.wait_until(WAIT, |c| c.is_closed()).await);
    assert!(!back.is_synced());
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
}

#[tokio::test]
async fn live_pages_lists_pages_with_an_actor() {
    let f = Fixture::new(SyncConfig::default()).await;
    let second = new_page(&f.store).await;
    let third = new_page(&f.store).await;
    // L'acteur de la page de la fixture vit depuis `create_page_doc`.
    assert_eq!(live(&f.hub).await, [f.page]);

    // Une connexion sur une autre page : un acteur de plus.
    let _client = Client::connect_synced(&f.server.url(second, SCHEMA_VERSION)).await;
    let mut both = vec![f.page, second];
    both.sort();
    assert_eq!(live(&f.hub).await, both);

    // Une page inconnue n'y entre pas, une page oubliée en sort.
    assert!(matches!(
        f.hub.read_blocks(Uuid::now_v7()).await,
        Err(SyncError::NotFound)
    ));
    assert_eq!(live(&f.hub).await, both);
    f.hub.forget(&[f.page]).await;
    assert_eq!(live(&f.hub).await, [second]);

    // Un acteur qui s'arrête de lui-même (ni connexion ni demande pendant
    // `idle_close`) en sort aussi.
    let idle = Hub::new(
        f.store.clone(),
        SyncConfig {
            idle_close: Duration::ZERO,
            ..SyncConfig::default()
        },
    );
    idle.read_blocks(third).await.unwrap();
    assert!(live_becomes(&idle, &[]).await, "l'acteur s'est arrêté");
}

#[tokio::test]
async fn client_update_and_dedupe_fix_are_one_write() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("A\n\nB\n\nC\n\nD\n").await;
    let ids = f.ids().await;
    let a = f.client().await;
    let b = f.client().await;
    let observer = f.client().await;

    // Le même bloc A, déplacé chez deux clients depuis le même état : après
    // C chez A, après D chez B.
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
    // Celui de A d'abord : seul, il ne duplique rien, et fait une ligne.
    let moved = |c: &Client| text(&c.blocks()) == "B\nC\nA\nD";
    a.apply_local(&from_a);
    processed(&a).await;
    assert!(b.wait_until(WAIT, moved).await);
    assert!(observer.wait_until(WAIT, moved).await);
    let before = f.store.load_doc(f.page).await.unwrap();
    let last = before.updates.last().unwrap().0;
    let seen = observer.state_vector();

    // Celui de B duplique le bloc : sa mise à jour et le correctif de
    // dédoublonnage du serveur font deux lignes. La panne tombe sur la seconde.
    exec_sql(
        &f.db,
        &format!(
            "CREATE TRIGGER panne BEFORE INSERT ON doc_updates WHEN NEW.seq = {} \
             BEGIN SELECT RAISE(ABORT, 'panne simulée'); END",
            last + 2
        ),
    )
    .await;
    b.apply_local(&from_b);
    for client in [&a, &b, &observer] {
        assert!(
            client.wait_until(WAIT, |c| c.is_closed()).await,
            "panne du store : tout le monde est fermé (Ruling R21)"
        );
    }

    // Aucune des deux lignes n'est en base, et rien n'a été diffusé (les
    // diffusions précèdent la fermeture, dans l'ordre).
    assert_eq!(
        f.store.load_doc(f.page).await.unwrap(),
        before,
        "ni la mise à jour ni son correctif"
    );
    assert_eq!(observer.state_vector(), seen, "rien de diffusé");
    let live = f.hub.read_blocks(f.page).await.unwrap();
    assert_eq!(
        text(&live),
        "B\nC\nA\nD",
        "document rechargé depuis le store"
    );
    assert_eq!(live.iter().filter(|x| x.id == ids[0]).count(), 1);

    // La panne passée, B revient avec son déplacement : les deux lignes
    // s'écrivent, et le doublon ne reste pas.
    exec_sql(&f.db, "DROP TRIGGER panne").await;
    let back = f.client().await;
    back.apply_local(&from_b);
    processed(&back).await;
    let after = f.store.load_doc(f.page).await.unwrap();
    assert_eq!(after.updates.len(), before.updates.len() + 2);
    let live = f.hub.read_blocks(f.page).await.unwrap();
    assert_eq!(text(&live), "B\nC\nA\nD");
    assert_eq!(live.iter().filter(|x| x.id == ids[0]).count(), 1);
    assert_eq!(stored_page(&f.store, f.page).await.read_blocks(), live);
}

#[tokio::test]
async fn updates_carry_the_actor() {
    let f = Fixture::new(SyncConfig::default()).await;
    let alice = new_user(&f.db).await;
    let bob = new_user(&f.db).await;
    // Une écriture du serveur sans compte (1).
    f.write_mmd("A\n\nB\n\nC\n\nD\n").await;
    let ids = f.ids().await;
    let with_account = Client::connect_synced(&f.url_for(ConnInfo {
        actor: Some(bob),
        ..writing_conn()
    }))
    .await;
    let without = f.client().await;

    // Une frappe de chaque connexion : la mise à jour porte son compte (2),
    // ou aucun (3).
    with_account.type_text(&ids[1], "!");
    processed(&with_account).await;
    without.type_text(&ids[2], "?");
    processed(&without).await;

    // Deux déplacements concurrents du même bloc : le premier, sans compte
    // (4) ; le second (5) et le correctif de dédoublonnage qui le suit (6)
    // portent le compte de la connexion dont la mise à jour l'a provoqué.
    let first = without
        .page()
        .unwrap()
        .move_block(&ids[0], Some(&ids[2]))
        .unwrap();
    let second = with_account
        .page()
        .unwrap()
        .move_block(&ids[0], Some(&ids[3]))
        .unwrap();
    without.apply_local(&first);
    processed(&without).await;
    with_account.apply_local(&second);
    processed(&with_account).await;

    // Une écriture du serveur au nom d'un compte (7).
    f.hub
        .write_blocks(f.page, parse("Fin\n").unwrap(), Some(alice))
        .await
        .unwrap();

    assert_eq!(
        actors(&f.db, f.page).await,
        [
            None,
            Some(bob),
            None,
            None,
            Some(bob),
            Some(bob),
            Some(alice)
        ]
    );
}

/// Report M7 du lot 1, vu de la synchro : l'acteur applique ce qui vient
/// d'un client par `apply_client_update`, et recharge le reste comme avant.
#[tokio::test]
async fn client_cannot_write_page_metadata() {
    let f = Fixture::new(SyncConfig::default()).await;
    let id = f.ids().await[0].clone();
    let a = f.client().await;
    let b = f.client().await;
    let hostile = f.client().await;
    let before = f.store.load_doc(f.page).await.unwrap();

    // `schemaVersion` écrasée, chez un client qui a reçu la page.
    let doc = detached(&hostile);
    let meta = doc.get_or_insert_map(META);
    let update = {
        let mut txn = doc.transact_mut();
        meta.insert(&mut txn, "schemaVersion", 999);
        txn.encode_update_v1()
    };
    hostile.send_update(&update);
    assert!(
        hostile.wait_until(WAIT, |c| c.is_closed()).await,
        "le client fautif est fermé"
    );
    assert_eq!(
        f.store.load_doc(f.page).await.unwrap(),
        before,
        "sa mise à jour n'est pas écrite"
    );

    // Ni diffusée ni gardée : les autres continuent, et tous lisent la
    // version posée par le serveur. La page, elle, se recharge du store
    // (c'est ce que l'acteur vient de faire).
    a.type_text(&id, "ok");
    assert!(b.wait_until(WAIT, |c| text(&c.blocks()) == "ok").await);
    assert!(!a.is_closed() && !b.is_closed());
    let newcomer = f.client().await;
    assert_eq!(text(&newcomer.blocks()), "ok");
    for page in [
        a.page().unwrap(),
        b.page().unwrap(),
        newcomer.page().unwrap(),
        stored_page(&f.store, f.page).await,
    ] {
        assert_eq!(page.schema_version(), Some(SCHEMA_VERSION));
    }
}

#[tokio::test]
async fn connect_with_headers_sends_them() {
    use std::io::{Read, Write};
    // Un serveur qui lit la demande de poignée de main, puis la refuse.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}/sync/page?schema=1", listener.local_addr().unwrap());
    let handshake = tokio::task::spawn_blocking(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut chunk = [0u8; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut chunk).unwrap();
            assert!(n > 0, "demande coupée");
            request.extend_from_slice(&chunk[..n]);
        }
        socket
            .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
            .unwrap();
        String::from_utf8(request).unwrap().to_ascii_lowercase()
    });

    let refused = Client::connect_with_headers(
        &url,
        &[
            ("Cookie", "mementum_session=jeton-de-test"),
            ("X-Autre", "valeur"),
        ],
    )
    .await;
    assert!(refused.is_err(), "une réponse 404 est une erreur");
    let request = handshake.await.unwrap();
    for header in ["cookie: mementum_session=jeton-de-test", "x-autre: valeur"] {
        assert!(
            request.contains(&format!("\r\n{header}\r\n")),
            "« {header} » absent de {request:?}"
        );
    }

    // Une en-tête impossible est une erreur, sans connexion.
    assert!(
        Client::connect_with_headers(&url, &[("en tête", "x")])
            .await
            .is_err()
    );
}
