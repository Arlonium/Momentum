//! Les fichiers d'une page (Rulings R244, R245) : un contenu rangé sous son SHA-256, partagé par
//! toutes les attaches qui le nomment ; une attache ne se lit que par sa page ; une attache se
//! recopie dans une autre page sans renvoyer d'octets ; le ménage ne retire que les contenus
//! qu'aucune page n'attache plus.

mod common;

use common::{Fixture, account, fresh, int, page};
use mementum_store::{Attachment, NewAttachment, StoreError};
use tokio::sync::broadcast::error::TryRecvError;
use uuid::Uuid;

/// Une page neuve, à la racine de l'espace de départ.
async fn new_page(f: &Fixture, title: &str) -> Uuid {
    f.store
        .create_node(page(&f.space, None, None, title))
        .await
        .unwrap()
        .id
}

/// Une attache neuve de `hash` sur `node_id`, au nom de `by`.
fn new_attachment<'a>(
    node_id: Uuid,
    hash: [u8; 32],
    size: i64,
    mime: &'a str,
    name: &'a str,
    by: Uuid,
) -> NewAttachment<'a> {
    NewAttachment {
        id: Uuid::now_v7(),
        node_id,
        hash,
        size,
        mime,
        name,
        created_by: by,
    }
}

/// Supprime la page pour de bon (corbeille, puis suppression).
async fn delete_page(f: &Fixture, id: Uuid, by: Uuid) {
    f.store.trash_node(id, by).await.unwrap();
    f.store.delete_node(id).await.unwrap();
}

async fn files(f: &Fixture) -> i64 {
    int(&f.db, "SELECT COUNT(*) FROM files").await
}

async fn attachments(f: &Fixture) -> i64 {
    int(&f.db, "SELECT COUNT(*) FROM attachments").await
}

/// Le même contenu attaché à deux pages : une ligne `files`, deux attaches, chacune avec son nom
/// et son identifiant. La ligne `files` déjà là n'est pas récrite : la seconde attache rend la
/// taille et le type gardés, pas ceux qu'on lui donne (R267). Attacher un fichier n'avance ni la
/// révision ni la version de l'espace, et n'annonce rien : ni les droits ni la barre latérale ne
/// changent.
#[tokio::test]
async fn same_content_attached_twice_is_one_file_and_two_attachments() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let (a, b) = (new_page(&f, "A").await, new_page(&f, "B").await);
    let (rev, lsn) = (
        f.store.space_rev(f.space.id).await.unwrap(),
        f.store.space_lsn(f.space.id).await.unwrap(),
    );
    let mut changes = f.store.changes();

    let hash = [0xab; 32];
    let first = f
        .store
        .attach_file(
            new_attachment(a, hash, 10, "image/png", "photo.png", olga),
            1_000,
        )
        .await
        .unwrap();
    assert_eq!(
        (
            first.node_id,
            first.hash,
            first.size,
            first.mime.as_str(),
            first.name.as_str()
        ),
        (a, hash, 10, "image/png", "photo.png")
    );
    let second = f
        .store
        .attach_file(
            new_attachment(b, hash, 99, "text/html", "autre nom.png", olga),
            2_000,
        )
        .await
        .unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(
        (
            second.node_id,
            second.hash,
            second.size,
            second.mime.as_str(),
            second.name.as_str()
        ),
        (b, hash, 10, "image/png", "autre nom.png")
    );

    assert_eq!(files(&f).await, 1);
    assert_eq!(attachments(&f).await, 2);
    assert_eq!(
        int(&f.db, "SELECT created_at FROM files").await,
        1_000,
        "la ligne du contenu n'est pas récrite"
    );
    assert_eq!(f.store.attachment(a, first.id).await.unwrap(), Some(first));
    assert_eq!(
        f.store.attachment(b, second.id).await.unwrap(),
        Some(second)
    );
    assert_eq!(f.store.known_files().await.unwrap(), [hash]);

    assert_eq!(f.store.space_rev(f.space.id).await.unwrap(), rev);
    assert_eq!(f.store.space_lsn(f.space.id).await.unwrap(), lsn);
    assert_eq!(changes.try_recv(), Err(TryRecvError::Empty));
}

/// Une attache ne se lit que par sa page : par une autre page, par une page inconnue, ou sous un
/// identifiant inconnu, rien.
#[tokio::test]
async fn attachment_is_read_only_through_its_page() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let (a, b) = (new_page(&f, "A").await, new_page(&f, "B").await);
    let attached = f
        .store
        .attach_file(
            new_attachment(a, [1; 32], 3, "application/pdf", "plan.pdf", olga),
            1_000,
        )
        .await
        .unwrap();

    assert_eq!(
        f.store.attachment(a, attached.id).await.unwrap(),
        Some(attached.clone())
    );
    assert_eq!(f.store.attachment(b, attached.id).await.unwrap(), None);
    assert_eq!(
        f.store
            .attachment(Uuid::now_v7(), attached.id)
            .await
            .unwrap(),
        None
    );
    assert_eq!(f.store.attachment(a, Uuid::now_v7()).await.unwrap(), None);
}

