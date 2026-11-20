//! L'index plein texte (Task S3 du lot 4 ; Rulings R900, R902, R916, R920 à R923, R926, R929,
//! R930) : ce qu'il trouve, ce qu'il ne dit jamais des pages non permises, ce qu'il écrit, son
//! jeton. Tout test qui affirme une absence a son témoin.

#[path = "../../mementum-sync/tests/common/limit.rs"]
mod limit;
#[path = "../../mementum-store/tests/common/tmp.rs"]
mod tmp;

use std::collections::HashSet;
use std::path::Path;

use mementum_search::{
    Found, Hit, IndexError, MAX_EXCERPT_CHARS, MAX_VERIFIED_BYTES, MAX_VERIFIED_ENTRIES,
    MAX_VERIFIED_PER_PAGE, PageSet, Query, SearchIndex, Trace, Witness, read_query,
};
use sha2::{Digest, Sha256};
use tmp::TestDir;
use uuid::Uuid;

/// Une page : des identifiants qui se suivent, pour qu'un test dise leur ordre.
fn page(n: u128) -> Uuid {
    Uuid::from_u128(0x0193_b2c4_5d6e_7f80_9a1b_0000_0000_0000 + n)
}

fn pages(of: impl IntoIterator<Item = u128>) -> PageSet {
    of.into_iter().map(page).collect()
}

fn query(raw: &str) -> Query {
    read_query(raw).expect("une requête sous les bornes")
}

/// Un index dans un dossier à lui. L'index est lâché avant le dossier (l'ordre des champs).
struct Bench {
    index: SearchIndex,
    _dir: TestDir,
}

impl Bench {
    fn new() -> Bench {
        let dir = TestDir::new();
        let index = SearchIndex::open(dir.path()).expect("ouvrir").index;
        Bench { index, _dir: dir }
    }

    /// L'entrée d'un bloc, telle que l'indexeur l'écrit : le titre de sa page avec elle.
    fn block(&self, n: u128, block: &str, title: &str, text: &str) {
        self.index
            .put_entry(page(n), Some(block), title, text)
            .expect("écrire une entrée");
    }

    /// L'entrée de titre d'une page.
    fn title(&self, n: u128, title: &str) {
        self.index
            .put_entry(page(n), None, title, "")
            .expect("écrire une entrée de titre");
    }

    /// Une page entière : son entrée de titre, puis ses blocs.
    fn page(&self, n: u128, title: &str, blocks: &[(&str, &str)]) {
        self.title(n, title);
        for (block, text) in blocks {
            self.block(n, block, title, text);
        }
    }

    fn commit(&self) {
        self.index.commit("jeton").expect("valider");
    }

    fn found(&self, raw: &str, allowed: &PageSet, skip: usize, limit: usize) -> Found {
        self.index
            .search(&query(raw), allowed, skip, limit)
            .expect("chercher")
    }

    fn traced(&self, raw: &str, allowed: &PageSet, witness: Option<Witness>) -> (Found, Trace) {
        self.index
            .search_traced(&query(raw), allowed, 0, 100, witness)
            .expect("chercher")
    }

    fn hits(&self, raw: &str, allowed: &PageSet) -> Vec<Hit> {
        self.found(raw, allowed, 0, 100).pages
    }

    /// Les pages trouvées, dans l'ordre, par leur numéro.
    fn numbers(&self, raw: &str, allowed: &PageSet) -> Vec<u128> {
        numbers(&self.hits(raw, allowed))
    }
}

fn numbers(hits: &[Hit]) -> Vec<u128> {
    hits.iter()
        .map(|hit| hit.node_id.as_u128() - page(0).as_u128())
        .collect()
}

/// La clé de départage d'un bloc (Ruling R922), refaite ici d'après sa définition : les seize
/// premiers octets du SHA-256 de la clé du bloc.
fn tie_key(block: &str) -> [u8; 16] {
    let digest = Sha256::digest(block.as_bytes());
    digest[..16].try_into().expect("seize octets")
}

/// Le bloc qui gagne un départage parmi ceux-ci.
fn first_by_key<'a>(blocks: &[&'a str]) -> &'a str {
    blocks
        .iter()
        .copied()
        .min_by_key(|block| tie_key(block))
        .expect("au moins un bloc")
}

// ---------------------------------------------------------------------------------------------
// Trouver.

#[test]
fn a_word_of_a_block_finds_its_page_and_its_block() {
    let b = Bench::new();
    b.page(
        1,
        "Courses",
        &[("b1", "du pain"), ("b2", "des pommes vertes")],
    );
    b.page(2, "Travaux", &[("b1", "repeindre la porte")]);
    b.commit();
    let hits = b.hits("pommes", &pages([1, 2]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].node_id, page(1));
    assert_eq!(hits[0].block_id.as_deref(), Some("b2"));
    assert_eq!(hits[0].excerpt, "des pommes vertes");
    // Le témoin : un mot que rien ne porte ne trouve rien.
    assert!(b.hits("poires", &pages([1, 2])).is_empty());
}

#[test]
fn accents_and_case_do_not_matter() {
    let b = Bench::new();
    b.page(1, "École", &[("b1", "Un Élève appliqué, à l'ŒUVRE")]);
    b.commit();
    let all = pages([1]);
    for raw in [
        "eleve applique",
        "ÉLÈVE",
        "élève",
        "oeuvre",
        "œuvre",
        "ecole eleve",
    ] {
        assert_eq!(b.numbers(raw, &all), [1], "« {raw} »");
    }
}

#[test]
fn every_term_is_wanted() {
    let b = Bench::new();
    b.page(
        1,
        "Recettes",
        &[("b1", "pomme rouge"), ("b2", "poire verte")],
    );
    b.commit();
    let all = pages([1]);
    // Deux mots de deux blocs différents ne trouvent pas la page (Ruling R902).
    assert!(b.hits("pomme verte", &all).is_empty());
    // Dans un même bloc, oui ; un mot du titre et un mot d'un bloc, oui, et c'est le bloc rendu.
    assert_eq!(
        b.hits("rouge pomme", &all)[0].block_id.as_deref(),
        Some("b1")
    );
    assert_eq!(
        b.hits("recettes poire", &all)[0].block_id.as_deref(),
        Some("b2")
    );
    assert!(b.hits("recettes pomme absente", &all).is_empty());
}

#[test]
fn the_last_term_matches_as_a_prefix() {
    let b = Bench::new();
    b.page(1, "Notes", &[("b1", "le développement avance")]);
    b.commit();
    let all = pages([1]);
    assert_eq!(b.numbers("dével", &all), [1]);
    assert_eq!(b.numbers("avance dé", &all), [1]);
    // Un début de mot n'est cherché que pour le dernier mot tapé.
    assert!(b.hits("dével avance", &all).is_empty());
    // Et ce n'est pas un morceau du milieu d'un mot.
    assert!(b.hits("veloppement", &all).is_empty());
}

