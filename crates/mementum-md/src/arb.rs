//! Générateur proptest d'arbres de blocs (feature `arbitrary`), partagé avec
//! les tests des autres crates.
//!
//! Les arbres produits sont canoniques (passés par [`normalize`]) et sans id,
//! sauf les blocs `unknown`, dont la forme MMD écrit toujours l'id. Ils
//! couvrent :
//! - tous les types du registre, des enfants sous tout type, des props hors
//!   défaut (dont des valeurs que la forme ne peut pas porter : titre de
//!   niveau 5, `start` hors du premier élément, langue avec une espace…) et
//!   des props inconnues du registre ;
//! - des sauts de ligne, des liens dont le `href` contient une espace, `)`,
//!   `&` ou `\`, du code avec des suites de backticks, des mentions et des
//!   formules collées à du texte stylé, des formules qui n'ont pas de forme
//!   `$…$` (`<math latex="…"/>`), un callout vide suivi d'un paragraphe
//!   enfant ;
//! - les caractères `* _ \ < > & { } $ | ^ [ ] ~`, des emoji, des accents et
//!   du texte de droite à gauche ;
//! - des tableaux à cellules fusionnées (`colspan` sur la première ligne,
//!   `rowspan` sur la deuxième) dans une grille que BlockNote sait écrire,
//!   avec une largeur par colonne de la grille (relecture de la Task 3, M7).
//!
//! Ils restent dans ce que BlockNote sait représenter : une colonne n'existe
//! que dans un `columnList` (de 2 ou 3 colonnes, chacune avec au moins un
//! bloc et sans `columnList` directement dedans) ; un tableau a le même
//! nombre de cellules par ligne et une largeur par colonne, ou forme une
//! grille de cellules fusionnées (Ruling R37 : `parse` refuse toute autre
//! fusion). Le reste
//! (liens voisins de même `href`, en-têtes hors de la grille, marques dans
//! un bloc de code) est ramené à sa forme canonique par [`normalize`].

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mementum_schema::{BlockSpec, ContentKind, PropSpec, PropType, block, blocks, cell_props};
use proptest::prelude::*;
use proptest::strategy::{BoxedStrategy, Union};

use crate::{Block, Cell, Content, Inline, PropValue, Props, Styles, Table, normalize};

/// Arbres de blocs canoniques, de 0 à 4 blocs au premier niveau.
pub fn tree() -> impl Strategy<Value = Vec<Block>> {
    prop::collection::vec(block_tree(), 0..5).prop_map(normalize)
}

fn block_tree() -> BoxedStrategy<Block> {
    leaf()
        .prop_recursive(3, 32, 3, |inner| {
            prop_oneof![
                5 => (leaf(), prop::collection::vec(inner.clone(), 1..4)).prop_map(
                    |(mut b, children)| {
                        b.children = children;
                        b
                    }
                ),
                1 => (props_of(spec("callout")), text(), prop::collection::vec(inner.clone(), 0..2))
                    .prop_map(|(props, first, rest)| empty_callout(props, first, rest)),
                1 => column_list(inner),
            ]
        })
        .boxed()
}

fn spec(name: &str) -> &'static BlockSpec {
    block(name).expect("type du registre")
}

/// `columnList` de 2 ou 3 colonnes, chacune avec 1 ou 2 blocs, dont aucun
/// `columnList` (BlockNote 0.57 l'exclut des enfants d'une colonne).
fn column_list(inner: BoxedStrategy<Block>) -> BoxedStrategy<Block> {
    let child = inner.prop_filter("pas de columnList dans une colonne", |b| {
        b.kind != "columnList"
    });
    let column = (block_of(spec("column")), prop::collection::vec(child, 1..3)).prop_map(
        |(mut column, children)| {
            column.children = children;
            column
        },
    );
    (
        block_of(spec("columnList")),
        prop::collection::vec(column, 2..4),
    )
        .prop_map(|(mut list, columns)| {
            list.children = columns;
            list
        })
        .boxed()
}

