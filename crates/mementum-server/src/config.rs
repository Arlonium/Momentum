//! Configuration du serveur : adresse d'écoute et dossier des données.
//!
//! Défauts : `127.0.0.1:8080` et `./data`. Les variables `MEMENTUM_BIND` et
//! `MEMENTUM_DATA_DIR` les remplacent ; les options de la ligne de commande
//! (`mementum serve --bind … --data-dir …`) l'emportent sur les variables.
//!
//! Le lot 1 n'a pas d'authentification : le serveur n'écoute jamais par
//! défaut sur une autre interface que la boucle locale.

use std::ffi::{OsStr, OsString};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

/// Adresse d'écoute par défaut : la boucle locale seulement.
pub const DEFAULT_BIND: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

/// Dossier des données par défaut, relatif au dossier courant.
pub const DEFAULT_DATA_DIR: &str = "./data";

/// Nom du fichier SQLite, dans le dossier des données.
pub const DB_FILE: &str = "mementum.db";

/// Variable d'environnement de l'adresse d'écoute.
pub const BIND_VAR: &str = "MEMENTUM_BIND";

/// Variable d'environnement du dossier des données.
pub const DATA_DIR_VAR: &str = "MEMENTUM_DATA_DIR";

/// Configuration du serveur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Adresse d'écoute. Un port 0 laisse le système en choisir un libre :
    /// `serve` écrit l'adresse réellement liée (Ruling R24).
    pub bind: SocketAddr,
    /// Dossier des données, créé au besoin. La base SQLite y est
    /// [`DB_FILE`].
    pub data_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind: DEFAULT_BIND,
            data_dir: PathBuf::from(DEFAULT_DATA_DIR),
        }
    }
}

impl Config {
    /// Les défauts, remplacés par `MEMENTUM_BIND` et `MEMENTUM_DATA_DIR`
    /// s'ils sont posés (une variable vide compte comme absente).
    pub fn from_env() -> anyhow::Result<Config> {
        Config::from_args(None, None)
    }

    /// Les options de la ligne de commande d'abord, puis les variables
    /// d'environnement, puis les défauts. Une variable n'est lue que si
    /// l'option manque : un `MEMENTUM_BIND` invalide n'empêche pas un
    /// `--bind` valide.
    pub fn from_args(
        bind: Option<SocketAddr>,
        data_dir: Option<PathBuf>,
    ) -> anyhow::Result<Config> {
        Config::resolve(bind, data_dir, |name| std::env::var_os(name))
    }

    /// Comme [`Config::from_args`], avec `var` pour lire les variables.
    pub fn resolve(
        bind: Option<SocketAddr>,
        data_dir: Option<PathBuf>,
        var: impl Fn(&str) -> Option<OsString>,
    ) -> anyhow::Result<Config> {
        // Vide, ou blanche quand elle est en UTF-8 : comme absente.
        let set = |name: &str| {
            var(name).filter(|v| !v.is_empty() && v.to_str().is_none_or(|s| !s.trim().is_empty()))
        };
        let bind = match bind {
            Some(bind) => bind,
            None => match set(BIND_VAR) {
                Some(raw) => parse_bind(&raw)?,
                None => DEFAULT_BIND,
            },
        };
        let data_dir = data_dir
            .or_else(|| set(DATA_DIR_VAR).map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_DIR));
        Ok(Config { bind, data_dir })
    }

    /// Chemin de la base SQLite.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join(DB_FILE)
    }
}

/// `MEMENTUM_BIND`, avec une erreur en français (sans la cause de std).
fn parse_bind(raw: &OsStr) -> anyhow::Result<SocketAddr> {
    raw.to_str()
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{BIND_VAR} invalide (« {} ») : attendu une adresse comme 127.0.0.1:8080",
                raw.to_string_lossy()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn defaults_listen_on_loopback_only() {
        let cfg = Config::resolve(None, None, vars(&[])).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.bind.to_string(), "127.0.0.1:8080");
        assert!(cfg.bind.ip().is_loopback());
        assert_eq!(cfg.data_dir, PathBuf::from("./data"));
        assert_eq!(cfg.db_path(), PathBuf::from("./data").join("mementum.db"));
    }

    #[test]
    fn variables_replace_defaults() {
        let cfg = Config::resolve(
            None,
            None,
            vars(&[
                ("MEMENTUM_BIND", "0.0.0.0:9000"),
                ("MEMENTUM_DATA_DIR", "/srv/mementum"),
            ]),
        )
        .unwrap();
        assert_eq!(cfg.bind.to_string(), "0.0.0.0:9000");
        assert_eq!(cfg.data_dir, PathBuf::from("/srv/mementum"));

        // Vides : comme absentes.
        let empty = vars(&[("MEMENTUM_BIND", " "), ("MEMENTUM_DATA_DIR", "")]);
        assert_eq!(
            Config::resolve(None, None, empty).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn options_win_over_variables() {
        let env = vars(&[
            ("MEMENTUM_BIND", "nimporte"),
            ("MEMENTUM_DATA_DIR", "/srv/ignore"),
        ]);
        let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
        // Un `MEMENTUM_BIND` invalide n'empêche pas un `--bind` valide.
        let cfg = Config::resolve(Some(bind), Some(PathBuf::from("ici")), env).unwrap();
        assert_eq!(cfg.bind, bind);
        assert_eq!(cfg.data_dir, PathBuf::from("ici"));
    }

    #[test]
    fn invalid_bind_is_an_error_in_french() {
        let err = Config::resolve(None, None, vars(&[("MEMENTUM_BIND", "localhost")])).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("MEMENTUM_BIND invalide"), "{message}");
        assert!(message.contains("localhost"), "{message}");
        assert!(!message.contains("invalid socket address"), "{message}");
    }

    /// `MEMENTUM_DATA_DIR` se lit tel quel, même hors UTF-8.
    #[test]
    fn data_dir_is_read_without_loss() {
        #[cfg(windows)]
        let raw = {
            use std::os::windows::ffi::OsStringExt;
            // « d », une moitié de paire UTF-16 seule, « x ».
            OsString::from_wide(&[0x64, 0xD800, 0x78])
        };
        #[cfg(unix)]
        let raw = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![b'd', 0xff, b'x'])
        };
        assert!(raw.to_str().is_none(), "pas de l'UTF-8");
        let expected = PathBuf::from(&raw);
        let cfg = Config::resolve(None, None, move |name| {
            (name == DATA_DIR_VAR).then(|| raw.clone())
        })
        .unwrap();
        assert_eq!(cfg.data_dir, expected);
    }
}
