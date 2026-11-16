//! La copie d'un sous-arbre de pages (Ruling R252) : nœuds, accès, documents et attaches en une
//! transaction, entière ou pas du tout ; la révision et la version de l'espace avancées une fois ;
//! la forme et l'ordre du sous-arbre gardés ; et ce sur quoi l'appelant a décidé, relu dans la
//! transaction (Ruling R590).

mod common;

use common::{Fixture, account, fresh, int, page, texts};
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_store::{
    CopiedAttachment, CopiedPage, NewAttachment, NewCopy, NewNode, NodeKind, Sibling, StoreError,
};
use tokio::sync::broadcast::error::TryRecvError;
use uuid::Uuid;

fn grant(subject: Subject, level: Level) -> Grant {
    Grant { subject, level }
}

/// Un compte, membre de l'espace de départ au rôle `role`.
async fn member(f: &Fixture, tag: &str, role: Role) -> Uuid {
    let id = account(&f.store, tag).await.id;
    f.store.add_member(f.space.id, id, role).await.unwrap();
    id
}

/// Une page de l'espace de départ, ouverte aux membres (`edit`) si elle est à la racine.
async fn team_page(f: &Fixture, parent: Option<Uuid>, after: Option<Uuid>, title: &str) -> Uuid {
    let grants = match parent {
        None => vec![grant(Subject::Members, Level::Edit)],
        Some(_) => Vec::new(),
    };
    f.store
        .create_node(NewNode {
            grants,
            ..page(&f.space, parent, after, title)
        })
        .await
        .unwrap()
        .id
}

fn copied(parent: Option<usize>, title: &str, position: Option<&str>) -> CopiedPage {
    CopiedPage {
        id: Uuid::now_v7(),
        parent,
        title: title.to_string(),
        position: position.map(str::to_string),
        doc: title.as_bytes().to_vec(),
        attachments: Vec::new(),
    }
}

/// La copie de `source`, posée juste après elle sous sa page parente, sans accès propre.
fn beside(
    f: &Fixture,
    source: Uuid,
    parent: Option<Uuid>,
    by: Uuid,
    pages: Vec<CopiedPage>,
) -> NewCopy {
    NewCopy {
        space_id: f.space.id,
        source,
        parent_id: parent,
        after: Some(Sibling::Node(source)),
        grants: Vec::new(),
        created_by: by,
        pages,
    }
}

/// La copie de `source`, à la racine, en dernier, privée à `by`.
fn private(f: &Fixture, source: Uuid, by: Uuid, pages: Vec<CopiedPage>) -> NewCopy {
    NewCopy {
        space_id: f.space.id,
        source,
        parent_id: None,
        after: Some(Sibling::Last),
        grants: vec![grant(Subject::User(by), Level::Full)],
        created_by: by,
        pages,
    }
}

async fn count(f: &Fixture, table: &str) -> i64 {
    int(&f.db, &format!("SELECT COUNT(*) FROM {table}")).await
}

async fn rev(f: &Fixture) -> i64 {
    f.store.space_rev(f.space.id).await.unwrap().unwrap()
}

async fn lsn(f: &Fixture) -> i64 {
    f.store.space_lsn(f.space.id).await.unwrap().unwrap()
}

/// Ce que la base garde : nœuds, instantanés, lignes d'accès, attaches, révision.
async fn written(f: &Fixture) -> (i64, i64, i64, i64, i64) {
    (
        count(f, "nodes").await,
        count(f, "doc_snapshots").await,
        count(f, "acl").await,
        count(f, "attachments").await,
        rev(f).await,
    )
}

