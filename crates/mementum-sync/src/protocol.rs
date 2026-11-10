//! Protocole de y-websocket : découpe des messages des clients, encodage des
//! messages du serveur.
//!
//! Un message WebSocket porte un ou plusieurs messages y-websocket, écrits en
//! lib0 : `varUint(type)`, puis le contenu.
//! - `0`, synchro : `varUint(étape)` (0 = étape 1, 1 = étape 2, 2 = mise à
//!   jour), puis `varUint8Array` (vecteur d'état ou mise à jour Yjs v1) ;
//! - `1`, awareness : `varUint8Array` (mise à jour d'awareness) ;
//! - `2`, authentification : ignorée ;
//! - `3`, demande d'awareness : sans contenu ;
//! - tout autre type : `varUint8Array`, ignoré (comme yrs).
//!
//! Ce qui sera confié à yrs est vérifié avant : un vecteur d'état ou une
//! awareness qui annonce plus d'entrées que ses octets n'en contiennent, un
//! identifiant de client hors de 53 bits (yrs ne le refuse que par un
//! `debug_assert!`) ou un état d'awareness qui n'est pas du JSON (les
//! navigateurs le relisent par `JSON.parse`) rendent le message illisible.
//! Les mises à jour Yjs, elles, sont lues par `PageDoc::apply_client_update`,
//! qui refuse aussi celles qui touchent aux métadonnées de la page.

use std::collections::HashMap;
use std::fmt;

use yrs::ClientID;
use yrs::encoding::read::{Cursor, Read};
use yrs::encoding::write::Write;
use yrs::sync::AwarenessUpdate;
use yrs::sync::awareness::AwarenessUpdateEntry;

const MSG_SYNC: u8 = 0;
const MSG_AWARENESS: u8 = 1;
const MSG_AUTH: u8 = 2;
const MSG_QUERY_AWARENESS: u8 = 3;

const SYNC_STEP_1: u8 = 0;
const SYNC_STEP_2: u8 = 1;
const SYNC_UPDATE: u8 = 2;

/// Refus d'accès d'un message d'authentification (suivi de sa raison).
const PERMISSION_DENIED: u8 = 0;

/// Plus grand identifiant de client Yjs (53 bits, sûr en JavaScript).
const MAX_CLIENT: u64 = (1 << 53) - 1;

/// Mise à jour Yjs vide (aucun bloc, aucune suppression) : la réponse d'un
/// document vide à une étape 1. Ni appliquée, ni écrite, ni diffusée.
pub(crate) const EMPTY_UPDATE: [u8; 2] = [0, 0];

/// Message d'un client, découpé et vérifié.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClientMessage {
    /// Étape 1 : le vecteur d'état du client (v1), à qui répondre par ce qui
    /// lui manque.
    SyncStep1(Vec<u8>),
    /// Étape 2 ou mise à jour : une mise à jour Yjs (v1), pas encore lue.
    Update(Vec<u8>),
    /// Awareness : la mise à jour lue, et ses octets tels quels pour la relayer.
    Awareness {
        update: AwarenessUpdate,
        raw: Vec<u8>,
    },
    /// Demande de tous les états d'awareness.
    AwarenessQuery,
}

/// Message illisible : la connexion qui l'a envoyé est fermée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProtocolError;

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "message y-websocket illisible")
    }
}

impl From<yrs::encoding::read::Error> for ProtocolError {
    fn from(_: yrs::encoding::read::Error) -> Self {
        ProtocolError
    }
}

