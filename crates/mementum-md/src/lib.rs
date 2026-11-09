//! Format MMD (Mementum Markdown) : conversion sans perte entre texte et arbre
//! de blocs.
//!
//! [`render`] écrit la forme canonique d'un arbre ; [`parse`] relit cette forme
//! et refuse tout le reste ([`MmdError`] avec la ligne et ce qu'il faut écrire).
//! Les types de blocs, leurs props, défauts et formes viennent du registre
//! `mementum-schema`. La spécification du format est au §6.2 de la conception.

mod attrs;
mod inline;
mod model;
mod parse;
mod render;

#[cfg(feature = "arbitrary")]
pub mod arb;

pub use model::{
    Block, Cell, Content, Inline, MAX_WIDTH_COLUMNS, MmdError, PropValue, Props, RenderOpts,
    Rendered, Styles, Table, block_text, cell_rowspan, cell_span, normalize,
};
pub use parse::{MAX_DEPTH, parse};
pub use render::render;
