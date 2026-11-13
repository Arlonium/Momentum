//! Ce qui passe sur la socket : le serveur ne relaie jamais les octets de présence qu'il a
//! reçus, mais ce qu'il a lu et gardé, réencodé (Ruling R123) ; il en va de même d'une mise à
//! jour du document (Ruling R130) ; et un message WebSocket ne porte pas plus de 32 messages
//! y-websocket (Ruling R124).
//!
//! Le serveur lisait une mise à jour de présence avec yrs, et relayait ses octets tels quels
//! aux clients, qui la lisent avec lib0. Les deux ne lisent pas pareil : yrs laisse tomber les
//! bits d'un entier qui dépassent son type, et la table du serveur ne garde qu'une entrée par
//! client, la dernière, quand un client garde celle de plus haute horloge. Tout écart était un
//! moyen de faire passer aux clients ce que les règles du serveur n'avaient pas regardé.
//!
//! Le client de test lit avec yrs, comme le serveur : il ne voit pas ces écarts. Ces tests
//! lisent donc les messages relayés sur une socket en mémoire, octet par octet (`Raw`, dans
//! `common/raw.rs`), à la manière de lib0 : un entier variable sans limite de taille, et la
//! règle de y-protocols pour ce qu'un client retient.

mod common;

use std::collections::BTreeMap;

use common::raw::{Raw, deletion, read_var, write_var};
use common::*;
use mementum_sync::{ConnInfo, SyncConfig};
use uuid::Uuid;

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

