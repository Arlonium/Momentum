//! La lecture d'un MMD (et le rendu qu'elle refait pour vérifier la forme canonique) croît comme
//! son entrée, en temps et en mémoire (constats critiques du tour 3 de la piste S, Ruling R380).
//! La méthode et ce qui la garde de passer à tort sont dans `common/linear.rs`.

#[path = "common/linear.rs"]
mod linear;

use linear::{Form, grows_like_input};
use mementum_md::parse;

/// Une forme : le MMD de taille `n`.
type Build = fn(usize) -> String;

/// Les formes lues : celles des constats, et celles qui pourraient l'être (structures longues,
/// imbrications, balises ou délimiteurs jamais fermés).
fn forms() -> Vec<(&'static str, Build)> {
    vec![
        // Constat critique du tour 3 : chaque lien fusionné renormalisait tout le lien accumulé
        // (200 Ko : 44 s en release).
        ("liens contigus au même href, styles alternés", |n| {
            "[**a**](u)[b](u)".repeat(n) + "\n"
        }),
        ("liens contigus au même href, imbriqués", |n| {
            "[[**a**](v)](u)[[b](v)](u)".repeat(n) + "\n"
        }),
        ("liens ouverts au-delà de la profondeur", |n| {
            format!("{}a{}\n", "<a href=\"u\">".repeat(n), "</a>".repeat(n))
        }),
        ("crochets ouvrants", |n| "[".repeat(n) + "a\n"),
        ("emphases non fermées", |n| "*a".repeat(n) + "\n"),
        ("emphases et crochets mêlés", |n| {
            "*[a**](".repeat(n) + "\n"
        }),
        ("accents graves non fermés", |n| "`a".repeat(n) + "\n"),
        ("formules", |n| "$a$ ".repeat(n) + "\n"),
        ("mentions", |n| {
            "<mention type=\"user\" id=\"u\">a</mention>".repeat(n) + "\n"
        }),
        ("cellules d'une ligne", |n| {
            format!("|{}\n|{}\n", " a |".repeat(n), " --- |".repeat(n))
        }),
        ("lignes de tableau", |n| {
            format!("| a | b |\n| --- | --- |\n{}", "| c | d |\n".repeat(n))
        }),
        ("paragraphes", |n| "Un paragraphe.\n\n".repeat(n) + "Fin.\n"),
        ("éléments de liste", |n| "- élément\n".repeat(n)),
        ("ligne longue au plus profond", |n| {
            nested_bullets() + &"  ".repeat(99) + "- " + &"b".repeat(4 * n) + "\n"
        }),
        ("blocs au plus profond", |n| {
            nested_bullets() + &format!("{}- b\n", "  ".repeat(99)).repeat(n)
        }),
        ("blocs dans des colonnes imbriquées", |n| {
            let open = "<columns>\n<column>\n- a\n\n".repeat(33);
            let close = "</column>\n<column>\nd\n</column>\n</columns>\n".repeat(33);
            open + &"b\n\n".repeat(n) + "c\n" + &close
        }),
    ]
}

/// 99 éléments de liste imbriqués : le suivant, indenté de 99 niveaux, est au niveau 100.
fn nested_bullets() -> String {
    (0..99).map(|depth| "  ".repeat(depth) + "- a\n").collect()
}

#[test]
fn reading_grows_like_its_input() {
    let forms = forms();
    let forms: Vec<Form<'_, String>> = forms
        .iter()
        .map(|(what, build)| Form { what, build })
        .collect();
    let failures = grows_like_input(&forms, String::len, |mmd| parse(mmd));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
