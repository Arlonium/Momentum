//! La lecture d'une requête (plan du lot 4, Task S2 ; R911, R926, R930) et le découpage en mots.

use mementum_search::{
    MAX_QUERY_CHARS, MAX_QUERY_TERMS, MAX_WORD_BYTES, Query, QueryRefusal, fold, indexed_words,
    nfc, read_query, words,
};
use serde_json::Value;

fn query(terms: &[&str], prefix: Option<&str>) -> Query {
    Query {
        terms: terms.iter().map(|t| t.to_string()).collect(),
        prefix: prefix.map(str::to_string),
    }
}

/// La table de cas partagée, écrite à la main.
#[test]
fn the_queries_read_as_written() {
    let table: Value =
        serde_json::from_str(include_str!("../../../spec/search/queries.json")).unwrap();
    let cases = table["cases"].as_array().unwrap();
    assert!(cases.len() >= 30, "la table est lue");
    for case in cases {
        let why = case["why"].as_str().unwrap();
        let q = case["q"].as_str().unwrap();
        let read = read_query(q);
        match case.get("refused").and_then(Value::as_str) {
            Some("too_long") => assert_eq!(read, Err(QueryRefusal::TooLong), "{why}"),
            Some("too_many_terms") => assert_eq!(read, Err(QueryRefusal::TooManyTerms), "{why}"),
            Some(other) => panic!("{why} : refus inconnu de la table, {other}"),
            None => {
                let terms: Vec<&str> = case["terms"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t.as_str().unwrap())
                    .collect();
                assert_eq!(read, Ok(query(&terms, case["prefix"].as_str())), "{why}");
            }
        }
    }
}

#[test]
fn a_query_is_folded_and_split() {
    assert_eq!(
        read_query("  Élève DU  plan"),
        Ok(query(&["eleve", "du"], Some("plan")))
    );
    // Le même texte en accents combinants (NFD, les textes venus de macOS) : R926.
    assert_eq!(
        read_query("E\u{301}le\u{300}ve du plan"),
        Ok(query(&["eleve", "du"], Some("plan")))
    );
    assert_eq!(
        read_query("ŒUVRE Straße"),
        Ok(query(&["oeuvre"], Some("strasse")))
    );
}

#[test]
fn operators_are_only_characters() {
    let read = read_query("\"plan\" AND -budget* title:été~ NOT").unwrap();
    assert_eq!(
        read,
        query(&["plan", "and", "budget", "title", "ete"], Some("not"))
    );
    // Aucun terme ne garde un caractère qu'un analyseur de requêtes lirait comme un opérateur.
    for operator in [
        "\"", "*", "-", "+", ":", "~", "^", "(", ")", "[", "]", "{", "}", "\\", "/", "?", "|", "&",
        "!", "<", ">", "=", ".", ",", ";", "'", "`", "@", "#", "$", "%", "_",
    ] {
        let raw = format!("ab{operator}cd{operator}");
        let read = read_query(&raw).unwrap();
        assert_eq!(read, query(&["ab"], Some("cd")), "{raw:?}");
    }
    let every = read_query("AND OR NOT \" * - title: ~").unwrap();
    for term in every.terms.iter().chain(&every.prefix) {
        assert!(term.chars().all(char::is_alphanumeric), "{term:?}");
    }
}

