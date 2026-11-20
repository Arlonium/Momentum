//! Step 2 : l'écriture par différence contre `rewrite_blocks`, sur une page de 10 000 paragraphes
//! et sur la fixture `all-blocks` (Ruling R1227, seuil de repli).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{Block, Content, Inline, PropValue, RenderOpts, Styles, normalize, parse, render};
use yrs::branch::BranchID;
use yrs::updates::decoder::Decode;
use yrs::{ReadTxn, StateVector, Transact, Xml, XmlElementRef, XmlFragment, XmlOut};

use crate::diff::{InlineMode, Options, Stats, write_diff};
use crate::stats::{median, ms, summary};

const BLOCKS: usize = 10_000;

fn paragraph(i: usize, changed: bool) -> String {
    if changed {
        format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire, modifié.")
    } else {
        format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire.")
    }
}

fn stripped(blocks: &[Block]) -> Vec<Block> {
    blocks
        .iter()
        .map(|b| Block {
            id: String::new(),
            children: stripped(&b.children),
            ..b.clone()
        })
        .collect()
}

/// Ce qui fait l'identité Yjs d'un bloc : son élément, son nœud de contenu et les enfants de
/// celui-ci (textes et éléments en ligne). Un bloc que l'écriture n'a pas touché garde la même.
fn signatures(page: &PageDoc) -> HashMap<String, Vec<BranchID>> {
    let doc = page.yrs_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    let mut out = HashMap::new();
    for node in fragment.successors(&txn) {
        let XmlOut::Element(e) = node else { continue };
        let tag = e.tag().to_string();
        if !matches!(tag.as_str(), "blockContainer" | "columnList" | "column") {
            continue;
        }
        let id = crate::blocknote::attr_string(&txn, &e, "id");
        let mut sig = vec![crate::diff::bid(&e)];
        if tag == "blockContainer" {
            for child in e.children(&txn) {
                if let XmlOut::Element(c) = &child
                    && c.tag().as_ref() != "blockGroup"
                {
                    sig.push(crate::diff::bid(c));
                    for inner in c.children(&txn) {
                        sig.push(XmlOut::id(&inner));
                    }
                }
            }
        }
        out.insert(id, sig);
    }
    out
}

fn units(before: &[u8], after: &[u8]) -> u64 {
    let b = StateVector::decode_v1(before).unwrap();
    let a = StateVector::decode_v1(after).unwrap();
    a.iter()
        .map(|(c, clock)| (*clock - b.get(c)) as u64)
        .sum()
}

struct Row {
    diff: Vec<Duration>,
    stats: Stats,
    diff_units: u64,
    sig_changed: usize,
    rewrite: Vec<Duration>,
    rewrite_bytes: usize,
    rewrite_units: u64,
    keep_ids: Duration,
}

fn measure(state: &[u8], target: &[Block], opts: Options, reps: usize, rewrite_reps: usize) -> Row {
    let mut diff = Vec::new();
    let mut last = None;
    let mut diff_units = 0;
    let mut sig_changed = 0;
    let wanted = render(&stripped(&normalize(target.to_vec())), RenderOpts::default()).markdown;
    for _ in 0..reps {
        let page = PageDoc::from_updates([state]).unwrap();
        let before = signatures(&page);
        let sv = page.state_vector();
        let start = Instant::now();
        let stats = write_diff(&page, target, opts).expect("page de forme connue");
        diff.push(start.elapsed());
        diff_units = units(&sv, &page.state_vector());
        let got = render(&stripped(&page.read_blocks()), RenderOpts::default()).markdown;
        assert_eq!(got, wanted, "la page écrite ne se lit pas comme les blocs voulus");
        // La page relue de son journal est la page vivante.
        let mut updates: Vec<&[u8]> = vec![state];
        updates.extend(stats.updates.iter().map(Vec::as_slice));
        let reloaded = PageDoc::from_updates(updates).unwrap();
        assert_eq!(reloaded.read_blocks(), page.read_blocks(), "relue ≠ vivante");
        let after = signatures(&page);
        sig_changed = before
            .iter()
            .filter(|(id, sig)| after.get(*id).is_some_and(|a| a != *sig))
            .count();
        last = Some(stats);
    }
    // `keep_ids` seul (il est compté dans la durée de l'écriture par différence).
    let keep_ids = {
        let page = PageDoc::from_updates([state]).unwrap();
        let current = page.read_blocks();
        let mut t = normalize(target.to_vec());
        let start = Instant::now();
        crate::kept_ids::keep_ids(&current, &mut t);
        start.elapsed()
    };
    let mut rewrite = Vec::new();
    let mut rewrite_bytes = 0;
    let mut rewrite_units = 0;
    for _ in 0..rewrite_reps {
        let page = PageDoc::from_updates([state]).unwrap();
        let sv = page.state_vector();
        let start = Instant::now();
        let update = page.rewrite_blocks(target).unwrap();
        rewrite.push(start.elapsed());
        rewrite_bytes = update.len();
        rewrite_units = units(&sv, &page.state_vector());
    }
    Row {
        diff,
        stats: last.unwrap(),
        diff_units,
        sig_changed,
        rewrite,
        rewrite_bytes,
        rewrite_units,
        keep_ids,
    }
}

