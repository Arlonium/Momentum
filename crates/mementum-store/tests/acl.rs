//! Accès posés sur les pages : pose, remplacement, retrait, forme texte des sujets, et le
//! changement d'un accès par le partage d'une page (`change_access`).

mod common;

use common::{account, exec_sql, fresh, new_user, page, side_pool, texts};
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_store::{
    AccessChange, AccessWritten, InvitationGrant, NewInvitation, ShareRefusal, Space, Store,
    StoreError,
};
use uuid::Uuid;

fn grant(subject: Subject, level: Level) -> Grant {
    Grant { subject, level }
}

#[tokio::test]
async fn set_grant_replaces_level() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let user = Subject::User(Uuid::now_v7());
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    assert_eq!(store.node_grants(a.id).await.unwrap(), []);

    store
        .set_grant(a.id, grant(user, Level::Read))
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [grant(user, Level::Read)]
    );
    // Un second niveau pour le même sujet remplace le premier : une seule ligne.
    store
        .set_grant(a.id, grant(user, Level::Full))
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [grant(user, Level::Full)]
    );
    // Et l'on peut l'abaisser.
    store
        .set_grant(a.id, grant(user, Level::Comment))
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [grant(user, Level::Comment)]
    );
    // Un autre sujet a sa propre ligne.
    store
        .set_grant(a.id, grant(Subject::Members, Level::Edit))
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [
            grant(Subject::Members, Level::Edit),
            grant(user, Level::Comment)
        ]
    );

    assert!(matches!(
        store
            .set_grant(Uuid::now_v7(), grant(user, Level::Read))
            .await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn remove_grant_drops_only_that_line() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let user = Subject::User(Uuid::now_v7());
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(user, Level::Edit))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::Members, Level::Read))
        .await
        .unwrap();

    store.remove_grant(a.id, user).await.unwrap();
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [grant(Subject::Members, Level::Read)]
    );
    // Retirer une ligne qui n'existe pas n'écrit rien.
    let rev = store.space_rev(space.id).await.unwrap();
    store.remove_grant(a.id, user).await.unwrap();
    assert_eq!(store.space_rev(space.id).await.unwrap(), rev);
    assert!(matches!(
        store.remove_grant(Uuid::now_v7(), user).await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn space_grants_stay_in_their_space() {
    let f = fresh().await;
    let (store, here) = (&f.store, &f.space);
    let there = store.create_space("Ailleurs", None).await.unwrap();
    let user = Subject::User(Uuid::now_v7());
    let a = store
        .create_node(page(here, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(here, Some(a.id), None, "B"))
        .await
        .unwrap();
    let z = store
        .create_node(page(&there, None, None, "Z"))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::Members, Level::Full))
        .await
        .unwrap();
    store
        .set_grant(b.id, grant(user, Level::Read))
        .await
        .unwrap();
    store
        .set_grant(z.id, grant(user, Level::Full))
        .await
        .unwrap();

    let mut found = store.space_grants(here.id).await.unwrap();
    found.sort_by_key(|(node, _)| *node);
    assert_eq!(
        found,
        [
            (a.id, grant(Subject::Members, Level::Full)),
            (b.id, grant(user, Level::Read)),
        ]
    );
    assert_eq!(
        store.space_grants(there.id).await.unwrap(),
        [(z.id, grant(user, Level::Full))]
    );
    assert_eq!(store.space_grants(Uuid::now_v7()).await.unwrap(), []);
}

#[tokio::test]
async fn subject_text_roundtrip() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    // Un compte réel, ou non : `acl.subject` est un texte, pas une clé étrangère.
    let id = new_user(&f.db).await;
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::User(id), Level::Comment))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::Members, Level::Edit))
        .await
        .unwrap();

    // En base : la forme texte de `mementum-acl`, et elle seule.
    assert_eq!(
        texts(
            &f.db,
            "SELECT subject || ' ' || level FROM acl ORDER BY subject"
        )
        .await,
        [
            Some("members edit".to_string()),
            Some(format!("user:{} comment", id.hyphenated())),
        ]
    );
    // Relue telle quelle.
    assert_eq!(
        store.node_grants(a.id).await.unwrap(),
        [
            grant(Subject::Members, Level::Edit),
            grant(Subject::User(id), Level::Comment),
        ]
    );
    // Et c'est par ce texte qu'une ligne se retire.
    store.remove_grant(a.id, Subject::User(id)).await.unwrap();
    store.remove_grant(a.id, Subject::Members).await.unwrap();
    assert_eq!(texts(&f.db, "SELECT subject FROM acl").await, []);
}

