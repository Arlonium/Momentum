//! Lecture MMD → arbre de blocs.
//!
//! Le texte est lu ligne à ligne, piloté par la forme MMD de chaque type du
//! registre ; le contenu en ligne passe par pulldown-cmark. La lecture est
//! stricte : les erreurs de structure sont signalées en route (marqueur `*`,
//! titre souligné, balise non fermée…), puis l'arbre lu est réécrit et doit
//! redonner l'entrée octet pour octet. Sinon, la première ligne qui diffère
//! est signalée avec la forme à écrire.

use mementum_schema::{BlockSpec, ContentKind, MmdForm, PropSpec, block, blocks, cell_props};

use crate::attrs::{
    Attr, Raw, parse_list, parse_tag, split_cells, split_props, typed, unescape_attr,
};
use crate::inline::{parse_image, parse_inline};
use crate::model::{Block, Cell, Content, Inline, MmdError, PropValue, Props, Table, normalize};
use crate::render::{Family, family, render_ids};

/// Lit un MMD canonique. Un bloc sans ancre ni attribut `id` est lu avec
/// `id = ""`.
pub fn parse(mmd: &str) -> Result<Vec<Block>, MmdError> {
    let lines = split_lines(mmd)?;
    let (blocks, _) = read_blocks(&lines, None)?;
    let blocks = normalize(blocks);
    verify(mmd, &blocks)?;
    Ok(blocks)
}

#[derive(Clone, Copy)]
struct Line<'a> {
    /// Numéro dans le document, à partir de 1.
    no: usize,
    text: &'a str,
}

fn err(line: usize, message: impl Into<String>) -> MmdError {
    MmdError {
        line,
        message: message.into(),
    }
}

fn split_lines(mmd: &str) -> Result<Vec<Line<'_>>, MmdError> {
    if mmd.is_empty() {
        return Ok(Vec::new());
    }
    let body = mmd.strip_suffix('\n').ok_or_else(|| {
        err(
            mmd.split('\n').count(),
            "le texte doit finir par un saut de ligne (\\n)",
        )
    })?;
    Ok(body
        .split('\n')
        .enumerate()
        .map(|(i, text)| Line { no: i + 1, text })
        .collect())
}

/// Réécrit l'arbre et compare : la première ligne qui diffère est l'erreur.
fn verify(mmd: &str, blocks: &[Block]) -> Result<(), MmdError> {
    let expected = render_ids(blocks);
    if expected == mmd {
        return Ok(());
    }
    let got: Vec<&str> = mmd.split('\n').collect();
    let want: Vec<&str> = expected.split('\n').collect();
    let k = (0..)
        .find(|&k| got.get(k) != want.get(k))
        .expect("les textes diffèrent");
    let message = match (got.get(k), want.get(k)) {
        (Some(&""), Some(_)) => "ligne vide en trop".to_string(),
        (Some(_), Some(&"")) => "ligne vide attendue".to_string(),
        (Some(_), Some(w)) => format!("forme non canonique : écrire « {w} »"),
        (Some(_), None) => "texte en trop".to_string(),
        (None, _) => "texte incomplet".to_string(),
    };
    Err(err(k + 1, message))
}

// ------------------------------------------------------------- les formes

