//! Les favoris d'un compte (Ruling R254) : une ligne `(compte, page)`, rendue dans l'ordre où le
//! compte l'a ajoutée ; poser un favori déjà posé, ou retirer un favori absent, n'écrit rien ; un
//! favori ne change que la barre latérale de son auteur, et n'avance donc ni `rev` ni `lsn`.

mod common;

use common::{Fixture, account, fresh, int, page};
use mementum_store::StoreError;
use tokio::sync::broadcast::error::TryRecvError;
use uuid::Uuid;

/// Une page neuve de l'espace de départ, sous `parent` ou à la racine.
async fn new_page(f: &Fixture, parent: Option<Uuid>, title: &str) -> Uuid {
    f.store
        .create_node(page(&f.space, parent, None, title))
        .await
        .unwrap()
        .id
}

async fn favorite_rows(f: &Fixture) -> i64 {
    int(&f.db, "SELECT COUNT(*) FROM favorites").await
}

/// Poser deux fois le même favori : le premier appel le pose (`true`), le second ne change rien
/// (`false`) : ni une ligne de plus, ni sa date, ni sa place dans la liste.
#[tokio::test]
async fn favorite_twice_is_one_favorite() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let (a, b) = (new_page(&f, None, "A").await, new_page(&f, None, "B").await);

    assert!(f.store.set_favorite(olga, a, true, 1_000).await.unwrap());
    assert!(f.store.set_favorite(olga, b, true, 2_000).await.unwrap());
    assert!(!f.store.set_favorite(olga, a, true, 3_000).await.unwrap());

    assert_eq!(favorite_rows(&f).await, 2);
    assert_eq!(f.store.favorites(olga, f.space.id).await.unwrap(), [a, b]);
    assert_eq!(
        int(
            &f.db,
            &format!("SELECT created_at FROM favorites WHERE node_id = '{a}'")
        )
        .await,
        1_000
    );
}

/// Poser puis retirer un favori n'avance ni la révision ni la version de l'espace, et n'annonce
/// rien sur `changes()` (R254). Témoin : un renommage fait ensuite avance la version d'un pas et
/// s'annonce.
#[tokio::test]
async fn favorite_advances_neither_rev_nor_lsn() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let a = new_page(&f, None, "A").await;
    let rev = f.store.space_rev(f.space.id).await.unwrap().unwrap();
    let lsn = f.store.space_lsn(f.space.id).await.unwrap().unwrap();
    let mut changes = f.store.changes();

    assert!(f.store.set_favorite(olga, a, true, 1_000).await.unwrap());
    assert!(f.store.set_favorite(olga, a, false, 2_000).await.unwrap());
    assert_eq!(f.store.space_rev(f.space.id).await.unwrap(), Some(rev));
    assert_eq!(f.store.space_lsn(f.space.id).await.unwrap(), Some(lsn));
    assert_eq!(changes.try_recv(), Err(TryRecvError::Empty));

    f.store
        .rename_node(a, "Renommée", Some(olga))
        .await
        .unwrap();
    assert_eq!(f.store.space_lsn(f.space.id).await.unwrap(), Some(lsn + 1));
    assert_eq!(changes.try_recv(), Ok(f.space.id));
}

/// Chaque compte a ses favoris, rendus dans l'ordre où il les a ajoutés ; à la même date,
/// l'identifiant de la page départage. Un favori d'un autre espace n'est pas dans la liste de
/// celui-ci.
#[tokio::test]
async fn favorites_are_per_account_in_the_order_added() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let bob = account(&f.store, "bob").await.id;
    // UUIDv7 : `a` est plus petit que `b`, créée avant elle.
    let a = new_page(&f, None, "A").await;
    let b = new_page(&f, None, "B").await;
    let c = new_page(&f, None, "C").await;
    let elsewhere = f.store.create_space("Ailleurs", None).await.unwrap();
    let far = f
        .store
        .create_node(page(&elsewhere, None, None, "Loin"))
        .await
        .unwrap()
        .id;

    for (who, node, at) in [
        (olga, c, 1_000),
        (olga, b, 2_000),
        (olga, a, 2_000),
        (olga, far, 2_500),
        (bob, b, 3_000),
    ] {
        assert!(f.store.set_favorite(who, node, true, at).await.unwrap());
    }

    assert_eq!(
        f.store.favorites(olga, f.space.id).await.unwrap(),
        [c, a, b]
    );
    assert_eq!(f.store.favorites(bob, f.space.id).await.unwrap(), [b]);
    assert_eq!(f.store.favorites(olga, elsewhere.id).await.unwrap(), [far]);
    assert_eq!(
        f.store.favorites(bob, elsewhere.id).await.unwrap(),
        Vec::<Uuid>::new()
    );
}

