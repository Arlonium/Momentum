//! Ligne de commande : les options l'emportent sur l'environnement, une
//! sortie standard fermée n'arrête pas le serveur, et `mementum user
//! set-password` rend un compte à qui a oublié son mot de passe.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use common::{Server, http};
use serde_json::json;

/// Délai maximal de démarrage.
const STARTUP: Duration = Duration::from_secs(30);

/// Mots de passe de test : des constantes manifestement fausses.
const PASSWORD: &str = "mot-de-passe-de-test";
const NEW_PASSWORD: &str = "nouveau-mot-de-passe-de-test";

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

/// Lance `mementum user set-password <args>` avec `stdin` sur son entrée
/// standard, et rend sa sortie. Les journaux sont au plus bavard : même là,
/// rien de secret ne doit sortir.
fn set_password(args: &[&str], vars: &[(&str, &str)], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mementum"))
        .args(["user", "set-password"])
        .args(args)
        .env_remove("MEMENTUM_DATA_DIR")
        .env("RUST_LOG", "trace")
        .envs(vars.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("lancement de mementum");
    // La commande peut finir sans lire (options refusées) : un tuyau fermé
    // n'est pas une erreur du test.
    let _ = child
        .stdin
        .take()
        .expect("entrée standard")
        .write_all(stdin.as_bytes());
    child.wait_with_output().expect("fin de mementum")
}

fn text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Attente maximale d'un démarrage refusé : bien plus qu'il n'en faut pour lire le réglage et
/// s'arrêter, bien moins qu'un test qui pendrait.
const REFUSAL_WAIT: Duration = Duration::from_secs(25);

/// Lance `mementum serve` avec `args` et `vars`, qui doivent arrêter le démarrage, et rend sa
/// sortie d'erreur. Un serveur qui démarre quand même (la validation a disparu) n'est pas
/// attendu sans fin : il est tué après [`REFUSAL_WAIT`], et le test échoue en le disant.
fn refused_start(data: &Path, args: &[&str], vars: &[(&str, &str)]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mementum"))
        .args(["serve", "--bind", "127.0.0.1:0", "--data-dir"])
        .arg(data)
        .args(args)
        .envs(vars.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("lancement de mementum");
    let deadline = std::time::Instant::now() + REFUSAL_WAIT;
    while child.try_wait().expect("état de mementum").is_none() {
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().expect("sortie de mementum");
            let (_, stderr) = text(&output);
            panic!("mementum a démarré malgré {args:?} et {vars:?} : {stderr}");
        }
        thread::sleep(Duration::from_millis(50));
    }
    let output = child.wait_with_output().expect("sortie de mementum");
    let (stdout, stderr) = text(&output);
    assert!(!output.status.success(), "{stdout}");
    stderr
}

/// Crée le compte d'Alice et rend le cookie de sa session.
fn sign_up_alice(server: &Server) -> String {
    let body = json!({ "email": "alice@exemple.fr", "name": "Alice", "password": PASSWORD });
    let reply = server.request("POST", "/api/v1/auth/signup", Some(&body.to_string()), &[]);
    assert_eq!(reply.status, 201, "{}", reply.body);
    reply.session_cookie().expect("cookie de session")
}

/// Le statut d'une connexion avec ce mot de passe.
fn log_in(server: &Server, password: &str) -> u16 {
    let body = json!({ "email": "alice@exemple.fr", "password": password });
    server
        .http("POST", "/api/v1/auth/login", Some(&body.to_string()))
        .0
}

/// Le statut de `GET /api/v1/me` avec ce cookie.
fn me(server: &Server, cookie: &str) -> u16 {
    server
        .request("GET", "/api/v1/me", None, &[("Cookie", cookie)])
        .status
}

#[test]
fn cli_set_password_recovers_account() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    // Le serveur aussi écrit tous ses journaux : ils sont relus à la fin.
    let server = Server::start_with_env(&data, &[("RUST_LOG", "trace")]);
    let cookie = sign_up_alice(&server);
    assert_eq!(me(&server, &cookie), 200);
    // Un jeton d'API, créé par la route (lot 5, Task S6) : la commande le révoque, et le dit.
    let created = server.request(
        "POST",
        "/api/v1/tokens",
        Some(
            &json!({ "name": "agent", "level": "read", "scope": "all", "password": PASSWORD })
                .to_string(),
        ),
        &[("Cookie", &cookie)],
    );
    assert_eq!(created.status, 201, "{}", created.body);
    let api_token = serde_json::from_str::<serde_json::Value>(&created.body).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let bearer = format!("Bearer {api_token}");
    let me_by_token = |server: &Server| {
        server
            .request("GET", "/api/v1/me", None, &[("Authorization", &bearer)])
            .status
    };
    // `GET /me` répond à un jeton depuis la Task S7 (sans adresse e-mail, Ruling R1388) : `200`,
    // et non `401`, dit que le jeton vit.
    assert_eq!(me_by_token(&server), 200);

    // Pendant que le serveur tourne. Le mot de passe est la première ligne
    // de l'entrée standard, sans sa fin de ligne ; la suite ne compte pas.
    let output = set_password(
        &[
            "--email",
            " Alice@Exemple.FR ",
            "--data-dir",
            data.to_str().unwrap(),
        ],
        &[],
        &format!("{NEW_PASSWORD}\r\nune seconde ligne\n"),
    );
    let (stdout, stderr) = text(&output);
    assert!(output.status.success(), "échec : {stdout}\n{stderr}");
    assert!(stdout.contains("alice@exemple.fr"), "{stdout}");
    // La commande dit la longueur de ce qu'elle a enregistré : de quoi voir
    // une ligne tronquée ou un caractère en trop.
    assert_eq!(NEW_PASSWORD.chars().count(), 28);
    assert!(
        stdout.contains("mot de passe de 28 caractères enregistré"),
        "{stdout}"
    );
    // Les jetons d'API révoqués, comptés (Ruling R1221) : le jeton ne vaut plus.
    assert!(stdout.contains("1 jeton d'API révoqué"), "{stdout}");
    assert_eq!(me_by_token(&server), 401);
    // Jamais affiché, même avec les journaux au plus bavard ; pas plus que
    // son hachage.
    for secret in [NEW_PASSWORD, PASSWORD, "$argon2", "une seconde ligne"] {
        assert!(!stdout.contains(secret), "« {secret} » sur la sortie");
        assert!(!stderr.contains(secret), "« {secret} » dans les journaux");
    }

    // L'ancien mot de passe échoue, le nouveau passe, les sessions d'avant
    // sont mortes.
    assert_eq!(log_in(&server, PASSWORD), 401);
    assert_eq!(log_in(&server, NEW_PASSWORD), 200);
    assert_eq!(log_in(&server, &format!("{NEW_PASSWORD}\r")), 401);
    assert_eq!(me(&server, &cookie), 401);

    // Le serveur non plus n'a rien écrit de secret, journaux au plus bavard :
    // ni mot de passe, ni hachage, ni jeton de session. Ses journaux portent
    // bien ce parcours.
    let log = server.stderr();
    assert!(
        log.contains("compte créé") && log.contains("connexion refusée"),
        "journaux du serveur attendus : {log}"
    );
    let token = cookie
        .strip_prefix("mementum_session=")
        .expect("le jeton du cookie");
    assert_eq!(token.len(), 43, "{token}");
    for secret in [NEW_PASSWORD, PASSWORD, "$argon2", token, api_token.as_str()] {
        assert!(
            !log.contains(secret),
            "« {secret} » dans le journal du serveur"
        );
    }
}

