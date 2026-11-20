//! La lecture d'une page (`PageDoc::read_blocks`, puis le rendu en MMD de `GET /mmd`) croît
//! comme son document, quoi qu'un client y ait écrit (Rulings R381 et R385). La méthode et ce qui
//! la garde de passer à tort sont dans `mementum-md/tests/common/linear.rs`.

#[path = "../../mementum-md/tests/common/linear.rs"]
mod linear;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use linear::{Answer, Form, Limits, grows_like_input};
use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{MAX_COLOR_BYTES, MAX_HREF_BYTES, RenderOpts, render};
use yrs::types::{Attrs, Delta};
use yrs::{
    Any, Doc, GetString, ReadTxn, Text, Transact, Xml, XmlElementPrelim, XmlElementRef,
    XmlFragment, XmlOut, XmlTextPrelim,
};

/// Une page dont le fragment porte `build` (des éléments posés à la main, comme un client
/// pourrait les écrire), relue par un `PageDoc`.
fn page_from(build: impl FnOnce(&Doc)) -> PageDoc {
    let doc = Doc::new();
    build(&doc);
    let state = doc
        .transact()
        .encode_state_as_update_v1(&yrs::StateVector::default());
    let (page, _) = PageDoc::new_page();
    page.apply_update(&state).unwrap();
    page
}

/// Une marque `{key: value}` de BlockNote.
fn mark(key: &str, value: &str) -> Any {
    Any::Map(Arc::new(HashMap::from([(
        key.to_string(),
        Any::from(value),
    )])))
}

/// Un bloc de la page fait d'un élément `tag` (`paragraph`, ou une balise inconnue : un nœud
/// étranger) dont le texte compte `runs` lettres, toutes sous les marques `marked`, que le document
/// ne porte qu'une fois, et la marque `toggled` une lettre sur deux.
///
/// Écrit en temps linéaire : chaque lettre est un élément à elle (insérée en tête, de la dernière à
/// la première : yrs ne les fond pas), les marques communes sont posées sur tout le texte d'un coup,
/// puis l'autre en une passe (`apply_delta`). Un texte écrit d'un tenant, puis marqué une lettre
/// sur deux, faisait couper à yrs l'élément à chaque lettre, en recopiant la suite (1,7 s pour
/// 80 000 lettres, en release) ; écrire plage par plage reparcourrait le texte à chaque plage, et
/// répéterait les marques.
fn marked_runs(tag: &str, runs: usize, marked: Attrs, toggled: &str) -> PageDoc {
    page_from(|doc| write_marked_runs(doc, tag, runs, marked, toggled))
}

/// Ce que [`marked_runs`] écrit dans le fragment de `doc`.
fn write_marked_runs(doc: &Doc, tag: &str, runs: usize, marked: Attrs, toggled: &str) {
    let toggle = || {
        let mut attrs = Attrs::new();
        attrs.insert(toggled.into(), Any::Map(Arc::new(HashMap::new())));
        Some(Box::new(attrs))
    };
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let mut txn = doc.transact_mut();
    let group = fragment.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
    let container = group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
    container.insert_attribute(&mut txn, "id", "bloc");
    let element = container.push_back(&mut txn, XmlElementPrelim::empty(tag));
    let text = element.push_back(&mut txn, XmlTextPrelim::new(""));
    for i in (0..runs).rev() {
        text.insert(&mut txn, 0, if i % 2 == 0 { "a" } else { "b" });
    }
    text.format(&mut txn, 0, runs as u32, marked);
    let formats: Vec<Delta<Any>> = (0..runs)
        .map(|i| Delta::Retain(1, if i % 2 == 1 { toggle() } else { None }))
        .collect();
    text.apply_delta(&mut txn, formats);
}

/// Un paragraphe de `runs` plages d'une lettre, en italique une sur deux, toutes sous la marque
/// `name` (`textColor`, `link`…) de valeur `value`.
fn alternating_runs(runs: usize, name: &str, key: &str, value: &str) -> PageDoc {
    let mut marked = Attrs::new();
    marked.insert(name.into(), mark(key, value));
    marked_runs("paragraph", runs, marked, "italic")
}

fn size(page: &PageDoc) -> usize {
    page.encode_state().len()
}

/// Ce que lit `GET /mmd` : les blocs de la page, rendus en MMD.
fn read(page: &PageDoc) -> String {
    render(&page.read_blocks(), RenderOpts::default()).markdown
}

/// Les bornes absolues de la lecture d'une page (Ruling R433), par octet de son état Yjs : 64
/// octets de pic (au plus 22 relevés, pour un nœud étranger), et un MMD de 8 octets (au plus 2,7,
/// pour des plages alternées sous une couleur de 100 octets, que le MMD récrit à chaque plage).
const LIMITS: Limits = Limits {
    peak_per_byte: 64,
    answer: Answer::PerByte(8),
};