/// Découpe un message WebSocket en messages y-websocket.
pub(crate) fn decode(data: &[u8]) -> Result<Vec<ClientMessage>, ProtocolError> {
    let mut cursor = Cursor::new(data);
    let mut messages = Vec::new();
    while cursor.has_content() {
        let kind: u8 = cursor.read_var()?;
        match kind {
            MSG_SYNC => {
                let step: u8 = cursor.read_var()?;
                let payload = cursor.read_buf()?;
                messages.push(match step {
                    SYNC_STEP_1 => {
                        check_state_vector(payload)?;
                        ClientMessage::SyncStep1(payload.to_vec())
                    }
                    SYNC_STEP_2 | SYNC_UPDATE => ClientMessage::Update(payload.to_vec()),
                    _ => return Err(ProtocolError),
                });
            }
            MSG_AWARENESS => {
                let payload = cursor.read_buf()?;
                messages.push(ClientMessage::Awareness {
                    update: decode_awareness(payload)?,
                    raw: payload.to_vec(),
                });
            }
            MSG_AUTH => {
                if cursor.read_var::<u8>()? == PERMISSION_DENIED {
                    cursor.read_string()?;
                }
            }
            MSG_QUERY_AWARENESS => messages.push(ClientMessage::AwarenessQuery),
            _ => {
                cursor.read_buf()?;
            }
        }
    }
    Ok(messages)
}

/// Vecteur d'état v1 : `varUint(n)`, puis `n` fois `varUint(client)
/// varUint(horloge)`.
fn check_state_vector(payload: &[u8]) -> Result<(), ProtocolError> {
    let mut cursor = Cursor::new(payload);
    let entries: u32 = cursor.read_var()?;
    // Une entrée tient sur 2 octets au moins.
    if entries as usize > payload.len() / 2 {
        return Err(ProtocolError);
    }
    for _ in 0..entries {
        read_client(&mut cursor)?;
        cursor.read_var::<u32>()?;
    }
    Ok(())
}

/// Mise à jour d'awareness : `varUint(n)`, puis `n` fois `varUint(client)
/// varUint(horloge) varString(état JSON)`.
fn decode_awareness(payload: &[u8]) -> Result<AwarenessUpdate, ProtocolError> {
    let mut cursor = Cursor::new(payload);
    let entries: u32 = cursor.read_var()?;
    // Une entrée tient sur 3 octets au moins.
    if entries as usize > payload.len() / 3 {
        return Err(ProtocolError);
    }
    let mut clients = HashMap::new();
    for _ in 0..entries {
        let client = read_client(&mut cursor)?;
        let clock: u32 = cursor.read_var()?;
        let json = cursor.read_string()?;
        serde_json::from_str::<serde_json::Value>(json).map_err(|_| ProtocolError)?;
        clients.insert(
            client,
            AwarenessUpdateEntry {
                clock,
                json: json.into(),
            },
        );
    }
    Ok(AwarenessUpdate { clients })
}

fn read_client(cursor: &mut Cursor<'_>) -> Result<ClientID, ProtocolError> {
    let client: u64 = cursor.read_var()?;
    if client > MAX_CLIENT {
        return Err(ProtocolError);
    }
    Ok(ClientID::new(client))
}

/// Étape 1 du serveur : son vecteur d'état (v1).
pub(crate) fn sync_step1(state_vector: &[u8]) -> Vec<u8> {
    sync(SYNC_STEP_1, state_vector)
}

/// Étape 2 : ce qui manque au client (mise à jour v1).
pub(crate) fn sync_step2(update: &[u8]) -> Vec<u8> {
    sync(SYNC_STEP_2, update)
}

/// Mise à jour à diffuser (v1).
pub(crate) fn update(update: &[u8]) -> Vec<u8> {
    sync(SYNC_UPDATE, update)
}

/// Mise à jour d'awareness (v1).
pub(crate) fn awareness(update: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(update.len() + 6);
    out.write_var(MSG_AWARENESS);
    out.write_buf(update);
    out
}

fn sync(step: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 7);
    out.write_var(MSG_SYNC);
    out.write_var(step);
    out.write_buf(payload);
    out
}

#[cfg(test)]
mod tests {
    use yrs::sync::{Awareness, Message, MessageReader, SyncMessage};
    use yrs::updates::decoder::{Decode, DecoderV1};
    use yrs::updates::encoder::Encode;
    use yrs::{Doc, GetString, ReadTxn, StateVector, Text, Transact};

    use super::*;

