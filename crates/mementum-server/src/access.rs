//! Les droits, vus du serveur : le seul chemin par lequel une route apprend ce qu'une personne
//! peut faire dans un espace ou sur une page, et ce qu'elle y voit.
//!
//! - [`AppState::space_access`] : ce que la personne voit de l'espace, et à quel niveau
//!   (`mementum_acl::SpaceAccess`), calculé une fois par révision de l'espace et gardé dans le
//!   cache des accès ; les données de l'espace dont il se calcule sont lues une fois par
//!   révision, pour tout le monde ([`InputsCache`]) ;
//! - [`AppState::role_at_least`] : son rôle, s'il suffit ;
//! - [`AppState::node_access`] : une page et ce qu'elle peut en faire ([`AppState::seen_node`]
//!   pour qui vérifie plusieurs actions, avec [`permit`]) ;
//! - [`AppState::rights_changed`] : à appeler après toute écriture qui change des droits, pour
//!   fermer les connexions de synchro qui les avaient lus.
//!
//! Qui n'a aucun rôle dans l'espace reçoit `404`, exactement comme pour un espace qui n'existe
//! pas ; l'admin de l'instance n'a aucun droit particulier dans un espace dont il n'est pas
//! membre. `403 forbidden` ne sert que dans un espace qu'on voit.
//!
//! **Pages.** Les refus viennent toujours dans cet ordre : `404 not_found` pour une page que
//! la personne ne voit pas (la réponse d'une page qui n'existe pas) ; `403 forbidden` si son
//! niveau ne permet pas l'action ; `409 trashed` si l'action écrit dans une page à la
//! corbeille. Jamais `403` ni `409` pour une page qu'elle ne voit pas.
//!
//! **Cache** : la clé est `spaces.rev`, que le store incrémente dans la transaction de toute
//! écriture qui change une entrée du calcul des accès. Deux étages, tous deux rangés sous elle :
//! - les données de l'espace ([`InputsCache`]) : ses nœuds, ses lignes d'accès et ses rôles,
//!   lus en une transaction avec leur révision (`Store::access_inputs`), et partagés entre les
//!   personnes ; après une écriture, ils sont lus une fois, pour tout le monde ;
//! - le calcul de chaque personne (`mementum_acl::AccessCache`), fait en mémoire sur ces
//!   données.
//!
//! Une requête lit d'abord la révision courante de l'espace. Un calcul ne lui est rendu que
//! s'il est rangé sous cette révision-là. À défaut, il est refait sur les données de l'espace :
//! celles du cache si leur révision est au moins celle que la requête a lue, sinon celles
//! qu'elle lit dans la base (Ruling R98). Jamais de données plus anciennes que la révision
//! lue : `rev` ne fait que croître, et des données plus récentes sont ce que la requête aurait
//! vu en arrivant un instant plus tard (l'argument est en tête de `inputs_cache.rs`). Le
//! calcul est rangé sous la révision des données, celle de leur transaction, jamais sous une
//! révision lue à part. Rien de périmé n'est donc servi : ce qui est rangé sous une révision
//! est l'état de l'espace à cette révision.
//!
//! [`InputsCache`]: crate::inputs_cache::InputsCache

use std::collections::HashSet;
use std::sync::Arc;

use mementum_acl::{Action, Grant, Level, NodeAccess, Role, SpaceAccess, TreeNode};
use mementum_store::Node;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::ApiError;

/// Plafond du cache des accès (Ruling R59), en nœuds visibles plus le coût fixe de chaque
/// calcul (`AccessCache`) : une quarantaine d'octets par unité, soit 40 Mo environ, cache plein.
pub(crate) const ACCESS_CACHE_NODES: usize = 1_000_000;

/// Le rôle de `access`, s'il vaut au moins `min` ; sinon `403 forbidden`.
pub(crate) fn at_least(access: &SpaceAccess, min: Role) -> Result<Role, ApiError> {
    // Un calcul ne sort de `space_access` qu'avec un rôle.
    require(access.role().ok_or_else(ApiError::space_not_found)?, min)
}

/// `role`, s'il vaut au moins `min` ; sinon `403 forbidden`. Pour un rôle lu par
/// [`AppState::role_at_least`] ou [`AppState::space_access`] : sans rôle, c'est `404`.
pub(crate) fn require(role: Role, min: Role) -> Result<Role, ApiError> {
    if role.rank() >= min.rank() {
        return Ok(role);
    }
    Err(ApiError::forbidden(match min {
        Role::Owner => "réservé aux propriétaires de l'espace",
        Role::Admin => "réservé aux propriétaires et aux admins de l'espace",
        Role::Member | Role::Guest => {
            "réservé aux membres de l'espace : un invité ne voit que les pages partagées avec lui"
        }
    }))
}

/// L'action écrit-elle dans la page ? Une page à la corbeille refuse ces actions-là ; elle se
/// lit, se restaure et se supprime encore.
fn writes(action: Action) -> bool {
    match action {
        Action::Edit
        | Action::Rename
        | Action::CreateChild
        | Action::Reorder
        | Action::Move
        | Action::Share
        | Action::Trash => true,
        Action::Read | Action::Comment | Action::Restore | Action::Delete => false,
    }
}