#[test]
fn a_query_over_the_bounds_is_refused() {
    // 200 caractères passent, 201 non : en caractères, pas en octets.
    let at_bound = "é".repeat(MAX_QUERY_CHARS);
    assert_eq!(at_bound.len(), 2 * MAX_QUERY_CHARS);
    assert!(read_query(&at_bound).is_ok(), "la borne est admise");
    assert_eq!(
        read_query(&"é".repeat(MAX_QUERY_CHARS + 1)),
        Err(QueryRefusal::TooLong)
    );
    assert!(read_query(&"🚀".repeat(MAX_QUERY_CHARS)).is_ok());
    assert_eq!(
        read_query(&"🚀".repeat(MAX_QUERY_CHARS + 1)),
        Err(QueryRefusal::TooLong)
    );
    // Des espaces comptent : la borne est celle de ce qui est envoyé.
    assert_eq!(
        read_query(&" ".repeat(MAX_QUERY_CHARS + 1)),
        Err(QueryRefusal::TooLong)
    );
    // 8 termes passent, 9 non ; le dernier compte.
    let words = |n: usize| {
        let distinct: Vec<String> = (0..n).map(|i| format!("mot{i}")).collect();
        distinct.join(" ")
    };
    let read = read_query(&words(MAX_QUERY_TERMS)).unwrap();
    assert_eq!(read.terms.len() + read.prefix.iter().len(), MAX_QUERY_TERMS);
    assert_eq!(
        read_query(&words(MAX_QUERY_TERMS + 1)),
        Err(QueryRefusal::TooManyTerms)
    );
    // Trop longue et trop de mots : c'est la longueur qui est dite, sans lire les mots.
    assert_eq!(
        read_query(&"a ".repeat(MAX_QUERY_CHARS)),
        Err(QueryRefusal::TooLong)
    );
}

/// Un message de refus tutoie, dit la borne, et ne recopie rien de la requête (R434, et la
/// contrainte « la requête n'est jamais recopiée »).
#[test]
fn a_refusal_never_repeats_the_query() {
    let too_long = format!("témoin {}", "x".repeat(MAX_QUERY_CHARS));
    let refusal = read_query(&too_long).unwrap_err();
    let message = refusal.to_string();
    assert!(
        !message.contains("témoin") && !message.contains("xx"),
        "{message}"
    );
    assert!(message.contains("200"), "{message}");
    assert!(message.starts_with("Ta recherche"), "{message}");

    let refusal = read_query("témoin bb cc dd ee ff gg hh ii").unwrap_err();
    let message = refusal.to_string();
    assert!(!message.contains("témoin"), "{message}");
    assert!(message.contains('8'), "{message}");
    assert!(message.starts_with("Ta recherche"), "{message}");
    assert!(message.len() < 200);
}

#[test]
fn a_prefix_under_two_characters_is_dropped() {
    // Le dernier mot d'un caractère n'est pas cherché comme début de mot : il reste un mot
    // cherché entier (R940).
    assert_eq!(read_query("plan d"), Ok(query(&["plan", "d"], None)));
    assert_eq!(read_query("d"), Ok(query(&["d"], None)));
    assert_eq!(read_query("é"), Ok(query(&["e"], None)));
    // Témoin : à deux caractères, c'est un début de mot.
    assert_eq!(read_query("plan de"), Ok(query(&["plan"], Some("de"))));
    // Seul le dernier mot est un début, jamais un autre.
    assert_eq!(read_query("pl bu"), Ok(query(&["pl"], Some("bu"))));
}

#[test]
fn an_empty_query_reads_empty() {
    for raw in ["", " ", " \t\n ", "***", "\"-~:", "🚀 🚀", "\u{301}"] {
        let read = read_query(raw).unwrap();
        assert!(read.is_empty(), "{raw:?} : {read:?}");
        assert_eq!(read, Query::default());
    }
    assert!(!read_query("a").unwrap().is_empty());
    assert!(!read_query("ab").unwrap().is_empty());
}

#[test]
fn words_are_cut_where_a_character_is_neither_a_letter_nor_a_digit() {
    let text = "Bonjour, l’élève snake_case 42!";
    let cut: Vec<(usize, &str)> = words(text).map(|w| (w.start, w.raw)).collect();
    let raws: Vec<&str> = cut.iter().map(|(_, raw)| *raw).collect();
    assert_eq!(raws, ["Bonjour", "l", "élève", "snake", "case", "42"]);
    for (start, raw) in cut {
        assert_eq!(&text[start..start + raw.len()], raw);
    }
    assert_eq!(words("").count(), 0);
    assert_eq!(words(" \t… — ").count(), 0);
    // Un mot au bord du texte, sans séparateur.
    assert_eq!(words("mot").map(|w| w.raw).collect::<Vec<_>>(), ["mot"]);
    // Un mot long est rendu : c'est à qui indexe de l'écarter (R926).
    let long = "x".repeat(MAX_WORD_BYTES + 1);
    assert_eq!(words(&long).map(|w| w.raw.len()).collect::<Vec<_>>(), [41]);
}

