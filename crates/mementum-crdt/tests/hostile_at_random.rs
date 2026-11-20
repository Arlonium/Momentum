//! Essai au hasard hostile (garde des mises à jour, tour 4 ; Ruling R800).
//!
//! yrs 0.28.1 n'est pas sûr en mémoire devant une mise à jour hostile : la sûreté du serveur
//! repose sur la garde, et la garde sur cet essai. Des mises à jour tirées au hasard (tous
//! contenus, du JSON écrit comme Yjs l'écrit ou comme yrs le lit, des octets, des sous-documents,
//! un déplacement de yrs, toutes les références de type ; parents, entrées posées dans le type que
//! la mise à jour vient de poser, origines, origines droites, clés, longueurs, éléments à cheval
//! sur ce que la page connaît, `Skip`, plages ramassées, deux clients, suppressions de plusieurs
//! clients et plusieurs plages, octets abîmés), mêlées d'écritures honnêtes, arrivent à une page
//! vivante par le chemin de l'acteur : d'un client (`apply_client_update`, ce qu'il rend va au
//! journal) ou relues comme un vieux journal (`apply_update`, les octets tels quels, une entrée
//! refusée étant écartée). Après chaque pas : l'invariant de la classe C1 tient dans l'état
//! (chaque entrée de map vivante est la seule vivante de sa clé, et n'a qu'une unité : Ruling
//! R817), l'état encodé se relit par un yrs neuf, ce qui manque à chaque vecteur d'état passé
//! aussi ; à la fin, puis après avoir supprimé les blocs un à un (de la mémoire est libérée) : la
//! page rechargée du journal est la page vivante (blocs, révision).
//!
//! Chaque mise à jour que la page a gardée est aussi donnée, telle que yrs l'a reçue, au yrs du
//! serveur et à celui de crates.io, à la suite de l'état de la page d'avant : ils doivent en faire
//! la même chose (`common/twins.rs`, Ruling R875). Dans le passage ordinaire de la suite, pour un
//! cas sur quatre ; dans un essai élargi, pour tous.
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

