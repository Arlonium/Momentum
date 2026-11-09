//! Forme canonique : fixtures relues octet pour octet, couverture du registre
//! et une assertion par règle de la P1.

use std::collections::BTreeSet;
use std::path::PathBuf;

use mementum_md::{
    Block, Cell, Content, Inline, PropValue, Props, RenderOpts, Styles, Table, block_text,
    normalize, parse, render,
};

fn plain() -> RenderOpts {
    RenderOpts { anchors: false }
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/mmd/fixtures")
}

fn fixture(name: &str) -> String {
    let path = fixtures_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} : {e}", path.display()))
}

#[test]
fn fixtures_are_canonical() {
    let mut seen = 0;
    for entry in std::fs::read_dir(fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("mmd") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let blocks = parse(&src).unwrap_or_else(|e| panic!("{} : {e}", path.display()));
        assert_eq!(
            render(&blocks, plain()).markdown,
            src,
            "{} n'est pas canonique",
            path.display()
        );
        seen += 1;
    }
    assert!(seen >= 2, "fixtures MMD introuvables");
}

fn collect_kinds(blocks: &[Block], out: &mut BTreeSet<String>) {
    for b in blocks {
        out.insert(b.kind.clone());
        collect_kinds(&b.children, out);
    }
}

#[test]
fn edge_cases_cover_registry() {
    let mut kinds = BTreeSet::new();
    for name in ["p0-all-blocks.mmd", "p1-edge-cases.mmd"] {
        let blocks = parse(&fixture(name)).unwrap_or_else(|e| panic!("{name} : {e}"));
        collect_kinds(&blocks, &mut kinds);
    }
    for spec in mementum_schema::blocks() {
        assert!(
            kinds.contains(spec.name),
            "type absent des fixtures : {}",
            spec.name
        );
    }
}

// ------------------------------------------------------------- p1_forms

fn text(s: &str) -> Inline {
    Inline::Text {
        text: s.into(),
        styles: Styles::default(),
    }
}

