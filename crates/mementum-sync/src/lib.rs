//! Synchronisation en direct des pages de Mementum.
//!
//! Chaque page ouverte a son **acteur** : une tâche tokio qui possède le
//! [`PageDoc`](mementum_crdt::PageDoc) de la page et traite ses commandes une
//! par une (`actor.rs`). Le [`Hub`] tient le registre des acteurs et sert les
//! connexions des navigateurs au protocole de y-websocket (`protocol.rs`).
//!
//! Règles tenues par l'acteur :
//! - **persister puis diffuser** : une mise à jour n'est diffusée qu'une fois
//!   écrite dans SQLite (`append_update` attendu), et ce qui a été diffusé
//!   n'est donc jamais perdu ;
//! - **compaction sûre** : l'instantané est pris dans l'acteur, entre deux
//!   commandes, si bien qu'il couvre exactement les mises à jour écrites
//!   jusqu'à son `seq` ;
//! - **dédoublonnage** après chaque mise à jour d'un client (déplacements
//!   concurrents), écrit puis diffusé à toutes les connexions ;
//! - **lecture seule** d'un client dont le schéma diffère de
//!   [`SCHEMA_VERSION`](mementum_schema::SCHEMA_VERSION) : il reçoit tout,
//!   ses mises à jour sont ignorées ;
//! - une mise à jour refusée par le document n'est ni gardée ni diffusée :
//!   l'acteur recharge son document depuis le store et ferme la connexion
//!   fautive (Rulings R17, R22) ; si c'est le store qui n'a pas pu l'écrire, il
//!   recharge de même et ferme toutes les connexions (Ruling R21).

mod actor;
mod hub;
mod protocol;
#[cfg(feature = "testkit")]
pub mod testkit;

use std::fmt;
use std::time::Duration;

use mementum_crdt::CrdtError;
use mementum_store::StoreError;

pub use hub::Hub;

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
    /// L'acteur d'une page s'arrête après ce délai sans connexion ni demande.
    pub idle_close: Duration,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            broadcast_capacity: 1024,
            snapshot_every: 500,
            idle_close: Duration::from_secs(30),
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
