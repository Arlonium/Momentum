//! Édition faite pendant le déplacement d'un bloc (limite R39 du lot 1, que la Task 13 lève,
//! Ruling R138), vue par l'acteur d'une page. Mona déplace le bloc X juste sous le bloc Z, Tom
//! ajoute du texte à X, chacun depuis le même état : les deux mises à jour sont bâties par
//! `PageDoc::move_block` (une copie de X sous Z, puis la suppression de l'original) et par
//! `PageDoc::replace_text`, comme dans `mementum-crdt/tests/move_edit.rs`. Chacune part par la
//! connexion de son auteur, telle qu'un client qui revient en ligne l'envoie, et l'acteur la
//! traite avant la suivante : un test par ordre d'arrivée.
//!
//! Ces tests disent ce qui est voulu, pas ce qui se passe aujourd'hui : X est dans la page une
//! seule fois, juste sous Z, et porte le texte de Tom, chez Mona, chez Tom, dans le document de
//! l'acteur et dans le journal relu. Chacun vérifie d'abord, par le même chemin, que le
//! déplacement seul et l'édition seule font ce qu'ils disent.
//!
//! Ce qu'ils montrent aujourd'hui est au registre du lot 3, sous « Task 12 ».

mod common;

use common::*;
use mementum_md::{Block, block_text};
use mementum_sync::SyncConfig;

/// La page de départ.
const START: &str = "Bloc X\n\nBloc Y\n\nBloc Z\n";

/// Ce que Tom ajoute à la fin du texte de X.
const TYPED: &str = " de Tom";

/// Une mise à jour du scénario.
#[derive(Clone, Copy)]
enum Step {
    /// Mona déplace X juste sous Z.
    Move,
    /// Tom ajoute « de Tom » à la fin du texte de X.
    Edit,
}

/// Ids des blocs X, Y et Z.
struct Ids {
    x: String,
    y: String,
    z: String,
}

/// Une page « Bloc X, Bloc Y, Bloc Z », Mona et Tom connectés, et leurs deux mises à jour,
/// bâties depuis le même état, que personne n'a encore envoyées.
struct Scene {
    f: Fixture,
    mona: Client,
    tom: Client,
    ids: Ids,
    moved: Vec<u8>,
    edited: Vec<u8>,
}

impl Scene {
    async fn new() -> Scene {
        let f = Fixture::new(SyncConfig::default()).await;
        f.write_mmd(START).await;
        let read = f.ids().await;
        let ids = Ids {
            x: read[0].clone(),
            y: read[1].clone(),
            z: read[2].clone(),
        };
        let mona = f.client().await;
        let tom = f.client().await;
        let moved = mona
            .page()
            .unwrap()
            .move_block(&ids.x, Some(&ids.z))
            .unwrap();
        let edited = tom
            .page()
            .unwrap()
            .replace_text(&ids.x, 6, 0, TYPED)
            .unwrap();
        Scene {
            f,
            mona,
            tom,
            ids,
            moved,
            edited,
        }
    }

    /// Envoie `steps` dans l'ordre, chacune par la connexion de son auteur, telle qu'elle a été
    /// bâtie : c'est ce qu'envoie un client qui revient en ligne (sa réponse à l'étape 1 du
    /// serveur ne porte que ce que le serveur n'a pas). L'acteur traite chacune avant la
    /// suivante ; son auteur reçoit en retour l'état du serveur, comme à une reconnexion. Attend
    /// ensuite que Mona et Tom aient tout ce que le serveur a écrit, puis rend ce que voient
    /// Mona, Tom, l'acteur et le journal relu.
    async fn play(&self, steps: &[Step]) -> Vec<(&'static str, Vec<Block>)> {
        for step in steps {
            let (author, update) = match step {
                Step::Move => (&self.mona, &self.moved),
                Step::Edit => (&self.tom, &self.edited),
            };
            author.send_update(update);
            processed(author).await;
        }
        let stored = stored_page(&self.f.store, self.f.page).await;
        let revision = stored.revision();
        for (who, client) in [("Mona", &self.mona), ("Tom", &self.tom)] {
            assert!(
                client
                    .wait_until(WAIT, |c| c.page().is_ok_and(|p| p.revision() == revision))
                    .await,
                "{who} n'a pas reçu tout ce que le serveur a écrit"
            );
        }
        vec![
            ("chez Mona", self.mona.blocks()),
            ("chez Tom", self.tom.blocks()),
            (
                "dans le document de l'acteur",
                self.f.hub.read_blocks(self.f.page).await.unwrap(),
            ),
            ("dans le journal relu", stored.read_blocks()),
        ]
    }

