//! L'essai **dense** sur la place des éléments (Rulings R825, R856). L'essai hostile tire ses voisins
//! parmi toutes les unités de la page : peu de ses éléments ont deux voisins dans un même texte.
//! Ici, rien d'autre : des lettres posées entre des lettres de deux textes, tous les voisins possibles,
//! en chaînes, avec des blocs supprimés. Après chaque pas : la page relue de son état, de son journal,
//! et d'un instantané suivi de la fin du journal, est la page vivante.
//!
//! Large : `MEMENTUM_FUZZ_SEEDS=6000 cargo test --release -p mementum-crdt --test placements_at_random
//! -- --nocapture`. À partir d'une graine : `MEMENTUM_FUZZ_FROM`.

#[path = "common/live.rs"]
mod live;

use mementum_crdt::{FRAGMENT, PageDoc};
use yrs::updates::decoder::Decode;
use yrs::{
    ClientID, Doc, GetString, OffsetKind, Options, StateVector, Text, Transact, Update, Xml,
    XmlElementPrelim, XmlFragment, XmlOut, XmlTextPrelim,
};

type Id = (u64, u32);

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
    fn chance(&mut self, p: u64) -> bool {
        self.below(100) < p
    }
    fn pick<T: Copy>(&mut self, list: &[T]) -> T {
        list[self.below(list.len() as u64) as usize]
    }
}

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

fn replica(id: u64, state: &[u8]) -> Doc {
    let doc = Doc::with_options(Options {
        client_id: ClientID::new(id),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(state).unwrap())
        .unwrap();
    doc
}

/// Les textes, dans l'ordre du document, tels qu'un lecteur les voit.
fn texts(doc: &Doc) -> Vec<String> {
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    let mut out = Vec::new();
    let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
        return out;
    };
    for container in group.children(&txn) {
        let XmlOut::Element(container) = container else {
            continue;
        };
        let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
            continue;
        };
        for child in paragraph.children(&txn) {
            if let XmlOut::Text(t) = child {
                out.push(t.get_string(&txn));
            }
        }
    }
    out
}

struct Bench {
    page: PageDoc,
    journal: Vec<Vec<u8>>,
    marks: Vec<(usize, Vec<u8>)>,
    letters: Vec<Id>,
    /// Les types où l'on peut poser une lettre sans voisin.
    homes: Vec<Id>,
    /// Le bloc du second texte, à supprimer : le serveur l'a écrit à l'horloge donnée du client.
    second_block: Id,
    stored: bool,
    trace: Vec<Vec<u8>>,
}

impl Bench {
    fn new(stored: bool) -> Bench {
        let (page, created) = PageDoc::new_page();
        let mut journal = vec![created];
        let (mut letters, mut homes) = (Vec::new(), Vec::new());
        // Le client 10 écrit « abcdef » dans le premier paragraphe.
        let doc = replica(10, &page.encode_state());
        let first = {
            let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
            let mut txn = doc.transact_mut();
            let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
                panic!()
            };
            let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
                panic!()
            };
            let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
                panic!()
            };
            let text = paragraph.insert(&mut txn, 0, XmlTextPrelim::new(""));
            text.insert(&mut txn, 0, "abcdef");
            txn.encode_update_v1()
        };
        journal.push(page.apply_client_update(&first).unwrap().read);
        homes.push((10, 0));
        letters.extend((1..=6).map(|c| (10, c)));
        // Le client 11 ajoute un bloc, et y écrit « ghij ».
        let doc = replica(11, &page.encode_state());
        let second = {
            let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
            let mut txn = doc.transact_mut();
            let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
                panic!()
            };
            let container = group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
            container.insert_attribute(&mut txn, "id", "0196f0a0-0000-7000-8000-000000000042");
            let paragraph = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
            let text = paragraph.push_back(&mut txn, XmlTextPrelim::new(""));
            text.insert(&mut txn, 0, "ghij");
            txn.encode_update_v1()
        };
        journal.push(page.apply_client_update(&second).unwrap().read);
        let sv = StateVector::decode_v1(&page.state_vector()).unwrap();
        let end = sv.get(&ClientID::new(11));
        letters.extend((end - 4..end).map(|c| (11, c)));
        homes.push((11, end - 5));
        Bench {
            page,
            journal,
            marks: Vec::new(),
            letters,
            homes,
            second_block: (11, 0),
            stored,
            trace: Vec::new(),
        }
    }

    fn floor(&self, client: u64) -> u32 {
        StateVector::decode_v1(&self.page.state_vector())
            .unwrap()
            .get(&ClientID::new(client))
    }

    /// Reçoit une mise à jour comme l'acteur.
    fn receive(&mut self, update: &[u8]) -> bool {
        self.trace.push(update.to_vec());
        if self.stored {
            match self.page.apply_update(update) {
                Ok(()) => {
                    self.journal.push(update.to_vec());
                    true
                }
                Err(_) => {
                    self.page = self.reloaded();
                    false
                }
            }
        } else {
            match self.page.try_client_update(update) {
                Ok(applied) => {
                    self.journal.push(applied.read);
                    self.journal.extend(applied.carried);
                    self.journal.extend(self.page.dedupe_blocks());
                    true
                }
                Err(refused) => {
                    if !refused.intact {
                        self.page = self.reloaded();
                    }
                    false
                }
            }
        }
    }

    fn reloaded(&self) -> PageDoc {
        PageDoc::from_updates(self.journal.iter().map(Vec::as_slice)).expect("le journal se relit")
    }

    fn check(&mut self, what: &str) {
        if let Some(why) = self.page.unsound_map_entries() {
            panic!("{what} : carte : {why}");
        }
        if let Some(why) = self.page.misjudged_units() {
            panic!("{what} : unités : {why}");
        }
        if let Some(why) = live::reloaded_from_state_differs(&self.page) {
            panic!("{what} : ÉTAT : {why}");
        }
        let tree = live::tree(self.page.yrs_doc());
        let again = self.reloaded();
        if live::tree(again.yrs_doc()) != tree {
            panic!(
                "{what} : JOURNAL : vivante {:?}, relue {:?}",
                texts(self.page.yrs_doc()),
                texts(again.yrs_doc())
            );
        }
        self.marks
            .push((self.journal.len(), self.page.encode_state()));
        let n = self.marks.len();
        for (len, state) in &self.marks[n.saturating_sub(6)..] {
            let tail = &self.journal[*len..];
            let again = PageDoc::from_updates(
                std::iter::once(state.as_slice()).chain(tail.iter().map(Vec::as_slice)),
            )
            .unwrap_or_else(|e| panic!("{what} : INSTANTANÉ à {len} : ne se charge pas : {e}"));
            if live::tree(again.yrs_doc()) != tree {
                panic!(
                    "{what} : INSTANTANÉ à {len} : vivante {:?}, rechargée {:?}",
                    texts(self.page.yrs_doc()),
                    texts(again.yrs_doc())
                );
            }
        }
    }
}

