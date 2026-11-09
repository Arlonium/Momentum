//! Contenu en ligne : rendu MMD (Markdown et balises) et lecture par
//! pulldown-cmark.

use std::fmt::Write as _;

use pulldown_cmark::{Event, Options, Parser, Tag as MdTag, TagEnd};

use crate::attrs::{Raw, Tag, escape_attr, parse_tag, quoted};
use crate::model::{Inline, Styles, normalize_inline};
use crate::parse::MAX_DEPTH;

const OPTIONS: Options = Options::ENABLE_STRIKETHROUGH.union(Options::ENABLE_MATH);

// ------------------------------------------------------------------- rendu

/// Échappe un texte littéral pour qu'il soit relu tel quel.
pub(crate) fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '*' | '_' | '`' | '[' | ']' | '~' | '$' | '|' | '^' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            '<' => out.push_str("&lt;"),
            '&' => out.push_str("&amp;"),
            // Une tabulation ou un saut de ligne casserait la ligne ou serait
            // supprimé en bord : référence numérique.
            c if c.is_ascii_control() && c != '\0' => {
                let _ = write!(out, "&#{};", c as u32);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Échappe le texte brut écrit entre deux balises (titre d'un `<page>`).
pub(crate) fn escape_plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

/// Vrai si `[texte](href)` relit exactement `href` ; sinon `<a href="…">`.
pub(crate) fn href_safe(href: &str) -> bool {
    !href.chars().any(|c| {
        c.is_whitespace() || c.is_control() || matches!(c, '(' | ')' | '<' | '>' | '\\' | '&')
    })
}

/// Vrai si `$latex$` se relit exactement en cette formule ; sinon (formule
/// vide, blanc en bord, `$`, accolades déséquilibrées, saut de ligne…) elle
/// s'écrit `<math latex="…"/>`. Le contexte n'y change rien : les accolades
/// et les `$` du texte voisin sont échappés.
pub(crate) fn math_fits(latex: &str) -> bool {
    !latex.is_empty()
        && !latex.contains(['\n', '\r'])
        && matches!(
            parse_inline(&format!("${latex}$")).as_slice(),
            [Inline::Math { latex: read }] if read == latex
        )
}

fn bounded_alnum(s: &str) -> bool {
    match (s.chars().next(), s.chars().last()) {
        (Some(a), Some(b)) => a.is_alphanumeric() && b.is_alphanumeric(),
        _ => false,
    }
}

/// Un seul genre de délimiteur (ou `***` pour gras et italique) : deux
/// genres imbriqués (`~~**a**~~`, `` **`a`** ``) ne se referment pas devant
/// une lettre, le délimiteur extérieur touchant une ponctuation.
fn single_delimiter(s: &Styles) -> bool {
    matches!(
        (s.code, s.italic, s.bold, s.strike),
        (true, false, false, false)
            | (false, true, false, false)
            | (false, false, true, false)
            | (false, true, true, false)
            | (false, false, false, true)
            | (false, false, false, false)
    )
}

fn plain_neighbor(n: Option<&Inline>) -> bool {
    match n {
        None => true,
        Some(Inline::Text { styles, .. }) => styles.is_plain(),
        Some(_) => false,
    }
}

/// Texte marqué : délimiteurs Markdown ou balises (règle de la P0).
fn render_marked(text: &str, styles: &Styles, delimiters: bool) -> String {
    let mut s = if styles.code && delimiters {
        format!("`{text}`")
    } else if styles.code {
        format!("<code>{}</code>", escape_text(text))
    } else {
        escape_text(text)
    };
    if styles.italic {
        s = if delimiters {
            format!("*{s}*")
        } else {
            format!("<em>{s}</em>")
        };
    }
    if styles.bold {
        s = if delimiters {
            format!("**{s}**")
        } else {
            format!("<strong>{s}</strong>")
        };
    }
    if styles.strike {
        s = if delimiters {
            format!("~~{s}~~")
        } else {
            format!("<s>{s}</s>")
        };
    }
    if styles.underline {
        s = format!("<u>{s}</u>");
    }
    if styles.background_color.is_some() || styles.text_color.is_some() {
        let mut open = String::from("<span");
        if let Some(bg) = &styles.background_color {
            let _ = write!(open, " bg={}", quoted(bg));
        }
        if let Some(color) = &styles.text_color {
            let _ = write!(open, " color={}", quoted(color));
        }
        s = format!("{open}>{s}</span>");
    }
    s
}

fn render_seq(content: &[Inline]) -> String {
    let mut out = String::new();
    for (i, inline) in content.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|p| content.get(p));
        let next = content.get(i + 1);
        match inline {
            Inline::Text { text, styles } if styles.is_plain() => {
                let mut s = escape_text(text);
                // « ! » suivi d'un lien se relirait comme une image `![…](…)`.
                if s.ends_with('!') && matches!(next, Some(Inline::Link { .. })) {
                    s.insert(s.len() - 1, '\\');
                }
                out.push_str(&s);
            }
            Inline::Text { text, styles } => {
                // Délimiteurs seulement sans ambiguïté, sinon balises.
                let delimiters = bounded_alnum(text)
                    && single_delimiter(styles)
                    && !(styles.code && text.chars().any(|c| c == '`' || c.is_control()))
                    && plain_neighbor(prev)
                    && plain_neighbor(next);
                out.push_str(&render_marked(text, styles, delimiters));
            }
            Inline::Link { href, children } if href_safe(href) => {
                let _ = write!(out, "[{}]({href})", render_seq(children));
            }
            Inline::Link { href, children } => {
                let _ = write!(
                    out,
                    "<a href=\"{}\">{}</a>",
                    escape_attr(href),
                    render_seq(children)
                );
            }
            Inline::Mention { kind, id, label } => {
                let _ = write!(
                    out,
                    "<mention type={} id={}>{}</mention>",
                    quoted(kind),
                    quoted(id),
                    escape_text(label)
                );
            }
            Inline::Math { latex } if math_fits(latex) => {
                let _ = write!(out, "${latex}$");
            }
            Inline::Math { latex } => {
                let _ = write!(out, "<math latex={}/>", quoted(latex));
            }
            Inline::HardBreak => out.push_str("<br/>"),
        }
    }
    out
}