/// Le nom d'un niveau, dans un message.
fn level_name(level: Level) -> &'static str {
    match level {
        Level::Read => "lecture",
        Level::Comment => "commentaire",
        Level::Edit => "modification",
        Level::Full => "accès complet",
    }
}

/// `access` permet-il `action` ? `403 forbidden` si le niveau ne suffit pas ; sinon `409
/// trashed` si l'action écrit dans une page à la corbeille. Pour une page que la personne
/// voit : l'appelant a déjà répondu `404` pour les autres.
pub(crate) fn permit(access: &NodeAccess, action: Action) -> Result<(), ApiError> {
    if !access.level.allows(action) {
        let needed = [Level::Read, Level::Comment, Level::Edit, Level::Full]
            .into_iter()
            .find(|level| level.allows(action))
            .unwrap_or(Level::Full);
        return Err(ApiError::forbidden(format!(
            "niveau insuffisant sur cette page : il faut « {} », vous avez « {} »",
            level_name(needed),
            level_name(access.level)
        )));
    }
    if access.in_trash && writes(action) {
        return Err(ApiError::trashed());
    }
    Ok(())
}

impl AppState {
    /// Le nœud et les accès de `user` dessus. `404` s'il ne le voit pas ; `403 forbidden` si
    /// son niveau ne permet pas `action` ; `409 trashed` si `action` écrit (Edit, Rename,
    /// CreateChild, Reorder, Move, Share, Trash) dans une page à la corbeille.
    pub(crate) async fn node_access(
        &self,
        user: &AuthUser,
        id: Uuid,
        action: Action,
    ) -> Result<(Node, NodeAccess), ApiError> {
        let (node, access, _) = self
            .seen_node(user, id)
            .await?
            .ok_or_else(ApiError::page_not_found)?;
        permit(&access, action)?;
        Ok((node, access))
    }

    /// Le nœud `id`, les accès de `user` dessus, et le calcul des accès de son espace.
    /// `None` s'il ne le voit pas : nœud inconnu, espace où il n'a aucun rôle, page qu'il ne
    /// peut pas lire. L'appelant répond alors comme pour un id inconnu, sans distinguer.
    pub(crate) async fn seen_node(
        &self,
        user: &AuthUser,
        id: Uuid,
    ) -> Result<Option<(Node, NodeAccess, Arc<SpaceAccess>)>, ApiError> {
        let Some(node) = self.store.get_node(id).await.map_err(internal)? else {
            return Ok(None);
        };
        let Some(space) = self.find_space_access(user, node.space_id).await? else {
            return Ok(None);
        };
        let Some(access) = space.node(id).copied() else {
            return Ok(None);
        };
        Ok(Some((node, access, space)))
    }

    /// Ce que les seules lignes posées sur une page (`own`) donnent à `user`, sans ses
    /// ancêtres : ses accès à cette page si elle était à la racine de l'espace, hors corbeille.
    /// `None` : elles ne lui donnent rien. Pour dire ce que devient une page qu'on vient de
    /// créer ou de changer de parent, sans relire les droits de tout l'espace.
    pub(crate) fn own_access(own: &[Grant], user: &AuthUser, role: Role) -> Option<NodeAccess> {
        // Une page seule dans son arbre : son id ne sert qu'à lui rattacher ses lignes.
        let alone = TreeNode {
            id: Uuid::nil(),
            parent_id: None,
            trashed: false,
        };
        let lines: Vec<_> = own.iter().map(|grant| (alone.id, *grant)).collect();
        let access = SpaceAccess::compute(&[alone], &lines, user.id, Some(role));
        access.node(alone.id).copied()
    }

    /// Les accès de `user` dans l'espace : lus dans le cache si `spaces.rev` n'a pas bougé ;
    /// sinon recalculés, sur les données de l'espace lues une fois pour tout le monde.
    /// `404` s'il n'y a aucun rôle.
    pub(crate) async fn space_access(
        &self,
        user: &AuthUser,
        space_id: Uuid,
    ) -> Result<Arc<SpaceAccess>, ApiError> {
        self.find_space_access(user, space_id)
            .await?
            .ok_or_else(ApiError::space_not_found)
    }