#[path = "common/live.rs"]
mod live;
#[path = "common/twins.rs"]
mod twins;

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
    /// Voie du journal : une entrée relue a laissé un trou d'horloge (un journal d'avant le
    /// Ruling R131). La page relue de son état peut alors ne pas être la page vivante : limite
    /// écrite (Ruling R830, `known_limits`), l'oracle ne la juge plus.
    holed: bool,
    /// La longueur du journal et l'état de la page à chaque pas : ce que relirait une page compactée
    /// à ce pas ([`Served::tail_check`]).
    marks: Vec<(usize, Vec<u8>)>,
    /// Les mises à jour gardées sur lesquelles les deux yrs ont été comparés.
    compared: twins::Compared,
    /// Faux pour ne pas les comparer dans ce cas (voir [`case`]).
    watched: bool,
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
            holed: false,
            marks: Vec::new(),
            compared: twins::Compared::default(),
            watched: true,
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
            holed: false,
            marks: Vec::new(),
            compared: twins::Compared::default(),
            watched: true,
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
            self.holed |= self.page.stored_update_leaves_gap(update) == Ok(true);
            let answer = if self.watched {
                twins::watching("une entrée relue", &mut self.compared, || {
                    self.page.apply_update(update)
                })
            } else {
                self.page.apply_update(update)
            };
            return match answer {
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
        let answer = if self.watched {
            twins::watching("une mise à jour reçue", &mut self.compared, || {
                self.page.apply_client_update(update)
            })
        } else {
            self.page.apply_client_update(update)
        };
        match answer {
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

    /// Le serveur récrit la page par différence (`PageDoc::write_diff`, Task S2 du lot 5 ;
    /// relecture de S2, I4) : les blocs qu'il en lit, retouchés. C'est la première écriture du
    /// serveur qui modifie sur place ce que des clients ont écrit, et elle ne passe pas par la
    /// lecture de la page : sa mise à jour est donnée aux deux yrs à la suite de l'état d'avant
    /// (`twins::written_alike`), puis écrite au journal comme l'écrirait l'acteur ; après une
    /// erreur, la page est rechargée. Elle est tracée comme une mise à jour reçue.
    fn diff_write(&mut self, rng: &mut Rng) -> Result<bool, String> {
        // Une suppression en attente peut nommer les horloges à venir du client de cette page : une
        // entrée relue du journal (un journal d'avant R198) que lit la page même qui a créé la page.
        // L'écriture du serveur prendrait ces horloges, et la page relue de son état les
        // supprimerait (vu par cet essai : graines 533, 923, 981, 1007 de la voie du journal, par
        // l'écriture par différence comme par la récriture complète). L'acteur qui écrit a chargé la
        // page de son journal, sous un client tiré au hasard, qu'aucune entrée ne nomme, et aucune
        // mise à jour de client ne laisse plus de suppression en attente (R198) : la page est
        // rechargée avant d'écrire, comme l'acteur (relecture de S2, R1299).
        if self.page.has_pending_deletions() {
            self.page = self.reloaded();
        }
        let mut blocks = self.page.read_blocks();
        retouch(&mut blocks, rng);
        let state = self.page.encode_state();
        match self.page.write_diff(&blocks) {
            Ok(written) if written.update == [0, 0] => Ok(false),
            Ok(written) => {
                trace(&written.update);
                if self.watched {
                    twins::written_alike(
                        "une écriture par différence",
                        &mut self.compared,
                        &state,
                        &written.update,
                    );
                }
                self.journal.push(written.update);
                Ok(true)
            }
            Err(error) => {
                self.page = self.reloaded();
                Err(error.to_string())
            }
        }
    }

    /// Ce que relit le chargement d'une page compactée à un point quelconque de la scène :
    /// l'instantané, puis la fin du journal. Elle se charge, et elle est la page vivante (constat C5
    /// de la re-relecture 8, Rulings R858, R865).
    fn tail_check(&mut self, what: &str) {
        if self.holed {
            return;
        }
        self.marks
            .push((self.journal.len(), self.page.encode_state()));
        for (len, state) in &self.marks {
            let tail = &self.journal[*len..];
            let again = PageDoc::from_updates(
                std::iter::once(state.as_slice()).chain(tail.iter().map(Vec::as_slice)),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{what} : l'instantané de l'entrée {len} et les {} suivantes ne se chargent pas : {error}",
                    tail.len()
                )
            });
            assert!(
                live::tree(self.page.yrs_doc()) == live::tree(again.yrs_doc())
                    && self.page.revision() == again.revision(),
                "{what} : l'instantané de l'entrée {len} et les {} suivantes ne relisent pas la page vivante",
                tail.len()
            );
        }
    }

    /// L'état encodé se relit, et ce qui manque à chaque vecteur d'état passé aussi : trois fois,
    /// pour qu'une lecture de mémoire libérée ait de quoi se montrer. Et l'invariant de la classe
    /// C1 tient (Ruling R817) : chaque entrée de map vivante est la seule vivante de sa clé, et n'a
    /// qu'une unité ; vu dans l'état, avant que yrs ne lise de la mémoire libérée. Et la page relue
    /// de son propre état (ce qu'écrit la compaction, ce que reçoit un onglet à l'étape 2) est la
    /// page vivante : blocs, arbre, révision (constat I-7.1, Ruling R825). Et ce que la garde tient
    /// pour vrai de chaque unité (son type, ou qu'elle est ramassée) est ce que yrs en garde (Ruling
    /// R853).
    fn hammer(&self, stages: &[Vec<u8>], what: &str) {
        if let Some(why) = self.page.unsound_map_entries() {
            panic!("{what} : {why}");
        }
        if let Some(why) = self.page.misjudged_units() {
            panic!("{what} : {why}");
        }
        if !self.holed
            && let Some(why) = live::reloaded_from_state_differs(&self.page)
        {
            panic!("{what} : {why}");
        }
        // Ruling R862 : la page relue repasse par les règles ; un lecteur sans garde, non.
        if !self.holed
            && let Some(why) = live::bare_reader_differs(&self.page)
        {
            panic!("{what} : {why}");
        }
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

    /// Écrit la page servie à la suite du fichier `MEMENTUM_FUZZ_SERVED`, s'il est donné : le vrai
    /// Yjs la relit (`apps/web/src/editor/servedStates.test.ts`, Ruling R862). Pas une page à trou
    /// d'horloge (Ruling R830).
    fn export(&self, seed: u64) {
        let Some(path) = std::env::var_os("MEMENTUM_FUZZ_SERVED") else {
            return;
        };
        if self.holed {
            return;
        }
        let state: String = self
            .page
            .encode_state()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let line = serde_json::json!({
            "seed": seed,
            "stored": self.stored,
            "state": state,
            "outline": live::outline(self.page.yrs_doc()),
        });
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(file, "{line}").unwrap();
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
fn content(rng: &mut Rng, out: &mut Vec<u8>, kind: u8, len: u32, wide: bool) -> u32 {
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
        // JSON : écrit comme Yjs l'écrit (`len` chaînes), que yrs lit avec une chaîne de trop, ou
        // comme yrs le lit (`len - 1`, puis `len` chaînes), qu'il intègre (constat C-6.1 de la
        // re-relecture 6 : l'essai ne tirait jamais un JSON que yrs intègre).
        2 => {
            var(out, u64::from(if rng.chance(50) { len - 1 } else { len }));
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
        // Type : un texte ou un paragraphe ; au large, toutes les références que la garde lit
        // (constat I-6.2 de la re-relecture 6).
        7 => {
            if !wide {
                if rng.chance(50) {
                    out.push(6);
                } else {
                    out.push(3);
                    string(out, "paragraph");
                }
                return 1;
            }
            match rng.below(12) {
                0..=3 => out.push(6),
                4..=6 => {
                    out.push(3);
                    let tag =
                        ["paragraph", "blockContainer", "blockGroup", "x"][rng.below(4) as usize];
                    string(out, tag);
                }
                7 => out.push(0),
                8 => out.push(1),
                9 => out.push(2),
                10 => out.push(4),
                _ => out.push([5u8, 9, 15, 7][rng.below(4) as usize]),
            }
            1
        }
        // Octets.
        3 => {
            let n = rng.below(5);
            var(out, n);
            for _ in 0..n {
                out.push(rng.below(256) as u8);
            }
            1
        }
        // Sous-document : un guid, puis ses options (une valeur `Any`).
        9 => {
            let guid = ["g", "0196f0a0-0000-7000-8000-000000000001", ""][rng.below(3) as usize];
            string(out, guid);
            match rng.below(3) {
                0 => out.push(127),
                1 => {
                    out.push(118);
                    var(out, 1);
                    string(out, "gc");
                    out.push(120);
                }
                _ => {
                    out.push(119);
                    string(out, "x");
                }
            }
            1
        }
        // Déplacement de yrs : la garde le refuse à la lecture.
        11 => {
            var(out, 0);
            var(out, 1);
            var(out, 1);
            1
        }
        // Objet incorporé.
        _ => {
            string(out, "\"x\"");
            1
        }
    }
}

/// Une mise à jour hostile, d'après ce que la page connaît (`sv`). Une sur deux est tirée **au
/// large** (constat I-6.2 de la re-relecture 6) : jusqu'à cinq éléments par client, des octets,
/// des sous-documents, un déplacement de yrs, toutes les références de type, des entrées posées
/// dans le type que la mise à jour vient de poser, des suppressions de plusieurs clients et
/// plusieurs plages. Ces formes sont presque toutes refusées : tirées seules, elles divisaient
/// par deux le nombre de mises à jour acceptées, donc ce que l'essai éprouve après la lecture.
fn hostile(rng: &mut Rng, sv: &[(u64, u32)], astride_bias: u64) -> Vec<u8> {
    let wide = rng.chance(50);
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
        let n_structs = 1 + rng.below(if wide { 5 } else { 3 });
        let mut body = Vec::new();
        let mut clock = start;
        let mut written = 0u64;
        // Le dernier type que la mise à jour pose : une entrée suivante peut s'y poser.
        let mut last_type: Option<(u64, u32)> = None;
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
            } else if wide {
                match rng.below(100) {
                    0..=19 => 1,
                    20..=37 => 4,
                    38..=52 => 8,
                    53..=57 => 2,
                    58..=64 => 6,
                    65..=79 => 7,
                    80..=85 => 5,
                    86..=91 => 3,
                    92..=97 => 9,
                    _ => 11,
                }
            } else {
                match rng.below(100) {
                    0..=29 => 1,
                    30..=54 => 4,
                    55..=74 => 8,
                    75..=78 => 2,
                    79..=85 => 6,
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
                    if let Some(t) = last_type.filter(|_| wide && rng.chance(60)) {
                        // Dans le type que la mise à jour vient de poser.
                        head.push(0);
                        var(&mut head, t.0);
                        var(&mut head, u64::from(t.1));
                    } else if rng.chance(20) {
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
            if kind == 7 {
                last_type = Some((client, clock));
            }
            clock += content(rng, &mut body, kind, len, wide);
            written += 1;
        }
        var(&mut u, written);
        var(&mut u, client);
        var(&mut u, u64::from(start));
        u.extend(body);
    }
    // Le jeu de suppressions : une plage d'un client ; au large, un ou deux clients, une à trois
    // plages chacun, parfois au-delà de ce que la page a.
    if !wide && rng.chance(15) {
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
    if wide && rng.chance(25) {
        let mut picked: Vec<(u64, u32)> = Vec::new();
        for _ in 0..(1 + rng.below(2)) {
            let p = sv[rng.below(sv.len() as u64) as usize];
            if p.1 > 0 && !picked.iter().any(|q| q.0 == p.0) {
                picked.push(p);
            }
        }
        if !picked.is_empty() {
            var(&mut u, picked.len() as u64);
            for (c, k) in picked {
                var(&mut u, c);
                let n = 1 + rng.below(3);
                var(&mut u, n);
                let mut at = 0u32;
                for _ in 0..n {
                    at += rng.below(u64::from(k)) as u32;
                    let len = 1 + rng.below(u64::from(k + 2)) as u32;
                    var(&mut u, u64::from(at.min(k + 1)));
                    var(&mut u, u64::from(len));
                }
            }
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
    /// Les mises à jour gardées sur lesquelles les deux yrs ont été comparés.
    compared: twins::Compared,
}

/// Retouche les blocs d'une page pour une écriture par différence : un paragraphe ajouté, un
/// texte allongé, un bloc retiré, le dernier remonté en tête, le gras d'une plage basculé, une
/// mention en tête d'un contenu (qui le fait récrire en entier).
fn retouch(blocks: &mut Vec<mementum_md::Block>, rng: &mut Rng) {
    use mementum_md::{Content, Inline};
    fn first_text(blocks: &mut [mementum_md::Block]) -> Option<&mut Vec<Inline>> {
        blocks.iter_mut().find_map(|b| match &mut b.content {
            Content::Inline(content) => Some(content),
            _ => None,
        })
    }
    let n = blocks.len();
    match rng.below(6) {
        0 => blocks.push(
            mementum_md::parse("Écrit par le serveur\n")
                .unwrap()
                .remove(0),
        ),
        1 => {
            if let Some(content) = first_text(blocks) {
                content.push(Inline::Text {
                    text: "plus".into(),
                    styles: Default::default(),
                });
            }
        }
        2 if n > 1 => {
            blocks.remove(rng.below(n as u64) as usize);
        }
        3 if n > 1 => {
            let last = blocks.pop().unwrap();
            blocks.insert(0, last);
        }
        4 => {
            if let Some(Inline::Text { styles, .. }) = first_text(blocks)
                .and_then(|c| c.iter_mut().find(|i| matches!(i, Inline::Text { .. })))
            {
                styles.bold = !styles.bold;
            }
        }
        _ => {
            if let Some(content) = first_text(blocks) {
                content.insert(
                    0,
                    Inline::Mention {
                        kind: "page".into(),
                        id: "pg_x".into(),
                        label: "x".into(),
                    },
                );
            }
        }
    }
}

fn case(seed: u64, stored: bool, tally: &mut Tally) {
    let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ u64::from(stored));
    let mut writes = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1FF ^ u64::from(stored));
    let mut served = Served::new(stored);
    // Comparer les deux yrs coûte quatre fois le cas lui-même : dans le passage ordinaire de la
    // suite, un cas sur quatre ; dans un essai élargi (`MEMENTUM_FUZZ_SEEDS`), tous.
    served.watched = std::env::var_os("MEMENTUM_FUZZ_SEEDS").is_some() || seed.is_multiple_of(4);
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
        // Une écriture du serveur par différence, un pas sur quatre (relecture de S2, I4). Tirée
        // d'un générateur à elle : les pas hostiles et honnêtes de chaque graine restent ceux
        // d'avant, tant qu'aucune écriture ne les a précédés.
        if writes.chance(25)
            && let Err(why) = served.diff_write(&mut writes)
        {
            let key: String = why.chars().take(110).collect();
            *tally.honest_refused.entry(key).or_default() += 1;
        }
        served.hammer(&stages, &what);
        served.tail_check(&what);
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
        served.tail_check(&what);
    }
    served.assert_sound(&format!("graine {seed}, après les suppressions"));
    tally.compared.updates += served.compared.updates;
    tally.compared.retried += served.compared.retried;
    served.export(seed);
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
    writeln!(
        out.lock(),
        "T {} {} {} {}",
        tally.accepted,
        tally.wrote,
        tally.compared.updates,
        tally.compared.retried
    )
    .unwrap();
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
    let (mut compared, mut retried) = (0u64, 0u64);
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
                    compared += numbers.next().unwrap_or(0);
                    retried += numbers.next().unwrap_or(0);
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
         (dont {wrote} ont écrit) ; {compared} mises à jour gardées, comparées dans les deux yrs (dont {retried} \
         où yrs ne fait pas deux fois la même chose) ; \
         échecs {} dont {crashes} arrêts du processus",
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
    assert!(compared > seeds, "les deux yrs n'ont pas été comparés");
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
        served.tail_check(&format!("ligne {n}"));
        if let Some(why) = served.page.unsound_map_entries() {
            let why: String = why.chars().filter(|c| !c.is_ascii_digit()).collect();
            findings.push(format!("ligne {n} : {why}"));
        }
        // Ruling R853 : ce que la garde tient pour vrai de chaque unité est ce que yrs en garde.
        if let Some(why) = served.page.misjudged_units() {
            let why: String = why.chars().filter(|c| !c.is_ascii_digit()).collect();
            findings.push(format!("ligne {n} : {why}"));
        }
        // Ruling R825 : la page relue de son propre état est la page vivante (hors d'une page à
        // trou, limite écrite : Ruling R830).
        if !served.holed
            && let Some(why) = live::reloaded_from_state_differs(&served.page)
        {
            let why: String = why.chars().take(70).collect();
            findings.push(format!("ligne {n} : {why}"));
        }
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

/// Ruling R818 : graine 776 de l'essai élargi, voie du journal, vue par l'invariant de la classe
/// C1 (R817). Une valeur `Any` de deux valeurs, sans la marque d'une entrée de map, placée par
/// l'origine de l'attribut `id` d'un bloc : yrs en fait une entrée de map. Un second élément de la
/// même entrée a pour origine sa première valeur : yrs la coupe dans la transaction, et chaque
/// morceau passait pour une entrée d'une unité. Les deux restaient vivants pour une même clé.
#[test]
fn a_many_valued_entry_cut_by_an_item_of_the_same_entry_is_deleted_whole() {
    assert_eq!(replay_apart("min-j776", true), Ok(()));
}

/// Constat I-7.1 de la re-relecture 7 (Ruling R825), vu par l'oracle neuf de l'essai : la page
/// relue de son propre état n'est pas la page vivante. Graine 1543 de la voie d'un client, réduite
/// à la main : une mise à jour à cheval, que la coupe (R784) récrit en un élément « zy »
/// d'origine « c » et d'origine droite « b », dans « abc » d'un tenant (un seul bloc de yrs). La
/// page vivante lisait « azybc », la page relue de son état « abczy », sous la même révision.
/// Graines 280, 1511 et 2657 de la voie du journal : la même forme.
#[test]
fn a_right_origin_left_of_the_origin_in_one_block_reloads_as_it_lives() {
    assert_eq!(replay_apart("min-1543", false), Ok(()));
    assert_eq!(replay_apart("min-j280", true), Ok(()));
    assert_eq!(replay_apart("min-j1511", true), Ok(()));
    assert_eq!(replay_apart("min-j2657", true), Ok(()));
}

/// Ruling R827 (règle G), vu par l'oracle de R825 : un élément dont un voisin est une plage
/// ramassée et l'autre un élément. yrs le rangeait par l'autre voisin (Yjs le ramasse), et sa place
/// dépendait de l'ordre d'arrivée : la page relue de son état ne le rangeait pas pareil. Graine
/// 1550 de la voie du journal (une plage ramassée posée par l'entrée, puis un texte d'origine
/// dedans et d'origine droite « c »), 15795 et 22948 de la voie d'un client (l'origine dans un
/// bloc supprimé, ramassé, l'origine droite dans un texte vivant).
#[test]
fn an_item_with_one_collected_neighbour_is_collected() {
    assert_eq!(replay_apart("min-j1550", true), Ok(()));
    assert_eq!(replay_apart("min-15795", false), Ok(()));
    assert_eq!(replay_apart("min-22948", false), Ok(()));
}

/// Ruling R828 (règle I1) : un élément sans origine dont l'origine droite en a une. Graine 14050
/// de la voie du journal (un texte sans origine, d'origine droite une plage du milieu du texte) et
/// 25235 de la voie d'un client (un sous-document sans origine, d'origine droite « z », au milieu).
#[test]
fn an_origin_less_item_before_a_placed_unit_is_collected() {
    assert_eq!(replay_apart("min-j14050", true), Ok(()));
    assert_eq!(replay_apart("min-25235", false), Ok(()));
}

/// Ruling R829 (règle P2) : un élément dont l'origine et l'origine droite sont dans deux parents ;
/// la page le range dans le parent de gauche, et quand ce voisin est ramassé plus tard, la page
/// relue de son état le rangeait dans l'autre. Graine 14050 de la voie du journal, réduite de
/// nouveau une fois G et I1 posées.
#[test]
fn an_item_between_two_parents_is_collected() {
    assert_eq!(replay_apart("min-j14050-b", true), Ok(()));
}

/// Ruling R856 (règle V, qui remplace la règle I3 de R834) : un élément d'origine « b » et d'origine
/// droite « l », alors que l'origine de « l » est « n », plus loin que « b » dans la même suite
/// (« bonjour ») : son auteur connaissait « n », entre ses deux voisins. Il perd son origine droite.
/// Graine 28569 de la voie d'un client.
#[test]
fn an_item_between_neighbours_its_author_did_not_see_side_by_side_loses_its_right_origin() {
    assert_eq!(replay_apart("min-28569", false), Ok(()));
}

/// Ruling R853 : une entrée de map dont le parent nommé n'est pas un type ne donne de clé à
/// personne. Graine 1854 de la voie du journal : `level` posé sur une valeur, ramassé par yrs, puis
/// une valeur entre cette entrée et une lettre. Tenue pour voisine d'une entrée de map, elle
/// échappait à la règle G, et yrs la rangeait dans le texte. Graine 577 : un élément entre une entrée
/// de map ramassée et un élément vivant ; c'est elle qui rougit quand la clé d'un voisin ramassé
/// compte, la trace de 1854 ne le montrant plus depuis le tour 7.
#[test]
fn an_entry_yrs_collects_gives_its_key_to_no_neighbour() {
    assert_eq!(replay_apart("min-j1854", true), Ok(()));
    assert_eq!(replay_apart("min-j577", true), Ok(()));
}

/// Ruling R853 : ce que la mise à jour range dans un de ses éléments qui n'est pas un type, yrs le
/// ramasse. Tenu pour vivant, un élément posé à côté de lui échappait à la règle G : la garde ne
/// disait plus ce que yrs garde, la page relue de son état n'était plus la page vivante, et le
/// journal ne se relisait plus (graines 5102, 12253, 37708 et 42716 de la voie du journal, 27404 de
/// celle d'un client).
#[test]
fn what_an_update_files_under_something_that_is_no_type_is_collected() {
    assert_eq!(replay_apart("min-j37708", true), Ok(()));
    assert_eq!(replay_apart("min-j42716", true), Ok(()));
    assert_eq!(replay_apart("min-27404", false), Ok(()));
}

/// Ruling R839 : une entrée de map ramassée ne donne sa clé à personne. La page que le serveur vient
/// d'écrire lit dans yrs une entrée déjà ramassée ; relue de son journal, elle se souvient d'une
/// entrée vivante : les deux doivent juger pareil le voisin qui la nomme, sans quoi le journal ne
/// se relit plus. Graines 5102 et 12253 de la voie du journal, rejouées telles quelles : leur trace
/// ne montre rien sur une page relue de sa création.
#[test]
fn a_collected_map_entry_gives_no_key_on_a_page_the_server_wrote() {
    for seed in [5102, 12253] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["child_cases", "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, format!("journal:{seed}:1"))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .expect("le processus fils se lance");
        let text = String::from_utf8_lossy(&output.stdout);
        let failed: Vec<&str> = text.lines().filter(|l| l.starts_with("F ")).collect();
        assert!(failed.is_empty(), "{failed:?}");
        assert!(
            text.lines().any(|l| l.starts_with("T ")),
            "le cas ne va pas au bout : {seed}"
        );
    }
}

/// Ruling R853 : ce qu'on ne sait pas d'une unité de la page ne se retient pas. Son voisin était
/// au-delà d'un trou d'horloge ; le trou comblé, la garde la tenait encore pour inconnue (graine 4445
/// de la voie du journal).
#[test]
fn a_neighbour_beyond_a_hole_is_read_again_once_the_hole_is_filled() {
    assert_eq!(replay_apart("min-j4445", true), Ok(()));
}

/// Ruling R858 : une unité se juge par ce qu'elle est aujourd'hui. La page vivante se souvenait
/// d'un élément que son état ne porte plus qu'en plage ramassée, et récrivait une entrée que
/// l'instantané suivi de la fin du journal ne récrivait pas : la page ne se chargeait plus (constat
/// C5 de la re-relecture 8, graine 5240 de la voie du journal).
#[test]
fn a_snapshot_and_the_end_of_the_journal_judge_a_collected_unit_alike() {
    assert_eq!(replay_apart("min-j5240", true), Ok(()));
}

/// Ruling R834 : une règle juge pareil la mise à jour et l'état qui la contient. Graine 629 de la
/// voie du journal : un « 😀 » d'origine « b » et d'origine droite la seconde moitié d'un emoji
/// posé par la même entrée après « c ». Jugée dans la seule suite de l'origine (« abc », de la
/// page), la règle I3 (retirée depuis, R856) ne voyait pas la chaîne qui continue dans la mise à
/// jour ; jugée dans l'état (une seule suite), elle la voyait, et la page relue de son état perdait
/// l'emoji.
#[test]
fn a_rule_judges_an_update_and_the_state_holding_it_alike() {
    assert_eq!(replay_apart("min-j629", true), Ok(()));
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

/// Ruling R824 (le texte pour yrs, R813) : yrs 0.28.1 ne relit pas le contenu JSON qu'il écrit
/// (`ItemContent::encode` : `len` chaînes ; `decode` : `len + 1`). Sur yrs seul : un texte racine,
/// un contenu JSON d'une valeur écrit comme yrs le lit (`len` à 0), puis l'état encodé, illisible.
/// Vert tant que le défaut est là ; rouge le jour où yrs le corrige : la suppression de R815 et la
/// lecture `JsonCount` de la garde seront alors à revoir.
#[test]
fn yrs_does_not_read_back_the_json_it_writes() {
    let doc = Doc::new();
    doc.get_or_insert_text("t");
    let written = [1, 1, 1, 0, 2, 1, 1, b't', 0, 3, b'"', b'a', b'"', 0];
    doc.transact_mut()
        .apply_update(Update::decode_v1(&written).expect("yrs lit ce JSON"))
        .expect("yrs l'intègre");
    let state = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    assert!(
        Update::decode_v1(&state).is_err(),
        "yrs relit l'état qu'il encode : {state:?}"
    );
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

/// Ruling R831 (constat M-7.3 de la re-relecture 7) : un contenu JSON relu du journal n'est
/// supprimé (R815) que si yrs l'intègre. Dans une page à trou d'horloge (un journal d'avant le
/// Ruling R131), une entrée peut porter du JSON à des horloges que la page connaît déjà au-delà du
/// trou : yrs l'ignore, et ces horloges sont celles d'un autre contenu de la page (ici « x »).
/// Supprimer d'après les octets de l'entrée sans écarter ce que la page connaît au-delà du trou
/// effaçait « x ».
#[test]
fn a_json_content_already_known_beyond_a_hole_deletes_nothing() {
    let (page, created) = PageDoc::new_page();
    let state = StateVector::decode_v1(&page.state_vector()).unwrap();
    let server = state.iter().next().unwrap().0.get();
    // Le client 9 000, à partir de l'horloge 5 (un trou de 0 à 5) : un texte dans le paragraphe
    // du serveur (son horloge 3), et dedans « xy ».
    let mut beyond = vec![1, 2];
    var(&mut beyond, 9_000);
    beyond.extend_from_slice(&[5, 7, 0]);
    var(&mut beyond, server);
    beyond.extend_from_slice(&[3, 6, 4, 0]);
    var(&mut beyond, 9_000);
    beyond.extend_from_slice(&[5, 2, b'x', b'y', 0]);
    // Puis du JSON du même client à l'horloge 6, que la page connaît (« x ») : sous la racine,
    // écrit comme yrs le lit (`len` à 0, puis une chaîne).
    let mut json = vec![1, 1];
    var(&mut json, 9_000);
    json.push(6);
    json.extend_from_slice(&[2, 1]);
    string(&mut json, FRAGMENT);
    json.extend_from_slice(&[0, 3, b'"', b'a', b'"', 0]);
    let journal = [created, beyond, json];
    let text = |page: &PageDoc| format!("{:?}", page.read_blocks()[0].content);
    let page = PageDoc::from_updates([journal[0].as_slice()]).unwrap();
    page.apply_update(&journal[1]).expect("le trou se relit");
    let before = text(&page);
    assert!(before.contains("xy"), "{before}");
    page.apply_update(&journal[2]).expect("l'entrée se relit");
    assert_eq!(
        text(&page),
        before,
        "le contenu que la page connaît au-delà du trou"
    );
    let reloaded = PageDoc::from_updates(journal.iter().map(Vec::as_slice)).unwrap();
    assert_eq!(text(&reloaded), before);
}

mod known_limits {
    //! Ce que la garde ne tient pas encore, écrit en tests verts tant que la limite est là, rouges
    //! le jour où elle est levée (sortir alors le test du module, et lui faire affirmer ce qui est
    //! voulu).

    use super::*;

    /// Ruling R830 : une page à trou d'horloge (un journal d'avant le Ruling R131) dont une entrée
    /// hostile comble le trou peut se lire autrement vivante et relue de son état. Graine 41449 de
    /// la voie du journal, réduite : le client du serveur porte une entrée de map à l'horloge 10
    /// (son trou va de 8 à 10), d'origine sa valeur de `schemaVersion` ; une entrée suivante comble
    /// le trou par une entrée d'origine la même valeur, du même client, sans origine droite. yrs
    /// comme Yjs départagent deux éléments de même origine et de même origine droite par leur
    /// client : du même client, c'est l'ordre d'arrivée qui décide, et la relecture de l'état les
    /// intègre dans l'ordre des horloges. Aucun client n'écrit de trou depuis R131 (R786), et deux
    /// éléments d'un client de même origine et de même origine droite ne s'écrivent pas : le
    /// second voit le premier.
    #[test]
    fn a_hole_filled_by_a_hostile_entry_reads_otherwise_from_the_state() {
        let journal = entries("min-j41449");
        let page = PageDoc::from_updates([journal[0].as_slice()]).unwrap();
        for entry in &journal[1..] {
            page.apply_update(entry).expect("l'entrée se relit");
        }
        assert!(
            live::reloaded_from_state_differs(&page).is_some(),
            "la limite est levée : la page à trou relue de son état est la page vivante"
        );
    }
}
