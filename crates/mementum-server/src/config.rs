//! Configuration du serveur : adresse d'écoute et dossier des données.
//!
//! Défauts : `127.0.0.1:8080` et `./data`. Les variables `MEMENTUM_BIND` et
//! `MEMENTUM_DATA_DIR` les remplacent ; les options de la ligne de commande
//! (`mementum serve --bind … --data-dir …`) l'emportent sur les variables.
//!
//! Le lot 1 n'a pas d'authentification : le serveur n'écoute jamais par
//! défaut sur une autre interface que la boucle locale.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use anyhow::Context;

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
        Config::from_vars(|name| std::env::var_os(name).map(|v| v.to_string_lossy().into_owned()))
    }

    /// Comme [`Config::from_env`], avec `var` pour lire les variables.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> anyhow::Result<Config> {
        let mut cfg = Config::default();
        let set = |name: &str| var(name).filter(|v| !v.trim().is_empty());
        if let Some(bind) = set(BIND_VAR) {
            cfg.bind = bind.trim().parse().with_context(|| {
                format!(
                    "{BIND_VAR} invalide (« {bind} ») : attendu une adresse comme 127.0.0.1:8080"
                )
            })?;
        }
        if let Some(dir) = set(DATA_DIR_VAR) {
            cfg.data_dir = PathBuf::from(dir);
        }
        Ok(cfg)
    }

    /// Chemin de la base SQLite.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join(DB_FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn defaults_listen_on_loopback_only() {
        let cfg = Config::from_vars(vars(&[])).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.bind.to_string(), "127.0.0.1:8080");
        assert!(cfg.bind.ip().is_loopback());
        assert_eq!(cfg.data_dir, PathBuf::from("./data"));
        assert_eq!(cfg.db_path(), PathBuf::from("./data").join("mementum.db"));
    }

    #[test]
    fn variables_replace_defaults() {
        let cfg = Config::from_vars(vars(&[
            ("MEMENTUM_BIND", "0.0.0.0:9000"),
            ("MEMENTUM_DATA_DIR", "/srv/mementum"),
        ]))
        .unwrap();
        assert_eq!(cfg.bind.to_string(), "0.0.0.0:9000");
        assert_eq!(cfg.data_dir, PathBuf::from("/srv/mementum"));

        // Vides : comme absentes.
        let cfg =
            Config::from_vars(vars(&[("MEMENTUM_BIND", " "), ("MEMENTUM_DATA_DIR", "")])).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn invalid_bind_is_an_error() {
        let err = Config::from_vars(vars(&[("MEMENTUM_BIND", "localhost")])).unwrap_err();
        assert!(err.to_string().contains("MEMENTUM_BIND invalide"), "{err}");
    }
}
