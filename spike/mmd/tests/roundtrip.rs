//! Task 3 — aller-retour MMD ⇄ arbre de blocs.

use std::collections::HashMap;

use proptest::prelude::*;
use spike_mmd::{normalize, parse, render, Block, BlockKind, Inline, Marks, RenderOpts};

const FIXTURE: &str = include_str!("../../fixtures/mmd/all-blocks.mmd");

fn plain() -> RenderOpts {
    RenderOpts { anchors: false }
}

#[test]
fn fixture_is_canonical() {
    let blocks = parse(FIXTURE).expect("la fixture doit se parser");
    let out = render(&blocks, plain()).markdown;
    assert_eq!(out, FIXTURE);
}

#[test]
fn fixture_has_28_top_level_blocks() {
    assert_eq!(parse(FIXTURE).unwrap().len(), 28);
}

fn kind_name(k: &BlockKind) -> &'static str {
    match k {
        BlockKind::Paragraph => "Paragraph",
        BlockKind::Heading { .. } => "Heading",
        BlockKind::BulletItem => "BulletItem",
        BlockKind::NumberedItem => "NumberedItem",
        BlockKind::Todo { .. } => "Todo",
        BlockKind::Toggle => "Toggle",
        BlockKind::Quote => "Quote",
        BlockKind::Callout { .. } => "Callout",
        BlockKind::Code { .. } => "Code",
        BlockKind::Equation { .. } => "Equation",
        BlockKind::Divider => "Divider",
        BlockKind::Image { .. } => "Image",
        BlockKind::File { .. } => "File",
        BlockKind::Bookmark { .. } => "Bookmark",
        BlockKind::Table { .. } => "Table",
        BlockKind::Columns => "Columns",
        BlockKind::Column { .. } => "Column",
        BlockKind::PageLink { .. } => "PageLink",
        BlockKind::DatabaseRef { .. } => "DatabaseRef",
        BlockKind::Synced { .. } => "Synced",
        BlockKind::Toc => "Toc",
        BlockKind::Unknown { .. } => "Unknown",
    }
}

fn collect_kinds(blocks: &[Block], out: &mut Vec<&'static str>) {
    for b in blocks {
        out.push(kind_name(&b.kind));
        collect_kinds(&b.children, out);
    }
}

#[test]
fn fixture_covers_all_kinds() {
    let mut kinds = Vec::new();
    collect_kinds(&parse(FIXTURE).unwrap(), &mut kinds);
    for k in [
        "Paragraph", "Heading", "BulletItem", "NumberedItem", "Todo", "Toggle", "Quote", "Callout", "Code", "Equation",
        "Divider", "Image", "File", "Bookmark", "Table", "Columns", "Column", "PageLink", "DatabaseRef", "Synced", "Toc",
        "Unknown",
    ] {
        assert!(kinds.contains(&k), "variante absente de la fixture : {k}");
    }
}

#[test]
fn unknown_block_survives() {
    let blocks = parse(FIXTURE).unwrap();
    let again = parse(&render(&blocks, plain()).markdown).unwrap();
    let u = again.last().unwrap();
    assert_eq!(u.id, "b_unknown01");
    assert_eq!(u.kind, BlockKind::Unknown { type_name: "future_widget".into(), data: "eyJ4IjoxfQ==".into() });
}

#[test]
fn anchors_render_and_parse() {
    let blocks = parse(FIXTURE).unwrap();
    let r = render(&blocks, RenderOpts { anchors: true });
    let livrer = r.markdown.lines().find(|l| l.starts_with("Livrer")).unwrap();
    assert!(livrer.ends_with(" ^b2"), "{livrer}");
    assert!(r.markdown.contains("<callout id=\"b17\" icon=\"⚠️\" color=\"yellow\">"), "{}", r.markdown);
    let again = parse(&r.markdown).unwrap();
    assert_eq!(again[1].id, "b2");
    assert_eq!(again[14].id, "b17");
    // L'alias b2 désigne bien le bloc d'origine.
    let map: HashMap<_, _> = r.anchors.iter().cloned().collect();
    assert_eq!(map["b2"], blocks[1].id);
}