#[test]
fn a_title_outranks_a_block() {
    let b = Bench::new();
    // La page au mot dans son titre a le plus grand identifiant : à note égale, elle serait
    // seconde.
    b.page(1, "Notes", &[("b1", "le budget est prêt")]);
    b.page(2, "Budget annuel", &[("b1", "rien à dire ici")]);
    b.commit();
    let hits = b.hits("budget", &pages([1, 2]));
    assert_eq!(numbers(&hits), [2, 1]);
    assert_eq!(hits[0].block_id, None);
    assert_eq!(hits[1].block_id.as_deref(), Some("b1"));
}

// ---------------------------------------------------------------------------------------------
// Pages (Ruling R902).

#[test]
fn a_page_is_found_once_whatever_its_blocks() {
    let b = Bench::new();
    b.title(1, "Liste");
    for i in 0..50 {
        b.block(1, &format!("b{i}"), "Liste", "encore une ligne de la liste");
    }
    b.commit();
    assert_eq!(b.numbers("ligne", &pages([1])), [1]);
    assert_eq!(b.numbers("liste", &pages([1])), [1]);
}

#[test]
fn a_title_word_of_a_10000_block_page_does_not_hide_other_pages() {
    let b = Bench::new();
    b.title(1, "Projet");
    for i in 0..10_000 {
        b.block(1, &format!("b{i}"), "Projet", "une ligne parmi dix mille");
    }
    for n in 2..=31 {
        b.page(n, "Projet", &[("b1", "une ligne")]);
    }
    b.commit();
    let all = pages(1..=31);
    let first = b.found("projet", &all, 0, 20);
    let second = b.found("projet", &all, 20, 20);
    assert_eq!(first.pages.len(), 20);
    assert!(first.more);
    assert_eq!(second.pages.len(), 11);
    assert!(!second.more);
    let mut seen = numbers(&first.pages);
    seen.extend(numbers(&second.pages));
    assert_eq!(seen, (1..=31).collect::<Vec<_>>());
}

/// Quarante-cinq pages de même note, de un à trois blocs qui portent le mot.
fn equal_pages() -> (Bench, PageSet) {
    let b = Bench::new();
    for n in 1..=45u128 {
        b.title(n, "Sans rapport");
        for i in 0..=(n % 3) {
            b.block(n, &format!("b{i}"), "Sans rapport", "le même mot partout");
        }
    }
    b.commit();
    (b, pages(1..=45))
}

#[test]
fn paging_never_repeats_a_page() {
    let (b, all) = equal_pages();
    // Deux vraies recherches, pas une liste coupée en deux.
    let first = b.found("partout", &all, 0, 20);
    let second = b.found("partout", &all, 20, 20);
    let whole = b.found("partout", &all, 0, 100);
    let (first, second) = (numbers(&first.pages), numbers(&second.pages));
    assert!(first.iter().all(|n| !second.contains(n)));
    let mut both = first.clone();
    both.extend(second);
    assert_eq!(both, numbers(&whole.pages)[..40]);
    assert_eq!(numbers(&whole.pages), (1..=45).collect::<Vec<_>>());
}

#[test]
fn paging_counts_pages() {
    let (b, all) = equal_pages();
    for (skip, len) in [(0, 20), (20, 20), (40, 5)] {
        assert_eq!(b.found("partout", &all, skip, 20).pages.len(), len);
    }
}

#[test]
fn more_is_true_only_when_another_page_follows() {
    let (b, all) = equal_pages();
    assert!(b.found("partout", &all, 0, 20).more);
    assert!(b.found("partout", &all, 20, 20).more);
    assert!(!b.found("partout", &all, 40, 20).more);
    assert!(b.found("partout", &all, 0, 44).more);
    assert!(!b.found("partout", &all, 0, 45).more);
    assert!(!b.found("partout", &all, 45, 20).more);
    // Avec les seules vingt premières pages permises : rien ne suit la vingtième.
    assert!(!b.found("partout", &pages(1..=20), 0, 20).more);
}

// ---------------------------------------------------------------------------------------------
// Droits (Rulings R916, R921, R922).

#[test]
fn only_allowed_pages_are_found() {
    let b = Bench::new();
    b.page(1, "Publique", &[("b1", "le secret des recettes")]);
    b.page(2, "Privée", &[("b1", "le secret du coffre")]);
    b.page(3, "Secret", &[("b1", "rien")]);
    b.commit();
    assert_eq!(b.numbers("secret", &pages([1])), [1]);
    assert!(b.hits("coffre", &pages([1])).is_empty());
    assert!(b.hits("secret", &pages([])).is_empty());
    // Le témoin : l'ensemble élargi trouve.
    assert_eq!(b.numbers("secret", &pages([1, 2, 3])), [3, 1, 2]);
    assert_eq!(b.numbers("coffre", &pages([1, 2])), [2]);
}

#[test]
fn another_space_is_never_found() {
    let b = Bench::new();
    // L'index n'a pas de champ d'espace (Ruling R921) : seul l'ensemble permis sépare.
    for n in 1..=5 {
        b.page(n, "Ici", &[("b1", "le mot commun")]);
    }
    for n in 101..=105 {
        b.page(n, "Ailleurs", &[("b1", "le mot commun")]);
    }
    b.commit();
    assert_eq!(b.numbers("commun", &pages(1..=5)), [1, 2, 3, 4, 5]);
    assert!(b.hits("ailleurs", &pages(1..=5)).is_empty());
    // Le témoin : avec l'ensemble de l'autre espace, ce sont ses pages.
    assert_eq!(
        b.numbers("commun", &pages(101..=105)),
        [101, 102, 103, 104, 105]
    );
    assert_eq!(b.numbers("ailleurs", &pages(101..=105)).len(), 5);
}

#[test]
fn the_filter_is_applied_in_the_collector_before_grouping() {
    let b = Bench::new();
    for n in 1..=5 {
        b.block(n, "b1", "Vue", "le mot commun");
    }
    for n in 101..=600 {
        b.block(n, "b1", "Non vue", "le mot commun");
    }
    b.commit();
    let (found, trace) = b.traced("commun", &pages(1..=5), None);
    assert_eq!(found.pages.len(), 5);
    // Le témoin : les entrées non permises arrivent bien au collecteur.
    assert_eq!(trace.collected, 505);
    assert_eq!(trace.grouped, 5);
}

