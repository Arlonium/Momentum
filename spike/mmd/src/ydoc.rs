//! Arbre de blocs ⇄ document Yjs au format BlockNote (y-prosemirror 1.x), côté serveur.
//!
//! Structure : `blockGroup > blockContainer[id] > <type props…>` (+ `blockGroup` pour les enfants) ;
//! `columnList[id] > column[id,width] > blockContainer…` sans enveloppe ; contenu en ligne = `XmlText`
//! formaté (marques en attributs) entrecoupé d'éléments `mention` / `math`.

use std::collections::HashMap;
use std::sync::Arc;

use yrs::types::text::YChange;
use yrs::types::Attrs;
use yrs::any::Number;
use yrs::{Any, GetString, Out, ReadTxn, Text, TransactionMut, Xml, XmlElementPrelim, XmlElementRef, XmlFragment, XmlFragmentRef, XmlOut, XmlTextPrelim, XmlTextRef};

use crate::model::{normalize, normalize_inline, Block, BlockKind, Inline, Marks};

pub const FRAGMENT: &str = "document-store";

/// Caractère qui représente un contenu en ligne non textuel (mention, maths) dans [`block_text`].
pub const OBJECT: char = '\u{FFFC}';

// ---------------------------------------------------------------- lecture

fn attr_out<T: ReadTxn>(el: &XmlElementRef, txn: &T, name: &str) -> Option<Out> {
    el.get_attribute(txn, name)
}

fn attr_str<T: ReadTxn>(el: &XmlElementRef, txn: &T, name: &str) -> String {
    match attr_out(el, txn, name) {
        Some(Out::Any(Any::String(s))) => s.to_string(),
        Some(Out::Any(Any::Number(Number::Int(n)))) => n.to_string(),
        Some(Out::Any(Any::Number(Number::Float(n)))) => n.to_string(),
        Some(Out::Any(Any::Bool(b))) => b.to_string(),
        _ => String::new(),
    }
}

fn attr_num<T: ReadTxn>(el: &XmlElementRef, txn: &T, name: &str) -> Option<f64> {
    match attr_out(el, txn, name) {
        Some(Out::Any(Any::Number(n))) => n.as_f64(),
        Some(Out::Any(Any::String(s))) => s.parse().ok(),
        _ => None,
    }
}

fn attr_bool<T: ReadTxn>(el: &XmlElementRef, txn: &T, name: &str) -> bool {
    matches!(attr_out(el, txn, name), Some(Out::Any(Any::Bool(true))))
        || matches!(attr_out(el, txn, name), Some(Out::Any(Any::String(s))) if &*s == "true")
}

fn elements<T: ReadTxn>(el: &XmlElementRef, txn: &T) -> Vec<XmlElementRef> {
    el.children(txn)
        .filter_map(|c| match c {
            XmlOut::Element(e) => Some(e),
            _ => None,
        })
        .collect()
}

