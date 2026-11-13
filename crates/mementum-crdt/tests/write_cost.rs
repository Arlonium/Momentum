//! Coût estimé d'une écriture complète, avant de l'écrire (constat I3 de la relecture de la
//! piste S, Ruling R368) : chaque liste que l'écriture bâtit par insertions successives coûte en
//! proportion du carré de sa longueur (frères, lignes, cellules d'une ligne, morceaux du contenu
//! en ligne d'un bloc), plus un terme par élément écrit. `PUT /mmd` refuse ce qui dépasse son
//! budget ; ces tests disent ce que l'estimation compte.

use mementum_crdt::{Cause, ListKind, write_cost};
use mementum_md::{Block, Content, Inline, Styles, parse};

/// Le MMD `mmd`, lu.
fn read(mmd: &str) -> Vec<Block> {
    parse(mmd).unwrap()
}

/// `n` paragraphes ordinaires, comme la page des budgets (`mementum-sync`).
fn ordinary(n: usize) -> String {
    (1..=n)
        .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}

/// Un paragraphe de `n` plages, grasses et maigres en alternance (`n` pair).
fn bold_runs(n: usize) -> String {
    "**a**b".repeat(n / 2) + "\n"
}

/// Un tableau de `rows` lignes sur `cols` colonnes, sans en-tête.
fn table(rows: usize, cols: usize) -> String {
    let row = format!("|{}\n", " a |".repeat(cols));
    let mut mmd = row.clone();
    mmd.push_str(&format!("|{}\n", " --- |".repeat(cols)));
    for _ in 1..rows {
        mmd.push_str(&row);
    }
    mmd
}

const SECOND: u64 = 1_000_000_000;

/// La page des budgets, 10 000 paragraphes ordinaires, reste loin du budget de `PUT /mmd`
/// (huit secondes estimées) : un peu moins de deux secondes, dont 1,5 pour ses frères.
#[test]
fn ordinary_page_of_10000_blocks_costs_under_two_seconds() {
    let cost = write_cost(&read(&ordinary(10_000)));
    assert!(cost.nanos < 2 * SECOND, "{cost:?}");
    assert!(cost.nanos > SECOND, "{cost:?}");
    assert_eq!(
        cost.cause,
        Cause::Lists {
            kind: ListKind::Siblings,
            longest: 10_000
        }
    );
}

/// Les plages d'un paragraphe : chacune est une chaîne du texte, plus deux marques par marque qui
/// change ; leur coût croît comme le carré de leur nombre, et c'est le paragraphe qui est nommé.
#[test]
fn runs_of_a_paragraph_cost_their_square() {
    let small = write_cost(&read(&bold_runs(4_000)));
    let large = write_cost(&read(&bold_runs(8_000)));
    let ratio = large.nanos as f64 / small.nanos as f64;
    assert!((3.5..=4.0).contains(&ratio), "{small:?} {large:?}");
    assert_eq!(
        large.cause,
        Cause::Lists {
            kind: ListKind::Inline,
            longest: 8_000
        }
    );
    // L'entrée hostile de la relecture : 20 000 plages en gras, 120 Ko, 91 s d'acteur.
    assert!(write_cost(&read(&bold_runs(40_000))).nanos > 60 * SECOND);
}

/// Une marque qui change compte deux éléments du texte, un lien comme une marque : des plages
/// toutes marquées en alternance avec des plages maigres coûtent plus que du gras seul.
#[test]
fn each_changed_mark_counts() {
    let bold = write_cost(&read(&bold_runs(2_000)));
    let all = Styles {
        bold: true,
        italic: true,
        underline: true,
        strike: true,
        code: true,
        text_color: Some("red".into()),
        background_color: Some("blue".into()),
    };
    let runs = (0..1_000).flat_map(|_| {
        [
            Inline::Link {
                href: "https://exemple.fr".into(),
                children: vec![Inline::Text {
                    text: "a".into(),
                    styles: all.clone(),
                }],
            },
            Inline::Text {
                text: "b".into(),
                styles: Styles::default(),
            },
        ]
    });
    let mut paragraph = read("a\n");
    paragraph[0].content = Content::Inline(runs.collect());
    let marked = write_cost(&paragraph);
    assert!(marked.nanos > 10 * bold.nanos, "{bold:?} {marked:?}");
}

/// Les sauts de ligne, mentions et formules sont des enfants du nœud de contenu, comme chaque
/// suite de plages : 40 000 sauts de ligne entre des lettres font 80 000 morceaux.
#[test]
fn hard_breaks_are_pieces_of_the_paragraph() {
    let cost = write_cost(&read(&("a<br/>".repeat(40_000) + "\n")));
    assert!(cost.nanos > 60 * SECOND, "{cost:?}");
    assert_eq!(
        cost.cause,
        Cause::Lists {
            kind: ListKind::Inline,
            longest: 80_000
        }
    );
}

/// Les cellules d'une ligne sont une liste : un tableau de 100 lignes sur 4 000 colonnes (1,6 Mo,
/// l'entrée hostile de la relecture : 20 à 25 s d'acteur) est nommé par ses lignes trop larges.
#[test]
fn cells_of_a_row_cost_their_square() {
    let cost = write_cost(&read(&table(100, 4_000)));
    assert!(cost.nanos > 20 * SECOND, "{cost:?}");
    assert_eq!(
        cost.cause,
        Cause::Lists {
            kind: ListKind::Cells,
            longest: 4_000
        }
    );
    // Dix fois plus étroit : cent fois moins par ligne, et c'est le volume qui pèse.
    let narrow = write_cost(&read(&table(100, 400)));
    assert!(narrow.nanos < 3 * SECOND, "{narrow:?}");
    assert!(matches!(narrow.cause, Cause::Volume { .. }), "{narrow:?}");
}

/// Un grand tableau de lignes ordinaires coûte par son volume : 1 500 lignes de 100 cellules,
/// trois secondes et demie d'acteur mesurées pour une récriture.
#[test]
fn many_cells_cost_their_volume() {
    let cost = write_cost(&read(&table(1_500, 100)));
    assert!((3 * SECOND..8 * SECOND).contains(&cost.nanos), "{cost:?}");
    assert_eq!(
        cost.cause,
        Cause::Volume {
            elements: 1 + 1_500 + 2 * 150_000
        }
    );
}

/// Un bloc de code s'écrit en une seule plage, quelle que soit la forme de son texte.
#[test]
fn code_block_is_a_single_run() {
    let lines = "**a**b\n".repeat(5_000);
    let cost = write_cost(&read(&format!("```\n{lines}```\n")));
    assert!(cost.nanos < SECOND / 10, "{cost:?}");
}