/// Une ligne que `mementum-acl` ne sait pas relire est une erreur, jamais une ligne sautée :
/// un accès ignoré en silence fausserait le calcul des droits.
#[tokio::test]
async fn unreadable_grant_is_an_error() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let a = store
        .create_node(page(space, None, None, "A"))
        .await
        .unwrap();
    let b = store
        .create_node(page(space, None, None, "B"))
        .await
        .unwrap();
    store
        .set_grant(a.id, grant(Subject::Members, Level::Read))
        .await
        .unwrap();

    // Un sujet hors de la forme canonique (uuid en majuscules) : le schéma l'admet.
    let shouted = Uuid::now_v7().hyphenated().to_string().to_uppercase();
    exec_sql(
        &f.db,
        &format!(
            "INSERT INTO acl (node_id, subject, level) VALUES ('{}', 'user:{shouted}', 'edit')",
            a.id
        ),
    )
    .await;
    let r = store.node_grants(a.id).await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
    let r = store.space_grants(space.id).await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
    // La page d'à côté se lit toujours.
    assert_eq!(store.node_grants(b.id).await.unwrap(), []);
    exec_sql(&f.db, "DELETE FROM acl WHERE subject LIKE 'user:%'").await;
    assert_eq!(store.space_grants(space.id).await.unwrap().len(), 1);

    // Un niveau inconnu : le schéma le refuse (`CHECK`) ; il faut le forcer pour l'écrire.
    let unknown_level = "INSERT INTO acl (node_id, subject, level) VALUES (?, 'members', 'admin')";
    let pool = side_pool(&f.db).await;
    let mut conn = pool.acquire().await.unwrap();
    let r = sqlx::query(unknown_level)
        .bind(b.id.to_string())
        .execute(&mut *conn)
        .await;
    assert!(r.is_err(), "niveau inconnu admis par le schéma");
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(unknown_level)
        .bind(b.id.to_string())
        .execute(&mut *conn)
        .await
        .unwrap();
    drop(conn);
    pool.close().await;
    let r = store.node_grants(b.id).await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
    let r = store.space_grants(space.id).await;
    assert!(matches!(r, Err(StoreError::Sqlx(_))), "{r:?}");
}

/// Un espace où Olga est propriétaire, Mona membre et Gina invitée ; Sam a un compte, sans rôle.
struct Team {
    olga: Uuid,
    mona: Uuid,
    gina: Uuid,
    sam: Uuid,
}

async fn team(store: &Store, space: Uuid) -> Team {
    let team = Team {
        olga: account(store, "olga").await.id,
        mona: account(store, "mona").await.id,
        gina: account(store, "gina").await.id,
        sam: account(store, "sam").await.id,
    };
    for (who, role) in [
        (team.olga, Role::Owner),
        (team.mona, Role::Member),
        (team.gina, Role::Guest),
    ] {
        store.add_member(space, who, role).await.unwrap();
    }
    team
}

/// Une page de l'espace, sous `parent`, avec ces lignes.
async fn page_with(
    store: &Store,
    space: &Space,
    parent: Option<Uuid>,
    grants: &[(Subject, Level)],
) -> Uuid {
    let mut new = page(space, parent, None, "P");
    new.grants = grants.iter().map(|&(s, l)| grant(s, l)).collect();
    store.create_node(new).await.unwrap().id
}

/// Ce qu'un refus doit laisser intact : les lignes de la page, et la révision de l'espace.
async fn untouched(store: &Store, space: &Space, node: Uuid) -> (Vec<Grant>, Option<i64>) {
    (
        store.node_grants(node).await.unwrap(),
        store.space_rev(space.id).await.unwrap(),
    )
}

