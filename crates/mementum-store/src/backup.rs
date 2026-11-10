//! Copie de la base avant toute migration en attente (conception, §7.4 : « Migrations de schéma
//! automatiques au démarrage, avec sauvegarde préalable »).
//!
//! La copie est écrite par SQLite lui-même (`VACUUM INTO`) : elle contient aussi ce qui n'est encore
//! que dans le fichier `-wal`, et se lit seule, sans `-wal` ni `-shm`. Elle n'est jamais écrasée ni
//! supprimée : si elle existe déjà (montée de version interrompue puis relancée), c'est elle qui
//! porte l'état d'avant.

use std::collections::HashSet;
use std::fmt;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use sqlx::migrate::Migrator;

use crate::StoreError;

/// La copie de la base qui précède les migrations d'une ouverture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backup {
    /// Le fichier de la copie.
    pub path: PathBuf,
    /// Sa taille, en octets.
    pub bytes: u64,
    /// La copie était déjà là : elle a été gardée telle quelle, pas réécrite.
    pub kept: bool,
}

/// Nom de la copie : `<fichier>.avant-<version>.bak`, où `<version>` est la version de la première
/// migration en attente, sur 4 chiffres (`mementum.db.avant-0002.bak`).
pub fn backup_path(db: &Path, first_pending: i64) -> PathBuf {
    let mut name = db.as_os_str().to_owned();
    name.push(format!(".avant-{first_pending:04}.bak"));
    PathBuf::from(name)
}

/// À appeler juste avant de lancer `migrator` sur la base `db`, ouverte par `pool` : si au moins
/// une migration est en attente, écrit la copie (ou garde celle qui est déjà là) et la rend.
///
/// Rien n'est copié pour une base neuve (sans table `_sqlx_migrations`), pour une base à jour, ni
/// pour une base qui porte une migration que `migrator` ne connaît pas (écrite par une version plus
/// récente) : sqlx refuse alors de la migrer, et c'est son erreur que l'appelant rend.
pub(crate) async fn before_migrations(
    pool: &SqlitePool,
    db: &Path,
    migrator: &Migrator,
) -> Result<Option<Backup>, StoreError> {
    let Some(first) = first_pending(pool, migrator).await? else {
        return Ok(None);
    };
    let path = backup_path(db, first);
    if let Ok(meta) = std::fs::metadata(&path)
        && meta.is_file()
    {
        return Ok(Some(Backup {
            path,
            bytes: meta.len(),
            kept: true,
        }));
    }
    // Un dossier ou autre chose qu'un fichier à ce nom : `VACUUM INTO` échoue, et l'ouverture avec.
    let target = path
        .to_str()
        .ok_or_else(|| failed(&path, first, "le chemin n'est pas en UTF-8"))?;
    sqlx::query("VACUUM INTO ?")
        .bind(target)
        .execute(pool)
        .await
        .map_err(|e| failed(&path, first, e))?;
    // SQLite ne force pas la copie sur le disque : elle doit y être avant que la migration écrive.
    let file = OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|e| failed(&path, first, e))?;
    file.sync_all().map_err(|e| failed(&path, first, e))?;
    let bytes = file.metadata().map_err(|e| failed(&path, first, e))?.len();
    Ok(Some(Backup {
        path,
        bytes,
        kept: false,
    }))
}

/// La version de la première migration de `migrator` que la base n'a pas encore, comme
/// `Migrator::run` les voit (toute ligne de `_sqlx_migrations` compte pour appliquée).
async fn first_pending(pool: &SqlitePool, migrator: &Migrator) -> Result<Option<i64>, StoreError> {
    let tracked: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;
    if !tracked {
        return Ok(None);
    }
    let applied: HashSet<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations")
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
    if applied.iter().any(|v| !migrator.version_exists(*v)) {
        return Ok(None);
    }
    Ok(migrator
        .iter()
        .filter(|m| !m.migration_type.is_down_migration())
        .map(|m| m.version)
        .filter(|v| !applied.contains(v))
        .min())
}

/// L'erreur d'une copie impossible : elle nomme le fichier.
fn failed(path: &Path, first: i64, cause: impl fmt::Display) -> StoreError {
    StoreError::Sqlx(sqlx::Error::Io(io::Error::other(format!(
        "impossible d'écrire la copie de la base {} avant la migration {first:04}, rien n'a été \
         migré : {cause}",
        path.display()
    ))))
}
