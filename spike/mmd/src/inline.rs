//! Contenu en ligne : rendu MMD (Markdown + balises) et analyse via pulldown-cmark.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::model::{normalize_inline, Inline, Marks};

// ---------------------------------------------------------------- rendu

/// Échappe un texte littéral pour qu'il reste du texte une fois relu.
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '*' | '_' | '`' | '[' | ']' | '~' | '$' | '|' | '^' => {
                out.push('\\');
                out.push(c);
            }
            '<' => out.push_str("&lt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}

/// Échappe une valeur d'attribut `"…"`.
pub fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

pub fn unescape_attr(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&lt;", "<").replace("&amp;", "&")
}

fn bounded_alnum(s: &str) -> bool {
    match (s.chars().next(), s.chars().last()) {
        (Some(a), Some(b)) => a.is_alphanumeric() && b.is_alphanumeric(),
        _ => false,
    }
}

fn plain_neighbor(n: Option<&Inline>) -> bool {
    match n {
        None => true,
        Some(Inline::Text { marks, .. }) => marks.is_plain(),
        _ => false,
    }
}

fn render_marked(text: &str, marks: &Marks, delimiters: bool) -> String {
    let mut s = escape_text(text);
    if marks.code {
        s = if delimiters { format!("`{text}`") } else { format!("<code>{s}</code>") };
    }
    if marks.italic {
        s = if delimiters { format!("*{s}*") } else { format!("<em>{s}</em>") };
    }
    if marks.bold {
        s = if delimiters { format!("**{s}**") } else { format!("<strong>{s}</strong>") };
    }
    if marks.strike {
        s = if delimiters { format!("~~{s}~~") } else { format!("<s>{s}</s>") };
    }
    if marks.underline {
        s = format!("<u>{s}</u>");
    }
    if let Some(color) = &marks.color {
        s = format!("<span color=\"{}\">{s}</span>", escape_attr(color));
    }
    s
}

fn render_seq(content: &[Inline]) -> String {
    let mut out = String::new();
    for (i, inline) in content.iter().enumerate() {
        match inline {
            Inline::Text { text, marks } if marks.is_plain() => {
                let mut s = escape_text(text);
                // « ! » suivi d'un lien se relirait comme une image `![…](…)`.
                if s.ends_with('!') && matches!(content.get(i + 1), Some(Inline::Link { .. })) {
                    s.insert(s.len() - 1, '\\');
                }
                out.push_str(&s);
            }
            Inline::Text { text, marks } => {
                // Délimiteurs Markdown seulement quand ils sont sans ambiguïté, sinon balises HTML.
                let delimiters = bounded_alnum(text)
                    && !(marks.code && text.contains('`'))
                    && plain_neighbor(if i == 0 { None } else { content.get(i - 1) })
                    && plain_neighbor(content.get(i + 1));
                out.push_str(&render_marked(text, marks, delimiters));
            }
            Inline::Link { href, children } => {
                out.push('[');
                out.push_str(&render_seq(children));
                out.push_str("](");
                out.push_str(href);
                out.push(')');
            }
            Inline::Mention { kind, id, label } => {
                out.push_str(&format!("<mention type=\"{}\" id=\"{}\">{}</mention>", escape_attr(kind), escape_attr(id), escape_text(label)));
            }
            Inline::Math { latex } => {
                out.push('$');
                out.push_str(latex);
                out.push('$');
            }
        }
    }
    out
}

/// Rend un contenu en ligne sur une seule ligne, sûr en début de ligne et sans espaces de bord perdus.
pub fn render_inline(content: &[Inline]) -> String {
    let mut s = render_seq(content);
    // Débuts de ligne qui seraient pris pour un bloc.
    let starts_block = s.starts_with('#') || s.starts_with('>') || s.starts_with('-') || s.starts_with('+');
    if starts_block {
        s.insert(0, '\\');
    } else {
        let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && matches!(s[digits..].chars().next(), Some('.') | Some(')')) {
            s.insert(digits, '\\');
        }
    }
    // Espaces de bord : sinon Markdown les supprime.
    if s.starts_with(' ') {
        s.replace_range(0..1, "&#32;");
    }
    if s.ends_with(' ') {
        let n = s.len();
        s.replace_range(n - 1..n, "&#32;");
    }
    s
}

// ---------------------------------------------------------------- analyse

#[derive(Default)]
struct State {
    bold: u32,
    italic: u32,
    strike: u32,
    code: u32,
    underline: u32,
    colors: Vec<String>,
}

impl State {
    fn marks(&self) -> Marks {
        Marks {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code: self.code > 0,
            underline: self.underline > 0,
            color: self.colors.last().cloned(),
        }
    }
}

/// Attributs `nom="valeur"` d'une balise.
pub fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(eq) = rest.find("=\"") {
        let name = rest[..eq].trim().trim_start_matches('<').to_string();
        let after = &rest[eq + 2..];
        let Some(end) = after.find('"') else { break };
        out.push((name.split_whitespace().last().unwrap_or("").to_string(), unescape_attr(&after[..end])));
        rest = &after[end + 1..];
    }
    out
}