#[test]
fn a_word_is_folded_without_case_or_latin_accents() {
    for (raw, folded) in [
        ("Élève", "eleve"),
        ("DÉJÀ", "deja"),
        ("naïve", "naive"),
        ("Noël", "noel"),
        ("garçon", "garcon"),
        ("ŒUVRE", "oeuvre"),
        ("Cæsar", "caesar"),
        ("Straße", "strasse"),
        ("Smørrebrød", "smorrebrod"),
        ("Łódź", "lodz"),
        ("Việt", "viet"),
        ("İstanbul", "istanbul"),
        ("señor", "senor"),
        ("abc123", "abc123"),
        ("", ""),
        // Hors de l'alphabet latin : minuscules, rien d'autre.
        ("Ελλάδα", "ελλάδα"),
        ("Привет", "привет"),
        ("한글", "한글"),
        ("搜索", "搜索"),
    ] {
        assert_eq!(fold(raw), folded, "{raw:?}");
    }
    // Plier deux fois ne change rien.
    for raw in ["Élève", "Straße", "ŒUVRE", "İstanbul", "Việt", "ΟΔΟΣ"] {
        assert_eq!(fold(&fold(raw)), fold(raw));
    }
}

/// R947 : seuls les accents que portent les lettres latines sont retirés (U+0300 à U+036F). Les
/// signes des autres écritures sont des lettres de leurs mots : retirés, « की » (de) se lirait
/// « क », et deux mots différents se confondraient.
#[test]
fn a_fold_keeps_the_marks_of_other_scripts() {
    let hindi = "\u{915}\u{940}"; // « की »
    let arabic = "\u{643}\u{650}\u{62a}\u{64e}\u{627}\u{628}"; // « kitāb », voyelles écrites
    let thai = "\u{e17}\u{e35}\u{e48}"; // « ที่ »
    for word in [hindi, arabic, thai, "\u{93e}", "\u{915}\u{93e}"] {
        assert_eq!(fold(word), word, "{word:?}");
    }
    assert_ne!(fold(hindi), fold("\u{915}"));
    // Un accent latin combinant resté seul (texte qui n'est pas en NFC, ou que NFC ne compose
    // pas) est retiré, comme celui qu'une lettre composée porte.
    assert_eq!(fold("x\u{301}"), "x");
    assert_eq!(fold("e\u{301}le\u{300}ve"), "eleve");
    // Un mot peut donc se plier en rien : qui rend des mots pliés l'écarte.
    assert_eq!(fold("\u{301}"), "");
    assert_eq!(fold("\u{345}"), "");
}

/// Le sigma final se lit comme un sigma : « ΟΔΟΣ » en majuscules donne « οδοσ », et doit trouver
/// « οδος » écrit en minuscules.
#[test]
fn a_final_sigma_is_a_sigma() {
    assert_eq!(fold("ΟΔΟΣ"), fold("οδος"));
    assert_eq!(fold("οδος"), "οδοσ");
    assert_eq!(read_query("ΟΔΟΣ"), read_query("οδος"));
}

