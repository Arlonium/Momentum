//! Essai au hasard hostile (garde des mises à jour, tour 4 ; Ruling R800).
//!
//! yrs 0.28.1 n'est pas sûr en mémoire devant une mise à jour hostile : la sûreté du serveur
//! repose sur la garde, et la garde sur cet essai. Des mises à jour tirées au hasard (tous
//! contenus, parents, origines, origines droites, clés, longueurs, éléments à cheval sur ce que
//! la page connaît, `Skip`, plages ramassées, deux clients, suppressions, octets abîmés), mêlées
//! d'écritures honnêtes, arrivent à une page vivante par le chemin de l'acteur : d'un client
//! (`apply_client_update`, ce qu'il rend va au journal) ou relues comme un vieux journal
//! (`apply_update`, les octets tels quels, une entrée refusée étant écartée). Après chaque pas :
//! l'état encodé se relit par un yrs neuf, ce qui manque à chaque vecteur d'état passé aussi ; à
//! la fin, puis après avoir supprimé les blocs un à un (de la mémoire est libérée) : la page
//! rechargée du journal est la page vivante (blocs, révision).
//!
//! Déterministe par graine. `MEMENTUM_FUZZ_SEEDS` règle le nombre de cas de chaque voie,
//! `MEMENTUM_FUZZ_FROM` le premier. Les cas tournent dans un **processus fils** (ce binaire,
//! relancé sur `child_cases`) : une violation d'accès arrête le fils, pas le rapport ; le parent
//! la compte comme un échec de la graine en cours, et reprend à la suivante.
//!
//! Les traces de `fixtures/hostile/` sont les cas trouvés par l'essai, réduits : `C [octets]`
//! est une mise à jour reçue (la première ligne, la création de la page), `R` un rechargement.

use std::collections::BTreeMap;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::{Command, Stdio};

use mementum_crdt::{FRAGMENT, PageDoc};
use yrs::updates::decoder::Decode;
use yrs::{
    ClientID, Doc, GetString, OffsetKind, Options, ReadTxn, StateVector, Text, Transact, Update,
    Xml, XmlElementPrelim, XmlFragment, XmlOut, XmlTextPrelim,
};

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

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

fn string(out: &mut Vec<u8>, s: &str) {
    var(out, s.len() as u64);
    out.extend(s.as_bytes());
}

fn fresh() -> Doc {
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc
}

/// Ce qu'un client neuf (yrs) lit d'un état. Dans la voie du journal, une page peut garder un
/// trou d'horloge (Ruling R131) : des éléments en attente y sont admis.
fn reread(state: &[u8], pending_allowed: bool) -> Result<String, String> {
    let doc = fresh();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = Update::decode_v1(state).map_err(|e| format!("illisible : {e:?}"))?;
    doc.transact_mut()
        .apply_update(update)
        .map_err(|e| format!("refusé : {e:?}"))?;
    let txn = doc.transact();
    if !pending_allowed && txn.store().pending_update().is_some() {
        return Err("des éléments restent en attente".to_string());
    }
    Ok(fragment.get_string(&txn))
}

/// Écrit une mise à jour reçue à la suite du fichier `MEMENTUM_FUZZ_TRACE`, s'il est donné : la
/// trace d'un cas, que `child_minimize` réduit et que [`replay`] rejoue.
fn trace(update: &[u8]) {
    if let Ok(path) = std::env::var("MEMENTUM_FUZZ_TRACE") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(file, "C {update:?}").unwrap();
    }
}

/// Une page servie comme l'acteur la sert, et son journal.
struct Served {
    page: PageDoc,
    journal: Vec<Vec<u8>>,
    /// Voie du journal : les entrées sont relues par `apply_update`, telles quelles.
    stored: bool,
    next_client: u64,
    next_block: u64,
}

impl Served {
    fn new(stored: bool) -> Served {
        let (page, created) = PageDoc::new_page();
        trace(&created);
        Served {
            page,
            journal: vec![created],
            stored,
            next_client: 100,
            next_block: 1,
        }
    }

    fn from_creation(created: Vec<u8>, stored: bool) -> Served {
        let journal = vec![created];
        let page = PageDoc::from_updates(journal.iter().map(Vec::as_slice)).expect("la création");
        Served {
            page,
            journal,
            stored,
            next_client: 100,
            next_block: 1,
        }
    }

