//! Le dépôt des fichiers (Rulings R244 à R247) : les contenus envoyés, rangés sur le disque sous le
//! SHA-256 de leurs octets, dans `<data_dir>/files/<2 premiers caractères hexadécimaux>/<les 62
//! autres>`. Un envoi s'écrit d'abord dans `files/tmp/<UUIDv7>`, en calculant son hash au fil de
//! l'eau, puis prend sa place. **Aucun nom venu d'un client n'entre dans un chemin** : le hash et
//! l'UUID sont les seuls noms que le dépôt écrit ou lit. Le nom d'un fichier n'est qu'un libellé
//! ([`display_name`]), gardé en base et rendu dans `Content-Disposition`
//! ([`content_disposition`]). Le dépôt ne sait rien des pages ni des droits : c'est le store qui
//! dit quelle page porte quel contenu, et `access.rs` qui juge qui le lit.
//!
//! Le type d'un contenu est lu dans ses premiers octets, à l'envoi, par une table fermée
//! ([`sniff`]) ; le type que déclare le client n'est jamais lu (R246).
//!
//! **Rien n'est supprimé au démarrage hors de `tmp/`** (R245). [`FileStore::open`] vide `tmp/`, et
//! rien d'autre : un contenu que la base ne connaît pas (arrêt entre la pose et l'attache, base
//! revenue en arrière) reste, compté par [`FileStore::strangers`] pour le journal du démarrage, et
//! repris tel quel si on le renvoie. Un contenu ne quitte le disque que par [`FileStore::remove`],
//! qu'appellent l'envoi dont l'attache a échoué (pour le seul contenu qu'il vient de poser,
//! [`Stored::fresh`]) et le ménage qui suit la suppression d'une page.
//!
//! **Un contenu posé est entier.** [`Upload::finish`] vide le fichier temporaire sur le disque
//! (`sync_all`) avant de le mettre à sa place. Si un fichier y est déjà : de même taille, c'est le
//! même contenu (le hash le nomme), et le temporaire est supprimé ; d'une autre taille, ou s'il
//! n'est pas un fichier ordinaire, il est abîmé, et le temporaire le remplace, ce que le journal
//! dit. Une écriture interrompue ou qui échoue met l'envoi hors d'usage (R273) : le fichier ne
//! porterait plus les octets que le hash a vus. Un envoi lâché sans `finish` supprime son
//! temporaire.
//!
//! **Un verrou** ([`FileStore::lock`]) ordonne « poser un contenu puis l'attacher » et le ménage :
//! [`Upload::finish`] et [`FileStore::remove`] demandent son garde (d'un autre dépôt, il est
//! refusé, R274). Ainsi un ménage qui relit la base sous le verrou voit l'attache d'un envoi qui
//! vient de poser le contenu qu'il allait retirer.
//!
//! **Les liens ne sont jamais suivis dans le dépôt** (R272) : un contenu n'est lu, gardé ou
//! retiré que s'il est un fichier ordinaire dans un vrai dossier de préfixe, et `tmp/`, vidé à
//! chaque démarrage, doit être un vrai dossier. Seul `files/` lui-même peut être un lien, vers
//! un autre disque.
//!
//! Tout est en `async` (`tokio::fs`), sauf l'ouverture et le compte des contenus inconnus (au
//! démarrage) et le retrait du temporaire d'un envoi lâché (`Drop`) : aucune écriture bloquante de
//! 100 Mio sur les fils du serveur, qui relaient les frappes (R247).

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::http::HeaderValue;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, MutexGuard};
use uuid::Uuid;

/// Le dossier du dépôt, dans le dossier des données.
pub(crate) const FILES_DIR: &str = "files";

/// Le dossier des envois en cours, dans le dépôt.
pub(crate) const TMP_DIR: &str = "tmp";

/// Le type de tout ce que la table de [`sniff`] ne reconnaît pas.
pub(crate) const OCTET_STREAM: &str = "application/octet-stream";

/// Le nom d'affichage d'un fichier dont le nom ne laisse rien (R247).
pub(crate) const DEFAULT_NAME: &str = "fichier";

/// Longueur maximale d'un nom d'affichage, en caractères (R247).
pub(crate) const MAX_NAME_CHARS: usize = 255;

/// Octets gardés pour reconnaître un type : le motif le plus long de la table finit au douzième.
const SNIFF_LEN: usize = 12;

/// Le tampon de tokio pour un fichier qu'on écrit (2 Mio par défaut), borné : un envoi ne tient
/// pas plus que cela de son fichier en mémoire, en plus du morceau qu'on lui donne (R276).
const WRITE_BUFFER: usize = 64 * 1024;

/// Longueur maximale de la valeur de `filename*`, encodée (R271) : avec `filename="…"` (255
/// octets au plus) et le reste de l'en-tête, `Content-Disposition` reste sous 2 Kio.
const MAX_ENCODED_NAME: usize = 1536;

/// Une extension (point compris) d'au plus tant de caractères est gardée quand le nom doit être
/// raccourci pour l'en-tête (R271).
const MAX_KEPT_EXTENSION: usize = 16;

/// Les chemins du dépôt et son verrou, partagés avec les envois en cours.
struct Dirs {
    /// `<data_dir>/files`.
    root: PathBuf,
    /// `<data_dir>/files/tmp`.
    tmp: PathBuf,
    lock: Mutex<()>,
}

impl Dirs {
    /// Le chemin du contenu `hash` : `files/<2 hex>/<62 hex>`. Le hash, écrit en hexadécimal,
    /// est le seul nom qui y entre.
    fn content_path(&self, hash: &[u8; 32]) -> PathBuf {
        let hex = hex(hash);
        self.root.join(&hex[..2]).join(&hex[2..])
    }

    /// Le chemin du contenu `hash`, une fois vérifié que son dossier de préfixe, s'il existe, est
    /// un vrai dossier et non un lien (R272).
    async fn checked_path(&self, hash: &[u8; 32]) -> io::Result<PathBuf> {
        let path = self.content_path(hash);
        if let Some(prefix) = path.parent() {
            match tokio::fs::symlink_metadata(prefix).await {
                Ok(meta) if !meta.is_dir() => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "{} n'est pas un dossier du dépôt (un lien n'y est pas suivi)",
                            prefix.display()
                        ),
                    ));
                }
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        Ok(path)
    }

    /// Le garde doit être celui de ce dépôt : celui d'un autre n'ordonne rien ici (R274).
    fn check(&self, guard: &MutexGuard<'_, ()>) -> io::Result<()> {
        if std::ptr::eq(MutexGuard::mutex(guard), &self.lock) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ce verrou n'est pas celui du dépôt des fichiers",
            ))
        }
    }
}

/// Le dépôt des fichiers, sous `<data_dir>/files`.
pub(crate) struct FileStore {
    dirs: Arc<Dirs>,
}

