//! Le binaire `mementum`.
//!
//! - `mementum serve [--bind <adresse>] [--data-dir <dossier>] [--signup
//!   <open|invite>] [--public-url <url>] [--version-every-secs <secondes>]
//!   [--max-file-mb <Mio>]` lance le serveur. Les options l'emportent sur
//!   `MEMENTUM_BIND`, `MEMENTUM_DATA_DIR`, `MEMENTUM_SIGNUP`,
//!   `MEMENTUM_PUBLIC_URL`, `MEMENTUM_VERSION_EVERY_SECS` et
//!   `MEMENTUM_MAX_FILE_MB`, qui l'emportent sur les défauts (`127.0.0.1:8080`,
//!   `./data`, `invite`, pas d'adresse publique, 600 s, 100 Mio).
//!   Arrêté par Ctrl-C (ou `SIGTERM`), le serveur laisse les pages ouvertes
//!   garder leur dernière version avant de fermer la base.
//! - `mementum user set-password --email <adresse> [--data-dir <dossier>]`
//!   donne un nouveau mot de passe à un compte : l'issue d'un mot de passe
//!   oublié, faute de SMTP. Le mot de passe est la première ligne de l'entrée
//!   standard ; il n'est jamais affiché (la commande dit sa longueur), et un
//!   caractère de contrôle le fait refuser.
//! - `mementum reindex [--text-only] [--data-dir <dossier>] [--public-url <url>]`
//!   reconstruit l'index de recherche, **serveur arrêté** : la projection de
//!   chaque page relue depuis son document, puis l'index plein texte
//!   (`<données>/search/`) depuis la projection ; `--text-only` ne refait que
//!   l'index plein texte. Un serveur qui tourne sur ces données la fait
//!   refuser, sans rien modifier. `--public-url` (ou `MEMENTUM_PUBLIC_URL`) est
//!   l'adresse publique du serveur, la même que pour `serve` (module
//!   `reindex`).
//!
//! Les journaux vont sur la sortie d'erreur, au niveau `info` par défaut,
//! réglable par `RUST_LOG` (par exemple `RUST_LOG=mementum_sync=debug`). La
//! sortie standard de `serve` ne porte que la ligne `mementum écoute sur
//! http://…`.

mod reindex;

use std::io::{BufRead, IsTerminal};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use mementum_server::{Backup, Config, SignupMode, config};
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
        /// Intervalle des versions d'une page, en secondes, 1 au moins : une
        /// version est gardée avant l'écriture qui suit de ce délai la
        /// première écriture pas encore versionnée (défaut :
        /// MEMENTUM_VERSION_EVERY_SECS, sinon 600, dix minutes).
        #[arg(long, value_name = "SECONDES")]
        version_every_secs: Option<String>,
        /// Taille la plus grande d'un fichier envoyé dans une page, en Mio, de 1 à 4096 (défaut :
        /// MEMENTUM_MAX_FILE_MB, sinon 100). Au-delà, l'envoi est refusé.
        #[arg(long, value_name = "MIO")]
        max_file_mb: Option<String>,
    },
    /// Gère les comptes de l'instance.
    User {
        #[command(subcommand)]
        command: UserCommand,
    },
    /// Reconstruit l'index de recherche, serveur arrêté : la projection de chaque page relue
    /// depuis son document, puis l'index plein texte depuis la projection. Refuse de tourner
    /// pendant qu'un serveur tient ces données, sans rien avoir modifié.
    Reindex {
        /// Ne refait que l'index plein texte (`<données>/search/`), depuis la projection que la
        /// base garde, sans relire aucun document.
        #[arg(long)]
        text_only: bool,
        /// Dossier des données du serveur (défaut : MEMENTUM_DATA_DIR, sinon ./data).
        #[arg(long, value_name = "DOSSIER")]
        data_dir: Option<PathBuf>,
        /// Adresse publique du serveur, la même que `serve --public-url` (défaut :
        /// MEMENTUM_PUBLIC_URL) : un lien qui la porte et mène à une page compte comme un
        /// lien vers elle. Sans effet, et refusée, avec `--text-only`.
        // Le refus est écrit par `run`, en français, comme les autres refus de la commande : celui
        // de clap (`conflicts_with`) est en anglais (recette du lot 4, G3).
        #[arg(long, value_name = "URL")]
        public_url: Option<String>,
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
            eprintln!("mementum : {}", report(&e));
            ExitCode::FAILURE
        }
    }
}