/// Les favoris d'une page supprimée pour de bon partent avec elle, et avec ses sous-pages. À la
/// corbeille, ils restent (c'est la route qui ne montre pas une page à la corbeille).
#[tokio::test]
async fn favorites_of_a_deleted_page_are_gone() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let parent = new_page(&f, None, "Parent").await;
    let child = new_page(&f, Some(parent), "Enfant").await;
    let other = new_page(&f, None, "Autre").await;
    for (node, at) in [(parent, 1_000), (child, 2_000), (other, 3_000)] {
        assert!(f.store.set_favorite(olga, node, true, at).await.unwrap());
    }

    f.store.trash_node(parent, olga).await.unwrap();
    assert_eq!(
        f.store.favorites(olga, f.space.id).await.unwrap(),
        [parent, child, other]
    );

    f.store.delete_node(parent).await.unwrap();
    assert_eq!(f.store.favorites(olga, f.space.id).await.unwrap(), [other]);
    assert_eq!(favorite_rows(&f).await, 1);
}

/// Retirer un favori qui n'est pas là (jamais posé, ou posé par un autre compte) rend `false` et
/// n'écrit rien : les favoris de chacun restent tels quels.
#[tokio::test]
async fn removing_a_favorite_that_is_not_there_writes_nothing() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let bob = account(&f.store, "bob").await.id;
    let (a, b, c) = (
        new_page(&f, None, "A").await,
        new_page(&f, None, "B").await,
        new_page(&f, None, "C").await,
    );
    assert!(f.store.set_favorite(olga, a, true, 1_000).await.unwrap());
    assert!(f.store.set_favorite(bob, b, true, 2_000).await.unwrap());

    assert!(!f.store.set_favorite(olga, b, false, 3_000).await.unwrap());
    assert!(!f.store.set_favorite(olga, c, false, 3_000).await.unwrap());

    assert_eq!(favorite_rows(&f).await, 2);
    assert_eq!(f.store.favorites(olga, f.space.id).await.unwrap(), [a]);
    assert_eq!(f.store.favorites(bob, f.space.id).await.unwrap(), [b]);
}

/// Une page inconnue : `NotFound`, pour poser comme pour retirer, et rien n'est écrit ; de même
/// pour poser au nom d'un compte inconnu.
#[tokio::test]
async fn favorite_of_an_unknown_page_is_not_found() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let a = new_page(&f, None, "A").await;

    for on in [true, false] {
        let refused = f.store.set_favorite(olga, Uuid::now_v7(), on, 1_000).await;
        assert!(matches!(refused, Err(StoreError::NotFound)), "on = {on}");
    }
    let refused = f.store.set_favorite(Uuid::now_v7(), a, true, 1_000).await;
    assert!(matches!(refused, Err(StoreError::NotFound)));
    assert_eq!(favorite_rows(&f).await, 0);
}

/// Poser un favori déjà posé, ou retirer un favori absent, ne prend pas le verrou d'écriture de
/// SQLite, unique pour toute l'instance (Ruling R283, la règle de R270) : l'appel rend `false`
/// pendant qu'une autre connexion tient ce verrou. Sinon un simple lecteur, en redemandant ce
/// qu'il a déjà, ferait attendre le journal de chaque page ouverte. Témoin : sous le même verrou,
/// un favori neuf attend, puis renonce au bout du délai de la connexion ; le verrou rendu, il se
/// pose.
#[tokio::test]
async fn favorite_without_effect_takes_no_write_lock() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let (a, b) = (new_page(&f, None, "A").await, new_page(&f, None, "B").await);
    assert!(f.store.set_favorite(olga, a, true, 1_000).await.unwrap());

    // Une autre connexion tient le verrou d'écriture.
    let side = common::side_pool(&f.db).await;
    let mut holder = side.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *holder)
        .await
        .unwrap();

    // Sans effet : rendu sans attendre, et rien n'est écrit.
    assert!(!f.store.set_favorite(olga, a, true, 2_000).await.unwrap());
    assert!(!f.store.set_favorite(olga, b, false, 2_000).await.unwrap());
    // Une page inconnue se dit toujours, sans verrou non plus.
    let unknown = f
        .store
        .set_favorite(olga, Uuid::now_v7(), false, 2_000)
        .await;
    assert!(matches!(unknown, Err(StoreError::NotFound)));
    // Témoin : ce verrou arrête bien une écriture. Un favori neuf l'attend, en vain.
    let refused = f.store.set_favorite(olga, b, true, 2_000).await;
    let why = format!("{refused:?}");
    assert!(
        matches!(refused, Err(StoreError::Sqlx(_))) && why.contains("database is locked"),
        "{why}"
    );
    sqlx::query("ROLLBACK").execute(&mut *holder).await.unwrap();
    drop(holder);
    side.close().await;

    // Le verrou rendu, il se pose ; le premier a gardé sa date.
    assert!(f.store.set_favorite(olga, b, true, 3_000).await.unwrap());
    assert_eq!(f.store.favorites(olga, f.space.id).await.unwrap(), [a, b]);
    assert_eq!(
        int(
            &f.db,
            &format!("SELECT created_at FROM favorites WHERE node_id = '{a}'")
        )
        .await,
        1_000
    );
}
