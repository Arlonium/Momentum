//! Copie de la base avant toute migration en attente (conception, §7.4 : « Migrations de schéma
//! automatiques au démarrage, avec sauvegarde préalable »).
//!
//! La copie est écrite par SQLite lui-même (`VACUUM INTO`) : elle contient aussi ce qui n'est encore
//! que dans le fichier `-wal`, et se lit seule, sans `-wal` ni `-shm`.
//!
//! Tant qu'une migration est en attente, chaque ouverture écrit une copie fraîche sous un nom
//! temporaire (`<copie>.partiel`), la force sur le disque, puis la range (Ruling R72) :
//! - si aucun fichier ne porte le nom de la copie ([`backup_path`]), elle le prend ;
//! - si un fichier le porte déjà (une tentative précédente), elle lui est comparée octet pour
//!   octet : identique, elle est supprimée, et l'ancienne suffit ; différente (la base a changé
//!   depuis), elle prend un nom daté encore libre, `<base>.avant-<version>.<AAAAMMJJ>T<HHMMSS>Z.bak`
//!   (heure UTC), suivi de `-2`, `-3`… avant `.bak` s'il est pris.
//!
//! Deux `VACUUM INTO` d'une même base donnent les mêmes octets (`tests/backup.rs`,
//! `same_database_gives_the_same_copy`) : c'est ce qui rend la comparaison utile.
//!
//! Une copie rangée n'est jamais écrasée ni supprimée. Un `.partiel` est le reste d'une copie
//! interrompue : il est supprimé juste avant la copie suivante, donc seulement quand une migration
//! est en attente. Sinon, rien n'y touche.

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use sqlx::migrate::Migrator;

use crate::StoreError;

/// La copie qui porte l'état de la base juste avant les migrations d'une ouverture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backup {
    /// Le fichier de la copie.
    pub path: PathBuf,
    /// Sa taille, en octets.
    pub bytes: u64,
    /// `true` : une copie identique à la copie fraîche portait déjà ce nom. Elle est gardée, et la
    /// copie fraîche, en double, est supprimée. `false` : la copie vient d'être écrite.
    pub kept: bool,
}

/// Nom de la copie : `<fichier>.avant-<version>.bak`, où `<version>` est la version de la première
/// migration en attente, sur 4 chiffres (`mementum.db.avant-0002.bak`).
pub fn backup_path(db: &Path, first_pending: i64) -> PathBuf {
    with_suffix(db, &format!(".avant-{first_pending:04}.bak"))
}

/// À appeler juste avant de lancer `migrator` sur la base `db`, ouverte par `pool` : si au moins
/// une migration est en attente, écrit une copie fraîche, la range, et rend la copie qui porte
/// l'état d'avant.
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
    let partial = with_suffix(&path, ".partiel");
    // Le reste d'une copie interrompue. Un dossier à ce nom n'en est pas un : il reste, et
    // l'ouverture échoue.
    match fs::remove_file(&partial) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(failed(&partial, first, e)),
        _ => {}
    }
    let bytes = write_copy(pool, &partial)
        .await
        .map_err(|e| discard(&partial, failed(&partial, first, e)))?;

    // Où ranger la copie fraîche.
    let target = match fs::metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => path,
        Ok(meta) if meta.is_file() => {
            let same = same_content(&partial, &path)
                .map_err(|e| discard(&partial, failed(&path, first, e)))?;
            if same {
                // Un `.partiel` qui resterait serait supprimé à la prochaine tentative.
                let _ = fs::remove_file(&partial);
                return Ok(Some(Backup {
                    path,
                    bytes,
                    kept: true,
                }));
            }
            free_dated_path(db, first, unix_secs())
                .map_err(|e| discard(&partial, failed(&path, first, e)))?
        }
        Ok(_) => {
            let cause = "ce nom est pris par autre chose qu'un fichier";
            return Err(discard(&partial, failed(&path, first, cause)));
        }
        Err(e) => return Err(discard(&partial, failed(&path, first, e))),
    };
    rename_durably(&partial, &target).map_err(|e| discard(&partial, failed(&target, first, e)))?;
    Ok(Some(Backup {
        path: target,
        bytes,
        kept: false,
    }))
}

/// Écrit une copie complète de la base dans `target`, forcée sur le disque, et rend sa taille.
async fn write_copy(
    pool: &SqlitePool,
    target: &Path,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let name = target.to_str().ok_or("le chemin n'est pas en UTF-8")?;
    sqlx::query("VACUUM INTO ?")
        .bind(name)
        .execute(pool)
        .await?;
    // SQLite ne force pas la copie sur le disque : elle doit y être avant que la migration écrive.
    let file = OpenOptions::new().write(true).open(target)?;
    file.sync_all()?;
    Ok(file.metadata()?.len())
}

