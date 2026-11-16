//! Aides des tests qui ouvrent la WebSocket de synchro d'une page : ce qu'un client tape et
//! reçoit, si sa connexion est encore servie ou fermée, et l'équipe dont ces tests posent les
//! accès par le store.
//!
//! L'URL, la connexion avec une session (`sync_url`, `sync_as`, `synced_as`) et la poignée de
//! main nue (`handshake`) sont dans `common/mod.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use mementum_acl::{Level, Subject};
use mementum_server::SignupMode;
use mementum_store::StoreOptions;
use mementum_sync::testkit::Client;

use tower::ServiceExt;

use super::{Account, Browser, TestApp, WAIT, synced_as};

/// Une application où tout le monde peut s'inscrire : les comptes de ces tests reçoivent leur
/// rôle par le store.
pub async fn open_app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

/// Un disque lent et un délai d'attente court sur le verrou de SQLite : chaque validation tient
/// le verrou 50 ms de plus, et une connexion n'attend le verrou que 100 ms (jusqu'à 250 ms sous
/// Windows, où chaque pause de SQLite dure au moins 15 ms). Dès que plus de six écritures se
/// disputent le verrou, l'une d'elles l'attend plus que ce délai, quel que soit l'ordre où il
/// les sert : sans la file d'écriture du store, elle échoue (`database is locked`, une route
/// répond `500`). Ce qui ne se voyait que par moments sur la CI sous Windows (Ruling R485)
/// devient sûr.
pub const SLOW_DISK: StoreOptions = StoreOptions {
    busy_timeout: Duration::from_millis(100),
    write_delay: Duration::from_millis(50),
    slow_write: mementum_store::SLOW_WRITE,
};

/// [`open_app`], dont la base a ces réglages ([`SLOW_DISK`], par exemple).
pub async fn open_app_with(store: StoreOptions) -> TestApp {
    TestApp::with_config(|cfg| {
        cfg.signup = SignupMode::Open;
        cfg.store = store;
    })
    .await
}

/// Des boucles de requêtes d'un même compte, sans relâche : chacune renvoie sa requête dès sa
/// réponse reçue. La première réponse qui n'a pas le statut attendu les arrête toutes.
pub struct Loops {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicUsize>,
    tasks: Vec<tokio::task::JoinHandle<Option<(StatusCode, String)>>>,
}

impl Loops {
    /// Lance `n` boucles de `by` ; chaque tour de la boucle `k` envoie `request(k, tour)` (avec
    /// l'hôte et le cookie de `by`, le tour compté de 0 dans la boucle), et attend `expected`.
    pub fn start(
        t: &TestApp,
        by: &Browser<'_>,
        n: usize,
        expected: StatusCode,
        request: impl Fn(usize, usize) -> Request<Body> + Send + Sync + 'static,
    ) -> Loops {
        let cookie = by.cookie().expect("une session");
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicUsize::new(0));
        let request = Arc::new(request);
        let tasks = (0..n)
            .map(|k| {
                let (router, cookie, request) = (t.router.clone(), cookie.clone(), request.clone());
                let (stop, done) = (stop.clone(), done.clone());
                tokio::spawn(async move {
                    let mut round = 0;
                    while !stop.load(Ordering::Relaxed) {
                        let mut req = request(k, round);
                        round += 1;
                        let headers = req.headers_mut();
                        headers.insert(header::HOST, "localhost".parse().unwrap());
                        headers.insert(header::COOKIE, cookie.parse().unwrap());
                        let reply = router.clone().oneshot(req).await.unwrap();
                        let status = reply.status();
                        if status != expected {
                            stop.store(true, Ordering::Relaxed);
                            let body = axum::body::to_bytes(reply.into_body(), usize::MAX)
                                .await
                                .unwrap_or_default();
                            return Some((status, String::from_utf8_lossy(&body).into_owned()));
                        }
                        done.fetch_add(1, Ordering::Relaxed);
                    }
                    None
                })
            })
            .collect();
        Loops { stop, done, tasks }
    }

    /// Les réponses attendues reçues jusqu'ici.
    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }

    /// Une réponse inattendue a arrêté les boucles.
    pub fn failed(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Arrête les boucles, attend la fin de leur tour en cours, et rend les réponses inattendues
    /// (statut, corps).
    pub async fn stop(self) -> Vec<(StatusCode, String)> {
        self.stop.store(true, Ordering::Relaxed);
        let mut refused = Vec::new();
        for task in self.tasks {
            refused.extend(task.await.unwrap());
        }
        refused
    }
}

