//! Lecture stricte : une entrée hors de la forme canonique est refusée, avec
//! la ligne et un message qui dit quoi écrire.

use mementum_md::parse;

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
}
