//! Un dossier temporaire de test, supprimé **après** le test (Ruling R344).
//!
//! `tempfile::TempDir` supprime son dossier quand on le lâche. Sous Windows, on ne supprime pas
//! un fichier ouvert (`os error 32`), et une base SQLite reste ouverte tant que vit le moteur
//! tokio du test : le pool de sqlx garde ses connexions dans une tâche de fond, même une fois
//! tous les `Store` lâchés ; seul `Store::close` les ferme. Le dossier d'un test, lâché à la fin
//! de son corps, donc avant le moteur, ne pouvait pas être supprimé ; `TempDir` n'en dit rien, et
//! chaque test laissait son dossier, avec sa base, dans le dossier temporaire du système : cinq
//! cents dossiers par passage de `cargo test --workspace`, un demi-gigaoctet par passage des
//! mesures (40 Go en une matinée de travail à plusieurs agents, le 2026-11-13).
//!
//! [`TestDir`] essaie comme `TempDir` quand on le lâche, puis laisse le dossier au **fil du
//! test**, qui le supprime en s'arrêtant : après le corps du test et après son moteur
//! (`#[tokio::test]` le lâche avant de rendre la main), quand sqlx a fermé ses connexions.
//! libtest attend la fin du fil de chaque test avant de finir.
//!
//! Ce fichier sert aux tests de `mementum-store`, de `mementum-sync` et de `mementum-server`,
//! qui l'incluent par son chemin : il ne dépend que de `tempfile` et de la bibliothèque standard.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Ce que le fil d'un test attend, en s'arrêtant, que ses dossiers se laissent supprimer : le
/// temps que sqlx ferme ses connexions, sur leurs fils à elles. Passé ce délai, le dossier reste
/// (comme avant), et le test n'en échoue pas.
const PATIENCE: Duration = Duration::from_secs(2);

/// Un dossier temporaire neuf, à soi. À la place de `tempfile::TempDir` dès qu'une base y est
/// ouverte.
pub struct TestDir(PathBuf);

impl TestDir {
    #[allow(clippy::new_without_default)]
    pub fn new() -> TestDir {
        // Les dossiers des tests déjà finis sur ce fil (libtest sur un seul fil) partent d'abord.
        let _ = LEFT.try_with(|left| left.sweep());
        let dir = tempfile::tempdir().expect("un dossier temporaire");
        TestDir(dir.keep())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let path = std::mem::take(&mut self.0);
        // Comme `TempDir` : tout de suite, si rien n'y est ouvert.
        if remove(&path) {
            return;
        }
        // Sinon à l'arrêt du fil. Un fil déjà en train de s'arrêter ne retient plus rien : le
        // dossier reste, comme avant.
        let _ = LEFT.try_with(|left| {
            if let Ok(mut left) = left.0.try_borrow_mut() {
                left.push(path);
            }
        });
    }
}

/// Supprime le dossier `path`. Vrai s'il n'existe plus.
fn remove(path: &Path) -> bool {
    std::fs::remove_dir_all(path).is_ok() || !path.exists()
}

/// Les dossiers que le fil n'a pas encore pu supprimer.
struct Left(RefCell<Vec<PathBuf>>);

impl Left {
    /// Un essai sur chaque dossier en attente.
    fn sweep(&self) -> bool {
        match self.0.try_borrow_mut() {
            Ok(mut left) => {
                left.retain(|path| !remove(path));
                left.is_empty()
            }
            Err(_) => true,
        }
    }
}

impl Drop for Left {
    /// À l'arrêt du fil : jusqu'à ce que tout soit parti, [`PATIENCE`] au plus. Ne panique
    /// jamais : libtest compterait pour un échec un test réussi.
    fn drop(&mut self) {
        let deadline = Instant::now() + PATIENCE;
        while !self.sweep() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

thread_local! {
    static LEFT: Left = const { Left(RefCell::new(Vec::new())) };
}
