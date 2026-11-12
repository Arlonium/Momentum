//! Mesuré : un message WebSocket d'un mégaoctet ne fait pas allouer au serveur plus que
//! quelques fois sa taille, quoi qu'il porte.
//!
//! - Ruling R124. Avant la borne de 32 messages y-websocket par message WebSocket, un message
//!   fait d'octets `03` (une demande d'awareness chacun) était découpé en autant de messages :
//!   72 Mio alloués pour 1 Mio reçu, à la re-relecture de la Task 15.
//! - La présence. Le décodeur relisait le JSON de chaque état, quelle que soit sa taille, avant
//!   la borne de 2 Kio et avant de savoir si la connexion écrit : un état `[0,0,…]` d'un
//!   mégaoctet faisait allouer une cinquantaine de Mio, à une connexion en lecture seule comme
//!   à une autre, qui restait servie. Un état n'est plus lu au décodage ; au-delà de 2 Kio, il
//!   est sauté.
//!
//! La borne d'un message est de 16 Mio.
//!
//! L'allocateur de ce fichier compte tout ce que le processus alloue : ses tests passent l'un
//! après l'autre ([`MEASURING`]), sans quoi l'un fausserait la mesure de l'autre.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use futures::{SinkExt, StreamExt};
use mementum_sync::{ConnInfo, SyncConfig};

/// Les octets alloués en ce moment, et leur plus haut niveau.
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// L'allocateur du système, qui compte.
struct Counting;

// SAFETY : tout est délégué à l'allocateur du système, avec les mêmes arguments ; seuls des
// compteurs sont tenus à côté. `realloc` garde sa définition par défaut, qui passe par `alloc`
// et `dealloc` : elle est comptée aussi.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY : l'appelant garantit ce que `System.alloc` exige.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY : `pointer` vient de `alloc` ci-dessus, donc de `System.alloc`, avec ce
        // `layout`.
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const MEGABYTE: usize = 1 << 20;

/// Un test à la fois : la mesure est celle de tout le processus.
static MEASURING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Une étape 1 au vecteur d'état vide : le serveur y répond par une étape 2.
const STEP_1: [u8; 4] = [0, 0, 1, 0];

/// Écrit un entier variable de lib0 : 7 bits par octet, poids faibles d'abord.
fn write_var(out: &mut Vec<u8>, mut value: usize) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Un message de présence dont chaque entrée porte l'état `state`, pour les clients 1, 2…
fn presence(states: &[&str]) -> Vec<u8> {
    let mut payload = Vec::new();
    write_var(&mut payload, states.len());
    for (client, state) in states.iter().enumerate() {
        write_var(&mut payload, client + 1);
        write_var(&mut payload, 1);
        write_var(&mut payload, state.len());
        payload.extend(state.as_bytes());
    }
    let mut message = vec![1];
    write_var(&mut message, payload.len());
    message.extend(payload);
    message
}

/// Un tableau JSON de zéros, de `bytes` octets : `[0,0,…,0]`. Relu en entier, il coûte une
/// trentaine de fois sa taille.
fn zeros(bytes: usize) -> String {
    format!("[{}0]", "0,".repeat((bytes - 3) / 2))
}

/// Une connexion par une socket en mémoire, son accueil lu.
async fn connected(f: &Fixture, conn: ConnInfo) -> MemorySocket {
    let (server_end, mut socket) = memory_socket(64);
    tokio::spawn(f.hub.clone().serve_connection(f.page, conn, server_end));
    let hello = tokio::time::timeout(WAIT, socket.next()).await.unwrap();
    assert!(hello.is_some(), "accueil attendu");
    socket
}

/// Demande une étape 2 et lit jusqu'à elle : la connexion est encore servie, et le serveur a
/// traité tout ce qu'elle a envoyé avant. Rend les messages de présence reçus entre-temps.
async fn presence_so_far(socket: &mut MemorySocket) -> Vec<Vec<u8>> {
    socket.send(STEP_1.to_vec()).await.expect("socket ouverte");
    let mut presence = Vec::new();
    loop {
        let message = tokio::time::timeout(WAIT, socket.next())
            .await
            .expect("une réponse du serveur")
            .expect("la connexion est fermée");
        match message.as_slice() {
            [0, 1, ..] => return presence,
            [1, ..] => presence.push(message),
            _ => {}
        }
    }
}