/// Callout sans contenu dont le premier enfant est un paragraphe (`<p/>` en tête).
fn empty_callout(props: Props, first: String, rest: Vec<Block>) -> Block {
    let paragraph = Block {
        id: String::new(),
        kind: "paragraph".into(),
        props: Props::new(),
        content: Content::Inline(vec![plain(first)]),
        children: vec![],
    };
    let mut children = vec![paragraph];
    children.extend(rest);
    Block {
        id: String::new(),
        kind: "callout".into(),
        props,
        content: Content::Inline(vec![]),
        children,
    }
}

fn plain(text: String) -> Inline {
    Inline::Text {
        text,
        styles: Styles::default(),
    }
}

/// Un bloc sans enfant, de n'importe quel type du registre sauf les colonnes
/// (voir [`column_list`]).
fn leaf() -> BoxedStrategy<Block> {
    let kinds: Vec<(u32, BoxedStrategy<Block>)> = blocks()
        .iter()
        .filter(|spec| !matches!(spec.name, "column" | "columnList"))
        .map(|spec| {
            // Les éléments de liste reviennent plus souvent, pour former des listes.
            let weight = match spec.name {
                "bulletListItem" | "numberedListItem" | "checkListItem" => 3,
                _ => 1,
            };
            (weight, block_of(spec))
        })
        .collect();
    Union::new_weighted(kinds).boxed()
}

fn block_of(spec: &'static BlockSpec) -> BoxedStrategy<Block> {
    (props_of(spec), content_of(spec), unknown_id())
        .prop_map(move |(props, content, id)| {
            let mut b = Block {
                id: String::new(),
                kind: spec.name.to_string(),
                props,
                content,
                children: vec![],
            };
            if spec.name == "unknown" {
                fix_unknown(&mut b, id);
            }
            b
        })
        .boxed()
}

/// Un bloc `unknown` garde son id. Étranger (`foreign`), il porte le JSON
/// base64 d'un vrai nœud Yjs, que `mementum-crdt` sait recréer.
fn fix_unknown(b: &mut Block, id: String) {
    b.id = id;
    if b.props.get("foreign") == Some(&PropValue::Bool(true)) {
        let data = serde_json::json!({
            "attrs": { "a": "1" },
            "content": [],
            "children": [],
        });
        b.props
            .insert("originalType".into(), PropValue::Str("futureWidget".into()));
        b.props.insert(
            "data".into(),
            PropValue::Str(STANDARD.encode(data.to_string())),
        );
    }
}

fn unknown_id() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "",
        "b_unknown01",
        "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
        "id \"bizarre\" & <x>",
    ])
    .prop_map(String::from)
}

// ------------------------------------------------------------------ props

/// Props d'un bloc : chaque prop du registre est présente une fois sur trois,
/// avec parfois une prop inconnue du registre (« sac de props »).
fn props_of(spec: &'static BlockSpec) -> BoxedStrategy<Props> {
    props_from(spec.props)
}

fn props_from(specs: &'static [PropSpec]) -> BoxedStrategy<Props> {
    let known: Vec<BoxedStrategy<Option<(String, PropValue)>>> = specs
        .iter()
        .map(|p| {
            prop::option::weighted(
                0.35,
                value_for(p).prop_map(move |v| (p.name.to_string(), v)),
            )
            .boxed()
        })
        .collect();
    let bag = prop::option::weighted(0.2, (bag_name(), any_value()));
    (known, bag)
        .prop_map(|(known, bag)| known.into_iter().flatten().chain(bag).collect())
        .boxed()
}

/// Noms de props inconnues : aucun ne recoupe un nom MMD du registre.
fn bag_name() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["fooBar", "zLevel", "aaFlag", "customProp"]).prop_map(String::from)
}

fn any_value() -> BoxedStrategy<PropValue> {
    prop_oneof![
        tricky_string().prop_map(PropValue::Str),
        int_value().prop_map(PropValue::Int),
        float_value().prop_map(PropValue::Float),
        any::<bool>().prop_map(PropValue::Bool),
    ]
    .boxed()
}

