//! Document de page de Mementum : la seule crate qui voit yrs.
//!
//! Une page est un document Yjs au format de BlockNote 0.57 (y-prosemirror
//! 1.x), dans le fragment [`FRAGMENT`]. [`PageDoc`] le lit et l'écrit en
//! arbre de blocs `mementum_md::Block`, applique les éditions d'un agent,
//! dédoublonne les blocs après des déplacements concurrents et calcule une
//! révision canonique.

mod blocknote;
mod dedupe;
mod edit;
mod page;
mod revision;

use std::fmt;

pub use page::PageDoc;

/// Nom du fragment XML partagé avec BlockNote.
pub const FRAGMENT: &str = "document-store";

/// Nom de la `Y.Map` des métadonnées de la page ; clé `schemaVersion` (entier).
pub const META: &str = "mementum";

/// Erreur d'une opération sur le document de page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrdtError {
    /// Mise à jour ou vecteur d'état Yjs illisible.
    Decode(String),
    /// Aucun bloc ne porte cet id.
    BlockNotFound(String),
    /// Plage ou position impossible dans ce bloc.
    OutOfRange { block_id: String },
}

impl fmt::Display for CrdtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrdtError::Decode(e) => write!(f, "mise à jour Yjs illisible : {e}"),
            CrdtError::BlockNotFound(id) => write!(f, "aucun bloc ne porte l'id « {id} »"),
            CrdtError::OutOfRange { block_id } => {
                write!(f, "plage impossible dans le bloc « {block_id} »")
            }
        }
    }
}

impl std::error::Error for CrdtError {}