/// Ce que la commande refuse, sans rien changer : pas de base, pas de compte,
/// mot de passe hors règles.
#[test]
fn cli_set_password_refusals() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    let failed = |output: &Output, expected: &str| {
        let (stdout, stderr) = text(output);
        assert!(!output.status.success(), "succès inattendu : {stdout}");
        assert!(
            stderr.contains(expected),
            "« {expected} » attendu : {stderr}"
        );
        for secret in [NEW_PASSWORD, "$argon2", "neuf-car."] {
            assert!(!stdout.contains(secret) && !stderr.contains(secret));
        }
    };
    let run = |email: &str, dir: &Path, stdin: &str| {
        set_password(
            &["--email", email, "--data-dir", dir.to_str().unwrap()],
            &[],
            stdin,
        )
    };

    // Pas de base dans ce dossier : la commande n'en crée pas.
    let new_line = format!("{NEW_PASSWORD}\n");
    failed(&run("alice@exemple.fr", &data, &new_line), "aucune base");
    assert!(!data.exists(), "rien n'est créé");

    let server = Server::start(&data);
    let cookie = sign_up_alice(&server);
    failed(&run("bob@exemple.fr", &data, &new_line), "aucun compte");
    failed(&run("alice@exemple.fr", &data, "neuf-car.\n"), "10 à 200");
    failed(&run("alice@exemple.fr", &data, ""), "10 à 200");
    let long = format!("{}\n", "a".repeat(201));
    failed(&run("alice@exemple.fr", &data, &long), "10 à 200");
    // Un caractère de contrôle : la commande dit lequel et où, sans rien
    // montrer d'autre du mot de passe. Une tabulation, un retour chariot au
    // milieu, un octet nul ; et une entrée en UTF-16, comme l'écrit
    // PowerShell 5 (un octet nul après chaque lettre).
    for (stdin, expected) in [
        (format!("{NEW_PASSWORD}\tx\n"), "U+0009, en position 29"),
        (
            "nouveau\rmot-de-passe\n".to_string(),
            "U+000D, en position 8",
        ),
        (
            "nouveau\0mot-de-passe\n".to_string(),
            "U+0000, en position 8",
        ),
        (
            "n\0o\0u\0v\0e\0a\0u\0-\0m\0o\0t\0\n\0".to_string(),
            "UTF-16",
        ),
    ] {
        let output = run("alice@exemple.fr", &data, &stdin);
        failed(&output, "caractère de contrôle");
        failed(&output, expected);
        failed(&output, "Rien n'a été changé");
    }
    // L'adresse est obligatoire.
    let output = set_password(&["--data-dir", data.to_str().unwrap()], &[], &new_line);
    assert!(!output.status.success());

    // Rien n'a changé.
    assert_eq!(log_in(&server, NEW_PASSWORD), 401);
    assert_eq!(log_in(&server, PASSWORD), 200);
    assert_eq!(me(&server, &cookie), 200);

    // Sans `--data-dir`, le dossier vient de `MEMENTUM_DATA_DIR` ; une autre
    // variable, même invalide, n'arrête pas la commande.
    let output = set_password(
        &["--email", "alice@exemple.fr"],
        &[
            ("MEMENTUM_DATA_DIR", data.to_str().unwrap()),
            ("MEMENTUM_BIND", "nimporte"),
            ("MEMENTUM_SIGNUP", "nimporte"),
        ],
        &new_line,
    );
    let (stdout, stderr) = text(&output);
    assert!(output.status.success(), "échec : {stdout}\n{stderr}");
    assert_eq!(log_in(&server, NEW_PASSWORD), 200);
    assert_eq!(me(&server, &cookie), 401);
}