/// Ruling R136 : le nom d'un état de présence est celui du compte de la connexion, que le
/// serveur écrit quoi que le client annonce. Ce que l'observatrice lit, octet par octet, porte le
/// nom du compte de Mallory, pas celui qu'elle a mis ; un état sans `user` en reçoit un ; un nom
/// mal formé ne fait plus ignorer l'état, le reste de sa forme toujours. Une connexion sans
/// compte (tests du hub) garde le nom annoncé.
#[tokio::test]
async fn presence_name_is_the_account_name() {
    let f = Fixture::new(SyncConfig::default()).await;
    let account = ConnInfo {
        name: Some("Mallory Martin".into()),
        ..of(new_user(&f.db).await)
    };
    let mallory = Client::connect_synced(&f.url_for(account)).await;
    let mut observer = Raw::connect(&f, of(new_user(&f.db).await)).await;
    assert_eq!(observer.presence_so_far().await, Some(vec![]));
    let id = mallory.client_id();
    let long = format!(r#"{{"user":{{"name":"{}"}}}}"#, "a".repeat(101));
    let cases: [(&str, &str); 7] = [
        (
            r#"{"user":{"name":"Alice"}}"#,
            r#"{"user":{"name":"Mallory Martin"}}"#,
        ),
        ("{}", r#"{"user":{"name":"Mallory Martin"}}"#),
        (
            r##"{"cursor":null,"user":{"color":"#2B59C3"}}"##,
            r##"{"cursor":null,"user":{"color":"#2B59C3","name":"Mallory Martin"}}"##,
        ),
        (
            r##"{"user":{"name":7,"color":"#2B59C3"}}"##,
            r##"{"user":{"name":"Mallory Martin","color":"#2B59C3"}}"##,
        ),
        (
            r#"{"user":{"name":""}}"#,
            r#"{"user":{"name":"Mallory Martin"}}"#,
        ),
        (
            r#"{"user":{"name":"Mal\u0007lory"}}"#,
            r#"{"user":{"name":"Mallory Martin"}}"#,
        ),
        (&long, r#"{"user":{"name":"Mallory Martin"}}"#),
    ];
    for (clock, (announced, relayed)) in (1..).zip(cases) {
        mallory.send_raw(presence(&[1], &[entry(id, clock, announced)]));
        assert!(still_served(&mallory).await);
        assert_eq!(
            observer
                .presence_so_far()
                .await
                .expect("observatrice servie"),
            [presence(&[1], &[entry(id, clock, relayed)])],
            "annoncé : {announced}"
        );
    }
    // Le reste de la forme est jugé comme avant : rien n'est relayé.
    for (clock, refused) in (10..).zip([
        r#"{"user":{"name":"Mona","color":"red;position:fixed"}}"#,
        r#"{"user":{"name":"Mona","avatar":"x"}}"#,
        r#"{"user":null}"#,
        r#"{"surprise":true}"#,
    ]) {
        mallory.send_raw(presence(&[1], &[entry(id, clock, refused)]));
        assert!(still_served(&mallory).await);
        assert_eq!(
            observer.presence_so_far().await,
            Some(vec![]),
            "relayé : {refused}"
        );
    }
    // Une connexion sans compte : son nom, s'il est en règle, est gardé.
    let nobody = f.client().await;
    let alice = r#"{"user":{"name":"Alice"}}"#;
    nobody.send_raw(presence(&[1], &[entry(nobody.client_id(), 1, alice)]));
    assert!(still_served(&nobody).await);
    assert_eq!(
        observer
            .presence_so_far()
            .await
            .expect("observatrice servie"),
        [presence(&[1], &[entry(nobody.client_id(), 1, alice)])]
    );
}

/// La piste du registre du lot 2 (reportée avec R109) : le serveur retirait l'état d'une
/// connexion qui part en avançant l'horloge de son client (`remove_state` de yrs). L'état
/// suivant de l'onglet, à l'horloge juste au-dessus de la sienne, n'était donc pas plus récent :
/// ignoré, et sa présence ne revenait qu'au changement d'état d'après. Le retrait se fait
/// désormais à l'horloge de l'état retiré : un client l'applique (rien ne reste d'une connexion
/// partie, pas de curseur fantôme), et l'état suivant que la connexion qui revient annonce, à
/// une horloge plus haute que celle du retrait, est relayé tout de suite, puis appliqué. Lu
/// octet par octet, comme un client le lit.
///
/// Ce que le test ne dit pas : y-websocket, à `onopen`, réannonce l'état de l'onglet à
/// l'horloge qu'il avait. Ce renvoi n'est pas plus récent que le retrait, et il est ignoré ; la
/// présence revient à l'état suivant (un mouvement de curseur, ou le renouvellement de
/// y-protocols). Pour un retour dès la reconnexion, le client doit annoncer un état neuf à
/// chaque connexion (registre du lot 3, Task 6).
#[tokio::test]
async fn presence_returns_at_once_after_a_reconnect() {
    const TAB: u64 = 4_242;
    let f = Fixture::new(SyncConfig::default()).await;
    let alice = new_user(&f.db).await;
    let mut observer = Raw::connect(&f, of(new_user(&f.db).await)).await;
    let mut retained = Retained::default();
    let first = Client::connect_synced(&f.url_for(of(alice))).await;
    first.send_raw(presence(&[1], &[entry(TAB, 1, &named("Alice"))]));
    assert!(still_served(&first).await);
    for message in observer
        .presence_so_far()
        .await
        .expect("observatrice servie")
    {
        retained.apply(&message);
    }
    assert_eq!(retained.state(TAB), Some(named("Alice").as_str()));

    // Coupure. Le retrait arrive, à l'horloge de l'état retiré.
    drop(first);
    let mut relayed = Vec::new();
    let deadline = tokio::time::Instant::now() + WAIT;
    while relayed.is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "le départ n'a pas été relayé"
        );
        relayed = observer
            .presence_so_far()
            .await
            .expect("observatrice servie");
    }
    assert_eq!(
        relayed,
        [presence(&[1], &[entry(TAB, 1, "null")])],
        "le retrait n'est pas à l'horloge de l'état"
    );
    retained.apply(&relayed[0]);
    assert_eq!(
        retained.state(TAB),
        None,
        "un état fantôme reste chez un client"
    );

    // L'onglet revient par une connexion neuve, et annonce son état suivant : horloge 2, plus
    // haute que celle du retrait (un renvoi à l'horloge 1 serait ignoré).
    let second = Client::connect_synced(&f.url_for(of(alice))).await;
    let back = named("Alice, revenue");
    second.send_raw(presence(&[1], &[entry(TAB, 2, &back)]));
    assert!(still_served(&second).await);
    let relayed = observer
        .presence_so_far()
        .await
        .expect("observatrice servie");
    assert_eq!(
        relayed,
        [presence(&[1], &[entry(TAB, 2, &back)])],
        "le premier état annoncé au retour n'est pas relayé"
    );
    retained.apply(&relayed[0]);
    assert_eq!(retained.state(TAB), Some(back.as_str()));
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

/// La seule plage qu'une mise à jour sans bloc supprime, lue à la manière de lib0, jusqu'au
/// dernier octet.
fn deletion_of(update: &[u8]) -> (u128, u128, u128) {
    let mut at = 0;
    assert_eq!(read_var(update, &mut at), 0, "aucun bloc");
    assert_eq!(read_var(update, &mut at), 1, "un client");
    let client = read_var(update, &mut at);
    assert_eq!(read_var(update, &mut at), 1, "une plage");
    let clock = read_var(update, &mut at);
    let len = read_var(update, &mut at);
    assert_eq!(at, update.len(), "des octets après les suppressions");
    (client, clock, len)
}

/// Une horloge écrite avec 2^32 de plus : yrs, qui lit sur 32 bits, y voit le texte de la page,
/// et le serveur le supprime ; un navigateur, qui lit l'entier en entier, y voit une plage
/// qu'il ne connaît pas, la garde en attente et ne supprime rien. Relayée telle quelle, la mise
/// à jour laissait donc les navigateurs afficher un texte que le serveur n'a plus : tout ce
/// qu'on y tapait ensuite était perdu, sans que rien le dise. Le serveur relaie et écrit ce
/// qu'il a lu, réencodé : les navigateurs suppriment le même texte que lui.
#[tokio::test]
async fn relayed_update_is_what_the_server_read_not_what_it_received() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd(
        "Bonjour
",
    )
    .await;
    let id = f.ids().await.remove(0);
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mut observer = Raw::connect(&f, of(new_user(&f.db).await)).await;
    assert_eq!(
        observer
            .updates_so_far()
            .await
            .expect("observatrice servie"),
        Vec::<Vec<u8>>::new()
    );

    // La suppression honnête de « Bonjour », préparée sans être envoyée : elle dit le client
    // et la plage du texte.
    let honest = mallory.page().unwrap().replace_text(&id, 0, 7, "").unwrap();
    let (client, clock, len) = deletion_of(&honest);
    let forged = deletion(client as u64, clock as u64 + (1 << 32), len as u64);
    assert_eq!(deletion_of(&forged), (client, clock + (1 << 32), len));

    mallory.send_update(&forged);
    assert!(still_served(&mallory).await);
    assert_eq!(
        text(&f.hub.read_blocks(f.page).await.unwrap()),
        "",
        "le serveur a supprimé le texte"
    );

    let relayed = observer
        .updates_so_far()
        .await
        .expect("observatrice servie");
    assert_eq!(relayed.len(), 1, "{relayed:?}");
    assert_eq!(
        deletion_of(&relayed[0]),
        (client, clock, len),
        "un navigateur supprime ce que le serveur a supprimé"
    );
    assert_ne!(relayed[0], forged);
    // Le journal garde lui aussi ce qui a été lu, pas ce qui a été reçu.
    let stored = f.store.load_doc(f.page).await.unwrap();
    let (_, last) = stored.updates.last().expect("une mise à jour au journal");
    assert_eq!(last, &relayed[0]);
}

/// Ruling R181 : une mise à jour d'un client qui ne change rien à la page n'est ni écrite au
/// journal ni relayée ; elle répond à son expéditeur comme une autre. Yjs met tout son jeu de
/// suppressions dans l'étape 2 : c'est ce qu'envoie un onglet qui revient sans rien de neuf, sur
/// une page qui a des suppressions. Tout ce qu'elle porte est déjà dans le document du serveur,
/// donc déjà chez les autres. Une mise à jour qui ne fait que supprimer change la page : elle est
/// écrite et relayée.
#[tokio::test]
async fn update_that_changes_nothing_is_neither_written_nor_relayed() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Bonjour\n").await;
    // Elle remplace la première : la page a des suppressions.
    f.write_mmd("Bonjour tout le monde\n").await;
    let id = f.ids().await.remove(0);
    let tab = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mut observer = Raw::connect(&f, of(new_user(&f.db).await)).await;
    assert_eq!(
        observer
            .updates_so_far()
            .await
            .expect("observatrice servie"),
        Vec::<Vec<u8>>::new()
    );
    let journal = f.store.load_doc(f.page).await.unwrap().updates.len();

    let page = tab.page().unwrap();
    let step2 = page.encode_diff(&page.state_vector()).unwrap();
    assert!(
        step2.len() > 2,
        "l'étape 2 porte des suppressions : {step2:?}"
    );
    tab.send_update(&step2);
    assert!(still_served(&tab).await);
    assert_eq!(
        f.store.load_doc(f.page).await.unwrap().updates.len(),
        journal,
        "rien au journal"
    );
    assert_eq!(
        observer
            .updates_so_far()
            .await
            .expect("observatrice servie"),
        Vec::<Vec<u8>>::new(),
        "rien chez l'observatrice"
    );

    // Une suppression seule : écrite et relayée.
    let deletion = tab.page().unwrap().replace_text(&id, 0, 8, "").unwrap();
    tab.send_update(&deletion);
    assert!(still_served(&tab).await);
    assert_eq!(
        text(&f.hub.read_blocks(f.page).await.unwrap()),
        "tout le monde"
    );
    let relayed = observer
        .updates_so_far()
        .await
        .expect("observatrice servie");
    assert_eq!(relayed.len(), 1, "{relayed:?}");
    let stored = f.store.load_doc(f.page).await.unwrap();
    assert_eq!(stored.updates.len(), journal + 1);
    assert_eq!(stored.updates.last().map(|(_, u)| u), Some(&relayed[0]));
}