    fn client(&mut self) -> Doc {
        self.next_client += 1;
        let doc = Doc::with_options(Options {
            client_id: ClientID::new(self.next_client),
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        doc.get_or_insert_xml_fragment(FRAGMENT);
        doc.transact_mut()
            .apply_update(Update::decode_v1(&self.page.encode_state()).expect("état lisible"))
            .expect("état applicable");
        doc
    }

    /// Reçoit une mise à jour comme l'acteur : `Ok(vrai)` si elle a écrit quelque chose. Après
    /// un refus, la page est rechargée de son journal.
    fn receive(&mut self, update: &[u8]) -> Result<bool, String> {
        trace(update);
        if self.stored {
            return match self.page.apply_update(update) {
                Ok(()) => {
                    self.journal.push(update.to_vec());
                    Ok(true)
                }
                Err(error) => {
                    self.page = self.reloaded();
                    Err(error.to_string())
                }
            };
        }
        match self.page.apply_client_update(update) {
            Ok(applied) => {
                let wrote = applied.read != [0, 0];
                self.journal.push(applied.read);
                self.journal.extend(applied.carried);
                self.journal.extend(self.page.dedupe_blocks());
                Ok(wrote)
            }
            Err(error) => {
                self.page = self.reloaded();
                Err(error.to_string())
            }
        }
    }

    fn reloaded(&self) -> PageDoc {
        PageDoc::from_updates(self.journal.iter().map(Vec::as_slice)).expect("le journal se relit")
    }

    fn sv(&self) -> Vec<(u64, u32)> {
        let state = StateVector::decode_v1(&self.page.state_vector()).unwrap();
        let mut all: Vec<(u64, u32)> = state.iter().map(|(c, k)| (c.get(), *k)).collect();
        all.sort_unstable();
        all
    }

    /// Une écriture honnête d'un client de passage : `None` si la page n'a pas la forme attendue.
    fn honest(&mut self, rng: &mut Rng) -> Option<Vec<u8>> {
        let doc = self.client();
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            return None;
        };
        let blocks = group.len(&txn);
        let pick = rng.below(u64::from(blocks.max(1))) as u32;
        match rng.below(10) {
            0..=2 => {
                self.next_block += 1;
                let container =
                    group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
                container.insert_attribute(
                    &mut txn,
                    "id",
                    format!("0196f0a0-0000-7000-8000-{:012}", self.next_block),
                );
                let paragraph = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
                paragraph.insert_attribute(&mut txn, "backgroundColor", "red");
                if rng.chance(50) {
                    paragraph.insert_attribute(&mut txn, "textColor", "blue");
                }
                let text = paragraph.push_back(&mut txn, XmlTextPrelim::new(""));
                text.insert(&mut txn, 0, "bonjour");
            }
            3..=5 => {
                let XmlOut::Element(container) = group.get(&txn, pick)? else {
                    return None;
                };
                let XmlOut::Element(paragraph) = container.get(&txn, 0)? else {
                    return None;
                };
                match paragraph.get(&txn, 0) {
                    Some(XmlOut::Text(text)) => {
                        let len = text.len(&txn);
                        let at = rng.below(u64::from(len) + 1) as u32;
                        text.insert(&mut txn, at, "xy");
                    }
                    None => {
                        let text = paragraph.push_back(&mut txn, XmlTextPrelim::new(""));
                        text.insert(&mut txn, 0, "abc");
                    }
                    _ => return None,
                }
            }
            6..=7 => {
                let XmlOut::Element(container) = group.get(&txn, pick)? else {
                    return None;
                };
                let XmlOut::Element(paragraph) = container.get(&txn, 0)? else {
                    return None;
                };
                let (key, value) = match rng.below(3) {
                    0 => (
                        "backgroundColor",
                        if rng.chance(50) { "blue" } else { "green" },
                    ),
                    1 => ("textColor", "red"),
                    _ => ("textAlignment", "center"),
                };
                paragraph.insert_attribute(&mut txn, key, value);
            }
            _ => {
                if blocks < 2 {
                    return None;
                }
                group.remove_range(&mut txn, pick, 1);
            }
        }
        Some(txn.encode_update_v1())
    }

    fn delete_first_block(&mut self) -> Option<Vec<u8>> {
        let doc = self.client();
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            return None;
        };
        if group.len(&txn) == 0 {
            return None;
        }
        group.remove_range(&mut txn, 0, 1);
        Some(txn.encode_update_v1())
    }

    /// L'état encodé se relit, et ce qui manque à chaque vecteur d'état passé aussi : trois fois,
    /// pour qu'une lecture de mémoire libérée ait de quoi se montrer.
    fn hammer(&self, stages: &[Vec<u8>], what: &str) {
        for _ in 0..3 {
            let state = self.page.encode_state();
            let read = reread(&state, self.stored);
            assert!(read.is_ok(), "{what} : l'état ne se relit pas : {read:?}");
            for stage in stages {
                let diff = self
                    .page
                    .encode_diff(stage)
                    .expect("ce qui manque s'encode");
                Update::decode_v1(&diff)
                    .unwrap_or_else(|e| panic!("{what} : ce qui manque est illisible : {e:?}"));
            }
        }
    }

