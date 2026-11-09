//! Rendu canonique MMD d'un arbre de blocs.

use crate::inline::{escape_attr, render_inline};
use crate::model::{Block, BlockKind, Inline};

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOpts {
    /// Ajoute une ancre courte `bN` à chaque bloc (` ^bN` en fin de ligne ou `id="bN"`).
    pub anchors: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub markdown: String,
    /// Alias `bN` → id du bloc, en profondeur d'abord, enfants compris.
    pub anchors: Vec<(String, String)>,
}

struct Ctx {
    opts: RenderOpts,
    anchors: Vec<(String, String)>,
}

impl Ctx {
    /// Alias du bloc (numérotation en profondeur d'abord), s'il y a des ancres.
    fn alias(&mut self, b: &Block) -> Option<String> {
        if !self.opts.anchors {
            return None;
        }
        let alias = format!("b{}", self.anchors.len() + 1);
        self.anchors.push((alias.clone(), b.id.clone()));
        Some(alias)
    }
}

pub fn render(blocks: &[Block], opts: RenderOpts) -> Rendered {
    let mut ctx = Ctx { opts, anchors: Vec::new() };
    let mut markdown = render_blocks(blocks, &mut ctx);
    markdown.push('\n');
    Rendered { markdown, anchors: ctx.anchors }
}

#[derive(PartialEq, Clone, Copy)]
enum Family {
    Bullet,
    Numbered,
    Todo,
    Column,
    Other,
}

/// Les blocs voisins d'une même famille se suivent sans ligne vide.
fn family(k: &BlockKind) -> Family {
    match k {
        BlockKind::BulletItem => Family::Bullet,
        BlockKind::NumberedItem => Family::Numbered,
        BlockKind::Todo { .. } => Family::Todo,
        BlockKind::Column { .. } => Family::Column,
        _ => Family::Other,
    }
}

fn render_blocks(blocks: &[Block], ctx: &mut Ctx) -> String {
    let mut out = String::new();
    let mut number = 0;
    for (i, b) in blocks.iter().enumerate() {
        let fam = family(&b.kind);
        if i > 0 {
            let prev = family(&blocks[i - 1].kind);
            out.push_str(if fam != Family::Other && fam == prev { "\n" } else { "\n\n" });
        }
        number = if fam == Family::Numbered { number + 1 } else { 0 };
        out.push_str(&render_block(b, number, ctx));
    }
    out
}

fn with_anchor(line: String, alias: Option<String>) -> String {
    match alias {
        Some(a) if line.is_empty() => format!("^{a}"),
        Some(a) => format!("{line} ^{a}"),
        None => line,
    }
}

fn own_line_anchor(block: String, alias: Option<String>) -> String {
    match alias {
        Some(a) => format!("{block}\n^{a}"),
        None => block,
    }
}

fn id_attr(alias: &Option<String>) -> String {
    alias.as_ref().map(|a| format!(" id=\"{a}\"")).unwrap_or_default()
}

fn indent(s: &str, width: usize) -> String {
    let pad = " ".repeat(width);
    s.lines().map(|l| if l.is_empty() { String::new() } else { format!("{pad}{l}") }).collect::<Vec<_>>().join("\n")
}

fn code_text(content: &[Inline]) -> String {
    content
        .iter()
        .map(|i| match i {
            Inline::Text { text, .. } => text.as_str(),
            _ => "",
        })
        .collect()
}

