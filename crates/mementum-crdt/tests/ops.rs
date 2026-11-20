//! Les opérations d'édition des agents (`mementum_crdt::ops`), leurs alias de blocs et leurs
//! empreintes (`mementum_crdt::anchors`) : Rulings R1224, R1226, R1228 à R1232, R1235 ; Task S3 du
//! lot 5.
//!
//! Une opération se résout et se vérifie sur les blocs lus une fois (`ops::plan`), puis s'écrit par
//! différence (`PageDoc::apply`, qui passe par `write_diff_over`, Task S2) : ce que l'écriture
//! garde des frappes concurrentes est celui de l'écriture par différence, éprouvé contre le vrai
//! Yjs par `tests/diff.rs` ; `str_replace_writes_the_diff_write_proven_against_yjs` le relie.
//!
//! La table de cas `spec/agents/operations.json` donne, pour une page et une liste d'opérations,
//! le MMD écrit ou le refus attendu.

#[path = "common/live.rs"]
mod live;

use std::collections::{BTreeMap, HashSet};

use mementum_crdt::anchors::{
    AnchorError, Anchors, BlockHashes, HashKey, Why, changed, hashes, id_key, is_alias, three_way,
};
use mementum_crdt::ops::{
    self, Base, OpRefusal, Operation, Place, PlanContext, Reason, read_markdown,
};
use mementum_crdt::{DiffWritten, FRAGMENT, PageDoc};
use mementum_md::{Block, RenderOpts, parse, render};
use sha2::{Digest, Sha256};
use yrs::branch::Branch;
use yrs::{ReadTxn, Transact, Xml, XmlElementRef, XmlFragment, XmlOut};

// ------------------------------------------------------------------------------------ outils

/// Une page qui porte `mmd`, écrite par le serveur ; une ancre `^p1` donne l'identifiant `p1`.
fn page_with(mmd: &str) -> PageDoc {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(mmd).unwrap()).unwrap();
    page
}

/// Le MMD de la page, sans ancres.
fn mmd_of(page: &PageDoc) -> String {
    render(&page.read_blocks(), RenderOpts::default()).markdown
}

fn md(mmd: &str) -> Vec<Block> {
    read_markdown(0, mmd).unwrap_or_else(|e| panic!("{mmd:?} : {e:?}"))
}

fn current(key: &HashKey) -> PlanContext<'_> {
    PlanContext {
        base: Base::Current,
        key,
        forbid_external_media: false,
    }
}

/// Planifie `ops` sur la page, puis l'écrit ; la page relue de son journal est la page vivante.
fn run_in(
    page: &PageDoc,
    ops: &[Operation],
    ctx: &PlanContext<'_>,
) -> Result<DiffWritten, OpRefusal> {
    let before = page.encode_state();
    let blocks = page.read_blocks();
    let plan = ops::plan(&blocks, ops, ctx)?;
    let written = page.apply(&plan).expect("l'écriture d'un plan vérifié");
    // Le plan appliqué est le plan vérifié, identifiants compris (une page vide s'écrit en un
    // paragraphe vide).
    if !plan.blocks().is_empty() {
        assert_eq!(
            page.read_blocks(),
            mementum_md::normalize(plan.blocks().to_vec()),
            "la page écrite est le plan"
        );
    }
    let replica = PageDoc::from_updates([before.as_slice(), written.update.as_slice()]).unwrap();
    assert_eq!(replica.read_blocks(), page.read_blocks(), "la page relue");
    assert_eq!(replica.revision(), page.revision());
    if let Some(why) = live::reloaded_from_state_differs(page) {
        panic!("{why}");
    }
    Ok(written)
}

fn run(page: &PageDoc, ops: &[Operation]) -> Result<DiffWritten, OpRefusal> {
    let key = HashKey::new();
    run_in(page, ops, &current(&key))
}

/// Le refus de `ops` sur la page, qui ne doit rien écrire.
fn refused(page: &PageDoc, ops: &[Operation], ctx: &PlanContext<'_>) -> OpRefusal {
    let state = page.encode_state();
    let blocks = page.read_blocks();
    let refusal = match ops::plan(&blocks, ops, ctx) {
        Ok(plan) => panic!("plan accepté : {:?}", plan.blocks()),
        Err(refusal) => refusal,
    };
    assert_eq!(page.encode_state(), state, "rien n'est écrit");
    refusal
}

fn str_replace(old: &str, new: &str) -> Operation {
    Operation::StrReplace {
        old: old.into(),
        new: new.into(),
        anchor: None,
        replace_all: false,
    }
}

fn block_by_id<'a>(blocks: &'a [Block], id: &str) -> Option<&'a Block> {
    for b in blocks {
        if b.id == id {
            return Some(b);
        }
        if let Some(found) = block_by_id(&b.children, id) {
            return Some(found);
        }
    }
    None
}

fn all_ids(blocks: &[Block], out: &mut Vec<String>) {
    for b in blocks {
        out.push(b.id.clone());
        all_ids(&b.children, out);
    }
}

fn ids_of(page: &PageDoc) -> Vec<String> {
    let mut out = Vec::new();
    all_ids(&page.read_blocks(), &mut out);
    out
}

fn without_ids(mut blocks: Vec<Block>) -> Vec<Block> {
    fn clear(blocks: &mut [Block]) {
        for b in blocks {
            b.id.clear();
            clear(&mut b.children);
        }
    }
    clear(&mut blocks);
    mementum_md::normalize(blocks)
}

fn branch_id(branch: &Branch) -> String {
    format!("{:?}", branch.id())
}

/// L'identité Yjs des éléments et des textes de chaque bloc, par identifiant de bloc (la règle de
/// mesure de R299, comme `tests/diff.rs`) : l'élément du bloc, son nœud de contenu et ce qu'il
/// porte, sans les blocs enfants.
fn identities(page: &PageDoc) -> BTreeMap<String, Vec<String>> {
    fn content<T: ReadTxn>(txn: &T, node: &XmlElementRef, out: &mut Vec<String>) {
        out.push(branch_id(node.as_ref()));
        for child in node.children(txn) {
            match child {
                XmlOut::Element(e) => content(txn, &e, out),
                XmlOut::Text(t) => out.push(branch_id(t.as_ref())),
                XmlOut::Fragment(_) => {}
            }
        }
    }
    fn walk<T: ReadTxn>(txn: &T, list: &XmlElementRef, into: &mut BTreeMap<String, Vec<String>>) {
        for child in list.children(txn) {
            let XmlOut::Element(element) = child else {
                continue;
            };
            let id = match element.get_attribute(txn, "id") {
                Some(yrs::Out::Any(yrs::Any::String(s))) => s.to_string(),
                _ => String::new(),
            };
            let mut own = vec![branch_id(element.as_ref())];
            match element.tag().as_ref() {
                "columnList" | "column" => walk(txn, &element, into),
                _ => {
                    for part in element.children(txn) {
                        match part {
                            XmlOut::Element(e) if e.tag().as_ref() == "blockGroup" => {
                                walk(txn, &e, into)
                            }
                            XmlOut::Element(e) => content(txn, &e, &mut own),
                            XmlOut::Text(t) => own.push(branch_id(t.as_ref())),
                            XmlOut::Fragment(_) => {}
                        }
                    }
                }
            }
            into.insert(id, own);
        }
    }
    let doc = page.yrs_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    let mut out = BTreeMap::new();
    for child in fragment.children(&txn) {
        if let XmlOut::Element(group) = child {
            walk(&txn, &group, &mut out);
        }
    }
    out
}

/// Les blocs dont l'identité Yjs a changé entre deux relevés.
fn recreated(
    before: &BTreeMap<String, Vec<String>>,
    after: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let ids: HashSet<&String> = before.keys().chain(after.keys()).collect();
    let mut out: Vec<String> = ids
        .into_iter()
        .filter(|id| before.get(*id) != after.get(*id))
        .cloned()
        .collect();
    out.sort();
    out
}

