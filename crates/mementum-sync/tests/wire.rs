//! Ce qui passe sur la socket : le serveur ne relaie jamais les octets de présence qu'il a
//! reçus, mais ce qu'il a lu et gardé, réencodé (Ruling R123) ; et un message WebSocket ne porte
//! pas plus de 32 messages y-websocket (Ruling R124).
//!
//! Le serveur lisait une mise à jour de présence avec yrs, et relayait ses octets tels quels
//! aux clients, qui la lisent avec lib0. Les deux ne lisent pas pareil : yrs laisse tomber les
//! bits d'un entier qui dépassent son type, et la table du serveur ne garde qu'une entrée par
//! client, la dernière, quand un client garde celle de plus haute horloge. Tout écart était un
//! moyen de faire passer aux clients ce que les règles du serveur n'avaient pas regardé.
//!
//! Le client de test lit avec yrs, comme le serveur : il ne voit pas ces écarts. Ces tests
//! lisent donc les messages relayés sur une socket en mémoire, octet par octet, avec une
//! lecture écrite ici, à la manière de lib0 : un entier variable sans limite de taille, et la
//! règle de y-protocols pour ce qu'un client retient.

mod common;

use std::collections::BTreeMap;

use common::*;
use futures::{SinkExt, StreamExt};
use mementum_sync::{ConnInfo, SyncConfig};
use uuid::Uuid;

/// Une étape 1 au vecteur d'état vide : le serveur y répond par une étape 2.
const STEP_1: [u8; 4] = [0, 0, 1, 0];

/// Une connexion du compte `account`, qui a le droit d'écrire.
fn of(account: Uuid) -> ConnInfo {
    ConnInfo {
        actor: Some(account),
        ..writing_conn()
    }
}