fn value_for(p: &PropSpec) -> BoxedStrategy<PropValue> {
    match p.ty {
        PropType::String => match p.name {
            "url" => select(&[
                "https://mementum.dev/logo.png",
                "https://ex.com/a b.png",
                "https://ex.com/a)b",
                "https://ex.com/?a=1&b=2",
                "C:\\images\\x.png",
                "",
            ]),
            "language" => select(&["rust", "c++", "text", "mon langage", "{x}", "a`b", ""]),
            "latex" => select(&[
                "\\int_0^1 x^2 \\, dx",
                "\\frac{1}{3}",
                "a\n\n  b",
                "x\n$$\ny",
                " x ",
                "",
            ]),
            _ => tricky_string(),
        }
        .prop_map(PropValue::Str)
        .boxed(),
        PropType::Int => match p.name {
            "level" => (1i64..=6).boxed(),
            _ => int_value().boxed(),
        }
        .prop_map(PropValue::Int)
        .boxed(),
        PropType::Float => float_value().prop_map(PropValue::Float).boxed(),
        PropType::Bool => any::<bool>().prop_map(PropValue::Bool).boxed(),
    }
}

fn select(values: &'static [&'static str]) -> BoxedStrategy<String> {
    prop::sample::select(values).prop_map(String::from).boxed()
}

fn tricky_string() -> BoxedStrategy<String> {
    prop_oneof![
        2 => select(&["red", "blue", "center", "right", "yellow", "⚠️", "💡"]),
        1 => select(&[
            "",
            "a b",
            "x\"y",
            "a&b<c>d",
            "é🚀",
            "{a}",
            "x|y",
            "ligne\ncoupée",
            "$x$",
            "\\",
            "^b3",
            "1. x",
            "# h",
            "שלום",
            "--",
            "`code`",
            "&#32;",
        ]),
    ]
    .boxed()
}

fn int_value() -> impl Strategy<Value = i64> {
    prop::sample::select(vec![0, 1, 2, 3, 9, 10, 99, -2, 999_999_999, 1_000_000_000])
}

/// Flottants : des valeurs entières (écrites `320`, jamais `320.0`) et non entières.
fn float_value() -> impl Strategy<Value = f64> {
    prop::sample::select(vec![320.0, 320.5, 0.25, 1.0, 2.0, 1e21, -3.0])
}

// ---------------------------------------------------------------- contenu

fn content_of(spec: &'static BlockSpec) -> BoxedStrategy<Content> {
    match spec.content {
        ContentKind::None => Just(Content::None).boxed(),
        ContentKind::Table => prop_oneof![3 => table(), 1 => merged_table()].boxed(),
        ContentKind::Inline if spec.name == "codeBlock" => code_text()
            .prop_map(|text| {
                Content::Inline(if text.is_empty() {
                    vec![]
                } else {
                    vec![plain(text)]
                })
            })
            .boxed(),
        ContentKind::Inline => inline_content().prop_map(Content::Inline).boxed(),
    }
}

/// Texte d'un bloc de code : suites de backticks, lignes vides, indentation,
/// lignes qui ressemblent à des clôtures ou à des balises.
fn code_text() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "let x = 1;",
            "a`b",
            "``x``",
            "```",
            "````",
            "",
            "  indenté",
            "fin  ",
            "</callout>",
            "$$",
            "---",
            "^b3",
            "\ttab",
            "{x}",
            "| a |",
            "    code",
        ]),
        0..4,
    )
    .prop_map(|lines| lines.join("\n"))
}

fn inline_content() -> impl Strategy<Value = Vec<Inline>> {
    prop::collection::vec(inline(), 0..5)
}

