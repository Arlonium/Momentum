//! Le canal de l'espace, `GET /events/{spaceId}` (Ruling R133) : une WebSocket qui ne porte que
//! la version de l'espace, en trames texte `{"lsn":N}`, une à l'ouverture puis une chaque fois
//! qu'elle avance, et un ping toutes les 20 s. Rien d'autre n'en sort, et rien n'y entre : une
//! trame de données reçue du client la ferme. Le serveur la ferme aussi quand la personne n'a
//! plus de rôle dans l'espace ou que sa session ne vaut plus.
//!
//! Le client de ces tests est celui de tungstenite, bloquant, sur un fil à lui ([`Channel`]) :
//! il note dans l'ordre tout ce qu'il reçoit (trames de texte, pings, fermeture), et répond aux
//! pings comme un navigateur (tungstenite renvoie le pong au `read` suivant). Pour un client qui
//! ne répond pas aux pings, la poignée de main et les trames sont écrites à la main
//! ([`Channel::connect_silent`]).
//!
//! Les écritures que le canal doit annoncer passent par les routes : le store de l'équipe
//! (`Team::store`, `Team::page`) est une autre instance, sur la même base, qui annonce sur son
//! propre canal. Il ne sert qu'à préparer l'espace avant d'ouvrir les canaux, et à relire la
//! version.
//!
//! Un test de la même série est dans `src/events.rs` : la file d'une connexion qui ne lit plus,
//! qu'une vraie socket ne remplit pas en un temps raisonnable.

mod common;

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::sync::{members, open_app, user};
use common::{Browser, PASSWORD, Reply, Team, TestApp, WAIT, handshake, outcome, tree_nodes};
use mementum_acl::Level;
use mementum_schema::SCHEMA_VERSION;
use mementum_server::{Config, EVENTS_PING, SignupMode};
use serde_json::json;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::HandshakeError;
use tokio_tungstenite::tungstenite::{self, Message, WebSocket};
use uuid::Uuid;

/// Ce que le client du canal a reçu.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    Text(String),
    Binary(Vec<u8>),
    Ping,
    /// Le serveur a fermé la connexion : trame de fermeture, ou connexion coupée.
    Closed,
}

/// Un client du canal de l'espace, sur un fil à lui, qui note ce qu'il reçoit et envoie ce
/// qu'on lui donne. Le fil s'arrête quand la connexion se ferme ou que la valeur tombe.
struct Channel {
    seen: Arc<Mutex<Vec<(Instant, Seen)>>>,
    outgoing: mpsc::Sender<Message>,
    stop: Arc<AtomicBool>,
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Channel {
    /// Ouvre la WebSocket `url` avec ce cookie de session. `Err` : le statut HTTP du refus de la
    /// poignée de main.
    fn connect(addr: SocketAddr, url: &str, cookie: Option<&str>) -> Result<Channel, u16> {
        let mut request = url.into_client_request().unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert(header::COOKIE, cookie.parse().unwrap());
        }
        let stream = TcpStream::connect(addr).unwrap();
        let mut socket = match tungstenite::client::client(request, stream) {
            Ok((socket, _)) => socket,
            Err(HandshakeError::Failure(tungstenite::Error::Http(response))) => {
                return Err(response.status().as_u16());
            }
            Err(error) => panic!("poignée de main du canal : {error}"),
        };
        // Non bloquante une fois ouverte : le fil lit sans attendre, et envoie entre deux
        // lectures.
        socket.get_mut().set_nonblocking(true).unwrap();
        Ok(Channel::start(move |seen, to_send, stop| {
            listen(socket, seen, to_send, stop);
        }))
    }

