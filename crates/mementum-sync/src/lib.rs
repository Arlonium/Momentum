//! Synchronisation en direct des pages de Mementum.
//!
//! Chaque page ouverte a son **acteur** : une tâche tokio qui possède le
//! [`PageDoc`](mementum_crdt::PageDoc) de la page et traite ses commandes une
//! par une (`actor.rs`). Le [`Hub`] tient le registre des acteurs et sert les
//! connexions des navigateurs au protocole de y-websocket (`protocol.rs`).
//!
//! Règles tenues par l'acteur :
//! - **persister puis diffuser** : une mise à jour n'est diffusée qu'une fois
//!   écrite dans SQLite (`append_updates` attendu), et ce qui a été diffusé
//!   n'est donc jamais perdu ; chaque mise à jour écrite porte le compte qui
//!   l'a faite, quand on le connaît ;
//! - **compaction sûre** : l'instantané est pris dans l'acteur, entre deux
//!   commandes, si bien qu'il couvre exactement les mises à jour écrites
//!   jusqu'à son `seq` ; elle a lieu toutes les `snapshot_every` mises à jour,
//!   et dès qu'une grosse suppression est écrite (Ruling R40), pour que le
//!   ramasse-miettes de yrs ne soit pas rejoué à chaque chargement ;
//! - **dédoublonnage** après chaque mise à jour d'un client (déplacements
//!   concurrents) : le correctif est écrit avec la mise à jour, tous deux ou
//!   aucun, puis diffusé à toutes les connexions ;
//! - **lecture seule** d'un client dont le schéma diffère de
//!   [`SCHEMA_VERSION`](mementum_schema::SCHEMA_VERSION), ou qui n'a pas le
//!   droit d'écrire la page ([`ConnInfo`]) : il reçoit tout, ses mises à jour
//!   sont ignorées ;
//! - **droits et sessions** : le hub ne les lit pas. La route qui accepte
//!   une connexion les lui résume ([`ConnInfo`]) : si elle écrit, de quel
//!   compte elle est, et la révision de l'espace lue avant ses droits. Quand
//!   les droits d'une page changent, [`Hub::kick`] ferme ses connexions ;
//!   quand une session se ferme, [`Hub::kick_user`] ferme celles du compte ;
//!   les clients reviennent, et la poignée de main relit tout. Une connexion
//!   inscrite alors que la révision de l'espace a bougé depuis la lecture de
//!   ses droits est fermée aussitôt, sans avoir rien reçu (Ruling R71,
//!   [`Hub::serve_connection`]). Quand la page est supprimée,
//!   [`Hub::forget`] arrête son acteur sans rien écrire ;
//! - une mise à jour refusée par le document n'est ni gardée ni diffusée :
//!   l'acteur recharge son document depuis le store et ferme la connexion
//!   fautive (Rulings R17, R22). C'est aussi le sort de la mise à jour d'un
//!   client qui touche aux métadonnées de la page (`schemaVersion`). Si
//!   c'est le store qui n'a pas pu l'écrire, il recharge de même et ferme
//!   toutes les connexions (Ruling R21) ;
//! - **bornes mémoire** (Ruling R27) : un message reçu ne dépasse pas
//!   `max_message_bytes`, la file d'une connexion ni `max_queue_bytes` ni
//!   `broadcast_capacity` messages, les diffusions sont partagées entre les
//!   files, un état d'awareness ne dépasse pas 64 Kio ;
//! - une page **sans document** reçoit le sien au premier chargement de son
//!   acteur (Ruling R28).
//!
//! La feature `axum` expose `ws::WsBytes`, qui ramène la WebSocket d'axum
//! au flux attendu par [`Hub::serve_connection`].

mod actor;
mod hub;
mod protocol;
#[cfg(feature = "testkit")]
pub mod testkit;
#[cfg(feature = "axum")]
pub mod ws;

use std::fmt;
use std::time::Duration;

use mementum_crdt::CrdtError;
use mementum_store::StoreError;
use yrs::{Doc, OffsetKind, Options};

pub use hub::{ConnInfo, Hub};

/// Réglages de la synchronisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncConfig {
    /// Messages qu'une connexion peut avoir en retard (diffusion de la page,
    /// puis file vers sa socket). Au-delà, elle est fermée : le client se
    /// reconnecte et se resynchronise. Au moins 1.
    pub broadcast_capacity: usize,
    /// Une compaction (instantané, puis purge du journal) toutes les
    /// `snapshot_every` mises à jour écrites. Au moins 1.
    pub snapshot_every: u64,
    /// Une compaction aussi dès que les mises à jour écrites depuis la
    /// dernière ont supprimé au moins autant d'unités d'horloge Yjs (un
    /// caractère, un élément ou un attribut chacune). Au moins 1.
    ///
    /// Ruling R40 : le ramasse-miettes de yrs est rejoué à chaque chargement
    /// tant qu'une suppression est au journal, et une grosse suppression coûte
    /// cher (2 s pour une page de 10 000 blocs réécrite) ; l'instantané garde
    /// l'état déjà nettoyé. Son coût dépend du nombre d'éléments supprimés, que
    /// les unités majorent : 2 000 unités coûtent au plus quelques dizaines de
    /// millisecondes au chargement d'une page de 10 000 blocs.
    pub compact_after_deletions: u64,
    /// L'acteur d'une page s'arrête après ce délai sans connexion ni demande.
    pub idle_close: Duration,
    /// Taille maximale d'un message reçu d'un client, contrôlée avant tout
    /// décodage : au-delà, la connexion est fermée (Ruling R27).
    pub max_message_bytes: usize,
    /// Octets qu'une connexion peut avoir en attente vers sa socket : au-delà,
    /// elle est fermée comme un client en retard (Ruling R27).
    pub max_queue_bytes: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            broadcast_capacity: 1024,
            snapshot_every: 500,
            compact_after_deletions: 2_000,
            idle_close: Duration::from_secs(30),
            max_message_bytes: 16 << 20,
            max_queue_bytes: 64 << 20,
        }
    }
}

/// Erreur de la synchronisation.
#[derive(Debug)]
pub enum SyncError {
    /// La page n'existe pas.
    NotFound,
    /// Erreur du store.
    Store(StoreError),
    /// Le document refuse l'opération (structure, profondeur…).
    Crdt(CrdtError),
    /// L'acteur de la page s'est arrêté avant de répondre (document impossible
    /// à charger), plusieurs fois de suite.
    Closed,
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::NotFound => write!(f, "page introuvable"),
            SyncError::Store(e) => write!(f, "{e}"),
            SyncError::Crdt(e) => write!(f, "{e}"),
            SyncError::Closed => write!(f, "la page n'a pas pu être ouverte"),
        }
    }
}

impl std::error::Error for SyncError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SyncError::Store(e) => Some(e),
            SyncError::Crdt(e) => Some(e),
            _ => None,
        }
    }
}

impl From<StoreError> for SyncError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => SyncError::NotFound,
            e => SyncError::Store(e),
        }
    }
}

impl From<CrdtError> for SyncError {
    fn from(e: CrdtError) -> Self {
        SyncError::Crdt(e)
    }
}

/// Un document yrs dont les positions comptent en UTF-16, comme Yjs dans le
/// navigateur.
pub(crate) fn utf16_doc() -> Doc {
    Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    })
}