/// L'état de présence de quelqu'un qui s'appelle `name`, tel que le serveur le réécrit.
fn named(name: &str) -> String {
    format!(r#"{{"user":{{"name":"{name}"}}}}"#)
}

/// Écrit un entier variable de lib0 : 7 bits par octet, poids faibles d'abord.
fn write_var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Lit un entier variable comme lib0 : sans limite de taille (ici, 128 bits).
fn read_var(bytes: &[u8], at: &mut usize) -> u128 {
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

/// Une entrée de présence : client, horloge, état.
fn entry(client: u64, clock: u64, json: &str) -> Vec<u8> {
    let mut out = Vec::new();
    write_var(&mut out, client);
    write_var(&mut out, clock);
    write_var(&mut out, json.len() as u64);
    out.extend(json.as_bytes());
    out
}

/// Un message de présence : `count`, le nombre d'entrées écrit tel quel, puis `entries`.
fn presence(count: &[u8], entries: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = count.to_vec();
    payload.extend(entries.concat());
    let mut out = vec![1];
    write_var(&mut out, payload.len() as u64);
    out.extend(payload);
    out
}

/// Les entrées d'un message de présence du serveur, lues à la main : `(client, horloge,
/// état)`, dans l'ordre. Le message ne porte rien d'autre : il est lu jusqu'au dernier octet.
fn entries_of(message: &[u8]) -> Vec<(u128, u128, String)> {
    assert_eq!(message[0], 1, "pas un message de présence : {message:?}");
    let mut at = 1;
    let length = read_var(message, &mut at) as usize;
    assert_eq!(at + length, message.len(), "un seul message y-websocket");
    let count = read_var(message, &mut at);
    let mut entries = Vec::new();
    for _ in 0..count {
        let client = read_var(message, &mut at);
        let clock = read_var(message, &mut at);
        let size = read_var(message, &mut at) as usize;
        let json = std::str::from_utf8(&message[at..at + size]).expect("un état en UTF-8");
        at += size;
        entries.push((client, clock, json.to_string()));
    }
    assert_eq!(at, message.len(), "des octets après les entrées annoncées");
    entries
}

/// Ce qu'un client y-websocket retient de la présence (`applyAwarenessUpdate` de
/// y-protocols) : par client, l'horloge et l'état (`None` : retiré). Une entrée n'est appliquée
/// que si son horloge est plus haute, ou égale pour un retrait d'un état encore là.
#[derive(Default)]
struct Retained(BTreeMap<u128, (u128, Option<String>)>);

impl Retained {
    /// Applique un message, et rend ce que le client en renvoie au serveur : les clients
    /// ajoutés, modifiés ou retirés, chacun à l'horloge qu'il retient.
    fn apply(&mut self, message: &[u8]) -> Vec<(u128, u128, String)> {
        let mut echoed: Vec<(u128, u128, String)> = Vec::new();
        for (client, clock, json) in entries_of(message) {
            let state = (json != "null").then_some(json);
            let known = self.0.get(&client);
            // Un client inconnu vaut l'horloge 0.
            let current = known.map_or(0, |(current, _)| *current);
            let held = known.is_some_and(|(_, held)| held.is_some());
            if !(current < clock || (current == clock && state.is_none() && held)) {
                continue;
            }
            // Ajouté, modifié ou retiré ; le retrait d'un client inconnu ne se renvoie pas.
            if state.is_some() || known.is_some() {
                let json = state.clone().unwrap_or_else(|| "null".to_string());
                echoed.retain(|(echoed_client, ..)| *echoed_client != client);
                echoed.push((client, clock, json));
            }
            self.0.insert(client, (clock, state));
        }
        echoed
    }

    fn state(&self, client: u64) -> Option<&str> {
        self.0
            .get(&u128::from(client))
            .and_then(|(_, state)| state.as_deref())
    }
}

/// Une connexion par une socket en mémoire : ses messages sont lus tels que le serveur les
/// écrit.
struct Raw {
    socket: MemorySocket,
}

impl Raw {
    async fn connect(f: &Fixture, conn: ConnInfo) -> Raw {
        let (server_end, socket) = memory_socket(1024);
        tokio::spawn(f.hub.clone().serve_connection(f.page, conn, server_end));
        Raw { socket }
    }

    async fn send(&mut self, message: Vec<u8>) {
        self.socket.send(message).await.expect("socket ouverte");
    }

    /// Le prochain message du serveur ; `None` s'il a fermé la connexion.
    async fn next(&mut self) -> Option<Vec<u8>> {
        tokio::time::timeout(WAIT, self.socket.next())
            .await
            .expect("un message du serveur, ou la fermeture")
    }

    /// Demande une étape 2 et lit jusqu'à elle : rend les messages de présence reçus avant.
    /// Le serveur met en file ce qu'il diffuse avant de lire le message suivant d'une
    /// connexion : tout ce qu'il a relayé avant cette demande est dans ce qui est rendu.
    /// `None` : il a fermé la connexion.
    async fn presence_so_far(&mut self) -> Option<Vec<Vec<u8>>> {
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
}

/// Le serveur a traité tout ce que `client` a envoyé, et ne l'a pas fermé.
async fn still_served(client: &Client) -> bool {
    let before = client.step2_count();
    client.resync();
    client
        .wait_until(WAIT, |c| c.is_closed() || c.step2_count() > before)
        .await;
    !client.is_closed() && client.step2_count() > before
}

/// La scène commune : Alice, qui écrit, a annoncé son état ; une observatrice lit la socket ;
/// Mallory, d'un autre compte, écrit aussi. L'observatrice a lu son accueil : elle retient
/// l'état d'Alice.
async fn scene(f: &Fixture) -> (Client, Client, Raw, Retained) {
    let alice = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    alice.set_awareness(&named("Alice"));
    assert!(still_served(&alice).await);
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mut observer = Raw::connect(f, of(new_user(&f.db).await)).await;
    let mut retained = Retained::default();
    for message in observer
        .presence_so_far()
        .await
        .expect("observatrice servie")
    {
        retained.apply(&message);
    }
    assert_eq!(
        retained.state(alice.client_id()),
        Some(named("Alice").as_str())
    );
    (alice, mallory, observer, retained)
}

/// Le serveur relaie ce qu'il a gardé, réencodé : une entrée par client gardé, l'état tel
/// qu'il l'a relu. Ni l'entrée écartée (elle visait le client d'Alice), ni la façon dont le
/// message était écrit (un nombre d'entrées sur deux octets, des espaces dans l'état) ne
/// passent.
#[tokio::test]
async fn relayed_presence_is_what_the_server_kept_not_what_it_received() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (alice, mallory, mut observer, mut retained) = scene(&f).await;
    let (alice_id, mallory_id) = (alice.client_id(), mallory.client_id());

    // Une entrée gardée, une écartée.
    let spaced = r#"{ "user" : { "name" : "Mallory" } }"#;
    let sent = presence(
        &[2],
        &[
            entry(mallory_id, 1, spaced),
            entry(alice_id, 1_000_000, &named("Alice (faux)")),
        ],
    );
    mallory.send_raw(sent.clone());
    assert!(still_served(&mallory).await);
    let relayed = observer
        .presence_so_far()
        .await
        .expect("observatrice servie");
    let expected = presence(&[1], &[entry(mallory_id, 1, &named("Mallory"))]);
    assert_eq!(relayed, [expected], "reçu du serveur, octet par octet");
    assert_ne!(relayed[0], sent);
    assert_eq!(
        entries_of(&relayed[0]),
        [(u128::from(mallory_id), 1, named("Mallory"))]
    );

    // Rien d'écarté, mais le message n'est pas écrit comme le serveur l'écrirait : le
    // nombre d'entrées tient ici sur deux octets (`81 00`, soit 1).
    let sent = presence(&[0x81, 0x00], &[entry(mallory_id, 2, spaced)]);
    mallory.send_raw(sent.clone());
    assert!(still_served(&mallory).await);
    let relayed = observer
        .presence_so_far()
        .await
        .expect("observatrice servie");
    let expected = presence(&[1], &[entry(mallory_id, 2, &named("Mallory"))]);
    assert_eq!(relayed, [expected], "reçu du serveur, octet par octet");
    assert_ne!(relayed[0], sent);

    // Chez un client, Alice est toujours Alice.
    for message in relayed {
        retained.apply(&message);
    }
    assert_eq!(retained.state(alice_id), Some(named("Alice").as_str()));
}

/// Les octets `81 80 80 80 10` valent 1 pour qui lit un nombre de 32 bits, et 2^32 + 1 pour
/// un client. Derrière la seule entrée que le serveur regardait, une entrée pour le client
/// d'Alice passait, et la remplaçait chez tout le monde. Le message est illisible : la
/// connexion est fermée, et rien n'en est relayé.
#[tokio::test]
async fn entry_count_beyond_32_bits_is_illegible() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (alice, mallory, mut observer, mut retained) = scene(&f).await;
    let (alice_id, mallory_id) = (alice.client_id(), mallory.client_id());

    mallory.send_raw(presence(
        &[0x81, 0x80, 0x80, 0x80, 0x10],
        &[
            entry(mallory_id, 1, &named("Mallory")),
            entry(alice_id, 1_000_000, &named("Alice (faux)")),
        ],
    ));
    assert!(
        mallory.wait_until(WAIT, |c| c.is_closed()).await,
        "la connexion qui a envoyé le message est encore servie"
    );
    // Alice annonce la suite : c'est tout ce que l'observatrice reçoit.
    alice.set_awareness(&named("Alice, plus loin"));
    assert!(still_served(&alice).await);
    let relayed = observer
        .presence_so_far()
        .await
        .expect("observatrice servie");
    assert_eq!(relayed.len(), 1, "{relayed:?}");
    for message in &relayed {
        assert!(
            !String::from_utf8_lossy(message).contains("faux"),
            "l'entrée forgée est relayée"
        );
        retained.apply(message);
    }
    assert_eq!(
        retained.state(alice_id),
        Some(named("Alice, plus loin").as_str())
    );
    assert_eq!(retained.state(mallory_id), None);
}

/// Un client répété dans un message : le serveur n'en garde qu'une entrée, la dernière, et
/// c'est elle seule que les clients reçoivent. Ils retenaient la première quand son horloge
/// était plus haute, et le serveur et eux ne connaissaient plus la même horloge.
#[tokio::test]
async fn client_repeated_in_one_message_is_relayed_once() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (_alice, mallory, mut observer, mut retained) = scene(&f).await;
    let mallory_id = mallory.client_id();

    mallory.send_raw(presence(
        &[2],
        &[
            entry(mallory_id, 1000, &named("Premier")),
            entry(mallory_id, 1, &named("Second")),
        ],
    ));
    assert!(still_served(&mallory).await);
    let relayed = observer
        .presence_so_far()
        .await
        .expect("observatrice servie");
    let expected = presence(&[1], &[entry(mallory_id, 1, &named("Second"))]);
    assert_eq!(relayed, [expected], "reçu du serveur, octet par octet");
    // Ce qu'un client en retient, et renverrait : l'horloge que le serveur connaît.
    let echoed = retained.apply(&relayed[0]);
    assert_eq!(echoed, [(u128::from(mallory_id), 1, named("Second"))]);
}

