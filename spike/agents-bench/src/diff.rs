//! Prototype jetable de l'écriture par différence (`PageDoc::write_diff`, Ruling R1227) : liste de
//! frères par liste de frères, les correspondances de `keep_ids`, un bloc inchangé jamais touché,
//! des props changées écrites en attributs, un contenu en ligne changé récrit en entier
//! (`InlineMode::Whole`) ou par préfixe et suffixe communs (`InlineMode::Affix`, Step 3), un bloc
//! déplacé parmi ses frères recréé au même identifiant, un bloc neuf inséré, un bloc absent
//! supprimé.
//!
//! Ne vaut que pour une page « propre » (celle qu'écrivent BlockNote et le serveur) : dès que les
//! éléments Yjs d'une liste ne répondent pas un à un aux blocs que `read_blocks` en lit, le
//! prototype rend `None`, et l'appelant retombe sur `rewrite_blocks`. Ce que le vrai `write_diff`
//! fait de ce que `read_blocks` ne montre pas reste à S2.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{Block, Content, Inline, PropValue, Props, Styles, Table, normalize};
use mementum_schema::{PropDefault, PropSpec, PropType, block as spec_of};
use yrs::any::Number;
use yrs::types::Attrs;
use yrs::types::text::YChange;
use yrs::{
    Any, Out, ReadTxn, Text, Transact, TransactionMut, Xml, XmlElementRef, XmlFragment, XmlOut,
    XmlTextRef,
};

use crate::blocknote::{
    BLOCK_CONTAINER, BLOCK_GROUP, COLUMN, COLUMN_LIST, Ids, attr_string, elements, write_children,
    write_inline, write_nodes,
};
use crate::kept_ids::keep_ids;
use crate::table::write_table;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InlineMode {
    /// Un contenu en ligne changé est récrit en entier (R1227 tel qu'écrit).
    Whole,
    /// Par préfixe et suffixe communs, dans un seul `XmlText` ; sinon en entier.
    Affix,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub inline: InlineMode,
    /// Les suppressions dans une transaction, les écritures dans une seconde (la forme de R367).
    pub two_txns: bool,
}

#[derive(Default, Debug, Clone)]
pub struct Stats {
    pub updates: Vec<Vec<u8>>,
    /// Blocs supprimés puis récrits sous leur identifiant (déplacés, ou de type changé).
    pub recreated: usize,
    pub inserted: usize,
    pub removed: usize,
    pub props_changed: usize,
    pub inline_whole: usize,
    pub inline_affix: usize,
    pub tables: usize,
    /// Blocs de la page courante (tous niveaux) et blocs touchés d'une façon ou d'une autre.
    pub current_blocks: usize,
    pub touched: usize,
    /// Lecture des blocs, `keep_ids`, plan, écriture (transactions validées comprises).
    pub phases: [std::time::Duration; 4],
    /// Par écriture par préfixe : (rang du texte dans le nœud de contenu, place UTF-16, longueur retirée).
    pub affix_at: Vec<(usize, u32, u32)>,
}

impl Stats {
    pub fn bytes(&self) -> usize {
        self.updates.iter().map(Vec::len).sum()
    }
}

/// Ce que fera l'écriture, lu avant d'écrire (transaction de lecture).
#[derive(Default)]
struct Plan {
    /// (liste, index, longueur) : retirés de la plus haute place à la plus basse dans une liste.
    deletions: Vec<(XmlElementRef, u32, u32)>,
    /// (élément, attribut, valeur ; `None` : retiré).
    attrs: Vec<(XmlElementRef, String, Option<Any>)>,
    /// (liste, entrées dans l'ordre voulu) : `None` pour un élément gardé, `Some` pour un bloc écrit.
    lists: Vec<(XmlElementRef, Vec<Option<Block>>)>,
    /// (conteneur, enfants) : un `blockGroup` à créer.
    groups: Vec<(XmlElementRef, Vec<Block>)>,
    /// Contenu en ligne récrit en entier : (nœud de contenu, longueur à retirer, contenu).
    inline_whole: Vec<(XmlElementRef, u32, Vec<Inline>)>,
    /// Par préfixe et suffixe : (texte, place UTF-16, longueur UTF-16 retirée, plages écrites).
    inline_affix: Vec<(XmlTextRef, u32, u32, Vec<(String, Attrs)>)>,
    /// Tableaux récrits : (nœud `table`, longueur à retirer, tableau).
    tables: Vec<(XmlElementRef, u32, Table)>,
    /// Identifiants des blocs gardés en place, à tout niveau : aucun bloc écrit ne les prend.
    kept: HashSet<String>,
    stats: Stats,
}

