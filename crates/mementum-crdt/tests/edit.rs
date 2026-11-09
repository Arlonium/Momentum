//! Éditions d'agent : remplacement de texte compté en caractères de
//! `block_text`, insertion et déplacement de blocs.

use mementum_crdt::{CrdtError, PageDoc};
use mementum_md::{Block, RenderOpts, block_text, parse, render};

/// Page écrite depuis un MMD canonique.
fn page(mmd: &str) -> PageDoc {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(mmd).expect("MMD canonique"))
        .expect("écriture");
    page
}

fn mmd(page: &PageDoc) -> String {
    render(&page.read_blocks(), RenderOpts::default()).markdown
}

fn first(page: &PageDoc) -> Block {
    page.read_blocks().remove(0)
}

/// Position (en caractères de `block_text`) de `needle` dans le bloc.
fn position(block: &Block, needle: &str) -> usize {
    let text = block_text(block);
    let byte = text.find(needle).expect("texte présent");
    text[..byte].chars().count()
}

/// Remplace la première occurrence de `old` dans le premier bloc.
fn replace(page: &PageDoc, old: &str, new: &str) -> Result<Vec<u8>, CrdtError> {
    let block = first(page);
    let start = position(&block, old);
    page.replace_text(&block.id, start, old.chars().count(), new)
}

#[test]
fn replace_after_marks_and_emoji() {
    let page = page("Du **gras** puis 🚀 déjà\n");
    replace(&page, "déjà", "prêt").expect("remplacement");
    assert_eq!(mmd(&page), "Du **gras** puis 🚀 prêt\n");
}

#[test]
fn replace_after_mention() {
    let page =
        page("Voir **ceci** et <mention type=\"user\" id=\"u1\">Lenny</mention> puis la cible\n");
    replace(&page, "cible", "fin").expect("remplacement");
    assert_eq!(
        mmd(&page),
        "Voir **ceci** et <mention type=\"user\" id=\"u1\">Lenny</mention> puis la fin\n"
    );
}

#[test]
fn replace_out_of_range() {
    let page = page("Court\n");
    let block = first(&page);
    let before = mmd(&page);
    assert_eq!(
        page.replace_text(&block.id, 3, 10, "x"),
        Err(CrdtError::OutOfRange {
            block_id: block.id.clone()
        })
    );
    assert_eq!(
        page.replace_text(&block.id, usize::MAX, 2, "x"),
        Err(CrdtError::OutOfRange {
            block_id: block.id.clone()
        })
    );
    assert_eq!(mmd(&page), before, "rien n'a changé");
}

#[test]
fn replace_keeps_marks_of_replaced_text() {
    let page = page("Du **gras** puis\n");
    replace(&page, "gras", "fort").expect("remplacement");
    assert_eq!(mmd(&page), "Du **fort** puis\n");
}

#[test]
fn replace_across_a_mention_and_insert() {
    let page = page("a <mention type=\"user\" id=\"u1\">L</mention> b\n");
    // « ␣￼␣ » : l'espace, la mention et l'espace deviennent « - ».
    let block = first(&page);
    page.replace_text(&block.id, 1, 3, "-")
        .expect("remplacement");
    assert_eq!(mmd(&page), "a-b\n");
    // Insertion pure, en fin de texte.
    let block = first(&page);
    page.replace_text(&block.id, 3, 0, "!").expect("insertion");
    assert_eq!(mmd(&page), "a-b!\n");
}

#[test]
fn replace_unknown_block_is_not_found() {
    let page = page("Texte\n");
    assert_eq!(
        page.replace_text("absent", 0, 0, "x"),
        Err(CrdtError::BlockNotFound("absent".into()))
    );
}

#[test]
fn insert_and_move_blocks() {
    let page = page("A\n\nB\n");
    let ids: Vec<String> = page.read_blocks().into_iter().map(|b| b.id).collect();
    let new = parse("C\n\nD\n").unwrap();
    page.insert_blocks_after(Some(&ids[0]), &new)
        .expect("insertion");
    assert_eq!(mmd(&page), "A\n\nC\n\nD\n\nB\n");
    page.insert_blocks_after(None, &parse("Tête\n").unwrap())
        .expect("insertion en tête");
    assert_eq!(mmd(&page), "Tête\n\nA\n\nC\n\nD\n\nB\n");

    // B en tête, puis A après B : les ids suivent le bloc.
    page.move_block(&ids[1], None).expect("déplacement");
    page.move_block(&ids[0], Some(&ids[1]))
        .expect("déplacement");
    assert_eq!(mmd(&page), "B\n\nA\n\nTête\n\nC\n\nD\n");
    let after: Vec<String> = page.read_blocks().into_iter().map(|b| b.id).collect();
    assert_eq!(&after[..2], &[ids[1].clone(), ids[0].clone()]);

    assert_eq!(
        page.move_block("absent", None),
        Err(CrdtError::BlockNotFound("absent".into()))
    );
}