/// Ruling R67 : la commande peut être la première à ouvrir une base du lot 1
/// avec ce binaire, donc à la migrer. La copie d'avant migration est alors
/// nommée sur la sortie d'erreur, même si la commande échoue ensuite (une
/// base du lot 1 n'a aucun compte), et quel que soit le niveau des journaux.
#[test]
fn cli_set_password_names_the_backup_it_made() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    std::fs::create_dir_all(&data).unwrap();
    common::lot1_database(&data.join("mementum.db"));
    let backup = data.join("mementum.db.avant-0002.bak");
    assert!(!backup.exists());

    let run = || {
        set_password(
            &[
                "--email",
                "alice@exemple.fr",
                "--data-dir",
                data.to_str().unwrap(),
            ],
            // Journaux coupés : la ligne ne passe pas par eux.
            &[("RUST_LOG", "off")],
            &format!("{NEW_PASSWORD}\n"),
        )
    };
    let output = run();
    let (stdout, stderr) = text(&output);
    assert!(!output.status.success(), "succès inattendu : {stdout}");
    assert!(stderr.contains("aucun compte"), "{stderr}");
    assert!(backup.is_file(), "copie attendue : {}", backup.display());
    let bytes = std::fs::metadata(&backup).unwrap().len();
    let line = stderr
        .lines()
        .find(|line| line.contains("mementum.db.avant-0002.bak"))
        .unwrap_or_else(|| panic!("la copie n'est pas nommée : {stderr}"));
    assert!(line.contains("migrée"), "{line}");
    assert!(line.contains(&format!("{bytes} octets")), "{line}");

    // La base est migrée : la fois suivante, plus de copie à annoncer.
    let (_, stderr) = text(&run());
    assert!(!stderr.contains(".bak"), "{stderr}");
}