fn props(items: &[(&str, PropValue)]) -> Props {
    items
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn s(v: &str) -> PropValue {
    PropValue::Str(v.into())
}

fn blk(kind: &str, p: Props, content: Content, children: Vec<Block>) -> Block {
    Block {
        id: String::new(),
        kind: kind.into(),
        props: p,
        content,
        children,
    }
}

fn para(content: Vec<Inline>) -> Block {
    blk("paragraph", Props::new(), Content::Inline(content), vec![])
}

/// La forme `mmd` est le rendu de `blocks`, et se relit en `blocks`.
#[track_caller]
fn same(blocks: Vec<Block>, mmd: &str) {
    assert_eq!(render(&blocks, plain()).markdown, mmd, "rendu");
    assert_eq!(parse(mmd), Ok(blocks), "lecture de {mmd:?}");
}

#[test]
fn p1_forms() {
    // Paragraphe avec un paragraphe enfant.
    let mut parent = para(vec![text("Parent")]);
    parent.children = vec![para(vec![text("Enfant")])];
    same(vec![parent], "Parent\n  Enfant\n");

    // Saut de ligne.
    same(
        vec![para(vec![text("a"), Inline::HardBreak, text("b")])],
        "a<br/>b\n",
    );

    // Props hors forme.
    same(
        vec![blk(
            "paragraph",
            props(&[("textAlignment", s("center")), ("textColor", s("red"))]),
            Content::Inline(vec![text("Texte")]),
            vec![],
        )],
        "Texte {align=\"center\" color=\"red\"}\n",
    );

    // Titre repliable.
    same(
        vec![blk(
            "heading",
            props(&[
                ("level", PropValue::Int(2)),
                ("isToggleable", PropValue::Bool(true)),
            ]),
            Content::Inline(vec![text("Titre")]),
            vec![],
        )],
        "## Titre {toggle=true}\n",
    );

    // Liste numérotée qui commence à 3.
    same(
        vec![
            blk(
                "numberedListItem",
                props(&[("start", PropValue::Int(3))]),
                Content::Inline(vec![text("Trois")]),
                vec![],
            ),
            blk(
                "numberedListItem",
                Props::new(),
                Content::Inline(vec![text("Quatre")]),
                vec![],
            ),
        ],
        "3. Trois\n4. Quatre\n",
    );

    // Accolades échappées.
    same(vec![para(vec![text("Prix {x}")])], "Prix \\{x\\}\n");

    // Lien dont le href contient une espace.
    same(
        vec![para(vec![Inline::Link {
            href: "https://ex.com/a b".into(),
            children: vec![text("lien")],
        }])],
        "<a href=\"https://ex.com/a b\">lien</a>\n",
    );

    // Callout vide suivi d'un paragraphe enfant.
    same(
        vec![blk(
            "callout",
            props(&[("color", s("blue"))]),
            Content::Inline(vec![]),
            vec![para(vec![text("Suite.")])],
        )],
        "<callout icon=\"💡\" color=\"blue\">\n<p/>\nSuite.\n</callout>\n",
    );

    // Tableau sans en-tête, colonne de 120.
    let cell = |t: &str| Cell {
        content: vec![text(t)],
        props: Props::new(),
    };
    same(
        vec![blk(
            "table",
            Props::new(),
            Content::Table(Table {
                header_rows: 0,
                header_cols: 0,
                column_widths: vec![Some(120.0)],
                rows: vec![vec![cell("A")], vec![cell("1")]],
            }),
            vec![],
        )],
        "| A |\n| --- |\n| 1 |\n{header-rows=0 widths=\"120\"}\n",
    );

    // Colonne d'en-tête (R11) : `header-cols`, omis à 0, trié avec les autres clés.
    same(
        vec![blk(
            "table",
            Props::new(),
            Content::Table(Table {
                header_rows: 1,
                header_cols: 1,
                column_widths: vec![None, None],
                rows: vec![vec![cell("A"), cell("B")]],
            }),
            vec![],
        )],
        "| A | B |\n| --- | --- |\n{header-cols=1}\n",
    );

    // Formule en ligne sans forme `$…$` (R8) : `<math latex="…"/>`.
    let math = |latex: &str| Inline::Math {
        latex: latex.into(),
    };
    same(
        vec![para(vec![
            math("x^2"),
            text(" puis "),
            math(""),
            text(", "),
            math(" x"),
            text(" et "),
            math("a$$b"),
        ])],
        "$x^2$ puis <math latex=\"\"/>, <math latex=\" x\"/> et <math latex=\"a$$b\"/>\n",
    );

    // Surlignage.
    same(
        vec![para(vec![
            text("Fond "),
            Inline::Text {
                text: "jaune".into(),
                styles: Styles {
                    background_color: Some("yellow".into()),
                    ..Styles::default()
                },
            },
        ])],
        "Fond <span bg=\"yellow\">jaune</span>\n",
    );

    // Props inconnues du registre, relues avec leur type (clés triées, R3).
    same(
        vec![blk(
            "paragraph",
            props(&[("foo", PropValue::Int(1)), ("bar", s("x"))]),
            Content::Inline(vec![text("Texte")]),
            vec![],
        )],
        "Texte {bar=\"x\" foo=1}\n",
    );

    // Nœud étranger (R4) : `foreign` écrit seulement quand il vaut vrai.
    same(
        vec![Block {
            id: "b_x".into(),
            ..blk(
                "unknown",
                props(&[
                    ("originalType", s("futureWidget")),
                    ("data", s("e30=")),
                    ("foreign", PropValue::Bool(true)),
                ]),
                Content::None,
                vec![],
            )
        }],
        "<block type=\"futureWidget\" id=\"b_x\" data=\"e30=\" foreign=true/>\n",
    );

    // Largeur d'aperçu entière : `width=320`, jamais `320.0`.
    same(
        vec![blk(
            "video",
            props(&[
                ("url", s("https://ex.com/v.mp4")),
                ("previewWidth", PropValue::Float(320.0)),
            ]),
            Content::None,
            vec![],
        )],
        "<video src=\"https://ex.com/v.mp4\" width=320/>\n",
    );
}

#[test]
fn anchors_give_aliases() {
    let blocks = vec![
        Block {
            id: "id-a".into(),
            ..para(vec![text("Un")])
        },
        Block {
            id: "id-b".into(),
            ..blk(
                "callout",
                Props::new(),
                Content::Inline(vec![text("Deux")]),
                vec![],
            )
        },
    ];
    let r = render(&blocks, RenderOpts { anchors: true });
    assert_eq!(
        r.markdown,
        "Un ^b1\n\n<callout id=\"b2\" icon=\"💡\" color=\"default\">\nDeux\n</callout>\n"
    );
    assert_eq!(r.aliases.get("b1").map(String::as_str), Some("id-a"));
    assert_eq!(r.aliases.get("b2").map(String::as_str), Some("id-b"));
    let again = parse(&r.markdown).unwrap();
    assert_eq!(again[0].id, "b1");
    assert_eq!(again[1].id, "b2");
}

#[test]
fn normalize_and_block_text() {
    let bold = Styles {
        bold: true,
        ..Styles::default()
    };
    let raw = blk(
        "heading",
        props(&[
            ("level", PropValue::Int(1)),
            ("textAlignment", s("left")),
            ("textColor", s("red")),
        ]),
        Content::Inline(vec![
            text("a"),
            text(""),
            text("b"),
            Inline::Text {
                text: "c".into(),
                styles: bold.clone(),
            },
            Inline::Mention {
                kind: "user".into(),
                id: "u1".into(),
                label: "Lenny".into(),
            },
            Inline::HardBreak,
            Inline::Link {
                href: "https://ex.com".into(),
                children: vec![text("d"), text("e")],
            },
            Inline::Math { latex: "x".into() },
        ]),
        vec![],
    );
    let normalized = normalize(vec![raw]).remove(0);
    // Défauts du registre (niveau 1, alignement à gauche) retirés.
    assert_eq!(normalized.props, props(&[("textColor", s("red"))]));
    assert_eq!(
        normalized.content,
        Content::Inline(vec![
            text("ab"),
            Inline::Text {
                text: "c".into(),
                styles: bold,
            },
            Inline::Mention {
                kind: "user".into(),
                id: "u1".into(),
                label: "Lenny".into(),
            },
            Inline::HardBreak,
            Inline::Link {
                href: "https://ex.com".into(),
                children: vec![text("de")],
            },
            Inline::Math { latex: "x".into() },
        ])
    );
    assert_eq!(block_text(&normalized), "abc\u{FFFC}\nde\u{FFFC}");
}
