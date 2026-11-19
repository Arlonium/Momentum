//! L'essai **dense** sur la place des éléments (Rulings R825, R856, R861, R862). L'essai hostile tire
//! ses voisins parmi toutes les unités de la page : peu de ses éléments ont deux voisins dans un même
//! texte. Ici, rien d'autre : des lettres posées entre des lettres de deux textes, tous les voisins
//! possibles, en chaînes, de plusieurs clients à la fois, avec pour voisins ce que la mise à jour
//! pose elle-même, des plages ramassées, des entrées de map, des blocs supprimés.
//!
//! Après chaque pas, la page vivante est ce que lisent : la page relue de son état, de son journal,
//! et de tout instantané suivi de la fin du journal ; **un lecteur sans garde**, un yrs neuf à qui
//! elle sert son état, et un onglet (un yrs sans garde) qui n'a reçu que ce qu'elle a relayé. Une
//! page relue repasse par les règles de la garde, et ne voit pas ce qu'elles récrivent mal.
//!
//! Le second lecteur sans garde est le vrai Yjs : `tests/fixtures/hostile/served.json` porte des
//! pages servies, que `apps/web/src/editor/servedStates.test.ts` relit. `EXPORT_FIXTURES=1` le
//! réécrit (Rust d'abord).
//!
//! Large : `MEMENTUM_FUZZ_SEEDS=6000 cargo test --release -p mementum-crdt --test placements_at_random
//! -- --nocapture`. À partir d'une graine : `MEMENTUM_FUZZ_FROM`. `MEMENTUM_FUZZ_SERVED=<fichier>`
//! y écrit aussi chaque page servie, que le test de Vitest relit, avec la même variable.

#[path = "common/live.rs"]
mod live;
#[path = "common/twins.rs"]
mod twins;

use std::io::Write;

use mementum_crdt::{FRAGMENT, PageDoc};
use serde_json::json;
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|n| u8::from_str_radix(&text[2 * n..2 * n + 2], 16).unwrap())
        .collect()
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
    /// Les entrées de map qu'on peut récrire, et l'élément qui en porte.
    entries: Vec<Id>,
    /// Les types où l'on peut poser une lettre sans voisin.
    homes: Vec<Id>,
    /// Le bloc du second texte, à supprimer : le serveur l'a écrit à l'horloge donnée du client.
    second_block: Id,
    stored: bool,
    trace: Vec<Vec<u8>>,
    /// Un onglet sans garde : l'état de départ, puis ce que la page a relayé (voie d'un client).
    tab: Doc,
    /// Les mises à jour gardées sur lesquelles le yrs du serveur et celui de crates.io ont été
    /// comparés (`common/twins.rs`, Ruling R875).
    compared: twins::Compared,
}