/// Le hachage en base32 minuscule du SHA-256 de `id`, recalculé ici (R1224).
fn base32_digest(id: &str) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let digest = Sha256::digest(id.as_bytes());
    let mut bits = String::new();
    for byte in digest {
        bits.push_str(&format!("{byte:08b}"));
    }
    while !bits.len().is_multiple_of(5) {
        bits.push('0');
    }
    bits.as_bytes()
        .chunks(5)
        .map(|c| {
            let v = u8::from_str_radix(std::str::from_utf8(c).unwrap(), 2).unwrap();
            ALPHABET[v as usize] as char
        })
        .collect()
}

/// Deux identifiants dont les hachages partagent leurs `chars` premiers caractères base32.
fn colliding_ids(chars: usize) -> (String, String) {
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for n in 0.. {
        let id = format!("x{n}");
        let prefix = base32_digest(&id)[..chars].to_string();
        if let Some(other) = seen.insert(prefix, id.clone()) {
            return (other, id);
        }
    }
    unreachable!()
}

// --------------------------------------------------------------------------------- alias

/// Un alias ne dépend que de l'identifiant de son bloc : un bloc inséré au-dessus ne change pas
/// ceux des autres (un alias par rang, `b1`, `b2`, changerait), et un alias s'écrit `b` suivi du
/// début du SHA-256 de l'identifiant, en base32 minuscule, quatre caractères au moins.
#[test]
fn aliases_are_stable_when_blocks_are_inserted_above() {
    let page = page_with("Un ^p1\n\nDeux ^p2\n\nTrois ^p3\n");
    let before = Anchors::of(&page.read_blocks());
    for id in ["p1", "p2", "p3"] {
        let alias = before.alias(id).unwrap();
        assert!(is_alias(&alias), "{alias}");
        assert_eq!(&alias[..1], "b");
        assert_eq!(alias.len(), 5, "seul dès quatre caractères : {alias}");
        assert!(base32_digest(id).starts_with(&alias[1..]), "{id} : {alias}");
    }
    run(
        &page,
        &[Operation::Insert {
            at: Place::Start,
            blocks: md("Zéro\n\nZéro bis\n"),
        }],
    )
    .unwrap();
    assert_eq!(mmd_of(&page), "Zéro\n\nZéro bis\n\nUn\n\nDeux\n\nTrois\n");
    let after = Anchors::of(&page.read_blocks());
    for id in ["p1", "p2", "p3"] {
        assert_eq!(after.alias(id), before.alias(id), "{id}");
        assert_eq!(after.resolve(&before.alias(id).unwrap()), Ok(id));
    }
}

/// Un alias s'allonge autant qu'il faut pour être seul dans la page, et pas plus ; l'identifiant
/// entier d'un bloc est toujours accepté ; un alias n'est jamais l'identifiant d'un autre bloc.
#[test]
fn an_alias_grows_until_it_is_alone() {
    let (a, b) = colliding_ids(4);
    let shared = base32_digest(&a)
        .chars()
        .zip(base32_digest(&b).chars())
        .take_while(|(x, y)| x == y)
        .count();
    assert!(shared >= 4);
    let page = page_with(&format!("A ^{a}\n\nB ^{b}\n\nC ^p3\n"));
    let anchors = Anchors::of(&page.read_blocks());
    for id in [&a, &b] {
        let alias = anchors.alias(id).unwrap();
        assert_eq!(alias.len(), 1 + shared + 1, "{id} : {alias}");
        assert_eq!(anchors.resolve(&alias), Ok(id.as_str()));
        // Un caractère de moins désigne les deux blocs.
        assert_eq!(
            anchors.resolve(&alias[..alias.len() - 1]),
            Err(AnchorError::Ambiguous)
        );
        assert_eq!(anchors.resolve(id), Ok(id.as_str()), "l'identifiant entier");
    }
    assert_eq!(anchors.alias("p3").unwrap().len(), 5);

    // Un bloc dont l'identifiant est l'alias court d'un autre : l'alias de l'autre s'allonge, et
    // cet identifiant, qui commence aussi le hachage de l'autre, les désigne tous deux (un bloc
    // posé sous l'identifiant d'un alias déjà servi ne prend pas la place du bloc de l'alias).
    let p1_short = format!("b{}", &base32_digest("p1")[..4]);
    let page = page_with(&format!("Un ^p1\n\nPiège ^{p1_short}\n"));
    let anchors = Anchors::of(&page.read_blocks());
    let alias = anchors.alias("p1").unwrap();
    assert_ne!(alias, p1_short);
    assert_eq!(alias.len(), 6);
    assert_eq!(anchors.resolve(&alias), Ok("p1"));
    assert_eq!(anchors.resolve(&p1_short), Err(AnchorError::Ambiguous));
    let trap = anchors.alias(&p1_short).unwrap();
    assert_eq!(
        anchors.resolve(&trap),
        Ok(p1_short.as_str()),
        "son alias le désigne"
    );
}

/// Un alias sans bloc (le bloc retiré depuis la lecture) et un alias de plusieurs blocs (un bloc
/// arrivé depuis, qui partage son début) se disent autrement : `stale_anchor`, `ambiguous_anchor`.
#[test]
fn a_stale_alias_and_an_ambiguous_alias_are_told_apart() {
    let (a, b) = colliding_ids(4);
    let page = page_with(&format!("A ^{a}\n\nGarde ^p2\n\nRetiré ^p3\n"));
    let read = Anchors::of(&page.read_blocks());
    let alias_a = read.alias(&a).unwrap().to_string();
    let alias_p3 = read.alias("p3").unwrap().to_string();
    assert_eq!(alias_a.len(), 5);

    // Le bloc p3 retiré, et un bloc arrivé dont le hachage commence comme celui de `a`.
    page.write_diff(&parse(&format!("A ^{a}\n\nGarde ^p2\n\nArrivé ^{b}\n")).unwrap())
        .unwrap();
    let now = Anchors::of(&page.read_blocks());
    assert_eq!(now.resolve(&alias_p3), Err(AnchorError::Stale));
    assert_eq!(now.resolve(&alias_a), Err(AnchorError::Ambiguous));
    assert_eq!(now.resolve("bzzzz"), Err(AnchorError::Stale));
    assert_eq!(now.resolve("inconnu"), Err(AnchorError::Stale));

    let key = HashKey::new();
    let delete = |anchor: &str| Operation::DeleteBlock {
        anchor: anchor.into(),
        allow_deleting_content: false,
    };
    let stale = refused(&page, &[delete(&alias_p3)], &current(&key));
    assert_eq!((stale.index, stale.reason), (0, Reason::StaleAnchor));
    let ambiguous = refused(&page, &[delete(&alias_a)], &current(&key));
    assert_eq!(
        (ambiguous.index, ambiguous.reason),
        (0, Reason::AmbiguousAnchor)
    );
    // L'alias allongé, ou l'identifiant entier, désigne le bloc.
    run(&page, &[delete(&now.alias(&a).unwrap())]).unwrap();
    assert_eq!(mmd_of(&page), "Garde\n\nArrivé\n");
}

// --------------------------------------------------------------------------------- str_replace

const MARKED: &str = "Du **gras**, un [lien](https://a.example), <mention type=\"page\" id=\"pg_a\">A</mention> puis la fin. ^p1\n\nSecond. ^p2\n";