fn print(name: &str, row: &Row) {
    let s = &row.stats;
    println!("--- {name}");
    println!(
        "  différence : {} ; {} octets en {} mise(s) à jour ; {} unités Yjs écrites ; keep_ids {}",
        summary(&row.diff),
        s.bytes(),
        s.updates.len(),
        row.diff_units,
        ms(row.keep_ids),
    );
    println!(
        "  phases (dernier passage) : lecture des blocs {} ; keep_ids {} ; plan {} ; écriture {}",
        ms(s.phases[0]),
        ms(s.phases[1]),
        ms(s.phases[2]),
        ms(s.phases[3])
    );
    println!(
        "  blocs : {} courants ; recréés {} ; insérés {} ; retirés {} ; props {} ; en ligne entier {} ; \
         en ligne par préfixe {} ; tableaux {} ; blocs dont l'identité Yjs a changé : {}",
        s.current_blocks,
        s.recreated,
        s.inserted,
        s.removed,
        s.props_changed,
        s.inline_whole,
        s.inline_affix,
        s.tables,
        row.sig_changed
    );
    if !row.rewrite.is_empty() {
        let ratio = median(&row.rewrite).as_secs_f64() / median(&row.diff).as_secs_f64();
        println!(
            "  rewrite_blocks : {} ; {} octets ; {} unités Yjs ; rapport des médianes {ratio:.1}",
            summary(&row.rewrite),
            row.rewrite_bytes,
            row.rewrite_units
        );
    }
}

fn base_page() -> (Vec<u8>, Vec<Block>) {
    let mmd = (1..=BLOCKS)
        .map(|i| paragraph(i, false))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    let blocks = parse(&mmd).unwrap();
    let (page, first) = PageDoc::new_page();
    let update = page.write_blocks(&blocks).unwrap();
    let state = page.encode_state();
    drop((first, update));
    let current = page.read_blocks();
    (state, current)
}

fn changed_every(current: &[Block], step: usize) -> Vec<Block> {
    current
        .iter()
        .enumerate()
        .map(|(k, b)| {
            if step > 0 && k % step == step / 2 {
                let mut b = b.clone();
                b.content = Content::Inline(vec![Inline::Text {
                    text: paragraph(k + 1, true),
                    styles: Styles::default(),
                }]);
                b
            } else {
                b.clone()
            }
        })
        .collect()
}

fn inserted_every(current: &[Block], step: usize) -> Vec<Block> {
    let mut out = Vec::new();
    for (k, b) in current.iter().enumerate() {
        out.push(b.clone());
        if k % step == step / 2 {
            out.push(Block {
                id: String::new(),
                content: Content::Inline(vec![Inline::Text {
                    text: format!("Bloc neuf après le paragraphe {}.", k + 1),
                    styles: Styles::default(),
                }]),
                ..b.clone()
            });
        }
    }
    out
}