/// Ruling R70, dans la transaction de l'écriture : un changement qui laisserait la page sans
/// personne en `full` est refusé, sans rien écrire. Une ligne `members` compte, un `full`
/// hérité aussi ; une ligne restée à quelqu'un sans rôle, non.
#[tokio::test]
async fn change_access_keeps_someone_in_full() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let (olga, mona, sam) = (
        Subject::User(t.olga),
        Subject::User(t.mona),
        Subject::User(t.sam),
    );
    let refused = async |node: Uuid, change: AccessChange| {
        let before = untouched(store, space, node).await;
        let r = store.change_access(node, change, t.olga).await;
        assert!(matches!(r, Err(StoreError::LastFullAccess)), "{r:?}");
        assert_eq!(untouched(store, space, node).await, before);
    };

    // La seule ligne `full` ne part pas, et ne baisse pas.
    let shared = page_with(
        store,
        space,
        None,
        &[(olga, Level::Full), (mona, Level::Read)],
    )
    .await;
    refused(shared, AccessChange::User(t.olga, None)).await;
    refused(shared, AccessChange::User(t.olga, Some(Level::Edit))).await;
    // Sam n'a pas de rôle : sa ligne `full` ne compte pas.
    let dormant = page_with(
        store,
        space,
        None,
        &[(olga, Level::Full), (sam, Level::Full)],
    )
    .await;
    refused(dormant, AccessChange::User(t.olga, None)).await;
    // La ligne `members` compte, à tout niveau ; seule, elle ne part pas.
    let open = page_with(store, space, None, &[(Subject::Members, Level::Full)]).await;
    refused(open, AccessChange::Members(None)).await;
    store
        .change_access(open, AccessChange::Members(Some(Level::Read)), t.olga)
        .await
        .unwrap();
    // Sous elle, une page peut perdre sa seule ligne `full`.
    let under = page_with(store, space, Some(open), &[(mona, Level::Full)]).await;
    let written = store
        .change_access(under, AccessChange::User(t.mona, None), t.olga)
        .await
        .unwrap();
    assert!(written.changed && !written.left_space);
    // Un `full` hérité compte aussi.
    let parent = page_with(store, space, None, &[(olga, Level::Full)]).await;
    let child = page_with(store, space, Some(parent), &[(mona, Level::Full)]).await;
    store
        .change_access(child, AccessChange::User(t.mona, Some(Level::Read)), t.olga)
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(child).await.unwrap(),
        [grant(mona, Level::Read)]
    );
    refused(parent, AccessChange::User(t.olga, None)).await;
    // Un autre `full` sur la page : Olga peut partir.
    store
        .change_access(
            shared,
            AccessChange::User(t.mona, Some(Level::Full)),
            t.olga,
        )
        .await
        .unwrap();
    store
        .change_access(shared, AccessChange::User(t.olga, None), t.olga)
        .await
        .unwrap();
    assert_eq!(
        store.node_grants(shared).await.unwrap(),
        [grant(mona, Level::Full)]
    );
}

