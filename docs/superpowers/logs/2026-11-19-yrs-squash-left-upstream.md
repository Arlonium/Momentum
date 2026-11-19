# Signalement amont pour yrs : `squash_left` est quadratique

Texte prêt à être posté par le propriétaire sur <https://github.com/y-crdt/y-crdt/issues> (Ruling R878). Rien n'a été posté. Le correctif proposé est celui que le dépôt porte dans `vendor/yrs` (Ruling R874) ; la reproduction a été compilée et lancée contre la crate `yrs` 0.28.1 de crates.io (chiffres plus bas).

---

**Title:** `ClientBlockList::squash_left` is quadratic in time and memory: 40,000 one-letter items need 1.2 GiB to commit

### Summary

At the end of a transaction, yrs squashes neighbouring blocks of a client (`TransactionMut::commit`, steps 7 and 8, through `ClientBlockList::squash_left` in `block_store.rs`). A run of `k` squashable blocks is folded **from its last block to its first**:

```rust
// yrs 0.28.1, block_store.rs
pub(crate) fn squash_left(&mut self, pos: usize) -> usize {
    let mut right = unsafe { &mut *self.inner[pos].get() };
    let mut i = pos;
    while i > 0 {
        let left = unsafe { &mut *self.inner[i - 1].get() };
        if left.is_deleted() == right.is_deleted()
            && left.same_type(right)
            && left.try_squash(right)   // copies ALL of `right` into `left`
        {
            /* ... map entry ... */
        } else {
            break;
        }
        i -= 1;
        right = left;                   // `left` now holds everything folded so far
    }
    let merged = pos - i;
    if merged > 0 {
        self.inner.drain(i + 1..=pos);  // the copies are only freed here
    }
    merged
}
```

Step `j` copies the content of the `j` blocks already folded into the next block on the left (`ItemContent::try_squash`: `push_str` for strings, `v1.append(&mut v2.clone())` for `Any` and JSON), and every intermediate block keeps its copy until the final `drain`. For a run of `k` blocks of one unit each, that is `k²/2` units copied **and held at the same time**.

The run does not have to come from a hostile peer. Any transaction that creates many small items next to each other pays it at commit:

- a local transaction that inserts characters one at a time (`text.insert` in a loop, an editor binding replaying keystrokes, an import);
- a remote update that carries such items: the update of a transaction encoded before its commit (`TransactionMut::encode_update_v1`, or an `observe_update_v1` consumer relaying it), or any update written by a peer that does not squash before encoding;
- `Array::push_back` in a loop (`ItemContent::Any` is squashed the same way, and is far more expensive per unit).

### Reproduction (yrs 0.28.1, public API only)

```rust
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use yrs::updates::decoder::Decode;
use yrs::{Array, Doc, GetString, Text, Transact, Update};

struct Peak;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static TOP: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
        TOP.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Peak = Peak;

fn measured(what: &str, n: u32, run: impl FnOnce()) {
    let before = LIVE.load(Ordering::Relaxed);
    TOP.store(before, Ordering::Relaxed);
    let start = Instant::now();
    run();
    println!(
        "{what}, n = {n}: peak {:.1} MiB, {:.2} s",
        (TOP.load(Ordering::Relaxed) - before) as f64 / (1 << 20) as f64,
        start.elapsed().as_secs_f64()
    );
}

fn main() {
    for n in [10_000u32, 20_000, 40_000] {
        // 1. One local transaction typing n letters, one `insert` per letter.
        let doc = Doc::new();
        let text = doc.get_or_insert_text("t");
        let mut update = Vec::new();
        measured("local transaction, letters", n, || {
            let mut txn = doc.transact_mut();
            for at in 0..n {
                text.insert(&mut txn, at, "x");
            }
            // What a provider would relay: the transaction, encoded before its commit.
            update = txn.encode_update_v1();
        }); // commit happens here

        // 2. A remote peer applying that update (n one-letter items, about 7 bytes each).
        let remote = Doc::new();
        let remote_text = remote.get_or_insert_text("t");
        println!("update of {} bytes", update.len());
        measured("remote update, letters", n, || {
            remote
                .transact_mut()
                .apply_update(Update::decode_v1(&update).unwrap())
                .unwrap();
        });
        assert_eq!(remote_text.get_string(&remote.transact()).len(), n as usize);
    }
    for n in [2_000u32, 5_000, 10_000] {
        // 3. The same with values pushed to an array.
        let doc = Doc::new();
        let array = doc.get_or_insert_array("a");
        measured("local transaction, array values", n, || {
            let mut txn = doc.transact_mut();
            for _ in 0..n {
                array.push_back(&mut txn, true);
            }
        });
        assert_eq!(array.len(&doc.transact()), n);
    }
}
```