    /// La page rechargée de son journal est la page vivante (Ruling R491).
    fn assert_sound(&self, what: &str) {
        let reloaded = self.reloaded();
        let again = reread(&reloaded.encode_state(), self.stored);
        assert!(again.is_ok(), "{what} : la page rechargée, {again:?}");
        assert_eq!(
            reloaded.read_blocks(),
            self.page.read_blocks(),
            "{what} : les blocs de la page rechargée"
        );
        assert_eq!(
            reloaded.revision(),
            self.page.revision(),
            "{what} : la révision de la page rechargée"
        );
    }
}

const KEYS: [&str; 6] = [
    "backgroundColor",
    "textColor",
    "textAlignment",
    "id",
    "bold",
    "level",
];

fn any_id(rng: &mut Rng, sv: &[(u64, u32)], me: (u64, u32)) -> (u64, u32) {
    match rng.below(10) {
        // Une unité à soi, juste avant.
        0 => (me.0, me.1.saturating_sub(1)),
        // Au-delà de ce que la page connaît.
        1 => {
            let (c, k) = sv[rng.below(sv.len() as u64) as usize];
            (c, k + rng.below(3) as u32)
        }
        _ => {
            let (c, k) = sv[rng.below(sv.len() as u64) as usize];
            (c, rng.below(u64::from(k.max(1))) as u32)
        }
    }
}

/// Un contenu de `len` unités si le genre le permet ; rend la longueur écrite.
fn content(rng: &mut Rng, out: &mut Vec<u8>, kind: u8, len: u32) -> u32 {
    match kind {
        // Supprimé.
        1 => {
            var(out, u64::from(len));
            len
        }
        // Chaîne, avec des paires de substitution.
        4 => {
            let mut s = String::new();
            let mut units = 0;
            while units < len {
                if len - units >= 2 && rng.chance(30) {
                    s.push('😀');
                    units += 2;
                } else {
                    s.push((b'a' + rng.below(26) as u8) as char);
                    units += 1;
                }
            }
            string(out, &s);
            len
        }
        // Valeurs `Any`.
        8 => {
            var(out, u64::from(len));
            for _ in 0..len {
                match rng.below(4) {
                    0 => {
                        out.push(119);
                        string(out, "red");
                    }
                    1 => {
                        out.push(119);
                        string(out, "blue");
                    }
                    2 => out.push(120),
                    _ => {
                        out.push(125);
                        out.push(3);
                    }
                }
            }
            len
        }
        // JSON.
        2 => {
            var(out, u64::from(len));
            for _ in 0..len {
                string(out, "\"red\"");
            }
            len
        }
        // Marque.
        6 => {
            string(out, "bold");
            string(out, "{}");
            1
        }
        // Type.
        7 => {
            if rng.chance(50) {
                out.push(6);
            } else {
                out.push(3);
                string(out, "paragraph");
            }
            1
        }
        // Objet incorporé.
        _ => {
            string(out, "\"x\"");
            1
        }
    }
}