/// Une voisine : souvent proche d'une autre unité du même client (les formes des règles N3 et V).
fn neighbour(rng: &mut Rng, letters: &[Id], near: Option<Id>) -> Id {
    if let Some((client, clock)) = near
        && rng.chance(55)
    {
        let close: Vec<Id> = letters
            .iter()
            .copied()
            .filter(|l| l.0 == client && l.1.abs_diff(clock) <= 4)
            .collect();
        if !close.is_empty() {
            return rng.pick(&close);
        }
    }
    rng.pick(letters)
}

fn hostile(rng: &mut Rng, bench: &Bench) -> (Vec<u8>, Vec<Id>) {
    let client = rng.pick(&[10u64, 11, 20, 21, 22, 23]);
    let start = bench.floor(client);
    let mut clock = start;
    let small = std::env::var_os("MEMENTUM_FUZZ_SMALL").is_some();
    let structs = 1 + rng.below(if small { 2 } else { 4 });
    let mut body = Vec::new();
    let mut posed = Vec::new();
    let mut previous: Option<Id> = None;
    for _ in 0..structs {
        let len = if small { 1 } else { 1 + rng.below(3) as u32 };
        let form = rng.below(100);
        let chain = previous.filter(|_| rng.chance(45));
        let origin = match chain {
            Some(p) => Some(p),
            None if form < 88 => Some(neighbour(rng, &bench.letters, previous)),
            None => None,
        };
        let right = if rng.chance(70) {
            Some(neighbour(rng, &bench.letters, origin))
        } else {
            None
        };
        let mut info = 4u8;
        if origin.is_some() {
            info |= 0x80;
        }
        if right.is_some() {
            info |= 0x40;
        }
        body.push(info);
        for id in origin.iter().chain(right.iter()) {
            var(&mut body, id.0);
            var(&mut body, u64::from(id.1));
        }
        if origin.is_none() && right.is_none() {
            let home = rng.pick(&bench.homes);
            body.push(0);
            var(&mut body, home.0);
            var(&mut body, u64::from(home.1));
        }
        var(&mut body, u64::from(len));
        for n in 0..len {
            body.push(b'p' + (n as u8));
        }
        for unit in 0..len {
            posed.push((client, clock + unit));
        }
        previous = Some((client, clock + len - 1));
        clock += len;
    }
    let mut update = vec![1];
    var(&mut update, structs);
    var(&mut update, client);
    var(&mut update, u64::from(start));
    update.extend(body);
    // Un jeu de suppressions : quelques lettres, ou le second bloc.
    if rng.chance(14) {
        let victim = rng.pick(&bench.letters);
        update.push(1);
        var(&mut update, victim.0);
        update.push(1);
        var(&mut update, u64::from(victim.1));
        update.push(1);
    } else {
        update.push(0);
    }
    (update, posed)
}

fn delete_block(bench: &Bench) -> Vec<u8> {
    let mut update = vec![0, 1];
    var(&mut update, bench.second_block.0);
    update.push(1);
    var(&mut update, u64::from(bench.second_block.1));
    update.push(1);
    update
}