/// Ruling R381 : une couleur ou une adresse de lien longue, sur des plages alternées, était
/// recopiée à chaque plage lue (et le MMD la récrivait à chaque plage) : le produit de sa
/// longueur par leur nombre, en mémoire pour la couleur, en temps pour l'adresse.
#[test]
fn reading_a_page_grows_like_its_document() {
    let long_color = |n: usize| alternating_runs(n, "textColor", "stringValue", &"c".repeat(4 * n));
    let long_background =
        |n: usize| alternating_runs(n, "backgroundColor", "stringValue", &"c".repeat(4 * n));
    let allowed_color =
        |n: usize| alternating_runs(n, "textColor", "stringValue", &"c".repeat(MAX_COLOR_BYTES));
    let long_link = |n: usize| alternating_runs(n, "link", "href", &"u".repeat(4 * n));
    let allowed_link = |n: usize| alternating_runs(n, "link", "href", &"u".repeat(MAX_HREF_BYTES));
    // Depuis le Ruling R431, une couleur de plus de 100 octets ou une adresse de plus de 16 Kio,
    // relue du journal, est supprimée : les formes longues disent que cela garde la lecture
    // linéaire ; celles à la borne, que la lecture d'une marque gardée l'est aussi.
    let forms = [
        Form {
            what: "couleur longue sur des plages alternées",
            build: &long_color,
        },
        Form {
            what: "surlignage long sur des plages alternées",
            build: &long_background,
        },
        Form {
            what: "plages alternées sous une couleur permise",
            build: &allowed_color,
        },
        Form {
            what: "adresse de lien longue sur des plages alternées",
            build: &long_link,
        },
        Form {
            what: "adresse de lien à la borne sur des plages alternées",
            build: &allowed_link,
        },
    ];
    let failures = grows_like_input(&forms, size, read, String::len, LIMITS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Un paragraphe de `runs` lettres sous `keys` marques hors du schéma (`k0`, `k1`…), posées une
/// fois sur tout le texte, le gras une lettre sur deux : la mise à jour qu'un client aurait pu
/// écrire avant le Ruling R431, que la page relit de son journal.
fn unknown_marks(keys: usize, runs: usize) -> PageDoc {
    let mut attrs = Attrs::new();
    for k in 0..keys {
        attrs.insert(format!("k{k}").into(), Any::Bool(true));
    }
    marked_runs("paragraph", runs, attrs, "bold")
}

/// Un nœud étranger (balise inconnue dans un `blockContainer`) dont le texte de `runs` lettres
/// porte la marque `name` de valeur `value` sur tout le texte, le gras une lettre sur deux.
fn foreign_marked(runs: usize, name: &str, value: Any) -> PageDoc {
    let mut marked = Attrs::new();
    marked.insert(name.into(), value);
    marked_runs("widget", runs, marked, "bold")
}

/// Constat critique C1 de la re-relecture du tour 4 (Rulings R431, R432) : des marques hors du
/// schéma, nombreuses, sur des plages alternées, coûtaient leur nombre multiplié par celui des
/// plages à chaque lecture (701 Ko envoyés, 5 Gio pour une lecture) ; dans un nœud étranger, une
/// marque longue était récrite en JSON à chaque plage (188 Ko envoyés, 86 Mo de MMD). Relues du
/// journal, les marques hors du schéma sont supprimées (R431) ; une marque du schéma n'est écrite
/// dans le JSON d'un nœud étranger qu'à la plage où elle change (R432).
#[test]
fn reading_marks_grows_like_the_document() {
    let unknown = |n: usize| unknown_marks(n, n);
    let foreign_unknown = |n: usize| foreign_marked(n, "note", Any::from("n".repeat(4 * n)));
    let foreign_link =
        |n: usize| foreign_marked(n, "link", mark("href", &"u".repeat(MAX_HREF_BYTES)));
    let forms = [
        Form {
            what: "marques hors du schéma, nombreuses, sur des plages alternées",
            build: &unknown,
        },
        Form {
            what: "nœud étranger, marque hors du schéma longue sur des plages alternées",
            build: &foreign_unknown,
        },
        Form {
            what: "nœud étranger, adresse à la borne sur des plages alternées",
            build: &foreign_link,
        },
    ];
    let failures = grows_like_input(&forms, size, read, String::len, LIMITS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Ruling R381 : une couleur plus longue que la borne est lue absente, comme `parse` la refuse ;
/// à la borne, elle est lue. Le MMD rendu se relit.
#[test]
fn a_color_longer_than_the_bound_is_read_absent() {
    for (len, kept) in [(MAX_COLOR_BYTES, true), (MAX_COLOR_BYTES + 1, false)] {
        let page = alternating_runs(4, "textColor", "stringValue", &"c".repeat(len));
        let mmd = read(&page);
        assert_eq!(mmd.contains("<span color="), kept, "{len} octets : {mmd}");
        mementum_md::parse(&mmd).unwrap_or_else(|e| panic!("{mmd} : {e}"));
    }
}

/// Des listes de colonnes d'une seule colonne (que la lecture défait), imbriquées sur `depth`
/// niveaux, un bloc par niveau.
fn nested_single_columns(depth: usize) -> PageDoc {
    page_from(|doc| {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let mut group: XmlElementRef =
            fragment.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
        for i in 0..depth {
            let list = group.push_back(&mut txn, XmlElementPrelim::empty("columnList"));
            let column = list.push_back(&mut txn, XmlElementPrelim::empty("column"));
            let container = column.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
            container.insert_attribute(&mut txn, "id", format!("b{i}"));
            let paragraph = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
            paragraph.push_back(&mut txn, XmlTextPrelim::new("x"));
            group = container.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        }
    })
}

/// Ruling R385 : une liste de colonnes défaite était lue deux fois, pour la juger puis défaite ;
/// imbriquées, chaque niveau coûtait le double du précédent (vingt niveaux : 2,2 s en release ;
/// cinquante tiennent sous la borne de profondeur du document, soit des jours d'acteur). Au plus
/// profond, la lecture et la recherche d'un bloc tiennent dans une seconde ; la lecture se fait
/// sur un fil à part, pour que le test échoue au bout du délai au lieu de durer des heures.
#[test]
fn nested_invalid_column_lists_are_read_once() {
    let depth = 50;
    let page = nested_single_columns(depth);
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || {
        let start = Instant::now();
        let blocks = page.read_blocks();
        let read = start.elapsed();
        let start = Instant::now();
        let found = page.has_block(&format!("b{}", depth - 1)) && !page.has_block("absent");
        let _ = sent.send((blocks, read, found, start.elapsed()));
    });
    let (blocks, read, found, search) = received
        .recv_timeout(Duration::from_secs(10))
        .expect("la lecture de 50 listes défaites imbriquées dure plus de dix secondes");
    assert!(
        found,
        "le bloc le plus profond se trouve, un bloc absent non"
    );
    // Chaque liste défaite laisse son bloc à sa place : un bloc par niveau, chacun enfant du
    // précédent ; au premier niveau, à côté du paragraphe vide de la page neuve.
    assert_eq!(blocks.len(), 2, "le paragraphe de la page neuve, et b0");
    let first = blocks.iter().position(|b| b.id == "b0").expect("b0");
    let mut level = &blocks[first..=first];
    for i in 0..depth {
        assert_eq!(level.len(), 1, "niveau {i}");
        assert_eq!(level[0].id, format!("b{i}"));
        level = &level[0].children;
    }
    assert!(
        read < Duration::from_secs(1) && search < Duration::from_secs(1),
        "lecture {read:?}, recherche {search:?}"
    );
}

/// `depth` listes de colonnes imbriquées sans bloc entre elles, chacune de deux colonnes : la
/// première porte la liste suivante, la seconde est vide (la liste est défaite). Sous la dernière,
/// `width` colonnes vides puis un bloc.
fn wrapped_columns(depth: usize, width: usize) -> PageDoc {
    page_from(|doc| {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let group = fragment.insert(&mut txn, 0, XmlElementPrelim::empty("blockGroup"));
        let mut parent: XmlElementRef = group;
        for _ in 0..depth {
            let list = parent.push_back(&mut txn, XmlElementPrelim::empty("columnList"));
            let first = list.push_back(&mut txn, XmlElementPrelim::empty("column"));
            list.push_back(&mut txn, XmlElementPrelim::empty("column"));
            parent = first;
        }
        for _ in 0..width {
            parent.push_back(&mut txn, XmlElementPrelim::empty("column"));
        }
        let container = parent.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "fond");
        let paragraph = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        paragraph.push_back(&mut txn, XmlTextPrelim::new("x"));
    })
}

/// Ruling R385 : une colonne n'est examinée qu'une fois par lecture, quelle que soit la
/// profondeur des listes défaites qui la contiennent. Sans cela, chaque liste défaite examinait de
/// nouveau tout ce qu'elle contient : cinquante niveaux au-dessus de 5 000 colonnes vides, un
/// quart de million d'éléments parcourus. Les octets alloués (une liste d'enfants par élément parcouru) le
/// disent sans dépendre du poste : avec cinquante niveaux, guère plus qu'avec un.
#[test]
fn wrapped_columns_are_examined_once() {
    let _alone = linear::ALONE.lock().unwrap_or_else(|e| e.into_inner());
    let width = 5_000;
    let (shallow, deep) = (wrapped_columns(1, width), wrapped_columns(50, width));
    for (what, read) in [
        (
            "lecture",
            &(|page: &PageDoc| drop(page.read_blocks())) as &dyn Fn(&PageDoc),
        ),
        ("recherche", &|page: &PageDoc| {
            assert!(page.has_block("fond"));
        }),
    ] {
        let (one, fifty) = (
            linear::once(|| read(&shallow)),
            linear::once(|| read(&deep)),
        );
        assert!(
            fifty.allocated <= 4 * one.allocated,
            "{what} : {} octets alloués sous un niveau, {} sous cinquante",
            one.allocated,
            fifty.allocated
        );
    }
}

// ------------------------------------------------------- écriture par différence (Task S2)

/// Une écriture par différence à mesurer : la page de départ (son état), les blocs écrits, et la
/// taille de leur MMD.
struct DiffCase {
    state: Vec<u8>,
    target: Vec<mementum_md::Block>,
    mmd: usize,
}

/// La page que le serveur écrit d'un MMD, et les blocs qu'il en lit, retouchés par `edit`.
fn diff_case(mmd: &str, edit: impl FnOnce(&mut Vec<mementum_md::Block>)) -> DiffCase {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&mementum_md::parse(mmd).unwrap())
        .unwrap();
    page_case(&page, edit)
}

fn page_case(page: &PageDoc, edit: impl FnOnce(&mut Vec<mementum_md::Block>)) -> DiffCase {
    let mut target = page.read_blocks();
    edit(&mut target);
    DiffCase {
        state: page.encode_state(),
        mmd: render(&target, RenderOpts::default()).markdown.len(),
        target,
    }
}

/// `n` paragraphes, séparés comme le MMD les sépare.
fn paragraphs_mmd(n: usize) -> String {
    let lines: Vec<String> = (0..n).map(|i| format!("Paragraphe numéro {i}")).collect();
    lines.join("\n\n") + "\n"
}

/// Une lettre ajoutée au premier texte du contenu de `block`, au milieu de ses plages.
fn retouched_text(block: &mut mementum_md::Block) {
    fn first_text(content: &mut [mementum_md::Inline]) -> Option<&mut String> {
        let middle = content.len() / 2;
        content[middle..].iter_mut().find_map(|i| match i {
            mementum_md::Inline::Text { text, .. } => Some(text),
            mementum_md::Inline::Link { children, .. } => first_text(children),
            _ => None,
        })
    }
    match &mut block.content {
        mementum_md::Content::Inline(content) => {
            if let Some(text) = first_text(content) {
                text.push('x');
            }
        }
        mementum_md::Content::Table(table) => {
            let middle = table.rows.len() / 2;
            if let Some(mementum_md::Inline::Text { text, .. }) =
                table.rows[middle][0].content.first_mut()
            {
                text.push('x');
            }
        }
        mementum_md::Content::None => {}
    }
}

/// Task S2 du lot 5 (Rulings R380, R1254, R1255, R1281, R1284) : l'écriture par différence croît
/// comme la page et les blocs écrits, quelle que soit leur forme : un bloc changé, tous, un
/// déplacé, un bloc sur deux retiré, dix sur onze retirés (les retraits se font par identifiant,
/// sans parcourir la liste), une feuille d'une liste imbriquée, un bloc d'une colonne, une cellule
/// d'un grand tableau, une lettre d'un long texte sous un lien de 16 Kio ou sous une couleur (une
/// marque se lit une fois, jamais recopiée par caractère : recopiée, le pic de mémoire dépasse sa
/// borne). Ce qui n'y est pas, et que `diff_cost` compte : des blocs insérés ou recréés en nombre
/// dans une même liste (des paires voisines échangées), dont yrs retrouve chaque place en
/// reparcourant la liste depuis son début, comme pour la récriture complète ; et, au-delà des
/// tailles de ce test (une dizaine de milliers de paragraphes en debug), des milliers de blocs
/// retirés à part, que la validation de yrs fond un à un dans la liste des blocs de leur client
/// (R1284 : un sur deux retiré, 0,1 s sur 10 000 paragraphes, 3,8 s sur 40 000, en release ; la
/// récriture complète, 0,2 et 6 s). La mesure `measure_diff_write_of_one_block_in_10000`
/// (`tests/diff.rs`) relève ces formes.
#[test]
fn a_diff_write_grows_like_the_page() {
    let one = |n: usize| {
        diff_case(&paragraphs_mmd(n), |blocks| {
            retouched_text(&mut blocks[n / 2]);
        })
    };
    let all = |n: usize| {
        diff_case(&paragraphs_mmd(n), |blocks| {
            blocks.iter_mut().for_each(retouched_text)
        })
    };
    let moved = |n: usize| {
        diff_case(&paragraphs_mmd(n), |blocks| {
            let block = blocks.remove(n / 2);
            blocks.push(block);
        })
    };
    let every_other = |n: usize| {
        diff_case(&paragraphs_mmd(n), |blocks| {
            let kept: Vec<_> = blocks.iter().step_by(2).cloned().collect();
            *blocks = kept;
        })
    };
    let ten_of_eleven = |n: usize| {
        diff_case(&paragraphs_mmd(n), |blocks| {
            let kept: Vec<_> = blocks.iter().step_by(11).cloned().collect();
            *blocks = kept;
        })
    };
    let nested = |n: usize| {
        let items: Vec<String> = (0..n)
            .map(|i| format!("- Élément {i}\n  - Enfant {i}\n    - Petit-enfant {i}"))
            .collect();
        diff_case(&(items.join("\n") + "\n"), |blocks| {
            retouched_text(&mut blocks[n / 2].children[0].children[0]);
        })
    };
    let columns = |n: usize| {
        let column = |side: &str| -> String {
            let lines: Vec<String> = (0..n / 2).map(|i| format!("{side} {i}")).collect();
            format!("<column>\n{}\n</column>\n", lines.join("\n\n"))
        };
        let mmd = format!(
            "<columns>\n{}{}</columns>\n",
            column("Gauche"),
            column("Droite")
        );
        diff_case(&mmd, |blocks| {
            retouched_text(&mut blocks[0].children[1].children[n / 4]);
        })
    };
    let table = |n: usize| {
        let rows: Vec<String> = (0..n).map(|i| format!("| a{i} | b{i} |")).collect();
        let mmd = format!("| x | y |\n| --- | --- |\n{}\n", rows.join("\n"));
        diff_case(&mmd, |blocks| retouched_text(&mut blocks[0]))
    };
    // Un seul bloc, seul dans sa page (une page qui garde aussi la racine d'une page neuve
    // retomberait sur la récriture complète).
    let alone = |n: usize, name: &str, key: &str, value: &str| {
        let doc = Doc::new();
        let mut marked = Attrs::new();
        marked.insert(name.into(), mark(key, value));
        write_marked_runs(&doc, "paragraph", n, marked, "italic");
        let state = doc
            .transact()
            .encode_state_as_update_v1(&yrs::StateVector::default());
        let page = PageDoc::from_updates([state.as_slice()]).unwrap();
        page_case(&page, |blocks| retouched_text(&mut blocks[0]))
    };
    let under_link = |n: usize| alone(n, "link", "href", &"u".repeat(MAX_HREF_BYTES));
    let under_color = |n: usize| alone(n, "textColor", "stringValue", &"c".repeat(MAX_COLOR_BYTES));
    let forms = [
        Form {
            what: "un paragraphe changé",
            build: &one as &dyn Fn(usize) -> DiffCase,
        },
        Form {
            what: "tous les paragraphes changés",
            build: &all,
        },
        Form {
            what: "un paragraphe déplacé",
            build: &moved,
        },
        Form {
            what: "un paragraphe sur deux retiré",
            build: &every_other,
        },
        Form {
            what: "dix paragraphes sur onze retirés",
            build: &ten_of_eleven,
        },
        Form {
            what: "un petit-enfant d'une liste imbriquée",
            build: &nested,
        },
        Form {
            what: "un bloc d'une colonne",
            build: &columns,
        },
        Form {
            what: "une cellule d'un grand tableau",
            build: &table,
        },
        Form {
            what: "une lettre d'un long texte sous un lien de 16 Kio",
            build: &under_link,
        },
        Form {
            what: "une lettre d'un long texte sous une couleur",
            build: &under_color,
        },
    ];
    let size = |case: &DiffCase| case.state.len() + case.mmd;
    let write = |case: &DiffCase| {
        let page = PageDoc::from_updates([case.state.as_slice()]).unwrap();
        let written = page.write_diff(&case.target).unwrap();
        assert!(!written.fell_back);
        written.update
    };
    let limits = Limits {
        peak_per_byte: 96,
        answer: Answer::PerByte(8),
    };
    let failures = grows_like_input(&forms, size, write, Vec::len, limits);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ------------------------------------------------------------ garde des attributs (R689)

/// Écrit un entier variable de lib0.
fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// `{"toString": 1}`.
fn object() -> Any {
    Any::Map(Arc::new(HashMap::from([(
        "toString".to_string(),
        Any::Number(yrs::any::Number::Int(1)),
    )])))
}

/// Un client d'identifiant fixé, qui a reçu `state`.
fn client_of(id: u64, state: &[u8]) -> Doc {
    use yrs::updates::decoder::Decode;
    let doc = Doc::with_options(yrs::Options {
        client_id: yrs::ClientID::new(id),
        offset_kind: yrs::OffsetKind::Utf16,
        ..yrs::Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(yrs::Update::decode_v1(state).unwrap())
        .unwrap();
    doc
}

/// Le paragraphe d'une page neuve.
fn first_paragraph<T: ReadTxn>(txn: &T) -> XmlElementRef {
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    let mut element = match fragment.get(txn, 0) {
        Some(yrs::XmlOut::Element(e)) => e,
        _ => panic!("la racine"),
    };
    for _ in 0..2 {
        element = match element.get(txn, 0) {
            Some(yrs::XmlOut::Element(e)) => e,
            _ => panic!("un élément"),
        };
    }
    element
}

/// Une page et la mise à jour qu'un client lui envoie, ou que son journal porte.
struct Sent {
    state: Vec<u8>,
    update: Vec<u8>,
}

/// `n` blocs neufs, chacun avec un attribut d'un autre type que le sien, une liste de largeurs
/// démesurée et une valeur imbriquée.
fn many_unsound_blocks(n: usize) -> Sent {
    let (_, state) = PageDoc::new_page();
    let doc = client_of(7, &state);
    let mut txn = doc.transact_mut();
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    let Some(yrs::XmlOut::Element(root)) = fragment.get(&txn, 0) else {
        panic!("la racine");
    };
    let nested = (0..8).fold(object(), |inner, _| Any::Array(vec![inner].into()));
    for i in 0..n {
        let container = root.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", format!("b{i}"));
        let heading = container.push_back(&mut txn, XmlElementPrelim::empty("heading"));
        heading.insert_attribute(&mut txn, "level", object());
        heading.insert_attribute(&mut txn, "backgroundColor", nested.clone());
        heading.insert_attribute(&mut txn, "textAlignment", "center");
        heading.insert_attribute(
            &mut txn,
            "colwidth",
            Any::Array(vec![Any::Number(yrs::any::Number::Int(120)); 200].into()),
        );
    }
    let update = txn.encode_update_v1();
    Sent { state, update }
}

/// Un attribut du paragraphe écrasé `n` fois dans la même mise à jour : une chaîne d'origines.
fn one_attribute_overwritten(n: usize) -> Sent {
    let (_, state) = PageDoc::new_page();
    let doc = client_of(7, &state);
    let mut txn = doc.transact_mut();
    let paragraph = first_paragraph(&txn);
    for i in 0..n {
        let value = if i % 2 == 0 {
            object()
        } else {
            Any::from("red")
        };
        paragraph.insert_attribute(&mut txn, "backgroundColor", value);
    }
    let update = txn.encode_update_v1();
    Sent { state, update }
}

/// Le texte du premier paragraphe d'une page neuve, écrit par le client 7 (son élément 0), puis
/// `letters(client)` : une chaîne par client, sans origine, de parent ce texte.
fn letters_of_many_clients(clients: usize, text: impl Fn(usize) -> String) -> Vec<u8> {
    let mut update = Vec::new();
    var(&mut update, clients as u64);
    for client in 0..clients {
        let text = text(client);
        update.push(1);
        var(&mut update, 1_000 + client as u64);
        update.extend([0, 0x04, 0, 7, 0]);
        var(&mut update, text.len() as u64);
        update.extend(text.as_bytes());
    }
    update.push(0);
    update
}

fn page_with_a_text() -> Vec<Vec<u8>> {
    let (_, state) = PageDoc::new_page();
    let doc = client_of(7, &state);
    let mut txn = doc.transact_mut();
    let paragraph = first_paragraph(&txn);
    paragraph.push_back(&mut txn, yrs::XmlTextPrelim::new(""));
    vec![state, txn.encode_update_v1()]
}

/// `n` clients ont chacun tapé une lettre ; chacun renvoie sa lettre et la suivante d'un tenant :
/// `n` éléments à cheval sur ce que la page connaît, coupés un à un (Ruling R784).
fn many_items_astride(n: usize) -> Sent {
    let mut journal = page_with_a_text();
    journal.push(letters_of_many_clients(n, |_| "a".to_string()));
    let state = PageDoc::from_updates(journal.iter().map(Vec::as_slice))
        .unwrap()
        .encode_state();
    let update = letters_of_many_clients(n, |_| "ab".to_string());
    Sent { state, update }
}

/// Un client a tapé une lettre ; il la renvoie avec `n` autres d'un tenant, des paires de
/// substitution comprises : un seul élément à cheval, long.
fn one_long_item_astride(n: usize) -> Sent {
    let mut journal = page_with_a_text();
    journal.push(letters_of_many_clients(1, |_| "a".to_string()));
    let state = PageDoc::from_updates(journal.iter().map(Vec::as_slice))
        .unwrap()
        .encode_state();
    let update = letters_of_many_clients(1, |_| format!("a{}", "b😀".repeat(n)));
    Sent { state, update }
}

/// Une page dont le paragraphe porte `n` lettres, chacune un élément Yjs, et une mise à jour
/// faite à la main de `n` valeurs `Any`, chacune placée par l'origine d'une de ces lettres : la
/// garde cherche, pour chacune, si son origine est une entrée de la page (la recherche suit les
/// voisins de l'origine ; sans plafond, `n` recherches de `n` pas).
fn values_placed_after_page_items(n: usize) -> Sent {
    let (_, root) = PageDoc::new_page();
    let doc = client_of(9, &root);
    let first = {
        let mut txn = doc.transact_mut();
        let paragraph = first_paragraph(&txn);
        let text = paragraph.push_back(&mut txn, XmlTextPrelim::new(""));
        let first = txn.state_vector().get(&yrs::ClientID::new(9));
        for _ in 0..n {
            text.insert(&mut txn, 0, "a");
        }
        first
    };
    let state = doc
        .transact()
        .encode_state_as_update_v1(&yrs::StateVector::default());
    // Un client, `n` éléments à partir de l'horloge 0 : contenu `Any` (8) et une origine (0x80).
    let mut update = Vec::new();
    var(&mut update, 1);
    var(&mut update, n as u64);
    var(&mut update, 11);
    var(&mut update, 0);
    for i in 0..n {
        update.push(0x88);
        var(&mut update, 9);
        var(&mut update, u64::from(first) + i as u64);
        var(&mut update, 1);
        update.extend_from_slice(&[119, 1, b'z']);
    }
    update.push(0);
    Sent { state, update }
}

/// I2 de la relecture (Ruling R704) : une rafale de mises à jour d'une valeur, chacune placée
/// après la dernière lettre d'un texte de `n` lettres. La recherche de l'origine suit les voisins
/// de gauche (tout le texte) ; chaque mise à jour est refusée à sa première recherche, avant
/// d'être appliquée : la rafale coûte une recherche par mise à jour, jamais 64.
fn burst_after_the_last_letter(n: usize) -> (Vec<u8>, Vec<Vec<u8>>) {
    let sent = values_placed_after_page_items(n);
    // Les lettres sont insérées en tête : la dernière du texte est la première écrite.
    let first = {
        let doc = client_of(12, &sent.state);
        let txn = doc.transact();
        // Le texte est le dernier type écrit par le client 9 avant ses lettres.
        txn.state_vector().get(&yrs::ClientID::new(9)) - n as u32
    };
    let updates = (0..BURST)
        .map(|k| {
            let mut update = Vec::new();
            var(&mut update, 1);
            var(&mut update, 64);
            var(&mut update, 100 + k as u64);
            var(&mut update, 0);
            for j in 0..64u64 {
                update.push(0x88);
                var(&mut update, 9);
                var(&mut update, u64::from(first) + j);
                var(&mut update, 1);
                update.extend_from_slice(&[119, 1, b'z']);
            }
            update.push(0);
            update
        })
        .collect();
    (sent.state, updates)
}

/// Une lettre `x` de `client`, à la suite de la mise à jour `update`, entre deux voisins.
fn push_letter_between(update: &mut Vec<u8>, origin: (u64, u32), right: (u64, u32)) {
    update.push(0xC4);
    for id in [origin, right] {
        var(update, id.0);
        var(update, u64::from(id.1));
    }
    update.extend_from_slice(&[1, b'x']);
}

/// L'en-tête d'une mise à jour de `n` éléments du client 77, à partir de l'horloge 0.
fn letters_header(n: usize) -> Vec<u8> {
    let mut update = Vec::new();
    var(&mut update, 1);
    var(&mut update, n as u64);
    var(&mut update, 77);
    var(&mut update, 0);
    update
}

/// Une page dont le paragraphe porte `n` lettres insérées en tête, chacune un élément et une
/// suite à elle : l'état, et l'horloge de la première.
fn page_of_letters(n: usize) -> (Vec<u8>, u32) {
    page_of_letters_by(9, n)
}

/// [`page_of_letters`], écrite par le client `client`.
fn page_of_letters_by(client: u64, n: usize) -> (Vec<u8>, u32) {
    let (_, root) = PageDoc::new_page();
    let doc = client_of(client, &root);
    let first = {
        let mut txn = doc.transact_mut();
        let paragraph = first_paragraph(&txn);
        let text = paragraph.push_back(&mut txn, XmlTextPrelim::new(""));
        let first = txn.state_vector().get(&yrs::ClientID::new(client));
        for _ in 0..n {
            text.insert(&mut txn, 0, "a");
        }
        first
    };
    let state = doc
        .transact()
        .encode_state_as_update_v1(&yrs::StateVector::default());
    (state, first)
}

/// Règles G et P2 (Ruling R853) : `n` lettres, chacune entre deux lettres d'un texte de la page.
/// Demander à yrs où vit chaque voisin parcourt le texte : `n` demandes de `n` pas.
fn letters_between_letters_of_the_page(n: usize) -> Sent {
    let (state, first) = page_of_letters(n + 1);
    let mut update = letters_header(n);
    for i in (0..n as u32).rev() {
        push_letter_between(&mut update, (9, first + i + 1), (9, first + i));
    }
    update.push(0);
    Sent { state, update }
}

/// De même dans des textes que le serveur a écrits : la garde ne connaît ses unités qu'en les
/// lisant dans ce que yrs garde, une fois pour toutes. Ni de longs textes (yrs seul met un temps
/// quadratique à couper un long texte en `n` morceaux : 936 ms pour 32 000 lettres, en release, sans
/// aucune garde), ni beaucoup de petits : chaque coupe décale les blocs du serveur qui suivent.
fn letters_between_letters_the_server_wrote(n: usize) -> Sent {
    const LETTERS: usize = 128;
    let paragraph = "a".repeat(LETTERS);
    let paragraphs = n.div_ceil(LETTERS - 1);
    let page = PageDoc::new_page().0;
    let mmd = vec![paragraph; paragraphs].join("\n\n") + "\n";
    page.write_blocks(&mementum_md::parse(&mmd).unwrap())
        .unwrap();
    let state = page.encode_state();
    let doc = client_of(77, &state);
    let before = doc.transact().state_vector();
    let texts: Vec<_> = {
        let txn = doc.transact();
        let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
        let Some(yrs::XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            panic!("la racine");
        };
        group
            .children(&txn)
            .map(|container| {
                let yrs::XmlOut::Element(container) = container else {
                    panic!("un bloc");
                };
                let Some(yrs::XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
                    panic!("un paragraphe");
                };
                match paragraph.get(&txn, 0) {
                    Some(yrs::XmlOut::Text(text)) => text,
                    _ => panic!("un texte"),
                }
            })
            .collect()
    };
    {
        let mut txn = doc.transact_mut();
        for text in &texts {
            for at in (1..LETTERS as u32).rev() {
                text.insert(&mut txn, at, "x");
            }
        }
    }
    let update = doc.transact().encode_diff_v1(&before);
    Sent { state, update }
}

/// `n` éléments dont chacun a pour origine droite le suivant de la mise à jour : le jugement de
/// l'un attend celui de l'autre, sur toute la longueur.
fn letters_that_wait_for_each_other(n: usize) -> Sent {
    let (state, first) = page_of_letters(n + 1);
    let mut update = letters_header(n);
    for i in 0..n as u32 {
        let right = if i + 1 == n as u32 {
            (9, first)
        } else {
            (77, i + 1)
        };
        push_letter_between(&mut update, (9, first + i + 1), right);
    }
    update.push(0);
    Sent { state, update }
}

/// `n` lettres qui nomment toutes la dernière d'une chaîne de `n` lettres posées l'une devant
/// l'autre : où vit cette lettre se lit en remontant la chaîne, une seule fois.
fn letters_naming_the_end_of_a_long_chain(n: usize) -> Sent {
    let (state, first) = page_of_letters(n + 1);
    let last = first + n as u32;
    let mut update = letters_header(n);
    for _ in 0..n {
        push_letter_between(&mut update, (9, last), (9, last - 1));
    }
    update.push(0);
    Sent { state, update }
}

/// Une page, puis une chaîne d'origines de `n` suites de huit lettres (deux clients qui s'y
/// relaient) : une lettre du client 7 à côté de la fin de chaque suite de sa première moitié, et, si
/// `leaves`, une lettre du client 6 à côté de la fin de chaque suite de rang pair. L'état, la dernière
/// unité de la suite de rang `i`, et le nombre de suites.
fn long_chain(n: usize, leaves: bool) -> (Vec<u8>, impl Fn(u32) -> (u64, u32), u32) {
    let (state, first) = page_of_letters(0);
    let chain = n.max(2) as u32 / 2 * 2;
    let unit = move |i: u32| -> (u64, u32) {
        if i.is_multiple_of(2) {
            (9, first + 8 * (i / 2) + 7)
        } else {
            (8, 8 * (i / 2) + 7)
        }
    };
    let mut update = Vec::new();
    var(&mut update, if leaves { 4 } else { 3 });
    for client in [9u64, 8] {
        var(&mut update, u64::from(chain / 2));
        var(&mut update, client);
        var(&mut update, u64::from(if client == 9 { first } else { 0 }));
        for i in (0..chain).filter(|i| (i % 2 == 0) == (client == 9)) {
            if i == 0 {
                update.push(0x04);
                update.push(0);
                var(&mut update, 9);
                var(&mut update, u64::from(first - 1));
            } else {
                update.push(0x84);
                var(&mut update, unit(i - 1).0);
                var(&mut update, u64::from(unit(i - 1).1));
            }
            update.push(8);
            update.extend_from_slice(b"aaaaaaaa");
        }
    }
    let beside = |update: &mut Vec<u8>, client: u64, at: &dyn Fn(u32) -> u32| {
        var(update, u64::from(chain / 2));
        var(update, client);
        update.push(0);
        for k in 0..chain / 2 {
            update.push(0x84);
            var(update, unit(at(k)).0);
            var(update, u64::from(unit(at(k)).1));
            update.extend_from_slice(&[1, b'r']);
        }
    };
    beside(&mut update, 7, &|k| k);
    if leaves {
        beside(&mut update, 6, &|k| 2 * k);
    }
    update.push(0);
    let state = PageDoc::from_updates([state.as_slice(), update.as_slice()])
        .unwrap()
        .encode_state();
    (state, unit, chain)
}

/// Règle V (Ruling R856) : `n / 2` éléments dont l'origine est une feuille posée à côté de l'unité
/// `2k` d'une longue chaîne, et l'origine droite la lettre posée à côté de l'unité `k`, loin
/// au-dessus. Remonter la chaîne pas à pas coûterait `n / 2` remontées de `k` pas. L'origine étant
/// une feuille, yrs les range sans parcourir la chaîne (voir `known_limits`).
fn letters_whose_right_is_far_up_the_chain(n: usize) -> Sent {
    let (state, _, chain) = long_chain(n, true);
    let mut update = letters_header(chain as usize / 2);
    for k in 0..chain / 2 {
        push_letter_between(&mut update, (6, k), (7, k));
    }
    update.push(0);
    Sent { state, update }
}

/// Ruling R861 : une cascade. Le premier élément a la forme N3 ; une fois récrit, il tient en une
/// suite avec ce qui précède, et le suivant a la forme N3 à son tour, jusqu'au dernier. Rejuger toute
/// la mise à jour après chaque récriture coûterait un tour par élément. Si `letters`, ses éléments
/// sont des lettres ; sinon des contenus supprimés, que yrs fond sans rien recopier : c'est alors la
/// garde que l'on mesure.
fn cascade_of_rewrites(n: usize, letters: bool) -> Sent {
    // Un identifiant de client comme Yjs les tire : chaque voisin nommé l'écrit en entier.
    const CLIENT: u64 = 1 << 44;
    let (state, first) = page_of_letters_by(CLIENT, 0);
    let doc = client_of(CLIENT, &state);
    {
        let mut txn = doc.transact_mut();
        let paragraph = first_paragraph(&txn);
        let Some(XmlOut::Text(text)) = paragraph.get(&txn, 0) else {
            panic!("le texte");
        };
        text.insert(&mut txn, 0, "abcdef");
    }
    let state = doc
        .transact()
        .encode_state_as_update_v1(&yrs::StateVector::default());
    let mut update = Vec::new();
    var(&mut update, 1);
    var(&mut update, n.max(1) as u64);
    var(&mut update, CLIENT);
    var(&mut update, u64::from(first + 6));
    for k in 0..n.max(1) as u32 {
        let (origin, right) = (
            (CLIENT, first + 5 + k),
            (CLIENT, first + if k == 0 { 4 } else { 2 }),
        );
        if letters {
            push_letter_between(&mut update, origin, right);
        } else {
            update.push(0xC1);
            for id in [origin, right] {
                var(&mut update, id.0);
                var(&mut update, u64::from(id.1));
            }
            update.push(1);
        }
    }
    update.push(0);
    Sent { state, update }
}

/// Règles G et P2 (Ruling R853) : juger les voisins des éléments d'une mise à jour croît comme
/// elle et comme la page, quelle que soit la place de ces voisins dans la liste où ils vivent.
#[test]
fn judging_the_neighbours_of_items_grows_like_the_update() {
    let forms = [
        Form {
            what: "lettres entre deux lettres de la page",
            build: &letters_between_letters_of_the_page,
        },
        Form {
            what: "lettres entre deux lettres écrites par le serveur",
            build: &letters_between_letters_the_server_wrote,
        },
        Form {
            what: "lettres qui attendent chacune la suivante",
            build: &letters_that_wait_for_each_other,
        },
        Form {
            what: "lettres qui nomment le bout d'une longue chaîne",
            build: &letters_naming_the_end_of_a_long_chain,
        },
        Form {
            what: "lettres dont la voisine de droite est loin au-dessus dans la chaîne",
            build: &letters_whose_right_is_far_up_the_chain,
        },
        Form {
            what: "éléments dont chaque récriture rend le suivant dangereux",
            build: &|n| cascade_of_rewrites(n, false),
        },
    ];
    let size = |sent: &Sent| sent.state.len() + sent.update.len();
    let judge = |sent: &Sent| {
        let page = PageDoc::from_updates([sent.state.as_slice()]).unwrap();
        let from_client = page.try_client_update(&sent.update).is_ok();
        let stored = PageDoc::from_updates([sent.state.as_slice(), sent.update.as_slice()]);
        (
            from_client,
            stored.map_or(0, |page| page.encode_state().len()),
        )
    };
    let limits = Limits {
        peak_per_byte: 96,
        answer: Answer::PerByte(8),
    };
    let failures = grows_like_input(&forms, size, judge, |_| 0, limits);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `count` boucles de trois clients : Y(i), trois lettres, a pour origine Z(i), qui a pour origine
/// la première lettre de Y(i) ; X(i) a l'origine et l'origine droite de la forme N3 dans Y(i), ce
/// qui fait chercher où vit Y(i). yrs gardera tout en attente : le calcul se fait avant.
fn loops_of_origins(count: usize) -> Vec<u8> {
    let (a, b, c) = (9_001u64, 9_002u64, 9_003u64);
    let mut u = Vec::new();
    var(&mut u, 3);
    var(&mut u, count as u64);
    var(&mut u, a);
    u.push(0);
    for i in 0..count as u64 {
        u.push(0x84);
        var(&mut u, b);
        var(&mut u, i);
        u.extend_from_slice(&[3, b'y', b'y', b'y']);
    }
    var(&mut u, count as u64);
    var(&mut u, b);
    u.push(0);
    for i in 0..count as u64 {
        u.push(0x84);
        var(&mut u, a);
        var(&mut u, 3 * i);
        u.extend_from_slice(&[1, b'z']);
    }
    var(&mut u, count as u64);
    var(&mut u, c);
    u.push(0);
    for i in 0..count as u64 {
        u.push(0xC4);
        var(&mut u, a);
        var(&mut u, 3 * i + 2);
        var(&mut u, a);
        var(&mut u, 3 * i + 1);
        u.extend_from_slice(&[1, b'x']);
    }
    u.push(0);
    u
}

/// `count` boucles de deux valeurs `Any` placées l'une par l'origine de l'autre.
fn loops_of_values(count: usize) -> Vec<u8> {
    let mut u = Vec::new();
    var(&mut u, 2);
    for (me, other) in [(9_001u64, 9_002u64), (9_002, 9_001)] {
        var(&mut u, count as u64);
        var(&mut u, me);
        u.push(0);
        for i in 0..count as u64 {
            u.push(0x88);
            var(&mut u, other);
            var(&mut u, i);
            u.extend_from_slice(&[1, 119, 1, b'v']);
        }
    }
    u.push(0);
    u
}

/// `count` boucles de deux éléments XML, chacun placé par l'origine (`by_origin`) de l'autre, ou
/// nommant l'autre pour parent.
fn loops_of_elements(count: usize, by_origin: bool) -> Vec<u8> {
    let mut u = Vec::new();
    var(&mut u, 2);
    for (me, other) in [(9_001u64, 9_002u64), (9_002, 9_001)] {
        var(&mut u, count as u64);
        var(&mut u, me);
        u.push(0);
        for i in 0..count as u64 {
            if by_origin {
                u.push(0x87);
                var(&mut u, other);
                var(&mut u, i);
            } else {
                u.push(0x07);
                u.push(0);
                var(&mut u, other);
                var(&mut u, i);
            }
            u.extend_from_slice(&[3, 1, b'p']);
        }
    }
    u.push(0);
    u
}

/// Une boucle ne se parcourt qu'une fois : au premier élément revu, le parcours s'arrête. Il allait
/// jusqu'au nombre d'éléments de la mise à jour et de la page, pour chaque boucle (constat C1 de la
/// re-relecture 8, Ruling R854).
#[test]
fn judging_loops_of_neighbours_grows_like_the_update() {
    let forms = [
        Form {
            what: "boucles d'origines entre des lettres",
            build: &loops_of_origins as &dyn Fn(usize) -> Vec<u8>,
        },
        Form {
            what: "boucles d'origines entre des valeurs",
            build: &loops_of_values,
        },
        Form {
            what: "boucles d'origines entre des éléments",
            build: &|n| loops_of_elements(n, true),
        },
        Form {
            what: "boucles de parents entre des éléments",
            build: &|n| loops_of_elements(n, false),
        },
    ];
    let created = PageDoc::new_page().1;
    let judge = |update: &Vec<u8>| {
        let page = PageDoc::from_updates([created.as_slice()]).unwrap();
        let from_client = page.try_client_update(update).is_ok();
        let stored = PageDoc::from_updates([created.as_slice(), update.as_slice()]);
        (from_client, stored.is_ok())
    };
    let limits = Limits {
        peak_per_byte: 256,
        answer: Answer::PerByte(8),
    };
    let failures = grows_like_input(&forms, Vec::len, judge, |_| 0, limits);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// À mise à jour égale, juger des boucles ne dépend pas de ce que la page porte par ailleurs.
#[test]
fn judging_loops_of_neighbours_does_not_grow_with_the_page() {
    let update = loops_of_origins(100);
    let allocated = |letters: usize| {
        let page = PageDoc::from_updates([page_of_letters(letters).0.as_slice()]).unwrap();
        linear::once(|| page.try_client_update(&update).is_ok()).allocated
    };
    let (small, large) = (allocated(1_000), allocated(32_000));
    assert!(
        large < small * 2,
        "une page de 32 000 suites alloue {large} octets, une de 1 000 en alloue {small}"
    );
}

/// Mises à jour d'une rafale.
const BURST: usize = 16;

#[test]
fn a_burst_of_values_placed_after_a_letter_costs_one_lookup_each() {
    let forms = [Form {
        what: "rafale de valeurs placées après la dernière lettre d'un texte",
        build: &burst_after_the_last_letter,
    }];
    let size = |(state, updates): &(Vec<u8>, Vec<Vec<u8>>)| {
        state.len() + updates.iter().map(Vec::len).sum::<usize>()
    };
    let judge = |(state, updates): &(Vec<u8>, Vec<Vec<u8>>)| {
        let page = PageDoc::from_updates([state.as_slice()]).unwrap();
        let before = Instant::now();
        let refused = updates
            .iter()
            .filter(|update| matches!(page.try_client_update(update), Err(r) if r.intact))
            .count();
        assert_eq!(
            refused, BURST,
            "chaque mise à jour est refusée avant d'être appliquée"
        );
        before.elapsed()
    };
    // Le pic est celui de la page chargée (un élément de yrs par lettre : 98 octets par octet
    // d'état, relevé), pas celui de la garde ; c'est le temps que ce test borne.
    let limits = Limits {
        peak_per_byte: 128,
        answer: Answer::PerByte(8),
    };
    let failures = grows_like_input(&forms, size, judge, |_| 0, limits);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // La borne absolue : la rafale entière, sur un texte de 160 000 lettres, coûte moins que deux
    // chargements de la page (une recherche par mise à jour, pas soixante-quatre).
    let input = burst_after_the_last_letter(160_000);
    let load = Instant::now();
    let page = PageDoc::from_updates([input.0.as_slice()]).unwrap();
    let load = load.elapsed();
    drop(page);
    let burst = judge(&input);
    assert!(
        burst < load * 2,
        "rafale de {BURST} mises à jour : {burst:?}, pour un chargement de {load:?}"
    );
}

/// Garde des attributs (Rulings R689 à R691) : juger une mise à jour, d'un client comme relue du
/// journal, croît comme elle et comme la page. Trois formes : des blocs nombreux aux attributs
/// fautifs, un attribut écrasé de nombreuses fois, et des valeurs placées par des origines de la
/// page (le plafond des recherches, `MAX_PAGE_LOOKUPS`).
#[test]
fn judging_attributes_grows_like_the_update() {
    let forms = [
        Form {
            what: "blocs nombreux, chacun aux attributs fautifs",
            build: &many_unsound_blocks,
        },
        Form {
            what: "un attribut écrasé de nombreuses fois",
            build: &one_attribute_overwritten,
        },
        Form {
            what: "valeurs placées par des origines de la page",
            build: &values_placed_after_page_items,
        },
        Form {
            what: "un long élément à cheval",
            build: &one_long_item_astride,
        },
    ];
    // Un élément à cheval par client (Ruling R784) : la forme demande autant de clients que
    // d'éléments, et c'est le client qui pèse dans yrs (83 octets de pic par octet d'entrée pour
    // la même mise à jour sans rien à cheval, 106 avec, mesurés). Sa borne est à elle.
    let per_client = [Form {
        what: "éléments à cheval nombreux, un par client",
        build: &many_items_astride,
    }];
    let size = |sent: &Sent| sent.state.len() + sent.update.len();
    // D'un client (refusée, ou acceptée : des valeurs dans un texte), puis relue du journal.
    let judge = |sent: &Sent| {
        let from_client = {
            let page = PageDoc::from_updates([sent.state.as_slice()]).unwrap();
            page.try_client_update(&sent.update).is_ok()
        };
        let stored =
            PageDoc::from_updates([sent.state.as_slice(), sent.update.as_slice()]).unwrap();
        (from_client, stored.encode_state().len())
    };
    let limits = Limits {
        peak_per_byte: 64,
        answer: Answer::PerByte(8),
    };
    let failures = grows_like_input(&forms, size, judge, |_| 0, limits);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let limits = Limits {
        peak_per_byte: 128,
        answer: Answer::PerByte(8),
    };
    // Jugée deux fois : une seule frôle le temps minimal de la mesure de base, qui passait alors une
    // fois sur deux à la taille au-dessus, où le temps par élément monte (19 fois le temps pour 8
    // fois l'entrée, relevé) sans que la garde y soit pour rien : d'un passage à l'autre, ce test
    // ne mesurait pas la même chose.
    let twice = |sent: &Sent| (judge(sent), judge(sent));
    let failures = grows_like_input(&per_client, size, twice, |_| 0, limits);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Ruling R861 : la cascade est jugée jusqu'au bout. Un lecteur sans garde lit l'état servi comme
/// la page.
#[test]
fn a_cascade_of_rewrites_is_judged_to_its_end() {
    let _alone = linear::ALONE.lock().unwrap_or_else(|e| e.into_inner());
    let sent = cascade_of_rewrites(300, true);
    let page = PageDoc::from_updates([sent.state.as_slice()]).unwrap();
    page.try_client_update(&sent.update).expect("acceptée");
    let text = |doc: &Doc| {
        let txn = doc.transact();
        let paragraph = first_paragraph(&txn);
        let Some(XmlOut::Text(text)) = paragraph.get(&txn, 0) else {
            panic!("le texte");
        };
        text.get_string(&txn)
    };
    let bare = client_of(98, &page.encode_state());
    let live = text(page.yrs_doc());
    assert_eq!(text(&bare), live);
    assert_eq!(live, format!("abcdef{}", "x".repeat(300)));
}

/// Règle V (Ruling R856) : un ancêtre se trouve en un nombre de pas logarithmique, quelle que soit la
/// distance. Se compte, sans minutage : remonter la chaîne pas à pas ne se verrait qu'au temps.
#[test]
fn an_ancestor_is_found_in_a_logarithmic_number_of_steps() {
    let _alone = linear::ALONE.lock().unwrap_or_else(|e| e.into_inner());
    for n in [2_000u32, 32_000] {
        let sent = letters_whose_right_is_far_up_the_chain(n as usize);
        let page = PageDoc::from_updates([sent.state.as_slice()]).unwrap();
        let before = PageDoc::ancestor_steps_on_this_thread();
        page.try_client_update(&sent.update).expect("acceptée");
        let steps = PageDoc::ancestor_steps_on_this_thread() - before;
        let queries = u64::from(n / 2);
        let bound = queries * 4 * u64::from(n.ilog2() + 1);
        assert!(steps >= queries, "{steps} pas pour {queries} recherches");
        assert!(
            steps <= bound,
            "{steps} pas pour {queries} recherches, borne {bound}"
        );
    }
}

/// Ruling R876 : vingt mille lettres envoyées une à une, d'un octet comme de trois, sont acceptées
/// pour quelques dizaines de mégaoctets (le yrs d'origine en demandait 290 : `squash_memory.rs`).
#[test]
fn letters_sent_one_by_one_cost_what_they_weigh_whatever_their_width() {
    let _alone = linear::ALONE.lock().unwrap_or_else(|e| e.into_inner());
    let count = 20_000u64;
    for letter in ["x", "€"] {
        let (state, first) = page_of_letters(0);
        let mut update = Vec::new();
        var(&mut update, 1);
        var(&mut update, count);
        var(&mut update, 9);
        var(&mut update, u64::from(first));
        for k in 0..count as u32 {
            if k == 0 {
                update.extend_from_slice(&[0x04, 0, 9]);
                var(&mut update, u64::from(first - 1));
            } else {
                update.extend_from_slice(&[0x84, 9]);
                var(&mut update, u64::from(first + k - 1));
            }
            var(&mut update, letter.len() as u64);
            update.extend_from_slice(letter.as_bytes());
        }
        update.push(0);
        let page = PageDoc::from_updates([state.as_slice()]).unwrap();
        let cost = linear::once(|| {
            page.try_client_update(&update).expect("acceptée");
        });
        println!(
            "MESURE « {letter} », {count} éléments : pic de {} octets",
            cost.peak
        );
        assert!(
            cost.peak < 64 << 20,
            "« {letter} » : pic de {} octets",
            cost.peak
        );
    }
}

/// `n` clients d'un élément chacun, à la file : l'élément de chacun a pour origine droite celui du
/// suivant, et le dernier est posé dans le texte du paragraphe. yrs les intègre en une passe ; la
/// page est acceptée.
fn clients_in_line(n: usize) -> Sent {
    const FIRST_CLIENT: u64 = 1_000;
    let (state, first) = page_of_letters(0);
    let mut update = Vec::new();
    var(&mut update, n as u64);
    for k in 0..n as u64 {
        var(&mut update, 1);
        var(&mut update, FIRST_CLIENT + k);
        var(&mut update, 0);
        if k + 1 == n as u64 {
            update.extend_from_slice(&[0x04, 0, 9]);
            var(&mut update, u64::from(first - 1));
        } else {
            update.push(0x44);
            var(&mut update, FIRST_CLIENT + k + 1);
            var(&mut update, 0);
        }
        update.extend_from_slice(&[1, b'q']);
    }
    update.push(0);
    Sent { state, update }
}

/// Ruling R883 (constat F de la re-relecture 12) : lire ce que yrs a fait d'une mise à jour
/// (`Shape::check_as_written`) croît comme elle, si nombreux qu'en soient les clients. Chaque
/// client de ce que yrs a encodé était cherché dans la liste des clients de la mise à jour reçue :
/// `n` recherches de `n` pas (2,3 s pour 80 000 clients, en release).
#[test]
fn reading_what_yrs_made_grows_like_the_update() {
    let forms = [Form {
        what: "des clients d'un élément chacun, à la file",
        build: &clients_in_line,
    }];
    let size = |sent: &Sent| sent.state.len() + sent.update.len();
    // D'un client, puis relue du journal : acceptée, sans quoi `check_as_written` ne lit rien.
    let judge = |sent: &Sent| {
        let page = PageDoc::from_updates([sent.state.as_slice()]).unwrap();
        page.try_client_update(&sent.update).expect("acceptée");
        let stored =
            PageDoc::from_updates([sent.state.as_slice(), sent.update.as_slice()]).unwrap();
        stored.encode_state().len()
    };
    // Un client d'un seul élément pèse dans yrs (sa liste de blocs, son entrée du vecteur d'état)
    // et dans la garde : de 195 à 215 octets de pic par octet reçu, mesurés.
    let limits = Limits {
        peak_per_byte: 256,
        answer: Answer::PerByte(8),
    };
    let failures = grows_like_input(&forms, size, judge, |_| 0, limits);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Ruling R885 : savoir si une entrée du journal dépend de ce que la page n'a pas (ce que la
/// relecture demande après une entrée écartée, Ruling R787) croît comme elle, si nombreux qu'en
/// soient les clients. Chaque client y relevait ses plages en parcourant tous ceux de l'entrée.
#[test]
fn knowing_what_an_entry_lacks_grows_like_it() {
    let forms = [Form {
        what: "des clients d'un élément chacun, à la file",
        build: &clients_in_line,
    }];
    let size = |sent: &Sent| sent.state.len() + sent.update.len();
    let judge = |sent: &Sent| {
        let page = PageDoc::from_updates([sent.state.as_slice()]).unwrap();
        let lacks = page.stored_update_lacks_dependency(&sent.update);
        assert!(matches!(lacks, Ok(false)), "{lacks:?}");
    };
    let limits = Limits {
        peak_per_byte: 256,
        answer: Answer::PerByte(8),
    };
    let failures = grows_like_input(&forms, size, judge, |_| 0, limits);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Ce que la garde ne borne pas.
mod known_limits {
    use super::linear::once;
    use super::*;
    use yrs::updates::decoder::Decode;
    use yrs::{OffsetKind, Options, Update};

    /// `n / 2` éléments dont l'origine est une unité d'une longue chaîne où deux clients se relaient.
    fn letters_inside_a_long_chain(n: usize, with_right: bool) -> Sent {
        let (state, unit, chain) = long_chain(n, false);
        let mut update = letters_header(chain as usize / 2);
        for k in 0..chain / 2 {
            if with_right {
                push_letter_between(&mut update, unit(2 * k), (7, k));
            } else {
                update.push(0x84);
                var(&mut update, unit(2 * k).0);
                var(&mut update, u64::from(unit(2 * k).1));
                update.extend_from_slice(&[1, b'r']);
            }
        }
        update.push(0);
        Sent { state, update }
    }

    /// Ce qu'alloue yrs seul, sans la garde, pour intégrer la mise à jour.
    fn allocated_by_yrs(sent: &Sent) -> usize {
        let doc = Doc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        doc.transact_mut()
            .apply_update(Update::decode_v1(&sent.state).unwrap())
            .unwrap();
        once(|| {
            doc.transact_mut()
                .apply_update(Update::decode_v1(&sent.update).unwrap())
                .unwrap();
        })
        .allocated
    }

    /// yrs range un élément en parcourant ce qui suit son origine, tant que cela descend d'elle :
    /// dans une chaîne où deux clients se relaient, chaque élément parcourt toute la fin de la
    /// chaîne, avec ou sans origine droite. Sur une chaîne de 32 000 suites (552 Ko), une mise à jour
    /// de 110 Ko tient l'acteur de sa page 49 s, autant que yrs seul : aucune règle de la garde ne
    /// l'ajoute, et aucune ne le borne (Ruling R860). Rouge le jour où ce coût est levé.
    #[test]
    fn yrs_walks_the_end_of_a_long_chain_for_each_item() {
        let _alone = super::linear::ALONE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for with_right in [true, false] {
            let small = allocated_by_yrs(&letters_inside_a_long_chain(500, with_right));
            let large = allocated_by_yrs(&letters_inside_a_long_chain(2_000, with_right));
            assert!(
                large > 10 * small,
                "origine droite {with_right} : {small} puis {large} octets alloués, pour une entrée                  quatre fois plus grande : la limite est levée"
            );
        }
    }
}
