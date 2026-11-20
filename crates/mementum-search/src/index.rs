//! L'index plein texte (Rulings R900, R902, R916, R920 à R923, R925, R926, R929, R930), derrière
//! [`SearchIndex`] : Tantivy, dans un dossier à lui, sans rien savoir du store ni des droits.
//!
//! **Ce que l'index porte.** Une entrée par bloc, qui recopie le titre de sa page, et une entrée
//! par page pour son titre seul. Aucun droit, aucun espace, pas l'état de corbeille : ce qui est
//! permis lui est donné à chaque recherche ([`PageSet`]), et il l'applique dans son collecteur,
//! avant de grouper par page (Ruling R921).
//!
//! **Ce qu'une recherche ne lit jamais** (Ruling R916) : une statistique de l'index. L'index est
//! un pour l'instance ; une note qui lirait le nombre d'entrées, la fréquence d'un mot ou une
//! longueur moyenne dirait, par l'ordre des résultats, ce qu'écrivent des gens que la personne ne
//! voit pas. Quatre règles tiennent cela, chacune avec son test (`tests/index.rs`) :
//! - la note d'un mot est la BM25 du moteur sous des statistiques constantes ([`Constants`]),
//!   jamais celles de l'index (Ruling R923) ;
//! - **chaque note de mot est posée sur une grille** ([`on_grid`]) avant d'être additionnée
//!   (Ruling R965) : le moteur range les mots voulus d'une requête par leur fréquence dans le
//!   segment (`intersect_scorers` de Tantivy 0.27), donc d'après les autres, puis additionne
//!   leurs notes dans cet ordre, et une somme de trois flottants ou plus dépend de son ordre ;
//!   sur la grille, les sommes sont exactes, donc les mêmes dans tout ordre ;
//! - deux notes égales se départagent par une valeur tirée de l'entrée seule, jamais par son
//!   adresse dans l'index ni par le rang de son bloc (Ruling R922) ;
//! - un début de mot est un terme exact d'un champ à lui, jamais un parcours du dictionnaire
//!   (Ruling R920), et l'extrait est bâti ici, pas par le moteur.
//!
//! Tout est synchrone : l'appelant mène ces fonctions sur le fil bloquant.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::Entry;
use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tantivy::collector::{Collector, SegmentCollector};
use tantivy::columnar::Column;
use tantivy::directory::error::LockError;
use tantivy::directory::{Directory, INDEX_WRITER_LOCK, MmapDirectory};
use tantivy::indexer::{IndexWriterOptions, LogMergePolicy};
use tantivy::query::{
    Bm25StatisticsProvider, BooleanQuery, ConstScoreQuery, EnableScoring, Explanation, Occur,
    Query as EngineQuery, Scorer, TermQuery, Weight,
};
use tantivy::schema::{
    FAST, Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};
use tantivy::{
    DocAddress, DocId, DocSet, Index, IndexReader, IndexWriter, ReloadPolicy, Score, Searcher,
    SegmentOrdinal, SegmentReader, TantivyDocument, TantivyError, Term,
};
use uuid::Uuid;

use crate::fold::{Word, indexed_words, nfc};
use crate::query::{MIN_PREFIX_CHARS, Query};

/// Longueur maximale, en caractères du mot plié, d'un début de mot indexé (Ruling R920) : un mot
/// donne au plus neuf termes au champ des débuts (ses débuts de 2 à 10 caractères), jamais un par
/// caractère. Un préfixe tapé plus long est cherché par ses dix premiers caractères, puis
/// vérifié sur le texte gardé.
pub const MAX_START_CHARS: usize = 10;
/// Nombre maximal d'entrées vérifiées pour un préfixe long, par requête (Rulings R920, R929).
pub const MAX_VERIFIED_ENTRIES: usize = 2_000;
/// Nombre maximal d'entrées vérifiées pour un préfixe long, par page (Ruling R929) : une page de
/// dix mille blocs qui partagent le début ne prend pas toutes les vérifications.
pub const MAX_VERIFIED_PER_PAGE: usize = 20;
/// Octets de titres et de textes gardés relus au plus pour vérifier un préfixe long, par requête
/// (Ruling R967) : l'entrée qui passe la borne est la dernière vérifiée.
pub const MAX_VERIFIED_BYTES: usize = 4 << 20;
/// Entrées gardées au plus par le collecteur, pour un préfixe long, en plus de la meilleure de
/// chaque page permise (Ruling R991) : huit fois ce qu'une requête vérifie. Les moins bien
/// rangées sortent ; aucune d'elles n'aurait été vérifiée.
pub const MAX_KEPT_CANDIDATES: usize = 8 * MAX_VERIFIED_ENTRIES;
/// Longueur maximale d'un extrait, en caractères, points de suspension compris.
pub const MAX_EXCERPT_CHARS: usize = 240;
/// Tampon de l'écrivain (Ruling R925).
pub const WRITER_BUFFER_BYTES: usize = 50_000_000;

/// Blocs de textes gardés que le lecteur tient décompressés, par segment (le moteur en tient 100
/// par défaut). Un bloc fait 16 Kio, ou une entrée entière si elle est plus longue (64 Kio au
/// plus) ; une recherche relit vingt entrées, ou deux mille pour un préfixe long, dans un ordre
/// qui n'est pas celui du disque, si bien qu'un grand cache ne sert guère qu'à la même requête
/// rejouée. Huit blocs bornent ce que le cache tient à un demi-Mio par segment.
const STORE_CACHE_BLOCKS: usize = 8;

/// La forme de l'index, écrite en tête de la charge utile de chaque validation : un dossier
/// validé sous une autre forme est vidé à l'ouverture, comme un dossier d'un autre schéma. Avance
/// à tout changement de ce qu'une entrée porte sans que le schéma change (la longueur des débuts
/// de mot, la clé de départage).
const INDEX_FORM: &str = "mementum-search/1";

/// À l'ouverture, le temps pendant lequel on attend le verrou de l'écrivain qu'on vient de
/// rendre pour que le moteur le reprenne, et la pause entre deux essais (Ruling R1200).
const WRITER_LOCK_WAIT: Duration = Duration::from_secs(2);
const WRITER_LOCK_PAUSE: Duration = Duration::from_millis(5);

/// Ce que le titre pèse de plus que le texte dans une note.
const TITLE_BOOST: Score = 3.0;
/// La note d'un début de mot, constante (Ruling R930, limite L-q) : dans un texte, dans un titre.
/// Sur la grille.
const TEXT_START_NOTE: Score = 1.0;
const TITLE_START_NOTE: Score = TEXT_START_NOTE * TITLE_BOOST;
/// Le pas de la grille des notes : une note de mot est un multiple de `1 / GRID`. Une note de mot
/// reste sous 64 (titre compris), une requête en additionne au plus une vingtaine : toutes les
/// sommes tiennent dans les 24 bits de mantisse d'un `f32`, et sont donc exactes.
const GRID: Score = 4096.0;

/// Les statistiques données au moteur à la place de celles de l'index (Rulings R916, R923) : un
/// million d'entrées, un mot porté par mille d'entre elles, quatre mots par titre, vingt par
/// texte. Seul leur rapport compte ; ce sont des constantes du programme.
const STAT_ENTRIES: u64 = 1_000_000;
const STAT_ENTRIES_WITH_A_WORD: u64 = 1_000;
const STAT_TITLE_WORDS: u64 = 4;
const STAT_TEXT_WORDS: u64 = 20;

