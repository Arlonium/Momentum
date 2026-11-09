//! Révision canonique : vecteur d'état et suppressions, indépendante de
//! l'ordre d'arrivée des mises à jour.

use mementum_crdt::PageDoc;
use mementum_md::{block_text, parse};

fn page(mmd: &str) -> PageDoc {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(mmd).unwrap()).unwrap();
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

    // Trois ordres d'arrivée. Dans le dernier, a2 arrive avant a1, dont il ne
    // dépend pas : comme yrs, la page l'intègre en laissant un trou, que a1
    // comble ensuite.
    let orders = [
        [&state, &a1, &a2, &b1, &b2],
        [&state, &b1, &b2, &a1, &a2],
        [&state, &a2, &b2, &a1, &b1],
    ];
    let [x, y, z] = orders
        .map(|order| PageDoc::from_updates(order.map(|u| u.as_slice())).expect("sans attente"));
    // Sans l'état de départ, dont elle dépend, b2 resterait en attente : refusée.
    let (other, _) = PageDoc::new_page();
    assert!(matches!(
        other.apply_update(&b2),
        Err(mementum_crdt::CrdtError::Decode(_))
    ));

    assert_eq!(x.read_blocks(), y.read_blocks());
    assert_eq!(x.read_blocks(), z.read_blocks());
    assert_shape(&x.revision());
    assert_eq!(x.revision(), y.revision());
    assert_eq!(x.revision(), z.revision());
    assert_ne!(x.revision(), base.revision());
}
