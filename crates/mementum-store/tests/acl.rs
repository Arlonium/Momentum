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

/// Ruling R93 : l'accès général d'une page a pour plancher celui que donne la ligne `members`
/// d'un ancêtre, et ne laisse jamais de ligne redondante. Avec un plancher : plus bas,
/// `InheritedAccess` ; égal, la ligne propre part (s'il y en a une) ; plus haut, elle est
/// posée ; `null` retire la ligne propre, et sans ligne propre, `InheritedAccess`. Sans
/// plancher : `null` retire la ligne propre (sans elle, rien), un niveau la pose. Chaque cas,
/// avec ce qui reste en base.
#[tokio::test]
async fn change_access_keeps_general_access_above_the_inherited_floor() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let parent = page_with(store, space, None, &[(Subject::Members, Level::Edit)]).await;
    let child = page_with(store, space, Some(parent), &[]).await;
    let general = |level: Option<Level>| AccessChange::Members(level);
    // La ligne `members` posée sur la page, s'il y en a une.
    let own = async |node: Uuid| {
        store
            .node_grants(node)
            .await
            .unwrap()
            .into_iter()
            .find(|g| g.subject == Subject::Members)
            .map(|g| g.level)
    };
    let refused = async |level: Option<Level>| {
        let before = untouched(store, space, child).await;
        let r = store.change_access(child, general(level), t.olga).await;
        assert!(
            matches!(r, Err(StoreError::Refused(ShareRefusal::InheritedAccess))),
            "{level:?} : {r:?}"
        );
        assert_eq!(untouched(store, space, child).await, before, "{level:?}");
    };
    // Rend vrai si une ligne a été écrite ou retirée.
    let set = async |node: Uuid, level: Option<Level>| {
        store
            .change_access(node, general(level), t.olga)
            .await
            .unwrap()
            .changed
    };

    // Plancher `edit`, sans ligne propre : `null` et plus bas refusés ; égal, rien à écrire.
    for level in [None, Some(Level::Read), Some(Level::Comment)] {
        refused(level).await;
    }
    assert!(!set(child, Some(Level::Edit)).await);
    assert_eq!(own(child).await, None);
    // Plus haut : la ligne est posée.
    assert!(set(child, Some(Level::Full)).await);
    assert_eq!(own(child).await, Some(Level::Full));
    // `null` avec une ligne propre : elle part, on revient au plancher, sans ligne.
    assert!(set(child, None).await);
    assert_eq!(own(child).await, None);
    // Égal au plancher avec une ligne propre plus haute : elle part aussi, sans ligne.
    assert!(set(child, Some(Level::Full)).await);
    assert!(set(child, Some(Level::Edit)).await);
    assert_eq!(own(child).await, None);

    // Une ligne redondante d'avant (posée sous le plancher, ou à son niveau) : plus bas reste
    // refusé ; égal ou `null` la retirent.
    store
        .set_grant(child, grant(Subject::Members, Level::Read))
        .await
        .unwrap();
    refused(Some(Level::Read)).await;
    assert_eq!(own(child).await, Some(Level::Read));
    assert!(set(child, Some(Level::Edit)).await);
    assert_eq!(own(child).await, None);
    store
        .set_grant(child, grant(Subject::Members, Level::Edit))
        .await
        .unwrap();
    assert!(set(child, None).await);
    assert_eq!(own(child).await, None);

    // Sans plancher (une page d'Olga, à la racine) : `null` sans ligne, rien ; un niveau, posé ;
    // `null`, retiré.
    let alone = page_with(store, space, None, &[(Subject::User(t.olga), Level::Full)]).await;
    assert!(!set(alone, None).await);
    assert!(set(alone, Some(Level::Read)).await);
    assert_eq!(own(alone).await, Some(Level::Read));
    assert!(set(alone, None).await);
    assert_eq!(own(alone).await, None);
    // La page parente n'a pas de plancher : elle baisse.
    assert!(set(parent, Some(Level::Read)).await);
    assert_eq!(own(parent).await, Some(Level::Read));
}

