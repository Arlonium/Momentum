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
//! - `100`, trame de page ([`MESSAGE_PAGE_EVENT`]) : le serveur seul en écrit. Reçue d'un
//!   client, elle rend tout le message illisible, et la connexion est fermée ;
//! - tout autre type : `varUint8Array`, ignoré (comme yrs).
//!
//! Le serveur écrit aussi, de lui-même, des trames de page (Ruling R136) : `100`, puis un JSON
//! en `varUint8Array`. Elles disent aux connexions de la page qui l'a ouverte (`roster`), ou
//! qu'il y a du neuf à relire par l'API (`comments`, `versions`).
//!
//! Ce qui sera confié à yrs est vérifié avant : un vecteur d'état ou une
//! awareness qui annonce plus d'entrées que ses octets n'en contiennent, un
//! identifiant de client hors de 53 bits (yrs ne le refuse que par un
//! `debug_assert!`) ou un état d'awareness gardé qui n'est pas de l'UTF-8
//! rendent le message illisible.
//! Les mises à jour Yjs, elles, sont lues par `PageDoc::apply_client_update`,
//! qui refuse aussi celles qui touchent aux métadonnées de la page, et rend
//! ce qu'il a lu, réencodé : c'est cela que l'acteur écrit et relaie (Ruling
//! R130).
//!
//! Un état d'awareness n'est pas lu ici : c'est une tranche d'octets, de la
//! longueur annoncée. Qu'il soit du JSON, et de la forme connue, se juge
//! après, sur un état de 2 Kio au plus (`presence::known_shape`, Ruling
//! R119). Le relire ici, quelle que soit sa taille, coûtait une cinquantaine
//! de fois la taille du message : un état `[0,0,…]` d'un mégaoctet faisait
//! allouer 54 Mio, à une connexion en lecture seule comme à une autre. Un
//! état de plus de 2 Kio est donc sauté sans être lu, et son entrée ignorée ;
//! et d'une connexion en lecture seule, dont rien n'est gardé (Ruling R118),
//! seules les longueurs sont lues.
//!
//! Un message WebSocket qui porte plus de [`MAX_PRESENCE_ENTRIES`] entrées
//! d'awareness n'est pas illisible : ses messages d'awareness sont ignorés,
//! sans qu'aucune de leurs entrées soit confiée à l'acteur, et le reste est
//! rendu comme d'habitude ([`Decoded`], Ruling R113).
//!
//! Une mise à jour d'awareness est lue comme la lisent les clients (Ruling
//! R123). yrs lit un entier variable en laissant tomber les bits qui ne
//! tiennent pas dans son type ; lib0, chez les clients, les garde : les octets
//! `81 80 80 80 10` valent 1 pour l'un et 2^32 + 1 pour l'autre. Lu ainsi, un
//! nombre d'entrées laissait passer, derrière la seule entrée que le serveur
//! regardait, une entrée pour le client de n'importe qui. Les entiers d'une
//! mise à jour d'awareness sont donc lus sans rien laisser tomber
//! ([`read_exact_var`]), et un tampon où il reste des octets après les entrées
//! annoncées est illisible. Et le serveur ne relaie jamais ces octets : il
//! réencode ce qu'il a lu et gardé (`actor.rs`).
//!
//! Un message WebSocket qui porte plus de [`MAX_MESSAGES`] messages
//! y-websocket est illisible (Ruling R124).

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;
use yrs::ClientID;
use yrs::encoding::read::{Cursor, Read};
use yrs::encoding::write::Write;
use yrs::sync::AwarenessUpdate;
use yrs::sync::awareness::AwarenessUpdateEntry;

use crate::PageEvent;
use crate::presence::MAX_PRESENCE_STATE;

const MSG_SYNC: u8 = 0;
const MSG_AWARENESS: u8 = 1;
const MSG_AUTH: u8 = 2;
const MSG_QUERY_AWARENESS: u8 = 3;