`cargo run --release` with `yrs = "=0.28.1"`:

```text
local transaction, letters, n = 10000: peak 72.9 MiB, 0.33 s
update of 129878 bytes
remote update, letters, n = 10000: peak 72.8 MiB, 0.03 s
local transaction, letters, n = 20000: peak 288.0 MiB, 1.79 s
update of 263494 bytes
remote update, letters, n = 20000: peak 287.5 MiB, 0.13 s
local transaction, letters, n = 40000: peak 1143.3 MiB, 8.42 s
update of 543494 bytes
remote update, letters, n = 40000: peak 1142.3 MiB, 0.73 s
local transaction, array values, n = 2000: peak 46.2 MiB, 0.05 s
local transaction, array values, n = 5000: peak 287.2 MiB, 0.34 s
local transaction, array values, n = 10000: peak 1146.7 MiB, 1.63 s
```

The memory is quadratic: twice the items, four times the peak. Half a megabyte of update is enough to make a server allocate more than a gigabyte in one transaction (with small client ids the same 40,000 letters fit in 264 KB), and 10,000 array values do the same.

### Proposed fix

Find the run first, then fold it **from its first block to its last**, so that each content is copied once into the block that survives. The conditions that decide whether two neighbours squash (`Block::try_squash` → `ItemPtr::try_squash` → `ItemContent::try_squash`) only read fields that a squash further to the right does not change (`id`, `len`, `origin`, `right_origin`, `right`, the deleted, `redone` and linked flags, and the kind of the content of each of the two blocks), so the run can be found by a predicate that does not mutate anything, and the left-to-right fold then meets exactly the same conditions through `try_squash` itself.

The patch below is the one we run in production on top of 0.28.1. It is written as additions only (the original loop is kept for runs of at most two blocks, where it is already linear), which makes it easy to review; upstream would probably rather have `try_squash` call the new predicate and drop the old loop.

`block.rs`, next to `Block::try_squash`:

```rust
/// Whether `ClientBlockList::squash_left` would squash `other`, the next block of the same
/// client, into this one: the conditions it checks before `Block::try_squash`, and those of
/// `ItemPtr::try_squash` and `ItemContent::try_squash`, without changing either block.
pub(crate) fn can_squash(&self, other: &Block) -> bool {
    match (self, other) {
        (Block::Item(a), Block::Item(b)) => {
            a.id.client == b.id.client
                && a.id.clock + a.len() == b.id.clock
                && b.origin == Some(a.last_id())
                && a.right_origin == b.right_origin
                && a.right == Some(ItemPtr::from(b.as_ref()))
                && a.is_deleted() == b.is_deleted()
                && (a.redone.is_none() && b.redone.is_none())
                && (!a.info.is_linked() && !b.info.is_linked())
                && matches!(
                    (&a.content, &b.content),
                    (ItemContent::Any(_), ItemContent::Any(_))
                        | (ItemContent::Deleted(_), ItemContent::Deleted(_))
                        | (ItemContent::JSON(_), ItemContent::JSON(_))
                        | (ItemContent::String(_), ItemContent::String(_))
                )
        }
        (Block::Skip(_), Block::Skip(_)) | (Block::GC(_), Block::GC(_)) => true,
        _ => false,
    }
}
```

`block_store.rs`, at the top of `squash_left`:

```rust
let mut first = pos;
while first > 0 {
    let left = unsafe { &*self.inner[first - 1].get() };
    let right = unsafe { &*self.inner[first].get() };
    if !left.can_squash(right) {
        break;
    }
    first -= 1;
}
if pos - first > 1 {
    let mut last = first;
    {
        let (head, tail) = self.inner.split_at_mut(first + 1);
        let into = unsafe { &mut *head[first].get() };
        for cell in &tail[..pos - first] {
            if !into.try_squash(unsafe { &*cell.get() }) {
                break; // never taken while `can_squash` says what `try_squash` does
            }
            last += 1;
        }
    }
    // update parents' map entries, block by block from the last one, as the original loop
    for i in (first + 1..=last).rev() {
        let left = unsafe { &*self.inner[i - 1].get() };
        let right = unsafe { &*self.inner[i].get() };
        if let Block::Item(right) = right {
            if let Some(parent_sub) = &right.parent_sub {
                if let Some(mut parent) = right.parent.as_branch().copied() {
                    let e = parent.map.get_mut(parent_sub).unwrap();
                    if &**e == &**right {
                        *e = left.as_item().unwrap();
                    }
                }
            }
        }
    }
    self.inner.drain(first + 1..=last);
    return last - first;
}
// ... original loop, now only reached for runs of one or two blocks
```

With the patch, the same program prints:

```text
local transaction, letters, n = 10000: peak 2.1 MiB, 0.30 s
remote update, letters, n = 10000: peak 2.3 MiB, 0.00 s
local transaction, letters, n = 20000: peak 4.6 MiB, 1.71 s
remote update, letters, n = 20000: peak 4.6 MiB, 0.01 s
local transaction, letters, n = 40000: peak 9.1 MiB, 7.78 s
remote update, letters, n = 40000: peak 9.3 MiB, 0.01 s
local transaction, array values, n = 2000: peak 0.5 MiB, 0.01 s
local transaction, array values, n = 5000: peak 1.3 MiB, 0.07 s
local transaction, array values, n = 10000: peak 2.7 MiB, 0.28 s
```

(The time of the local transaction of letters stays quadratic: `Text::insert` walks to its index for every letter. That is another matter.)

### How we checked that nothing else changes

The patched crate and the published one are driven side by side by the same test code (the published crate as a renamed dev-dependency), and everything observable is compared after every step: the encoded state (v1 and v2), the state vector, and a full read of every root type (text chunks and formatting, arrays, map entries, XML trees and attributes, quoted ranges):

- random scenes through the public API: three peers, transactions relayed before their commit, out-of-order delivery, state exchanges, formatting, embeds, arrays, maps, weak links, undo and redo, with and without `skip_gc`;
- hand-made updates, one per condition of `try_squash`, from 1 to 200 items: runs that squash, and runs that must not at exactly one seam between two runs that do (the origin, the right origin, the right neighbour, the deleted flag, the kind of content, a GC range). A condition missed at every seam tests nothing: with a predicate that is too wide, `try_squash` still refuses each pair, and the outcome is the same;
- about 1.9 million updates kept by our server (1.48 million from a hostile fuzzer, behind a guard that refuses what yrs does not handle safely; 250,000 from a denser fuzzer of placements; 200,000 written by the real Yjs, undo and redo included), each replayed on both crates on top of the state that preceded it.

No difference was found. Mutating the patch line by line turns these tests red, except for lines that cannot change the outcome (the client and clock conditions, which always hold between neighbours of one client's list; the kinds of content whose squash copies nothing), the two `redone` checks (we found no run of three blocks with a redone item next to two blocks that squash) and the map-entry rewiring, which no input we could safely give to the published crate reaches.

### Three things we met on the way (not part of this report, mentioned in case they are news)

The first and the third were read in the code, not reproduced; the second was observed, on the published crate compared with itself.

- `squash_left_range_compaction` (the delete-set path) folds items right to left in the same way. It is harmless as long as the garbage collector has turned deleted contents into `ItemContent::Deleted`, but with `skip_gc`, or for items kept by an `UndoManager`, a run of deleted strings pays the same square.
- yrs is not deterministic for a given sequence of updates: `TransactionMut::delete` pushes the already-deleted map entries of a deleted type to `merge_blocks` in the iteration order of a `HashMap`, and `commit` then squashes neighbouring GC ranges in that order, which leaves one range or two; `UndoManager::try_process` redoes the items of a stack item in the order of a `HashSet<ItemPtr>`, so the clocks of the redone items differ from one run to the next; and the options of a sub-document and the keys of an `Any::Map` are encoded in `HashMap` order. Two replicas given the same updates in the same order can therefore encode different bytes.
- In the map-entry rewiring of `squash_left` (`if &**e == &**right`), the comparison is the derived `PartialEq` of `Item`, which compares contents: an entry whose `Any` content holds a `NaN` is not equal to itself, is not rewired, and is left pointing to a block that `drain` then frees.
