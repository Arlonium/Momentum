//! Tantivy : la forme d'index du plan (bloc par bloc, titre recopié, débuts de mot), le
//! collecteur qui groupe par page (R902), la note sans statistique d'ensemble (R916), la
//! charge utile d'une validation (R909).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tantivy::collector::{Collector, SegmentCollector};
use tantivy::columnar::Column;
use tantivy::indexer::{IndexWriterOptions, NoMergePolicy};
use tantivy::query::{
    Bm25StatisticsProvider, BooleanQuery, BoostQuery, DisjunctionMaxQuery, Occur, Query, TermQuery, TermSetQuery,
};
use tantivy::schema::{
    FAST, Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::{
    AsciiFoldingFilter, Language, LowerCaser, PreTokenizedString, RemoveLongFilter, SimpleTokenizer, Stemmer,
    TextAnalyzer, Token,
};
use tantivy::{
    DocAddress, DocId, Index, IndexReader, IndexWriter, ReloadPolicy, Score, Searcher, SegmentOrdinal, SegmentReader,
    TantivyDocument, Term,
};

use crate::corpus::{self, Entry, Rng, Vocab};
use crate::{Args, dir_size, heap_live, heap_peak, heap_reset_peak, mb, ms, process_memory, stats};

/// Le plus long mot gardé : 40 octets (`RemoveLongFilter` écarte un mot dont la longueur
/// atteint sa limite).
const LONGEST_WORD: usize = 40;

#[derive(Clone, Copy)]
struct Fields {
    space: Field,
    node: Field,
    entry: Field,
    block_id: Field,
    title: Field,
    text: Field,
    title_starts: Option<Field>,
    text_starts: Option<Field>,
    page_hi: Field,
    page_lo: Field,
    blk: Field,
    marker: Field,
}

fn schema(with_starts: bool) -> (Schema, Fields) {
    let mut b = Schema::builder();
    let searched =
        TextFieldIndexing::default().set_tokenizer("mm").set_index_option(IndexRecordOption::WithFreqs).set_fieldnorms(true);
    let starts =
        TextFieldIndexing::default().set_tokenizer("raw").set_index_option(IndexRecordOption::Basic).set_fieldnorms(false);
    let space = b.add_text_field("space", STRING);
    let node = b.add_text_field("node", STRING);
    let entry = b.add_text_field("entry", STRING);
    let block_id = b.add_text_field("block_id", STORED);
    let title = b.add_text_field("title", TextOptions::default().set_indexing_options(searched.clone()));
    let text = b.add_text_field("text", TextOptions::default().set_indexing_options(searched).set_stored());
    let (title_starts, text_starts) = if with_starts {
        (
            Some(b.add_text_field("title_starts", TextOptions::default().set_indexing_options(starts.clone()))),
            Some(b.add_text_field("text_starts", TextOptions::default().set_indexing_options(starts))),
        )
    } else {
        (None, None)
    };
    let page_hi = b.add_u64_field("page_hi", FAST);
    let page_lo = b.add_u64_field("page_lo", FAST);
    let blk = b.add_u64_field("blk", FAST);
    let marker = b.add_u64_field("marker", FAST);
    let f = Fields { space, node, entry, block_id, title, text, title_starts, text_starts, page_hi, page_lo, blk, marker };
    (b.build(), f)
}

fn fields_of(schema: &Schema) -> Fields {
    let g = |n: &str| schema.get_field(n).unwrap_or_else(|_| panic!("champ {n}"));
    Fields {
        space: g("space"),
        node: g("node"),
        entry: g("entry"),
        block_id: g("block_id"),
        title: g("title"),
        text: g("text"),
        title_starts: schema.get_field("title_starts").ok(),
        text_starts: schema.get_field("text_starts").ok(),
        page_hi: g("page_hi"),
        page_lo: g("page_lo"),
        blk: g("blk"),
        marker: g("marker"),
    }
}

fn analyzer(stem: bool) -> TextAnalyzer {
    let b = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(LONGEST_WORD + 1))
        .filter(LowerCaser)
        .filter(AsciiFoldingFilter);
    if stem { b.filter(Stemmer::new(Language::French)).build() } else { b.build() }
}

fn register(index: &Index) {
    index.tokenizers().register("mm", analyzer(false));
}

static BUDGET_MB: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(15);

fn writer(index: &Index) -> IndexWriter {
    // Par défaut le plus petit tampon admis (15 Mo), un fil d'indexation, un fil de fusion.
    let options = IndexWriterOptions::builder()
        .memory_budget_per_thread(BUDGET_MB.load(Ordering::Relaxed) * 1_000_000)
        .num_worker_threads(1)
        .num_merge_threads(1)
        .build();
    index.writer_with_options(options).expect("écrivain")
}

pub fn set_budget(args: &Args) {
    if let Some(v) = args.value("--budget-mb") {
        BUDGET_MB.store(v.parse().expect("--budget-mb"), Ordering::Relaxed);
    }
}

fn block_uuid(page: u32, block: u32) -> String {
    let mut r = Rng(((page as u64) << 32) | block as u64);
    let (a, b) = (r.next(), r.next());
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0xffff,
        b >> 48,
        b & 0xffff_ffff_ffff
    )
}

struct Scratch {
    folded: String,
}

fn pre_tokens(text: &str, scratch: &mut Scratch) -> PreTokenizedString {
    corpus::fold(text, &mut scratch.folded);
    let mut tokens = Vec::new();
    corpus::starts(&scratch.folded, |s| {
        tokens.push(Token {
            offset_from: 0,
            offset_to: 0,
            position: tokens.len(),
            text: s.to_string(),
            position_length: 1,
        })
    });
    PreTokenizedString { text: String::new(), tokens }
}

fn make_doc(f: &Fields, e: &Entry, title_on_blocks: bool, scratch: &mut Scratch) -> TantivyDocument {
    let mut d = TantivyDocument::default();
    let hex = corpus::page_hex(e.page);
    let id = corpus::page_id(e.page);
    let block = if e.block == 0 { String::new() } else { block_uuid(e.page, e.block) };
    d.add_text(f.space, e.space);
    d.add_text(f.entry, format!("{hex}/{block}"));
    d.add_text(f.node, &hex);
    d.add_text(f.block_id, &block);
    d.add_u64(f.page_hi, (id >> 64) as u64);
    d.add_u64(f.page_lo, id as u64);
    d.add_u64(f.blk, e.block as u64);
    if e.block == 0 || title_on_blocks {
        d.add_text(f.title, e.title);
        if let Some(ts) = f.title_starts {
            d.add_pre_tokenized_text(ts, pre_tokens(e.title, scratch));
        }
    }
    if e.block > 0 {
        d.add_text(f.text, e.text);
        if let Some(ts) = f.text_starts {
            d.add_pre_tokenized_text(ts, pre_tokens(e.text, scratch));
        }
    }
    d
}

