//! Déplacements concurrents d'un même bloc, puis dédoublonnage.

use mementum_crdt::PageDoc;
use mementum_md::{RenderOpts, parse, render};

fn mmd(page: &PageDoc) -> String {
    render(&page.read_blocks(), RenderOpts::default()).markdown
}

fn count(page: &PageDoc, id: &str) -> usize {
    page.read_blocks().iter().filter(|b| b.id == id).count()
}

#[test]
fn concurrent_moves_then_dedupe() {
    let (base, _) = PageDoc::new_page();
    base.write_blocks(&parse("A\n\nB\n\nC\n\nD\n").unwrap())
        .unwrap();
    let state = base.encode_state();
    let a = PageDoc::from_updates([state.as_slice()]).unwrap();
    let b = PageDoc::from_updates([state.as_slice()]).unwrap();
    let ids: Vec<String> = a.read_blocks().into_iter().map(|b| b.id).collect();

    // Le même bloc A, déplacé à deux endroits différents.
    let from_a = a.move_block(&ids[0], Some(&ids[2])).unwrap();
    let from_b = b.move_block(&ids[0], Some(&ids[3])).unwrap();
    a.apply_update(&from_b).unwrap();
    b.apply_update(&from_a).unwrap();
    assert_eq!(count(&a, &ids[0]), 2, "un déplacement Yjs copie le bloc");
    assert_eq!(mmd(&a), mmd(&b));

    let fix = a.dedupe_blocks().expect("un doublon à retirer");
    b.apply_update(&fix).unwrap();
    assert_eq!(count(&a, &ids[0]), 1);
    assert_eq!(count(&b, &ids[0]), 1);
    assert_eq!(a.read_blocks(), b.read_blocks());
    assert_eq!(mmd(&a), mmd(&b));
    assert_eq!(a.dedupe_blocks(), None);
    assert_eq!(b.dedupe_blocks(), None);
}

#[test]
fn dedupe_without_duplicate_is_none() {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse("A\n\n- B\n  - C\n").unwrap())
        .unwrap();
    assert_eq!(page.dedupe_blocks(), None);
}