/// Forme reconnue en tête d'une ligne.
#[derive(Clone, Copy)]
enum Form<'a> {
    Paragraph,
    Heading {
        level: i64,
        rest: &'a str,
    },
    Bullet(&'a str),
    Check(bool, &'a str),
    Numbered {
        n: i64,
        width: usize,
        rest: &'a str,
    },
    Quote(&'a str),
    Code,
    Equation,
    Divider,
    Image,
    Table,
    /// Balise de bloc, `<p/>` compris.
    Tag(&'a str),
}

impl Form<'_> {
    fn family(&self) -> Option<Family> {
        match self {
            Form::Bullet(_) => family("bulletListItem"),
            Form::Check(..) => family("checkListItem"),
            Form::Numbered { .. } => family("numberedListItem"),
            Form::Tag(name) => tag_spec(name).and_then(|s| family(s.name)),
            _ => None,
        }
    }
}

/// Type de bloc écrit par la balise `name` (`None` pour `p`, le paragraphe vide).
fn tag_spec(name: &str) -> Option<&'static BlockSpec> {
    blocks().iter().find(|b| match b.form {
        MmdForm::Tag(n) | MmdForm::TitleTag(n) | MmdForm::SelfClosing(n) => n == name,
        MmdForm::TextTag { tag, .. } => tag == name,
        MmdForm::Unknown => name == "block",
        _ => false,
    })
}

fn is_block_tag(name: &str) -> bool {
    name == "p" || tag_spec(name).is_some()
}

/// Ce qui suit un marqueur : rien, ou une espace puis le reste.
fn after_marker(rest: &str) -> Option<&str> {
    if rest.is_empty() {
        Some("")
    } else {
        rest.strip_prefix(' ')
    }
}

fn is_anchor_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn classify<'a>(line: &Line<'a>) -> Result<Form<'a>, MmdError> {
    let t = line.text;
    let fail = |m: String| Err(err(line.no, m));
    if t.ends_with('\r') {
        return fail("retour chariot en fin de ligne : fins de ligne LF (\\n) attendues".into());
    }
    if t.starts_with("    ") {
        return fail("code indenté : écrire le code entre deux clôtures « ``` »".into());
    }
    if t.starts_with([' ', '\t']) {
        return fail(
            "indentation inattendue : un enfant suit son parent sans ligne vide, indenté de \
             2 espaces (de la largeur du marqueur sous « N. »)"
                .into(),
        );
    }
    if t.starts_with("```") {
        return Ok(Form::Code);
    }
    if t.starts_with("~~~") {
        return fail("clôture « ~~~ » : écrire « ``` »".into());
    }
    if t == "$$" || t.starts_with("$$ ") {
        return Ok(Form::Equation);
    }
    if t == "---" || t.starts_with("--- ") {
        return Ok(Form::Divider);
    }
    if t == "***" || t == "___" {
        return fail("séparateur : écrire « --- »".into());
    }
    let hashes = t.bytes().take_while(|&b| b == b'#').count();
    if hashes > 0
        && let Some(rest) = after_marker(&t[hashes..])
    {
        if hashes > 4 {
            return fail(format!(
                "titre de niveau {hashes} : les titres vont des niveaux 1 à 4 (« # » à « #### »)"
            ));
        }
        return Ok(Form::Heading {
            level: hashes as i64,
            rest,
        });
    }
    for (marker, checked) in [("- [ ]", false), ("- [x]", true)] {
        if let Some(rest) = t.strip_prefix(marker).and_then(after_marker) {
            return Ok(Form::Check(checked, rest));
        }
    }
    if let Some(rest) = t.strip_prefix('-').and_then(after_marker) {
        return Ok(Form::Bullet(rest));
    }
    for marker in ['*', '+'] {
        if t.strip_prefix(marker).and_then(after_marker).is_some() {
            return fail(format!("marqueur de liste « {marker} » : écrire « - »"));
        }
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        if let Some(rest) = t[digits..].strip_prefix('.').and_then(after_marker) {
            let n = t[..digits]
                .parse()
                .map_err(|_| err(line.no, "numéro de liste trop grand"))?;
            return Ok(Form::Numbered {
                n,
                width: digits + 2,
                rest,
            });
        }
        if t[digits..]
            .strip_prefix(')')
            .and_then(after_marker)
            .is_some()
        {
            return fail(format!(
                "liste « {}) » : écrire « {}. »",
                &t[..digits],
                &t[..digits]
            ));
        }
    }
    if let Some(rest) = t.strip_prefix('>').and_then(after_marker) {
        return Ok(Form::Quote(rest));
    }
    if t.starts_with('|') {
        return Ok(Form::Table);
    }
    if t.starts_with("![") {
        return Ok(Form::Image);
    }
    if let Some(rest) = t.strip_prefix('<') {
        let closing = rest.starts_with('/');
        let name_part = rest.trim_start_matches('/');
        let nlen = name_part
            .bytes()
            .take_while(u8::is_ascii_alphanumeric)
            .count();
        let name = &name_part[..nlen];
        let ends_name = name_part[nlen..].starts_with([' ', '>', '/']);
        if is_block_tag(name) && ends_name {
            if closing {
                return fail(format!("balise fermante « {t} » sans balise ouvrante"));
            }
            return Ok(Form::Tag(name));
        }
    }
    if let Some(id) = t.strip_prefix('^')
        && is_anchor_id(id)
    {
        return fail(format!(
            "ancre « {t} » seule : elle suit un bloc de code, une équation, un séparateur, \
             une image ou un tableau ; ailleurs, elle termine la ligne du bloc"
        ));
    }
    if t.starts_with('{') {
        return fail(
            "liste {…} seule : elle suit un tableau, ou termine la ligne d'un bloc".into(),
        );
    }
    Ok(Form::Paragraph)
}