/// `str_replace` cherche dans le texte brut du bloc (une mention y vaut un caractère), jamais dans
/// le MMD rendu (RESULTS, leçon 5 : compter sur le texte brut) ; le texte neuf prend les marques
/// du premier caractère remplacé ; un remplacement ne touche que son bloc, et dans son bloc que le
/// texte qu'il change.
#[test]
fn str_replace_counts_on_plain_text_after_marks_and_a_mention() {
    let page = page_with(MARKED);
    let before = identities(&page);
    let written = run(&page, &[str_replace("puis la fin", "et la suite")]).unwrap();
    assert_eq!(
        mmd_of(&page),
        "Du **gras**, un [lien](https://a.example), <mention type=\"page\" id=\"pg_a\">A</mention> et la suite.\n\nSecond.\n"
    );
    assert_eq!(written.touched, ["p1"]);
    assert_eq!(recreated(&before, &identities(&page)), Vec::<String>::new());

    // Dans le gras et dans le lien : les marques du premier caractère remplacé.
    let page = page_with(MARKED);
    run(
        &page,
        &[str_replace("ras", "ros"), str_replace("lien", "lieu")],
    )
    .unwrap();
    assert!(mmd_of(&page).starts_with("Du **gros**, un [lieu](https://a.example), "));

    // À cheval sur le gras et le texte : les marques du premier caractère (gras).
    let page = page_with(MARKED);
    run(&page, &[str_replace("as, un", "os et un")]).unwrap();
    assert!(
        mmd_of(&page).starts_with("Du **gros et un** [lien]"),
        "{}",
        mmd_of(&page)
    );

    // Le texte brut, pas le MMD : « gras, un lien » s'y trouve, « **gras** » non.
    let page = page_with(MARKED);
    run(&page, &[str_replace("gras, un lien", "mot")]).unwrap();
    assert!(
        mmd_of(&page).starts_with("Du **mot**, <mention"),
        "{}",
        mmd_of(&page)
    );
    let key = HashKey::new();
    let refusal = refused(
        &page_with(MARKED),
        &[str_replace("**gras**", "x")],
        &current(&key),
    );
    assert_eq!(refusal.reason, Reason::NoMatch);

    // Une mention couverte par le texte remplacé part avec lui.
    let page = page_with(MARKED);
    run(&page, &[str_replace(", \u{FFFC} puis", " puis")]).unwrap();
    assert!(
        mmd_of(&page).starts_with("Du **gras**, un [lien](https://a.example) puis la fin."),
        "{}",
        mmd_of(&page)
    );

    // Un saut de ligne dans `new` devient un saut de ligne du bloc ; dans un bloc de code, un
    // caractère du texte.
    let page = page_with("Une ligne. ^p1\n\n```\nlet a = 1;\n```\n^c1\n");
    run(
        &page,
        &[
            str_replace("Une ligne.", "Une ligne.\nDeux."),
            str_replace("let a = 1;", "let a = 1;\nlet b = 2;"),
        ],
    )
    .unwrap();
    assert_eq!(
        mmd_of(&page),
        "Une ligne.<br/>Deux.\n\n```\nlet a = 1;\nlet b = 2;\n```\n"
    );
}

/// `old` ne se cherche jamais à cheval sur deux blocs ; sans ancre, il doit se trouver une seule
/// fois dans la page ; avec une ancre, une fois dans le bloc ; `replace_all` les remplace tous.
#[test]
fn str_replace_never_spans_two_blocks() {
    let mmd = "Fin du premier ^p1\n\nDébut du second, fin du second, fin du reste ^p2\n";
    let key = HashKey::new();
    for old in [
        "premier\n\nDébut",
        "premierDébut",
        "premier Début",
        "premier\nDébut",
    ] {
        let refusal = refused(&page_with(mmd), &[str_replace(old, "x")], &current(&key));
        assert_eq!(refusal.reason, Reason::NoMatch, "{old:?}");
    }
    let refusal = refused(&page_with(mmd), &[str_replace("FIN", "x")], &current(&key));
    assert_eq!(refusal.reason, Reason::NoMatch, "la casse compte");
    let refusal = refused(
        &page_with(mmd),
        &[str_replace("fin du", "x")],
        &current(&key),
    );
    assert_eq!(refusal.reason, Reason::AmbiguousMatch);
    assert_eq!(refusal.candidates.len(), 1, "un bloc, deux fois");
    let refusal = refused(&page_with(mmd), &[str_replace("du", "x")], &current(&key));
    assert_eq!(refusal.reason, Reason::AmbiguousMatch);
    assert_eq!(refusal.candidates.len(), 2);

    let page = page_with(mmd);
    let anchored = Operation::StrReplace {
        old: "fin du".into(),
        new: "FIN DU".into(),
        anchor: Some("p1".into()),
        replace_all: false,
    };
    let refusal = refused(&page, std::slice::from_ref(&anchored), &current(&key));
    assert_eq!(refusal.reason, Reason::NoMatch, "pas dans p1");
    let all = Operation::StrReplace {
        old: "du".into(),
        new: "DU".into(),
        anchor: None,
        replace_all: true,
    };
    run(&page, &[all]).unwrap();
    assert_eq!(
        mmd_of(&page),
        "Fin DU premier\n\nDébut DU second, fin DU second, fin DU reste\n"
    );
}

/// Ce qu'un appel écrit par `str_replace` est borné (R380) : `replace_all` d'un caractère courant
/// par un long texte écrirait le produit des deux ; au-delà de 2 Mio, refusé
/// (`too_long_to_write`), avant de bâtir le texte.
#[test]
fn a_replace_all_cannot_blow_the_page_up() {
    let page = page_with(&format!("{} ^p1\n", "a".repeat(10_000)));
    let key = HashKey::new();
    let blow_up = Operation::StrReplace {
        old: "a".into(),
        new: "b".repeat(1_000),
        anchor: None,
        replace_all: true,
    };
    let refusal = refused(&page, std::slice::from_ref(&blow_up), &current(&key));
    assert_eq!((refusal.index, refusal.reason), (0, Reason::TooLong));
    assert_eq!(refusal.reason.code(), "too_long_to_write");
    // Deux opérations qui, ensemble, dépassent la borne.
    let half = Operation::StrReplace {
        old: "a".into(),
        new: "b".repeat(150),
        anchor: None,
        replace_all: true,
    };
    let page = page_with(&format!("{} ^p1\n\nb ^p2\n", "a".repeat(8_000)));
    let refusal = refused(&page, &[half.clone(), half.clone()], &current(&key));
    assert_eq!(
        (refusal.index, refusal.reason),
        (1, Reason::NoMatch),
        "plus de « a »"
    );
    let page = page_with(&format!(
        "{} ^p1\n\n{} ^p2\n",
        "a".repeat(8_000),
        "x".repeat(8_000)
    ));
    let twice = [
        half,
        Operation::StrReplace {
            old: "x".into(),
            new: "y".repeat(150),
            anchor: None,
            replace_all: true,
        },
    ];
    let refusal = refused(&page, &twice, &current(&key));
    assert_eq!((refusal.index, refusal.reason), (1, Reason::TooLong));
    // Sous la borne, il passe.
    let page = page_with(&format!("{} ^p1\n", "a".repeat(1_000)));
    run(
        &page,
        &[Operation::StrReplace {
            old: "a".into(),
            new: "bc".into(),
            anchor: None,
            replace_all: true,
        }],
    )
    .unwrap();
    assert_eq!(mmd_of(&page), format!("{}\n", "bc".repeat(1_000)));
}