pub fn run(args: &[String]) {
    let reps: usize = arg(args, "--reps").unwrap_or(7);
    let only: Option<String> = arg(args, "--only");
    let wanted = |name: &str| only.as_deref().is_none_or(|o| name.contains(o));
    let whole1 = Options {
        inline: InlineMode::Whole,
        two_txns: false,
    };
    let whole2 = Options {
        two_txns: true,
        ..whole1
    };
    let affix1 = Options {
        inline: InlineMode::Affix,
        two_txns: false,
    };

    let (state, current) = base_page();
    println!(
        "page de {BLOCKS} paragraphes : état {} octets ; mesures en release, {reps} passages \
         (rewrite_blocks : 3) ; MMD sans ancres (keep_ids donne les identifiants)",
        state.len()
    );
    let mid = BLOCKS / 2;

    if wanted("p10k-un-change") {
        let mut t = current.clone();
        t[mid].content = Content::Inline(vec![Inline::Text {
            text: paragraph(mid + 1, true),
            styles: Styles::default(),
        }]);
        let t = stripped(&t);
        for (o, label) in [(whole1, "entier, 1 txn"), (whole2, "entier, 2 txn"), (affix1, "préfixe, 1 txn")] {
            print(&format!("p10k-un-change ({label})"), &measure(&state, &t, o, reps, 3));
        }
    }
    if wanted("p10k-une-insertion") {
        let mut t = current.clone();
        t.insert(
            mid,
            Block {
                id: String::new(),
                content: Content::Inline(vec![Inline::Text {
                    text: "Un bloc neuf.".into(),
                    styles: Styles::default(),
                }]),
                ..current[0].clone()
            },
        );
        print("p10k-une-insertion", &measure(&state, &stripped(&t), whole1, reps, 3));
    }
    if wanted("p10k-un-retrait") {
        let mut t = current.clone();
        t.remove(mid);
        print("p10k-un-retrait", &measure(&state, &stripped(&t), whole1, reps, 3));
    }
    if wanted("p10k-un-deplacement") {
        let mut t = current.clone();
        let b = t.remove(100);
        t.insert(9000, b);
        print("p10k-un-deplacement", &measure(&state, &stripped(&t), whole1, reps, 3));
    }
    if wanted("p10k-moitie") {
        let t = stripped(&changed_every(&current, 2));
        for (o, label) in [(whole1, "entier, 1 txn"), (whole2, "entier, 2 txn"), (affix1, "préfixe, 1 txn")] {
            print(&format!("p10k-moitie ({label})"), &measure(&state, &t, o, reps.min(5), 3));
        }
    }
    if wanted("p10k-echelle") {
        // La part de blocs changés (texte) : où `rewrite_blocks` rattrape-t-il la différence ?
        for step in [100usize, 50, 20, 10, 5, 3, 2, 1] {
            let t = stripped(&changed_every(&current, step));
            let row = measure(&state, &t, whole1, reps.min(5), 3);
            print(&format!("p10k-echelle changés 1/{step}"), &row);
        }
        // Des blocs neufs répartis : chaque insertion reparcourt la liste des frères.
        for step in [1000usize, 100, 20, 10, 4, 2, 1] {
            let t = stripped(&inserted_every(&current, step));
            let row = measure(&state, &t, whole1, reps.min(5), 3);
            print(&format!("p10k-echelle insérés 1/{step}"), &row);
        }
    }
    if wanted("p10k-inverse") {
        // Tout l'ordre renversé : un seul bloc reste en place, tous les autres sont recréés.
        let mut t = current.clone();
        t.reverse();
        let t = stripped(&t);
        for (o, label) in [(whole1, "1 txn"), (whole2, "2 txn")] {
            print(&format!("p10k-inverse ({label})"), &measure(&state, &t, o, 3, 3));
        }
    }
    if wanted("meme-client") {
        // Le contenu retiré a été écrit par le client qui écrit (l'acteur qui a déjà écrit la
        // page) : le ramasse-miettes de R367 joue.
        let plain = parse(
            &((1..=BLOCKS).map(|i| paragraph(i, false)).collect::<Vec<_>>().join("\n\n") + "\n"),
        )
        .unwrap();
        let rich_line = "**du texte** ordinaire et **du texte** ordinaire et **du texte** fin";
        let rich = parse(&(vec![rich_line; BLOCKS].join("\n\n") + "\n")).unwrap();
        for (pname, blocks) in [("10k simples", &plain), ("10k de six plages", &rich)] {
            let build = || {
                let (page, _) = PageDoc::new_page();
                page.write_blocks(blocks).unwrap();
                page
            };
            let current = build().read_blocks();
            let half = stripped(&changed_every(&current, 2));
            let all = stripped(&changed_every(&current, 1));
            let mut reversed = current.clone();
            reversed.reverse();
            let reversed = stripped(&reversed);
            for (cname, t) in [("moitié changée", &half), ("tout changé", &all), ("inverse", &reversed)] {
                let affix2 = Options { inline: InlineMode::Affix, two_txns: true };
                for (o, label) in [(whole1, "1 txn"), (whole2, "2 txn"), (affix2, "préfixe, 2 txn")] {
                    let mut times = Vec::new();
                    let mut last = None;
                    for _ in 0..3 {
                        let page = build();
                        let start = Instant::now();
                        let s = write_diff(&page, t, o).unwrap();
                        times.push(start.elapsed());
                        last = Some(s);
                    }
                    let s = last.unwrap();
                    println!(
                        "--- même client, {pname}, {cname} ({label}) : {} ; {} octets ; écriture {}",
                        summary(&times),
                        s.bytes(),
                        ms(s.phases[3])
                    );
                }
                let mut times = Vec::new();
                for _ in 0..3 {
                    let page = build();
                    let start = Instant::now();
                    page.rewrite_blocks(t).unwrap();
                    times.push(start.elapsed());
                }
                println!("    rewrite_blocks, même client : {}", summary(&times));
            }
        }
    }
    if wanted("all-blocks") {
        all_blocks(reps);
    }
}

