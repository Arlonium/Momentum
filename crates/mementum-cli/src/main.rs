//! Le binaire `mementum`.
//!
//! - `mementum serve [--bind <adresse>] [--data-dir <dossier>] [--signup
//!   <open|invite>] [--public-url <url>]` lance le serveur. Les options
//!   l'emportent sur `MEMENTUM_BIND`, `MEMENTUM_DATA_DIR`, `MEMENTUM_SIGNUP`
//!   et `MEMENTUM_PUBLIC_URL`, qui l'emportent sur les défauts
//!   (`127.0.0.1:8080`, `./data`, `invite`, pas d'adresse publique).
//! - `mementum user set-password --email <adresse> [--data-dir <dossier>]`
//!   donne un nouveau mot de passe à un compte : l'issue d'un mot de passe
//!   oublié, faute de SMTP. Le mot de passe est la première ligne de l'entrée
//!   standard ; il n'est jamais affiché (la commande dit sa longueur), et un
//!   caractère de contrôle le fait refuser.
//!
//! Les journaux vont sur la sortie d'erreur, au niveau `info` par défaut,
//! réglable par `RUST_LOG` (par exemple `RUST_LOG=mementum_sync=debug`). La
//! sortie standard de `serve` ne porte que la ligne `mementum écoute sur
//! http://…`.

use std::io::{BufRead, IsTerminal};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand};
use mementum_server::{Config, SignupMode};
use tracing_subscriber::EnvFilter;

/// Mementum : un espace de travail façon Notion, auto-hébergeable.
#[derive(Parser)]
#[command(name = "mementum", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lance le serveur (API REST et WebSocket de synchronisation).
    Serve {
        /// Adresse d'écoute (défaut : MEMENTUM_BIND, sinon 127.0.0.1:8080).
        /// Hors de la boucle locale, ce sont les comptes qui protègent
        /// l'accès : créer le premier compte avant d'exposer le serveur.
        #[arg(long, value_name = "ADRESSE")]
        bind: Option<SocketAddr>,
        /// Dossier des données, créé au besoin (défaut : MEMENTUM_DATA_DIR,
        /// sinon ./data). La base SQLite y est `mementum.db`.
        #[arg(long, value_name = "DOSSIER")]
        data_dir: Option<PathBuf>,
        /// Qui peut créer un compte : `open` (tout le monde) ou `invite`
        /// (avec le lien d'une invitation). Défaut : MEMENTUM_SIGNUP, sinon
        /// invite. Le premier compte est toujours permis, et il administre
        /// l'instance.
        #[arg(long, value_name = "MODE")]
        signup: Option<SignupMode>,
        /// Adresse sous laquelle les gens atteignent l'instance, par exemple
        /// https://notes.exemple.fr (défaut : MEMENTUM_PUBLIC_URL). En https,
        /// le cookie de session porte `Secure`.
        #[arg(long, value_name = "URL")]
        public_url: Option<String>,
    },
    /// Gère les comptes de l'instance.
    User {
        #[command(subcommand)]
        command: UserCommand,
    },
}

#[derive(Subcommand)]
enum UserCommand {
    /// Donne un nouveau mot de passe à un compte et ferme toutes ses
    /// sessions. Le mot de passe est la première ligne de l'entrée standard
    /// (10 à 200 caractères, sans caractère de contrôle) ; il n'est jamais
    /// affiché, seule sa longueur est dite. Peut tourner pendant que le
    /// serveur tourne.
    SetPassword {
        /// Adresse e-mail du compte.
        #[arg(long, value_name = "ADRESSE")]
        email: String,
        /// Dossier des données du serveur (défaut : MEMENTUM_DATA_DIR, sinon
        /// ./data).
        #[arg(long, value_name = "DOSSIER")]
        data_dir: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mementum : {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve {
            bind,
            data_dir,
            signup,
            public_url,
        } => {
            // Les options d'abord : une variable n'est lue que si l'option manque.
            let cfg = Config::from_args(bind, data_dir, signup, public_url)?;
            mementum_server::serve(cfg).await
        }
        Command::User {
            command: UserCommand::SetPassword { email, data_dir },
        } => {
            let data_dir = mementum_server::config::data_dir(data_dir);
            let password = read_password()?;
            let account = mementum_server::set_password(&data_dir, &email, &password, |backup| {
                // La commande vient de migrer la base : dire où est celle
                // d'avant, quel que soit le niveau des journaux.
                eprintln!(
                    "mementum : base migrée ; la copie d'avant migration est {} ({} octets{})",
                    backup.path.display(),
                    backup.bytes,
                    if backup.kept {
                        " ; une copie identique, d'une tentative précédente, y était déjà"
                    } else {
                        ""
                    }
                );
                if let Some(warning) = mementum_server::backup_warning(backup) {
                    eprintln!("mementum : attention : {warning}");
                }
            })
            .await?;
            // La longueur, jamais le mot de passe : de quoi voir qu'une ligne
            // tronquée ou un caractère en trop s'est glissé dans la saisie.
            println!(
                "mot de passe de {} caractères enregistré pour {account} ; toutes ses sessions \
                 sont fermées",
                password.chars().count()
            );
            Ok(())
        }
    }
}

/// Le mot de passe : la première ligne de l'entrée standard, sans sa fin de
/// ligne. Les espaces en font partie. Il n'est écrit nulle part.
fn read_password() -> anyhow::Result<String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!(
            "Nouveau mot de passe, puis Entrée. Attention : ce terminal affiche ce qui est \
             tapé ; pour l'éviter, passer le mot de passe par un tuyau ou une redirection."
        );
    }
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .context("mot de passe illisible sur l'entrée standard (attendu : une ligne en UTF-8)")?;
    let line = line.strip_suffix('\n').unwrap_or(&line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    // Une marque d'ordre d'octets en tête (fichier écrit par un éditeur)
    // n'est pas un caractère du mot de passe.
    Ok(line.strip_prefix('\u{feff}').unwrap_or(line).to_string())
}

/// Journaux sur la sortie d'erreur : `info` par défaut, ou `RUST_LOG`.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
}
