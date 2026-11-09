//! Lecture stricte : une entrée hors de la forme canonique est refusée, avec
//! la ligne et un message qui dit quoi écrire.

use mementum_md::{MAX_DEPTH, parse};

#[track_caller]
fn rejected(mmd: &str, line: usize, hint: &str) {
    match parse(mmd) {
        Ok(blocks) => panic!("{mmd:?} accepté : {blocks:?}"),
        Err(e) => {
            assert_eq!(e.line, line, "ligne de l'erreur pour {mmd:?} ({e})");
            assert!(
                e.message.contains(hint),
                "message de l'erreur pour {mmd:?} sans « {hint} » : {e}"
            );
        }
    }
}

#[test]
fn non_canonical_input_is_rejected() {
    rejected("* item\n", 1, "« - »");
    rejected("+ item\n", 1, "« - »");
    rejected("Titre\n=====\n", 2, "#");
    rejected("##### Titre\n", 1, "niveaux 1 à 4");
    rejected("<toggle title=\"x\">\nsans fin\n", 1, "</toggle>");
    rejected("Texte {align=center\n", 1, "{");
}

#[test]
fn other_non_canonical_forms_are_rejected() {
    // Code indenté, indentation incohérente, clôture non fermée.
    rejected("    code\n", 1, "```");
    rejected("- a\n   b\n", 2, "indentation");
    rejected("```rust\nfn main() {}\n", 1, "```");
    // Ligne vide manquante ou en trop, saut de ligne final absent.
    rejected("Un\nDeux\n", 2, "ligne vide");
    rejected("Un\n\n\nDeux\n", 3, "ligne vide");
    rejected("Texte", 1, "saut de ligne");
    // Valeur par défaut écrite, échappement manquant.
    rejected("Texte {align=\"left\"}\n", 1, "Texte");
    rejected("Prix {x}\n", 1, "\\{");
    // Fins de ligne CRLF.
    rejected("Texte\r\n", 1, "LF");
    // Formule sûre écrite en balise : la forme canonique est `$…$` (R8).
    rejected("<math latex=\"x\"/> y\n", 1, "$x$ y");
}

/// `n` toggles imbriqués, le plus profond vide : `n` niveaux.
fn toggles(n: usize) -> String {
    "<toggle title=\"x\">\n".repeat(n) + &"</toggle>\n".repeat(n)
}

/// `n` paragraphes, chacun enfant du précédent : `n` niveaux.
fn indented(n: usize) -> String {
    (0..n).map(|k| format!("{}P\n", "  ".repeat(k))).collect()
}

#[test]
fn nesting_is_bounded() {
    const MESSAGE: &str = "imbrication trop profonde : 100 niveaux au plus";
    assert_eq!(MAX_DEPTH, 100);
    assert!(parse(&toggles(MAX_DEPTH)).is_ok(), "100 toggles refusés");
    rejected(&toggles(MAX_DEPTH + 1), MAX_DEPTH + 1, MESSAGE);
    assert!(
        parse(&indented(MAX_DEPTH)).is_ok(),
        "100 niveaux d'indentation refusés"
    );
    rejected(&indented(MAX_DEPTH + 1), MAX_DEPTH + 1, MESSAGE);
}

/// Une entrée très imbriquée renvoie une erreur sans faire déborder la pile
/// d'un thread de 2 Mio (celle d'un thread tokio). L'indentation s'arrête à
/// 2 000 niveaux : 10 000 feraient une entrée de 100 Mo.
#[test]
fn deep_nesting_does_not_overflow_the_stack() {
    let links = format!(
        "{}lien{}\n",
        "<a href=\"x y\">".repeat(10_000),
        "</a>".repeat(10_000)
    );
    for src in [toggles(10_000), indented(2_000), links] {
        let result = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || parse(&src).map(|_| ()))
            .unwrap()
            .join()
            .expect("la lecture ne doit pas déborder la pile");
        assert!(result.is_err(), "imbrication très profonde acceptée");
    }
}

