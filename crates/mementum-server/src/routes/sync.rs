//! `GET /sync/{id}?schema=N` : la WebSocket de synchronisation d'une page,
//! au protocole de y-websocket, servie par [`Hub::serve_connection_if`].
//!
//! **Qui entre.** Les refus viennent **avant** la poignée de main, toujours
//! dans cet ordre :
//! 1. `403 bad_origin` pour une en-tête `Origin` d'une autre page que celles
//!    de la boucle locale, du serveur ou de son adresse publique (Rulings R31
//!    et R51 : une WebSocket échappe à CORS) ;
//! 2. `401 unauthenticated` sans session ;
//! 3. `404 not_found` pour une page que la personne ne lit pas : inconnue,
//!    mal formée, d'un espace où elle n'a aucun rôle, ou hors de ses accès.
//!    La réponse est la même dans tous ces cas, et aucun acteur n'est lancé ;
//! 4. `400 invalid_request` sans demande de WebSocket.
//!
//! Le canal de l'espace (`routes/events.rs`) passe par les mêmes contrôles, dans le même ordre
//! ([`caller`], puis son propre `404`, puis [`websocket`]).
//!
//! **Ce qu'elle peut faire.** La connexion est servie au nom du compte de la
//! session. Elle écrit si le niveau de la personne permet de modifier la
//! page **et** que la page n'est pas à la corbeille, ce que demande aussi
//! `PUT /mmd` ; sinon elle est en lecture seule : elle reçoit tout, ses mises
//! à jour sont ignorées. Ce qu'elle écrit est attribué à son compte.
//!
//! **Ce qui se passe après la poignée de main** (Ruling R89). Le contrôle
//! d'avant ne sert qu'aux refus ci-dessus : rien de ce qu'il a lu n'entre
//! dans la connexion. Une fois la WebSocket ouverte, dans l'ordre :
//! 1. la page est chargée ([`Hub::load_page`]) : c'est ce qui peut durer,
//!    pour une page que personne n'a ouverte ;
//! 2. `spaces.rev` est lu ;
//! 3. les droits sont lus une seconde fois, et la connexion ne tient que de
//!    cette lecture : si elle écrit, et la révision qui date ses droits ;
//! 4. la connexion est inscrite dans l'acteur de la page ; puis, avant de lui
//!    avoir rien envoyé, le hub relit `spaces.rev` (Ruling R71) et demande à
//!    cette route de relire la session (Ruling R88).
//!
//! Une page supprimée ou devenue illisible entre-temps, une révision qui a
//! bougé, une session qui ne vaut plus : la connexion est fermée sans avoir
//! rien reçu ni rien écrit, et le client revient par la poignée de main.
//!
//! **Quand ses droits changent.** Ces droits valent pour toute la connexion :
//! le hub ne les relit pas. Ce sont les routes qui les changent qui ferment
//! les connexions concernées, après leur écriture, et le client revient par
//! cette route, qui relit tout :
//! - corbeille, restauration, changement de parent, partage : les connexions
//!   du sous-arbre (`rights_changed`) ; retrait d'un membre, changement de
//!   rôle : celles de tout l'espace ; suppression définitive : l'acteur de
//!   chaque page supprimée est arrêté (`Hub::forget`). Toute route qui
//!   retire un droit doit en faire autant ;
//! - déconnexion, changement de mot de passe, connexion ou inscription
//!   par-dessus une session : celles du compte dont la session se ferme
//!   (`Hub::kick_user`, Rulings R73 et R90).
//!
//! Ces fermetures ne trouvent pas une connexion en train de s'ouvrir : ce
//! sont les deux relectures de l'étape 4 qui la ferment. Pour les droits, la
//! révision est lue **avant** eux (étapes 2 et 3) : lue trop tôt, elle ne
//! coûte qu'une fermeture de trop ; lue après, elle rouvrirait la course. Le
//! raisonnement complet, pour les droits comme pour la session, est dans la
//! documentation de [`Hub::serve_connection_if`]. Parce que la page est
//! chargée d'abord, il ne passe qu'une ou deux millisecondes entre la lecture
//! de la révision et sa relecture : une écriture de l'espace sans rapport
//! avec cette page y tombe rarement.
//!
//! Restent deux fenêtres, de l'ordre de la milliseconde et sans aller-retour
//! réseau, acceptées (Ruling R74) : les messages d'une connexion déjà dans la
//! boîte de l'acteur quand la fermeture y arrive sont traités avec les
//! anciens droits ; `PUT /mmd` relit les droits une fois la page chargée,
//! puis envoie sa commande à l'acteur.
//!
//! `schema` est la version de schéma du client. Absente ou illisible, elle
//! vaut 0 : un client qui ne la donne pas est traité comme d'un autre schéma,
//! en lecture seule, quel que soit son niveau, et le serveur le journalise.
//!
//! [`Hub::serve_connection_if`]: mementum_sync::Hub::serve_connection_if
//! [`Hub::load_page`]: mementum_sync::Hub::load_page

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use mementum_acl::Action;
use mementum_sync::ws::WsBytes;
use mementum_sync::{ConnInfo, SyncError};
use uuid::Uuid;