/// Un contenu posé par [`Upload::finish`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stored {
    /// SHA-256 des octets envoyés.
    pub hash: [u8; 32],
    pub size: u64,
    /// Lu dans les premiers octets ([`sniff`]).
    pub mime: &'static str,
    /// Ce `finish` a posé le contenu lui-même : rien n'était à sa place. L'envoi dont l'attache
    /// échoue ne retire que ce contenu-là ; un contenu qui était déjà là reste.
    pub fresh: bool,
}

/// Un envoi en cours : un fichier de `files/tmp/`, et le hash de ce qui y a été écrit.
pub(crate) struct Upload {
    dirs: Arc<Dirs>,
    /// `files/tmp/<UUIDv7>`.
    path: PathBuf,
    /// Lâché avant que le fichier ne prenne sa place.
    file: Option<tokio::fs::File>,
    hasher: Sha256,
    len: u64,
    /// Les premiers octets, pour [`sniff`].
    head: Vec<u8>,
    /// Vrai pendant une écriture, et pour de bon si elle n'est pas allée au bout (R273).
    broken: bool,
    /// Le temporaire a pris sa place, ou a été supprimé : `Drop` n'a plus rien à faire.
    settled: bool,
}

impl FileStore {
    /// Ouvre le dépôt de `data_dir` (au démarrage, donc synchrone) : crée `files/` et
    /// `files/tmp/`, vide `tmp/` des restes d'envois interrompus, **et rien d'autre** : aucun
    /// contenu n'est supprimé au démarrage (R245). Une entrée de `tmp/` qui ne part pas est
    /// journalisée et laissée là, pour le démarrage suivant. `tmp/` doit être un vrai dossier :
    /// un lien ne serait pas vidé, mais ce qu'il vise (R272).
    pub(crate) fn open(data_dir: &Path) -> io::Result<FileStore> {
        let root = data_dir.join(FILES_DIR);
        let tmp = root.join(TMP_DIR);
        std::fs::create_dir_all(&tmp)?;
        if !std::fs::symlink_metadata(&tmp)?.is_dir() {
            return Err(io::Error::other(format!(
                "{} n'est pas un dossier : le dépôt des fichiers y écrit ses envois en cours, et \
                 le vide à chaque démarrage (un lien n'y est pas suivi)",
                tmp.display()
            )));
        }
        let mut removed = 0u64;
        for entry in std::fs::read_dir(&tmp)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            // Un lien est retiré lui-même, jamais ce qu'il vise ; sous Windows, un lien vers un
            // dossier (ou une jonction) se retire comme un dossier.
            let gone = if kind.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path).or_else(|error| {
                    if kind.is_symlink() {
                        std::fs::remove_dir(&path)
                    } else {
                        Err(error)
                    }
                })
            };
            match gone {
                Ok(()) => removed += 1,
                Err(error) => tracing::warn!(
                    path = %path.display(),
                    %error,
                    "reste d'un envoi interrompu impossible à retirer ; le prochain démarrage \
                     réessaiera"
                ),
            }
        }
        if removed > 0 {
            tracing::info!(removed, "restes d'envois interrompus retirés de files/tmp");
        }
        Ok(FileStore {
            dirs: Arc::new(Dirs {
                root,
                tmp,
                lock: Mutex::new(()),
            }),
        })
    }

    /// Le nombre et le poids des contenus du dépôt que la base ne connaît pas (`known`), pour
    /// le journal du démarrage. Ne lit que les entrées de la forme `<2 hex>/<62 hex>` qui sont des
    /// fichiers ordinaires dans un vrai dossier (pas un lien), et ne touche à rien.
    pub(crate) fn strangers(&self, known: &[[u8; 32]]) -> io::Result<(u64, u64)> {
        let known: HashSet<&[u8; 32]> = known.iter().collect();
        let (mut count, mut bytes) = (0, 0);
        for prefix in std::fs::read_dir(&self.dirs.root)? {
            let prefix = prefix?;
            let prefix_name = prefix.file_name();
            let Some(head) = prefix_name.to_str().filter(|name| is_hex(name, 2)) else {
                continue;
            };
            if !prefix.file_type()?.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(prefix.path())? {
                let entry = entry?;
                let name = entry.file_name();
                let Some(tail) = name.to_str().filter(|name| is_hex(name, 62)) else {
                    continue;
                };
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let Some(hash) = parse_hex(head, tail) else {
                    continue;
                };
                if !known.contains(&hash) {
                    count += 1;
                    bytes += entry.metadata()?.len();
                }
            }
        }
        Ok((count, bytes))
    }

    /// Commence un envoi : un fichier neuf, `files/tmp/<UUIDv7>`.
    pub(crate) async fn begin(&self) -> io::Result<Upload> {
        let path = self.dirs.tmp.join(Uuid::now_v7().to_string());
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await?;
        file.set_max_buf_size(WRITE_BUFFER);
        Ok(Upload {
            dirs: self.dirs.clone(),
            path,
            file: Some(file),
            hasher: Sha256::new(),
            len: 0,
            head: Vec::with_capacity(SNIFF_LEN),
            broken: false,
            settled: false,
        })
    }

    /// Le verrou du dépôt : un seul pour « poser un contenu puis l'attacher » et pour le ménage.
    pub(crate) async fn lock(&self) -> MutexGuard<'_, ()> {
        self.dirs.lock.lock().await
    }

    /// Ouvre le contenu `hash` pour le lire. `NotFound` s'il n'est pas là, ou si ce qui est à sa
    /// place n'est pas un fichier ordinaire (un lien n'est pas suivi, R272).
    pub(crate) async fn open_file(&self, hash: &[u8; 32]) -> io::Result<tokio::fs::File> {
        let path = self.dirs.checked_path(hash).await?;
        if !tokio::fs::symlink_metadata(&path).await?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{} n'est pas un fichier ordinaire (un lien n'y est pas suivi)",
                    path.display()
                ),
            ));
        }
        tokio::fs::File::open(&path).await
    }

    /// Retire le contenu `hash` du disque, sous le verrou du dépôt. Un contenu absent n'est pas
    /// une erreur : le ménage peut repasser (R275).
    pub(crate) async fn remove(
        &self,
        guard: &MutexGuard<'_, ()>,
        hash: &[u8; 32],
    ) -> io::Result<()> {
        self.dirs.check(guard)?;
        let path = self.dirs.checked_path(hash).await?;
        match tokio::fs::remove_file(&path).await {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            done => done,
        }
    }
}

impl Upload {
    /// Ajoute `chunk` au fichier, et à son hash. Une écriture qui n'est pas allée au bout (une
    /// erreur, ou le futur lâché en route) met l'envoi hors d'usage (R273).
    pub(crate) async fn write(&mut self, chunk: &[u8]) -> io::Result<()> {
        let file = match (self.broken, self.file.as_mut()) {
            (false, Some(file)) => file,
            _ => return Err(broken()),
        };
        self.broken = true;
        file.write_all(chunk).await?;
        self.broken = false;
        self.hasher.update(chunk);
        self.len += chunk.len() as u64;
        let wanted = SNIFF_LEN.saturating_sub(self.head.len()).min(chunk.len());
        self.head.extend_from_slice(&chunk[..wanted]);
        Ok(())
    }

