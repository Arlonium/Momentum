//! Message d'entretien (Ruling R107) : une connexion à qui le serveur n'a rien envoyé depuis
//! `keepalive_after` reçoit une mise à jour de présence vide.
//!
//! y-websocket ferme sa WebSocket quand il n'a reçu aucun message depuis 30 s, puis se
//! reconnecte ; le serveur ne renvoie jamais à une connexion ses propres messages. Sans ce
//! message, un onglet seul sur sa page se reconnectait toutes les 30 s.
//!
//! Les connexions de ces tests sont des sockets en mémoire, lues message par message : le
//! client de test, lui, applique ce qu'il reçoit sans le montrer.

mod common;

use std::time::{Duration, Instant};

use common::*;
use futures::{SinkExt, StreamExt};
use mementum_schema::SCHEMA_VERSION;
use mementum_sync::{ConnInfo, KEEPALIVE_AFTER, SyncConfig, SyncError};
use tokio::task::JoinHandle;
use yrs::sync::{Message, MessageReader, SyncMessage};
use yrs::updates::decoder::DecoderV1;

/// Le délai d'entretien de ces tests : assez court pour les répéter, assez long pour qu'une
/// demande faite à mi-délai y tombe bien au milieu.
const KEEPALIVE: Duration = Duration::from_millis(400);

/// Le message d'entretien, tel qu'il passe sur la socket : `1` (awareness), puis un tampon
/// d'un octet, `0` (aucune entrée).
const EMPTY_PRESENCE: [u8; 3] = [1, 1, 0];

/// Une étape 1 au vecteur d'état vide : le serveur y répond par une étape 2.
const STEP_1: [u8; 4] = [0, 0, 1, 0];

fn config() -> SyncConfig {
    SyncConfig {
        keepalive_after: KEEPALIVE,
        ..SyncConfig::default()
    }
}

/// Le message y-websocket que porte un message WebSocket du serveur : un seul.
fn decoded(message: &[u8]) -> Message {
    let mut decoder = DecoderV1::from(message);
    let mut read: Vec<Message> = MessageReader::new(&mut decoder)
        .collect::<Result<_, _>>()
        .expect("un message y-websocket lisible");
    assert_eq!(
        read.len(),
        1,
        "un message y-websocket par message WebSocket"
    );
    read.remove(0)
}

/// Une mise à jour de présence de zéro entrée ?
fn is_empty_presence(message: &Message) -> bool {
    matches!(message, Message::Awareness(update) if update.clients.is_empty())
}

/// Une connexion `conn` sur la page, par une socket en mémoire, son accueil lu : l'étape 1 du
/// serveur, et rien d'autre (personne n'a annoncé de présence).
async fn connected(
    f: &Fixture,
    conn: ConnInfo,
) -> (JoinHandle<Result<(), SyncError>>, MemorySocket) {
    let (server_end, mut socket) = memory_socket(64);
    let serving = tokio::spawn(f.hub.clone().serve_connection(f.page, conn, server_end));
    let hello = next(&mut socket).await;
    let step_1 = matches!(decoded(&hello), Message::Sync(SyncMessage::SyncStep1(_)));
    assert!(step_1, "accueil : {hello:?}");
    (serving, socket)
}

/// Le prochain message du serveur ([`WAIT`] au plus, bien au-delà du délai d'entretien).
async fn next(socket: &mut MemorySocket) -> Vec<u8> {
    tokio::time::timeout(WAIT, socket.next())
        .await
        .expect("aucun message du serveur")
        .expect("connexion fermée par le serveur")
}

/// Le réglage par défaut : 20 s, sous les 30 s au bout desquelles y-websocket se reconnecte.
#[test]
fn default_keepalive_is_under_the_client_timeout() {
    assert_eq!(KEEPALIVE_AFTER, Duration::from_secs(20));
    assert_eq!(SyncConfig::default().keepalive_after, KEEPALIVE_AFTER);
}

/// Une connexion seule sur sa page, qui n'envoie rien : le serveur n'a rien à lui dire, et lui
/// envoie donc, le délai passé, une mise à jour de présence de zéro entrée ; puis une autre,
/// tant qu'elle se tait. Rien n'en est écrit, et la connexion reste servie.
#[tokio::test]
async fn idle_connection_receives_an_empty_presence_message() {
    let f = Fixture::new(config()).await;
    let rows = row_counts(&f.db).await;
    let (serving, mut socket) = connected(&f, writing_conn()).await;

    for round in 1..=3 {
        let message = next(&mut socket).await;
        assert!(
            is_empty_presence(&decoded(&message)),
            "message {round} : {message:?}"
        );
        assert_eq!(message, EMPTY_PRESENCE, "message {round}");
    }
    assert!(!serving.is_finished(), "la connexion est toujours servie");
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
}