/// La chaîne d'une page va jusqu'à la racine, pas jusqu'à sa page parente : le plancher de
/// l'accès général (R93) est la plus forte ligne `members` de **tous** ses ancêtres, où qu'elle
/// soit ; le niveau de qui demande et la corbeille se lisent de même sur toute la chaîne.
#[tokio::test]
async fn change_access_reads_every_ancestor_of_the_page() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let general = |level: Level| AccessChange::Members(Some(level));
    let own_general = async |node: Uuid| {
        let lines = store.node_grants(node).await.unwrap();
        let line = lines.into_iter().find(|g| g.subject == Subject::Members);
        line.map(|g| g.level)
    };

    // Le plancher : `edit`, posé tour à tour sur la page parente et sur la grand-parente,
    // l'autre ne donnant que `read`.
    for (top, middle) in [(Level::Read, Level::Edit), (Level::Edit, Level::Read)] {
        let what = format!("grand-parente {top:?}, parente {middle:?}");
        let grandparent = page_with(store, space, None, &[(Subject::Members, top)]).await;
        let lines = [(Subject::Members, middle)];
        let parent = page_with(store, space, Some(grandparent), &lines).await;
        let child = page_with(store, space, Some(parent), &[]).await;
        // Sous le plancher, même au-dessus de la plus faible des deux lignes : refusé.
        for below in [Level::Read, Level::Comment] {
            let before = untouched(store, space, child).await;
            let r = store.change_access(child, general(below), t.olga).await;
            assert!(
                matches!(r, Err(StoreError::Refused(ShareRefusal::InheritedAccess))),
                "{what}, {below:?} : {r:?}"
            );
            assert_eq!(untouched(store, space, child).await, before, "{what}");
        }
        // Au plancher : rien à écrire. Au-dessus : la ligne est posée.
        let written = store.change_access(child, general(Level::Edit), t.olga);
        assert!(!written.await.unwrap().changed, "{what}");
        assert_eq!(own_general(child).await, None, "{what}");
        let written = store.change_access(child, general(Level::Full), t.olga);
        assert!(written.await.unwrap().changed, "{what}");
        assert_eq!(own_general(child).await, Some(Level::Full), "{what}");
    }

    // Le niveau de qui demande : Mona tient l'accès complet d'une ligne à son nom, tour à tour
    // sur la page parente et sur la grand-parente, l'autre ne lui donnant que la lecture.
    let (olga, mona) = (Subject::User(t.olga), Subject::User(t.mona));
    let to_gina = AccessChange::User(t.gina, Some(Level::Read));
    for (top, middle) in [(Level::Read, Level::Full), (Level::Full, Level::Read)] {
        let what = format!("grand-parente {top:?}, parente {middle:?}");
        let lines = [(olga, Level::Full), (mona, top)];
        let grandparent = page_with(store, space, None, &lines).await;
        let parent = page_with(store, space, Some(grandparent), &[(mona, middle)]).await;
        let child = page_with(store, space, Some(parent), &[]).await;
        let written = store.change_access(child, to_gina, t.mona).await;
        assert!(written.is_ok_and(|w| w.changed), "{what}");
        // La corbeille aussi : celle de la grand-parente vaut pour la petite-fille.
        store.trash_node(grandparent, t.olga).await.unwrap();
        let to_gina = AccessChange::User(t.gina, Some(Level::Edit));
        let r = store.change_access(child, to_gina, t.mona).await;
        assert!(
            matches!(r, Err(StoreError::Refused(ShareRefusal::Trashed))),
            "{what} : {r:?}"
        );
    }
}

/// Un nœud pris dans un cycle de parents, ou suspendu sous un cycle, n'est vu par personne
/// (R58) : pour le partage, la page n'existe pas (`Unseen`), même pour une propriétaire qui y
/// a une ligne `full`, et rien n'est écrit. Le store interdit les cycles : celui-ci est écrit en
/// base.
#[tokio::test]
async fn change_access_sees_no_page_on_a_cycle_of_parents() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let olga = Subject::User(t.olga);
    let lines = [(olga, Level::Full), (Subject::Members, Level::Full)];
    let a = page_with(store, space, None, &lines).await;
    let b = page_with(store, space, Some(a), &[(olga, Level::Full)]).await;
    let under = page_with(store, space, Some(b), &[(olga, Level::Full)]).await;
    // Avant le cycle, Olga y partage ce qu'elle veut.
    let to_mona = AccessChange::User(t.mona, Some(Level::Read));
    for node in [a, b, under] {
        let written = store.change_access(node, to_mona, t.olga).await;
        assert!(written.is_ok_and(|w| w.changed));
    }

    // `a` passe sous `b`, son propre enfant : `a` et `b` tournent en rond, `under` y est
    // suspendu.
    let cycle = format!("UPDATE nodes SET parent_id = '{b}' WHERE id = '{a}'");
    exec_sql(&f.db, &cycle).await;
    let changes = [
        AccessChange::User(t.mona, Some(Level::Edit)),
        AccessChange::User(t.mona, None),
        AccessChange::Members(Some(Level::Read)),
        AccessChange::Members(None),
    ];
    for node in [a, b, under] {
        for change in changes {
            let before = untouched(store, space, node).await;
            let r = store.change_access(node, change, t.olga).await;
            assert!(
                matches!(r, Err(StoreError::Refused(ShareRefusal::Unseen))),
                "{change:?} : {r:?}"
            );
            assert_eq!(untouched(store, space, node).await, before, "{change:?}");
        }
    }
}