fn inline() -> BoxedStrategy<Inline> {
    prop_oneof![
        6 => (text(), styles()).prop_map(|(text, styles)| Inline::Text { text, styles }),
        1 => (href(), prop::collection::vec((text(), styles()), 1..3)).prop_map(
            |(href, parts)| Inline::Link {
                href,
                children: parts
                    .into_iter()
                    .map(|(text, styles)| Inline::Text { text, styles })
                    .collect(),
            }
        ),
        1 => (
            select(&["page", "user", "date"]),
            prop_oneof![
                "[a-z0-9_]{1,8}".boxed(),
                select(&["a\"b", "x&y", "2026-12-01"]),
            ],
            text(),
        )
            .prop_map(|(kind, id, label)| Inline::Mention { kind, id, label }),
        1 => latex().prop_map(|latex| Inline::Math { latex }),
        1 => Just(Inline::HardBreak),
    ]
    .boxed()
}

/// Texte libre : mots accentués, emoji, texte de droite à gauche et
/// caractères qui piègent le Markdown.
fn text() -> BoxedStrategy<String> {
    prop::collection::vec(
        prop_oneof![
            3 => "[a-zA-Z0-9éàçÉ]{1,6}".boxed(),
            2 => select(&[
                "*", "_", "\\", "<", ">", "&", "{", "}", "$", "|", "^", "[", "]", "~", "!", "`",
                "#", " ", "- ", "1. ", "^b3", "&#32;", "<br/>", "{a=1}", "](x)", "**", "🚀",
                "👍🏽", "שלום", "مرحبا", "\u{a0}", "\t", "\n", "=", "---", "\"", "'",
            ]),
        ],
        1..5,
    )
    .prop_map(|parts| parts.concat())
    .boxed()
}

fn styles() -> impl Strategy<Value = Styles> {
    (
        prop::bool::weighted(0.2),
        prop::bool::weighted(0.2),
        prop::bool::weighted(0.2),
        prop::bool::weighted(0.2),
        prop::bool::weighted(0.2),
        prop::option::weighted(0.15, select(&["red", "blue", "a\"b&c"])),
        prop::option::weighted(0.15, select(&["yellow", "gray"])),
    )
        .prop_map(
            |(bold, italic, underline, strike, code, text_color, background_color)| Styles {
                bold,
                italic,
                underline,
                strike,
                code,
                text_color,
                background_color,
            },
        )
}

/// Adresses de lien, dont celles qui imposent `<a href="…">`.
fn href() -> BoxedStrategy<String> {
    prop_oneof![
        "https://ex\\.com/[a-z]{1,6}".boxed(),
        select(&[
            "https://ex.com/a b",
            "https://ex.com/a)b",
            "https://ex.com/(x)",
            "https://ex.com/?a=1&b=2",
            "C:\\dossier\\f.txt",
            "",
            "https://ex.com/{x}|$y`z",
            "https://ex.com/é#ancre",
            "mailto:a@b.c",
        ]),
    ]
    .boxed()
}

/// LaTeX en ligne : surtout des formules sûres en `$…$`, et des formules qui
/// imposent `<math latex="…"/>` (vide, blanc en bord, accolade seule, saut
/// de ligne, `$`, `$$`, backtick, barre oblique finale).
fn latex() -> BoxedStrategy<String> {
    prop_oneof![
        3 => prop::collection::vec(
            prop::sample::select(vec![
                "x",
                "y^2",
                "\\frac{a}{b}",
                "a_{i}",
                "\\{x\\}",
                "|x|",
                "a<b",
                "\\alpha",
                "{x}",
                "E = mc^2",
                "\\$5",
                "a`b",
            ]),
            1..3,
        )
        .prop_map(|parts| parts.join(" ")),
        1 => select(&[
            "", " x", "x ", "{", "}", "x\ny", "$", "$$", "a$$b", "a$b", "`", "a\\", "x}{",
        ]),
    ]
    .boxed()
}