/// Le message d'une erreur suivi de ceux de ses causes, séparés par « : », chaque texte une fois
/// (Ruling R842) : une cause que le message recopie déjà (le message lui est égal, ou finit par un
/// deux-points, précédé ou non d'une espace, puis une espace et elle) n'est pas répétée, et une
/// cause vide n'est pas écrite (Ruling R852).
/// Une erreur du store (`StoreError::Sqlx`) et celles de sqlx recopient le message de leur source
/// après un deux-points, et la rendent aussi comme cause : `{:#}` d'anyhow écrivait ainsi le même
/// texte jusqu'à trois fois.
fn report(error: &anyhow::Error) -> String {
    let mut message = String::new();
    for cause in error.chain() {
        let text = cause.to_string();
        let copied = message == text
            || message
                .strip_suffix(text.as_str())
                .is_some_and(|before| before.ends_with(": "));
        if text.is_empty() || copied {
            continue;
        }
        if !message.is_empty() {
            message.push_str(" : ");
        }
        message.push_str(&text);
    }
    message
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve {
            bind,
            data_dir,
            signup,
            public_url,
            version_every_secs,
            max_file_mb,
        } => {
            // Les options d'abord : une variable n'est lue que si l'option manque.
            let cfg = Config::from_args(
                bind,
                data_dir,
                signup,
                public_url,
                version_every_secs,
                max_file_mb,
            )?;
            mementum_server::serve(cfg).await
        }
        Command::User {
            command: UserCommand::SetPassword { email, data_dir },
        } => {
            let data_dir = config::data_dir(data_dir);
            let password = read_password()?;
            let set = mementum_server::set_password(&data_dir, &email, &password, announce_backup)
                .await?;
            // La longueur, jamais le mot de passe : de quoi voir qu'une ligne
            // tronquée ou un caractère en trop s'est glissé dans la saisie. Les
            // jetons d'API révoqués, comptés (Ruling R1221) : ceux qu'un agent
            // portait ne valent plus.
            println!(
                "mot de passe de {} caractères enregistré pour {} ; toutes ses sessions sont \
                 fermées, et {}",
                password.chars().count(),
                set.email,
                revoked_tokens(set.tokens_revoked)
            );
            Ok(())
        }
        Command::Reindex {
            text_only,
            data_dir,
            public_url,
        } => {
            if text_only && public_url.is_some() {
                anyhow::bail!(
                    "--text-only ne prend pas --public-url : l'adresse publique ne sert qu'à \
                     relire les documents des pages, ce que --text-only ne fait pas ; retire l'une \
                     des deux options. Rien n'a été modifié"
                );
            }
            let data_dir = config::data_dir(data_dir);
            // Avant de toucher au disque. Sous `--text-only`, aucune page n'est projetée : la
            // variable n'est pas lue.
            let origin = if text_only {
                None
            } else {
                public_origin(public_url)?
            };
            // Dite au résumé : sans elle, les liens écrits avec l'adresse entière de l'instance ne
            // sont plus des rétroliens (relecture de S7, I1).
            let without_origin = !text_only && origin.is_none();
            let address = match &origin {
                Some(origin) => format!("adresse publique : {origin}"),
                None => "sans adresse publique".to_string(),
            };
            let report = reindex::reindex(&data_dir, text_only, origin, announce_backup).await?;
            println!("{}", summary(&address, &report));
            if without_origin {
                // Sur la sortie d'erreur, quel que soit le niveau des journaux, après le résumé.
                eprintln!(
                    "mementum : attention : aucune adresse publique (ni --public-url, ni \
                     MEMENTUM_PUBLIC_URL) : un lien écrit avec l'adresse entière de l'instance \
                     (https://…/p/…) ne comptera pas comme rétrolien de sa page ; si le serveur a \
                     une adresse publique, relance la commande avec --public-url"
                );
            }
            Ok(())
        }
    }
}