/// Une mise à jour hostile, d'après ce que la page connaît (`sv`).
fn hostile(rng: &mut Rng, sv: &[(u64, u32)], astride_bias: u64) -> Vec<u8> {
    let mut u = Vec::new();
    let n_clients = if rng.chance(15) { 2 } else { 1 };
    var(&mut u, n_clients);
    let mut used: Vec<u64> = Vec::new();
    for _ in 0..n_clients {
        let (client, floor) = loop {
            let pick = if rng.chance(8) {
                (9_000 + rng.below(5), 0)
            } else {
                sv[rng.below(sv.len() as u64) as usize]
            };
            if !used.contains(&pick.0) {
                break pick;
            }
        };
        used.push(client);
        let astride = floor > 0 && rng.chance(astride_bias);
        let start = if astride {
            floor - 1 - rng.below(u64::from(floor.min(4))) as u32
        } else {
            match rng.below(10) {
                // Tout connu.
                0 if floor > 2 => floor - 2,
                // Un trou.
                1 => floor + 1 + rng.below(3) as u32,
                _ => floor,
            }
        };
        let n_structs = 1 + rng.below(3);
        let mut body = Vec::new();
        let mut clock = start;
        let mut written = 0u64;
        for n in 0..n_structs {
            let first_astride = astride && n == 0;
            let roll = rng.below(100);
            if !first_astride && roll < 6 {
                // Un trou de la mise à jour.
                body.push(10);
                let len = 1 + rng.below(3) as u32;
                var(&mut body, u64::from(len));
                clock += len;
                written += 1;
                continue;
            }
            if roll < 12 && (!first_astride || rng.chance(30)) {
                // Une plage ramassée.
                body.push(0);
                let len = if first_astride {
                    floor - clock + 1 + rng.below(3) as u32
                } else {
                    1 + rng.below(4) as u32
                };
                var(&mut body, u64::from(len));
                clock += len;
                written += 1;
                continue;
            }
            let kind: u8 = if first_astride {
                match rng.below(100) {
                    0..=39 => 1,
                    40..=69 => 4,
                    70..=94 => 8,
                    _ => 2,
                }
            } else {
                match rng.below(100) {
                    0..=29 => 1,
                    30..=54 => 4,
                    55..=74 => 8,
                    75..=77 => 2,
                    78..=85 => 6,
                    86..=93 => 7,
                    _ => 5,
                }
            };
            let len = if first_astride {
                floor - clock + 1 + rng.below(3) as u32
            } else {
                1 + rng.below(4) as u32
            };
            let me = (client, clock);
            let mut info = kind;
            let mut head = Vec::new();
            match rng.below(10) {
                // Parent nommé.
                0..=3 => {
                    if rng.chance(20) {
                        head.push(1);
                        string(&mut head, FRAGMENT);
                    } else {
                        head.push(0);
                        let id = any_id(rng, sv, me);
                        var(&mut head, id.0);
                        var(&mut head, u64::from(id.1));
                    }
                    if rng.chance(55) {
                        info |= 0x20;
                        string(&mut head, KEYS[rng.below(KEYS.len() as u64) as usize]);
                    }
                }
                // Une origine.
                4..=6 => {
                    info |= 0x80;
                    let id = any_id(rng, sv, me);
                    var(&mut head, id.0);
                    var(&mut head, u64::from(id.1));
                    if rng.chance(15) {
                        info |= 0x20;
                    }
                }
                // Une origine droite seule.
                7 => {
                    info |= 0x40;
                    let id = any_id(rng, sv, me);
                    var(&mut head, id.0);
                    var(&mut head, u64::from(id.1));
                    if rng.chance(15) {
                        info |= 0x20;
                    }
                }
                // Les deux.
                _ => {
                    info |= 0xC0;
                    for id in [any_id(rng, sv, me), any_id(rng, sv, me)] {
                        var(&mut head, id.0);
                        var(&mut head, u64::from(id.1));
                    }
                    if rng.chance(15) {
                        info |= 0x20;
                    }
                }
            }
            body.push(info);
            body.extend(head);
            clock += content(rng, &mut body, kind, len);
            written += 1;
        }
        var(&mut u, written);
        var(&mut u, client);
        var(&mut u, u64::from(start));
        u.extend(body);
    }
    // Le jeu de suppressions.
    if rng.chance(15) {
        let (c, k) = sv[rng.below(sv.len() as u64) as usize];
        if k > 0 {
            u.push(1);
            var(&mut u, c);
            u.push(1);
            let at = rng.below(u64::from(k)) as u32;
            var(&mut u, u64::from(at));
            var(&mut u, 1 + rng.below(u64::from(k - at)));
            return u;
        }
    }
    u.push(0);
    u
}

#[derive(Default)]
struct Tally {
    accepted: u64,
    wrote: u64,
    refused: BTreeMap<String, u64>,
    honest_refused: BTreeMap<String, u64>,
}

