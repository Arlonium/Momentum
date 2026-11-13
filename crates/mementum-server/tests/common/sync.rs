//! Aides des tests qui ouvrent la WebSocket de synchro d'une page : ce qu'un client tape et
//! reçoit, si sa connexion est encore servie ou fermée, et l'équipe dont ces tests posent les
//! accès par le store.
//!
//! L'URL, la connexion avec une session (`sync_url`, `sync_as`, `synced_as`) et la poignée de
//! main nue (`handshake`) sont dans `common/mod.rs`.

use std::sync::atomic::{AtomicU32, Ordering};

use mementum_acl::{Level, Subject};
use mementum_server::SignupMode;
use mementum_sync::testkit::Client;

use super::{Account, Browser, TestApp, WAIT, synced_as};

/// Une application où tout le monde peut s'inscrire : les comptes de ces tests reçoivent leur
/// rôle par le store.
pub async fn open_app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

/// L'accès de tous les membres de l'espace, à ce niveau.
pub fn members(level: Level) -> (Subject, Level) {
    (Subject::Members, level)
}

/// L'accès de ce compte, à ce niveau.
pub fn user(account: &Account<'_>, level: Level) -> (Subject, Level) {
    (Subject::User(account.id), level)
}

/// Le texte de chaque bloc de premier niveau du document du client, une ligne par bloc.
pub fn text(client: &Client) -> String {
    client
        .blocks()
        .iter()
        .map(mementum_md::block_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tape `typed` à la fin du premier bloc, puis attend que le serveur ait traité tout ce que
/// la connexion a envoyé : il traite ses messages dans l'ordre, et répond ensuite à la
/// resynchronisation.
pub async fn types(client: &Client, typed: &str) {
    let block = client.blocks()[0].id.clone();
    let start = client.step2_count();
    client.type_text(&block, typed);
    client.resync();
    assert!(
        client.wait_until(WAIT, |c| c.step2_count() > start).await,
        "le serveur n'a pas traité « {typed} »"
    );
}

/// Tape `typed` à la fin du premier bloc sans rien attendre : pour une connexion que le
/// serveur vient de fermer, et qui ne le sait peut-être pas encore.
pub fn types_unheard(client: &Client, typed: &str) {
    let block = client.blocks()[0].id.clone();
    client.type_text(&block, typed);
}

/// Le serveur ferme la connexion ([`WAIT`] au plus).
pub async fn closed(client: &Client) -> bool {
    client.wait_until(WAIT, |c| c.is_closed()).await
}

/// La connexion est encore servie : une resynchronisation reçoit sa réponse.
///
/// Ce n'est pas une barrière pour ce que l'acteur diffuse : la réponse passe derrière ce qu'il
/// avait diffusé avant que la connexion reçoive la resynchronisation, mais **devant** ce qu'il
/// diffuse pendant qu'elle attend sa réponse, même demandé avant (une trame de page, que la
/// route demande sans attendre l'acteur). Pour compter ce qu'un client a reçu, ou prouver
/// qu'il n'a rien reçu, passer par un [`Witness`].
pub async fn still_served(client: &Client) -> bool {
    let start = client.step2_count();
    client.resync();
    client.wait_until(WAIT, |c| c.step2_count() > start).await && !client.is_closed()
}

/// Un témoin : une connexion de plus sur une page, d'un compte qui peut y écrire, dont la
/// présence sert de barrière (Ruling R342). La diffusion d'une page est ordonnée, et l'acteur
/// traite ses demandes dans l'ordre : quand un client voit une marque du témoin, tout ce que
/// l'acteur a diffusé pour les demandes arrivées avant elle lui est déjà parvenu. Une demande
/// faite par une route avant de répondre (`Hub::page_event`) est de celles-là pour une marque
/// posée après la réponse.
pub struct Witness {
    client: Client,
    marks: AtomicU32,
}

impl Witness {
    /// Ouvre la page `url` avec la session de `by`, qui doit pouvoir y écrire : la présence
    /// d'une connexion en lecture seule n'est ni gardée ni relayée (Ruling R118).
    pub async fn join(by: &Browser<'_>, url: &str) -> Witness {
        Witness {
            client: synced_as(by, url).await,
            marks: AtomicU32::new(0),
        }
    }

    /// Pose une marque neuve (une couleur jamais annoncée ; le nom, lui, est récrit par le
    /// serveur), puis attend que chacun des `observers` l'ait vue, [`WAIT`] au plus. La borne
    /// peut faire échouer le test à tort sur un poste très chargé ; passée, ce que l'acteur a
    /// diffusé avant la marque est arrivé chez eux, et rien de plus ancien n'arrivera.
    pub async fn seen_by(&self, observers: &[&Client]) {
        let mark = format!("#{:06x}", self.marks.fetch_add(1, Ordering::SeqCst) + 1);
        self.client.set_awareness(&format!(
            r#"{{"user":{{"name":"Témoin","color":"{mark}"}}}}"#
        ));
        let witness = self.client.client_id();
        let marked = |observer: &Client| {
            observer
                .awareness_states()
                .get(&witness)
                .and_then(|state| serde_json::from_str::<serde_json::Value>(state).ok())
                .is_some_and(|state| state["user"]["color"] == mark.as_str())
        };
        for observer in observers {
            assert!(
                observer.wait_until(WAIT, marked).await,
                "la marque {mark} du témoin n'est pas arrivée : connexion fermée, ou présence \
                 non relayée ({:?})",
                observer.awareness_states().get(&witness)
            );
        }
    }
}
