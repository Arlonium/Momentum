//! Aller-retour arbre → Yjs (format BlockNote) → arbre sur des arbres
//! aléatoires (`arb::tree`), et forme Yjs d'un saut de ligne.

use std::collections::BTreeSet;

use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{Block, Cell, Content, Inline, Styles, arb, normalize};
use proptest::prelude::*;
use yrs::any::Number;
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, OffsetKind, Options, Out, Text, Transact, Update, Xml, XmlElementPrelim, XmlFragment,
    XmlOut, XmlTextPrelim,
};

/// L'arbre canonique tel que Yjs le porte : une page vide n'existe pas
/// pour BlockNote, et `write_blocks(&[])` écrit un paragraphe vide, comme
/// `new_page` (Ruling R16). Le reste est déjà dans `normalize` (liens voisins
/// fusionnés, en-têtes dans la grille, code en texte brut).
fn yjs_view(t: &[Block]) -> Vec<Block> {
    if t.is_empty() {
        return vec![Block {
            id: String::new(),
            kind: "paragraph".into(),
            props: Default::default(),
            content: Content::Inline(vec![]),
            children: vec![],
        }];
    }
    t.to_vec()
}

/// Vide les ids de l'arbre et les renvoie dans l'ordre du document.
fn take_ids(blocks: &mut [Block], out: &mut Vec<String>) {
    for b in blocks {
        out.push(std::mem::take(&mut b.id));
        take_ids(&mut b.children, out);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn prop_roundtrip_through_yjs(t in arb::tree()) {
        let (page, _) = PageDoc::new_page();
        page.write_blocks(&t).expect("écriture");
        let back = page.read_blocks();

        // Les mêmes blocs, une fois la page encodée puis relue.
        let state = page.encode_state();
        let decoded = PageDoc::from_updates([state.as_slice()]).expect("état relisible");
        prop_assert_eq!(&decoded.read_blocks(), &back);

        let mut expected = yjs_view(&normalize(t.clone()));
        let mut got = back.clone();
        let (mut want_ids, mut got_ids) = (Vec::new(), Vec::new());
        take_ids(&mut expected, &mut want_ids);
        take_ids(&mut got, &mut got_ids);
        prop_assert_eq!(&got, &expected);

        // Un id donné est gardé à sa première occurrence (ordre du document) ;
        // un id vide ou répété est généré (UUIDv7).
        let mut seen = BTreeSet::new();
        for (want, got) in want_ids.iter().zip(&got_ids) {
            if !want.is_empty() && seen.insert(want.as_str()) {
                prop_assert_eq!(got, want);
            } else {
                let uuid = uuid::Uuid::parse_str(got);
                prop_assert!(
                    uuid.is_ok_and(|u| u.get_version_num() == 7),
                    "id généré attendu, lu « {} »",
                    got
                );
            }
        }
    }
}

/// Une cellule telle qu'un client peut l'écrire dans Yjs, hors de toute
/// grille : `tableHeader` ou `tableCell`, `colspan` et `rowspan` absents ou
/// de 0 à 200, `colwidth` de longueur quelconque (`None` : `undefined`).
#[derive(Debug, Clone)]
struct ClientCell {
    header: bool,
    colspan: Option<i64>,
    rowspan: Option<i64>,
    colwidth: Option<Vec<Option<i64>>>,
}

fn client_span() -> impl Strategy<Value = Option<i64>> {
    prop_oneof![
        3 => Just(None),
        3 => (0i64..=3).prop_map(Some),
        1 => (0i64..=200).prop_map(Some),
    ]
}

fn client_cell() -> impl Strategy<Value = ClientCell> {
    (
        any::<bool>(),
        client_span(),
        client_span(),
        prop::option::of(prop::collection::vec(prop::option::of(50i64..300), 0..4)),
    )
        .prop_map(|(header, colspan, rowspan, colwidth)| ClientCell {
            header,
            colspan,
            rowspan,
            colwidth,
        })
}

/// Un tableau écrit par un client : de 0 à 5 lignes, chacune de 0 à 4
/// cellules ; lignes inégales, fusions qui sortent de la grille ou se
/// recouvrent.
fn client_table() -> impl Strategy<Value = Vec<Vec<ClientCell>>> {
    prop::collection::vec(prop::collection::vec(client_cell(), 0..5), 0..6)
}

/// Texte de la cellule `i` de la ligne `r` : unique dans le tableau.
fn cell_label(r: usize, i: usize) -> String {
    format!("c{r}x{i}")
}

/// La page d'un seul tableau, écrit directement dans Yjs, élément par
/// élément et attribut par attribut, comme le ferait un client.
fn client_table_page(rows: &[Vec<ClientCell>]) -> PageDoc {
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = {
        let mut txn = doc.transact_mut();
        let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        let container = root.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "t");
        let table = container.push_back(&mut txn, XmlElementPrelim::empty("table"));
        for (r, cells) in rows.iter().enumerate() {
            let row = table.push_back(&mut txn, XmlElementPrelim::empty("tableRow"));
            for (i, c) in cells.iter().enumerate() {
                let tag = if c.header { "tableHeader" } else { "tableCell" };
                let cell = row.push_back(&mut txn, XmlElementPrelim::empty(tag));
                for (name, value) in [("colspan", c.colspan), ("rowspan", c.rowspan)] {
                    if let Some(v) = value {
                        cell.insert_attribute(&mut txn, name, Any::Number(Number::Int(v)));
                    }
                }
                if let Some(widths) = &c.colwidth {
                    let values: Vec<Any> = widths
                        .iter()
                        .map(|w| w.map_or(Any::Undefined, |w| Any::Number(Number::Int(w))))
                        .collect();
                    cell.insert_attribute(&mut txn, "colwidth", Any::Array(values.into()));
                }
                let paragraph = cell.push_back(&mut txn, XmlElementPrelim::empty("tableParagraph"));
                paragraph.push_back(&mut txn, XmlTextPrelim::new(cell_label(r, i)));
            }
        }
        txn.encode_update_v1()
    };
    PageDoc::from_updates([update.as_slice()]).expect("mise à jour lisible")
}