/// `no_match` propose au plus trois blocs, ceux qui portent le plus long début de `old`, parmi les
/// blocs de la page lue encore là : ni un bloc qu'une opération d'avant a retiré, ni un bloc
/// qu'elle a inséré (R1230).
#[test]
fn no_match_offers_candidates_of_this_page_only() {
    let mmd = "Le rapport annuel est prêt. ^p1\n\nAutre chose. ^p2\n\nLe rapport mensuel. ^p3\n\nUn rapport. ^p4\n\nLe rap. ^p5\n";
    let key = HashKey::new();
    let page = page_with(mmd);
    let anchors = Anchors::of(&page.read_blocks());
    let refusal = refused(
        &page,
        &[str_replace("rapport trimestriel", "x")],
        &current(&key),
    );
    assert_eq!((refusal.index, refusal.reason), (0, Reason::NoMatch));
    let aliases: Vec<&str> = refusal
        .candidates
        .iter()
        .map(|c| c.alias.as_str())
        .collect();
    assert_eq!(
        aliases,
        [
            anchors.alias("p1").unwrap(),
            anchors.alias("p3").unwrap(),
            anchors.alias("p4").unwrap()
        ],
        "les trois plus longs débuts (« rapport »), dans l'ordre de la page"
    );
    assert_eq!(refusal.candidates[0].text, "Le rapport annuel est prêt.");

    let delete_p1 = Operation::DeleteBlock {
        anchor: "p1".into(),
        allow_deleting_content: false,
    };
    let insert = Operation::Insert {
        at: Place::End,
        blocks: md("Le rapport trimestre.\n"),
    };
    let refusal = refused(
        &page,
        &[delete_p1, insert, str_replace("rapport trimestriel", "x")],
        &current(&key),
    );
    assert_eq!((refusal.index, refusal.reason), (2, Reason::NoMatch));
    let aliases: Vec<&str> = refusal
        .candidates
        .iter()
        .map(|c| c.alias.as_str())
        .collect();
    assert_eq!(
        aliases,
        [
            anchors.alias("p3").unwrap(),
            anchors.alias("p4").unwrap(),
            anchors.alias("p5").unwrap()
        ]
    );

    // Un texte long : le candidat est coupé à 120 caractères autour du début trouvé.
    let long = format!("{} rapport final.", "mot ".repeat(80));
    let page = page_with(&format!("{long} ^p1\n"));
    let refusal = refused(
        &page,
        &[str_replace("rapport trimestriel", "x")],
        &current(&key),
    );
    assert_eq!(refusal.candidates.len(), 1);
    assert!(refusal.candidates[0].text.chars().count() <= 120);
    assert!(refusal.candidates[0].text.contains("rapport final."));
}

// ------------------------------------------------------------------- atomicité, identifiants

/// Une opération qui échoue arrête tout avant d'écrire : le plan n'existe pas, la page ne change
/// pas, et le refus nomme l'opération par son rang.
#[test]
fn a_failing_operation_leaves_the_plan_empty() {
    let page = page_with("Un ^p1\n\nDeux ^p2\n");
    let key = HashKey::new();
    let ops = [
        str_replace("Un", "Premier"),
        Operation::Insert {
            at: Place::After("p1".into()),
            blocks: md("Inséré\n"),
        },
        str_replace("absent", "x"),
    ];
    let refusal = refused(&page, &ops, &current(&key));
    assert_eq!((refusal.index, refusal.reason), (2, Reason::NoMatch));
    assert_eq!(mmd_of(&page), "Un\n\nDeux\n");
    // Les deux premières seules passent, et s'écrivent ensemble.
    run(&page, &ops[..2]).unwrap();
    assert_eq!(mmd_of(&page), "Premier\n\nInséré\n\nDeux\n");
}

/// Les blocs qu'une opération insère reçoivent des identifiants neufs, quoi que porte leur MMD
/// (R1232 ; R639 : un identifiant recopié serait défait par le dédoublonnage) ; les blocs de la
/// page ne sont pas touchés.
#[test]
fn inserted_blocks_get_fresh_ids() {
    let page = page_with("Un ^p1\n\nDeux ^p2\n");
    let before = identities(&page);
    let written = run(
        &page,
        &[Operation::Insert {
            at: Place::After("p1".into()),
            blocks: md("Copie ^p1\n\n- Puce ^p2\n  - Enfant ^p9\n"),
        }],
    )
    .unwrap();
    assert_eq!(mmd_of(&page), "Un\n\nCopie\n\n- Puce\n  - Enfant\n\nDeux\n");
    let ids = ids_of(&page);
    assert_eq!(ids[0], "p1");
    assert_eq!(ids[4], "p2");
    for id in &ids[1..4] {
        assert!(!["p1", "p2", "p9", ""].contains(&id.as_str()), "{id}");
        assert!(uuid::Uuid::parse_str(id).is_ok(), "{id}");
    }
    assert_eq!(written.touched, ids[1..4]);
    assert_eq!(
        recreated(&before, &identities(&page)),
        sorted(ids[1..4].to_vec())
    );
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids
}

/// `replace_block` donne l'identifiant du bloc remplacé au premier bloc neuf de son type, parmi
/// ceux de premier niveau (les fils de commentaires gardent leur bloc), des identifiants neufs aux
/// autres ; sans bloc de son type, il part.
#[test]
fn a_replaced_block_keeps_its_id_for_the_first_block_of_its_type() {
    let page = page_with("# Titre ^h1\n\nTexte ^p1\n  Enfant ^c1\n\nFin ^p2\n");
    let replace = |anchor: &str, mmd: &str| Operation::ReplaceBlock {
        anchor: anchor.into(),
        blocks: md(mmd),
        allow_deleting_content: false,
    };
    run(
        &page,
        &[replace(
            "p1",
            "# Nouveau titre\n\nNouveau texte\n\nEncore ^c1\n",
        )],
    )
    .unwrap();
    assert_eq!(
        mmd_of(&page),
        "# Titre\n\n# Nouveau titre\n\nNouveau texte\n\nEncore\n\nFin\n"
    );
    let blocks = page.read_blocks();
    assert_eq!(blocks[2].id, "p1", "le premier paragraphe neuf");
    assert!(
        block_by_id(&blocks, "c1").is_none(),
        "l'enfant part, sans donner son id"
    );
    assert!(!["h1", "p1", "c1"].contains(&blocks[1].id.as_str()));
    assert!(!["h1", "p1", "c1"].contains(&blocks[3].id.as_str()));

    run(&page, &[replace("h1", "Un paragraphe\n\n## Sous-titre\n")]).unwrap();
    let blocks = page.read_blocks();
    assert_eq!(blocks[1].id, "h1", "le premier titre neuf");
    run(&page, &[replace("h1", "Plus de titre\n")]).unwrap();
    assert!(!ids_of(&page).contains(&"h1".to_string()));
}

/// Un bloc `unknown` (ou un lien de sous-page, une base, un bloc synchronisé) que l'opération
/// retirerait ne part que si elle porte `allow_deleting_content` (R1232).
#[test]
fn an_unknown_block_is_deleted_only_when_allowed() {
    let mmd = "Avant ^p1\n\n<block type=\"future_widget\" id=\"u1\" data=\"eyJ4IjoxfQ==\"/>\n\nAprès ^p2\n";
    let key = HashKey::new();
    let delete = |allow: bool| Operation::DeleteBlock {
        anchor: "u1".into(),
        allow_deleting_content: allow,
    };
    let refusal = refused(&page_with(mmd), &[delete(false)], &current(&key));
    assert_eq!(
        (refusal.index, refusal.reason),
        (0, Reason::ProtectedContent)
    );
    assert_eq!(refusal.blocks, ["u1"]);
    let page = page_with(mmd);
    run(&page, &[delete(true)]).unwrap();
    assert_eq!(mmd_of(&page), "Avant\n\nAprès\n");
}

/// De même un lien de sous-page, retiré par `delete_block`, `replace_block` (lui ou un parent) ou
/// `replace_page` ; `replace_page` qui le garde passe sans l'option.
#[test]
fn a_removed_link_block_needs_the_option() {
    let mmd = "Avant ^p1\n\n<page id=\"pg_child01\" anchor=\"l1\">Sous-page</page>\n\n- Liste ^p2\n  <page id=\"pg_child02\" anchor=\"l2\">Autre</page>\n";
    let key = HashKey::new();
    let cases: Vec<(Vec<Operation>, &str)> = vec![
        (
            vec![Operation::DeleteBlock {
                anchor: "l1".into(),
                allow_deleting_content: false,
            }],
            "l1",
        ),
        (
            vec![Operation::ReplaceBlock {
                anchor: "p2".into(),
                blocks: md("- Liste sans lien\n"),
                allow_deleting_content: false,
            }],
            "l2",
        ),
        (
            vec![Operation::ReplacePage {
                blocks: md(
                    "Avant ^p1\n\n- Liste ^p2\n  <page id=\"pg_child02\" anchor=\"l2\">Autre</page>\n",
                ),
                allow_deleting_content: false,
            }],
            "l1",
        ),
    ];
    for (ops, gone) in cases {
        let refusal = refused(&page_with(mmd), &ops, &current(&key));
        assert_eq!(refusal.reason, Reason::ProtectedContent, "{ops:?}");
        assert_eq!(refusal.blocks, [gone]);
        let mut allowed = ops.clone();
        match &mut allowed[0] {
            Operation::DeleteBlock {
                allow_deleting_content,
                ..
            }
            | Operation::ReplaceBlock {
                allow_deleting_content,
                ..
            }
            | Operation::ReplacePage {
                allow_deleting_content,
                ..
            } => *allow_deleting_content = true,
            _ => unreachable!(),
        }
        let page = page_with(mmd);
        run(&page, &allowed).unwrap();
        assert!(!ids_of(&page).contains(&gone.to_string()));
    }
    // Récrite telle qu'elle se lit, sans ancres : rien ne part.
    let page = page_with(mmd);
    let written = run(
        &page,
        &[Operation::ReplacePage {
            blocks: md(&mmd_of(&page)),
            allow_deleting_content: false,
        }],
    )
    .unwrap();
    assert_eq!(written.update, [0, 0]);
}