#[test]
fn what_unseen_pages_hold_changes_neither_the_hits_nor_more() {
    let b = Bench::new();
    for n in 1..=25 {
        b.page(
            n,
            "Vue",
            &[("b1", "le mot commun"), ("b2", "commun encore")],
        );
    }
    b.commit();
    let allowed = pages(1..=25);
    let before = b.found("commun", &allowed, 0, 20);
    assert_eq!(before.pages.len(), 20);
    assert!(before.more);
    // Des pages non permises, mieux notées (le mot est dans leur titre), d'identifiants plus
    // petits et plus grands.
    for n in (0..1).chain(26..=1_025) {
        b.page(
            n,
            "Commun",
            &[("b1", "commun commun commun"), ("b0", "commun")],
        );
    }
    b.commit();
    assert_eq!(b.found("commun", &allowed, 0, 20), before);
    let after = b.found("commun", &allowed, 20, 20);
    assert_eq!(after.pages.len(), 5);
    assert!(!after.more);
    // Le témoin : ces pages sont bien dans l'index, et passeraient toutes devant.
    let wider = b.found("commun", &pages(0..=1_025), 0, 100);
    assert!(wider.more);
    assert_eq!(numbers(&wider.pages)[0], 0);
    assert!(numbers(&wider.pages).iter().all(|n| !(1..=25).contains(n)));
}

/// Deux blocs permis dosés (trois « alpha » et un « omega », puis l'inverse), et ce que portent
/// des pages non permises.
fn dosed(others: &[(&str, bool)]) -> Bench {
    let b = Bench::new();
    b.block(1, "b1", "Dosé", "alpha alpha alpha omega");
    b.block(2, "b1", "Dosé", "alpha omega omega omega");
    for (round, (word, removed)) in others.iter().enumerate() {
        let from = 1_000 * (round as u128 + 1);
        for n in 0..10_000 {
            b.block(from * 1_000 + n, "b1", "Autre", word);
        }
        b.commit();
        if *removed {
            // Supprimées sans fusion : elles restent dans les fichiers de l'index.
            for n in 0..10_000 {
                b.index
                    .remove_page(page(from * 1_000 + n))
                    .expect("retirer");
            }
        }
    }
    b.commit();
    b
}

#[test]
fn what_unseen_pages_hold_changes_no_order() {
    let allowed = pages([1, 2]);
    let alone = dosed(&[]);
    let alpha = dosed(&[("alpha", false)]);
    let omega = dosed(&[("omega", false)]);
    let removed = dosed(&[("alpha", true)]);
    let order = |b: &Bench, witness| numbers(&b.traced("alpha omega", &allowed, witness).0.pages);
    let wanted = order(&alone, None);
    assert_eq!(wanted.len(), 2);
    for (what, b) in [("alpha", &alpha), ("omega", &omega), ("retirées", &removed)] {
        assert_eq!(order(b, None), wanted, "pages non permises : {what}");
    }
    // Le témoin : une note qui lit les statistiques de l'index range selon ce que les autres
    // écrivent, entrées supprimées comprises.
    let stats = Some(Witness::IndexStatistics);
    assert_eq!(order(&alpha, stats), [2, 1]);
    assert_eq!(order(&omega, stats), [1, 2]);
    assert_eq!(order(&removed, stats), [2, 1]);
}

const TIED_BLOCKS: [&str; 3] = ["b1", "b2", "b3"];

/// Douze pages permises de trois blocs de même note. `mixed` : écrites dans l'ordre inverse,
/// mêlées de pages non permises, en plusieurs validations, puis fusionnées.
fn tied(mixed: bool) -> Bench {
    let b = Bench::new();
    if !mixed {
        for n in 1..=12 {
            for block in TIED_BLOCKS {
                b.block(n, block, "Fruits", "pomme verte");
            }
        }
        b.commit();
        return b;
    }
    for n in (1..=12).rev() {
        for block in TIED_BLOCKS.iter().rev() {
            b.block(100 + 2 * n, block, "Fruits", "pomme verte");
            b.block(n, block, "Fruits", "pomme verte");
        }
        b.block(n + 50, "b1", "Pomme", "pomme pomme");
        if n % 4 == 0 {
            b.commit();
        }
    }
    b.commit();
    b.index.merge_all().expect("fusionner");
    assert_eq!(b.index.segments(), 1);
    b
}

#[test]
fn equal_notes_keep_their_order_whatever_the_index_holds() {
    let allowed = pages(1..=12);
    let (plain, mixed) = (tied(false), tied(true));
    let found = plain.found("pomme", &allowed, 0, 100);
    assert_eq!(numbers(&found.pages), (1..=12).collect::<Vec<_>>());
    let winner = first_by_key(&TIED_BLOCKS);
    for hit in &found.pages {
        assert_eq!(hit.block_id.as_deref(), Some(winner));
    }
    assert_eq!(mixed.found("pomme", &allowed, 0, 100), found);
    // Le témoin : un départage par l'adresse d'une entrée dans l'index suit l'ordre d'écriture.
    let by_address = |b: &Bench| {
        b.traced("pomme", &allowed, Some(Witness::AddressTieBreak))
            .0
    };
    assert_ne!(by_address(&mixed), by_address(&plain));
}

#[test]
fn a_block_inserted_before_another_changes_no_tie_break() {
    let blocks = ["bloc-a", "bloc-b", "bloc-c", "bloc-d"];
    let winner = first_by_key(&blocks);
    // Les mêmes blocs écrits dans les deux ordres : le rang n'y est pour rien.
    for reversed in [false, true] {
        let b = Bench::new();
        let mut order = blocks.to_vec();
        if reversed {
            order.reverse();
        }
        for block in order {
            b.block(1, block, "Page", "le même texte");
        }
        b.commit();
        let all = pages([1]);
        assert_eq!(b.hits("texte", &all)[0].block_id.as_deref(), Some(winner));
        // Un bloc inséré avant les autres, qui ne porte pas le mot, ne change rien ; un bloc
        // récrit non plus.
        b.block(1, "bloc-0", "Page", "autre chose");
        b.block(1, blocks[0], "Page", "le même texte");
        b.commit();
        assert_eq!(b.hits("texte", &all)[0].block_id.as_deref(), Some(winner));
    }
}

#[test]
fn the_title_entry_wins_a_tie_in_its_page() {
    let b = Bench::new();
    // Les blocs d'abord, le titre ensuite : l'ordre d'écriture n'y est pour rien.
    for i in 0..20 {
        b.block(1, &format!("b{i}"), "Vacances", "sans rapport");
    }
    b.title(1, "Vacances");
    b.commit();
    let hits = b.hits("vacances", &pages([1]));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].block_id, None);
    assert_eq!(hits[0].excerpt, "");
    assert!(hits[0].marks.is_empty());
}

