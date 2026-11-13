//! Routes du partage d'une page : qui y a accès et à quel niveau, l'accès général (la ligne
//! `members`), l'accès d'une personne, les invitations sur la page.
//!
//! | Route | Réponse |
//! |---|---|
//! | `GET /api/v1/pages/{id}/access` | [`PageAccess`] |
//! | `PUT /api/v1/pages/{id}/access/general` ([`SetGeneral`]) | [`PageAccess`], où figure qui a restreint l'accès général (R122) ; `level: null` : la ligne `members` de la page est retirée ; réservé aux rôles `member` et plus (R85) ; `409 inherited_access` sous le plancher d'une page parente (R93) |
//! | `PUT /api/v1/pages/{id}/access/users/{userId}` ([`SetLevel`]) | [`PageAccess`] ; `404` si le compte n'a aucun rôle dans l'espace |
//! | `DELETE /api/v1/pages/{id}/access/users/{userId}` | [`PageAccess`] ; `404` si la page n'a pas de ligne propre pour ce compte |
//! | `POST /api/v1/pages/{id}/invitations` ([`InviteToPage`]) | `201`, `InvitationCreated` |
//!
//! **Droits.** Toutes demandent `Share`, donc `full` sur la page. Les refus viennent dans cet
//! ordre : `404` pour qui ne la voit pas, la réponse d'une page qui n'existe pas ; `403
//! forbidden` sous `full` ; pour l'accès général, `403 forbidden` pour un invité, même en `full`
//! (Ruling R85 : ce que voient les membres de l'espace, c'est aux membres de le décider) ;
//! `409 trashed` pour un changement sur une page à la corbeille, dont les accès se lisent
//! encore ; `404` pour la personne visée (sans rôle dans l'espace, ou sans ligne propre à
//! retirer) ; `409 inherited_access` ; `409 last_full_access`. `GET …/access` porte le nom et
//! l'adresse des comptes : il n'est rendu qu'à qui a `full` ; après un changement qui lui ôte
//! `full` (on peut se retirer soi-même, ou baisser sa ligne), la réponse est `204`, sans corps.
//! Un changement de l'accès général ne l'ôte jamais à qui le demande (Ruling R122) : sa réponse
//! est le `PageAccess`, sauf si un autre changement lui ôte `full` entre le commit et la
//! relecture des accès : la réponse est alors `204`, que le client traite de même.
//!
//! **Ce qui fait foi est la transaction** (Ruling R87). Avant de lire le corps, la route répond
//! par le cache des accès (`access.rs`) : `404`, `403`, `409 trashed`, sans attendre. Mais un
//! changement est vérifié en entier par le store, dans la transaction qui l'écrit
//! (`Store::change_access`, au nom de qui demande) : son rôle et son niveau sur la page et ses
//! ancêtres (`mementum_acl::chain_level`, le même calcul que celui de tout l'espace), R85, la
//! corbeille, la personne visée, l'accès général hérité, et R70. Rien n'y dépend de la révision
//! de l'espace : écrire ailleurs dans l'espace ne bloque pas un partage, et un appelant qui perd
//! `full` entre sa requête et l'écriture n'écrit rien.
//!
//! **Règles.**
//! - on donne `read`, `comment`, `edit` ou `full`, à un compte qui a un rôle dans l'espace,
//!   membre ou invité. Un compte sans rôle répond comme un compte inconnu ;
//! - un invité en `full` fait tout ce que donne `full` (Ruling R86 : inviter jusqu'à `full`,
//!   changer ou retirer les lignes des autres, lire les accès, révoquer les invitations de la
//!   page), sauf toucher à l'accès général (R85) ;
//! - l'accès général qu'une page parente donne est le plancher de la sous-page (Ruling R93,
//!   `inheritedGeneral` ; `general` est le niveau effectif) : un niveau plus bas, ou `null`
//!   sans ligne propre, répond `409 inherited_access` ; au plancher, la ligne propre part ;
//!   au-dessus, elle est posée ; `null` la retire (on revient au plancher, ou, sans plancher, à
//!   « seulement les personnes nommées »). Ce `PUT` ne laisse donc pas de ligne redondante
//!   derrière lui. Il peut en rester d'ailleurs, sans effet sur les droits : une ligne propre
//!   posée avant qu'une page parente reçoive un accès général plus fort, ou venue d'un
//!   changement de parent ; demander le niveau du plancher, ou `null`, la retire ;
//! - qui restreint l'accès général d'une page y garde l'accès complet (Ruling R122) : quand
//!   retirer la ligne `members`, ou la baisser, lui ferait perdre `full`, sa ligne
//!   `(user:<lui>, full)` est posée dans la même transaction, et il figure dans le `PageAccess`
//!   rendu. C'est la règle du passage à la racine (R82), sans ligne redondante : rien n'est
//!   écrit pour lui s'il garde `full` autrement (déjà nommé sur la page ou sur une page parente ;
//!   propriétaire ou admin là où une ligne `members` s'applique encore). Une page née dans
//!   « Espace », où personne n'est nommé, redevient ainsi privée : elle passe dans « Privé », ou
//!   dans « Partagé » s'il reste d'autres personnes nommées, et les autres membres, admins
//!   compris, ne la voient plus (`404`). Sous un plancher (R93), la ligne propre retirée, un
//!   membre simple retombe au niveau du plancher : sa ligne est posée aussi ;
//! - après tout changement, la page garde quelqu'un en `full`, par ses lignes ou par celles
//!   d'un ancêtre (Ruling R70 : une ligne `members` compte, les admins y ont `full` ; une ligne
//!   restée à quelqu'un sans rôle, non) ; sinon `409 last_full_access`, et rien n'est écrit.
//!   Depuis R122, ce refus ne vient plus que d'un accès nommé qu'on retire ou qu'on baisse ;
//! - retirer son dernier accès à un invité le fait sortir de l'espace, avec ses invitations en
//!   attente (même transaction) ;
//! - après le commit, les connexions de synchro de la page et de ses sous-pages sont fermées
//!   (`rights_changed`), et la page sort de l'arbre de qui l'a perdue à l'appel suivant (le
//!   cache des accès suit `spaces.rev`). Écriture et fermeture vont dans une tâche à part :
//!   elles vont au bout même si le client coupe la connexion. Un changement qui ne change rien
//!   n'écrit rien et ne ferme rien ;
//! - une invitation sur une page ne cherche aucun compte : la réponse est la même, que
//!   l'adresse en ait un ou non (Ruling R47). Elle ne vaut que tant que son auteur peut
//!   partager la page (Ruling R69, `invitations::valid_invitation`, à chaque usage) : sa
//!   création n'est pas revérifiée dans une transaction, une invitation faite par quelqu'un qui
//!   vient de perdre `full` ne servant à rien. Ce n'est pas un droit : la créer ne ferme aucune
//!   connexion.