// --------------------------------------------------------------------------------- médias

/// Une écriture par jeton ne fait entrer aucune image, vidéo ou son à adresse web que la page ne
/// portait pas, par aucune opération (R1235) ; une adresse que la page porte déjà, celle d'un
/// fichier de l'instance, un lien, un signet ou un bloc `file` passent ; sans le drapeau, tout
/// passe.
#[test]
fn a_token_write_cannot_add_external_media() {
    let mmd = "Un ^p1\n\n![Logo](https://mementum.dev/logo.png)\n^i1\n";
    let key = HashKey::new();
    let guarded = PlanContext {
        base: Base::Current,
        key: &key,
        forbid_external_media: true,
    };
    let insert = |mmd: &str| Operation::Insert {
        at: Place::End,
        blocks: md(mmd),
    };
    let file = "/api/v1/pages/0193a1b2-0000-7000-8000-000000000003/files/0193a1b2-0000-7000-8000-000000000004";
    let outside = [
        insert("![Pixel](https://ailleurs.example/p.png?q=secret)\n"),
        insert("<video src=\"https://ailleurs.example/v.mp4\"/>\n"),
        insert("<audio src=\"https://ailleurs.example/a.mp3\"/>\n"),
        insert("- Liste\n  ![Pixel](https://ailleurs.example/p.png)\n"),
        insert("![Pixel](//ailleurs.example/p.png)\n"),
        Operation::ReplaceBlock {
            anchor: "p1".into(),
            blocks: md("![Pixel](https://ailleurs.example/p.png)\n"),
            allow_deleting_content: false,
        },
        Operation::ReplacePage {
            blocks: md("Un ^p1\n\n![Logo](https://ailleurs.example/logo.png)\n^i1\n"),
            allow_deleting_content: false,
        },
    ];
    for op in &outside {
        let refusal = refused(&page_with(mmd), std::slice::from_ref(op), &guarded);
        assert_eq!(
            (refusal.index, refusal.reason),
            (0, Reason::ExternalMedia),
            "{op:?}"
        );
        let page = page_with(mmd);
        run(&page, std::slice::from_ref(op)).unwrap();
    }
    let inside = [
        insert("![Logo](https://mementum.dev/logo.png)\n"),
        insert(&format!("![Fichier]({file})\n")),
        insert("Un [lien](https://ailleurs.example/?q=secret).\n"),
        insert("<bookmark url=\"https://ailleurs.example\"/>\n"),
        insert("<file src=\"https://ailleurs.example/spec.pdf\" name=\"spec.pdf\"/>\n"),
    ];
    for op in &inside {
        let page = page_with(mmd);
        run_in(&page, std::slice::from_ref(op), &guarded).unwrap();
    }
}

// ------------------------------------------------------------------------------- empreintes

/// Les empreintes de contenu et de place dépendent de la clé du processus ; celle de
/// l'identifiant, non : c'est le début du SHA-256 de l'identifiant, celui de l'alias.
#[test]
fn hashes_depend_on_the_process_key() {
    let page = page_with("Un ^p1\n\nDeux ^p2\n  Enfant ^c1\n");
    let blocks = page.read_blocks();
    let (one, two) = (HashKey::new(), HashKey::new());
    let (a, b) = (hashes(&blocks, &one), hashes(&blocks, &two));
    assert_eq!(a.len(), 3);
    assert_eq!(a, hashes(&blocks, &one), "une clé, les mêmes empreintes");
    for (x, y) in a.as_slice().iter().zip(b.as_slice()) {
        assert_eq!(x.id, y.id);
        assert_ne!(x.content, y.content);
        assert_ne!(x.place, y.place);
    }
    let ids: Vec<u64> = a.as_slice().iter().map(|h| h.id).collect();
    assert_eq!(ids, [id_key("p1"), id_key("p2"), id_key("c1")]);
    let digest = Sha256::digest(b"p2");
    assert_eq!(
        id_key("p2"),
        u64::from_be_bytes(digest[..8].try_into().unwrap())
    );
    assert!(a.bytes() >= 3 * 24);
}

/// Un bloc déplacé change d'empreinte de place, pas de contenu ; son nouveau voisin de dessous
/// aussi (son frère d'avant change) ; les autres, non. Un texte changé ne change que le contenu de
/// son bloc.
#[test]
fn a_moved_block_changes_its_place_hash() {
    let page = page_with("Un ^p1\n\nDeux ^p2\n\nTrois ^p3\n\nQuatre ^p4\n");
    let key = HashKey::new();
    let before = hashes(&page.read_blocks(), &key);
    run(
        &page,
        &[Operation::MoveBlock {
            anchor: "p4".into(),
            to: Place::After("p1".into()),
        }],
    )
    .unwrap();
    assert_eq!(mmd_of(&page), "Un\n\nQuatre\n\nDeux\n\nTrois\n");
    let after = hashes(&page.read_blocks(), &key);
    let by_id =
        |h: &BlockHashes, id: &str| *h.as_slice().iter().find(|x| x.id == id_key(id)).unwrap();
    assert_eq!(by_id(&before, "p4").content, by_id(&after, "p4").content);
    assert_ne!(by_id(&before, "p4").place, by_id(&after, "p4").place);
    assert_ne!(by_id(&before, "p2").place, by_id(&after, "p2").place);
    assert_eq!(by_id(&before, "p1"), by_id(&after, "p1"));
    assert_eq!(by_id(&before, "p3"), by_id(&after, "p3"));

    run(&page, &[str_replace("Trois", "3")]).unwrap();
    let edited = hashes(&page.read_blocks(), &key);
    assert_ne!(by_id(&after, "p3").content, by_id(&edited, "p3").content);
    assert_eq!(by_id(&after, "p3").place, by_id(&edited, "p3").place);

    // Un enfant qui change de parent change de place.
    let page = page_with("Un ^p1\n  Enfant ^c1\n\nDeux ^p2\n");
    let before = hashes(&page.read_blocks(), &key);
    run(
        &page,
        &[Operation::MoveBlock {
            anchor: "c1".into(),
            to: Place::After("p2".into()),
        }],
    )
    .unwrap();
    let after = hashes(&page.read_blocks(), &key);
    assert_ne!(by_id(&before, "c1").place, by_id(&after, "c1").place);
    assert_eq!(by_id(&before, "c1").content, by_id(&after, "c1").content);
}

const FIVE: &str = "Un ^p1\n\nDeux ^p2\n  Enfant ^c2\n\nTrois ^p3\n\nQuatre ^p4\n\nCinq ^p5\n";

/// La page de `FIVE`, ses empreintes à la lecture de l'agent, et une écriture faite depuis par un
/// autre (`human`).
fn rebased(human: &str) -> (PageDoc, BlockHashes, HashKey) {
    let page = page_with(FIVE);
    let key = HashKey::new();
    let base = hashes(&page.read_blocks(), &key);
    page.write_diff(&parse(human).unwrap()).unwrap();
    (page, base, key)
}