/// Une copie de trois pages dont la troisième attache un contenu que la base ne connaît pas :
/// `NotFound`, et rien n'est écrit, ni nœud, ni document, ni accès, ni attache ; la révision n'a
/// pas bougé et rien n'est annoncé. Jamais de ligne `files` créée (Ruling R579). Témoin : avec un
/// contenu connu, la même copie est écrite entière.
#[tokio::test]
async fn copy_is_written_whole_or_not_at_all() {
    let f = fresh().await;
    let mona = member(&f, "mona", Role::Member).await;
    let parent = team_page(&f, None, None, "Projet").await;
    let source = team_page(&f, Some(parent), None, "Notes").await;
    let known = [7u8; 32];
    f.store
        .attach_file(
            NewAttachment {
                id: Uuid::now_v7(),
                node_id: source,
                hash: known,
                size: 3,
                mime: "image/png",
                name: "photo.png",
                created_by: mona,
            },
            1,
        )
        .await
        .unwrap();

    let pages = |hash: [u8; 32]| {
        let mut third = copied(Some(1), "Petite-fille", Some("a0"));
        third.attachments.push(CopiedAttachment {
            id: Uuid::now_v7(),
            hash,
            name: "photo.png".to_string(),
        });
        vec![
            copied(None, "Notes (copie)", None),
            copied(Some(0), "Fille", Some("a0")),
            third,
        ]
    };
    let before = written(&f).await;
    let files = count(&f, "files").await;
    let mut changes = f.store.changes();

    let unknown = beside(&f, source, Some(parent), mona, pages([9u8; 32]));
    let refused = f.store.duplicate_pages(&unknown, 5).await;
    assert!(matches!(refused, Err(StoreError::NotFound)), "{refused:?}");
    assert_eq!(written(&f).await, before, "rien d'écrit");
    assert_eq!(
        count(&f, "files").await,
        files,
        "aucune ligne de contenu créée"
    );
    assert!(matches!(changes.try_recv(), Err(TryRecvError::Empty)));

    // Témoin : le contenu connu, la copie passe entière.
    let whole = beside(&f, source, Some(parent), mona, pages(known));
    let root = f.store.duplicate_pages(&whole, 5).await.unwrap();
    assert_eq!(root.id, whole.pages[0].id);
    assert_eq!(root.title, "Notes (copie)");
    assert_eq!(root.parent_id, Some(parent));
    assert_eq!(root.created_by, Some(mona));
    let after = written(&f).await;
    assert_eq!(
        (after.0, after.1, after.2, after.3),
        (before.0 + 3, before.1 + 3, before.2, before.3 + 1)
    );
    assert_eq!(count(&f, "files").await, files);
    // Chaque document est l'instantané de `seq` 0 de sa page, et l'attache est sur la troisième.
    for copy in &whole.pages {
        let state = f.store.load_doc(copy.id).await.unwrap();
        assert_eq!(state.snapshot, Some((0, copy.doc.clone())));
        assert!(state.updates.is_empty());
    }
    let attachment = f
        .store
        .attachment(whole.pages[2].id, whole.pages[2].attachments[0].id)
        .await
        .unwrap()
        .expect("l'attache de la copie");
    assert_eq!(
        (attachment.hash, attachment.name.as_str(), attachment.size),
        (known, "photo.png", 3)
    );
}

/// Une copie de plusieurs pages avance la révision et la version de l'espace d'un cran, pas d'un
/// par page, et annonce l'espace une fois.
#[tokio::test]
async fn copy_advances_rev_and_lsn_once() {
    let f = fresh().await;
    let mona = member(&f, "mona", Role::Member).await;
    let source = team_page(&f, None, None, "Notes").await;
    let (rev_before, lsn_before) = (rev(&f).await, lsn(&f).await);
    let mut changes = f.store.changes();

    let pages = vec![
        copied(None, "Notes (copie)", None),
        copied(Some(0), "A", Some("a0")),
        copied(Some(0), "B", Some("a1")),
    ];
    f.store
        .duplicate_pages(&private(&f, source, mona, pages), 5)
        .await
        .unwrap();
    assert_eq!(rev(&f).await, rev_before + 1);
    assert_eq!(lsn(&f).await, lsn_before + 1);
    assert_eq!(changes.try_recv().unwrap(), f.space.id);
    assert!(matches!(changes.try_recv(), Err(TryRecvError::Empty)));
}

/// Les titres des enfants de `parent`, dans l'ordre de leurs positions.
async fn children(f: &Fixture, parent: Option<Uuid>) -> Vec<String> {
    let of = match parent {
        Some(parent) => format!("parent_id = '{parent}'"),
        None => "parent_id IS NULL".to_string(),
    };
    texts(
        &f.db,
        &format!("SELECT title FROM nodes WHERE {of} ORDER BY position, id"),
    )
    .await
    .into_iter()
    .flatten()
    .collect()
}

