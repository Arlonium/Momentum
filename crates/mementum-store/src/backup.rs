//! Copie de la base avant toute migration en attente (conception, §7.4 : « Migrations de schéma
//! automatiques au démarrage, avec sauvegarde préalable »).
//!
//! La copie est écrite par SQLite lui-même (`VACUUM INTO`) : elle contient aussi ce qui n'est encore
//! que dans le fichier `-wal`, et se lit seule, sans `-wal` ni `-shm`.
//!
//! Tant qu'une migration est en attente, chaque ouverture écrit une copie fraîche sous un nom
//! temporaire (`<copie>.partiel`), la force sur le disque, puis la range (Ruling R72) :
//! - elle est comparée, octet pour octet, à **toutes** les copies déjà rangées de cette version (le
//!   nom sans date, [`backup_path`], et les noms datés), en ne lisant que celles qui ont sa taille :
//!   si l'une est identique, la copie fraîche est supprimée, et celle-là suffit. Une boucle de
//!   redémarrages sur une migration qui échoue n'empile donc pas les copies ;
//! - sinon, elle prend le nom sans date s'il est libre, ou le premier nom daté libre,
//!   `<base>.avant-<version>.<AAAAMMJJ>T<HHMMSS>Z.bak` (heure UTC), puis `…Z-2.bak`, `…Z-3.bak`…
//!
//! Deux `VACUUM INTO` d'une même base donnent les mêmes octets (`tests/backup.rs`,
//! `same_database_gives_the_same_copy`) : c'est ce qui rend la comparaison utile.
//!
//! Une copie rangée n'est jamais écrasée ni supprimée. La copie fraîche prend son nom par
//! `hard_link`, qui refuse de façon atomique un nom déjà pris, jamais par `rename`, qui remplacerait
//! sa cible ; un système de fichiers sans liens physiques fait échouer l'ouverture. Un `.partiel`
//! est le reste d'une copie interrompue : il est supprimé juste avant la copie suivante, donc
//! seulement quand une migration est en attente. Sinon, rien n'y touche.

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
    /// `true` : une copie déjà rangée (sous le nom sans date ou un nom daté) était identique à la
    /// copie fraîche. Elle est gardée, et la copie fraîche, en double, est supprimée. `false` : la
    /// copie vient d'être écrite.
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

    // Une copie déjà rangée, identique, suffit.
    let existing = copies(db, first).map_err(|e| discard(&partial, failed(&path, first, e)))?;
    for (copy, _) in existing.iter().filter(|(_, len)| *len == bytes) {
        if same_content(&partial, copy).map_err(|e| discard(&partial, failed(copy, first, e)))? {
            discard_quietly(&partial);
            return Ok(Some(Backup {
                path: copy.clone(),
                bytes,
                kept: true,
            }));
        }
    }
    let undated_free = !existing
        .iter()
        .any(|(copy, _)| copy.file_name() == path.file_name());
    match place_copy(&partial, db, first, unix_secs(), undated_free) {
        Ok(Placed::Fresh(path)) => Ok(Some(Backup {
            path,
            bytes,
            kept: false,
        })),
        Ok(Placed::Identical(path)) => {
            discard_quietly(&partial);
            Ok(Some(Backup {
                path,
                bytes,
                kept: true,
            }))
        }
        Err((name, e)) => Err(discard(&partial, failed(&name, first, e))),
    }
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

/// Les copies déjà rangées de `db` d'avant la migration `first` (le nom sans date et les noms
/// datés), avec leur taille, rangées par nom. La taille vient du dossier, sans ouvrir les fichiers ;
/// seuls les fichiers comptent, ni les dossiers ni les liens symboliques.
fn copies(db: &Path, first: i64) -> io::Result<Vec<(PathBuf, u64)>> {
    let parent = db.parent().unwrap_or(Path::new(""));
    let Some(base) = db.file_name().and_then(|n| n.to_str()) else {
        return Ok(Vec::new());
    };
    let prefix = format!("{base}.avant-{first:04}.");
    let listed = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let mut found = Vec::new();
    for entry in fs::read_dir(listed)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !is_copy_name(name, &prefix) {
            continue;
        }
        // `DirEntry::metadata` ne suit pas les liens symboliques.
        let meta = entry.metadata()?;
        if meta.is_file() {
            found.push((parent.join(name), meta.len()));
        }
    }
    found.sort();
    Ok(found)
}