fn case(seed: u64, stored: bool, tally: &mut Tally) {
    let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ u64::from(stored));
    let mut served = Served::new(stored);
    let mut stages: Vec<Vec<u8>> = vec![served.page.state_vector()];
    // Un peu de page : des blocs, du texte, des attributs.
    for _ in 0..(3 + rng.below(5)) {
        if let Some(update) = served.honest(&mut rng) {
            served.receive(&update).expect("écriture honnête de départ");
        }
    }
    let steps = 6 + rng.below(10);
    for step in 0..steps {
        let what = format!("graine {seed}, pas {step}");
        if rng.chance(72) {
            let mut update = hostile(&mut rng, &served.sv(), 75);
            // Des octets abîmés : la lecture ne doit ni paniquer ni boucler.
            if rng.chance(25) {
                for _ in 0..(1 + rng.below(3)) {
                    let at = rng.below(update.len() as u64) as usize;
                    match rng.below(4) {
                        0 => update[at] = rng.below(256) as u8,
                        1 => update[at] |= 0x80,
                        2 => update.truncate(at.max(1)),
                        _ => update.insert(at, 0xFF),
                    }
                }
            }
            match served.receive(&update) {
                Ok(wrote) => {
                    tally.accepted += 1;
                    tally.wrote += u64::from(wrote);
                }
                Err(why) => {
                    let key: String = why
                        .chars()
                        .filter(|c| !c.is_ascii_digit())
                        .take(110)
                        .collect();
                    *tally.refused.entry(key).or_default() += 1;
                }
            }
        } else if let Some(update) = served.honest(&mut rng)
            && let Err(why) = served.receive(&update)
        {
            let key: String = why.chars().take(110).collect();
            *tally.honest_refused.entry(key).or_default() += 1;
        }
        served.hammer(&stages, &what);
        if stages.len() < 4 || rng.chance(20) {
            stages.push(served.page.state_vector());
            if stages.len() > 5 {
                stages.remove(1);
            }
        }
    }
    served.assert_sound(&format!("graine {seed}, fin"));
    // Les blocs partent un à un, des clients honnêtes écrivent : de la mémoire est libérée.
    for n in 0..8 {
        let what = format!("graine {seed}, suite {n}");
        if let Some(update) = served.delete_first_block() {
            let _ = served.receive(&update);
        }
        served.hammer(&stages, &what);
        if let Some(update) = served.honest(&mut rng) {
            let _ = served.receive(&update);
        }
        served.hammer(&stages, &what);
    }
    served.assert_sound(&format!("graine {seed}, après les suppressions"));
}

const CHILD: &str = "MEMENTUM_HOSTILE_CHILD";

fn number(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_default()
        .replace('\n', " ")
}

/// Fait taire les paniques du fils (le parent en rend le message), sauf si
/// `MEMENTUM_FUZZ_LOUD` est posé : pour lire la pile d'un cas, relancé seul
/// (`MEMENTUM_HOSTILE_CHILD=journal:518:1 MEMENTUM_FUZZ_LOUD=1 RUST_BACKTRACE=1`, sur
/// `child_cases --exact --nocapture`).
fn quiet() {
    if std::env::var("MEMENTUM_FUZZ_LOUD").is_err() {
        std::panic::set_hook(Box::new(|_| {}));
    }
}

/// Le processus fils : les cas demandés par le parent (`voie:premier:nombre`), un par ligne.
/// Sans la variable (un passage ordinaire de la suite), il ne fait rien.
#[test]
fn child_cases() {
    let Ok(order) = std::env::var(CHILD) else {
        return;
    };
    let mut parts = order.split(':');
    let lane = parts.next();
    if lane != Some("journal") && lane != Some("client") {
        return;
    }
    let stored = lane == Some("journal");
    let (Some(Ok(from)), Some(Ok(count))) = (
        parts.next().map(str::parse::<u64>),
        parts.next().map(str::parse::<u64>),
    ) else {
        return;
    };
    quiet();
    let out = std::io::stdout();
    // libtest a écrit « test child_cases ... » sans fin de ligne.
    writeln!(out.lock()).unwrap();
    let mut tally = Tally::default();
    for seed in from..from + count {
        writeln!(out.lock(), "S {seed}").unwrap();
        out.lock().flush().unwrap();
        if let Err(panic) = catch_unwind(AssertUnwindSafe(|| case(seed, stored, &mut tally))) {
            let why: String = message(&*panic).chars().take(600).collect();
            writeln!(out.lock(), "F graine {seed} : {why}").unwrap();
        }
    }
    writeln!(out.lock(), "T {} {}", tally.accepted, tally.wrote).unwrap();
    for (why, n) in &tally.refused {
        writeln!(out.lock(), "R {n} {why}").unwrap();
    }
    for (why, n) in &tally.honest_refused {
        writeln!(out.lock(), "H {n} {why}").unwrap();
    }
    out.lock().flush().unwrap();
}