/// Le résumé de `mementum reindex`, sur la sortie standard, accordé au nombre : zéro et un au
/// singulier, comme en français (recette du lot 4, G2). `address` dit l'adresse publique retenue.
fn summary(address: &str, report: &reindex::Report) -> String {
    let written = format!(
        "{} dans l'index de recherche ({})",
        agreed(report.entries, "entrée écrite", "entrées écrites"),
        agreed(report.commits, "validation", "validations")
    );
    let reread = |pages: u64| {
        agreed(
            pages,
            "page relue depuis son document",
            "pages relues depuis leur document",
        )
    };
    match report.pages {
        Some((projected, 0)) => format!(
            "réindexation finie ({address}) : {}, {written}",
            agreed(
                projected,
                "page relue depuis son document et projetée",
                "pages relues depuis leur document et projetées",
            )
        ),
        Some((projected, unreadable)) => {
            let total = projected + unreadable;
            let lost = match (total, unreadable) {
                (1, _) => "elle ne se relit pas : elle sort de l'index, et le journal la nomme"
                    .to_string(),
                (_, 1) => "1 d'entre elles ne se relit pas : elle sort de l'index, et le journal \
                           la nomme"
                    .to_string(),
                _ => format!(
                    "{unreadable} d'entre elles ne se relisent pas : elles sortent de l'index, et \
                     le journal les nomme"
                ),
            };
            format!(
                "réindexation finie ({address}) : {}, {written} ; {lost}",
                reread(total)
            )
        }
        None => format!(
            "index de recherche refait depuis les tables, sans relire aucun document : {written}"
        ),
    }
}

/// `count` suivi de `one` (zéro ou un) ou de `many`.
fn agreed(count: u64, one: &str, many: &str) -> String {
    format!("{count} {}", if count <= 1 { one } else { many })
}

/// Les jetons d'API qu'un changement de mot de passe a révoqués (Ruling R1221), accordés au
/// nombre.
fn revoked_tokens(count: u64) -> String {
    match count {
        0 => "aucun jeton d'API n'était à révoquer".to_string(),
        _ => format!(
            "{} : un agent qui s'en servait demande un jeton neuf",
            agreed(count, "jeton d'API révoqué", "jetons d'API révoqués")
        ),
    }
}

/// La commande vient de migrer la base : dire où est celle d'avant, sur la sortie d'erreur,
/// quel que soit le niveau des journaux.
fn announce_backup(backup: &Backup) {
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
}

