//! Ruling R124, mesuré : un message WebSocket fait d'un mégaoctet de messages y-websocket ne
//! fait pas allouer au serveur plus que quelques fois sa taille.
//!
//! Avant la borne de 32 messages y-websocket par message WebSocket, un message fait d'octets
//! `03` (une demande d'awareness chacun) était découpé en autant de messages : 72 Mio alloués
//! pour 1 Mio reçu, à la re-relecture de la Task 15. La borne d'un message est de 16 Mio.
//!
//! Ce fichier ne porte qu'un test : son allocateur compte tout ce que le processus alloue, et
//! un autre test, lancé en même temps, fausserait la mesure.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use futures::{SinkExt, StreamExt};
use mementum_sync::SyncConfig;

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

#[tokio::test]
async fn a_megabyte_of_messages_allocates_less_than_twice_its_size() {
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