use std::collections::HashMap;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mementum_acl::{Action, Level, NodeAccess, Subject};
use mementum_store::{AccessChange, AccessWritten, InvitationGrant, Node};
use uuid::Uuid;

use crate::access::permit;
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::routes::invitations::{self, pending_invitation};
use crate::routes::pages::page_id;
use crate::types::{
    AccessEntry, InvitationCreated, InviteToPage, PageAccess, SetGeneral, SetLevel,
};
use crate::{AppState, to_completion};

/// `GET /api/v1/pages/{id}/access` : qui a accès à la page, et à quel niveau (`full`). Une
/// page à la corbeille se lit encore : ses accès aussi.
pub(crate) async fn access(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<PageAccess>, ApiError> {
    let id = page_id(path)?;
    let (node, access, _) = state
        .seen_node(&user, id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    permit(&ignoring_trash(access), Action::Share)?;
    Ok(Json(page_access(&state, &node).await?))
}

/// `PUT /api/v1/pages/{id}/access/general` : pose la ligne `members` de la page à ce niveau,
/// ou la retire (`null`). Réservé aux rôles `member` et plus (R85).
pub(crate) async fn set_general(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<SetGeneral>, JsonRejection>,
) -> Result<Response, ApiError> {
    let id = page_id(path)?;
    // Avant le corps, par le cache, dans l'ordre des refus : la page, le niveau, R85, la
    // corbeille. La transaction du store revérifie tout.
    let (_, access, space) = state
        .seen_node(&user, id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    permit(&ignoring_trash(access), Action::Share)?;
    if !space.role().is_some_and(|role| role.is_member()) {
        return Err(ApiError::guest_general_access());
    }
    permit(&access, Action::Share)?;
    let Json(body) = body?;
    let change = AccessChange::Members(body.level.map(Level::from));
    change_and_answer(state, user, id, change).await
}

/// `PUT /api/v1/pages/{id}/access/users/{userId}` : donne ce niveau sur la page au compte, qui
/// doit avoir un rôle dans l'espace (sinon `404`, comme pour un compte inconnu).
pub(crate) async fn set_user(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<SetLevel>, JsonRejection>,
) -> Result<Response, ApiError> {
    let (id, target) = user_path(path)?;
    state.node_access(&user, id, Action::Share).await?;
    let Json(body) = body?;
    let target = target.ok_or_else(ApiError::member_not_found)?;
    let change = AccessChange::User(target, Some(body.level.into()));
    change_and_answer(state, user, id, change).await
}

/// `DELETE /api/v1/pages/{id}/access/users/{userId}` : retire la ligne du compte sur la page
/// (`404` s'il n'en a pas : un accès hérité se retire sur la page qui le porte). Un invité qui
/// n'a plus aucune ligne dans l'espace en sort.
pub(crate) async fn remove_user(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let (id, target) = user_path(path)?;
    state.node_access(&user, id, Action::Share).await?;
    let target = target.ok_or_else(ApiError::access_not_found)?;
    change_and_answer(state, user, id, AccessChange::User(target, None)).await
}

/// `POST /api/v1/pages/{id}/invitations` : `201`, l'id de l'invitation et son jeton, montré
/// cette fois seulement. La réponse ne dit pas si l'adresse a un compte.
pub(crate) async fn invite(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<InviteToPage>, JsonRejection>,
) -> Result<(StatusCode, Json<InvitationCreated>), ApiError> {
    let id = page_id(path)?;
    let (node, _) = state.node_access(&user, id, Action::Share).await?;
    let Json(body) = body?;
    let grant = InvitationGrant::Page {
        node_id: id,
        level: body.level.into(),
    };
    invitations::create(&state, &user, node.space_id, grant, &body.email).await
}

/// Les accès `access`, sans la corbeille : pour une lecture, ou pour donner le refus de niveau
/// avant `409 trashed` quand un refus se glisse entre les deux (R85).
fn ignoring_trash(access: NodeAccess) -> NodeAccess {
    NodeAccess {
        in_trash: false,
        ..access
    }
}

/// La page et le compte visé d'un chemin `…/pages/{id}/access/users/{userId}`. Une page
/// illisible n'existe pas (`404`) ; un compte illisible (`None`) n'existe pas non plus, mais on
/// ne le dit qu'à qui peut partager la page.
fn user_path(
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<(Uuid, Option<Uuid>), ApiError> {
    let Ok(Path((page, user))) = path else {
        return Err(ApiError::page_not_found());
    };
    let page = Uuid::parse_str(&page).map_err(|_| ApiError::page_not_found())?;
    Ok((page, Uuid::parse_str(&user).ok()))
}

/// Écrit `change` dans une tâche à part, puis répond : les accès de la page si l'appelant y a
/// encore `full`, sinon `204`.
async fn change_and_answer(
    state: AppState,
    user: AuthUser,
    id: Uuid,
    change: AccessChange,
) -> Result<Response, ApiError> {
    let writer = state.clone();
    let by = user.clone();
    to_completion(async move { write(&writer, &by, id, change).await }).await?;
    match state.seen_node(&user, id).await? {
        Some((node, access, _)) if access.level.allows(Action::Share) => {
            Ok(Json(page_access(&state, &node).await?).into_response())
        }
        // Le changement est fait, mais l'appelant n'a plus `full` sur la page (il s'est retiré
        // son accès, ou l'a baissé) : il n'en voit plus les accès.
        _ => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

/// Écrit `change` au nom de `user` : le store vérifie tout dans la transaction qui écrit, et
/// rend un refus typé, que `error.rs` traduit dans les statuts du partage. Après le commit, les
/// connexions de synchro du sous-arbre de la page sont fermées.
async fn write(
    state: &AppState,
    user: &AuthUser,
    id: Uuid,
    change: AccessChange,
) -> Result<AccessWritten, ApiError> {
    let written = state.store.change_access(id, change, user.id).await?;
    if written.changed {
        log_change(user, id, change, written);
        state.subtree_rights_changed(written.space_id, id).await;
    }
    Ok(written)
}

/// Journal d'un changement écrit : qui, sur quelle page, pour qui, et la ligne que la page porte
/// désormais (`line` : son niveau, ou `aucune`) ; jamais d'adresse. C'est ce que le store a
/// écrit, pas ce qui était demandé : un accès général demandé au niveau du plancher retire la
/// ligne propre de la page. Pour l'accès général, `actor_line_posed` dit l'autre écriture
/// (Ruling R122) : la ligne `(user:<user_id>, full)` posée avec lui sur la page, pour qui l'a
/// restreint.
fn log_change(user: &AuthUser, id: Uuid, change: AccessChange, written: AccessWritten) {
    let line = written.line.map_or("aucune", Level::as_str);
    match change {
        AccessChange::Members(_) => tracing::info!(
            node_id = %id,
            space_id = %written.space_id,
            user_id = %user.id,
            line,
            actor_line_posed = written.actor_line_posed,
            "accès général d'une page changé"
        ),
        AccessChange::User(target, _) => tracing::info!(
            node_id = %id,
            space_id = %written.space_id,
            user_id = %user.id,
            target_id = %target,
            line,
            left_space = written.left_space,
            "accès d'une personne à une page changé"
        ),
    }
}

/// Les accès de la page `node`, lus sur sa chaîne (la page et ses ancêtres) : l'accès général
/// effectif (`general`, la plus forte ligne `members` de la chaîne) et son plancher
/// (`inheritedGeneral`, la plus forte de celles des ancêtres, Ruling R93) ; les comptes nommés
/// (une fois chacun, à leur niveau le plus fort, hérité ou non) ; les invitations sur la page
/// qui attendent encore, avec son titre (report M11 du lot 2).
/// Seuls les comptes qui ont un rôle dans l'espace y figurent : une ligne restée à quelqu'un
/// qui n'en a plus ne donne rien.
async fn page_access(state: &AppState, node: &Node) -> Result<PageAccess, ApiError> {
    let mut own_general: Option<Level> = None;
    let mut above_general: Option<Level> = None;
    let mut own: HashMap<Uuid, Level> = HashMap::new();
    let mut above: HashMap<Uuid, Level> = HashMap::new();
    for (on, grant) in state.store.chain_grants(node.id).await? {
        let here = on == node.id;
        match grant.subject {
            Subject::Members if here => own_general = own_general.max(Some(grant.level)),
            Subject::Members => above_general = above_general.max(Some(grant.level)),
            Subject::User(user) if here => {
                own.insert(user, grant.level);
            }
            Subject::User(user) => {
                let level = above.entry(user).or_insert(grant.level);
                *level = (*level).max(grant.level);
            }
        }
    }
    let entries = state
        .store
        .members(node.space_id)
        .await?
        .into_iter()
        .filter_map(|member| {
            let (level, inherited) = match (own.get(&member.user_id), above.get(&member.user_id)) {
                (None, None) => return None,
                (Some(&own), Some(&above)) if above > own => (above, true),
                (Some(&own), _) => (own, false),
                (None, Some(&above)) => (above, true),
            };
            Some(AccessEntry {
                user_id: member.user_id.to_string(),
                name: member.name,
                email: member.email,
                level: level.into(),
                inherited,
            })
        })
        .collect();
    let invitations = state
        .store
        .pending_invitations(node.space_id)
        .await?
        .into_iter()
        .filter(|i| matches!(i.grant, InvitationGrant::Page { node_id, .. } if node_id == node.id))
        .map(|i| pending_invitation(i, Some(node.title.clone())))
        .collect();
    Ok(PageAccess {
        general: own_general.max(above_general).map(Into::into),
        inherited_general: above_general.map(Into::into),
        entries,
        invitations,
    })
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use mementum_acl::{Grant, Role};
    use mementum_store::{NewNode, NodeKind};

    use super::*;
    use crate::access::tests::{account, state};

    /// Mona a passé le contrôle de la route (elle avait `full`) ; Olga la baisse en lecture
    /// avant que Mona écrive. La transaction de l'écriture revérifie son niveau : `403`, et Tom
    /// n'a rien reçu. Olga, elle, écrit.
    #[tokio::test]
    async fn caller_who_loses_full_before_writing_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let mona = account(&state, "mona").await;
        let tom = account(&state, "tom").await;
        let space = state
            .store
            .create_space("Équipe", Some(olga.id))
            .await
            .unwrap();
        for who in [&mona, &tom] {
            state
                .store
                .add_member(space.id, who.id, Role::Member)
                .await
                .unwrap();
        }
        let line = |who: &AuthUser, level| Grant {
            subject: Subject::User(who.id),
            level,
        };
        let page = state
            .store
            .create_node(NewNode {
                space_id: space.id,
                parent_id: None,
                after: None,
                kind: NodeKind::Page,
                title: "P".to_string(),
                created_by: Some(olga.id),
                grants: vec![line(&olga, Level::Full), line(&mona, Level::Full)],
                doc: None,
            })
            .await
            .unwrap();
        // Le contrôle de la route de Mona, avant le corps : il passe.
        state
            .node_access(&mona, page.id, Action::Share)
            .await
            .unwrap();
        // Olga baisse Mona entre-temps.
        let lower_mona = AccessChange::User(mona.id, Some(Level::Read));
        write(&state, &olga, page.id, lower_mona).await.unwrap();

        let give_tom = AccessChange::User(tom.id, Some(Level::Full));
        let error = write(&state, &mona, page.id, give_tom).await.unwrap_err();
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        // Le refus de la transaction nomme le niveau comme celui du contrôle de la route, et
        // comme l'écran.
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body["message"],
            "niveau insuffisant sur cette page : il faut « Accès complet »"
        );
        let grants = state.store.node_grants(page.id).await.unwrap();
        assert!(
            !grants.iter().any(|g| g.subject == Subject::User(tom.id)),
            "{grants:?}"
        );

        let written = write(&state, &olga, page.id, give_tom).await.unwrap();
        assert!(written.changed);
        assert_eq!(written.space_id, space.id);
    }
}