/// `rebase` : la page a changé depuis la lecture de l'agent, mais pas les blocs que l'appel
/// touche : il s'applique (R1231). Pour `insert`, le bloc d'ancrage ne compte que par sa place.
#[test]
fn operations_touching_unchanged_blocks_pass_a_rebase() {
    let human = "Un ^p1\n\nDeux ^p2\n  Enfant ^c2\n\nTrois et plus ^p3\n\nQuatre ^p4\n\nCinq ^p5\n";
    let (page, base, key) = rebased(human);
    let ctx = PlanContext {
        base: Base::Changed(&base),
        key: &key,
        forbid_external_media: false,
    };
    let anchors = Anchors::of(&page.read_blocks());
    let ops = [
        str_replace("Un", "Premier"),
        Operation::Insert {
            at: Place::After(anchors.alias("p4").unwrap()),
            blocks: md("Après quatre\n"),
        },
        Operation::DeleteBlock {
            anchor: "p5".into(),
            allow_deleting_content: false,
        },
        Operation::ReplaceBlock {
            anchor: "p2".into(),
            blocks: md("Deux remplacé\n"),
            allow_deleting_content: false,
        },
        Operation::MoveBlock {
            anchor: "p1".into(),
            to: Place::End,
        },
    ];
    run_in(&page, &ops, &ctx).unwrap();
    assert_eq!(
        mmd_of(&page),
        "Deux remplacé\n\nTrois et plus\n\nQuatre\n\nAprès quatre\n\nPremier\n"
    );
}

/// … et refuse dès qu'un bloc qu'il touche a changé : son contenu, sa place, un de ses sous-blocs
/// (pour un retrait ou un remplacement), ou la place du bloc d'ancrage d'une insertion ; le refus
/// nomme les blocs en cause.
#[test]
fn a_changed_block_refuses_it() {
    let human = "Un ^p1\n\nDeux ^p2\n  Enfant changé ^c2\n\nTrois et plus ^p3\n\nNeuf\n\nQuatre ^p4\n\nCinq ^p5\n";
    let (page, base, key) = rebased(human);
    let ctx = PlanContext {
        base: Base::Changed(&base),
        key: &key,
        forbid_external_media: false,
    };
    let cases: Vec<(Operation, Vec<&str>)> = vec![
        (str_replace("Trois", "3"), vec!["p3"]),
        (
            Operation::Insert {
                at: Place::After("p4".into()),
                blocks: md("x\n"),
            },
            vec!["p4"],
        ),
        (
            Operation::ReplaceBlock {
                anchor: "p2".into(),
                blocks: md("x\n"),
                allow_deleting_content: false,
            },
            vec!["c2"],
        ),
        (
            Operation::DeleteBlock {
                anchor: "p2".into(),
                allow_deleting_content: false,
            },
            vec!["c2"],
        ),
        (
            Operation::MoveBlock {
                anchor: "p4".into(),
                to: Place::Start,
            },
            vec!["p4"],
        ),
    ];
    for (op, blocks) in cases {
        let refusal = refused(&page, &[str_replace("Un", "1"), op.clone()], &ctx);
        assert_eq!(
            (refusal.index, refusal.reason),
            (1, Reason::RevisionConflict),
            "{op:?}"
        );
        assert_eq!(refusal.blocks, blocks, "{op:?}");
    }
    // Sans `rebase` (la page à la révision lue), les mêmes opérations passent.
    run(&page, &[str_replace("Trois", "3")]).unwrap();
}

/// `replace_page` en `rebase` : une fusion à trois par bloc ; seuls les blocs que l'agent a changés
/// depuis sa lecture s'écrivent, ceux qu'un autre a changés gardent leur contenu d'aujourd'hui.
#[test]
fn replace_page_writes_only_what_the_agent_changed_and_keeps_what_others_changed() {
    let human = "Un ^p1\n\nDeux de l'humain ^p2\n  Enfant ^c2\n\nTrois ^p3\n\nQuatre ^p4\n\nCinq ^p5\n\nAjout de l'humain\n";
    let (page, base, key) = rebased(human);
    let ctx = PlanContext {
        base: Base::Changed(&base),
        key: &key,
        forbid_external_media: false,
    };
    // L'agent récrit la page qu'il a lue (alias de sa lecture) : p3 changé, p4 retiré (p5 change
    // donc de place), un bloc neuf après p1 (p2 change donc de place, et l'humain a changé son
    // texte : deux dimensions différentes, qui se fondent).
    let read = Anchors::of(&page_with(FIVE).read_blocks());
    let alias = |id: &str| read.alias(id).unwrap().to_string();
    let written = format!(
        "Un ^{}\n\nNeuf de l'agent\n\nDeux ^{}\n  Enfant ^{}\n\nTrois de l'agent ^{}\n\nCinq ^{}\n",
        alias("p1"),
        alias("p2"),
        alias("c2"),
        alias("p3"),
        alias("p5")
    );
    let before = identities(&page);
    run_in(
        &page,
        &[Operation::ReplacePage {
            blocks: md(&written),
            allow_deleting_content: false,
        }],
        &ctx,
    )
    .unwrap();
    assert_eq!(
        mmd_of(&page),
        "Un\n\nNeuf de l'agent\n\nDeux de l'humain\n  Enfant\n\nTrois de l'agent\n\nCinq\n\nAjout de l'humain\n"
    );
    let after = identities(&page);
    for id in ["p1", "p2", "c2", "p5"] {
        assert_eq!(before.get(id), after.get(id), "{id} n'est pas touché");
    }
    assert!(!after.contains_key("p4"));
}

/// … et refuse quand l'agent et un autre ont changé le même bloc (son contenu), ou que l'un a
/// retiré ce que l'autre a changé, ou qu'ils ont inséré au même endroit.
#[test]
fn replace_page_conflicts_when_both_changed_one_block() {
    let read = Anchors::of(&page_with(FIVE).read_blocks());
    let alias = |id: &str| read.alias(id).unwrap().to_string();
    let base_mmd = |p2: &str, extra: &str| {
        format!(
            "Un ^{}\n\n{p2} ^{}\n  Enfant ^{}\n\n{extra}Trois ^{}\n\nQuatre ^{}\n\nCinq ^{}\n",
            alias("p1"),
            alias("p2"),
            alias("c2"),
            alias("p3"),
            alias("p4"),
            alias("p5")
        )
    };
    let cases = [
        (
            "Un ^p1\n\nDeux de l'humain ^p2\n  Enfant ^c2\n\nTrois ^p3\n\nQuatre ^p4\n\nCinq ^p5\n",
            base_mmd("Deux de l'agent", ""),
            vec!["p2"],
        ),
        (
            "Un ^p1\n\nDeux ^p2\n  Enfant changé ^c2\n\nTrois ^p3\n\nQuatre ^p4\n\nCinq ^p5\n",
            format!(
                "Un ^{}\n\nTrois ^{}\n\nQuatre ^{}\n\nCinq ^{}\n",
                alias("p1"),
                alias("p3"),
                alias("p4"),
                alias("p5")
            ),
            vec!["c2"],
        ),
        (
            "Un ^p1\n\nDeux ^p2\n  Enfant ^c2\n\nDe l'humain\n\nTrois ^p3\n\nQuatre ^p4\n\nCinq ^p5\n",
            base_mmd("Deux", "De l'agent\n\n"),
            vec!["p3"],
        ),
        // Un bloc arrivé après un bloc que l'agent retire : la fusion ne se bâtit pas.
        (
            "Un ^p1\n\nDeux ^p2\n  Enfant ^c2\n\nTrois ^p3\n\nQuatre ^p4\n\nCinq ^p5\n\nAprès cinq\n",
            format!(
                "Un ^{}\n\nDeux ^{}\n  Enfant ^{}\n\nTrois ^{}\n\nQuatre ^{}\n",
                alias("p1"),
                alias("p2"),
                alias("c2"),
                alias("p3"),
                alias("p4")
            ),
            vec!["p5"],
        ),
    ];
    for (human, written, blocks) in cases {
        let (page, base, key) = rebased(human);
        let ctx = PlanContext {
            base: Base::Changed(&base),
            key: &key,
            forbid_external_media: false,
        };
        let refusal = refused(
            &page,
            &[Operation::ReplacePage {
                blocks: md(&written),
                allow_deleting_content: false,
            }],
            &ctx,
        );
        assert_eq!(
            (refusal.index, refusal.reason),
            (0, Reason::RevisionConflict),
            "{human}"
        );
        for id in &blocks {
            assert!(
                refusal.blocks.contains(&id.to_string()),
                "{human} : {:?}",
                refusal.blocks
            );
        }
        // `three_way` seule dit le même conflit.
        let now = page.read_blocks();
        let mut target = md(&written);
        resolve_aliases(&mut target, &Anchors::of(&now));
        assert!(three_way(&base, &target, &now, &key).is_err());
    }
}