const WORDS: &str = "mm_words";
const STARTS: &str = "mm_starts";

// ---------------------------------------------------------------------------------------------
// Les pages permises.

/// Un hachage pour des identifiants déjà tirés au hasard : un mélange, pas SipHash (l'ensemble
/// est interrogé une fois par entrée trouvée).
#[derive(Default, Clone, Copy)]
struct Mixed(u64);

impl Hasher for Mixed {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn write_u128(&mut self, id: u128) {
        let mut x = (id as u64) ^ ((id >> 64) as u64).rotate_left(32);
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        x ^= x >> 33;
        self.0 = x;
    }
}

type Mixing = BuildHasherDefault<Mixed>;

/// Les pages qu'une recherche peut rendre : celles que le compte voit, hors corbeille, dans
/// l'espace cherché. C'est l'appelant qui le bâtit, des droits ; l'index n'en sait rien d'autre
/// (Ruling R921 : il n'a pas de champ d'espace, et toute la séparation des espaces tient ici).
#[derive(Debug, Clone, Default)]
pub struct PageSet(Arc<HashSet<u128, Mixing>>);

impl FromIterator<Uuid> for PageSet {
    fn from_iter<I: IntoIterator<Item = Uuid>>(pages: I) -> PageSet {
        PageSet(Arc::new(
            pages.into_iter().map(|page| page.as_u128()).collect(),
        ))
    }
}

impl PageSet {
    pub fn contains(&self, page: &Uuid) -> bool {
        self.0.contains(&page.as_u128())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------
// Erreurs, résultats.

/// Une erreur de l'index. Hors [`IndexError::Busy`], son texte porte des chemins du serveur :
/// il va au journal, jamais dans une réponse.
#[derive(Debug)]
pub enum IndexError {
    /// Un autre écrivain tient l'index : un serveur qui tourne, ou `mementum reindex`.
    Busy,
    /// Le dossier donné porte autre chose qu'un index : rien n'y est touché.
    NotAnIndex,
    /// Le moteur a refusé.
    Engine(String),
    /// Le dossier n'a pas pu être lu, créé ou vidé.
    Io(std::io::Error),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndexError::Busy => write!(
                f,
                "l'index de recherche est déjà ouvert par un autre processus (un serveur qui \
                 tourne, ou une réindexation en cours)"
            ),
            IndexError::NotAnIndex => write!(
                f,
                "le dossier de l'index de recherche n'est pas un index : il porte d'autres \
                 fichiers, et rien n'y a été touché"
            ),
            IndexError::Engine(error) => write!(f, "index de recherche : {error}"),
            IndexError::Io(error) => write!(f, "dossier de l'index de recherche : {error}"),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<TantivyError> for IndexError {
    fn from(error: TantivyError) -> IndexError {
        match error {
            TantivyError::LockFailure(..) => IndexError::Busy,
            other => IndexError::Engine(other.to_string()),
        }
    }
}

impl From<std::io::Error> for IndexError {
    fn from(error: std::io::Error) -> IndexError {
        IndexError::Io(error)
    }
}

/// Une page trouvée, par sa meilleure entrée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub node_id: Uuid,
    /// La clé du bloc trouvé, telle que donnée à [`SearchIndex::put_entry`] (un texte venu
    /// d'ailleurs, ou une clé de remplacement : [`crate::is_stand_in`]) ; absente quand la page
    /// est trouvée par son titre.
    pub block_id: Option<String>,
    /// Un morceau du texte du bloc, tel quel : jamais de balise, rien d'échappé. Vide pour une
    /// page trouvée par son titre.
    pub excerpt: String,
    /// Les mots trouvés dans `excerpt` : des plages `[début, fin[` en unités UTF-16, comme les
    /// places d'une chaîne JavaScript, dans l'ordre et sans chevauchement.
    pub marks: Vec<(u32, u32)>,
}

/// Une réponse : des pages, et s'il en reste après elles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub pages: Vec<Hit>,
    pub more: bool,
}

/// Ce qu'une recherche a fait, pour les tests (exporté sous la fonction `test-hooks`).
#[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trace {
    /// La note de chaque page rendue, dans l'ordre.
    pub notes: Vec<f32>,
    /// Les entrées arrivées au collecteur.
    pub collected: u64,
    /// Celles d'une page permise, les seules à atteindre le groupement par page.
    pub grouped: u64,
    /// Les entrées que le collecteur a gardées en tout.
    pub kept: usize,
    /// Les termes exacts que la requête lit.
    pub terms: usize,
    /// Les entrées relues pour vérifier un préfixe long, dans l'ordre.
    pub verified: Vec<(Uuid, Option<String>)>,
}

/// Les témoins des tests : ce qu'il ne faut pas faire, posé le temps d'une preuve (exporté sous
/// la fonction `test-hooks`, sans laquelle rien ne les pose).
#[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Witness {
    /// La note lit les statistiques de l'index (ce que R916 interdit).
    IndexStatistics,
    /// Deux notes égales se départagent par l'adresse de l'entrée (ce que R922 interdit).
    AddressTieBreak,
}

/// Les réglages d'un index, pour les mesures (exporté sous la fonction `test-hooks`) : le
/// serveur prend ceux de [`Options::default`].
#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub buffer_bytes: usize,
    /// Un segment de plus de tant d'entrées n'est plus fusionné.
    pub max_docs_before_merge: Option<usize>,
    pub keep_title: bool,
    pub keep_text: bool,
    /// Témoin de R1200 : à l'ouverture, le verrou de l'écrivain est repris par un autre, pour
    /// cette durée, dès que l'ouverture l'a rendu et avant que le moteur le reprenne (ce que
    /// fait, sous Unix, le processus fils qu'un autre fil lance à cet instant).
    pub writer_lock_taken_for: Option<Duration>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            buffer_bytes: WRITER_BUFFER_BYTES,
            max_docs_before_merge: None,
            keep_title: true,
            keep_text: true,
            writer_lock_taken_for: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Le schéma, les mots.

#[derive(Clone, Copy)]
struct Fields {
    /// L'identifiant de la page, en terme exact : retirer toutes ses entrées. Pas pour filtrer.
    node: Field,
    /// `<page>/<clé du bloc>`, en terme exact : remplacer une entrée.
    entry: Field,
    /// La clé du bloc, gardée ; absente de l'entrée de titre.
    block_id: Field,
    title: Field,
    text: Field,
    title_starts: Field,
    text_starts: Field,
    /// Colonnes rapides : l'identifiant de la page et la clé de départage, en deux entiers.
    page_hi: Field,
    page_lo: Field,
    blk_hi: Field,
    blk_lo: Field,
}

const PAGE_HI: &str = "page_hi";
const PAGE_LO: &str = "page_lo";
const BLK_HI: &str = "blk_hi";
const BLK_LO: &str = "blk_lo";

fn schema(options: Options) -> (Schema, Fields) {
    let mut builder = Schema::builder();
    // Cherchés avec leurs fréquences et leur longueur (la note), sans positions.
    let searched = |keep: bool| {
        let options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(WORDS)
                .set_index_option(IndexRecordOption::WithFreqs)
                .set_fieldnorms(true),
        );
        if keep { options.set_stored() } else { options }
    };
    // Les débuts de mot : sans fréquence ni longueur, leur note est constante.
    let starts = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(STARTS)
            .set_index_option(IndexRecordOption::Basic)
            .set_fieldnorms(false),
    );
    let fields = Fields {
        node: builder.add_text_field("node", STRING),
        entry: builder.add_text_field("entry", STRING),
        block_id: builder.add_text_field("block_id", STORED),
        title: builder.add_text_field("title", searched(options.keep_title)),
        text: builder.add_text_field("text", searched(options.keep_text)),
        title_starts: builder.add_text_field("title_starts", starts.clone()),
        text_starts: builder.add_text_field("text_starts", starts),
        page_hi: builder.add_u64_field(PAGE_HI, FAST),
        page_lo: builder.add_u64_field(PAGE_LO, FAST),
        blk_hi: builder.add_u64_field(BLK_HI, FAST),
        blk_lo: builder.add_u64_field(BLK_LO, FAST),
    };
    (builder.build(), fields)
}

