//! Garde de profondeur (Ruling R17) : un arbre Yjs profond ne fait jamais
//! déborder la pile. Les tests du « serveur » tournent sur un fil de 2 Mio,
//! comme un fil tokio, en debug.

use std::sync::Arc;

use mementum_crdt::{CrdtError, FRAGMENT, MAX_YJS_DEPTH, PageDoc};
use mementum_md::{Block, Content, Inline, MAX_DEPTH, Styles};
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, OffsetKind, Options, ReadTxn, StateVector, Transact, TransactionMut, Update, Xml,
    XmlElementPrelim, XmlElementRef, XmlFragment, XmlOut, XmlTextPrelim,
};

/// Plus long que tout ce qu'une pile de 2 Mio supprime sans déborder.
const DEEP: usize = 4_000;

fn on_small_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(test)
        .unwrap()
        .join()
        .expect("le fil de 2 Mio a survécu");
}

/// Le client, lui, peut avoir besoin d'une grosse pile (yrs récurse en
/// supprimant ou en libérant un arbre profond).
fn on_big_stack<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(work)
        .unwrap()
        .join()
        .unwrap()
}

fn raw_doc(skip_gc: bool) -> Doc {
    Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        skip_gc,
        ..Options::default()
    })
}

/// `levels` paragraphes imbriqués sous `group`, renvoie le dernier `blockGroup`.
fn nest(
    txn: &mut TransactionMut,
    group: &XmlElementRef,
    levels: usize,
    tag: &str,
) -> XmlElementRef {
    let mut group = group.clone();
    for i in 0..levels {
        let container = group.push_back(txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(txn, "id", format!("{tag}{i}"));
        let paragraph = container.push_back(txn, XmlElementPrelim::empty("paragraph"));
        paragraph.push_back(txn, XmlTextPrelim::new("x"));
        group = container.push_back(txn, XmlElementPrelim::empty("blockGroup"));
    }
    group
}

fn paragraph(children: Vec<Block>) -> Block {
    Block {
        id: String::new(),
        kind: "paragraph".into(),
        props: Default::default(),
        content: Content::Inline(vec![Inline::Text {
            text: "x".into(),
            styles: Styles::default(),
        }]),
        children,
    }
}

/// `levels` paragraphes imbriqués : le texte du plus profond est à la
/// profondeur Yjs `2 × levels + 2`.
fn nested(levels: usize) -> Vec<Block> {
    let mut b = paragraph(vec![]);
    for _ in 1..levels {
        b = paragraph(vec![b]);
    }
    vec![b]
}

/// Le plus de niveaux qu'une page admet.
const AT_BOUND: usize = (MAX_YJS_DEPTH - 2) / 2;

#[test]
fn deep_client_tree_is_refused() {
    let update = on_big_stack(|| {
        let doc = raw_doc(false);
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
        nest(&mut txn, &root, DEEP, "n");
        let update = txn.encode_update_v1();
        drop(txn);
        // Puis le client supprime sa racine : 15 octets environ.
        let mut txn = doc.transact_mut();
        fragment.remove_range(&mut txn, 0, 1);
        (update, txn.encode_update_v1())
    });
    on_small_stack(move || {
        let (page, _) = PageDoc::new_page();
        let before = page.read_blocks();
        assert_eq!(page.apply_update(&update.0), Err(CrdtError::TooDeep));
        assert_eq!(page.read_blocks(), before, "refusée avant d'être appliquée");
        // La suppression qui suit ne vise rien de connu : sans effet.
        let _ = page.apply_update(&update.1);
        assert_eq!(page.read_blocks(), before);
    });
}

#[test]
fn deep_insert_and_delete_in_one_update_is_refused() {
    // Sans ramasse-miettes, la mise à jour garde l'arbre et sa suppression.
    let update = on_big_stack(|| {
        let doc = raw_doc(true);
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        {
            let mut txn = doc.transact_mut();
            let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
            nest(&mut txn, &root, DEEP, "n");
        }
        {
            let mut txn = doc.transact_mut();
            fragment.remove_range(&mut txn, 0, 1);
        }
        let update = doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default());
        // Le document du client se libère sur la grosse pile.
        drop(doc);
        update
    });
    on_small_stack(move || {
        let (page, _) = PageDoc::new_page();
        assert_eq!(page.apply_update(&update), Err(CrdtError::TooDeep));
    });
}

