//! Le canal de l'espace (`GET /events/{spaceId}`, Ruling R133) : une WebSocket qui ne porte
//! qu'un numéro, la version de l'espace (`spaces.lsn`). La route, et ses refus d'avant la
//! poignée de main, sont dans `routes/events.rs`.
//!
//! **Ce qui en sort.** Des trames texte `{"lsn":N}`, écrites par le serveur : une à l'ouverture,
//! la version courante, puis une chaque fois qu'elle avance ; et un ping WebSocket, vide, toutes
//! les [`Config::events_ping`] ([`EVENTS_PING`] par défaut), précédé de la version courante
//! redite, même égale (Ruling R205) : le client ne voit pas les pings, et un canal à demi ouvert
//! laissait sa barre latérale périmée sans limite ; il ne relit rien pour un numéro égal (R156),
//! et s'en sert comme signe de vie. Rien d'autre : ni identifiant, ni
//! titre, ni compte, et la trame de fermeture n'a ni code ni raison. Le client relit alors
//! l'arbre par `GET /api/v1/spaces/{id}/tree`, déjà filtré par `access.rs` : aucun second
//! calcul de « qui voit quoi » n'est écrit pour ce canal. Ce que la personne en apprend, invitée
//! comprise : que ce que montre la barre latérale de l'espace a changé pour quelqu'un, quand, et
//! combien de fois. La version avance d'un pas par écriture de structure (au moins un par
//! transaction), pages privées comprises : l'écart entre deux numéros compte les écritures
//! faites entre-temps dans tout l'espace, et le numéro lui-même leur total depuis la création de
//! l'espace (le coût accepté par R133). Le contenu d'une page n'avance pas la version.
//!
//! **Ce qui y entre.** Aucune donnée : toute trame de données reçue, texte ou binaire, ferme la
//! connexion. Les pings du client passent (tungstenite y répond seul). Les pongs disent que le
//! client vit (R153) : une connexion qui n'a répondu à aucun des [`MAX_MISSED_PONGS`] derniers
//! pings est fermée au ping suivant ; un pong, sollicité ou non, remet le compte à zéro, et ne
//! fait rien d'autre. Un message reçu ne dépasse pas [`MAX_INCOMING_BYTES`] : au-delà, axum le
//! refuse avant de le garder en entier, et la connexion se ferme.
//!
//! **Quand la version avance.** Une tâche par connexion, celle qu'axum lance à la poignée de
//! main, qui ne partage rien avec les autres que l'abonnement aux annonces du store
//! (`Store::changes`). Elle s'abonne **avant** de lire la version : une écriture validée après
//! l'abonnement lui est annoncée, après sa validation (R151) ; une écriture validée avant est
//! dans la version lue. À chaque annonce de son espace, et quand elle a perdu des annonces
//! (`Lagged`), elle relit la version, et n'écrit que si le numéro a monté : une annonce de trop
//! n'envoie rien, et plusieurs écritures rapprochées peuvent ne faire qu'une trame.
//!
//! **Droits.** Avant de mettre en file chaque trame de version, la première comprise, et chaque
//! ping avec la version qu'il redit, elle revérifie que la session lue à la poignée de main vaut encore et que le compte a
//! encore un rôle dans l'espace (`AppState::role_at_least`) ; sinon elle ferme, et ce qui
//! attendait dans la file ne part pas. La version est lue avant cette vérification : aucun
//! numéro lu après un retrait n'est mis en file. Un numéro lu, et vérifié, juste avant un retrait
//! peut encore partir : c'est la trame déjà en file ou en cours d'envoi. Retirer quelqu'un de
//! l'espace avance la version : son canal est fermé dès l'annonce. Une session fermée
//! (déconnexion, changement de mot de passe) ou échue n'avance rien : le canal est fermé au ping
//! suivant, ou à la trame suivante si elle vient avant, et rien n'est plus mis en file
//! entre-temps.
//!
//! **Vie de la connexion.** Le critère est le pong (R153) : un pair qui ne répond plus aux pings,
//! disparu ou qui ne lit plus, reçoit encore [`MAX_MISSED_PONGS`] pings, puis il est fermé à
//! l'échéance du suivant, soit 80 s au plus après son dernier pong avec le défaut. Un navigateur
//! répond aux pings sans que la page fasse rien, onglet en arrière-plan compris. Les pongs ne
//! sont lus que quand rien ne part : pendant un envoi qui n'aboutit pas, les pings continuent
//! d'être comptés, et le pair est fermé de même. Un pong ne donne rien d'autre que de rester en
//! vie : les droits sont relus à chaque ping quoi qu'il arrive.
//!
//! **File.** Une trame attend dans une file que la précédente soit partie. Au-delà de
//! [`QUEUE_FRAMES`] trames en attente, la connexion est fermée, elle seule. C'est un second
//! garde-fou : avec des trames de quelques octets, l'envoi n'attend qu'une fois pleins les
//! tampons du système, des centaines de kilo-octets ; il faudrait plus de 64 écritures de
//! structure pendant un envoi bloqué, avant que les pings sans réponse ne ferment la connexion.
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

