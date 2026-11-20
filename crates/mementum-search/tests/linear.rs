//! Linéarité de la projection, de la lecture d'une requête et du découpage en mots (Rulings R380,
//! R433) : tout ce qui lit un contenu venu d'un client croît comme lui, sous des bornes absolues.
//! L'aide, la façon de choisir les tailles et ce qui empêche la garde de passer à tort sont dans
//! `mementum-md/tests/common/linear.rs`.

#[path = "../../mementum-md/tests/common/linear.rs"]
mod linear;

use linear::{Answer, Form, Limits, grows_like_input};
use mementum_md::{Block, Cell, Content, Inline, PropValue, Props, Styles, Table};
use mementum_search::{Projection, fold, nfc, project, read_query, words};

const ORIGIN: &str = "https://notes.exemple.fr";

fn plain(text: &str) -> Inline {
    Inline::Text {
        text: text.to_string(),
        styles: Styles::default(),
    }
}

fn block(id: String, kind: &str, content: Content, children: Vec<Block>) -> Block {
    Block {
        id,
        kind: kind.to_string(),
        props: Props::new(),
        content,
        children,
    }
}

fn para(i: usize, content: Vec<Inline>) -> Block {
    block(
        format!("b{i:08}"),
        "paragraph",
        Content::Inline(content),
        Vec::new(),
    )
}

fn page_id(i: usize) -> String {
    format!("0193b2c4-5d6e-7f80-9a1b-{i:012x}")
}

/// Les octets d'une page lue : ses identifiants, ses types, ses textes, ses adresses, ses props.
fn page_bytes(blocks: &[Block]) -> usize {
    fn inlines(content: &[Inline]) -> usize {
        content
            .iter()
            .map(|inline| {
                8 + match inline {
                    Inline::Text { text, .. } => text.len(),
                    Inline::Link { href, children } => href.len() + inlines(children),
                    Inline::Mention { kind, id, label } => kind.len() + id.len() + label.len(),
                    Inline::Math { latex } => latex.len(),
                    Inline::HardBreak => 0,
                }
            })
            .sum()
    }
    blocks
        .iter()
        .map(|block| {
            let props: usize = block
                .props
                .iter()
                .map(|(name, value)| {
                    name.len()
                        + match value {
                            PropValue::Str(text) => text.len(),
                            _ => 8,
                        }
                })
                .sum();
            let content = match &block.content {
                Content::None => 0,
                Content::Inline(content) => inlines(content),
                Content::Table(table) => table
                    .rows
                    .iter()
                    .flatten()
                    .map(|cell| 8 + inlines(&cell.content))
                    .sum(),
            };
            block.id.len() + block.kind.len() + props + content + page_bytes(&block.children)
        })
        .sum()
}

/// Ce que pèse une projection : ce que la base en gardera.
fn projection_bytes(projection: &Projection) -> usize {
    projection
        .rows
        .iter()
        .map(|row| row.block_id.len() + row.text.len() + row.hash.len() + 16 * row.links.len())
        .sum()
}

/// Les bornes absolues d'une projection, à la grande taille : seize octets de mémoire vive par
/// octet lu, quatre octets rendus par octet lu.
const PROJECTION: Limits = Limits {
    peak_per_byte: 16,
    answer: Answer::PerByte(4),
};

