//! Présence (awareness) : à qui appartient un `ClientID` (Ruling R101).
//!
//! Un état de présence (un nom, un curseur) est annoncé sous le `ClientID` du document Yjs de
//! son onglet. Le serveur ne le vérifie pas (un client peut annoncer le nom qu'il veut pour
//! lui-même, Ruling R52), mais il le range sous le compte de la connexion : un `ClientID`
//! appartient au compte de la première connexion vivante qui l'annonce, jusqu'à ce qu'elle
//! parte. Une entrée qui vise le `ClientID` d'un autre compte est ignorée, avant d'être
//! appliquée (les états portent une horloge : appliquée puis défaite, elle aurait déjà dépassé
//! celle du titulaire), sans fermer la connexion : un client y-websocket honnête renvoie au
//! serveur tous les états qu'il reçoit des autres.
//!
//! Les messages d'un attaquant sont écrits à la main (`awareness_message`, du testkit) : le
//! client de test, comme un navigateur, n'annonce que le sien.

mod common;

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

/// Une connexion du compte `account` en lecture seule : un simple lecteur, un invité.
fn reader(account: Uuid) -> ConnInfo {
    ConnInfo {
        can_write: false,
        ..of(account)
    }
}

/// L'état de présence de quelqu'un qui s'appelle `name`.
fn named(name: &str) -> String {
    format!(r#"{{"user":{{"name":"{name}"}}}}"#)
}

/// L'état du client `client`, tel que `seen_by` l'a reçu du serveur.
fn state_of(seen_by: &Client, client: u64) -> Option<String> {
    seen_by.awareness_states().get(&client).cloned()
}

/// Attend que `seen_by` voie cet état pour `client` (`None` : plus d'état).
async fn sees(seen_by: &Client, client: u64, state: Option<&str>) -> bool {
    seen_by
        .wait_until(WAIT, |c| state_of(c, client).as_deref() == state)
        .await
}

/// Le serveur a traité tout ce que `client` a envoyé, et ne l'a pas fermé. (Contrairement à
/// `processed`, ne panique pas si la connexion est fermée : rend faux.)
async fn still_served(client: &Client) -> bool {
    let before = client.step2_count();
    client.resync();
    client
        .wait_until(WAIT, |c| c.is_closed() || c.step2_count() > before)
        .await;
    !client.is_closed() && client.step2_count() > before
}

/// Un témoin : un compte de plus sur la page, dont l'état sert de barrière. La diffusion d'une
/// page est ordonnée : quand un client voit la marque du témoin, tout ce que le serveur a
/// relayé avant elle lui est déjà parvenu.
struct Witness {
    client: Client,
    marks: u32,
}

impl Witness {
    async fn join(f: &Fixture) -> Witness {
        let account = new_user(&f.db).await;
        Witness {
            client: Client::connect_synced(&f.url_for(of(account))).await,
            marks: 0,
        }
    }

    /// Le serveur traite d'abord tout ce que `sender` a envoyé ; puis le témoin pose une marque,
    /// que chacun des `observers` attend. Ce que `sender` aurait fait relayer est alors arrivé
    /// chez eux.
    async fn after(&mut self, sender: &Client, observers: &[&Client]) {
        assert!(
            still_served(sender).await,
            "la connexion de l'expéditeur a été fermée"
        );
        self.marks += 1;
        let mark = format!(r#"{{"marque":{}}}"#, self.marks);
        self.client.set_awareness(&mark);
        for observer in observers {
            assert!(
                sees(observer, self.client.client_id(), Some(&mark)).await,
                "la marque du témoin n'est pas arrivée"
            );
        }
    }
}

/// Une lectrice ne remplace pas l'état de présence d'une autre : ni chez ceux qui sont là, ni
/// pour qui arrive ensuite, ni en prenant de l'avance sur son horloge. Le reste de son message
/// (son propre état) passe, et sa connexion reste ouverte.
#[tokio::test]
async fn presence_of_another_account_cannot_be_overwritten() {
    let f = Fixture::new(SyncConfig::default()).await;
    let alice = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    // Mallory ne fait que lire la page.
    let mallory = Client::connect_synced(&f.url_for(reader(new_user(&f.db).await))).await;
    let mut witness = Witness::join(&f).await;
    let (alice_id, mallory_id) = (alice.client_id(), mallory.client_id());
    let truth = named("Alice");
    alice.set_awareness(&truth);
    for seen_by in [&carol, &mallory] {
        assert!(sees(seen_by, alice_id, Some(&truth)).await);
    }

    // L'état d'Alice remplacé, avec une horloge que la sienne ne rattrapera pas ; dans le même
    // message, l'état de Mallory elle-même.
    let (fake, own) = (named("Alice (faux)"), named("Mallory"));
    mallory.send_raw(awareness_message(&[
        (alice_id, 1_000, &fake),
        (mallory_id, 1, &own),
    ]));
    witness.after(&mallory, &[&carol]).await;
    // Le reste du message est passé : son propre état.
    assert_eq!(state_of(&carol, mallory_id), Some(own));
    // Celui d'Alice n'a pas bougé.
    assert_eq!(state_of(&carol, alice_id).as_deref(), Some(truth.as_str()));

    // Seul dans son message, et d'une connexion sans compte : pas davantage.
    let nobody = f.client().await;
    for (sender, clock) in [(&mallory, 2_000), (&nobody, 3_000)] {
        sender.send_raw(awareness_message(&[(alice_id, clock, &fake)]));
        witness.after(sender, &[&carol]).await;
        assert_eq!(state_of(&carol, alice_id).as_deref(), Some(truth.as_str()));
    }
    // Le serveur n'a rien gardé du faux : qui arrive reçoit l'état d'Alice.
    let newcomer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    assert!(sees(&newcomer, alice_id, Some(&truth)).await);
    // Et son horloge n'a pas été dépassée : son état suivant passe, partout.
    let next = named("Alice, plus loin");
    alice.set_awareness(&next);
    for seen_by in [&carol, &newcomer, &mallory] {
        assert!(
            sees(seen_by, alice_id, Some(&next)).await,
            "l'état suivant d'Alice n'est pas arrivé"
        );
    }
    assert!(!mallory.is_closed() && !nobody.is_closed());
}

/// Une lectrice n'efface pas non plus l'état de présence d'une autre : ni par un retrait à la
/// même horloge (celui qu'un client envoie quand il croit quelqu'un parti), ni par un retrait
/// en avance sur elle.
#[tokio::test]
async fn presence_of_another_account_cannot_be_removed() {
    let f = Fixture::new(SyncConfig::default()).await;
    let alice = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory = Client::connect_synced(&f.url_for(reader(new_user(&f.db).await))).await;
    let mut witness = Witness::join(&f).await;
    let alice_id = alice.client_id();
    let truth = named("Alice");
    // Son premier état : horloge 1.
    alice.set_awareness(&truth);
    for seen_by in [&carol, &mallory] {
        assert!(sees(seen_by, alice_id, Some(&truth)).await);
    }

    for clock in [1, 1_000] {
        mallory.send_raw(awareness_message(&[(alice_id, clock, "null")]));
        witness.after(&mallory, &[&carol]).await;
        assert_eq!(
            state_of(&carol, alice_id).as_deref(),
            Some(truth.as_str()),
            "retrait à l'horloge {clock}"
        );
        let newcomer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
        assert!(
            sees(&newcomer, alice_id, Some(&truth)).await,
            "retrait à l'horloge {clock} : absente pour qui arrive"
        );
    }
    // Son état suivant passe : l'horloge du serveur est restée la sienne.
    let next = named("Alice, plus loin");
    alice.set_awareness(&next);
    assert!(sees(&carol, alice_id, Some(&next)).await);
    assert!(!mallory.is_closed());

    // Alice, elle, retire son propre état, et il part chez tous.
    alice.send_raw(awareness_message(&[(alice_id, 3, "null")]));
    assert!(sees(&carol, alice_id, None).await);
}

/// Un `ClientID` suit son compte d'une connexion à l'autre : après une coupure, le même
/// document revient par une connexion neuve du même compte et reprend son `ClientID` ; pendant
/// qu'une connexion du compte le tient, une autre du même compte peut l'annoncer aussi (la
/// nouvelle arrive avant que l'ancienne soit fermée), et aucun autre compte ne le prend. Quand
/// la dernière connexion qui le tenait part, son état est retiré et il est libre.
#[tokio::test]
async fn same_account_takes_its_client_id_back_after_reconnecting() {
    let f = Fixture::new(SyncConfig::default()).await;
    let alice = new_user(&f.db).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory = Client::connect_synced(&f.url_for(reader(new_user(&f.db).await))).await;
    let mut witness = Witness::join(&f).await;

    let first = Client::connect_synced(&f.url_for(of(alice))).await;
    let tab = first.client_id();
    first.set_awareness(&named("Alice"));
    assert!(sees(&carol, tab, Some(&named("Alice"))).await);
    // Coupure : le serveur retire son état (son horloge passe à 2).
    drop(first);
    assert!(sees(&carol, tab, None).await, "état gardé après le départ");
    // Pendant la coupure, un retrait en avance sur son horloge, pour ce client que personne ne
    // tient : ignoré. Appliqué, il laisserait l'horloge du client à 1 000, et l'état qu'Alice
    // annonce en revenant serait ignoré de tous.
    mallory.send_raw(awareness_message(&[(tab, 1_000, "null")]));
    witness.after(&mallory, &[&carol]).await;

    // Le même onglet revient, par une connexion neuve du même compte.
    let second = Client::connect_synced(&f.url_for(of(alice))).await;
    let back = named("Alice, revenue");
    second.send_raw(awareness_message(&[(tab, 3, &back)]));
    assert!(
        sees(&carol, tab, Some(&back)).await,
        "le compte n'a pas repris son client"
    );
    // Cette connexion annonce aussi le client de son propre document : son départ se verra.
    second.set_awareness(&named("Alice, autre onglet"));
    let second_id = second.client_id();
    assert!(sees(&carol, second_id, Some(&named("Alice, autre onglet"))).await);

    // Une troisième connexion du compte l'annonce pendant que la seconde vit encore.
    let third = Client::connect_synced(&f.url_for(of(alice))).await;
    let again = named("Alice, encore");
    third.send_raw(awareness_message(&[(tab, 4, &again)]));
    assert!(sees(&carol, tab, Some(&again)).await);
    // La seconde part : son propre client s'en va, celui que la troisième a repris reste.
    drop(second);
    assert!(sees(&carol, second_id, None).await);
    assert_eq!(state_of(&carol, tab).as_deref(), Some(again.as_str()));

    // Tant qu'une connexion d'Alice le tient, un autre compte ne le prend pas.
    let squat = named("Mallory, à la place d'Alice");
    mallory.send_raw(awareness_message(&[(tab, 50, &squat)]));
    witness.after(&mallory, &[&carol]).await;
    assert_eq!(state_of(&carol, tab).as_deref(), Some(again.as_str()));

    // La dernière connexion qui le tenait part : son état est retiré, et il est libre.
    drop(third);
    assert!(sees(&carol, tab, None).await);
    mallory.send_raw(awareness_message(&[(tab, 60, &squat)]));
    assert!(
        sees(&carol, tab, Some(&squat)).await,
        "client non libéré au départ de la connexion qui le tenait"
    );
}

/// Un client y-websocket renvoie au serveur chaque changement de présence qu'il reçoit des
/// autres, tel quel. Ces entrées sont ignorées en silence : la connexion n'est pas fermée, même
/// avec plus de présents qu'une connexion ne peut tenir de clients (huit), et renvoyer le départ
/// de quelqu'un ne prend pas son `ClientID`.
#[tokio::test]
async fn echoed_states_of_others_do_not_close_the_connection() {
    /// Plus que ce qu'une connexion peut tenir de clients d'awareness.
    const OTHERS: usize = 9;
    let f = Fixture::new(SyncConfig::default()).await;
    let mut others = Vec::new();
    for i in 0..OTHERS {
        let account = new_user(&f.db).await;
        let client = Client::connect_synced(&f.url_for(of(account))).await;
        let state = named(&format!("Présent {i}"));
        client.set_awareness(&state);
        others.push((account, client, state));
    }
    let bob = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    for (_, client, state) in &others {
        assert!(sees(&bob, client.client_id(), Some(state)).await);
    }

    // À son arrivée, il renvoie les neuf états reçus, à leur horloge, avec le sien.
    let own = named("Bob");
    let mut echo: Vec<(u64, u32, &str)> = others
        .iter()
        .map(|(_, client, state)| (client.client_id(), 1, state.as_str()))
        .collect();
    echo.push((bob.client_id(), 1, &own));
    bob.send_raw(awareness_message(&echo));
    assert!(
        still_served(&bob).await,
        "fermé pour avoir renvoyé des états"
    );
    let (_, first, _) = &others[0];
    assert!(sees(first, bob.client_id(), Some(&own)).await);

    // Puis chaque changement, un par un, autant de fois qu'il y en a.
    for round in 2..5u32 {
        for (i, (_, client, _)) in others.iter().enumerate() {
            let state = named(&format!("Présent {i}, tour {round}"));
            client.set_awareness(&state);
            assert!(sees(&bob, client.client_id(), Some(&state)).await);
            bob.send_raw(awareness_message(&[(client.client_id(), round, &state)]));
        }
        assert!(still_served(&bob).await, "fermé au tour {round}");
    }

    // Quelqu'un s'en va : Bob renvoie ce retrait, comme il renvoie le reste.
    let (gone_account, gone, _) = others.pop().expect("neuf présents");
    let gone_id = gone.client_id();
    drop(gone);
    assert!(sees(&bob, gone_id, None).await);
    // Quatre états annoncés, puis le retrait du serveur : horloge 5.
    bob.send_raw(awareness_message(&[(gone_id, 5, "null")]));
    assert!(
        still_served(&bob).await,
        "fermé pour avoir renvoyé un départ"
    );
    // Ce renvoi ne lui a pas donné le client : son titulaire revient et le reprend.
    let back = Client::connect_synced(&f.url_for(of(gone_account))).await;
    let state = named("De retour");
    back.send_raw(awareness_message(&[(gone_id, 6, &state)]));
    assert!(
        sees(&bob, gone_id, Some(&state)).await,
        "le renvoi d'un départ a pris le client de celui qui est parti"
    );
    assert!(!bob.is_closed());
}

/// Les connexions sans compte (celles des tests du hub) ne sont pas bridées entre elles : l'une
/// écrit l'état annoncé par l'autre, comme avant. Mais un compte ne leur prend rien, et elles ne
/// prennent rien à un compte.
#[tokio::test]
async fn connections_without_an_account_share_their_client_ids() {
    let f = Fixture::new(SyncConfig::default()).await;
    let first = f.client().await;
    let second = f.client().await;
    let observer = f.client().await;
    let mut witness = Witness::join(&f).await;
    let id = first.client_id();
    first.set_awareness(&named("Sans compte"));
    assert!(sees(&observer, id, Some(&named("Sans compte"))).await);

    let other = named("Par une autre connexion sans compte");
    second.send_raw(awareness_message(&[(id, 10, &other)]));
    assert!(sees(&observer, id, Some(&other)).await);
    // Un compte n'y touche pas.
    let account = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    account.send_raw(awareness_message(&[(id, 100, &named("Par un compte"))]));
    witness.after(&account, &[&observer]).await;
    assert_eq!(state_of(&observer, id), Some(other));
}
