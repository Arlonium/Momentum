//! Outils des tests du binaire : `mementum serve` lancé en sous-processus sur
//! un port libre de 127.0.0.1 (jamais 8080, Ruling R24) et un petit client
//! HTTP/1.1.
//!
//! Le processus est tué dans le `Drop` de [`Server`] : aucun `mementum` ne
//! survit à un test, même en échec.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

/// Début de la ligne que `serve` écrit sur la sortie standard une fois à
/// l'écoute (Ruling R24).
pub const READY: &str = "mementum écoute sur http://";

/// Délai maximal de démarrage du serveur.
const STARTUP: Duration = Duration::from_secs(30);

/// Un `mementum serve` en sous-processus.
pub struct Server {
    child: Option<Child>,
    pub addr: SocketAddr,
    stderr: Arc<Mutex<String>>,
}

impl Server {
    /// Lance `mementum serve --bind 127.0.0.1:0 --data-dir <data_dir>` et
    /// attend la ligne qui donne l'adresse liée.
    pub fn start(data_dir: &Path) -> Server {
        Server::start_with_env(data_dir, &[])
    }

    /// Comme [`Server::start`], avec des variables d'environnement en plus.
    pub fn start_with_env(data_dir: &Path, vars: &[(&str, &str)]) -> Server {
        Server::start_with_args(data_dir, &[], vars)
    }

    /// Comme [`Server::start`], avec d'autres options de `serve` et des
    /// variables d'environnement en plus.
    pub fn start_with_args(data_dir: &Path, args: &[&str], vars: &[(&str, &str)]) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mementum"))
            .args(["serve", "--bind", "127.0.0.1:0", "--data-dir"])
            .arg(data_dir)
            .args(args)
            .envs(vars.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("lancement de mementum");
        let stdout = child.stdout.take().expect("sortie standard");
        let stderr = child.stderr.take().expect("sortie d'erreur");
        let mut server = Server {
            child: Some(child),
            addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            stderr: Arc::new(Mutex::new(String::new())),
        };

        // Les deux sorties sont lues jusqu'au bout : un tuyau plein bloquerait
        // le serveur.
        let (ready, addr) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(rest) = line.strip_prefix(READY) {
                    let _ = ready.send(rest.trim().parse::<SocketAddr>());
                }
            }
        });
        let log = server.stderr.clone();
        thread::spawn(move || {
            let mut stderr = BufReader::new(stderr);
            let mut line = String::new();
            while stderr.read_line(&mut line).is_ok_and(|n| n > 0) {
                log.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push_str(&line);
                line.clear();
            }
        });

        match addr.recv_timeout(STARTUP) {
            Ok(Ok(addr)) => {
                assert!(addr.ip().is_loopback(), "écoute sur {addr}");
                assert_ne!(addr.port(), 8080, "jamais le port 8080");
                server.addr = addr;
                server
            }
            Ok(Err(e)) => panic!("adresse illisible ({e}) ; stderr :\n{}", server.stderr()),
            Err(_) => panic!(
                "mementum n'a pas annoncé son adresse ; stderr :\n{}",
                server.stderr()
            ),
        }
    }

    /// Identifiant du processus.
    pub fn pid(&self) -> u32 {
        self.child.as_ref().expect("processus vivant").id()
    }

    /// Ce que le serveur a écrit sur sa sortie d'erreur.
    pub fn stderr(&self) -> String {
        self.stderr
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Tue le processus (`TerminateProcess` sous Windows, `SIGKILL` ailleurs :
    /// l'équivalent d'un `kill -9`) et attend sa fin.
    pub fn kill(mut self) {
        self.terminate();
    }

    fn terminate(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// URL WebSocket de la page pour un client du schéma `schema`.
    pub fn ws_url(&self, page: &str, schema: u64) -> String {
        format!("ws://{}/sync/{page}?schema={schema}", self.addr)
    }

    /// Requête HTTP vers ce serveur.
    pub fn http(&self, method: &str, path: &str, json: Option<&str>) -> (u16, String) {
        http(self.addr, method, path, json)
    }

    /// Requête HTTP vers ce serveur, avec des en-têtes en plus ; rend aussi
    /// celles de la réponse.
    pub fn request(
        &self,
        method: &str,
        path: &str,
        json: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Response {
        request(self.addr, method, path, json, headers)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// Écrit dans `db` une base telle que le code du lot 1 la laisse, comme dans
/// les tests du store : les options de connexion de `Store::open`, la seule
/// migration `0001_init.sql` lancée par sqlx, puis une organisation, un coffre
/// et une page écrits en SQL. Les connexions sont fermées au retour.
pub fn lot1_database(db: &Path) {
    use sqlx::sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
    };
    let migrations = tempfile::tempdir().unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mementum-store/migrations/0001_init.sql"),
        migrations.path().join("0001_init.sql"),
    )
    .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let options = SqliteConnectOptions::new()
            .filename(db)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_millis(5000));
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate::Migrator::new(migrations.path())
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();
        for sql in [
            "INSERT INTO orgs (id, slug, name, created_at) \
             VALUES ('01890000-0000-7000-8000-000000000001', 'default', 'default', 1000)",
            "INSERT INTO vaults (id, org_id, name, kind, created_at) \
             VALUES ('01890000-0000-7000-8000-000000000002', \
                     '01890000-0000-7000-8000-000000000001', 'Mon coffre', 'personal', 1001)",
            "INSERT INTO nodes \
                 (id, org_id, vault_id, kind, title, position, created_at, updated_at) \
             VALUES ('01890000-0000-7000-8000-000000000003', \
                     '01890000-0000-7000-8000-000000000001', \
                     '01890000-0000-7000-8000-000000000002', 'page', 'Accueil', 'a0', 2000, 2000)",
        ] {
            sqlx::query(sql).execute(&pool).await.unwrap();
        }
        pool.close().await;
    });
}

/// La réponse d'une requête HTTP.
pub struct Response {
    pub status: u16,
    /// Les en-têtes, noms en minuscules.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Response {
    /// Le cookie de session posé par la réponse, sous la forme à renvoyer
    /// dans une en-tête `Cookie` (`mementum_session=<jeton>`).
    pub fn session_cookie(&self) -> Option<String> {
        self.headers
            .iter()
            .filter(|(name, _)| name == "set-cookie")
            .filter_map(|(_, value)| value.split(';').next())
            .find(|pair| pair.starts_with("mementum_session="))
            .map(str::to_string)
    }
}

/// Une requête HTTP/1.1 (`Connection: close`), corps JSON facultatif.
/// Renvoie le statut et le corps de la réponse.
pub fn http(addr: SocketAddr, method: &str, path: &str, json: Option<&str>) -> (u16, String) {
    let response = request(addr, method, path, json, &[]);
    (response.status, response.body)
}

/// Comme [`http`], avec des en-têtes en plus ; rend aussi celles de la
/// réponse.
pub fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    json: Option<&str>,
    headers: &[(&str, &str)],
) -> Response {
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(5)).expect("connexion HTTP");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let body = json.unwrap_or("");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if json.is_some() {
        request.push_str("Content-Type: application/json\r\n");
    }
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).expect("envoi HTTP");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("réponse HTTP");
    let raw = String::from_utf8(raw).expect("réponse en UTF-8");
    let (head, body) = raw.split_once("\r\n\r\n").expect("en-têtes HTTP");
    assert!(
        !head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked"),
        "réponse découpée inattendue : {head}"
    );
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .expect("statut HTTP");
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    Response {
        status,
        headers,
        body: body.to_string(),
    }
}