    /// `(id, texte)` attendus, dans l'ordre de la page.
    fn expected(&self, wanted: [(&String, &str); 3]) -> Vec<(String, String)> {
        wanted
            .into_iter()
            .map(|(id, text)| (id.clone(), text.to_string()))
            .collect()
    }
}

/// Les blocs de premier niveau : `(id, texte)`, dans l'ordre de la page.
fn summary(blocks: &[Block]) -> Vec<(String, String)> {
    blocks
        .iter()
        .map(|b| (b.id.clone(), block_text(b)))
        .collect()
}

/// Le déplacement seul, puis l'édition seule, chacun sur une page à lui et par le chemin du
/// scénario : chacun fait ce qu'il dit, partout. Un échec du scénario ne vient donc ni d'une
/// mise à jour mal bâtie ni d'une page qui n'a pas convergé.
async fn controls() {
    let s = Scene::new().await;
    let Ids { x, y, z } = &s.ids;
    let wanted = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X")]);
    for (who, blocks) in s.play(&[Step::Move]).await {
        assert_eq!(summary(&blocks), wanted, "déplacement seul, {who}");
    }

    let s = Scene::new().await;
    let Ids { x, y, z } = &s.ids;
    let wanted = s.expected([(x, "Bloc X de Tom"), (y, "Bloc Y"), (z, "Bloc Z")]);
    for (who, blocks) in s.play(&[Step::Edit]).await {
        assert_eq!(summary(&blocks), wanted, "édition seule, {who}");
    }
}

/// Ce que la Task 13 doit rendre vrai, partout (`seen`) : une exigence par ligne d'échec, toutes
/// relevées avant d'échouer.
fn assert_wanted(s: &Scene, seen: &[(&str, Vec<Block>)]) {
    let Ids { x, y, z } = &s.ids;
    let wanted = s.expected([(y, "Bloc Y"), (z, "Bloc Z"), (x, "Bloc X de Tom")]);
    let mut failures = Vec::new();
    for (who, blocks) in seen {
        let page = summary(blocks);
        let texts: Vec<&str> = page.iter().map(|(_, text)| text.as_str()).collect();
        let xs: Vec<&(String, String)> = page.iter().filter(|(id, _)| id == x).collect();
        if xs.len() != 1 {
            failures.push(format!(
                "{who} : le bloc X est dans la page {} fois, pas une (vu : {texts:?})",
                xs.len()
            ));
        }
        let above = page
            .iter()
            .position(|(id, _)| id == x)
            .and_then(|at| at.checked_sub(1))
            .map(|at| &page[at].0);
        if above != Some(z) {
            failures.push(format!("{who} : X n'est pas juste sous Z (vu : {texts:?})"));
        }
        let x_text = xs.first().map(|(_, text)| text.as_str());
        if x_text != Some("Bloc X de Tom") {
            failures.push(format!(
                "{who} : X ne porte pas le texte de Tom : {x_text:?} (vu : {texts:?})"
            ));
        }
        let elsewhere: Vec<&str> = page
            .iter()
            .filter(|(id, text)| id != x && text.contains(TYPED.trim()))
            .map(|(_, text)| text.as_str())
            .collect();
        if !elsewhere.is_empty() {
            failures.push(format!(
                "{who} : le texte de Tom est dans un autre bloc : {elsewhere:?}"
            ));
        }
        if page != wanted {
            failures.push(format!(
                "{who} : la page n'est pas celle voulue (vu : {page:?}, voulu : {wanted:?})"
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Le déplacement arrive d'abord, l'édition de Tom ensuite : Tom écrivait hors ligne, et revient
/// après que Mona a déplacé le bloc.
#[tokio::test]
#[ignore = "perte connue R39, levée par la Task 13"]
async fn edit_arriving_after_the_move_survives() {
    controls().await;
    let s = Scene::new().await;
    let seen = s.play(&[Step::Move, Step::Edit]).await;
    assert_wanted(&s, &seen);
}

/// L'édition de Tom arrive d'abord, le déplacement ensuite : Mona déplaçait hors ligne, et
/// revient après que Tom a écrit dans le bloc.
#[tokio::test]
#[ignore = "perte connue R39, levée par la Task 13"]
async fn move_arriving_after_the_edit_keeps_the_edit() {
    controls().await;
    let s = Scene::new().await;
    let seen = s.play(&[Step::Edit, Step::Move]).await;
    assert_wanted(&s, &seen);
}
