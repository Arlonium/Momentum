//! Analyse MMD → arbre de blocs. Les blocs sont reconnus ligne à ligne ; le contenu en ligne
//! passe par pulldown-cmark (voir `inline.rs`).

use crate::inline::{attr, parse_attrs, parse_inline, unescape_attr};
use crate::model::{normalize, Block, BlockKind, Inline, MmdError};

const CONTAINERS: [&str; 5] = ["toggle", "callout", "columns", "column", "synced"];
const SELF_CLOSING: [&str; 6] = ["file", "bookmark", "database", "toc", "block", "p"];

pub fn parse(src: &str) -> Result<Vec<Block>, MmdError> {
    let lines: Vec<&str> = src.lines().collect();
    Ok(normalize(parse_lines(&lines, 0)?))
}

fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn block(kind: BlockKind, content: Vec<Inline>, children: Vec<Block>, id: Option<String>) -> Block {
    Block { id: id.unwrap_or_else(new_id), kind, content, children }
}

/// `^b12` seul sur sa ligne.
fn anchor_line(line: &str) -> Option<String> {
    let t = line.trim();
    let rest = t.strip_prefix("^b")?;
    (!rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())).then(|| t[1..].to_string())
}

/// Sépare une ancre finale ` ^bN` (ou une ligne réduite à `^bN`) du texte.
fn split_anchor(text: &str) -> (&str, Option<String>) {
    if let Some(a) = anchor_line(text) {
        return ("", Some(a));
    }
    if let Some(pos) = text.rfind(" ^b") {
        if let Some(a) = anchor_line(&text[pos + 1..]) {
            return (&text[..pos], Some(a));
        }
    }
    (text, None)
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

enum ListMarker {
    Bullet,
    Numbered,
    Todo(bool),
}

/// Marqueur de liste en tête de ligne : (type, largeur d'indentation des enfants, texte).
fn list_marker(line: &str) -> Option<(ListMarker, usize, &str)> {
    if let Some(rest) = line.strip_prefix("- [ ] ").or_else(|| (line == "- [ ]").then_some("")) {
        return Some((ListMarker::Todo(false), 2, rest));
    }
    if let Some(rest) = line.strip_prefix("- [x] ").or_else(|| (line == "- [x]").then_some("")) {
        return Some((ListMarker::Todo(true), 2, rest));
    }
    if let Some(rest) = line.strip_prefix("- ").or_else(|| (line == "-").then_some("")) {
        return Some((ListMarker::Bullet, 2, rest));
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits < 10 {
        if let Some(rest) = line[digits..].strip_prefix(". ") {
            return Some((ListMarker::Numbered, digits + 2, rest));
        }
        if &line[digits..] == "." {
            return Some((ListMarker::Numbered, digits + 2, ""));
        }
    }
    None
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    if rest.is_empty() {
        return Some((hashes.min(4) as u8, ""));
    }
    rest.strip_prefix(' ').map(|r| (hashes.min(4) as u8, r))
}

/// Balise de bloc en tête de ligne : (nom, attributs, auto-fermante, reste après `>`).
fn block_tag(line: &str) -> Option<(String, Vec<(String, String)>, bool, String)> {
    let t = line.strip_prefix('<')?;
    let name: String = t.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    let known = CONTAINERS.contains(&name.as_str()) || SELF_CLOSING.contains(&name.as_str()) || name == "page";
    if !known || t[name.len()..].starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return None;
    }
    // Fin de la balise ouvrante : premier `>` hors guillemets.
    let mut in_quotes = false;
    let mut end = None;
    for (i, c) in t.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '>' if !in_quotes => {
                end = Some(i);
                break;
            }
            _ => {}
        }
    }
    let end = end?;
    let head = &t[..end];
    let self_closing = head.ends_with('/');
    let attrs = parse_attrs(head.trim_end_matches('/'));
    Some((name, attrs, self_closing, t[end + 1..].to_string()))
}

fn starts_block(line: &str) -> bool {
    line.starts_with("```")
        || line.starts_with("~~~")
        || line.trim() == "$$"
        || matches!(line.trim(), "---" | "***" | "___")
        || heading(line).is_some()
        || line.starts_with('>')
        || list_marker(line).is_some()
        || line.starts_with('|')
        || block_tag(line).is_some()
        || anchor_line(line).is_some()
}

fn err(line: usize, message: impl Into<String>) -> MmdError {
    MmdError { line: line + 1, message: message.into() }
}

