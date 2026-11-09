//! Rendu canonique d'un arbre de blocs en MMD, piloté par la forme MMD de
//! chaque type du registre (§6.2 de la conception).

use std::collections::BTreeMap;

use mementum_schema::{BlockSpec, ContentKind, MmdForm, PropSpec, block, cell_props};

use crate::attrs::{format_attrs, format_list, format_value, quoted};
use crate::inline::{escape_plain, escape_text, href_safe, render_inline};
use crate::model::{
    Block, Content, Inline, PropValue, Props, RenderOpts, Rendered, default_value, is_default,
    normalize,
};

/// Écrit la forme canonique de `blocks` (normalisés au passage).
///
/// L'appelant garantit (`mementum-crdt` le fait en lisant la page, Ruling
/// R10 du lot 1) :
/// - les props connues du registre sont du type qu'il déclare ;
/// - les flottants sont finis ;
/// - un tableau a au moins une ligne ;
/// - l'imbrication ne dépasse pas [`crate::MAX_DEPTH`] niveaux.
///
/// Sinon, `render` ne panique pas mais écrit un MMD que [`crate::parse`]
/// refuse : une prop mal typée s'écrit selon le type de sa valeur ; un
/// flottant non fini s'écrit `NaN` ou `inf` ; un tableau sans ligne n'écrit
/// aucune ligne de tableau ; un arbre trop profond s'écrit en entier (le
/// rendu est récursif : des milliers de niveaux peuvent épuiser la pile).
/// Un type hors registre s'écrit `<block type="…"/>` et se relit en
/// `unknown`.
pub fn render(blocks: &[Block], opts: RenderOpts) -> Rendered {
    let mut r = Renderer {
        ids: if opts.anchors {
            Ids::Aliases
        } else {
            Ids::None
        },
        aliases: BTreeMap::new(),
    };
    let markdown = r.document(&normalize(blocks.to_vec()));
    Rendered {
        markdown,
        aliases: r.aliases,
    }
}

/// Rendu qui écrit les vrais ids comme ancres : la lecture s'en sert pour
/// vérifier qu'un MMD est canonique.
pub(crate) fn render_ids(blocks: &[Block]) -> String {
    Renderer {
        ids: Ids::Real,
        aliases: BTreeMap::new(),
    }
    .document(blocks)
}

/// Les éléments voisins d'une même famille se suivent sans ligne vide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    Bullet,
    Numbered,
    Check,
    Column,
}

pub(crate) fn family(kind: &str) -> Option<Family> {
    match kind {
        "bulletListItem" => Some(Family::Bullet),
        "numberedListItem" => Some(Family::Numbered),
        "checkListItem" => Some(Family::Check),
        "column" => Some(Family::Column),
        _ => None,
    }
}

/// Plus grand numéro écrit dans le marqueur du premier élément d'une liste
/// numérotée (au-delà, `start` va dans la liste `{…}`).
const MAX_START: i64 = 999_999_999;

#[derive(Clone, Copy, PartialEq)]
enum Ids {
    None,
    Aliases,
    Real,
}

struct Renderer {
    ids: Ids,
    aliases: BTreeMap<String, String>,
}

/// Numéro d'un élément de liste numérotée, et vrai s'il porte `start`.
#[derive(Clone, Copy)]
struct Number {
    n: i64,
    carries_start: bool,
}

fn str_prop<'a>(props: &'a Props, name: &str) -> Option<&'a str> {
    match props.get(name) {
        Some(PropValue::Str(s)) => Some(s),
        _ => None,
    }
}

fn inline_of(b: &Block) -> &[Inline] {
    match &b.content {
        Content::Inline(c) => c,
        _ => &[],
    }
}

/// Props à écrire hors de la forme : (nom MMD, valeur écrite), triées.
/// Une prop inconnue du registre garde son nom BlockNote.
fn extra(props: &Props, specs: &[PropSpec], carried: &[&str]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = props
        .iter()
        .filter(|(name, _)| !carried.contains(&name.as_str()))
        .filter_map(
            |(name, value)| match specs.iter().find(|p| p.name == name) {
                Some(p) if is_default(value, p.default) => None,
                Some(p) => Some((p.mmd.to_string(), format_value(value, true))),
                None => Some((name.clone(), format_value(value, false))),
            },
        )
        .collect();
    out.sort();
    out
}

