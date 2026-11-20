//! Recherche de Mementum (conception §5.2, §5.3, §5.7 ; plan du lot 4).
//!
//! Trois morceaux purs, sans moteur ni base :
//! - [`project`] : une page lue (ses blocs) devient des lignes d'index, une par bloc qui porte du
//!   texte ou un lien vers une page ;
//! - [`read_query`] : ce qu'une personne tape devient des termes, sous des bornes, sans aucune
//!   syntaxe ;
//! - [`nfc`], [`words`], [`fold`] : la seule façon de couper un texte en mots et de plier un mot
//!   (casse, accents). Le texte indexé et la requête passent par les mêmes fonctions : un mot plié
//!   autrement d'un côté que de l'autre ne se trouverait plus.

mod fold;
mod project;
mod query;

pub use fold::{MAX_WORD_BYTES, Word, fold, nfc, words};
pub use project::{
    IndexedBlock, MAX_BLOCK_ID_BYTES, MAX_BLOCK_TEXT_BYTES, MAX_PAGE_LINKS, MAX_PAGE_TEXT_BYTES,
    Projection, Rule, is_stand_in, page_link, project, rule,
};
pub use query::{
    MAX_QUERY_CHARS, MAX_QUERY_TERMS, MIN_PREFIX_CHARS, Query, QueryRefusal, read_query,
};