/// Les noms des entrées de `dir`, triés.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Ruling R840 : une base qu'un binaire intermédiaire d'un lot a migrée (sa
/// migration `0003` n'est pas celle de ce binaire) est refusée au démarrage,
/// avant toute copie, par un message en français, dit une fois, qui nomme la
/// copie à reprendre. La base n'a pas changé d'un octet, et rien n'est posé à
/// côté d'elle.
#[test]
fn a_database_migrated_by_an_intermediate_binary_is_refused_once_in_french() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    std::fs::create_dir_all(&data).unwrap();
    let db = data.join("mementum.db");
    common::intermediate_database(&db, 3);
    // La copie que le binaire intermédiaire a écrite avant sa `0003`.
    let resume = data.join("mementum.db.avant-0003.bak");
    std::fs::write(&resume, b"copie d'avant la 0003").unwrap();
    let bytes = std::fs::read(&db).unwrap();

    let stderr = refused_start(&data, &[], &[("RUST_LOG", "info")]);
    assert_eq!(
        stderr,
        format!(
            "mementum : impossible d'ouvrir la base {} : la migration 0003 de cette base n'est \
             pas celle de ce binaire : un binaire intermédiaire, qui en avait une autre version, \
             l'a appliquée. La base n'a pas été modifiée, et aucune copie n'a été écrite. \
             Reprends la copie d'avant la migration 0003, {} ; ce qui a été écrit dans la base \
             depuis cette copie est perdu. Voir « Copies avant migration » dans le README.\n",
            db.display(),
            resume.display()
        )
    );
    assert_eq!(std::fs::read(&db).unwrap(), bytes, "la base a changé");
    assert_eq!(
        entries(&data),
        ["mementum.db", "mementum.db.avant-0003.bak"]
    );
}

/// Ruling R842 : une erreur du démarrage qui recopie le message de sa source
/// (la copie d'avant migration impossible, ici) n'est écrite qu'une fois.
#[test]
fn a_startup_error_is_written_once() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    std::fs::create_dir_all(&data).unwrap();
    common::lot1_database(&data.join("mementum.db"));
    // Un dossier au nom temporaire de la copie : elle ne peut pas s'écrire.
    std::fs::create_dir(data.join("mementum.db.avant-0002.bak.partiel")).unwrap();

    let stderr = refused_start(&data, &[], &[("RUST_LOG", "info")]);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert_eq!(
        stderr
            .matches("impossible d'écrire la copie de la base")
            .count(),
        1,
        "{stderr}"
    );
    assert!(
        stderr.starts_with(&format!(
            "mementum : impossible d'ouvrir la base {} : erreur de base de données : ",
            data.join("mementum.db").display()
        )),
        "{stderr}"
    );
}

