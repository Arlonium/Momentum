//! Arbre de blocs commun à l'éditeur (BlockNote), au format MMD et au serveur.

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub id: String,
    pub kind: BlockKind,
    pub content: Vec<Inline>,
    pub children: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Paragraph,
    Heading { level: u8 },
    BulletItem,
    NumberedItem,
    Todo { checked: bool },
    Toggle,
    Quote,
    Callout { icon: String, color: String },
    Code { lang: String },
    Equation { latex: String },
    Divider,
    Image { url: String, alt: String },
    File { url: String, name: String },
    Bookmark { url: String },
    Table { rows: Vec<Vec<Vec<Inline>>> },
    Columns,
    Column { width: Option<f32> },
    PageLink { page_id: String, title: String },
    DatabaseRef { database_id: String, view: String },
    Synced { reference: String },
    Toc,
    Unknown { type_name: String, data: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text { text: String, marks: Marks },
    Link { href: String, children: Vec<Inline> },
    Mention { kind: String, id: String, label: String },
    Math { latex: String },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub underline: bool,
    pub color: Option<String>,
}

impl Marks {
    pub fn is_plain(&self) -> bool {
        *self == Marks::default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MmdError {
    pub line: usize,
    pub message: String,
}

/// Forme canonique d'un contenu en ligne : textes vides retirés, textes voisins de mêmes marques fusionnés.
pub fn normalize_inline(content: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(content.len());
    for inline in content {
        let inline = match inline {
            Inline::Link { href, children } => Inline::Link { href, children: normalize_inline(children) },
            other => other,
        };
        if let Inline::Text { text, .. } = &inline {
            if text.is_empty() {
                continue;
            }
        }
        if let (Some(Inline::Text { text: prev, marks: pm }), Inline::Text { text, marks }) = (out.last_mut(), &inline) {
            if pm == marks {
                prev.push_str(text);
                continue;
            }
        }
        out.push(inline);
    }
    out
}

/// Forme canonique d'un arbre de blocs (voir [`normalize_inline`]).
pub fn normalize(blocks: Vec<Block>) -> Vec<Block> {
    blocks
        .into_iter()
        .map(|b| {
            let kind = match b.kind {
                BlockKind::Table { rows } => BlockKind::Table {
                    rows: rows.into_iter().map(|r| r.into_iter().map(normalize_inline).collect()).collect(),
                },
                k => k,
            };
            Block { id: b.id, kind, content: normalize_inline(b.content), children: normalize(b.children) }
        })
        .collect()
}