/// Un tampon de présence où il reste des octets après les entrées annoncées est illisible :
/// une entrée de plus que le nombre annoncé, ou un seul octet.
#[tokio::test]
async fn trailing_bytes_after_the_entries_are_illegible() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (alice, mallory, mut observer, mut retained) = scene(&f).await;
    let (alice_id, mallory_id) = (alice.client_id(), mallory.client_id());

    mallory.send_raw(presence(
        &[1],
        &[
            entry(mallory_id, 1, &named("Mallory")),
            entry(alice_id, 1_000_000, &named("Alice (faux)")),
        ],
    ));
    assert!(mallory.wait_until(WAIT, |c| c.is_closed()).await);
    let again = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    again.send_raw(presence(
        &[1],
        &[entry(again.client_id(), 1, &named("Encore")), vec![0]],
    ));
    assert!(again.wait_until(WAIT, |c| c.is_closed()).await);

    alice.set_awareness(&named("Alice, plus loin"));
    assert!(still_served(&alice).await);
    let relayed = observer
        .presence_so_far()
        .await
        .expect("observatrice servie");
    assert_eq!(relayed.len(), 1, "{relayed:?}");
    retained.apply(&relayed[0]);
    assert_eq!(
        retained.state(alice_id),
        Some(named("Alice, plus loin").as_str())
    );
    assert_eq!(retained.state(mallory_id), None);
    assert_eq!(retained.state(again.client_id()), None);
}