/// Le délai repart à chaque message envoyé à la connexion : après une réponse du serveur, le
/// message d'entretien ne vient pas avant que le délai entier se soit écoulé. Un minuteur qui
/// battrait depuis l'ouverture de la connexion, sans regarder ce qui lui est envoyé, le
/// ferait arriver à mi-délai.
#[tokio::test]
async fn keepalive_waits_for_silence() {
    let f = Fixture::new(config()).await;
    let (_serving, mut socket) = connected(&f, writing_conn()).await;

    for round in 1..=2 {
        // À mi-délai du dernier message reçu (l'accueil, puis un message d'entretien), la
        // connexion demande une étape 2. Cette attente ne décide de rien : trop longue sur
        // un poste chargé, elle laisse seulement passer un message d'entretien de plus avant
        // la réponse, et la borne qui suit tient quand même.
        tokio::time::sleep(KEEPALIVE / 2).await;
        let asked = Instant::now();
        socket.send(STEP_1.to_vec()).await.unwrap();
        loop {
            let message = next(&mut socket).await;
            match decoded(&message) {
                Message::Sync(SyncMessage::SyncStep2(_)) => break,
                earlier if is_empty_presence(&earlier) => {}
                other => panic!("tour {round} : message inattendu avant la réponse : {other:?}"),
            }
        }
        // Le serveur vient d'envoyer sa réponse, après `asked`. Le message suivant est celui
        // d'entretien, et il ne peut pas venir plus tôt que le délai compté depuis `asked`.
        let message = next(&mut socket).await;
        let waited = asked.elapsed();
        assert!(
            is_empty_presence(&decoded(&message)),
            "tour {round} : {message:?}"
        );
        assert!(
            waited >= KEEPALIVE,
            "tour {round} : message d'entretien {waited:?} après un message envoyé à la \
             connexion, avant le délai de {KEEPALIVE:?}"
        );
    }
}

/// Une connexion en lecture seule (sans le droit d'écrire, ou d'un autre schéma) est
/// entretenue comme les autres : son onglet se reconnecterait tout autant.
#[tokio::test]
async fn read_only_connection_is_kept_alive_too() {
    let f = Fixture::new(config()).await;
    let read_only = [
        (
            "sans le droit d'écrire",
            ConnInfo {
                can_write: false,
                ..writing_conn()
            },
        ),
        (
            "d'un autre schéma",
            ConnInfo {
                client_schema: SCHEMA_VERSION + 1,
                ..writing_conn()
            },
        ),
    ];
    for (what, conn) in read_only {
        let (serving, mut socket) = connected(&f, conn).await;
        let message = next(&mut socket).await;
        assert_eq!(message, EMPTY_PRESENCE, "{what}");
        assert!(is_empty_presence(&decoded(&message)), "{what}");
        assert!(!serving.is_finished(), "{what}");
    }
}

/// Un client Yjs (le client de test applique ce qu'il reçoit comme un navigateur) prend le
/// message d'entretien pour ce qu'il est : rien. Ni fermeture, ni état de présence, et la
/// synchro continue.
#[tokio::test]
async fn yjs_client_ignores_the_empty_presence_message() {
    let f = Fixture::new(config()).await;
    f.write_mmd("Bonjour\n").await;
    let id = f.ids().await[0].clone();
    let writer = f.client().await;
    let reader = f.client().await;
    // Personne ne fait rien : ce que le lecteur reçoit encore, ce sont des messages
    // d'entretien. La condition est réévaluée à chaque message reçu : trois de plus.
    let mut evaluated = 0;
    let received = reader.wait_until(WAIT, |_| {
        evaluated += 1;
        evaluated > 3
    });
    assert!(received.await, "aucun message d'entretien reçu");
    assert!(!reader.is_closed(), "fermé par un message d'entretien");
    assert!(reader.awareness_states().is_empty());

    writer.type_text(&id, " !");
    assert!(
        reader
            .wait_until(WAIT, |c| text(&c.blocks()) == "Bonjour !")
            .await
    );
    assert!(!writer.is_closed() && !reader.is_closed());
}