type FoldedWords<'a> = Box<dyn Iterator<Item = (Word<'a>, String)> + 'a>;

/// L'analyseur des champs cherchés : les mots de [`indexed_words`], tels quels (Ruling R942).
#[derive(Clone)]
struct Words;

struct WordStream<'a> {
    words: FoldedWords<'a>,
    token: Token,
}

impl Tokenizer for Words {
    type TokenStream<'a> = WordStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> WordStream<'a> {
        WordStream {
            words: Box::new(indexed_words(text)),
            token: Token::default(),
        }
    }
}

impl TokenStream for WordStream<'_> {
    fn advance(&mut self) -> bool {
        let Some((word, folded)) = self.words.next() else {
            return false;
        };
        self.token.offset_from = word.start;
        self.token.offset_to = word.start + word.raw.len();
        self.token.position = self.token.position.wrapping_add(1);
        self.token.text = folded;
        true
    }

    fn token(&self) -> &Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.token
    }
}

/// L'analyseur des champs des débuts de mot : pour chaque mot de [`indexed_words`], ses débuts
/// de [`MIN_PREFIX_CHARS`] à [`MAX_START_CHARS`] caractères du mot plié (Ruling R920).
#[derive(Clone)]
struct Starts;

struct StartStream<'a> {
    words: FoldedWords<'a>,
    /// Le mot en cours, plié, le nombre de caractères de son dernier début rendu et où il finit.
    word: String,
    chars: usize,
    end: usize,
    token: Token,
}

impl Tokenizer for Starts {
    type TokenStream<'a> = StartStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> StartStream<'a> {
        StartStream {
            words: Box::new(indexed_words(text)),
            word: String::new(),
            chars: 0,
            end: 0,
            token: Token::default(),
        }
    }
}

impl TokenStream for StartStream<'_> {
    fn advance(&mut self) -> bool {
        loop {
            if self.chars >= MIN_PREFIX_CHARS
                && self.chars < MAX_START_CHARS
                && let Some(next) = self.word[self.end..].chars().next()
            {
                self.chars += 1;
                self.end += next.len_utf8();
                break;
            }
            let Some((_, folded)) = self.words.next() else {
                return false;
            };
            self.word = folded;
            self.chars = 0;
            // Où finissent ses premiers caractères : un mot plus court n'a aucun début.
            let mut ends = self
                .word
                .char_indices()
                .map(|(at, c)| at + c.len_utf8())
                .skip(MIN_PREFIX_CHARS - 1);
            if let Some(end) = ends.next() {
                self.chars = MIN_PREFIX_CHARS;
                self.end = end;
                break;
            }
        }
        self.token.position = self.token.position.wrapping_add(1);
        self.token.text.clear();
        self.token.text.push_str(&self.word[..self.end]);
        true
    }

    fn token(&self) -> &Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.token
    }
}

/// Vrai si un mot indexé de `text` commence par `prefix` (déjà plié).
fn has_start(text: &str, prefix: &str) -> bool {
    indexed_words(text).any(|(_, folded)| folded.starts_with(prefix))
}

// ---------------------------------------------------------------------------------------------
// La note.

/// Les statistiques constantes (Rulings R916, R923) : rien n'est lu dans l'index.
struct Constants {
    title: Field,
}

impl Bm25StatisticsProvider for Constants {
    fn total_num_tokens(&self, field: Field) -> tantivy::Result<u64> {
        let words = if field == self.title {
            STAT_TITLE_WORDS
        } else {
            STAT_TEXT_WORDS
        };
        Ok(words * STAT_ENTRIES)
    }

    fn total_num_docs(&self) -> tantivy::Result<u64> {
        Ok(STAT_ENTRIES)
    }

    fn doc_freq(&self, _term: &Term) -> tantivy::Result<u64> {
        Ok(STAT_ENTRIES_WITH_A_WORD)
    }
}

/// Pose une note de mot sur la grille, au plus proche, sans jamais la rendre nulle. Par une
/// conversion en entier : c'est le chemin de chaque entrée trouvée.
fn on_grid(note: Score) -> Score {
    ((note * GRID + 0.5) as u32).max(1) as Score / GRID
}

/// Un mot entier dans un champ : le terme exact du moteur, sa note multipliée par `boost` puis
/// posée sur la grille (voir le module).
#[derive(Clone, Debug)]
struct Exact {
    term: TermQuery,
    boost: Score,
}

impl EngineQuery for Exact {
    fn weight(&self, scoring: EnableScoring<'_>) -> tantivy::Result<Box<dyn Weight>> {
        Ok(Box::new(ExactWeight {
            term: self.term.weight(scoring)?,
            boost: self.boost,
        }))
    }

    fn query_terms<'a>(&'a self, visitor: &mut dyn FnMut(&'a Term, bool)) {
        self.term.query_terms(visitor);
    }
}

struct ExactWeight {
    term: Box<dyn Weight>,
    boost: Score,
}

impl Weight for ExactWeight {
    fn scorer(&self, reader: &SegmentReader, boost: Score) -> tantivy::Result<Box<dyn Scorer>> {
        Ok(Box::new(OnGrid {
            term: self.term.scorer(reader, 1.0)?,
            boost: boost * self.boost,
        }))
    }

    fn explain(&self, reader: &SegmentReader, doc: DocId) -> tantivy::Result<Explanation> {
        self.term.explain(reader, doc)
    }
}

struct OnGrid {
    term: Box<dyn Scorer>,
    boost: Score,
}

impl DocSet for OnGrid {
    fn advance(&mut self) -> DocId {
        self.term.advance()
    }

    fn seek(&mut self, target: DocId) -> DocId {
        self.term.seek(target)
    }

    fn doc(&self) -> DocId {
        self.term.doc()
    }

    fn size_hint(&self) -> u32 {
        self.term.size_hint()
    }

    fn cost(&self) -> u64 {
        self.term.cost()
    }
}

impl Scorer for OnGrid {
    fn score(&mut self) -> Score {
        on_grid(self.term.score() * self.boost)
    }
}

