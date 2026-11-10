//! Les droits, vus du serveur : le seul chemin par lequel une route apprend ce qu'une personne
//! peut faire dans un espace, et ce qu'elle y voit.
//!
//! - [`AppState::space_access`] : ce que la personne voit de l'espace, et à quel niveau
//!   (`mementum_acl::SpaceAccess`), calculé une fois par révision de l'espace et gardé dans le
//!   cache des accès ;
//! - [`AppState::role_at_least`] : son rôle, s'il suffit ;
//! - [`AppState::rights_changed`] : à appeler après toute écriture qui change des droits, pour
//!   fermer les connexions de synchro qui les avaient lus.
//!
//! Qui n'a aucun rôle dans l'espace reçoit `404`, exactement comme pour un espace qui n'existe
//! pas ; l'admin de l'instance n'a aucun droit particulier dans un espace dont il n'est pas
//! membre. `403 forbidden` ne sert que dans un espace qu'on voit.
//!
//! **Cache** : la clé est `spaces.rev`, que le store incrémente dans la transaction de toute
//! écriture qui touche à la structure ou aux droits de l'espace. La révision est lue **avant**
//! le rôle, les nœuds et les accès, et le calcul est rangé sous elle : si une écriture passe
//! entre-temps, la révision suivante ne trouve pas ce calcul, et rien de périmé n'est servi.

use std::collections::HashSet;
use std::sync::Arc;

use mementum_acl::{Role, SpaceAccess, TreeNode};
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::ApiError;

/// Nœuds visibles que garde le cache des accès, tous calculs confondus (Ruling R59) : à une
/// trentaine d'octets par nœud, environ 36 Mo au plus.
pub(crate) const ACCESS_CACHE_NODES: usize = 1_000_000;

/// Rang d'un rôle : `guest` < `member` < `admin` < `owner`.
fn rank(role: Role) -> u8 {
    match role {
        Role::Guest => 0,
        Role::Member => 1,
        Role::Admin => 2,
        Role::Owner => 3,
    }
}

/// Le rôle de `access`, s'il vaut au moins `min` ; sinon `403 forbidden`.
pub(crate) fn at_least(access: &SpaceAccess, min: Role) -> Result<Role, ApiError> {
    // Un calcul ne sort de `space_access` qu'avec un rôle.
    require(access.role().ok_or_else(ApiError::space_not_found)?, min)
}

/// `role`, s'il vaut au moins `min` ; sinon `403 forbidden`. Pour un rôle lu par
/// [`AppState::role_at_least`] ou [`AppState::space_access`] : sans rôle, c'est `404`.
pub(crate) fn require(role: Role, min: Role) -> Result<Role, ApiError> {
    if rank(role) >= rank(min) {
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

impl AppState {
    /// Les accès de `user` dans l'espace : lus dans le cache si `spaces.rev` n'a pas bougé.
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
        // D'abord la révision : le calcul sera rangé sous elle (voir l'en-tête).
        let Some(rev) = self.store.space_rev(space_id).await.map_err(internal)? else {
            return Ok(None);
        };
        if let Some(access) = self.access.get(user.id, space_id, rev) {
            return Ok(Some(access));
        }
        let Some(role) = self
            .store
            .role_in(space_id, user.id)
            .await
            .map_err(internal)?
        else {
            return Ok(None);
        };
        // Tout l'arbre, corbeille comprise : un ancêtre omis ferait de son enfant une racine.
        let nodes: Vec<TreeNode> = self
            .store
            .list_nodes(space_id)
            .await
            .map_err(internal)?
            .iter()
            .map(|n| TreeNode {
                id: n.id,
                parent_id: n.parent_id,
                trashed: n.trashed_at.is_some(),
            })
            .collect();
        let grants = self.store.space_grants(space_id).await.map_err(internal)?;
        let access = Arc::new(SpaceAccess::compute(&nodes, &grants, user.id, Some(role)));
        self.access.put(user.id, space_id, rev, Arc::clone(&access));
        Ok(Some(access))
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
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use mementum_acl::{AccessCache, Grant, Level, Section, Subject};
    use mementum_store::{NewNode, NewUser, NodeKind, Store};
    use mementum_sync::{Hub, SyncConfig};

    use super::*;
    use crate::auth::LoginThrottle;
    use crate::types::SignupMode;

    /// Un état de serveur sur une base neuve, sans routes, avec le hub que monte `serve`
    /// (le test `e2e_wait_follows_idle_close` de `mementum-sync` y veille).
    async fn state(dir: &tempfile::TempDir) -> AppState {
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
        }
    }

    async fn account(state: &AppState, name: &str) -> AuthUser {
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
            .set_role(space.id, mona.id, Role::Guest)
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
        state.store.remove_member(space.id, mona.id).await.unwrap();
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
}
