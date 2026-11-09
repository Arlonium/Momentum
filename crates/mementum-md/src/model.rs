//! Arbre de blocs : le modèle commun au format MMD, au document Yjs et au serveur.

use std::collections::BTreeMap;
use std::fmt;

use mementum_schema::{MmdForm, PropDefault, PropSpec, block, cell_props};

/// Un bloc et ses enfants.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// Identifiant du bloc, vide s'il n'est pas encore attribué.
    pub id: String,
    /// Nom du type BlockNote (`paragraph`, `heading`…), ou `unknown`.
    pub kind: String,
    /// Props, par nom BlockNote. Après [`normalize`], celles qui valent leur
    /// défaut (déclaré dans `mementum-schema`) sont absentes.
    pub props: Props,
    pub content: Content,
    pub children: Vec<Block>,
}

/// Props d'un bloc ou d'une cellule, par nom BlockNote.
pub type Props = BTreeMap<String, PropValue>;

/// Valeur d'une prop.
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// Contenu d'un bloc.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    None,
    Inline(Vec<Inline>),
    Table(Table),
}

/// Contenu d'un tableau.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Lignes d'en-tête (1 par défaut, comme BlockNote).
    pub header_rows: u32,
    /// Colonnes d'en-tête (`headerCols` de BlockNote, 0 par défaut).
    pub header_cols: u32,
    /// Une largeur par colonne, `None` = automatique.
    pub column_widths: Vec<Option<f64>>,
    pub rows: Vec<Vec<Cell>>,
}

/// Une cellule de tableau.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub content: Vec<Inline>,
    pub props: Props,
}

/// Contenu en ligne.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text {
        text: String,
        styles: Styles,
    },
    Link {
        href: String,
        children: Vec<Inline>,
    },
    Mention {
        kind: String,
        id: String,
        label: String,
    },
    Math {
        latex: String,
    },
    HardBreak,
}

/// Marques d'un texte.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Styles {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub text_color: Option<String>,
    pub background_color: Option<String>,
}

impl Styles {
    /// Vrai pour un texte sans aucune marque.
    pub fn is_plain(&self) -> bool {
        *self == Styles::default()
    }
}

/// Erreur de lecture d'un MMD : ligne (à partir de 1) et message qui dit quoi écrire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmdError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for MmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ligne {} : {}", self.line, self.message)
    }
}

impl std::error::Error for MmdError {}

/// Options de rendu.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderOpts {
    /// Donne à chaque bloc un alias court `bN` (` ^bN` en fin de ligne,
    /// `id="bN"` sur une balise).
    pub anchors: bool,
}

/// Résultat du rendu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rendered {
    pub markdown: String,
    /// Alias `bN` → id du bloc (vide si les ancres sont désactivées).
    pub aliases: BTreeMap<String, String>,
}

/// Vrai si `value` est la valeur par défaut déclarée.
pub(crate) fn is_default(value: &PropValue, default: Option<PropDefault>) -> bool {
    match (value, default) {
        (PropValue::Str(v), Some(PropDefault::Str(d))) => v == d,
        (PropValue::Int(v), Some(PropDefault::Int(d))) => *v == d,
        (PropValue::Float(v), Some(PropDefault::Float(d))) => *v == d,
        (PropValue::Bool(v), Some(PropDefault::Bool(d))) => *v == d,
        _ => false,
    }
}

/// La valeur par défaut déclarée, en [`PropValue`].
pub(crate) fn default_value(default: Option<PropDefault>) -> Option<PropValue> {
    default.map(|d| match d {
        PropDefault::Str(s) => PropValue::Str(s.to_string()),
        PropDefault::Int(i) => PropValue::Int(i),
        PropDefault::Float(f) => PropValue::Float(f),
        PropDefault::Bool(b) => PropValue::Bool(b),
    })
}

fn strip_defaults(props: &mut Props, specs: &[PropSpec]) {
    props.retain(|name, value| {
        !specs
            .iter()
            .any(|p| p.name == name && is_default(value, p.default))
    });
}

/// Forme canonique d'un arbre : ce que BlockNote (et donc Yjs) sait porter,
/// écrit d'une seule façon.
/// - Les props égales au défaut du registre sont retirées.
/// - Les textes vides sont retirés, et deux textes voisins de mêmes styles
///   fusionnés.
/// - Deux liens voisins de même `href` n'en font qu'un : une marque Yjs ou
///   ProseMirror ne sépare pas deux plages de même valeur (Ruling R16).
/// - Le contenu d'un bloc de code est du texte brut, comme dans BlockNote
///   0.57 (`content: "plain"`) : marques et liens retirés, mentions et
///   formules retirées, saut de ligne changé en `\n`. BlockNote n'y admet que
///   les marques d'annotation (commentaires), que le modèle ne porte pas
///   (Ruling R19).
/// - Les lignes et colonnes d'en-tête d'un tableau sont ramenées dans la
///   grille, sous la seule forme que les cellules `tableHeader` de BlockNote
///   distinguent : un tableau tout en en-têtes vaut `(lignes, 0)` (Ruling
///   R19).
pub fn normalize(blocks: Vec<Block>) -> Vec<Block> {
    blocks.into_iter().map(normalize_block).collect()
}

