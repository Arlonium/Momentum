//! L'horloge de tokio arrêtée (Ruling R179), pour les tests qui disent « dix minutes plus tard »
//! sans dépendre de la vitesse du poste. Ce fichier ne dépend que de tokio (fonction `test-util`) :
//! les tests d'une autre crate l'incluent par son chemin, comme `TestDir`.
//!
//! Une horloge simplement en pause (`tokio::time::pause`) ne suffit pas : dès que plus aucune tâche
//! n'a de travail, tokio l'avance de lui-même jusqu'au prochain délai, et c'est le cas à chaque
//! lecture ou écriture dans SQLite, faite par un fil à part : les dix minutes de la maintenance du
//! pool de sqlx, les 30 s de son délai d'acquisition ou de `idle_close` passeraient pendant une
//! écriture. Une tâche qui cède la main sans fin garde le moteur occupé, et l'horloge ne bouge plus
//! que par `tokio::time::advance`.
//!
//! En contrepartie, aucun délai de tokio ne finit tout seul : les attentes se bornent en temps réel
//! (`std::time::Instant`), une connexion tient l'acteur d'une page en vie pendant qu'on avance de
//! plus d'`idle_close`, et on n'avance l'horloge qu'une fois le serveur au repos (une acquisition
//! du pool en cours dépasserait son délai).

use tokio::task::JoinHandle;

/// L'horloge de tokio arrêtée, tant que la valeur vit (voir l'en-tête du fichier).
pub struct StoppedClock(JoinHandle<()>);

impl StoppedClock {
    pub fn start() -> StoppedClock {
        tokio::time::pause();
        StoppedClock(tokio::spawn(async {
            loop {
                tokio::task::yield_now().await;
            }
        }))
    }
}

impl Drop for StoppedClock {
    fn drop(&mut self) {
        self.0.abort();
    }
}