/// Renomme `from` en `to`. Sous Unix, force aussi le dossier sur le disque : sans cela, une coupure
/// peut faire perdre le nouveau nom.
fn rename_durably(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)?;
    #[cfg(unix)]
    {
        let dir = to
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// `a` et `b` ont-ils les mêmes octets ? La taille d'abord, puis le contenu, par morceaux.
fn same_content(a: &Path, b: &Path) -> io::Result<bool> {
    const CHUNK: usize = 64 * 1024;
    let (mut fa, mut fb) = (File::open(a)?, File::open(b)?);
    if fa.metadata()?.len() != fb.metadata()?.len() {
        return Ok(false);
    }
    let (mut ba, mut bb) = (vec![0; CHUNK], vec![0; CHUNK]);
    loop {
        let n = fill(&mut fa, &mut ba)?;
        if fill(&mut fb, &mut bb)? != n || ba[..n] != bb[..n] {
            return Ok(false);
        }
        if n == 0 {
            return Ok(true);
        }
    }
}

/// Remplit `buf` autant que `file` le permet ; rend le nombre d'octets lus (moins à la fin).
fn fill(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Le premier nom daté libre pour une copie de `db` d'avant la migration `first`, à l'instant
/// `unix_secs` : `<base>.avant-<version>.<AAAAMMJJ>T<HHMMSS>Z.bak`, puis `…Z-2.bak`, `…Z-3.bak`…
fn free_dated_path(db: &Path, first: i64, unix_secs: u64) -> io::Result<PathBuf> {
    let stamp = utc_stamp(unix_secs);
    let mut n = 1;
    loop {
        let suffix = if n == 1 {
            String::new()
        } else {
            format!("-{n}")
        };
        let candidate = with_suffix(db, &format!(".avant-{first:04}.{stamp}{suffix}.bak"));
        match fs::symlink_metadata(&candidate) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Err(e) => return Err(e),
            Ok(_) => n += 1,
        }
    }
}

/// `AAAAMMJJTHHMMSSZ`, en UTC, pour `unix_secs` secondes depuis le 1970-01-01.
fn utc_stamp(unix_secs: u64) -> String {
    let (days, secs) = (unix_secs / 86_400, unix_secs % 86_400);
    // Date civile d'un nombre de jours depuis le 1970-01-01 (algorithme de Howard Hinnant, sur des
    // ères de 400 ans commençant un 1er mars).
    let z = days + 719_468;
    let (era, doe) = (z / 146_097, z % 146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    )
}

/// Secondes Unix.
fn unix_secs() -> u64 {
    u64::try_from(crate::now_ms() / 1000).unwrap_or(0)
}

/// `path` suivi de `suffix`, sans passer par une chaîne (le chemin peut ne pas être en UTF-8).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
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

/// Supprime la copie fraîche `partial`, qui n'a pas trouvé sa place, et rend `error`.
fn discard(partial: &Path, error: StoreError) -> StoreError {
    let _ = fs::remove_file(partial);
    error
}

/// L'erreur d'une copie impossible : elle nomme le fichier en cause (la copie temporaire, ou le nom
/// où elle devait aller).
fn failed(path: &Path, first: i64, cause: impl fmt::Display) -> StoreError {
    StoreError::Sqlx(sqlx::Error::Io(io::Error::other(format!(
        "impossible d'écrire la copie de la base {} avant la migration {first:04}, rien n'a été \
         migré : {cause}",
        path.display()
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_stamp_counts_days_like_the_calendar() {
        // Les valeurs attendues viennent de `datetime.fromtimestamp(s, timezone.utc)` (Python).
        for (secs, stamp) in [
            (0, "19700101T000000Z"),
            (951_782_400, "20000229T000000Z"),
            (1_709_164_800, "20240229T000000Z"),
            (1_762_739_619, "20251110T015339Z"),
            (4_102_444_799, "20991231T235959Z"),
        ] {
            assert_eq!(utc_stamp(secs), stamp, "{secs}");
        }
    }

    #[test]
    fn dated_path_skips_names_already_taken() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("mementum.db");
        let at = 1_762_739_619;
        let first = dir
            .path()
            .join("mementum.db.avant-0002.20251110T015339Z.bak");
        assert_eq!(free_dated_path(&db, 2, at).unwrap(), first);
        fs::write(&first, b"").unwrap();
        let second = dir
            .path()
            .join("mementum.db.avant-0002.20251110T015339Z-2.bak");
        assert_eq!(free_dated_path(&db, 2, at).unwrap(), second);
        fs::create_dir(&second).unwrap();
        assert_eq!(
            free_dated_path(&db, 2, at).unwrap(),
            dir.path()
                .join("mementum.db.avant-0002.20251110T015339Z-3.bak")
        );
    }

    #[test]
    fn same_content_compares_every_byte() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        // Plus d'un morceau, et une différence dans le dernier octet seulement.
        let mut bytes = vec![7u8; 200_000];
        fs::write(&a, &bytes).unwrap();
        fs::write(&b, &bytes).unwrap();
        assert!(same_content(&a, &b).unwrap());
        *bytes.last_mut().unwrap() = 8;
        fs::write(&b, &bytes).unwrap();
        assert!(!same_content(&a, &b).unwrap());
        fs::write(&b, &bytes[..199_999]).unwrap();
        assert!(!same_content(&a, &b).unwrap());
    }
}
