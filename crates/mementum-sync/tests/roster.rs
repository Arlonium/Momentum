//! Trame de page (Ruling R136) : ce qui ne doit aller qu'aux personnes qui lisent une page passe
//! par sa connexion de synchro, dans une trame que le serveur écrit seul (type 100, puis un
//! JSON) :
//! - `roster` : qui a la page ouverte, une ligne par compte, lecteurs compris (Ruling R143) ;
//! - `comments` et `versions` : relire les commentaires, ou l'historique, par l'API.
//!
//! Le client de test garde le JSON de chaque trame qu'il reçoit (`Client::page_events`). La
//! diffusion d'une page est ordonnée : quand un client reçoit une trame, tout ce que le serveur
//! a diffusé avant elle lui est déjà parvenu. Une trame de trop se voit donc à la suivante, qui
//! n'arrive pas à sa place.

mod common;

use common::*;
use mementum_store::{NewNode, NodeKind};
use mementum_sync::{ConnInfo, MESSAGE_PAGE_EVENT, PageEvent, SyncConfig};
use serde_json::{Value, json};
use uuid::Uuid;
use yrs::sync::{Message, SyncMessage};
use yrs::updates::encoder::Encode;
use yrs::{Transact, XmlFragment, XmlTextPrelim};

/// Une connexion du compte `account`, qui s'appelle `name` et peut écrire la page.
fn of(account: Uuid, name: &str) -> ConnInfo {
    ConnInfo {
        actor: Some(account),
        name: Some(name.into()),
        ..writing_conn()
    }
}

/// La même, en lecture seule : une lectrice, une invitée.
fn reading(account: Uuid, name: &str) -> ConnInfo {
    ConnInfo {
        can_write: false,
        ..of(account, name)
    }
}

/// Les trames que `client` a reçues, relues.
fn frames(client: &Client) -> Vec<Value> {
    client
        .page_events()
        .iter()
        .map(|json| serde_json::from_str(json).expect("une trame en JSON"))
        .collect()
}

/// La trame d'indice `index` que `client` a reçue (la première vaut 0), attendue [`WAIT`] au
/// plus. Une trame de trop avant elle la décale, et le test le voit.
async fn nth(client: &Client, index: usize) -> Value {
    assert!(
        client
            .wait_until(WAIT, |c| c.page_events().len() > index)
            .await,
        "trame {index} attendue, reçues : {:?}",
        client.page_events()
    );
    frames(client).swap_remove(index)
}

/// La trame `roster` qui nomme ces comptes, dans cet ordre : `(compte, nom, peut écrire)`.
fn roster(users: &[(Uuid, &str, bool)]) -> Value {
    let users: Vec<Value> = users
        .iter()
        .map(|(id, name, can_write)| {
            json!({ "id": id.to_string(), "name": name, "canWrite": can_write })
        })
        .collect();
    json!({ "type": "roster", "users": users })
}

/// Trois comptes, créés dans cet ordre : leurs identifiants (UUIDv7) se suivent, et c'est
/// l'ordre de la liste.
async fn three_accounts(f: &Fixture) -> (Uuid, Uuid, Uuid) {
    (
        new_user(&f.db).await,
        new_user(&f.db).await,
        new_user(&f.db).await,
    )
}