/// L'accès de tous les membres de l'espace, à ce niveau.
pub fn members(level: Level) -> (Subject, Level) {
    (Subject::Members, level)
}

/// L'accès de ce compte, à ce niveau.
pub fn user(account: &Account<'_>, level: Level) -> (Subject, Level) {
    (Subject::User(account.id), level)
}

/// Le texte de chaque bloc de premier niveau du document du client, une ligne par bloc.
pub fn text(client: &Client) -> String {
    client
        .blocks()
        .iter()
        .map(mementum_md::block_text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tape `typed` à la fin du premier bloc, puis attend que le serveur ait traité tout ce que
/// la connexion a envoyé : il traite ses messages dans l'ordre, et répond ensuite à la
/// resynchronisation.
pub async fn types(client: &Client, typed: &str) {
    let block = client.blocks()[0].id.clone();
    let start = client.step2_count();
    client.type_text(&block, typed);
    client.resync();
    assert!(
        client.wait_until(WAIT, |c| c.step2_count() > start).await,
        "le serveur n'a pas traité « {typed} »"
    );
}

/// Tape `typed` à la fin du premier bloc sans rien attendre : pour une connexion que le
/// serveur vient de fermer, et qui ne le sait peut-être pas encore.
pub fn types_unheard(client: &Client, typed: &str) {
    let block = client.blocks()[0].id.clone();
    client.type_text(&block, typed);
}

/// Le serveur ferme la connexion ([`WAIT`] au plus).
pub async fn closed(client: &Client) -> bool {
    client.wait_until(WAIT, |c| c.is_closed()).await
}

/// La connexion est encore servie : une resynchronisation reçoit sa réponse.
///
/// Ce n'est pas une barrière pour ce que l'acteur diffuse : la réponse passe derrière ce qu'il
/// avait diffusé avant que la connexion reçoive la resynchronisation, mais **devant** ce qu'il
/// diffuse pendant qu'elle attend sa réponse, même demandé avant (une trame de page, que la
/// route demande sans attendre l'acteur). Pour compter ce qu'un client a reçu, ou prouver
/// qu'il n'a rien reçu, passer par un [`Witness`].
pub async fn still_served(client: &Client) -> bool {
    let start = client.step2_count();
    client.resync();
    client.wait_until(WAIT, |c| c.step2_count() > start).await && !client.is_closed()
}

/// Un témoin : une connexion de plus sur une page, d'un compte qui peut y écrire, dont la
/// présence sert de barrière (Ruling R342). La diffusion d'une page est ordonnée, et l'acteur
/// traite ses demandes dans l'ordre : quand un client voit une marque du témoin, tout ce que
/// l'acteur a diffusé pour les demandes arrivées avant elle lui est déjà parvenu. Une demande
/// faite par une route avant de répondre (`Hub::page_event`) est de celles-là pour une marque
/// posée après la réponse.
pub struct Witness {
    client: Client,
    marks: AtomicU32,
}

impl Witness {
    /// Ouvre la page `url` avec la session de `by`, qui doit pouvoir y écrire : la présence
    /// d'une connexion en lecture seule n'est ni gardée ni relayée (Ruling R118).
    pub async fn join(by: &Browser<'_>, url: &str) -> Witness {
        Witness {
            client: synced_as(by, url).await,
            marks: AtomicU32::new(0),
        }
    }

    /// Pose une marque neuve (une couleur jamais annoncée ; le nom, lui, est récrit par le
    /// serveur), puis attend que chacun des `observers` l'ait vue, [`WAIT`] au plus. La borne
    /// peut faire échouer le test à tort sur un poste très chargé ; passée, ce que l'acteur a
    /// diffusé avant la marque est arrivé chez eux, et rien de plus ancien n'arrivera.
    pub async fn seen_by(&self, observers: &[&Client]) {
        let mark = format!("#{:06x}", self.marks.fetch_add(1, Ordering::SeqCst) + 1);
        self.client.set_awareness(&format!(
            r#"{{"user":{{"name":"Témoin","color":"{mark}"}}}}"#
        ));
        let witness = self.client.client_id();
        let marked = |observer: &Client| {
            observer
                .awareness_states()
                .get(&witness)
                .and_then(|state| serde_json::from_str::<serde_json::Value>(state).ok())
                .is_some_and(|state| state["user"]["color"] == mark.as_str())
        };
        for observer in observers {
            assert!(
                observer.wait_until(WAIT, marked).await,
                "la marque {mark} du témoin n'est pas arrivée : connexion fermée, ou présence \
                 non relayée ({:?})",
                observer.awareness_states().get(&witness)
            );
        }
    }
}
