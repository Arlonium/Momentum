//! `GET /sync/{id}?schema=N` : la WebSocket de synchronisation d'une page,
//! au protocole de y-websocket, servie par [`Hub::serve_connection`].
//!
//! **Qui entre.** Tout est vérifié **avant** la poignée de main, et les refus
//! viennent toujours dans cet ordre :
//! 1. `403 bad_origin` pour une en-tête `Origin` d'une autre page que celles
//!    de la boucle locale, du serveur ou de son adresse publique (Rulings R31
//!    et R51 : une WebSocket échappe à CORS) ;
//! 2. `401 unauthenticated` sans session ;
//! 3. `404 not_found` pour une page que la personne ne lit pas : inconnue,
//!    mal formée, d'un espace où elle n'a aucun rôle, ou hors de ses accès.
//!    La réponse est la même dans tous ces cas, et aucun acteur n'est lancé.
//!
//! **Ce qu'elle peut faire.** La connexion est servie au nom du compte de la
//! session. Elle écrit si le niveau de la personne permet de modifier la
//! page **et** que la page n'est pas à la corbeille, ce que demande aussi
//! `PUT /mmd` ; sinon elle est en lecture seule : elle reçoit tout, ses mises
//! à jour sont ignorées. Ce qu'elle écrit est attribué à son compte.
//!
//! **Quand ses droits changent.** Ces droits valent pour toute la connexion :
//! le hub ne les relit pas. Ce sont les routes qui les changent qui ferment
//! les connexions concernées, après leur écriture, et le client revient par
//! cette route, qui relit tout :
//! - corbeille, restauration, changement de parent : les connexions du
//!   sous-arbre (`rights_changed`) ; retrait d'un membre, changement de
//!   rôle : celles de tout l'espace ; suppression définitive : l'acteur de
//!   chaque page supprimée est arrêté (`Hub::forget`). Toute route qui
//!   retire un droit doit en faire autant ;
//! - déconnexion, changement de mot de passe : celles du compte
//!   (`Hub::kick_user`, Ruling R73).
//!
//! Un changement de droits peut tomber entre la lecture des droits, ici, et
//! l'inscription de la connexion dans l'acteur de la page : la fermeture ne la
//! trouverait pas. La route lit donc `spaces.rev` **avant** les droits, et le
//! hub le relit une fois la connexion inscrite : s'il a bougé, il la ferme
//! avant de lui avoir rien envoyé (Ruling R71 ; le raisonnement complet est
//! dans la documentation de [`Hub::serve_connection`]). Une révision lue trop
//! tôt ne coûte qu'une fermeture de trop ; lue après les droits, elle
//! rouvrirait la course.
//!
//! Restent deux fenêtres de l'ordre de la milliseconde, acceptées (Ruling
//! R74) : les messages d'une connexion déjà dans la boîte de l'acteur quand
//! la fermeture y arrive sont traités avec les anciens droits ; `PUT /mmd`
//! contrôle les droits, puis envoie sa commande à l'acteur.
//!
//! `schema` est la version de schéma du client. Absente ou illisible, elle
//! vaut 0 : un client qui ne la donne pas est traité comme d'un autre schéma,
//! en lecture seule, quel que soit son niveau, et le serveur le journalise.
//!
//! [`Hub::serve_connection`]: mementum_sync::Hub::serve_connection

use std::collections::HashMap;

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use mementum_acl::Action;
use mementum_sync::ConnInfo;
use mementum_sync::ws::WsBytes;

use crate::AppState;
use crate::access::permit;
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::guard;
use crate::routes::pages::page_id;
use crate::types::ErrorCode;

pub(crate) async fn sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    user: Result<AuthUser, ApiError>,
    path: Result<Path<String>, PathRejection>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    if !guard::origin_allowed(
        guard::request_origin(&headers),
        guard::request_host(&headers, &uri),
        state.public_origin.as_deref(),
    ) {
        return Err(guard::bad_origin());
    }
    let user = user?;
    let id = page_id(path)?;
    // Ruling R71 : la révision de l'espace d'abord, les droits ensuite. Elle peut être plus
    // ancienne que celle sous laquelle les droits sont calculés, jamais plus récente.
    let space_id = match state.store.get_node(id).await? {
        Some(node) => node.space_id,
        None => return Err(ApiError::page_not_found()),
    };
    let rev = state
        .store
        .space_rev(space_id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    let (node, access) = state.node_access(&user, id, Action::Read).await?;
    // Un nœud ne change pas d'espace : sinon la révision lue ne daterait pas ces droits.
    if node.space_id != space_id {
        return Err(ApiError::page_not_found());
    }
    // Le niveau permet de modifier, et la page n'est pas à la corbeille : la règle de
    // `PUT /mmd`.
    let can_write = permit(&access, Action::Edit).is_ok();
    let upgrade = upgrade.map_err(|e| {
        ApiError::new(
            e.status(),
            ErrorCode::InvalidRequest,
            "connexion WebSocket attendue (en-têtes Upgrade: websocket)",
        )
    })?;
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
    // entier ; `serve_connection` contrôle la même borne.
    let max = state.hub.config().max_message_bytes;
    let hub = state.hub.clone();
    let conn = ConnInfo {
        client_schema: schema,
        can_write,
        actor: Some(user.id),
        space: Some((space_id, rev)),
    };
    Ok(upgrade
        .max_message_size(max)
        .max_frame_size(max)
        .on_failed_upgrade(move |error| {
            tracing::warn!(node_id = %id, %error, "poignée de main WebSocket manquée");
        })
        .on_upgrade(move |socket| async move {
            if let Err(error) = hub.serve_connection(id, conn, WsBytes(socket)).await {
                tracing::warn!(node_id = %id, %error, "connexion de synchronisation refusée");
            }
        }))
}