fn marks_from(attrs: Option<&Attrs>) -> (Marks, Option<String>) {
    let mut marks = Marks::default();
    let mut link = None;
    if let Some(attrs) = attrs {
        for (k, v) in attrs.iter() {
            let on = !matches!(v, Any::Null | Any::Bool(false));
            match k.as_ref() {
                "bold" => marks.bold = on,
                "italic" => marks.italic = on,
                "strike" => marks.strike = on,
                "code" => marks.code = on,
                "underline" => marks.underline = on,
                "textColor" => {
                    if let Any::Map(m) = v {
                        if let Some(Any::String(c)) = m.get("stringValue") {
                            marks.color = Some(c.to_string());
                        }
                    }
                }
                "link" => {
                    if let Any::Map(m) = v {
                        if let Some(Any::String(h)) = m.get("href") {
                            link = Some(h.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    (marks, link)
}

fn read_text_runs<T: ReadTxn>(text: &XmlTextRef, txn: &T, out: &mut Vec<Inline>) {
    for d in text.diff(txn, YChange::identity) {
        let Out::Any(Any::String(s)) = &d.insert else { continue };
        let (marks, link) = marks_from(d.attributes.as_deref());
        let inline = Inline::Text { text: s.to_string(), marks };
        match link {
            None => out.push(inline),
            Some(href) => match out.last_mut() {
                Some(Inline::Link { href: h, children }) if *h == href => children.push(inline),
                _ => out.push(Inline::Link { href, children: vec![inline] }),
            },
        }
    }
}

/// Contenu en ligne d'un élément BlockNote (paragraphe, titre, cellule…).
fn read_inline<T: ReadTxn>(el: &XmlElementRef, txn: &T) -> Vec<Inline> {
    let mut out = Vec::new();
    for child in el.children(txn) {
        match child {
            XmlOut::Text(t) => read_text_runs(&t, txn, &mut out),
            XmlOut::Element(e) => match e.tag().as_ref() {
                "mention" => out.push(Inline::Mention {
                    kind: attr_str(&e, txn, "kind"),
                    id: attr_str(&e, txn, "id"),
                    label: attr_str(&e, txn, "label"),
                }),
                "math" => out.push(Inline::Math { latex: attr_str(&e, txn, "latex") }),
                _ => {}
            },
            XmlOut::Fragment(_) => {}
        }
    }
    normalize_inline(out)
}

fn read_table<T: ReadTxn>(table: &XmlElementRef, txn: &T) -> Vec<Vec<Vec<Inline>>> {
    elements(table, txn)
        .iter()
        .filter(|r| r.tag().as_ref() == "tableRow")
        .map(|row| {
            elements(row, txn)
                .iter()
                .map(|cell| elements(cell, txn).first().map(|p| read_inline(p, txn)).unwrap_or_default())
                .collect()
        })
        .collect()
}

fn read_container<T: ReadTxn>(bc: &XmlElementRef, txn: &T) -> Option<Block> {
    let id = attr_str(bc, txn, "id");
    let els = elements(bc, txn);
    let node = els.first()?;
    let children = els.get(1).filter(|g| g.tag().as_ref() == "blockGroup").map(|g| read_group(g, txn)).unwrap_or_default();
    let a = |n: &str| attr_str(node, txn, n);
    let (kind, content) = match node.tag().as_ref() {
        "paragraph" => (BlockKind::Paragraph, read_inline(node, txn)),
        "heading" => (BlockKind::Heading { level: attr_num(node, txn, "level").unwrap_or(1.0).clamp(1.0, 4.0) as u8 }, read_inline(node, txn)),
        "bulletListItem" => (BlockKind::BulletItem, read_inline(node, txn)),
        "numberedListItem" => (BlockKind::NumberedItem, read_inline(node, txn)),
        "checkListItem" => (BlockKind::Todo { checked: attr_bool(node, txn, "checked") }, read_inline(node, txn)),
        "toggleListItem" => (BlockKind::Toggle, read_inline(node, txn)),
        "quote" => (BlockKind::Quote, read_inline(node, txn)),
        "callout" => (BlockKind::Callout { icon: a("icon"), color: a("color") }, read_inline(node, txn)),
        "codeBlock" => {
            let text: String = node
                .children(txn)
                .filter_map(|c| match c {
                    XmlOut::Text(t) => Some(t.get_string(txn)),
                    _ => None,
                })
                .collect();
            let content = if text.is_empty() { vec![] } else { vec![Inline::Text { text, marks: Marks::default() }] };
            (BlockKind::Code { lang: a("language") }, content)
        }
        "equation" => (BlockKind::Equation { latex: a("latex") }, vec![]),
        "divider" => (BlockKind::Divider, vec![]),
        "image" => (BlockKind::Image { url: a("url"), alt: a("caption") }, vec![]),
        "file" => (BlockKind::File { url: a("url"), name: a("name") }, vec![]),
        "bookmark" => (BlockKind::Bookmark { url: a("url") }, vec![]),
        "table" => (BlockKind::Table { rows: read_table(node, txn) }, vec![]),
        "pageLink" => (BlockKind::PageLink { page_id: a("pageId"), title: a("title") }, vec![]),
        "databaseRef" => (BlockKind::DatabaseRef { database_id: a("databaseId"), view: a("view") }, vec![]),
        "syncedBlock" => (BlockKind::Synced { reference: a("ref") }, vec![]),
        "toc" => (BlockKind::Toc, vec![]),
        "unknown" => (BlockKind::Unknown { type_name: a("originalType"), data: a("data") }, vec![]),
        other => (BlockKind::Unknown { type_name: other.to_string(), data: String::new() }, vec![]),
    };
    Some(Block { id, kind, content, children })
}

/// Enfants d'un `blockGroup` ou d'une `column`.
fn read_group<T: ReadTxn>(group: &XmlElementRef, txn: &T) -> Vec<Block> {
    let mut out = Vec::new();
    for el in elements(group, txn) {
        match el.tag().as_ref() {
            "blockContainer" => out.extend(read_container(&el, txn)),
            "columnList" => out.push(Block {
                id: attr_str(&el, txn, "id"),
                kind: BlockKind::Columns,
                content: vec![],
                children: elements(&el, txn)
                    .iter()
                    .map(|col| {
                        let w = attr_num(col, txn, "width").unwrap_or(1.0);
                        Block {
                            id: attr_str(col, txn, "id"),
                            kind: BlockKind::Column { width: if (w - 1.0).abs() < f64::EPSILON { None } else { Some(w as f32) } },
                            content: vec![],
                            children: read_group(col, txn),
                        }
                    })
                    .collect(),
            }),
            _ => {}
        }
    }
    out
}

/// Lit tout le document BlockNote.
pub fn read_blocks<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef) -> Vec<Block> {
    let mut out = Vec::new();
    for child in frag.children(txn) {
        if let XmlOut::Element(g) = child {
            if g.tag().as_ref() == "blockGroup" {
                out.extend(read_group(&g, txn));
            }
        }
    }
    normalize(out)
}

// ---------------------------------------------------------------- écriture

fn empty_map() -> Any {
    Any::Map(Arc::new(HashMap::new()))
}

fn attrs_for(marks: &Marks, link: Option<&str>) -> Attrs {
    let mut a = Attrs::new();
    for (on, key) in [(marks.bold, "bold"), (marks.italic, "italic"), (marks.strike, "strike"), (marks.code, "code"), (marks.underline, "underline")] {
        if on {
            a.insert(Arc::from(key), empty_map());
        }
    }
    if let Some(c) = &marks.color {
        a.insert(Arc::from("textColor"), Any::Map(Arc::new(HashMap::from([("stringValue".to_string(), Any::from(c.as_str()))]))));
    }
    if let Some(href) = link {
        a.insert(Arc::from("link"), Any::Map(Arc::new(HashMap::from([("href".to_string(), Any::from(href))]))));
    }
    a
}

fn append(text: &XmlTextRef, txn: &mut TransactionMut, chunk: &str, attrs: Attrs) {
    let at = text.len(txn);
    // Attributs explicites (même vides) : le texte n'hérite pas du formatage précédent.
    text.insert_with_attributes(txn, at, chunk, attrs);
}

fn write_inline(el: &XmlElementRef, txn: &mut TransactionMut, content: &[Inline]) {
    let mut current: Option<XmlTextRef> = None;
    for inline in content {
        match inline {
            Inline::Text { text, marks } => {
                let t = current.get_or_insert_with(|| el.push_back(txn, XmlTextPrelim::new(""))).clone();
                append(&t, txn, text, attrs_for(marks, None));
            }
            Inline::Link { href, children } => {
                let t = current.get_or_insert_with(|| el.push_back(txn, XmlTextPrelim::new(""))).clone();
                for c in children {
                    if let Inline::Text { text, marks } = c {
                        append(&t, txn, text, attrs_for(marks, Some(href)));
                    }
                }
            }
            Inline::Mention { kind, id, label } => {
                current = None;
                let m = el.push_back(txn, XmlElementPrelim::empty("mention"));
                m.insert_attribute(txn, "kind", kind.as_str());
                m.insert_attribute(txn, "id", id.as_str());
                m.insert_attribute(txn, "label", label.as_str());
            }
            Inline::Math { latex } => {
                current = None;
                let m = el.push_back(txn, XmlElementPrelim::empty("math"));
                m.insert_attribute(txn, "latex", latex.as_str());
            }
        }
    }
}

fn node(parent: &XmlElementRef, txn: &mut TransactionMut, tag: &str, attrs: &[(&str, Any)]) -> XmlElementRef {
    let el = parent.push_back(txn, XmlElementPrelim::empty(tag));
    for (k, v) in attrs {
        el.insert_attribute(txn, *k, v.clone());
    }
    el
}

fn s(v: &str) -> Any {
    Any::from(v)
}

fn write_block(parent: &XmlElementRef, txn: &mut TransactionMut, b: &Block) {
    if let BlockKind::Columns = b.kind {
        let list = node(parent, txn, "columnList", &[("id", s(&b.id))]);
        for col in &b.children {
            let width = match col.kind {
                BlockKind::Column { width: Some(w) } => w as f64,
                _ => 1.0,
            };
            let c = node(&list, txn, "column", &[("id", s(&col.id)), ("width", num(width))]);
            for child in &col.children {
                write_block(&c, txn, child);
            }
        }
        return;
    }
    let bc = node(parent, txn, "blockContainer", &[("id", s(&b.id))]);
    match &b.kind {
        BlockKind::Paragraph => write_inline(&node(&bc, txn, "paragraph", &[]), txn, &b.content),
        BlockKind::Heading { level } => write_inline(&node(&bc, txn, "heading", &[("level", num(*level as f64))]), txn, &b.content),
        BlockKind::BulletItem => write_inline(&node(&bc, txn, "bulletListItem", &[]), txn, &b.content),
        BlockKind::NumberedItem => write_inline(&node(&bc, txn, "numberedListItem", &[]), txn, &b.content),
        BlockKind::Todo { checked } => write_inline(&node(&bc, txn, "checkListItem", &[("checked", Any::Bool(*checked))]), txn, &b.content),
        BlockKind::Toggle => write_inline(&node(&bc, txn, "toggleListItem", &[]), txn, &b.content),
        BlockKind::Quote => write_inline(&node(&bc, txn, "quote", &[]), txn, &b.content),
        BlockKind::Callout { icon, color } => write_inline(&node(&bc, txn, "callout", &[("icon", s(icon)), ("color", s(color))]), txn, &b.content),
        BlockKind::Code { lang } => {
            let el = node(&bc, txn, "codeBlock", &[("language", s(lang))]);
            let text = block_text(b);
            el.push_back(txn, XmlTextPrelim::new(text.as_str()));
        }
        BlockKind::Equation { latex } => {
            node(&bc, txn, "equation", &[("latex", s(latex))]);
        }
        BlockKind::Divider => {
            node(&bc, txn, "divider", &[]);
        }
        BlockKind::Image { url, alt } => {
            node(&bc, txn, "image", &[("url", s(url)), ("caption", s(alt))]);
        }
        BlockKind::File { url, name } => {
            node(&bc, txn, "file", &[("url", s(url)), ("name", s(name))]);
        }
        BlockKind::Bookmark { url } => {
            node(&bc, txn, "bookmark", &[("url", s(url))]);
        }
        BlockKind::Table { rows } => {
            let table = node(&bc, txn, "table", &[]);
            for (r, row) in rows.iter().enumerate() {
                let tr = node(&table, txn, "tableRow", &[]);
                for cell in row {
                    let td = node(&tr, txn, if r == 0 { "tableHeader" } else { "tableCell" }, &[]);
                    write_inline(&node(&td, txn, "tableParagraph", &[]), txn, cell);
                }
            }
        }
        BlockKind::PageLink { page_id, title } => {
            node(&bc, txn, "pageLink", &[("pageId", s(page_id)), ("title", s(title))]);
        }
        BlockKind::DatabaseRef { database_id, view } => {
            node(&bc, txn, "databaseRef", &[("databaseId", s(database_id)), ("view", s(view))]);
        }
        BlockKind::Synced { reference } => {
            node(&bc, txn, "syncedBlock", &[("ref", s(reference))]);
        }
        BlockKind::Toc => {
            node(&bc, txn, "toc", &[]);
        }
        BlockKind::Unknown { type_name, data } => {
            node(&bc, txn, "unknown", &[("originalType", s(type_name)), ("data", s(data))]);
        }
        BlockKind::Columns | BlockKind::Column { .. } => {}
    }
    if !b.children.is_empty() {
        let group = node(&bc, txn, "blockGroup", &[]);
        for child in &b.children {
            write_block(&group, txn, child);
        }
    }
}

/// Remplace tout le document par `blocks`.
pub fn write_blocks(txn: &mut TransactionMut, frag: &XmlFragmentRef, blocks: &[Block]) {
    let len = frag.len(txn);
    if len > 0 {
        frag.remove_range(txn, 0, len);
    }
    let group = frag.insert(txn, 0, XmlElementPrelim::empty("blockGroup"));
    for b in blocks {
        write_block(&group, txn, b);
    }
}

/// Texte brut du contenu en ligne ; mentions et maths valent chacune [`OBJECT`].
pub fn block_text(block: &Block) -> String {
    fn walk(content: &[Inline], out: &mut String) {
        for i in content {
            match i {
                Inline::Text { text, .. } => out.push_str(text),
                Inline::Link { children, .. } => walk(children, out),
                Inline::Mention { .. } | Inline::Math { .. } => out.push(OBJECT),
            }
        }
    }
    let mut out = String::new();
    walk(&block.content, &mut out);
    out
}

/// Nombre au format Yjs : entier si possible (comme le fait le JavaScript), sinon flottant.
fn num(v: f64) -> Any {
    if v.fract() == 0.0 && v.abs() < 9.0e15 { Any::Number(Number::Int(v as i64)) } else { Any::Number(Number::Float(v)) }
}