/// Relève la pointe de mémoire de travail du processus tant qu'il vit.
struct PeakWatch {
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl PeakWatch {
    fn start() -> PeakWatch {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU64::new(0));
        let (s, p) = (stop.clone(), peak.clone());
        let handle = std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                p.fetch_max(process_memory().0, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        PeakWatch { stop, peak, handle: Some(handle) }
    }
    fn finish(mut self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.take().unwrap().join().unwrap();
        self.peak.load(Ordering::Relaxed)
    }
}

fn fresh_dir(path: &Path) {
    if path.exists() {
        std::fs::remove_dir_all(path).expect("vider le dossier");
    }
    std::fs::create_dir_all(path).expect("créer le dossier");
}

pub fn build(args: &Args) {
    let dir = PathBuf::from(args.pos(0));
    let scale = args.scale();
    let with_starts = !args.flag("--no-starts");
    let title_on_blocks = !args.flag("--no-title-on-blocks");
    let every: u64 = args.value("--commit-every").map(|v| v.parse().unwrap()).unwrap_or(100_000);
    fresh_dir(&dir);
    let vocab = Vocab::new(60_000);
    let (schema, f) = schema(with_starts);
    let index = Index::create_in_dir(&dir, schema).expect("créer l'index");
    register(&index);
    let mut w = writer(&index);
    let watch = PeakWatch::start();
    heap_reset_peak();
    let mut scratch = Scratch { folded: String::new() };
    let (mut n, mut bytes) = (0u64, 0u64);
    let mut commit_times = Vec::new();
    let start = Instant::now();
    corpus::stream(&scale, &vocab, |e| {
        bytes += e.text.len() as u64 + if e.block == 0 { e.title.len() as u64 } else { 0 };
        w.add_document(make_doc(&f, &e, title_on_blocks, &mut scratch)).expect("ajout");
        n += 1;
        if n % every == 0 {
            let t = Instant::now();
            let mut p = w.prepare_commit().expect("préparer");
            p.set_payload(&format!("jeton-{n}"));
            p.commit().expect("valider");
            commit_times.push(t.elapsed());
        }
    });
    let t = Instant::now();
    let mut p = w.prepare_commit().expect("préparer");
    p.set_payload("jeton-final");
    p.commit().expect("valider");
    commit_times.push(t.elapsed());
    let indexed = start.elapsed();
    let size_before = dir_size(&dir);
    let t = Instant::now();
    w.wait_merging_threads().expect("fusions");
    let merges = t.elapsed();
    let peak_ws = watch.finish();
    std::thread::sleep(Duration::from_secs(2));
    let (ws, private) = process_memory();
    println!(
        "MESURE build repos : 2 s après la fin de l'indexation et des fusions, écrivain lâché : mémoire de travail {:.0} Mo, privée {:.0} Mo, tas {:.0} Mo",
        mb(ws),
        mb(private),
        mb(heap_live() as u64)
    );
    let index = Index::open_in_dir(&dir).unwrap();
    let segments = index.searchable_segment_metas().unwrap().len();
    println!(
        "MESURE build débuts={with_starts} titre_sur_blocs={title_on_blocks} tampon={} Mo : {n} entrées, {:.1} Mo de texte reçu, indexées en {:.1} s \
         ({:.0} entrées/s, {:.2} Mo/s), fusions attendues ensuite {:.1} s",
        BUDGET_MB.load(Ordering::Relaxed),
        mb(bytes),
        indexed.as_secs_f64(),
        n as f64 / indexed.as_secs_f64(),
        mb(bytes) / indexed.as_secs_f64(),
        merges.as_secs_f64()
    );
    let mut ct = commit_times.clone();
    let s = stats(&mut ct);
    println!(
        "MESURE build validations (toutes les {every} entrées, {} en tout) : p50 {:.0} ms, max {:.0} ms",
        commit_times.len(),
        ms(s.p50),
        ms(s.max)
    );
    println!(
        "MESURE build mémoire : pointe de mémoire de travail {:.0} Mo, pointe du tas {:.0} Mo",
        mb(peak_ws),
        mb(heap_peak() as u64)
    );
    println!(
        "MESURE build disque : {:.1} Mo avant la fin des fusions, {:.1} Mo après, {segments} segments ; rapport au texte reçu {:.2}",
        mb(size_before),
        mb(dir_size(&dir)),
        dir_size(&dir) as f64 / bytes as f64
    );
    print_sizes(&dir);
}

pub fn sizes(args: &Args) {
    print_sizes(Path::new(args.pos(0)));
}

fn print_sizes(dir: &Path) {
    let index = Index::open_in_dir(dir).expect("ouvrir");
    register(&index);
    let reader = index.reader().expect("lecteur");
    let searcher = reader.searcher();
    let usage = searcher.space_usage().expect("usage");
    let mut by: BTreeMap<String, [u64; 4]> = BTreeMap::new();
    let (mut store, mut deletes, mut total) = (0u64, 0u64, 0u64);
    for seg in usage.segments() {
        for (i, part) in [seg.termdict(), seg.postings(), seg.fieldnorms(), seg.fast_fields()].iter().enumerate() {
            for fu in part.fields() {
                by.entry(fu.field_name().to_string()).or_default()[i] += fu.total().get_bytes();
            }
        }
        store += seg.store().total().get_bytes();
        deletes += seg.deletes().get_bytes();
        total += seg.total().get_bytes();
    }
    println!("MESURE tailles (Mo) : champ | dictionnaire | listes | normes | colonnes | somme");
    let mut starts_total = 0;
    for (name, v) in &by {
        let sum: u64 = v.iter().sum();
        if name.ends_with("_starts") {
            starts_total += sum;
        }
        println!("MESURE tailles : {name} | {:.1} | {:.1} | {:.1} | {:.1} | {:.1}", mb(v[0]), mb(v[1]), mb(v[2]), mb(v[3]), mb(sum));
    }
    println!(
        "MESURE tailles : textes gardés {:.1} Mo, suppressions {:.1} Mo, total des segments {:.1} Mo, dossier {:.1} Mo ; \
         champs des débuts de mot {:.1} Mo (sans eux : {:.1} Mo)",
        mb(store),
        mb(deletes),
        mb(total),
        mb(dir_size(dir)),
        mb(starts_total),
        mb(total - starts_total)
    );
}

// ---------------------------------------------------------------------------------------------
// La requête, la note, le collecteur.

#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn write_u128(&mut self, v: u128) {
        // Les identifiants sont déjà tirés au hasard.
        self.0 = (v as u64) ^ ((v >> 64) as u64).rotate_left(29);
    }
}

type IdBuild = BuildHasherDefault<IdHasher>;
type Allowed = HashSet<u128, IdBuild>;

fn allowed_of(pages: impl Iterator<Item = u32>) -> Arc<Allowed> {
    Arc::new(pages.map(corpus::page_id).collect())
}

#[derive(Clone, Copy, Debug)]
struct Best {
    score: Score,
    blk: u64,
    addr: DocAddress,
}

impl Best {
    fn beats(&self, other: &Best) -> bool {
        self.score > other.score || (self.score == other.score && self.blk < other.blk)
    }
}

/// R902 : la meilleure entrée de chaque page, les seules pages permises ; `skip` et `limit`
/// se comptent ensuite en pages.
struct ByPage {
    allowed: Option<Arc<Allowed>>,
}

struct ByPageSegment {
    seg: SegmentOrdinal,
    hi: Column<u64>,
    lo: Column<u64>,
    blk: Column<u64>,
    allowed: Option<Arc<Allowed>>,
    best: HashMap<u128, Best, IdBuild>,
    seen: u64,
}

impl Collector for ByPage {
    type Fruit = (HashMap<u128, Best, IdBuild>, u64);
    type Child = ByPageSegment;

    fn for_segment(&self, seg: SegmentOrdinal, reader: &SegmentReader) -> tantivy::Result<ByPageSegment> {
        let ff = reader.fast_fields();
        Ok(ByPageSegment {
            seg,
            hi: ff.u64("page_hi")?,
            lo: ff.u64("page_lo")?,
            blk: ff.u64("blk")?,
            allowed: self.allowed.clone(),
            best: HashMap::default(),
            seen: 0,
        })
    }

