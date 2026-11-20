//! Recherche de Mementum (conception §5.2, §5.3, §5.7 ; plan du lot 4).
//!
//! [`SearchIndex`] est l'index plein texte (Tantivy, dans un dossier à lui) : il ne sait rien du
//! store ni des droits, et ce qui est permis lui est donné à chaque recherche ([`PageSet`]).
//!
//! Trois morceaux purs, sans moteur ni base :
//! - [`project`] : une page lue (ses blocs) devient des lignes d'index, une par bloc qui porte du
//!   texte ou un lien vers une page ;
//! - [`read_query`] : ce qu'une personne tape devient des termes, sous des bornes, sans aucune
//!   syntaxe ;
//! - [`nfc`], [`words`], [`fold`] : la seule façon de couper un texte en mots et de plier un mot
//!   (casse, accents). Le texte indexé et la requête passent par les mêmes fonctions : un mot plié
//!   autrement d'un côté que de l'autre ne se trouverait plus. [`indexed_words`] rend les mots
//!   qu'un index garde d'un texte, déjà pliés.
//!
//! [`PROJECTION_VERSION`] avance à tout changement de ce que ces fonctions rendent.

mod fold;
mod index;
mod project;
mod query;

pub use fold::{MAX_WORD_BYTES, Word, fold, indexed_words, nfc, words};
pub use index::{
    Found, Hit, IndexError, MAX_EXCERPT_CHARS, MAX_KEPT_CANDIDATES, MAX_START_CHARS,
    MAX_VERIFIED_BYTES, MAX_VERIFIED_ENTRIES, MAX_VERIFIED_PER_PAGE, Opened, PageSet, SearchIndex,
    WRITER_BUFFER_BYTES,
};
#[cfg(feature = "test-hooks")]
pub use index::{Options, Trace, Witness};
pub use project::{
    IndexedBlock, MAX_BLOCK_ID_BYTES, MAX_BLOCK_TEXT_BYTES, MAX_PAGE_LINKS, MAX_PAGE_TEXT_BYTES,
    PROJECTION_VERSION, Projection, Rule, is_stand_in, kind_named, page_link, project, rule,
};
pub use query::{
    MAX_QUERY_CHARS, MAX_QUERY_TERMS, MIN_PREFIX_CHARS, Query, QueryRefusal, read_query,
};