/// `name` est-il celui d'une copie rangée, `<prefix>bak` ou `<prefix><AAAAMMJJ>T<HHMMSS>Z[-<n>].bak` ?
fn is_copy_name(name: &str, prefix: &str) -> bool {
    let Some(rest) = name
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix("bak"))
    else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    let Some(rest) = rest.strip_suffix('.') else {
        return false;
    };
    let (stamp, n) = match rest.split_once('-') {
        Some((stamp, n)) => (stamp, Some(n)),
        None => (rest, None),
    };
    let is_stamp = stamp.len() == 16
        && stamp.bytes().enumerate().all(|(i, c)| match i {
            8 => c == b'T',
            15 => c == b'Z',
            _ => c.is_ascii_digit(),
        });
    is_stamp && n.is_none_or(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}

/// Où la copie fraîche a été rangée.
#[derive(Debug, PartialEq, Eq)]
enum Placed {
    /// Sous ce nom, qui était libre.
    Fresh(PathBuf),
    /// Nulle part : le nom sans date a été pris entre-temps par une copie identique, qui suffit.
    /// Le `.partiel` reste à supprimer.
    Identical(PathBuf),
}

/// Pose la copie fraîche `partial` sous un nom libre, sans jamais remplacer un fichier : le nom
/// sans date d'abord si `undated_free` le dit libre, puis le premier nom daté libre à l'instant
/// `at`. Si le nom sans date se révèle pris (un autre processus, entre-temps), c'est qu'une copie
/// existe déjà : identique, elle suffit ; sinon, la copie fraîche prend un nom daté. Sur un échec,
/// rend le nom en cause.
fn place_copy(
    partial: &Path,
    db: &Path,
    first: i64,
    at: u64,
    undated_free: bool,
) -> Result<Placed, (PathBuf, io::Error)> {
    if undated_free {
        let undated = backup_path(db, first);
        match link_durably(partial, &undated) {
            Ok(()) => return Ok(Placed::Fresh(undated)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let taken = fs::symlink_metadata(&undated).map_err(|e| (undated.clone(), e))?;
                if !taken.is_file() {
                    let cause = io::Error::other("ce nom est pris par autre chose qu'un fichier");
                    return Err((undated, cause));
                }
                if same_content(partial, &undated).map_err(|e| (undated.clone(), e))? {
                    return Ok(Placed::Identical(undated));
                }
            }
            Err(e) => return Err((undated, e)),
        }
    }
    let stamp = utc_stamp(at);
    let mut n = 1;
    loop {
        let dated = dated_path(db, first, &stamp, n);
        match link_durably(partial, &dated) {
            Ok(()) => return Ok(Placed::Fresh(dated)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err((dated, e)),
        }
    }
}

/// Donne à `partial` le nom `target`, qui doit être libre : `hard_link` échoue (`AlreadyExists`)
/// plutôt que de remplacer un fichier. Puis le `.partiel` est supprimé (s'il reste, la tentative
/// suivante le supprimera) et, sous Unix, le dossier est forcé sur le disque : sans cela, une
/// coupure peut faire perdre le nouveau nom.
fn link_durably(partial: &Path, target: &Path) -> io::Result<()> {
    fs::hard_link(partial, target)?;
    discard_quietly(partial);
    #[cfg(unix)]
    {
        let dir = target
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// `<base>.avant-<version>.<stamp>.bak`, ou `….<stamp>-<n>.bak` à partir de `n = 2`.
fn dated_path(db: &Path, first: i64, stamp: &str, n: u32) -> PathBuf {
    let suffix = if n == 1 {
        String::new()
    } else {
        format!("-{n}")
    };
    with_suffix(db, &format!(".avant-{first:04}.{stamp}{suffix}.bak"))
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

/// Supprime le nom `partial`, sans erreur : un `.partiel` qui resterait serait supprimé à la
/// tentative suivante. Après `hard_link`, seul ce nom part ; la copie rangée reste.
fn discard_quietly(partial: &Path) {
    let _ = fs::remove_file(partial);
}

/// Supprime la copie fraîche `partial`, qui n'a pas trouvé sa place, et rend `error`.
fn discard(partial: &Path, error: StoreError) -> StoreError {
    discard_quietly(partial);
    error
}

/// L'erreur d'une copie impossible : elle nomme le fichier en cause (la copie temporaire, une copie
/// déjà rangée, ou le nom où elle devait aller).
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

    /// La pose de la copie au niveau de [`place_copy`] : des noms pris entre le contrôle et la pose
    /// (par un autre processus) ne sont jamais remplacés ; la copie prend le nom suivant.
    #[test]
    fn a_name_taken_meanwhile_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("mementum.db");
        let partial = dir.path().join("mementum.db.avant-0002.bak.partiel");
        let at = 1_762_739_619;
        let name = |n: &str| dir.path().join(format!("mementum.db.avant-0002.{n}"));

        // Le nom daté de cet instant est pris : la copie prend `-2`.
        fs::write(&partial, b"copie fraiche").unwrap();
        fs::write(name("20251110T015339Z.bak"), b"deja la").unwrap();
        assert_eq!(
            place_copy(&partial, &db, 2, at, false).unwrap(),
            Placed::Fresh(name("20251110T015339Z-2.bak"))
        );
        assert_eq!(fs::read(name("20251110T015339Z.bak")).unwrap(), b"deja la");
        assert_eq!(
            fs::read(name("20251110T015339Z-2.bak")).unwrap(),
            b"copie fraiche"
        );
        assert!(!partial.exists());

        // Le nom sans date, que le contrôle croyait libre, a été pris par une autre copie : elle
        // reste, et la copie fraîche prend le premier nom daté libre (`-3`).
        fs::write(&partial, b"copie fraiche 2").unwrap();
        fs::write(name("bak"), b"autre copie").unwrap();
        assert_eq!(
            place_copy(&partial, &db, 2, at, true).unwrap(),
            Placed::Fresh(name("20251110T015339Z-3.bak"))
        );
        assert_eq!(fs::read(name("bak")).unwrap(), b"autre copie");
        assert!(!partial.exists());

        // Pris par une copie identique : elle suffit, rien n'est rangé.
        fs::write(&partial, b"autre copie").unwrap();
        assert_eq!(
            place_copy(&partial, &db, 2, at, true).unwrap(),
            Placed::Identical(name("bak"))
        );
        assert!(!name("20251110T015339Z-4.bak").exists());

        // Pris par un dossier : ce n'est pas une copie, la pose échoue en le nommant.
        fs::remove_file(name("bak")).unwrap();
        fs::create_dir(name("bak")).unwrap();
        let (failed_on, _) = place_copy(&partial, &db, 2, at, true).unwrap_err();
        assert_eq!(failed_on, name("bak"));
    }

    #[test]
    fn copy_names_are_recognized() {
        let prefix = "mementum.db.avant-0002.";
        for name in [
            "mementum.db.avant-0002.bak",
            "mementum.db.avant-0002.20251110T015339Z.bak",
            "mementum.db.avant-0002.20251110T015339Z-2.bak",
            "mementum.db.avant-0002.20251110T015339Z-17.bak",
        ] {
            assert!(is_copy_name(name, prefix), "{name}");
        }
        for name in [
            "mementum.db",
            "mementum.db.avant-0002.bak.partiel",
            "mementum.db.avant-0003.bak",
            "mementum.db.avant-0002.20251110T015339Z-.bak",
            "mementum.db.avant-0002.20251110T015339Z-x.bak",
            "mementum.db.avant-0002.2025111T015339Z.bak",
            "mementum.db.avant-0002.copie.bak",
            "mementum.db.avant-0002.xbak",
        ] {
            assert!(!is_copy_name(name, prefix), "{name}");
        }
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
