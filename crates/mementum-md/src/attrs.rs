//! Listes de props `{…}` et attributs de balises : écriture, lecture, et
//! repérage d'une liste ou des cellules dans une ligne.

use std::collections::HashSet;

use mementum_schema::PropType;

use crate::model::PropValue;

/// Valeur lue dans une liste ou un attribut, avant d'être typée.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Raw {
    Str(String),
    Number(String),
    Bool(bool),
}

/// Une paire `clé=valeur`.
pub(crate) type Attr = (String, Raw);

// ---------------------------------------------------------------- écriture

/// Échappe une valeur entre guillemets (attribut ou chaîne d'une liste).
pub(crate) fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

/// Inverse de [`escape_attr`] (et de l'échappement du texte brut d'un `<page>`).
pub(crate) fn unescape_attr(s: &str) -> String {
    const ENTITIES: [(&str, char); 5] = [
        ("&amp;", '&'),
        ("&quot;", '"'),
        ("&lt;", '<'),
        ("&#10;", '\n'),
        ("&#13;", '\r'),
    ];
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let (c, len) = ENTITIES
            .iter()
            .find(|(e, _)| rest.starts_with(e))
            .map_or(('&', 1), |(e, c)| (*c, e.len()));
        out.push(c);
        rest = &rest[len..];
    }
    out.push_str(rest);
    out
}

/// Une chaîne entre guillemets.
pub(crate) fn quoted(s: &str) -> String {
    format!("\"{}\"", escape_attr(s))
}

/// Écrit une valeur : chaîne entre guillemets, nombre et booléen nus. Un
/// flottant d'une prop connue du registre s'écrit sans « .0 » (`width=320`) ;
/// celui d'une prop inconnue garde son point, pour être relu en flottant.
pub(crate) fn format_value(v: &PropValue, known: bool) -> String {
    match v {
        PropValue::Str(s) => quoted(s),
        PropValue::Int(i) => i.to_string(),
        PropValue::Float(f) => {
            let s = f.to_string();
            if !known && f.is_finite() && !s.contains('.') {
                format!("{s}.0")
            } else {
                s
            }
        }
        PropValue::Bool(b) => b.to_string(),
    }
}

/// `{clé=valeur clé=valeur}`, valeurs déjà écrites.
pub(crate) fn format_list(items: &[(String, String)]) -> String {
    let inner: Vec<String> = items.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{{{}}}", inner.join(" "))
}

/// ` clé=valeur` répétés, pour une balise.
pub(crate) fn format_attrs(items: &[(String, String)]) -> String {
    items.iter().map(|(k, v)| format!(" {k}={v}")).collect()
}

// ----------------------------------------------------------------- lecture

/// Type une valeur lue : d'après le registre pour une prop connue, d'après
/// son écriture sinon (guillemets : chaîne ; point : flottant ; nombre :
/// entier ; `true`/`false` : booléen).
pub(crate) fn typed(key: &str, raw: Raw, ty: Option<PropType>) -> Result<PropValue, String> {
    let expected = |ty: PropType| match ty {
        PropType::String => "une chaîne entre guillemets",
        PropType::Int => "un entier",
        PropType::Float => "un nombre",
        PropType::Bool => "true ou false",
    };
    match (raw, ty) {
        (Raw::Str(s), None | Some(PropType::String)) => Ok(PropValue::Str(s)),
        (Raw::Bool(b), None | Some(PropType::Bool)) => Ok(PropValue::Bool(b)),
        (Raw::Number(n), Some(PropType::Float)) => n
            .parse()
            .map(PropValue::Float)
            .map_err(|_| format!("« {key} » attend un nombre")),
        (Raw::Number(n), None) if n.contains('.') => n
            .parse()
            .map(PropValue::Float)
            .map_err(|_| format!("« {key} » : nombre illisible")),
        (Raw::Number(n), None | Some(PropType::Int)) => n
            .parse()
            .map(PropValue::Int)
            .map_err(|_| format!("« {key} » attend un entier")),
        (_, Some(ty)) => Err(format!("« {key} » attend {}", expected(ty))),
    }
}