fn render_block(b: &Block, number: usize, ctx: &mut Ctx) -> String {
    let alias = ctx.alias(b);
    match &b.kind {
        BlockKind::Paragraph => {
            if b.content.is_empty() {
                format!("<p{}/>", id_attr(&alias))
            } else {
                with_anchor(render_inline(&b.content), alias)
            }
        }
        BlockKind::Heading { level } => {
            let head = format!("{} {}", "#".repeat((*level).clamp(1, 4) as usize), render_inline(&b.content));
            with_anchor(head, alias)
        }
        BlockKind::Quote => with_anchor(format!("> {}", render_inline(&b.content)), alias),
        BlockKind::BulletItem | BlockKind::NumberedItem | BlockKind::Todo { .. } => {
            let marker = match &b.kind {
                BlockKind::BulletItem => "- ".to_string(),
                BlockKind::NumberedItem => format!("{number}. "),
                BlockKind::Todo { checked } => format!("- [{}] ", if *checked { "x" } else { " " }),
                _ => unreachable!(),
            };
            let width = match &b.kind {
                BlockKind::NumberedItem => marker.len(),
                _ => 2,
            };
            let line = with_anchor(format!("{marker}{}", render_inline(&b.content)), alias);
            if b.children.is_empty() {
                line
            } else {
                format!("{line}\n{}", indent(&render_blocks(&b.children, ctx), width))
            }
        }
        BlockKind::Toggle => {
            let title = escape_attr(&render_inline(&b.content));
            container(&format!("<toggle{} title=\"{title}\">", id_attr(&alias)), "</toggle>", &b.children, ctx)
        }
        BlockKind::Callout { icon, color } => {
            let open = format!("<callout{} icon=\"{}\" color=\"{}\">", id_attr(&alias), escape_attr(icon), escape_attr(color));
            let mut inner = String::new();
            if !b.content.is_empty() {
                inner.push_str(&render_inline(&b.content));
            }
            if !b.children.is_empty() {
                if !inner.is_empty() {
                    inner.push_str("\n\n");
                }
                inner.push_str(&render_blocks(&b.children, ctx));
            }
            if inner.is_empty() { format!("{open}\n</callout>") } else { format!("{open}\n{inner}\n</callout>") }
        }
        BlockKind::Code { lang } => {
            let text = code_text(&b.content);
            let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
            let fence = "`".repeat(longest.max(2) + 1);
            let body = if text.is_empty() { format!("{fence}{lang}\n{fence}") } else { format!("{fence}{lang}\n{text}\n{fence}") };
            own_line_anchor(body, alias)
        }
        BlockKind::Equation { latex } => own_line_anchor(format!("$$\n{latex}\n$$"), alias),
        BlockKind::Divider => own_line_anchor("---".into(), alias),
        BlockKind::Image { url, alt } => {
            own_line_anchor(format!("![{}]({url})", render_inline(&[Inline::Text { text: alt.clone(), marks: Default::default() }])), alias)
        }
        BlockKind::File { url, name } => format!("<file{} src=\"{}\" name=\"{}\"/>", id_attr(&alias), escape_attr(url), escape_attr(name)),
        BlockKind::Bookmark { url } => format!("<bookmark{} url=\"{}\"/>", id_attr(&alias), escape_attr(url)),
        BlockKind::Table { rows } => {
            let mut lines = Vec::new();
            for (r, row) in rows.iter().enumerate() {
                let cells: Vec<String> = row.iter().map(|c| render_inline(c)).collect();
                lines.push(format!("| {} |", cells.join(" | ")));
                if r == 0 {
                    lines.push(format!("|{}", " --- |".repeat(row.len())));
                }
            }
            own_line_anchor(lines.join("\n"), alias)
        }
        BlockKind::Columns => container(&format!("<columns{}>", id_attr(&alias)), "</columns>", &b.children, ctx),
        BlockKind::Column { width } => {
            let w = width.map(|w| format!(" width=\"{w}\"")).unwrap_or_default();
            container(&format!("<column{}{w}>", id_attr(&alias)), "</column>", &b.children, ctx)
        }
        BlockKind::PageLink { page_id, title } => {
            format!("<page{} id=\"{}\">{}</page>", alias.map(|a| format!(" anchor=\"{a}\"")).unwrap_or_default(), escape_attr(page_id), escape_text_plain(title))
        }
        BlockKind::DatabaseRef { database_id, view } => {
            format!("<database{} id=\"{}\" view=\"{}\"/>", alias.map(|a| format!(" anchor=\"{a}\"")).unwrap_or_default(), escape_attr(database_id), escape_attr(view))
        }
        BlockKind::Synced { reference } => {
            container(&format!("<synced{} ref=\"{}\">", id_attr(&alias), escape_attr(reference)), "</synced>", &b.children, ctx)
        }
        BlockKind::Toc => format!("<toc{}/>", id_attr(&alias)),
        BlockKind::Unknown { type_name, data } => {
            // Toujours son vrai id : un bloc inconnu doit revenir intact.
            format!("<block type=\"{}\" id=\"{}\" data=\"{}\"/>", escape_attr(type_name), escape_attr(&b.id), escape_attr(data))
        }
    }
}

/// Texte brut entre balises (titre de page) : seulement `<` et `&` échappés.
fn escape_text_plain(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;")
}

fn container(open: &str, close: &str, children: &[Block], ctx: &mut Ctx) -> String {
    if children.is_empty() {
        format!("{open}\n{close}")
    } else {
        format!("{open}\n{}\n{close}", render_blocks(children, ctx))
    }
}
