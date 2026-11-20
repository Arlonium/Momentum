//! Un envoi et une lecture de fichier ne tiennent pas le fichier en mémoire (Ruling R247 : le
//! corps est lu par morceaux, écrit sur le disque en calculant son hash, et servi par morceaux ;
//! Ruling R276 : le tampon de tokio du fichier d'un envoi est borné).
//!
//! L'allocateur de ce fichier compte tout ce que le processus alloue : ce binaire n'a que ce test,
//! sans quoi un autre fausserait la mesure (comme `crates/mementum-sync/tests/frame_alloc.rs`).
//! Le corps de l'envoi est produit au fil de l'eau, d'un tampon statique, et la réponse de la
//! lecture est lue trame par trame : sinon c'est le test qui allouerait le fichier. Les requêtes
//! passent par l'application en mémoire, sans la couche TCP, dont les tampons ne sont pas ceux du
//! serveur.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use common::sync::open_app;
use common::upload_uri;
use http_body_util::BodyExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use uuid::Uuid;

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

/// Un morceau du corps envoyé : statique, il n'est jamais alloué.
static CHUNK: [u8; 64 * 1024] = [42; 64 * 1024];

/// Un corps sans longueur, fait de `left` fois [`CHUNK`], produit au fil de la lecture.
struct Generated {
    left: usize,
}

impl http_body::Body for Generated {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        if self.left == 0 {
            return Poll::Ready(None);
        }
        self.left -= 1;
        Poll::Ready(Some(Ok(http_body::Frame::data(Bytes::from_static(&CHUNK)))))
    }
}

/// Envoie `chunks` fois [`CHUNK`] dans la page, puis lit le fichier trame par trame : rend la
/// taille et le hash de ce qui a été relu.
async fn round_trip(
    t: &common::TestApp,
    cookie: &str,
    page: Uuid,
    chunks: usize,
) -> (usize, [u8; 32]) {
    let request = Request::post(upload_uri(page, "gros.bin"))
        .header(header::HOST, "localhost")
        .header(header::COOKIE, cookie)
        .body(Body::new(Generated { left: chunks }))
        .unwrap();
    let response = t.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let info: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let url = info["url"].as_str().unwrap().to_string();

    let request = Request::get(&url)
        .header(header::HOST, "localhost")
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap();
    let response = t.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let (mut read, mut hasher) = (0, Sha256::new());
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.unwrap().into_data() {
            read += data.len();
            hasher.update(&data);
        }
    }
    (read, hasher.finalize().into())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_and_download_never_hold_the_file_in_memory() {
    let t = open_app().await;
    let alice = t.account("Alice").await;
    let page = alice.create(json!({ "title": "Gros fichiers" })).await;
    let page = Uuid::parse_str(&page.id).unwrap();
    let cookie = alice.browser.cookie().unwrap();
    // Un premier tour, petit, hors de la mesure : ce que le serveur alloue une fois pour toutes
    // (requêtes préparées, fils bloquants, dossier du dépôt) ne compte pas.
    round_trip(&t, &cookie, page, 2).await;
    // L'indexeur (lot 4) alloue en fond pour indexer la page créée : il a fini avant la mesure.
    t.indexer.settled().await;

    let chunks = 32 * MEGABYTE / CHUNK.len();
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let (read, hash) = round_trip(&t, &cookie, page, chunks).await;
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(before);

    assert_eq!(read, 32 * MEGABYTE);
    let mut expected = Sha256::new();
    for _ in 0..chunks {
        expected.update(CHUNK);
    }
    assert_eq!(
        hash,
        <[u8; 32]>::from(expected.finalize()),
        "les octets relus"
    );
    println!(
        "MESURE pointe d'allocation, envoi puis lecture d'un fichier de 32 Mio : {peak} octets"
    );
    assert!(
        peak < MEGABYTE,
        "{peak} octets alloués pour envoyer puis lire 32 Mio (moins de {MEGABYTE} attendus)"
    );
}