/// La copie garde la forme du sous-arbre et l'ordre des frères : chaque page est sous la copie de
/// sa page parente, à la position de l'original. La racine de la copie se range juste après
/// l'original sous sa page parente, sans accès propre ; ou à la racine de l'espace, en dernier,
/// avec les accès donnés, et eux seuls.
#[tokio::test]
async fn copied_subtree_keeps_its_shape_and_its_order() {
    let f = fresh().await;
    let mona = member(&f, "mona", Role::Member).await;
    let parent = team_page(&f, None, None, "Projet").await;
    let source = team_page(&f, Some(parent), None, "Notes").await;
    let _next = team_page(&f, Some(parent), Some(source), "Suite").await;

    let shape = || {
        vec![
            copied(None, "Notes (copie)", None),
            copied(Some(0), "Deuxième", Some("a1")),
            copied(Some(0), "Première", Some("a0")),
            copied(Some(2), "Sous la première", Some("a0")),
            copied(Some(1), "Sous la deuxième", Some("Zz")),
        ]
    };

    // Juste après l'original, sous sa page parente.
    let copy = beside(&f, source, Some(parent), mona, shape());
    let root = f.store.duplicate_pages(&copy, 5).await.unwrap();
    assert_eq!(
        children(&f, Some(parent)).await,
        ["Notes", "Notes (copie)", "Suite"]
    );
    assert_eq!(
        children(&f, Some(root.id)).await,
        ["Première", "Deuxième"],
        "l'ordre est celui des positions, pas celui de la liste"
    );
    assert_eq!(
        children(&f, Some(copy.pages[2].id)).await,
        ["Sous la première"]
    );
    assert_eq!(
        children(&f, Some(copy.pages[1].id)).await,
        ["Sous la deuxième"]
    );
    assert_eq!(count(&f, "acl").await, 1, "aucun accès propre");
    for page in &copy.pages {
        let node = f.store.get_node(page.id).await.unwrap().unwrap();
        assert_eq!(node.kind, NodeKind::Page);
        assert_eq!((node.created_at, node.updated_at), (5, 5));
        assert_eq!((node.created_by, node.updated_by), (Some(mona), Some(mona)));
        assert_eq!(node.space_id, f.space.id);
    }

    // À la racine, en dernier, privée.
    let copy = private(&f, source, mona, shape());
    let root = f.store.duplicate_pages(&copy, 6).await.unwrap();
    assert_eq!(root.parent_id, None);
    assert_eq!(children(&f, None).await, ["Projet", "Notes (copie)"]);
    assert_eq!(children(&f, Some(root.id)).await, ["Première", "Deuxième"]);
    assert_eq!(
        f.store.node_grants(root.id).await.unwrap(),
        [grant(Subject::User(mona), Level::Full)]
    );
    for page in &copy.pages[1..] {
        assert!(f.store.node_grants(page.id).await.unwrap().is_empty());
    }
}

/// Ruling R590 : ce sur quoi la route a décidé est relu dans la transaction, au nom de qui copie.
/// `Stale`, sans rien écrire, si la page copiée n'est plus lue ou est à la corbeille (elle-même
/// ou par une page parente), si qui copie ne peut plus créer sous la page parente visée, ou
/// n'est plus membre pour une copie à la racine. Témoin de chaque cas : la même copie, avant le
/// changement, est écrite.
#[tokio::test]
async fn copy_is_checked_again_in_its_transaction() {
    let f = fresh().await;
    let mona = member(&f, "mona", Role::Member).await;
    let gina = member(&f, "gina", Role::Guest).await;
    let one = || vec![copied(None, "Copie", None)];
    let stale = async |copy: NewCopy, what: &str| {
        let before = written(&f).await;
        let refused = f.store.duplicate_pages(&copy, 5).await;
        assert!(
            matches!(refused, Err(StoreError::Stale)),
            "{what} : {refused:?}"
        );
        assert_eq!(written(&f).await, before, "{what} : rien d'écrit");
    };

    // La page copiée n'est plus lue : privée à un autre.
    let olga = member(&f, "olga", Role::Owner).await;
    let hidden = f
        .store
        .create_node(NewNode {
            grants: vec![
                grant(Subject::User(olga), Level::Full),
                grant(Subject::User(mona), Level::Read),
            ],
            ..page(&f.space, None, None, "Cachée")
        })
        .await
        .unwrap()
        .id;
    f.store
        .duplicate_pages(&private(&f, hidden, mona, one()), 5)
        .await
        .expect("témoin : lue, la page se copie");
    f.store
        .remove_grant(hidden, Subject::User(mona))
        .await
        .unwrap();
    stale(private(&f, hidden, mona, one()), "page plus lue").await;

    // Elle est à la corbeille, elle-même, puis par sa page parente.
    let parent = team_page(&f, None, None, "Projet").await;
    let source = team_page(&f, Some(parent), None, "Notes").await;
    f.store
        .duplicate_pages(&beside(&f, source, Some(parent), mona, one()), 5)
        .await
        .expect("témoin : hors corbeille, la page se copie");
    f.store.trash_node(source, olga).await.unwrap();
    stale(private(&f, source, mona, one()), "page à la corbeille").await;
    f.store.restore_node(source).await.unwrap();
    f.store.trash_node(parent, olga).await.unwrap();
    stale(
        private(&f, source, mona, one()),
        "page parente à la corbeille",
    )
    .await;
    f.store.restore_node(parent).await.unwrap();

    // La page parente visée est à la corbeille, sans que la page copiée y soit.
    let bin = team_page(&f, None, None, "Jetée").await;
    let under_bin = || NewCopy {
        parent_id: Some(bin),
        after: Some(Sibling::Last),
        ..beside(&f, source, Some(parent), mona, one())
    };
    f.store
        .duplicate_pages(&under_bin(), 5)
        .await
        .expect("témoin : sous une page hors corbeille, la copie passe");
    f.store.trash_node(bin, olga).await.unwrap();
    stale(under_bin(), "page parente visée à la corbeille").await;

    // Qui copie ne peut plus créer sous la page parente : les membres n'y font plus que lire.
    f.store
        .set_grant(parent, grant(Subject::Members, Level::Read))
        .await
        .unwrap();
    stale(
        beside(&f, source, Some(parent), mona, one()),
        "plus de création sous la page parente",
    )
    .await;
    f.store
        .duplicate_pages(&private(&f, source, mona, one()), 5)
        .await
        .expect("témoin : la même page se copie à la racine");

    // Une invitée ne reçoit pas de page à la racine ; sous une page qu'elle modifie, si.
    f.store
        .set_grant(parent, grant(Subject::User(gina), Level::Edit))
        .await
        .unwrap();
    stale(private(&f, source, gina, one()), "invitée, à la racine").await;
    f.store
        .duplicate_pages(&beside(&f, source, Some(parent), gina, one()), 5)
        .await
        .expect("témoin : sous une page qu'elle modifie, l'invitée copie");

    // Qui n'a plus de rôle ne copie rien.
    let nobody = account(&f.store, "personne").await.id;
    stale(private(&f, source, nobody, one()), "sans rôle").await;
}