/// L'origine publique du serveur, pour `mementum reindex` : l'option, sinon
/// `MEMENTUM_PUBLIC_URL` (vide ou blanche : absente), relue par `config::public_origin`, la règle
/// du serveur (Ruling R998). Une autre variable du serveur, même invalide, n'arrête pas la
/// commande.
fn public_origin(option: Option<String>) -> anyhow::Result<Option<Arc<str>>> {
    let url = match option {
        Some(url) => Some(url),
        None => match std::env::var(config::PUBLIC_URL_VAR) {
            Ok(url) => Some(url).filter(|url| !url.trim().is_empty()),
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(raw)) => anyhow::bail!(
                "{} invalide (« {} ») : attendu une adresse en UTF-8, comme \
                 https://notes.exemple.fr",
                config::PUBLIC_URL_VAR,
                raw.to_string_lossy()
            ),
        },
    };
    url.map(|url| config::public_origin(&url).map(Arc::from))
        .transpose()
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

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::{report, revoked_tokens, summary};
    use crate::reindex::Report;

    /// `user set-password` dit combien de jetons d'API il a révoqués (Task S6, Ruling R1221),
    /// accordé au nombre.
    #[test]
    fn revoked_tokens_agree_with_their_count() {
        assert_eq!(revoked_tokens(0), "aucun jeton d'API n'était à révoquer");
        assert_eq!(
            revoked_tokens(1),
            "1 jeton d'API révoqué : un agent qui s'en servait demande un jeton neuf"
        );
        assert_eq!(
            revoked_tokens(3),
            "3 jetons d'API révoqués : un agent qui s'en servait demande un jeton neuf"
        );
    }

    fn done(pages: Option<(u64, u64)>, entries: u64, commits: u64) -> String {
        summary(
            "adresse publique : https://notes.exemple.fr",
            &Report {
                pages,
                entries,
                commits,
            },
        )
    }

    /// Recette du lot 4, G2 : le résumé s'accorde au nombre, zéro et un au singulier, comme en
    /// français ; il disait « (1 validations) ».
    #[test]
    fn the_summary_agrees_with_its_counts() {
        assert_eq!(
            done(Some((1, 0)), 1, 1),
            "réindexation finie (adresse publique : https://notes.exemple.fr) : 1 page relue \
             depuis son document et projetée, 1 entrée écrite dans l'index de recherche (1 \
             validation)"
        );
        assert_eq!(
            done(Some((9, 0)), 194, 2),
            "réindexation finie (adresse publique : https://notes.exemple.fr) : 9 pages relues \
             depuis leur document et projetées, 194 entrées écrites dans l'index de recherche (2 \
             validations)"
        );
        assert_eq!(
            done(Some((0, 0)), 0, 0),
            "réindexation finie (adresse publique : https://notes.exemple.fr) : 0 page relue \
             depuis son document et projetée, 0 entrée écrite dans l'index de recherche (0 \
             validation)"
        );
        // Des pages qui ne se relisent pas : une seule, puis plusieurs.
        assert_eq!(
            done(Some((0, 1)), 0, 1),
            "réindexation finie (adresse publique : https://notes.exemple.fr) : 1 page relue \
             depuis son document, 0 entrée écrite dans l'index de recherche (1 validation) ; elle \
             ne se relit pas : elle sort de l'index, et le journal la nomme"
        );
        assert_eq!(
            done(Some((2, 1)), 5, 1),
            "réindexation finie (adresse publique : https://notes.exemple.fr) : 3 pages relues \
             depuis leur document, 5 entrées écrites dans l'index de recherche (1 validation) ; 1 \
             d'entre elles ne se relit pas : elle sort de l'index, et le journal la nomme"
        );
        assert_eq!(
            done(Some((1, 2)), 5, 2),
            "réindexation finie (adresse publique : https://notes.exemple.fr) : 3 pages relues \
             depuis leur document, 5 entrées écrites dans l'index de recherche (2 validations) ; 2 \
             d'entre elles ne se relisent pas : elles sortent de l'index, et le journal les nomme"
        );
        // `--text-only`.
        assert_eq!(
            done(None, 1, 1),
            "index de recherche refait depuis les tables, sans relire aucun document : 1 entrée \
             écrite dans l'index de recherche (1 validation)"
        );
        assert_eq!(
            done(None, 194, 3),
            "index de recherche refait depuis les tables, sans relire aucun document : 194 \
             entrées écrites dans l'index de recherche (3 validations)"
        );
    }

    /// Une erreur qui recopie le message de sa source, et la rend comme cause, à la manière de
    /// `StoreError::Sqlx`.
    #[derive(Debug)]
    struct Copying(Option<Box<Copying>>, &'static str);

    impl fmt::Display for Copying {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match &self.0 {
                Some(source) => write!(f, "{} : {source}", self.1),
                None => f.write_str(self.1),
            }
        }
    }

    impl std::error::Error for Copying {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.0.as_deref().map(|e| e as _)
        }
    }

    #[test]
    fn each_text_of_the_chain_is_written_once() {
        let copying = Copying(
            Some(Box::new(Copying(Some(Box::new(Copying(None, "c"))), "b"))),
            "a",
        );
        let error = anyhow::Error::new(copying).context("contexte");
        assert_eq!(report(&error), "contexte : a : b : c");
        // Une cause qui ne recopie rien est écrite, à la suite.
        let error = anyhow::Error::new(Copying(None, "cause")).context("contexte");
        assert_eq!(report(&error), "contexte : cause");
        assert_eq!(report(&anyhow::anyhow!("seule")), "seule");
    }

    /// Une erreur qui ne recopie pas sa source : son texte seul.
    #[derive(Debug)]
    struct Link(&'static str, Option<Box<Link>>);

    impl fmt::Display for Link {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for Link {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1.as_deref().map(|e| e as _)
        }
    }

    /// Ruling R852 : une cause n'est tue que si le message lui est égal ou se termine par elle
    /// après un deux-points ; une cause dont le texte termine le message au milieu d'un mot est
    /// écrite, et une cause vide, qui n'est pas écrite, ne fait rien taire.
    #[test]
    fn a_cause_is_dropped_only_after_a_colon() {
        let error =
            anyhow::Error::new(Link("valide", None)).context("le jeton de session est invalide");
        assert_eq!(report(&error), "le jeton de session est invalide : valide");
        let error = anyhow::Error::new(Link("base", None)).context("erreur de base");
        assert_eq!(report(&error), "erreur de base : base");
        // Après un deux-points, avec ou sans espace avant lui : la cause recopiée est tue.
        let error = anyhow::Error::new(Link("c", None)).context("b: c");
        assert_eq!(report(&error), "b: c");
        let error = anyhow::Error::new(Link("contexte", None)).context("contexte");
        assert_eq!(report(&error), "contexte");
        // Une cause vide n'est pas écrite, et la suivante l'est.
        let error = anyhow::Error::new(Link("", Some(Box::new(Link("c", None))))).context("a");
        assert_eq!(report(&error), "a : c");
        let error = anyhow::Error::new(Link("", None)).context("contexte");
        assert_eq!(report(&error), "contexte");
    }
}