    fn sample() -> (Doc, Vec<u8>, StateVector) {
        let doc = Doc::with_client_id(7);
        let text = doc.get_or_insert_text("t");
        text.insert(&mut doc.transact_mut(), 0, "Café 🚀");
        let update = doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default());
        let sv = doc.transact().state_vector();
        (doc, update, sv)
    }

    fn awareness_update() -> AwarenessUpdate {
        let mut awareness = Awareness::new(Doc::with_client_id(42));
        awareness.set_local_state_raw(r#"{"user":"A"}"#);
        awareness.update().unwrap()
    }

    #[test]
    fn reads_what_yrs_writes() {
        let (_, update, sv) = sample();
        let aw = awareness_update();
        let mut data = Vec::new();
        for m in [
            Message::Sync(SyncMessage::SyncStep1(sv.clone())),
            Message::Sync(SyncMessage::SyncStep2(update.clone())),
            Message::Sync(SyncMessage::Update(update.clone())),
            Message::Awareness(aw.clone()),
            Message::AwarenessQuery,
            Message::Auth(Some("refusé".into())),
            Message::Auth(None),
            Message::Custom(9, vec![1, 2, 3]),
        ] {
            data.extend(m.encode_v1());
        }
        assert_eq!(
            decode(&data).unwrap(),
            vec![
                ClientMessage::SyncStep1(sv.encode_v1()),
                ClientMessage::Update(update.clone()),
                ClientMessage::Update(update),
                ClientMessage::Awareness {
                    update: aw.clone(),
                    raw: aw.encode_v1(),
                },
                ClientMessage::AwarenessQuery,
            ]
        );
    }

    #[test]
    fn yrs_reads_what_we_write() {
        let (doc, update, sv) = sample();
        let aw = awareness_update();
        let mut data = sync_step1(&sv.encode_v1());
        data.extend(sync_step2(&update));
        data.extend(self::update(&update));
        data.extend(awareness(&aw.encode_v1()));
        let mut decoder = DecoderV1::from(data.as_slice());
        let read: Vec<Message> = MessageReader::new(&mut decoder)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            read,
            vec![
                Message::Sync(SyncMessage::SyncStep1(sv)),
                Message::Sync(SyncMessage::SyncStep2(update.clone())),
                Message::Sync(SyncMessage::Update(update.clone())),
                Message::Awareness(aw),
            ]
        );
        // La mise à jour transportée se relit.
        let other = Doc::new();
        other
            .transact_mut()
            .apply_update(yrs::Update::decode_v1(&update).unwrap())
            .unwrap();
        let text = other.get_or_insert_text("t");
        assert_eq!(
            text.get_string(&other.transact()),
            doc.get_or_insert_text("t").get_string(&doc.transact())
        );
    }

    #[test]
    fn refuses_malformed_messages() {
        let huge_client = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F];
        let mut sv_huge_client = vec![0, 0, 11, 1];
        sv_huge_client.extend(huge_client);
        sv_huge_client.push(1);
        let mut aw_huge_client = vec![1, 14, 1];
        aw_huge_client.extend(huge_client);
        aw_huge_client.extend([1, 2, b'{', b'}']);
        let not_json = awareness(&{
            let mut p = vec![1, 42, 1];
            p.write_string("{pas du json");
            p
        });
        for (what, data) in [
            (
                "vecteur d'état géant",
                vec![0, 0, 5, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F],
            ),
            ("client hors de 53 bits", sv_huge_client),
            ("étape inconnue", vec![0, 9, 0]),
            ("longueur au-delà du message", vec![0, 2, 50, 1, 2]),
            ("awareness : client hors de 53 bits", aw_huge_client),
            ("awareness géante", vec![1, 3, 0xFF, 0xFF, 0x0F]),
            ("awareness hors JSON", not_json),
            ("type tronqué", vec![0x80]),
        ] {
            assert_eq!(decode(&data), Err(ProtocolError), "{what}");
        }
    }

    #[test]
    fn empty_update_is_what_an_empty_doc_answers() {
        let doc = Doc::new();
        let answer = doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default());
        assert_eq!(answer, EMPTY_UPDATE);
    }
}
