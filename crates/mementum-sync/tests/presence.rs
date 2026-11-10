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
use yrs::sync::{Message, SyncMessage};
use yrs::updates::encoder::Encode;
use yrs::{Transact, XmlFragment, XmlTextPrelim};

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
        let mark = named(&format!("Témoin {}", self.marks));
        self.client.set_awareness(&mark);
        for observer in observers {
            assert!(
                sees(observer, self.client.client_id(), Some(&mark)).await,
                "la marque du témoin n'est pas arrivée"
            );
        }
    }
}

/// Une personne ne remplace pas l'état de présence d'une autre : ni chez ceux qui sont là, ni
/// pour qui arrive ensuite, ni en prenant de l'avance sur son horloge. Le reste de son message
/// (son propre état) passe, et sa connexion reste ouverte.
#[tokio::test]
async fn presence_of_another_account_cannot_be_overwritten() {
    let f = Fixture::new(SyncConfig::default()).await;
    let alice = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    // Mallory écrit dans la page comme les autres : c'est son compte qui la retient. En lecture
    // seule, rien de ce qu'elle annonce ne serait gardé (Ruling R118).
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
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

/// Une personne n'efface pas non plus l'état de présence d'une autre : ni par un retrait à la
/// même horloge (celui qu'un client envoie quand il croit quelqu'un parti), ni par un retrait
/// en avance sur elle.
#[tokio::test]
async fn presence_of_another_account_cannot_be_removed() {
    let f = Fixture::new(SyncConfig::default()).await;
    let alice = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
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
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
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

// ---------------------------------------------------------------------------------------------
// Un client libre ne se prend que par une entrée plus récente (Ruling R112).
//
// Un client y-websocket renvoie au serveur chaque état qu'il reçoit, et ce renvoi met un
// aller-retour à arriver. Si le titulaire est parti entre-temps, son client est libre, et son
// horloge a avancé d'un cran (le retrait du serveur). Le renvoi, à l'ancienne horloge, n'est
// pas appliqué : il ne doit pas non plus donner le client à qui le renvoie. Sinon quelqu'un
// ferme les connexions des autres (huit clients inventés, puis il part : chacun les « prend »
// en les renvoyant, et dépasse la limite), et quelqu'un qui revient reste invisible.

/// Quelqu'un annonce huit clients inventés dans un message, puis coupe sa connexion. Le renvoi
/// de ces huit états par une connexion honnête, arrivé après le départ, ne la ferme pas, et
/// elle ne tient toujours que son propre client : il lui reste sept places. Aux horloges
/// ordinaires, puis au sommet : son départ porte alors l'horloge de ses clients à la
/// dernière, sans qu'elle reboucle.
#[tokio::test]
async fn late_echo_of_invented_clients_never_closes_an_honest_connection() {
    let f = Fixture::new(SyncConfig::default()).await;
    for (round, clock) in [1, u32::MAX - 1].into_iter().enumerate() {
        let alice = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
        alice.set_awareness(&named("Alice"));
        let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
        let base = 900_000 + 100 * round as u64;
        let fakes: Vec<u64> = (1..=8).map(|i| base + i).collect();
        let ghost = named("Fantôme");
        let entries: Vec<(u64, u32, &str)> = fakes
            .iter()
            .map(|id| (*id, clock, ghost.as_str()))
            .collect();
        mallory.send_raw(awareness_message(&entries));
        for id in &fakes {
            assert!(sees(&alice, *id, Some(&ghost)).await, "horloge {clock}");
        }
        drop(mallory);
        for id in &fakes {
            assert!(sees(&alice, *id, None).await, "horloge {clock}");
        }

        // Ce que y-websocket renvoie de ces huit états, arrivé après son départ.
        alice.send_raw(awareness_message(&entries));
        assert!(
            still_served(&alice).await,
            "horloge {clock} : fermée pour avoir renvoyé les états d'un client parti"
        );
        // Elle ne tient que le sien : sept clients de plus passent encore. Un seul des huit
        // renvoyés, pris, et elle dépasserait ici la limite.
        let more: Vec<(u64, u32, &str)> = (1..=7)
            .map(|i| (base + 50 + i, 1, ghost.as_str()))
            .collect();
        alice.send_raw(awareness_message(&more));
        assert!(
            still_served(&alice).await,
            "horloge {clock} : le renvoi lui a pris des places"
        );
    }
}

/// Le même mécanisme entre clients honnêtes : Bob annonce un état, puis sa connexion tombe ;
/// le renvoi d'Alice arrive après. Il ne prend pas le client de Bob : revenu avec le même
/// document, Bob est vu de tous.
#[tokio::test]
async fn late_echo_does_not_take_the_client_of_someone_who_left() {
    let f = Fixture::new(SyncConfig::default()).await;
    let bob_account = new_user(&f.db).await;
    let alice = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let bob = Client::connect_synced(&f.url_for(of(bob_account))).await;
    let tab = bob.client_id();
    bob.set_awareness(&named("Bob"));
    for seen_by in [&alice, &carol] {
        assert!(sees(seen_by, tab, Some(&named("Bob"))).await);
    }
    drop(bob);
    for seen_by in [&alice, &carol] {
        assert!(sees(seen_by, tab, None).await);
    }

    // Le renvoi tardif d'Alice : l'état de Bob, à l'horloge où elle l'avait reçu. Puis à
    // l'horloge de son retrait (2) : pas plus récent non plus, il faut la dépasser.
    alice.send_raw(awareness_message(&[(tab, 1, &named("Bob"))]));
    alice.send_raw(awareness_message(&[(tab, 2, &named("Bob"))]));
    assert!(still_served(&alice).await);
    // Il n'a rien fait voir non plus.
    assert_eq!(state_of(&carol, tab), None);

    // Bob revient, avec le même client : il est vu, d'Alice comme de Carol.
    let back = Client::connect_synced(&f.url_for(of(bob_account))).await;
    let returned = named("Bob, revenu");
    back.send_raw(awareness_message(&[(tab, 10, &returned)]));
    for (seen_by, who) in [(&carol, "Carol"), (&alice, "Alice")] {
        assert!(
            sees(seen_by, tab, Some(&returned)).await,
            "{who} ne voit pas Bob revenu : le renvoi tardif avait pris son client"
        );
    }
}

/// La règle ne vaut que pour un client libre. Un onglet qui revient avant que son ancienne
/// connexion soit fermée renvoie son état à la même horloge : la connexion neuve tient le
/// client avec l'ancienne, et l'état reste quand l'ancienne part.
#[tokio::test]
async fn returning_tab_keeps_its_client_at_the_same_clock() {
    let f = Fixture::new(SyncConfig::default()).await;
    let account = new_user(&f.db).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let old = Client::connect_synced(&f.url_for(of(account))).await;
    let old_id = old.client_id();
    let state = named("Alice");
    // Le client de l'onglet, et celui de l'ancienne connexion elle-même, dont la disparition
    // dira que son départ est traité.
    old.send_raw(awareness_message(&[
        (SPARE_CLIENT, 5, &state),
        (old_id, 1, &named("Ancienne connexion")),
    ]));
    assert!(sees(&carol, SPARE_CLIENT, Some(&state)).await);

    // La connexion neuve du même compte : le même état, à la même horloge.
    let new = Client::connect_synced(&f.url_for(of(account))).await;
    new.send_raw(awareness_message(&[(SPARE_CLIENT, 5, &state)]));
    assert!(still_served(&new).await);
    drop(old);
    assert!(sees(&carol, old_id, None).await, "départ non traité");
    assert_eq!(
        state_of(&carol, SPARE_CLIENT).as_deref(),
        Some(state.as_str()),
        "l'état de l'onglet est parti avec son ancienne connexion"
    );
    // La neuve partie à son tour, il est retiré.
    drop(new);
    assert!(sees(&carol, SPARE_CLIENT, None).await);
}

// ---------------------------------------------------------------------------------------------
// La dernière horloge (Ruling R108).
//
// Un état de présence porte une horloge par client, que yrs incrémente quand il retire l'état
// d'un client parti (`Awareness::remove_state`), sans regarder si elle peut encore l'être. À
// `u32::MAX`, l'addition déborde : l'acteur de la page panique dans un build de développement,
// et toutes ses connexions se ferment. Une entrée à cette horloge est donc ignorée, et un départ
// ne compte plus rien pour un client qui y est déjà.

/// Un `ClientID` que personne n'a annoncé : celui qu'un message écrit à la main vise.
const SPARE_CLIENT: u64 = 4_242;

/// Une entrée de présence à la dernière horloge est ignorée : elle n'apparaît chez personne, ni
/// tout de suite, ni pour qui arrive ; elle ne donne pas son client à qui l'envoie ; le reste du
/// message passe, et la connexion reste ouverte. De même pour un retrait à cette horloge.
#[tokio::test]
async fn presence_at_the_last_clock_is_ignored() {
    let f = Fixture::new(SyncConfig::default()).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mut witness = Witness::join(&f).await;
    let mallory_id = mallory.client_id();

    // Dans le même message : un état à la dernière horloge, et l'état de Mallory elle-même.
    let (last, own) = (named("À la dernière horloge"), named("Mallory"));
    mallory.send_raw(awareness_message(&[
        (SPARE_CLIENT, u32::MAX, &last),
        (mallory_id, 1, &own),
    ]));
    witness.after(&mallory, &[&carol]).await;
    assert_eq!(state_of(&carol, mallory_id).as_deref(), Some(own.as_str()));
    assert_eq!(state_of(&carol, SPARE_CLIENT), None);
    let newcomer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    assert!(sees(&newcomer, mallory_id, Some(&own)).await);
    assert_eq!(state_of(&newcomer, SPARE_CLIENT), None);

    // Un retrait à la dernière horloge, de son propre état : ignoré lui aussi.
    mallory.send_raw(awareness_message(&[(mallory_id, u32::MAX, "null")]));
    witness.after(&mallory, &[&carol]).await;
    assert_eq!(state_of(&carol, mallory_id).as_deref(), Some(own.as_str()));

    // L'entrée ignorée n'a pas donné le client à Mallory : un autre compte l'annonce.
    let dave = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let taken = named("Dave");
    dave.send_raw(awareness_message(&[(SPARE_CLIENT, 1, &taken)]));
    assert!(
        sees(&carol, SPARE_CLIENT, Some(&taken)).await,
        "l'entrée ignorée a donné le client à qui l'a envoyée"
    );
    assert!(!mallory.is_closed());
}

/// Ce que `carol` voit quand la connexion d'un client part, puis ce qu'elle reçoit encore :
/// le départ est traité (l'état que le client annonçait sous `own` disparaît), la connexion de
/// `carol` est toujours ouverte, et ce que `writer` tape ensuite lui parvient.
async fn page_survives_the_departure(
    carol: &Client,
    writer: &Client,
    block: &str,
    own: u64,
    typed: &str,
) {
    assert!(
        carol
            .wait_until(WAIT, |c| c.is_closed() || state_of(c, own).is_none())
            .await,
        "le départ n'a pas été traité"
    );
    assert!(
        !carol.is_closed(),
        "la page a fermé ses connexions au départ du client"
    );
    writer.type_text(block, typed);
    assert!(
        carol
            .wait_until(WAIT, |c| c.is_closed() || text(&c.blocks()).contains(typed))
            .await
    );
    assert!(
        !carol.is_closed() && text(&carol.blocks()).contains(typed),
        "la page ne sert plus ses connexions"
    );
}

/// Le scénario entier : quelqu'un annonce un état à la dernière horloge, puis s'en va. La page
/// reste en vie : une autre connexion est toujours ouverte, et reçoit ce qu'on écrit ensuite.
/// Avant la garde, le départ faisait paniquer l'acteur de la page.
#[tokio::test]
async fn a_departure_after_the_last_clock_leaves_the_page_alive() {
    let f = Fixture::new(SyncConfig::default()).await;
    let block = f.ids().await[0].clone();
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let writer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory_id = mallory.client_id();

    // Son propre état, à une horloge ordinaire : sa disparition dira que son départ est traité.
    let own = named("Mallory");
    mallory.send_raw(awareness_message(&[
        (SPARE_CLIENT, u32::MAX, &named("À la dernière horloge")),
        (mallory_id, 1, &own),
    ]));
    assert!(sees(&carol, mallory_id, Some(&own)).await);
    assert!(still_served(&mallory).await);

    drop(mallory);
    page_survives_the_departure(&carol, &writer, &block, mallory_id, "après son départ").await;
}

/// La dernière horloge s'atteint aussi par un départ : un état annoncé juste en dessous, et le
/// retrait qu'en fait le serveur l'y porte. Le client y reste, pour de bon : qui l'annonce de
/// nouveau ne le prend pas (son entrée n'est pas plus récente, Ruling R112), et son départ ne
/// fait plus rien compter. Quand une telle entrée donnait encore le client, ce second départ
/// faisait paniquer l'acteur dans un build de développement ; en release, l'horloge du client
/// rebouclait à zéro, et tout redevenait « plus récent » qu'elle.
#[tokio::test]
async fn a_departure_just_below_the_last_clock_leaves_the_page_alive() {
    let f = Fixture::new(SyncConfig::default()).await;
    let block = f.ids().await[0].clone();
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let writer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let account = new_user(&f.db).await;

    // Premier passage : un état juste sous la dernière horloge, que tout le monde reçoit.
    let first = Client::connect_synced(&f.url_for(of(account))).await;
    let below = named("Juste en dessous");
    first.send_raw(awareness_message(&[
        (SPARE_CLIENT, u32::MAX - 1, &below),
        (first.client_id(), 1, &named("Mallory")),
    ]));
    assert!(sees(&carol, SPARE_CLIENT, Some(&below)).await);
    let first_id = first.client_id();
    drop(first);
    // Son départ retire l'état : l'horloge du client est maintenant la dernière.
    assert!(sees(&carol, SPARE_CLIENT, None).await);
    page_survives_the_departure(&carol, &writer, &block, first_id, "un").await;

    // Second passage : le même client, annoncé de nouveau (sans effet : son horloge ne peut
    // plus monter), puis un second départ.
    let second = Client::connect_synced(&f.url_for(of(account))).await;
    let second_id = second.client_id();
    second.send_raw(awareness_message(&[
        (SPARE_CLIENT, 7, &named("De retour")),
        (second_id, 1, &named("Mallory, encore")),
    ]));
    assert!(sees(&carol, second_id, Some(&named("Mallory, encore"))).await);
    assert!(still_served(&second).await);
    drop(second);
    page_survives_the_departure(&carol, &writer, &block, second_id, "deux").await;
    assert_eq!(state_of(&carol, SPARE_CLIENT), None);

    // L'horloge du client n'a pas rebouclé : une annonce à une horloge ordinaire, venue d'un
    // autre compte, n'est toujours pas plus récente qu'elle, et personne ne la voit. (C'est
    // ce que ce second départ changerait en release, sans panique, si l'horloge repartait
    // de zéro.)
    let mut witness = Witness::join(&f).await;
    writer.send_raw(awareness_message(&[(
        SPARE_CLIENT,
        9,
        &named("Après le tour"),
    )]));
    witness.after(&writer, &[&carol]).await;
    assert_eq!(state_of(&carol, SPARE_CLIENT), None);
    let newcomer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    assert!(
        sees(
            &newcomer,
            witness.client.client_id(),
            Some(&named("Témoin 1"))
        )
        .await
    );
    assert_eq!(state_of(&newcomer, SPARE_CLIENT), None);
}

// ---------------------------------------------------------------------------------------------
// Pas plus de 512 entrées de présence par message (Ruling R113).
//
// Le serveur regarde chaque entrée d'un message de présence contre chaque connexion de la page.
// La borne en octets d'un message laisse passer des millions d'entrées, qui occuperaient l'acteur
// de la page aux dépens de tous. Au-delà de 512 entrées, la présence d'un message est donc
// ignorée : aucune entrée n'est appliquée ni relayée, aucun client n'est pris.
//
// La connexion, elle, reste ouverte. Un client y-websocket renvoie en un seul message tous les
// états qu'il reçoit d'un coup, et rien ne borne ceux d'une page : la fermer donnait à un
// lecteur le moyen de fermer les autres. Ignorer ne coûte rien à un client honnête, qui annonce
// son propre état dans un message à part.

/// Le plus grand nombre d'entrées de présence qu'un message porte sans qu'elles soient ignorées.
const MAX_ENTRIES: usize = 512;

/// `count` retraits de clients que personne ne tient : des entrées que le serveur ignore de
/// toute façon (Ruling R104), de quoi remplir un message sans rien lui faire dire.
fn free_removals(count: usize) -> Vec<(u64, u32, &'static str)> {
    (0..count as u64)
        .map(|i| (800_000 + i, 1, "null"))
        .collect()
}

/// Ces entrées en un message WebSocket, réparties sur `parts` messages d'awareness.
fn frame(entries: &[(u64, u32, &str)], parts: usize) -> Vec<u8> {
    entries
        .chunks(entries.len().div_ceil(parts))
        .flat_map(awareness_message)
        .collect()
}

/// Une personne a deux onglets. De l'un, un message de 512 entrées passe : les 511 retraits
/// sont ignorés, et son propre état arrive chez tous. À 513, rien du message n'est traité : ni
/// l'état de plus qu'il portait pour elle, ni le remplacement de celui de son autre onglet (ce
/// que son compte a le droit de faire), ni le client libre qu'il annonçait, qu'un autre compte
/// prend ensuite. Sa connexion reste servie, et l'état qu'elle annonce après, dans un message à
/// part, arrive chez tous.
///
/// Les entrées qui disent quelque chose sont en tête : réparties sur deux messages d'awareness,
/// elles sont dans le premier, lu quand la limite n'est pas encore dépassée.
async fn too_many_entries_are_ignored(parts: usize) {
    let f = Fixture::new(SyncConfig::default()).await;
    let account = new_user(&f.db).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let tab = Client::connect_synced(&f.url_for(of(account))).await;
    let sender = Client::connect_synced(&f.url_for(of(account))).await;
    let mut witness = Witness::join(&f).await;
    let (tab_id, sender_id) = (tab.client_id(), sender.client_id());
    let other_tab = named("Mallory, autre onglet");
    tab.set_awareness(&other_tab);
    assert!(sees(&carol, tab_id, Some(&other_tab)).await);

    // À la limite : le message est traité.
    let at_limit = named("Mallory, à la limite");
    let mut entries = vec![(sender_id, 1, at_limit.as_str())];
    entries.extend(free_removals(MAX_ENTRIES - 1));
    assert_eq!(entries.len(), MAX_ENTRIES);
    sender.send_raw(frame(&entries, parts));
    witness.after(&sender, &[&carol]).await;
    assert_eq!(
        state_of(&carol, sender_id).as_deref(),
        Some(at_limit.as_str()),
        "un message de {MAX_ENTRIES} entrées n'est pas passé"
    );

    // Une de plus : ignoré en entier, et la connexion est encore servie (`after` l'affirme).
    let (beyond, replaced, ghost) = (
        named("Mallory, au-delà"),
        named("Mallory, remplacée"),
        named("Fantôme"),
    );
    let mut entries = vec![
        (sender_id, 2, beyond.as_str()),
        (tab_id, 1_000, replaced.as_str()),
        (SPARE_CLIENT, 1, ghost.as_str()),
    ];
    entries.extend(free_removals(MAX_ENTRIES - 2));
    assert_eq!(entries.len(), MAX_ENTRIES + 1);
    sender.send_raw(frame(&entries, parts));
    witness.after(&sender, &[&carol]).await;
    // Rien d'appliqué ni de relayé.
    assert_eq!(
        state_of(&carol, sender_id).as_deref(),
        Some(at_limit.as_str()),
        "une entrée du message a été traitée"
    );
    assert_eq!(
        state_of(&carol, tab_id).as_deref(),
        Some(other_tab.as_str()),
        "une entrée du message a été traitée"
    );
    assert_eq!(state_of(&carol, SPARE_CLIENT), None);
    // Aucun client pris : le client libre que le message annonçait, un autre compte l'annonce,
    // et il est vu.
    let by_carol = named("De Carol");
    carol.send_raw(awareness_message(&[(SPARE_CLIENT, 1, &by_carol)]));
    assert!(
        sees(&tab, SPARE_CLIENT, Some(&by_carol)).await,
        "le message ignoré a donné un client à qui l'a envoyé"
    );

    // Son propre état, annoncé ensuite dans un message à part, est relayé.
    let afterwards = named("Mallory, ensuite");
    sender.send_raw(awareness_message(&[(sender_id, 2, &afterwards)]));
    assert!(
        sees(&carol, sender_id, Some(&afterwards)).await,
        "l'état annoncé après le message ignoré n'est pas arrivé"
    );
    // Le serveur n'a rien gardé du message ignoré : qui arrive reçoit le reste.
    let newcomer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    assert!(sees(&newcomer, sender_id, Some(&afterwards)).await);
    assert!(sees(&newcomer, tab_id, Some(&other_tab)).await);
    assert!(sees(&newcomer, SPARE_CLIENT, Some(&by_carol)).await);
}

#[tokio::test]
async fn presence_message_with_too_many_entries_is_ignored() {
    too_many_entries_are_ignored(1).await;
}

/// Le compte porte sur le message WebSocket entier : les mêmes entrées réparties sur deux
/// messages d'awareness, envoyés ensemble, sont ignorées de même, celles du premier comprises.
/// Sinon la limite se contournerait en mettant bout à bout des messages de 512 entrées.
#[tokio::test]
async fn presence_entries_are_counted_over_the_whole_websocket_message() {
    too_many_entries_are_ignored(2).await;
}

/// Le scénario qui a fait réviser le Ruling. Un compte ouvre 65 connexions et annonce huit
/// clients inventés sur chacune : 520 états. Qui arrive les reçoit, et un client y-websocket
/// renvoie aussitôt au serveur tout ce qu'il vient de recevoir, en un seul message. Ce renvoi
/// est ignoré sans fermer la connexion du nouveau venu, et son propre état, qu'il annonce dans
/// un message à part, est vu des autres.
#[tokio::test]
async fn newcomer_echoing_a_crowded_page_stays_connected() {
    let f = Fixture::new(SyncConfig::default()).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory = new_user(&f.db).await;
    let ghost = named("Fantôme");
    let mut crowd = Vec::new();
    for conn in 0..65u64 {
        let client = Client::connect_synced(&f.url_for(of(mallory))).await;
        let invented: Vec<(u64, u32, &str)> = (0..8)
            .map(|i| (600_000 + conn * 8 + i, 1, ghost.as_str()))
            .collect();
        client.send_raw(awareness_message(&invented));
        crowd.push(client);
    }

    let newcomer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    assert!(
        newcomer
            .wait_until(WAIT, |c| c.awareness_states().len() >= 520)
            .await,
        "le nouveau venu n'a pas reçu les états de la page"
    );
    let states = newcomer.awareness_states();
    assert!(states.len() > MAX_ENTRIES);
    // Ce que y-websocket en renvoie : tous, en un message.
    let echo: Vec<(u64, u32, &str)> = states
        .iter()
        .map(|(id, json)| (*id, 1, json.as_str()))
        .collect();
    newcomer.send_raw(awareness_message(&echo));
    assert!(
        still_served(&newcomer).await,
        "le nouveau venu est fermé pour avoir renvoyé les états de la page"
    );
    // Son propre état, annoncé à part, est vu.
    let own = named("Nouveau venu");
    newcomer.set_awareness(&own);
    assert!(sees(&carol, newcomer.client_id(), Some(&own)).await);
    assert!(still_served(&newcomer).await);
    // La foule est restée jusqu'au bout : ses états étaient bien ceux d'une page pleine.
    assert!(crowd.iter().all(|client| !client.is_closed()));
}

/// Seule la présence d'un tel message est ignorée : ce qu'il porte d'autre est traité. Une mise
/// à jour du document envoyée dans le même message WebSocket, après les entrées de trop, est
/// appliquée, écrite et relayée.
#[tokio::test]
async fn document_update_beside_too_many_presence_entries_is_not_lost() {
    let f = Fixture::new(SyncConfig::default()).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let writer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mut witness = Witness::join(&f).await;

    // Une frappe préparée à part, pour l'écrire à la main dans le message.
    let doc = detached(&writer);
    let paragraph = first_content(&doc);
    let typed = {
        let mut txn = doc.transact_mut();
        paragraph.insert(&mut txn, 0, XmlTextPrelim::new("gardée"));
        txn.encode_update_v1()
    };
    let beyond = named("Au-delà");
    let mut entries = vec![(writer.client_id(), 1, beyond.as_str())];
    entries.extend(free_removals(MAX_ENTRIES));
    let mut message = awareness_message(&entries);
    message.extend(Message::Sync(SyncMessage::Update(typed)).encode_v1());
    writer.send_raw(message);

    assert!(
        carol
            .wait_until(WAIT, |c| text(&c.blocks()) == "gardée")
            .await,
        "la mise à jour du document, envoyée avec trop d'entrées de présence, est perdue"
    );
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "gardée"
    );
    // La présence du même message, elle, est ignorée.
    witness.after(&writer, &[&carol]).await;
    assert_eq!(state_of(&carol, writer.client_id()), None);
}

// ---------------------------------------------------------------------------------------------
// Qui n'écrit pas n'annonce rien (Ruling R118), et un état n'est relayé que s'il a la forme
// connue (Ruling R119).
//
// Le client pose la couleur d'un curseur telle quelle dans un attribut `style`, et décode sa
// position sans garde : une couleur piégée était du CSS chez tous ceux qui écrivent, et un
// curseur mal formé figeait leur éditeur, qui récrivait ensuite le document par-dessus ce que
// les autres avaient tapé (e2e `presence-guard.spec.ts`). Une invitée en lecture y suffisait.

/// La fin du document, en position relative de Yjs : elle ne demande aucun identifiant.
const END: &str = r#"{"tname":"document-store","assoc":0}"#;

/// Un état tel que le client l'écrit : un nom, une couleur, un curseur à la fin du document.
fn with_cursor(name: &str, colour: &str) -> String {
    format!(
        r#"{{"user":{{"name":"{name}","color":"{colour}"}},"cursor":{{"anchor":{END},"head":{END}}}}}"#
    )
}

/// L'état du client `client` chez `seen_by`, relu : l'ordre des clés n'y compte pas.
fn value_of(seen_by: &Client, client: u64) -> Option<serde_json::Value> {
    state_of(seen_by, client).map(|json| serde_json::from_str(&json).expect("un état en JSON"))
}

fn value(json: &str) -> Option<serde_json::Value> {
    Some(serde_json::from_str(json).expect("un état en JSON"))
}

/// D'une connexion en lecture seule, rien n'est gardé ni relayé, et aucun client n'est pris :
/// ni son propre état, ni des clients inventés, même plus que la limite d'une connexion. Elle
/// n'est pas fermée, et reçoit toujours la présence des autres.
#[tokio::test]
async fn presence_of_a_read_only_connection_is_ignored() {
    let f = Fixture::new(SyncConfig::default()).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let dave = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let gina = Client::connect_synced(&f.url_for(reader(new_user(&f.db).await))).await;
    let mut witness = Witness::join(&f).await;
    let (carol_id, gina_id) = (carol.client_id(), gina.client_id());

    gina.set_awareness(&with_cursor("Gina", "#B4462B"));
    // Neuf clients inventés : d'une connexion qui écrit, le message la fermerait.
    let ghost = named("Fantôme");
    let invented: Vec<(u64, u32, &str)> =
        (0..9).map(|i| (650_000 + i, 1, ghost.as_str())).collect();
    gina.send_raw(awareness_message(&invented));
    witness.after(&gina, &[&carol, &dave]).await;
    assert_eq!(
        state_of(&carol, gina_id),
        None,
        "l'état d'une lectrice est relayé"
    );
    for (id, ..) in &invented {
        assert_eq!(state_of(&carol, *id), None);
    }
    // Aucun client pris : un compte qui écrit annonce l'un d'eux, et il est vu.
    let by_carol = named("De Carol");
    carol.send_raw(awareness_message(&[(650_000, 1, &by_carol)]));
    assert!(
        sees(&dave, 650_000, Some(&by_carol)).await,
        "une lectrice a pris un client"
    );
    // Elle reçoit la présence des autres, comme avant.
    let here = named("Carol");
    carol.set_awareness(&here);
    assert!(sees(&gina, carol_id, Some(&here)).await);
    // Le serveur n'a rien gardé d'elle : qui arrive ne la voit pas.
    let newcomer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    assert!(sees(&newcomer, carol_id, Some(&here)).await);
    assert_eq!(state_of(&newcomer, gina_id), None);
    assert!(still_served(&gina).await);
}

/// La couleur du relecteur : la suite de l'attribut `style` où le client la poserait. L'état
/// n'est ni gardé ni relayé, et ne donne pas son client ; le reste du message passe, et la
/// connexion reste ouverte.
#[tokio::test]
async fn presence_with_a_trapped_colour_is_not_relayed() {
    let f = Fixture::new(SyncConfig::default()).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mut witness = Witness::join(&f).await;
    let mallory_id = mallory.client_id();

    let trapped = with_cursor(
        "Olga",
        "red;position:fixed;inset:0;z-index:99999;background:url(https://evil.example/x.png)",
    );
    let fine = with_cursor("Mallory", "#2B59C3");
    mallory.send_raw(awareness_message(&[
        (SPARE_CLIENT, 1, &trapped),
        (mallory_id, 1, &fine),
    ]));
    witness.after(&mallory, &[&carol]).await;
    assert_eq!(
        value_of(&carol, SPARE_CLIENT),
        None,
        "la couleur piégée est relayée"
    );
    // Le reste du message est passé.
    assert_eq!(value_of(&carol, mallory_id), value(&fine));
    // Sur son propre client non plus, et l'état d'avant reste.
    mallory.send_raw(awareness_message(&[(mallory_id, 2, &trapped)]));
    witness.after(&mallory, &[&carol]).await;
    assert_eq!(value_of(&carol, mallory_id), value(&fine));
    // L'état refusé n'a pas donné son client : un autre compte l'annonce, et il est vu.
    let by_carol = named("De Carol");
    carol.send_raw(awareness_message(&[(SPARE_CLIENT, 1, &by_carol)]));
    assert!(
        sees(&mallory, SPARE_CLIENT, Some(&by_carol)).await,
        "l'état refusé a donné son client"
    );
    let newcomer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    assert!(sees(&newcomer, SPARE_CLIENT, Some(&by_carol)).await);
    assert_eq!(value_of(&newcomer, mallory_id), value(&fine));
}

/// Un curseur que le client ne saurait pas lire : une position qui n'est pas un objet, une
/// position qui ne nomme rien (Yjs lève une exception), une clé inconnue. Aucun n'est relayé.
#[tokio::test]
async fn presence_with_a_malformed_cursor_is_not_relayed() {
    let f = Fixture::new(SyncConfig::default()).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mut witness = Witness::join(&f).await;
    let user = r##""user":{"name":"Mallory","color":"#2B59C3"}"##;
    let nothing = r#"{"type":null,"tname":null,"item":null}"#;
    let malformed = [
        format!(r#"{{{user},"cursor":{{"anchor":7,"head":7}}}}"#),
        format!(r#"{{{user},"cursor":{{"anchor":{nothing},"head":{nothing}}}}}"#),
        format!(r#"{{{user},"cursor":{{"anchor":{END},"head":{END}}},"surprise":true}}"#),
        format!(r#"{{{user},"cursor":{{"anchor":{END}}}}}"#),
        format!(r#"{{{user},"cursor":{{"anchor":{{"tname":"autre"}},"head":{END}}}}}"#),
    ];
    for (i, state) in malformed.iter().enumerate() {
        let client = SPARE_CLIENT + i as u64;
        mallory.send_raw(awareness_message(&[(client, 1, state)]));
        witness.after(&mallory, &[&carol]).await;
        assert_eq!(value_of(&carol, client), None, "relayé : {state}");
    }
    // Aucun n'a été gardé : qui arrive n'en reçoit rien. Et un état en règle passe ensuite,
    // sous l'un de ces clients, que les états refusés n'avaient pas donnés.
    let fine = with_cursor("Mallory", "#2B59C3");
    mallory.send_raw(awareness_message(&[(SPARE_CLIENT, 1, &fine)]));
    let newcomer = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    for seen_by in [&carol, &newcomer] {
        assert!(
            seen_by
                .wait_until(WAIT, |c| value_of(c, SPARE_CLIENT) == value(&fine))
                .await
        );
    }
    for i in 1..malformed.len() as u64 {
        assert_eq!(value_of(&newcomer, SPARE_CLIENT + i), None);
    }
}

/// Ce que le client réel écrit passe : l'état initial de y-protocols (un objet vide), puis le
/// nom et la couleur que BlockNote y pose, puis le curseur de y-prosemirror, qui redevient
/// `null` quand l'éditeur perd le focus ; une position dans un bloc porte des identifiants.
#[tokio::test]
async fn presence_of_the_real_client_shape_is_relayed() {
    let f = Fixture::new(SyncConfig::default()).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mona = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mona_id = mona.client_id();
    let user = r##""user":{"name":"Mona","color":"#B4462B"}"##;
    let in_text = r#"{"type":{"client":1440742776,"clock":6},"item":{"client":206716952,"clock":41},"assoc":-1}"#;
    let states = [
        "{}".to_string(),
        format!("{{{user}}}"),
        format!(r#"{{{user},"cursor":{{"anchor":{END},"head":{END}}}}}"#),
        format!(r#"{{{user},"cursor":{{"anchor":{in_text},"head":{END}}}}}"#),
        format!(r#"{{{user},"cursor":null}}"#),
        // Une couleur en minuscules en est une aussi.
        with_cursor("Mona", "#b4462b"),
    ];
    for (i, state) in states.iter().enumerate() {
        mona.send_raw(awareness_message(&[(mona_id, i as u32 + 1, state)]));
        assert!(
            carol
                .wait_until(WAIT, |c| value_of(c, mona_id) == value(state))
                .await,
            "non relayé : {state}"
        );
    }
}

/// Un état de plus de 2 Kio est ignoré, quelle que soit sa forme ; les autres entrées du
/// message passent. À 2 Kio tout juste, il passe.
#[tokio::test]
async fn presence_state_over_2_kib_is_ignored() {
    let f = Fixture::new(SyncConfig::default()).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mut witness = Witness::join(&f).await;
    let mallory_id = mallory.client_id();
    // Un état en règle, étiré par des espaces jusqu'à `bytes` octets.
    let padded = |bytes: usize| {
        let state = named("Mallory");
        format!(
            "{}{}}}",
            &state[..state.len() - 1],
            " ".repeat(bytes - state.len())
        )
    };
    assert_eq!(padded(2049).len(), 2049);

    let fine = named("Fantôme");
    mallory.send_raw(awareness_message(&[
        (mallory_id, 1, &padded(2049)),
        (SPARE_CLIENT, 1, &fine),
    ]));
    witness.after(&mallory, &[&carol]).await;
    assert_eq!(
        state_of(&carol, mallory_id),
        None,
        "un état de 2 049 octets est relayé"
    );
    assert_eq!(state_of(&carol, SPARE_CLIENT), Some(fine));
    // À la limite : gardé. Ce qui est relayé est l'état relu, sans ses espaces.
    mallory.send_raw(awareness_message(&[(mallory_id, 1, &padded(2048))]));
    assert!(sees(&carol, mallory_id, Some(&named("Mallory"))).await);
}

/// Un état qui n'est pas du JSON est une forme inconnue comme une autre : ignoré, sans fermer
/// la connexion. (Le décodeur ne relit plus les états : ce qui n'est pas du JSON n'y est plus
/// un message illisible.)
#[tokio::test]
async fn presence_state_that_is_not_json_is_ignored() {
    let f = Fixture::new(SyncConfig::default()).await;
    let carol = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mallory = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let mut witness = Witness::join(&f).await;
    let mallory_id = mallory.client_id();
    let fine = named("Fantôme");
    mallory.send_raw(awareness_message(&[
        (mallory_id, 1, "{pas du json"),
        (SPARE_CLIENT, 1, &fine),
    ]));
    witness.after(&mallory, &[&carol]).await;
    assert_eq!(state_of(&carol, mallory_id), None);
    assert_eq!(state_of(&carol, SPARE_CLIENT), Some(fine));
    // Son état suivant, en règle, passe : l'entrée ignorée n'a rien laissé.
    mallory.send_raw(awareness_message(&[(mallory_id, 1, &named("Mallory"))]));
    assert!(sees(&carol, mallory_id, Some(&named("Mallory"))).await);
}