/// Texte d'une cellule lue (le générateur n'y écrit qu'un texte sans marque).
fn cell_plain(cell: &Cell) -> String {
    cell.content
        .iter()
        .map(|inline| match inline {
            Inline::Text { text, .. } => text.as_str(),
            _ => "",
        })
        .collect()
}

/// Les blocs sans leurs ids, que le MMD sans ancres ne porte pas.
fn without_ids(blocks: &[Block]) -> Vec<Block> {
    let mut out = blocks.to_vec();
    take_ids(&mut out, &mut Vec::new());
    out
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// Rulings R54 et R137 : quoi qu'un client écrive dans un tableau, la page
    /// le lit en une grille que `parse` accepte, et le MMD rendu redonne
    /// exactement les blocs lus. Aucune cellule n'est perdue ni répétée, le
    /// bourrage ne dépasse pas les cellules lues plus cent (Ruling R147), et
    /// ce MMD, écrit tel quel (`PUT /mmd`), se relit pareil.
    #[test]
    fn any_yjs_table_reads_as_mmd_that_parses(rows in client_table()) {
        let page = client_table_page(&rows);
        let blocks = page.read_blocks();
        let markdown = mementum_md::render(&blocks, Default::default()).markdown;
        let parsed = mementum_md::parse(&markdown);
        prop_assert!(parsed.is_ok(), "{}\nMMD :\n{}", parsed.unwrap_err(), markdown);
        let parsed = parsed.unwrap();
        prop_assert_eq!(&parsed, &without_ids(&blocks), "MMD :\n{}", markdown);

        let Content::Table(table) = &blocks[0].content else {
            panic!("tableau attendu, lu {:?}", blocks[0]);
        };
        let written: Vec<String> = rows
            .iter()
            .enumerate()
            .flat_map(|(r, cells)| (0..cells.len()).map(move |i| cell_label(r, i)))
            .collect();
        let read: Vec<String> = table
            .rows
            .iter()
            .flatten()
            .map(cell_plain)
            .filter(|t| !t.is_empty())
            .collect();
        prop_assert_eq!(read, written, "MMD :\n{}", markdown);
        // Cellules lues : celles du client, plus une par ligne sans cellule
        // (une ligne s'il n'y en a pas) ; le bourrage en ajoute au plus
        // autant, plus cent.
        let real = rows.iter().map(|r| r.len().max(1)).sum::<usize>().max(1);
        let total: usize = table.rows.iter().map(Vec::len).sum();
        prop_assert!(
            total <= 2 * real + 100,
            "{} cellules pour {} lues\nMMD :\n{}", total, real, markdown
        );

        let (fresh, _) = PageDoc::new_page();
        fresh.write_blocks(&parsed).expect("écriture");
        prop_assert_eq!(without_ids(&fresh.read_blocks()), parsed, "MMD :\n{}", markdown);
    }
}