fn resolve_aliases(blocks: &mut [Block], anchors: &Anchors) {
    for b in blocks {
        if let Ok(id) = anchors.resolve(&b.id) {
            b.id = id.to_string();
        }
        resolve_aliases(&mut b.children, anchors);
    }
}

// ------------------------------------------------------------------- ce qui a changé (`since`)

/// `changed` dit les blocs dont une empreinte a changé, les blocs partis (par l'alias de la page
/// d'avant), et le bloc après lequel un bloc est arrivé, chacun avec son rang dans la page d'avant
/// (R1226).
#[test]
fn changed_tells_changed_gone_and_arrived_blocks() {
    let page = page_with(FIVE);
    let key = HashKey::new();
    let read = page.read_blocks();
    let before = hashes(&read, &key);
    let alias_before = Anchors::of(&read);
    let now_hashes = hashes(&read, &key);
    let none = changed(&before, &read, &now_hashes);
    assert!(!none.any);
    assert!(none.blocks.is_empty());

    page.write_diff(
        &parse("Un ^p1\n\nDeux ^p2\n  Enfant changé ^c2\n\nTrois ^p3\n\nArrivé\n\nQuatre ^p4\n")
            .unwrap(),
    )
    .unwrap();
    let now = page.read_blocks();
    let report = changed(&before, &now, &hashes(&now, &key));
    assert!(report.any);
    let anchors = Anchors::of(&now);
    let seen: Vec<(String, Option<usize>, Why)> = report
        .blocks
        .iter()
        .map(|c| (c.alias.clone(), c.rank, c.why))
        .collect();
    assert_eq!(
        seen,
        vec![
            (
                anchors.alias("c2").unwrap().to_string(),
                Some(2),
                Why::Content
            ),
            (
                anchors.alias("p3").unwrap().to_string(),
                Some(3),
                Why::Arrival
            ),
            (
                anchors.alias("p4").unwrap().to_string(),
                Some(4),
                Why::Place
            ),
            (
                alias_before.alias("p5").unwrap().to_string(),
                Some(5),
                Why::Gone
            ),
        ]
    );
}

// ------------------------------------------------------------- le vrai Yjs, par S2

/// `str_replace` écrit, octet pour octet, l'écriture par différence des scènes de `tests/diff.rs`,
/// dont les frappes concurrentes du vrai Yjs (`keys.json`) sont jugées là : ce qu'une opération
/// garde des frappes est ce que l'écriture par différence en garde.
#[test]
fn str_replace_writes_the_diff_write_proven_against_yjs() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diff/scenes.json");
    let scenes: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = [
        ("gras", "gras", "gros"),
        ("lien", "lien", "lieu"),
        ("après une mention", "fin.", "finale."),
        ("en tête", "Début", "Le début"),
        ("milieu long", "texte à remplacer", "mot neuf"),
    ];
    for (scene, old, new) in cases {
        let stored = &scenes[scene];
        let base: Vec<u8> = serde_json::from_value(stored["base"].clone()).unwrap();
        let write: Vec<u8> = serde_json::from_value(stored["write"].clone()).unwrap();
        let page = PageDoc::from_updates_for(2, [base.as_slice()]).unwrap();
        let blocks = page.read_blocks();
        let key = HashKey::new();
        let plan = ops::plan(&blocks, &[str_replace(old, new)], &current(&key)).unwrap();
        let written = page.apply(&plan).unwrap();
        assert_eq!(written.update, write, "{scene}");
    }
}

// ------------------------------------------------------------------ les fixtures BlockNote

/// Sur les pages écrites par le vrai BlockNote (`all-blocks`, `non-default`), chaque opération
/// n'écrit que ce qu'elle change : les blocs qu'elle ne touche pas gardent leurs éléments, la page
/// se lit comme le plan, et relue de son journal comme de son état.
#[test]
fn operations_on_the_blocknote_fixtures_touch_only_their_blocks() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/mmd/fixtures/yjs");
    for name in ["all-blocks", "non-default"] {
        let state = std::fs::read(root.join(format!("{name}.bin"))).unwrap();
        let fresh = || PageDoc::from_updates([state.as_slice()]).unwrap();
        let blocks = fresh().read_blocks();
        let top: Vec<&Block> = blocks.iter().collect();
        let text_block = top
            .iter()
            .rev()
            .find(|b| mementum_md::block_text(b).chars().count() >= 4 && b.children.is_empty())
            .expect("un bloc de texte");
        let text = mementum_md::block_text(text_block);
        let word: String = text.chars().take(4).collect();
        let first = top[0].id.clone();
        let last = top[top.len() - 1].id.clone();
        let middle = top[top.len() / 2].id.clone();
        let cases: Vec<(Vec<Operation>, Vec<String>)> = vec![
            (
                vec![Operation::StrReplace {
                    old: word.clone(),
                    new: "Mot!".into(),
                    anchor: Some(text_block.id.clone()),
                    replace_all: true,
                }],
                vec![text_block.id.clone()],
            ),
            (
                vec![Operation::Insert {
                    at: Place::After(first.clone()),
                    blocks: md("Inséré par un agent.\n"),
                }],
                vec![],
            ),
            (
                vec![Operation::MoveBlock {
                    anchor: last.clone(),
                    to: Place::Start,
                }],
                vec![last.clone()],
            ),
            (
                vec![Operation::DeleteBlock {
                    anchor: middle.clone(),
                    allow_deleting_content: true,
                }],
                vec![middle.clone()],
            ),
        ];
        for (ops, own) in cases {
            let page = fresh();
            let before = identities(&page);
            let read = page.read_blocks();
            let key = HashKey::new();
            let plan = ops::plan(&read, &ops, &current(&key)).unwrap();
            let expected = without_ids(plan.blocks().to_vec());
            drop(plan);
            let written = run(&page, &ops).unwrap();
            assert_eq!(
                without_ids(page.read_blocks()),
                expected,
                "{name} : {ops:?}"
            );
            assert!(!written.fell_back, "{name}");
            let after = identities(&page);
            let mut owned: HashSet<String> = written.touched.iter().cloned().collect();
            owned.extend(own);
            for (id, elements) in &before {
                if !owned.contains(id) && !is_under(&read, id, &owned) {
                    assert_eq!(
                        after.get(id),
                        Some(elements),
                        "{name} : {id} touché par {ops:?}"
                    );
                }
            }
        }
    }
}

/// Vrai si le bloc `id` est sous un des blocs `owned`.
fn is_under(blocks: &[Block], id: &str, owned: &HashSet<String>) -> bool {
    fn walk(blocks: &[Block], id: &str, owned: &HashSet<String>, inside: bool) -> Option<bool> {
        for b in blocks {
            if b.id == id {
                return Some(inside);
            }
            if let Some(found) = walk(&b.children, id, owned, inside || owned.contains(&b.id)) {
                return Some(found);
            }
        }
        None
    }
    walk(blocks, id, owned, false).unwrap_or(false)
}

// ------------------------------------------------------------------------------- mesure

