//! Une limite connue, affirmée par un test qu'on exécute (Ruling R340).
//!
//! `#[ignore]` ne marque que des mesures, à lancer en release. Une limite (le comportement
//! voulu n'est pas tenu) ne s'écrit pas en test ignoré : il échoue par construction dès qu'on
//! lance les tests ignorés (le job `budgets` de la CI les lance tous), et personne ne voit le
//! jour où il se met à passer. Elle s'écrit en test ordinaire, rangé dans un module
//! `known_limits`, qui relève les exigences manquées et les donne à [`known_limit`] avec celles
//! de la perte dite, mot pour mot :
//!
//! - les mêmes : la limite est constatée, le test passe ;
//! - aucune : la limite est levée, le test échoue, pour qu'on retire la marque et qu'on affirme
//!   le comportement voulu ;
//! - d'autres : ce n'est pas la perte dite (un autre défaut, ou la perte a changé de forme), le
//!   test échoue.
//!
//! Les préconditions du test restent des `assert!` ordinaires, faits avant l'appel : une
//! précondition manquée ou une panique font échouer le test, elles ne passent jamais pour la
//! limite. C'est l'équivalent de `knownLimit` côté client
//! (`apps/web/src/editor/gestures.test.ts`), où `test.fails` ne reçoit que la perte dite.
//!
//! Ce fichier sert aussi à `crates/mementum-crdt/tests/move_edit.rs`, qui l'inclut par son
//! chemin : il ne dépend que de la bibliothèque standard.

/// Affirme une limite connue. `reason` : pourquoi le comportement voulu n'est pas tenu, et ce
/// qui doit le lever. `unmet` : les exigences que le test a vues manquer, une ligne chacune.
/// `loss` : celles de la perte dite, dans le même ordre et mot pour mot.
///
/// Panique si rien ne manque (la limite est levée : retirer la marque), si ce qui manque n'est
/// pas la perte dite, ou si la perte dite est vide (ce ne serait pas une limite).
pub fn known_limit(reason: &str, unmet: &[String], loss: &[String]) {
    assert!(
        !loss.is_empty(),
        "limite sans perte dite ({reason}) : un test qui n'attend aucun manque n'affirme pas \
         une limite"
    );
    assert!(
        !unmet.is_empty(),
        "limite levée ({reason}) : tout ce qui est voulu est tenu. Sortir ce test de \
         `known_limits`, lui faire affirmer le comportement voulu, et consigner la levée au \
         registre."
    );
    assert!(
        unmet == loss,
        "pas la perte dite ({reason}) : un autre défaut, ou la perte a changé de forme.\n\
         La perte dite, mot pour mot :\n{}\nCe qui manque :\n{}",
        loss.join("\n"),
        unmet.join("\n")
    );
    println!("limite constatée ({reason})");
}
