//! La version de l'espace, `spaces.lsn` (Ruling R133) : toute écriture qui change ce que montre
//! la barre latérale l'avance, et annonce l'espace sur `Store::changes` une fois validée ; le
//! contenu d'une page ne la touche pas ; une écriture refusée n'avance rien et n'annonce rien.

mod common;

use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use common::{account, break_table, page, repair};
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_store::{
    AccessChange, Expected, InvitationGrant, NewInvitation, NewNode, NewSession, NewUser, Node,
    Place, ShareRefusal, Store, StoreError,
};
use tokio::sync::broadcast::error::TryRecvError;
use uuid::Uuid;

const DAY: i64 = 86_400_000;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

async fn lsn(store: &Store, space: Uuid) -> i64 {
    store.space_lsn(space).await.unwrap().expect("espace")
}

async fn rev(store: &Store, space: Uuid) -> i64 {
    store.space_rev(space).await.unwrap().expect("espace")
}

fn grant(subject: Subject, level: Level) -> Grant {
    Grant { subject, level }
}

fn seen(actor: Uuid, actor_role: Role, target_role: Role) -> Expected {
    Expected {
        actor,
        actor_role,
        target_role,
    }
}

/// Une page de `space` au nom de `owner`, qui y a l'accès complet.
fn owned_page(space: Uuid, parent: Option<Uuid>, title: &str, owner: Uuid) -> NewNode {
    NewNode {
        space_id: space,
        parent_id: parent,
        after: None,
        kind: mementum_store::NodeKind::Page,
        title: title.to_string(),
        created_by: Some(owner),
        grants: vec![grant(Subject::User(owner), Level::Full)],
        doc: None,
    }
}

/// Une invitation de `by`, qui expire dans une semaine.
fn invitation(space: Uuid, grant: InvitationGrant, by: Uuid, token: u8) -> NewInvitation {
    NewInvitation {
        space_id: space,
        grant,
        email: "invite@exemple.test".to_string(),
        token_hash: [token; 32],
        created_by: by,
        expires_at: now() + 7 * DAY,
    }
}