/// Une frappe honnête d'un client de passage (yrs), dans un texte au hasard.
fn honest(rng: &mut Rng, bench: &Bench, client: u64) -> Option<Vec<u8>> {
    let doc = replica(client, &bench.page.encode_state());
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = doc.transact_mut();
    let XmlOut::Element(group) = fragment.get(&txn, 0)? else {
        return None;
    };
    let blocks = group.len(&txn);
    let XmlOut::Element(container) = group.get(&txn, rng.below(u64::from(blocks.max(1))) as u32)?
    else {
        return None;
    };
    let XmlOut::Element(paragraph) = container.get(&txn, 0)? else {
        return None;
    };
    let XmlOut::Text(text) = paragraph.get(&txn, 0)? else {
        return None;
    };
    let len = text.len(&txn);
    text.insert(&mut txn, rng.below(u64::from(len) + 1) as u32, "z");
    Some(txn.encode_update_v1())
}

fn scene(seed: u64, stored: bool) -> (u64, u64) {
    let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ u64::from(stored));
    let mut bench = Bench::new(stored);
    let (mut accepted, mut refused) = (0, 0);
    let steps = if std::env::var_os("MEMENTUM_FUZZ_SMALL").is_some() {
        3 + rng.below(6)
    } else {
        10 + rng.below(30)
    };
    for step in 0..steps {
        let what = format!("graine {seed}, pas {step}");
        let roll = rng.below(100);
        if roll < 4 {
            let update = delete_block(&bench);
            bench.receive(&update);
        } else if roll < 14 {
            if let Some(update) = honest(&mut rng, &bench, 300 + step) {
                assert!(
                    bench.receive(&update),
                    "{what} : une frappe honnête refusée"
                );
            }
        } else {
            let (update, posed) = hostile(&mut rng, &bench);
            if bench.receive(&update) {
                accepted += 1;
                bench.letters.extend(posed);
            } else {
                refused += 1;
            }
        }
        let check = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bench.check(&what)));
        if let Err(panic) = check {
            std::panic::set_hook(Box::new(|_| {}));
            let why = panic.downcast_ref::<String>().cloned().unwrap_or_default();
            let kind = why.split(':').nth(1).unwrap_or("?").trim().to_string();
            *FAILS
                .lock()
                .unwrap()
                .entry(format!(
                    "{} {kind}",
                    if stored { "journal" } else { "client" }
                ))
                .or_insert(0u64) += 1;
            let min = minimized(bench.trace.clone(), stored);
            let size: usize = min.iter().map(Vec::len).sum();
            let mut best = BEST.lock().unwrap();
            if best
                .as_ref()
                .is_none_or(|b: &(usize, Vec<Vec<u8>>, String, bool)| size < b.0)
            {
                *best = Some((size, min, why, stored));
            }
            return (accepted, refused);
        }
    }
    (accepted, refused)
}

/// Dans les deux voies, d'un client et du journal : aucune graine ne montre un écart.
#[test]
fn placements_at_random() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let (from, seeds) = (
        number("MEMENTUM_FUZZ_FROM", 1),
        number("MEMENTUM_FUZZ_SEEDS", 60),
    );
    let (mut accepted, mut refused) = (0, 0);
    for seed in from..from + seeds {
        for stored in [false, true] {
            let (a, r) = scene(seed, stored);
            accepted += a;
            refused += r;
        }
    }
    println!(
        "ESSAI {seeds} graines par voie : {accepted} mises à jour acceptées, {refused} refusées ; échecs {:?}",
        FAILS.lock().unwrap()
    );
    if let Some((size, trace, why, stored)) = BEST.lock().unwrap().as_ref() {
        let trace: Vec<String> = trace
            .iter()
            .map(|update| update.iter().map(|b| format!("{b:02x}")).collect())
            .collect();
        panic!(
            "la plus petite trace ({size} octets, voie {}) : {why}\n{}",
            if *stored { "journal" } else { "client" },
            trace.join("\n")
        );
    }
}

static FAILS: std::sync::Mutex<std::collections::BTreeMap<String, u64>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
/// La plus courte trace en échec : sa longueur, ses lignes, ce qu'elle montre, sa voie.
type Shortest = (usize, Vec<Vec<u8>>, String, bool);
static BEST: std::sync::Mutex<Option<Shortest>> = std::sync::Mutex::new(None);

fn fails(trace: &[Vec<u8>], stored: bool) -> Option<String> {
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut bench = Bench::new(stored);
        for (n, update) in trace.iter().enumerate() {
            bench.receive(update);
            bench.check(&format!("ligne {n}"));
        }
    }));
    run.err().map(|p| {
        p.downcast_ref::<String>()
            .cloned()
            .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    })
}

/// Réduit la suite de mises à jour.
fn minimized(mut trace: Vec<Vec<u8>>, stored: bool) -> Vec<Vec<u8>> {
    let Some(first) = fails(&trace, stored) else {
        return trace;
    };
    let kind = first.split(':').nth(1).unwrap_or("").trim().to_string();
    let mut at = trace.len();
    while at > 0 {
        at -= 1;
        let mut without = trace.clone();
        without.remove(at);
        if fails(&without, stored).is_some_and(|f| f.contains(&kind)) {
            trace = without;
        }
    }
    trace
}
