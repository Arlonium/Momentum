//! Le canal de l'espace (`GET /events/{spaceId}`, Ruling R133) : une WebSocket qui ne porte
//! qu'un numéro, la version de l'espace (`spaces.lsn`). La route, et ses refus d'avant la
//! poignée de main, sont dans `routes/events.rs`.
//!
//! **Ce qui en sort.** Des trames texte `{"lsn":N}`, écrites par le serveur : une à l'ouverture,
//! la version courante, puis une chaque fois qu'elle avance ; et un ping WebSocket, vide, toutes
//! les [`Config::events_ping`] ([`EVENTS_PING`] par défaut). Rien d'autre : ni identifiant, ni
//! titre, ni compte, et la trame de fermeture n'a ni code ni raison. Le client relit alors
//! l'arbre par `GET /api/v1/spaces/{id}/tree`, déjà filtré par `access.rs` : aucun second
//! calcul de « qui voit quoi » n'est écrit pour ce canal. Ce que la personne en apprend, invitée
//! comprise : que ce que montre la barre latérale de l'espace a changé pour quelqu'un, et quand
//! (le coût accepté par R133). Le contenu d'une page n'avance pas la version.
//!
//! **Ce qui y entre.** Rien : toute trame de données reçue, texte ou binaire, ferme la
//! connexion. Les pings et les pongs du client passent (tungstenite répond seul aux pings). Un
//! message reçu ne dépasse pas [`MAX_INCOMING_BYTES`] : au-delà, axum le refuse avant de le
//! garder en entier, et la connexion se ferme.
//!
//! **Quand la version avance.** Une tâche par connexion, celle qu'axum lance à la poignée de
//! main, qui ne partage rien avec les autres que l'abonnement aux annonces du store
//! (`Store::changes`). Elle s'abonne **avant** de lire la version : une écriture validée après
//! l'abonnement lui est annoncée, après sa validation (R151) ; une écriture validée avant est
//! dans la version lue. À chaque annonce de son espace, et quand elle a perdu des annonces
//! (`Lagged`), elle relit la version, et n'écrit que si le numéro a monté : une annonce de trop
//! n'envoie rien, et plusieurs écritures rapprochées peuvent ne faire qu'une trame.
//!
//! **Droits.** Avant chaque trame de version, la première comprise, et à chaque ping, elle
//! revérifie que la session lue à la poignée de main vaut encore et que le compte a encore un
//! rôle dans l'espace (`AppState::role_at_least`) ; sinon elle ferme, sans rien envoyer. La
//! version est lue avant cette vérification : aucun numéro ne part qui ait été lu avant un
//! retrait. Retirer quelqu'un de l'espace avance la version : son canal est fermé dès l'annonce.
//! Une session fermée (déconnexion, changement de mot de passe) ou échue n'avance rien : le canal
//! est fermé au ping suivant, ou à la trame suivante si elle vient avant, et ne reçoit rien
//! entre-temps.
//!
//! **File.** Une trame attend dans une file que la précédente soit partie. Une connexion qui ne
//! lit plus la remplit : au-delà de [`QUEUE_FRAMES`] trames en attente, elle est fermée, elle
//! seule.
//!
//! [`Config::events_ping`]: crate::Config::events_ping

use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use mementum_acl::Role;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::{AppState, now_ms};

/// Intervalle par défaut des pings du canal de l'espace ([`Config::events_ping`]) : 20 s.
///
/// Un navigateur ne ferme pas de lui-même une WebSocket muette, mais un mandataire, oui : nginx
/// au bout de 60 s par défaut (`proxy_read_timeout`), d'autres plus tôt. Le canal d'un espace où
/// rien ne bouge serait coupé, puis rouvert, sans fin. 20 s, comme le message d'entretien de la
/// synchro (`mementum_sync::KEEPALIVE_AFTER`, Ruling R107), laisse de la marge sous ces délais.
/// C'est aussi le plus long qu'un canal survit, sans rien recevoir, à la fin de sa session.
///
/// [`Config::events_ping`]: crate::Config::events_ping
pub const EVENTS_PING: Duration = Duration::from_secs(20);

/// Trames qu'une connexion peut avoir en attente derrière celle qui part. Au-delà, elle est
/// fermée : un client qui ne lit plus ne fait pas grossir la mémoire du serveur.
pub(crate) const QUEUE_FRAMES: usize = 64;