/// `NotFound`, sans rien écrire : un espace, une page copiée ou une page parente inconnus, une
/// page parente d'un autre espace, un `after` qui n'est pas un frère à l'endroit visé (la page a
/// changé de parent depuis), un compte inconnu.
#[tokio::test]
async fn copy_to_a_place_that_is_gone_writes_nothing() {
    let f = fresh().await;
    let mona = member(&f, "mona", Role::Member).await;
    let parent = team_page(&f, None, None, "Projet").await;
    let other = team_page(&f, None, None, "Ailleurs").await;
    let source = team_page(&f, Some(parent), None, "Notes").await;
    let elsewhere = f.store.create_space("Autre", None).await.unwrap();
    let foreign = f
        .store
        .create_node(page(&elsewhere, None, None, "D'un autre espace"))
        .await
        .unwrap()
        .id;
    let one = || vec![copied(None, "Copie", None)];
    let base = || beside(&f, source, Some(parent), mona, one());
    let cases = [
        (
            "espace inconnu",
            NewCopy {
                space_id: Uuid::now_v7(),
                ..base()
            },
        ),
        (
            "page copiée inconnue",
            NewCopy {
                source: Uuid::now_v7(),
                ..private(&f, source, mona, one())
            },
        ),
        (
            "page copiée d'un autre espace",
            NewCopy {
                source: foreign,
                ..private(&f, source, mona, one())
            },
        ),
        (
            "page parente inconnue",
            NewCopy {
                parent_id: Some(Uuid::now_v7()),
                ..base()
            },
        ),
        (
            "page parente d'un autre espace",
            NewCopy {
                parent_id: Some(foreign),
                ..base()
            },
        ),
        (
            "la page n'est plus sous cette page parente",
            NewCopy {
                parent_id: Some(other),
                ..base()
            },
        ),
    ];
    for (what, copy) in cases {
        let before = written(&f).await;
        let refused = f.store.duplicate_pages(&copy, 5).await;
        assert!(
            matches!(refused, Err(StoreError::NotFound)),
            "{what} : {refused:?}"
        );
        assert_eq!(written(&f).await, before, "{what} : rien d'écrit");
    }
    f.store
        .duplicate_pages(&base(), 5)
        .await
        .expect("témoin : la copie de départ passe");
}

/// Une liste de pages mal formée (vide, une racine qui a une page parente, une page nommée avant
/// sa page parente) est une faute de l'appelant : une erreur, rien d'écrit.
#[tokio::test]
async fn malformed_copy_is_an_error() {
    let f = fresh().await;
    let mona = member(&f, "mona", Role::Member).await;
    let source = team_page(&f, None, None, "Notes").await;
    let cases = [
        ("aucune page", Vec::new()),
        (
            "racine sous une page",
            vec![copied(Some(0), "Racine", None)],
        ),
        (
            "page avant sa page parente",
            vec![
                copied(None, "Racine", None),
                copied(Some(2), "Fille", Some("a0")),
                copied(Some(0), "Mère", Some("a0")),
            ],
        ),
        (
            "seconde racine",
            vec![copied(None, "Racine", None), copied(None, "Autre", None)],
        ),
    ];
    for (what, pages) in cases {
        let before = written(&f).await;
        let refused = f
            .store
            .duplicate_pages(&private(&f, source, mona, pages), 5)
            .await;
        assert!(
            matches!(refused, Err(StoreError::Sqlx(_))),
            "{what} : {refused:?}"
        );
        assert_eq!(written(&f).await, before, "{what} : rien d'écrit");
    }
}
