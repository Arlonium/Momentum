//! Suppressions qui visent des horloges que la page n'a pas encore (Ruling R198).
//!
//! Une mise à jour sans bloc, de dix octets, peut supprimer une plage d'horloges à venir du
//! client d'un autre : yrs la garde en attente, puis supprime à l'arrivée tout ce que ce client
//! écrit dans la plage, sur le serveur et chez tous, sans que rien en reste dans aucune version.
//! Le serveur la traite donc comme un trou (Ruling R131) : elle attend, dans la file de sa
//! connexion, que la page ait ces horloges, et part avec la connexion. Ce qu'un client honnête
//! envoie passe toujours : une suppression qui suit un ajout qu'elle vise passe derrière lui ;
//! une suppression de ce que la page a déjà ramassé se juge sur son vecteur d'état ; et une
//! suppression que la page garde déjà en attente (une base d'avant ce Ruling), que chaque client
//! renvoie dans son étape 2, ne retient rien (Ruling R199).

mod common;

use common::raw::{Raw, deletion};
use common::*;
use mementum_sync::{ConnInfo, SyncConfig};
use uuid::Uuid;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{ClientID, Update};

/// Une connexion du compte `account`, qui a le droit d'écrire.
fn of(account: Uuid) -> ConnInfo {
    ConnInfo {
        actor: Some(account),
        ..writing_conn()
    }
}

/// Ce que le serveur écrit et relaie d'une mise à jour reçue d'un client : ce que yrs en a lu,
/// réencodé (Ruling R130).
fn as_read(update: &[u8]) -> Vec<u8> {
    Update::decode_v1(update).unwrap().encode_v1()
}

/// Les mises à jour au journal de la page, dans l'ordre des `seq`.
async fn journal(f: &Fixture) -> Vec<Vec<u8>> {
    f.store
        .load_doc(f.page)
        .await
        .unwrap()
        .updates
        .into_iter()
        .map(|(_, update)| update)
        .collect()
}

/// Le texte de la page, lu par le hub.
async fn served_text(f: &Fixture) -> String {
    text(&f.hub.read_blocks(f.page).await.unwrap())
}

/// L'horloge que le client écrira ensuite pour lui-même.
fn next_clock(client: &Client) -> u64 {
    u64::from(
        client
            .state_vector()
            .get(&ClientID::new(client.client_id())),
    )
}

/// Le scénario de la relecture finale, avec l'attente inverse. Tom, qui peut écrire, lit le
/// `ClientID` de Mona, et envoie une mise à jour de dix octets : aucun bloc, la suppression de
/// `[horloge actuelle de Mona, + 1 000 000)`. Elle était écrite, relayée, et supprimait à
/// l'arrivée tout ce que Mona tapait ensuite, sur le serveur, chez Mona à sa prochaine réception,
/// chez Tom, chez un arrivant et au rechargement. Elle est retenue : ni écrite ni relayée, Tom
/// n'est pas fermé, et ce que Mona tape reste partout. Puis une plage courte, que Mona dépassera :
/// retenue elle aussi, elle part quand Tom est fermé, et ne supprime rien.
#[tokio::test]
async fn deletion_of_clocks_still_to_come_is_held_and_leaves_with_its_connection() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Alpha\n").await;
    let id = f.ids().await.remove(0);
    let tom_account = new_user(&f.db).await;
    let mona = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let tom = Client::connect_synced(&f.url_for(of(tom_account))).await;
    mona.type_text(&id, " un");
    processed(&mona).await;
    assert!(
        tom.wait_until(WAIT, |c| text(&c.blocks()) == "Alpha un")
            .await
    );
    let mut observer = Raw::connect(&f, writing_conn()).await;
    assert_eq!(observer.updates_so_far().await, Some(vec![]));
    let before = journal(&f).await;

    tom.send_update(&deletion(mona.client_id(), next_clock(&mona), 1_000_000));
    processed(&tom).await;
    let tom_closed = tom.is_closed();
    let written = journal(&f).await.len() - before.len();
    let relayed = observer.updates_so_far().await;
    assert!(
        !tom_closed && written == 0 && relayed == Some(vec![]),
        "Tom fermé : {tom_closed} ; doc_updates : {} -> {} ; relayée : {relayed:?}",
        before.len(),
        before.len() + written
    );

    mona.type_text(&id, " deux");
    processed(&mona).await;
    processed(&tom).await;
    let newcomer = f.client().await;
    let texts = [
        ("serveur", served_text(&f).await),
        ("Mona", text(&mona.blocks())),
        ("Tom", text(&tom.blocks())),
        ("arrivant", text(&newcomer.blocks())),
        (
            "rechargé",
            text(&stored_page(&f.store, f.page).await.read_blocks()),
        ),
    ];
    assert!(
        texts.iter().all(|(_, t)| t == "Alpha un deux"),
        "après « deux » tapé par Mona : {texts:?}"
    );
    assert!(!stored_page(&f.store, f.page).await.has_pending_deletions());

    // Une plage de quatre unités, que les six de « trois » dépassent : retenue, puis perdue à la
    // fermeture de Tom. Sans quoi elle passerait dès que la page a ces horloges.
    tom.send_update(&deletion(mona.client_id(), next_clock(&mona), 4));
    processed(&tom).await;
    f.hub.kick_user(tom_account).await;
    assert!(tom.wait_until(WAIT, |c| c.is_closed()).await);
    mona.type_text(&id, " trois");
    processed(&mona).await;
    assert_eq!(served_text(&f).await, "Alpha un deux trois");
    assert_eq!(text(&mona.blocks()), "Alpha un deux trois");
    assert_eq!(
        journal(&f).await.len(),
        before.len() + 2,
        "les deux frappes de Mona, rien de Tom"
    );
    assert_eq!(
        observer.updates_so_far().await.map(|u| u.len()),
        Some(2),
        "l'observatrice ne reçoit que les frappes de Mona"
    );
}