/// Titre souligné à la mode setext (`===` ou `---` sous un paragraphe).
fn is_setext(t: &str) -> bool {
    let t = t.trim_end();
    !t.is_empty() && (t.bytes().all(|b| b == b'=') || t.bytes().all(|b| b == b'-'))
}

// ---------------------------------------------------------- les séquences

/// Ce qu'il faut savoir du bloc précédent pour lire le suivant.
struct Prev {
    family: Option<Family>,
    /// Numéro affiché, pour un élément de liste numérotée.
    number: Option<i64>,
    /// Le bloc était un paragraphe sans enfant (sa ligne précède).
    paragraph_line: bool,
}

/// Lit des blocs frères jusqu'à la ligne `term` (exclue) ou la fin. Renvoie
/// les blocs et l'indice où la lecture s'est arrêtée.
fn read_blocks(lines: &[Line<'_>], term: Option<&str>) -> Result<(Vec<Block>, usize), MmdError> {
    let mut out = Vec::new();
    let mut prev: Option<Prev> = None;
    let mut blanks = 0;
    let mut first_blank = 0;
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if term == Some(line.text) {
            break;
        }
        if line.text.is_empty() {
            if blanks == 0 {
                first_blank = line.no;
            }
            blanks += 1;
            i += 1;
            continue;
        }
        let form = classify(line)?;
        let fam = form.family();
        match &prev {
            None if blanks > 0 => return Err(err(first_blank, "ligne vide en trop")),
            None => {}
            Some(p) => {
                let tight = fam.is_some() && fam == p.family;
                if tight && blanks > 0 {
                    return Err(err(
                        first_blank,
                        "ligne vide en trop : les éléments d'une même liste se suivent",
                    ));
                }
                if !tight && blanks == 0 {
                    if p.paragraph_line && is_setext(line.text) {
                        return Err(err(
                            line.no,
                            "titre souligné (setext) : écrire « # Titre » ou « ## Titre »",
                        ));
                    }
                    return Err(err(line.no, "ligne vide attendue avant ce bloc"));
                }
                if blanks > 1 {
                    return Err(err(
                        first_blank + 1,
                        "une seule ligne vide entre deux blocs",
                    ));
                }
            }
        }
        blanks = 0;
        let prev_number = prev.as_ref().and_then(|p| p.number);
        let (b, next, number) = read_block(lines, i, form, prev_number)?;
        prev = Some(Prev {
            family: family(&b.kind),
            number,
            paragraph_line: matches!(form, Form::Paragraph) && b.children.is_empty(),
        });
        out.push(b);
        i = next;
    }
    if blanks > 0 {
        return Err(err(first_blank, "ligne vide en trop"));
    }
    Ok((out, i))
}