/// Type des trames de page (Ruling R136) : ce qui ne doit aller qu'aux personnes qui lisent une
/// page passe par sa connexion de synchro, dans une trame que le serveur écrit seul. `100`, puis
/// un JSON en UTF-8, en `varUint8Array` (sa longueur en entier variable, puis ses octets) :
/// - `{"type":"roster","users":[{"id","name","canWrite"}]}` : qui a la page ouverte, une ligne
///   par compte ;
/// - `{"type":"comments"}`, `{"type":"versions"}` : relire les fils de commentaires, ou
///   l'historique, par l'API.
///
/// Une connexion de synchro n'existe que pour qui peut lire la page : rien de ce qui y passe ne
/// sort vers quelqu'un qui ne la voit pas. Le client retire ces trames avant y-websocket, qui ne
/// les connaît pas (un client plus ancien écrit « Unable to compute message » dans sa console,
/// sans autre effet). Un client n'en envoie jamais : reçue d'un client, la trame rend son message
/// illisible, et sa connexion est fermée ([`decode`]).
pub const MESSAGE_PAGE_EVENT: u8 = 100;

const SYNC_STEP_1: u8 = 0;
const SYNC_STEP_2: u8 = 1;
const SYNC_UPDATE: u8 = 2;

/// Refus d'accès d'un message d'authentification (suivi de sa raison).
const PERMISSION_DENIED: u8 = 0;

/// Plus grand identifiant de client Yjs (53 bits, sûr en JavaScript).
pub(crate) const MAX_CLIENT: u64 = (1 << 53) - 1;

/// Messages y-websocket qu'un message WebSocket peut porter (Ruling R124) :
/// au-delà, il est illisible, et la connexion est fermée.
///
/// Un client y-websocket en envoie un par message WebSocket : son étape 1 et
/// son état de présence à la connexion, chaque mise à jour, chaque changement
/// de présence, et la réponse à un message reçu, dont il ne lit que le premier
/// message y-websocket (`y-websocket.js`, `readMessage`). Rien de ce que le
/// serveur envoie ne peut l'amener à en mettre plusieurs bout à bout.
///
/// Sans borne, un message WebSocket fait d'octets `03` (une demande
/// d'awareness chacun) était découpé en autant de messages : 72 Mio alloués
/// pour 1 Mio reçu, et la borne d'un message est de 16 Mio. Le compte se fait
/// avant de lire le message suivant : rien n'est alloué au-delà de la borne.
pub(crate) const MAX_MESSAGES: usize = 32;

/// Entrées d'awareness qu'un message WebSocket peut porter, tous ses messages
/// d'awareness comptés ensemble (Ruling R113) : au-delà, aucune n'est traitée
/// (ni appliquée, ni relayée, et elle ne donne aucun client). Le reste du
/// message est traité, et la connexion reste ouverte.
///
/// Le traitement coûte « entrées × connexions de la page » : ce qui est gardé
/// part à chaque connexion (et jusqu'au Ruling R136, l'acteur regardait aussi
/// chaque entrée contre chaque connexion, pour savoir qui tenait son client ;
/// il le lit désormais dans une table). Or la borne en octets d'un message
/// ([`SyncConfig::max_message_bytes`](crate::SyncConfig::max_message_bytes))
/// laisse passer des millions d'entrées, qui occuperaient l'acteur aux dépens
/// de toute la page. Un client honnête n'envoie que le sien, ou renvoie ceux
/// des présents.
///
/// Ignorées, et pas refusées : y-websocket renvoie en un seul message tous les
/// états qu'il reçoit d'un coup, et rien ne borne ceux d'une page (huit par
/// connexion, autant de connexions qu'on veut). Fermer la connexion donnait à
/// un lecteur le moyen de fermer celles des autres : il remplit la page de
/// plus de 512 états, et le renvoi de qui arrive dépasse la limite. Ignorer ne
/// coûte rien à un client honnête, qui annonce son propre état dans un message
/// à part.
///
/// Le compte se fait sur le message WebSocket, pas sur chacun de ses messages
/// d'awareness : l'acteur les traite d'une traite, et la limite se
/// contournerait en les mettant bout à bout.
pub(crate) const MAX_PRESENCE_ENTRIES: usize = 512;

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
    /// Awareness : la mise à jour lue. Ses octets ne sont pas gardés : ce qui
    /// est relayé est réencodé d'après elle (Ruling R123).
    Awareness(AwarenessUpdate),
    /// Demande de tous les états d'awareness.
    AwarenessQuery,
}