/// Une suppression honnête arrivée avant l'ajout qu'elle vise passe quand il arrive, à sa place.
/// Un client tape « un » puis « deux » hors ligne, efface « deux », et revient : ses messages
/// arrivent dans le désordre, l'effacement et « deux » avant « un ». « deux » laisse un trou ;
/// l'effacement vise des horloges que la page n'a pas : tous deux attendent, dans l'ordre. Quand
/// « un » arrive, les trois passent, et le journal comme les autres les reçoivent dans l'ordre où
/// ils ont été faits. Avant le Ruling R198, l'effacement passait le premier, écrit en attente.
#[tokio::test]
async fn honest_deletion_ahead_of_what_it_deletes_waits_behind_it() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Alpha\n").await;
    let id = f.ids().await.remove(0);
    let writer = f.client().await;
    let mut observer = Raw::connect(&f, writing_conn()).await;
    assert_eq!(observer.updates_so_far().await, Some(vec![]));
    let before = journal(&f).await;

    let edits = writer.page().unwrap();
    let one = edits.replace_text(&id, 5, 0, " un").unwrap();
    let two = edits.replace_text(&id, 8, 0, " deux").unwrap();
    let erase = edits.replace_text(&id, 8, 5, "").unwrap();

    writer.send_update(&two);
    writer.send_update(&erase);
    processed(&writer).await;
    assert_eq!(served_text(&f).await, "Alpha");
    assert_eq!(journal(&f).await, before, "rien d'écrit");
    assert_eq!(
        observer.updates_so_far().await,
        Some(vec![]),
        "rien de relayé"
    );

    writer.send_update(&one);
    processed(&writer).await;
    assert!(!writer.is_closed());
    assert_eq!(served_text(&f).await, "Alpha un");
    let in_order = vec![as_read(&one), as_read(&two), as_read(&erase)];
    let mut expected = before;
    expected.extend(in_order.clone());
    assert_eq!(journal(&f).await, expected, "un, deux, puis l'effacement");
    assert_eq!(observer.updates_so_far().await, Some(in_order));
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "Alpha un"
    );
}

