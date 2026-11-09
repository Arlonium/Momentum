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