    /// Comme [`AppState::space_access`], mais `None` pour un espace inconnu ou sans rôle : la
    /// route choisit sa réponse (une invitation introuvable, par exemple).
    pub(crate) async fn find_space_access(
        &self,
        user: &AuthUser,
        space_id: Uuid,
    ) -> Result<Option<Arc<SpaceAccess>>, ApiError> {
        // D'abord la révision courante : un calcul n'est pris du cache que s'il est rangé sous
        // elle, et des données, que si elles ne sont pas plus anciennes (voir l'en-tête).
        let Some(rev) = self.store.space_rev(space_id).await.map_err(internal)? else {
            return Ok(None);
        };
        if let Some(access) = self.access.get(user.id, space_id, rev) {
            return Ok(Some(access));
        }
        // Les données de l'espace, communes à tous : celles du cache, ou lues à l'instant (tout
        // l'arbre, corbeille comprise ; les lignes d'accès ; les rôles). Deux lectures au plus.
        let load = || self.store.access_inputs(space_id);
        for _ in 0..2 {
            let Some(inputs) = self
                .inputs
                .get_or_load(space_id, rev, load)
                .await
                .map_err(internal)?
            else {
                return Ok(None);
            };
            // Ruling R98 : jamais de données plus anciennes que la révision lue. Plus récentes,
            // elles sont ce que la requête aurait vu un instant plus tard ; plus anciennes,
            // elles porteraient encore un droit retiré depuis. Le cache n'en rend pas, et une
            // lecture de la base part du dernier commit : il faudrait que `rev` recule, ce
            // que le store ne fait jamais. Si cela arrive quand même, elles sont relues.
            if inputs.rev < rev {
                continue;
            }
            // Sans rôle : rien, et aucun calcul n'est rangé.
            let Some(role) = inputs.role_of(user.id) else {
                return Ok(None);
            };
            let access = Arc::new(SpaceAccess::compute(
                &inputs.nodes,
                &inputs.grants,
                user.id,
                Some(role),
            ));
            // Sous la révision des données, lue avec elles : celle lue plus haut peut être
            // plus ancienne, et ce calcul n'est pas le sien.
            self.access
                .put(user.id, space_id, inputs.rev, Arc::clone(&access));
            return Ok(Some(access));
        }
        // Relues, et toujours plus anciennes : pas de droits calculés sur un état dépassé.
        Err(internal(format_args!(
            "les données d'accès de l'espace {space_id} sont plus anciennes que sa révision \
             {rev}, relecture comprise : la révision de l'espace a reculé"
        )))
    }

    /// Le rôle de `user`, au moins `min` (rang guest < member < admin < owner) : `404` sans
    /// rôle, `403 forbidden` en dessous.
    pub(crate) async fn role_at_least(
        &self,
        user: &AuthUser,
        space_id: Uuid,
        min: Role,
    ) -> Result<Role, ApiError> {
        at_least(&*self.space_access(user, space_id).await?, min)
    }

    /// À appeler après le commit de toute écriture qui change des droits : `nodes` = les pages
    /// touchées, sous-arbres compris ; `None` = toutes les pages vivantes de l'espace.
    ///
    /// Ferme les connexions de synchro de ces pages (`Hub::kick`) : les clients se
    /// reconnectent, et la poignée de main relit leurs droits. Les pages vivantes sont lues
    /// ici, donc après le commit. Si l'arbre de l'espace ne peut pas être lu, toutes les pages
    /// vivantes de l'instance sont fermées : une connexion fermée de trop se rouvre, une
    /// connexion qui garde un droit retiré ne se voit pas.
    pub(crate) async fn rights_changed(&self, space_id: Uuid, nodes: Option<&[Uuid]>) {
        match nodes {
            Some(nodes) => self.hub.kick(nodes).await,
            None => self.hub.kick(&self.live_pages_of(space_id).await).await,
        }
    }

    /// [`AppState::rights_changed`] pour `node` et tout son sous-arbre : après le commit d'une
    /// mise à la corbeille, d'une restauration ou d'un changement de parent. Si le sous-arbre
    /// ne peut pas être lu, ce sont toutes les pages vivantes de l'espace qui sont fermées.
    pub(crate) async fn subtree_rights_changed(&self, space_id: Uuid, node: Uuid) {
        match self.store.subtree_ids(node).await {
            Ok(nodes) => self.rights_changed(space_id, Some(&nodes)).await,
            Err(error) => {
                tracing::warn!(
                    space_id = %space_id,
                    node_id = %node,
                    %error,
                    "sous-arbre illisible après un changement de droits : les connexions de \
                     synchro de tout l'espace sont fermées"
                );
                self.rights_changed(space_id, None).await;
            }
        }
    }

    /// Les pages de l'espace qui ont un acteur vivant.
    async fn live_pages_of(&self, space_id: Uuid) -> Vec<Uuid> {
        let live = self.hub.live_pages().await;
        if live.is_empty() {
            return live;
        }
        match self.store.list_nodes(space_id).await {
            Ok(nodes) => {
                let ours: HashSet<Uuid> = nodes.iter().map(|n| n.id).collect();
                live.into_iter().filter(|id| ours.contains(id)).collect()
            }
            Err(error) => {
                tracing::warn!(
                    space_id = %space_id,
                    %error,
                    "arbre de l'espace illisible après un changement de droits : toutes les \
                     connexions de synchro sont fermées"
                );
                live
            }
        }
    }
}

/// Une erreur du store qui ne désigne ici ni page ni espace : `500 internal`.
fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(&error)
}