/// Taille maximale d'un message reçu d'un client (`WebSocketUpgrade::max_message_size`). Le
/// canal n'en lit aucun : seules passent les trames de contrôle, de 125 octets au plus. Sans
/// cette borne, axum garderait jusqu'à 64 Mio avant de rendre un message, que la connexion
/// refuserait ensuite.
pub(crate) const MAX_INCOMING_BYTES: usize = 1024;

/// Délai laissé à une connexion pour recevoir sa trame de fermeture.
const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// Pourquoi une connexion du canal se termine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    /// Le client est parti, ou la connexion a échoué.
    Left,
    /// Le client a envoyé une trame de données.
    DataReceived,
    /// La session ne vaut plus, ou le compte n'a plus de rôle dans l'espace.
    Revoked,
    /// L'espace n'existe plus.
    SpaceGone,
    /// Plus de [`QUEUE_FRAMES`] trames en attente.
    QueueFull,
    /// Le store n'a pas répondu (journalisé là où c'est arrivé), ou ses annonces ont cessé.
    Unavailable,
}

impl End {
    fn as_str(self) -> &'static str {
        match self {
            End::Left => "client parti",
            End::DataReceived => "trame de données reçue du client",
            End::Revoked => "session fermée ou rôle retiré",
            End::SpaceGone => "espace supprimé",
            End::QueueFull => "file vers le client pleine",
            End::Unavailable => "store indisponible",
        }
    }
}

/// Ce dont le canal a besoin d'une WebSocket : envoyer un message, en recevoir un. Celle d'axum,
/// et celle des tests de ce module, qui ne lit jamais.
pub(crate) trait Socket: Send {
    /// Envoie `message` ; `false` si la connexion a échoué.
    fn send_frame(&mut self, message: Message) -> impl Future<Output = bool> + Send;
    /// Le message suivant du client ; `None` quand la connexion est fermée ou a échoué.
    fn next_frame(&mut self) -> impl Future<Output = Option<Message>> + Send;
}

impl Socket for WebSocket {
    async fn send_frame(&mut self, message: Message) -> bool {
        self.send(message).await.is_ok()
    }

    async fn next_frame(&mut self) -> Option<Message> {
        self.recv().await?.ok()
    }
}

/// Sert une connexion du canal de l'espace `space` au nom de `user`, ouverte avec la session
/// `session`, jusqu'à sa fin, puis lui envoie une trame de fermeture sans raison.
pub(crate) async fn serve<S: Socket>(
    state: AppState,
    user: AuthUser,
    session: [u8; 32],
    space: Uuid,
    mut socket: S,
) -> End {
    let mut conn = Conn {
        state: &state,
        user: &user,
        session,
        space,
        sent: None,
        queue: VecDeque::new(),
    };
    let Err(end) = run(&mut conn, &mut socket).await;
    tracing::debug!(space_id = %space, user_id = %user.id, reason = end.as_str(), "canal de l'espace fermé");
    let _ = tokio::time::timeout(CLOSE_GRACE, socket.send_frame(Message::Close(None))).await;
    end
}

/// La boucle d'une connexion. Pendant qu'une trame part, la connexion suit les annonces et les
/// pings, et empile ce qu'ils produisent ; quand rien ne part, elle lit aussi le client.
async fn run<S: Socket>(conn: &mut Conn<'_>, socket: &mut S) -> Result<Infallible, End> {
    // L'abonnement d'abord, la version ensuite (voir l'en-tête).
    let mut changes = conn.state.store.changes();
    conn.refresh().await?;
    let every = conn.state.events_ping;
    let ping = tokio::time::sleep_until(next_ping(every));
    tokio::pin!(ping);
    loop {
        if let Some(frame) = conn.queue.pop_front() {
            let sending = socket.send_frame(frame);
            tokio::pin!(sending);
            loop {
                tokio::select! {
                    biased;
                    sent = &mut sending => {
                        if !sent {
                            return Err(End::Left);
                        }
                        break;
                    }
                    change = changes.recv() => conn.on_change(change).await?,
                    () = &mut ping => {
                        ping.as_mut().reset(next_ping(every));
                        conn.on_ping().await?;
                    }
                }
            }
        } else {
            tokio::select! {
                incoming = socket.next_frame() => on_incoming(incoming)?,
                change = changes.recv() => conn.on_change(change).await?,
                () = &mut ping => {
                    ping.as_mut().reset(next_ping(every));
                    conn.on_ping().await?;
                }
            }
        }
    }
}