/// Ruling R87 : ce sur quoi un changement d'accès repose est vérifié dans sa transaction, sans
/// dépendre de la révision de l'espace, et chaque refus a son cas, dans cet ordre : la page
/// n'existe pas pour qui demande (aucun rôle, sous `read`, page inconnue) ; il la voit sous
/// `full` ; un invité touche à l'accès général (R85) ; la page ou un ancêtre est à la
/// corbeille ; la personne visée n'a pas de rôle, ou pas de ligne propre à retirer. Aucun
/// refus n'écrit rien.
#[tokio::test]
async fn change_access_checks_who_asks_in_its_transaction() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let (olga, mona, gina) = (
        Subject::User(t.olga),
        Subject::User(t.mona),
        Subject::User(t.gina),
    );
    let refused = async |node: Uuid, change: AccessChange, actor: Uuid, why: ShareRefusal| {
        let before = untouched(store, space, node).await;
        let r = store.change_access(node, change, actor).await;
        assert!(
            matches!(r, Err(StoreError::Refused(got)) if got == why),
            "{change:?} par {actor} : {r:?}, attendu {why:?}"
        );
        assert_eq!(untouched(store, space, node).await, before);
    };
    let read_mona = AccessChange::User(t.mona, Some(Level::Read));
    let general = AccessChange::Members(Some(Level::Read));

    // Olga en `full`, Mona en `edit`, Gina (invitée) en `full`.
    let shared = page_with(
        store,
        space,
        None,
        &[
            (olga, Level::Full),
            (mona, Level::Edit),
            (gina, Level::Full),
        ],
    )
    .await;
    let child = page_with(store, space, Some(shared), &[]).await;
    let private = page_with(store, space, None, &[(olga, Level::Full)]).await;
    let team_page = page_with(
        store,
        space,
        None,
        &[(Subject::Members, Level::Full), (gina, Level::Full)],
    )
    .await;

    // La page n'existe pas pour qui demande.
    refused(Uuid::now_v7(), read_mona, t.olga, ShareRefusal::Unseen).await;
    refused(shared, read_mona, t.sam, ShareRefusal::Unseen).await;
    refused(shared, read_mona, Uuid::now_v7(), ShareRefusal::Unseen).await;
    refused(private, read_mona, t.mona, ShareRefusal::Unseen).await;
    // Sous `full`.
    refused(shared, read_mona, t.mona, ShareRefusal::BelowFull).await;
    refused(shared, general, t.mona, ShareRefusal::BelowFull).await;
    // R85 : un invité en `full` ne pose ni ne retire l'accès général.
    refused(shared, general, t.gina, ShareRefusal::GuestGeneral).await;
    refused(
        team_page,
        AccessChange::Members(None),
        t.gina,
        ShareRefusal::GuestGeneral,
    )
    .await;
    // La personne visée : un rôle pour recevoir un accès, une ligne propre pour le perdre.
    for nobody in [t.sam, Uuid::now_v7()] {
        let to_nobody = AccessChange::User(nobody, Some(Level::Read));
        refused(shared, to_nobody, t.olga, ShareRefusal::NotInSpace).await;
    }
    let remove_sam = AccessChange::User(t.sam, None);
    refused(shared, remove_sam, t.olga, ShareRefusal::NoOwnLine).await;
    // La ligne de Mona est sur la page parente.
    let remove_mona = AccessChange::User(t.mona, None);
    refused(child, remove_mona, t.olga, ShareRefusal::NoOwnLine).await;

    // Le reste découle de `full` (R86) : Gina change la ligne d'un autre.
    let written = store
        .change_access(shared, read_mona, t.gina)
        .await
        .unwrap();
    assert!(written.changed);
    assert_eq!(written.space_id, space.id);

    // À la corbeille, la page et sa sous-page : après les refus de niveau et de R85.
    store.trash_node(shared, t.olga).await.unwrap();
    for node in [shared, child] {
        refused(node, read_mona, t.olga, ShareRefusal::Trashed).await;
        refused(node, general, t.gina, ShareRefusal::GuestGeneral).await;
        let edit_mona = AccessChange::User(t.mona, Some(Level::Edit));
        refused(node, edit_mona, t.mona, ShareRefusal::BelowFull).await;
    }
}

/// L'accès général d'une page ne se baisse ni ne se retire en dessous de celui que donne une
/// ligne `members` d'un ancêtre (`InheritedAccess`, rien d'écrit) ; un niveau égal ou plus haut
/// s'écrit.
#[tokio::test]
async fn change_access_refuses_to_lower_an_inherited_general_access() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let parent = page_with(store, space, None, &[(Subject::Members, Level::Edit)]).await;
    let child = page_with(store, space, Some(parent), &[]).await;
    let refused = async |level: Option<Level>| {
        let before = untouched(store, space, child).await;
        let r = store
            .change_access(child, AccessChange::Members(level), t.olga)
            .await;
        assert!(
            matches!(r, Err(StoreError::Refused(ShareRefusal::InheritedAccess))),
            "{level:?} : {r:?}"
        );
        assert_eq!(untouched(store, space, child).await, before);
    };
    for level in [None, Some(Level::Read), Some(Level::Comment)] {
        refused(level).await;
    }
    for level in [Level::Edit, Level::Full] {
        let written = store
            .change_access(child, AccessChange::Members(Some(level)), t.olga)
            .await
            .unwrap();
        assert!(written.changed, "{level:?}");
        assert_eq!(
            store.node_grants(child).await.unwrap(),
            [grant(Subject::Members, level)]
        );
    }
    // Plus haut que l'héritage, la ligne de la page ne se retire pas non plus : on revient au
    // niveau hérité.
    refused(None).await;
    store
        .change_access(child, AccessChange::Members(Some(Level::Edit)), t.olga)
        .await
        .unwrap();
    // Sur la page parente, rien n'est hérité : elle baisse.
    store
        .change_access(parent, AccessChange::Members(Some(Level::Read)), t.olga)
        .await
        .unwrap();
}