    /// Comme [`Channel::connect`], pour un client qui lit tout et ne répond jamais aux pings :
    /// la poignée de main et les trames passent à la main sur la socket, sans tungstenite, qui
    /// répond de lui-même à chaque ping lu. Il ne sait envoyer qu'un pong vide
    /// ([`Channel::send`] d'un `Message::Pong`). La poignée de main doit aboutir.
    fn connect_silent(addr: SocketAddr, path: &str, cookie: Option<&str>) -> Channel {
        let mut stream = TcpStream::connect(addr).unwrap();
        let mut request = format!(
            "GET {path} HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n"
        );
        if let Some(cookie) = cookie {
            request.push_str(&format!("Cookie: {cookie}\r\n"));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        // La réponse jusqu'à la ligne vide ; ce qui la suit est déjà des trames.
        let mut received = Vec::new();
        let head_end = loop {
            let mut chunk = [0; 1024];
            let read = stream.read(&mut chunk).unwrap();
            assert!(read > 0, "connexion fermée pendant la poignée de main");
            received.extend_from_slice(&chunk[..read]);
            if let Some(at) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
        };
        let head = String::from_utf8_lossy(&received[..head_end]).into_owned();
        assert!(
            head.starts_with("HTTP/1.1 101"),
            "poignée de main refusée : {head}"
        );
        let pending = received.split_off(head_end);
        stream.set_nonblocking(true).unwrap();
        Channel::start(move |seen, to_send, stop| {
            listen_silent(stream, pending, seen, to_send, stop);
        })
    }

    /// Lance le fil d'un client.
    fn start(
        listen: impl FnOnce(&Mutex<Vec<(Instant, Seen)>>, &mpsc::Receiver<Message>, &AtomicBool)
        + Send
        + 'static,
    ) -> Channel {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (outgoing, to_send) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        std::thread::spawn({
            let (seen, stop) = (seen.clone(), stop.clone());
            move || listen(&seen, &to_send, &stop)
        });
        Channel {
            seen,
            outgoing,
            stop,
        }
    }

    fn seen(&self) -> Vec<Seen> {
        let seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.iter().map(|(_, what)| what.clone()).collect()
    }

    /// Les trames texte reçues, dans l'ordre.
    fn texts(&self) -> Vec<String> {
        self.seen()
            .into_iter()
            .filter_map(|what| match what {
                Seen::Text(text) => Some(text),
                _ => None,
            })
            .collect()
    }

    /// Les versions reçues, dans l'ordre ; chaque trame est vérifiée de la forme exacte
    /// `{"lsn":N}` ([`version`]).
    fn versions(&self) -> Vec<i64> {
        self.texts().iter().map(|frame| version(frame)).collect()
    }

    /// Quand chaque ping est arrivé.
    fn ping_times(&self) -> Vec<Instant> {
        let seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.iter()
            .filter(|(_, what)| *what == Seen::Ping)
            .map(|(at, _)| *at)
            .collect()
    }

    fn is_closed(&self) -> bool {
        self.seen().contains(&Seen::Closed)
    }

    /// Quand la connexion s'est fermée, si elle l'est.
    fn closed_at(&self) -> Option<Instant> {
        let seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.iter()
            .find(|(_, what)| *what == Seen::Closed)
            .map(|(at, _)| *at)
    }

    /// Envoie ce message au serveur.
    fn send(&self, message: Message) {
        self.outgoing.send(message).unwrap();
    }

    /// Attend que `done` soit vrai ([`WAIT`] au plus).
    async fn wait_until(&self, done: impl Fn(&Channel) -> bool) -> bool {
        let deadline = Instant::now() + WAIT;
        loop {
            if done(self) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// Le fil d'un [`Channel`].
fn listen(
    mut socket: WebSocket<TcpStream>,
    seen: &Mutex<Vec<(Instant, Seen)>>,
    to_send: &mpsc::Receiver<Message>,
    stop: &AtomicBool,
) {
    let note = |what| {
        seen.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((Instant::now(), what));
    };
    let would_block = |error: &tungstenite::Error| matches!(error, tungstenite::Error::Io(e) if e.kind() == ErrorKind::WouldBlock);
    while !stop.load(Ordering::Relaxed) {
        while let Ok(message) = to_send.try_recv() {
            if let Err(error) = socket.send(message)
                && !would_block(&error)
            {
                note(Seen::Closed);
                return;
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => note(Seen::Text(text.as_str().to_owned())),
            Ok(Message::Binary(bytes)) => note(Seen::Binary(bytes.to_vec())),
            Ok(Message::Ping(_)) => note(Seen::Ping),
            Ok(Message::Pong(_) | Message::Frame(_)) => {}
            Ok(Message::Close(_)) => {
                note(Seen::Closed);
                return;
            }
            Err(error) if would_block(&error) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                note(Seen::Closed);
                return;
            }
        }
    }
}

/// Le fil d'un client silencieux ([`Channel::connect_silent`]) : il lit les trames du serveur
/// à la main, et n'envoie que les pongs qu'on lui donne. `pending` : ce qui suivait la réponse à
/// la poignée de main.
fn listen_silent(
    mut stream: TcpStream,
    mut pending: Vec<u8>,
    seen: &Mutex<Vec<(Instant, Seen)>>,
    to_send: &mpsc::Receiver<Message>,
    stop: &AtomicBool,
) {
    let note = |what| {
        seen.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((Instant::now(), what));
    };
    while !stop.load(Ordering::Relaxed) {
        while let Ok(message) = to_send.try_recv() {
            assert!(
                matches!(message, Message::Pong(_)),
                "un client silencieux n'envoie que des pongs : {message:?}"
            );
            // Un pong vide, masqué comme toute trame d'un client.
            if stream
                .write_all(&[0x8A, 0x80, 0x12, 0x34, 0x56, 0x78])
                .is_err()
            {
                note(Seen::Closed);
                return;
            }
        }
        while let Some((opcode, payload, length)) = raw_frame(&pending) {
            pending.drain(..length);
            match opcode {
                0x1 => note(Seen::Text(String::from_utf8(payload).unwrap())),
                0x2 => note(Seen::Binary(payload)),
                0x9 => note(Seen::Ping),
                0x8 => {
                    note(Seen::Closed);
                    return;
                }
                _ => {}
            }
        }
        let mut chunk = [0; 1024];
        match stream.read(&mut chunk) {
            Ok(0) => {
                note(Seen::Closed);
                return;
            }
            Ok(read) => pending.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => {
                note(Seen::Closed);
                return;
            }
        }
    }
}

/// La première trame complète de `bytes`, si elle y est : son code d'opération, ses données et
/// sa longueur totale. Les trames du serveur ne sont pas masquées.
fn raw_frame(bytes: &[u8]) -> Option<(u8, Vec<u8>, usize)> {
    let [first, second, ..] = *bytes else {
        return None;
    };
    let (length, header) = match second & 0x7F {
        126 => (
            usize::from(u16::from_be_bytes([*bytes.get(2)?, *bytes.get(3)?])),
            4,
        ),
        127 => {
            let mut length = [0; 8];
            length.copy_from_slice(bytes.get(2..10)?);
            (usize::try_from(u64::from_be_bytes(length)).unwrap(), 10)
        }
        short => (usize::from(short), 2),
    };
    let payload = bytes.get(header..header + length)?;
    Some((first & 0x0F, payload.to_vec(), header + length))
}

/// La version que porte une trame, qui doit être exactement `{"lsn":N}` : ni espace, ni autre
/// champ, ni zéro en tête.
fn version(frame: &str) -> i64 {
    let lsn = frame
        .strip_prefix("{\"lsn\":")
        .and_then(|rest| rest.strip_suffix('}'))
        .and_then(|n| n.parse::<i64>().ok())
        .unwrap_or_else(|| panic!("trame hors de la forme {{\"lsn\":N}} : {frame}"));
    assert_eq!(frame, format!("{{\"lsn\":{lsn}}}"), "forme exacte");
    lsn
}

fn events_url(addr: SocketAddr, space: impl std::fmt::Display) -> String {
    format!("ws://{addr}/events/{space}")
}

/// Ouvre le canal de l'espace avec la session de `by` (sans cookie s'il n'en porte pas).
/// `Err` : le statut HTTP du refus.
async fn open(
    by: &Browser<'_>,
    addr: SocketAddr,
    space: impl std::fmt::Display,
) -> Result<Channel, u16> {
    let url = events_url(addr, space);
    let cookie = by.cookie();
    tokio::task::spawn_blocking(move || Channel::connect(addr, &url, cookie.as_deref()))
        .await
        .unwrap()
}

/// Comme [`open`], pour un canal qui doit s'ouvrir : attend sa trame d'ouverture.
async fn opened(by: &Browser<'_>, addr: SocketAddr, space: impl std::fmt::Display) -> Channel {
    let channel = open(by, addr, space)
        .await
        .unwrap_or_else(|status| panic!("canal de l'espace refusé : {status}"));
    assert!(
        channel.wait_until(|c| !c.texts().is_empty()).await,
        "pas de trame d'ouverture : {:?}",
        channel.seen()
    );
    channel
}

/// Comme [`opened`], pour un client qui ne répond jamais aux pings
/// ([`Channel::connect_silent`]).
async fn silent(by: &Browser<'_>, addr: SocketAddr, space: impl std::fmt::Display) -> Channel {
    let path = format!("/events/{space}");
    let cookie = by.cookie();
    let channel = tokio::task::spawn_blocking(move || {
        Channel::connect_silent(addr, &path, cookie.as_deref())
    })
    .await
    .unwrap();
    assert!(
        channel.wait_until(|c| !c.texts().is_empty()).await,
        "pas de trame d'ouverture : {:?}",
        channel.seen()
    );
    channel
}

/// La version de l'espace « Équipe », lue dans la base.
async fn lsn(team: &Team<'_>) -> i64 {
    team.store
        .space_lsn(team.space)
        .await
        .unwrap()
        .expect("l'espace")
}

/// Une application où tout le monde peut s'inscrire, et dont le canal de l'espace envoie un
/// ping tous les `ping`.
async fn app_pinging_every(ping: Duration) -> TestApp {
    TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.events_ping = ping;
    })
    .await
}

/// Le corps d'une page à la racine de l'espace « Équipe ».
fn in_team(team: &Team<'_>, title: &str) -> serde_json::Value {
    json!({ "title": title, "spaceId": team.space.to_string() })
}

fn member_uri(team: &Team<'_>, member: Uuid) -> String {
    format!("/api/v1/spaces/{}/members/{member}", team.space)
}

/// La première trame est la version de l'espace à l'ouverture ; la suivante arrive avec
/// l'écriture suivante, et la porte.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_frame_is_the_current_version() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    // Des écritures avant l'ouverture : la version n'est plus celle d'un espace neuf.
    team.page(None, "Une", &[members(Level::Full)]).await;
    team.page(None, "Deux", &[members(Level::Full)]).await;
    let before = lsn(&team).await;
    assert!(before > 0);

    let olga = opened(&team.olga.browser, addr, team.space).await;
    assert_eq!(olga.versions(), [before]);

    team.olga.create(in_team(&team, "Trois")).await;
    assert!(
        olga.wait_until(|c| c.versions().len() == 2).await,
        "{:?}",
        olga.seen()
    );
    let after = lsn(&team).await;
    assert!(after > before);
    assert_eq!(olga.versions(), [before, after]);
    assert!(!olga.is_closed());
}

/// Une membre et un invité qui ne voit qu'une page ouvrent le canal ; la membre crée une page
/// privée, la renomme, la met à la corbeille. Chacun reçoit des trames, toutes de la forme
/// exacte `{"lsn":N}`, croissantes, jusqu'à la version d'après la dernière écriture ; aucune ne
/// porte l'identifiant ni le titre de la page. L'invité ne voit toujours que sa page.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_events_carry_only_the_version() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let shared = team
        .page(None, "Pour Gina", &[user(&team.gina, Level::Read)])
        .await;
    let seen_by_gina = async || -> Vec<String> {
        tree_nodes(&team.gina.browser, team.space)
            .await
            .iter()
            .map(|node| node["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(seen_by_gina().await, [shared.to_string()]);
    let mona = opened(&team.mona.browser, addr, team.space).await;
    let gina = opened(&team.gina.browser, addr, team.space).await;

    let page = team.mona.create(in_team(&team, "Projet secret")).await;
    team.mona
        .patched(&page.id, json!({ "title": "Renommé en secret" }))
        .await;
    team.mona
        .browser
        .post_empty(&format!("/api/v1/pages/{}/trash", page.id))
        .await
        .assert_status(StatusCode::OK);
    let last = lsn(&team).await;

    let secrets = [
        page.id.as_str(),
        "Projet secret",
        "Renommé en secret",
        "secret",
    ];
    for (who, channel) in [("Mona", &mona), ("Gina", &gina)] {
        assert!(
            channel
                .wait_until(|c| c.versions().last() == Some(&last))
                .await,
            "{who} : {:?}",
            channel.seen()
        );
        let frames = channel.texts();
        assert!(
            frames.len() >= 2,
            "{who} : la trame d'ouverture et au moins une autre : {frames:?}"
        );
        let versions: Vec<i64> = frames.iter().map(|frame| version(frame)).collect();
        assert!(
            versions.windows(2).all(|pair| pair[0] < pair[1]),
            "{who} : croissantes : {versions:?}"
        );
        for frame in &frames {
            for secret in secrets {
                assert!(
                    !frame.contains(secret),
                    "{who} : {frame} porte « {secret} »"
                );
            }
        }
        // Que du texte et des pings, et la connexion vit.
        assert!(
            channel
                .seen()
                .iter()
                .all(|what| matches!(what, Seen::Text(_) | Seen::Ping)),
            "{who} : {:?}",
            channel.seen()
        );
    }
    assert_eq!(seen_by_gina().await, [shared.to_string()]);
}

/// Mona est retirée de l'espace : son canal est fermé par le serveur dès le retrait (le ping,
/// lui, ne viendrait que dans 20 s), sans lui avoir envoyé la version qui la retire ; une
/// écriture faite ensuite ne lui envoie rien, et elle ne peut plus l'ouvrir. Tom, resté
/// membre, reçoit les deux.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_stop_for_a_removed_member() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let mona = opened(&team.mona.browser, addr, team.space).await;
    let tom = opened(&team.tom.browser, addr, team.space).await;

    team.olga
        .browser
        .delete(&member_uri(&team, team.mona.id))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert!(
        mona.wait_until(Channel::is_closed).await,
        "fermé par le serveur : {:?}",
        mona.seen()
    );
    assert_eq!(mona.texts().len(), 1, "la trame d'ouverture, seule");
    let removed = lsn(&team).await;
    assert!(
        tom.wait_until(|c| c.versions().last() == Some(&removed))
            .await
    );

    team.olga.create(in_team(&team, "Après son départ")).await;
    let last = lsn(&team).await;
    assert!(tom.wait_until(|c| c.versions().last() == Some(&last)).await);
    assert_eq!(mona.texts().len(), 1);
    assert_eq!(mona.seen().last(), Some(&Seen::Closed));
    assert_eq!(
        open(&team.mona.browser, addr, team.space).await.err(),
        Some(404)
    );
    assert!(!tom.is_closed());
}

/// La session qui a ouvert le canal se ferme : déconnexion, changement de mot de passe (qui
/// ferme toutes celles du compte). D'abord avec le ping par défaut (20 s) : l'écriture
/// suivante ne leur envoie rien, et ferme leur canal ; Adam, lui, la reçoit. Puis avec un ping
/// court, sans aucune écriture : le ping suivant ferme le canal, après avoir laissé passer les
/// précédents tant que la session valait.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_stop_when_the_session_ends() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let adam = opened(&team.adam.browser, addr, team.space).await;
    let mona = opened(&team.mona.browser, addr, team.space).await;
    let tom = opened(&team.tom.browser, addr, team.space).await;

    team.mona
        .browser
        .log_out()
        .await
        .assert_status(StatusCode::NO_CONTENT);
    team.tom
        .browser
        .change_password(PASSWORD, "un-autre-mot-de-passe")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    team.olga.create(in_team(&team, "Après eux")).await;
    let last = lsn(&team).await;
    assert!(
        adam.wait_until(|c| c.versions().last() == Some(&last))
            .await
    );
    for (who, channel) in [("déconnexion", &mona), ("mot de passe changé", &tom)] {
        assert!(
            channel.wait_until(Channel::is_closed).await,
            "{who} : {:?}",
            channel.seen()
        );
        assert_eq!(channel.texts().len(), 1, "{who} : rien après l'ouverture");
    }
    assert!(!adam.is_closed());

    const PING: Duration = Duration::from_millis(200);
    let t = app_pinging_every(PING).await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let mona = opened(&team.mona.browser, addr, team.space).await;
    let tom = opened(&team.tom.browser, addr, team.space).await;
    for channel in [&mona, &tom] {
        assert!(channel.wait_until(|c| c.ping_times().len() >= 2).await);
        assert!(!channel.is_closed(), "la session vaut : les pings passent");
    }
    team.mona
        .browser
        .log_out()
        .await
        .assert_status(StatusCode::NO_CONTENT);
    team.tom
        .browser
        .change_password(PASSWORD, "un-autre-mot-de-passe")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    for (who, channel) in [("déconnexion", &mona), ("mot de passe changé", &tom)] {
        assert!(
            channel.wait_until(Channel::is_closed).await,
            "{who} : {:?}",
            channel.seen()
        );
        assert_eq!(channel.texts().len(), 1, "{who} : rien après l'ouverture");
    }
}

/// Une requête `GET` sans poignée de main, avec la session de `by` et cette en-tête `Origin` :
/// la réponse entière, pour comparer deux refus.
async fn plain_get(by: &Browser<'_>, uri: &str, origin: Option<&str>) -> Reply {
    let mut request = Request::get(uri);
    if let Some(origin) = origin {
        request = request.header(header::ORIGIN, origin);
    }
    by.send(request.body(Body::empty()).unwrap()).await
}

/// Deux refus sont les mêmes : statut, type de contenu, corps, et les mêmes en-têtes de cache.
fn assert_same(events: &Reply, sync: &Reply, what: &str) {
    assert_eq!(outcome(events), outcome(sync), "{what}");
    assert_eq!(
        events.headers.get(header::CACHE_CONTROL),
        sync.headers.get(header::CACHE_CONTROL),
        "{what} : en-têtes de /sync, pas ceux de l'API"
    );
}

/// Les refus d'avant la poignée de main, dans l'ordre de `/sync` et avec ses réponses :
/// `403 bad_origin`, `401`, `404`, `400 invalid_request`. Le `404` est celui de tout espace
/// introuvable, le même pour un espace inconnu, mal nommé ou sans rôle, admin de l'instance
/// compris (Ruling R152) : même statut et même code que celui de `/sync`. Le contrôle de
/// l'en-tête `Host` vaut aussi.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_refusals_match_sync() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    let page = team.page(None, "Commune", &[members(Level::Read)]).await;
    let nobody = t.browser();
    let mona = &team.mona.browser;
    let events = |space: &str| format!("/events/{space}");
    let sync = |page: &str| format!("/sync/{page}?schema={SCHEMA_VERSION}");
    let team_space = team.space.to_string();
    let unknown = Uuid::now_v7().to_string();