fn is_number(t: &str) -> bool {
    let t = t.strip_prefix('-').unwrap_or(t);
    let (int, frac) = match t.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (t, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    digits(int) && (int == "0" || !int.starts_with('0')) && frac.is_none_or(digits)
}

/// Lit `clé=valeur` en tête de `s` et renvoie la suite.
fn parse_pair(s: &str) -> Result<(Attr, &str), String> {
    let klen = s
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        .count();
    if klen == 0 || !s.as_bytes()[0].is_ascii_alphabetic() {
        return Err("nom de prop attendu (lettres, chiffres, « - » ou « _ »)".into());
    }
    let key = &s[..klen];
    let rest = s[klen..]
        .strip_prefix('=')
        .ok_or_else(|| format!("« = » attendu après « {key} »"))?;
    if let Some(r) = rest.strip_prefix('"') {
        let end = r
            .find('"')
            .ok_or_else(|| format!("guillemet fermant attendu pour « {key} »"))?;
        return Ok((
            (key.into(), Raw::Str(unescape_attr(&r[..end]))),
            &r[end + 1..],
        ));
    }
    let vlen = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'.')
        .count();
    let raw = match &rest[..vlen] {
        "true" => Raw::Bool(true),
        "false" => Raw::Bool(false),
        "" => return Err(format!("valeur attendue pour « {key} »")),
        t if is_number(t) => Raw::Number(t.into()),
        t => {
            return Err(format!(
                "valeur « {t} » de « {key} » : une chaîne s'écrit entre guillemets (« \"{t}\" »)"
            ));
        }
    };
    Ok(((key.into(), raw), &rest[vlen..]))
}

fn check_unique(items: &[Attr]) -> Result<(), String> {
    let mut seen = HashSet::with_capacity(items.len());
    match items.iter().find(|(k, _)| !seen.insert(k.as_str())) {
        Some((k, _)) => Err(format!("« {k} » écrit deux fois")),
        None => Ok(()),
    }
}

/// Lit une liste `{clé=valeur …}` qui occupe tout `s`.
pub(crate) fn parse_list(s: &str) -> Result<Vec<Attr>, String> {
    let mut rest = s.strip_prefix('{').ok_or("« { » attendu")?;
    let mut items = Vec::new();
    loop {
        let (item, r) = parse_pair(rest)?;
        items.push(item);
        if let Some(r) = r.strip_prefix(' ') {
            rest = r;
        } else if r == "}" {
            break;
        } else if r.is_empty() {
            return Err("« } » manquante".into());
        } else {
            return Err(format!("« {r} » inattendu"));
        }
    }
    check_unique(&items)?;
    Ok(items)
}

/// Une balise ouvrante lue en tête d'une chaîne.
pub(crate) struct Tag<'a> {
    pub name: &'a str,
    pub attrs: Vec<Attr>,
    pub self_closing: bool,
    /// Ce qui suit `>` ou `/>`.
    pub rest: &'a str,
}

/// Lit `<nom clé=valeur …>` ou `<nom …/>` en tête de `s`.
pub(crate) fn parse_tag(s: &str) -> Result<Tag<'_>, String> {
    let body = s.strip_prefix('<').ok_or("« < » attendu")?;
    let nlen = body.bytes().take_while(u8::is_ascii_alphanumeric).count();
    let name = &body[..nlen];
    let mut rest = &body[nlen..];
    let mut attrs = Vec::new();
    loop {
        let (self_closing, after) = if let Some(r) = rest.strip_prefix("/>") {
            (true, r)
        } else if let Some(r) = rest.strip_prefix('>') {
            (false, r)
        } else {
            let r = rest
                .strip_prefix(' ')
                .ok_or_else(|| format!("balise « <{name}> » mal formée près de « {rest} »"))?;
            let (item, r) = parse_pair(r)?;
            attrs.push(item);
            rest = r;
            continue;
        };
        check_unique(&attrs)?;
        return Ok(Tag {
            name,
            attrs,
            self_closing,
            rest: after,
        });
    }
}

// --------------------------------------------------------------- repérage

