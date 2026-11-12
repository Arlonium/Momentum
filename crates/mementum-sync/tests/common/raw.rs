//! Une connexion par une socket en mémoire, dont les messages sont lus tels que le serveur les
//! écrit, octet par octet, à la manière de lib0.
//!
//! Le client de test lit avec yrs, comme le serveur : il ne voit pas ce qu'un navigateur lirait
//! autrement. Cette connexion-ci rend les octets eux-mêmes.

use futures::{SinkExt, StreamExt};
use mementum_sync::ConnInfo;

use super::{Fixture, MemorySocket, WAIT, memory_socket};

/// Une étape 1 au vecteur d'état vide : le serveur y répond par une étape 2.
pub const STEP_1: [u8; 4] = [0, 0, 1, 0];

/// Lit un entier variable comme lib0 : sans limite de taille (ici, 128 bits).
pub fn read_var(bytes: &[u8], at: &mut usize) -> u128 {
    let (mut value, mut shift) = (0u128, 0u32);
    loop {
        let byte = bytes[*at];
        *at += 1;
        value |= u128::from(byte & 0x7F) << shift;
        if byte < 0x80 {
            return value;
        }
        shift += 7;
    }
}

/// Une connexion par une socket en mémoire : ses messages sont lus tels que le serveur les
/// écrit.
pub struct Raw {
    socket: MemorySocket,
}

impl Raw {
    pub async fn connect(f: &Fixture, conn: ConnInfo) -> Raw {
        let (server_end, socket) = memory_socket(1024);
        tokio::spawn(f.hub.clone().serve_connection(f.page, conn, server_end));
        Raw { socket }
    }

    pub async fn send(&mut self, message: Vec<u8>) {
        self.socket.send(message).await.expect("socket ouverte");
    }

    /// Le prochain message du serveur ; `None` s'il a fermé la connexion.
    pub async fn next(&mut self) -> Option<Vec<u8>> {
        tokio::time::timeout(WAIT, self.socket.next())
            .await
            .expect("un message du serveur, ou la fermeture")
    }

    /// Demande une étape 2 et lit jusqu'à elle : rend les messages de présence reçus avant.
    /// Le serveur met en file ce qu'il diffuse avant de lire le message suivant d'une
    /// connexion : tout ce qu'il a relayé avant cette demande est dans ce qui est rendu.
    /// `None` : il a fermé la connexion.
    pub async fn presence_so_far(&mut self) -> Option<Vec<Vec<u8>>> {
        self.send(STEP_1.to_vec()).await;
        let mut presence = Vec::new();
        loop {
            let message = self.next().await?;
            match message[..] {
                [0, 1, ..] => return Some(presence),
                [1, ..] => presence.push(message),
                _ => {}
            }
        }
    }

    /// Comme [`Raw::presence_so_far`], pour les mises à jour du document : rend celles que le
    /// serveur a relayées avant cette demande, chacune telle qu'un client la reçoit (sans
    /// l'en-tête du message de synchro).
    pub async fn updates_so_far(&mut self) -> Option<Vec<Vec<u8>>> {
        self.send(STEP_1.to_vec()).await;
        let mut updates = Vec::new();
        loop {
            let message = self.next().await?;
            match message[..] {
                [0, 1, ..] => return Some(updates),
                [0, 2, ..] => {
                    let mut at = 2;
                    let length = read_var(&message, &mut at) as usize;
                    assert_eq!(at + length, message.len(), "un seul message y-websocket");
                    updates.push(message[at..].to_vec());
                }
                _ => {}
            }
        }
    }
}