/// Page construite à la main : un tableau d'une ligne de deux cellules
/// `tag`, avec les attributs `table_attrs` sur `table`.
fn one_row_table(tag: &str, table_attrs: &[(&str, i64)]) -> PageDoc {
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = {
        let mut txn = doc.transact_mut();
        let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        let container = root.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "t");
        let table = container.push_back(&mut txn, XmlElementPrelim::empty("table"));
        for (k, v) in table_attrs {
            table.insert_attribute(&mut txn, *k, Any::Number(Number::Int(*v)));
        }
        let row = table.push_back(&mut txn, XmlElementPrelim::empty("tableRow"));
        for _ in 0..2 {
            let cell = row.push_back(&mut txn, XmlElementPrelim::empty(tag));
            cell.push_back(&mut txn, XmlElementPrelim::empty("tableParagraph"));
        }
        txn.encode_update_v1()
    };
    PageDoc::from_updates([update.as_slice()]).unwrap()
}

fn headers(page: &PageDoc) -> (u32, u32) {
    match &page.read_blocks()[0].content {
        Content::Table(t) => (t.header_rows, t.header_cols),
        other => panic!("tableau attendu, trouvé {other:?}"),
    }
}

#[test]
fn table_headers_through_yjs() {
    // Écrit par BlockNote : les en-têtes se lisent sur les cellules `tableHeader`,
    // et un tableau tout en en-têtes vaut (lignes, 0) (Ruling R19).
    assert_eq!(headers(&one_row_table("tableHeader", &[])), (1, 0));
    assert_eq!(headers(&one_row_table("tableCell", &[])), (0, 0));
    // Des attributs `headerRows` / `headerCols` sur `table` (le R15 provisoire,
    // annulé) sont ignorés : seules les cellules comptent.
    let hinted = [("headerRows", 0), ("headerCols", 2)];
    assert_eq!(headers(&one_row_table("tableCell", &hinted)), (0, 0));

    // Écrit par le serveur : les en-têtes reviennent tels quels, sans
    // attribut hors du schéma de BlockNote sur `table`.
    let (page, _) = PageDoc::new_page();
    let table = mementum_md::parse(
        "| a | b |
| --- | --- |
| 1 | 2 |
{header-cols=1}
",
    )
    .unwrap();
    page.write_blocks(&table).unwrap();
    assert_eq!(headers(&page), (1, 1));
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(&page.encode_state()).unwrap())
        .unwrap();
    let txn = doc.transact();
    let table = fragment
        .successors(&txn)
        .find_map(|n| match n {
            XmlOut::Element(e) if e.tag().as_ref() == "table" => Some(e),
            _ => None,
        })
        .unwrap();
    let mut attrs: Vec<String> = table.attributes(&txn).map(|(k, _)| k.to_string()).collect();
    attrs.sort();
    assert_eq!(attrs, ["textColor"]);
}