/// La variante sur le client du document du serveur, celui de ses écritures (`PUT /mmd`,
/// restauration). Visé de même, il faisait arriver vides les écritures qui suivent : chez
/// chaque client, à l'arrivée, et dans le document rechargé. La suppression est retenue, et
/// l'écriture arrive entière partout.
#[tokio::test]
async fn deletion_aimed_at_the_server_client_does_not_empty_its_writes() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Alpha\n").await;
    let tom = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
    let observer = f.client().await;
    // Le client du serveur : celui de sa dernière écriture, que chacun reçoit.
    let written = journal(&f).await.pop().unwrap();
    let inserted = Update::decode_v1(&written).unwrap().insertions(true);
    let (server, clock) = inserted
        .iter()
        .next()
        .and_then(|(client, ranges)| {
            Some((client.get(), u64::from(ranges.iter().next_back()?.end)))
        })
        .unwrap();
    let before = journal(&f).await.len();

    tom.send_update(&deletion(server, clock, 1_000_000));
    processed(&tom).await;
    assert!(!tom.is_closed(), "retenue, pas refusée");
    assert_eq!(journal(&f).await.len(), before, "écrite");

    f.write_mmd("Bonjour\n").await;
    processed(&observer).await;
    let newcomer = f.client().await;
    let texts = [
        ("serveur", served_text(&f).await),
        ("observatrice", text(&observer.blocks())),
        ("arrivant", text(&newcomer.blocks())),
        (
            "rechargé",
            text(&stored_page(&f.store, f.page).await.read_blocks()),
        ),
    ];
    assert!(
        texts.iter().all(|(_, t)| t == "Bonjour"),
        "après l'écriture du serveur : {texts:?}"
    );
}

/// Yjs met tout son jeu de suppressions dans l'étape 2, y compris celles d'éléments que le
/// serveur a ramassés depuis : elles visent des horloges que la page a, même si aucun élément ne
/// les porte plus. Elles se jugent sur le vecteur d'état, pas sur la présence de l'élément. Ici,
/// le texte de Mona est effacé puis compacté, l'acteur repart de l'instantané (il n'a jamais rien
/// reçu de Mona), et Tom revient avec ce qu'il a tapé hors ligne : son étape 2 passe aussitôt.
#[tokio::test]
async fn step_2_deleting_what_the_page_collected_is_not_held() {
    let f = Fixture::new(SyncConfig {
        compact_after_deletions: 1,
        ..SyncConfig::default()
    })
    .await;
    f.write_mmd("Alpha\n").await;
    let id = f.ids().await.remove(0);
    let mona = f.client().await;
    let tom = f.client().await;
    mona.type_text(&id, " un");
    processed(&mona).await;
    let erase = mona.page().unwrap().replace_text(&id, 5, 3, "").unwrap();
    mona.apply_local(&erase);
    processed(&mona).await;
    assert!(tom.wait_until(WAIT, |c| text(&c.blocks()) == "Alpha").await);
    let stored = f.store.load_doc(f.page).await.unwrap();
    assert!(
        stored.snapshot.is_some() && stored.updates.is_empty(),
        "la suppression a fait compacter la page"
    );

    // L'acteur repart de l'instantané : les éléments de Mona n'y sont plus qu'une plage ramassée.
    let offline = tom.page().unwrap();
    f.hub.forget(&[f.page]).await;
    let back = f.client().await;
    let server_state = offline.state_vector();
    offline.replace_text(&id, 5, 0, " hors ligne").unwrap();
    let step2 = offline.encode_diff(&server_state).unwrap();
    back.send_update(&step2);
    processed(&back).await;
    assert!(!back.is_closed());
    assert_eq!(served_text(&f).await, "Alpha hors ligne");
}