/// Ligne d'un bloc : marqueur, contenu, liste `{…}`, ancre, séparés d'une espace.
fn assemble(head: &str, content: &str, list: &[(String, String)], anchor: Option<&str>) -> String {
    let list = (!list.is_empty()).then(|| format_list(list));
    let anchor = anchor.map(|a| format!("^{a}"));
    [
        Some(head),
        Some(content),
        list.as_deref(),
        anchor.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

fn indent(lines: Vec<String>, width: usize) -> impl Iterator<Item = String> {
    let pad = " ".repeat(width);
    lines
        .into_iter()
        .map(move |l| if l.is_empty() { l } else { format!("{pad}{l}") })
}

/// Langue écrite après la clôture d'un bloc de code (sinon dans la liste).
fn language_fits(lang: &str) -> bool {
    !lang.is_empty()
        && !lang.starts_with('{')
        && !lang
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '`')
}

/// Attribut d'ancre d'une balise : `anchor` si une prop s'écrit déjà `id`
/// (`<page>`, `<database>`), sinon `id`. Partagé par le rendu et la lecture.
pub(crate) fn anchor_key(spec: &BlockSpec) -> &'static str {
    if spec.props.iter().any(|p| p.mmd == "id") {
        "anchor"
    } else {
        "id"
    }
}

/// Attributs d'une balise : ancre `id`, `extra_first` (titre du toggle),
/// attributs portés par la forme (toujours écrits), autres props triées,
/// puis l'ancre `anchor` quand `id` est déjà pris.
fn tag_attrs(
    b: &Block,
    spec: &BlockSpec,
    anchor: Option<&str>,
    extra_first: &[(String, String)],
) -> Vec<(String, String)> {
    let key = anchor_key(spec);
    let mut attrs = Vec::new();
    if key == "id"
        && let Some(a) = anchor
    {
        attrs.push(("id".to_string(), quoted(a)));
    }
    attrs.extend_from_slice(extra_first);
    let text_prop = match spec.form {
        MmdForm::TextTag { prop, .. } => Some(prop),
        _ => None,
    };
    let mut carried = Vec::new();
    for p in spec.props.iter().filter(|p| p.in_form) {
        carried.push(p.name);
        if Some(p.name) == text_prop {
            continue;
        }
        if let Some(v) = b
            .props
            .get(p.name)
            .cloned()
            .or_else(|| default_value(p.default))
        {
            attrs.push((p.mmd.to_string(), format_value(&v, true)));
        }
    }
    attrs.extend(extra(&b.props, spec.props, &carried));
    if key == "anchor"
        && let Some(a) = anchor
    {
        attrs.push(("anchor".to_string(), quoted(a)));
    }
    attrs
}

impl Renderer {
    fn document(&mut self, blocks: &[Block]) -> String {
        let lines = self.seq(blocks);
        if lines.is_empty() {
            return String::new();
        }
        let mut s = lines.join("\n");
        s.push('\n');
        s
    }

    fn anchor(&mut self, b: &Block) -> Option<String> {
        match self.ids {
            Ids::None => None,
            Ids::Real => (!b.id.is_empty()).then(|| b.id.clone()),
            Ids::Aliases => {
                let alias = format!("b{}", self.aliases.len() + 1);
                self.aliases.insert(alias.clone(), b.id.clone());
                Some(alias)
            }
        }
    }