#[cfg(test)]
mod tests {
    //! Ce que `tests/sync_rights.rs` ne voit pas de l'extérieur : `rights_changed`, appelé
    //! comme le fera la route de partage, et les acteurs vivants du hub. Les requêtes passent
    //! par les vraies routes, la WebSocket par un port libre de la boucle locale.

    use std::net::SocketAddr;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use mementum_acl::{Grant, Level, Role, Subject};
    use mementum_schema::SCHEMA_VERSION;
    use mementum_store::{NewNode, NodeKind};
    use mementum_sync::testkit::Client;
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::access::tests::{account, state};
    use crate::auth::{self, AuthUser, SESSION_COOKIE, SESSION_TTL_MS};
    use crate::{AppState, now_ms, router};

    /// Délai maximal d'une attente de convergence.
    const WAIT: Duration = Duration::from_secs(10);

    /// Délai dans lequel une connexion doit être fermée quand ses droits changent (Review
    /// Focus 1).
    const CLOSES_WITHIN: Duration = Duration::from_secs(2);

    /// Un compte et l'en-tête `Cookie` d'une session ouverte pour lui.
    struct Person {
        user: AuthUser,
        cookie: String,
    }

    async fn person(state: &AppState, name: &str) -> Person {
        let user = account(state, name).await;
        let (token, hash) = auth::new_token();
        state
            .store
            .create_session(user.id, hash, now_ms() + SESSION_TTL_MS)
            .await
            .unwrap();
        Person {
            user,
            cookie: format!("{SESSION_COOKIE}={token}"),
        }
    }

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