/// Une ligne déjà ainsi n'écrit rien : la révision de l'espace ne bouge pas. Un changement fait
/// l'incrémente d'un cran, et dit dans quel espace il a écrit.
#[tokio::test]
async fn change_access_writes_nothing_when_nothing_changes() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let node = page_with(store, space, None, &[(Subject::Members, Level::Full)]).await;
    let rev = store.space_rev(space.id).await.unwrap().unwrap();
    let written = store
        .change_access(node, AccessChange::User(t.mona, Some(Level::Edit)), t.olga)
        .await
        .unwrap();
    assert_eq!(
        written,
        AccessWritten {
            space_id: space.id,
            changed: true,
            left_space: false
        }
    );
    assert_eq!(store.space_rev(space.id).await.unwrap(), Some(rev + 1));
    for change in [
        AccessChange::User(t.mona, Some(Level::Edit)),
        AccessChange::Members(Some(Level::Full)),
    ] {
        let written = store.change_access(node, change, t.olga).await.unwrap();
        assert!(!written.changed && !written.left_space, "{change:?}");
    }
    assert_eq!(store.space_rev(space.id).await.unwrap(), Some(rev + 1));
}

/// Retirer à un invité sa dernière ligne de l'espace l'en fait sortir, avec les invitations
/// qu'il y avait faites et qui attendaient encore, comme au départ d'un membre ; tant qu'il
/// garde une ligne, il reste, et elles aussi. Un membre reste membre.
#[tokio::test]
async fn change_access_lets_a_bare_guest_leave() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let (gina, mona) = (Subject::User(t.gina), Subject::User(t.mona));
    let first = page_with(
        store,
        space,
        None,
        &[
            (Subject::Members, Level::Full),
            (gina, Level::Read),
            (mona, Level::Edit),
        ],
    )
    .await;
    let second = page_with(
        store,
        space,
        None,
        &[(Subject::Members, Level::Full), (gina, Level::Full)],
    )
    .await;
    // Gina, en `full` sur la seconde page, y a invité quelqu'un.
    let hers = store
        .create_invitation(NewInvitation {
            space_id: space.id,
            grant: InvitationGrant::Page {
                node_id: second,
                level: Level::Read,
            },
            email: "invite@exemple.test".to_string(),
            token_hash: [7; 32],
            created_by: t.gina,
            expires_at: i64::MAX,
        })
        .await
        .unwrap();
    let pending = async || {
        store
            .pending_invitations(space.id)
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect::<Vec<_>>()
    };

    let written = store
        .change_access(first, AccessChange::User(t.gina, None), t.olga)
        .await
        .unwrap();
    assert!(written.changed && !written.left_space);
    assert_eq!(
        store.role_in(space.id, t.gina).await.unwrap(),
        Some(Role::Guest)
    );
    assert_eq!(pending().await, [hers.id]);
    let written = store
        .change_access(second, AccessChange::User(t.gina, None), t.olga)
        .await
        .unwrap();
    assert!(written.changed && written.left_space);
    assert_eq!(store.role_in(space.id, t.gina).await.unwrap(), None);
    assert!(pending().await.is_empty());
    // Un membre sans plus aucune ligne reste membre.
    let written = store
        .change_access(first, AccessChange::User(t.mona, None), t.olga)
        .await
        .unwrap();
    assert!(written.changed && !written.left_space);
    assert_eq!(
        store.role_in(space.id, t.mona).await.unwrap(),
        Some(Role::Member)
    );
}