// ---------- Propriété : arbre aléatoire → MMD → arbre ----------

fn word() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9éàç]{1,6}".prop_map(|s| s)
}

/// Texte libre, avec les caractères qui piègent le Markdown.
fn free_text() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            word(),
            Just(" ".to_string()),
            Just("🚀".to_string()),
            Just("<".to_string()),
            Just("*".to_string()),
            Just("&".to_string()),
            Just("_".to_string()),
            Just("[x]".to_string()),
            Just("`".to_string()),
            Just("$".to_string()),
            Just("#".to_string()),
            Just("|".to_string()),
            Just("\\".to_string()),
            Just("~".to_string()),
            Just("^b3".to_string()),
            Just("- ".to_string()),
            Just("1. ".to_string()),
            Just("!".to_string()),
        ],
        1..6,
    )
    .prop_map(|v| v.concat())
}

fn marks() -> impl Strategy<Value = Marks> {
    (any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>(), prop::option::of(prop_oneof![Just("red".to_string()), Just("blue".to_string())]))
        .prop_map(|(bold, italic, strike, code, underline, color)| Marks { bold, italic, strike, code, underline, color })
}

fn inline() -> impl Strategy<Value = Inline> {
    prop_oneof![
        6 => (free_text(), marks()).prop_map(|(text, marks)| Inline::Text { text, marks }),
        1 => (prop_oneof![Just("page"), Just("user"), Just("date")], "[a-z0-9_]{1,8}", free_text())
            .prop_map(|(k, id, label)| Inline::Mention { kind: k.to_string(), id, label }),
        // LaTeX valide : accolades équilibrées (pulldown-cmark l'exige pour `$…$`).
        1 => "[a-zA-Z0-9]{1,4}( [a-z^]{1,4})?( [a-z]\\{[a-z0-9]{1,3}\\})?".prop_map(|latex| Inline::Math { latex }),
        1 => ("[a-z]{1,6}", free_text()).prop_map(|(path, text)| Inline::Link {
            href: format!("https://ex.dev/{path}"),
            children: vec![Inline::Text { text, marks: Marks::default() }],
        }),
    ]
}

fn content() -> impl Strategy<Value = Vec<Inline>> {
    prop::collection::vec(inline(), 0..4)
}

fn leaf_kind() -> impl Strategy<Value = BlockKind> {
    prop_oneof![
        Just(BlockKind::Paragraph),
        (1u8..=4).prop_map(|level| BlockKind::Heading { level }),
        Just(BlockKind::Quote),
        (prop_oneof![Just("⚠️"), Just("💡")], prop_oneof![Just("yellow"), Just("default")])
            .prop_map(|(i, c)| BlockKind::Callout { icon: i.into(), color: c.into() }),
        Just(BlockKind::Divider),
        "[a-z]{0,5}".prop_map(|lang| BlockKind::Code { lang }),
        "[a-z0-9 ^{}\\\\]{1,12}".prop_map(|latex| BlockKind::Equation { latex: latex.trim().to_string() }),
        (free_text(), "[a-z]{1,5}").prop_map(|(alt, p)| BlockKind::Image { url: format!("https://ex.dev/{p}.png"), alt }),
        ("[a-z]{1,5}", free_text()).prop_map(|(p, name)| BlockKind::File { url: format!("https://ex.dev/{p}"), name }),
        "[a-z]{1,5}".prop_map(|p| BlockKind::Bookmark { url: format!("https://ex.dev/{p}") }),
        prop::collection::vec(prop::collection::vec(content(), 2), 1..3).prop_map(|rows| BlockKind::Table { rows }),
        ("pg_[a-z0-9]{1,6}", free_text()).prop_map(|(page_id, title)| BlockKind::PageLink { page_id, title }),
        ("db_[a-z0-9]{1,6}", "[A-Za-z]{1,8}").prop_map(|(database_id, view)| BlockKind::DatabaseRef { database_id, view }),
        Just(BlockKind::Toc),
        ("[a-z_]{1,10}", "[A-Za-z0-9+/=]{1,12}").prop_map(|(type_name, data)| BlockKind::Unknown { type_name, data }),
    ]
}