#[test]
fn deep_tree_built_across_updates_is_refused() {
    // Deux mises à jour de 100 niveaux chacune : chacune passe la lecture,
    // leur somme dépasse la borne.
    let (first, second) = on_big_stack(|| {
        let doc = raw_doc(false);
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let (first, deepest) = {
            let mut txn = doc.transact_mut();
            let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
            let deepest = nest(&mut txn, &root, MAX_DEPTH, "a");
            (txn.encode_update_v1(), deepest)
        };
        let second = {
            let mut txn = doc.transact_mut();
            nest(&mut txn, &deepest, MAX_DEPTH, "b");
            txn.encode_update_v1()
        };
        drop(doc);
        (first, second)
    });
    on_small_stack(move || {
        let (page, created) = PageDoc::new_page();
        page.apply_update(&first)
            .expect("100 niveaux : sous la borne");
        // Sans la première, la seconde dépend de l'absent : refusée.
        let (alone, _) = PageDoc::new_page();
        assert!(matches!(
            alone.apply_update(&second),
            Err(CrdtError::Decode(_))
        ));
        assert_eq!(page.apply_update(&second), Err(CrdtError::TooDeep));
        // Le document est à jeter ; il se recharge sans la mise à jour refusée.
        drop(page);
        let reloaded = PageDoc::from_updates([created.as_slice(), first.as_slice()]).unwrap();
        assert!(!reloaded.read_blocks().is_empty());
    });
}

#[test]
fn deeply_nested_value_is_refused() {
    let update = on_big_stack(|| {
        let mut value = Any::Null;
        for _ in 0..10_000 {
            value = Any::Array(Arc::from(vec![value]));
        }
        let doc = raw_doc(false);
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let update = {
            let mut txn = doc.transact_mut();
            let root = fragment.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
            root.insert_attribute(&mut txn, "x", value);
            txn.encode_update_v1()
        };
        drop(doc);
        update
    });
    on_small_stack(move || {
        let (page, _) = PageDoc::new_page();
        assert!(matches!(
            page.apply_update(&update),
            Err(CrdtError::Decode(_))
        ));
    });
}

/// Le bloc de niveau `level` (1 au premier niveau) en suivant le premier
/// enfant ; au-delà de `MAX_DEPTH`, la lecture place les blocs plus profonds
/// à la suite de celui du niveau `MAX_DEPTH`.
fn id_at(blocks: &[Block], level: usize) -> String {
    let mut list = blocks;
    for _ in 1..level.min(MAX_DEPTH) {
        list = &list[0].children;
    }
    list[level.saturating_sub(MAX_DEPTH)].id.clone()
}

#[test]
fn at_the_bound_everything_passes() {
    on_small_stack(|| {
        let (page, _) = PageDoc::new_page();
        page.write_blocks(&nested(AT_BOUND)).expect("à la borne");
        let blocks = page.read_blocks();
        assert!(!blocks.is_empty());

        // Déplacer le sous-arbre profond du niveau 2 en tête : sa suppression
        // parcourt toute la profondeur.
        page.move_block(&id_at(&blocks, 2), None)
            .expect("déplacement");

        // Un client supprime le premier bloc de la page, puis l'arbre profond.
        let state = page.encode_state();
        let deletions = {
            let doc = raw_doc(false);
            let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
            doc.transact_mut()
                .apply_update(Update::decode_v1(&state).unwrap())
                .unwrap();
            let mut txn = doc.transact_mut();
            let Some(XmlOut::Element(root)) = fragment.get(&txn, 0) else {
                panic!("racine attendue")
            };
            let n = root.len(&txn);
            root.remove_range(&mut txn, 0, n);
            txn.encode_update_v1()
        };
        page.apply_update(&deletions).expect("suppression reçue");
        assert!(page.read_blocks().is_empty());
    });
}