/// La requête du moteur, bâtie par programme : aucun analyseur de requêtes, des termes exacts
/// seulement. Rend aussi le nombre de termes qu'elle lit.
fn engine_query(fields: &Fields, query: &Query) -> (BooleanQuery, usize) {
    let mut terms = 0;
    let mut whole = |field: Field, word: &str, boost: Score| -> Box<dyn EngineQuery> {
        terms += 1;
        Box::new(Exact {
            term: TermQuery::new(
                Term::from_field_text(field, word),
                IndexRecordOption::WithFreqs,
            ),
            boost,
        })
    };
    // L'un ou l'autre, leurs notes additionnées.
    let any = |parts: Vec<Box<dyn EngineQuery>>| -> Box<dyn EngineQuery> {
        Box::new(BooleanQuery::new(
            parts
                .into_iter()
                .map(|part| (Occur::Should, part))
                .collect(),
        ))
    };
    let mut wanted: Vec<(Occur, Box<dyn EngineQuery>)> = Vec::new();
    for word in &query.terms {
        let parts = vec![
            whole(fields.title, word, TITLE_BOOST),
            whole(fields.text, word, 1.0),
        ];
        wanted.push((Occur::Must, any(parts)));
    }
    if let Some(prefix) = &query.prefix {
        // Le mot entier ou le début de mot (Ruling R930). Le début est un terme exact du champ
        // des débuts ; au-delà de dix caractères, celui de ses dix premiers, sans note : la note
        // du début est ajoutée à la vérification, pour le champ qui porte vraiment le préfixe.
        let (cut, long) = cut_start(prefix);
        let mut parts = vec![
            whole(fields.title, prefix, TITLE_BOOST),
            whole(fields.text, prefix, 1.0),
        ];
        for (field, note) in [
            (fields.title_starts, TITLE_START_NOTE),
            (fields.text_starts, TEXT_START_NOTE),
        ] {
            terms += 1;
            let start = TermQuery::new(Term::from_field_text(field, cut), IndexRecordOption::Basic);
            let note = if long { 0.0 } else { note };
            parts.push(Box::new(ConstScoreQuery::new(Box::new(start), note)));
        }
        wanted.push((Occur::Must, any(parts)));
    }
    (BooleanQuery::new(wanted), terms)
}

/// Les [`MAX_START_CHARS`] premiers caractères d'un préfixe, et s'il est plus long qu'eux.
fn cut_start(prefix: &str) -> (&str, bool) {
    match prefix.char_indices().nth(MAX_START_CHARS) {
        Some((end, _)) => (&prefix[..end], true),
        None => (prefix, false),
    }
}

// ---------------------------------------------------------------------------------------------
// Le collecteur : le filtre, puis le groupement par page (Rulings R902, R921, R922).

/// Une entrée trouvée.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    note: Score,
    /// La clé de départage de l'entrée (Ruling R922) : zéro pour l'entrée de titre.
    key: u128,
    address: DocAddress,
}

/// Comment ranger : toujours par la note, puis par une valeur de l'entrée seule.
#[derive(Clone, Copy, Default)]
struct Ranking {
    /// Le témoin de R922 : départager par l'adresse dans l'index.
    by_address: bool,
}

impl Ranking {
    /// L'ordre de deux entrées d'une même page : la mieux notée, puis la plus petite clé.
    fn in_page(self, a: &Candidate, b: &Candidate) -> Ordering {
        b.note.total_cmp(&a.note).then_with(|| {
            if self.by_address {
                a.address.cmp(&b.address)
            } else {
                a.key.cmp(&b.key)
            }
        })
    }

    /// L'ordre de deux entrées de pages quelconques : la note, la page, la clé.
    fn across(self, a: &(u128, Candidate), b: &(u128, Candidate)) -> Ordering {
        b.1.note.total_cmp(&a.1.note).then_with(|| {
            if self.by_address {
                a.1.address.cmp(&b.1.address)
            } else {
                a.0.cmp(&b.0).then(a.1.key.cmp(&b.1.key))
            }
        })
    }
}

/// Les meilleures entrées d'une page : la première, et les suivantes quand il en faut plusieurs
/// (un préfixe long à vérifier).
struct Top {
    best: Candidate,
    rest: Vec<Candidate>,
}

impl Top {
    /// Propose une entrée ; rend vrai si la page en garde une de plus.
    fn offer(&mut self, mut candidate: Candidate, keep: usize, ranking: Ranking) -> bool {
        if ranking.in_page(&candidate, &self.best) == Ordering::Less {
            std::mem::swap(&mut candidate, &mut self.best);
        }
        if keep <= 1 {
            return false;
        }
        let at = self
            .rest
            .partition_point(|kept| ranking.in_page(kept, &candidate) != Ordering::Greater);
        if at >= keep - 1 {
            return false;
        }
        // La dernière sort avant l'insertion : le tableau ne grandit jamais au-delà de ce
        // qu'une page garde.
        let grows = self.rest.len() < keep - 1;
        if !grows {
            self.rest.pop();
        }
        self.rest.insert(at, candidate);
        grows
    }
}

#[derive(Default)]
struct Groups {
    pages: HashMap<u128, Top, Mixing>,
    /// Les entrées gardées en plus de la meilleure de chaque page.
    extras: usize,
    collected: u64,
    grouped: u64,
}

impl Groups {
    /// Tient les entrées gardées en plus sous [`MAX_KEPT_CANDIDATES`] (Ruling R991) : passé le
    /// double, seules restent les mieux rangées, dans l'ordre (note, page, clé). Une entrée
    /// écartée est derrière [`MAX_KEPT_CANDIDATES`] autres, qui ne sont remplacées que par de
    /// mieux rangées : ce qui reste à la fin ne dépend pas de l'ordre d'arrivée, donc pas de
    /// l'index.
    fn bound(&mut self, ranking: Ranking, slack: usize) {
        if self.extras <= MAX_KEPT_CANDIDATES + slack {
            return;
        }
        let mut all: Vec<(u128, Candidate)> = Vec::with_capacity(self.extras);
        for (page, top) in &self.pages {
            all.extend(top.rest.iter().map(|candidate| (*page, *candidate)));
        }
        let (_, last, _) =
            all.select_nth_unstable_by(MAX_KEPT_CANDIDATES - 1, |a, b| ranking.across(a, b));
        let last = *last;
        let mut extras = 0;
        for (page, top) in &mut self.pages {
            // Dans une page, l'ordre de `rest` est celui de `across`.
            let kept = top.rest.partition_point(|candidate| {
                ranking.across(&(*page, *candidate), &last) != Ordering::Greater
            });
            top.rest.truncate(kept);
            top.rest.shrink_to_fit();
            extras += kept;
        }
        self.extras = extras;
    }
}

struct ByPage {
    allowed: PageSet,
    /// Combien d'entrées garder par page.
    keep: usize,
    ranking: Ranking,
}

struct ByPageSegment {
    segment: SegmentOrdinal,
    page_hi: Column<u64>,
    page_lo: Column<u64>,
    blk_hi: Column<u64>,
    blk_lo: Column<u64>,
    allowed: PageSet,
    keep: usize,
    ranking: Ranking,
    groups: Groups,
}

fn wide(hi: &Column<u64>, lo: &Column<u64>, doc: DocId) -> u128 {
    (u128::from(hi.first(doc).unwrap_or(0)) << 64) | u128::from(lo.first(doc).unwrap_or(0))
}

impl Collector for ByPage {
    type Fruit = Groups;
    type Child = ByPageSegment;

    fn for_segment(
        &self,
        segment: SegmentOrdinal,
        reader: &SegmentReader,
    ) -> tantivy::Result<ByPageSegment> {
        let columns = reader.fast_fields();
        Ok(ByPageSegment {
            segment,
            page_hi: columns.u64(PAGE_HI)?,
            page_lo: columns.u64(PAGE_LO)?,
            blk_hi: columns.u64(BLK_HI)?,
            blk_lo: columns.u64(BLK_LO)?,
            allowed: self.allowed.clone(),
            keep: self.keep,
            ranking: self.ranking,
            groups: Groups::default(),
        })
    }

    fn requires_scoring(&self) -> bool {
        true
    }

