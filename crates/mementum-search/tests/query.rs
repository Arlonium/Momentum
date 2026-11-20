//! La lecture d'une requête (plan du lot 4, Task S2 ; R911, R926, R930) et le découpage en mots.

use mementum_search::{
    MAX_QUERY_CHARS, MAX_QUERY_TERMS, MAX_WORD_BYTES, Query, QueryRefusal, fold, nfc, read_query,
    words,
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
    for raw in [
        "a\"b", "a*b", "a-b", "a+b", "a:b", "a~b", "a^b", "a(b)", "a[b]", "a{b}", "a\\b", "a/b",
        "a?b", "a|b", "a&b", "a!b", "a<b>", "a=b", "a.b", "a,b", "a;b", "a'b", "a`b", "a@b", "a#b",
        "a$b", "a%b", "a_b",
    ] {
        let read = read_query(raw).unwrap();
        assert_eq!(read, query(&["a", "b"], None), "{raw:?}");
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
    let words = |n: usize| vec!["mot"; n].join(" ");
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

    let refusal = read_query("témoin b c d e f g h i").unwrap_err();
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
    for raw in ["Élève", "Straße", "ŒUVRE", "İstanbul", "Việt"] {
        assert_eq!(fold(&fold(raw)), fold(raw));
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