    // 1. Une `Origin` d'un autre site, avant tout : avec ou sans session, espace connu ou non.
    let evil = Some("http://evil.example");
    let sync_403 = plain_get(mona, &sync(&page.to_string()), evil).await;
    sync_403.assert_error(StatusCode::FORBIDDEN, "bad_origin");
    for (who, by, space) in [
        ("membre", mona, &team_space),
        ("sans session", &nobody, &team_space),
        ("sans session, espace inconnu", &nobody, &unknown),
    ] {
        assert_same(&plain_get(by, &events(space), evil).await, &sync_403, who);
        let url = events_url(addr, space);
        assert_eq!(
            handshake(&url, evil, by.cookie().as_deref()).await,
            Err(403),
            "{who}"
        );
    }

    // 2. Sans session : `401`, que l'espace existe ou non.
    let sync_401 = plain_get(&nobody, &sync(&page.to_string()), None).await;
    sync_401.assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    let logged_out = t.browser();
    logged_out
        .log_in("mona@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let closed_session = t.browser();
    closed_session.set_session(logged_out.session().as_deref());
    logged_out
        .log_out()
        .await
        .assert_status(StatusCode::NO_CONTENT);
    for (who, by, space) in [
        ("sans session", &nobody, team_space.as_str()),
        ("espace inconnu", &nobody, unknown.as_str()),
        ("id illisible", &nobody, "pas-un-uuid"),
        ("session fermée", &closed_session, team_space.as_str()),
    ] {
        assert_same(&plain_get(by, &events(space), None).await, &sync_401, who);
        let url = events_url(addr, space);
        assert_eq!(
            handshake(&url, None, by.cookie().as_deref()).await,
            Err(401),
            "{who}"
        );
    }

    // 3. Un espace où l'on n'a aucun rôle : la réponse d'un espace qui n'existe pas.
    let missing = plain_get(mona, &events(&unknown), None).await;
    missing.assert_error(StatusCode::NOT_FOUND, "not_found");
    let tree_404 = mona.get(&format!("/api/v1/spaces/{unknown}/tree")).await;
    assert_eq!(
        outcome(&missing),
        outcome(&tree_404),
        "celle de tout espace introuvable"
    );
    let sync_404 = plain_get(mona, &sync(&Uuid::now_v7().to_string()), None).await;
    sync_404.assert_error(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(missing.json()["error"], sync_404.json()["error"]);
    assert_eq!(
        missing.headers.get(header::CACHE_CONTROL),
        sync_404.headers.get(header::CACHE_CONTROL)
    );
    let zoe_space = team.zoe.space.to_string();
    for (who, by, space) in [
        ("id illisible", mona, "pas-un-uuid"),
        ("espace d'une autre", mona, zoe_space.as_str()),
        (
            "admin de l'instance, sans rôle",
            &team.sam.browser,
            team_space.as_str(),
        ),
        ("sans rôle", &team.zoe.browser, team_space.as_str()),
    ] {
        assert_eq!(
            outcome(&plain_get(by, &events(space), None).await),
            outcome(&missing),
            "{who}"
        );
        let url = events_url(addr, space);
        assert_eq!(
            handshake(&url, None, by.cookie().as_deref()).await,
            Err(404),
            "{who}"
        );
    }

    // 4. Sans demande de WebSocket, dans un espace où l'on a un rôle, invité compris.
    let sync_400 = plain_get(mona, &sync(&page.to_string()), None).await;
    sync_400.assert_error(StatusCode::BAD_REQUEST, "invalid_request");
    for (who, by) in [
        ("propriétaire", &team.olga.browser),
        ("membre", mona),
        ("invitée", &team.gina.browser),
    ] {
        assert_same(
            &plain_get(by, &events(&team_space), None).await,
            &sync_400,
            who,
        );
        let url = events_url(addr, &team_space);
        assert_eq!(
            handshake(&url, None, by.cookie().as_deref()).await,
            Ok(()),
            "{who}"
        );
    }

    // L'en-tête `Host` est contrôlé comme partout.
    let reply = t
        .send_raw(
            Request::get(events(&team_space))
                .header(header::HOST, "evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    reply.assert_error(StatusCode::MISDIRECTED_REQUEST, "bad_host");
}

/// Un canal où rien ne se passe reçoit un ping toutes les `events_ping` (20 s par défaut), et
/// reste ouvert tant qu'il y répond, bien au-delà de trois pings (R153). Le `k`-ième ping
/// n'arrive jamais avant `k` intervalles après l'ouverture : une borne que la charge du poste ne
/// peut pas faire manquer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_events_connection_is_kept_alive() {
    assert_eq!(EVENTS_PING, Duration::from_secs(20));
    assert_eq!(Config::default().events_ping, EVENTS_PING);

    const PING: Duration = Duration::from_millis(100);
    let t = app_pinging_every(PING).await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let started = Instant::now();
    let channel = opened(&alice.browser, addr, alice.space).await;
    assert!(
        channel.wait_until(|c| c.ping_times().len() >= 12).await,
        "{:?}",
        channel.seen()
    );
    for (k, at) in channel.ping_times().into_iter().enumerate() {
        let intervals = u32::try_from(k + 1).unwrap();
        assert!(
            at.duration_since(started) >= PING * intervals,
            "ping {} arrivé avant {intervals} intervalles",
            k + 1
        );
    }
    assert!(!channel.is_closed());
    assert_eq!(
        channel.texts().len(),
        1,
        "rien d'autre que la trame d'ouverture"
    );
}

/// Le canal ne lit rien : une trame de données du client, texte ou binaire, le ferme. Un ping
/// du client, lui, ne ferme rien.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn data_frames_from_the_client_close_the_channel() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    for message in [Message::text("{\"lsn\":1}"), Message::binary(vec![0, 1, 2])] {
        let channel = opened(&team.olga.browser, addr, team.space).await;
        channel.send(message.clone());
        assert!(
            channel.wait_until(Channel::is_closed).await,
            "{message:?} : {:?}",
            channel.seen()
        );
        assert_eq!(channel.texts().len(), 1);
    }

    let channel = opened(&team.olga.browser, addr, team.space).await;
    channel.send(Message::Ping(vec![1, 2, 3].into()));
    team.olga.create(in_team(&team, "Après un ping")).await;
    let last = lsn(&team).await;
    assert!(
        channel
            .wait_until(|c| c.versions().last() == Some(&last))
            .await,
        "{:?}",
        channel.seen()
    );
    assert!(!channel.is_closed());
}

/// La version que porte l'arbre (`GET /api/v1/spaces/{id}/tree`), en nombre : sans elle, le
/// test échoue.
async fn tree_version(by: &Browser<'_>, space: Uuid) -> i64 {
    let reply = by.get(&format!("/api/v1/spaces/{space}/tree")).await;
    reply.assert_status(StatusCode::OK);
    let tree = reply.json();
    tree["lsn"]
        .as_i64()
        .unwrap_or_else(|| panic!("l'arbre porte la version de l'espace, « lsn » : {tree}"))
}

/// Ruling R141 : l'arbre porte la version de l'espace, lue avant les droits et les pages. Après
/// une écriture de structure, l'arbre relu porte une version au moins égale à celle que le canal
/// a poussée pour elle : le client qui compare les deux ne manque aucun changement. Pour un
/// invité comme pour une membre ; et sans écriture, c'est la version de la base.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tree_is_as_recent_as_the_version_pushed() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let addr = t.listen().await;
    team.page(None, "Pour Gina", &[user(&team.gina, Level::Read)])
        .await;
    for (who, by) in [("Mona", &team.mona.browser), ("Gina", &team.gina.browser)] {
        assert_eq!(
            tree_version(by, team.space).await,
            lsn(&team).await,
            "{who}"
        );
    }
    let mona = opened(&team.mona.browser, addr, team.space).await;
    let gina = opened(&team.gina.browser, addr, team.space).await;