fn leaf_block() -> impl Strategy<Value = Block> {
    (leaf_kind(), content(), "[a-z0-9]{6}").prop_map(|(kind, content, id)| {
        let content = match &kind {
            BlockKind::Paragraph | BlockKind::Heading { .. } | BlockKind::Quote | BlockKind::Callout { .. } => content,
            BlockKind::Code { .. } => vec![Inline::Text { text: "let x = 1;\n  y()".into(), marks: Marks::default() }],
            _ => vec![],
        };
        let id = match &kind {
            BlockKind::Unknown { .. } => format!("u_{id}"),
            _ => format!("id_{id}"),
        };
        Block { id, kind, content, children: vec![] }
    })
}

fn block_tree() -> impl Strategy<Value = Block> {
    leaf_block().prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            (content(), prop::collection::vec(inner.clone(), 0..3), "[a-z0-9]{6}")
                .prop_map(|(c, ch, id)| list_item(BlockKind::BulletItem, c, ch, id)),
            (content(), prop::collection::vec(inner.clone(), 0..3), "[a-z0-9]{6}")
                .prop_map(|(c, ch, id)| list_item(BlockKind::NumberedItem, c, ch, id)),
            (any::<bool>(), content(), prop::collection::vec(inner.clone(), 0..3), "[a-z0-9]{6}")
                .prop_map(|(checked, c, ch, id)| list_item(BlockKind::Todo { checked }, c, ch, id)),
            (content(), prop::collection::vec(inner.clone(), 0..3), "[a-z0-9]{6}")
                .prop_map(|(c, ch, id)| Block { id: format!("id_{id}"), kind: BlockKind::Toggle, content: c, children: ch }),
            ("b_[a-z0-9]{1,6}", prop::collection::vec(inner.clone(), 0..3), "[a-z0-9]{6}")
                .prop_map(|(reference, ch, id)| Block { id: format!("id_{id}"), kind: BlockKind::Synced { reference }, content: vec![], children: ch }),
            (prop::collection::vec(prop::collection::vec(inner.clone(), 0..3), 1..3), "[a-z0-9]{6}").prop_map(|(cols, id)| Block {
                id: format!("id_{id}"),
                kind: BlockKind::Columns,
                content: vec![],
                children: cols
                    .into_iter()
                    .enumerate()
                    .map(|(i, ch)| Block { id: format!("id_{id}c{i}"), kind: BlockKind::Column { width: None }, content: vec![], children: ch })
                    .collect(),
            }),
        ]
    })
}

fn list_item(kind: BlockKind, content: Vec<Inline>, children: Vec<Block>, id: String) -> Block {
    Block { id: format!("id_{id}"), kind, content, children }
}

/// Retire les ids (sauf ceux des blocs inconnus, toujours écrits) pour comparer sans ancres.
fn strip_ids(blocks: &mut [Block]) {
    for b in blocks {
        if !matches!(b.kind, BlockKind::Unknown { .. }) {
            b.id.clear();
        }
        strip_ids(&mut b.children);
    }
}

fn remap_ids(blocks: &mut [Block], map: &HashMap<String, String>) {
    for b in blocks {
        if let Some(orig) = map.get(&b.id) {
            b.id = orig.clone();
        }
        remap_ids(&mut b.children, map);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn prop_roundtrip(tree in prop::collection::vec(block_tree(), 1..6), anchors in any::<bool>()) {
        let expected = normalize(tree);
        let rendered = render(&expected, RenderOpts { anchors });
        let mut parsed = parse(&rendered.markdown).map_err(|e| TestCaseError::fail(format!("{e:?}\n---\n{}", rendered.markdown)))?;
        let mut expected = expected;
        if anchors {
            let map: HashMap<_, _> = rendered.anchors.iter().cloned().collect();
            remap_ids(&mut parsed, &map);
        } else {
            strip_ids(&mut parsed);
            strip_ids(&mut expected);
        }
        prop_assert_eq!(parsed, expected, "MMD :\n{}", rendered.markdown);
    }
}