impl Bench {
    /// Une page neuve, ou relue de `base` (des scènes rejouables à l'octet près).
    fn new(stored: bool, base: Option<&[u8]>) -> Bench {
        let (page, created) = match base {
            Some(base) => (PageDoc::from_updates([base]).unwrap(), base.to_vec()),
            None => PageDoc::new_page(),
        };
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
        let tab = replica(1, &page.encode_state());
        Bench {
            page,
            journal,
            marks: Vec::new(),
            letters,
            entries: vec![(11, 1)],
            homes,
            second_block: (11, 0),
            stored,
            trace: Vec::new(),
            tab,
            compared: twins::Compared::default(),
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
            let answer = twins::watching("une entrée relue", &mut self.compared, || {
                self.page.apply_update(update)
            });
            match answer {
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
            let answer = twins::watching("une mise à jour reçue", &mut self.compared, || {
                self.page.try_client_update(update)
            });
            match answer {
                Ok(applied) => {
                    let mut relayed = vec![applied.read];
                    relayed.extend(applied.carried);
                    relayed.extend(self.page.dedupe_blocks());
                    for bytes in &relayed {
                        self.tab
                            .transact_mut()
                            .apply_update(Update::decode_v1(bytes).unwrap())
                            .unwrap();
                    }
                    self.journal.extend(relayed);
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
        if let Some(why) = live::bare_reader_differs(&self.page) {
            panic!("{what} : SANS GARDE : {why}");
        }
        let tree = live::tree(self.page.yrs_doc());
        if !self.stored && live::tree(&self.tab) != tree {
            panic!(
                "{what} : ONGLET : vivante {:?}, onglet {:?}",
                texts(self.page.yrs_doc()),
                texts(&self.tab)
            );
        }
        let again = self.reloaded();
        if live::tree(again.yrs_doc()) != tree {
            panic!(
                "{what} : JOURNAL : vivante {:?}, relue {:?}",
                texts(self.page.yrs_doc()),
                texts(again.yrs_doc())
            );
        }
        // Une compaction coupe le journal à un point quelconque : tous les points de la scène.
        self.marks
            .push((self.journal.len(), self.page.encode_state()));
        for (len, state) in &self.marks {
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

/// Ce qu'un client écrit dans une mise à jour.
struct Written {
    client: u64,
    start: u32,
    clock: u32,
    structs: u64,
    body: Vec<u8>,
    previous: Option<Id>,
}

/// Ce que tire une mise à jour : ses octets, les unités qu'elle pose, les entrées de map parmi elles.
struct Drawn {
    update: Vec<u8>,
    posed: Vec<Id>,
    entries: Vec<Id>,
}

/// Une mise à jour d'un à trois clients : des lettres, dont les voisins sont des lettres de la page
/// ou ce que la mise à jour pose elle-même, des plages ramassées, des entrées de map, et un jeu de
/// suppressions.
fn hostile(rng: &mut Rng, bench: &Bench) -> Drawn {
    let mut clients = vec![10u64, 11, 20, 21, 22, 23];
    let n_clients = if rng.chance(70) {
        1
    } else {
        2 + rng.below(2) as usize
    };
    let mut out: Vec<Written> = (0..n_clients)
        .map(|_| {
            let client = clients.remove(rng.below(clients.len() as u64) as usize);
            let start = bench.floor(client);
            Written {
                client,
                start,
                clock: start,
                structs: 0,
                body: Vec::new(),
                previous: None,
            }
        })
        .collect();
    let (mut posed, mut entries): (Vec<Id>, Vec<Id>) = (Vec::new(), Vec::new());
    for _ in 0..1 + rng.below(7) {
        let k = rng.below(out.len() as u64) as usize;
        let previous = out[k].previous;
        let chained = if rng.chance(25) && !posed.is_empty() {
            Some(rng.pick(&posed))
        } else {
            previous
        };
        let roll = rng.below(100);
        let w = &mut out[k];
        w.structs += 1;
        // Une plage ramassée.
        if roll < 4 {
            let len = 1 + rng.below(3) as u32;
            w.body.push(0);
            var(&mut w.body, u64::from(len));
            posed.extend((0..len).map(|unit| (w.client, w.clock + unit)));
            w.previous = Some((w.client, w.clock + len - 1));
            w.clock += len;
            continue;
        }
        // Une entrée de map : une clé neuve du bloc, ou une entrée récrite par son origine.
        if roll < 12 {
            if rng.chance(50) {
                let origin = rng.pick(&bench.entries);
                w.body.push(0x88);
                var(&mut w.body, origin.0);
                var(&mut w.body, u64::from(origin.1));
            } else {
                w.body.extend_from_slice(&[0x28, 0]);
                var(&mut w.body, bench.second_block.0);
                var(&mut w.body, u64::from(bench.second_block.1));
                w.body.extend_from_slice(&[2, b'z', b'z']);
            }
            w.body.extend_from_slice(&[1, 119, 1, b'v']);
            posed.push((w.client, w.clock));
            entries.push((w.client, w.clock));
            w.previous = Some((w.client, w.clock));
            w.clock += 1;
            continue;
        }
        let len = if rng.chance(70) {
            1
        } else {
            1 + rng.below(3) as u32
        };
        let origin = match chained.filter(|_| rng.chance(55)) {
            Some(p) => Some(p),
            None if rng.chance(90) => Some(neighbour(rng, &bench.letters, previous)),
            None => None,
        };
        let right = if rng.chance(30) {
            None
        } else if !posed.is_empty() && rng.chance(40) {
            Some(rng.pick(&posed))
        } else {
            Some(neighbour(rng, &bench.letters, origin))
        };
        let mut info = 4u8;
        if origin.is_some() {
            info |= 0x80;
        }
        if right.is_some() {
            info |= 0x40;
        }
        w.body.push(info);
        for id in origin.iter().chain(right.iter()) {
            var(&mut w.body, id.0);
            var(&mut w.body, u64::from(id.1));
        }
        if origin.is_none() && right.is_none() {
            let home = rng.pick(&bench.homes);
            w.body.push(0);
            var(&mut w.body, home.0);
            var(&mut w.body, u64::from(home.1));
        }
        var(&mut w.body, u64::from(len));
        w.body.extend((0..len).map(|n| b'p' + n as u8));
        posed.extend((0..len).map(|unit| (w.client, w.clock + unit)));
        w.previous = Some((w.client, w.clock + len - 1));
        w.clock += len;
    }
    out.retain(|w| w.structs > 0);
    let mut update = Vec::new();
    var(&mut update, out.len() as u64);
    for w in &out {
        var(&mut update, w.structs);
        var(&mut update, w.client);
        var(&mut update, u64::from(w.start));
        update.extend(&w.body);
    }
    // Un jeu de suppressions : des lettres de la page, ou ce qui vient d'être posé.
    let mut victims: Vec<Id> = Vec::new();
    if rng.chance(20) {
        victims.push(rng.pick(&bench.letters));
    }
    if rng.chance(15) && !posed.is_empty() {
        victims.push(rng.pick(&posed));
    }
    victims.sort_unstable();
    victims.dedup_by_key(|v| v.0);
    var(&mut update, victims.len() as u64);
    for (client, clock) in victims {
        var(&mut update, client);
        update.push(1);
        var(&mut update, u64::from(clock));
        update.push(1);
    }
    Drawn {
        update,
        posed,
        entries,
    }
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

/// Ce que donne une scène : les mises à jour acceptées, refusées, et la page servie à sa fin (rien
/// si un pas a montré un écart).
struct Played {
    accepted: u64,
    refused: u64,
    served: Option<Bench>,
}

fn scene(seed: u64, stored: bool, base: Option<&[u8]>, steps: Option<u64>) -> Played {
    let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ u64::from(stored));
    let mut bench = Bench::new(stored, base);
    let (mut accepted, mut refused) = (0, 0);
    let steps = steps.unwrap_or_else(|| 10 + rng.below(30));
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
            let drawn = hostile(&mut rng, &bench);
            if bench.receive(&drawn.update) {
                accepted += 1;
                bench.letters.extend(drawn.posed);
                bench.entries.extend(drawn.entries);
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
            return Played {
                accepted,
                refused,
                served: None,
            };
        }
    }
    Played {
        accepted,
        refused,
        served: Some(bench),
    }
}

/// Une page servie, telle que le test de Vitest la relit.
fn served(seed: u64, stored: bool, page: &PageDoc) -> serde_json::Value {
    json!({
        "seed": seed,
        "stored": stored,
        "state": hex(&page.encode_state()),
        "outline": live::outline(page.yrs_doc()),
    })
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
    let mut sink = std::env::var_os("MEMENTUM_FUZZ_SERVED")
        .map(|path| std::io::BufWriter::new(std::fs::File::create(path).unwrap()));
    let (mut accepted, mut refused) = (0, 0);
    let mut compared = twins::Compared::default();
    for seed in from..from + seeds {
        for stored in [false, true] {
            let played = scene(seed, stored, None, None);
            accepted += played.accepted;
            refused += played.refused;
            if let Some(bench) = &played.served {
                compared.updates += bench.compared.updates;
                compared.retried += bench.compared.retried;
            }
            if let (Some(sink), Some(bench)) = (&mut sink, &played.served) {
                writeln!(sink, "{}", served(seed, stored, &bench.page)).unwrap();
            }
        }
    }
    println!(
        "ESSAI {seeds} graines par voie : {accepted} mises à jour acceptées, {refused} refusées ; \
         {} mises à jour gardées, comparées dans les deux yrs (dont {} où yrs ne fait pas deux \
         fois la même chose) ; échecs {:?}",
        compared.updates,
        compared.retried,
        FAILS.lock().unwrap()
    );
    assert!(
        compared.updates > seeds,
        "les deux yrs n'ont pas été comparés"
    );
    if let Some((size, trace, why, stored)) = BEST.lock().unwrap().as_ref() {
        let trace: Vec<String> = trace.iter().map(|update| hex(update)).collect();
        panic!(
            "la plus petite trace ({size} octets, voie {}) : {why}\n{}",
            if *stored { "journal" } else { "client" },
            trace.join("\n")
        );
    }
}

/// Le nombre de graines, par voie, de `served.json`.
const SERVED_SCENES: u64 = 30;
/// Le nombre de pas de ses scènes.
const SERVED_STEPS: u64 = 12;

/// `served.json` est ce que la page sert aujourd'hui : un changement de la garde qui change une page
/// servie demande de le réécrire (`EXPORT_FIXTURES=1`), puis de le faire relire au vrai Yjs.
#[test]
fn the_pages_given_to_the_real_yjs_are_what_the_page_serves() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/hostile/served.json"
    );
    let export = std::env::var_os("EXPORT_FIXTURES").is_some();
    let base = if export {
        PageDoc::new_page().1
    } else {
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        unhex(written["base"].as_str().unwrap())
    };
    let mut scenes = Vec::new();
    for seed in 1..=SERVED_SCENES {
        for stored in [false, true] {
            let played = scene(seed, stored, Some(&base), Some(SERVED_STEPS));
            let bench = played.served.expect("la scène ne montre aucun écart");
            scenes.push(served(seed, stored, &bench.page));
        }
    }
    let written =
        serde_json::to_string(&json!({ "base": hex(&base), "scenes": scenes })).unwrap() + "\n";
    if export {
        std::fs::write(path, &written).unwrap();
    } else {
        assert!(
            std::fs::read_to_string(path).unwrap() == written,
            "{path} n'est plus ce que la page sert (EXPORT_FIXTURES=1 le réécrit)"
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
        let mut bench = Bench::new(stored, None);
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
