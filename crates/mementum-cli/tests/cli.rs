//! Ligne de commande : les options l'emportent sur l'environnement, et une
//! sortie standard fermée n'arrête pas le serveur.

mod common;

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use common::{Server, http};

/// Délai maximal de démarrage.
const STARTUP: Duration = Duration::from_secs(30);

#[test]
fn options_override_environment() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("option");
    let ignored = dir.path().join("variable");
    // `MEMENTUM_BIND` invalide : sans effet, puisque `--bind` est donné.
    let server = Server::start_with_env(
        &data,
        &[
            ("MEMENTUM_BIND", "nimporte"),
            ("MEMENTUM_DATA_DIR", ignored.to_str().unwrap()),
        ],
    );
    assert_eq!(server.http("GET", "/health", None), (200, "ok".to_string()));
    assert!(data.join("mementum.db").is_file(), "base dans --data-dir");
    assert!(!ignored.exists(), "MEMENTUM_DATA_DIR ignoré");
}

/// Tue le processus quand le test se termine, même en échec.
struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn closed_stdout_does_not_stop_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Kill(
        Command::new(env!("CARGO_BIN_EXE_mementum"))
            .args(["serve", "--bind", "127.0.0.1:0", "--data-dir"])
            .arg(dir.path().join("donnees"))
            .env("RUST_LOG", "info")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("lancement de mementum"),
    );
    // Sortie standard fermée avant que le serveur n'y écrive.
    drop(child.0.stdout.take());
    let stderr = child.0.stderr.take().unwrap();
    let (lines, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if lines.send(line).is_err() {
                break;
            }
        }
    });

    // L'adresse se lit alors dans le journal « serveur prêt ».
    let mut log = String::new();
    let addr = loop {
        let line = rx
            .recv_timeout(STARTUP)
            .unwrap_or_else(|_| panic!("serveur jamais prêt ; journal :\n{log}"));
        log.push_str(&line);
        log.push('\n');
        if line.contains("serveur prêt") {
            let addr = line
                .split_whitespace()
                .find_map(|word| word.strip_prefix("addr="))
                .expect("adresse dans le journal");
            break addr.parse::<SocketAddr>().unwrap();
        }
    };
    assert!(
        log.contains("WARN") && log.contains("sortie standard"),
        "avertissement attendu :\n{log}"
    );
    assert_eq!(http(addr, "GET", "/health", None), (200, "ok".to_string()));
    assert!(
        child.0.try_wait().unwrap().is_none(),
        "le serveur tourne toujours"
    );
}
