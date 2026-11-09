//! Format MMD (Mementum Markdown) : conversion sans perte entre texte et arbre de blocs.

mod inline;
mod model;
mod parse;
mod render;
pub mod ydoc;

pub use inline::{parse_inline, render_inline};
pub use model::{normalize, normalize_inline, Block, BlockKind, Inline, Marks, MmdError};
pub use parse::parse;
pub use render::{render, RenderOpts, Rendered};