/// Ce que le client envoie : un ping ou un pong passe ; une trame de données, une trame de
/// fermeture ou une erreur termine la connexion.
fn on_incoming(incoming: Option<Message>) -> Result<(), End> {
    match incoming {
        Some(Message::Ping(_) | Message::Pong(_)) => Ok(()),
        Some(Message::Text(_) | Message::Binary(_)) => Err(End::DataReceived),
        Some(Message::Close(_)) | None => Err(End::Left),
    }
}

/// L'instant du prochain ping : dans `every`, une milliseconde au moins. Un délai qui ne tient
/// pas dans l'horloge vaut « jamais ».
fn next_ping(every: Duration) -> Instant {
    /// Une trentaine d'années : « jamais », sans dépasser l'horloge.
    const NEVER: Duration = Duration::from_secs(30 * 365 * 24 * 60 * 60);
    let now = Instant::now();
    now.checked_add(every.max(Duration::from_millis(1)))
        .unwrap_or(now + NEVER)
}

/// Une connexion du canal : à qui elle est, ce qu'elle a envoyé, ce qui attend de partir.
struct Conn<'a> {
    state: &'a AppState,
    user: &'a AuthUser,
    /// La session lue à la poignée de main : c'est elle, et pas une autre du compte, qui est
    /// relue.
    session: [u8; 32],
    space: Uuid,
    /// La dernière version mise en file ; `None` avant la trame d'ouverture.
    sent: Option<i64>,
    queue: VecDeque<Message>,
}

