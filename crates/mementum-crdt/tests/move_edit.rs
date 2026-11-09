//! Édition concurrente d'un bloc déplacé. Avec Yjs, un déplacement vaut
//! copie + suppression : A recopie le bloc X ailleurs et supprime l'original,
//! pendant que B change le texte de cet original (voir `CLAUDE.md`,
//! « Déplacements concurrents », et `spike/RESULTS.md`).
//!
//! Perte connue, laissée ouverte au §11 de la conception : l'édition de B
//! disparaît dans les deux ordres d'arrivée. Le report par le serveur, à
//! partir de l'original gardé par `skip_gc`, a été essayé et écarté au lot 1
//! (Task 10) ; piste : déplacements interceptés côté client et faits par le
//! serveur, au lot 3.

use mementum_crdt::PageDoc;
use mementum_md::{block_text, parse};

/// Ce que l'acteur de `mementum-sync` fait d'une mise à jour d'un client :
/// l'appliquer, puis dédoublonner (le correctif s'applique au document du
/// serveur et part chez tous).
fn receive(server: &PageDoc, update: &[u8]) {
    server.apply_update(update).expect("mise à jour acceptée");
    server.dedupe_blocks();
}

/// Les blocs de la page : (id, texte), dans l'ordre du document.
fn blocks(page: &PageDoc) -> Vec<(String, String)> {
    page.read_blocks()
        .iter()
        .map(|b| (b.id.clone(), block_text(b)))
        .collect()
}

#[test]
#[ignore = "perte connue : édition concurrente d'un bloc déplacé (RESULTS P0)"]
fn edit_during_concurrent_move_survives() {
    let (base, _) = PageDoc::new_page();
    base.write_blocks(&parse("Bloc X\n\nBloc Y\n\nBloc Z\n").unwrap())
        .unwrap();
    let state = base.encode_state();
    let ids: Vec<String> = base.read_blocks().into_iter().map(|b| b.id).collect();
    let (x, y, z) = (&ids[0], &ids[1], &ids[2]);

    // Depuis le même état : A déplace X après Z, B change le texte de X.
    let a = PageDoc::from_updates([state.as_slice()]).unwrap();
    let b = PageDoc::from_updates([state.as_slice()]).unwrap();
    let moved = a.move_block(x, Some(z)).unwrap();
    let edited = b.replace_text(x, 6, 0, " modifié par B").unwrap();

    // Un seul X, à la place choisie par A, avec l'édition de B.
    let expected = vec![
        (y.clone(), "Bloc Y".to_string()),
        (z.clone(), "Bloc Z".to_string()),
        (x.clone(), "Bloc X modifié par B".to_string()),
    ];
    // Sur un troisième document, dans les deux ordres d'arrivée.
    let orders = [
        ("déplacement puis édition", [&moved, &edited]),
        ("édition puis déplacement", [&edited, &moved]),
    ];
    let got = orders.map(|(name, order)| {
        let server = PageDoc::from_updates([state.as_slice()]).unwrap();
        for update in order {
            receive(&server, update);
        }
        (name, blocks(&server))
    });
    assert_eq!(got, orders.map(|(name, _)| (name, expected.clone())));
}