/// L'écriture `write` avance la version de `space` d'au moins 1, et l'annonce sur `changes()`,
/// une fois, après sa validation : un lecteur qui relit la version dès qu'il reçoit l'annonce
/// voit déjà la nouvelle. La révision (`rev`) avance si `rev_moves`, et sinon ne bouge pas : la
/// règle de `rev` ne change pas (R83). Rend ce que rend l'écriture.
async fn advances<T>(
    store: &Store,
    space: Uuid,
    rev_moves: bool,
    what: &str,
    write: impl Future<Output = T>,
) -> T {
    let (lsn_before, rev_before) = (lsn(store, space).await, rev(store, space).await);
    // Le lecteur relit la version dès l'annonce, dans une autre tâche : si l'annonce partait
    // avant la validation, il lirait l'ancienne (la transaction d'écriture n'est pas encore
    // visible des autres connexions).
    let mut heard = store.changes();
    let reader = {
        let store = store.clone();
        tokio::spawn(async move {
            let announced = heard.recv().await.unwrap();
            (announced, store.space_lsn(announced).await.unwrap())
        })
    };
    let mut changes = store.changes();
    let out = write.await;
    // L'annonce est partie avant que l'écriture rende la main : elle est déjà là.
    assert_eq!(changes.try_recv(), Ok(space), "{what} : annonce");
    assert_eq!(
        changes.try_recv(),
        Err(TryRecvError::Empty),
        "{what} : une seule annonce par écriture"
    );
    let (announced, read) = reader.await.unwrap();
    assert_eq!(announced, space, "{what}");
    let read = read.expect("espace");
    assert!(
        read > lsn_before,
        "{what} : relue à l'annonce, la version vaut {read}, comme avant l'écriture"
    );
    assert!(lsn(store, space).await > lsn_before, "{what} : version");
    let rev_after = rev(store, space).await;
    if rev_moves {
        assert!(rev_after > rev_before, "{what} : révision");
    } else {
        assert_eq!(rev_after, rev_before, "{what} : révision (R83)");
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn every_structural_write_advances_lsn() {
    let f = common::fresh().await;
    let store = &f.store;
    assert_eq!(store.space_lsn(Uuid::now_v7()).await.unwrap(), None);
    let olga = account(store, "olga").await.id;
    let team = store.create_space("Équipe", Some(olga)).await.unwrap().id;
    let bob = account(store, "bob").await.id;

    // Pages : création, renommage, place parmi les mêmes frères, changement de parent,
    // corbeille, restauration, suppression.
    let a: Node = advances(store, team, true, "création", async {
        store
            .create_node(owned_page(team, None, "A", olga))
            .await
            .unwrap()
    })
    .await;
    let mut new_b = owned_page(team, None, "B", olga);
    new_b.after = Some(mementum_store::Sibling::Node(a.id));
    let b = store.create_node(new_b).await.unwrap();
    let c = store
        .create_node(owned_page(team, Some(a.id), "C", olga))
        .await
        .unwrap();
    advances(store, team, false, "renommage", async {
        store
            .rename_node(a.id, "A renommée", Some(olga))
            .await
            .unwrap()
    })
    .await;
    let moved = advances(store, team, false, "place parmi les mêmes frères", async {
        store.move_node(b.id, None, None, Some(olga)).await.unwrap()
    })
    .await;
    assert!(moved.position < a.position);
    advances(
        store,
        team,
        false,
        "titre et place, d'un même appel",
        async {
            let place = Place {
                parent_id: None,
                after: Some(mementum_store::Sibling::Node(a.id)),
                from: Some(None),
            };
            store
                .update_node(b.id, Some("B renommée"), Some(place), Some(olga), None)
                .await
                .unwrap()
        },
    )
    .await;
    advances(store, team, true, "changement de parent", async {
        store
            .move_node(b.id, Some(a.id), None, Some(olga))
            .await
            .unwrap()
    })
    .await;
    advances(store, team, true, "corbeille", async {
        store.trash_node(b.id, olga).await.unwrap()
    })
    .await;
    advances(store, team, true, "restauration", async {
        store.restore_node(b.id).await.unwrap()
    })
    .await;
    store.trash_node(c.id, olga).await.unwrap();
    advances(store, team, true, "suppression", async {
        store.delete_node(c.id).await.unwrap()
    })
    .await;

    // Membres : ajouté, rôle changé.
    advances(store, team, true, "membre ajouté", async {
        store.add_member(team, bob, Role::Member).await.unwrap()
    })
    .await;
    advances(store, team, true, "rôle changé", async {
        store
            .set_role(
                team,
                bob,
                Role::Admin,
                seen(olga, Role::Owner, Role::Member),
            )
            .await
            .unwrap()
    })
    .await;

    // Accès d'une page, par le partage : posé, changé, retiré.
    for (what, level) in [
        ("accès posé", Some(Level::Read)),
        ("accès changé", Some(Level::Edit)),
        ("accès retiré", None),
    ] {
        let written = advances(store, team, true, what, async {
            store
                .change_access(a.id, AccessChange::User(bob, level), olga)
                .await
                .unwrap()
        })
        .await;
        assert!(written.changed, "{what}");
    }
    advances(store, team, true, "accès général", async {
        store
            .change_access(a.id, AccessChange::Members(Some(Level::Comment)), olga)
            .await
            .unwrap()
    })
    .await;

    // Invitations acceptées : dans l'espace, sur une page, et à l'inscription.
    let carl = account(store, "carl").await.id;
    store
        .create_invitation(invitation(
            team,
            InvitationGrant::Space(Role::Member),
            olga,
            1,
        ))
        .await
        .unwrap();
    advances(store, team, true, "invitation d'espace acceptée", async {
        store.accept_invitation([1; 32], carl, now()).await.unwrap()
    })
    .await;
    let dina = account(store, "dina").await.id;
    let to_page = InvitationGrant::Page {
        node_id: a.id,
        level: Level::Read,
    };
    store
        .create_invitation(invitation(team, to_page, olga, 2))
        .await
        .unwrap();
    advances(store, team, true, "invitation de page acceptée", async {
        store.accept_invitation([2; 32], dina, now()).await.unwrap()
    })
    .await;
    store
        .create_invitation(invitation(
            team,
            InvitationGrant::Space(Role::Guest),
            olga,
            3,
        ))
        .await
        .unwrap();
    let signed = advances(store, team, true, "inscription par une invitation", async {
        let eve = NewUser {
            email: "eve@exemple.test".to_string(),
            name: "Eve".to_string(),
            password_hash: "pas-un-hachage".to_string(),
        };
        store
            .sign_up(
                eve,
                Some([3; 32]),
                "Espace d'Eve",
                NewSession {
                    token_hash: [3; 32],
                    expires_at: now() + 86_400_000,
                },
                now(),
            )
            .await
            .unwrap()
    })
    .await;
    assert!(signed.accepted.is_some());

    // Le nom de l'espace, que montre la barre latérale (Ruling R150) : la version avance, pas la
    // révision.
    advances(store, team, false, "renommage de l'espace", async {
        store.rename_space(team, "Équipe renommée").await.unwrap()
    })
    .await;

    // Membre retiré.
    advances(store, team, true, "membre retiré", async {
        store
            .remove_member(team, bob, seen(olga, Role::Owner, Role::Admin))
            .await
            .unwrap()
    })
    .await;

    // Les écritures réservées aux tests passent par les mêmes chemins.
    advances(store, team, true, "accès posé sans contrôle", async {
        store
            .set_grant(a.id, grant(Subject::User(carl), Level::Edit))
            .await
            .unwrap()
    })
    .await;
    advances(store, team, true, "accès retiré sans contrôle", async {
        store.remove_grant(a.id, Subject::User(carl)).await.unwrap()
    })
    .await;
    advances(store, team, true, "accès d'un invité retiré", async {
        store.revoke_user_grant(a.id, dina).await.unwrap()
    })
    .await;
    let orphan = f.space.id;
    advances(store, orphan, true, "espace sans membre adopté", async {
        store.adopt_orphan_spaces(olga).await.unwrap()
    })
    .await;
}

/// Une écriture du contenu d'une page (le journal, un instantané, la compaction qu'il porte)
/// n'avance pas la version, et n'annonce rien.
#[tokio::test]
async fn content_update_does_not_advance_lsn() {
    let f = common::fresh().await;
    let (store, space) = (&f.store, f.space.id);
    let node = store
        .create_node(page(&f.space, None, None, "Page"))
        .await
        .unwrap();
    let (lsn_before, rev_before) = (lsn(store, space).await, rev(store, space).await);
    let mut changes = store.changes();

    assert_eq!(
        store.append_updates(node.id, &[b"un"], None).await.unwrap(),
        1
    );
    assert_eq!(
        store
            .append_updates(node.id, &[b"deux", b"trois"], None)
            .await
            .unwrap(),
        3
    );
    assert_eq!(store.append_updates(node.id, &[], None).await.unwrap(), 3);
    store.put_snapshot(node.id, 2, b"etat").await.unwrap();
    assert_eq!(store.load_doc(node.id).await.unwrap().updates.len(), 1);

    assert_eq!(lsn(store, space).await, lsn_before);
    assert_eq!(rev(store, space).await, rev_before);
    assert_eq!(changes.try_recv(), Err(TryRecvError::Empty));
}

/// Une écriture refusée, par le store (`Stale`, un refus de partage, une autre règle) ou par la
/// base, n'avance ni la version ni la révision, et n'annonce rien : elle n'a rien validé.
#[tokio::test]
async fn refused_write_announces_nothing() {
    let f = common::fresh().await;
    let store = &f.store;
    let olga = account(store, "olga").await.id;
    let team = store.create_space("Équipe", Some(olga)).await.unwrap().id;
    let bob = account(store, "bob").await.id;
    store.add_member(team, bob, Role::Member).await.unwrap();
    let a = store
        .create_node(owned_page(team, None, "A", olga))
        .await
        .unwrap();
    let c = store
        .create_node(owned_page(team, Some(a.id), "C", olga))
        .await
        .unwrap();
    let hidden = store
        .create_node(owned_page(team, None, "Privée", olga))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::User(bob), Level::Read))
        .await
        .unwrap();

    // Le refus attendu, sans rien de validé.
    async fn refused<T: std::fmt::Debug>(
        store: &Store,
        space: Uuid,
        what: &str,
        expected: impl Fn(&StoreError) -> bool,
        write: impl Future<Output = Result<T, StoreError>>,
    ) {
        let (lsn_before, rev_before) = (lsn(store, space).await, rev(store, space).await);
        let mut changes = store.changes();
        match write.await {
            Err(e) if expected(&e) => {}
            other => panic!("{what} : {other:?}"),
        }
        assert_eq!(lsn(store, space).await, lsn_before, "{what} : version");
        assert_eq!(rev(store, space).await, rev_before, "{what} : révision");
        assert_eq!(changes.try_recv(), Err(TryRecvError::Empty), "{what}");
    }

    // `Stale` : les rôles ont bougé, la place a bougé, le passage à la racine ne tient plus.
    refused(
        store,
        team,
        "rôle, sur des rôles qui ont bougé",
        |e| matches!(e, StoreError::Stale),
        store.set_role(team, bob, Role::Admin, seen(olga, Role::Owner, Role::Guest)),
    )
    .await;
    refused(
        store,
        team,
        "départ, sur des rôles qui ont bougé",
        |e| matches!(e, StoreError::Stale),
        store.remove_member(team, bob, seen(olga, Role::Admin, Role::Member)),
    )
    .await;
    let from_elsewhere = Place {
        parent_id: None,
        after: None,
        from: Some(Some(hidden.id)),
    };
    refused(
        store,
        team,
        "place, depuis un parent qui n'est plus le sien",
        |e| matches!(e, StoreError::Stale),
        store.update_node(c.id, Some("C"), Some(from_elsewhere), Some(olga), None),
    )
    .await;
    let to_root = Place {
        parent_id: None,
        after: None,
        from: Some(Some(a.id)),
    };
    refused(
        store,
        team,
        "passage à la racine, par qui n'a pas l'accès complet",
        |e| matches!(e, StoreError::Stale),
        store.update_node(
            c.id,
            None,
            Some(to_root),
            Some(bob),
            Some(grant(Subject::User(bob), Level::Full)),
        ),
    )
    .await;

    // Refus du partage : la page ne se voit pas, l'accès complet manque, ou la page n'aurait
    // plus personne en accès complet (le retrait est écrit, puis annulé).
    refused(
        store,
        team,
        "partage d'une page qu'on ne voit pas",
        |e| matches!(e, StoreError::Refused(ShareRefusal::Unseen)),
        store.change_access(hidden.id, AccessChange::Members(Some(Level::Read)), bob),
    )
    .await;
    refused(
        store,
        team,
        "partage sous l'accès complet",
        |e| matches!(e, StoreError::Refused(ShareRefusal::BelowFull)),
        store.change_access(a.id, AccessChange::User(bob, Some(Level::Full)), bob),
    )
    .await;
    refused(
        store,
        team,
        "dernier accès complet retiré",
        |e| matches!(e, StoreError::LastFullAccess),
        store.change_access(a.id, AccessChange::User(olga, None), olga),
    )
    .await;

    // Les autres règles du store.
    refused(
        store,
        team,
        "cycle",
        |e| matches!(e, StoreError::Cycle),
        store.move_node(a.id, Some(c.id), None, Some(olga)),
    )
    .await;
    refused(
        store,
        team,
        "suppression hors corbeille",
        |e| matches!(e, StoreError::NotTrashed),
        store.delete_node(a.id),
    )
    .await;
    refused(
        store,
        team,
        "dernier propriétaire",
        |e| matches!(e, StoreError::LastOwner),
        store.set_role(
            team,
            olga,
            Role::Member,
            seen(olga, Role::Owner, Role::Owner),
        ),
    )
    .await;
    refused(
        store,
        team,
        "invitation inconnue",
        |e| matches!(e, StoreError::NotFound),
        store.accept_invitation([9; 32], bob, now()),
    )
    .await;

    // La base refuse : avant l'écriture de la version (le titre d'un nœud), puis l'écriture de
    // la version elle-même, après que la page est mise à la corbeille dans la même transaction.
    break_table(&f.db, "UPDATE", "nodes", "1").await;
    refused(
        store,
        team,
        "renommage refusé par la base",
        |e| matches!(e, StoreError::Sqlx(_)),
        store.rename_node(a.id, "Refusé", Some(olga)),
    )
    .await;
    repair(&f.db).await;
    break_table(&f.db, "UPDATE", "spaces", "NEW.lsn != OLD.lsn").await;
    refused(
        store,
        team,
        "version refusée par la base",
        |e| matches!(e, StoreError::Sqlx(_)),
        store.trash_node(a.id, olga),
    )
    .await;
    refused(
        store,
        team,
        "nom d'espace refusé par la base",
        |e| matches!(e, StoreError::Sqlx(_)),
        store.rename_space(team, "Refusé"),
    )
    .await;
    repair(&f.db).await;
    assert_eq!(
        store.get_node(a.id).await.unwrap().unwrap().trashed_at,
        None
    );
    assert_eq!(store.get_node(a.id).await.unwrap().unwrap().title, "A");
    assert_eq!(store.get_space(team).await.unwrap().unwrap().name, "Équipe");
}