struct Mismatch;

/// Écrit `target` par différence avec la page. `None` : la page n'est pas de la forme que le
/// prototype sait lire (l'appelant retombe sur `rewrite_blocks`).
pub fn write_diff(page: &PageDoc, target: &[Block], opts: Options) -> Option<Stats> {
    let start = std::time::Instant::now();
    let mut target = normalize(target.to_vec());
    let current = page.read_blocks();
    let t_read = start.elapsed();
    keep_ids(&current, &mut target);
    let t_keep = start.elapsed() - t_read;
    let doc = page.yrs_doc();
    // Avant toute transaction (feature `sync` de yrs).
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut plan = Plan::default();
    {
        let txn = doc.transact();
        let roots: Vec<XmlElementRef> = fragment
            .children(&txn)
            .filter_map(|c| match c {
                XmlOut::Element(e) if e.tag().as_ref() == BLOCK_GROUP => Some(e),
                _ => None,
            })
            .collect();
        if roots.len() != 1 || fragment.len(&txn) != 1 {
            return None;
        }
        let root = roots[0].clone();
        let elems = elements(&txn, &root);
        plan.stats.current_blocks = count(&current);
        plan_list(&txn, &root, elems, &current, &target, opts, &mut plan).ok()?;
    }
    let t_plan = start.elapsed() - t_read - t_keep;
    let mut stats = std::mem::take(&mut plan.stats);
    stats.phases = [t_read, t_keep, t_plan, std::time::Duration::ZERO];
    let mut ids = Ids::taken(std::mem::take(&mut plan.kept));
    if opts.two_txns {
        {
            let mut txn = doc.transact_mut();
            delete(&mut txn, &mut plan);
            stats.updates.push(txn.encode_update_v1());
        }
        let mut txn = doc.transact_mut();
        write(&mut txn, &mut plan, &mut ids);
        stats.updates.push(txn.encode_update_v1());
    } else {
        let mut txn = doc.transact_mut();
        delete(&mut txn, &mut plan);
        write(&mut txn, &mut plan, &mut ids);
        stats.updates.push(txn.encode_update_v1());
    }
    stats.phases[3] = start.elapsed() - t_read - t_keep - t_plan;
    stats.touched = stats.recreated
        + stats.inserted
        + stats.removed
        + stats.props_changed.max(stats.inline_whole + stats.inline_affix + stats.tables);
    Some(stats)
}

fn count(blocks: &[Block]) -> usize {
    blocks.iter().map(|b| 1 + count(&b.children)).sum()
}

/// L'identité Yjs d'un élément.
pub fn bid(e: &XmlElementRef) -> yrs::branch::BranchID {
    AsRef::<yrs::branch::Branch>::as_ref(e).id()
}

fn addr(e: &XmlElementRef) -> usize {
    AsRef::<yrs::branch::Branch>::as_ref(e) as *const yrs::branch::Branch as usize
}

fn delete(txn: &mut TransactionMut, plan: &mut Plan) {
    // Par liste, de la plus haute place à la plus basse : les places lues restent justes.
    plan.deletions
        .sort_by_key(|(list, at, _)| (addr(list), std::cmp::Reverse(*at)));
    for (list, at, len) in &plan.deletions {
        list.remove_range(txn, *at, *len);
    }
    for (node, len, _) in &plan.inline_whole {
        if *len > 0 {
            node.remove_range(txn, 0, *len);
        }
    }
    for (text, at, len, _) in &plan.inline_affix {
        if *len > 0 {
            text.remove_range(txn, *at, *len);
        }
    }
    for (node, len, _) in &plan.tables {
        if *len > 0 {
            node.remove_range(txn, 0, *len);
        }
    }
}