/// `--signup` et `--public-url` règlent le serveur, et l'emportent sur les
/// variables.
#[test]
fn signup_and_public_url_options() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    let info = |server: &Server| -> serde_json::Value {
        let (status, body) = server.http("GET", "/api/v1/info", None);
        assert_eq!(status, 200, "{body}");
        serde_json::from_str(&body).unwrap()
    };

    // Par défaut : sur invitation.
    let server = Server::start(&data);
    assert_eq!(info(&server)["signup"], "invite");
    assert_eq!(info(&server)["setup"], true);
    server.kill();

    // La variable, puis l'option qui l'emporte.
    let server = Server::start_with_env(&data, &[("MEMENTUM_SIGNUP", "open")]);
    assert_eq!(info(&server)["signup"], "open");
    server.kill();
    let server = Server::start_with_args(
        &data,
        &[
            "--signup",
            "open",
            "--public-url",
            "https://notes.exemple.fr",
        ],
        &[
            ("MEMENTUM_SIGNUP", "nimporte"),
            ("MEMENTUM_PUBLIC_URL", "nimporte"),
        ],
    );
    assert_eq!(info(&server)["signup"], "open");
    // L'adresse publique en https : le cookie de session porte `Secure`.
    let body = json!({ "email": "alice@exemple.fr", "name": "Alice", "password": PASSWORD });
    let reply = server.request("POST", "/api/v1/auth/signup", Some(&body.to_string()), &[]);
    assert_eq!(reply.status, 201, "{}", reply.body);
    let set_cookie = reply
        .headers
        .iter()
        .find(|(name, _)| name == "set-cookie")
        .map(|(_, value)| value.clone())
        .expect("Set-Cookie");
    assert!(set_cookie.contains("; Secure"), "{set_cookie}");
    server.kill();

    // Une valeur refusée arrête le démarrage, avec un message en français.
    for (args, expected) in [
        (["--signup", "ouvert"], "open ou invite"),
        (
            ["--public-url", "notes.exemple.fr"],
            "adresse publique invalide",
        ),
    ] {
        let stderr = refused_start(&data, &args, &[]);
        assert!(
            stderr.contains(expected),
            "« {expected} » attendu : {stderr}"
        );
    }
}

/// L'intervalle des versions (`--version-every-secs`, `MEMENTUM_VERSION_EVERY_SECS`) : une
/// valeur refusée arrête le démarrage en nommant le réglage et la valeur, en français ; une
/// option valide l'emporte sur une variable invalide.
#[test]
fn invalid_version_interval_stops_startup() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    /// Les options de `serve`, les variables d'environnement, et ce que l'erreur doit dire.
    type Case<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)], &'a str);
    let cases: [Case; 2] = [
        (
            &["--version-every-secs", "0"],
            &[],
            "--version-every-secs invalide (« 0 »)",
        ),
        (
            &[],
            &[("MEMENTUM_VERSION_EVERY_SECS", "dix")],
            "MEMENTUM_VERSION_EVERY_SECS invalide (« dix »)",
        ),
    ];
    for (args, vars, expected) in cases {
        let stderr = refused_start(&data, args, vars);
        assert!(
            stderr.contains(expected),
            "« {expected} » attendu : {stderr}"
        );
        assert!(stderr.contains("1 au moins"), "{stderr}");
    }
    let server = Server::start_with_args(
        &data,
        &["--version-every-secs", "1"],
        &[("MEMENTUM_VERSION_EVERY_SECS", "nimporte")],
    );
    assert_eq!(server.http("GET", "/health", None), (200, "ok".to_string()));
}

/// La taille la plus grande d'un fichier envoyé (`--max-file-mb`, `MEMENTUM_MAX_FILE_MB`) : une
/// valeur refusée arrête le démarrage en nommant le réglage et la valeur, en français ; une option
/// valide l'emporte sur une variable invalide, et la route des bornes la dit.
#[test]
fn file_bound_option_sets_the_limit_and_refuses_what_is_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    /// Les options de `serve`, les variables d'environnement, et ce que l'erreur doit dire.
    type Case<'a> = (&'a [&'a str], &'a [(&'a str, &'a str)], &'a str);
    let cases: [Case; 2] = [
        (
            &["--max-file-mb", "0"],
            &[],
            "--max-file-mb invalide (« 0 »)",
        ),
        (
            &[],
            &[("MEMENTUM_MAX_FILE_MB", "4097")],
            "MEMENTUM_MAX_FILE_MB invalide (« 4097 »)",
        ),
    ];
    for (args, vars, expected) in cases {
        let stderr = refused_start(&data, args, vars);
        assert!(
            stderr.contains(expected),
            "« {expected} » attendu : {stderr}"
        );
        assert!(stderr.contains("de 1 à 4096"), "{stderr}");
    }
    let server = Server::start_with_args(
        &data,
        &["--max-file-mb", "1"],
        &[("MEMENTUM_MAX_FILE_MB", "nimporte")],
    );
    let cookie = sign_up_alice(&server);
    let reply = server.request("GET", "/api/v1/files/limits", None, &[("Cookie", &cookie)]);
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&reply.body).unwrap(),
        json!({ "maxFileBytes": 1_048_576 })
    );
}