use crate::access::permit;
use crate::auth::{self, AuthUser};
use crate::error::ApiError;
use crate::guard;
use crate::routes::pages::page_id;
use crate::types::ErrorCode;
use crate::{AppState, now_ms};

/// Délai laissé à une connexion refusée après la poignée de main pour
/// recevoir sa trame de fermeture.
const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// Qui demande une WebSocket (`/sync`, `/events`), une fois passés les deux premiers refus
/// ([`caller`]).
pub(crate) struct Caller {
    pub(crate) user: AuthUser,
    /// La session que l'extracteur vient de lire : c'est elle, et pas une autre du compte, que
    /// la connexion relira (Ruling R88 pour `/sync`).
    pub(crate) session: [u8; 32],
}

/// Les deux premiers refus d'avant la poignée de main, communs à `/sync` et à `/events` :
/// `403 bad_origin` pour une en-tête `Origin` d'un autre site (une WebSocket échappe à CORS, et
/// la couche de `guard.rs` ne regarde pas l'`Origin` d'un `GET`), puis `401` sans session.
pub(crate) fn caller(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
    user: Result<AuthUser, ApiError>,
) -> Result<Caller, ApiError> {
    if !guard::origin_allowed(
        guard::request_origin(headers),
        guard::request_host(headers, uri),
        state.public_origin.as_deref(),
    ) {
        return Err(guard::bad_origin());
    }
    let user = user?;
    let session = auth::session_hash(headers).ok_or_else(ApiError::unauthenticated)?;
    Ok(Caller { user, session })
}

/// Le dernier refus d'avant la poignée de main, commun à `/sync` et à `/events` : `400
/// invalid_request` (ou le statut de l'extracteur) sans demande de WebSocket.
pub(crate) fn websocket(
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<WebSocketUpgrade, ApiError> {
    upgrade.map_err(|e| {
        ApiError::new(
            e.status(),
            ErrorCode::InvalidRequest,
            "connexion WebSocket attendue (en-têtes Upgrade: websocket)",
        )
    })
}

pub(crate) async fn sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    user: Result<AuthUser, ApiError>,
    path: Result<Path<String>, PathRejection>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    let Caller { user, session } = caller(&state, &headers, &uri, user)?;
    let id = page_id(path)?;
    // Ce contrôle ne sert qu'aux refus : rien de ce qu'il lit n'entre dans la connexion
    // (Ruling R89).
    state.node_access(&user, id, Action::Read).await?;
    let upgrade = websocket(upgrade)?;
    let schema = query
        .ok()
        .and_then(|Query(q)| q.get("schema").and_then(|s| s.trim().parse::<u32>().ok()));
    let schema = match schema {
        Some(schema) => schema,
        None => {
            tracing::warn!(
                node_id = %id,
                "client sans version de schéma (paramètre schema absent ou illisible) : \
                 connexion en lecture seule"
            );
            0
        }
    };
    // Ruling R27 : axum refuse un message trop gros avant de le garder en
    // entier ; `serve_connection_if` contrôle la même borne.
    let max = state.hub.config().max_message_bytes;
    Ok(upgrade
        .max_message_size(max)
        .max_frame_size(max)
        .on_failed_upgrade(move |error| {
            tracing::warn!(node_id = %id, %error, "poignée de main WebSocket manquée");
        })
        .on_upgrade(move |socket| serve(state, user, session, id, schema, socket)))
}