fn write(txn: &mut TransactionMut, plan: &mut Plan, ids: &mut Ids) {
    for (element, name, value) in &plan.attrs {
        match value {
            Some(v) => {
                element.insert_attribute(txn, name.as_str(), v.clone());
            }
            None => element.remove_attribute(txn, &name.as_str()),
        }
    }
    for (list, entries) in &plan.lists {
        let mut at = 0u32;
        for entry in entries {
            match entry {
                None => at += 1,
                Some(b) => at += write_nodes(txn, list, at, std::slice::from_ref(b), ids),
            }
        }
    }
    for (container, children) in &plan.groups {
        write_children(txn, container, children, ids);
    }
    for (node, _, content) in &plan.inline_whole {
        write_inline(txn, node, content);
    }
    for (text, at, _, runs) in &plan.inline_affix {
        let mut pos = *at;
        for (s, attrs) in runs {
            text.insert_with_attributes(txn, pos, s, attrs.clone());
            pos += s.encode_utf16().count() as u32;
        }
    }
    for (node, _, table) in &plan.tables {
        write_table(txn, node, Some(table));
    }
}

/// Le genre d'élément qui porte un bloc de ce type.
fn tag_of(kind: &str) -> &str {
    match kind {
        COLUMN_LIST | COLUMN => kind,
        _ => BLOCK_CONTAINER,
    }
}

fn plan_list<T: ReadTxn>(
    txn: &T,
    list: &XmlElementRef,
    elems: Vec<XmlElementRef>,
    current: &[Block],
    target: &[Block],
    opts: Options,
    plan: &mut Plan,
) -> Result<(), Mismatch> {
    if elems.len() != current.len() || list.len(txn) as usize != elems.len() {
        return Err(Mismatch);
    }
    for (e, b) in elems.iter().zip(current) {
        if attr_string(txn, e, "id") != b.id || e.tag().as_ref() != tag_of(&b.kind) {
            return Err(Mismatch);
        }
    }
    let at: HashMap<&str, usize> = current
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id.as_str(), i))
        .collect();
    // Paires (courant, voulu) de même identifiant et de même type, dans l'ordre voulu.
    let pairs: Vec<(usize, usize)> = target
        .iter()
        .enumerate()
        .filter_map(|(j, b)| {
            let i = *at.get(b.id.as_str())?;
            (current[i].kind == b.kind).then_some((j, i))
        })
        .collect();
    // Les gardés en place : la plus longue sous-suite croissante des places courantes.
    let stay = longest_increasing(&pairs);
    let stay_cur: HashSet<usize> = stay.iter().map(|&(_, i)| i).collect();
    let stay_by_target: HashMap<usize, usize> = stay.iter().copied().collect();
    let written_ids: HashSet<&str> = target.iter().map(|b| b.id.as_str()).collect();

    // Suppressions : les éléments courants qui ne restent pas, par suites contiguës.
    let mut i = 0;
    while i < current.len() {
        if stay_cur.contains(&i) {
            i += 1;
            continue;
        }
        let start = i;
        while i < current.len() && !stay_cur.contains(&i) {
            if !written_ids.contains(current[i].id.as_str()) {
                plan.stats.removed += 1;
            }
            i += 1;
        }
        plan.deletions
            .push((list.clone(), start as u32, (i - start) as u32));
    }

    let mut entries = Vec::with_capacity(target.len());
    let mut nested = Vec::new();
    for (j, b) in target.iter().enumerate() {
        match stay_by_target.get(&j) {
            Some(&i) => {
                entries.push(None);
                plan.kept.insert(b.id.clone());
                nested.push((elems[i].clone(), i, j));
            }
            None => {
                if at.contains_key(b.id.as_str()) {
                    plan.stats.recreated += 1;
                } else {
                    plan.stats.inserted += 1;
                }
                entries.push(Some(b.clone()));
            }
        }
    }
    if entries.iter().any(Option::is_some) {
        plan.lists.push((list.clone(), entries));
    }
    for (element, i, j) in nested {
        plan_block(txn, &element, &current[i], &target[j], opts, plan)?;
    }
    Ok(())
}