/// Rend un contenu en ligne sur une seule ligne, sûre en début de ligne et
/// sans espace de bord perdue.
pub(crate) fn render_inline(content: &[Inline]) -> String {
    let mut s = render_seq(content);
    // Débuts de ligne qui seraient pris pour un bloc.
    if s.starts_with(['#', '>', '-', '+']) {
        s.insert(0, '\\');
    } else {
        let digits = s.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && s[digits..].starts_with(['.', ')']) {
            s.insert(digits, '\\');
        }
    }
    // Espaces de bord : le Markdown les supprimerait.
    if s.starts_with(' ') {
        s.replace_range(0..1, "&#32;");
    }
    if s.ends_with(' ') {
        let n = s.len();
        s.replace_range(n - 1..n, "&#32;");
    }
    s
}

// ----------------------------------------------------------------- lecture

#[derive(Default)]
struct State {
    bold: u32,
    italic: u32,
    strike: u32,
    code: u32,
    underline: u32,
    /// Une entrée par `<span>` ouvert : (couleur, surlignage) en vigueur
    /// après ce `<span>`, pour lire les styles sans parcourir la pile.
    spans: Vec<(Option<String>, Option<String>)>,
}

impl State {
    fn styles(&self) -> Styles {
        let (text_color, background_color) = self.spans.last().cloned().unwrap_or_default();
        Styles {
            bold: self.bold > 0,
            italic: self.italic > 0,
            underline: self.underline > 0,
            strike: self.strike > 0,
            code: self.code > 0,
            text_color,
            background_color,
        }
    }

    fn open_span(&mut self, color: Option<String>, bg: Option<String>) {
        let (cur_color, cur_bg) = self.spans.last().cloned().unwrap_or_default();
        self.spans.push((color.or(cur_color), bg.or(cur_bg)));
    }
}

fn attr(tag: &Tag<'_>, key: &str) -> Option<String> {
    tag.attrs.iter().find_map(|(k, v)| match v {
        Raw::Str(s) if k == key => Some(s.clone()),
        _ => None,
    })
}

struct Reader {
    st: State,
    /// Contenu courant, puis un cadre par lien ouvert (adresse, contenu).
    frames: Vec<(String, Vec<Inline>)>,
    /// Liens ouverts au-delà de [`MAX_DEPTH`] : ignorés, pour ne jamais
    /// bâtir un arbre plus profond (l'entrée, non canonique, est refusée).
    excess_links: usize,
    mention: Option<(String, String, String)>,
}

impl Reader {
    fn push(&mut self, inline: Inline) {
        if let Some(frame) = self.frames.last_mut() {
            frame.1.push(inline);
        }
    }

    fn text(&mut self, text: &str) {
        if let Some(m) = self.mention.as_mut() {
            m.2.push_str(text);
        } else {
            let styles = self.st.styles();
            self.push(Inline::Text {
                text: text.into(),
                styles,
            });
        }
    }

    fn open_link(&mut self, href: String) {
        if self.frames.len() > MAX_DEPTH {
            self.excess_links += 1;
        } else {
            self.frames.push((href, Vec::new()));
        }
    }