#[test]
fn hard_break_is_a_yjs_element() {
    let text = |s: &str| Inline::Text {
        text: s.into(),
        styles: Styles::default(),
    };
    let paragraph = Block {
        id: String::new(),
        kind: "paragraph".into(),
        props: Default::default(),
        content: Content::Inline(vec![text("a"), Inline::HardBreak, text("b")]),
        children: vec![],
    };
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&[paragraph]).unwrap();

    // Lecture du document brut, dans un Doc à part.
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let state = page.encode_state();
    doc.transact_mut()
        .apply_update(Update::decode_v1(&state).unwrap())
        .unwrap();
    let txn = doc.transact();

    let element = |node: Option<XmlOut>| match node {
        Some(XmlOut::Element(e)) => e,
        other => panic!("élément attendu, trouvé {other:?}"),
    };
    let group = element(fragment.get(&txn, 0));
    let container = element(group.get(&txn, 0));
    let node = element(container.get(&txn, 0));
    assert_eq!(node.tag().as_ref(), "paragraph");

    let children: Vec<XmlOut> = node.children(&txn).collect();
    assert_eq!(children.len(), 3, "XmlText, hardBreak, XmlText attendus");
    let plain = |node: &XmlOut| match node {
        XmlOut::Text(t) => t
            .diff(&txn, YChange::identity)
            .into_iter()
            .map(|d| match d.insert {
                Out::Any(Any::String(s)) => s.to_string(),
                other => panic!("texte attendu, trouvé {other:?}"),
            })
            .collect::<String>(),
        other => panic!("XmlText attendu, trouvé {other:?}"),
    };
    assert_eq!(plain(&children[0]), "a");
    match &children[1] {
        XmlOut::Element(e) => assert_eq!(e.tag().as_ref(), "hardBreak"),
        other => panic!("élément hardBreak attendu, trouvé {other:?}"),
    }
    assert_eq!(plain(&children[2]), "b");
}

/// `colwidth` de chaque cellule de la page, dans l'ordre du document.
fn colwidths(page: &PageDoc) -> Vec<Option<Vec<Any>>> {
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(&page.encode_state()).unwrap())
        .unwrap();
    let txn = doc.transact();
    fragment
        .successors(&txn)
        .filter_map(|n| match n {
            XmlOut::Element(e) if matches!(e.tag().as_ref(), "tableCell" | "tableHeader") => {
                Some(match e.get_attribute(&txn, "colwidth") {
                    Some(Out::Any(Any::Array(values))) => Some(values.to_vec()),
                    None => None,
                    other => panic!("colwidth inattendu : {other:?}"),
                })
            }
            _ => None,
        })
        .collect()
}

/// Largeurs d'un tableau à cellules fusionnées (relecture de la Task 3, M7) :
/// `colwidth` porte une valeur par colonne de la grille que couvre la
/// cellule, colonnes décalées par un `rowspan` comprises, comme BlockNote.
#[test]
fn merged_cell_widths_follow_the_grid() {
    let mmd = "| A {colspan=2} | B |\n| --- | --- |\n| C {rowspan=2} | D | E |\n| F | G |\n\
               {widths=\"120,,80\"}\n";
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&mementum_md::parse(mmd).unwrap())
        .unwrap();
    let int = |i: i64| Any::Number(Number::Int(i));
    assert_eq!(
        colwidths(&page),
        [
            Some(vec![int(120), Any::Undefined]), // A : colonnes 0 et 1
            Some(vec![int(80)]),                  // B : colonne 2
            Some(vec![int(120)]),                 // C : colonne 0, sur deux lignes
            None,                                 // D : colonne 1
            Some(vec![int(80)]),                  // E : colonne 2
            None,                                 // F : colonne 1, sous C
            Some(vec![int(80)]),                  // G : colonne 2
        ]
    );
    let back = mementum_md::render(&page.read_blocks(), Default::default()).markdown;
    assert_eq!(back, mmd);
}