    /// Blocs frères : une ligne vide entre deux blocs, sauf entre éléments
    /// voisins d'une même famille.
    fn seq(&mut self, blocks: &[Block]) -> Vec<String> {
        let mut out = Vec::new();
        let mut prev_number: Option<i64> = None;
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                let f = family(&b.kind);
                if f.is_none() || f != family(&blocks[i - 1].kind) {
                    out.push(String::new());
                }
            }
            let number = (b.kind == "numberedListItem").then(|| match prev_number {
                Some(prev) => Number {
                    n: prev.saturating_add(1),
                    carries_start: false,
                },
                None => match b.props.get("start") {
                    Some(PropValue::Int(s)) if *s != 1 && (0..=MAX_START).contains(s) => Number {
                        n: *s,
                        carries_start: true,
                    },
                    _ => Number {
                        n: 1,
                        carries_start: false,
                    },
                },
            });
            prev_number = number.map(|n| n.n);
            out.extend(self.block(b, number));
        }
        out
    }

    fn children(&mut self, b: &Block, width: usize) -> impl Iterator<Item = String> {
        indent(self.seq(&b.children), width)
    }

    fn block(&mut self, b: &Block, number: Option<Number>) -> Vec<String> {
        let spec = block(&b.kind)
            .or_else(|| block("unknown"))
            .expect("unknown dans le registre");
        // `<block>` écrit toujours le vrai id et ne prend pas d'alias.
        let anchor = match spec.form {
            MmdForm::Unknown => None,
            _ => self.anchor(b),
        };
        let anchor = anchor.as_deref();
        match spec.form {
            MmdForm::Unknown => self.unknown(b, spec),
            MmdForm::Line if inline_of(b).is_empty() => self.empty_paragraph(b, spec, anchor),
            MmdForm::Line
            | MmdForm::Heading
            | MmdForm::Bullet
            | MmdForm::Numbered
            | MmdForm::Check
            | MmdForm::Quote => self.line_form(b, spec, number, anchor),
            MmdForm::Code => self.code(b, spec, anchor),
            MmdForm::Equation => self.equation(b, spec, anchor),
            MmdForm::Divider => {
                let first = assemble("---", "", &extra(&b.props, spec.props, &[]), None);
                self.fenced(b, vec![first], anchor)
            }
            MmdForm::Image => self.image(b, spec, anchor),
            MmdForm::Table => self.table(b, spec, anchor),
            MmdForm::Tag(name) | MmdForm::TitleTag(name) => self.container(b, spec, name, anchor),
            MmdForm::TextTag { tag, prop } => self.text_tag(b, spec, tag, prop, anchor),
            MmdForm::SelfClosing(name) => {
                let attrs = tag_attrs(b, spec, anchor, &[]);
                let mut out = vec![format!("<{name}{}/>", format_attrs(&attrs))];
                out.extend(self.children(b, 2));
                out
            }
        }
    }

    /// Paragraphe vide : `<p/>`, props en attributs.
    fn empty_paragraph(
        &mut self,
        b: &Block,
        spec: &BlockSpec,
        anchor: Option<&str>,
    ) -> Vec<String> {
        let attrs = tag_attrs(b, spec, anchor, &[]);
        let mut out = vec![format!("<p{}/>", format_attrs(&attrs))];
        out.extend(self.children(b, 2));
        out
    }

    fn line_form(
        &mut self,
        b: &Block,
        spec: &BlockSpec,
        number: Option<Number>,
        anchor: Option<&str>,
    ) -> Vec<String> {
        let mut carried: Vec<&str> = Vec::new();
        let mut width = 2;
        let head = match spec.form {
            MmdForm::Heading => match b.props.get("level") {
                Some(PropValue::Int(l)) if (1..=4).contains(l) => {
                    carried.push("level");
                    "#".repeat(*l as usize)
                }
                // Niveau absent (1), ou que `#` ne peut pas porter : liste `{…}`.
                _ => "#".into(),
            },
            MmdForm::Bullet => "-".into(),
            MmdForm::Check => match b.props.get("checked") {
                Some(PropValue::Bool(c)) => {
                    carried.push("checked");
                    if *c { "- [x]" } else { "- [ ]" }.into()
                }
                _ => "- [ ]".into(),
            },
            MmdForm::Numbered => {
                let number = number.unwrap_or(Number {
                    n: 1,
                    carries_start: false,
                });
                if number.carries_start {
                    carried.push("start");
                }
                let head = format!("{}.", number.n);
                width = head.len() + 1;
                head
            }
            MmdForm::Quote => ">".into(),
            _ => String::new(),
        };
        let list = extra(&b.props, spec.props, &carried);
        let first = assemble(&head, &render_inline(inline_of(b)), &list, anchor);
        let mut out = vec![first];
        out.extend(self.children(b, width));
        out
    }

    /// Bloc à clôture : ses lignes, l'ancre sur sa propre ligne, puis les
    /// enfants indentés de 2 espaces.
    fn fenced(&mut self, b: &Block, mut lines: Vec<String>, anchor: Option<&str>) -> Vec<String> {
        if let Some(a) = anchor {
            lines.push(format!("^{a}"));
        }
        lines.extend(self.children(b, 2));
        lines
    }

    fn code(&mut self, b: &Block, spec: &BlockSpec, anchor: Option<&str>) -> Vec<String> {
        let text: String = inline_of(b)
            .iter()
            .filter_map(|i| match i {
                Inline::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        // Clôture plus longue d'un cran que la plus longue suite de backticks.
        let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
        let fence = "`".repeat((longest + 1).max(3));
        let (lang, carried) = match str_prop(&b.props, "language") {
            Some(l) if language_fits(l) => (l, vec!["language"]),
            _ => ("", vec![]),
        };
        let list = extra(&b.props, spec.props, &carried);
        let mut lines = vec![assemble(&format!("{fence}{lang}"), "", &list, None)];
        if !text.is_empty() {
            lines.extend(text.split('\n').map(String::from));
        }
        lines.push(fence);
        self.fenced(b, lines, anchor)
    }

    fn equation(&mut self, b: &Block, spec: &BlockSpec, anchor: Option<&str>) -> Vec<String> {
        let latex = str_prop(&b.props, "latex").unwrap_or("");
        // Une ligne `$$` dans le LaTeX fermerait l'équation : liste `{…}`.
        let fits = !latex.split('\n').any(|l| l == "$$");
        let carried: &[&str] = if fits { &["latex"] } else { &[] };
        let list = extra(&b.props, spec.props, carried);
        let mut lines = vec![assemble("$$", "", &list, None)];
        lines.extend(if fits { latex } else { "" }.split('\n').map(String::from));
        lines.push("$$".into());
        self.fenced(b, lines, anchor)
    }

    fn image(&mut self, b: &Block, spec: &BlockSpec, anchor: Option<&str>) -> Vec<String> {
        let mut carried = vec!["caption"];
        let url = match str_prop(&b.props, "url") {
            Some(u) if href_safe(u) => {
                carried.push("url");
                u
            }
            _ => "",
        };
        let caption = str_prop(&b.props, "caption").unwrap_or("");
        let head = format!("![{}]({url})", escape_text(caption));
        let list = extra(&b.props, spec.props, &carried);
        let first = assemble(&head, "", &list, None);
        self.fenced(b, vec![first], anchor)
    }

    fn table(&mut self, b: &Block, spec: &BlockSpec, anchor: Option<&str>) -> Vec<String> {
        let mut lines = Vec::new();
        let mut list = extra(&b.props, spec.props, &[]);
        if let Content::Table(t) = &b.content {
            let ncols = t.rows.first().map_or(0, Vec::len);
            for (r, row) in t.rows.iter().enumerate() {
                let cells: Vec<String> = row
                    .iter()
                    .map(|c| {
                        let props = extra(&c.props, cell_props(), &[]);
                        assemble("", &render_inline(&c.content), &props, None)
                    })
                    .collect();
                lines.push(format!("| {} |", cells.join(" | ")));
                if r == 0 {
                    lines.push(format!("|{}", " --- |".repeat(ncols)));
                }
            }
            if t.header_rows != 1 {
                list.push(("header-rows".into(), t.header_rows.to_string()));
            }
            if t.header_cols != 0 {
                list.push(("header-cols".into(), t.header_cols.to_string()));
            }
            if t.column_widths != vec![None; ncols] {
                let widths: Vec<String> = t
                    .column_widths
                    .iter()
                    .map(|w| w.map(|w| w.to_string()).unwrap_or_default())
                    .collect();
                list.push(("widths".into(), quoted(&widths.join(","))));
            }
            list.sort();
        }
        if !list.is_empty() {
            lines.push(format_list(&list));
        }
        self.fenced(b, lines, anchor)
    }

    /// `<nom …>` enfants `</nom>`. Avec un contenu en ligne : dans l'attribut
    /// `title` (toggle), ou en premier paragraphe (callout, `<p/>` quand il
    /// est vide et que le premier enfant est un paragraphe).
    fn container(
        &mut self,
        b: &Block,
        spec: &BlockSpec,
        name: &str,
        anchor: Option<&str>,
    ) -> Vec<String> {
        let title = match spec.form {
            MmdForm::TitleTag(_) => {
                vec![("title".to_string(), quoted(&render_inline(inline_of(b))))]
            }
            _ => vec![],
        };
        let attrs = tag_attrs(b, spec, anchor, &title);
        let mut out = vec![format!("<{name}{}>", format_attrs(&attrs))];
        if matches!(spec.form, MmdForm::Tag(_)) && spec.content == ContentKind::Inline {
            let content = inline_of(b);
            if !content.is_empty() {
                out.push(render_inline(content));
            } else if b.children.first().is_some_and(|c| c.kind == "paragraph") {
                out.push("<p/>".into());
            }
        }
        out.extend(self.seq(&b.children));
        out.push(format!("</{name}>"));
        out
    }

    /// `<nom …>texte</nom>` sur une ligne, enfants indentés.
    fn text_tag(
        &mut self,
        b: &Block,
        spec: &BlockSpec,
        tag: &str,
        prop: &str,
        anchor: Option<&str>,
    ) -> Vec<String> {
        let attrs = tag_attrs(b, spec, anchor, &[]);
        let text = escape_plain(str_prop(&b.props, prop).unwrap_or(""));
        let mut out = vec![format!("<{tag}{}>{text}</{tag}>", format_attrs(&attrs))];
        out.extend(self.children(b, 2));
        out
    }

    /// `<block type="…" id="…" data="…"/>` : toujours le vrai id, pour qu'un
    /// bloc inconnu revienne intact. Un type hors registre s'écrit ainsi.
    fn unknown(&mut self, b: &Block, spec: &BlockSpec) -> Vec<String> {
        let original = match str_prop(&b.props, "originalType") {
            Some(t) => t,
            None if b.kind != "unknown" => b.kind.as_str(),
            None => "",
        };
        let mut attrs = vec![("type".to_string(), quoted(original))];
        if !b.id.is_empty() {
            attrs.push(("id".into(), quoted(&b.id)));
        }
        let data = str_prop(&b.props, "data").unwrap_or("");
        attrs.push(("data".into(), quoted(data)));
        attrs.extend(extra(&b.props, spec.props, &["originalType", "data"]));
        let mut out = vec![format!("<block{}/>", format_attrs(&attrs))];
        out.extend(self.children(b, 2));
        out
    }
}