    fn requires_scoring(&self) -> bool {
        true
    }

    fn merge_fruits(&self, fruits: Vec<Self::Fruit>) -> tantivy::Result<Self::Fruit> {
        let mut it = fruits.into_iter();
        let (mut all, mut seen) = it.next().unwrap_or_default();
        for (map, n) in it {
            seen += n;
            for (page, b) in map {
                all.entry(page).and_modify(|cur| if b.beats(cur) { *cur = b }).or_insert(b);
            }
        }
        Ok((all, seen))
    }
}

impl SegmentCollector for ByPageSegment {
    type Fruit = (HashMap<u128, Best, IdBuild>, u64);

    fn collect(&mut self, doc: DocId, score: Score) {
        self.seen += 1;
        let page = ((self.hi.first(doc).unwrap_or(0) as u128) << 64) | self.lo.first(doc).unwrap_or(0) as u128;
        if let Some(allowed) = &self.allowed {
            if !allowed.contains(&page) {
                return;
            }
        }
        let b = Best { score, blk: self.blk.first(doc).unwrap_or(0), addr: DocAddress::new(self.seg, doc) };
        self.best.entry(page).and_modify(|cur| if b.beats(cur) { *cur = b }).or_insert(b);
    }

    fn harvest(self) -> Self::Fruit {
        (self.best, self.seen)
    }
}

/// R916 : des statistiques constantes, données par programme. Rien n'est lu dans l'index.
struct Fixed {
    title: Field,
}

