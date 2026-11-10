//! Les droits qui changent pendant qu'une connexion s'ouvre ou qu'elle est
//! ouverte, vus du hub : les connexions d'un compte fermées à la demande
//! (`kick_user`, Ruling R73), et la connexion inscrite après un changement de
//! droits, fermée dès son inscription (Ruling R71).
//!
//! Le reste des droits (lecture seule, `kick`, `forget`, écritures
//! attribuées) est dans `sync.rs`.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use futures::{SinkExt, StreamExt};
use mementum_md::parse;
use mementum_schema::SCHEMA_VERSION;
use mementum_store::{NewNode, NodeKind, Store};
use mementum_sync::{ConnInfo, SyncConfig, SyncError};
use uuid::Uuid;
use yrs::sync::{Message, SyncMessage};
use yrs::updates::encoder::Encode;

/// Une écriture de structure dans l'espace (une page de plus), comme il en
/// passe à tout moment : `spaces.rev` avance d'un cran.
async fn structure_write(store: &Store, space: Uuid) {
    store
        .create_node(NewNode {
            space_id: space,
            parent_id: None,
            after: None,
            kind: NodeKind::Page,
            title: "Une de plus".into(),
            created_by: None,
            grants: Vec::new(),
            doc: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn kick_user_closes_only_that_account() {
    let f = Fixture::new(SyncConfig::default()).await;
    let elsewhere = new_page(&f.store).await;
    let alice = new_user(&f.db).await;
    let bob = new_user(&f.db).await;
    let of = |account: Uuid| ConnInfo {
        actor: Some(account),
        ..writing_conn()
    };
    let id = f.ids().await[0].clone();
    // Alice : deux connexions sur la page, une sur une autre page. Bob et
    // une connexion sans compte : sur la page.
    let alice_1 = Client::connect_synced(&f.url_for(of(alice))).await;
    let alice_2 = Client::connect_synced(&f.url_for(of(alice))).await;
    let alice_elsewhere = Client::connect_synced(&f.server.url_for(elsewhere, of(alice))).await;
    let bob_1 = Client::connect_synced(&f.url_for(of(bob))).await;
    let nobody = f.client().await;
    alice_1.type_text(&id, "avant");
    processed(&alice_1).await;
    assert!(
        bob_1
            .wait_until(WAIT, |c| text(&c.blocks()) == "avant")
            .await
    );
    let rows = row_counts(&f.db).await;
    let mut pages = vec![f.page, elsewhere];
    pages.sort();
    assert_eq!(live(&f.hub).await, pages);

    f.hub.kick_user(alice).await;
    // Au retour, les acteurs ne traitent plus rien de ses connexions : ce
    // qu'elle tape avant d'avoir vu la fermeture n'arrive pas au serveur.
    alice_1.type_text(&id, " perdu");
    for (client, which) in [
        (&alice_1, "la première"),
        (&alice_2, "la seconde"),
        (&alice_elsewhere, "celle de l'autre page"),
    ] {
        assert!(
            client.wait_until(WAIT, |c| c.is_closed()).await,
            "connexion d'Alice encore ouverte : {which}"
        );
    }
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");

    // Les autres comptes, et la connexion sans compte, sont toujours servis :
    // Bob écrit, l'autre le reçoit, et ni l'un ni l'autre n'a vu « perdu ».
    bob_1.type_text(&id, " après");
    processed(&bob_1).await;
    assert!(
        nobody
            .wait_until(WAIT, |c| text(&c.blocks()) == "avant après")
            .await,
        "la connexion sans compte ne reçoit plus rien"
    );
    assert_eq!(text(&bob_1.blocks()), "avant après");
    assert!(!bob_1.is_closed() && !nobody.is_closed());
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "avant après"
    );
    // Les acteurs sont restés, avec leur document.
    assert_eq!(live(&f.hub).await, pages);

    // Un compte sans connexion : rien à fermer, et la fonction rend la main.
    let returned = tokio::time::timeout(WAIT, f.hub.kick_user(Uuid::now_v7())).await;
    assert!(returned.is_ok(), "kick_user rend la main");
    processed(&bob_1).await;
    assert!(!bob_1.is_closed() && !nobody.is_closed());

    // Le hub ne retient rien : Alice revient (la poignée de main aura relu
    // sa session), et elle est servie.
    let back = Client::connect_synced(&f.url_for(of(alice))).await;
    back.type_text(&id, " !");
    processed(&back).await;
    assert_eq!(
        text(&f.hub.read_blocks(f.page).await.unwrap()),
        "avant après !"
    );
    assert!(!back.is_closed());
}

#[tokio::test]
async fn connection_joined_after_a_rights_change_is_closed() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Bonjour\n").await;
    let id = f.ids().await[0].clone();
    let space = f.store.get_node(f.page).await.unwrap().unwrap().space_id;
    let at = |rev: i64| ConnInfo {
        space: Some((space, rev)),
        ..writing_conn()
    };
    // Ce que la route a lu avant les droits ; puis une écriture de l'espace
    // passe, avant que la connexion soit inscrite.
    let seen = f.store.space_rev(space).await.unwrap().unwrap();
    structure_write(&f.store, space).await;
    assert_eq!(f.store.space_rev(space).await.unwrap(), Some(seen + 1));
    let observer = f.client().await;
    let rows = row_counts(&f.db).await;

    // Une mise à jour que le client envoie dès l'ouverture : elle attend déjà
    // dans la socket quand la connexion est servie.
    let update = observer
        .page()
        .unwrap()
        .write_blocks(&parse("Périmé\n").unwrap())
        .unwrap();
    let (server_end, mut client_end) = memory_socket(8);
    client_end
        .send(Message::Sync(SyncMessage::Update(update)).encode_v1())
        .await
        .unwrap();
    let served = tokio::time::timeout(
        WAIT,
        f.hub.clone().serve_connection(f.page, at(seen), server_end),
    )
    .await
    .expect("fermée dès son inscription");
    assert!(served.is_ok(), "{served:?}");
    // Rien n'a été envoyé, pas même l'accueil : la socket est fermée, vide.
    assert_eq!(client_end.next().await, None);
    // Rien n'a été lu non plus : ni écrit, ni diffusé.
    processed(&observer).await;
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "Bonjour");
    assert_eq!(text(&observer.blocks()), "Bonjour", "rien de diffusé");

    // Par une vraie WebSocket : fermée sans avoir rien reçu.
    let stale = Client::connect(&f.url_for(at(seen))).await.unwrap();
    assert!(
        stale.wait_until(WAIT, |c| c.is_closed()).await,
        "fermée dès son inscription"
    );
    assert!(!stale.is_synced(), "elle n'a rien reçu");
    assert!(stale.blocks().is_empty());
    // Un espace qui n'existe plus : fermée de même.
    let gone = ConnInfo {
        space: Some((Uuid::now_v7(), 0)),
        ..writing_conn()
    };
    let orphan = Client::connect(&f.url_for(gone)).await.unwrap();
    assert!(orphan.wait_until(WAIT, |c| c.is_closed()).await);
    assert!(!orphan.is_synced());
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");

    // Avec la bonne révision, la connexion reste ouverte, et elle écrit.
    let fresh = Client::connect_synced(&f.url_for(at(seen + 1))).await;
    assert_eq!(text(&fresh.blocks()), "Bonjour");
    fresh.type_text(&id, " !");
    processed(&fresh).await;
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "Bonjour !"
    );
    // Le contrôle n'a lieu qu'à l'inscription : ensuite, c'est `kick` qui
    // ferme. Une écriture de l'espace sans `kick` ne ferme personne.
    structure_write(&f.store, space).await;
    fresh.type_text(&id, "!");
    processed(&fresh).await;
    assert!(!fresh.is_closed());
    assert_eq!(
        text(&f.hub.read_blocks(f.page).await.unwrap()),
        "Bonjour !!"
    );

    // Une révision que le store ne sait plus lire : dans le doute, fermée.
    exec_sql(
        &f.db,
        "ALTER TABLE spaces RENAME COLUMN rev TO rev_illisible",
    )
    .await;
    let unsure = Client::connect(&f.url_for(at(seen + 2))).await.unwrap();
    assert!(
        unsure
            .wait_until(Duration::from_secs(5), |c| c.is_closed())
            .await,
        "révision illisible : la connexion est fermée"
    );
    assert!(!unsure.is_synced());
    // Celle qui était inscrite n'est pas touchée.
    assert!(!fresh.is_closed());
}

