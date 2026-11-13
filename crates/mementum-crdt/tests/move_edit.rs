//! Édition concurrente d'un bloc déplacé. Avec Yjs, un déplacement vaut
//! copie + suppression : A recopie le bloc X ailleurs et supprime l'original,
//! pendant que B change le texte de cet original (voir `CLAUDE.md`,
//! « Déplacements concurrents », et `spike/RESULTS.md`).
//!
//! Limite connue (Ruling R39), laissée ouverte au §11 de la conception :
//! l'édition de B disparaît dans les deux ordres d'arrivée. Le report par le
//! serveur, à partir de l'original gardé par `skip_gc`, a été essayé et écarté
//! au lot 1 (Task 10) ; piste : déplacements interceptés côté client et faits
//! par le serveur, au lot 3.
//!
//! Le test dit ce qui est voulu, et affirme la limite (Ruling R340) : exécuté à
//! chaque passage, il passe tant que la perte dite se produit, mot pour mot, et
//! échoue le jour où elle est levée, comme pour toute autre issue. Il n'est pas
//! `#[ignore]`, marque réservée aux mesures.

use mementum_crdt::PageDoc;
use mementum_md::{block_text, parse};

/// L'affirmation d'une limite connue, partagée avec les tests de `mementum-sync`.
#[path = "../../mementum-sync/tests/common/limit.rs"]
mod limit;

/// Ce que l'acteur de `mementum-sync` fait d'une mise à jour d'un client :
/// l'appliquer, puis dédoublonner (le correctif s'applique au document du
/// serveur et part chez tous).
fn receive(server: &PageDoc, update: &[u8]) {
    server.apply_update(update).expect("mise à jour acceptée");
    server.dedupe_blocks();
}

/// Une page : `(id, texte)` de chaque bloc, dans l'ordre du document.
type Page = Vec<(String, String)>;

fn blocks(page: &PageDoc) -> Page {
    page.read_blocks()
        .iter()
        .map(|b| (b.id.clone(), block_text(b)))
        .collect()
}

/// La page d'un serveur parti de `state`, qui reçoit `updates` dans l'ordre.
fn after(state: &[u8], updates: &[&Vec<u8>]) -> Page {
    let server = PageDoc::from_updates([state]).unwrap();
    for update in updates {
        receive(&server, update);
    }
    blocks(&server)
}

/// L'exigence manquée quand, après `order`, la page est `seen` et non `wanted`.
fn unmet(order: &str, seen: &Page, wanted: &Page) -> String {
    format!("{order} : la page n'est pas celle voulue (vu : {seen:?}, voulu : {wanted:?})")
}

/// Les limites connues (Ruling R340) : ce que le test dit voulu n'est pas tenu,
/// et il l'affirme. Le jour où la perte est levée, il échoue : le sortir de ce
/// module, et lui faire affirmer la page voulue.
mod known_limits {
    use super::limit::known_limit;
    use super::*;

    /// Voulu : un seul X, à la place choisie par A, avec l'édition de B, dans les
    /// deux ordres d'arrivée. Aujourd'hui : X est à la place choisie par A, sans
    /// l'édition de B.
    #[test]
    fn edit_during_concurrent_move_survives() {
        let (base, _) = PageDoc::new_page();
        base.write_blocks(&parse("Bloc X\n\nBloc Y\n\nBloc Z\n").unwrap())
            .unwrap();
        let state = base.encode_state();
        let ids: Vec<String> = base.read_blocks().into_iter().map(|b| b.id).collect();
        let (x, y, z) = (&ids[0], &ids[1], &ids[2]);
        let page = |blocks: [(&String, &str); 3]| -> Page {
            blocks
                .into_iter()
                .map(|(id, text)| (id.clone(), text.to_string()))
                .collect()
        };

        // Depuis le même état : A déplace X après Z, B change le texte de X.
        let a = PageDoc::from_updates([state.as_slice()]).unwrap();
        let b = PageDoc::from_updates([state.as_slice()]).unwrap();
        let moved = a.move_block(x, Some(z)).unwrap();
        let edited = b.replace_text(x, 6, 0, " modifié par B").unwrap();

        // Chacune seule fait ce qu'elle dit : la perte ne vient pas d'une mise à
        // jour mal faite.
        assert_eq!(
            after(&state, &[&moved]),
            page([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X")]),
            "déplacement seul"
        );
        assert_eq!(
            after(&state, &[&edited]),
            page([(x, "Bloc X modifié par B"), (y, "Bloc Y"), (z, "Bloc Z")]),
            "édition seule"
        );

        let wanted = page([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X modifié par B")]);
        let lost = page([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X")]);
        // Sur un troisième document, dans les deux ordres d'arrivée.
        let orders = [
            ("déplacement puis édition", [&moved, &edited]),
            ("édition puis déplacement", [&edited, &moved]),
        ];
        let (mut missing, mut loss) = (Vec::new(), Vec::new());
        for (order, updates) in orders {
            let seen = after(&state, &updates);
            if seen != wanted {
                missing.push(unmet(order, &seen, &wanted));
            }
            loss.push(unmet(order, &lost, &wanted));
        }
        known_limit(
            "R39 : l'édition faite dans un bloc qu'un autre copie puis supprime part avec \
             l'original ; à lever par la Task 13b",
            &missing,
            &loss,
        );
    }
}