/// Une mise à jour qui supprime ce que la page ne connaît pas encore ne change rien sur le
/// moment : yrs garderait la suppression en attente, et l'appliquerait à l'arrivée des éléments.
/// Le Ruling R181 l'écrivait et la relayait. Depuis le Ruling R198, elle attend comme un trou
/// (R131), dans la file de sa connexion : ni appliquée, ni écrite, ni relayée, et rien n'est en
/// attente dans le document du serveur. Ce test changeait de sens avec R198 : il s'appelait
/// `pending_deletion_is_still_written_and_relayed`.
#[tokio::test]
async fn pending_deletion_is_held_neither_written_nor_relayed() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Bonjour\n").await;
    let tab = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mut observer = Raw::connect(&f, of(new_user(&f.db).await)).await;
    assert_eq!(
        observer
            .updates_so_far()
            .await
            .expect("observatrice servie"),
        Vec::<Vec<u8>>::new()
    );
    let journal = f.store.load_doc(f.page).await.unwrap().updates.len();

    // Un client que la page ne connaît pas, des éléments qu'elle n'a jamais reçus.
    let pending = deletion(4242, 10, 3);
    tab.send_update(&pending);
    assert!(still_served(&tab).await, "retenue, pas refusée");
    assert_eq!(
        observer
            .updates_so_far()
            .await
            .expect("observatrice servie"),
        Vec::<Vec<u8>>::new(),
        "relayée"
    );
    let stored = f.store.load_doc(f.page).await.unwrap();
    assert_eq!(stored.updates.len(), journal, "écrite");
    assert!(
        !stored_page(&f.store, f.page).await.has_pending_deletions(),
        "une suppression en attente au journal"
    );
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "Bonjour");
}