/// Les fichiers de `src/`, leurs lignes.
fn sources() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut lines = Vec::new();
    for entry in std::fs::read_dir(src).expect("lire src/") {
        let path = entry.expect("une entrée").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        for line in std::fs::read_to_string(&path).expect("lire").lines() {
            lines.push((name.clone(), line.trim().to_string()));
        }
    }
    lines
}

#[test]
fn no_search_path_reads_the_statistics_of_the_index() {
    let lines = sources();
    let code = || {
        lines
            .iter()
            .filter(|(_, line)| !line.starts_with("//"))
            .map(|(_, line)| line.as_str())
    };
    // Ni le générateur d'extraits du moteur, ni un de ses collecteurs qui notent, ni une note
    // bâtie à la main des statistiques de l'index (Ruling R923).
    for forbidden in [
        "SnippetGenerator",
        "snippet",
        "TopDocs",
        "TopScore",
        "MultiCollector",
        "DocSetCollector",
        "FacetCollector",
        "Bm25Weight",
        ".doc_freq(",
        ".num_docs(",
        ".total_num_tokens(",
        "search_with_executor",
        "QueryParser",
    ] {
        let found: Vec<_> = code().filter(|line| line.contains(forbidden)).collect();
        assert!(found.is_empty(), "« {forbidden} » dans src/ : {found:?}");
    }
    // Du module des collecteurs, seuls les deux traits qu'implémente le nôtre.
    let imports: Vec<_> = code()
        .filter(|line| line.contains("tantivy::collector"))
        .collect();
    assert_eq!(
        imports,
        ["use tantivy::collector::{Collector, SegmentCollector};"]
    );
    // `Searcher::search` prend les statistiques de l'index : une liste fermée d'une seule
    // place, le témoin de `what_unseen_pages_hold_changes_no_order`, qui n'est pas compilé dans
    // le serveur. C'est aussi le témoin de ce test : il voit bien cet appel.
    let searches: Vec<_> = code().filter(|line| line.contains(".search(")).collect();
    assert_eq!(
        searches,
        ["Some(Witness::IndexStatistics) => searcher.search(&engine, &collector),"]
    );
    let hooked = lines
        .iter()
        .position(|(_, line)| line.contains("searcher.search(&engine"))
        .expect("le témoin");
    assert!(
        lines[..hooked]
            .iter()
            .rev()
            .take(4)
            .any(|(_, line)| line == "#[cfg(feature = \"test-hooks\")]"),
        "le témoin doit rester sous la fonction `test-hooks`"
    );
    // La recherche passe par le fournisseur de statistiques constantes.
    assert_eq!(
        code()
            .filter(|line| line.contains("search_with_statistics_provider("))
            .count(),
        1
    );
}

// ---------------------------------------------------------------------------------------------
// Le dernier terme (Rulings R920, R929, R930).

#[test]
fn a_whole_word_outranks_a_longer_word() {
    let b = Bench::new();
    // Le mot plus long est dans la page de plus petit identifiant, et dans un texte plus court.
    b.block(1, "b1", "Animaux", "chaton");
    b.block(2, "b1", "Animaux", "le chat de la voisine dort sur le toit");
    b.commit();
    assert_eq!(b.numbers("chat", &pages([1, 2])), [2, 1]);
}

#[test]
fn a_prefix_alone_ranks_titles_first_then_by_page() {
    let b = Bench::new();
    b.page(
        1,
        "Notes",
        &[("b1", "un chaton, deux chatons, trois chatons")],
    );
    b.page(2, "Notes", &[("b1", "chaton")]);
    b.page(3, "Chatons du quartier", &[("b1", "rien")]);
    b.page(4, "Notes", &[("b1", "le chaton de la voisine dort encore")]);
    b.commit();
    // Limite L-q : hors titre, un début de mot a une note constante, et l'ordre est celui des
    // identifiants de page.
    assert_eq!(b.numbers("chato", &pages(1..=4)), [3, 1, 2, 4]);
}

#[test]
fn a_prefix_costs_one_term_whatever_the_dictionary_holds() {
    let b = Bench::new();
    b.page(1, "Notes", &[("b1", "le chaton dort")]);
    b.commit();
    let allowed = pages([1]);
    let (found, trace) = b.traced("chat", &allowed, None);
    assert_eq!(found.pages.len(), 1);
    // Cent mille mots d'un autre espace qui commencent pareil.
    let mut n = 0u32;
    for block in 0..20 {
        let mut text = String::new();
        for _ in 0..5_000 {
            text.push_str(&format!("chat{n:06} "));
            n += 1;
        }
        b.block(500, &format!("b{block}"), "Ailleurs", &text);
    }
    b.commit();
    // Le témoin : ces mots sont bien dans le dictionnaire des débuts de mot.
    assert!(b.index.terms_of("text_starts").expect("compter") > 100_000);
    let (after, traced) = b.traced("chat", &allowed, None);
    assert_eq!(after, found);
    // Quatre termes exacts : le mot entier et le début, dans le titre et dans le texte.
    assert_eq!(trace.terms, 4);
    assert_eq!(traced.terms, 4);
    assert_eq!(b.traced("le chat", &allowed, None).1.terms, 6);
}

#[test]
fn a_word_gives_at_most_nine_starts() {
    let b = Bench::new();
    b.block(1, "b1", "", &"a".repeat(40));
    b.commit();
    assert_eq!(b.index.terms_of("text_starts").expect("compter"), 9);
    assert_eq!(b.index.terms_of("text").expect("compter"), 1);
    assert_eq!(b.index.terms_of("title_starts").expect("compter"), 0);
    let b = Bench::new();
    // Deux débuts pour un mot de trois lettres, aucun pour une lettre seule ; les débuts se
    // comptent en caractères, pas en octets.
    b.block(1, "b1", "é", "été");
    b.commit();
    assert_eq!(b.index.terms_of("text_starts").expect("compter"), 2);
    assert_eq!(b.index.terms_of("title_starts").expect("compter"), 0);
    assert_eq!(b.index.terms_of("title").expect("compter"), 1);
}

#[test]
fn a_prefix_of_ten_characters_is_one_exact_term() {
    let b = Bench::new();
    b.block(1, "b1", "Notes", "le développement");
    b.block(2, "b1", "Notes", "les développeurs");
    b.commit();
    let all = pages([1, 2]);
    // Dix caractères une fois pliés : « developpem ».
    let (found, trace) = b.traced("développem", &all, None);
    assert_eq!(numbers(&found.pages), [1]);
    assert!(trace.verified.is_empty());
    assert_eq!(trace.terms, 4);
    assert_eq!(b.numbers("développe", &all), [1, 2]);
}