    /// Les octets écrits jusqu'ici.
    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    /// Pose le contenu, sous le verrou du dépôt (`guard`) : le fichier temporaire est vidé sur le
    /// disque (`sync_all`), puis mis à sa place s'il n'y a rien (`fresh`). S'il y a déjà un
    /// fichier ordinaire de même taille, c'est ce contenu, et le temporaire est supprimé ;
    /// autre chose (une autre taille, un lien, R272), il est abîmé : le temporaire le remplace, et
    /// le journal le dit. Sur une erreur, rien n'est posé et le temporaire part.
    pub(crate) async fn finish(mut self, guard: &MutexGuard<'_, ()>) -> io::Result<Stored> {
        self.dirs.check(guard)?;
        let mut file = match (self.broken, self.file.take()) {
            (false, Some(file)) => file,
            _ => return Err(broken()),
        };
        // Une écriture que tokio a laissée échouer en fond ne se dit qu'à l'opération suivante :
        // `flush` la rend, avant que le contenu ne soit vidé sur le disque.
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        let hash: [u8; 32] = std::mem::take(&mut self.hasher).finalize().into();
        let stored = Stored {
            hash,
            size: self.len,
            mime: sniff(&self.head),
            fresh: false,
        };
        if let Some(prefix) = self.dirs.content_path(&hash).parent() {
            tokio::fs::create_dir_all(prefix).await?;
        }
        // Un lien à la place du dossier de préfixe a été suivi par `create_dir_all` : il est
        // refusé ici, avant que rien ne soit posé à travers lui (R272).
        let dest = self.dirs.checked_path(&hash).await?;
        match tokio::fs::symlink_metadata(&dest).await {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tokio::fs::rename(&self.path, &dest).await?;
                self.settled = true;
                Ok(Stored {
                    fresh: true,
                    ..stored
                })
            }
            Err(error) => Err(error),
            Ok(meta) if meta.is_file() && meta.len() == self.len => {
                match tokio::fs::remove_file(&self.path).await {
                    Ok(()) => self.settled = true,
                    // `Drop` réessaiera, et le prochain démarrage vide `tmp/`.
                    Err(error) => tracing::warn!(
                        path = %self.path.display(),
                        %error,
                        "temporaire d'un contenu déjà gardé impossible à supprimer"
                    ),
                }
                Ok(stored)
            }
            Ok(meta) => {
                // Un lien se retire lui-même, jamais ce qu'il vise ; sous Windows, un lien vers un
                // dossier se retire comme un dossier. Un vrai dossier reste, et le renommage échoue.
                if meta.file_type().is_symlink() && tokio::fs::remove_file(&dest).await.is_err() {
                    tokio::fs::remove_dir(&dest).await?;
                }
                tokio::fs::rename(&self.path, &dest).await?;
                self.settled = true;
                tracing::warn!(
                    hash = %hex(&hash),
                    found_bytes = meta.len(),
                    link = meta.file_type().is_symlink(),
                    size = self.len,
                    "contenu abîmé (une autre taille, ou autre chose qu'un fichier) remplacé par \
                     le même contenu, renvoyé"
                );
                Ok(stored)
            }
        }
    }
}

impl Drop for Upload {
    /// Un envoi lâché sans avoir posé son contenu supprime son temporaire : le seul appel
    /// bloquant du dépôt hors du démarrage, un `unlink`.
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        // Le fichier d'abord : sous Windows, il ne part qu'une fois fermé (tokio le ferme dès
        // qu'aucune écriture n'est plus en route).
        drop(self.file.take());
        match std::fs::remove_file(&self.path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => tracing::warn!(
                path = %self.path.display(),
                %error,
                "temporaire d'un envoi abandonné impossible à supprimer ; le prochain démarrage \
                 videra files/tmp"
            ),
            _ => {}
        }
    }
}

/// L'erreur d'un envoi hors d'usage (R273).
fn broken() -> io::Error {
    io::Error::other(
        "cet envoi est hors d'usage : une écriture n'est pas allée au bout, et le fichier ne porte \
         plus les octets que son hash a vus",
    )
}

/// Le type d'un contenu, lu dans ses premiers octets par une table fermée (R246) ; tout le reste
/// (SVG, HTML, texte, archives, toute autre marque `ftyp`) est `application/octet-stream`. Le
/// type que déclare le client n'est jamais lu.
pub(crate) fn sniff(head: &[u8]) -> &'static str {
    let at = |range: std::ops::Range<usize>| head.get(range);
    let is = |range: std::ops::Range<usize>, motif: &[u8]| at(range) == Some(motif);
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if head.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        "image/gif"
    } else if head.starts_with(b"RIFF") && is(8..12, b"WEBP") {
        "image/webp"
    } else if head.starts_with(b"RIFF") && is(8..12, b"WAVE") {
        "audio/wav"
    } else if is(4..8, b"ftyp") {
        match at(8..12) {
            Some(
                b"isom" | b"iso2" | b"iso3" | b"iso4" | b"iso5" | b"iso6" | b"mp41" | b"mp42"
                | b"avc1" | b"M4V " | b"dash",
            ) => "video/mp4",
            Some(b"M4A " | b"M4B ") => "audio/mp4",
            _ => OCTET_STREAM,
        }
    } else if head.starts_with(b"\x1a\x45\xdf\xa3") {
        "video/webm"
    } else if head.starts_with(b"ID3")
        || head.starts_with(b"\xff\xfb")
        || head.starts_with(b"\xff\xf3")
        || head.starts_with(b"\xff\xf2")
    {
        "audio/mpeg"
    } else if head.starts_with(b"OggS") {
        "audio/ogg"
    } else if head.starts_with(b"%PDF-") {
        "application/pdf"
    } else {
        OCTET_STREAM
    }
}

/// Le nom d'affichage d'un fichier (R247) : un libellé, jamais un chemin. Sans caractère de
/// contrôle ni de direction (U+061C, U+200E, U+200F, U+202A à U+202E, U+2066 à U+2069), `/` et
/// `\` remplacés par `_`, espaces de bord retirées, 255 caractères au plus, « fichier » s'il n'en
/// reste rien.
pub(crate) fn display_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|&c| !c.is_control() && !is_direction_mark(c))
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect();
    let kept: String = cleaned.trim().chars().take(MAX_NAME_CHARS).collect();
    match kept.trim_end() {
        "" => DEFAULT_NAME.to_string(),
        kept => kept.to_string(),
    }
}