fn indent_of(t: &str) -> usize {
    t.bytes().take_while(|&b| b == b' ').count()
}

/// Enfants d'un bloc à forme en ligne, à clôture ou autofermante : les lignes
/// qui suivent, indentées d'au moins `width` espaces.
fn read_children(
    lines: &[Line<'_>],
    next: usize,
    width: usize,
) -> Result<(Vec<Block>, usize), MmdError> {
    match lines.get(next) {
        Some(l) if !l.text.is_empty() && indent_of(l.text) >= width => {}
        _ => return Ok((Vec::new(), next)),
    }
    let mut region = Vec::new();
    let mut j = next;
    while j < lines.len() {
        let l = lines[j];
        if l.text.is_empty() {
            // Des lignes vides en font partie si la suite est encore indentée.
            let end = lines[j..]
                .iter()
                .position(|l| !l.text.is_empty())
                .map_or(lines.len(), |p| j + p);
            if !lines.get(end).is_some_and(|l| indent_of(l.text) >= width) {
                break;
            }
            region.extend_from_slice(&lines[j..end]);
            j = end;
            continue;
        } else if indent_of(l.text) >= width {
            region.push(Line {
                no: l.no,
                text: &l.text[width..],
            });
        } else {
            break;
        }
        j += 1;
    }
    let (children, _) = read_blocks(&region, None)?;
    Ok((children, j))
}

/// Ancre `^id` seule sur la ligne `next`, après un bloc à clôture.
fn own_anchor<'a>(lines: &[Line<'a>], next: usize) -> (&'a str, usize) {
    match lines.get(next).and_then(|l| l.text.strip_prefix('^')) {
        Some(id) if is_anchor_id(id) => (id, next + 1),
        _ => ("", next),
    }
}

/// Sépare l'ancre finale ` ^id` (ou une ligne réduite à `^id`).
fn split_anchor(s: &str) -> (&str, &str) {
    let Some(pos) = s.rfind('^') else {
        return (s, "");
    };
    let id = &s[pos + 1..];
    if !is_anchor_id(id) {
        return (s, "");
    }
    match pos {
        0 => ("", id),
        _ if s.as_bytes()[pos - 1] == b' ' => (&s[..pos - 1], id),
        _ => (s, ""),
    }
}

/// Range une liste `{…}` ou des attributs dans les props : nom MMD d'une prop
/// du registre, sinon nom BlockNote d'une prop inconnue (typée par son écriture).
fn apply(
    props: &mut Props,
    specs: &[PropSpec],
    items: Vec<Attr>,
    line: usize,
) -> Result<(), MmdError> {
    for (key, raw) in items {
        let spec = specs.iter().find(|p| p.mmd == key);
        let value = typed(&key, raw, spec.map(|p| p.ty)).map_err(|m| err(line, m))?;
        props.insert(spec.map_or(key, |p| p.name.to_string()), value);
    }
    Ok(())
}

fn props_list(src: &str, line: usize) -> Result<Vec<Attr>, MmdError> {
    parse_list(src).map_err(|e| err(line, format!("liste {{…}} mal formée : {e}")))
}

fn new_block(kind: &str, props: Props, content: Content, children: Vec<Block>, id: &str) -> Block {
    Block {
        id: id.into(),
        kind: kind.into(),
        props,
        content,
        children,
    }
}

