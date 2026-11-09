//! Aller-retour arbre → MMD → arbre sur des arbres aléatoires (`arb::tree`).

use std::collections::BTreeSet;

use mementum_md::{Block, Content, Inline, PropValue, RenderOpts, arb, parse, render};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

/// 512 cas, ou `PROPTEST_CASES` s'il est donné.
fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(512)
}

/// Remplace les alias `bN` relus par les ids d'origine.
fn unalias(blocks: &mut [Block], aliases: &std::collections::BTreeMap<String, String>) {
    for b in blocks {
        if let Some(id) = aliases.get(&b.id) {
            b.id = id.clone();
        }
        unalias(&mut b.children, aliases);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    #[test]
    fn prop_roundtrip(t in arb::tree()) {
        let md = render(&t, RenderOpts { anchors: false }).markdown;
        prop_assert_eq!(parse(&md), Ok(t.clone()), "MMD :\n{}", md);

        // Avec les ancres, chaque alias relu désigne le bloc d'origine.
        let r = render(&t, RenderOpts { anchors: true });
        let mut again = parse(&r.markdown)
            .map_err(|e| TestCaseError::fail(format!("{e}\nMMD :\n{}", r.markdown)))?;
        unalias(&mut again, &r.aliases);
        prop_assert_eq!(again, t, "MMD :\n{}", r.markdown);
    }
}

// ------------------------------------------- couverture du générateur

/// Caractères tapés qui piègent le Markdown (revue, point 3).
const SPECIAL: &str = "*_\\<>&{}$|^[]~";
/// Caractères de `href` qui imposent `<a href="…">`.
const HREF_CHARS: [char; 4] = [' ', ')', '&', '\\'];

fn texts(content: &[Inline], out: &mut Vec<String>) {
    for i in content {
        match i {
            Inline::Text { text, .. } => out.push(text.clone()),
            Inline::Link { children, .. } => texts(children, out),
            Inline::Mention { label, .. } => out.push(label.clone()),
            _ => {}
        }
    }
}

fn styled(i: Option<&Inline>) -> bool {
    matches!(i, Some(Inline::Text { styles, .. }) if !styles.is_plain())
}

/// Note les traits d'un contenu en ligne.
fn inline_features(content: &[Inline], seen: &mut BTreeSet<String>) {
    for (k, i) in content.iter().enumerate() {
        let near_styled =
            styled(k.checked_sub(1).and_then(|p| content.get(p))) || styled(content.get(k + 1));
        match i {
            Inline::HardBreak => {
                seen.insert("hardBreak".into());
            }
            Inline::Link { href, .. } => {
                for c in HREF_CHARS {
                    if href.contains(c) {
                        seen.insert(format!("href {c:?}"));
                    }
                }
            }
            Inline::Mention { .. } if near_styled => {
                seen.insert("mention collée à du texte stylé".into());
            }
            Inline::Math { .. } if near_styled => {
                seen.insert("formule collée à du texte stylé".into());
            }
            Inline::Text { text, styles } if styles.code && text.contains('`') => {
                seen.insert("code en ligne avec backtick".into());
            }
            _ => {}
        }
    }
    let mut all = Vec::new();
    texts(content, &mut all);
    for t in all {
        for c in SPECIAL.chars() {
            if t.contains(c) {
                seen.insert(format!("caractère {c}"));
            }
        }
        for (name, sample) in [
            ("emoji", "🚀"),
            ("accent", "é"),
            ("droite à gauche", "שלום"),
        ] {
            if t.contains(sample) {
                seen.insert(name.into());
            }
        }
    }
}

fn features(blocks: &[Block], seen: &mut BTreeSet<String>) {
    let mut prev_numbered = false;
    for b in blocks {
        seen.insert(format!("type {}", b.kind));
        if !b.children.is_empty() {
            seen.insert(format!("enfants sous {}", b.kind));
        }
        let spec = mementum_schema::block(&b.kind).expect("type du registre");
        for name in b.props.keys() {
            if spec.props.iter().any(|p| p.name == name) {
                seen.insert("prop hors défaut".into());
            } else {
                seen.insert("prop inconnue".into());
            }
        }
        let prop = |name: &str| b.props.get(name);
        match (b.kind.as_str(), &b.content) {
            ("heading", _) if matches!(prop("level"), Some(PropValue::Int(5 | 6))) => {
                seen.insert("titre de niveau 5 ou 6".into());
            }
            ("numberedListItem", _) if prev_numbered && prop("start").is_some() => {
                seen.insert("start hors du premier élément".into());
            }
            ("codeBlock", Content::Inline(c)) => {
                if matches!(prop("language"), Some(PropValue::Str(l)) if l.contains(' ')) {
                    seen.insert("langue avec une espace".into());
                }
                let mut t = Vec::new();
                texts(c, &mut t);
                if t.iter().any(|t| t.contains("```")) {
                    seen.insert("bloc de code avec backticks".into());
                }
            }
            ("image", _) if matches!(prop("url"), Some(PropValue::Str(u)) if u.contains(' ')) => {
                seen.insert("url d'image avec une espace".into());
            }
            ("equation", _) if matches!(prop("latex"), Some(PropValue::Str(l)) if l.split('\n').any(|l| l == "$$")) =>
            {
                seen.insert("équation avec une ligne $$".into());
            }
            ("callout", Content::Inline(c))
                if c.is_empty() && b.children.first().is_some_and(|f| f.kind == "paragraph") =>
            {
                seen.insert("callout vide suivi d'un paragraphe".into());
            }
            ("unknown", _) if prop("foreign") == Some(&PropValue::Bool(true)) => {
                seen.insert("nœud étranger".into());
            }
            ("table", Content::Table(t)) => {
                if t.header_rows != 1 {
                    seen.insert("header-rows".into());
                }
                if t.header_cols != 0 {
                    seen.insert("header-cols".into());
                }
                if t.column_widths.iter().any(Option::is_some) {
                    seen.insert("widths".into());
                }
                for cell in t.rows.iter().flatten() {
                    if !cell.props.is_empty() {
                        seen.insert("props de cellule".into());
                    }
                    inline_features(&cell.content, seen);
                }
            }
            _ => {}
        }
        if matches!(prop("previewWidth"), Some(PropValue::Float(f)) if f.fract() == 0.0) {
            seen.insert("largeur d'aperçu entière".into());
        }
        if let Content::Inline(c) = &b.content {
            inline_features(c, seen);
        }
        features(&b.children, seen);
        prev_numbered = b.kind == "numberedListItem";
    }
}

/// Structure représentable par BlockNote : une colonne n'existe que dans un
/// `columnList`, qui ne contient que 2 colonnes ou plus.
fn assert_columns_placed(blocks: &[Block], in_list: bool) {
    for b in blocks {
        assert_eq!(b.kind == "column", in_list, "colonne mal placée : {b:?}");
        if b.kind == "columnList" {
            assert!(b.children.len() >= 2, "columnList de moins de 2 colonnes");
        }
        if b.kind == "column" {
            assert!(!b.children.is_empty(), "colonne vide");
            assert!(
                b.children.iter().all(|c| c.kind != "columnList"),
                "columnList directement dans une colonne"
            );
        }
        assert_columns_placed(&b.children, b.kind == "columnList");
    }
}

/// Le générateur produit bien tout ce que le brief de la Task 2 demande, dans
/// une structure que BlockNote sait représenter.
#[test]
fn generator_covers_brief() {
    let mut runner = TestRunner::deterministic();
    let strategy = arb::tree();
    let mut seen = BTreeSet::new();
    for _ in 0..1000 {
        let tree = strategy.new_tree(&mut runner).unwrap().current();
        assert_columns_placed(&tree, false);
        features(&tree, &mut seen);
        if render(&tree, RenderOpts { anchors: false })
            .markdown
            .contains("<math latex=")
        {
            seen.insert("formule en <math>".into());
        }
    }
    let mut expected: Vec<String> = Vec::new();
    for spec in mementum_schema::blocks() {
        expected.push(format!("type {}", spec.name));
        expected.push(format!("enfants sous {}", spec.name));
    }
    for c in SPECIAL.chars() {
        expected.push(format!("caractère {c}"));
    }
    for c in HREF_CHARS {
        expected.push(format!("href {c:?}"));
    }
    for name in [
        "prop hors défaut",
        "prop inconnue",
        "hardBreak",
        "code en ligne avec backtick",
        "bloc de code avec backticks",
        "mention collée à du texte stylé",
        "formule collée à du texte stylé",
        "callout vide suivi d'un paragraphe",
        "emoji",
        "accent",
        "droite à gauche",
        "titre de niveau 5 ou 6",
        "start hors du premier élément",
        "langue avec une espace",
        "url d'image avec une espace",
        "équation avec une ligne $$",
        "nœud étranger",
        "header-rows",
        "header-cols",
        "widths",
        "formule en <math>",
        "props de cellule",
        "largeur d'aperçu entière",
    ] {
        expected.push(name.into());
    }
    let missing: Vec<&String> = expected.iter().filter(|e| !seen.contains(*e)).collect();
    assert!(missing.is_empty(), "absents du générateur : {missing:?}");
}