#[cfg(test)]
pub(crate) mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use mementum_acl::{AccessCache, Grant, Level, Section, Subject};
    use mementum_store::{Expected, NewNode, NewUser, NodeKind, Space, Store};
    use mementum_sync::{Hub, SyncConfig};

    use super::*;
    use crate::auth::LoginThrottle;
    use crate::inputs_cache::tests::wait_until;
    use crate::inputs_cache::{INPUTS_CACHE_UNITS, InputsCache};
    use crate::types::SignupMode;

    /// Un état de serveur sur une base neuve, sans routes, avec le hub que monte `serve`
    /// (le test `e2e_wait_follows_idle_close` de `mementum-sync` y veille). Sert aussi aux
    /// tests des routes.
    pub(crate) async fn state(dir: &tempfile::TempDir) -> AppState {
        let store = Store::open(&dir.path().join("m.db")).await.unwrap();
        let hub = Hub::new(store.clone(), SyncConfig::default());
        AppState {
            hub,
            store,
            check_host: true,
            signup: SignupMode::Open,
            secure_cookies: false,
            public_origin: None,
            logins: Arc::new(LoginThrottle::default()),
            signups: Arc::new(tokio::sync::Mutex::new(())),
            access: Arc::new(AccessCache::new(ACCESS_CACHE_NODES)),
            inputs: Arc::new(InputsCache::new(INPUTS_CACHE_UNITS)),
        }
    }

    pub(crate) async fn account(state: &AppState, name: &str) -> AuthUser {
        state
            .store
            .create_user(NewUser {
                email: format!("{name}@exemple.fr"),
                name: name.to_string(),
                password_hash: "pas-un-hachage".to_string(),
            })
            .await
            .unwrap()
            .into()
    }

    /// Le cache suit `spaces.rev` (complément de `access_cache_follows_rev`, dans
    /// `tests/spaces.rs`, qui ne voit pas les sections) : un membre passé invité perd la
    /// section « Espace » à l'appel suivant ; retiré, il n'a plus rien. Entre deux écritures,
    /// le calcul est servi tel quel.
    #[tokio::test]
    async fn demoted_member_loses_the_space_section() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        // Le premier compte : admin de l'instance, sans rôle dans l'espace.
        let sam = account(&state, "sam").await;
        let olga = account(&state, "olga").await;
        let mona = account(&state, "mona").await;
        let space = state
            .store
            .create_space("Équipe", Some(olga.id))
            .await
            .unwrap();
        state
            .store
            .add_member(space.id, mona.id, Role::Member)
            .await
            .unwrap();
        // Une page des membres, où Mona a aussi sa propre ligne.
        let page = state
            .store
            .create_node(NewNode {
                space_id: space.id,
                parent_id: None,
                after: None,
                kind: NodeKind::Page,
                title: "P".to_string(),
                created_by: Some(olga.id),
                grants: vec![
                    Grant {
                        subject: Subject::Members,
                        level: Level::Full,
                    },
                    Grant {
                        subject: Subject::User(mona.id),
                        level: Level::Read,
                    },
                ],
                doc: None,
            })
            .await
            .unwrap();
        let seen = |access: &SpaceAccess| access.node(page.id).map(|a| (a.level, a.section));

        let first = state.space_access(&mona, space.id).await.unwrap();
        assert_eq!(first.role(), Some(Role::Member));
        assert_eq!(seen(&first), Some((Level::Full, Section::Space)));
        // Sans écriture, le même calcul.
        let again = state.space_access(&mona, space.id).await.unwrap();
        assert!(Arc::ptr_eq(&first, &again));

        // Passée invitée : plus de section « Espace », à l'appel suivant.
        state
            .store
            .set_role(
                space.id,
                mona.id,
                Role::Guest,
                Expected {
                    actor: olga.id,
                    actor_role: Role::Owner,
                    target_role: Role::Member,
                },
            )
            .await
            .unwrap();
        let guest = state.space_access(&mona, space.id).await.unwrap();
        assert_eq!(guest.role(), Some(Role::Guest));
        assert_eq!(seen(&guest), Some((Level::Read, Section::Shared)));
        let status = |result: Result<Role, ApiError>| result.unwrap_err().into_response().status();
        assert_eq!(
            status(state.role_at_least(&mona, space.id, Role::Member).await),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            state.role_at_least(&mona, space.id, Role::Guest).await.ok(),
            Some(Role::Guest)
        );

        // Retirée : `404`, comme un espace qui n'existe pas.
        let by_olga = Expected {
            actor: olga.id,
            actor_role: Role::Owner,
            target_role: Role::Guest,
        };
        state
            .store
            .remove_member(space.id, mona.id, by_olga)
            .await
            .unwrap();
        assert!(
            state
                .find_space_access(&mona, space.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .find_space_access(&mona, Uuid::now_v7())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            status(state.role_at_least(&mona, space.id, Role::Guest).await),
            StatusCode::NOT_FOUND
        );
        // L'admin de l'instance n'y a pas plus de droits qu'un autre.
        assert!(sam.instance_admin);
        assert!(
            state
                .find_space_access(&sam, space.id)
                .await
                .unwrap()
                .is_none()
        );
    }

    /// L'espace « Équipe » d'Olga, avec ces membres.
    async fn team(state: &AppState, olga: &AuthUser, members: &[&AuthUser]) -> Space {
        let space = state
            .store
            .create_space("Équipe", Some(olga.id))
            .await
            .unwrap();
        for member in members {
            state
                .store
                .add_member(space.id, member.id, Role::Member)
                .await
                .unwrap();
        }
        space
    }

    /// Crée une page à la racine de l'espace, ouverte aux membres en accès complet, et rend
    /// son id. Une écriture : la révision de l'espace avance.
    async fn members_page(state: &AppState, space: &Space, by: &AuthUser, title: &str) -> Uuid {
        state
            .store
            .create_node(NewNode {
                space_id: space.id,
                parent_id: None,
                after: None,
                kind: NodeKind::Page,
                title: title.to_string(),
                created_by: Some(by.id),
                grants: vec![Grant {
                    subject: Subject::Members,
                    level: Level::Full,
                }],
                doc: None,
            })
            .await
            .unwrap()
            .id
    }

    /// La révision courante de l'espace.
    async fn rev(state: &AppState, space: &Space) -> i64 {
        state
            .store
            .space_rev(space.id)
            .await
            .unwrap()
            .expect("l'espace")
    }

    /// Les données d'un espace sont lues une fois par révision, pour tout le monde : trois
    /// personnes, une lecture ; les mêmes à nouveau, aucune ; après une écriture, une seule
    /// de plus, pas trois.
    #[tokio::test]
    async fn inputs_are_read_once_per_revision() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let mona = account(&state, "mona").await;
        let tom = account(&state, "tom").await;
        let space = team(&state, &olga, &[&mona, &tom]).await;
        let first = members_page(&state, &space, &olga, "Première").await;
        assert_eq!(state.inputs.loads(), 0);

        for who in [&olga, &mona, &tom] {
            let access = state.space_access(who, space.id).await.unwrap();
            assert_eq!(access.node(first).map(|a| a.level), Some(Level::Full));
        }
        assert_eq!(state.inputs.loads(), 1);
        // Les mêmes : leurs calculs sont gardés, rien n'est relu.
        for who in [&olga, &mona, &tom] {
            state.space_access(who, space.id).await.unwrap();
        }
        assert_eq!(state.inputs.loads(), 1);

        // Une page de plus : chacun la voit à sa requête suivante, pour une seule lecture.
        let second = members_page(&state, &space, &olga, "Seconde").await;
        for who in [&olga, &mona, &tom] {
            let access = state.space_access(who, space.id).await.unwrap();
            assert_eq!(access.node(second).map(|a| a.level), Some(Level::Full));
        }
        assert_eq!(state.inputs.loads(), 2);
        assert_eq!(
            state.inputs.kept(space.id).map(|inputs| inputs.rev),
            Some(rev(&state, &space).await)
        );
    }

    /// Après une écriture, des requêtes simultanées de personnes différentes ne lisent les
    /// données de l'espace qu'une fois. D'abord lancées ensemble, sans rien forcer ; puis
    /// pendant qu'une lecture est en cours (le test tient le tour de lecture de l'espace) :
    /// toutes l'attendent, la première lit, les autres prennent ce qu'elle a lu.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn simultaneous_misses_read_once() {
        const PEOPLE: usize = 12;
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let mut people = vec![olga.clone()];
        for i in 1..PEOPLE {
            people.push(account(&state, &format!("membre-{i}")).await);
        }
        let members: Vec<&AuthUser> = people[1..].iter().collect();
        let space = team(&state, &olga, &members).await;
        // Chacun demande les accès de l'espace, dans sa tâche, et dit s'il voit la page.
        let ask = |page: Uuid| -> Vec<_> {
            let start = Arc::new(tokio::sync::Barrier::new(PEOPLE));
            people
                .iter()
                .cloned()
                .map(|who| {
                    let (state, start, space) = (state.clone(), Arc::clone(&start), space.id);
                    tokio::spawn(async move {
                        start.wait().await;
                        let access = state.find_space_access(&who, space).await;
                        access.ok().flatten().map(|a| a.node(page).is_some())
                    })
                })
                .collect()
        };

        let page = members_page(&state, &space, &olga, "Première").await;
        for task in ask(page) {
            assert_eq!(task.await.unwrap(), Some(true));
        }
        assert_eq!(state.inputs.loads(), 1);

        let page = members_page(&state, &space, &olga, "Seconde").await;
        let held = state.inputs.hold_turn(space.id).await;
        let tasks = ask(page);
        wait_until("toutes les requêtes attendent la lecture en cours", || {
            state.inputs.waiting(space.id) == PEOPLE + 1
        })
        .await;
        assert_eq!(state.inputs.loads(), 1);
        drop(held);
        for task in tasks {
            assert_eq!(task.await.unwrap(), Some(true));
        }
        assert_eq!(state.inputs.loads(), 2);
    }

    /// Des données gardées ne servent qu'à leur révision. Les données de l'espace sont en
    /// cache, et Mona, qui n'a encore rien demandé, y est membre ; elle est retirée : à sa
    /// première requête, elle n'a rien. Tom, dont le calcul est gardé, passe invité : à sa
    /// requête suivante, il ne voit plus la page des membres.
    #[tokio::test]
    async fn stale_inputs_are_never_served() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let mona = account(&state, "mona").await;
        let tom = account(&state, "tom").await;
        let space = team(&state, &olga, &[&mona, &tom]).await;
        let page = members_page(&state, &space, &olga, "P").await;
        for who in [&olga, &tom] {
            let access = state.space_access(who, space.id).await.unwrap();
            assert!(access.node(page).is_some());
        }
        let before = rev(&state, &space).await;
        let by_olga = Expected {
            actor: olga.id,
            actor_role: Role::Owner,
            target_role: Role::Member,
        };

        state
            .store
            .remove_member(space.id, mona.id, by_olga)
            .await
            .unwrap();
        // Les données d'avant sont toujours là, et Mona y a un rôle.
        let kept = state.inputs.kept(space.id).expect("des données gardées");
        assert_eq!(
            (kept.rev, kept.role_of(mona.id)),
            (before, Some(Role::Member))
        );
        assert!(
            state
                .find_space_access(&mona, space.id)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(state.inputs.loads(), 2);
        let kept = state.inputs.kept(space.id).expect("des données gardées");
        assert_eq!((kept.rev, kept.role_of(mona.id)), (before + 1, None));

        state
            .store
            .set_role(space.id, tom.id, Role::Guest, by_olga)
            .await
            .unwrap();
        let access = state.space_access(&tom, space.id).await.unwrap();
        assert_eq!(access.role(), Some(Role::Guest));
        assert!(access.node(page).is_none());
        assert_eq!(state.inputs.loads(), 3);
    }

    /// Une écriture qui passe entre la lecture de la révision et celle des données : la
    /// requête reçoit les données d'après l'écriture, et son calcul est rangé sous leur
    /// révision, pas sous celle qu'elle avait lue. L'ordre est forcé par le tour de lecture de
    /// l'espace, que le test tient : la requête de Mona lit la révision, puis attend son tour ;
    /// Mona passe invitée ; le tour est rendu.
    #[tokio::test]
    async fn access_is_filed_under_the_revision_of_its_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let mona = account(&state, "mona").await;
        let space = team(&state, &olga, &[&mona]).await;
        let page = members_page(&state, &space, &olga, "P").await;
        let before = rev(&state, &space).await;

        let held = state.inputs.hold_turn(space.id).await;
        let request = tokio::spawn({
            let (state, mona, space) = (state.clone(), mona.clone(), space.id);
            async move { state.find_space_access(&mona, space).await }
        });
        wait_until("la requête attend son tour de lecture", || {
            state.inputs.waiting(space.id) == 2
        })
        .await;
        let by_olga = Expected {
            actor: olga.id,
            actor_role: Role::Owner,
            target_role: Role::Member,
        };
        state
            .store
            .set_role(space.id, mona.id, Role::Guest, by_olga)
            .await
            .unwrap();
        drop(held);

        let access = request.await.unwrap().unwrap().expect("un rôle");
        assert_eq!(access.role(), Some(Role::Guest));
        assert!(access.node(page).is_none());
        assert!(state.access.get(mona.id, space.id, before).is_none());
        let filed = state.access.get(mona.id, space.id, before + 1);
        assert!(filed.is_some_and(|filed| Arc::ptr_eq(&filed, &access)));
        assert_eq!(
            state.inputs.kept(space.id).map(|inputs| inputs.rev),
            Some(before + 1)
        );
    }

    /// Ruling R98 : après l'attente de son tour, une requête prend des données plus récentes
    /// que la révision qu'elle a lue, au lieu de relire. Mona et Tom lisent la révision, puis
    /// attendent le tour de lecture, que le test tient ; une page est créée ; le tour est rendu.
    /// La première requête lit les données d'après l'écriture, l'autre les prend : une seule
    /// lecture pour les deux, et le calcul de chacune est rangé sous la révision des données.
    #[tokio::test]
    async fn waiting_requests_take_the_inputs_read_after_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let mona = account(&state, "mona").await;
        let tom = account(&state, "tom").await;
        let space = team(&state, &olga, &[&mona, &tom]).await;
        let before = rev(&state, &space).await;

        let held = state.inputs.hold_turn(space.id).await;
        let requests = [&mona, &tom].map(|who| {
            let (state, who, space) = (state.clone(), who.clone(), space.id);
            tokio::spawn(async move { state.find_space_access(&who, space).await })
        });
        wait_until("les deux requêtes attendent leur tour de lecture", || {
            state.inputs.waiting(space.id) == 3
        })
        .await;
        let page = members_page(&state, &space, &olga, "Écrite pendant l'attente").await;
        drop(held);

        for (request, who) in requests.into_iter().zip([&mona, &tom]) {
            let access = request.await.unwrap().unwrap().expect("un rôle");
            assert!(access.node(page).is_some());
            assert!(state.access.get(who.id, space.id, before).is_none());
            let filed = state.access.get(who.id, space.id, before + 1);
            assert!(filed.is_some_and(|filed| Arc::ptr_eq(&filed, &access)));
        }
        assert_eq!(state.inputs.loads(), 1);
        assert_eq!(
            state.inputs.kept(space.id).map(|inputs| inputs.rev),
            Some(before + 1)
        );
    }

    /// Une connexion à la base de `dir`, à côté du store : pour y écrire ce qu'aucune de ses
    /// fonctions n'écrit.
    async fn beside_the_store(dir: &tempfile::TempDir) -> sqlx::SqlitePool {
        let db = dir.path().join("m.db");
        sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
            .await
            .unwrap()
    }

    /// Ajoute `by` à la révision de l'espace, sans rien écrire d'autre : ce que le store ne
    /// fait jamais, surtout pas vers le bas.
    async fn shift_rev(side: &sqlx::SqlitePool, space: &Space, by: i64) {
        sqlx::query("UPDATE spaces SET rev = rev + ? WHERE id = ?")
            .bind(by)
            .bind(space.id.to_string())
            .execute(side)
            .await
            .unwrap();
    }

    /// La garde du Ruling R98, là où le calcul est fait : des données plus anciennes que la
    /// révision lue par la requête ne servent pas à son calcul, elles sont relues. Il faut pour
    /// cela que la révision recule, ce que le store ne fait jamais : le test l'écrit dans la
    /// base, à côté de lui, entre la lecture de la révision et celle des données. L'ordre est
    /// forcé par le tour de lecture : le test le tient, la requête de Mona attend derrière lui,
    /// puis un second tour derrière elle ; la révision recule, la requête lit ; pendant que le
    /// second tour est tenu, la révision revient ; la requête relit, et sert ce qu'elle relit.
    #[tokio::test]
    async fn inputs_older_than_the_revision_read_are_reread() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let mona = account(&state, "mona").await;
        let space = team(&state, &olga, &[&mona]).await;
        let page = members_page(&state, &space, &olga, "P").await;
        let before = rev(&state, &space).await;
        let side = beside_the_store(&dir).await;

        let first = state.inputs.hold_turn(space.id).await;
        let request = tokio::spawn({
            let (state, mona, space) = (state.clone(), mona.clone(), space.id);
            async move { state.find_space_access(&mona, space).await }
        });
        wait_until("la requête attend son tour de lecture", || {
            state.inputs.waiting(space.id) == 2
        })
        .await;
        let (taken, is_taken) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel::<()>();
        let second = tokio::spawn({
            let (state, space) = (state.clone(), space.id);
            async move {
                let held = state.inputs.hold_turn(space).await;
                taken.send(()).unwrap();
                released.await.unwrap();
                drop(held);
            }
        });
        wait_until("un second tour attend derrière la requête", || {
            state.inputs.waiting(space.id) == 3
        })
        .await;

        shift_rev(&side, &space, -1).await;
        drop(first);
        // Le second tour est pris : la requête a lu, et rendu le sien.
        is_taken.await.unwrap();
        assert_eq!(
            state.inputs.kept(space.id).map(|inputs| inputs.rev),
            Some(before - 1)
        );
        shift_rev(&side, &space, 1).await;
        release.send(()).unwrap();

        let access = request.await.unwrap().unwrap().expect("un rôle");
        second.await.unwrap();
        assert!(access.node(page).is_some());
        assert_eq!(state.inputs.loads(), 2);
        assert!(state.access.get(mona.id, space.id, before - 1).is_none());
        let filed = state.access.get(mona.id, space.id, before);
        assert!(filed.is_some_and(|filed| Arc::ptr_eq(&filed, &access)));
        side.close().await;
    }

    /// La même garde, quand la relecture rapporte encore des données plus anciennes que la
    /// révision lue : la requête répond `500` plutôt que de calculer dessus, et aucun calcul
    /// n'est rangé. La requête suivante lit la révision d'à présent, et sert.
    #[tokio::test]
    async fn inputs_still_older_after_a_reread_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let mona = account(&state, "mona").await;
        let space = team(&state, &olga, &[&mona]).await;
        let page = members_page(&state, &space, &olga, "P").await;
        let before = rev(&state, &space).await;
        let side = beside_the_store(&dir).await;

        let held = state.inputs.hold_turn(space.id).await;
        let request = tokio::spawn({
            let (state, mona, space) = (state.clone(), mona.clone(), space.id);
            async move { state.find_space_access(&mona, space).await }
        });
        wait_until("la requête attend son tour de lecture", || {
            state.inputs.waiting(space.id) == 2
        })
        .await;
        shift_rev(&side, &space, -1).await;
        side.close().await;
        drop(held);

        let error = request.await.unwrap().unwrap_err();
        assert_eq!(
            error.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(state.inputs.loads(), 2);
        for rev in [before - 1, before] {
            assert!(state.access.get(mona.id, space.id, rev).is_none());
        }

        let access = state.space_access(&mona, space.id).await.unwrap();
        assert!(access.node(page).is_some());
        assert_eq!(state.inputs.loads(), 2);
    }

    /// Sans rôle dans l'espace : rien, et aucun calcul n'est rangé. Les données de l'espace,
    /// elles, sont gardées : elles servent à ceux qui y ont un rôle.
    #[tokio::test]
    async fn no_role_is_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let sam = account(&state, "sam").await;
        let olga = account(&state, "olga").await;
        let space = team(&state, &olga, &[]).await;
        members_page(&state, &space, &olga, "P").await;
        let now = rev(&state, &space).await;

        for _ in 0..2 {
            assert!(
                state
                    .find_space_access(&sam, space.id)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(state.access.get(sam.id, space.id, now).is_none());
        }
        assert_eq!(state.inputs.loads(), 1);
        state.space_access(&olga, space.id).await.unwrap();
        assert!(state.access.get(olga.id, space.id, now).is_some());
        assert!(state.access.get(sam.id, space.id, now).is_none());
        assert_eq!(state.inputs.loads(), 1);

        // Un espace qui n'existe pas : rien n'est lu ni gardé pour lui.
        let nowhere = Uuid::now_v7();
        assert!(
            state
                .find_space_access(&olga, nowhere)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!((state.inputs.loads(), state.inputs.slots()), (1, 1));
    }

    /// Le plafond, en unités, est tenu : quand les données d'un espace de plus n'y tiennent pas,
    /// celles de l'espace le plus anciennement servi sortent d'abord ; des données plus grosses
    /// que le plafond, à elles seules, ne sont pas gardées, et ne chassent rien.
    #[tokio::test]
    async fn inputs_cache_is_bounded() {
        // Un espace de 3 pages pèse 16 : 3 nœuds, 3 lignes d'accès, 2 rôles, et le coût fixe.
        // Deux tiennent sous le plafond, pas trois.
        const MAX: usize = 40;
        let dir = tempfile::tempdir().unwrap();
        let mut state = state(&dir).await;
        state.inputs = Arc::new(InputsCache::new(MAX));
        let olga = account(&state, "olga").await;
        let mona = account(&state, "mona").await;
        let mut spaces = Vec::new();
        for pages in [3, 3, 3, 20] {
            let space = team(&state, &olga, &[&mona]).await;
            for i in 0..pages {
                members_page(&state, &space, &olga, &format!("Page {i}")).await;
            }
            spaces.push(space.id);
        }
        let [a, b, c, big] = spaces[..] else {
            unreachable!("quatre espaces");
        };
        let kept = |space: Uuid| state.inputs.kept(space).is_some();

        state.space_access(&olga, a).await.unwrap();
        state.space_access(&olga, b).await.unwrap();
        assert_eq!(state.inputs.kept_units(), 32);
        // Mona est servie des données de A, sans lecture : B devient le plus ancien servi.
        state.space_access(&mona, a).await.unwrap();
        assert_eq!(state.inputs.loads(), 2);

        state.space_access(&olga, c).await.unwrap();
        assert_eq!((kept(a), kept(b), kept(c)), (true, false, true));
        assert_eq!(state.inputs.kept_units(), 32);

        // 20 pages pèsent 50 : servies, pas gardées ; la personne suivante les relit.
        let access = state.space_access(&olga, big).await.unwrap();
        assert_eq!(access.visible().count(), 20);
        assert_eq!((kept(a), kept(c), kept(big)), (true, true, false));
        state.space_access(&mona, big).await.unwrap();
        assert_eq!(state.inputs.loads(), 5);
        assert_eq!((state.inputs.kept_units(), state.inputs.slots()), (32, 2));
    }

    /// Une lecture qui échoue : l'erreur remonte (`500`), et rien n'est rangé, ni données ni
    /// calcul. La lecture suivante, réparée, sert.
    #[tokio::test]
    async fn failed_read_is_an_error_and_keeps_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let space = team(&state, &olga, &[]).await;
        let page = members_page(&state, &space, &olga, "P").await;
        let now = rev(&state, &space).await;
        // Une ligne d'accès que le store ne sait pas relire : il refuse de lire l'espace.
        let db = dir.path().join("m.db");
        let side = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
            .await
            .unwrap();
        sqlx::query("INSERT INTO acl (node_id, subject, level) VALUES (?, 'personne', 'read')")
            .bind(page.to_string())
            .execute(&side)
            .await
            .unwrap();

        let error = state.find_space_access(&olga, space.id).await.unwrap_err();
        assert_eq!(
            error.into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!((state.inputs.loads(), state.inputs.slots()), (1, 0));
        assert!(state.access.get(olga.id, space.id, now).is_none());

        sqlx::query("DELETE FROM acl WHERE subject = 'personne'")
            .execute(&side)
            .await
            .unwrap();
        side.close().await;
        let access = state.space_access(&olga, space.id).await.unwrap();
        assert!(access.node(page).is_some());
        assert_eq!(state.inputs.loads(), 2);
    }
}