    fn sync_url(addr: SocketAddr, page: Uuid) -> String {
        format!("ws://{addr}/sync/{page}?schema={SCHEMA_VERSION}")
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

    /// Ouvre la WebSocket de synchro avec la session de `who`. `Err` : le refus de la poignée
    /// de main, où figure son statut HTTP.
    async fn open(url: &str, who: &Person) -> Result<Client, String> {
        Client::connect_with_headers(url, &[("cookie", &who.cookie)]).await
    }

    /// Comme [`open`], pour une connexion qui doit aboutir : attend la première synchro.
    async fn synced(url: &str, who: &Person) -> Client {
        let client = open(url, who)
            .await
            .unwrap_or_else(|refusal| panic!("connexion de synchro refusée : {refusal}"));
        assert!(
            client.wait_until(WAIT, |c| c.is_synced()).await,
            "le client n'a pas reçu l'état du serveur"
        );
        client
    }

    /// Le refus d'une connexion de synchro demandée par `who`, où figure son statut HTTP.
    async fn refusal(url: &str, who: &Person) -> String {
        open(url, who)
            .await
            .err()
            .unwrap_or_else(|| panic!("connexion de synchro acceptée : {url}"))
    }

    /// Une requête sans corps aux routes, avec la session de `who` : le statut et le corps.
    async fn send(
        state: &AppState,
        method: Method,
        uri: &str,
        who: &Person,
    ) -> (StatusCode, String) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "localhost")
            .header(header::COOKIE, &who.cookie)
            .body(Body::empty())
            .unwrap();
        let response = router(state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    /// Le texte de chaque bloc de premier niveau du document du client, une ligne par bloc.
    fn text(client: &Client) -> String {
        client
            .blocks()
            .iter()
            .map(mementum_md::block_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Tape `typed` à la fin du premier bloc, puis attend que le serveur ait traité tout ce
    /// que la connexion a envoyé (il répond ensuite à la resynchronisation).
    async fn types(client: &Client, typed: &str) {
        let block = client.blocks()[0].id.clone();
        let start = client.step2_count();
        client.type_text(&block, typed);
        client.resync();
        assert!(
            client.wait_until(WAIT, |c| c.step2_count() > start).await,
            "le serveur n'a pas traité « {typed} »"
        );
    }

    /// Le serveur ferme la connexion, en moins de deux secondes.
    async fn closed(client: &Client) -> bool {
        client.wait_until(CLOSES_WITHIN, |c| c.is_closed()).await
    }

    /// Review Focus 1 : B écrit sur une page partagée avec lui, puis son accès est retiré. Sa
    /// connexion se ferme en moins de deux secondes, sa reconnexion reçoit `404`, et ce qu'il
    /// envoie ensuite n'est pas dans le MMD.
    ///
    /// L'accès est retiré comme le fera la route de partage : l'écriture du store, puis
    /// `rights_changed` sur le sous-arbre de la page.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn revoked_access_closes_live_connection() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let alice = person(&state, "alice").await;
        let bob = person(&state, "bob").await;
        let space = state
            .store
            .create_space("Équipe", Some(alice.user.id))
            .await
            .unwrap()
            .id;
        state
            .store
            .add_member(space, bob.user.id, Role::Guest)
            .await
            .unwrap();
        // Une page d'Alice, partagée avec Bob en modification ; sa sous-page en hérite.
        let grants = [
            (Subject::User(alice.user.id), Level::Full),
            (Subject::User(bob.user.id), Level::Edit),
        ];
        let shared = page(&state, space, None, &grants).await;
        let child = page(&state, space, Some(shared), &[]).await;
        let addr = listen(&state).await;
        let (shared_url, child_url) = (sync_url(addr, shared), sync_url(addr, child));
        let a = synced(&shared_url, &alice).await;
        let b = synced(&shared_url, &bob).await;
        let b_child = synced(&child_url, &bob).await;
        types(&b, "de Bob").await;
        assert!(a.wait_until(WAIT, |c| text(c) == "de Bob").await);

        state
            .store
            .revoke_user_grant(shared, bob.user.id)
            .await
            .unwrap();
        state.subtree_rights_changed(space, shared).await;
        // Au retour, plus rien de ce qu'il envoie n'est traité.
        let block = b.blocks()[0].id.clone();
        b.type_text(&block, " en trop");
        assert!(closed(&b).await, "sa connexion sur la page est fermée");
        assert!(closed(&b_child).await, "celle de la sous-page aussi");

        // Il ne revient pas : la réponse d'une page qui n'existe pas.
        for url in [&shared_url, &child_url, &sync_url(addr, Uuid::now_v7())] {
            let refusal = refusal(url, &bob).await;
            assert!(refusal.contains("404"), "{refusal}");
        }
        let mmd = format!("/api/v1/pages/{shared}/mmd");
        assert_eq!(
            send(&state, Method::GET, &mmd, &alice).await,
            (StatusCode::OK, "de Bob\n".to_string())
        );

        // Alice, fermée avec toute la page, revient et écrit.
        assert!(closed(&a).await);
        let a = synced(&shared_url, &alice).await;
        assert_eq!(text(&a), "de Bob");
        types(&a, ", puis d'Alice").await;
        assert_eq!(
            send(&state, Method::GET, &mmd, &alice).await,
            (StatusCode::OK, "de Bob, puis d'Alice\n".to_string())
        );
    }

    /// Après `DELETE`, la page et ses sous-pages n'ont plus d'acteur vivant, leurs connexions
    /// sont fermées, et une reconnexion reçoit `404`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deleted_page_actor_is_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let alice = person(&state, "alice").await;
        let space = state
            .store
            .create_space("Équipe", Some(alice.user.id))
            .await
            .unwrap()
            .id;
        let own = [(Subject::User(alice.user.id), Level::Full)];
        let parent = page(&state, space, None, &own).await;
        let child = page(&state, space, Some(parent), &[]).await;
        let kept = page(&state, space, None, &own).await;
        let addr = listen(&state).await;
        let (parent_url, child_url) = (sync_url(addr, parent), sync_url(addr, child));
        let live = async || {
            let mut pages = state.hub.live_pages().await;
            pages.sort();
            pages
        };
        let mut all = vec![parent, child, kept];
        all.sort();

        let on_parent = synced(&parent_url, &alice).await;
        let on_child = synced(&child_url, &alice).await;
        let on_kept = synced(&sync_url(addr, kept), &alice).await;
        assert_eq!(live().await, all);

        // À la corbeille : les connexions sont fermées, les acteurs restent, et elles
        // reviennent.
        let trash = format!("/api/v1/pages/{parent}/trash");
        assert_eq!(
            send(&state, Method::POST, &trash, &alice).await.0,
            StatusCode::OK
        );
        assert!(closed(&on_parent).await && closed(&on_child).await);
        let on_parent = synced(&parent_url, &alice).await;
        let on_child = synced(&child_url, &alice).await;
        assert_eq!(live().await, all);

        let delete = format!("/api/v1/pages/{parent}");
        assert_eq!(
            send(&state, Method::DELETE, &delete, &alice).await,
            (StatusCode::NO_CONTENT, String::new())
        );
        // Au retour de la route : plus d'acteur pour la page ni pour sa sous-page.
        assert_eq!(live().await, [kept]);
        assert!(closed(&on_parent).await, "connexion de la page fermée");
        assert!(closed(&on_child).await, "connexion de la sous-page fermée");
        for url in [&parent_url, &child_url] {
            let refusal = refusal(url, &alice).await;
            assert!(refusal.contains("404"), "{refusal}");
        }
        // Personne ne les a relancés, et la page d'à côté n'a pas été touchée.
        assert_eq!(live().await, [kept]);
        types(&on_kept, "toujours là").await;
        assert!(!on_kept.is_closed());
    }
}