pub fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// Nom de balise et fermeture éventuelle : `<span color="x">` → ("span", false).
fn tag_name(html: &str) -> (String, bool) {
    let t = html.trim().trim_start_matches('<');
    let closing = t.starts_with('/');
    let t = t.trim_start_matches('/');
    let name: String = t.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (name.to_ascii_lowercase(), closing)
}

pub fn parse_inline(src: &str) -> Vec<Inline> {
    let parser = Parser::new_ext(src, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_MATH);
    let mut st = State::default();
    // Pile de cadres : le contenu courant, puis un cadre par lien ouvert.
    let mut frames: Vec<(Option<String>, Vec<Inline>)> = vec![(None, Vec::new())];
    let mut mention: Option<(String, String, String)> = None;

    let push = |frames: &mut Vec<(Option<String>, Vec<Inline>)>, inline: Inline| frames.last_mut().unwrap().1.push(inline);

    for event in parser {
        match event {
            Event::Text(t) => {
                if let Some(m) = mention.as_mut() {
                    m.2.push_str(&t);
                } else {
                    push(&mut frames, Inline::Text { text: t.to_string(), marks: st.marks() });
                }
            }
            Event::Code(t) => {
                let mut marks = st.marks();
                marks.code = true;
                push(&mut frames, Inline::Text { text: t.to_string(), marks });
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => push(&mut frames, Inline::Math { latex: t.to_string() }),
            Event::SoftBreak => push(&mut frames, Inline::Text { text: " ".into(), marks: st.marks() }),
            Event::HardBreak => push(&mut frames, Inline::Text { text: "\n".into(), marks: st.marks() }),
            Event::Start(Tag::Emphasis) => st.italic += 1,
            Event::End(TagEnd::Emphasis) => st.italic = st.italic.saturating_sub(1),
            Event::Start(Tag::Strong) => st.bold += 1,
            Event::End(TagEnd::Strong) => st.bold = st.bold.saturating_sub(1),
            Event::Start(Tag::Strikethrough) => st.strike += 1,
            Event::End(TagEnd::Strikethrough) => st.strike = st.strike.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. }) => frames.push((Some(dest_url.to_string()), Vec::new())),
            Event::End(TagEnd::Link) => {
                if frames.len() > 1 {
                    let (href, children) = frames.pop().unwrap();
                    push(&mut frames, Inline::Link { href: href.unwrap_or_default(), children });
                }
            }
            Event::InlineHtml(h) | Event::Html(h) => {
                let (name, closing) = tag_name(&h);
                match (name.as_str(), closing) {
                    ("u", false) => st.underline += 1,
                    ("u", true) => st.underline = st.underline.saturating_sub(1),
                    ("strong" | "b", false) => st.bold += 1,
                    ("strong" | "b", true) => st.bold = st.bold.saturating_sub(1),
                    ("em" | "i", false) => st.italic += 1,
                    ("em" | "i", true) => st.italic = st.italic.saturating_sub(1),
                    ("s" | "del", false) => st.strike += 1,
                    ("s" | "del", true) => st.strike = st.strike.saturating_sub(1),
                    ("code", false) => st.code += 1,
                    ("code", true) => st.code = st.code.saturating_sub(1),
                    ("span", false) => st.colors.push(attr(&parse_attrs(&h), "color").unwrap_or("default").to_string()),
                    ("span", true) => {
                        st.colors.pop();
                    }
                    ("mention", false) => {
                        let a = parse_attrs(&h);
                        mention = Some((attr(&a, "type").unwrap_or("page").to_string(), attr(&a, "id").unwrap_or("").to_string(), String::new()));
                    }
                    ("mention", true) => {
                        if let Some((kind, id, label)) = mention.take() {
                            push(&mut frames, Inline::Mention { kind, id, label });
                        }
                    }
                    _ => push(&mut frames, Inline::Text { text: h.to_string(), marks: st.marks() }),
                }
            }
            _ => {}
        }
    }
    normalize_inline(frames.swap_remove(0).1)
}