#[test]
fn a_longer_prefix_is_checked_on_the_kept_text() {
    let b = Bench::new();
    // Les deux mots partagent leurs douze premiers caractères.
    b.block(1, "b1", "Notes", "des accords internationaux");
    b.block(2, "b1", "Notes", "agir internationalement");
    b.block(3, "b1", "Notes", "une règle internationale");
    b.commit();
    let all = pages(1..=3);
    let (found, trace) = b.traced("internationale", &all, None);
    // Le mot entier d'abord (Ruling R930), puis le mot plus long.
    assert_eq!(numbers(&found.pages), [3, 2]);
    assert_eq!(trace.verified.len(), 3);
    assert_eq!(found.pages[1].excerpt, "agir internationalement");
    assert_eq!(found.pages[1].marks, [(5, 23)]);
    // Le témoin : le début commun, plus long que dix caractères lui aussi, les trouve tous.
    assert_eq!(b.numbers("internationa", &all), [1, 2, 3]);
    // Plié comme le reste.
    assert_eq!(b.numbers("INTERNATIONALEM", &all), [2]);
}

/// Des pages d'un bloc qui portent toutes un mot de même début long.
fn sharing_a_long_start(b: &Bench, of: impl Iterator<Item = u128>) {
    for n in of {
        b.block(n, "b1", "Notes", "agir internationalement");
    }
}

#[test]
fn the_check_of_a_long_prefix_is_bounded_and_counts_allowed_entries_only() {
    let b = Bench::new();
    sharing_a_long_start(&b, 1..=2_500);
    b.commit();
    let allowed = pages(1..=2_500);
    let (found, trace) = b.traced("internationalem", &allowed, None);
    assert_eq!(trace.verified.len(), MAX_VERIFIED_ENTRIES);
    assert_eq!(found.pages.len(), 100);
    // Sous la borne, tout est vérifié : c'est le témoin de ce que la borne coupe.
    let few = pages(1..=1_500);
    assert_eq!(
        b.traced("internationalem", &few, None).1.verified.len(),
        1_500
    );
    // Des entrées non permises qui partagent le préfixe, avant et après les permises.
    sharing_a_long_start(&b, 5_000..6_500);
    b.block(0, "b1", "Internationalement", "internationalement");
    b.commit();
    let (again, traced) = b.traced("internationalem", &allowed, None);
    assert_eq!(traced.collected, trace.collected + 1_501);
    assert_eq!(traced.verified, trace.verified);
    assert_eq!(again, found);
    assert_eq!(
        b.traced("internationalem", &few, None).1.verified.len(),
        1_500
    );
}

#[test]
fn a_long_prefix_in_a_title_is_checked_on_the_kept_title() {
    let b = Bench::new();
    b.page(1, "Accords internationaux", &[("b1", "le budget")]);
    b.page(2, "Agir internationalement", &[("b1", "le budget")]);
    b.commit();
    let all = pages([1, 2]);
    let hits = b.hits("internationalem", &all);
    assert_eq!(numbers(&hits), [2]);
    assert_eq!(hits[0].block_id, None);
    // Un mot du bloc et le préfixe long dans le titre : la page, par son bloc.
    let hits = b.hits("budget internationalem", &all);
    assert_eq!(numbers(&hits), [2]);
    assert_eq!(hits[0].block_id.as_deref(), Some("b1"));
    // Le témoin : le début commun trouve les deux titres.
    assert_eq!(b.numbers("budget internationa", &all), [1, 2]);
}

#[test]
fn the_checked_entries_do_not_depend_on_the_order_of_the_index() {
    let build = |mixed: bool| {
        let b = Bench::new();
        let order: Vec<u128> = if mixed {
            (1..=150).rev().collect()
        } else {
            (1..=150).collect()
        };
        for (i, n) in order.iter().enumerate() {
            for k in 0..30 {
                let block = if mixed { 29 - k } else { k };
                b.block(*n, &format!("b{block}"), "Notes", "agir internationalement");
                if mixed {
                    b.block(
                        1_000 + n,
                        &format!("b{block}"),
                        "Notes",
                        "internationalement",
                    );
                }
            }
            if mixed && i % 40 == 39 {
                b.commit();
            }
        }
        b.commit();
        if mixed {
            b.index.merge_all().expect("fusionner");
        }
        b
    };
    let allowed = pages(1..=150);
    let (plain, plain_trace) = build(false).traced("internationalem", &allowed, None);
    let (mixed, mixed_trace) = build(true).traced("internationalem", &allowed, None);
    assert_eq!(plain_trace.verified.len(), MAX_VERIFIED_ENTRIES);
    assert_eq!(mixed_trace.verified, plain_trace.verified);
    assert_eq!(mixed, plain);
    // Vingt entrées par page au plus, et les mêmes : celles de plus petite clé.
    let first: Vec<_> = plain_trace
        .verified
        .iter()
        .filter(|(node, _)| *node == page(1))
        .map(|(_, block)| block.clone().expect("un bloc"))
        .collect();
    let mut wanted: Vec<String> = (0..30).map(|k| format!("b{k}")).collect();
    wanted.sort_by_key(|block| tie_key(block));
    wanted.truncate(MAX_VERIFIED_PER_PAGE);
    assert_eq!(first, wanted);
}

#[test]
fn a_page_of_10000_candidates_does_not_take_all_the_checks() {
    let b = Bench::new();
    // La page de plus petit identifiant : dix mille blocs qui partagent les dix premiers
    // caractères du préfixe, sans le porter.
    for i in 0..10_000 {
        b.block(1, &format!("b{i}"), "Notes", "des accords internationaux");
    }
    sharing_a_long_start(&b, 2..=31);
    b.commit();
    let (found, trace) = b.traced("internationalem", &pages(1..=31), None);
    assert_eq!(numbers(&found.pages), (2..=31).collect::<Vec<_>>());
    let of_the_big_page = trace
        .verified
        .iter()
        .filter(|(node, _)| *node == page(1))
        .count();
    assert_eq!(of_the_big_page, MAX_VERIFIED_PER_PAGE);
    assert_eq!(trace.verified.len(), MAX_VERIFIED_PER_PAGE + 30);
}

#[test]
fn the_check_of_a_long_prefix_is_bounded_in_bytes() {
    let b = Bench::new();
    // Des blocs de 64 Kio qui partagent le début, sans porter le préfixe.
    let long = "internationaux ".repeat(4_370);
    for n in 1..=100 {
        b.block(n, "b1", "Notes", &long);
    }
    b.commit();
    let (found, trace) = b.traced("internationalem", &pages(1..=100), None);
    assert!(found.pages.is_empty());
    // La borne est passée par la soixante-quatrième entrée, la dernière relue.
    let read = |entries: usize| entries * (long.len() + "Notes".len());
    assert!(read(63) < MAX_VERIFIED_BYTES && read(64) >= MAX_VERIFIED_BYTES);
    assert_eq!(trace.verified.len(), 64);
    // Le témoin : sous la borne, toutes sont relues.
    let (_, trace) = b.traced("internationalem", &pages(1..=60), None);
    assert_eq!(trace.verified.len(), 60);
}