/// Mène les cas d'une voie dans des processus fils, et rend les échecs.
fn run(stored: bool, default_seeds: u64) {
    let seeds = number("MEMENTUM_FUZZ_SEEDS", default_seeds);
    let first = number("MEMENTUM_FUZZ_FROM", 1);
    let lane = if stored { "journal" } else { "client" };
    let (mut accepted, mut wrote, mut crashes) = (0u64, 0u64, 0u64);
    let mut refused: BTreeMap<String, u64> = BTreeMap::new();
    let mut honest_refused: BTreeMap<String, u64> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    let mut from = first;
    let end = first + seeds;
    while from < end {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["child_cases", "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, format!("{lane}:{from}:{}", end - from))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .expect("le processus fils se lance");
        let text = String::from_utf8_lossy(&output.stdout);
        let mut last = None;
        let mut finished = false;
        for line in text.lines() {
            let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
            match kind {
                "S" => last = rest.parse::<u64>().ok(),
                "F" => failures.push(rest.to_string()),
                "T" => {
                    let mut numbers = rest.split(' ').map(|n| n.parse::<u64>().unwrap_or(0));
                    accepted += numbers.next().unwrap_or(0);
                    wrote += numbers.next().unwrap_or(0);
                    finished = true;
                }
                "R" | "H" => {
                    let (n, why) = rest.split_once(' ').unwrap_or((rest, ""));
                    let into = if kind == "R" {
                        &mut refused
                    } else {
                        &mut honest_refused
                    };
                    *into.entry(why.to_string()).or_default() += n.parse::<u64>().unwrap_or(0);
                }
                _ => {}
            }
        }
        if finished {
            break;
        }
        // Le fils s'est arrêté en route : une violation d'accès, une allocation impossible.
        let seed = last.expect("le fils a commencé un cas");
        crashes += 1;
        failures.push(format!(
            "graine {seed} : ARRÊT DU PROCESSUS ({})",
            output.status
        ));
        from = seed + 1;
        // Au-delà, ce n'est plus un rapport : la garde est ouverte.
        if crashes >= 20 {
            break;
        }
    }
    println!(
        "ESSAI voie {lane} : {seeds} cas à partir de {first} ; hostiles acceptées {accepted} \
         (dont {wrote} ont écrit) ; échecs {} dont {crashes} arrêts du processus",
        failures.len()
    );
    for (why, n) in &refused {
        println!("  refus {n:>7} : {why}");
    }
    for (why, n) in &honest_refused {
        println!("  HONNÊTE REFUSÉ {n:>5} : {why}");
    }
    for failure in failures.iter().take(20) {
        println!("ÉCHEC {failure}");
    }
    assert!(
        failures.is_empty(),
        "{} échecs, voie {lane} ; les premiers : {:?}",
        failures.len(),
        failures.iter().take(5).collect::<Vec<_>>()
    );
    assert!(
        honest_refused.is_empty(),
        "une écriture honnête refusée : {honest_refused:?}"
    );
}

#[test]
fn hostile_updates_at_random_from_a_client_leave_a_sound_page() {
    run(false, 1_500);
}

#[test]
fn hostile_entries_at_random_in_the_journal_leave_a_sound_page() {
    run(true, 1_500);
}

// --- Les cas trouvés par l'essai, réduits. ---

/// Rejoue une trace ; rend les écarts (aucun : la page est saine à chaque pas).
fn replay(name: &str, stored: bool) -> Vec<String> {
    let path = format!(
        "{}/tests/fixtures/hostile/{name}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
    replay_text(&text, stored)
}

/// Réduit une trace (`MEMENTUM_HOSTILE_CHILD=minimize:<voie>:<fichier>`, sur
/// `child_minimize --exact --nocapture`) : chaque ligne est retirée tour à tour, et le reste
/// gardé s'il montre encore un écart (trois essais : une lecture de mémoire libérée ne se montre
/// pas à chaque passage). Écrit `<fichier>.min`. Pour obtenir la trace d'une graine :
/// `MEMENTUM_FUZZ_TRACE=<fichier> MEMENTUM_HOSTILE_CHILD=client:<graine>:1` sur `child_cases`.
#[test]
fn child_minimize() {
    let Ok(order) = std::env::var(CHILD) else {
        return;
    };
    let Some(rest) = order.strip_prefix("minimize:") else {
        return;
    };
    let (lane, path) = rest.split_once(':').unwrap();
    let stored = lane == "journal";
    quiet();
    let fails = |lines: &[&str]| {
        let text = lines.join("\n");
        (0..3).any(|_| {
            catch_unwind(AssertUnwindSafe(|| replay_text(&text, stored)))
                .map_or(true, |findings| !findings.is_empty())
        })
    };
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    assert!(fails(&lines), "la trace ne montre aucun écart");
    let mut at = lines.len();
    while at > 1 {
        at -= 1;
        let mut without = lines.clone();
        without.remove(at);
        if fails(&without) {
            lines = without;
        }
    }
    std::fs::write(format!("{path}.min"), lines.join("\n") + "\n").unwrap();
    println!("\nréduite à {} lignes", lines.len());
}

fn replay_text(text: &str, stored: bool) -> Vec<String> {
    let mut served: Option<Served> = None;
    let mut findings = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line == "R" {
            let served = served.as_mut().unwrap();
            served.page = served.reloaded();
            continue;
        }
        let bytes: Vec<u8> = serde_json::from_str(line.strip_prefix("C ").unwrap()).unwrap();
        let Some(served) = served.as_mut() else {
            served = Some(Served::from_creation(bytes, stored));
            continue;
        };
        let _ = served.receive(&bytes);
        let sv = served.page.state_vector();
        let mut read = reread(&served.page.encode_state(), stored);
        for _ in 0..40 {
            if read.is_err() {
                break;
            }
            read = reread(&served.page.encode_state(), stored);
            let _ = served.page.encode_diff(&sv);
            let _ = served.page.read_blocks();
        }
        let reloaded = served.reloaded();
        let blocks = reloaded.read_blocks() == served.page.read_blocks();
        let revision = reloaded.revision() == served.page.revision();
        if read.is_err() || !blocks || !revision {
            findings.push(format!(
                "ligne {n} : {}, blocs pareils {blocks}, révision pareille {revision}",
                read.err()
                    .map_or_else(
                        || "l'état se relit".to_string(),
                        |why| format!("l'état {why}")
                    )
                    .chars()
                    .filter(|c| !c.is_ascii_digit())
                    .take(70)
                    .collect::<String>()
            ));
        }
    }
    findings
}

/// Rejoue une trace dans un processus fils (elle peut arrêter le processus) : `Err` dit
/// pourquoi la page n'est pas saine.
fn replay_apart(name: &str, stored: bool) -> Result<(), String> {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["child_trace", "--exact", "--nocapture", "--test-threads=1"])
        .env(
            CHILD,
            format!("{}:{name}", if stored { "journal" } else { "client" }),
        )
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .expect("le processus fils se lance");
    let text = String::from_utf8_lossy(&output.stdout);
    let mut findings: Vec<&str> = text.lines().filter_map(|l| l.strip_prefix("F ")).collect();
    findings.sort_unstable();
    findings.dedup();
    if !text.lines().any(|l| l == "T") {
        return Err(format!("ARRÊT DU PROCESSUS ({})", output.status));
    }
    if findings.is_empty() {
        Ok(())
    } else {
        Err(findings.join(" ; "))
    }
}