/// Une base d'avant le Ruling R198 peut garder au journal une suppression en attente. yrs la
/// garde en attente au chargement, la met dans chaque état complet et dans l'étape 2 qu'il
/// envoie ; chaque client la garde de même et la renvoie dans son étape 2. Ce renvoi n'apporte
/// rien : il ne retient pas l'étape 2 (sinon, ce qu'elle porte de tapé hors ligne ne passerait
/// jamais, et la frappe suivante, retenue derrière, finirait par fermer la connexion), et il
/// n'est pas écrit (Ruling R181 : rien de neuf, rien d'écrit).
#[tokio::test]
async fn deletion_already_pending_in_a_stored_page_holds_nothing() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Alpha\n").await;
    let id = f.ids().await.remove(0);
    f.store
        .append_updates(f.page, &[&deletion(4242, 0, 5)], None)
        .await
        .unwrap();
    f.hub.forget(&[f.page]).await;
    let tab = f.client().await;
    assert!(
        tab.page().unwrap().has_pending_deletions(),
        "l'onglet a reçu la suppression en attente"
    );
    let before = journal(&f).await.len();

    // L'étape 2 d'un onglet qui revient sans rien de neuf.
    let page = tab.page().unwrap();
    let server_state = page.state_vector();
    let echo = page.encode_diff(&server_state).unwrap();
    tab.send_update(&echo);
    processed(&tab).await;
    assert!(!tab.is_closed());
    assert_eq!(journal(&f).await.len(), before, "le renvoi est écrit");

    // Avec ce qu'il a tapé hors ligne : appliquée aussitôt.
    page.replace_text(&id, 5, 0, " hors ligne").unwrap();
    let step2 = page.encode_diff(&server_state).unwrap();
    tab.send_update(&step2);
    processed(&tab).await;
    assert!(!tab.is_closed());
    assert_eq!(served_text(&f).await, "Alpha hors ligne");
    assert_eq!(journal(&f).await.len(), before + 1);
}

/// Constat C1 de la relecture de la piste S (Ruling R207), avec l'attente inverse. Tom écrit sur
/// sa connexion une mise à jour de 26 octets, sans suppression : deux entrées pour le client qui
/// a écrit la racine de la page, un `Skip` d'une unité à son horloge courante, puis une plage
/// ramassée `[0, 1)`, que la page connaît déjà. yrs écrivait la plage par-dessus la racine :
/// Tom n'était pas fermé, la page se vidait chez chaque connexion à sa prochaine étape 2, chez
/// chaque arrivant, puis pour de bon au compactage. La garde refuse la mise à jour : Tom est
/// fermé, rien n'est écrit ni relayé, et la page reste entière partout, avec le compactage le
/// plus serré comme sans.
#[tokio::test]
async fn repeated_client_in_an_update_closes_its_connection_and_harms_nothing() {
    for snapshot_every in [SyncConfig::default().snapshot_every, 2] {
        let f = Fixture::new(SyncConfig {
            snapshot_every,
            ..SyncConfig::default()
        })
        .await;
        f.write_mmd("Alpha\n\nBeta\n").await;
        let tom = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
        let mona = Client::connect_synced(&f.url_for(of(new_user(&f.db).await))).await;
        let mut observer = Raw::connect(&f, writing_conn()).await;
        assert_eq!(observer.updates_so_far().await, Some(vec![]));
        let before = journal(&f).await;

        // Le client qui a écrit la racine : le seul que porte l'instantané de la page neuve.
        let snapshot = f.store.load_doc(f.page).await.unwrap().snapshot.unwrap().1;
        let created = Update::decode_v1(&snapshot).unwrap().insertions(true);
        let root = created
            .iter()
            .next()
            .map(|(client, _)| client.get())
            .unwrap();
        let clock = u64::from(tom.state_vector().get(&ClientID::new(root)));
        let mut forged = vec![2, 1];
        common::raw::write_var(&mut forged, root);
        common::raw::write_var(&mut forged, clock);
        forged.extend([10, 1, 1]);
        common::raw::write_var(&mut forged, root);
        forged.extend([0, 0, 1, 0]);
        tom.send_update(&forged);

        let what = format!("compactage toutes les {snapshot_every} écritures");
        assert!(
            tom.wait_until(WAIT, |c| c.is_closed()).await,
            "{what} : Tom n'est pas fermé"
        );
        assert_eq!(journal(&f).await, before, "{what} : écrite");
        assert_eq!(
            observer.updates_so_far().await,
            Some(vec![]),
            "{what} : relayée"
        );
        processed(&mona).await;
        let newcomer = f.client().await;
        let texts = [
            ("serveur", served_text(&f).await),
            ("Mona", text(&mona.blocks())),
            ("arrivant", text(&newcomer.blocks())),
            (
                "rechargé du store",
                text(&stored_page(&f.store, f.page).await.read_blocks()),
            ),
        ];
        assert!(
            texts.iter().all(|(_, t)| t == "Alpha\nBeta"),
            "{what} : {texts:?}"
        );
    }
}