/// Pings sans réponse qu'une connexion peut accumuler (Ruling R153) : au ping suivant, elle est
/// fermée. Un pong reçu remet le compte à zéro.
///
/// Sans ce critère, un pair encore vivant en TCP mais qui ne lit plus garderait sa tâche et son
/// abonnement pour toujours : la file ne se remplit pas avec des trames de quelques octets. Trois,
/// et pas un : un pong peut tarder derrière un réseau lent ou un mandataire occupé.
pub(crate) const MAX_MISSED_PONGS: u32 = 3;

/// Trames qu'une connexion peut avoir en attente derrière celle qui part. Au-delà, elle est
/// fermée. Second garde-fou après les pongs ([`MAX_MISSED_PONGS`]) : la file ne grossit que si
/// l'envoi attend, donc une fois pleins les tampons du système.
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
    /// Aucun pong pour les [`MAX_MISSED_PONGS`] derniers pings.
    Unresponsive,
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
            End::Unresponsive => "pings restés sans réponse",
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
        unanswered: 0,
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
                incoming = socket.next_frame() => conn.on_incoming(incoming)?,
                change = changes.recv() => conn.on_change(change).await?,
                () = &mut ping => {
                    ping.as_mut().reset(next_ping(every));
                    conn.on_ping().await?;
                }
            }
        }
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
    /// Pings mis en file depuis le dernier pong reçu (Ruling R153).
    unanswered: u32,
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

    /// Ce que le client envoie : un pong remet à zéro le compte des pings sans réponse, et rien
    /// d'autre ; un ping passe ; une trame de données, une trame de fermeture ou une erreur
    /// termine la connexion.
    fn on_incoming(&mut self, incoming: Option<Message>) -> Result<(), End> {
        match incoming {
            Some(Message::Pong(_)) => {
                self.unanswered = 0;
                Ok(())
            }
            Some(Message::Ping(_)) => Ok(()),
            Some(Message::Text(_) | Message::Binary(_)) => Err(End::DataReceived),
            Some(Message::Close(_)) | None => Err(End::Left),
        }
    }

    /// Le ping : une connexion qui n'a répondu à aucun des derniers est fermée (Ruling R153) ;
    /// sinon la version relue, les droits, puis la version et le ping en file (Ruling R205). Le
    /// client ne voit pas les pings : la version redite, même égale, lui dit que le canal vit,
    /// et il ne relit rien pour un numéro égal (R156).
    async fn on_ping(&mut self) -> Result<(), End> {
        if self.unanswered >= MAX_MISSED_PONGS {
            return Err(End::Unresponsive);
        }
        let lsn = self.read_lsn().await?;
        self.check().await?;
        self.push_version(lsn)?;
        self.push(Message::Ping(Bytes::new()))?;
        self.unanswered += 1;
        Ok(())
    }

    /// Relit la version ; si elle a monté depuis la dernière trame (ou s'il n'y en a pas eu),
    /// revérifie les droits, puis met sa trame en file.
    async fn refresh(&mut self) -> Result<(), End> {
        let lsn = self.read_lsn().await?;
        if self.sent.is_some_and(|sent| lsn <= sent) {
            return Ok(());
        }
        self.check().await?;
        self.push_version(lsn)
    }

    /// La version de l'espace, lue dans la base.
    async fn read_lsn(&self) -> Result<i64, End> {
        match self.state.store.space_lsn(self.space).await {
            Ok(Some(lsn)) => Ok(lsn),
            Ok(None) => Err(End::SpaceGone),
            Err(error) => {
                tracing::warn!(space_id = %self.space, %error, "version de l'espace illisible : canal de l'espace fermé");
                Err(End::Unavailable)
            }
        }
    }

    /// Met en file la trame `{"lsn":N}`, les droits déjà revérifiés.
    fn push_version(&mut self, lsn: i64) -> Result<(), End> {
        self.push(Message::text(format!("{{\"lsn\":{lsn}}}")))?;
        self.sent = Some(self.sent.map_or(lsn, |sent| sent.max(lsn)));
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
    //! Ce que `tests/events.rs` ne peut pas montrer avec une vraie socket : une connexion dont
    //! l'envoi n'aboutit plus (le système garde des centaines de kilo-octets pour elle avant que
    //! l'envoi n'attende, soit des dizaines de milliers de trames), et la fin exacte d'une
    //! connexion (`End`). Une socket de test fait le même effet tout de suite.

    use std::sync::{Arc, Mutex, PoisonError};

    use axum::extract::ws::Message;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    use super::*;
    use crate::AppState;
    use crate::access::tests::{account, state};
    use crate::auth::{self, AuthUser, SESSION_TTL_MS};
    use crate::now_ms;

    /// Délai maximal d'une attente.
    const WAIT: Duration = Duration::from_secs(10);

    /// Le client au bout d'une socket de test.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Peer {
        /// L'envoi n'aboutit jamais : un client qui ne lit plus, dont la connexion est pleine.
        Stuck,
        /// L'envoi aboutit, et le client ne répond jamais aux pings.
        Silent,
        /// L'envoi aboutit, et le client répond à chaque ping par un pong, comme un navigateur.
        Answering,
    }

    /// Une socket qui note ce qu'on lui donne, et se comporte comme `peer`.
    struct TestSocket {
        peer: Peer,
        handed: Arc<Mutex<Vec<Message>>>,
        /// Les pongs à rendre, un par ping envoyé quand `peer` répond.
        pongs: (mpsc::UnboundedSender<()>, mpsc::UnboundedReceiver<()>),
    }

    impl TestSocket {
        fn new(peer: Peer, handed: &Arc<Mutex<Vec<Message>>>) -> TestSocket {
            TestSocket {
                peer,
                handed: handed.clone(),
                pongs: mpsc::unbounded_channel(),
            }
        }
    }

    impl Socket for TestSocket {
        async fn send_frame(&mut self, message: Message) -> bool {
            let ping = matches!(message, Message::Ping(_));
            self.handed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(message);
            match self.peer {
                Peer::Stuck => std::future::pending::<()>().await,
                Peer::Silent => {}
                Peer::Answering => {
                    if ping {
                        let _ = self.pongs.0.send(());
                    }
                }
            }
            true
        }

        async fn next_frame(&mut self) -> Option<Message> {
            // La socket garde l'émetteur : sans pong à rendre, l'attente ne finit jamais.
            self.pongs.1.recv().await;
            Some(Message::Pong(Bytes::new()))
        }
    }

    /// Ce qu'une socket de test a reçu.
    fn handed(handed: &Arc<Mutex<Vec<Message>>>) -> Vec<Message> {
        handed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Un état de serveur dont le canal envoie un ping tous les `every`, un compte avec sa
    /// session, et un espace dont il est propriétaire.
    async fn pinging(
        dir: &tempfile::TempDir,
        every: Duration,
    ) -> (AppState, AuthUser, [u8; 32], Uuid) {
        let mut state = state(dir).await;
        state.events_ping = every;
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

    /// R153 : un client à qui tout part mais qui ne répond à aucun ping est fermé après le
    /// troisième resté sans réponse, au tick suivant. Il a reçu la trame d'ouverture, trois
    /// pings précédés chacun de la version redite (Ruling R205), puis la trame de fermeture, sans
    /// raison.
    #[tokio::test]
    async fn a_connection_that_never_answers_pings_is_closed_after_the_third() {
        assert_eq!(MAX_MISSED_PONGS, 3);
        let dir = tempfile::tempdir().unwrap();
        let (state, alice, session, space) = pinging(&dir, Duration::from_millis(1)).await;
        let lsn = state.store.space_lsn(space).await.unwrap().unwrap();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let socket = TestSocket::new(Peer::Silent, &sent);

        let end = tokio::time::timeout(WAIT, serve(state.clone(), alice, session, space, socket))
            .await
            .expect("la connexion est fermée");
        assert_eq!(end, End::Unresponsive);
        let ping = Message::Ping(Bytes::new());
        assert_eq!(
            handed(&sent),
            [
                version_frame(lsn),
                version_frame(lsn),
                ping.clone(),
                version_frame(lsn),
                ping.clone(),
                version_frame(lsn),
                ping,
                Message::Close(None)
            ]
        );
    }

    /// La même cadence de pings vers un client qui y répond ne ferme rien, bien au-delà de trois
    /// pings et de la taille de la file : seuls comptent les pongs, et la file, qui se vide.
    /// L'intervalle laisse au pong le temps d'être lu avant le ping suivant.
    #[tokio::test]
    async fn a_connection_that_answers_pings_is_kept_whatever_the_number_of_frames() {
        let dir = tempfile::tempdir().unwrap();
        let (state, alice, session, space) = pinging(&dir, Duration::from_millis(20)).await;
        let lsn = state.store.space_lsn(space).await.unwrap().unwrap();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let socket = TestSocket::new(Peer::Answering, &sent);
        let pings = || {
            handed(&sent)
                .iter()
                .filter(|message| matches!(message, Message::Ping(_)))
                .count()
        };

        let served = serve(state.clone(), alice, session, space, socket);
        let enough = async {
            while pings() <= QUEUE_FRAMES {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        tokio::select! {
            end = served => panic!("connexion fermée : {end:?}"),
            () = tokio::time::sleep(4 * WAIT) => panic!("{} pings en {:?}", pings(), 4 * WAIT),
            () = enough => {}
        }
        let sent = handed(&sent);
        assert_eq!(sent[0], version_frame(lsn));
        assert!(
            sent[1..]
                .iter()
                .all(|message| *message == version_frame(lsn)
                    || matches!(message, Message::Ping(payload) if payload.is_empty())),
            "que des pings vides, et la version redite avec chacun (Ruling R205), après la trame \
             d'ouverture"
        );
    }

    /// Le second garde-fou : une connexion dont l'envoi n'aboutit plus, sans ping pendant le
    /// test, est fermée quand plus de 64 trames de version attendent derrière la trame
    /// d'ouverture, qui ne part jamais. Les écritures passent par le store de l'état, qui les
    /// annonce ; plusieurs peuvent ne faire qu'une trame, d'où une écriture de plus tant que la
    /// connexion vit. Elle n'a reçu que la trame d'ouverture, puis la trame de fermeture.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_connection_whose_sending_is_stuck_is_closed_past_its_queue() {
        assert_eq!(QUEUE_FRAMES, 64);
        let dir = tempfile::tempdir().unwrap();
        let (state, alice, session, space) = pinging(&dir, Duration::from_secs(3600)).await;
        let lsn = state.store.space_lsn(space).await.unwrap().unwrap();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let socket = TestSocket::new(Peer::Stuck, &sent);
        let served = tokio::spawn(serve(state.clone(), alice, session, space, socket));

        let deadline = std::time::Instant::now() + WAIT;
        while handed(&sent).is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "pas de trame d'ouverture"
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let mut writes = 0;
        while !handed(&sent).contains(&Message::Close(None)) {
            assert!(
                std::time::Instant::now() < deadline,
                "toujours ouverte après {writes} écritures"
            );
            state
                .store
                .rename_space(space, &format!("Équipe {writes}"))
                .await
                .unwrap();
            writes += 1;
        }
        let end = tokio::time::timeout(WAIT, served)
            .await
            .expect("la connexion est fermée")
            .unwrap();
        assert_eq!(end, End::QueueFull);
        assert!(writes > QUEUE_FRAMES, "{writes} écritures");
        assert_eq!(handed(&sent), [version_frame(lsn), Message::Close(None)]);
    }
}