/// Lit le bloc qui commence à la ligne `i` : le bloc, l'indice de la ligne
/// qui le suit et, pour un élément numéroté, son numéro.
fn read_block(
    lines: &[Line<'_>],
    i: usize,
    form: Form<'_>,
    prev_number: Option<i64>,
) -> Result<(Block, usize, Option<i64>), MmdError> {
    let line = lines[i];
    let (kind, rest, width, number) = match form {
        Form::Paragraph => ("paragraph", line.text, 2, None),
        Form::Heading { rest, .. } => ("heading", rest, 2, None),
        Form::Bullet(rest) => ("bulletListItem", rest, 2, None),
        Form::Check(_, rest) => ("checkListItem", rest, 2, None),
        Form::Numbered { n, width, rest } => ("numberedListItem", rest, width, Some(n)),
        Form::Quote(rest) => ("quote", rest, 2, None),
        Form::Code => return read_code(lines, i).map(|(b, next)| (b, next, None)),
        Form::Equation => return read_equation(lines, i).map(|(b, next)| (b, next, None)),
        Form::Divider => return read_divider(lines, i).map(|(b, next)| (b, next, None)),
        Form::Image => return read_image(lines, i).map(|(b, next)| (b, next, None)),
        Form::Table => return read_table(lines, i).map(|(b, next)| (b, next, None)),
        Form::Tag(name) => return read_tag(lines, i, name).map(|(b, next)| (b, next, None)),
    };
    let spec = block(kind).expect("type du registre");
    let mut props = Props::new();
    match form {
        Form::Heading { level, .. } if level != 1 => {
            props.insert("level".into(), PropValue::Int(level));
        }
        Form::Check(true, _) => {
            props.insert("checked".into(), PropValue::Bool(true));
        }
        Form::Numbered { n, .. } => match prev_number {
            None if n != 1 => {
                props.insert("start".into(), PropValue::Int(n));
            }
            Some(prev) if n != prev.saturating_add(1) => {
                return Err(err(
                    line.no,
                    format!("numérotation : écrire « {}. »", prev.saturating_add(1)),
                ));
            }
            _ => {}
        },
        _ => {}
    }
    let (rest, id) = split_anchor(rest);
    let (src, list) = split_props(rest).map_err(|m| err(line.no, m))?;
    if let Some(list) = list {
        apply(&mut props, spec.props, list, line.no)?;
    }
    let content = Content::Inline(parse_inline(src));
    let (children, next) = read_children(lines, i + 1, width)?;
    Ok((new_block(kind, props, content, children, id), next, number))
}

/// Fin de ligne optionnelle ` {…}` après un marqueur de bloc à clôture.
fn fence_list(
    rest: &str,
    props: &mut Props,
    spec: &BlockSpec,
    line: usize,
) -> Result<(), MmdError> {
    match rest.strip_prefix(' ') {
        Some(src) => apply(props, spec.props, props_list(src, line)?, line),
        None if rest.is_empty() => Ok(()),
        None => Err(err(line, format!("« {rest} » inattendu"))),
    }
}

fn read_code(lines: &[Line<'_>], i: usize) -> Result<(Block, usize), MmdError> {
    let line = lines[i];
    let spec = block("codeBlock").expect("codeBlock");
    let n = line.text.bytes().take_while(|&b| b == b'`').count();
    let fence = &line.text[..n];
    let info = &line.text[n..];
    let (lang, rest) = match info.find(' ') {
        Some(p) => (&info[..p], &info[p..]),
        None => (info, ""),
    };
    let mut props = Props::new();
    if !lang.is_empty() {
        props.insert("language".into(), PropValue::Str(lang.into()));
    }
    fence_list(rest, &mut props, spec, line.no)?;
    let close = lines[i + 1..]
        .iter()
        .position(|l| l.text == fence)
        .map(|p| i + 1 + p)
        .ok_or_else(|| {
            err(
                line.no,
                format!("bloc de code non fermé : ajouter « {fence} »"),
            )
        })?;
    let text = lines[i + 1..close]
        .iter()
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n");
    let content = if text.is_empty() {
        vec![]
    } else {
        vec![Inline::Text {
            text,
            styles: Default::default(),
        }]
    };
    let (id, next) = own_anchor(lines, close + 1);
    let (children, next) = read_children(lines, next, 2)?;
    Ok((
        new_block("codeBlock", props, Content::Inline(content), children, id),
        next,
    ))
}

fn read_equation(lines: &[Line<'_>], i: usize) -> Result<(Block, usize), MmdError> {
    let line = lines[i];
    let spec = block("equation").expect("equation");
    let close = lines[i + 1..]
        .iter()
        .position(|l| l.text == "$$")
        .map(|p| i + 1 + p)
        .ok_or_else(|| err(line.no, "équation non fermée : ajouter « $$ »"))?;
    let latex = lines[i + 1..close]
        .iter()
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n");
    let mut props = Props::new();
    if !latex.is_empty() {
        props.insert("latex".into(), PropValue::Str(latex));
    }
    fence_list(&line.text[2..], &mut props, spec, line.no)?;
    let (id, next) = own_anchor(lines, close + 1);
    let (children, next) = read_children(lines, next, 2)?;
    Ok((
        new_block("equation", props, Content::None, children, id),
        next,
    ))
}

fn read_divider(lines: &[Line<'_>], i: usize) -> Result<(Block, usize), MmdError> {
    let line = lines[i];
    let spec = block("divider").expect("divider");
    let mut props = Props::new();
    fence_list(&line.text[3..], &mut props, spec, line.no)?;
    let (id, next) = own_anchor(lines, i + 1);
    let (children, next) = read_children(lines, next, 2)?;
    Ok((
        new_block("divider", props, Content::None, children, id),
        next,
    ))
}

fn read_image(lines: &[Line<'_>], i: usize) -> Result<(Block, usize), MmdError> {
    let line = lines[i];
    let spec = block("image").expect("image");
    let (src, list) = split_props(line.text).map_err(|m| err(line.no, m))?;
    let (url, caption) = parse_image(src)
        .ok_or_else(|| err(line.no, "image mal formée : écrire « ![légende](adresse) »"))?;
    let mut props = Props::new();
    for (name, value) in [("url", url), ("caption", caption)] {
        if !value.is_empty() {
            props.insert(name.into(), PropValue::Str(value));
        }
    }
    if let Some(list) = list {
        apply(&mut props, spec.props, list, line.no)?;
    }
    let (id, next) = own_anchor(lines, i + 1);
    let (children, next) = read_children(lines, next, 2)?;
    Ok((new_block("image", props, Content::None, children, id), next))
}

fn read_cell(seg: &str, line: usize) -> Result<Cell, MmdError> {
    let inner = seg
        .strip_prefix(' ')
        .and_then(|s| s.strip_suffix(' '))
        .ok_or_else(|| err(line, "cellule : une espace de chaque côté, « | texte | »"))?;
    let (src, list) = split_props(inner).map_err(|m| err(line, m))?;
    let mut props = Props::new();
    if let Some(list) = list {
        apply(&mut props, cell_props(), list, line)?;
    }
    Ok(Cell {
        content: parse_inline(src),
        props,
    })
}

fn read_row(line: &Line<'_>) -> Result<Vec<Cell>, MmdError> {
    split_cells(line.text)
        .ok_or_else(|| {
            err(
                line.no,
                "ligne de tableau : écrire « | cellule | cellule | »",
            )
        })?
        .into_iter()
        .map(|seg| read_cell(seg, line.no))
        .collect()
}

fn read_table(lines: &[Line<'_>], i: usize) -> Result<(Block, usize), MmdError> {
    let spec = block("table").expect("table");
    let end = lines[i..]
        .iter()
        .position(|l| !l.text.starts_with('|'))
        .map_or(lines.len(), |p| i + p);
    let header = read_row(&lines[i])?;
    let ncols = header.len();
    let separator = format!("|{}", " --- |".repeat(ncols));
    match lines.get(i + 1) {
        Some(l) if i + 1 < end && l.text == separator => {}
        Some(l) if i + 1 < end => {
            return Err(err(
                l.no,
                format!("ligne de séparation : écrire « {separator} »"),
            ));
        }
        _ => {
            return Err(err(
                lines[i].no,
                format!("tableau : la ligne « {separator} » suit la première ligne"),
            ));
        }
    }
    let mut rows = vec![header];
    for l in &lines[i + 2..end] {
        rows.push(read_row(l)?);
    }
    let mut table = Table {
        header_rows: 1,
        column_widths: vec![None; ncols],
        rows,
    };
    let mut props = Props::new();
    let mut next = end;
    if let Some(l) = lines.get(end).filter(|l| l.text.starts_with('{')) {
        let mut items = Vec::new();
        for (key, raw) in props_list(l.text, l.no)? {
            match (key.as_str(), raw) {
                ("header-rows", Raw::Number(n)) => {
                    table.header_rows = n
                        .parse()
                        .map_err(|_| err(l.no, "« header-rows » attend un entier positif"))?;
                }
                ("widths", Raw::Str(w)) => table.column_widths = parse_widths(&w, l.no)?,
                ("header-rows" | "widths", _) => {
                    return Err(err(
                        l.no,
                        "« header-rows » attend un entier, « widths » une chaîne (« \"120,,80\" »)",
                    ));
                }
                (_, raw) => items.push((key, raw)),
            }
        }
        apply(&mut props, spec.props, items, l.no)?;
        next += 1;
    }
    let (id, next) = own_anchor(lines, next);
    let (children, next) = read_children(lines, next, 2)?;
    Ok((
        new_block("table", props, Content::Table(table), children, id),
        next,
    ))
}

/// `"120,,80"` : une largeur par colonne, vide = automatique.
fn parse_widths(w: &str, line: usize) -> Result<Vec<Option<f64>>, MmdError> {
    w.split(',')
        .map(|part| {
            if part.is_empty() {
                return Ok(None);
            }
            match typed("widths", Raw::Number(part.into()), None) {
                Ok(PropValue::Float(f)) => Ok(Some(f)),
                Ok(PropValue::Int(n)) => Ok(Some(n as f64)),
                _ => Err(err(line, format!("largeur « {part} » : un nombre attendu"))),
            }
        })
        .collect()
}

/// Range les attributs d'une balise : ancre (`id`, ou `anchor` si une prop
/// s'écrit `id`), titre d'un toggle, props.
fn tag_props(
    spec: &BlockSpec,
    attrs: Vec<Attr>,
    line: usize,
    with_title: bool,
) -> Result<(String, Props, String), MmdError> {
    let anchor_key = if spec.props.iter().any(|p| p.mmd == "id") {
        "anchor"
    } else {
        "id"
    };
    let mut id = String::new();
    let mut title = String::new();
    let mut items = Vec::new();
    for (key, raw) in attrs {
        let is_id = key == anchor_key;
        let is_title = with_title && key == "title";
        match raw {
            Raw::Str(s) if is_id => id = s,
            Raw::Str(s) if is_title => title = s,
            _ if is_id || is_title => {
                return Err(err(
                    line,
                    format!("« {key} » attend une chaîne entre guillemets"),
                ));
            }
            raw => items.push((key, raw)),
        }
    }
    let mut props = Props::new();
    apply(&mut props, spec.props, items, line)?;
    Ok((id, props, title))
}

fn read_tag(lines: &[Line<'_>], i: usize, name: &str) -> Result<(Block, usize), MmdError> {
    let line = lines[i];
    let tag = parse_tag(line.text).map_err(|m| err(line.no, m))?;
    let alone = tag.rest.is_empty();
    let Some(spec) = tag_spec(name) else {
        // `<p/>` : paragraphe vide.
        if !(tag.self_closing && alone) {
            return Err(err(
                line.no,
                "paragraphe vide : écrire « <p/> » seul sur sa ligne",
            ));
        }
        let spec = block("paragraph").expect("paragraph");
        let (id, props, _) = tag_props(spec, tag.attrs, line.no, false)?;
        let (children, next) = read_children(lines, i + 1, 2)?;
        let content = Content::Inline(vec![]);
        return Ok((new_block("paragraph", props, content, children, &id), next));
    };
    match spec.form {
        MmdForm::Tag(n) | MmdForm::TitleTag(n) => {
            if tag.self_closing || !alone {
                return Err(err(
                    line.no,
                    format!("balise « <{n}> » : seule sur sa ligne, fermée par « </{n}> »"),
                ));
            }
            let titled = matches!(spec.form, MmdForm::TitleTag(_));
            let (id, props, title) = tag_props(spec, tag.attrs, line.no, titled)?;
            let term = format!("</{n}>");
            let inner = &lines[i + 1..];
            // Le titre d'un toggle est du MMD en ligne, déjà désechappé.
            let (content, skip) = if titled {
                (parse_inline(&title), 0)
            } else if spec.content == ContentKind::Inline {
                callout_content(inner, &term)?
            } else {
                (vec![], 0)
            };
            let (children, stop) = read_blocks(&inner[skip..], Some(&term))?;
            if skip + stop >= inner.len() {
                return Err(err(
                    line.no,
                    format!("balise « <{n}> » non fermée : ajouter « {term} »"),
                ));
            }
            let content = match spec.content {
                ContentKind::Inline => Content::Inline(content),
                _ => Content::None,
            };
            let next = i + 1 + skip + stop + 1;
            Ok((new_block(spec.name, props, content, children, &id), next))
        }
        MmdForm::TextTag { tag: n, prop } => {
            let close = format!("</{n}>");
            let text = tag
                .rest
                .strip_suffix(close.as_str())
                .filter(|_| !tag.self_closing)
                .ok_or_else(|| err(line.no, format!("« <{n} …>texte{close} » sur une ligne")))?;
            let (id, mut props, _) = tag_props(spec, tag.attrs, line.no, false)?;
            if !text.is_empty() {
                props.insert(prop.into(), PropValue::Str(unescape_attr(text)));
            }
            let (children, next) = read_children(lines, i + 1, 2)?;
            Ok((
                new_block(spec.name, props, Content::None, children, &id),
                next,
            ))
        }
        MmdForm::SelfClosing(_) | MmdForm::Unknown => {
            if !(tag.self_closing && alone) {
                return Err(err(
                    line.no,
                    format!("balise « <{name} …/> » : autofermante, seule sur sa ligne"),
                ));
            }
            let (id, props, _) = tag_props(spec, tag.attrs, line.no, false)?;
            let (children, next) = read_children(lines, i + 1, 2)?;
            Ok((
                new_block(spec.name, props, Content::None, children, &id),
                next,
            ))
        }
        _ => unreachable!("tag_spec ne renvoie que des balises"),
    }
}

/// Contenu d'un callout : sa première ligne si c'est un paragraphe, rien si
/// c'est `<p/>` (callout vide suivi d'un paragraphe). Renvoie le contenu et
/// le nombre de lignes lues.
fn callout_content(inner: &[Line<'_>], term: &str) -> Result<(Vec<Inline>, usize), MmdError> {
    let Some(first) = inner.first() else {
        return Ok((vec![], 0));
    };
    if first.text == "<p/>" {
        return Ok((vec![], 1));
    }
    if first.text.is_empty() || first.text == term || !matches!(classify(first)?, Form::Paragraph) {
        return Ok((vec![], 0));
    }
    let (rest, id) = split_anchor(first.text);
    let (src, list) = split_props(rest).map_err(|m| err(first.no, m))?;
    if !id.is_empty() || list.is_some() {
        return Err(err(
            first.no,
            "contenu d'un callout : ni liste {…} ni ancre (elles vont sur la balise)",
        ));
    }
    Ok((parse_inline(src), 1))
}
