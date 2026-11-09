//! Arbre de blocs : le modèle commun au format MMD, au document Yjs et au serveur.

use std::collections::BTreeMap;
use std::fmt;

use mementum_schema::{PropDefault, PropSpec, block, cell_props};

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
    pub header_rows: u32,
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

/// Forme canonique d'un arbre : props égales au défaut du registre retirées,
/// textes vides retirés, textes voisins de mêmes styles fusionnés.
pub fn normalize(blocks: Vec<Block>) -> Vec<Block> {
    blocks.into_iter().map(normalize_block).collect()
}

fn normalize_block(mut b: Block) -> Block {
    if let Some(spec) = block(&b.kind) {
        strip_defaults(&mut b.props, spec.props);
    }
    b.content = match b.content {
        Content::None => Content::None,
        Content::Inline(c) => Content::Inline(normalize_inline(c)),
        Content::Table(t) => Content::Table(normalize_table(t)),
    };
    b.children = normalize(b.children);
    b
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
    t
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
        if let (
            Some(Inline::Text {
                text: prev,
                styles: prev_styles,
            }),
            Inline::Text { text, styles },
        ) = (out.last_mut(), &inline)
            && prev_styles == styles
        {
            prev.push_str(text);
            continue;
        }
        out.push(inline);
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