/// Ruling R88 : une fois la connexion inscrite, le hub demande à l'appelant si
/// elle vaut encore (la route y relit la session). « Non » : fermée dès son
/// inscription, sans avoir rien reçu ni rien écrit. « Oui » : elle est servie.
#[tokio::test]
async fn connection_refused_by_the_caller_check_is_closed_at_join() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Bonjour\n").await;
    let observer = f.client().await;
    let rows = row_counts(&f.db).await;
    // Une mise à jour que le client envoie dès l'ouverture : elle attend déjà
    // dans la socket quand la connexion est servie.
    let update = observer
        .page()
        .unwrap()
        .write_blocks(&parse("Trop tard\n").unwrap())
        .unwrap();
    let update = Message::Sync(SyncMessage::Update(update)).encode_v1();
    let asked = Arc::new(AtomicUsize::new(0));
    let answer = |allowed: bool| {
        let asked = asked.clone();
        async move {
            asked.fetch_add(1, Ordering::SeqCst);
            allowed
        }
    };

    // Une page qui n'existe pas : rien n'est inscrit, la question n'est pas
    // posée.
    let (server_end, _client_end) = memory_socket(8);
    let served = f
        .hub
        .clone()
        .serve_connection_if(Uuid::now_v7(), writing_conn(), answer(true), server_end)
        .await;
    assert!(matches!(served, Err(SyncError::NotFound)), "{served:?}");
    assert_eq!(asked.load(Ordering::SeqCst), 0);

    // « Non ».
    let (server_end, mut client_end) = memory_socket(8);
    client_end.send(update.clone()).await.unwrap();
    let served = tokio::time::timeout(
        WAIT,
        f.hub
            .clone()
            .serve_connection_if(f.page, writing_conn(), answer(false), server_end),
    )
    .await
    .expect("fermée dès son inscription");
    assert!(served.is_ok(), "{served:?}");
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "la question est posée une fois"
    );
    // Rien n'a été envoyé, pas même l'accueil : la socket est fermée, vide.
    assert_eq!(client_end.next().await, None);
    // Rien n'a été lu non plus : ni écrit, ni diffusé.
    processed(&observer).await;
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
    assert_eq!(text(&f.hub.read_blocks(f.page).await.unwrap()), "Bonjour");
    assert_eq!(text(&observer.blocks()), "Bonjour", "rien de diffusé");

    // « Oui » : elle reçoit l'accueil, et ce qu'elle envoie est écrit et
    // diffusé.
    let (server_end, mut client_end) = memory_socket(8);
    let serving = tokio::spawn(f.hub.clone().serve_connection_if(
        f.page,
        writing_conn(),
        answer(true),
        server_end,
    ));
    let hello = tokio::time::timeout(WAIT, client_end.next())
        .await
        .expect("l'accueil arrive");
    assert!(hello.is_some(), "la socket est restée ouverte");
    assert_eq!(asked.load(Ordering::SeqCst), 2);
    client_end.send(update).await.unwrap();
    assert!(
        observer
            .wait_until(WAIT, |c| text(&c.blocks()) == "Trop tard")
            .await,
        "sa mise à jour n'a pas été diffusée"
    );
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "Trop tard"
    );
    assert!(!serving.is_finished(), "elle est toujours servie");
}