    fn merge_fruits(&self, fruits: Vec<Groups>) -> tantivy::Result<Groups> {
        let mut fruits = fruits.into_iter();
        let mut all = fruits.next().unwrap_or_default();
        for groups in fruits {
            all.collected += groups.collected;
            all.grouped += groups.grouped;
            for (page, top) in groups.pages {
                match all.pages.entry(page) {
                    Entry::Vacant(vacant) => {
                        all.extras += top.rest.len();
                        vacant.insert(top);
                    }
                    Entry::Occupied(mut occupied) => {
                        let kept = occupied.get_mut();
                        for candidate in std::iter::once(top.best).chain(top.rest) {
                            if kept.offer(candidate, self.keep, self.ranking) {
                                all.extras += 1;
                            }
                        }
                    }
                }
            }
            all.bound(self.ranking, MAX_KEPT_CANDIDATES);
        }
        all.bound(self.ranking, 0);
        Ok(all)
    }
}

impl SegmentCollector for ByPageSegment {
    type Fruit = Groups;

    fn collect(&mut self, doc: DocId, note: Score) {
        self.groups.collected += 1;
        // Le filtre d'abord (Ruling R921) : une entrée d'une page non permise n'atteint ni le
        // groupement, ni la vérification d'un préfixe, ni aucun compte.
        let page = wide(&self.page_hi, &self.page_lo, doc);
        if !self.allowed.0.contains(&page) {
            return;
        }
        self.groups.grouped += 1;
        let address = DocAddress::new(self.segment, doc);
        let one_more = match self.groups.pages.entry(page) {
            Entry::Vacant(vacant) => {
                let key = wide(&self.blk_hi, &self.blk_lo, doc);
                vacant.insert(Top {
                    best: Candidate { note, key, address },
                    rest: Vec::new(),
                });
                false
            }
            Entry::Occupied(mut occupied) => {
                let top = occupied.get_mut();
                // La clé de départage n'est lue qu'à note égale ou meilleure.
                if self.keep == 1 && note < top.best.note {
                    return;
                }
                let key = wide(&self.blk_hi, &self.blk_lo, doc);
                top.offer(Candidate { note, key, address }, self.keep, self.ranking)
            }
        };
        if one_more {
            self.groups.extras += 1;
            self.groups.bound(self.ranking, MAX_KEPT_CANDIDATES);
        }
    }

    fn harvest(self) -> Groups {
        self.groups
    }
}

// ---------------------------------------------------------------------------------------------
// L'extrait (Ruling R923) : bâti ici, sur le texte gardé de l'entrée.

/// Caractères gardés au plus avant le premier mot trouvé.
const EXCERPT_LEAD_CHARS: usize = 40;

/// Vrai si ce mot plié est un de ceux que la requête cherche.
fn is_sought(query: &Query, folded: &str) -> bool {
    query.terms.iter().any(|term| term == folded)
        || query
            .prefix
            .as_deref()
            .is_some_and(|prefix| folded.starts_with(prefix))
}

/// L'extrait d'un texte et ses plages à surligner, en unités UTF-16. Le texte entier s'il tient
/// dans [`MAX_EXCERPT_CHARS`] ; sinon une fenêtre qui commence peu avant le premier mot trouvé
/// (au début du texte si aucun ne l'est), coupée entre deux caractères, entre deux mots quand
/// c'est possible, des points de suspension là où elle coupe.
fn excerpt(text: &str, query: &Query) -> (String, Vec<(u32, u32)>) {
    let (from, to) = match text.char_indices().nth(MAX_EXCERPT_CHARS) {
        None => (0, text.len()),
        Some(_) => window(text, query),
    };
    let mut out = String::with_capacity(to - from + 8);
    if from > 0 {
        out.push('…');
    }
    let lead = out.encode_utf16().count();
    let shown = &text[from..to];
    out.push_str(shown);
    if to < text.len() {
        out.push('…');
    }
    // Les plages : les mots trouvés de la fenêtre, leurs places comptées au fil du texte.
    let mut marks = Vec::new();
    let (mut at, mut units) = (0, lead);
    for (word, folded) in indexed_words(shown) {
        if !is_sought(query, &folded) {
            continue;
        }
        units += shown[at..word.start].encode_utf16().count();
        let len = word.raw.encode_utf16().count();
        marks.push((units as u32, (units + len) as u32));
        units += len;
        at = word.start + word.raw.len();
    }
    (out, marks)
}

/// La fenêtre d'un texte plus long qu'un extrait : `[from, to[`, en octets.
fn window(text: &str, query: &Query) -> (usize, usize) {
    let first = indexed_words(text)
        .find(|(_, folded)| is_sought(query, folded))
        .map(|(word, _)| word);
    let mut from = 0;
    if let Some(word) = first {
        // Quelques caractères avant le mot, à partir du début d'un mot.
        let lead = text[..word.start]
            .char_indices()
            .rev()
            .take(EXCERPT_LEAD_CHARS)
            .last()
            .map_or(word.start, |(at, _)| at);
        if lead > 0 {
            let cut_a_word = text[..lead]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
            let rest = &text[lead..word.start];
            let skipped = if cut_a_word {
                rest.find(|c: char| !c.is_alphanumeric())
                    .unwrap_or(rest.len())
            } else {
                0
            };
            let rest = &rest[skipped..];
            from = word.start - rest.trim_start().len();
        }
    }
    // Deux caractères pour les points de suspension.
    let budget = MAX_EXCERPT_CHARS - 2;
    let rest = &text[from..];
    let mut to = match rest.char_indices().nth(budget) {
        Some((end, _)) => from + end,
        None => text.len(),
    };
    if to < text.len() {
        // Ne pas finir au milieu d'un mot, sauf s'il faudrait pour cela lâcher le mot trouvé.
        let keep = first.map_or(from, |word| word.start + word.raw.len());
        let cuts_a_word = text[to..].chars().next().is_some_and(char::is_alphanumeric)
            && text[..to]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
        if cuts_a_word {
            let whole = text[from..to]
                .char_indices()
                .rev()
                .find(|(_, c)| !c.is_alphanumeric())
                .map(|(at, _)| from + at);
            if let Some(whole) = whole.filter(|whole| *whole >= keep) {
                to = whole;
            }
        }
        to = from + text[from..to].trim_end().len();
    }
    (from, to)
}

// ---------------------------------------------------------------------------------------------
// L'index.

/// L'index plein texte d'une instance : un écrivain (celui de l'indexeur, ou de
/// `mementum reindex`) et un lecteur. Se partage entre fils (`Arc<SearchIndex>`).
pub struct SearchIndex {
    // L'index et ses réglages ne sont relus que par les fonctions des tests et des mesures.
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    index: Index,
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    fields: Fields,
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    options: Options,
}

/// Un index ouvert, et le jeton de sa dernière validation (Ruling R909) : `None` pour un index
/// neuf, ou refait parce que le dossier n'était pas le sien.
pub struct Opened {
    pub index: SearchIndex,
    pub token: Option<String>,
}

/// La clé d'une entrée : `<page>/<clé du bloc>`, `<page>/` pour l'entrée de titre.
fn entry_key(node_id: Uuid, block_id: Option<&str>) -> String {
    format!("{}/{}", node_id.hyphenated(), block_id.unwrap_or(""))
}

/// La clé de départage d'une entrée (Ruling R922) : zéro pour l'entrée de titre, sinon les seize
/// premiers octets du SHA-256 de la clé de son bloc.
fn tie_key(block_id: Option<&str>) -> (u64, u64) {
    let Some(block_id) = block_id else {
        return (0, 0);
    };
    let digest = Sha256::digest(block_id.as_bytes());
    let half = |from: usize| {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(&digest[from..from + 8]);
        u64::from_be_bytes(bytes)
    };
    (half(0), half(8))
}