/// Le processus fils d'une trace (`voie:nom`), rejouée six fois : une lecture de mémoire libérée
/// ne se montre pas à chaque passage.
#[test]
fn child_trace() {
    let Ok(order) = std::env::var(CHILD) else {
        return;
    };
    let Some((lane, name)) = order.split_once(':') else {
        return;
    };
    if lane == "minimize" || name.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return;
    }
    quiet();
    let out = std::io::stdout();
    writeln!(out.lock()).unwrap();
    for _ in 0..6 {
        match catch_unwind(AssertUnwindSafe(|| replay(name, lane == "journal"))) {
            Ok(findings) => {
                for finding in findings {
                    writeln!(out.lock(), "F {finding}").unwrap();
                }
            }
            Err(panic) => writeln!(out.lock(), "F panique : {}", message(&*panic)).unwrap(),
        }
    }
    writeln!(out.lock(), "T").unwrap();
    out.lock().flush().unwrap();
}

/// C1 (re-relecture 5) : une entrée de map dont le contenu `Any` porte plusieurs valeurs, une
/// valeur supprimée, puis le bloc. Graines 22330 et 23097 de l'essai.
#[test]
fn a_many_valued_attribute_then_one_value_deleted_then_the_block() {
    assert_eq!(replay_apart("min-22330", false), Ok(()));
    assert_eq!(replay_apart("min-23097", false), Ok(()));
}

/// C1, par un élément à cheval (graine 4767, forme directe).
#[test]
fn a_many_valued_attribute_placed_by_its_origin() {
    assert_eq!(replay_apart("trace-4767-direct", false), Ok(()));
}

/// I1 : au journal, un trou dans l'entrée ne cache rien de ce qu'elle pose au-delà.
#[test]
fn a_hole_in_a_stored_entry_hides_nothing_of_what_it_puts_beyond() {
    assert_eq!(replay_apart("var-nohole", true), Ok(()));
    assert_eq!(replay_apart("var-hole", true), Ok(()));
    assert_eq!(replay_apart("trace-j17040-inputs", true), Ok(()));
    assert_eq!(replay_apart("min-j55405", true), Ok(()));
}

/// I2 : un élément dont l'origine droite est son origine ; la page vivante reste son journal.
#[test]
fn an_item_whose_right_origin_is_its_origin_leaves_the_page_its_journal() {
    assert_eq!(replay_apart("min-22468", false), Ok(()));
}

/// m1 : un contenu supprimé placé par sa seule origine droite ; la révision ne change pas au
/// rechargement.
#[test]
fn a_deleted_content_placed_by_its_right_origin_keeps_the_revision() {
    assert_eq!(replay_apart("min-8251", false), Ok(()));
}

