//! Ce que le calcul des accès lit d'un espace (`Store::access_inputs`) : la révision, les
//! nœuds, les lignes d'accès et les rôles, pris au même instant.

mod common;

use std::collections::HashSet;

use common::{account, fresh, page};
use mementum_acl::{Grant, Level, Role, Subject, TreeNode};
use mementum_store::{AccessInputs, NewNode, Space};
use uuid::Uuid;

fn grant(subject: Subject, level: Level) -> Grant {
    Grant { subject, level }
}

/// Une page à la racine de l'espace, avec ces accès posés sur elle.
fn page_with(space: &Space, title: &str, grants: &[Grant]) -> NewNode {
    NewNode {
        grants: grants.to_vec(),
        ..page(space, None, None, title)
    }
}

/// Les données lues, dans un ordre fixe : le store ne promet aucun ordre.
fn sorted(mut inputs: AccessInputs) -> AccessInputs {
    inputs.nodes.sort_by_key(|node| node.id);
    inputs
        .grants
        .sort_by_key(|(node, grant)| (*node, grant.subject.to_string()));
    inputs.roles.sort_by_key(|(user, _)| *user);
    inputs
}

/// Contenu exact sur un petit espace : une page à la corbeille (elle y est, avec ses lignes),
/// une ligne `members`, une ligne `user:`, deux rôles. Rien d'un autre espace n'y entre.
#[tokio::test]
async fn inputs_carry_trashed_nodes_grants_and_roles() {
    let f = fresh().await;
    let (store, space) = (&f.store, &f.space);
    let olga = account(store, "olga").await.id;
    let gina = account(store, "gina").await.id;
    store.add_member(space.id, olga, Role::Owner).await.unwrap();
    store.add_member(space.id, gina, Role::Guest).await.unwrap();
    let shared = store
        .create_node(page_with(
            space,
            "Équipe",
            &[grant(Subject::Members, Level::Edit)],
        ))
        .await
        .unwrap();
    let child = store
        .create_node(NewNode {
            grants: vec![grant(Subject::User(gina), Level::Read)],
            ..page(space, Some(shared.id), None, "Pour Gina")
        })
        .await
        .unwrap();
    let trashed = store
        .create_node(page_with(
            space,
            "Jetée",
            &[grant(Subject::User(olga), Level::Full)],
        ))
        .await
        .unwrap();
    store.trash_node(trashed.id, olga).await.unwrap();
    // Un autre espace, avec sa page, sa ligne et son membre.
    let other = store.create_space("Ailleurs", Some(gina)).await.unwrap();
    store
        .create_node(page_with(
            &other,
            "Autre",
            &[grant(Subject::Members, Level::Full)],
        ))
        .await
        .unwrap();

    let inputs = store
        .access_inputs(space.id)
        .await
        .unwrap()
        .expect("l'espace existe");
    assert_eq!(Some(inputs.rev), store.space_rev(space.id).await.unwrap());
    assert_eq!(inputs.role_of(olga), Some(Role::Owner));
    assert_eq!(inputs.role_of(gina), Some(Role::Guest));
    assert_eq!(inputs.role_of(Uuid::now_v7()), None);

    let mut nodes = vec![
        TreeNode {
            id: shared.id,
            parent_id: None,
            trashed: false,
        },
        TreeNode {
            id: child.id,
            parent_id: Some(shared.id),
            trashed: false,
        },
        TreeNode {
            id: trashed.id,
            parent_id: None,
            trashed: true,
        },
    ];
    nodes.sort_by_key(|node| node.id);
    let mut grants = vec![
        (shared.id, grant(Subject::Members, Level::Edit)),
        (child.id, grant(Subject::User(gina), Level::Read)),
        (trashed.id, grant(Subject::User(olga), Level::Full)),
    ];
    grants.sort_by_key(|(node, _)| *node);
    let mut roles = vec![(olga, Role::Owner), (gina, Role::Guest)];
    roles.sort_by_key(|(user, _)| *user);
    assert_eq!(
        sorted(inputs.clone()),
        AccessInputs {
            rev: inputs.rev,
            nodes,
            grants,
            roles,
        }
    );

    // L'autre espace n'a que les siens.
    let elsewhere = store.access_inputs(other.id).await.unwrap().unwrap();
    assert_eq!(elsewhere.nodes.len(), 1);
    assert_eq!(elsewhere.grants.len(), 1);
    assert_eq!(elsewhere.roles, [(gina, Role::Owner)]);
}

/// Un espace qui n'existe pas n'a pas de données ; un espace vide en a, vides.
#[tokio::test]
async fn unknown_space_has_no_inputs() {
    let f = fresh().await;
    assert_eq!(f.store.access_inputs(Uuid::now_v7()).await.unwrap(), None);
    assert_eq!(
        f.store.access_inputs(f.space.id).await.unwrap(),
        Some(AccessInputs {
            rev: 0,
            nodes: Vec::new(),
            grants: Vec::new(),
            roles: Vec::new(),
        })
    );
}

/// Écritures faites pendant que `inputs_are_one_snapshot` lit.
const WRITES: usize = 120;

/// Les quatre valeurs sont du même instant. Une tâche écrit sans arrêt : tour à tour une page
/// avec sa ligne d'accès, puis un membre ; chaque écriture avance la révision d'un cran. Une
/// autre lit pendant ce temps : dans chaque lecture, le nombre de nœuds, de lignes d'accès et
/// de rôles est exactement celui de la révision lue. Lues hors d'une même transaction, une
/// écriture validée entre deux lectures donnerait une révision d'avant avec des nœuds d'après.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inputs_are_one_snapshot() {
    let f = fresh().await;
    let (store, space) = (f.store.clone(), f.space.clone());
    let mut newcomers = Vec::new();
    for i in 0..WRITES / 2 {
        newcomers.push(account(&store, &format!("membre-{i}")).await.id);
    }
    let base = store.access_inputs(space.id).await.unwrap().unwrap();
    assert_eq!((base.rev, base.nodes.len(), base.roles.len()), (0, 0, 0));

    let writer = tokio::spawn({
        let store = store.clone();
        let space = space.clone();
        async move {
            for (i, newcomer) in newcomers.into_iter().enumerate() {
                let title = format!("Page {i}");
                let members = [grant(Subject::Members, Level::Full)];
                store
                    .create_node(page_with(&space, &title, &members))
                    .await
                    .unwrap();
                store
                    .add_member(space.id, newcomer, Role::Member)
                    .await
                    .unwrap();
            }
        }
    });

    let mut seen = HashSet::new();
    while !writer.is_finished() {
        let inputs = store.access_inputs(space.id).await.unwrap().unwrap();
        // À la révision `rev`, `rev` écritures sont faites : une page de plus que de membres
        // si `rev` est impair.
        let rev = usize::try_from(inputs.rev).unwrap();
        assert_eq!(
            (inputs.nodes.len(), inputs.grants.len(), inputs.roles.len()),
            (rev.div_ceil(2), rev.div_ceil(2), rev / 2),
            "à la révision {rev}"
        );
        seen.insert(rev);
    }
    writer.await.unwrap();
    // Le test a bien lu pendant les écritures.
    assert!(
        seen.len() > WRITES / 10,
        "révisions vues en passant : {}",
        seen.len()
    );
    let last = store.access_inputs(space.id).await.unwrap().unwrap();
    assert_eq!(
        (last.rev, last.nodes.len(), last.roles.len()),
        (WRITES as i64, WRITES / 2, WRITES / 2)
    );
}