#[test]
fn projecting_grows_like_the_page() {
    let forms: [Form<'_, Vec<Block>>; 11] = [
        Form {
            what: "un long texte dans un bloc",
            build: &|n| vec![para(0, vec![plain(&"mot élevé ".repeat(8 * n))])],
        },
        Form {
            what: "un long texte en accents combinants dans un bloc",
            build: &|n| vec![para(0, vec![plain(&"e\u{301}le\u{300}ve ".repeat(8 * n))])],
        },
        Form {
            what: "des paragraphes ordinaires",
            build: &|n| {
                (0..n)
                    .map(|i| {
                        para(
                            i,
                            vec![plain("Un paragraphe ordinaire, d'une ligne ou deux.")],
                        )
                    })
                    .collect()
            },
        },
        Form {
            what: "des paragraphes en accents combinants",
            build: &|n| {
                (0..n)
                    .map(|i| {
                        para(
                            i,
                            vec![plain(
                                "Un e\u{301}le\u{300}ve de\u{301}ja\u{300} la\u{300}.",
                            )],
                        )
                    })
                    .collect()
            },
        },
        Form {
            what: "mille plages marquées par bloc",
            build: &|n| {
                (0..n.div_ceil(100))
                    .map(|i| {
                        let runs = (0..1000)
                            .map(|run| Inline::Text {
                                text: format!("mot{run} "),
                                styles: Styles {
                                    bold: run % 2 == 0,
                                    text_color: (run % 3 == 0).then(|| "red".to_string()),
                                    ..Styles::default()
                                },
                            })
                            .collect();
                        para(i, runs)
                    })
                    .collect()
            },
        },
        Form {
            what: "un tableau large",
            build: &|n| {
                let cell = |i: usize| Cell {
                    content: vec![plain(&format!("c{i}"))],
                    props: Props::new(),
                };
                let table = Table {
                    header_rows: 1,
                    header_cols: 0,
                    column_widths: Vec::new(),
                    rows: vec![
                        (0..4 * n).map(cell).collect(),
                        (0..4 * n).map(cell).collect(),
                    ],
                };
                vec![block(
                    "t".to_string(),
                    "table",
                    Content::Table(table),
                    Vec::new(),
                )]
            },
        },
        Form {
            what: "des listes imbriquées",
            build: &|n| {
                // Des chaînes de trente points, chacun enfant du précédent.
                (0..n.div_ceil(30))
                    .map(|chain| {
                        (0..30).rev().fold(None, |child: Option<Block>, depth| {
                            Some(block(
                                format!("l{chain:06}-{depth:02}"),
                                "bulletListItem",
                                Content::Inline(vec![plain("Un point")]),
                                child.into_iter().collect(),
                            ))
                        })
                    })
                    .map(Option::unwrap)
                    .collect()
            },
        },
        Form {
            what: "cent mille mentions dans un bloc",
            build: &|n| {
                let mentions = (0..8 * n)
                    .map(|i| Inline::Mention {
                        kind: "page".to_string(),
                        id: page_id(i),
                        label: "p".to_string(),
                    })
                    .collect();
                vec![para(0, mentions)]
            },
        },
        Form {
            what: "des liens vers des pages, sous l'origine publique, un par bloc",
            build: &|n| {
                (0..n)
                    .map(|i| {
                        let link = Inline::Link {
                            href: format!("{ORIGIN}/p/{}#b=abc", page_id(i % 50_000)),
                            children: vec![plain("lien")],
                        };
                        para(i, vec![link])
                    })
                    .collect()
            },
        },
        Form {
            what: "des blocs au même identifiant, des blocs sans identifiant",
            build: &|n| {
                (0..2 * n)
                    .map(|i| {
                        let id = if i % 2 == 0 { "même" } else { "" };
                        block(
                            id.to_string(),
                            "paragraph",
                            Content::Inline(vec![plain("texte")]),
                            Vec::new(),
                        )
                    })
                    .collect()
            },
        },
        Form {
            what: "des blocs sans ligne : inconnus, colonnes, identifiants longs",
            build: &|n| {
                (0..n)
                    .map(|i| {
                        let kind = ["unknown", "column", "future", "divider"][i % 4];
                        block(
                            "i".repeat(200),
                            kind,
                            Content::Inline(vec![plain("texte qui n'est pas cherché")]),
                            vec![para(i, vec![plain("enfant")])],
                        )
                    })
                    .collect()
            },
        },
    ];
    let failures = grows_like_input(
        &forms,
        |blocks| page_bytes(blocks),
        |blocks| project(blocks, Some(ORIGIN)),
        projection_bytes,
        PROJECTION,
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn reading_a_query_does_not_grow_with_it() {
    let forms: [Form<'_, String>; 4] = [
        Form {
            what: "une requête longue, de mots courts",
            build: &|n| "mot ".repeat(n),
        },
        Form {
            what: "une requête d'un seul mot long",
            build: &|n| "é".repeat(4 * n),
        },
        Form {
            what: "une requête d'accents combinants",
            build: &|n| "e\u{301}".repeat(2 * n),
        },
        Form {
            what: "une requête d'opérateurs",
            build: &|n| "\"a\" AND -b* ".repeat(n),
        },
    ];
    let failures = grows_like_input(
        &forms,
        String::len,
        |raw| read_query(raw),
        // Ce que le serveur répondrait : le message du refus.
        |read| {
            read.as_ref()
                .map_or_else(|refusal| refusal.to_string().len(), |_| 0)
        },
        Limits {
            peak_per_byte: 1,
            answer: Answer::Absolute(512),
        },
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn cutting_and_folding_words_grows_like_the_text() {
    let forms: [Form<'_, String>; 5] = [
        Form {
            what: "des mots ordinaires",
            build: &|n| "Un Élève déjà là, œuvre ".repeat(n),
        },
        Form {
            what: "des mots en accents combinants",
            build: &|n| "e\u{301}le\u{300}ve ".repeat(2 * n),
        },
        Form {
            what: "un seul mot",
            build: &|n| "É".repeat(8 * n),
        },
        Form {
            what: "une suite d'accents combinants seuls",
            build: &|n| "\u{301}".repeat(8 * n),
        },
        Form {
            what: "des séparateurs seuls",
            build: &|n| "-*~\" ".repeat(4 * n),
        },
    ];
    let failures = grows_like_input(
        &forms,
        String::len,
        |text| {
            let text = nfc(text);
            words(&text).map(|word| fold(word.raw).len()).sum::<usize>()
        },
        |folded| *folded,
        // Une suite d'accents combinants seuls est la forme la plus chère : la mise en NFC les
        // garde tous, huit octets chacun, le temps de les ranger (mesuré : 8,0 octets par octet).
        Limits {
            peak_per_byte: 12,
            answer: Answer::PerByte(4),
        },
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Mesure (release, seule) : ce que coûte la projection d'une page aux bornes de R912, que le plan
/// donne comme des valeurs de départ. Aucun seuil : ses lignes `MESURE` vont au registre.
#[test]
#[ignore = "mesure : cargo test --release -p mementum-search --test linear -- --ignored --nocapture"]
fn measure_projecting_pages_at_the_bounds() {
    use mementum_search::{MAX_BLOCK_TEXT_BYTES, MAX_PAGE_LINKS, MAX_PAGE_TEXT_BYTES};

    let _alone = linear::ALONE.lock().unwrap_or_else(|e| e.into_inner());
    let sentence = "Un paragraphe ordinaire, déjà relu par l'élève. ";
    let full = sentence.repeat(MAX_BLOCK_TEXT_BYTES / sentence.len());
    let pages: [(&str, Vec<Block>); 4] = [
        (
            "10 000 paragraphes d'une phrase",
            (0..10_000)
                .map(|i| para(i, vec![plain(sentence)]))
                .collect(),
        ),
        (
            "une page pleine : 8 Mio en blocs de 64 Kio",
            (0..MAX_PAGE_TEXT_BYTES / full.len() + 1)
                .map(|i| para(i, vec![plain(&full)]))
                .collect(),
        ),
        (
            "une page pleine en accents combinants",
            (0..MAX_PAGE_TEXT_BYTES / MAX_BLOCK_TEXT_BYTES)
                .map(|i| para(i, vec![plain(&"e\u{301}".repeat(MAX_BLOCK_TEXT_BYTES / 3))]))
                .collect(),
        ),
        (
            "10 000 blocs d'un lien chacun",
            (0..MAX_PAGE_LINKS)
                .map(|i| {
                    let mention = Inline::Mention {
                        kind: "page".to_string(),
                        id: page_id(i),
                        label: "Une page".to_string(),
                    };
                    para(i, vec![mention])
                })
                .collect(),
        ),
    ];
    for (what, blocks) in &pages {
        let cost = linear::cost(|| project(blocks, Some(ORIGIN)));
        let projection = project(blocks, Some(ORIGIN));
        let text: usize = projection.rows.iter().map(|row| row.text.len()).sum();
        println!(
            "MESURE projection, {what} : {} octets lus, {} lignes, {text} octets de texte, coupée : \
             {} ; {:?}, pic {} octets, {} octets alloués",
            page_bytes(blocks),
            projection.rows.len(),
            projection.truncated,
            cost.time,
            cost.peak,
            cost.allocated,
        );
    }
}