#[test]
fn every_note_is_a_sum_on_the_grid() {
    let b = Bench::new();
    // Des longueurs et des fréquences variées, dans le titre et dans le texte.
    for n in 1..=40u128 {
        let filler = "mot ".repeat(n as usize * 3);
        let text = format!(
            "{}{filler}{}budgets internationalement",
            "alpha ".repeat(1 + n as usize % 5),
            "omega ".repeat(1 + n as usize % 3),
        );
        let title = if n % 2 == 0 { "Alpha" } else { "Notes omega" };
        b.page(n, title, &[("b1", &text)]);
    }
    b.commit();
    let all = pages(1..=40);
    for raw in [
        "alpha",
        "alpha omega mot",
        "omega alpha budg",
        "mot internationalem",
    ] {
        let (found, trace) = b.traced(raw, &all, None);
        assert_eq!(found.pages.len(), 40, "« {raw} »");
        let mut distinct = HashSet::new();
        for note in &trace.notes {
            // Un multiple du pas de la grille (1/4096) : les sommes de notes sont exactes, et
            // ne dépendent donc pas de l'ordre où le moteur les additionne.
            let steps = note * 4096.0;
            assert!(steps > 0.0 && steps == steps.round(), "« {raw} » : {note}");
            distinct.insert(note.to_bits());
        }
        // Le témoin : ce ne sont pas des notes constantes.
        assert!(distinct.len() > 10, "« {raw} » : {} notes", distinct.len());
    }
}

// ---------------------------------------------------------------------------------------------
// Mots (Ruling R926).

#[test]
fn text_in_nfd_is_found_like_text_in_nfc() {
    let nfd = "e\u{301}le\u{300}ve";
    let nfc = "élève";
    assert_ne!(nfd, nfc);
    let b = Bench::new();
    b.page(1, &format!("L'{nfd} du jour"), &[("b1", "rien")]);
    b.page(2, "Notes", &[("b1", &format!("un {nfd} sage"))]);
    b.page(3, "Notes", &[("b1", &format!("un {nfc} sage"))]);
    b.commit();
    let all = pages(1..=3);
    for raw in [nfd, nfc, "eleve"] {
        assert_eq!(b.numbers(raw, &all), [1, 2, 3], "« {raw} »");
    }
    // Le texte gardé est en NFC : les plages de l'extrait y tombent juste.
    let hit = &b.hits(&format!("{nfd} sage"), &all)[0];
    assert_eq!(hit.excerpt, "un élève sage");
    assert_eq!(hit.marks, [(3, 8), (9, 13)]);
}

#[test]
fn a_word_of_40_bytes_is_indexed_and_a_longer_one_is_not() {
    let (kept, dropped) = ("k".repeat(40), "d".repeat(41));
    let b = Bench::new();
    b.block(1, "b1", "Notes", &format!("avant {kept} après"));
    b.block(2, "b1", "Notes", &format!("avant {dropped} après"));
    b.commit();
    let all = pages([1, 2]);
    assert_eq!(b.numbers(&kept, &all), [1]);
    assert_eq!(b.numbers(&format!("{kept} avant"), &all), [1]);
    assert!(b.hits(&dropped, &all).is_empty());
    assert!(b.hits(&"d".repeat(10), &all).is_empty());
    // Le témoin : le bloc au mot trop long est bien là, trouvé par ses autres mots.
    let mut both = b.numbers("après", &all);
    both.sort();
    assert_eq!(both, [1, 2]);
    // Un mot plié peut dépasser 40 octets (Ruling R947) : « Ⱥ », deux octets, en fait trois.
    let folded_longer = "Ⱥ".repeat(20);
    b.block(3, "b1", "Notes", &folded_longer);
    b.commit();
    assert_eq!(b.numbers(&folded_longer, &pages([3])), [3]);
}

mod known_limits {
    use super::*;