/// Un message WebSocket découpé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Decoded {
    /// Ses messages y-websocket, dans l'ordre, moins ceux qui sont ignorés.
    pub(crate) messages: Vec<ClientMessage>,
    /// `Some` : le message annonçait ce nombre d'entrées d'awareness, plus que
    /// [`MAX_PRESENCE_ENTRIES`], et aucun de ses messages d'awareness n'est
    /// dans `messages` (Ruling R113).
    pub(crate) presence_dropped: Option<usize>,
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
///
/// `keep_presence` : faux pour une connexion en lecture seule, dont la
/// présence n'est pas gardée (Ruling R118). Ses messages d'awareness sont lus
/// comme les autres (un tampon mal formé reste illisible), mais aucun n'est
/// rendu, et aucun état n'est copié.
pub(crate) fn decode(data: &[u8], keep_presence: bool) -> Result<Decoded, ProtocolError> {
    let mut cursor = Cursor::new(data);
    let mut messages = Vec::new();
    // Entrées d'awareness que le message annonce, tous ses messages
    // d'awareness comptés ensemble (Ruling R113). Chacun en annonce moins
    // qu'il n'a d'octets : la somme ne déborde pas.
    let mut presence_entries = 0usize;
    // Messages y-websocket lus, comptés avant de lire le suivant (Ruling R124).
    let mut read = 0usize;
    while cursor.has_content() {
        read += 1;
        if read > MAX_MESSAGES {
            return Err(ProtocolError);
        }
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
                let (entries, first) = awareness_header(payload)?;
                presence_entries += entries;
                // Au-delà de la limite, les entrées ne sont pas même lues.
                if presence_entries <= MAX_PRESENCE_ENTRIES {
                    let update = awareness_entries(first, entries, keep_presence)?;
                    if keep_presence {
                        messages.push(ClientMessage::Awareness(update));
                    }
                }
            }
            MSG_AUTH => {
                if cursor.read_var::<u8>()? == PERMISSION_DENIED {
                    cursor.read_string()?;
                }
            }
            MSG_QUERY_AWARENESS => messages.push(ClientMessage::AwarenessQuery),
            // Ruling R136 : le serveur seul en écrit. Rien de ce message n'est traité.
            MESSAGE_PAGE_EVENT => return Err(ProtocolError),
            _ => {
                cursor.read_buf()?;
            }
        }
    }
    let presence_dropped = (presence_entries > MAX_PRESENCE_ENTRIES).then(|| {
        // Tous : ceux qui ont été lus avant que la limite soit dépassée aussi.
        messages.retain(|message| !matches!(message, ClientMessage::Awareness(_)));
        presence_entries
    });
    Ok(Decoded {
        messages,
        presence_dropped,
    })
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
/// varUint(horloge) varString(état JSON)`. Rend `n`, et le curseur posé sur la
/// première entrée : aucune n'est lue.
fn awareness_header(payload: &[u8]) -> Result<(usize, Cursor<'_>), ProtocolError> {
    let mut cursor = Cursor::new(payload);
    let entries = read_exact_var(&mut cursor)?;
    // Une entrée tient sur 3 octets au moins.
    if entries > (payload.len() / 3) as u64 {
        return Err(ProtocolError);
    }
    Ok((entries as usize, cursor))
}

/// Un entier variable de lib0 (7 bits par octet, poids faibles d'abord), lu
/// sans rien laisser tomber : s'il ne tient pas dans 64 bits, il est
/// illisible. `Read::read_var` de yrs, lui, tronque (Ruling R123).
fn read_exact_var(cursor: &mut Cursor<'_>) -> Result<u64, ProtocolError> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = cursor.read_u8()?;
        let bits = u64::from(byte & 0x7F);
        if shift >= u64::BITS || (bits << shift) >> shift != bits {
            return Err(ProtocolError);
        }
        value |= bits << shift;
        if byte < 0x80 {
            return Ok(value);
        }
        shift += 7;
    }
}