/// Vide un dossier sans suivre aucun lien (la règle du dépôt des fichiers, Ruling R272) : un lien
/// posé dedans est retiré lui-même, jamais ce qu'il vise.
///
/// `meta.json` part en dernier : un vidage coupé en route (un arrêt brutal, un fichier tenu)
/// laisse un dossier qui porte encore la marque d'un index, et se vide de nouveau.
fn empty_dir(dir: &Path, spare: Option<&Path>) -> std::io::Result<()> {
    let mut meta = None;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if spare.is_some_and(|spared| entry.file_name() == spared.as_os_str()) {
            continue;
        }
        let path = entry.path();
        if entry.file_name() == "meta.json" {
            meta = Some(path);
            continue;
        }
        let meta = std::fs::symlink_metadata(&path)?;
        if is_link(&meta) {
            // Un lien vers un dossier se retire comme un dossier sous Windows, comme un fichier
            // sous Unix.
            if std::fs::remove_file(&path).is_err() {
                std::fs::remove_dir(&path)?;
            }
        } else if meta.is_dir() {
            empty_dir(&path, None)?;
            std::fs::remove_dir(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    if let Some(meta) = meta {
        // Un lien posé à sa place est retiré lui-même, comme les autres.
        if std::fs::remove_file(&meta).is_err() {
            std::fs::remove_dir(&meta)?;
        }
    }
    Ok(())
}

/// Vrai si ce dossier peut être celui d'un index, donc vidé (Ruling R990) : vide, ou porteur d'un
/// `meta.json`, ou porteur des seuls fichiers que le moteur pose lui-même ([`engine_file`]) : un
/// index dont la création ou le vidage a été coupé par un arrêt brutal (Ruling R1026). Tout
/// autre dossier (celui des données, par un chemin mal joint) n'est jamais touché.
fn may_hold_an_index(dir: &Path) -> std::io::Result<bool> {
    let mut only_engine_files = true;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "meta.json" {
            return Ok(true);
        }
        // Un fichier ordinaire seulement : ni un dossier ni un lien au nom d'un fichier du moteur.
        let plain = std::fs::symlink_metadata(entry.path())
            .is_ok_and(|meta| meta.is_file() && !is_link(&meta));
        only_engine_files &= plain && engine_file(&name);
    }
    Ok(only_engine_files)
}

/// Vrai pour un nom que le moteur (Tantivy 0.27) pose dans son dossier, hors `meta.json` : ses
/// deux fichiers cachés ; un temporaire d'`atomic_write`, que `tempfile` nomme `.tmp` suivi de
/// six lettres ou chiffres ; un fichier de segment, nommé de l'identifiant du segment (32
/// chiffres hexadécimaux en minuscules) et d'une extension de `SegmentComponent`
/// (`<opstamp>.del` pour les suppressions). Rien d'autre : c'est cette liste fermée qui garde un
/// dossier étranger d'être vidé. À relire à chaque montée de version du moteur.
fn engine_file(name: &str) -> bool {
    if name == ".managed.json" || (name.starts_with(".tantivy-") && name.ends_with(".lock")) {
        return true;
    }
    if let Some(random) = name.strip_prefix(".tmp") {
        return random.len() == 6 && random.bytes().all(|b| b.is_ascii_alphanumeric());
    }
    let Some((id, extension)) = name.split_once('.') else {
        return false;
    };
    let segment = id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let known = matches!(
        extension,
        "idx" | "pos" | "term" | "store" | "store.temp" | "fast" | "fieldnorm"
    ) || extension
        .strip_suffix(".del")
        .is_some_and(|stamp| !stamp.is_empty() && stamp.bytes().all(|b| b.is_ascii_digit()));
    segment && known
}

fn is_link(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Tout point d'analyse (lien symbolique, jonction) : jamais traversé.
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

impl SearchIndex {
    /// Ouvre l'index du dossier `dir`, créé s'il manque, et prend son écrivain. Un dossier qui
    /// n'est pas le sien (illisible, d'un autre schéma, d'une autre forme) est **vidé** et
    /// refait : l'index est dérivé, et l'appelant, qui reçoit `token: None`, reconstruit tout.
    /// [`IndexError::Busy`] si un autre écrivain le tient : rien n'est alors vidé.
    /// [`IndexError::NotAnIndex`] si le dossier porte autre chose qu'un index (ni vide, ni
    /// `meta.json`, ni les seuls fichiers du moteur) : rien n'y est touché (Ruling R990).
    pub fn open(dir: &Path) -> Result<Opened, IndexError> {
        SearchIndex::open_as(dir, Options::default(), false)
    }

    /// Vide le dossier, sans suivre de lien, et rouvre un index neuf ; mêmes refus que
    /// [`SearchIndex::open`], dont celui d'un dossier qui n'est pas un index. L'index d'avant
    /// doit avoir été fermé ([`SearchIndex::close`]) : sous Windows, un fichier ouvert ne se
    /// supprime pas.
    pub fn reset(dir: &Path) -> Result<Opened, IndexError> {
        SearchIndex::open_as(dir, Options::default(), true)
    }

    fn open_as(dir: &Path, options: Options, wipe: bool) -> Result<Opened, IndexError> {
        std::fs::create_dir_all(dir)?;
        // Avant tout, et sans rien y poser : un dossier qui porte autre chose qu'un index.
        if !may_hold_an_index(dir)? {
            return Err(IndexError::NotAnIndex);
        }
        let (schema, fields) = schema(options);
        // Le verrou de l'écrivain ensuite : on ne juge ni ne vide un dossier qu'un autre tient.
        // Il est tenu pendant qu'on vide, et rendu juste avant de prendre l'écrivain.
        let directory = MmapDirectory::open(dir).map_err(|e| IndexError::Engine(e.to_string()))?;
        let lock = directory
            .acquire_lock(&INDEX_WRITER_LOCK)
            .map_err(|error| match error {
                LockError::LockBusy => IndexError::Busy,
                LockError::IoError(error) => IndexError::Engine(error.to_string()),
            })?;
        let mut kept = None;
        {
            if !wipe && let Ok(index) = Index::open(directory) {
                let token = match index.load_metas().map(|metas| metas.payload) {
                    // Jamais validé : un index neuf, s'il est du bon schéma.
                    Ok(None) => Some(None),
                    Ok(Some(payload)) => payload
                        .strip_prefix(INDEX_FORM)
                        .and_then(|rest| rest.strip_prefix(' '))
                        .map(|token| Some(token.to_string())),
                    Err(_) => None,
                };
                if let Some(token) = token.filter(|_| index.schema() == schema) {
                    kept = Some((index, token));
                }
            }
        }
        let (index, token) = match kept {
            Some(kept) => kept,
            None => {
                // Tout sauf le fichier du verrou, qu'on tient.
                empty_dir(dir, Some(&INDEX_WRITER_LOCK.filepath))?;
                (Index::create_in_dir(dir, schema)?, None)
            }
        };
        index.tokenizers().register(WORDS, Words);
        index.tokenizers().register(STARTS, Starts);
        drop(lock);
        if let Some(taken_for) = options.writer_lock_taken_for {
            let taken = MmapDirectory::open(dir)
                .map_err(|e| IndexError::Engine(e.to_string()))?
                .acquire_lock(&INDEX_WRITER_LOCK)
                .map_err(|e| IndexError::Engine(e.to_string()))?;
            std::thread::spawn(move || {
                std::thread::sleep(taken_for);
                drop(taken);
            });
        }
        // Un fil d'indexation, un fil de fusion (Ruling R925). Le verrou, qu'on tenait l'instant
        // d'avant, peut ne pas être libre tout de suite (Ruling R1200) : sous Unix c'est un
        // `flock`, qu'un processus fils lancé par un autre fil garde de son `fork` à son
        // `exec`. On l'attend donc, un temps borné ; le premier verrou, plus haut, garde le
        // refus immédiat devant un autre écrivain.
        let deadline = Instant::now() + WRITER_LOCK_WAIT;
        let writer: IndexWriter = loop {
            let taken = index.writer_with_options(
                IndexWriterOptions::builder()
                    .memory_budget_per_thread(options.buffer_bytes)
                    .num_worker_threads(1)
                    .num_merge_threads(1)
                    .build(),
            );
            match taken {
                Err(TantivyError::LockFailure(..)) if Instant::now() < deadline => {
                    std::thread::sleep(WRITER_LOCK_PAUSE);
                }
                taken => break taken?,
            }
        };
        if let Some(max_docs) = options.max_docs_before_merge {
            let mut policy = LogMergePolicy::default();
            policy.set_max_docs_before_merge(max_docs);
            writer.set_merge_policy(Box::new(policy));
        }
        // Le lecteur ne suit que les validations faites ici : il est rechargé par `commit`.
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .doc_store_cache_num_blocks(STORE_CACHE_BLOCKS)
            .try_into()?;
        let index = SearchIndex {
            index,
            reader,
            writer: Mutex::new(writer),
            fields,
            options,
        };
        Ok(Opened { index, token })
    }

    /// Ferme l'index en attendant la fin des fusions en cours : après elle, plus aucun fichier
    /// du dossier n'est tenu par ce processus. À appeler avant un [`SearchIndex::reset`] fait
    /// dans le même processus (sous Windows, un fichier qu'une fusion tient ne se supprime pas).
    /// Peut durer le temps d'une fusion ; à l'arrêt du serveur, lâcher l'index suffit.
    pub fn close(self) -> Result<(), IndexError> {
        let SearchIndex { reader, writer, .. } = self;
        drop(reader);
        writer
            .into_inner()
            .unwrap_or_else(|e| e.into_inner())
            .wait_merging_threads()?;
        Ok(())
    }

    fn writer(&self) -> MutexGuard<'_, IndexWriter> {
        // Un fil qui a paniqué en tenant l'écrivain n'empêche pas les autres de s'en servir.
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Écrit une entrée, à la place de celle de même page et de même bloc s'il y en a une :
    /// celle d'un bloc (`block_id` est sa clé, `text` son texte projeté, `title` le titre de sa
    /// page), ou l'entrée de titre de la page (`block_id` absent, `text` vide). Le titre et le
    /// texte sont mis en NFC ici (Ruling R926). Rien n'est trouvé avant [`SearchIndex::commit`].
    pub fn put_entry(
        &self,
        node_id: Uuid,
        block_id: Option<&str>,
        title: &str,
        text: &str,
    ) -> Result<(), IndexError> {
        let (title, text) = (nfc(title), nfc(text));
        let fields = &self.fields;
        let page = node_id.as_u128();
        let (blk_hi, blk_lo) = tie_key(block_id);
        let entry = entry_key(node_id, block_id);
        let mut doc = TantivyDocument::default();
        doc.add_text(fields.node, node_id.hyphenated().to_string());
        doc.add_text(fields.entry, &entry);
        if let Some(block_id) = block_id {
            doc.add_text(fields.block_id, block_id);
        }
        doc.add_u64(fields.page_hi, (page >> 64) as u64);
        doc.add_u64(fields.page_lo, page as u64);
        doc.add_u64(fields.blk_hi, blk_hi);
        doc.add_u64(fields.blk_lo, blk_lo);
        if !title.is_empty() {
            doc.add_text(fields.title, &title);
            doc.add_text(fields.title_starts, &title);
        }
        if !text.is_empty() {
            doc.add_text(fields.text, &text);
            doc.add_text(fields.text_starts, &text);
        }
        let writer = self.writer();
        writer.delete_term(Term::from_field_text(fields.entry, &entry));
        writer.add_document(doc)?;
        Ok(())
    }

    /// Retire une entrée : celle d'un bloc, ou l'entrée de titre (`block_id` absent).
    pub fn remove_entry(&self, node_id: Uuid, block_id: Option<&str>) -> Result<(), IndexError> {
        let entry = entry_key(node_id, block_id);
        self.writer()
            .delete_term(Term::from_field_text(self.fields.entry, &entry));
        Ok(())
    }

    /// Retire toutes les entrées d'une page.
    pub fn remove_page(&self, node_id: Uuid) -> Result<(), IndexError> {
        let node = node_id.hyphenated().to_string();
        self.writer()
            .delete_term(Term::from_field_text(self.fields.node, &node));
        Ok(())
    }

    /// Valide ce qui a été écrit et retiré depuis la dernière validation, avec `token` pour
    /// charge utile (Ruling R909) : c'est lui que [`SearchIndex::open`] rendra.
    ///
    /// **Sur `Err`, rien n'est dit de ce que le disque porte** : la validation a pu échouer
    /// avant d'être durable (le jeton d'avant reste), ou être durable et n'avoir échoué qu'au
    /// rechargement du lecteur (le jeton neuf est écrit, les recherches lisent encore l'état
    /// d'avant). L'appelant ne réessaie pas sur cet index : il le lâche, le rouvre, et relit le
    /// jeton rendu par [`SearchIndex::open`], qui seul dit laquelle des deux a eu lieu.
    pub fn commit(&self, token: &str) -> Result<(), IndexError> {
        let mut writer = self.writer();
        let mut prepared = writer.prepare_commit()?;
        prepared.set_payload(&format!("{INDEX_FORM} {token}"));
        prepared.commit()?;
        drop(writer);
        self.reader.reload()?;
        Ok(())
    }

    /// Cherche : **une ligne par page**, sa meilleure entrée ; `skip` et `limit` comptent des
    /// pages, groupées avant d'être comptées (Ruling R902). Seules les pages de `allowed`
    /// existent pour cette recherche : les autres ne changent ni les pages rendues, ni leur
    /// ordre, ni `more`. Une requête vide ne trouve rien, sans lire l'index.
    pub fn search(
        &self,
        query: &Query,
        allowed: &PageSet,
        skip: usize,
        limit: usize,
    ) -> Result<Found, IndexError> {
        Ok(self.run(query, allowed, skip, limit, None)?.0)
    }

    fn run(
        &self,
        query: &Query,
        allowed: &PageSet,
        skip: usize,
        limit: usize,
        witness: Option<Witness>,
    ) -> Result<(Found, Trace), IndexError> {
        let mut trace = Trace::default();
        if query.is_empty() {
            return Ok((Found::default(), trace));
        }
        let fields = &self.fields;
        let searcher = self.reader.searcher();
        let long = query.prefix.as_deref().filter(|prefix| cut_start(prefix).1);
        let ranking = Ranking {
            by_address: witness == Some(Witness::AddressTieBreak),
        };
        let collector = ByPage {
            allowed: allowed.clone(),
            keep: if long.is_some() {
                MAX_VERIFIED_PER_PAGE
            } else {
                1
            },
            ranking,
        };
        let (engine, terms) = engine_query(fields, query);
        let constants = Constants {
            title: fields.title,
        };
        let groups = match witness {
            #[cfg(feature = "test-hooks")]
            Some(Witness::IndexStatistics) => searcher.search(&engine, &collector),
            _ => searcher.search_with_statistics_provider(&engine, &collector, &constants),
        }?;
        trace.terms = terms;
        trace.collected = groups.collected;
        trace.grouped = groups.grouped;
        trace.kept = groups.pages.values().map(|top| 1 + top.rest.len()).sum();

        // La meilleure entrée de chaque page.
        let mut pages: Vec<(u128, Candidate)> = match long {
            None => groups
                .pages
                .into_iter()
                .map(|(page, top)| (page, top.best))
                .collect(),
            Some(prefix) => self.verified(&searcher, groups, prefix, ranking, &mut trace)?,
        };
        pages.sort_unstable_by(|a, b| ranking.across(a, b));
        let more = pages.len() > skip.saturating_add(limit);
        let mut hits = Vec::new();
        for (page, candidate) in pages.into_iter().skip(skip).take(limit) {
            trace.notes.push(candidate.note);
            let doc: TantivyDocument = searcher.doc(candidate.address)?;
            let stored = |field: Field| doc.get_first(field).and_then(|value| value.as_str());
            let (excerpt, marks) = excerpt(stored(fields.text).unwrap_or(""), query);
            hits.push(Hit {
                node_id: Uuid::from_u128(page),
                block_id: stored(fields.block_id).map(str::to_string),
                excerpt,
                marks,
            });
        }
        Ok((Found { pages: hits, more }, trace))
    }

    /// Un préfixe de plus de [`MAX_START_CHARS`] caractères : les entrées trouvées par ses dix
    /// premiers sont relues, dans l'ordre (note, page, clé de départage : jamais l'ordre de
    /// l'index), sous trois bornes, et seules restent celles dont un mot du titre ou du texte
    /// gardé commence par tout le préfixe (Rulings R920, R929). Rend la meilleure de chaque page.
    fn verified(
        &self,
        searcher: &Searcher,
        groups: Groups,
        prefix: &str,
        ranking: Ranking,
        trace: &mut Trace,
    ) -> Result<Vec<(u128, Candidate)>, IndexError> {
        let fields = &self.fields;
        let mut candidates: Vec<(u128, Candidate)> = Vec::new();
        for (page, top) in groups.pages {
            candidates.push((page, top.best));
            candidates.extend(top.rest.into_iter().map(|candidate| (page, candidate)));
        }
        candidates.sort_unstable_by(|a, b| ranking.across(a, b));
        let mut best: HashMap<u128, Candidate, Mixing> = HashMap::default();
        let mut bytes = 0;
        for (page, mut candidate) in candidates.into_iter().take(MAX_VERIFIED_ENTRIES) {
            if bytes >= MAX_VERIFIED_BYTES {
                break;
            }
            let doc: TantivyDocument = searcher.doc(candidate.address)?;
            let stored = |field: Field| doc.get_first(field).and_then(|value| value.as_str());
            let (title, text) = (
                stored(fields.title).unwrap_or(""),
                stored(fields.text).unwrap_or(""),
            );
            bytes += title.len() + text.len();
            trace.verified.push((
                Uuid::from_u128(page),
                stored(fields.block_id).map(str::to_string),
            ));
            // La note du début de mot, pour le champ qui le porte vraiment.
            let (in_title, in_text) = (has_start(title, prefix), has_start(text, prefix));
            if in_title {
                candidate.note += TITLE_START_NOTE;
            }
            if in_text {
                candidate.note += TEXT_START_NOTE;
            }
            if !(in_title || in_text) {
                continue;
            }
            match best.entry(page) {
                Entry::Vacant(vacant) => {
                    vacant.insert(candidate);
                }
                Entry::Occupied(mut occupied) => {
                    if ranking.in_page(&candidate, occupied.get()) == Ordering::Less {
                        occupied.insert(candidate);
                    }
                }
            }
        }
        Ok(best.into_iter().collect())
    }
}

/// Pour les tests et les mesures (fonction `test-hooks`).
#[cfg(feature = "test-hooks")]
impl SearchIndex {
    pub fn open_with(dir: &Path, options: Options) -> Result<Opened, IndexError> {
        SearchIndex::open_as(dir, options, false)
    }

    /// Comme [`SearchIndex::search`], avec ce que la recherche a fait, et un témoin à poser.
    pub fn search_traced(
        &self,
        query: &Query,
        allowed: &PageSet,
        skip: usize,
        limit: usize,
        witness: Option<Witness>,
    ) -> Result<(Found, Trace), IndexError> {
        self.run(query, allowed, skip, limit, witness)
    }

    /// Fusionne tous les segments en un seul.
    pub fn merge_all(&self) -> Result<(), IndexError> {
        let segments = self.index.searchable_segment_ids()?;
        if segments.len() > 1 {
            let mut writer = self.writer();
            writer.merge(&segments).wait()?;
        }
        self.reader.reload()?;
        Ok(())
    }

    /// Attend la fin des fusions en cours, puis rouvre un écrivain.
    pub fn wait_for_merges(self) -> Result<SearchIndex, IndexError> {
        let SearchIndex {
            index,
            reader,
            writer,
            fields,
            options,
        } = self;
        writer
            .into_inner()
            .unwrap_or_else(|e| e.into_inner())
            .wait_merging_threads()?;
        let writer: IndexWriter = index.writer_with_options(
            IndexWriterOptions::builder()
                .memory_budget_per_thread(options.buffer_bytes)
                .num_worker_threads(1)
                .num_merge_threads(1)
                .build(),
        )?;
        reader.reload()?;
        Ok(SearchIndex {
            index,
            reader,
            writer: Mutex::new(writer),
            fields,
            options,
        })
    }

    /// Le nombre de termes du dictionnaire d'un champ, tous segments additionnés.
    pub fn terms_of(&self, field: &str) -> Result<u64, IndexError> {
        let field = self.index.schema().get_field(field)?;
        let mut terms = 0;
        for segment in self.reader.searcher().segment_readers() {
            terms += segment.inverted_index(field)?.terms().num_terms() as u64;
        }
        Ok(terms)
    }

    pub fn segments(&self) -> usize {
        self.reader.searcher().segment_readers().len()
    }

    /// Les octets de l'index, par champ (dictionnaire, listes, longueurs, colonne), puis les
    /// textes gardés (`(gardés)`) et le total des segments (`(total)`).
    pub fn usage(&self) -> Result<Vec<(String, u64)>, IndexError> {
        let usage = self.reader.searcher().space_usage()?;
        let mut by: std::collections::BTreeMap<String, u64> = Default::default();
        let (mut stored, mut total) = (0, 0);
        for segment in usage.segments() {
            for part in [
                segment.termdict(),
                segment.postings(),
                segment.positions(),
                segment.fieldnorms(),
                segment.fast_fields(),
            ] {
                for field in part.fields() {
                    *by.entry(field.field_name().to_string()).or_default() +=
                        field.total().get_bytes();
                }
            }
            stored += segment.store().total().get_bytes();
            total += segment.total().get_bytes();
        }
        let mut all: Vec<_> = by.into_iter().collect();
        all.push(("(gardés)".to_string(), stored));
        all.push(("(total)".to_string(), total));
        Ok(all)
    }
}