fn table() -> impl Strategy<Value = Content> {
    (1usize..4, 1usize..4)
        .prop_flat_map(|(nrows, ncols)| {
            let widths = prop_oneof![
                Just(vec![None; ncols]).boxed(),
                prop::collection::vec(width(), ncols).boxed(),
            ];
            (
                prop::collection::vec(prop::collection::vec(cell(), ncols), nrows),
                0..=nrows as u32,
                0..=ncols as u32,
                widths,
            )
        })
        .prop_map(|(rows, header_rows, header_cols, column_widths)| {
            Content::Table(Table {
                header_rows,
                header_cols,
                column_widths,
                rows,
            })
        })
}

/// Tableau de `ncols` colonnes et `nrows` lignes dans une grille que
/// BlockNote sait écrire : sur la première ligne, une cellule couvre `span`
/// colonnes à partir de la colonne `at` ; sur la deuxième (s'il y a au moins
/// trois lignes), la cellule de la colonne `down` couvre `rows_down` lignes,
/// dont les suivantes ont une cellule de moins. Une fois sur deux, quand la
/// grille a au moins trois colonnes et que `down` n'est pas la dernière,
/// cette cellule couvre aussi deux colonnes : elle porte `colspan` et
/// `rowspan` à la fois, et il reste une case libre sur chaque ligne. Une
/// largeur par colonne de la grille.
fn merged_table() -> impl Strategy<Value = Content> {
    (2usize..5, 2usize..5)
        .prop_flat_map(|(ncols, nrows)| {
            (
                Just(ncols),
                Just(nrows),
                (2..=ncols).prop_flat_map(move |span| (Just(span), 0..=ncols - span)),
                0..ncols,
                2..=nrows.max(3) - 1,
                any::<bool>(),
                prop::collection::vec(cell(), ncols * nrows),
                0..=nrows as u32,
                0..=1u32,
                prop::collection::vec(width(), ncols),
            )
        })
        .prop_map(
            |(
                ncols,
                nrows,
                (span, at),
                down,
                rows_down,
                wide,
                cells,
                header_rows,
                header_cols,
                widths,
            )| {
                let mut cells = cells.into_iter();
                let mut rows: Vec<Vec<Cell>> = Vec::with_capacity(nrows);
                let tall = nrows >= 3;
                // Colonnes couvertes par la cellule haute de la ligne 1.
                let down_span = if wide && ncols >= 3 && down + 2 <= ncols {
                    2
                } else {
                    1
                };
                for r in 0..nrows {
                    let mut row = Vec::new();
                    let mut col = 0;
                    while col < ncols {
                        // Colonnes occupées par la cellule haute de la ligne 1.
                        if tall && (2..1 + rows_down).contains(&r) && col == down {
                            col += down_span;
                            continue;
                        }
                        let mut c = cells.next().expect("une cellule par case de la grille");
                        if r == 0 && col == at {
                            c.props
                                .insert("colspan".into(), PropValue::Int(span as i64));
                            col += span;
                        } else if tall && r == 1 && col == down {
                            c.props
                                .insert("rowspan".into(), PropValue::Int(rows_down as i64));
                            if down_span > 1 {
                                c.props
                                    .insert("colspan".into(), PropValue::Int(down_span as i64));
                            }
                            col += down_span;
                        } else {
                            col += 1;
                        }
                        row.push(c);
                    }
                    rows.push(row);
                }
                Content::Table(Table {
                    header_rows,
                    header_cols,
                    column_widths: widths,
                    rows,
                })
            },
        )
}

fn width() -> impl Strategy<Value = Option<f64>> {
    prop::sample::select(vec![None, Some(120.0), Some(80.5)])
}

/// Une cellule sans fusion : `colspan` et `rowspan` ne sont posés que par
/// [`merged_table`], dans une grille.
fn cell() -> impl Strategy<Value = Cell> {
    (
        prop::collection::vec(inline(), 0..3),
        props_from(cell_props()),
    )
        .prop_map(|(content, mut props)| {
            props.remove("colspan");
            props.remove("rowspan");
            Cell { content, props }
        })
}
