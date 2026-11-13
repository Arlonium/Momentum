//! Une entrée du journal où un client paraît deux fois (Ruling R207, décision du contrôleur à la
//! reprise, Ruling R258). Depuis R207, la garde refuse une telle mise à jour, d'un client comme
//! du stockage. Une base d'avant R130 gardait au journal les octets reçus d'un client : une mise à
//! jour hostile écrite alors rendrait sa page inchargeable. À la relecture, l'entrée est écartée
//! et journalisée (`error`), et la page se charge sans elle.

mod common;

use axum::http::StatusCode;
use common::sync::{open_app, user};
use common::{Logs, Team};
use mementum_acl::Level;
use mementum_crdt::PageDoc;

/// Écrit un entier variable de lib0.
fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Lit un entier variable de lib0.
fn read_var(bytes: &[u8], at: &mut usize) -> u64 {
    let (mut value, mut shift) = (0u64, 0);
    loop {
        let byte = bytes[*at];
        *at += 1;
        value |= u64::from(byte & 0x7F) << shift;
        if byte < 0x80 {
            return value;
        }
        shift += 7;
    }
}

#[tokio::test]
async fn journal_entry_with_a_repeated_client_is_set_aside() {
    let logs = Logs::capture();
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Journal", &[user(&team.olga, Level::Full)])
        .await;

    // Le journal d'une page, bâti à la main : la création, une frappe honnête, l'entrée hostile
    // (deux entrées pour le client qui a écrit la racine : un `Skip` à son horloge courante, puis
    // une plage ramassée `[0, 1)`), et une seconde frappe honnête.
    let (created, creation) = PageDoc::new_page();
    let paragraph = created.read_blocks()[0].id.clone();
    let first = PageDoc::from_updates([creation.as_slice()]).unwrap();
    let one = first.replace_text(&paragraph, 0, 0, "Un").unwrap();
    // Le vecteur d'état de la page neuve : un client, celui de la racine, et son horloge.
    let state = created.state_vector();
    let mut at = 0;
    assert_eq!(read_var(&state, &mut at), 1);
    let root = read_var(&state, &mut at);
    let clock = read_var(&state, &mut at);
    let mut hostile = vec![2, 1];
    var(&mut hostile, root);
    var(&mut hostile, clock);
    hostile.extend([10, 1, 1]);
    var(&mut hostile, root);
    hostile.extend([0, 0, 1, 0]);
    let second = PageDoc::from_updates([creation.as_slice(), one.as_slice()]).unwrap();
    let two = second.replace_text(&paragraph, 2, 0, " deux").unwrap();
    team.store
        .append_updates(page, &[&creation, &one, &hostile, &two], None)
        .await
        .unwrap();

    let read = team.olga.get_mmd(&page.to_string()).await;
    read.assert_status(StatusCode::OK);
    assert_eq!(read.text(), "Un deux\n");
    let journal = logs.text();
    assert!(
        journal.contains("ERROR") && journal.contains("écartée"),
        "une ligne `error` qui dit l'entrée écartée : {journal}"
    );
}

/// Constat N1 de la re-relecture de la piste S (Ruling R366). L'entrée écartée portait des blocs
/// d'un client C (« abc ») ; l'entrée suivante de C (« def », écrite en tête du paragraphe) n'en
/// dépend pas, mais ses horloges les suivent : sans eux, elle laisse un trou. yrs l'intégrait
/// en laissant le trou, et le serveur lisait « defUn » ; un navigateur (Yjs 13) la garde en
/// attente et lit « Un » (vérifié par un script Node, voir le registre). Après une entrée écartée,
/// une entrée qui laisserait un trou est écartée aussi, et journalisée : le serveur lit ce que
/// lisent les navigateurs.
#[tokio::test]
async fn entry_left_with_a_gap_after_a_set_aside_one_is_set_aside_too() {
    let logs = Logs::capture();
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Journal", &[user(&team.olga, Level::Full)])
        .await;
    let (created, creation) = PageDoc::new_page();
    let paragraph = created.read_blocks()[0].id.clone();
    let a = PageDoc::from_updates([creation.as_slice()]).unwrap();
    let one = a.replace_text(&paragraph, 0, 0, "Un").unwrap();
    let c = PageDoc::from_updates([creation.as_slice(), one.as_slice()]).unwrap();
    let abc = c.replace_text(&paragraph, 2, 0, "abc").unwrap();
    let def = c.replace_text(&paragraph, 0, 0, "def").unwrap();
    // « abc » rendu hostile : une seconde entrée pour son client (un `Skip` d'une unité).
    let mut at = 0;
    assert_eq!(read_var(&abc, &mut at), 1, "un seul client");
    let _structs = read_var(&abc, &mut at);
    let client = read_var(&abc, &mut at);
    assert_eq!(abc.last(), Some(&0), "aucune suppression");
    let mut hostile = vec![2];
    hostile.extend(&abc[1..abc.len() - 1]);
    hostile.push(1);
    var(&mut hostile, client);
    var(&mut hostile, 100);
    hostile.extend([10, 1, 0]);
    assert!(PageDoc::repeats_a_client(&hostile));
    team.store
        .append_updates(page, &[&creation, &one, &hostile, &def], None)
        .await
        .unwrap();

    let read = team.olga.get_mmd(&page.to_string()).await;
    read.assert_status(StatusCode::OK);
    assert_eq!(read.text(), "Un\n");
    let journal = logs.text();
    assert!(
        journal.matches("écartée").count() >= 2 && journal.contains("trou"),
        "deux lignes `error`, la seconde pour le trou : {journal}"
    );
}
