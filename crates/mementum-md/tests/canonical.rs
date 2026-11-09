//! Forme canonique : fixtures relues octet pour octet, couverture du registre
//! et une assertion par règle de la P1.

use std::collections::BTreeSet;
use std::path::PathBuf;

use mementum_md::{
    Block, Cell, Content, Inline, MAX_WIDTH_COLUMNS, PropValue, Props, RenderOpts, Styles, Table,
    block_text, normalize, parse, render,
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
                rows: vec![vec![cell("A"), cell("B")], vec![cell("1"), cell("2")]],
            }),
            vec![],
        )],
        "| A | B |\n| --- | --- |\n| 1 | 2 |\n{header-cols=1}\n",
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

/// Ruling R16 (a) : deux liens voisins de même `href` n'en font qu'un, comme
/// dans Yjs ; la lecture refuse les deux liens écrits côte à côte.
#[test]
fn adjacent_links_merge() {
    let link = |href: &str, children: Vec<Inline>| Inline::Link {
        href: href.into(),
        children,
    };
    let bold = Inline::Text {
        text: "c".into(),
        styles: Styles {
            bold: true,
            ..Styles::default()
        },
    };
    let raw = para(vec![
        link("x", vec![text("a")]),
        link("x", vec![text("b"), bold.clone()]),
        link("y", vec![text("d")]),
    ]);
    assert_eq!(
        normalize(vec![raw]),
        [para(vec![
            link("x", vec![text("ab"), bold]),
            link("y", vec![text("d")]),
        ])]
    );
    let error = parse("[a](x)[b](x)\n").unwrap_err();
    assert_eq!(error.line, 1);
    assert!(error.message.contains("[ab](x)"), "{error}");
}

fn grid(rows: usize, cols: usize, header_rows: u32, header_cols: u32) -> Block {
    let cell = Cell {
        content: vec![text("c")],
        props: Props::new(),
    };
    blk(
        "table",
        Props::new(),
        Content::Table(Table {
            header_rows,
            header_cols,
            column_widths: vec![],
            rows: vec![vec![cell; cols]; rows],
        }),
        vec![],
    )
}

fn headers(b: &Block) -> (u32, u32) {
    match &b.content {
        Content::Table(t) => (t.header_rows, t.header_cols),
        other => panic!("tableau attendu, trouvé {other:?}"),
    }
}

/// Ruling R19 (a) : en-têtes ramenés dans la grille ; un tableau tout en
/// en-têtes vaut `(lignes, 0)`.
#[test]
fn table_headers_fit_the_grid() {
    let canonical = |b: Block| headers(&normalize(vec![b])[0]);
    assert_eq!(canonical(grid(2, 2, 5, 0)), (2, 0));
    assert_eq!(canonical(grid(2, 2, 0, 9)), (2, 0));
    assert_eq!(canonical(grid(1, 2, 1, 0)), (1, 0));
    assert_eq!(canonical(grid(1, 2, 1, 1)), (1, 0));
    assert_eq!(canonical(grid(3, 3, 1, 1)), (1, 1));
    assert_eq!(canonical(grid(3, 3, 0, 2)), (0, 2));
    assert_eq!(canonical(grid(2, 2, 0, 0)), (0, 0));
    // `header-rows=5` sur deux lignes : la lecture demande la forme canonique.
    let error = parse("| A |\n| --- |\n| 1 |\n{header-rows=5}\n").unwrap_err();
    assert_eq!(error.line, 4);
    assert!(error.message.contains("header-rows=2"), "{error}");
}

fn cell_with(t: &str, items: &[(&str, PropValue)]) -> Cell {
    Cell {
        content: vec![text(t)],
        props: props(items),
    }
}

fn widths_of(b: &Block) -> Vec<Option<f64>> {
    match &b.content {
        Content::Table(t) => t.column_widths.clone(),
        other => panic!("tableau attendu, trouvé {other:?}"),
    }
}