/// Position, dans `b` à partir de `from`, du premier `target` hors guillemets.
fn find_unquoted(b: &[u8], from: usize, target: u8) -> Option<usize> {
    let mut quoted = false;
    for (i, &c) in b.iter().enumerate().skip(from) {
        match c {
            b'"' => quoted = !quoted,
            c if c == target && !quoted => return Some(i),
            _ => {}
        }
    }
    None
}

/// Fin (position qui suit) de la prochaine suite d'exactement `run` backticks.
fn find_run(b: &[u8], from: usize, run: usize) -> Option<usize> {
    let mut i = from;
    while i < b.len() {
        if b[i] == b'`' {
            let n = b[i..].iter().take_while(|&&c| c == b'`').count();
            if n == run {
                return Some(i + n);
            }
            i += n;
        } else {
            i += 1;
        }
    }
    None
}

/// Positions des `{` (débuts de liste, liste sautée) et des `|` de premier
/// niveau : hors échappements, code, maths, balises et destinations de liens.
/// Dans le MMD canonique, ces caractères y sont toujours structurels, et
/// code, balises et destinations sont toujours fermés : une ouverture sans
/// fermeture (entrée non canonique, refusée plus loin) clôt l'examen de la
/// ligne, ce qui garde le parcours linéaire.
fn marks(s: &str) -> Vec<(usize, u8)> {
    let b = s.as_bytes();
    let n = b.len();
    let next_is = |i: usize, f: fn(u8) -> bool| i + 1 < n && f(b[i + 1]);
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        i = match b[i] {
            b'\\' => i + 2,
            b'`' => {
                let run = b[i..].iter().take_while(|&&c| c == b'`').count();
                find_run(b, i + run, run).unwrap_or(n)
            }
            b'<' if next_is(i, |c| c.is_ascii_alphabetic() || c == b'/') => {
                find_unquoted(b, i + 1, b'>').map_or(n, |e| e + 1)
            }
            // Formule : jusqu'au `$` suivant non échappé (`\$` y reste un
            // caractère, comme pour pulldown-cmark).
            b'$' => {
                let mut j = i + 1;
                loop {
                    match b.get(j) {
                        None => break i + 1,
                        Some(b'\\') => j += 2,
                        Some(b'$') => break j + 1,
                        Some(_) => j += 1,
                    }
                }
            }
            b']' if next_is(i, |c| c == b'(') => b[i + 2..]
                .iter()
                .position(|&c| c == b')')
                .map_or(n, |p| i + 2 + p + 1),
            b'{' => {
                out.push((i, b'{'));
                find_unquoted(b, i + 1, b'}').map_or(n, |e| e + 1)
            }
            b'|' => {
                out.push((i, b'|'));
                i + 1
            }
            _ => i + 1,
        };
    }
    out
}

/// Sépare le contenu d'une ligne de la liste `{…}` qui la termine.
pub(crate) fn split_props(s: &str) -> Result<(&str, Option<Vec<Attr>>), String> {
    let Some(&(pos, _)) = marks(s).iter().find(|(_, c)| *c == b'{') else {
        return Ok((s, None));
    };
    let content = match pos {
        0 => "",
        _ if s.as_bytes()[pos - 1] == b' ' => &s[..pos - 1],
        _ => {
            return Err(
                "accolade : dans le texte, écrire « \\{ » ; une liste {…} suit une espace".into(),
            );
        }
    };
    let list = parse_list(&s[pos..]).map_err(|e| {
        format!("liste {{…}} mal formée : {e} (une accolade du texte s'écrit « \\{{ »)")
    })?;
    Ok((content, Some(list)))
}

/// Cellules d'une ligne de tableau `| a | b |` (contenu entre les `|`).
pub(crate) fn split_cells(row: &str) -> Option<Vec<&str>> {
    let pipes: Vec<usize> = marks(row)
        .into_iter()
        .filter(|(_, c)| *c == b'|')
        .map(|(p, _)| p)
        .collect();
    if pipes.len() < 2 || pipes[0] != 0 || pipes[pipes.len() - 1] != row.len() - 1 {
        return None;
    }
    Some(pipes.windows(2).map(|w| &row[w[0] + 1..w[1]]).collect())
}