    #[test]
    fn a_run_of_cjk_is_one_word() {
        let b = Bench::new();
        b.block(1, "b1", "Notes", "東京都渋谷区の天気");
        b.commit();
        let all = pages([1]);
        // Le début de la suite la trouve : c'est un mot.
        assert_eq!(b.numbers("東京都", &all), [1]);
        let mut unmet = Vec::new();
        if b.hits("渋谷", &all).is_empty() {
            unmet.push("« 渋谷 » trouve le bloc « 東京都渋谷区の天気 »".to_string());
        }
        limit::known_limit(
            "limite L-p : une suite de caractères chinois, japonais ou coréens sans espace est \
             un seul mot (Ruling R926) ; la lever demande un découpage propre à ces écritures",
            &unmet,
            &["« 渋谷 » trouve le bloc « 東京都渋谷区の天気 »".to_string()],
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Écrire.

#[test]
fn a_removed_entry_is_no_longer_found() {
    let b = Bench::new();
    b.page(1, "Notes", &[("b1", "un mot rare"), ("b2", "autre chose")]);
    b.commit();
    let all = pages([1]);
    assert_eq!(b.numbers("rare", &all), [1]);
    b.index.remove_entry(page(1), Some("b1")).expect("retirer");
    b.commit();
    assert!(b.hits("rare", &all).is_empty());
    assert_eq!(b.numbers("autre", &all), [1]);
    // L'entrée de titre se retire de même, sans les blocs.
    b.index.remove_entry(page(1), None).expect("retirer");
    b.commit();
    let hits = b.hits("notes", &all);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].block_id.as_deref(), Some("b2"));
}

#[test]
fn putting_an_entry_again_replaces_it() {
    let b = Bench::new();
    b.block(1, "b1", "Notes", "le premier texte");
    b.commit();
    b.block(1, "b1", "Notes", "le second texte");
    // Deux fois dans la même validation aussi.
    b.block(1, "b2", "Notes", "un brouillon");
    b.block(1, "b2", "Notes", "une version finale");
    b.commit();
    let all = pages([1]);
    assert!(b.hits("premier", &all).is_empty());
    assert!(b.hits("brouillon", &all).is_empty());
    assert_eq!(b.hits("second", &all)[0].excerpt, "le second texte");
    assert_eq!(b.hits("finale", &all)[0].excerpt, "une version finale");
    let (_, trace) = b.traced("texte", &all, None);
    assert_eq!(trace.collected, 1);
    // Rien n'est trouvé avant la validation.
    b.block(1, "b3", "Notes", "pas encore validé");
    assert!(b.hits("validé", &all).is_empty());
    b.commit();
    assert_eq!(b.numbers("validé", &all), [1]);
}

#[test]
fn removing_a_page_removes_all_its_entries() {
    let b = Bench::new();
    b.page(1, "Partie", &[("b1", "un mot"), ("b2", "un autre mot")]);
    b.page(2, "Restée", &[("b1", "un mot")]);
    b.commit();
    let all = pages([1, 2]);
    assert_eq!(b.numbers("mot", &all), [1, 2]);
    b.index.remove_page(page(1)).expect("retirer");
    b.commit();
    assert_eq!(b.numbers("mot", &all), [2]);
    assert!(b.hits("partie", &all).is_empty());
    assert_eq!(b.traced("mot", &all, None).1.collected, 1);
    // Une page renommée : retirée puis récrite dans la même validation, elle n'est trouvée que
    // par ce qui est écrit après le retrait.
    b.index.remove_page(page(2)).expect("retirer");
    b.page(2, "Renommée", &[("b1", "un mot")]);
    b.commit();
    assert_eq!(b.numbers("mot", &all), [2]);
    assert_eq!(b.traced("mot", &all, None).1.collected, 1);
    assert_eq!(b.numbers("renommée", &all), [2]);
    assert!(b.hits("restée", &all).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Extrait (bâti par programme, Ruling R923).

fn utf16(text: &str, (from, to): (u32, u32)) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16(&units[from as usize..to as usize]).expect("une plage entre caractères")
}

#[test]
fn the_excerpt_is_text_with_utf16_marks() {
    let b = Bench::new();
    let text = "🙂 Bonjour le <b>Monde</b> & le monde entier";
    b.block(1, "b1", "Notes", text);
    b.commit();
    let hit = &b.hits("bonjour monde", &pages([1]))[0];
    // Le texte tel quel : aucune balise ajoutée, rien d'échappé.
    assert_eq!(hit.excerpt, text);
    // L'emoji compte pour deux unités, comme dans une chaîne JavaScript.
    assert_eq!(hit.marks, [(3, 10), (17, 22), (32, 37)]);
    let marked: Vec<_> = hit.marks.iter().map(|m| utf16(&hit.excerpt, *m)).collect();
    assert_eq!(marked, ["Bonjour", "Monde", "monde"]);
    // Un début de mot marque le mot entier ; un mot trouvé par le titre seul ne marque rien.
    let hit = &b.hits("entier bonj", &pages([1]))[0];
    let marked: Vec<_> = hit.marks.iter().map(|m| utf16(&hit.excerpt, *m)).collect();
    assert_eq!(marked, ["Bonjour", "entier"]);
    let hit = &b.hits("notes entier", &pages([1]))[0];
    assert_eq!(hit.marks.len(), 1);
}

#[test]
fn the_excerpt_is_bounded() {
    let b = Bench::new();
    let filler = "remplissage é🙂 ".repeat(4_000);
    b.block(
        1,
        "b1",
        "Notes",
        &format!("{filler}le trésor caché {filler}"),
    );
    b.block(2, "b1", "Notes", &format!("trésor {filler}"));
    b.block(3, "b1", "Notes", &format!("{} ", "x".repeat(40)).repeat(50));
    b.block(4, "b1", "Trésor", &filler);
    b.commit();
    let all = pages(1..=4);
    for raw in ["trésor", "xxxxxxxxxx", "remplissage tré"] {
        for hit in b.hits(raw, &all) {
            assert!(
                hit.excerpt.chars().count() <= MAX_EXCERPT_CHARS,
                "« {raw} » : {} caractères",
                hit.excerpt.chars().count()
            );
            let units = hit.excerpt.encode_utf16().count() as u32;
            let mut end = 0;
            for (from, to) in &hit.marks {
                assert!(end <= *from && from < to && *to <= units);
                end = *to;
                // Une plage tombe entre deux caractères.
                utf16(&hit.excerpt, (*from, *to));
            }
        }
    }
    // Le mot trouvé est dans l'extrait, où qu'il soit dans le texte.
    let hits = b.hits("trésor caché", &all);
    assert_eq!(numbers(&hits), [1]);
    let marked: Vec<_> = hits[0]
        .marks
        .iter()
        .map(|m| utf16(&hits[0].excerpt, *m))
        .collect();
    assert_eq!(marked, ["trésor", "caché"]);
    // Un texte trouvé par son titre : son début, sans plage.
    let hits = b.hits("trésor remplissage", &pages([4]));
    assert!(hits[0].excerpt.starts_with("remplissage é🙂 remplissage"));
}

// ---------------------------------------------------------------------------------------------
// Jeton (Ruling R909), dossier, verrou.

fn open(dir: &Path) -> (SearchIndex, Option<String>) {
    let opened = SearchIndex::open(dir).expect("ouvrir");
    (opened.index, opened.token)
}

fn finds(index: &SearchIndex, raw: &str, allowed: &PageSet) -> usize {
    index
        .search(&query(raw), allowed, 0, 100)
        .expect("chercher")
        .pages
        .len()
}

#[test]
fn the_token_of_the_last_commit_is_read_back() {
    let dir = TestDir::new();
    let (index, token) = open(dir.path());
    assert_eq!(token, None);
    index
        .put_entry(page(1), Some("b1"), "Notes", "un mot")
        .unwrap();
    index.commit("premier").unwrap();
    index.commit("second jeton, avec des espaces").unwrap();
    drop(index);
    let (index, token) = open(dir.path());
    assert_eq!(token.as_deref(), Some("second jeton, avec des espaces"));
    assert_eq!(finds(&index, "mot", &pages([1])), 1);
}

#[test]
fn a_commit_cut_short_reads_the_token_before_it() {
    let dir = TestDir::new();
    let (index, _) = open(dir.path());
    index
        .put_entry(page(1), Some("b1"), "Notes", "validé")
        .unwrap();
    index.commit("avant").unwrap();
    // Écrit, jamais validé : le processus s'arrête avant.
    index
        .put_entry(page(1), Some("b2"), "Notes", "perdu")
        .unwrap();
    index.remove_entry(page(1), Some("b1")).unwrap();
    drop(index);
    let (index, token) = open(dir.path());
    assert_eq!(token.as_deref(), Some("avant"));
    assert_eq!(finds(&index, "validé", &pages([1])), 1);
    assert_eq!(finds(&index, "perdu", &pages([1])), 0);
}

#[test]
fn a_missing_unreadable_or_other_schema_directory_reads_no_token() {
    let root = TestDir::new();
    // Absent : il est créé, avec ses dossiers parents.
    let missing = root.path().join("données").join("search");
    let (index, token) = open(&missing);
    assert_eq!(token, None);
    index
        .put_entry(page(1), Some("b1"), "Notes", "un mot")
        .unwrap();
    index.commit("jeton").unwrap();
    drop(index);
    // Le témoin : rouvert tel quel, il rend son jeton.
    let (index, token) = open(&missing);
    assert_eq!(token.as_deref(), Some("jeton"));
    drop(index);

    // Illisible : ses métadonnées ne sont plus du JSON.
    std::fs::write(missing.join("meta.json"), b"{ ceci n'est pas").unwrap();
    let (index, token) = open(&missing);
    assert_eq!(token, None);
    assert_eq!(finds(&index, "mot", &pages([1])), 0);
    index.commit("refait").unwrap();
    drop(index);
    assert_eq!(open(&missing).1.as_deref(), Some("refait"));

    // D'un autre schéma : un index de Tantivy qui n'est pas le nôtre, avec une charge utile.
    let other = root.path().join("autre");
    std::fs::create_dir_all(&other).unwrap();
    {
        let mut schema = tantivy::schema::Schema::builder();
        let text = schema.add_text_field("text", tantivy::schema::TEXT);
        let index = tantivy::Index::create_in_dir(&other, schema.build()).unwrap();
        let mut writer: tantivy::IndexWriter = index.writer(15_000_000).unwrap();
        let mut doc = tantivy::TantivyDocument::default();
        doc.add_text(text, "un mot");
        writer.add_document(doc).unwrap();
        let mut prepared = writer.prepare_commit().unwrap();
        prepared.set_payload("jeton d'un autre");
        prepared.commit().unwrap();
        writer.wait_merging_threads().unwrap();
    }
    let (index, token) = open(&other);
    assert_eq!(token, None);
    assert_eq!(finds(&index, "mot", &pages([1])), 0);
    index
        .put_entry(page(1), Some("b1"), "Notes", "un mot")
        .unwrap();
    index.commit("le nôtre").unwrap();
    assert_eq!(finds(&index, "mot", &pages([1])), 1);
}

/// Pose à `at` un lien vers le dossier `target` : un lien symbolique sous Unix ; sous Windows,
/// où un lien symbolique demande un privilège que le poste n'a pas toujours, une jonction.
fn link_dir(target: &Path, at: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, at).unwrap();
    #[cfg(windows)]
    {
        let out = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(at)
            .arg(target)
            .output()
            .unwrap();
        assert!(out.status.success(), "mklink /J : {out:?}");
    }
}

#[test]
fn reset_removes_a_link_without_following_it() {
    let root = TestDir::new();
    let search = root.path().join("search");
    let outside = root.path().join("ailleurs");
    std::fs::create_dir_all(outside.join("sous-dossier")).unwrap();
    std::fs::write(outside.join("gardé.txt"), b"garde").unwrap();
    std::fs::write(outside.join("sous-dossier").join("gardé.txt"), b"garde").unwrap();
    let (index, _) = open(&search);
    index
        .put_entry(page(1), Some("b1"), "Notes", "un mot")
        .unwrap();
    index.commit("jeton").unwrap();
    drop(index);
    link_dir(&outside, &search.join("lien"));
    std::fs::create_dir_all(search.join("vrai-dossier")).unwrap();
    std::fs::write(search.join("vrai-dossier").join("reste.bin"), b"x").unwrap();
    // Le témoin : le lien mène bien au fichier.
    assert!(search.join("lien").join("gardé.txt").exists());

    let opened = SearchIndex::reset(&search).expect("vider et rouvrir");
    assert_eq!(opened.token, None);
    assert_eq!(finds(&opened.index, "mot", &pages([1])), 0);
    assert!(std::fs::symlink_metadata(search.join("lien")).is_err());
    assert!(!search.join("vrai-dossier").exists());
    assert_eq!(std::fs::read(outside.join("gardé.txt")).unwrap(), b"garde");
    assert!(outside.join("sous-dossier").join("gardé.txt").exists());
    // L'index rouvert sert.
    opened
        .index
        .put_entry(page(2), Some("b1"), "Notes", "un mot")
        .unwrap();
    opened.index.commit("neuf").unwrap();
    assert_eq!(finds(&opened.index, "mot", &pages([1, 2])), 1);
}

#[test]
fn a_second_writer_is_refused_with_a_message() {
    let dir = TestDir::new();
    let (index, _) = open(dir.path());
    index
        .put_entry(page(1), Some("b1"), "Notes", "un mot")
        .unwrap();
    index.commit("jeton").unwrap();
    for refused in [
        SearchIndex::open(dir.path()),
        SearchIndex::reset(dir.path()),
    ] {
        let error = refused.err().expect("un second écrivain est refusé");
        assert!(matches!(error, IndexError::Busy), "{error:?}");
        let message = error.to_string();
        assert!(message.contains("déjà ouvert"), "{message}");
        // Aucun chemin du serveur dans le message.
        assert!(!message.contains(&*dir.path().to_string_lossy()));
    }
    // Rien n'a été vidé par le refus, et le premier sert toujours.
    assert_eq!(finds(&index, "mot", &pages([1])), 1);
    drop(index);
    // Le témoin : le premier lâché, l'index s'ouvre, avec son jeton.
    let (index, token) = open(dir.path());
    assert_eq!(token.as_deref(), Some("jeton"));
    assert_eq!(finds(&index, "mot", &pages([1])), 1);
}

#[test]
fn the_page_set_holds_what_it_was_given() {
    let set = pages([1, 2, 2, 3]);
    assert_eq!(set.len(), 3);
    assert!(set.contains(&page(2)));
    assert!(!set.contains(&page(4)));
    assert!(pages([]).is_empty());
    let seen: HashSet<Uuid> = [page(1)].into();
    let set: PageSet = seen.iter().copied().collect();
    assert!(set.contains(&page(1)));
}

#[test]
fn the_index_is_shared_between_the_indexer_and_the_searches() {
    // L'indexeur écrit et valide sur un fil, les recherches lisent sur d'autres.
    let b = std::sync::Arc::new(Bench::new());
    b.page(1, "Notes", &[("b0", "un mot")]);
    b.commit();
    let writer = {
        let b = b.clone();
        std::thread::spawn(move || {
            for i in 1..=50 {
                b.block(1, &format!("b{i}"), "Notes", "un mot de plus");
                b.commit();
            }
        })
    };
    let all = pages([1]);
    while !writer.is_finished() {
        assert_eq!(b.numbers("mot", &all), [1]);
    }
    writer.join().expect("le fil qui écrit");
    assert_eq!(b.traced("plus", &all, None).1.collected, 50);
}