/// p50 et pire de `runs` passages, en millisecondes.
fn timed<R>(runs: usize, mut f: impl FnMut() -> R) -> (f64, f64) {
    let mut ms: Vec<f64> = (0..runs)
        .map(|_| {
            let start = std::time::Instant::now();
            std::hint::black_box(f());
            start.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    ms.sort_by(f64::total_cmp);
    (ms[ms.len() / 2], ms[ms.len() - 1])
}

/// Mesure (Task S3 ; release, une fois : `cargo test --release -p mementum-crdt --test ops
/// measure_hashes_and_one_operation_on_10000_blocks -- --ignored --nocapture`) : sur la page de
/// 10 000 paragraphes des budgets, les empreintes (R1226, ce que S4 calcule à chaque lecture
/// servie), l'index des ancres, le plan d'une opération (avec et sans ancre, en `rebase`), son
/// écriture, et la fusion à trois d'une page inchangée.
#[test]
#[ignore]
fn measure_hashes_and_one_operation_on_10000_blocks() {
    let mmd = (0..10_000)
        .map(|i| format!("Paragraphe numéro {i}"))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    let page = page_with(&mmd);
    let state = page.encode_state();
    let blocks = page.read_blocks();
    let key = HashKey::new();
    let base = hashes(&blocks, &key);
    let alias = Anchors::of(&blocks)
        .alias(&blocks[5_000].id)
        .unwrap()
        .to_string();
    let anchored = [Operation::StrReplace {
        old: "numéro".into(),
        new: "n°".into(),
        anchor: Some(alias.clone()),
        replace_all: false,
    }];
    let unanchored = [str_replace("Paragraphe numéro 5000", "Paragraphe n° 5000")];
    let rebase = PlanContext {
        base: Base::Changed(&base),
        key: &key,
        forbid_external_media: true,
    };
    let lines = [
        ("lecture des blocs", timed(5, || page.read_blocks())),
        ("empreintes", timed(9, || hashes(&blocks, &key))),
        ("index des ancres", timed(9, || Anchors::of(&blocks))),
        (
            "plan d'un str_replace à une ancre",
            timed(9, || ops::plan(&blocks, &anchored, &current(&key)).is_ok()),
        ),
        (
            "plan d'un str_replace sans ancre",
            timed(9, || {
                ops::plan(&blocks, &unanchored, &current(&key)).is_ok()
            }),
        ),
        (
            "plan d'un str_replace à une ancre, en rebase",
            timed(9, || ops::plan(&blocks, &anchored, &rebase).is_ok()),
        ),
        (
            "page rechargée de son état, lue, plan et écriture d'un str_replace à une ancre",
            timed(5, || {
                let page = PageDoc::from_updates([state.as_slice()]).unwrap();
                let read = page.read_blocks();
                let plan = ops::plan(&read, &anchored, &current(&key)).unwrap();
                page.apply(&plan).unwrap().update.len()
            }),
        ),
        (
            "fusion à trois d'une page inchangée",
            timed(5, || three_way(&base, &blocks, &blocks, &key).is_ok()),
        ),
    ];
    for (what, (p50, worst)) in lines {
        println!(
            "MESURE opérations des agents, 10 000 paragraphes, {what} : p50 {p50:.1} ms, pire {worst:.1}"
        );
    }
    println!(
        "MESURE empreintes de 10 000 paragraphes : {} octets",
        base.bytes()
    );
}

// ---------------------------------------------------------------------- la table de cas

fn table() -> serde_json::Value {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/agents/operations.json");
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
        .unwrap_or_else(|e| panic!("{} : {e}", path.display()))
}

/// Une ancre de la table : `alias:<id>` désigne l'alias du bloc `<id>` dans la page de départ.
fn anchor_of(value: &serde_json::Value, anchors: &Anchors) -> Option<String> {
    let text = value.as_str()?;
    Some(match text.strip_prefix("alias:") {
        Some(id) => anchors
            .alias(id)
            .unwrap_or_else(|| panic!("pas d'alias pour {id}"))
            .to_string(),
        None => text.to_string(),
    })
}

/// Une opération de la table, sous la forme de l'outil `update_page` (`spec/mcp/tools.json`).
fn operation_of(
    index: usize,
    value: &serde_json::Value,
    anchors: &Anchors,
) -> Result<Operation, OpRefusal> {
    let anchor = |name: &str| anchor_of(&value[name], anchors);
    let flag = |name: &str| value[name].as_bool().unwrap_or(false);
    let markdown = || read_markdown(index, value["markdown"].as_str().expect("markdown"));
    let place = || match (
        anchor("after"),
        anchor("before"),
        value["position"].as_str(),
    ) {
        (Some(after), None, None) => Place::After(after),
        (None, Some(before), None) => Place::Before(before),
        (None, None, Some("start")) => Place::Start,
        (None, None, Some("end")) => Place::End,
        other => panic!("une seule place : {other:?}"),
    };
    Ok(match value["op"].as_str().expect("op") {
        "str_replace" => Operation::StrReplace {
            old: value["old"].as_str().unwrap().into(),
            new: value["new"].as_str().unwrap().into(),
            anchor: anchor("anchor"),
            replace_all: flag("replace_all"),
        },
        "insert" => Operation::Insert {
            at: place(),
            blocks: markdown()?,
        },
        "replace_block" => Operation::ReplaceBlock {
            anchor: anchor("anchor").unwrap(),
            blocks: markdown()?,
            allow_deleting_content: flag("allow_deleting_content"),
        },
        "delete_block" => Operation::DeleteBlock {
            anchor: anchor("anchor").unwrap(),
            allow_deleting_content: flag("allow_deleting_content"),
        },
        "move_block" => Operation::MoveBlock {
            anchor: anchor("anchor").unwrap(),
            to: place(),
        },
        "replace_page" => Operation::ReplacePage {
            blocks: markdown()?,
            allow_deleting_content: flag("allow_deleting_content"),
        },
        other => panic!("opération inconnue : {other}"),
    })
}

/// La table de cas `spec/agents/operations.json` : pour chaque page et chaque liste d'opérations,
/// le MMD écrit, ou le refus (rang et raison) ; un refus n'écrit rien.
#[test]
fn the_operations_table_holds() {
    let table = table();
    let cases = table["cases"].as_array().expect("cases");
    assert!(cases.len() >= 20, "{}", cases.len());
    let mut names = HashSet::new();
    for case in cases {
        let name = case["name"].as_str().expect("name");
        assert!(names.insert(name), "{name} : deux fois");
        let page = page_with(case["page"].as_str().expect("page"));
        let anchors = Anchors::of(&page.read_blocks());
        let key = HashKey::new();
        let ctx = PlanContext {
            base: Base::Current,
            key: &key,
            forbid_external_media: case["media_guard"].as_bool().unwrap_or(false),
        };
        let operations: Result<Vec<Operation>, OpRefusal> = case["operations"]
            .as_array()
            .expect("operations")
            .iter()
            .enumerate()
            .map(|(i, op)| operation_of(i, op, &anchors))
            .collect();
        let expect = &case["expect"];
        let outcome = operations.and_then(|ops| {
            let state = page.encode_state();
            let result = run_in(&page, &ops, &ctx);
            if result.is_err() {
                assert_eq!(page.encode_state(), state, "{name} : rien n'est écrit");
            }
            result
        });
        match (expect.get("mmd"), expect.get("refused"), outcome) {
            (Some(mmd), None, Ok(_)) => {
                assert_eq!(mmd_of(&page), mmd.as_str().unwrap(), "{name}");
            }
            (None, Some(refused), Err(refusal)) => {
                assert_eq!(
                    (refusal.index as u64, refusal.reason.code()),
                    (
                        refused["index"].as_u64().unwrap(),
                        refused["reason"].as_str().unwrap()
                    ),
                    "{name} : {}",
                    refusal.message
                );
                assert!(!refusal.message.is_empty());
            }
            (_, _, outcome) => panic!("{name} : {outcome:?}"),
        }
    }
}