impl Bm25StatisticsProvider for Fixed {
    fn total_num_tokens(&self, field: Field) -> tantivy::Result<u64> {
        // Longueur moyenne tenue pour constante : 4 mots pour un titre, 20 pour un texte.
        Ok(if field == self.title { 4_000_000 } else { 20_000_000 })
    }
    fn total_num_docs(&self) -> tantivy::Result<u64> {
        Ok(1_000_000)
    }
    fn doc_freq(&self, _term: &Term) -> tantivy::Result<u64> {
        Ok(1_000)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Note {
    /// Statistiques constantes (R916).
    Fixed,
    /// La BM25 de Tantivy telle quelle : le témoin de ce qu'il ne faut pas.
    Bm25,
}

#[derive(Clone, Default)]
struct Ask {
    terms: Vec<String>,
    prefix: Option<String>,
    /// Le filtre posé dans la requête, par les termes exacts de `node` (question a).
    term_set: Option<Arc<Vec<u32>>>,
    /// L'espace en terme exigé.
    space: Option<&'static str>,
}

fn ask(terms: &[&str], prefix: Option<&str>) -> Ask {
    let mut folded = String::new();
    Ask {
        terms: terms
            .iter()
            .map(|t| {
                corpus::fold(t, &mut folded);
                folded.clone()
            })
            .collect(),
        prefix: prefix.map(|p| {
            corpus::fold(p, &mut folded);
            folded.clone()
        }),
        ..Ask::default()
    }
}

fn either(title: Field, text: Field, word: &str, option: IndexRecordOption) -> Box<dyn Query> {
    let t = TermQuery::new(Term::from_field_text(title, word), option);
    let x = TermQuery::new(Term::from_field_text(text, word), option);
    Box::new(DisjunctionMaxQuery::new(vec![Box::new(BoostQuery::new(Box::new(t), 3.0)), Box::new(x)]))
}

/// La requête est bâtie par programme : aucun analyseur de requêtes, aucun parcours du
/// dictionnaire (un préfixe est un terme exact du champ des débuts de mot).
fn build_query(f: &Fields, a: &Ask) -> BooleanQuery {
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
    for t in &a.terms {
        clauses.push((Occur::Must, either(f.title, f.text, t, IndexRecordOption::WithFreqs)));
    }
    if let Some(p) = &a.prefix {
        let cut: String = p.chars().take(corpus::MAX_START_CHARS).collect();
        clauses.push((
            Occur::Must,
            either(f.title_starts.expect("débuts"), f.text_starts.expect("débuts"), &cut, IndexRecordOption::Basic),
        ));
    }
    if let Some(space) = a.space {
        clauses.push((
            Occur::Must,
            Box::new(TermQuery::new(Term::from_field_text(f.space, space), IndexRecordOption::Basic)),
        ));
    }
    if let Some(pages) = &a.term_set {
        let terms = pages.iter().map(|p| Term::from_field_text(f.node, &corpus::page_hex(*p)));
        clauses.push((Occur::Must, Box::new(TermSetQuery::new(terms))));
    }
    BooleanQuery::new(clauses)
}

#[derive(Clone, Debug, PartialEq)]
struct Hit {
    page: u128,
    score_bits: u32,
    blk: u64,
    addr: DocAddress,
}

/// Toutes les pages trouvées, rangées par note puis par identifiant (jamais par l'adresse
/// dans l'index, qui dépend des autres entrées), et le nombre d'entrées vues par le collecteur.
fn search(searcher: &Searcher, f: &Fields, a: &Ask, allowed: Option<&Arc<Allowed>>, note: Note) -> (Vec<Hit>, u64) {
    let query = build_query(f, a);
    let collector = ByPage { allowed: allowed.cloned() };
    let (map, seen) = match note {
        Note::Fixed => searcher.search_with_statistics_provider(&query, &collector, &Fixed { title: f.title }),
        Note::Bm25 => searcher.search(&query, &collector),
    }
    .expect("recherche");
    let mut hits: Vec<Hit> =
        map.into_iter().map(|(page, b)| Hit { page, score_bits: b.score.to_bits(), blk: b.blk, addr: b.addr }).collect();
    hits.sort_by(|x, y| {
        f32::from_bits(y.score_bits).total_cmp(&f32::from_bits(x.score_bits)).then(x.page.cmp(&y.page))
    });
    (hits, seen)
}

/// Les textes gardés d'une réponse (20 pages) : l'extrait est bâti par programme (le
/// générateur d'extraits de Tantivy lit la fréquence des termes dans l'index, R916).
fn excerpts(searcher: &Searcher, f: &Fields, hits: &[Hit], skip: usize, limit: usize) -> usize {
    let mut chars = 0;
    for h in hits.iter().skip(skip).take(limit) {
        let doc: TantivyDocument = searcher.doc(h.addr).expect("entrée gardée");
        if let Some(text) = doc.get_first(f.text).and_then(|v| v.as_str()) {
            chars += text.chars().take(240).count();
        }
        if let Some(id) = doc.get_first(f.block_id).and_then(|v| v.as_str()) {
            chars += id.len();
        }
    }
    chars
}

struct Opened {
    index: Index,
    reader: IndexReader,
    f: Fields,
}

fn open_index(dir: &Path) -> Opened {
    let index = Index::open_in_dir(dir).expect("ouvrir l'index");
    register(&index);
    let reader = index.reader_builder().reload_policy(ReloadPolicy::Manual).try_into().expect("lecteur");
    let f = fields_of(&index.schema());
    Opened { index, reader, f }
}

pub fn open(args: &Args) {
    let dir = PathBuf::from(args.pos(0));
    let before = process_memory();
    let t0 = Instant::now();
    let index = Index::open_in_dir(&dir).expect("ouvrir");
    register(&index);
    let t1 = Instant::now();
    let reader = index.reader_builder().reload_policy(ReloadPolicy::Manual).try_into().expect("lecteur");
    let searcher = reader.searcher();
    let t2 = Instant::now();
    let payload = index.load_metas().expect("métadonnées").payload;
    let t3 = Instant::now();
    let w = writer(&index);
    let t4 = Instant::now();
    let f = fields_of(&index.schema());
    let (hits, _) = search(&searcher, &f, &ask(&["recherche"], None), None, Note::Fixed);
    let t5 = Instant::now();
    let after = process_memory();
    println!(
        "MESURE ouverture : index {:.1} ms, lecteur {:.1} ms, charge utile {:.2} ms ({:?}), écrivain {:.1} ms, total {:.1} ms ; \
         première requête {:.1} ms ({} pages) ; {} segments, {} entrées ; mémoire de travail {:.1} -> {:.1} Mo",
        ms(t1 - t0),
        ms(t2 - t1),
        ms(t3 - t2),
        payload,
        ms(t4 - t3),
        ms(t4 - t0),
        ms(t5 - t4),
        hits.len(),
        searcher.segment_readers().len(),
        searcher.num_docs(),
        mb(before.0),
        mb(after.0)
    );
    drop(w);
}

fn mixed_queries(o: &Opened, vocab: &Vocab, n: usize, allowed: &Arc<Allowed>) {
    let searcher = o.reader.searcher();
    let mut rng = Rng(99);
    for i in 0..n {
        let a = vocab.pick(&mut rng).to_string();
        let b = vocab.pick(&mut rng).to_string();
        let start: String = b.chars().take(3).collect();
        let q = if i % 3 == 0 && start.chars().count() >= 2 { ask(&[&a], Some(&start)) } else { ask(&[&a, &b], None) };
        let (hits, _) = search(&searcher, &o.f, &q, Some(allowed), Note::Fixed);
        excerpts(&searcher, &o.f, &hits, 0, 20);
    }
}

pub fn rest(args: &Args) {
    let dir = PathBuf::from(args.pos(0));
    let scale = args.scale();
    let say = |what: &str| {
        let (ws, private) = process_memory();
        println!(
            "MESURE repos {what} : mémoire de travail {:.1} Mo, mémoire privée {:.1} Mo, tas de Rust {:.1} Mo",
            mb(ws),
            mb(private),
            mb(heap_live() as u64)
        );
    };
    say("processus nu");
    let o = open_index(&dir);
    say("index ouvert, lecteur prêt");
    let w = if args.flag("--no-writer") { None } else { Some(writer(&o.index)) };
    say("écrivain au repos (tampon 15 Mo, un fil, un fil de fusion)");
    std::thread::sleep(Duration::from_secs(2));
    say("après 2 s");
    let vocab = Vocab::new(60_000);
    say("vocabulaire du banc chargé (hors mesure : à retrancher)");
    let allowed = allowed_of(0..scale.big);
    if !args.flag("--no-queries") {
        mixed_queries(&o, &vocab, 500, &allowed);
        say("après 500 requêtes mêlées");
    }
    std::thread::sleep(Duration::from_secs(2));
    say("2 s plus tard");
    if let Some(mut w) = w {
        // Une petite validation, comme l'indexeur en fait, puis le repos.
        let mut scratch = Scratch { folded: String::new() };
        let e = Entry { space: "a", page: 3, block: 7, title: "Projet repos", text: "une phrase de plus pour le repos" };
        w.delete_term(Term::from_field_text(o.f.entry, &format!("{}/{}", corpus::page_hex(3), block_uuid(3, 7))));
        w.add_document(make_doc(&o.f, &e, true, &mut scratch)).unwrap();
        let mut p = w.prepare_commit().unwrap();
        p.set_payload("jeton-repos");
        p.commit().unwrap();
        o.reader.reload().unwrap();
        std::thread::sleep(Duration::from_secs(2));
        say("après une validation d'une entrée, 2 s plus tard");
    }
}

fn measure(name: &str, filter: &str, mode: &str, iters: usize, mut run: impl FnMut() -> (usize, u64)) -> Duration {
    let mut found = (0, 0);
    for _ in 0..3 {
        found = run();
    }
    let base = heap_reset_peak();
    run();
    let peak = heap_peak() - base;
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t = Instant::now();
        found = run();
        samples.push(t.elapsed());
        if samples.len() >= 10 && samples.iter().sum::<Duration>() > Duration::from_secs(20) {
            break;
        }
    }
    let n = samples.len();
    let s = stats(&mut samples);
    println!(
        "MESURE requête | {name} | {filter} | {mode} | {} pages, {} entrées vues | p50 {:.2} ms | p95 {:.2} ms | max {:.2} ms | pic du tas {:.2} Mo | {n} passages",
        found.0,
        found.1,
        ms(s.p50),
        ms(s.p95),
        ms(s.max),
        mb(peak as u64)
    );
    s.p95
}

pub fn query(args: &Args) {
    let dir = PathBuf::from(args.pos(0));
    let scale = args.scale();
    let iters: usize = args.value("--iters").map(|v| v.parse().unwrap()).unwrap_or(100);
    let o = open_index(&dir);
    let searcher = o.reader.searcher();
    let vocab = Vocab::new(60_000);
    let w = |rank: usize| vocab.words[rank].as_str();
    println!(
        "index : {} entrées, {} segments ; mots : rang 0 « {} », 5 « {} », 20 « {} », 60 « {} », 300 « {} », 2000 « {} », 20000 « {} »",
        searcher.num_docs(),
        searcher.segment_readers().len(),
        w(0),
        w(5),
        w(20),
        w(60),
        w(300),
        w(2000),
        w(20000)
    );
    let f10 = (1..=10u32).collect::<Vec<_>>();
    let f10k = (0..scale.big).collect::<Vec<_>>();
    let f100k = (0..scale.pages_a()).collect::<Vec<_>>();
    let filters: Vec<(&str, Arc<Vec<u32>>, Arc<Allowed>)> = [("10 pages", f10), ("10 000 pages", f10k), ("100 000 pages", f100k)]
        .into_iter()
        .map(|(n, v)| (n, Arc::new(v.clone()), allowed_of(v.into_iter())))
        .collect();

    let eight: Vec<&str> = (0..8).map(w).collect();
    let asks: Vec<(&str, Ask)> = vec![
        ("1 terme rare (rang 20000)", ask(&[w(20000)], None)),
        ("1 terme moyen (rang 300)", ask(&[w(300)], None)),
        ("1 terme très courant (rang 0)", ask(&[w(0)], None)),
        ("titre « projet » (toutes les entrées des grandes pages)", ask(&["projet"], None)),
        ("titre « mastodonte » (10 001 entrées d'une page)", ask(&["mastodonte"], None)),
        ("3 termes (rangs 5, 20, 60)", ask(&[w(5), w(20), w(60)], None)),
        ("3 termes dont un préfixe « pr »", ask(&[w(5), w(20)], Some("pr"))),
        ("8 termes (rangs 0 à 7)", ask(&eight, None)),
        ("8 termes dont un préfixe « pro »", ask(&eight[..7], Some("pro"))),
        ("préfixe seul « pr »", ask(&[], Some("pr"))),
        ("préfixe seul de 10 caractères", ask(&[], Some(&{
            let mut s = String::new();
            corpus::fold(w(20000), &mut s);
            s.chars().take(10).collect::<String>()
        }))),
    ];

    // Le filtre dans le collecteur (une table des pages permises, lue pour chaque entrée vue).
    let mut worst = Duration::ZERO;
    for (name, a) in &asks {
        for (fname, _, allowed) in &filters {
            let p95 = measure(name, fname, "filtre dans le collecteur", iters, || {
                let (hits, seen) = search(&searcher, &o.f, a, Some(allowed), Note::Fixed);
                excerpts(&searcher, &o.f, &hits, 0, 20);
                (hits.len(), seen)
            });
            worst = worst.max(p95);
        }
    }
    println!("MESURE requête : pire p95, filtre dans le collecteur : {:.2} ms", ms(worst));

    // Question (a) : le filtre posé dans la requête, par un ensemble de termes exacts.
    for (name, a) in [&asks[1], &asks[3], &asks[5]] {
        for (fname, pages, _) in &filters {
            let mut a = a.clone();
            a.term_set = Some(pages.clone());
            measure(name, fname, "filtre dans la requête (TermSetQuery)", iters.min(30), || {
                let (hits, seen) = search(&searcher, &o.f, &a, None, Note::Fixed);
                excerpts(&searcher, &o.f, &hits, 0, 20);
                (hits.len(), seen)
            });
        }
    }
    // L'espace en terme exigé, en plus du filtre du collecteur.
    for (name, a) in [&asks[1], &asks[3]] {
        let mut a = a.clone();
        a.space = Some("a");
        measure(name, "10 000 pages", "collecteur + terme d'espace exigé", iters, || {
            let (hits, seen) = search(&searcher, &o.f, &a, Some(&filters[1].2), Note::Fixed);
            excerpts(&searcher, &o.f, &hits, 0, 20);
            (hits.len(), seen)
        });
    }
    // R902 : ce que rend le groupement.
    let (hits, seen) = search(&searcher, &o.f, &asks[3].1, Some(&filters[1].2), Note::Fixed);
    let distinct: HashSet<u128> = hits.iter().map(|h| h.page).collect();
    println!(
        "R902 « projet » : {seen} entrées vues, {} pages rendues, {} distinctes (attendu {}) ; la page de 10 000 blocs y paraît {} fois",
        hits.len(),
        distinct.len(),
        scale.big,
        hits.iter().filter(|h| h.page == corpus::page_id(0)).count()
    );
    assert_eq!(hits.len(), scale.big as usize);
    assert_eq!(distinct.len(), hits.len());
    let first: HashSet<u128> = hits.iter().take(20).map(|h| h.page).collect();
    let second: HashSet<u128> = hits.iter().skip(20).take(20).map(|h| h.page).collect();
    assert!(first.is_disjoint(&second) && second.len() == 20.min(hits.len().saturating_sub(20)));
    let (hits, seen) = search(&searcher, &o.f, &asks[4].1, Some(&filters[1].2), Note::Fixed);
    println!("R902 « mastodonte » : {seen} entrées vues, {} page rendue", hits.len());
    assert_eq!(hits.len(), 1);
    let (hits, _) = search(&searcher, &o.f, &ask(&["canalcent"], None), Some(&filters[1].2), Note::Fixed);
    println!("filtre : « canalcent » rend {} pages permises (attendu 10), aucune de l'espace B", hits.len());
    assert_eq!(hits.len(), 10.min(scale.big as usize - 1));

    // R920 : un préfixe de plus de 10 caractères vérifie le texte gardé de 2 000 entrées au plus.
    let some: Vec<DocAddress> = {
        let (hits, _) = search(&searcher, &o.f, &asks[2].1, Some(&filters[1].2), Note::Fixed);
        hits.iter().take(2000).map(|h| h.addr).collect()
    };
    let mut samples = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        let mut n = 0;
        let mut folded = String::new();
        for addr in &some {
            let doc: TantivyDocument = searcher.doc(*addr).unwrap();
            if let Some(text) = doc.get_first(o.f.text).and_then(|v| v.as_str()) {
                corpus::fold(text, &mut folded);
                n += folded.split(|c: char| !c.is_alphanumeric()).filter(|w| w.starts_with("recherchera")).count();
            }
        }
        std::hint::black_box(n);
        samples.push(t.elapsed());
    }
    let s = stats(&mut samples);
    println!(
        "MESURE R920 : relire et vérifier le texte gardé de {} entrées de pages différentes : p50 {:.1} ms, max {:.1} ms",
        some.len(),
        ms(s.p50),
        ms(s.max)
    );