/// Graine 3346 de l'essai (tour 4) : une chaîne de plusieurs unités posée comme attribut d'un
/// texte, sa clé nommée ; une unité du milieu supprimée, puis le texte. La forme de C1, par un
/// contenu qui n'est pas une valeur, sur un type dont la garde des attributs ne juge rien.
#[test]
fn a_string_set_as_an_attribute_of_a_text_then_cut_in_the_middle() {
    assert_eq!(replay_apart("min-3346", false), Ok(()));
}

/// Ruling R804, en test nommé (constat I-6.2 de la re-relecture 6 : l'essai ne la voyait qu'à
/// partir de 2 171 cas). Graine 2171 de la voie du journal : jugé sur l'encodage de la seule
/// transaction, ce que l'entrée posait au-delà d'un trou n'était pas vu, et l'état ne se relisait
/// plus après la suppression d'un bloc (ou le processus s'arrêtait).
#[test]
fn what_a_stored_entry_puts_beyond_a_hole_is_judged() {
    assert_eq!(replay_apart("min-j2171", true), Ok(()));
}

/// Ruling R808, en test nommé (l'essai ne la voyait qu'à partir de 5 401 cas). Graine 5401 de la
/// voie du journal : une horloge au-delà de 2^31 ; yrs paniquait (`unwrap` sur `None`).
#[test]
fn a_stored_clock_beyond_two_to_the_31_leaves_a_sound_page() {
    assert_eq!(replay_apart("min-j5401", true), Ok(()));
}

/// Ruling R802 sans son filet L1, en test nommé (l'essai ne voyait N2 et L1 retirées ensemble qu'à
/// partir de 38 953 cas). Graine 38953 de la voie d'un client : un élément dont l'origine droite
/// est son origine ; sans la récriture ni le refus, la page rechargée n'était plus la page vivante.
#[test]
fn an_item_whose_right_origin_is_its_origin_reloads_as_it_lives() {
    assert_eq!(replay_apart("min-38953", false), Ok(()));
}

/// Les entrées d'une trace, telles quelles.
fn entries(name: &str) -> Vec<Vec<u8>> {
    let path = format!(
        "{}/tests/fixtures/hostile/{name}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path} : {e}"))
        .lines()
        .filter_map(|line| line.strip_prefix("C "))
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Constat C-6.1 de la re-relecture 6 (graines 60530, 8618 et 72782 de l'essai élargi, voie du
/// journal) : un contenu JSON relu du journal dans un texte. yrs 0.28.1 l'écrit en `len` chaînes et
/// le relit en `len + 1` : l'état de la page ne se relisait plus, la compaction suivante l'écrivait
/// comme instantané à la place du journal, et yrs paniquait dans `encode_state` quand la page
/// gardait des suppressions en attente (72782).
#[test]
fn a_json_content_read_from_the_journal_leaves_a_readable_state() {
    assert_eq!(replay_apart("min-j60530", true), Ok(()));
    assert_eq!(replay_apart("min-j8618", true), Ok(()));
    assert_eq!(replay_apart("min-j72782", true), Ok(()));
}

/// Ruling R815 : le contenu JSON qu'une entrée du journal intègre est supprimé dans la transaction
/// qui l'intègre (le ramasse-miettes en fait un contenu supprimé). L'état de la page se relit par
/// un yrs neuf, la page se recharge de son propre état et de son journal, et la version qu'on en
/// couperait se sert ([`PageDoc::served_state`]), suppressions en attente comprises (72782).
#[test]
fn a_json_content_read_from_the_journal_is_deleted_with_its_entry() {
    let mut pending = false;
    for name in ["min-j60530", "min-j8618", "min-j72782"] {
        let journal = entries(name);
        let page = PageDoc::from_updates(journal.iter().map(Vec::as_slice))
            .unwrap_or_else(|e| panic!("{name} : le journal ne se relit pas : {e}"));
        pending |= page.has_pending_deletions();
        let state = page.encode_state();
        assert!(
            Update::decode_v1(&state).is_ok(),
            "{name} : l'état ne se relit pas"
        );
        let again = PageDoc::from_updates([state.as_slice()])
            .unwrap_or_else(|e| panic!("{name} : la page ne se recharge pas de son état : {e}"));
        assert_eq!(again.read_blocks(), page.read_blocks(), "{name}");
        assert_eq!(again.revision(), page.revision(), "{name}");
        assert!(
            PageDoc::served_state(&state).is_some(),
            "{name} : une version de cet état ne se servirait pas"
        );
        assert!(reread(&state, true).is_ok(), "{name}");
    }
    assert!(pending, "aucune trace ne garde de suppression en attente");
}