/// Une trame `roster` part à chaque connexion de la page, lecteurs compris, à chaque arrivée et
/// à chaque départ qui change la liste ; un arrivant la reçoit dès son accueil. Une ligne par
/// compte, même avec deux onglets ; une lectrice y figure, sans pouvoir écrire ; une connexion
/// sans compte n'y figure pas. Le nom est celui du compte, passé par `display_name`, et la trame
/// ne porte rien d'autre que l'identifiant, le nom et le droit d'écrire.
#[tokio::test]
async fn roster_names_everyone_on_the_page() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (alice, bob, gina) = three_accounts(&f).await;

    // Alice arrive : la liste de son accueil ne nomme qu'elle.
    let alice_tab = Client::connect_synced(&f.url_for(of(alice, "Alice"))).await;
    assert_eq!(nth(&alice_tab, 0).await, roster(&[(alice, "Alice", true)]));

    // Bob arrive : chacun reçoit la liste des deux.
    let bob_1 = Client::connect_synced(&f.url_for(of(bob, "Bob"))).await;
    let both = roster(&[(alice, "Alice", true), (bob, "Bob", true)]);
    assert_eq!(nth(&alice_tab, 1).await, both);
    assert_eq!(nth(&bob_1, 0).await, both);

    // Un second onglet de Bob : la liste ne change pas. Lui la reçoit, dans son accueil.
    let bob_2 = Client::connect_synced(&f.url_for(of(bob, "Bob"))).await;
    assert_eq!(nth(&bob_2, 0).await, both);

    // Gina, en lecture seule, figure sans pouvoir écrire ; son nom de compte porte un caractère
    // de contrôle, que la liste ne montre pas. Les autres reçoivent cette trame à la suite de la
    // précédente : le second onglet de Bob ne leur en avait envoyé aucune.
    let gina_reading = Client::connect_synced(&f.url_for(reading(gina, "Gi\u{7}na"))).await;
    let three = roster(&[
        (alice, "Alice", true),
        (bob, "Bob", true),
        (gina, "Gina", false),
    ]);
    assert_eq!(nth(&gina_reading, 0).await, three);
    assert_eq!(nth(&alice_tab, 2).await, three);
    assert_eq!(nth(&bob_1, 1).await, three);
    assert_eq!(nth(&bob_2, 1).await, three);

    // Une connexion sans compte : elle reçoit la liste, n'y figure pas, et ne la change pas.
    let nobody = f.client().await;
    assert_eq!(nth(&nobody, 0).await, three);

    // Gina ouvre un onglet où elle peut écrire : sa ligne change, et chacun le reçoit, elle
    // comprise, et la connexion sans compte aussi.
    let gina_writing = Client::connect_synced(&f.url_for(of(gina, "Gi\u{7}na"))).await;
    let gina_writes = roster(&[
        (alice, "Alice", true),
        (bob, "Bob", true),
        (gina, "Gina", true),
    ]);
    assert_eq!(nth(&gina_writing, 0).await, gina_writes);
    for (client, index) in [
        (&alice_tab, 3),
        (&bob_1, 2),
        (&bob_2, 2),
        (&gina_reading, 1),
        (&nobody, 1),
    ] {
        assert_eq!(nth(client, index).await, gina_writes);
    }
    // Il se ferme : elle ne peut plus écrire, chacun le reçoit.
    drop(gina_writing);
    assert_eq!(nth(&alice_tab, 4).await, three);
    assert_eq!(nth(&gina_reading, 2).await, three);

    // Bob ferme un onglet : la liste ne change pas. Puis l'autre : il n'y figure plus. Les
    // autres ne reçoivent que cette trame-là.
    drop(bob_1);
    drop(bob_2);
    let without_bob = roster(&[(alice, "Alice", true), (gina, "Gina", false)]);
    assert_eq!(nth(&alice_tab, 5).await, without_bob);
    assert_eq!(nth(&gina_reading, 3).await, without_bob);
    assert_eq!(nth(&nobody, 3).await, without_bob);

    // La connexion sans compte part : rien ne change, personne ne reçoit rien. Alice part : la
    // lectrice reste seule nommée.
    drop(nobody);
    drop(alice_tab);
    assert_eq!(
        nth(&gina_reading, 4).await,
        roster(&[(gina, "Gina", false)])
    );
}

/// Une connexion fermée à son inscription (droits changés depuis leur lecture, Ruling R71) n'a
/// rien reçu : elle n'est pas non plus nommée aux autres, même un instant.
#[tokio::test]
async fn connection_closed_at_its_registration_is_never_named() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (alice, mallory, carol) = three_accounts(&f).await;
    let alice_tab = Client::connect_synced(&f.url_for(of(alice, "Alice"))).await;
    assert_eq!(nth(&alice_tab, 0).await, roster(&[(alice, "Alice", true)]));

    // Ce que la route a lu, avant une écriture de l'espace qui passe avant l'inscription.
    let space = f.store.get_node(f.page).await.unwrap().unwrap().space_id;
    let seen = f.store.space_rev(space).await.unwrap().unwrap();
    f.store
        .create_node(NewNode {
            space_id: space,
            parent_id: None,
            after: None,
            kind: NodeKind::Page,
            title: "Ailleurs".into(),
            created_by: None,
            grants: Vec::new(),
            doc: None,
        })
        .await
        .unwrap();
    let stale = ConnInfo {
        space: Some((space, seen)),
        ..of(mallory, "Mallory")
    };
    let closed = Client::connect(&f.url_for(stale)).await.unwrap();
    assert!(closed.wait_until(WAIT, |c| c.is_closed()).await);
    assert!(closed.page_events().is_empty(), "elle a reçu une liste");

    // Carol arrive ensuite : la trame qu'Alice reçoit ne nomme qu'elles deux, et aucune avant
    // elle n'a nommé Mallory.
    let _carol_tab = Client::connect_synced(&f.url_for(of(carol, "Carol"))).await;
    assert_eq!(
        nth(&alice_tab, 1).await,
        roster(&[(alice, "Alice", true), (carol, "Carol", true)])
    );
}