/// Le scénario de la re-relecture, de bout en bout. Mallory annonce huit clients inventés,
/// chacun deux fois dans le même message : à l'horloge 1000, puis à l'horloge 1. Le serveur
/// retenait 1, les clients 1000. Elle part ; son départ porte l'horloge du serveur à 2. Le
/// renvoi d'Alice, que fait tout client y-websocket de ce qu'il reçoit, arrive après : à
/// l'horloge 1000, il était « plus récent », Alice prenait les huit clients, et le serveur
/// fermait sa connexion (neuf clients avec le sien).
///
/// Alice lit ici la socket comme un client : ce qu'elle renvoie est ce qu'elle a retenu.
#[tokio::test]
async fn repeated_client_does_not_let_a_late_echo_close_honest_connections() {
    let f = Fixture::new(SyncConfig::default()).await;
    let mut alice = Raw::connect(&f, of(new_user(&f.db).await)).await;
    const ALICE: u64 = 4_242;
    alice
        .send(presence(&[1], &[entry(ALICE, 1, &named("Alice"))]))
        .await;
    let mut retained = Retained::default();
    assert_eq!(alice.presence_so_far().await, Some(vec![]));

    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let fakes: Vec<u64> = (1..=8).map(|i| 900_000 + i).collect();
    let ghost = named("Fantôme");
    let mut entries: Vec<Vec<u8>> = fakes.iter().map(|id| entry(*id, 1000, &ghost)).collect();
    entries.extend(fakes.iter().map(|id| entry(*id, 1, &ghost)));
    mallory.send_raw(presence(&[16], &entries));
    assert!(still_served(&mallory).await);
    // Ce qu'Alice en reçoit, et ce que son client en renverra.
    let relayed = alice.presence_so_far().await.expect("Alice servie");
    assert_eq!(relayed.len(), 1, "{relayed:?}");
    let echo = retained.apply(&relayed[0]);
    assert_eq!(echo.len(), 8);

    // Mallory part : ses clients sont retirés, ce qu'Alice reçoit aussi.
    drop(mallory);
    let mut removals = 0;
    while removals < 8 {
        for message in alice.presence_so_far().await.expect("Alice servie") {
            removals += entries_of(&message).len();
            retained.apply(&message);
        }
    }
    // Le renvoi, arrivé après le départ.
    let late: Vec<Vec<u8>> = echo
        .iter()
        .map(|(client, clock, json)| entry(*client as u64, *clock as u64, json))
        .collect();
    alice.send(presence(&[8], &late)).await;
    assert!(
        alice.presence_so_far().await.is_some(),
        "Alice est fermée pour avoir renvoyé ce qu'elle a reçu"
    );
    // Et les fantômes sont partis de chez elle : le retrait du serveur portait une horloge
    // plus haute que celle qu'elle retenait.
    for id in &fakes {
        assert_eq!(
            retained.state(*id),
            None,
            "le fantôme {id} reste chez Alice"
        );
    }
    // Il lui reste sept places : elle ne tient que le sien.
    let more: Vec<Vec<u8>> = (1..=7).map(|i| entry(910_000 + i, 1, &ghost)).collect();
    alice.send(presence(&[7], &more)).await;
    assert!(alice.presence_so_far().await.is_some());
}

/// Ruling R124 : un message WebSocket qui porte plus de 32 messages y-websocket est
/// illisible, et ferme sa connexion ; à 32, il passe.
#[tokio::test]
async fn websocket_message_with_more_than_32_messages_closes_the_connection() {
    let f = Fixture::new(SyncConfig::default()).await;
    let other = f.client().await;
    // Trente-deux demandes d'awareness (un octet chacune) : servi.
    let at_limit = f.client().await;
    at_limit.send_raw(vec![3; 32]);
    assert!(still_served(&at_limit).await);
    // Une de plus : fermé.
    let beyond = f.client().await;
    beyond.send_raw(vec![3; 33]);
    assert!(
        beyond.wait_until(WAIT, |c| c.is_closed()).await,
        "33 messages y-websocket dans un message WebSocket : la connexion est encore servie"
    );
    // Des messages que le serveur ignore (authentification) comptent aussi.
    let ignored = f.client().await;
    ignored.send_raw([2, 1].repeat(33));
    assert!(ignored.wait_until(WAIT, |c| c.is_closed()).await);
    assert!(still_served(&other).await && still_served(&at_limit).await);
}