    fn close_link(&mut self) {
        if self.excess_links > 0 {
            self.excess_links -= 1;
        } else if self.frames.len() > 1
            && let Some((href, children)) = self.frames.pop()
        {
            self.push(Inline::Link { href, children });
        }
    }

    fn html(&mut self, raw: &str) {
        let h = raw.trim_end();
        let dec = |n: &mut u32| *n = n.saturating_sub(1);
        if let Some(name) = h.strip_prefix("</").and_then(|r| r.strip_suffix('>')) {
            match name {
                "u" => dec(&mut self.st.underline),
                "strong" => dec(&mut self.st.bold),
                "em" => dec(&mut self.st.italic),
                "s" => dec(&mut self.st.strike),
                "code" => dec(&mut self.st.code),
                "span" => {
                    self.st.spans.pop();
                }
                "a" => self.close_link(),
                "mention" => {
                    if let Some((kind, id, label)) = self.mention.take() {
                        self.push(Inline::Mention { kind, id, label });
                    }
                }
                _ => self.text(h),
            }
            return;
        }
        let Ok(tag) = parse_tag(h) else {
            return self.text(h);
        };
        if !tag.rest.is_empty() {
            return self.text(h);
        }
        match (tag.name, tag.self_closing) {
            ("br", true) => self.push(Inline::HardBreak),
            ("math", true) => self.push(Inline::Math {
                latex: attr(&tag, "latex").unwrap_or_default(),
            }),
            ("u", false) => self.st.underline += 1,
            ("strong", false) => self.st.bold += 1,
            ("em", false) => self.st.italic += 1,
            ("s", false) => self.st.strike += 1,
            ("code", false) => self.st.code += 1,
            ("span", false) => self.st.open_span(attr(&tag, "color"), attr(&tag, "bg")),
            ("a", false) => self.open_link(attr(&tag, "href").unwrap_or_default()),
            ("mention", false) => {
                self.mention = Some((
                    attr(&tag, "type").unwrap_or_default(),
                    attr(&tag, "id").unwrap_or_default(),
                    String::new(),
                ));
            }
            _ => self.text(h),
        }
    }
}

/// Lit un contenu en ligne MMD. Une écriture non canonique est lue au mieux :
/// la vérification finale de [`crate::parse`] la refuse.
pub(crate) fn parse_inline(src: &str) -> Vec<Inline> {
    let mut r = Reader {
        st: State::default(),
        frames: vec![(String::new(), Vec::new())],
        excess_links: 0,
        mention: None,
    };
    for event in Parser::new_ext(src, OPTIONS) {
        match event {
            Event::Text(t) => r.text(&t),
            Event::Code(t) => {
                let mut styles = r.st.styles();
                styles.code = true;
                r.push(Inline::Text {
                    text: t.to_string(),
                    styles,
                });
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => r.push(Inline::Math {
                latex: t.to_string(),
            }),
            Event::SoftBreak | Event::HardBreak => r.text("\n"),
            Event::Start(MdTag::Emphasis) => r.st.italic += 1,
            Event::End(TagEnd::Emphasis) => r.st.italic = r.st.italic.saturating_sub(1),
            Event::Start(MdTag::Strong) => r.st.bold += 1,
            Event::End(TagEnd::Strong) => r.st.bold = r.st.bold.saturating_sub(1),
            Event::Start(MdTag::Strikethrough) => r.st.strike += 1,
            Event::End(TagEnd::Strikethrough) => r.st.strike = r.st.strike.saturating_sub(1),
            Event::Start(MdTag::Link { dest_url, .. }) => r.open_link(dest_url.to_string()),
            Event::End(TagEnd::Link) => r.close_link(),
            Event::InlineHtml(h) | Event::Html(h) => r.html(&h),
            _ => {}
        }
    }
    while r.frames.len() > 1 {
        r.close_link();
    }
    normalize_inline(r.frames.pop().map(|f| f.1).unwrap_or_default())
}

/// Lit `![légende](adresse)` seul : (adresse, légende en texte brut).
pub(crate) fn parse_image(src: &str) -> Option<(String, String)> {
    let mut events = Parser::new_ext(src, OPTIONS).filter(|e| {
        !matches!(
            e,
            Event::Start(MdTag::Paragraph) | Event::End(TagEnd::Paragraph)
        )
    });
    let Some(Event::Start(MdTag::Image { dest_url, .. })) = events.next() else {
        return None;
    };
    let mut caption = String::new();
    loop {
        match events.next()? {
            Event::Text(t) => caption.push_str(&t),
            Event::End(TagEnd::Image) => break,
            _ => return None,
        }
    }
    events
        .next()
        .is_none()
        .then(|| (dest_url.to_string(), caption))
}
