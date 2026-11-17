//! Une frappe livrée en retard par l'ancienne connexion du même onglet (Ruling R784 ; constat C1
//! de la re-relecture 4 de la garde des attributs).
//!
//! L'onglet (identifiant de client 5) tape « a » : la mise à jour part sur sa connexion S1, et y
//! reste (réseau coupé, le noyau la garde à renvoyer). Il tape « b » hors ligne. Il rouvre une
//! connexion S2 : le serveur l'accueille par son étape 1, qui ne connaît pas « a ». La frappe de
//! S1 arrive. Puis l'étape 2 de S2, calculée depuis l'étape 1 reçue : « ab », un seul élément, à
//! cheval sur ce que la page connaît maintenant. Refusée depuis R740, elle fermait S2 ; la page la
//! coupe, comme Yjs l'aurait écrite pour elle.

mod common;

use common::raw::{Raw, STEP_1, read_var, write_var};
use common::*;
use mementum_sync::SyncConfig;
use yrs::updates::decoder::Decode;
use yrs::{ClientID, Doc, OffsetKind, Options, ReadTxn, Text, Transact, Update};
use yrs::{XmlFragment, XmlOut};

fn message(kind: u8, update: &[u8]) -> Vec<u8> {
    let mut m = vec![0, kind];
    write_var(&mut m, update.len() as u64);
    m.extend_from_slice(update);
    m
}

/// L'état complet de la page, demandé par une étape 1 vide.
async fn state(raw: &mut Raw) -> Vec<u8> {
    raw.send(STEP_1.to_vec()).await;
    loop {
        let m = raw.next().await.expect("connexion ouverte");
        if let [0, 1, ..] = m[..] {
            let mut at = 2;
            let len = read_var(&m, &mut at) as usize;
            return m[at..at + len].to_vec();
        }
    }
}

/// Lit jusqu'à l'étape 1 du serveur (son accueil) : rend son vecteur d'état.
async fn welcome(raw: &mut Raw) -> Vec<u8> {
    loop {
        let m = raw.next().await.expect("connexion ouverte");
        if let [0, 0, ..] = m[..] {
            let mut at = 2;
            let len = read_var(&m, &mut at) as usize;
            return m[at..at + len].to_vec();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_honest_tab_whose_old_connection_delivers_late_is_not_refused() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("abc\n").await;

    // S1 : la connexion de l'onglet, synchronisée.
    let mut s1 = Raw::connect(&f, writing_conn()).await;
    welcome(&mut s1).await;
    let tab = Doc::with_options(Options {
        client_id: ClientID::new(5),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    tab.get_or_insert_xml_fragment(mementum_crdt::FRAGMENT);
    let base = state(&mut s1).await;
    tab.transact_mut()
        .apply_update(Update::decode_v1(&base).unwrap())
        .unwrap();
    let paragraph = first_content(&tab);
    let text = {
        let txn = tab.transact();
        match paragraph.get(&txn, 0) {
            Some(XmlOut::Text(t)) => t,
            _ => panic!("pas de texte"),
        }
    };
    // « a », envoyée sur S1… qui ne la livre pas encore.
    let u1 = {
        let mut txn = tab.transact_mut();
        let end = text.len(&txn);
        text.insert(&mut txn, end, "a");
        txn.encode_update_v1()
    };
    // « b », hors ligne.
    {
        let mut txn = tab.transact_mut();
        let end = text.len(&txn);
        text.insert(&mut txn, end, "b");
    }

    // S2 : la nouvelle connexion du même onglet. Le serveur l'accueille par son étape 1.
    let mut s2 = Raw::connect(&f, writing_conn()).await;
    let server_sv = welcome(&mut s2).await;
    // La frappe de S1 arrive enfin.
    s1.send(message(2, &u1)).await;
    s1.updates_so_far().await.expect("S1 ouverte");
    // L'étape 2 de l'onglet, depuis l'étape 1 reçue.
    let step2 = tab
        .transact()
        .encode_diff_v1(&yrs::StateVector::decode_v1(&server_sv).unwrap());
    assert_eq!(
        &step2[..4],
        &[1, 1, 5, 0],
        "un élément du client 5, horloge 0"
    );
    assert!(
        step2.windows(3).any(|w| w == [2, 97, 98]),
        "« ab », d'un tenant"
    );
    s2.send(message(1, &step2)).await;

    // S2 reste ouverte, la page lit tout, et rien n'a été rechargé.
    s2.updates_so_far().await.expect("S2 fermée par le serveur");
    assert_eq!(text_of(&f).await, "abcab");
    assert_eq!(f.hub.reloads(), 0);
    // La reconnexion suivante (S3) : le serveur annonce tout, l'onglet n'a plus rien à envoyer.
    let mut s3 = Raw::connect(&f, writing_conn()).await;
    let sv3 = welcome(&mut s3).await;
    let step2 = tab
        .transact()
        .encode_diff_v1(&yrs::StateVector::decode_v1(&sv3).unwrap());
    assert_eq!(&step2[..1], &[0], "plus rien à envoyer : {step2:?}");
    s3.send(message(1, &step2)).await;
    s3.updates_so_far().await.expect("S3 ouverte");
    assert_eq!(text_of(&f).await, "abcab");
    s2.updates_so_far()
        .await
        .expect("la connexion d'un onglet honnête a été fermée");
    assert_eq!(f.hub.reloads(), 0);
}

async fn text_of(f: &Fixture) -> String {
    text(&f.hub.read_blocks(f.page).await.unwrap())
}