/// La suite, une fois la WebSocket ouverte (les quatre étapes de l'en-tête) : la page est
/// chargée, les droits sont relus, puis la connexion est inscrite et servie jusqu'à sa fin.
async fn serve(
    state: AppState,
    user: AuthUser,
    session: [u8; 32],
    id: Uuid,
    schema: u32,
    mut socket: WebSocket,
) {
    let Some(conn) = admit(&state, &user, id, schema).await else {
        // Ni inscrite ni servie : une trame de fermeture, et le client revient par la poignée
        // de main, qui lui répondra ce qu'il en est.
        let _ = tokio::time::timeout(CLOSE_GRACE, socket.send(Message::Close(None))).await;
        return;
    };
    // Ruling R88 : attendu par le hub une fois la connexion inscrite. La session ne sort
    // pas d'ici, et sa relecture n'écrit rien (son échéance ne glisse pas).
    let store = state.store.clone();
    let session_still_valid = async move {
        match store.session_is_live(session, now_ms()).await {
            Ok(live) => live,
            Err(error) => {
                tracing::warn!(
                    node_id = %id,
                    %error,
                    "session illisible à l'inscription : connexion de synchronisation fermée"
                );
                false
            }
        }
    };
    let served = state
        .hub
        .clone()
        .serve_connection_if(id, conn, session_still_valid, WsBytes(socket))
        .await;
    match served {
        Ok(()) => {}
        // Supprimée entre l'admission et l'inscription : rien d'anormal, et rien à dire de
        // plus que dans `admit`. Le client revient, et la poignée de main lui répond `404`.
        Err(SyncError::NotFound) => {
            tracing::debug!(node_id = %id, "page supprimée pendant l'ouverture de sa connexion de synchronisation");
        }
        Err(error) => {
            tracing::warn!(node_id = %id, %error, "connexion de synchronisation refusée");
        }
    }
}