/// Un parent d'un autre espace vaut la racine, comme pour le calcul des accès de tout l'espace :
/// ses lignes ne donnent à la page ni niveau ni plancher. Le store refuse un tel parent : il
/// est écrit en base.
#[tokio::test]
async fn change_access_ignores_a_parent_from_another_space() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let t = team(store, space.id).await;
    let (olga, mona) = (Subject::User(t.olga), Subject::User(t.mona));
    let there = store.create_space("Ailleurs", None).await.unwrap();
    // Ailleurs : une page ouverte à tous en accès complet, où Mona a aussi sa ligne.
    let lines = [(Subject::Members, Level::Full), (mona, Level::Full)];
    let foreign = page_with(store, &there, None, &lines).await;
    let page = page_with(store, space, None, &[(olga, Level::Full)]).await;
    let adopted = format!("UPDATE nodes SET parent_id = '{foreign}' WHERE id = '{page}'");
    exec_sql(&f.db, &adopted).await;

    // Mona, membre de l'espace de la page, n'y voit rien : ni la ligne `members` ni la ligne à
    // son nom de l'autre espace ne comptent.
    let before = untouched(store, space, page).await;
    let to_gina = AccessChange::User(t.gina, Some(Level::Read));
    let r = store.change_access(page, to_gina, t.mona).await;
    assert!(
        matches!(r, Err(StoreError::Refused(ShareRefusal::Unseen))),
        "{r:?}"
    );
    assert_eq!(untouched(store, space, page).await, before);
    // Olga y est chez elle, sans plancher : un accès général en lecture se pose, et se retire.
    let read = AccessChange::Members(Some(Level::Read));
    let written = store.change_access(page, read, t.olga).await;
    assert!(written.is_ok_and(|w| w.changed));
    let none = AccessChange::Members(None);
    let written = store.change_access(page, none, t.olga).await;
    assert!(written.is_ok_and(|w| w.changed));
    assert_eq!(
        store.node_grants(page).await.unwrap(),
        [grant(olga, Level::Full)]
    );
    // Ruling R70 : la ligne `members` de l'autre espace ne met personne en accès complet ici.
    // Celle d'Olga est la seule qui compte : elle ne la baisse pas.
    let before = untouched(store, space, page).await;
    let lower = AccessChange::User(t.olga, Some(Level::Edit));
    let r = store.change_access(page, lower, t.olga).await;
    assert!(matches!(r, Err(StoreError::LastFullAccess)), "{r:?}");
    assert_eq!(untouched(store, space, page).await, before);
    // Les lignes de sa chaîne sont les siennes, et elles seules.
    assert_eq!(
        store.chain_grants(page).await.unwrap(),
        [(page, grant(olga, Level::Full))]
    );
}

/// `chain_grants` rend les lignes de la page et de chacun de ses ancêtres, et d'eux seuls, par
/// nœud puis par sujet. Rien pour un nœud inconnu, ni pour un nœud pris dans un cycle de
/// parents ou suspendu dessous : sa chaîne ne se lit pas (R58).
#[tokio::test]
async fn chain_grants_follow_the_chain_of_the_page() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let user = Subject::User(Uuid::now_v7());
    let top = page_with(store, space, None, &[(Subject::Members, Level::Read)]).await;
    let lines = [(user, Level::Edit), (Subject::Members, Level::Full)];
    let middle = page_with(store, space, Some(top), &lines).await;
    let leaf = page_with(store, space, Some(middle), &[(user, Level::Full)]).await;
    // Hors de la chaîne de `leaf` : une sœur, une sous-page, une autre page de racine.
    page_with(store, space, Some(middle), &[(user, Level::Read)]).await;
    page_with(store, space, Some(leaf), &[(user, Level::Comment)]).await;
    page_with(store, space, None, &[(Subject::Members, Level::Edit)]).await;

    let mut expected = vec![
        (top, grant(Subject::Members, Level::Read)),
        (middle, grant(Subject::Members, Level::Full)),
        (middle, grant(user, Level::Edit)),
        (leaf, grant(user, Level::Full)),
    ];
    expected.sort_by_key(|(node, line)| (node.to_string(), line.subject.to_string()));
    assert_eq!(store.chain_grants(leaf).await.unwrap(), expected);
    assert_eq!(
        store.chain_grants(top).await.unwrap(),
        [(top, grant(Subject::Members, Level::Read))]
    );
    assert_eq!(store.chain_grants(Uuid::now_v7()).await.unwrap(), []);

    // `top` passe sous `middle`, son propre enfant : plus aucune de ces chaînes ne se lit.
    let cycle = format!("UPDATE nodes SET parent_id = '{middle}' WHERE id = '{top}'");
    exec_sql(&f.db, &cycle).await;
    for node in [top, middle, leaf] {
        assert_eq!(store.chain_grants(node).await.unwrap(), []);
    }
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