fn plan_block<T: ReadTxn>(
    txn: &T,
    element: &XmlElementRef,
    current: &Block,
    target: &Block,
    opts: Options,
    plan: &mut Plan,
) -> Result<(), Mismatch> {
    let spec = spec_of(&target.kind).ok_or(Mismatch)?;
    if matches!(target.kind.as_str(), COLUMN_LIST | COLUMN) {
        plan_props(element, &current.props, &target.props, spec.props, plan);
        let elems = elements(txn, element);
        return plan_list(
            txn,
            element,
            elems,
            &current.children,
            &target.children,
            opts,
            plan,
        );
    }
    // Un conteneur : son nœud de contenu, puis son `blockGroup`.
    let mut content = None;
    let mut group = None;
    for (k, child) in element.children(txn).enumerate() {
        match child {
            XmlOut::Element(e) if e.tag().as_ref() == BLOCK_GROUP => {
                if group.is_some() {
                    return Err(Mismatch);
                }
                group = Some((k as u32, e));
            }
            XmlOut::Element(e) if content.is_none() => content = Some(e),
            _ => return Err(Mismatch),
        }
    }
    let content = content.ok_or(Mismatch)?;
    if content.tag().as_ref() != target.kind {
        return Err(Mismatch);
    }
    plan_props(&content, &current.props, &target.props, spec.props, plan);
    if current.content != target.content {
        match &target.content {
            Content::Inline(inline) => {
                let len = content.len(txn);
                let affix = (opts.inline == InlineMode::Affix)
                    .then(|| plan_affix(txn, &content, inline))
                    .flatten();
                match affix {
                    Some(edit) => {
                        plan.stats.inline_affix += 1;
                        plan.stats.affix_at.push((edit.4, edit.1, edit.2));
                        plan.inline_affix.push((edit.0, edit.1, edit.2, edit.3));
                    }
                    None => {
                        plan.stats.inline_whole += 1;
                        plan.inline_whole.push((content.clone(), len, inline.clone()));
                    }
                }
            }
            Content::Table(table) => {
                plan.stats.tables += 1;
                plan.tables
                    .push((content.clone(), content.len(txn), table.clone()));
            }
            Content::None => return Err(Mismatch),
        }
    }
    match (group, current.children.is_empty(), target.children.is_empty()) {
        (None, true, true) => {}
        (Some((k, _)), false, true) => plan.deletions.push((element.clone(), k, 1)),
        (None, true, false) => plan.groups.push((element.clone(), target.children.clone())),
        (Some((_, g)), false, false) => {
            let elems = elements(txn, &g);
            plan_list(
                txn,
                &g,
                elems,
                &current.children,
                &target.children,
                opts,
                plan,
            )?;
        }
        _ => return Err(Mismatch),
    }
    Ok(())
}

/// Les props qui diffèrent, écrites comme `write_props` les écrirait (défaut du registre pour une
/// prop du registre retirée ; une prop inconnue retirée l'est de l'élément).
fn plan_props(
    element: &XmlElementRef,
    current: &Props,
    target: &Props,
    specs: &[PropSpec],
    plan: &mut Plan,
) {
    if current == target {
        return;
    }
    plan.stats.props_changed += 1;
    let names: HashSet<&String> = current.keys().chain(target.keys()).collect();
    for name in names {
        if current.get(name) == target.get(name) {
            continue;
        }
        let spec = specs.iter().find(|s| s.name == name);
        let value = match (target.get(name), spec) {
            (Some(v), spec) => Some(prop_any(v, spec.map(|s| s.ty))),
            (None, Some(spec)) => Some(spec.default.map_or(Any::Undefined, default_any)),
            (None, None) => None,
        };
        plan.attrs.push((element.clone(), name.clone(), value));
    }
}

fn prop_any(value: &PropValue, ty: Option<PropType>) -> Any {
    match value {
        PropValue::Str(s) => Any::from(s.as_str()),
        PropValue::Int(i) => Any::Number(Number::Int(*i)),
        PropValue::Float(f) if ty == Some(PropType::Float) => js_number(*f),
        PropValue::Float(f) => Any::Number(Number::Float(*f)),
        PropValue::Bool(b) => Any::Bool(*b),
    }
}