/// Les `entries` entrées d'une mise à jour d'awareness, que `cursor` va lire
/// ([`awareness_header`]). `keep` : faux, les entrées sont parcourues, et
/// aucune n'est gardée.
///
/// Un état n'est pas lu : c'est une tranche d'octets, de la longueur
/// annoncée. Au-delà de [`MAX_PRESENCE_STATE`], elle est sautée, et l'entrée
/// ignorée ; sinon elle est copiée, et doit être de l'UTF-8.
fn awareness_entries(
    mut cursor: Cursor<'_>,
    entries: usize,
    keep: bool,
) -> Result<AwarenessUpdate, ProtocolError> {
    let mut clients = HashMap::new();
    for _ in 0..entries {
        let client = read_exact_var(&mut cursor)?;
        if client > MAX_CLIENT {
            return Err(ProtocolError);
        }
        let clock = u32::try_from(read_exact_var(&mut cursor)?).map_err(|_| ProtocolError)?;
        let length = usize::try_from(read_exact_var(&mut cursor)?).map_err(|_| ProtocolError)?;
        let state = cursor.read_exact(length)?;
        let client = ClientID::new(client);
        // Un client répété : la dernière entrée fait foi. C'est elle seule qui sera
        // regardée, gardée et relayée ; les clients ne voient jamais l'autre.
        if !keep || length > MAX_PRESENCE_STATE {
            // Sautée sans être lue. Ignorée, elle fait foi quand même : ce que le
            // message disait de ce client avant elle ne vaut plus.
            clients.remove(&client);
            continue;
        }
        let json = std::str::from_utf8(state).map_err(|_| ProtocolError)?;
        clients.insert(
            client,
            AwarenessUpdateEntry {
                clock,
                json: json.into(),
            },
        );
    }
    // Les entrées annoncées doivent épuiser le tampon : ce qui suivrait ne serait lu par
    // personne ici, et un client qui compterait autrement y trouverait d'autres entrées.
    if cursor.has_content() {
        return Err(ProtocolError);
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

/// Une ligne de la trame `roster` : un compte qui a la page ouverte (Ruling R143).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RosterLine {
    /// Le compte.
    pub(crate) id: Uuid,
    /// Son nom, tel que le montre le serveur (`display_name`).
    pub(crate) name: Arc<str>,
    /// L'une de ses connexions au moins peut écrire la page.
    pub(crate) can_write: bool,
}

/// La trame `roster` : qui a la page ouverte, une ligne par compte, dans l'ordre donné. Elle ne
/// porte de chaque compte que son identifiant, son nom et s'il peut écrire : ni son adresse, ni
/// rien d'autre.
pub(crate) fn roster(lines: &[RosterLine]) -> Vec<u8> {
    let users: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| {
            json!({
                "id": line.id.to_string(),
                "name": &*line.name,
                "canWrite": line.can_write,
            })
        })
        .collect();
    page_frame(&json!({ "type": "roster", "users": users }))
}

/// La trame qui dit aux connexions de la page qu'il y a du neuf à relire par l'API.
pub(crate) fn page_event(event: PageEvent) -> Vec<u8> {
    let kind = match event {
        PageEvent::Comments => "comments",
        PageEvent::Versions => "versions",
    };
    page_frame(&json!({ "type": kind }))
}

/// Une trame de page ([`MESSAGE_PAGE_EVENT`]) qui porte `json`.
fn page_frame(json: &serde_json::Value) -> Vec<u8> {
    let json = json.to_string();
    let mut out = Vec::with_capacity(json.len() + 6);
    out.write_var(MESSAGE_PAGE_EVENT);
    out.write_buf(json.as_bytes());
    out
}

