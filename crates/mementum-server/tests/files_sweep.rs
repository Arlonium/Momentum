//! Le ménage des fichiers (`Store::sweep_files`, Ruling R270) lit ses candidats sans écrire,
//! puis ne supprime que ceux qu'aucune attache ne nomme encore : une attache écrite entre sa
//! lecture et sa suppression garde son contenu, et le ménage aboutit.
//!
//! Le moment où le ménage a lu se voit dans le journal des requêtes de sqlx (cible
//! `sqlx::query`, une ligne par instruction, son SQL en entier, écrite à la fin de son
//! exécution), écrit par le fil de SQLite : il faut l'abonné de tout le processus
//! ([`all_logs`]). Ce fichier n'a qu'un test, pour que les instructions d'un autre ne s'y
//! mêlent pas (Ruling R266).

mod common;

use std::time::{Duration, Instant};

use common::all_logs;
use mementum_store::{NewAttachment, NewNode, NewUser, NodeKind, Store};
use uuid::Uuid;

/// Ce que le journal porte de la lecture des candidats, et d'elle seule : ni `known_files`
/// (`… FROM files ORDER BY hash`) ni la suppression (`DELETE FROM files WHERE hash IN …`).
const CANDIDATES_READ: &str = "SELECT hash FROM files WHERE NOT EXISTS";

fn page(space: Uuid, title: &str) -> NewNode {
    NewNode {
        space_id: space,
        parent_id: None,
        after: None,
        kind: NodeKind::Page,
        title: title.to_string(),
        created_by: None,
        grants: Vec::new(),
        doc: None,
    }
}

/// Le contenu H n'est plus attaché à aucune page (sa page est supprimée). Une autre connexion
/// tient le verrou d'écriture et y attache H à la page B, sans valider. Le ménage lit H parmi ses
/// candidats (la lecture ne voit pas l'attache en cours), puis attend le verrou pour le
/// supprimer ; l'autre connexion valide alors. Le ménage aboutit sans rien retirer, et le
/// contenu reste, attaché à B. Sans la revérification de la suppression, la clé étrangère
/// sans cascade la refusait : rien n'était perdu, mais tout le ménage échouait.
///
/// Plancher : si le journal de sqlx changeait de forme, la lecture des candidats n'y paraîtrait
/// plus, et le test échouerait au bout de sa borne, au lieu de passer.
#[tokio::test]
async fn sweep_keeps_a_content_attached_between_its_read_and_its_delete() {
    // Avant toute requête : les journaux de tous les fils, au niveau TRACE.
    let logs = all_logs();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mementum.db");
    let store = Store::open(&db).await.unwrap();
    let space = store.create_space("Essais", None).await.unwrap().id;
    let olga = store
        .create_user(NewUser {
            email: "olga@exemple.test".to_string(),
            name: "Olga".to_string(),
            password_hash: "pas-un-hachage".to_string(),
        })
        .await
        .unwrap()
        .id;
    let a = store.create_node(page(space, "A")).await.unwrap().id;
    let b = store.create_node(page(space, "B")).await.unwrap().id;
    let hash = [9; 32];
    store
        .attach_file(
            NewAttachment {
                id: Uuid::now_v7(),
                node_id: a,
                hash,
                size: 1,
                mime: "image/png",
                name: "x.png",
                created_by: olga,
            },
            1_000,
        )
        .await
        .unwrap();
    store.trash_node(a, olga).await.unwrap();
    store.delete_node(a).await.unwrap();

    // Une autre connexion attache H à B, sans valider : elle tient le verrou d'écriture.
    let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    let mut holder = side.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *holder)
        .await
        .unwrap();
    let kept = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO attachments (id, node_id, file_hash, name, created_by, created_at) \
         VALUES (?, ?, ?, 'x.png', ?, 2000)",
    )
    .bind(kept.to_string())
    .bind(b.to_string())
    .bind(&hash[..])
    .bind(olga.to_string())
    .execute(&mut *holder)
    .await
    .unwrap();

    let start = logs.text().len();
    let sweep = tokio::spawn({
        let store = store.clone();
        async move { store.sweep_files().await }
    });
    // Le ménage a lu ses candidats ; il attend maintenant le verrou (5 s au plus, le délai
    // d'attente de `Store::open`). Une borne large, qui ne peut que faire échouer à tort.
    let deadline = Instant::now() + Duration::from_secs(4);
    while !logs.text()[start..].contains(CANDIDATES_READ) {
        assert!(
            Instant::now() < deadline,
            "la lecture des candidats n'a jamais paru au journal de sqlx"
        );
        assert!(!sweep.is_finished(), "le ménage a fini sans lire");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    sqlx::query("COMMIT").execute(&mut *holder).await.unwrap();
    drop(holder);
    side.close().await;

    assert_eq!(
        sweep.await.unwrap().expect("le ménage aboutit"),
        Vec::<[u8; 32]>::new()
    );
    assert_eq!(store.known_files().await.unwrap(), [hash]);
    let attachment = store.attachment(b, kept).await.unwrap().expect("attache");
    assert_eq!(attachment.hash, hash);
    store.close().await;
}