    let page = team.mona.create(in_team(&team, "Neuve")).await;
    for (who, channel, by) in [
        ("Mona", &mona, &team.mona.browser),
        ("Gina", &gina, &team.gina.browser),
    ] {
        assert!(
            channel.wait_until(|c| c.versions().len() == 2).await,
            "{who} : {:?}",
            channel.seen()
        );
        let pushed = channel.versions()[1];
        let tree = tree_version(by, team.space).await;
        assert!(tree >= pushed, "{who} : arbre {tree}, trame {pushed}");
    }
    assert!(
        tree_nodes(&team.mona.browser, team.space)
            .await
            .iter()
            .any(|node| node["id"] == page.id.as_str())
    );
}

/// R153 : un client qui lit tout mais ne répond à aucun ping est fermé après le troisième resté
/// sans réponse : trois pings, puis la fermeture, au quatrième intervalle au plus tôt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unanswered_pings_close_the_channel() {
    const PING: Duration = Duration::from_millis(100);
    let t = app_pinging_every(PING).await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let started = Instant::now();
    let channel = silent(&alice.browser, addr, alice.space).await;

    assert!(
        channel.wait_until(Channel::is_closed).await,
        "{:?}",
        channel.seen()
    );
    assert_eq!(channel.ping_times().len(), 3, "{:?}", channel.seen());
    assert_eq!(channel.texts().len(), 1);
    let closed = channel.closed_at().unwrap().duration_since(started);
    assert!(closed >= PING * 4, "fermé après {closed:?}");
}

