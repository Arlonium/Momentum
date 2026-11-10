//! Aides des tests qui ouvrent la WebSocket de synchro d'une page : ce qu'un client tape et
//! reçoit, si sa connexion est encore servie ou fermée, et l'équipe dont ces tests posent les
//! accès par le store.
//!
//! L'URL, la connexion avec une session (`sync_url`, `sync_as`, `synced_as`) et la poignée de
//! main nue (`handshake`) sont dans `common/mod.rs`.

use mementum_acl::{Level, Subject};
use mementum_server::SignupMode;
use mementum_sync::testkit::Client;

use super::{Account, TestApp, WAIT};

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
pub async fn still_served(client: &Client) -> bool {
    let start = client.step2_count();
    client.resync();
    client.wait_until(WAIT, |c| c.step2_count() > start).await && !client.is_closed()
}
