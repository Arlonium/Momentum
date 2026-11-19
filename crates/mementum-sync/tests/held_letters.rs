//! Des lettres retenues par l'acteur, puis libérées ensemble (Rulings R131, R867, R869). Chaque mise
//! à jour retenue passe seule par la garde : leur somme ne fait rien recopier de plus à yrs, et une
//! mise à jour au-delà de la borne n'en profite pas pour passer.
//!
//! Seul dans son binaire : le pic de mémoire se compte pour tout le processus.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use common::{Client, Fixture};
use mementum_md::block_text;
use mementum_sync::SyncConfig;

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now =
            LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed) + layout.size() as isize;
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        let grown = new as isize - layout.size() as isize;
        let now = LIVE.fetch_add(grown, Ordering::Relaxed) + grown;
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Ce qu'une scène peut demander de plus que ce qui était vivant à son début : 20 000 lettres fondues
/// d'un coup en demanderaient 287 Mio.
const PEAK_BOUND: isize = 128 << 20;

const WRITER: u64 = 900;

/// Le pic se compte pour tout le processus : une scène à la fois.
static ALONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// `count` lettres de `WRITER` à partir de l'horloge `from`, chacune à la suite de la précédente ; la
/// première après `origin` si `from` vaut 0.
fn letters(from: u32, count: u32, origin: (u64, u32)) -> Vec<u8> {
    let mut update = vec![1];
    var(&mut update, u64::from(count));
    var(&mut update, WRITER);
    var(&mut update, u64::from(from));
    for clock in from..from + count {
        let (client, at) = if clock == 0 {
            origin
        } else {
            (WRITER, clock - 1)
        };
        update.push(0x84);
        var(&mut update, client);
        var(&mut update, u64::from(at));
        update.extend_from_slice(&[1, b'x']);
    }
    update.push(0);
    update
}

/// Un client qui a tapé une lettre, et l'identifiant de cette lettre.
async fn typed(fixture: &Fixture) -> (Client, (u64, u32)) {
    let client = fixture.client().await;
    let block = client.blocks()[0].id.clone();
    client.type_text(&block, "a");
    let seen = client.step2_count();
    client.resync();
    assert!(
        client
            .wait_until(Duration::from_secs(10), |c| c.step2_count() > seen)
            .await
    );
    let letter = (client.client_id(), 1);
    (client, letter)
}

fn start_counting() -> isize {
    let live = LIVE.load(Ordering::Relaxed);
    PEAK.store(live, Ordering::Relaxed);
    live
}

/// Quinze messages retenus, faute du premier, puis libérés ensemble quand il arrive : chacun paie ce
/// qu'il fond.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn letters_held_then_released_together_cost_what_each_message_costs() {
    let _alone = ALONE.lock().await;
    const MESSAGE: u32 = 4_000;
    let fixture = Fixture::new(SyncConfig::default()).await;
    let (client, letter) = typed(&fixture).await;
    let live = start_counting();
    for message in 1..16 {
        client.send_update(&letters(message * MESSAGE, MESSAGE, letter));
    }
    client.send_update(&letters(0, MESSAGE, letter));
    let page = fixture.page;
    let mut text = 0;
    for _ in 0..600 {
        let blocks = fixture.hub.read_blocks(page).await.unwrap();
        text = block_text(&blocks[0]).chars().count();
        if text > 16 * MESSAGE as usize {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        text,
        16 * MESSAGE as usize + 1,
        "toutes les lettres sont là"
    );
    assert!(!client.is_closed());
    let peak = PEAK.load(Ordering::Relaxed) - live;
    assert!(peak < PEAK_BOUND, "pic de {peak} octets");
}

/// Une mise à jour au-delà de la borne, retenue puis libérée, est refusée comme si elle arrivait
/// seule : rien d'elle n'entre dans la page, et yrs ne fond rien.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn letters_beyond_the_bound_are_refused_when_they_are_released() {
    let _alone = ALONE.lock().await;
    let fixture = Fixture::new(SyncConfig::default()).await;
    let (client, letter) = typed(&fixture).await;
    let live = start_counting();
    client.send_update(&letters(1, 20_000, letter));
    client.send_update(&letters(0, 1, letter));
    assert!(
        client
            .wait_until(Duration::from_secs(30), Client::is_closed)
            .await,
        "la connexion est fermée"
    );
    let peak = PEAK.load(Ordering::Relaxed) - live;
    assert!(peak < PEAK_BOUND, "pic de {peak} octets");
    let blocks = fixture.hub.read_blocks(fixture.page).await.unwrap();
    assert!(block_text(&blocks[0]).chars().count() <= 2);
}