fn default_any(default: PropDefault) -> Any {
    match default {
        PropDefault::Str(s) => Any::from(s),
        PropDefault::Int(i) => Any::Number(Number::Int(i)),
        PropDefault::Float(f) => js_number(f),
        PropDefault::Bool(b) => Any::Bool(b),
    }
}

fn js_number(f: f64) -> Any {
    if f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_991.0 {
        Any::Number(Number::Int(f as i64))
    } else {
        Any::Number(Number::Float(f))
    }
}

fn longest_increasing(items: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; items.len()];
    for (k, &(_, i)) in items.iter().enumerate() {
        let pile = tails.partition_point(|&t| items[t].1 < i);
        if pile > 0 {
            previous[k] = Some(tails[pile - 1]);
        }
        if pile == tails.len() {
            tails.push(k);
        } else {
            tails[pile] = k;
        }
    }
    let mut out = Vec::new();
    let mut at = tails.last().copied();
    while let Some(k) = at {
        out.push(items[k]);
        at = previous[k];
    }
    out.reverse();
    out
}

// ======================================================= contenu en ligne par préfixe et suffixe

/// Une unité de contenu en ligne : un caractère et ses marques, ou un élément en ligne.
#[derive(Clone, PartialEq, Debug)]
enum Unit {
    Char(char, Styles, Option<String>),
    Element(String, Vec<(String, String)>),
}

/// Où vit une unité courante : (rang de l'enfant du nœud de contenu, place UTF-16 dans son texte).
type Place = (usize, u32);

fn current_units<T: ReadTxn>(txn: &T, content: &XmlElementRef) -> Option<(Vec<Unit>, Vec<Place>)> {
    let mut units = Vec::new();
    let mut places = Vec::new();
    for (k, child) in content.children(txn).enumerate() {
        match child {
            XmlOut::Text(text) => {
                let mut offset = 0u32;
                for run in text.diff(txn, YChange::identity) {
                    let Out::Any(Any::String(s)) = &run.insert else {
                        return None;
                    };
                    let (styles, link) = styles_of(run.attributes.as_deref());
                    for c in s.chars() {
                        units.push(Unit::Char(c, styles.clone(), link.clone()));
                        places.push((k, offset));
                        offset += c.len_utf16() as u32;
                    }
                }
            }
            XmlOut::Element(e) => {
                let mut attrs: Vec<(String, String)> = e
                    .attributes(txn)
                    .map(|(n, _)| (n.to_string(), attr_string(txn, &e, n)))
                    .collect();
                attrs.sort();
                units.push(Unit::Element(e.tag().to_string(), attrs));
                places.push((k, 0));
            }
            XmlOut::Fragment(_) => return None,
        }
    }
    Some((units, places))
}

fn target_units(content: &[Inline], link: Option<&str>, out: &mut Vec<Unit>) {
    for inline in content {
        match inline {
            Inline::Text { text, styles } => {
                for c in text.chars() {
                    out.push(Unit::Char(c, styles.clone(), link.map(str::to_string)));
                }
            }
            Inline::Link { href, children } => target_units(children, Some(href), out),
            Inline::Mention { kind, id, label } => {
                let mut attrs = vec![
                    ("id".to_string(), id.clone()),
                    ("kind".to_string(), kind.clone()),
                    ("label".to_string(), label.clone()),
                ];
                attrs.sort();
                out.push(Unit::Element("mention".into(), attrs));
            }
            Inline::Math { latex } => out.push(Unit::Element(
                "math".into(),
                vec![("latex".to_string(), latex.clone())],
            )),
            Inline::HardBreak => out.push(Unit::Element("hardBreak".into(), vec![])),
        }
    }
}

