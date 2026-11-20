//! Couper un texte en mots, plier un mot (Ruling R926).
//!
//! Le texte indexé et la requête passent par les mêmes trois fonctions, dans cet ordre : [`nfc`],
//! [`words`], [`fold`]. C'est la chaîne que S0 a mesurée avec les filtres de Tantivy (découpage sur
//! tout ce qui n'est ni lettre ni chiffre, minuscules, accents retirés), écrite une fois ici pour
//! que l'index et la requête ne puissent pas diverger : un mot plié autrement d'un côté que de
//! l'autre ne se trouverait plus.

use std::borrow::Cow;

use unicode_normalization::char::decompose_canonical;
use unicode_normalization::{IsNormalized, UnicodeNormalization, is_nfc_quick};

/// Longueur maximale, en octets, d'un mot indexé, tel qu'écrit (avant [`fold`]) : un mot plus long
/// n'est pas indexé du tout (Ruling R926 ; `RemoveLongFilter::limit(41)` de Tantivy). [`words`] le
/// rend quand même : c'est à qui indexe de l'écarter, et une requête qui le porte ne trouve rien.
pub const MAX_WORD_BYTES: usize = 40;

/// Le texte en forme NFC : une lettre et son accent combinant ne font qu'un caractère. Sans cela,
/// un texte venu de macOS (« élève » en accents combinants) serait coupé en trois mots, l'accent
/// n'étant ni une lettre ni un chiffre. Rend le texte tel quel s'il est déjà en NFC.
pub fn nfc(text: &str) -> Cow<'_, str> {
    match is_nfc_quick(text.chars()) {
        IsNormalized::Yes => Cow::Borrowed(text),
        _ => Cow::Owned(text.nfc().collect()),
    }
}

/// Un mot d'un texte, tel qu'écrit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Word<'a> {
    /// Où il commence dans le texte, en octets ; il finit à `start + raw.len()`.
    pub start: usize,
    /// Le mot, pas encore plié.
    pub raw: &'a str,
}

/// Les mots d'un texte, dans l'ordre : les plus longues suites de lettres et de chiffres
/// (`char::is_alphanumeric`). Tout autre caractère sépare, et n'est dans aucun mot : la
/// ponctuation, le trait de soulignement, un accent combinant resté seul, un emoji. Une suite de
/// caractères chinois, japonais ou coréens sans espace est un seul mot (limite L-p).
///
/// Le texte est supposé en NFC ([`nfc`]) : l'appelant normalise d'abord, pour que les places
/// rendues soient celles du texte qu'il garde.
pub fn words(text: &str) -> impl Iterator<Item = Word<'_>> {
    let mut rest = text;
    std::iter::from_fn(move || {
        let offset = rest.find(char::is_alphanumeric)?;
        let from = &rest[offset..];
        let len = from
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(from.len());
        let start = text.len() - from.len();
        rest = &from[len..];
        Some(Word {
            start,
            raw: &from[..len],
        })
    })
}

/// Les accents combinants que portent les lettres latines (U+0300 à U+036F) : les seuls que le
/// pliage retire (Ruling R947). Les signes des autres écritures (une voyelle du hindi, de l'arabe,
/// du thaï) sont des lettres de leurs mots : retirés, des mots différents se confondraient.
fn is_latin_accent(c: char) -> bool {
    ('\u{300}'..='\u{36F}').contains(&c)
}

/// Un mot plié : en minuscules, les accents des lettres latines retirés, les ligatures ouvertes
/// (« ŒUVRE » se lit « oeuvre », « Straße » « strasse »). Hors de l'alphabet latin, seules les
/// minuscules jouent, et le sigma final du grec, lu comme un sigma (« ΟΔΟΣ » trouve « οδος ») :
/// ni le grec, ni le cyrillique ne perdent leurs accents, une syllabe coréenne n'est pas défaite,
/// et les signes des autres écritures restent.
///
/// Deux choses à savoir pour qui s'en sert :
/// - **un mot peut se plier en rien** (un accent combinant resté seul, que Unicode tient pour une
///   lettre, comme U+0345) : un mot plié vide n'est pas un mot, il s'écarte ([`indexed_words`] et
///   la lecture d'une requête le font) ;
/// - **un mot plié peut être plus long, en octets, que le mot écrit** (« ß » donne « ss », « Ⱥ »,
///   deux octets, donne « ⱥ », trois) : un mot de [`MAX_WORD_BYTES`] octets peut en faire 60 une
///   fois plié. La borne se juge sur le mot écrit, jamais sur le mot plié.
pub fn fold(word: &str) -> String {
    let mut folded = String::with_capacity(word.len());
    for upper in word.chars() {
        for c in upper.to_lowercase() {
            match c {
                'ß' => folded.push_str("ss"),
                'æ' => folded.push_str("ae"),
                'œ' => folded.push_str("oe"),
                'ø' => folded.push('o'),
                'đ' | 'ð' => folded.push('d'),
                'ł' => folded.push('l'),
                'ħ' => folded.push('h'),
                'ı' => folded.push('i'),
                'þ' => folded.push_str("th"),
                'ς' => folded.push('σ'),
                // Latin-1, latin étendu A et B, latin étendu additionnel (le vietnamien) : la
                // lettre sans ses accents.
                '\u{C0}'..='\u{24F}' | '\u{1E00}'..='\u{1EFF}' => {
                    decompose_canonical(c, |part| {
                        if !is_latin_accent(part) {
                            folded.push(part);
                        }
                    });
                }
                // Un accent latin resté seul : celui que les minuscules font paraître (« İ »
                // donne « i » et un point combinant), ou celui que NFC n'a pas pu composer.
                c if is_latin_accent(c) => {}
                c => folded.push(c),
            }
        }
    }
    folded
}

/// Les mots d'un texte que l'index garde, chacun avec sa forme pliée : ceux de [`words`], hors
/// les mots de plus de [`MAX_WORD_BYTES`] octets tels qu'écrits (Ruling R926) et ceux qui se
/// plient en rien (Ruling R947). C'est ce que l'analyseur de l'index plein texte rend, mot pour
/// mot ; la place d'un mot est celle du mot écrit, dans le texte donné (supposé en NFC, comme
/// pour [`words`]). Un mot plié peut dépasser [`MAX_WORD_BYTES`] octets ([`fold`]).
pub fn indexed_words(text: &str) -> impl Iterator<Item = (Word<'_>, String)> {
    words(text)
        .filter(|word| word.raw.len() <= MAX_WORD_BYTES)
        .map(|word| (word, fold(word.raw)))
        .filter(|(_, folded)| !folded.is_empty())
}