/// Une attache sur une page inconnue, ou au nom d'un compte inconnu : `NotFound`, et rien n'est
/// écrit, pas même la ligne du contenu. Témoin : la même attache sur une vraie page s'écrit.
#[tokio::test]
async fn attach_to_a_missing_page_writes_nothing() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let a = new_page(&f, "A").await;
    let hash = [2; 32];

    for (what, node, by) in [
        ("page inconnue", Uuid::now_v7(), olga),
        ("compte inconnu", a, Uuid::now_v7()),
    ] {
        let refused = f
            .store
            .attach_file(
                new_attachment(node, hash, 3, "image/gif", "x.gif", by),
                1_000,
            )
            .await;
        assert!(matches!(refused, Err(StoreError::NotFound)), "{what}");
        assert_eq!(files(&f).await, 0, "{what}");
        assert_eq!(attachments(&f).await, 0, "{what}");
    }

    f.store
        .attach_file(
            new_attachment(a, hash, 3, "image/gif", "x.gif", olga),
            1_000,
        )
        .await
        .unwrap();
    assert_eq!((files(&f).await, attachments(&f).await), (1, 1));
}

/// Recopier une attache dans une autre page : une attache neuve, sur cette page, sous
/// l'identifiant donné, au même contenu et au même nom ; la ligne du contenu est partagée. Une
/// source qui n'est pas de la page nommée, ou inconnue : `None`, et rien n'est écrit. Une page de
/// destination inconnue : `NotFound`, rien n'est écrit.
#[tokio::test]
async fn copy_attachment_shares_the_file() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let bob = account(&f.store, "bob").await.id;
    let (a, b, c) = (
        new_page(&f, "A").await,
        new_page(&f, "B").await,
        new_page(&f, "C").await,
    );
    let source = f
        .store
        .attach_file(
            new_attachment(a, [3; 32], 7, "image/webp", "chat.webp", olga),
            1_000,
        )
        .await
        .unwrap();

    let id = Uuid::now_v7();
    let copy = f
        .store
        .copy_attachment(a, source.id, id, b, bob, 2_000)
        .await
        .unwrap()
        .expect("la source est de la page A");
    assert_eq!(
        copy,
        Attachment {
            id,
            node_id: b,
            hash: source.hash,
            size: source.size,
            mime: source.mime.clone(),
            name: source.name.clone(),
        }
    );
    assert_eq!(f.store.attachment(b, id).await.unwrap(), Some(copy));
    assert_eq!(
        f.store.attachment(a, source.id).await.unwrap(),
        Some(source.clone())
    );
    assert_eq!((files(&f).await, attachments(&f).await), (1, 2));
    assert_eq!(
        int(
            &f.db,
            &format!("SELECT created_at FROM attachments WHERE id = '{id}'")
        )
        .await,
        2_000
    );

    // La source nommée par une autre page que la sienne, ou inconnue : rien.
    for (what, from_node, from_id) in [
        ("source d'une autre page", c, source.id),
        ("source inconnue", a, Uuid::now_v7()),
        ("page d'origine inconnue", Uuid::now_v7(), source.id),
    ] {
        let none = f
            .store
            .copy_attachment(from_node, from_id, Uuid::now_v7(), c, bob, 3_000)
            .await
            .unwrap();
        assert_eq!(none, None, "{what}");
        assert_eq!(attachments(&f).await, 2, "{what}");
    }

    // Une page de destination inconnue : `NotFound`.
    let refused = f
        .store
        .copy_attachment(a, source.id, Uuid::now_v7(), Uuid::now_v7(), bob, 3_000)
        .await;
    assert!(matches!(refused, Err(StoreError::NotFound)));
    assert_eq!((files(&f).await, attachments(&f).await), (1, 2));
}

/// Deux pages attachent le même contenu, une troisième un autre. La première supprimée, le
/// ménage ne rend rien et la ligne reste ; la seconde supprimée, il rend ce contenu, une fois, et
/// sa ligne est partie ; celui de la troisième reste.
#[tokio::test]
async fn sweep_returns_only_what_no_page_keeps() {
    let f = fresh().await;
    let olga = account(&f.store, "olga").await.id;
    let (a, b, c) = (
        new_page(&f, "A").await,
        new_page(&f, "B").await,
        new_page(&f, "C").await,
    );
    let (shared, kept) = ([4; 32], [5; 32]);
    for (node, hash) in [(a, shared), (b, shared), (c, kept)] {
        f.store
            .attach_file(
                new_attachment(node, hash, 1, "audio/ogg", "son.ogg", olga),
                1_000,
            )
            .await
            .unwrap();
    }
    assert_eq!(f.store.sweep_files().await.unwrap(), Vec::<[u8; 32]>::new());
    assert_eq!(f.store.known_files().await.unwrap(), [shared, kept]);

    delete_page(&f, a, olga).await;
    assert_eq!(f.store.sweep_files().await.unwrap(), Vec::<[u8; 32]>::new());
    assert_eq!(f.store.known_files().await.unwrap(), [shared, kept]);

    delete_page(&f, b, olga).await;
    assert_eq!(f.store.sweep_files().await.unwrap(), [shared]);
    assert_eq!(f.store.known_files().await.unwrap(), [kept]);
    assert_eq!(f.store.sweep_files().await.unwrap(), Vec::<[u8; 32]>::new());
    assert_eq!((files(&f).await, attachments(&f).await), (1, 1));
}