    canal(&searcher, &o.f, &filters, args);
}

/// Question (d) : la même requête, le même ensemble permis, pour un terme que portent 10
/// entrées permises (« canalzero ») puis 10 entrées permises et 100 000 non permises
/// (« canalcent »). Les deux sont mesurés tour à tour.
fn canal(searcher: &Searcher, f: &Fields, filters: &[(&str, Arc<Vec<u32>>, Arc<Allowed>)], args: &Args) {
    let n: usize = args.value("--canal-iters").map(|v| v.parse().unwrap()).unwrap_or(3000);
    for (fname, _, allowed) in &filters[..2] {
        let (zero, cent) = (ask(&["canalzero"], None), ask(&["canalcent"], None));
        let (mut tz, mut tc) = (Vec::with_capacity(n), Vec::with_capacity(n));
        let mut seen = (0, 0);
        for _ in 0..n {
            let t = Instant::now();
            let (hits, s) = search(searcher, f, &zero, Some(allowed), Note::Fixed);
            excerpts(searcher, f, &hits, 0, 20);
            tz.push(t.elapsed());
            seen.0 = s;
            let t = Instant::now();
            let (hits, s) = search(searcher, f, &cent, Some(allowed), Note::Fixed);
            excerpts(searcher, f, &hits, 0, 20);
            tc.push(t.elapsed());
            seen.1 = s;
        }
        let (sz, sc) = (stats(&mut tz), stats(&mut tc));
        println!(
            "MESURE canal | {fname} | 0 entrée non permise ({} vues) : p50 {:.3} ms, moyenne {:.3} ms, écart type {:.3} ms | \
             100 000 non permises ({} vues) : p50 {:.3} ms, moyenne {:.3} ms, écart type {:.3} ms | écart des p50 {:.3} ms",
            seen.0,
            ms(sz.p50),
            ms(sz.mean),
            ms(sz.sd),
            seen.1,
            ms(sc.p50),
            ms(sc.mean),
            ms(sc.sd),
            ms(sc.p50) - ms(sz.p50)
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Validations : durée et coût.

pub fn commits(args: &Args) {
    let dir = PathBuf::from(args.pos(0));
    let scale = args.scale();
    let o = open_index(&dir);
    let mut w = writer(&o.index);
    let vocab = Vocab::new(60_000);
    let mut rng = Rng(1234);
    let mut scratch = Scratch { folded: String::new() };
    let mut text = String::new();
    let mut token = 0u64;
    let before = dir_size(&dir);
    let watch = PeakWatch::start();
    let sentence = |rng: &mut Rng, text: &mut String| {
        text.clear();
        for _ in 0..22 {
            text.push_str(vocab.pick(rng));
            text.push(' ');
        }
    };
    // n blocs changés d'une page, remplacés un à un par leur terme `entry` (R919).
    for (n, reps) in [(1u32, 20), (100, 10), (1000, 5)] {
        let (mut feed, mut commit) = (Vec::new(), Vec::new());
        for rep in 0..reps {
            let t = Instant::now();
            for i in 0..n {
                let page = 100 + (i / 99) + rep * 20;
                let block = 1 + i % 99;
                sentence(&mut rng, &mut text);
                let e = Entry { space: "a", page, block, title: "Projet des validations", text: &text };
                w.delete_term(Term::from_field_text(
                    o.f.entry,
                    &format!("{}/{}", corpus::page_hex(page), block_uuid(page, block)),
                ));
                w.add_document(make_doc(&o.f, &e, true, &mut scratch)).unwrap();
            }
            feed.push(t.elapsed());
            let t = Instant::now();
            token += 1;
            let mut p = w.prepare_commit().unwrap();
            p.set_payload(&format!("jeton-{token}"));
            p.commit().unwrap();
            commit.push(t.elapsed());
        }
        let (sf, sc) = (stats(&mut feed), stats(&mut commit));
        println!(
            "MESURE validation | {n} blocs remplacés | écriture p50 {:.2} ms, max {:.2} ms | validation p50 {:.1} ms, max {:.1} ms | {reps} passages",
            ms(sf.p50),
            ms(sf.max),
            ms(sc.p50),
            ms(sc.max)
        );
    }
    // Un renommage : toutes les entrées de la page retirées par son terme `node`, puis récrites.
    for (what, page, blocks, reps) in [("page de 99 blocs", 50u32, scale.per_page, 10), ("page de 10 000 blocs", 0, scale.mastodon, 3)] {
        let mut samples = Vec::new();
        for rep in 0..reps {
            let title = format!("Projet renommé {rep} mastodonte");
            let t = Instant::now();
            w.delete_term(Term::from_field_text(o.f.node, &corpus::page_hex(page)));
            w.add_document(make_doc(&o.f, &Entry { space: "a", page, block: 0, title: &title, text: "" }, true, &mut scratch)).unwrap();
            for block in 1..=blocks {
                sentence(&mut rng, &mut text);
                let e = Entry { space: "a", page, block, title: &title, text: &text };
                w.add_document(make_doc(&o.f, &e, true, &mut scratch)).unwrap();
            }
            token += 1;
            let mut p = w.prepare_commit().unwrap();
            p.set_payload(&format!("jeton-{token}"));
            p.commit().unwrap();
            samples.push(t.elapsed());
        }
        let s = stats(&mut samples);
        println!(
            "MESURE validation | renommage d'une {what} (toutes ses entrées récrites, validation comprise) | p50 {:.1} ms, max {:.1} ms | {reps} passages",
            ms(s.p50),
            ms(s.max)
        );
    }
    // Une validation sans rien : ce que coûte le seul jeton.
    let mut samples = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        token += 1;
        let mut p = w.prepare_commit().unwrap();
        p.set_payload(&format!("jeton-{token}"));
        p.commit().unwrap();
        samples.push(t.elapsed());
    }
    let s = stats(&mut samples);
    println!("MESURE validation | vide (le jeton seul) | p50 {:.1} ms, max {:.1} ms", ms(s.p50), ms(s.max));
    let segments = o.index.searchable_segment_metas().unwrap().len();
    let t = Instant::now();
    w.wait_merging_threads().unwrap();
    let peak = watch.finish();
    println!(
        "MESURE validation | après {token} validations : {segments} segments, dossier {:.1} Mo -> {:.1} Mo ; fusions restantes {:.1} s ; \
         pointe de mémoire de travail {:.0} Mo ; charge utile relue {:?}",
        mb(before),
        mb(dir_size(&dir)),
        t.elapsed().as_secs_f64(),
        mb(peak),
        o.index.load_metas().unwrap().payload
    );
}

// ---------------------------------------------------------------------------------------------
// R916 : deux index qui ne diffèrent que hors du filtre rendent les mêmes pages, même ordre.

fn small_index(dir: &Path, entries: &[(&'static str, u32, u32, String)], deleted_pages: &[u32]) -> Opened {
    fresh_dir(dir);
    let (schema, f) = schema(true);
    let index = Index::create_in_dir(dir, schema).unwrap();
    register(&index);
    let mut w = writer(&index);
    w.set_merge_policy(Box::new(NoMergePolicy));
    let mut scratch = Scratch { folded: String::new() };
    for (i, (space, page, block, text)) in entries.iter().enumerate() {
        let e = Entry { space, page: *page, block: *block, title: "Page", text };
        w.add_document(make_doc(&f, &e, true, &mut scratch)).unwrap();
        if i % 700 == 699 {
            w.commit().unwrap();
        }
    }
    w.commit().unwrap();
    for page in deleted_pages {
        w.delete_term(Term::from_field_text(f.node, &corpus::page_hex(*page)));
    }
    w.commit().unwrap();
    drop(w);
    open_index(dir)
}

pub type Row = (&'static str, u32, u32, String);

/// Les entrées dosées de l'essai R916 : la base (pages 1 à 39, toutes permises), et trois
/// variantes qui n'ajoutent que des entrées hors du filtre.
pub fn dosed() -> (Vec<Row>, Vec<(&'static str, Vec<Row>, Vec<u32>)>) {
    let filler = |n: usize| " lorem".repeat(n);
    let mut base: Vec<Row> = vec![
        ("a", 1, 1, format!("dosea dosea dosea doseb{}", filler(16))),
        ("a", 2, 1, format!("dosea doseb doseb doseb{}", filler(16))),
        ("a", 3, 1, "doseb alpha beta".to_string()),
        ("a", 4, 1, format!("dosea{}", filler(40))),
        ("a", 5, 1, format!("doseb dossier{}", filler(5))),
        ("a", 6, 1, "dossier dosage dosea".to_string()),
        ("a", 6, 2, "dosea".to_string()),
    ];
    // Assez de pages permises pour que les notes se départagent.
    for p in 7..40 {
        base.push(("a", p, 1, format!("dosea{} doseb{}", filler((p % 7) as usize), filler((p % 5) as usize))));
    }
    let mut other_space = base.clone();
    for i in 0..3000 {
        other_space.push(("b", 1000 + i / 50, 1 + i % 50, format!("dosea dosea dosea dosea{}", filler(60))));
    }
    let mut unseen = base.clone();
    for i in 0..3000 {
        unseen.push(("a", 2000 + i / 50, 1 + i % 50, format!("dosea dosea{}", filler(3))));
    }
    let mut deleted = base.clone();
    for i in 0..3000 {
        deleted.push(("a", 3000 + i / 50, 1 + i % 50, format!("dosea dosea dosea{}", filler(80))));
    }
    let variants = vec![
        ("un autre espace (3 000 entrées)", other_space, vec![]),
        ("des pages non permises du même espace (3 000 entrées)", unseen, vec![]),
        ("3 000 entrées supprimées, pas encore fusionnées", deleted, (3000..3060).collect()),
    ];
    (base, variants)
}

pub fn r916(args: &Args) {
    let root = PathBuf::from(args.pos(0));
    let (base, variants) = dosed();
    let allowed = allowed_of(1..40);
    let x = small_index(&root.join("x"), &base, &[]);
    let variants: Vec<(&str, Opened)> = variants
        .iter()
        .enumerate()
        .map(|(i, (name, rows, gone))| (*name, small_index(&root.join(format!("y{i}")), rows, gone)))
        .collect();
    let asks = [
        ("dosea", ask(&["dosea"], None)),
        ("doseb", ask(&["doseb"], None)),
        ("dosea doseb", ask(&["dosea", "doseb"], None)),
        ("dos*", ask(&[], Some("dos"))),
        ("doseb dos*", ask(&["doseb"], Some("dos"))),
    ];
    let run = |o: &Opened, a: &Ask, note: Note| -> Vec<(u128, u64, u32)> {
        let s = o.reader.searcher();
        search(&s, &o.f, a, Some(&allowed), note).0.into_iter().map(|h| (h.page, h.blk, h.score_bits)).collect()
    };
    let mut fixed_same = true;
    let mut bm25_differs = 0;
    for (vname, v) in &variants {
        let s = v.reader.searcher();
        let deleted_docs: u32 = s.segment_readers().iter().map(|r| r.max_doc() - r.num_docs()).sum();
        println!("variante « {vname} » : {} entrées vivantes, {deleted_docs} supprimées encore dans les segments", s.num_docs());
        for (qname, a) in &asks {
            let (fx, fy) = (run(&x, a, Note::Fixed), run(v, a, Note::Fixed));
            let (bx, by) = (run(&x, a, Note::Bm25), run(v, a, Note::Bm25));
            let order = |r: &[(u128, u64, u32)]| r.iter().map(|h| (h.0, h.1)).collect::<Vec<_>>();
            let same_fixed = fx == fy;
            let same_bm25_order = order(&bx) == order(&by);
            let same_bm25_scores = bx == by;
            fixed_same &= same_fixed;
            if !same_bm25_order {
                bm25_differs += 1;
            }
            println!(
                "R916 | {vname} | « {qname} » | {} pages | statistiques constantes : mêmes pages, même ordre, mêmes notes au bit près = {same_fixed} | \
                 BM25 de l'index (témoin) : même ordre = {same_bm25_order}, mêmes notes = {same_bm25_scores}",
                fx.len()
            );
        }
    }
    println!(
        "R916 verdict : statistiques constantes, tout identique = {fixed_same} ; le témoin (BM25 de l'index) change d'ordre dans {bm25_differs} cas sur {}",
        variants.len() * asks.len()
    );
    assert!(fixed_same, "la note constante dépend de ce qui est hors du filtre");
    assert!(bm25_differs > 0, "le témoin ne voit rien : l'essai ne prouve rien");
}

// ---------------------------------------------------------------------------------------------
// R909 : la charge utile d'une validation, et ce qu'on lit après une validation coupée.

fn payload_schema() -> (Schema, Field) {
    let mut b = Schema::builder();
    let marker = b.add_u64_field("marker", FAST | STORED);
    b.add_text_field("text", STRING);
    (b.build(), marker)
}

fn max_marker(index: &Index) -> u64 {
    let reader = index.reader().unwrap();
    let s = reader.searcher();
    let mut max = 0;
    for seg in s.segment_readers() {
        let col = seg.fast_fields().u64("marker").unwrap();
        for doc in 0..seg.max_doc() {
            if !seg.is_deleted(doc) {
                max = max.max(col.first(doc).unwrap_or(0));
            }
        }
    }
    max
}

pub fn payload_child(args: &Args) {
    let dir = PathBuf::from(args.pos(0));
    let index = Index::open_in_dir(&dir).unwrap();
    let marker = index.schema().get_field("marker").unwrap();
    let text = index.schema().get_field("text").unwrap();
    let mut w: IndexWriter = index.writer_with_num_threads(1, 15_000_000).unwrap();
    let mut i: u64 = index.load_metas().unwrap().payload.and_then(|p| p.parse().ok()).unwrap_or(0);
    loop {
        i += 1;
        for k in 0..200 {
            let mut d = TantivyDocument::default();
            d.add_u64(marker, i);
            d.add_text(text, format!("m{i}-{k}"));
            w.add_document(d).unwrap();
        }
        let mut p = w.prepare_commit().unwrap();
        p.set_payload(&i.to_string());
        p.commit().unwrap();
        std::fs::write(dir.join("..").join("progress.txt"), i.to_string()).unwrap();
    }
}

pub fn payload(args: &Args) {
    let root = PathBuf::from(args.pos(0));
    let dir = root.join("index");
    fresh_dir(&root);
    fresh_dir(&dir);
    let (schema, marker) = payload_schema();
    let index = Index::create_in_dir(&dir, schema).unwrap();
    println!("R909 index neuf : charge utile {:?}", index.load_metas().unwrap().payload);
    {
        let mut w: IndexWriter = index.writer_with_num_threads(1, 15_000_000).unwrap();
        let mut d = TantivyDocument::default();
        d.add_u64(marker, 0);
        w.add_document(d).unwrap();
        let mut p = w.prepare_commit().unwrap();
        p.set_payload("0");
        p.commit().unwrap();
        // Une validation préparée, jeton posé, puis abandonnée : rien ne doit en rester.
        let mut d = TantivyDocument::default();
        d.add_u64(marker, 999_999);
        w.add_document(d).unwrap();
        let mut p = w.prepare_commit().unwrap();
        p.set_payload("jamais-validé");
        p.abort().unwrap();
    }
    let index = Index::open_in_dir(&dir).unwrap();
    let p = index.load_metas().unwrap().payload;
    println!("R909 après une validation préparée puis abandonnée : charge utile {p:?}, plus grand marqueur lisible {}", max_marker(&index));
    assert_eq!(p.as_deref(), Some("0"));
    assert_eq!(max_marker(&index), 0);
    drop(index);

    let trials: u32 = args.value("--trials").map(|v| v.parse().unwrap()).unwrap_or(25);
    let exe = std::env::current_exe().unwrap();
    let mut rng = Rng(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64);
    let (mut ahead, mut equal, mut bad) = (0, 0, 0);
    for trial in 0..trials {
        let mut child = std::process::Command::new(&exe).arg("payload-child").arg(&dir).spawn().unwrap();
        // Tué à un instant quelconque : c'est un processus de ce banc, lancé par lui.
        std::thread::sleep(Duration::from_micros(150_000 + rng.below(600_000)));
        child.kill().unwrap();
        child.wait().unwrap();
        let said: u64 = std::fs::read_to_string(root.join("progress.txt")).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        let t = Instant::now();
        let index = Index::open_in_dir(&dir).unwrap();
        let p: u64 = index.load_metas().unwrap().payload.unwrap().parse().unwrap();
        let read = t.elapsed();
        let m = max_marker(&index);
        let damaged = index.validate_checksum().unwrap();
        // L'écrivain se reprend-il (verrou rendu par le processus tué) ?
        let w: tantivy::Result<IndexWriter> = index.writer_with_num_threads(1, 15_000_000);
        let ok = m == p && (p == said || p == said + 1) && damaged.is_empty() && w.is_ok();
        if p == said + 1 {
            ahead += 1;
        } else if p == said {
            equal += 1;
        }
        if !ok {
            bad += 1;
        }
        println!(
            "R909 essai {trial} : le processus tué avait dit {said} ; charge utile relue {p} (en {:.2} ms), plus grand marqueur lisible {m}, \
             fichiers abîmés {}, écrivain repris = {} -> {}",
            ms(read),
            damaged.len(),
            w.is_ok(),
            if ok { "cohérent" } else { "INCOHÉRENT" }
        );
    }
    println!(
        "R909 verdict : {trials} arrêts brutaux, {equal} fois la charge utile est celle de la dernière validation dite, {ahead} fois celle d'une \
         validation faite et pas encore dite, {bad} incohérence(s)"
    );
    assert_eq!(bad, 0);
}

// ---------------------------------------------------------------------------------------------
// Questions (b) et (c) : les tokeniseurs.

fn tokens_of(a: &mut TextAnalyzer, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stream = a.token_stream(text);
    while let Some(t) = stream.next() {
        out.push(t.text.clone());
    }
    out
}

pub fn tokens() {
    let mut plain = analyzer(false);
    let mut stem = analyzer(true);
    println!("(b) SimpleTokenizer + RemoveLongFilter(41) + LowerCaser + AsciiFoldingFilter :");
    let lengths = format!("{} {} (40 puis 41 octets)", "q".repeat(40), "x".repeat(41));
    for sample in [
        "L'élève Éléonore a mangé à l'école, aujourd'hui.",
        "Cœur, œuvre, Æther, naïve, façade, GARÇON, straße",
        "user_account_id snake_case camelCase kebab-case e-mail C++ 3.14 v2.0.1",
        "中文搜索引擎测试 日本語のテキスト 한국어",
        "anticonstitutionnellementanticonstitutionnellement court (50 octets)",
lengths.as_str(),
        "e\u{301}le\u{300}ve (accents combinants)",
        "https://exemple.fr/chemin/page?x=1 jean.dupont@exemple.fr",
    ] {
        println!("  « {sample} »\n    -> {:?}", tokens_of(&mut plain, sample));
    }
    let found = |a: &mut TextAnalyzer, doc: &str, q: &str| {
        let d = tokens_of(a, doc);
        tokens_of(a, q).iter().all(|t| d.contains(t))
    };
    println!("  « élève » trouvé par « eleve » : {}", found(&mut plain, "un élève", "eleve"));
    println!("  « ÉLÈVE » trouvé par « élève » : {}", found(&mut plain, "ÉLÈVE", "élève"));
    println!("  « snake_case » trouvé par « snake » : {}", found(&mut plain, "snake_case", "snake"));
    println!("  « 中文搜索引擎测试 » trouvé par « 搜索 » : {}", found(&mut plain, "中文搜索引擎测试", "搜索"));

    println!("(c) avec Stemmer(French) en plus :");
    let fr = [
        ("cheval", "chevaux"),
        ("journal", "journaux"),
        ("page", "pages"),
        ("manger", "mangé"),
        ("manger", "mangeons"),
        ("national", "nationale"),
        ("national", "nationaux"),
        ("recherche", "rechercher"),
        ("recherche", "recherches"),
        ("décision", "décisions"),
        ("décider", "décision"),
        ("nouveau", "nouvelle"),
        ("œil", "yeux"),
        ("créer", "créée"),
        ("travail", "travaux"),
        ("réunion", "réunions"),
        ("petit", "petite"),
        ("aller", "vais"),
        ("être", "sont"),
        ("écrire", "écrit"),
    ];
    let mut plus = 0;
    for (a, b) in fr {
        let (sa, sb) = (tokens_of(&mut stem, a), tokens_of(&mut stem, b));
        let same = sa == sb;
        let already = tokens_of(&mut plain, a) == tokens_of(&mut plain, b);
        if same && !already {
            plus += 1;
        }
        println!("  fr  {a} / {b} : {:?} / {:?} -> {}", sa, sb, if same { "réunis" } else { "séparés" });
    }
    println!("  français : {plus} paires sur {} réunies par la racinisation (aucune ne l'est sans elle)", fr.len());
    // Des mots anglais de sens différent : la racinisation française les réunit-elle ?
    let en = [
        ("universe", "university"),
        ("general", "generous"),
        ("police", "policy"),
        ("organ", "organization"),
        ("import", "important"),
        ("port", "portion"),
        ("action", "active"),
        ("station", "static"),
        ("message", "messy"),
        ("mine", "minor"),
        ("pages", "pager"),
        ("sale", "sales"),
        ("change", "changer"),
        ("content", "contents"),
        ("test", "tester"),
        ("tests", "testing"),
        ("server", "serve"),
        ("serving", "serve"),
        ("running", "run"),
        ("files", "file"),
        ("user", "use"),
        ("users", "user"),
        ("document", "documents"),
        ("documentation", "document"),
        ("marine", "marin"),
        ("author", "authority"),
        ("commit", "committee"),
        ("relation", "relative"),
        ("nation", "native"),
        ("fortune", "fort"),
    ];
    let (mut wrong, mut missed) = (0, 0);
    const SAME_SENSE: &[&str] = &["sales", "contents", "tester", "testing", "serve", "run", "file", "user", "documents"];
    for (a, b) in en {
        let (sa, sb) = (tokens_of(&mut stem, a), tokens_of(&mut stem, b));
        let same = sa == sb;
        let related = SAME_SENSE.contains(&b) && !(a == "user" && b == "use") && !(a == "server");
        if same && !related {
            wrong += 1;
        }
        if !same && related {
            missed += 1;
        }
        println!(
            "  en  {a} / {b} : {:?} / {:?} -> {}{}",
            sa,
            sb,
            if same { "réunis" } else { "séparés" },
            if same && !related { " (À TORT)" } else { "" }
        );
    }
    println!("  anglais : {wrong} paires de sens différent réunies à tort, {missed} formes d'un même mot laissées séparées, sur {}", en.len());
}

// ---------------------------------------------------------------------------------------------
// La forme hostile : des blocs de 64 Kio de mots de 40 octets tous différents (R920).

pub fn hostile(args: &Args) {
    let dir = PathBuf::from(args.pos(0));
    let blocks: u32 = args.value("--blocks").map(|v| v.parse().unwrap()).unwrap_or(1000);
    let with_starts = !args.flag("--no-starts");
    fresh_dir(&dir);
    let (schema, f) = schema(with_starts);
    let index = Index::create_in_dir(&dir, schema).unwrap();
    register(&index);
    let mut w = writer(&index);
    let watch = PeakWatch::start();
    let mut scratch = Scratch { folded: String::new() };
    let mut rng = Rng(5);
    let mut bytes = 0u64;
    let mut text = String::with_capacity(65_536);
    let start = Instant::now();
    for b in 0..blocks {
        text.clear();
        while text.len() + LONGEST_WORD + 1 <= 65_536 {
            for _ in 0..LONGEST_WORD {
                text.push((b'a' + rng.below(26) as u8) as char);
            }
            text.push(' ');
        }
        bytes += text.len() as u64;
        // 128 blocs de 64 Kio font la borne de 8 Mio d'une page (R912).
        let e = Entry { space: "a", page: b / 128, block: 1 + b % 128, title: "Projet hostile", text: &text };
        w.add_document(make_doc(&f, &e, true, &mut scratch)).unwrap();
        if b % 100 == 99 {
            w.commit().unwrap();
        }
    }
    w.commit().unwrap();
    let indexed = start.elapsed();
    w.wait_merging_threads().unwrap();
    let peak = watch.finish();
    println!(
        "MESURE hostile débuts={with_starts} : {blocks} blocs de 64 Kio, {:.1} Mo reçus, indexés en {:.1} s ({:.2} Mo/s), fusions comprises {:.1} s ; \
         pointe de mémoire de travail {:.0} Mo ; dossier {:.1} Mo, soit {:.2} fois les octets reçus",
        mb(bytes),
        indexed.as_secs_f64(),
        mb(bytes) / indexed.as_secs_f64(),
        start.elapsed().as_secs_f64(),
        mb(peak),
        mb(dir_size(&dir)),
        dir_size(&dir) as f64 / bytes as f64
    );
    print_sizes(&dir);
}