#[test]
fn beyond_the_bound_writes_are_refused() {
    on_small_stack(|| {
        let (page, _) = PageDoc::new_page();
        let before = page.read_blocks();
        assert_eq!(
            page.write_blocks(&nested(AT_BOUND + 1)),
            Err(CrdtError::TooDeep)
        );
        assert_eq!(page.read_blocks(), before);

        // Un bloc à deux niveaux, déplacé sous le bloc le plus profond.
        let mut blocks = nested(AT_BOUND);
        blocks.push(paragraph(vec![paragraph(vec![])]));
        page.write_blocks(&blocks).expect("à la borne");
        let read = page.read_blocks();
        let deepest = id_at(&read, AT_BOUND);
        let small = read.last().unwrap().id.clone();
        assert_eq!(
            page.move_block(&small, Some(&deepest)),
            Err(CrdtError::TooDeep)
        );
        assert_eq!(page.read_blocks(), read);
        assert_eq!(
            page.insert_blocks_after(Some(&deepest), &nested(2)),
            Err(CrdtError::TooDeep)
        );
    });
}

#[test]
fn twice_the_bound_in_one_update_is_refused() {
    // Ce que la garde promet pendant l'application (`guard.rs`) : la
    // récursion de yrs reste sous deux fois la borne. Le pire cas : une page
    // à la borne, et une seule mise à jour qui accroche sous son élément le
    // plus profond une chaîne que la lecture admet tout juste
    // (`MAX_YJS_DEPTH` types imbriqués : un `blockGroup`, puis `AT_BOUND`
    // niveaux), et qui supprime la racine. yrs supprime alors un arbre de
    // presque deux fois la borne, avant que la mesure ne refuse la mise à
    // jour. Marge mesurée en debug : ce test passe sur 512 Kio et déborde sur
    // 256 Kio (la relecture finale, sur son propre montage, passait sur 1 Mio
    // et débordait sur 512 Kio).
    on_small_stack(|| {
        let (page, _) = PageDoc::new_page();
        page.write_blocks(&nested(AT_BOUND)).expect("à la borne");
        let state = page.encode_state();
        let update = on_big_stack(move || {
            // Sans ramasse-miettes, la mise à jour garde la chaîne et sa
            // suppression.
            let doc = raw_doc(true);
            let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
            doc.transact_mut()
                .apply_update(Update::decode_v1(&state).unwrap())
                .unwrap();
            let before = doc.transact().state_vector();
            {
                let mut txn = doc.transact_mut();
                let Some(XmlOut::Element(mut group)) = fragment.get(&txn, 0) else {
                    panic!("racine attendue")
                };
                // Le `blockContainer` le plus profond n'a pas de `blockGroup`.
                let deepest = loop {
                    let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
                        panic!("blockContainer attendu")
                    };
                    match container.get(&txn, 1) {
                        Some(XmlOut::Element(next)) => group = next,
                        _ => break container,
                    }
                };
                let chain = deepest.push_back(&mut txn, XmlElementPrelim::empty("blockGroup"));
                nest(&mut txn, &chain, AT_BOUND, "z");
                fragment.remove_range(&mut txn, 0, 1);
            }
            let update = doc.transact().encode_diff_v1(&before);
            // Le document du client se libère sur la grosse pile.
            drop(doc);
            update
        });
        assert_eq!(page.apply_update(&update), Err(CrdtError::TooDeep));
    });
}

#[test]
fn the_page_never_holds_a_deep_tree() {
    // Ce que la garde mesure : un élément au-delà de la borne n'existe pas.
    on_small_stack(|| {
        let (page, _) = PageDoc::new_page();
        page.write_blocks(&nested(AT_BOUND)).unwrap();
        let doc = raw_doc(false);
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        doc.transact_mut()
            .apply_update(Update::decode_v1(&page.encode_state()).unwrap())
            .unwrap();
        let txn = doc.transact();
        let mut deepest = 0;
        let mut stack: Vec<(XmlOut, usize)> = fragment.children(&txn).map(|c| (c, 1)).collect();
        while let Some((node, depth)) = stack.pop() {
            deepest = deepest.max(depth);
            if let XmlOut::Element(e) = node {
                stack.extend(e.children(&txn).map(|c| (c, depth + 1)));
            }
        }
        assert_eq!(deepest, MAX_YJS_DEPTH);
    });
}