/// Une largeur par colonne de la grille, comme `columnWidths` de BlockNote :
/// une cellule fusionnée de la première ligne couvre `colspan` colonnes
/// (relecture de la Task 3, M7).
#[test]
fn widths_follow_the_grid() {
    let table = |rows: Vec<Vec<Cell>>, widths: Vec<Option<f64>>| {
        blk(
            "table",
            Props::new(),
            Content::Table(Table {
                header_rows: 1,
                header_cols: 0,
                column_widths: widths,
                rows,
            }),
            vec![],
        )
    };
    let merged = || {
        vec![
            vec![
                cell_with("A", &[("colspan", PropValue::Int(2))]),
                cell_with("B", &[]),
            ],
            vec![
                cell_with("C", &[("rowspan", PropValue::Int(2))]),
                cell_with("D", &[]),
                cell_with("E", &[]),
            ],
            vec![cell_with("F", &[]), cell_with("G", &[])],
        ]
    };
    same(
        vec![table(merged(), vec![Some(120.0), None, Some(80.0)])],
        "| A {colspan=2} | B |\n| --- | --- |\n| C {rowspan=2} | D | E |\n| F | G |\n\
         {widths=\"120,,80\"}\n",
    );

    // Largeurs absentes : une automatique par colonne de la grille ; celles de
    // trop sont retirées, celles qui manquent sont automatiques.
    let canonical = |rows, widths| widths_of(&normalize(vec![table(rows, widths)])[0]);
    assert_eq!(canonical(merged(), vec![]), [None, None, None]);
    assert_eq!(
        canonical(merged(), vec![Some(1.0), Some(2.0), Some(3.0), Some(4.0)]),
        [Some(1.0), Some(2.0), Some(3.0)]
    );
    assert_eq!(
        canonical(merged(), vec![Some(1.0)]),
        [Some(1.0), None, None]
    );
    // Un `colspan` hors de toute mesure ne fait pas écrire des millions de
    // largeurs : au plus `MAX_WIDTH_COLUMNS`.
    let huge = vec![vec![cell_with(
        "A",
        &[("colspan", PropValue::Int(i64::MAX))],
    )]];
    assert_eq!(canonical(huge, vec![Some(1.0)]).len(), MAX_WIDTH_COLUMNS);
    let negative = vec![vec![cell_with("A", &[("colspan", PropValue::Int(-3))])]];
    assert_eq!(canonical(negative, vec![]), [None]);

    // La lecture demande la forme canonique : une largeur par colonne de la grille.
    let error = parse("| A | B |\n| --- | --- |\n| 1 | 2 |\n{widths=\"1,2,3\"}\n").unwrap_err();
    assert_eq!(error.line, 4);
    assert!(error.message.contains("{widths=\"1,2\"}"), "{error}");
    let error = parse("| A {colspan=2} |\n| --- |\n| 1 | 2 |\n{widths=\"120\"}\n").unwrap_err();
    assert_eq!(error.line, 4);
    assert!(error.message.contains("{widths=\"120,\"}"), "{error}");
}

/// Ruling R19 (b) : le contenu d'un bloc de code est du texte brut.
#[test]
fn code_block_is_plain_text() {
    let raw = blk(
        "codeBlock",
        Props::new(),
        Content::Inline(vec![
            Inline::Text {
                text: "let a".into(),
                styles: Styles {
                    bold: true,
                    ..Styles::default()
                },
            },
            Inline::HardBreak,
            Inline::Link {
                href: "x".into(),
                children: vec![text("b")],
            },
            Inline::Mention {
                kind: "user".into(),
                id: "u1".into(),
                label: "L".into(),
            },
            Inline::Math { latex: "y".into() },
        ]),
        vec![],
    );
    assert_eq!(
        normalize(vec![raw]),
        [blk(
            "codeBlock",
            Props::new(),
            Content::Inline(vec![text("let a\nb")]),
            vec![],
        )]
    );
    let only_mention = blk(
        "codeBlock",
        Props::new(),
        Content::Inline(vec![Inline::Math { latex: "y".into() }]),
        vec![],
    );
    assert_eq!(
        normalize(vec![only_mention])[0].content,
        Content::Inline(vec![])
    );
}

/// Ordre d'imbrication des marques, documenté au §6.2 : `<code>`, `<em>`,
/// `<strong>`, `<s>`, `<u>`, puis `<span>`, de l'intérieur vers l'extérieur ;
/// un délimiteur Markdown prend la place de sa balise ; un lien entoure ses
/// textes marqués.
#[test]
fn tag_mark_order() {
    let styled = |t: &str, styles: Styles| Inline::Text {
        text: t.into(),
        styles,
    };
    let all = Styles {
        bold: true,
        italic: true,
        underline: true,
        strike: true,
        code: true,
        text_color: Some("blue".into()),
        background_color: Some("yellow".into()),
    };
    same(
        vec![para(vec![text("x "), styled("a", all), text(" y")])],
        "x <span bg=\"yellow\" color=\"blue\"><u><s><strong><em><code>a</code></em></strong></s></u></span> y\n",
    );
    let underline_color = Styles {
        underline: true,
        text_color: Some("blue".into()),
        ..Styles::default()
    };
    same(
        vec![para(vec![styled("souligné coloré", underline_color)])],
        "<span color=\"blue\"><u>souligné coloré</u></span>\n",
    );
    let bold_underline = Styles {
        bold: true,
        underline: true,
        ..Styles::default()
    };
    same(
        vec![para(vec![styled("gras souligné", bold_underline)])],
        "<u>**gras souligné**</u>\n",
    );
    let bold = Styles {
        bold: true,
        ..Styles::default()
    };
    same(
        vec![para(vec![Inline::Link {
            href: "x".into(),
            children: vec![styled("a", bold)],
        }])],
        "[**a**](x)\n",
    );
}
