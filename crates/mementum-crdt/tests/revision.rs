//! Révision canonique : vecteur d'état et suppressions, indépendante de
//! l'ordre d'arrivée des mises à jour.

use mementum_crdt::PageDoc;
use mementum_md::{block_text, parse};

fn page(mmd: &str) -> PageDoc {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(mmd).unwrap());
    page
}

fn assert_shape(revision: &str) {
    let hex = revision.strip_prefix("r_").expect("préfixe r_");
    assert_eq!(hex.len(), 16, "{revision}");
    assert!(
        hex.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{revision}"
    );
}

#[test]
fn pure_delete_changes_revision() {
    let page = page("Bonjour\n");
    let id = page.read_blocks()[0].id.clone();
    let (sv, before) = (page.state_vector(), page.revision());
    assert_shape(&before);

    page.replace_text(&id, 0, 3, "").expect("suppression");
    assert_eq!(block_text(&page.read_blocks()[0]), "jour");
    assert_eq!(page.state_vector(), sv, "une suppression seule");
    assert_ne!(page.revision(), before);
}

#[test]
fn revision_is_canonical() {
    let base = page("Alpha\n\nBêta\n\nGamma\n");
    let state = base.encode_state();
    let ids: Vec<String> = base.read_blocks().into_iter().map(|b| b.id).collect();

    let a = PageDoc::from_updates([state.as_slice()]).unwrap();
    let b = PageDoc::from_updates([state.as_slice()]).unwrap();
    let a1 = a.replace_text(&ids[0], 1, 2, "🚀").unwrap();
    let a2 = a.move_block(&ids[2], None).unwrap();
    let b1 = b.replace_text(&ids[1], 0, 4, "").unwrap();
    let b2 = b
        .insert_blocks_after(Some(&ids[1]), &parse("Delta\n").unwrap())
        .unwrap();

    let updates = [&state, &a1, &a2, &b1, &b2];
    let x = PageDoc::from_updates(updates.iter().map(|u| u.as_slice())).unwrap();
    let y = PageDoc::from_updates(updates.iter().rev().map(|u| u.as_slice())).unwrap();
    let z = PageDoc::from_updates([&b2, &a1, &state, &b1, &a2].map(|u| u.as_slice())).unwrap();

    assert_eq!(x.read_blocks(), y.read_blocks());
    assert_eq!(x.read_blocks(), z.read_blocks());
    assert_shape(&x.revision());
    assert_eq!(x.revision(), y.revision());
    assert_eq!(x.revision(), z.revision());
    assert_ne!(x.revision(), base.revision());
}