#[test]
fn moves_and_inserts_keep_blocknote_structure() {
    let page = page(
        "<columns>\n<column>\nG\n</column>\n<column>\nD\n</column>\n</columns>\n\n- A\n  - B\n",
    );
    let blocks = page.read_blocks();
    let (list, item) = (&blocks[0], &blocks[1]);
    let (left, right) = (&list.children[0], &list.children[1]);
    let child = &item.children[0];
    let out = |id: &str| {
        Err(CrdtError::OutOfRange {
            block_id: id.into(),
        })
    };

    let refused = |r: Result<Vec<u8>, CrdtError>| matches!(r, Err(CrdtError::Structure(_)));
    // Un paragraphe ne va pas après une colonne, ni une colonne hors d'une liste.
    assert!(refused(
        page.insert_blocks_after(Some(&left.id), &parse("X\n").unwrap())
    ));
    assert!(refused(page.move_block(&right.id, None)));
    assert!(refused(page.move_block(&item.id, Some(&left.id))));
    // Un bloc ne va pas dans son propre sous-arbre.
    assert_eq!(page.move_block(&item.id, Some(&child.id)), out(&child.id));
    assert_eq!(page.read_blocks(), blocks, "rien n'a changé");

    // Une colonne après une colonne : permis.
    page.move_block(&left.id, Some(&right.id))
        .expect("colonnes échangées");
    assert_eq!(
        mmd(&page),
        "<columns>\n<column>\nD\n</column>\n<column>\nG\n</column>\n</columns>\n\n- A\n  - B\n"
    );
}

#[test]
fn inserted_ids_never_collide() {
    let page = page("A\n");
    let existing = first(&page).id;
    let mut copy = parse("B\n").unwrap();
    copy[0].id = existing.clone();
    page.insert_blocks_after(Some(&existing), &copy)
        .expect("insertion");
    let ids: Vec<String> = page.read_blocks().into_iter().map(|b| b.id).collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], existing);
    assert_ne!(ids[1], existing, "un id déjà pris est remplacé");
    assert_eq!(page.dedupe_blocks(), None);
}

#[test]
fn move_copies_the_subtree_exactly() {
    let page = page(
        "- **A** <span bg=\"yellow\">et</span> <mention type=\"user\" id=\"u1\">L</mention> {align=\"center\"}\n  - B\n    1. C\n\nFin\n",
    );
    let before = page.read_blocks();
    // L'élément de liste (marques, surlignage, mention, props, enfants) après « Fin ».
    page.move_block(&before[0].id, Some(&before[1].id))
        .expect("déplacement");
    let after = page.read_blocks();
    assert_eq!(after, [before[1].clone(), before[0].clone()]);
}

/// Ruling R19 (b) : le contenu d'un bloc de code est du texte brut, à la
/// lecture comme pour compter les positions.
#[test]
fn code_block_is_plain_text() {
    use mementum_crdt::FRAGMENT;
    use mementum_md::{Content, Inline, Styles};
    use yrs::types::Attrs;
    use yrs::{
        Any, Doc, OffsetKind, Options, Text, Transact, Xml, XmlElementPrelim, XmlFragment,
        XmlTextPrelim,
    };
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = {
        let mut txn = doc.transact_mut();
        let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        let container = root.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "code");
        let code = container.push_back(&mut txn, XmlElementPrelim::empty("codeBlock"));
        let text = code.push_back(&mut txn, XmlTextPrelim::new(""));
        let bold = Attrs::from([("bold".into(), Any::Map(Default::default()))]);
        text.insert_with_attributes(&mut txn, 0, "let", bold);
        code.push_back(&mut txn, XmlElementPrelim::empty("hardBreak"));
        let mention = code.push_back(&mut txn, XmlElementPrelim::empty("mention"));
        mention.insert_attribute(&mut txn, "label", "L");
        code.push_back(&mut txn, XmlTextPrelim::new("x"));
        txn.encode_update_v1()
    };
    let page = PageDoc::from_updates([update.as_slice()]).unwrap();
    let plain = |s: &str| {
        Content::Inline(vec![Inline::Text {
            text: s.into(),
            styles: Styles::default(),
        }])
    };
    assert_eq!(first(&page).content, plain("let\nx"));
    // « x » est le 5ᵉ caractère : la mention ne compte pas.
    page.replace_text("code", 4, 1, "y").expect("remplacement");
    assert_eq!(first(&page).content, plain("let\ny"));
}