fn is_direction_mark(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// `Content-Disposition` d'un fichier servi, pour un nom déjà nettoyé ([`display_name`]) : toujours
/// `attachment` (R246), et le nom sous deux formes : `filename="…"`, où tout caractère hors de
/// l'ASCII imprimable, `"`, `\` et `%` devient `_` ; `filename*=UTF-8''…`, encodé en pour-cent
/// hors des `attr-char` de la RFC 5987. Un nom qui ferait passer `filename*` au-delà de 1,5 Kio
/// est raccourci, son extension gardée, pour que l'en-tête reste sous 2 Kio (R271) ; un nom vide
/// devient « fichier ».
pub(crate) fn content_disposition(name: &str) -> HeaderValue {
    let name = shorten_for_header(if name.is_empty() { DEFAULT_NAME } else { name });
    let fallback: String = name
        .chars()
        .map(|c| match c {
            '"' | '\\' | '%' => '_',
            ' '..='~' => c,
            _ => '_',
        })
        .collect();
    let mut encoded = String::with_capacity(name.len() * 3);
    for c in name.chars() {
        if is_attr_char(c) {
            encoded.push(c);
        } else {
            let mut utf8 = [0; 4];
            for byte in c.encode_utf8(&mut utf8).bytes() {
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }
    // Rien que de l'ASCII imprimable : la construction ne peut pas échouer.
    HeaderValue::try_from(format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// Les `attr-char` de la RFC 5987 : passent tels quels dans `filename*`.
fn is_attr_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "!#$&+-.^_`|~".contains(c)
}

/// Octets que prend `c` dans `filename*`.
fn encoded_len(c: char) -> usize {
    if is_attr_char(c) { 1 } else { 3 * c.len_utf8() }
}

/// `name`, raccourci par la fin de ce qui précède son extension s'il ferait passer `filename*`
/// au-delà de [`MAX_ENCODED_NAME`] (R271).
fn shorten_for_header(name: &str) -> Cow<'_, str> {
    if name.chars().map(encoded_len).sum::<usize>() <= MAX_ENCODED_NAME {
        return Cow::Borrowed(name);
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 && name[dot..].chars().count() <= MAX_KEPT_EXTENSION => {
            name.split_at(dot)
        }
        _ => (name, ""),
    };
    let mut budget = MAX_ENCODED_NAME - extension.chars().map(encoded_len).sum::<usize>();
    let mut shortened = String::new();
    for c in stem.chars() {
        let cost = encoded_len(c);
        if cost > budget {
            break;
        }
        budget -= cost;
        shortened.push(c);
    }
    shortened.push_str(extension);
    Cow::Owned(shortened)
}

/// Un hash en hexadécimal, en minuscules : 64 caractères.
pub(crate) fn hex(hash: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in hash {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// `name` fait `len` caractères hexadécimaux en minuscules, comme les écrit [`hex`].
fn is_hex(name: &str, len: usize) -> bool {
    name.len() == len && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Le hash que nomment un dossier de préfixe et un nom de contenu, déjà reconnus par [`is_hex`].
fn parse_hex(head: &str, tail: &str) -> Option<[u8; 32]> {
    let digits = format!("{head}{tail}");
    let mut hash = [0u8; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(digits.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(hash)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::Component;
    use std::sync::{Mutex as StdMutex, PoisonError};
    use std::time::{Duration, Instant, SystemTime};

    use tokio::io::AsyncReadExt;

    use super::*;

    /// Le chemin, relatif au dépôt, du contenu « abc » : le SHA-256 de ces trois octets, coupé
    /// après ses deux premiers caractères.
    const ABC_PATH: &str = "ba/7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// Le journal de ce fil, le temps que la valeur vive. Un second abonné, inerte, est inscrit
    /// avec lui : seul inscrit du processus, l'abonné de ce fil ne recevrait rien d'un point de
    /// journal que le fil d'un autre test, sans abonné, atteint le premier (`CLAUDE.md`, « Test
    /// qui lit le journal »). Les tests qui s'en servent tournent sur un seul fil (le défaut de
    /// `#[tokio::test]`) : tout ce qu'ils attendent est mené sur celui-ci.
    struct Logs {
        buffer: Arc<StdMutex<Vec<u8>>>,
        _guard: tracing::subscriber::DefaultGuard,
        _never_alone: tracing::Dispatch,
    }

    struct LogWriter(Arc<StdMutex<Vec<u8>>>);

    impl std::io::Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Logs {
        fn capture() -> Logs {
            let never_alone = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
            let buffer = Arc::new(StdMutex::new(Vec::new()));
            let writer = buffer.clone();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(move || LogWriter(writer.clone()))
                .with_ansi(false)
                .finish();
            Logs {
                buffer,
                _guard: tracing::subscriber::set_default(subscriber),
                _never_alone: never_alone,
            }
        }

        fn text(&self) -> String {
            String::from_utf8_lossy(&self.buffer.lock().unwrap_or_else(PoisonError::into_inner))
                .into_owned()
        }
    }

    /// Envoie `bytes` d'un coup et pose le contenu, sous le verrou.
    async fn put(store: &FileStore, bytes: &[u8]) -> Stored {
        let mut upload = store.begin().await.unwrap();
        upload.write(bytes).await.unwrap();
        assert_eq!(upload.len(), bytes.len() as u64);
        let guard = store.lock().await;
        upload.finish(&guard).await.unwrap()
    }

    /// Les fichiers sous `dir`, à toute profondeur, en chemins relatifs écrits avec `/`, triés.
    /// Les dossiers vides n'y paraissent pas ; un lien n'est pas suivi, il paraît sous son nom.
    fn listing(dir: &Path) -> Vec<String> {
        fn walk(base: &Path, dir: &Path, into: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    walk(base, &entry.path(), into);
                } else {
                    let relative = entry.path().strip_prefix(base).unwrap().to_owned();
                    let parts: Vec<String> = relative
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect();
                    into.push(parts.join("/"));
                }
            }
        }
        let mut found = Vec::new();
        walk(dir, dir, &mut found);
        found.sort();
        found
    }

    /// Ce que contient `files/tmp/`, au premier niveau.
    fn tmp_entries(data_dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(data_dir.join(FILES_DIR).join(TMP_DIR))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Attend, en temps réel, que `files/tmp/` soit vide : un fichier temporaire dont tokio
    /// écrit encore un morceau, sur un fil de calcul bloquant, ne disparaît sous Windows qu'une
    /// fois ce morceau écrit. La borne, large, ne peut que faire échouer à tort.
    async fn until_tmp_is_empty(data_dir: &Path) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !tmp_entries(data_dir).is_empty() {
            assert!(
                Instant::now() < deadline,
                "files/tmp/ n'est pas vide : {:?}",
                tmp_entries(data_dir)
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn sha256(bytes: &[u8]) -> [u8; 32] {
        Sha256::digest(bytes).into()
    }

    /// Pose à `at` un lien vers le dossier `target` : un lien symbolique sous Unix ; sous
    /// Windows, où un lien symbolique demande un privilège que le poste n'a pas toujours, une
    /// jonction (`mklink /J`), que `symlink_metadata` voit de même comme un lien.
    fn link_dir(target: &Path, at: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, at).unwrap();
        #[cfg(windows)]
        {
            let out = std::process::Command::new("cmd")
                .arg("/C")
                .arg("mklink")
                .arg("/J")
                .arg(at)
                .arg(target)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "jonction impossible à poser : {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        assert!(
            std::fs::symlink_metadata(at)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    /// La table de R246, ligne par ligne, chaque marque `ftyp` nommée comprise ; tout le reste,
    /// dont ce qui s'exécuterait ouvert comme un document, est `application/octet-stream`.
    #[test]
    fn type_is_read_in_the_bytes() {
        let riff = |kind: &[u8; 4]| [b"RIFF\x24\x00\x00\x00".as_slice(), kind, b"fmt "].concat();
        let ftyp = |brand: &[u8; 4]| {
            [
                b"\x00\x00\x00\x20ftyp".as_slice(),
                brand,
                b"\x00\x00\x02\x00isomiso2",
            ]
            .concat()
        };
        let mut cases: Vec<(Vec<u8>, &str)> = vec![
            (b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec(), "image/png"),
            (b"\xff\xd8\xff\xe0\x00\x10JFIF".to_vec(), "image/jpeg"),
            (b"\xff\xd8\xff".to_vec(), "image/jpeg"),
            (b"GIF87a\x01\x00".to_vec(), "image/gif"),
            (b"GIF89a\x01\x00".to_vec(), "image/gif"),
            (riff(b"WEBP"), "image/webp"),
            (riff(b"WAVE"), "audio/wav"),
            (ftyp(b"M4A "), "audio/mp4"),
            (ftyp(b"M4B "), "audio/mp4"),
            (b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81".to_vec(), "video/webm"),
            (b"ID3\x04\x00\x00".to_vec(), "audio/mpeg"),
            (b"\xff\xfb\x90\x64".to_vec(), "audio/mpeg"),
            (b"\xff\xf3\x48\xc4".to_vec(), "audio/mpeg"),
            (b"\xff\xf2\x48\xc4".to_vec(), "audio/mpeg"),
            (b"OggS\x00\x02".to_vec(), "audio/ogg"),
            (b"%PDF-1.7\n%\xe2\xe3".to_vec(), "application/pdf"),
        ];
        for brand in [
            b"isom", b"iso2", b"iso3", b"iso4", b"iso5", b"iso6", b"mp41", b"mp42", b"avc1",
            b"M4V ", b"dash",
        ] {
            cases.push((ftyp(brand), "video/mp4"));
        }
        for others in [
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>".to_vec(),
            b"<!doctype html><html><script>alert(1)</script>".to_vec(),
            b"<script>alert(document.cookie)</script>".to_vec(),
            Vec::new(),
            b"abc".to_vec(),
            // Les sept premiers octets d'un PNG.
            b"\x89PNG\r\n\x1a".to_vec(),
            riff(b"AVI "),
            b"RIFF".to_vec(),
            ftyp(b"qt  "),
            ftyp(b"heic"),
            ftyp(b"avif"),
            ftyp(b"zzzz"),
            // Une marque coupée.
            b"\x00\x00\x00\x20ftypis".to_vec(),
            b"PK\x03\x04".to_vec(),
        ] {
            cases.push((others, OCTET_STREAM));
        }
        for (head, expected) in cases {
            assert_eq!(
                sniff(&head),
                expected,
                "{:?}",
                String::from_utf8_lossy(&head)
            );
        }
    }

    /// Les deux formes de R246 : `filename="…"`, où tout ce qui n'est pas de l'ASCII imprimable,
    /// `"`, `\` et `%` devient `_` ; `filename*=UTF-8''…`, encodé en pour-cent hors des
    /// `attr-char` de la RFC 5987. Un nom très long reste sous 2 Kio (R271).
    #[test]
    fn content_disposition_is_well_formed() {
        let header = |name: &str| content_disposition(name).to_str().unwrap().to_string();
        assert_eq!(
            header("été \"2026\".pdf"),
            "attachment; filename=\"_t_ _2026_.pdf\"; \
             filename*=UTF-8''%C3%A9t%C3%A9%20%222026%22.pdf"
        );
        assert_eq!(
            header("a;b%.png"),
            "attachment; filename=\"a;b_.png\"; filename*=UTF-8''a%3Bb%25.png"
        );
        assert_eq!(
            header("a;b.png"),
            "attachment; filename=\"a;b.png\"; filename*=UTF-8''a%3Bb.png"
        );
        assert_eq!(
            header("a\\b"),
            "attachment; filename=\"a_b\"; filename*=UTF-8''a%5Cb"
        );
        assert_eq!(
            header("fichier"),
            "attachment; filename=\"fichier\"; filename*=UTF-8''fichier"
        );
        // Un nom vide ne vient pas de `display_name` ; s'il arrivait, c'est « fichier ».
        assert_eq!(header(""), header("fichier"));
        // Garde : un saut de ligne, que `display_name` retire déjà, n'ouvre pas un autre
        // en-tête.
        assert_eq!(
            header("a\r\nSet-Cookie: x"),
            "attachment; filename=\"a__Set-Cookie: x\"; filename*=UTF-8''a%0D%0ASet-Cookie%3A%20x"
        );
        // Les `attr-char` passent tels quels.
        assert_eq!(
            header("A-z_0.9!#$&+^`|~"),
            "attachment; filename=\"A-z_0.9!#$&+^`|~\"; filename*=UTF-8''A-z_0.9!#$&+^`|~"
        );

        // 255 caractères hors ASCII, de deux, trois et quatre octets : l'en-tête se construit et
        // reste sous 2 Kio ; s'il faut raccourcir, l'extension reste (R271).
        for c in ['é', '漢', '😀'] {
            for extension in ["", ".pdf"] {
                let stem: String = std::iter::repeat_n(c, 255 - extension.len()).collect();
                let name = format!("{stem}{extension}");
                assert_eq!(name.chars().count(), 255);
                let value = header(&name);
                assert!(value.len() < 2048, "{c} : {} octets", value.len());
                let (fallback, encoded) = value
                    .strip_prefix("attachment; filename=\"")
                    .and_then(|rest| rest.split_once("\"; filename*=UTF-8''"))
                    .unwrap_or_else(|| panic!("forme inattendue : {value}"));
                assert!(fallback.ends_with(extension), "{fallback}");
                assert!(encoded.ends_with(extension), "{encoded}");
                assert!(
                    fallback
                        .chars()
                        .all(|c| (' '..='~').contains(&c) && !matches!(c, '"' | '\\' | '%')),
                    "{fallback}"
                );
                // Un préfixe du nom, encodé caractère par caractère, rien d'autre.
                let decoded = percent_decode(encoded);
                let decoded_stem = decoded.strip_suffix(extension).unwrap();
                assert!(stem.starts_with(decoded_stem), "{decoded}");
                assert!(!decoded_stem.is_empty());
                assert_eq!(fallback.chars().count(), decoded.chars().count());
            }
        }
        // À deux octets, rien n'est raccourci.
        let accents: String = std::iter::repeat_n('é', 255).collect();
        let value = header(&accents);
        assert_eq!(
            percent_decode(value.split_once("UTF-8''").unwrap().1),
            accents
        );
    }

    /// Relit une valeur `filename*` : les `attr-char` tels quels, les `%XX` en octets.
    fn percent_decode(encoded: &str) -> String {
        let mut bytes = Vec::new();
        let mut rest = encoded.as_bytes();
        while let Some((&first, tail)) = rest.split_first() {
            if first == b'%' {
                let pair = std::str::from_utf8(&tail[..2]).unwrap();
                bytes.push(u8::from_str_radix(pair, 16).unwrap());
                rest = &tail[2..];
            } else {
                assert!(
                    first.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&first),
                    "{} n'est pas un attr-char",
                    first as char
                );
                bytes.push(first);
                rest = tail;
            }
        }
        String::from_utf8(bytes).unwrap()
    }

    /// Le nom n'est qu'un libellé (R247) : sans caractère de contrôle ni de direction, `/` et
    /// `\` remplacés, espaces de bord retirées, 255 caractères au plus, « fichier » s'il n'en
    /// reste rien.
    #[test]
    fn display_name_is_only_a_label() {
        for (raw, expected) in [
            ("../../etc/passwd", ".._.._etc_passwd"),
            ("C:\\Windows\\x", "C:_Windows_x"),
            ("photo\u{202E}gnp.exe", "photognp.exe"),
            ("ligne\nsuivante.txt", "lignesuivante.txt"),
            ("a\u{0}b\u{7}c\u{7f}d\u{9f}e", "abcde"),
            (
                "\u{61C}a\u{200E}b\u{200F}c\u{202A}\u{202B}\u{202C}\u{202D}d\u{2066}\u{2067}\u{2068}\u{2069}",
                "abcd",
            ),
            ("  rapport final.pdf \t", "rapport final.pdf"),
            ("été \"2026\".pdf", "été \"2026\".pdf"),
            ("", "fichier"),
            ("    ", "fichier"),
            (" \n\u{202E} ", "fichier"),
        ] {
            assert_eq!(display_name(raw), expected, "{raw:?}");
        }
        let long = "a".repeat(300);
        assert_eq!(display_name(&long), "a".repeat(255));
        // Des caractères, pas des octets.
        let accents: String = std::iter::repeat_n('é', 300).collect();
        assert_eq!(display_name(&accents).chars().count(), 255);
        // Une coupure qui tombe sur une espace ne la laisse pas au bord.
        let spaced = format!("{} b", "a".repeat(254));
        assert_eq!(display_name(&spaced), "a".repeat(254));
    }

    /// « abc » envoyé deux fois : un seul fichier, sous son hash, et rien d'autre dans le dépôt.
    #[tokio::test]
    async fn same_bytes_are_kept_once_under_their_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let first = put(&store, b"abc").await;
        let second = put(&store, b"abc").await;
        assert!(first.fresh, "le premier envoi pose le contenu");
        assert!(!second.fresh, "le second le trouve");
        assert_eq!(first.hash, sha256(b"abc"));
        assert_eq!(second.hash, first.hash);
        assert_eq!(
            hex(&first.hash),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!((first.size, first.mime), (3, OCTET_STREAM));
        let root = dir.path().join(FILES_DIR);
        assert_eq!(listing(&root), [ABC_PATH]);
        assert!(tmp_entries(dir.path()).is_empty());
        let mut read = Vec::new();
        let mut file = store.open_file(&first.hash).await.unwrap();
        file.read_to_end(&mut read).await.unwrap();
        assert_eq!(read, b"abc");
        // Un PNG est reconnu à ses octets, à l'envoi, même arrivés octet par octet.
        let png = put(
            &store,
            b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01",
        )
        .await;
        assert_eq!(png.mime, "image/png");
        let mut upload = store.begin().await.unwrap();
        for byte in b"\x00\x00\x00\x20ftypM4A \x00\x00\x00\x00" {
            upload.write(&[*byte]).await.unwrap();
        }
        let guard = store.lock().await;
        assert_eq!(upload.finish(&guard).await.unwrap().mime, "audio/mp4");
    }

    /// Un envoi lâché sans `finish` ne laisse rien : ni temporaire, ni contenu.
    #[tokio::test]
    async fn abandoned_upload_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let mut upload = store.begin().await.unwrap();
        upload.write(b"la moitie d'un fichier").await.unwrap();
        // Témoin : pendant l'envoi, le temporaire est là.
        assert_eq!(tmp_entries(dir.path()).len(), 1);
        drop(upload);
        assert!(tmp_entries(dir.path()).is_empty());
        assert!(listing(&dir.path().join(FILES_DIR)).is_empty());
    }

    /// Une écriture interrompue en route (le futur lâché pendant que tokio écrit) ou qui échoue
    /// laisse l'envoi hors d'usage (R273) : le fichier ne porte plus les octets que le hash a
    /// vus. Toute écriture suivante et `finish` échouent, et le temporaire part.
    #[tokio::test]
    async fn an_interrupted_write_breaks_the_upload() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let mut upload = store.begin().await.unwrap();
        upload.write(b"debut").await.unwrap();
        let big = vec![7u8; 16 << 20];
        let interrupted = tokio::select! {
            biased;
            written = upload.write(&big) => {
                written.unwrap();
                false
            }
            () = std::future::ready(()) => true,
        };
        assert!(interrupted, "l'écriture de 16 Mio a fini en un seul tour");
        let error = upload.write(b"suite").await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other, "{error}");
        let guard = store.lock().await;
        let error = upload.finish(&guard).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other, "{error}");
        drop(guard);
        until_tmp_is_empty(dir.path()).await;
        assert!(listing(&dir.path().join(FILES_DIR)).is_empty());
    }

    /// Au démarrage, seul `tmp/` est vidé (R245) : un contenu connu, un contenu que la base ne
    /// connaît pas et un fichier d'un autre nom restent ; `strangers` compte le contenu inconnu
    /// et son poids, et rien d'autre.
    #[tokio::test]
    async fn opening_empties_tmp_and_keeps_what_the_database_does_not_know() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(FILES_DIR);
        let (known, unknown) = {
            let store = FileStore::open(dir.path()).unwrap();
            (
                put(&store, b"connu de la base").await,
                put(&store, "inconnu de la base, mais gardé".as_bytes()).await,
            )
        };
        // Posés là par quelqu'un : un fichier à la racine du dépôt, un autre sous un dossier qui
        // n'est pas un préfixe, un autre dans le dossier d'un contenu, un nom en majuscules.
        let unknown_hex = hex(&unknown.hash);
        std::fs::write(root.join("notes.txt"), b"a moi").unwrap();
        std::fs::create_dir_all(root.join("zz")).unwrap();
        std::fs::write(root.join("zz").join("a".repeat(62)), b"pas un prefixe").unwrap();
        std::fs::write(root.join(&unknown_hex[..2]).join("LISEZMOI"), b"autre").unwrap();
        // (Pas un nom qui ne diffère que par la casse : sous Windows, il écraserait le contenu.)
        std::fs::write(
            root.join(&unknown_hex[..2])
                .join(format!("{}.bak", &unknown_hex[2..])),
            b"copie",
        )
        .unwrap();
        // Les restes d'un arrêt brutal : un temporaire, et un dossier.
        let tmp = root.join(TMP_DIR);
        std::fs::write(tmp.join(Uuid::now_v7().to_string()), b"reste").unwrap();
        std::fs::create_dir_all(tmp.join("dossier")).unwrap();
        std::fs::write(tmp.join("dossier").join("x"), b"reste").unwrap();
        let before: Vec<String> = listing(&root)
            .into_iter()
            .filter(|path| !path.starts_with("tmp/"))
            .collect();
        assert_eq!(before.len(), 6, "{before:?}");

        let store = FileStore::open(dir.path()).unwrap();
        assert!(tmp_entries(dir.path()).is_empty());
        assert_eq!(listing(&root), before, "rien d'autre n'a bougé");
        assert_eq!(store.strangers(&[known.hash]).unwrap(), (1, unknown.size));
        assert_eq!(
            store.strangers(&[unknown.hash, known.hash]).unwrap(),
            (0, 0)
        );
        assert_eq!(
            store.strangers(&[]).unwrap(),
            (2, known.size + unknown.size)
        );
        // Et ce qui reste se relit.
        assert!(store.open_file(&unknown.hash).await.is_ok());
    }

    /// Un contenu tronqué (une coupure, un disque abîmé) est remis entier par le même contenu
    /// renvoyé, et c'est journalisé. Témoin : renvoyé sur un contenu entier, rien n'est récrit.
    #[tokio::test]
    async fn a_truncated_content_is_replaced_by_the_next_upload() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let content: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let stored = put(&store, &content).await;
        let path = store.dirs.content_path(&stored.hash);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(10)
            .unwrap();

        let logs = Logs::capture();
        let again = put(&store, &content).await;
        assert!(!again.fresh, "le contenu était là, abîmé");
        assert_eq!(std::fs::read(&path).unwrap(), content);
        assert!(tmp_entries(dir.path()).is_empty());
        let journal = logs.text();
        assert!(
            journal.contains("WARN") && journal.contains(&hex(&stored.hash)),
            "le remplacement est journalisé : {journal}"
        );

        // Témoin : sur un contenu entier, le fichier n'est pas récrit (sa date reste).
        let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let third = put(&store, &content).await;
        assert!(!third.fresh);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), old);
        assert_eq!(std::fs::read(&path).unwrap(), content);
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// Le verrou ordonne un envoi et un ménage du même contenu. Le test tient le verrou ; un
    /// ménage simulé, qui retirerait le contenu si la base ne le connaissait pas, l'attend ; le
    /// test finit un envoi du même contenu, l'ajoute à ce que le ménage relira, puis rend le
    /// verrou : le ménage relit la base après l'avoir pris, et le fichier reste. Rien n'attend un
    /// délai : c'est le verrou qui ordonne. **Ce test ne prouve que le verrou** : la course
    /// réelle, entre un envoi et le ménage d'une page supprimée, est prouvée en S4b.
    #[tokio::test]
    async fn finish_and_sweep_never_cross() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(FileStore::open(dir.path()).unwrap());
        // Un contenu qu'aucune attache ne nomme encore : le ménage le retirerait.
        let first = put(&store, b"contenu en sursis").await;
        let attached: Arc<StdMutex<HashSet<[u8; 32]>>> = Arc::default();

        let guard = store.lock().await;
        let (waiting, waits) = tokio::sync::oneshot::channel();
        let sweep = tokio::spawn({
            let store = store.clone();
            let attached = attached.clone();
            async move {
                waiting.send(()).unwrap();
                let guard = store.lock().await;
                let kept = attached.lock().unwrap().contains(&first.hash);
                if !kept {
                    store.remove(&guard, &first.hash).await.unwrap();
                }
                kept
            }
        });
        // Le ménage a été mené jusqu'au verrou, où il attend (un seul fil d'exécution).
        waits.await.unwrap();
        let mut upload = store.begin().await.unwrap();
        upload.write(b"contenu en sursis").await.unwrap();
        let again = upload.finish(&guard).await.unwrap();
        assert_eq!(again.hash, first.hash);
        attached.lock().unwrap().insert(again.hash);
        drop(guard);

        assert!(sweep.await.unwrap(), "le ménage a vu l'attache");
        assert!(store.open_file(&first.hash).await.is_ok());
    }

    /// Deux envois du même contenu finis de front : le verrou les range l'un après l'autre, un
    /// seul pose le contenu, l'autre le trouve ; un fichier, aucun temporaire.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_finishes_of_the_same_content_keep_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(FileStore::open(dir.path()).unwrap());
        let tasks: Vec<_> = (0..4)
            .map(|_| {
                let store = store.clone();
                tokio::spawn(async move {
                    let mut upload = store.begin().await.unwrap();
                    upload.write(b"envoye quatre fois").await.unwrap();
                    let guard = store.lock().await;
                    upload.finish(&guard).await.unwrap()
                })
            })
            .collect();
        let mut fresh = 0;
        for task in tasks {
            let stored = task.await.unwrap();
            assert_eq!(stored.hash, sha256(b"envoye quatre fois"));
            fresh += usize::from(stored.fresh);
        }
        assert_eq!(fresh, 1, "un seul envoi pose le contenu");
        assert_eq!(listing(&dir.path().join(FILES_DIR)).len(), 1);
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// Un `finish` qui échoue (ici, un dossier occupe la place du contenu, comme le ferait un
    /// disque plein ou une écriture refusée) rend l'erreur et ne laisse rien : ni temporaire, ni
    /// contenu à moitié posé.
    #[tokio::test]
    async fn a_failed_finish_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let root = dir.path().join(FILES_DIR);
        std::fs::create_dir_all(root.join(ABC_PATH)).unwrap();
        let mut upload = store.begin().await.unwrap();
        upload.write(b"abc").await.unwrap();
        let guard = store.lock().await;
        assert!(upload.finish(&guard).await.is_err());
        assert!(tmp_entries(dir.path()).is_empty());
        assert!(listing(&root).is_empty(), "{:?}", listing(&root));
        assert!(root.join(ABC_PATH).is_dir());
    }

    /// Le hash est le seul nom d'un contenu, l'UUID le seul nom d'un envoi : quels que soient ses
    /// octets (des `.`, des `/`, des `\`, de quoi écrire `../mementum.db`), un hash nomme
    /// `files/<2 hex>/<62 hex>` et rien d'autre ; `open_file` et `remove` n'atteignent rien
    /// hors du dépôt.
    #[tokio::test]
    async fn content_paths_never_leave_the_depot() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let root = dir.path().join(FILES_DIR);
        let db = dir.path().join("mementum.db");
        std::fs::write(&db, b"la base").unwrap();
        let mut crafted = [0u8; 32];
        crafted[..23].copy_from_slice(b"../../../../mementum.db");
        for hash in [
            [0u8; 32],
            [0xff; 32],
            [b'.'; 32],
            [b'/'; 32],
            [b'\\'; 32],
            crafted,
            sha256(b"abc"),
        ] {
            let path = store.dirs.content_path(&hash);
            let relative = path.strip_prefix(&root).unwrap();
            let parts: Vec<String> = relative
                .components()
                .map(|part| match part {
                    Component::Normal(name) => name.to_str().unwrap().to_owned(),
                    other => panic!("{other:?} dans {}", path.display()),
                })
                .collect();
            assert_eq!(parts.len(), 2, "{parts:?}");
            assert!(is_hex(&parts[0], 2) && is_hex(&parts[1], 62), "{parts:?}");
            assert_eq!(parts.concat(), hex(&hash));
            let error = store.open_file(&hash).await.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
            let guard = store.lock().await;
            store.remove(&guard, &hash).await.unwrap();
        }
        assert_eq!(std::fs::read(&db).unwrap(), b"la base");
        // Le temporaire d'un envoi : un UUID, dans `tmp/`.
        let upload = store.begin().await.unwrap();
        let name = upload.path.strip_prefix(root.join(TMP_DIR)).unwrap();
        assert_eq!(name.components().count(), 1);
        Uuid::parse_str(name.to_str().unwrap()).unwrap();
    }

    /// Le verrou d'un autre dépôt n'ordonne rien ici : `finish` et `remove` le refusent, sans
    /// rien poser ni retirer (R274).
    #[tokio::test]
    async fn a_guard_of_another_store_is_refused() {
        let (here, there) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let store = FileStore::open(here.path()).unwrap();
        let other = FileStore::open(there.path()).unwrap();
        let kept = put(&store, b"deja la").await;
        let foreign = other.lock().await;
        let mut upload = store.begin().await.unwrap();
        upload.write(b"abc").await.unwrap();
        let error = upload.finish(&foreign).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{error}");
        let error = store.remove(&foreign, &kept.hash).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{error}");
        assert!(tmp_entries(here.path()).is_empty());
        assert_eq!(
            listing(&here.path().join(FILES_DIR)),
            [hex(&kept.hash)[..2].to_owned() + "/" + &hex(&kept.hash)[2..]]
        );
    }

    /// Retirer un contenu absent n'est pas une erreur (R275) : le ménage peut repasser.
    #[tokio::test]
    async fn removing_an_absent_content_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let stored = put(&store, b"abc").await;
        let guard = store.lock().await;
        store.remove(&guard, &stored.hash).await.unwrap();
        assert!(listing(&dir.path().join(FILES_DIR)).is_empty());
        store.remove(&guard, &stored.hash).await.unwrap();
    }

    /// Un lien posé dans le dépôt n'est jamais suivi (R272) : à la place d'un contenu, il n'est
    /// ni servi ni compté, et l'envoi du contenu le remplace par le vrai, sans toucher à ce qu'il
    /// visait ; à la place d'un dossier de préfixe, rien n'est servi ni posé à travers lui.
    #[tokio::test]
    async fn a_link_in_the_depot_is_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("ailleurs");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let root = dir.path().join(FILES_DIR);

        // À la place d'un contenu.
        let hash = sha256(b"secret");
        let path = store.dirs.content_path(&hash);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        link_dir(&outside, &path);
        let error = store.open_file(&hash).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
        assert_eq!(store.strangers(&[]).unwrap(), (0, 0));
        let stored = put(&store, b"secret").await;
        assert_eq!(stored.hash, hash);
        assert!(std::fs::symlink_metadata(&path).unwrap().is_file());
        assert_eq!(std::fs::read(&path).unwrap(), b"secret");
        assert_eq!(listing(&outside), ["secret.txt"]);
        #[cfg(unix)]
        {
            // Sous Unix, un lien vers un fichier, le cas que vise une attaque.
            let other = sha256(b"autre");
            let at = store.dirs.content_path(&other);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(outside.join("secret.txt"), &at).unwrap();
            let error = store.open_file(&other).await.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
            assert_eq!(store.strangers(&[stored.hash]).unwrap(), (0, 0));
            // Le retirer retire le lien, jamais ce qu'il vise.
            let guard = store.lock().await;
            store.remove(&guard, &other).await.unwrap();
            drop(guard);
            assert!(std::fs::symlink_metadata(&at).is_err());
            assert_eq!(
                std::fs::read(outside.join("secret.txt")).unwrap(),
                b"secret"
            );
        }

        // À la place d'un dossier de préfixe : un contenu dont le hash y mène n'est ni servi, ni
        // posé, ni retiré à travers lui.
        let target = sha256(b"abc");
        let prefix = root.join(&hex(&target)[..2]);
        let elsewhere = dir.path().join("prefixe-ailleurs");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join(&hex(&target)[2..]), b"abc").unwrap();
        link_dir(&elsewhere, &prefix);
        let error = store.open_file(&target).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
        let mut upload = store.begin().await.unwrap();
        upload.write(b"abc").await.unwrap();
        let guard = store.lock().await;
        assert!(upload.finish(&guard).await.is_err());
        assert!(store.remove(&guard, &target).await.is_err());
        drop(guard);
        assert_eq!(listing(&elsewhere), [hex(&target)[2..].to_owned()]);
        assert_eq!(store.strangers(&[stored.hash]).unwrap(), (0, 0));
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// `files/tmp` est vidé à chaque démarrage : s'il est un lien, le dépôt refuse de s'ouvrir,
    /// au lieu de vider ce qu'il vise (R272). Un lien laissé dans `tmp/` est retiré sans être
    /// suivi.
    #[test]
    fn a_link_as_tmp_stops_the_opening() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("ailleurs");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precieux.txt"), b"a garder").unwrap();

        // Un lien dans `tmp/` : retiré, sa cible intacte.
        FileStore::open(dir.path()).unwrap();
        let tmp = dir.path().join(FILES_DIR).join(TMP_DIR);
        link_dir(&outside, &tmp.join("lien"));
        FileStore::open(dir.path()).unwrap();
        assert!(tmp_entries(dir.path()).is_empty());
        assert_eq!(listing(&outside), ["precieux.txt"]);

        // `tmp` lui-même : refusé.
        std::fs::remove_dir(&tmp).unwrap();
        link_dir(&outside, &tmp);
        let error = FileStore::open(dir.path())
            .err()
            .expect("le dépôt refuse de s'ouvrir");
        assert!(
            error.to_string().contains("n'est pas un dossier"),
            "{error}"
        );
        assert_eq!(listing(&outside), ["precieux.txt"]);
    }
}