/// Charge la page, puis lit la révision de son espace, puis les droits de `user` : ce que la
/// connexion pourra faire, tiré de cette seule lecture (Ruling R89). `None` : la connexion
/// est à fermer (page supprimée ou devenue illisible depuis la poignée de main, panne du
/// store) ; c'est journalisé ici.
async fn admit(state: &AppState, user: &AuthUser, id: Uuid, schema: u32) -> Option<ConnInfo> {
    match state.hub.load_page(id).await {
        Ok(()) => {}
        // Supprimée depuis la poignée de main : rien à dire de plus.
        Err(SyncError::NotFound) => return None,
        Err(error) => {
            tracing::warn!(node_id = %id, %error, "page impossible à charger : connexion de synchronisation fermée");
            return None;
        }
    }
    // Ruling R71 : la révision de l'espace d'abord, les droits ensuite. Elle peut être plus
    // ancienne que celle sous laquelle les droits sont calculés, jamais plus récente.
    let rights = async {
        let space_id = match state.store.get_node(id).await? {
            Some(node) => node.space_id,
            None => return Err(ApiError::page_not_found()),
        };
        let rev = state
            .store
            .space_rev(space_id)
            .await?
            .ok_or_else(ApiError::page_not_found)?;
        let (node, access) = state.node_access(user, id, Action::Read).await?;
        // Un nœud ne change pas d'espace : sinon la révision lue ne daterait pas ces droits.
        if node.space_id != space_id {
            return Err(ApiError::page_not_found());
        }
        Ok((space_id, rev, access))
    };
    match rights.await {
        Ok((space_id, rev, access)) => Some(ConnInfo {
            client_schema: schema,
            // Le niveau permet de modifier, et la page n'est pas à la corbeille : la règle
            // de `PUT /mmd`.
            can_write: permit(&access, Action::Edit).is_ok(),
            actor: Some(user.id),
            // Lu avec la session : le nom que montrent la liste des présents et la présence de
            // la connexion, quoi que le client annonce (Ruling R136).
            name: Some(user.name.as_str().into()),
            space: Some((space_id, rev)),
        }),
        Err(refusal) => {
            // `404` : supprimée ou devenue illisible, rien à dire de plus. Le reste est une
            // panne, déjà journalisée par l'erreur elle-même.
            let status = refusal.into_response().status();
            if status != StatusCode::NOT_FOUND {
                tracing::warn!(node_id = %id, %status, "droits illisibles après la poignée de main : connexion de synchronisation fermée");
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    //! Ce que `tests/sync_rights.rs` ne voit pas de l'extérieur : les acteurs vivants du hub.
    //! Les requêtes passent par les vraies routes, la WebSocket par un port libre de la boucle
    //! locale.

    use std::net::SocketAddr;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use mementum_acl::{Grant, Level, Subject};
    use mementum_schema::SCHEMA_VERSION;
    use mementum_store::{NewNode, NodeKind};
    use mementum_sync::testkit::Client;
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::access::tests::{account, state};
    use crate::auth::{self, SESSION_COOKIE, SESSION_TTL_MS};
    use crate::{AppState, now_ms, router};

    /// Délai maximal d'une attente.
    const WAIT: Duration = Duration::from_secs(10);

    /// Sert les routes de `state` sur un port libre de la boucle locale.
    async fn listen(state: &AppState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = router(state.clone());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        addr
    }

    /// Une page de l'espace, avec ces accès posés sur elle.
    async fn page(
        state: &AppState,
        space: Uuid,
        parent: Option<Uuid>,
        grants: &[(Subject, Level)],
    ) -> Uuid {
        state
            .store
            .create_node(NewNode {
                space_id: space,
                parent_id: parent,
                after: None,
                kind: NodeKind::Page,
                title: "Page".to_string(),
                created_by: None,
                grants: grants
                    .iter()
                    .map(|&(subject, level)| Grant { subject, level })
                    .collect(),
                doc: None,
            })
            .await
            .unwrap()
            .id
    }

    /// Ouvre la WebSocket de synchro de la page avec ce cookie de session. `Err` : le refus
    /// de la poignée de main, où figure son statut HTTP.
    async fn open(addr: SocketAddr, page: Uuid, cookie: &str) -> Result<Client, String> {
        let url = format!("ws://{addr}/sync/{page}?schema={SCHEMA_VERSION}");
        Client::connect_with_headers(&url, &[("cookie", cookie)]).await
    }

    /// Comme [`open`], pour une connexion qui doit aboutir : attend la première synchro.
    async fn synced(addr: SocketAddr, page: Uuid, cookie: &str) -> Client {
        let client = open(addr, page, cookie)
            .await
            .unwrap_or_else(|refusal| panic!("connexion de synchro refusée : {refusal}"));
        assert!(
            client.wait_until(WAIT, |c| c.is_synced()).await,
            "le client n'a pas reçu l'état du serveur"
        );
        client
    }

    /// Une requête sans corps aux routes, avec ce cookie de session : le statut et le corps.
    async fn send(
        state: &AppState,
        method: Method,
        uri: &str,
        cookie: &str,
    ) -> (StatusCode, String) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "localhost")
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap();
        let response = router(state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    /// Le serveur ferme la connexion ([`WAIT`] au plus).
    async fn closed(client: &Client) -> bool {
        client.wait_until(WAIT, |c| c.is_closed()).await
    }

    /// Après `DELETE`, la page et ses sous-pages n'ont plus d'acteur vivant, leurs connexions
    /// sont fermées, et une reconnexion reçoit `404`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deleted_page_actor_is_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let alice = account(&state, "alice").await;
        let (token, hash) = auth::new_token();
        state
            .store
            .create_session(alice.id, hash, now_ms() + SESSION_TTL_MS)
            .await
            .unwrap();
        let cookie = format!("{SESSION_COOKIE}={token}");
        let space = state
            .store
            .create_space("Équipe", Some(alice.id))
            .await
            .unwrap()
            .id;
        let own = [(Subject::User(alice.id), Level::Full)];
        let parent = page(&state, space, None, &own).await;
        let child = page(&state, space, Some(parent), &[]).await;
        let kept = page(&state, space, None, &own).await;
        let addr = listen(&state).await;
        let live = async || {
            let mut pages = state.hub.live_pages().await;
            pages.sort();
            pages
        };
        let mut all = vec![parent, child, kept];
        all.sort();

        let on_parent = synced(addr, parent, &cookie).await;
        let on_child = synced(addr, child, &cookie).await;
        let on_kept = synced(addr, kept, &cookie).await;
        assert_eq!(live().await, all);

        // À la corbeille : les connexions sont fermées, les acteurs restent, et elles
        // reviennent.
        let trash = format!("/api/v1/pages/{parent}/trash");
        assert_eq!(
            send(&state, Method::POST, &trash, &cookie).await.0,
            StatusCode::OK
        );
        assert!(closed(&on_parent).await && closed(&on_child).await);
        let on_parent = synced(addr, parent, &cookie).await;
        let on_child = synced(addr, child, &cookie).await;
        assert_eq!(live().await, all);

        let delete = format!("/api/v1/pages/{parent}");
        assert_eq!(
            send(&state, Method::DELETE, &delete, &cookie).await,
            (StatusCode::NO_CONTENT, String::new())
        );
        // Au retour de la route : plus d'acteur pour la page ni pour sa sous-page.
        assert_eq!(live().await, [kept]);
        assert!(closed(&on_parent).await, "connexion de la page fermée");
        assert!(closed(&on_child).await, "connexion de la sous-page fermée");
        for gone in [parent, child] {
            let refusal = open(addr, gone, &cookie)
                .await
                .err()
                .expect("connexion de synchro acceptée sur une page supprimée");
            assert!(refusal.contains("404"), "{refusal}");
        }
        // Personne ne les a relancés, et la page d'à côté est toujours servie.
        assert_eq!(live().await, [kept]);
        let answered = on_kept.step2_count();
        on_kept.resync();
        assert!(
            on_kept
                .wait_until(WAIT, |c| c.step2_count() > answered)
                .await
        );
        assert!(!on_kept.is_closed());
    }
}