/// Les trames `comments` et `versions` parviennent à toutes les connexions de la page, lecteurs
/// et connexions sans compte compris, et à celles d'aucune autre page. Sans acteur vivant, rien
/// n'est fait : la page n'est pas chargée pour autant.
#[tokio::test]
async fn page_event_reaches_every_connection_of_the_page() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (alice, bob, gina) = three_accounts(&f).await;
    let elsewhere_page = new_page(&f.store).await;
    let on_page = [
        Client::connect_synced(&f.url_for(of(alice, "Alice"))).await,
        Client::connect_synced(&f.url_for(reading(gina, "Gina"))).await,
        f.client().await,
    ];
    let elsewhere = Client::connect_synced(&f.server.url_for(elsewhere_page, of(bob, "Bob"))).await;
    // Les trames autres que la liste des présents.
    let events = |client: &Client| -> Vec<Value> {
        frames(client)
            .into_iter()
            .filter(|frame| frame["type"] != "roster")
            .collect()
    };
    let became = async |client: &Client, expected: &[Value]| {
        client
            .wait_until(WAIT, |c| events(c).as_slice() == expected)
            .await
    };
    let both = [json!({ "type": "comments" }), json!({ "type": "versions" })];
    let (comments, versions) = (&both[..1], &both[1..]);

    f.hub.page_event(f.page, PageEvent::Comments);
    for client in &on_page {
        assert!(became(client, comments).await, "{:?}", client.page_events());
    }
    f.hub.page_event(elsewhere_page, PageEvent::Versions);
    assert!(became(&elsewhere, versions).await);
    // Une seconde trame pour la page : elle arrive seule, derrière la première ; celle de
    // l'autre page n'est pas passée entre les deux.
    f.hub.page_event(f.page, PageEvent::Versions);
    for client in &on_page {
        assert!(became(client, &both).await, "{:?}", client.page_events());
    }
    assert_eq!(events(&elsewhere), versions);

    // Une page que personne n'a ouverte, une page qui n'existe pas : rien, et aucun acteur
    // n'est lancé.
    let closed_page = new_page(&f.store).await;
    let before = live(&f.hub).await;
    assert!(!before.contains(&closed_page));
    f.hub.page_event(closed_page, PageEvent::Comments);
    f.hub.page_event(Uuid::now_v7(), PageEvent::Versions);
    assert_eq!(live(&f.hub).await, before);
}

/// Une trame de page vient du serveur seul. Reçue d'un client, elle rend tout son message
/// illisible : la connexion est fermée, et rien de ce message n'est traité ni relayé, pas même
/// la mise à jour du document qui la précède.
#[tokio::test]
async fn frame_100_from_a_client_is_illegible() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Bonjour\n").await;
    let (alice, mallory, _) = three_accounts(&f).await;
    let observer = Client::connect_synced(&f.url_for(of(alice, "Alice"))).await;
    let alone = roster(&[(alice, "Alice", true)]);
    assert_eq!(nth(&observer, 0).await, alone);
    let rows = row_counts(&f.db).await;
    for forged in [
        json!({ "type": "comments" }),
        json!({ "type": "roster", "users": [{ "id": alice.to_string(), "name": "Faux", "canWrite": false }] }),
    ] {
        // La trame de son arrivée, qui les nomme toutes deux, est la suivante d'Alice.
        let arrival = observer.page_events().len();
        let sender = Client::connect_synced(&f.url_for(of(mallory, "Mallory"))).await;
        assert_eq!(
            nth(&observer, arrival).await,
            roster(&[(alice, "Alice", true), (mallory, "Mallory", true)])
        );
        // Une frappe en règle, puis la trame, dans le même message WebSocket.
        let doc = detached(&sender);
        let paragraph = first_content(&doc);
        let typed = {
            let mut txn = doc.transact_mut();
            paragraph.insert(&mut txn, 0, XmlTextPrelim::new("Perdu "));
            txn.encode_update_v1()
        };
        let mut message = Message::Sync(SyncMessage::Update(typed)).encode_v1();
        message.extend(
            Message::Custom(MESSAGE_PAGE_EVENT, forged.to_string().into_bytes()).encode_v1(),
        );
        sender.send_raw(message);
        assert!(
            sender.wait_until(WAIT, |c| c.is_closed()).await,
            "la connexion qui a envoyé une trame de page est encore servie"
        );
        // Son départ change la liste : la trame qui le dit est la seule qu'Alice reçoit
        // ensuite, et rien du message ne l'a précédée.
        assert_eq!(nth(&observer, arrival + 1).await, alone, "{forged}");
        assert!(
            !frames(&observer).contains(&forged),
            "la trame d'un client est relayée : {forged}"
        );
    }
    assert_eq!(text(&observer.blocks()), "Bonjour");
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "Bonjour");
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
    assert!(!observer.is_closed());
}