/// `offset` = numéro de la première ligne dans le document (pour les messages d'erreur).
fn parse_lines(lines: &[&str], offset: usize) -> Result<Vec<Block>, MmdError> {
    let mut out: Vec<Block> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        // Ancre sur sa propre ligne : elle désigne le bloc précédent.
        if let Some(a) = anchor_line(line) {
            match out.last_mut() {
                Some(prev) => prev.id = a,
                None => return Err(err(offset + i, "ancre sans bloc précédent")),
            }
            i += 1;
            continue;
        }
        // Bloc de code.
        if line.starts_with("```") || line.starts_with("~~~") {
            let ch = line.chars().next().unwrap();
            let fence_len = line.chars().take_while(|&c| c == ch).count();
            let lang = line[fence_len..].trim().to_string();
            let mut j = i + 1;
            let mut body = Vec::new();
            while j < lines.len() {
                let l = lines[j];
                let n = l.chars().take_while(|&c| c == ch).count();
                if n >= fence_len && l.trim_end().len() == n {
                    break;
                }
                body.push(l);
                j += 1;
            }
            if j >= lines.len() {
                return Err(err(offset + i, "bloc de code non fermé"));
            }
            let text = body.join("\n");
            let content = if text.is_empty() { vec![] } else { vec![Inline::Text { text, marks: Default::default() }] };
            out.push(block(BlockKind::Code { lang }, content, vec![], None));
            i = j + 1;
            continue;
        }
        // Équation.
        if line.trim() == "$$" {
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim() != "$$" {
                j += 1;
            }
            if j >= lines.len() {
                return Err(err(offset + i, "équation non fermée"));
            }
            out.push(block(BlockKind::Equation { latex: lines[i + 1..j].join("\n") }, vec![], vec![], None));
            i = j + 1;
            continue;
        }
        if matches!(line.trim(), "---" | "***" | "___") {
            out.push(block(BlockKind::Divider, vec![], vec![], None));
            i += 1;
            continue;
        }
        if let Some((level, text)) = heading(line) {
            let (text, a) = split_anchor(text);
            out.push(block(BlockKind::Heading { level }, parse_inline(text), vec![], a));
            i += 1;
            continue;
        }
        if line.starts_with('>') {
            let mut parts = Vec::new();
            while i < lines.len() && lines[i].starts_with('>') {
                let l = lines[i].strip_prefix('>').unwrap();
                parts.push(l.strip_prefix(' ').unwrap_or(l));
                i += 1;
            }
            let joined = parts.join("\n");
            let (text, a) = split_anchor(&joined);
            out.push(block(BlockKind::Quote, parse_inline(text), vec![], a));
            continue;
        }
        if let Some((marker, width, text)) = list_marker(line) {
            let (text, a) = split_anchor(text);
            let kind = match marker {
                ListMarker::Bullet => BlockKind::BulletItem,
                ListMarker::Numbered => BlockKind::NumberedItem,
                ListMarker::Todo(checked) => BlockKind::Todo { checked },
            };
            // Enfants : lignes indentées d'au moins `width` (lignes vides comprises si la suite l'est).
            let mut j = i + 1;
            let mut child_lines: Vec<&str> = Vec::new();
            while j < lines.len() {
                let l = lines[j];
                if l.trim().is_empty() {
                    let next = lines[j + 1..].iter().find(|x| !x.trim().is_empty());
                    if next.map(|x| indent_of(x) >= width).unwrap_or(false) {
                        child_lines.push("");
                        j += 1;
                        continue;
                    }
                    break;
                }
                if indent_of(l) >= width {
                    child_lines.push(&l[width..]);
                    j += 1;
                } else {
                    break;
                }
            }
            let children = parse_lines(&child_lines, offset + i + 1)?;
            out.push(block(kind, parse_inline(text), children, a));
            i = j;
            continue;
        }
        if line.starts_with('|') {
            let mut rows = Vec::new();
            let mut j = i;
            while j < lines.len() && lines[j].starts_with('|') {
                if j != i + 1 {
                    rows.push(split_row(lines[j]).into_iter().map(|c| parse_inline(c.trim())).collect());
                }
                j += 1;
            }
            out.push(block(BlockKind::Table { rows }, vec![], vec![], None));
            i = j;
            continue;
        }
        if let Some(rest) = line.strip_prefix("![") {
            if let Some(close) = find_unescaped(rest, "](") {
                if rest.ends_with(')') {
                    let alt: String = parse_inline(&rest[..close])
                        .into_iter()
                        .map(|x| match x {
                            Inline::Text { text, .. } => text,
                            _ => String::new(),
                        })
                        .collect();
                    let url = rest[close + 2..rest.len() - 1].to_string();
                    out.push(block(BlockKind::Image { url, alt }, vec![], vec![], None));
                    i += 1;
                    continue;
                }
            }
        }
        if let Some((name, attrs, self_closing, rest)) = block_tag(line) {
            let a = |k: &str| attr(&attrs, k).unwrap_or("").to_string();
            let id = attr(&attrs, "id").map(str::to_string);
            if name == "page" {
                let title = rest.strip_suffix("</page>").ok_or_else(|| err(offset + i, "<page> doit tenir sur une ligne"))?;
                let anchor = attr(&attrs, "anchor").map(str::to_string);
                out.push(block(BlockKind::PageLink { page_id: a("id"), title: unescape_attr(title) }, vec![], vec![], anchor));
                i += 1;
                continue;
            }
            if self_closing {
                let b = match name.as_str() {
                    "p" => block(BlockKind::Paragraph, vec![], vec![], id),
                    "file" => block(BlockKind::File { url: a("src"), name: a("name") }, vec![], vec![], id),
                    "bookmark" => block(BlockKind::Bookmark { url: a("url") }, vec![], vec![], id),
                    "database" => block(
                        BlockKind::DatabaseRef { database_id: a("id"), view: a("view") },
                        vec![],
                        vec![],
                        attr(&attrs, "anchor").map(str::to_string),
                    ),
                    "toc" => block(BlockKind::Toc, vec![], vec![], id),
                    "block" => block(BlockKind::Unknown { type_name: a("type"), data: a("data") }, vec![], vec![], id),
                    other => return Err(err(offset + i, format!("<{other}/> inattendu"))),
                };
                out.push(b);
                i += 1;
                continue;
            }
            // Conteneur : chercher la fermeture au même niveau.
            let close = format!("</{name}>");
            let mut depth = 1;
            let mut j = i + 1;
            while j < lines.len() {
                let l = lines[j];
                if l == close {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if let Some((n2, _, sc, _)) = block_tag(l) {
                    if n2 == name && !sc {
                        depth += 1;
                    }
                }
                j += 1;
            }
            if j >= lines.len() {
                return Err(err(offset + i, format!("<{name}> non fermé")));
            }
            let inner = parse_lines(&lines[i + 1..j], offset + i + 1)?;
            let b = match name.as_str() {
                "toggle" => block(BlockKind::Toggle, parse_inline(&a("title")), inner, id),
                "callout" => {
                    let mut inner = inner;
                    let content = if inner.first().map(|b| b.kind == BlockKind::Paragraph).unwrap_or(false) {
                        inner.remove(0).content
                    } else {
                        vec![]
                    };
                    block(BlockKind::Callout { icon: a("icon"), color: a("color") }, content, inner, id)
                }
                "columns" => block(BlockKind::Columns, vec![], inner, id),
                "column" => {
                    let width = attr(&attrs, "width").and_then(|w| w.parse().ok());
                    block(BlockKind::Column { width }, vec![], inner, id)
                }
                "synced" => block(BlockKind::Synced { reference: a("ref") }, vec![], inner, id),
                other => return Err(err(offset + i, format!("<{other}> inattendu"))),
            };
            out.push(b);
            i = j + 1;
            continue;
        }
        // Paragraphe : jusqu'à une ligne vide ou un début de bloc.
        let mut parts = vec![line];
        let mut j = i + 1;
        while j < lines.len() && !lines[j].trim().is_empty() && !starts_block(lines[j]) {
            parts.push(lines[j]);
            j += 1;
        }
        let joined = parts.join("\n");
        let (text, a) = split_anchor(&joined);
        out.push(block(BlockKind::Paragraph, parse_inline(text), vec![], a));
        i = j;
    }
    Ok(out)
}

/// Découpe une ligne de tableau `| a | b |` sur les `|` non échappés.
fn split_row(line: &str) -> Vec<&str> {
    let inner = line.trim().trim_start_matches('|');
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut start = 0;
    let bytes = inner.as_bytes();
    let mut k = 0;
    while k < bytes.len() {
        match bytes[k] {
            b'\\' => k += 2,
            b'|' => {
                cells.push(&inner[start..k]);
                start = k + 1;
                k += 1;
            }
            _ => k += 1,
        }
    }
    cells.push(&inner[start.min(inner.len())..]);
    cells
}

fn find_unescaped(s: &str, pat: &str) -> Option<usize> {
    let mut k = 0;
    while k < s.len() {
        if s[k..].starts_with('\\') {
            k += 1 + s[k + 1..].chars().next().map(char::len_utf8).unwrap_or(0);
            continue;
        }
        if s[k..].starts_with(pat) {
            return Some(k);
        }
        k += s[k..].chars().next().map(char::len_utf8).unwrap_or(1);
    }
    None
}