fn normalize_block(mut b: Block) -> Block {
    let spec = block(&b.kind);
    if let Some(spec) = spec {
        strip_defaults(&mut b.props, spec.props);
    }
    let plain = spec.is_some_and(|s| s.form == MmdForm::Code);
    b.content = match b.content {
        Content::None => Content::None,
        Content::Inline(c) if plain => {
            let text = plain_text(&c);
            Content::Inline(if text.is_empty() {
                vec![]
            } else {
                vec![Inline::Text {
                    text,
                    styles: Styles::default(),
                }]
            })
        }
        Content::Inline(c) => Content::Inline(normalize_inline(c)),
        Content::Table(t) => Content::Table(normalize_table(t)),
    };
    b.children = normalize(b.children);
    b
}

/// Texte brut d'un bloc de code : textes (liens compris) et sauts de ligne.
fn plain_text(content: &[Inline]) -> String {
    let mut out = String::new();
    for inline in content {
        match inline {
            Inline::Text { text, .. } => out.push_str(text),
            Inline::Link { children, .. } => out.push_str(&plain_text(children)),
            Inline::HardBreak => out.push('\n'),
            Inline::Mention { .. } | Inline::Math { .. } => {}
        }
    }
    out
}

fn normalize_table(mut t: Table) -> Table {
    for cell in t.rows.iter_mut().flatten() {
        cell.content = normalize_inline(std::mem::take(&mut cell.content));
        strip_defaults(&mut cell.props, cell_props());
    }
    // Sans largeurs : une largeur automatique par colonne de la première ligne.
    if t.column_widths.is_empty()
        && let Some(first) = t.rows.first()
    {
        t.column_widths = vec![None; first.len()];
    }
    let (rows, cols) = canonical_headers(&t);
    t.header_rows = rows;
    t.header_cols = cols;
    t
}

/// En-têtes d'un tableau, tels que BlockNote les relirait dans ses cellules.
///
/// Une cellule est un en-tête si sa ligne ou sa colonne l'est. Les valeurs
/// sont d'abord ramenées dans la grille (au plus le nombre de lignes, et la
/// longueur de la plus courte ligne pour les colonnes), puis relues sur la
/// grille des cellules d'en-tête : lignes d'en-tête en tête du tableau,
/// colonnes d'en-tête à gauche ; un tableau tout en en-têtes vaut
/// `(lignes, 0)`. `mementum-crdt` relit les cellules `tableHeader` de la même
/// façon, et cette forme est stable.
fn canonical_headers(t: &Table) -> (u32, u32) {
    let shortest = t.rows.iter().map(Vec::len).min().unwrap_or(0);
    let header_rows = (t.header_rows as usize).min(t.rows.len());
    let header_cols = (t.header_cols as usize).min(shortest);
    let header = |r: usize, c: usize| r < header_rows || c < header_cols;
    let full_row = |r: usize| (0..t.rows[r].len()).all(|c| header(r, c));
    if (0..t.rows.len()).all(full_row) {
        return (t.rows.len() as u32, 0);
    }
    let rows = (0..t.rows.len()).take_while(|&r| full_row(r)).count();
    let cols = (0..shortest)
        .take_while(|&c| (0..t.rows.len()).all(|r| header(r, c)))
        .count();
    (rows as u32, cols as u32)
}

/// Forme canonique d'un contenu en ligne (voir [`normalize`]).
pub(crate) fn normalize_inline(content: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(content.len());
    for inline in content {
        let inline = match inline {
            Inline::Link { href, children } => Inline::Link {
                href,
                children: normalize_inline(children),
            },
            Inline::Text { text, .. } if text.is_empty() => continue,
            other => other,
        };
        match (out.last_mut(), inline) {
            (
                Some(Inline::Text {
                    text: prev,
                    styles: prev_styles,
                }),
                Inline::Text { text, styles },
            ) if *prev_styles == styles => prev.push_str(&text),
            (
                Some(Inline::Link {
                    href: prev,
                    children: kept,
                }),
                Inline::Link { href, children },
            ) if *prev == href => {
                kept.extend(children);
                *kept = normalize_inline(std::mem::take(kept));
            }
            (_, inline) => out.push(inline),
        }
    }
    out
}

/// Texte brut d'un bloc : `U+FFFC` pour une mention ou une formule, `\n`
/// pour un saut de ligne. Vide pour un bloc sans contenu en ligne.
pub fn block_text(block: &Block) -> String {
    let mut out = String::new();
    if let Content::Inline(content) = &block.content {
        push_text(content, &mut out);
    }
    out
}

fn push_text(content: &[Inline], out: &mut String) {
    for inline in content {
        match inline {
            Inline::Text { text, .. } => out.push_str(text),
            Inline::Link { children, .. } => push_text(children, out),
            Inline::Mention { .. } | Inline::Math { .. } => out.push('\u{FFFC}'),
            Inline::HardBreak => out.push('\n'),
        }
    }
}