/// Ruling R89 : `load_page` lance l'acteur d'une page et attend son document,
/// sans y inscrire de connexion. Qui doit lire des droits frais juste avant
/// d'inscrire une connexion a ainsi le chargement derrière lui.
#[tokio::test]
async fn load_page_starts_the_actor_without_a_connection() {
    let f = Fixture::new(SyncConfig::default()).await;
    // Une page que personne n'a ouverte, et qui n'a pas encore de document.
    let cold = new_page(&f.store).await;
    assert_eq!(live(&f.hub).await, [f.page]);
    assert!(f.store.load_doc(cold).await.unwrap().snapshot.is_none());

    f.hub.load_page(cold).await.unwrap();
    let mut pages = vec![f.page, cold];
    pages.sort();
    assert_eq!(live(&f.hub).await, pages, "son acteur est lancé");
    // Au retour, le document est là : celui que l'acteur écrit à son premier
    // chargement (Ruling R28).
    let doc = f.store.load_doc(cold).await.unwrap();
    assert_eq!(doc.snapshot.map(|(seq, _)| seq), Some(0));
    let rows = row_counts(&f.db).await;

    // Déjà chargée : rien n'est écrit, rien n'est relancé.
    f.hub.load_page(cold).await.unwrap();
    f.hub.load_page(f.page).await.unwrap();
    assert_eq!(live(&f.hub).await, pages);
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
    // La connexion qui suit est servie par cet acteur.
    let client = Client::connect_synced(&f.server.url(cold, SCHEMA_VERSION)).await;
    assert_eq!(client.blocks().len(), 1);
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");

    // Une page qui n'existe pas : ni acteur ni écriture.
    let loaded = f.hub.load_page(Uuid::now_v7()).await;
    assert!(matches!(loaded, Err(SyncError::NotFound)), "{loaded:?}");
    assert_eq!(live(&f.hub).await, pages);
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
}
