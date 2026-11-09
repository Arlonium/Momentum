//! Le binaire `mementum`.
//!
//! `mementum serve [--bind <adresse>] [--data-dir <dossier>]` lance le
//! serveur. Les options l'emportent sur `MEMENTUM_BIND` et
//! `MEMENTUM_DATA_DIR`, qui l'emportent sur les défauts (`127.0.0.1:8080`,
//! `./data`).
//!
//! Les journaux vont sur la sortie d'erreur, au niveau `info` par défaut,
//! réglable par `RUST_LOG` (par exemple `RUST_LOG=mementum_sync=debug`). La
//! sortie standard ne porte que la ligne `mementum écoute sur http://…`.

use std::io::IsTerminal;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use mementum_server::Config;
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
        /// Le lot 1 n'a pas d'authentification : n'écouter que sur la boucle
        /// locale, sauf derrière un mandataire qui protège l'accès.
        #[arg(long, value_name = "ADRESSE")]
        bind: Option<SocketAddr>,
        /// Dossier des données, créé au besoin (défaut : MEMENTUM_DATA_DIR,
        /// sinon ./data). La base SQLite y est `mementum.db`.
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
        Command::Serve { bind, data_dir } => {
            // Les options d'abord : une variable n'est lue que si l'option manque.
            let cfg = Config::from_args(bind, data_dir)?;
            mementum_server::serve(cfg).await
        }
    }
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