/// Un message de présence d'un mégaoctet n'est pas relu au décodage : ni son unique état
/// `[0,0,…]` d'un mégaoctet, ni ses 512 états de 2 Kio, la plus grande taille qu'un état puisse
/// avoir. D'une connexion qui écrit, le serveur n'alloue pas deux fois la taille du message ;
/// d'une connexion en lecture seule, pas le seizième. La connexion reste servie, et rien n'est
/// relayé.
#[tokio::test]
async fn a_megabyte_of_presence_allocates_less_than_twice_its_size() {
    let _alone = MEASURING.lock().await;
    let f = Fixture::new(SyncConfig::default()).await;
    let mut observer = connected(&f, writing_conn()).await;
    assert!(presence_so_far(&mut observer).await.is_empty());
    let reader = ConnInfo {
        can_write: false,
        ..writing_conn()
    };
    let one_state = zeros(MEGABYTE);
    let small = zeros(2048);
    let many_states = vec![small.as_str(); 512];
    let messages = [
        ("un état d'un mégaoctet", presence(&[&one_state])),
        ("512 états de 2 Kio", presence(&many_states)),
    ];
    // Les quatre mesures sont prises avant qu'aucune fasse échouer le test.
    let mut over = Vec::new();
    for (who, conn) in [("lecture seule", reader), ("écriture", writing_conn())] {
        for (what, message) in &messages {
            let mut socket = connected(&f, conn.clone()).await;
            assert!(message.len() >= MEGABYTE);
            // Le message lui-même est alloué avant la mesure.
            let message = message.clone();
            let size = message.len();
            let before = LIVE.load(Ordering::Relaxed);
            PEAK.store(before, Ordering::Relaxed);
            socket.send(message).await.unwrap();
            // Traité, et la connexion est encore servie.
            presence_so_far(&mut socket).await;
            let peak = PEAK.load(Ordering::Relaxed) - before;
            println!(
                "pointe d'allocation, {who}, {what} : {peak} octets, pour un message de {size} octets"
            );
            // En lecture seule, rien n'est gardé : le serveur ne lit que des longueurs, et
            // n'alloue presque rien. En écriture, les états de 2 Kio au plus sont copiés
            // pour être jugés : moins de deux fois le message.
            let limit = if conn.can_write { 2 * size } else { size / 16 };
            if peak >= limit {
                over.push(format!(
                    "{who}, {what} : {peak} octets alloués pour un message de {size} octets                      (moins de {limit} attendus)"
                ));
            }
        }
    }
    assert!(over.is_empty(), "{over:#?}");
    // Rien n'en a été relayé.
    assert_eq!(presence_so_far(&mut observer).await, Vec::<Vec<u8>>::new());
}

#[tokio::test]
async fn a_megabyte_of_messages_allocates_less_than_twice_its_size() {
    let _alone = MEASURING.lock().await;
    let f = Fixture::new(SyncConfig::default()).await;
    let (server_end, mut socket) = memory_socket(8);
    let serving = tokio::spawn(
        f.hub
            .clone()
            .serve_connection(f.page, writing_conn(), server_end),
    );
    let hello = tokio::time::timeout(WAIT, socket.next()).await.unwrap();
    assert!(hello.is_some(), "accueil attendu");

    // Un mégaoctet de demandes d'awareness : 1 048 576 messages y-websocket en un message
    // WebSocket. Le message lui-même est alloué avant la mesure.
    let message = vec![3u8; MEGABYTE];
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    socket.send(message).await.unwrap();
    // Illisible : la connexion est fermée.
    let served = tokio::time::timeout(WAIT, serving)
        .await
        .expect("la connexion qui a envoyé le message est fermée")
        .unwrap();
    assert!(served.is_ok(), "{served:?}");
    let peak = PEAK.load(Ordering::Relaxed) - before;
    println!("pointe d'allocation : {peak} octets, pour un message de {MEGABYTE} octets");
    assert!(
        peak < 2 * MEGABYTE,
        "{peak} octets alloués pour un message de {MEGABYTE} octets"
    );
}