/// Fusions bornées (Ruling R37) : un `colspan` démesuré est refusé dès le
/// MMD ; au plus haut admis (`MAX_WIDTH_COLUMNS`), la cellule porte une
/// largeur par colonne couverte. Un `colwidth` démesuré écrit par un client
/// Yjs ne fait pas lire plus de `MAX_WIDTH_COLUMNS` largeurs.
#[test]
fn merged_cell_widths_are_bounded() {
    let max = mementum_md::MAX_WIDTH_COLUMNS;
    let table = |colspan: usize| {
        let widths = format!("120{}", ",".repeat(max - 1));
        format!(
            "| A {{colspan={colspan}}} |\n| --- |\n| B {{colspan={colspan}}} |\n\
             {{widths=\"{widths}\"}}\n"
        )
    };
    for colspan in [max + 1, 1_000_000_000] {
        let error = mementum_md::parse(&table(colspan)).unwrap_err();
        assert!(error.message.contains("de 1 à 100"), "{error}");
    }
    let mmd = table(max);
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&mementum_md::parse(&mmd).unwrap())
        .unwrap();
    let written = colwidths(&page);
    assert_eq!(written.len(), 2);
    for cell in &written {
        let values = cell.as_ref().expect("colwidth écrit");
        assert_eq!(values.len(), max);
        assert_eq!(values[0], Any::Number(Number::Int(120)));
        assert!(values[1..].iter().all(|v| *v == Any::Undefined));
    }
    let back = mementum_md::render(&page.read_blocks(), Default::default()).markdown;
    assert_eq!(back, mmd);

    // Un client écrit 100 000 largeurs sur une cellule seule.
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = {
        let mut txn = doc.transact_mut();
        let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        let container = root.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "t");
        let table = container.push_back(&mut txn, XmlElementPrelim::empty("table"));
        let row = table.push_back(&mut txn, XmlElementPrelim::empty("tableRow"));
        let cell = row.push_back(&mut txn, XmlElementPrelim::empty("tableCell"));
        let many: Vec<Any> = (0..100_000).map(|i| Any::Number(Number::Int(i))).collect();
        cell.insert_attribute(&mut txn, "colwidth", Any::Array(many.into()));
        cell.push_back(&mut txn, XmlElementPrelim::empty("tableParagraph"));
        txn.encode_update_v1()
    };
    let page = PageDoc::from_updates([update.as_slice()]).unwrap();
    match &page.read_blocks()[0].content {
        Content::Table(t) => assert_eq!(t.column_widths, [Some(0.0)]),
        other => panic!("tableau attendu, trouvé {other:?}"),
    }
}

/// Décision D4 de la Task 8 : une cellule fusionnée dont aucune colonne n'a
/// de largeur ne reçoit pas d'attribut `colwidth` (BlockNote écrit
/// `[undefined, undefined]`, que prosemirror-tables et BlockNote lisent de la
/// même façon).
#[test]
fn merged_cell_without_width_has_no_colwidth() {
    let (page, _) = PageDoc::new_page();
    let mmd = "| A {colspan=2} | B |\n| --- | --- |\n| C | D | E |\n{widths=\",,80\"}\n";
    page.write_blocks(&mementum_md::parse(mmd).unwrap())
        .unwrap();
    let int = |i: i64| Any::Number(Number::Int(i));
    assert_eq!(
        colwidths(&page),
        [None, Some(vec![int(80)]), None, None, Some(vec![int(80)])]
    );
    let back = mementum_md::render(&page.read_blocks(), Default::default()).markdown;
    assert_eq!(back, mmd);

    // Sans aucune largeur : aucun `colwidth`.
    let (page, _) = PageDoc::new_page();
    let mmd = "| A {colspan=2} | B |\n| --- | --- |\n| C | D | E |\n";
    page.write_blocks(&mementum_md::parse(mmd).unwrap())
        .unwrap();
    assert!(colwidths(&page).iter().all(Option::is_none));
}