/// Le préfixe et le suffixe communs ; le milieu changé doit tenir dans un seul `XmlText`, et ne
/// porter aucun élément en ligne, des deux côtés. Sinon `None` (récrire en entier).
fn plan_affix<T: ReadTxn>(
    txn: &T,
    content: &XmlElementRef,
    inline: &[Inline],
) -> Option<(XmlTextRef, u32, u32, Vec<(String, Attrs)>, usize)> {
    let (old, places) = current_units(txn, content)?;
    let mut new = Vec::new();
    target_units(inline, None, &mut new);
    let mut p = 0;
    while p < old.len() && p < new.len() && old[p] == new[p] {
        p += 1;
    }
    let mut s = 0;
    while s < old.len() - p && s < new.len() - p && old[old.len() - 1 - s] == new[new.len() - 1 - s]
    {
        s += 1;
    }
    let old_mid = &old[p..old.len() - s];
    let new_mid = &new[p..new.len() - s];
    if old_mid
        .iter()
        .chain(new_mid)
        .any(|u| matches!(u, Unit::Element(..)))
    {
        return None;
    }
    // Le texte où écrire : celui du milieu retiré, sinon celui de l'unité d'avant, sinon d'après.
    let children: Vec<XmlOut> = content.children(txn).collect();
    let (child, at) = if !old_mid.is_empty() {
        let first = places[p];
        let last = places[p + old_mid.len() - 1];
        if first.0 != last.0 {
            return None;
        }
        first
    } else if p > 0 && matches!(old[p - 1], Unit::Char(..)) {
        let (k, o) = places[p - 1];
        let Unit::Char(c, ..) = &old[p - 1] else {
            unreachable!()
        };
        (k, o + c.len_utf16() as u32)
    } else if p < old.len() && matches!(old[p], Unit::Char(..)) {
        places[p]
    } else {
        return None;
    };
    let XmlOut::Text(text) = children.get(child)?.clone() else {
        return None;
    };
    let removed: u32 = old_mid
        .iter()
        .map(|u| match u {
            Unit::Char(c, ..) => c.len_utf16() as u32,
            Unit::Element(..) => 0,
        })
        .sum();
    let mut runs: Vec<(String, Styles, Option<String>)> = Vec::new();
    for u in new_mid {
        let Unit::Char(c, styles, link) = u else {
            unreachable!()
        };
        match runs.last_mut() {
            Some((s, st, l)) if st == styles && l == link => s.push(*c),
            _ => runs.push((c.to_string(), styles.clone(), link.clone())),
        }
    }
    let runs = runs
        .into_iter()
        .map(|(s, st, l)| (s, marks(&st, l.as_deref())))
        .collect();
    Some((text, at, removed, runs, child))
}

fn styles_of(attrs: Option<&Attrs>) -> (Styles, Option<String>) {
    let mut styles = Styles::default();
    let mut link = None;
    let field = |value: &Any, key: &str| match value {
        Any::Map(m) => match m.get(key) {
            Some(Any::String(s)) => Some(s.to_string()),
            _ => None,
        },
        Any::String(s) => Some(s.to_string()),
        _ => None,
    };
    for (key, value) in attrs.into_iter().flatten() {
        let on = !matches!(value, Any::Null | Any::Undefined | Any::Bool(false));
        match key.as_ref() {
            "bold" => styles.bold = on,
            "italic" => styles.italic = on,
            "underline" => styles.underline = on,
            "strike" => styles.strike = on,
            "code" => styles.code = on,
            "textColor" => styles.text_color = field(value, "stringValue"),
            "backgroundColor" => styles.background_color = field(value, "stringValue"),
            "link" => link = field(value, "href"),
            _ => {}
        }
    }
    (styles, link)
}

/// Les marques d'une plage, comme `blocknote::marks` (attributs explicites : le texte n'hérite
/// pas des marques voisines).
pub fn marks(styles: &Styles, link: Option<&str>) -> Attrs {
    let one = |key: &str, value: &str| {
        Any::Map(Arc::new(HashMap::from([(
            key.to_string(),
            Any::from(value),
        )])))
    };
    let mut attrs = Attrs::new();
    for (on, key) in [
        (styles.bold, "bold"),
        (styles.italic, "italic"),
        (styles.underline, "underline"),
        (styles.strike, "strike"),
        (styles.code, "code"),
    ] {
        if on {
            attrs.insert(key.into(), Any::Map(Arc::new(HashMap::new())));
        }
    }
    if let Some(color) = &styles.text_color {
        attrs.insert("textColor".into(), one("stringValue", color));
    }
    if let Some(color) = &styles.background_color {
        attrs.insert("backgroundColor".into(), one("stringValue", color));
    }
    if let Some(href) = link {
        attrs.insert("link".into(), one("href", href));
    }
    attrs
}