/// Un mot plié peut être plus long, en octets, que le mot écrit : la borne de 40 octets se juge
/// sur le mot écrit, jamais sur le mot plié.
#[test]
fn a_folded_word_can_be_longer_than_the_written_one() {
    let written = "Ⱥ".repeat(20);
    assert_eq!(written.len(), MAX_WORD_BYTES);
    assert_eq!(fold(&written).len(), 60);
    // Ce que l'index garde d'un texte : les mots d'au plus 40 octets écrits, pliés, jamais vides.
    let text = format!("Élève {written} x{written} \u{345} fin");
    let kept: Vec<(usize, &str, String)> = indexed_words(&text)
        .map(|(word, folded)| (word.start, word.raw, folded))
        .collect();
    assert_eq!(kept.len(), 3, "{kept:?}");
    assert_eq!((kept[0].1, kept[0].2.as_str()), ("Élève", "eleve"));
    assert_eq!(kept[1].1, written);
    assert_eq!(kept[1].2.len(), 60);
    assert_eq!((kept[2].1, kept[2].2.as_str()), ("fin", "fin"));
    for (start, raw, _) in &kept {
        assert_eq!(&text[*start..start + raw.len()], *raw);
    }
}

/// R949 : un mot d'une seule lettre qui n'est pas le dernier n'est ni exigé ni compté (les
/// élisions : « l' », « d' », « c' ») ; un chiffre seul reste un mot ; le dernier mot garde sa
/// règle (R940). Les mots répétés ne comptent qu'une fois.
#[test]
fn single_letters_before_the_last_word_and_repeats_are_not_wanted() {
    assert_eq!(
        read_query("l'anniversaire d'Anne"),
        Ok(query(&["anniversaire"], Some("anne")))
    );
    assert_eq!(
        read_query("lot 4 plan"),
        Ok(query(&["lot", "4"], Some("plan")))
    );
    assert_eq!(read_query("à é plan b"), Ok(query(&["plan", "b"], None)));
    // Une lettre d'une autre écriture aussi ; un idéogramme est une lettre.
    assert_eq!(read_query("я plan"), Ok(query(&[], Some("plan"))));
    // Les répétitions, dans l'ordre de première venue ; le dernier mot reste le dernier.
    assert_eq!(
        read_query("plan budget PLAN Budget plan lot"),
        Ok(query(&["plan", "budget"], Some("lot")))
    );
    assert_eq!(read_query("pla pla"), Ok(query(&["pla"], Some("pla"))));
    // Ni les lettres seules ni les répétitions ne comptent dans les huit mots.
    assert_eq!(
        read_query("l a b c d e f g h i j k un un un un un un un un deux"),
        Ok(query(&["un"], Some("deux")))
    );
    assert_eq!(
        read_query("1 2 3 4 5 6 7 8 9"),
        Err(QueryRefusal::TooManyTerms)
    );
}

#[test]
fn a_word_folded_to_nothing_is_not_a_term() {
    // U+0345 est une lettre pour Unicode, et un accent que le pliage retire.
    assert_eq!(read_query("plan \u{345}"), Ok(query(&[], Some("plan"))));
    assert_eq!(read_query("\u{345} plan"), Ok(query(&[], Some("plan"))));
    assert!(read_query("\u{345}").unwrap().is_empty());
    // Un signe d'une autre écriture resté seul est un mot comme un autre, jamais un terme vide.
    let read = read_query("plan \u{93e}").unwrap();
    assert_eq!(read, query(&["plan", "\u{93e}"], None));
    for term in read.terms.iter().chain(&read.prefix) {
        assert!(!term.is_empty());
    }
}

#[test]
fn nfc_composes_what_can_be_and_borrows_what_already_is() {
    assert_eq!(nfc("e\u{301}le\u{300}ve"), "élève");
    assert!(matches!(nfc("élève"), std::borrow::Cow::Borrowed(_)));
    assert!(matches!(nfc("plain"), std::borrow::Cow::Borrowed(_)));
    // En NFD, « élève » serait coupé en trois mots : c'est pourquoi on normalise d'abord.
    assert_eq!(words("e\u{301}le\u{300}ve").count(), 3);
    assert_eq!(words(&nfc("e\u{301}le\u{300}ve")).count(), 1);
}