/// Message d'entretien (Ruling R107) : une mise à jour d'awareness de zéro
/// entrée. Un client y-websocket la lit sans rien y trouver (ni état ajouté,
/// ni état retiré, aucun événement), mais il a reçu un message : il ne se
/// croit pas seul, et ne ferme pas sa WebSocket.
pub(crate) fn keepalive() -> Vec<u8> {
    // `varUint(0)` : aucune entrée.
    awareness(&[0])
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

    /// La découpe d'un message d'une connexion qui écrit : sa présence est gardée.
    fn decode(data: &[u8]) -> Result<Decoded, ProtocolError> {
        super::decode(data, true)
    }

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

    /// Les messages d'un message WebSocket lisible, dont rien n'est ignoré.
    fn messages(data: &[u8]) -> Vec<ClientMessage> {
        let decoded = decode(data).unwrap();
        assert_eq!(decoded.presence_dropped, None);
        decoded.messages
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
            messages(&data),
            vec![
                ClientMessage::SyncStep1(sv.encode_v1()),
                ClientMessage::Update(update.clone()),
                ClientMessage::Update(update),
                ClientMessage::Awareness(aw.clone()),
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
        let not_utf8 = awareness(&[1, 42, 1, 2, 0xC3, 0x28]);
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
            ("awareness hors UTF-8", not_utf8),
            ("type tronqué", vec![0x80]),
        ] {
            assert_eq!(decode(&data), Err(ProtocolError), "{what}");
        }
    }

    /// Le message d'entretien est ce que yrs écrit pour une awareness sans
    /// aucun état, et se relit en mise à jour de zéro entrée ; ce serveur le
    /// relirait de même (un client qui le renverrait n'est pas fermé).
    #[test]
    fn keepalive_is_an_empty_awareness_update() {
        let empty = AwarenessUpdate {
            clients: HashMap::new(),
        };
        let message = keepalive();
        assert_eq!(message, Message::Awareness(empty.clone()).encode_v1());
        assert_eq!(message, [1, 1, 0]);
        let mut decoder = DecoderV1::from(message.as_slice());
        let read: Vec<Message> = MessageReader::new(&mut decoder)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(read, vec![Message::Awareness(empty.clone())]);
        assert_eq!(messages(&message), vec![ClientMessage::Awareness(empty)]);
    }

    /// Un message d'awareness de `count` entrées, pour les clients `first` et
    /// suivants.
    fn presence_of(first: u64, count: u64) -> Vec<u8> {
        let clients = (first..first + count)
            .map(|client| {
                let entry = AwarenessUpdateEntry {
                    clock: 1,
                    json: "{}".into(),
                };
                (ClientID::new(client), entry)
            })
            .collect();
        Message::Awareness(AwarenessUpdate { clients }).encode_v1()
    }

    /// Ce qu'un message WebSocket lisible donne de sa présence : le nombre
    /// d'entrées de chaque message d'awareness rendu, et celui des entrées
    /// ignorées.
    fn presence(data: &[u8]) -> (Vec<usize>, Option<usize>) {
        let decoded = decode(data).unwrap();
        let kept = decoded
            .messages
            .iter()
            .filter_map(|message| match message {
                ClientMessage::Awareness(update) => Some(update.clients.len()),
                _ => None,
            })
            .collect();
        (kept, decoded.presence_dropped)
    }

    /// Ruling R113 : jusqu'à 512 entrées d'awareness, le message est rendu tel
    /// quel ; à 513, ses messages d'awareness sont ignorés, tous, que les
    /// entrées tiennent en un message d'awareness ou en plusieurs mis bout à
    /// bout.
    #[test]
    fn drops_presence_beyond_the_limit() {
        let max = MAX_PRESENCE_ENTRIES as u64;
        assert_eq!(max, 512);
        assert_eq!(presence(&presence_of(1, max)), (vec![512], None));
        assert_eq!(presence(&presence_of(1, max + 1)), (vec![], Some(513)));
        let mut two = presence_of(1, 256);
        two.extend(presence_of(1_000, 256));
        assert_eq!(presence(&two), (vec![256, 256], None));
        // Le premier aussi, lu quand la limite n'était pas encore dépassée.
        let mut two = presence_of(1, 256);
        two.extend(presence_of(1_000, 257));
        assert_eq!(presence(&two), (vec![], Some(513)));
    }

    /// Seule la présence est ignorée : le reste du message est rendu, dans
    /// l'ordre, qu'il la précède ou la suive. Une mise à jour du document
    /// envoyée avec trop d'entrées de présence n'est pas perdue.
    #[test]
    fn keeps_the_rest_of_a_message_with_too_much_presence() {
        let (_, update, sv) = sample();
        let mut data = Message::Sync(SyncMessage::SyncStep1(sv.clone())).encode_v1();
        data.extend(presence_of(1, 300));
        data.extend(Message::Sync(SyncMessage::Update(update.clone())).encode_v1());
        data.extend(presence_of(1_000, 300));
        data.extend(Message::AwarenessQuery.encode_v1());
        assert_eq!(
            decode(&data),
            Ok(Decoded {
                messages: vec![
                    ClientMessage::SyncStep1(sv.encode_v1()),
                    ClientMessage::Update(update),
                    ClientMessage::AwarenessQuery,
                ],
                presence_dropped: Some(600),
            })
        );
    }

    /// Au-delà de la limite, les entrées ne sont pas lues : ce qu'elles
    /// contiennent n'est pas regardé. Les mêmes octets sous la limite sont
    /// lus, et illisibles. Un nombre d'entrées que les octets ne peuvent pas
    /// contenir reste illisible aussi, si grand soit-il.
    #[test]
    fn does_not_read_the_entries_beyond_the_limit() {
        let garbage = |entries: u32| {
            let mut payload = Vec::new();
            payload.write_var(entries);
            payload.extend([0xFF; 513 * 3]);
            awareness(&payload)
        };
        assert_eq!(
            decode(&garbage(513)),
            Ok(Decoded {
                messages: vec![],
                presence_dropped: Some(513),
            })
        );
        assert_eq!(decode(&garbage(512)), Err(ProtocolError));
        assert_eq!(decode(&[1, 3, 0xFF, 0xFF, 0x0F]), Err(ProtocolError));
    }

    /// Une entrée d'awareness, écrite à la main.
    fn entry(client: u64, clock: u32, json: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.write_var(client);
        bytes.write_var(clock);
        bytes.write_string(json);
        bytes
    }

    /// Un message d'awareness dont le tampon est `count`, écrit tel quel, puis `entries`.
    fn framed(count: &[u8], entries: &[Vec<u8>]) -> Vec<u8> {
        let mut payload = count.to_vec();
        payload.extend(entries.concat());
        awareness(&payload)
    }

    /// Ce que le message donne : `(client, horloge, état)` de chaque entrée gardée.
    fn entries_of(data: &[u8]) -> Vec<(u64, u32, String)> {
        let mut read: Vec<(u64, u32, String)> = messages(data)
            .into_iter()
            .flat_map(|message| match message {
                ClientMessage::Awareness(update) => update.clients,
                other => panic!("{other:?}"),
            })
            .map(|(client, entry)| (client.get(), entry.clock, entry.json.to_string()))
            .collect();
        read.sort();
        read
    }

    /// Ruling R123 : le nombre d'entrées est lu comme le lisent les clients. Les octets
    /// `81 80 80 80 10` valent 2^32 + 1 pour lib0, et 1 pour le lecteur de yrs, qui laisse
    /// tomber le 33e bit : derrière l'entrée que le serveur regardait, il en passait une
    /// autre, pour le client de n'importe qui.
    #[test]
    fn entry_count_beyond_32_bits_is_illegible() {
        let own = entry(7, 1, "{}");
        let forged = entry(42, 1_000_000, r#"{"user":{"name":"Alice (faux)"}}"#);
        let beyond = [0x81, 0x80, 0x80, 0x80, 0x10];
        // Ce que le lecteur de yrs en fait : la raison de ce test.
        assert_eq!(Cursor::new(&beyond).read_var::<u32>().ok(), Some(1));
        assert_eq!(
            decode(&framed(&beyond, &[own.clone(), forged])),
            Err(ProtocolError)
        );
        assert_eq!(
            decode(&framed(&beyond, std::slice::from_ref(&own))),
            Err(ProtocolError)
        );
        // Un nombre qui ne tient dans aucun entier : illisible aussi, jamais tronqué.
        let endless = [
            0x81, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01,
        ];
        assert_eq!(
            decode(&framed(&endless, std::slice::from_ref(&own))),
            Err(ProtocolError)
        );
        // Le même nombre écrit sur plus d'octets qu'il n'en faut vaut pareil pour tous.
        assert_eq!(
            entries_of(&framed(&[0x81, 0x00], std::slice::from_ref(&own))),
            [(7, 1, "{}".to_string())]
        );
        assert_eq!(
            entries_of(&framed(&[1], &[own])),
            [(7, 1, "{}".to_string())]
        );
    }

    /// Ruling R123 : les entrées annoncées épuisent le tampon. Ce qui suivrait ne serait lu
    /// par personne ici.
    #[test]
    fn trailing_bytes_after_the_entries_are_illegible() {
        let (own, other) = (entry(7, 1, "{}"), entry(42, 9, "{}"));
        assert_eq!(
            decode(&framed(&[1], &[own.clone(), other.clone()])),
            Err(ProtocolError)
        );
        assert_eq!(
            decode(&framed(&[1], &[own.clone(), vec![0]])),
            Err(ProtocolError)
        );
        assert_eq!(decode(&framed(&[0], &[vec![0]])), Err(ProtocolError));
        assert_eq!(
            entries_of(&framed(&[2], &[own, other])),
            [(7, 1, "{}".to_string()), (42, 9, "{}".to_string())]
        );
    }

    /// L'horloge et le client d'une entrée sont lus de même, sans rien laisser tomber : une
    /// horloge qui ne tient pas dans 32 bits est illisible, pas ramenée à une autre.
    #[test]
    fn clock_beyond_32_bits_is_illegible() {
        let mut beyond = Vec::new();
        beyond.write_var(7u64);
        beyond.extend([0x81, 0x80, 0x80, 0x80, 0x10]);
        beyond.write_string("{}");
        assert_eq!(decode(&framed(&[1], &[beyond])), Err(ProtocolError));
        assert_eq!(
            entries_of(&framed(&[1], &[entry(7, u32::MAX, "{}")])),
            [(7, u32::MAX, "{}".to_string())]
        );
    }

    /// Un état n'est pas lu au décodage : ce qui n'est pas du JSON en sort tel quel, et sera
    /// jugé après, à 2 Kio au plus. Au-delà, l'état est sauté sans être lu, pas même pour
    /// savoir s'il est de l'UTF-8, et son entrée ignorée ; les autres entrées du message
    /// restent.
    #[test]
    fn presence_states_are_not_read_at_decoding() {
        assert_eq!(MAX_PRESENCE_STATE, 2048);
        assert_eq!(
            entries_of(&framed(&[1], &[entry(42, 1, "{pas du json")])),
            [(42, 1, "{pas du json".to_string())]
        );
        let at_limit = "[".repeat(2048);
        assert_eq!(
            entries_of(&framed(&[1], &[entry(42, 1, &at_limit)])),
            [(42, 1, at_limit.clone())]
        );
        let beyond = "[".repeat(2049);
        assert_eq!(
            entries_of(&framed(&[2], &[entry(42, 1, &beyond), entry(7, 1, "{}")])),
            [(7, 1, "{}".to_string())]
        );
        // Sauté sans être lu : des octets qui ne sont pas de l'UTF-8 n'y changent rien.
        let mut raw = vec![42, 1];
        raw.write_var(2049u32);
        raw.extend([0xFF; 2049]);
        assert!(entries_of(&framed(&[1], &[raw])).is_empty());
        // Mais sa longueur est lue : un état qui déborde du tampon reste illisible.
        let mut truncated = vec![42, 1];
        truncated.write_var(4096u32);
        truncated.extend([b'0'; 100]);
        assert_eq!(decode(&framed(&[1], &[truncated])), Err(ProtocolError));
        // Ignorée, la dernière entrée d'un client fait foi quand même.
        assert!(entries_of(&framed(&[2], &[entry(42, 1, "{}"), entry(42, 2, &beyond)])).is_empty());
    }

    /// Ruling R118, au décodage : d'une connexion en lecture seule, aucun message
    /// d'awareness n'est rendu, et aucun état n'est copié. Le reste du message l'est, et un
    /// tampon mal formé reste illisible.
    #[test]
    fn presence_of_a_read_only_connection_is_not_decoded() {
        let reader = |data: &[u8]| super::decode(data, false);
        let (_, update, _) = sample();
        let mut data = framed(&[2], &[entry(42, 1, "{}"), entry(7, 1, "{}")]);
        data.extend(Message::Sync(SyncMessage::Update(update.clone())).encode_v1());
        data.extend(keepalive());
        assert_eq!(
            reader(&data),
            Ok(Decoded {
                messages: vec![ClientMessage::Update(update)],
                presence_dropped: None,
            })
        );
        // Ni un nombre d'entrées que les octets ne contiennent pas, ni des octets après
        // elles, ni un client hors de 53 bits ne passent pour autant.
        assert_eq!(reader(&[1, 3, 0xFF, 0xFF, 0x0F]), Err(ProtocolError));
        assert_eq!(
            reader(&framed(&[1], &[entry(42, 1, "{}"), vec![0]])),
            Err(ProtocolError)
        );
        assert_eq!(
            reader(&framed(&[1], &[entry(MAX_CLIENT + 1, 1, "{}")])),
            Err(ProtocolError)
        );
        // Plus de 512 entrées : comptées de même.
        let many: Vec<Vec<u8>> = (0..513).map(|client| entry(client, 1, "{}")).collect();
        let mut count = Vec::new();
        count.write_var(513u32);
        assert_eq!(
            reader(&framed(&count, &many)),
            Ok(Decoded {
                messages: vec![],
                presence_dropped: Some(513),
            })
        );
    }

    /// Un client répété dans un message : une seule entrée en sort, la dernière.
    #[test]
    fn repeated_client_keeps_its_last_entry() {
        let twice = framed(
            &[2],
            &[entry(7, 1000, r#"{"a":1}"#), entry(7, 1, r#"{"b":2}"#)],
        );
        assert_eq!(entries_of(&twice), [(7, 1, r#"{"b":2}"#.to_string())]);
    }

    /// Ruling R124 : 32 messages y-websocket dans un message WebSocket passent, 33 le
    /// rendent illisible, quels qu'ils soient.
    #[test]
    fn refuses_more_messages_than_the_limit() {
        assert_eq!(MAX_MESSAGES, 32);
        let queries = |count: usize| vec![MSG_QUERY_AWARENESS; count];
        assert_eq!(messages(&queries(32)).len(), 32);
        assert_eq!(decode(&queries(33)), Err(ProtocolError));
        // Ceux que le serveur ignore comptent aussi : ils sont lus.
        let ignored = |count: usize| Message::Auth(None).encode_v1().repeat(count);
        assert!(messages(&ignored(32)).is_empty());
        assert_eq!(decode(&ignored(33)), Err(ProtocolError));
        let mut mixed = ignored(31);
        mixed.extend(keepalive());
        assert_eq!(messages(&mixed).len(), 1);
        mixed.extend(queries(1));
        assert_eq!(decode(&mixed), Err(ProtocolError));
    }

    /// Ruling R136 : une trame de page reçue d'un client rend tout son message illisible, seule
    /// ou derrière des messages en règle. Les autres types inconnus restent ignorés, comme
    /// avant (et comme yrs).
    #[test]
    fn page_frame_from_a_client_is_illegible() {
        let frame = Message::Custom(MESSAGE_PAGE_EVENT, br#"{"type":"comments"}"#.to_vec());
        assert_eq!(decode(&frame.encode_v1()), Err(ProtocolError));
        let mut behind = keepalive();
        behind.extend(Message::AwarenessQuery.encode_v1());
        behind.extend(frame.encode_v1());
        assert_eq!(decode(&behind), Err(ProtocolError));
        // Vide, ou d'une longueur qui ne tient pas : illisible tout autant.
        assert_eq!(decode(&[MESSAGE_PAGE_EVENT, 0]), Err(ProtocolError));
        assert_eq!(decode(&[MESSAGE_PAGE_EVENT]), Err(ProtocolError));
        // Un autre type inconnu, lui, est sauté.
        for kind in [4, 99, 101, 127] {
            let other = Message::Custom(kind, vec![1, 2, 3]).encode_v1();
            assert_eq!(messages(&other), vec![], "type {kind}");
        }
    }

    /// Ce que le serveur écrit d'une trame de page : `100`, puis le JSON en `varUint8Array`.
    /// yrs le lit en message inconnu (`Custom`), comme y-websocket ; et le JSON a exactement la
    /// forme de R136, sans rien d'autre.
    #[test]
    fn page_frames_carry_their_json() {
        let read = |frame: &[u8]| -> serde_json::Value {
            assert_eq!(frame[0], 100);
            let mut decoder = DecoderV1::from(frame);
            let mut read: Vec<Message> = MessageReader::new(&mut decoder)
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(read.len(), 1, "un seul message");
            match read.remove(0) {
                Message::Custom(MESSAGE_PAGE_EVENT, json) => serde_json::from_slice(&json).unwrap(),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(
            read(&page_event(PageEvent::Comments)),
            json!({ "type": "comments" })
        );
        assert_eq!(
            read(&page_event(PageEvent::Versions)),
            json!({ "type": "versions" })
        );
        let (mona, tom) = (Uuid::now_v7(), Uuid::now_v7());
        let lines = [
            RosterLine {
                id: mona,
                name: "Mona \"M\"".into(),
                can_write: true,
            },
            RosterLine {
                id: tom,
                name: "Tom".into(),
                can_write: false,
            },
        ];
        assert_eq!(
            read(&roster(&lines)),
            json!({
                "type": "roster",
                "users": [
                    { "id": mona.to_string(), "name": "Mona \"M\"", "canWrite": true },
                    { "id": tom.to_string(), "name": "Tom", "canWrite": false },
                ],
            })
        );
        assert_eq!(read(&roster(&[])), json!({ "type": "roster", "users": [] }));
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