fn arg<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

/// Un changement du bloc, selon ce qu'il porte : son texte, une cellule, une prop texte.
fn mutate(b: &Block) -> Option<Block> {
    let mut b = b.clone();
    match &mut b.content {
        Content::Inline(inline) => {
            inline.push(Inline::Text {
                text: " (modifié)".into(),
                styles: Styles::default(),
            });
        }
        Content::Table(table) => {
            table.rows[0][0].content.push(Inline::Text {
                text: " (modifié)".into(),
                styles: Styles::default(),
            });
        }
        Content::None => {
            let spec = mementum_schema::block(&b.kind)?;
            let p = spec
                .props
                .iter()
                .find(|p| p.ty == mementum_schema::PropType::String)?;
            b.props
                .insert(p.name.to_string(), PropValue::Str("modifié".into()));
        }
    }
    Some(b)
}

fn all_blocks(reps: usize) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/mmd/fixtures/yjs/all-blocks.bin");
    let bin = std::fs::read(path).unwrap();
    let page = PageDoc::from_updates([bin.as_slice()]).unwrap();
    let state = page.encode_state();
    let current = page.read_blocks();
    println!(
        "fixture all-blocks : {} blocs de premier niveau, {} en tout ; état {} octets",
        current.len(),
        count(&current),
        state.len()
    );
    let opts = Options {
        inline: InlineMode::Whole,
        two_txns: false,
    };
    // Avec leurs identifiants (une écriture qui les porte), puis sans (MMD sans ancres).
    for (label, ids) in [("avec identifiants", true), ("sans identifiants", false)] {
        let prep = |t: Vec<Block>| if ids { t } else { stripped(&t) };
        let mut cases: Vec<(String, Vec<Block>)> = Vec::new();
        cases.push(("identique".into(), prep(current.clone())));
        // Un changement de chaque type de bloc, un à la fois.
        for (k, b) in current.iter().enumerate() {
            if let Some(m) = mutate(b) {
                let mut t = current.clone();
                t[k] = m;
                cases.push((format!("change {} ({})", k, b.kind), prep(t)));
            }
        }
        let mut t = current.clone();
        t.insert(3, Block { id: String::new(), ..current[1].clone() });
        cases.push(("insertion".into(), prep(t)));
        let mut t = current.clone();
        t.remove(5);
        cases.push(("retrait".into(), prep(t)));
        let mut t = current.clone();
        let b = t.remove(2);
        t.insert(12, b);
        cases.push(("déplacement".into(), prep(t)));
        let t: Vec<Block> = current
            .iter()
            .enumerate()
            .map(|(k, b)| if k % 2 == 0 { mutate(b).unwrap_or_else(|| b.clone()) } else { b.clone() })
            .collect();
        cases.push(("moitié changée".into(), prep(t)));
        // Dans les colonnes : le texte d'un bloc d'une colonne, puis un bloc ajouté à l'autre.
        if let Some(k) = current.iter().position(|b| b.kind == "columnList") {
            let mut t = current.clone();
            let inner = &mut t[k].children[0].children[0];
            *inner = mutate(inner).unwrap();
            cases.push(("colonne : texte changé".into(), prep(t)));
            let mut t = current.clone();
            let extra = Block { id: String::new(), ..t[k].children[1].children[0].clone() };
            t[k].children[1].children.push(extra);
            cases.push(("colonne : bloc ajouté".into(), prep(t)));
        }
        // Les enfants d'un bloc : un sous-bloc changé, un sous-bloc ajouté, tous retirés.
        if let Some(k) = current.iter().position(|b| !b.children.is_empty() && b.kind != "columnList") {
            let mut t = current.clone();
            t[k].children[0] = mutate(&t[k].children[0]).unwrap();
            cases.push(("sous-bloc changé".into(), prep(t)));
            let mut t = current.clone();
            let extra = Block { id: String::new(), ..t[k].children[0].clone() };
            t[k].children.push(extra);
            cases.push(("sous-bloc ajouté".into(), prep(t)));
            let mut t = current.clone();
            t[k].children.clear();
            cases.push(("sous-blocs retirés".into(), prep(t)));
            let mut t = current.clone();
            let child = t[k].children.remove(0);
            t.insert(k + 1, child);
            cases.push(("sous-bloc sorti au premier niveau".into(), prep(t)));
        }
        for (name, t) in cases {
            let row = measure(&state, &t, opts, reps, 1);
            print(&format!("all-blocks {label} : {name}"), &row);
        }
    }
}

fn count(blocks: &[Block]) -> usize {
    blocks.iter().map(|b| 1 + count(&b.children)).sum()
}
