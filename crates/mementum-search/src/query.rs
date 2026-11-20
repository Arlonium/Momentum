//! Lecture d'une requête (Rulings R911, R926, R930).
//!
//! Ce qu'une personne tape n'est jamais donné à un analyseur de requêtes : il est coupé en mots et
//! plié par les fonctions qui coupent et plient le texte indexé ([`crate::words`],
//! [`crate::fold`]), et la recherche est bâtie par programme de ces mots. Un opérateur (`AND`,
//! `"`, `*`, `-`, `title:`, `~`) n'est donc que des caractères : des lettres font un mot, le reste
//! sépare.

use std::fmt;

use crate::fold::{fold, nfc, words};

/// Longueur maximale d'une requête, en caractères (Ruling R911), espaces compris.
pub const MAX_QUERY_CHARS: usize = 200;
/// Nombre maximal de mots d'une requête, le dernier compris (Ruling R911).
pub const MAX_QUERY_TERMS: usize = 8;
/// Longueur minimale, en caractères une fois plié, du dernier mot pour qu'il soit cherché comme
/// début de mot (Ruling R911) : un début d'une lettre toucherait tout l'espace.
pub const MIN_PREFIX_CHARS: usize = 2;

/// Une requête lue : des mots pliés, sans aucune syntaxe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// Les mots cherchés entiers, tous voulus, dans l'ordre tapé.
    pub terms: Vec<String>,
    /// Le dernier mot tapé, voulu lui aussi, cherché comme mot entier **ou** comme début de mot
    /// (Ruling R930) ; absent d'une requête vide, ou quand le dernier mot a moins de
    /// [`MIN_PREFIX_CHARS`] caractères (il est alors le dernier de `terms`, Ruling R940).
    pub prefix: Option<String>,
}

impl Query {
    /// Vrai pour une requête sans aucun mot : elle ne trouve rien, et ne se calcule pas.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.prefix.is_none()
    }
}

/// Une requête refusée. Son message tutoie, dit la borne, et ne recopie rien de la requête.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryRefusal {
    /// Plus de [`MAX_QUERY_CHARS`] caractères.
    TooLong,
    /// Plus de [`MAX_QUERY_TERMS`] mots.
    TooManyTerms,
}

impl fmt::Display for QueryRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryRefusal::TooLong => write!(
                f,
                "Ta recherche est trop longue : {MAX_QUERY_CHARS} caractères au plus."
            ),
            QueryRefusal::TooManyTerms => write!(
                f,
                "Ta recherche a trop de mots : {MAX_QUERY_TERMS} au plus."
            ),
        }
    }
}

impl std::error::Error for QueryRefusal {}

/// Lit une requête telle que tapée. Une requête trop longue est refusée sans être lue (sa taille
/// en octets suffit à le dire au-delà de quatre octets par caractère), si bien que la lecture ne
/// coûte jamais plus que celle de [`MAX_QUERY_CHARS`] caractères.
pub fn read_query(raw: &str) -> Result<Query, QueryRefusal> {
    if raw.len() > 4 * MAX_QUERY_CHARS || raw.chars().count() > MAX_QUERY_CHARS {
        return Err(QueryRefusal::TooLong);
    }
    let text = nfc(raw);
    let mut terms = Vec::new();
    for word in words(&text) {
        if terms.len() == MAX_QUERY_TERMS {
            return Err(QueryRefusal::TooManyTerms);
        }
        terms.push(fold(word.raw));
    }
    let prefix = match terms.last() {
        Some(last) if last.chars().count() >= MIN_PREFIX_CHARS => terms.pop(),
        _ => None,
    };
    Ok(Query { terms, prefix })
}