/// R153 : un pong, même tardif, remet le compte à zéro. Le client silencieux répond au deuxième
/// ping seulement : il reçoit plus de trois pings avant d'être fermé, et il l'est quand même,
/// trois pings sans réponse plus tard.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pong_starts_the_count_again() {
    const PING: Duration = Duration::from_millis(300);
    let t = app_pinging_every(PING).await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let channel = silent(&alice.browser, addr, alice.space).await;

    assert!(channel.wait_until(|c| c.ping_times().len() >= 2).await);
    channel.send(Message::Pong(Vec::new().into()));
    assert!(
        channel.wait_until(Channel::is_closed).await,
        "{:?}",
        channel.seen()
    );
    let pings = channel.ping_times().len();
    assert!(pings > 3, "{pings} pings : le pong n'a rien remis à zéro");
}

/// R153, vu comme une attaque : des pongs ne gardent en vie que le compte des pings sans
/// réponse. Un client dont la session est fermée a beau en envoyer sans arrêt, le ping suivant
/// relit sa session et ferme le canal, sans qu'il ait rien reçu de plus.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pongs_do_not_keep_a_closed_session_alive() {
    const PING: Duration = Duration::from_millis(100);
    let t = app_pinging_every(PING).await;
    let alice = t.account("Alice").await;
    let addr = t.listen().await;
    let channel = silent(&alice.browser, addr, alice.space).await;

    alice
        .browser
        .log_out()
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let deadline = Instant::now() + WAIT;
    while !channel.is_closed() {
        assert!(Instant::now() < deadline, "{:?}", channel.seen());
        channel.send(Message::Pong(Vec::new().into()));
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(channel.texts().len(), 1);
}