/// Ruling R16 (c) : le schéma de `@blocknote/xl-multi-column` 0.57.
#[test]
fn column_structure_follows_blocknote() {
    const COLUMN: &str = "<column>\nA\n</column>\n";
    // Une colonne hors d'un `<columns>`.
    rejected(COLUMN, 1, "hors de « <columns> »");
    // Un `<columns>` d'une seule colonne.
    rejected(
        &format!("<columns>\n{COLUMN}</columns>\n"),
        1,
        "deux « <column> »",
    );
    // Autre chose qu'une colonne dans un `<columns>`.
    rejected(
        &format!("<columns>\n{COLUMN}\nTexte\n</columns>\n"),
        6,
        "seulement des « <column> »",
    );
    // Une colonne vide.
    rejected(
        &format!("<columns>\n{COLUMN}<column>\n</column>\n</columns>\n"),
        5,
        "vide",
    );
    // Un `<columns>` directement dans une colonne.
    let inner = format!("<columns>\n{COLUMN}{COLUMN}</columns>\n");
    rejected(
        &format!("<columns>\n<column>\n{inner}</column>\n{COLUMN}</columns>\n"),
        3,
        "directement dans une « <column> »",
    );
    // Plus bas dans l'arbre (enfant d'un bloc de la colonne), c'est permis.
    let nested = format!(
        "<columns>\n<column>\nA\n  {}\n</column>\n{COLUMN}</columns>\n",
        "x"
    );
    assert!(parse(&nested).is_ok(), "{nested}");
}

/// Ruling R37 : fusions de cellules bornées, dans une grille rectangulaire.
/// prosemirror-tables parcourt `largeur × hauteur` cases et crée une colonne
/// par case de la grille : un `colspan` démesuré fige l'onglet, un négatif
/// fait lever BlockNote, et une ligne trop courte ou un `rowspan` trop long
/// sont « corrigés » par `fixTables`, qui réécrit le document.
#[test]
fn table_merges_form_a_bounded_grid() {
    let one = |cell: &str| format!("| {cell} |\n| --- |\n| x |\n");
    for value in ["0", "-3", "101", "1000000000"] {
        rejected(
            &one(&format!("A {{colspan={value}}}")),
            1,
            "« colspan » vaut de 1 à 100",
        );
    }
    rejected(&one("A {rowspan=0}"), 1, "« rowspan » vaut de 1 à 2");
    // `rowspan` jusqu'à la dernière ligne, pas au-delà.
    rejected(&one("A {rowspan=3}"), 1, "« rowspan » vaut de 1 à 2");
    rejected(
        "| A | B |\n| --- | --- |\n| C {rowspan=2} | D |\n",
        3,
        "« rowspan » vaut de 1 à 1",
    );
    // Lignes inégales, sans fusion.
    rejected(
        "| a | b |\n| --- | --- |\n| c |\n",
        3,
        "couvre 1 colonne, la première 2",
    );
    rejected(
        "| a | b |\n| --- | --- |\n| c | d | e |\n",
        3,
        "couvre 3 colonnes, la première 2",
    );
    assert_eq!(
        parse("| a | b |\n| --- | --- |\n| c |\n")
            .unwrap_err()
            .message,
        "ligne de tableau : elle couvre 1 colonne, la première 2 (fusions et cellules des \
         lignes au-dessus comptées)"
    );
    assert_eq!(
        parse("| A {colspan=0} |\n| --- |\n").unwrap_err().message,
        "« colspan » vaut de 1 à 100 (colonnes couvertes par la cellule)"
    );
    // Fusions comptées : une ligne trop longue sous un `rowspan`…
    rejected(
        "| A {rowspan=2} | B |\n| --- | --- |\n| C | D |\n",
        3,
        "couvre 3 colonnes, la première 2",
    );
    // … ou une cellule fusionnée qui chevauche celle d'une ligne au-dessus.
    rejected(
        "| A | B {rowspan=2} | C |\n| --- | --- | --- |\n| D {colspan=2} | E |\n",
        3,
        "chevauche",
    );
    // Les fusions qui forment une grille sont admises.
    for ok in [
        "| A {colspan=100} |\n| --- |\n",
        "| a | B {colspan=2} |\n| --- | --- |\n| c | d | e |\n",
        "| a | B {rowspan=3} |\n| --- | --- |\n| c |\n| d |\n",
        "| A {colspan=2 rowspan=2} | b |\n| --- | --- |\n| c |\n| d | e | f |\n",
        "| a | b | c |\n| --- | --- | --- |\n| D {colspan=2} | e |\n",
    ] {
        assert!(parse(ok).is_ok(), "{ok:?} refusé : {:?}", parse(ok));
    }
}

#[test]
fn a_number_too_large_for_a_float_says_so() {
    // Un nombre de plusieurs centaines de chiffres se lit `inf`, qui n'a pas
    // de forme MMD : le message parle du nombre, pas de forme non canonique.
    let huge = "9".repeat(400);
    // Prop inconnue écrite en flottant, puis prop flottante du registre.
    rejected(
        &format!("Texte {{foo={huge}.5}}\n"),
        1,
        "« foo » : nombre trop grand",
    );
    rejected(
        &format!("![Logo](https://ex.com/i.png) {{width={huge}}}\n"),
        1,
        "« width » : nombre trop grand",
    );
}