impl Conn<'_> {
    /// Une annonce du store : pour cet espace, ou des annonces perdues dont il pouvait être, la
    /// version est relue ; pour un autre espace, rien.
    async fn on_change(&mut self, change: Result<Uuid, RecvError>) -> Result<(), End> {
        match change {
            Ok(space) if space != self.space => Ok(()),
            Ok(_) | Err(RecvError::Lagged(_)) => self.refresh().await,
            Err(RecvError::Closed) => Err(End::Unavailable),
        }
    }

    /// Le ping : les droits d'abord, puis le ping en file.
    async fn on_ping(&mut self) -> Result<(), End> {
        self.check().await?;
        self.push(Message::Ping(Bytes::new()))
    }

    /// Relit la version ; si elle a monté depuis la dernière trame (ou s'il n'y en a pas eu),
    /// revérifie les droits, puis met sa trame en file.
    async fn refresh(&mut self) -> Result<(), End> {
        let lsn = match self.state.store.space_lsn(self.space).await {
            Ok(Some(lsn)) => lsn,
            Ok(None) => return Err(End::SpaceGone),
            Err(error) => {
                tracing::warn!(space_id = %self.space, %error, "version de l'espace illisible : canal de l'espace fermé");
                return Err(End::Unavailable);
            }
        };
        if self.sent.is_some_and(|sent| lsn <= sent) {
            return Ok(());
        }
        self.check().await?;
        self.push(Message::text(format!("{{\"lsn\":{lsn}}}")))?;
        self.sent = Some(lsn);
        Ok(())
    }

    /// La session vaut-elle encore, et le compte a-t-il encore un rôle dans l'espace ? Le rôle
    /// passe par `access.rs`, comme dans toute route.
    async fn check(&self) -> Result<(), End> {
        match self
            .state
            .store
            .session_is_live(self.session, now_ms())
            .await
        {
            Ok(true) => {}
            Ok(false) => return Err(End::Revoked),
            Err(error) => {
                tracing::warn!(space_id = %self.space, %error, "session illisible : canal de l'espace fermé");
                return Err(End::Unavailable);
            }
        }
        let refusal = match self
            .state
            .role_at_least(self.user, self.space, Role::Guest)
            .await
        {
            Ok(_) => return Ok(()),
            Err(refusal) => refusal,
        };
        // `404` : plus de rôle, ou plus d'espace. Le reste est une panne, déjà journalisée par
        // l'erreur elle-même.
        match refusal.into_response().status() {
            StatusCode::NOT_FOUND => Err(End::Revoked),
            _ => Err(End::Unavailable),
        }
    }

    /// Met `frame` en file, derrière celle qui part ; au-delà de [`QUEUE_FRAMES`], la connexion
    /// est à fermer.
    fn push(&mut self, frame: Message) -> Result<(), End> {
        if self.queue.len() >= QUEUE_FRAMES {
            return Err(End::QueueFull);
        }
        self.queue.push_back(frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Ce que `tests/events.rs` ne peut pas montrer avec une vraie socket : une connexion qui ne
    //! lit plus. Le système garde des centaines de kilo-octets pour elle avant que l'envoi
    //! n'attende, soit des dizaines de milliers de trames ; une socket de test qui n'accepte
    //! rien fait le même effet tout de suite.

    use std::sync::{Arc, Mutex, PoisonError};

    use axum::extract::ws::Message;
    use uuid::Uuid;

    use super::*;
    use crate::AppState;
    use crate::access::tests::{account, state};
    use crate::auth::{self, AuthUser, SESSION_TTL_MS};
    use crate::now_ms;

    /// Délai maximal d'une attente.
    const WAIT: Duration = Duration::from_secs(10);

    /// Une socket qui note ce qu'on lui donne. `accepts` : l'envoi aboutit aussitôt ; sinon il
    /// n'aboutit jamais, comme vers un client qui ne lit plus. Elle ne reçoit rien.
    struct TestSocket {
        accepts: bool,
        handed: Arc<Mutex<Vec<Message>>>,
    }

    impl Socket for TestSocket {
        async fn send_frame(&mut self, message: Message) -> bool {
            self.handed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(message);
            if !self.accepts {
                std::future::pending::<()>().await;
            }
            true
        }

        async fn next_frame(&mut self) -> Option<Message> {
            std::future::pending().await
        }
    }

    /// Un état de serveur dont le canal envoie un ping toutes les millisecondes, un compte avec
    /// sa session, et un espace dont il est propriétaire.
    async fn pinging(dir: &tempfile::TempDir) -> (AppState, AuthUser, [u8; 32], Uuid) {
        let mut state = state(dir).await;
        state.events_ping = Duration::from_millis(1);
        let alice = account(&state, "alice").await;
        let (_, session) = auth::new_token();
        state
            .store
            .create_session(alice.id, session, now_ms() + SESSION_TTL_MS)
            .await
            .unwrap();
        let space = state
            .store
            .create_space("Équipe", Some(alice.id))
            .await
            .unwrap()
            .id;
        (state, alice, session, space)
    }

    /// La trame de version qu'attend le client : `{"lsn":N}`.
    fn version_frame(lsn: i64) -> Message {
        Message::text(format!("{{\"lsn\":{lsn}}}"))
    }

    /// Une connexion dont l'envoi n'aboutit plus : la trame d'ouverture ne part pas, les pings
    /// s'empilent derrière elle, et au-delà de 64 en attente la connexion est fermée. Elle seule
    /// a reçu quelque chose : la trame d'ouverture, puis la trame de fermeture, sans raison.
    #[tokio::test]
    async fn a_connection_that_does_not_read_is_closed_past_its_queue() {
        assert_eq!(QUEUE_FRAMES, 64);
        let dir = tempfile::tempdir().unwrap();
        let (state, alice, session, space) = pinging(&dir).await;
        let lsn = state.store.space_lsn(space).await.unwrap().unwrap();
        let handed = Arc::new(Mutex::new(Vec::new()));
        let socket = TestSocket {
            accepts: false,
            handed: handed.clone(),
        };

        let end = tokio::time::timeout(WAIT, serve(state.clone(), alice, session, space, socket))
            .await
            .expect("la connexion est fermée");
        assert_eq!(end, End::QueueFull);
        let handed = handed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        assert_eq!(handed, [version_frame(lsn), Message::Close(None)]);
    }

    /// La même cadence de pings vers un client qui lit ne ferme rien : c'est la file qui compte,
    /// pas le nombre de trames.
    #[tokio::test]
    async fn a_connection_that_reads_is_kept_whatever_the_number_of_frames() {
        let dir = tempfile::tempdir().unwrap();
        let (state, alice, session, space) = pinging(&dir).await;
        let lsn = state.store.space_lsn(space).await.unwrap().unwrap();
        let handed = Arc::new(Mutex::new(Vec::new()));
        let socket = TestSocket {
            accepts: true,
            handed: handed.clone(),
        };
        let pings = || {
            handed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .filter(|message| matches!(message, Message::Ping(_)))
                .count()
        };

        let served = serve(state.clone(), alice, session, space, socket);
        let enough = async {
            while pings() < 4 * QUEUE_FRAMES {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        tokio::select! {
            end = served => panic!("connexion fermée : {end:?}"),
            () = tokio::time::sleep(WAIT) => panic!("{} pings en {WAIT:?}", pings()),
            () = enough => {}
        }
        let handed = handed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        assert_eq!(handed[0], version_frame(lsn));
        assert!(
            handed[1..]
                .iter()
                .all(|message| matches!(message, Message::Ping(payload) if payload.is_empty())),
            "que des pings vides après la trame d'ouverture"
        );
    }
}
