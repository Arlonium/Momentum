//! Le dépôt des fichiers (Rulings R244 à R247) : les contenus envoyés, rangés sur le disque sous le
//! SHA-256 de leurs octets, dans `<data_dir>/files/<2 premiers caractères hexadécimaux>/<les 62
//! autres>`. Un envoi s'écrit d'abord dans `files/tmp/<UUIDv7>`, en calculant son hash au fil de
//! l'eau, puis prend sa place. **Aucun nom venu d'un client n'entre dans un chemin** : le hash et
//! l'UUID sont les seuls noms que le dépôt écrit ou lit. Le nom d'un fichier n'est qu'un libellé
//! ([`display_name`]), que [`content_disposition`] met en forme pour l'en-tête d'un fichier servi.
//! Le dépôt ne sait rien des pages ni des droits : c'est le store qui dit quelle page porte quel
//! contenu, et `access.rs` qui juge qui le lit. Ce module n'a pas de route : envoyer et lire un
//! fichier est la Task S4a, le ménage la Task S4b.
//!
//! Le type d'un contenu est lu dans ses premiers octets, à l'envoi, par une table fermée
//! ([`sniff`]) ; le type que déclare le client n'est jamais lu (R246).
//!
//! **Rien n'est supprimé au démarrage hors de `tmp/`** (R245). [`FileStore::open`] vide `tmp/`, et
//! rien d'autre : un contenu que la base ne connaît pas (arrêt entre la pose et l'attache, base
//! revenue en arrière) reste, compté par [`FileStore::strangers`], et repris tel quel si on le
//! renvoie ; un contenu que la base connaît et que le dépôt n'a pas est compté par
//! [`FileStore::missing`]. Un contenu ne quitte le disque que par [`FileStore::remove`].
//!
//! **Un contenu posé est entier, et il y reste** (R278). Le fichier temporaire est vidé sur le
//! disque ([`Upload::sync`] : `flush`, puis `sync_all`) avant de prendre sa place, et, sous Unix,
//! le dossier où il vient d'entrer est synchronisé à son tour, dans le même geste : quand `finish`
//! rend, une coupure de courant ne reprend plus le contenu (sous Windows, rien ne synchronise un
//! dossier : la fenêtre reste, et `missing` dit au démarrage ce qu'elle a coûté). Si un fichier
//! est déjà à sa place : de même taille, il est **tenu pour** le même contenu, sans être relu, et
//! le temporaire est supprimé (un contenu abîmé à taille égale n'est donc ni vu ni réparé) ; d'une
//! autre taille, ou s'il n'est pas un fichier ordinaire, il est abîmé, et le temporaire le
//! remplace, ce que le journal dit. Une écriture ou un vidage interrompu, ou qui échoue, met
//! l'envoi hors d'usage (R273) : le fichier ne porterait plus les octets que le hash a vus. Un
//! envoi lâché sans `finish` supprime son temporaire.
//!
//! **Deux verrous.** Celui du dépôt ([`FileStore::lock`]) est tenu par l'appelant : il ordonne
//! « poser un contenu puis l'attacher » et le ménage. [`Upload::finish`] et [`FileStore::remove`]
//! demandent son garde (d'un autre dépôt, il est refusé, R274) ; ainsi un ménage qui relit la base
//! sous le verrou voit l'attache d'un envoi qui vient de poser le contenu qu'il allait retirer.
//! Celui du geste est interne (R285) : chaque « regarder puis agir » sur le disque (`finish` : le
//! dossier de préfixe, ce qui est à la place du contenu, le renommage, la synchronisation du
//! dossier ; `remove` : ce qui est à sa place, le retrait) se fait d'un seul tenant sur un fil
//! bloquant, qui tient ce verrou jusqu'à sa fin.
//!
//! **Ce qui tient quand l'appelant est lâché en route** (une requête abandonnée, un `select!`, un
//! `timeout`). Rien n'annule ce qu'un fil bloquant a commencé, et le garde de l'appelant est rendu
//! avec son futur. Le dépôt garantit ceci :
//! - un geste parti va au bout, et **aucun autre geste ne regarde ni ne touche le dépôt avant sa
//!   fin** : l'envoi suivant du même contenu attend le retrait en vol, puis pose le contenu
//!   lui-même ; il attend la pose en vol, puis trouve le contenu ;
//! - un contenu est à sa place entier, ou n'y est pas : il y arrive par un renommage ;
//! - aucun temporaire ne reste sans propriétaire : celui d'un `begin`, d'un `write`, d'un `sync` ou
//!   d'un `finish` lâché part avec lui, ou avec le geste qui le tenait.
//!
//! Et il ne garantit pas ceci : **la suite de l'appelant**. Lâché entre `finish` et l'écriture de
//! son attache, il laisse un contenu que la base ne connaît pas ; lâché pendant `finish` ou
//! `remove`, il ne sait pas si le geste a eu lieu. Qui pose puis attache, ou lit la base puis
//! retire, le fait donc dans une tâche que l'abandon de la requête n'arrête pas (`to_completion`).
//!
//! **Les liens ne sont jamais suivis dans le dépôt** (R272) : un contenu n'est lu, gardé ou
//! compté que s'il est un fichier ordinaire dans un vrai dossier de préfixe ; un lien à la place
//! d'un contenu est retiré lui-même, jamais ce qu'il vise ; et `tmp/`, vidé à chaque démarrage,
//! doit être un vrai dossier. Seul `files/` lui-même peut être un lien, vers un autre disque.
//!
//! Aucun appel bloquant sur les fils du serveur, qui relaient les frappes (R247) : l'écriture
//! d'un envoi passe par `tokio::fs`, et tout autre geste sur le disque par un fil bloquant
//! (`spawn_blocking`). Restent synchrones l'ouverture et les comptes du démarrage, et le retrait
//! du temporaire d'un envoi lâché (`Drop`, un `unlink`).

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

/// Les chemins du dépôt et ses verrous, partagés avec les envois en cours et les gestes en vol.
struct Dirs {
    /// `<data_dir>/files`.
    root: PathBuf,
    /// `<data_dir>/files/tmp`.
    tmp: PathBuf,
    /// Le verrou du dépôt, que tient l'appelant ([`FileStore::lock`]).
    lock: Mutex<()>,
    /// Le verrou du geste (R285) : tenu par le fil bloquant qui regarde le dépôt puis y agit,
    /// jusqu'à la fin de son geste. L'appelant lâché en route ne peut pas le rendre à sa place.
    gesture: Arc<Mutex<()>>,
}

impl Dirs {
    /// Le dossier de préfixe du contenu `hash` : `files/<2 hex>`.
    fn prefix_path(&self, hash: &[u8; 32]) -> PathBuf {
        self.root.join(&hex(hash)[..2])
    }

    /// Le chemin du contenu `hash` : `files/<2 hex>/<62 hex>`. Le hash, écrit en hexadécimal,
    /// est le seul nom qui y entre.
    fn content_path(&self, hash: &[u8; 32]) -> PathBuf {
        self.prefix_path(hash).join(&hex(hash)[2..])
    }

    /// Le dossier de préfixe du contenu `hash` est-il là ? Il doit alors être un vrai dossier :
    /// un lien n'y est pas suivi (R272).
    fn prefix_is_there(&self, hash: &[u8; 32]) -> io::Result<bool> {
        let prefix = self.prefix_path(hash);
        match std::fs::symlink_metadata(&prefix) {
            Ok(meta) if meta.is_dir() => Ok(true),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{} n'est pas un dossier du dépôt (un lien n'y est pas suivi)",
                    prefix.display()
                ),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Le chemin du contenu `hash`, s'il y est un fichier ordinaire, dans un vrai dossier de
    /// préfixe ; `NotFound` s'il n'est pas là, ou si ce qui est à sa place est autre chose (un
    /// lien n'est pas suivi, R272).
    fn ordinary(&self, hash: &[u8; 32]) -> io::Result<PathBuf> {
        self.prefix_is_there(hash)?;
        let path = self.content_path(hash);
        if std::fs::symlink_metadata(&path)?.is_file() {
            Ok(path)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{} n'est pas un fichier ordinaire (un lien n'y est pas suivi)",
                    path.display()
                ),
            ))
        }
    }

    /// Le geste de [`Upload::finish`] : regarder ce qui est à la place du contenu `hash`, de
    /// `len` octets, puis y mettre `temp`, ou le supprimer si le contenu y est déjà. Sur une
    /// erreur, rien n'est posé, et `temp` part.
    fn place(&self, temp: Temporary, hash: &[u8; 32], len: u64) -> io::Result<Placed> {
        let new_prefix = !self.prefix_is_there(hash)?;
        if new_prefix {
            std::fs::create_dir_all(self.prefix_path(hash))?;
        }
        let dest = self.content_path(hash);
        let found = match std::fs::symlink_metadata(&dest) {
            Ok(meta) => Some(meta),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        match found {
            None => {
                std::fs::rename(&temp.path, &dest)?;
                temp.placed();
                Ok(Placed::Fresh { new_prefix })
            }
            // Tenu pour le même contenu, sans le relire : le hash le nomme, et sa taille est la
            // bonne. `temp`, lâché, est supprimé.
            Some(meta) if meta.is_file() && meta.len() == len => Ok(Placed::Found),
            Some(meta) => {
                // Un lien se retire lui-même, jamais ce qu'il vise. Un vrai dossier reste, et le
                // renommage échoue.
                if meta.file_type().is_symlink() {
                    remove_link(&dest)?;
                }
                std::fs::rename(&temp.path, &dest)?;
                temp.placed();
                tracing::warn!(
                    hash = %hex(hash),
                    found_bytes = meta.len(),
                    link = meta.file_type().is_symlink(),
                    size = len,
                    "contenu abîmé (une autre taille, ou autre chose qu'un fichier) remplacé par \
                     le même contenu, renvoyé"
                );
                Ok(Placed::Replaced)
            }
        }
    }

    /// Le geste de [`FileStore::remove`] : regarder ce qui est à la place du contenu `hash`,
    /// puis le retirer. Un lien est retiré lui-même, jamais ce qu'il vise (R272) ; un contenu
    /// absent n'est pas une erreur (R275).
    fn unlink(&self, hash: &[u8; 32]) -> io::Result<()> {
        if !self.prefix_is_there(hash)? {
            return Ok(());
        }
        let path = self.content_path(hash);
        let gone = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => remove_link(&path),
            Ok(_) => std::fs::remove_file(&path),
            Err(error) => Err(error),
        };
        match gone {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            done => done,
        }
    }

    /// Les dossiers où le geste `placed` vient d'écrire une entrée, à rendre durables (R278) : le
    /// dossier de préfixe, où le contenu est entré, puis `files/`, si ce dossier vient d'y être
    /// créé. Aucun pour un contenu trouvé là.
    fn written(&self, hash: &[u8; 32], placed: Placed) -> Vec<PathBuf> {
        match placed {
            Placed::Found => Vec::new(),
            Placed::Replaced | Placed::Fresh { new_prefix: false } => vec![self.prefix_path(hash)],
            Placed::Fresh { new_prefix: true } => vec![self.prefix_path(hash), self.root.clone()],
        }
    }

    /// Mène `work`, un geste qui regarde le dépôt puis y agit, d'un seul tenant sur un fil
    /// bloquant, sous le verrou du geste (R285). Le verrou entre dans la tâche, qui le rend à sa
    /// fin, et pas avant : si l'appelant est lâché en route, le geste va quand même au bout, et
    /// aucun autre ne regarde ni ne touche le dépôt d'ici là.
    async fn act<T: Send + 'static>(
        self: &Arc<Self>,
        work: impl FnOnce(&Dirs) -> io::Result<T> + Send + 'static,
    ) -> io::Result<T> {
        let held = self.gesture.clone().lock_owned().await;
        let dirs = self.clone();
        blocking(move || {
            let done = work(&dirs);
            drop(held);
            done
        })
        .await
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

/// Ce qu'a fait le geste de [`Upload::finish`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placed {
    /// Le temporaire a pris sa place, où il n'y avait rien ; `new_prefix` : dans un dossier de
    /// préfixe que ce geste vient de créer.
    Fresh { new_prefix: bool },
    /// Le contenu y était déjà : le temporaire a été supprimé, rien n'a été écrit dans le dépôt.
    Found,
    /// Le temporaire a remplacé ce qui était à sa place, abîmé.
    Replaced,
}

/// Le fichier temporaire d'un envoi, `files/tmp/<UUIDv7>`. Il n'a jamais qu'un propriétaire :
/// l'envoi, puis le geste qui le pose. Lâché sans avoir pris la place d'un contenu
/// ([`Temporary::placed`]), il est supprimé : par un `unlink`, le seul appel bloquant que le dépôt
/// fait, hors du démarrage, sur le fil qui lâche un envoi.
struct Temporary {
    path: PathBuf,
    /// Le fichier est encore dans `tmp/`.
    there: bool,
}

impl Temporary {
    /// Le fichier a pris la place d'un contenu : il n'y a plus rien à supprimer.
    fn placed(mut self) {
        self.there = false;
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if !self.there {
            return;
        }
        match std::fs::remove_file(&self.path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => tracing::warn!(
                path = %self.path.display(),
                %error,
                "temporaire d'un envoi impossible à supprimer ; le prochain démarrage videra \
                 files/tmp"
            ),
            _ => {}
        }
    }
}

/// Mène `work` sur un fil bloquant, dans le contexte de journal de qui l'appelle (son abonné, sa
/// portée) : ce que le geste journalise est dit comme si l'appelant l'avait dit, même parti.
/// **Rien n'annule `work` une fois commencé** : si le futur est lâché, `work` va au bout, et ce
/// qu'il rend est lâché à son tour.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let span = tracing::Span::current();
    let task = tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&dispatch, || span.in_scope(work))
    });
    match task.await {
        Ok(done) => done,
        // Le runtime s'arrête, ou `work` a paniqué.
        Err(error) => Err(io::Error::other(format!(
            "un geste sur le dépôt des fichiers n'a pas abouti : {error}"
        ))),
    }
}

/// Rend durables les entrées que ces dossiers viennent de recevoir (un contenu renommé, un
/// dossier créé), sous Unix : une entrée n'y est sur le disque qu'une fois son dossier synchronisé
/// à son tour (R278). Une synchronisation qui échoue est journalisée, sans faire échouer le
/// geste : le contenu est à sa place, entier, et dire l'inverse serait faux ; SQLite fait de même
/// pour le dossier de sa base.
fn durable(dirs: &[PathBuf]) {
    for dir in dirs {
        if let Err(error) = sync_dir(dir) {
            tracing::warn!(
                dir = %dir.display(),
                %error,
                "dossier du dépôt des fichiers impossible à synchroniser : une coupure de \
                 courant peut encore reprendre ce qui vient d'y entrer"
            );
        }
    }
}

/// Synchronise le dossier `dir` lui-même (`fsync` sur le dossier ouvert), pas ce qu'il contient.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// Hors d'Unix, rien : sous Windows un dossier ne s'ouvre pas ainsi, et la bibliothèque standard
/// n'a pas d'autre appel.
#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// Retire de `tmp/` le reste d'un envoi interrompu : un dossier avec ce qu'il contient, un lien
/// lui-même, jamais ce qu'il vise.
fn remove_leftover(entry: &std::fs::DirEntry) -> io::Result<()> {
    let path = entry.path();
    let gone = entry.file_type().and_then(|kind| {
        if kind.is_dir() {
            std::fs::remove_dir_all(&path)
        } else if kind.is_symlink() {
            remove_link(&path)
        } else {
            std::fs::remove_file(&path)
        }
    });
    gone.map_err(|error| at(&path, error))
}

/// `error`, avec le chemin où elle est arrivée : celles du système ne le disent pas, et le
/// journal du démarrage doit nommer ce qui ne se lit pas ou ne part pas.
fn at(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{} : {error}", path.display()))
}

/// Les contenus du dossier de préfixe `prefix` que la base ne connaît pas : leur nombre et leur
/// poids. Rien pour une entrée de `files/` qui n'est pas un vrai dossier nommé de deux
/// caractères hexadécimaux.
fn strangers_under(
    prefix: &std::fs::DirEntry,
    known: &HashSet<&[u8; 32]>,
) -> io::Result<(u64, u64)> {
    let (mut count, mut bytes) = (0, 0);
    let prefix_name = prefix.file_name();
    let Some(head) = prefix_name.to_str().filter(|name| is_hex(name, 2)) else {
        return Ok((0, 0));
    };
    if !prefix.file_type()?.is_dir() {
        return Ok((0, 0));
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
    Ok((count, bytes))
}

/// Retire le lien posé à `path`, jamais ce qu'il vise ; sous Windows, un lien vers un dossier (ou
/// une jonction) se retire comme un dossier.
fn remove_link(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))
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

/// Un envoi en cours : un fichier de `files/tmp/`, et le hash de ce qui y a été écrit. Lâché sans
/// `finish`, il supprime son temporaire ([`Temporary`]).
pub(crate) struct Upload {
    dirs: Arc<Dirs>,
    /// Déclaré avant `temp`, donc fermé avant lui : sous Windows, un fichier ne quitte son
    /// dossier qu'une fois fermé (tokio le ferme dès qu'aucune écriture n'est plus en route).
    file: tokio::fs::File,
    temp: Temporary,
    hasher: Sha256,
    len: u64,
    /// Les premiers octets, pour [`sniff`].
    head: Vec<u8>,
    /// Vrai pendant une écriture ou un vidage, et pour de bon s'il n'est pas allé au bout (R273).
    broken: bool,
    /// Tout ce que l'envoi a reçu est sur le disque ([`Upload::sync`]) ; faux dès l'écriture
    /// suivante.
    synced: bool,
}

impl FileStore {
    /// Ouvre le dépôt de `data_dir` (au démarrage, donc synchrone) : crée `files/` et
    /// `files/tmp/`, vide `tmp/` des restes d'envois interrompus, **et rien d'autre** : aucun
    /// contenu n'est supprimé au démarrage (R245). Seul ce qui empêcherait le dépôt de servir
    /// arrête l'ouverture : un dossier qui ne se crée pas, ou `tmp/` qui n'est pas un vrai
    /// dossier (un lien ne serait pas vidé, mais ce qu'il vise, R272). Le vidage, lui, n'arrête
    /// rien (R279) : une entrée de `tmp/` qui ne se lit pas ou ne part pas, ou `tmp/` qui ne se
    /// liste pas, est journalisé et laissé là, pour le démarrage suivant.
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
        // `files/` et `files/tmp/`, s'ils viennent d'être créés (R278).
        durable(&[data_dir.to_path_buf(), root.clone()]);
        let mut removed = 0u64;
        match std::fs::read_dir(&tmp) {
            Ok(entries) => {
                for entry in entries {
                    match entry.and_then(|entry| remove_leftover(&entry)) {
                        Ok(()) => removed += 1,
                        Err(error) => tracing::warn!(
                            %error,
                            "reste d'un envoi interrompu impossible à retirer de files/tmp ; le \
                             prochain démarrage réessaiera"
                        ),
                    }
                }
            }
            Err(error) => tracing::warn!(
                path = %tmp.display(),
                %error,
                "files/tmp impossible à lire : les restes d'envois interrompus y restent ; le \
                 prochain démarrage réessaiera"
            ),
        }
        if removed > 0 {
            tracing::info!(removed, "restes d'envois interrompus retirés de files/tmp");
        }
        Ok(FileStore {
            dirs: Arc::new(Dirs {
                root,
                tmp,
                lock: Mutex::new(()),
                gesture: Arc::default(),
            }),
        })
    }

    /// Le nombre et le poids des contenus du dépôt que la base ne connaît pas (`known`), pour
    /// le journal du démarrage. Ne lit que les entrées de la forme `<2 hex>/<62 hex>` qui sont des
    /// fichiers ordinaires dans un vrai dossier (pas un lien), et ne touche à rien. Un dossier de
    /// préfixe qui ne se lit pas est journalisé et passé (R279) : le compte est alors un
    /// minimum. Seul `files/` illisible est une erreur.
    pub(crate) fn strangers(&self, known: &[[u8; 32]]) -> io::Result<(u64, u64)> {
        let known: HashSet<&[u8; 32]> = known.iter().collect();
        let (mut count, mut bytes) = (0, 0);
        for prefix in std::fs::read_dir(&self.dirs.root)? {
            let read = prefix.and_then(|prefix| {
                strangers_under(&prefix, &known).map_err(|error| at(&prefix.path(), error))
            });
            match read {
                Ok((found, weight)) => {
                    count += found;
                    bytes += weight;
                }
                Err(error) => tracing::warn!(
                    %error,
                    "une partie du dépôt des fichiers n'a pas pu être lue : les contenus que la \
                     base ne connaît pas y sont peut-être plus nombreux que comptés"
                ),
            }
        }
        Ok((count, bytes))
    }

    /// Le nombre des contenus que la base connaît (`known`) et que le dépôt ne peut pas servir,
    /// pour le journal du démarrage : rien à leur place, ou autre chose qu'un fichier ordinaire
    /// dans un vrai dossier de préfixe (ce que [`FileStore::open_file`] refuse). C'est ce que
    /// laissent une coupure de courant entre une attache validée et un renommage qui n'était pas
    /// encore sur le disque (R278), ou une base restaurée sans son dossier `files/`. Ne touche à
    /// rien ; la taille n'est pas comparée à celle que la base garde.
    pub(crate) fn missing(&self, known: &[[u8; 32]]) -> u64 {
        known
            .iter()
            .filter(|hash| self.dirs.ordinary(hash).is_err())
            .count() as u64
    }

    /// Commence un envoi : un fichier neuf, `files/tmp/<UUIDv7>`. Lâché en route, il ne laisse
    /// pas de temporaire : le fichier et son propriétaire naissent dans le même geste, et ce que
    /// rend un geste que plus personne n'attend est lâché, le fichier fermé puis supprimé.
    pub(crate) async fn begin(&self) -> io::Result<Upload> {
        let path = self.dirs.tmp.join(Uuid::now_v7().to_string());
        let (file, temp) = blocking(move || {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            // Le fichier d'abord : lâchés ensemble, il est fermé avant d'être supprimé.
            Ok((file, Temporary { path, there: true }))
        })
        .await?;
        let mut file = tokio::fs::File::from_std(file);
        file.set_max_buf_size(WRITE_BUFFER);
        Ok(Upload {
            dirs: self.dirs.clone(),
            file,
            temp,
            hasher: Sha256::new(),
            len: 0,
            head: Vec::with_capacity(SNIFF_LEN),
            broken: false,
            synced: false,
        })
    }

    /// Le verrou du dépôt : un seul pour « poser un contenu puis l'attacher » et pour le ménage.
    pub(crate) async fn lock(&self) -> MutexGuard<'_, ()> {
        self.dirs.lock.lock().await
    }

    /// Ouvre le contenu `hash` pour le lire. `NotFound` s'il n'est pas là, ou si ce qui est à sa
    /// place n'est pas un fichier ordinaire (un lien n'est pas suivi, R272).
    pub(crate) async fn open_file(&self, hash: &[u8; 32]) -> io::Result<tokio::fs::File> {
        let (dirs, hash) = (self.dirs.clone(), *hash);
        let file = blocking(move || std::fs::File::open(dirs.ordinary(&hash)?)).await?;
        Ok(tokio::fs::File::from_std(file))
    }

    /// Retire le contenu `hash` du disque, sous le verrou du dépôt. Un contenu absent n'est pas
    /// une erreur : le ménage peut repasser (R275). Un lien posé à sa place est retiré lui-même,
    /// jamais ce qu'il vise (R272). Lâché en route, le retrait parti va au bout, et aucun autre
    /// geste ne touche le dépôt avant sa fin (R285).
    pub(crate) async fn remove(
        &self,
        guard: &MutexGuard<'_, ()>,
        hash: &[u8; 32],
    ) -> io::Result<()> {
        self.dirs.check(guard)?;
        let hash = *hash;
        self.dirs.act(move |dirs| dirs.unlink(&hash)).await
    }
}

impl Upload {
    /// Ajoute `chunk` au fichier, et à son hash. Une écriture qui n'est pas allée au bout (une
    /// erreur, ou le futur lâché en route) met l'envoi hors d'usage (R273).
    pub(crate) async fn write(&mut self, chunk: &[u8]) -> io::Result<()> {
        if self.broken {
            return Err(broken());
        }
        self.broken = true;
        self.synced = false;
        self.file.write_all(chunk).await?;
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

    /// Vide sur le disque ce que l'envoi a reçu (`flush`, puis `sync_all`). **À mener avant de
    /// prendre le verrou du dépôt** : sinon c'est `finish` qui le fait, sous le verrou, et les
    /// autres envois de l'instance attendent le vidage d'un gros fichier. Sans effet sur un
    /// envoi déjà vidé. Un vidage qui n'est pas allé au bout (une erreur, dont celle d'une
    /// écriture que tokio a laissée échouer en fond, ou le futur lâché en route) met l'envoi hors
    /// d'usage, comme une écriture (R273).
    pub(crate) async fn sync(&mut self) -> io::Result<()> {
        if self.broken {
            return Err(broken());
        }
        if self.synced {
            return Ok(());
        }
        self.broken = true;
        // Une écriture que tokio a laissée échouer en fond ne se dit qu'à l'opération suivante :
        // `flush` la rend, alors que `sync_all` la garderait pour lui.
        self.file.flush().await?;
        self.file.sync_all().await?;
        self.broken = false;
        self.synced = true;
        Ok(())
    }

    /// Pose le contenu, sous le verrou du dépôt (`guard`) : le fichier temporaire, vidé sur le
    /// disque (par [`Upload::sync`], que l'appelant mène avant de prendre le verrou ; à défaut,
    /// ici), est mis à sa place s'il n'y a rien (`fresh`). S'il y a déjà un fichier ordinaire de
    /// même taille, il est tenu pour ce contenu, sans être relu, et le temporaire est supprimé ;
    /// autre chose (une autre taille, un lien, R272), il est abîmé : le temporaire le remplace, et
    /// le journal le dit. Sur une erreur, rien n'est posé et le temporaire part. Lâché en route,
    /// le geste parti va au bout, avec son temporaire, et aucun autre geste ne touche le dépôt
    /// avant sa fin (R285) ; l'appelant, lui, ne sait pas si le contenu est posé.
    pub(crate) async fn finish(mut self, guard: &MutexGuard<'_, ()>) -> io::Result<Stored> {
        self.dirs.check(guard)?;
        // Le filet de qui n'a pas vidé son envoi avant de prendre le verrou.
        self.sync().await?;
        let Upload {
            dirs,
            file,
            temp,
            hasher,
            len,
            head,
            ..
        } = self;
        // Fermé avant de changer de nom : plus aucune écriture n'est en route.
        drop(file);
        let hash: [u8; 32] = hasher.finalize().into();
        let placed = dirs
            .act(move |dirs| {
                let placed = dirs.place(temp, &hash, len)?;
                // Dans le même geste, avant de rendre son verrou : quand `finish` rend, le
                // contenu est à sa place pour de bon, et c'est après seulement que l'appelant
                // écrit son attache (R278).
                durable(&dirs.written(&hash, placed));
                Ok(placed)
            })
            .await?;
        Ok(Stored {
            hash,
            size: len,
            mime: sniff(&head),
            fresh: matches!(placed, Placed::Fresh { .. }),
        })
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

/// Le nom d'affichage d'un fichier (R247, R287) : un libellé, jamais un chemin. Sans caractère
/// de contrôle, de direction (U+061C, U+200E, U+200F, U+202A à U+202E, U+2066 à U+2069) ni de
/// séparation de ligne (U+2028, U+2029), `/` et `\` remplacés par `_`, espaces de bord retirées,
/// 255 caractères au plus, l'extension gardée quand il faut couper ([`shorten`]), et « fichier »
/// s'il ne reste rien à montrer : rien, ou seulement des points, des espaces et des caractères
/// sans dessin. Les noms que Windows réserve (`CON`, `NUL.txt`) restent tels quels : aucun nom
/// n'entre dans un chemin du serveur.
pub(crate) fn display_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|&c| !c.is_control() && !is_direction_mark(c) && !is_line_break(c))
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect();
    let kept = shorten(cleaned.trim(), MAX_NAME_CHARS, |_| 1);
    // Jugé sur ce qui est gardé, pas sur le nom reçu : la coupe peut n'avoir laissé que des
    // caractères sans dessin.
    if kept.chars().all(|c| c == '.' || is_blank(c)) {
        DEFAULT_NAME.to_string()
    } else {
        kept.into_owned()
    }
}

fn is_direction_mark(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// Les séparateurs de ligne et de paragraphe d'Unicode : des sauts de ligne que `is_control` ne
/// connaît pas.
fn is_line_break(c: char) -> bool {
    matches!(c, '\u{2028}' | '\u{2029}')
}

/// Un caractère qui ne montre rien : une espace, ou l'un des caractères sans largeur ni dessin
/// (les « ignorables par défaut » d'Unicode, et le braille vide). Ils restent dans un nom qui a
/// autre chose à montrer (le liant U+200D tient ensemble les émojis composés, l'antiliant
/// U+200C s'écrit en persan) ; un nom qui n'a qu'eux ne se verrait pas à l'écran.
fn is_blank(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{034F}'
                | '\u{115F}'
                | '\u{1160}'
                | '\u{17B4}'
                | '\u{17B5}'
                | '\u{180B}'..='\u{180F}'
                | '\u{200B}'..='\u{200D}'
                | '\u{2060}'..='\u{2064}'
                | '\u{206A}'..='\u{206F}'
                | '\u{2800}'
                | '\u{3164}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{1D173}'..='\u{1D17A}'
                | '\u{E0000}'..='\u{E0FFF}'
        )
}

/// `Content-Disposition` d'un fichier servi, pour un nom déjà nettoyé ([`display_name`]) : toujours
/// `attachment` (R246), et le nom sous deux formes : `filename="…"`, où tout caractère hors de
/// l'ASCII imprimable, `"`, `\` et `%` devient `_` ; `filename*=UTF-8''…`, encodé en pour-cent
/// hors des `attr-char` de la RFC 5987. Un nom qui ferait passer `filename*` au-delà de 1,5 Kio
/// est raccourci, son extension gardée, pour que l'en-tête reste sous 2 Kio (R271) ; un nom
/// vide, ou dont la coupe ne laisse rien, devient « fichier ».
pub(crate) fn content_disposition(name: &str) -> HeaderValue {
    let name = shorten(name, MAX_ENCODED_NAME, encoded_len);
    let name = if name.is_empty() {
        DEFAULT_NAME
    } else {
        &*name
    };
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

/// `name`, raccourci s'il coûte plus que `budget`, à `cost` par caractère : par la fin de ce qui
/// précède son extension, gardée si elle fait au plus [`MAX_KEPT_EXTENSION`] caractères, point
/// compris (R271). La même règle pour les 255 caractères du nom gardé et pour les octets de
/// l'en-tête (R287). La coupe ne laisse au bord ni espace, ni liant U+200D, qui attendrait le
/// caractère suivant.
fn shorten(name: &str, budget: usize, cost: impl Fn(char) -> usize) -> Cow<'_, str> {
    if name.chars().map(&cost).sum::<usize>() <= budget {
        return Cow::Borrowed(name);
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 && name[dot..].chars().count() <= MAX_KEPT_EXTENSION => {
            name.split_at(dot)
        }
        _ => (name, ""),
    };
    let mut left = budget.saturating_sub(extension.chars().map(&cost).sum());
    let mut end = 0;
    for (at, c) in stem.char_indices() {
        let cost = cost(c);
        if cost > left {
            break;
        }
        left -= cost;
        end = at + c.len_utf8();
    }
    let stem = stem[..end].trim_end_matches(|c: char| c.is_whitespace() || c == '\u{200D}');
    Cow::Owned(format!("{stem}{extension}"))
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
    use std::future::Future;
    use std::path::Component;
    use std::pin::Pin;
    use std::sync::{Mutex as StdMutex, PoisonError, mpsc};
    use std::task::Poll;
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

    /// Mène `future` d'un seul tour.
    async fn poll_once<F: Future + ?Sized>(future: &mut Pin<Box<F>>) -> Poll<F::Output> {
        std::future::poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await
    }

    /// Un runtime qui n'a qu'un fil bloquant, où les tâches passent dans l'ordre de leur envoi :
    /// un test peut le retenir, et ce que le dépôt y envoie attend alors dans la file, comme sur
    /// un serveur dont les fils bloquants sont pris.
    fn one_blocking_thread() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap()
    }

    /// Mène `future` aussi loin qu'il va sans ce qu'un autre retient, sur un runtime à un seul fil
    /// bloquant ([`one_blocking_thread`]) dont le fil est libre : un tour, puis le temps que ce
    /// fil ait fait ce que le tour lui a envoyé (il prend ses tâches dans l'ordre : quand une
    /// tâche vide envoyée après est faite, celles d'avant le sont), et ainsi de suite. Rend sa
    /// valeur s'il finit ; `None` s'il attend autre chose que ce fil. Rien n'y dépend d'un délai.
    async fn as_far_as_it_goes<F: Future + ?Sized>(future: &mut Pin<Box<F>>) -> Option<F::Output> {
        for _ in 0..32 {
            if let Poll::Ready(done) = poll_once(future).await {
                return Some(done);
            }
            tokio::task::spawn_blocking(|| ()).await.unwrap();
        }
        None
    }

    /// Retient le seul fil bloquant du runtime courant, jusqu'à ce qu'on envoie un message à la
    /// valeur rendue (ou qu'on la lâche).
    fn hold_the_blocking_thread() -> mpsc::Sender<()> {
        let (release, held) = mpsc::channel::<()>();
        tokio::task::spawn_blocking(move || {
            let _ = held.recv();
        });
        release
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
            // Presque une signature : un PDF sans son tiret, un MP3 d'un autre en-tête de trame.
            b"%PDF".to_vec(),
            b"%PDF1.7\n%\xe2\xe3".to_vec(),
            b"\xff\xfa\x90\x64".to_vec(),
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
        // L'apostrophe, les parenthèses et l'astérisque n'en sont pas (RFC 5987) : l'apostrophe
        // sépare le jeu de caractères, la langue et la valeur de `filename*`.
        assert_eq!(
            header("photo (1).png"),
            "attachment; filename=\"photo (1).png\"; filename*=UTF-8''photo%20%281%29.png"
        );
        assert_eq!(
            header("l'été.png"),
            "attachment; filename=\"l'_t_.png\"; filename*=UTF-8''l%27%C3%A9t%C3%A9.png"
        );
        assert_eq!(
            header("a*b.txt"),
            "attachment; filename=\"a*b.txt\"; filename*=UTF-8''a%2Ab.txt"
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
                assert!(
                    pair.bytes()
                        .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)),
                    "%{pair}"
                );
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

    /// Un nom raccourci garde son extension, par la règle de l'en-tête (R271) ; un nom qui ne
    /// montre rien (des points, des espaces, des caractères sans dessin) devient « fichier » ;
    /// les séparateurs de ligne et de paragraphe d'Unicode partent comme les caractères de
    /// contrôle. Les noms réservés de Windows restent tels quels : aucun nom n'entre dans un
    /// chemin du serveur.
    #[test]
    fn display_name_keeps_the_extension_and_shows_something() {
        // 255 caractères au plus, l'extension gardée.
        let long = format!("{}.pdf", "a".repeat(300));
        assert_eq!(display_name(&long), format!("{}.pdf", "a".repeat(251)));
        let accents = format!("{}.tar.gz", "é".repeat(300));
        assert_eq!(display_name(&accents), format!("{}.gz", "é".repeat(252)));
        // Une « extension » de plus de seize caractères n'en est pas une : le nom est coupé par
        // la fin.
        let odd = format!("a.{}", "b".repeat(300));
        assert_eq!(display_name(&odd), format!("a.{}", "b".repeat(253)));
        // La coupe ne laisse au bord ni espace, ni liant U+200D, qui attendrait le caractère
        // suivant.
        let spaced = format!("{} b.txt", "a".repeat(250));
        assert_eq!(display_name(&spaced), format!("{}.txt", "a".repeat(250)));
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
        let joined = format!("a{}", family.repeat(60));
        let kept = display_name(&joined);
        assert!(joined.starts_with(&kept));
        assert_eq!(kept.chars().count(), 254);
        assert!(kept.ends_with('\u{1f469}'), "{:?}", kept.chars().last());

        // Rien à montrer : des points, des espaces, des caractères sans dessin.
        for raw in [
            ".",
            "..",
            "....",
            ". .",
            " . ",
            "\u{200b}",
            "\u{feff}",
            "\u{2060}\u{ad}",
            "\u{3164}",
            "\u{200b}.\u{feff}",
            "\u{2800}\u{2800}",
            "\u{200d}",
            "\u{2028}",
        ] {
            assert_eq!(display_name(raw), "fichier", "{raw:?}");
        }
        // Rien à montrer non plus dans ce que la coupe garde.
        let hidden = format!("{}a", "\u{200b}".repeat(300));
        assert_eq!(display_name(&hidden), "fichier");

        assert_eq!(display_name("a\u{2028}b\u{2029}c.txt"), "abc.txt");
        // Ce qui reste tel quel.
        for raw in [
            "CON",
            "NUL.txt",
            "a:b",
            "rapport.pdf.",
            ".htaccess",
            "~",
            "a.b",
            "\u{200b}.txt",
            "caf\u{e9} \u{1f468}\u{200d}\u{1f469}.png",
        ] {
            assert_eq!(display_name(raw), raw, "{raw:?}");
        }
    }

    /// La coupe de l'en-tête ne finit pas sur un liant U+200D ; et s'il ne reste rien du nom
    /// une fois coupé, c'est « fichier ».
    #[test]
    fn the_header_is_not_cut_on_a_joiner() {
        let header = |name: &str| content_disposition(name).to_str().unwrap().to_string();
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
        // Quarante familles : 2 160 octets encodés. Le budget s'arrête après vingt-huit
        // familles, un homme et un liant.
        let name = family.repeat(40);
        let value = header(&name);
        let decoded = percent_decode(value.split_once("UTF-8''").unwrap().1);
        assert!(name.starts_with(&decoded), "{decoded:?}");
        assert_eq!(decoded.chars().count(), 28 * 5 + 1);
        assert!(
            decoded.ends_with('\u{1f468}'),
            "{:?}",
            decoded.chars().last()
        );
        // Que des liants : rien ne reste de la coupe.
        assert_eq!(header(&"\u{200d}".repeat(200)), header("fichier"));
    }

    /// Une « extension » de plus de seize caractères n'est pas gardée : c'est le nom entier qui
    /// est raccourci (R271). Sans cette borne, le budget de ce qui précède l'extension passait
    /// sous zéro, et l'en-tête ne se construisait plus.
    #[test]
    fn a_long_extension_does_not_break_the_header() {
        for c in ['é', '漢', '😀'] {
            let name = format!("a.{}", std::iter::repeat_n(c, 253).collect::<String>());
            assert_eq!(name.chars().count(), 255);
            let header = content_disposition(&name);
            let value = header.to_str().unwrap();
            assert!(value.len() < 2048, "{}", value.len());
            assert!(value.starts_with("attachment; filename=\"a."), "{value}");
        }
    }

    /// Une suite de nombres fixe (xorshift) : les essais « au hasard » sont les mêmes à chaque
    /// lancement.
    fn sequence(seed: u64) -> impl FnMut() -> u64 {
        let mut x = seed;
        move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        }
    }

    /// Sur deux mille noms faits de caractères hostiles, le nom nettoyé et l'en-tête tiennent
    /// leurs règles, octet par octet ; l'en-tête les tient aussi pour un nom qui n'est pas passé
    /// par `display_name`.
    #[test]
    fn names_and_headers_hold_their_rules_for_hostile_names() {
        const ALPHABET: &[char] = &[
            'a',
            'Z',
            '0',
            ' ',
            ' ',
            '.',
            '.',
            '/',
            '\\',
            '"',
            '%',
            ';',
            '=',
            '\'',
            '*',
            '(',
            ')',
            ',',
            ':',
            '<',
            '>',
            '?',
            '|',
            '~',
            '\r',
            '\n',
            '\t',
            '\0',
            '\u{7f}',
            '\u{85}',
            '\u{9f}',
            '\u{a0}',
            '\u{ad}',
            'é',
            '\u{301}',
            '\u{61c}',
            '\u{200b}',
            '\u{200d}',
            '\u{200e}',
            '\u{200f}',
            '\u{2028}',
            '\u{2029}',
            '\u{202a}',
            '\u{202e}',
            '\u{2066}',
            '\u{2069}',
            '\u{feff}',
            '漢',
            '😀',
            '\u{1f468}',
            '\u{e0001}',
            '\u{10ffff}',
            '\u{3000}',
        ];
        let mut next = sequence(0x2545_F491_4F6C_DD1D);
        let mut shortened = 0;
        for round in 0..2_000 {
            let len = (next() % 400) as usize;
            // Une fois sur quatre, un seul caractère répété, puis une extension.
            let raw: String = if round % 4 == 0 {
                let c = ALPHABET[(next() % ALPHABET.len() as u64) as usize];
                let mut repeated: String = std::iter::repeat_n(c, len).collect();
                repeated.push_str(
                    [".pdf", ".tar.gz", ".", "", ".été", ".😀😀😀"][(next() % 6) as usize],
                );
                repeated
            } else {
                (0..len)
                    .map(|_| ALPHABET[(next() % ALPHABET.len() as u64) as usize])
                    .collect()
            };
            let name = display_name(&raw);
            assert!(!name.is_empty(), "{raw:?}");
            assert!(name.chars().count() <= MAX_NAME_CHARS, "{raw:?}");
            assert_eq!(name, name.trim(), "{raw:?}");
            assert!(
                !name.chars().any(|c| c.is_control()
                    || is_direction_mark(c)
                    || matches!(c, '/' | '\\' | '\u{2028}' | '\u{2029}')),
                "{name:?}"
            );
            assert!(
                name.chars().any(|c| c != '.' && !is_blank(c)),
                "rien à montrer : {name:?}"
            );
            assert_eq!(display_name(&name), name, "nettoyé deux fois : {raw:?}");
            // Coupé, il garde l'extension qu'il avait, si elle est courte.
            let cleaned = display_name(&raw.chars().take(MAX_NAME_CHARS / 2).collect::<String>());
            if let Some(dot) = cleaned.rfind('.').filter(|&dot| dot > 0)
                && cleaned[dot..].chars().count() <= MAX_KEPT_EXTENSION
            {
                let long = format!("{}{cleaned}", "a".repeat(MAX_NAME_CHARS));
                let kept = display_name(&long);
                assert_eq!(kept.chars().count(), MAX_NAME_CHARS, "{long:?}");
                assert!(kept.ends_with(&cleaned[dot..]), "{kept:?} de {long:?}");
            }

            for subject in [name.as_str(), raw.as_str()] {
                let header = content_disposition(subject);
                let value = header.to_str().unwrap();
                assert!(value.bytes().all(|b| (0x20..=0x7e).contains(&b)), "{value}");
                let (fallback, encoded) = value
                    .strip_prefix("attachment; filename=\"")
                    .and_then(|rest| rest.split_once("\"; filename*=UTF-8''"))
                    .unwrap_or_else(|| panic!("forme inattendue : {value}"));
                assert!(!fallback.contains(['"', '\\', '%']), "{fallback}");
                assert!(encoded.len() <= MAX_ENCODED_NAME, "{}", encoded.len());
                let decoded = percent_decode(encoded);
                assert_eq!(fallback.chars().count(), decoded.chars().count());
                let whole: usize = subject.chars().map(encoded_len).sum();
                if subject.is_empty() {
                    assert_eq!(decoded, DEFAULT_NAME);
                } else if whole <= MAX_ENCODED_NAME {
                    assert_eq!(decoded, subject);
                } else if decoded == DEFAULT_NAME {
                    // Il ne restait rien de la coupe : le début du nom, sur plus que ce que
                    // l'en-tête peut porter, n'était fait que d'espaces et de liants.
                    shortened += 1;
                    assert!(
                        subject
                            .chars()
                            .take(MAX_ENCODED_NAME / 12)
                            .all(|c| c.is_whitespace() || c == '\u{200d}'),
                        "{subject:?}"
                    );
                } else {
                    shortened += 1;
                    // Un début du nom, puis son extension si elle est courte, rien d'autre ; et
                    // la coupe ne laisse au bord ni espace, ni liant.
                    let extension = match subject.rfind('.') {
                        Some(dot)
                            if dot > 0 && subject[dot..].chars().count() <= MAX_KEPT_EXTENSION =>
                        {
                            &subject[dot..]
                        }
                        _ => "",
                    };
                    let stem = decoded
                        .strip_suffix(extension)
                        .unwrap_or_else(|| panic!("{decoded:?} de {subject:?}"));
                    assert!(subject.starts_with(stem), "{decoded:?} de {subject:?}");
                    assert!(
                        !stem.ends_with(|c: char| c.is_whitespace() || c == '\u{200d}'),
                        "{decoded:?}"
                    );
                }
                if subject.chars().count() <= MAX_NAME_CHARS {
                    assert!(value.len() < 2048, "{} octets", value.len());
                }
            }
        }
        assert!(
            shortened > 100,
            "témoin : des noms ont été raccourcis ({shortened})"
        );
    }

    /// La table ne lit que les premiers octets, là où elle les attend : un fichier à deux
    /// lectures (une image, un son ou un PDF qui porte aussi du HTML ou du script) a le type de
    /// sa signature, et ce qui ne commence pas par une signature n'en a aucun. Sur trois cent
    /// mille débuts tirés autour des signatures, aucun type ne sort des onze de R246, ni rien qui
    /// s'ouvrirait comme un document.
    #[test]
    fn the_table_is_closed_and_reads_only_the_first_bytes() {
        let cases: [(&[u8], &str); 22] = [
            (
                b"\x89PNG\r\n\x1a\n<html><script>alert(1)</script>",
                "image/png",
            ),
            (b"GIF89a=1;alert(1)//", "image/gif"),
            (b"GIF89a<svg onload=alert(1)>", "image/gif"),
            (b"\xff\xd8\xff<script>", "image/jpeg"),
            (b"%PDF-1.7<script>alert(1)</script>", "application/pdf"),
            (b"ID3<html><body onload=alert(1)>", "audio/mpeg"),
            (b"OggS<html>", "audio/ogg"),
            (b"RIFF<htmWEBP<script>", "image/webp"),
            (b"<htmftypisom<script>", "video/mp4"),
            (b" %PDF-1.7", OCTET_STREAM),
            (b"\n%PDF-1.7", OCTET_STREAM),
            (b"\xef\xbb\xbf<!doctype html>", OCTET_STREAM),
            (b"  <html>", OCTET_STREAM),
            (b"<?xml version=\"1.0\"?><svg", OCTET_STREAM),
            (b"<svg", OCTET_STREAM),
            (b"<HTML>", OCTET_STREAM),
            (b"<!--", OCTET_STREAM),
            (b"{\"a\":1}", OCTET_STREAM),
            (b"#!/bin/sh\n", OCTET_STREAM),
            (b"MZ\x90\x00", OCTET_STREAM),
            (b"\x89PNG\r\n\x1a", OCTET_STREAM),
            (b"%PDF", OCTET_STREAM),
        ];
        for (head, expected) in cases {
            let head = &head[..head.len().min(SNIFF_LEN)];
            assert_eq!(sniff(head), expected, "{:?}", String::from_utf8_lossy(head));
        }
        const TABLE: [&str; 12] = [
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "audio/wav",
            "video/mp4",
            "audio/mp4",
            "video/webm",
            "audio/mpeg",
            "audio/ogg",
            "application/pdf",
            OCTET_STREAM,
        ];
        const SEEDS: [&[u8]; 12] = [
            b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0d",
            b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01",
            b"GIF89a\x01\x00\x01\x00\x00\x00",
            b"RIFF\x24\x00\x00\x00WEBP",
            b"RIFF\x24\x00\x00\x00WAVE",
            b"\x00\x00\x00\x20ftypisom",
            b"\x00\x00\x00\x20ftypM4A ",
            b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81\x01\x42\xf7\x81",
            b"ID3\x04\x00\x00\x00\x00\x00\x00\x00\x00",
            b"OggS\x00\x02\x00\x00\x00\x00\x00\x00",
            b"%PDF-1.7\n%\xe2\xe3",
            b"<!doctype ht",
        ];
        let mut next = sequence(0x9E37_79B9_7F4A_7C15);
        let mut seen = HashSet::new();
        for _ in 0..300_000 {
            let mut head = SEEDS[(next() % 12) as usize].to_vec();
            for _ in 0..next() % 3 {
                let at = (next() % head.len() as u64) as usize;
                head[at] = next() as u8;
            }
            head.truncate((next() % 13) as usize);
            let kind = sniff(&head);
            assert!(TABLE.contains(&kind), "{kind}");
            assert!(
                !kind.contains("html") && !kind.contains("xml") && !kind.contains("script"),
                "{kind}"
            );
            seen.insert(kind);
        }
        assert_eq!(
            seen.len(),
            TABLE.len(),
            "témoin : tous les types ont été vus"
        );
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

    /// Un `begin` lâché en route ne laisse pas de temporaire : la création, déjà partie vers un
    /// fil bloquant, se fait quand même, et le fichier est supprimé aussitôt, faute de
    /// propriétaire. Sans cela, il restait dans `tmp/` jusqu'au démarrage suivant.
    #[test]
    fn a_cancelled_begin_leaves_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        one_blocking_thread().block_on(async {
            // Témoin : mené au bout, `begin` crée un temporaire.
            let upload = store.begin().await.unwrap();
            assert_eq!(tmp_entries(dir.path()).len(), 1);
            drop(upload);
            assert!(tmp_entries(dir.path()).is_empty());

            let release = hold_the_blocking_thread();
            let mut beginning = Box::pin(store.begin());
            assert!(poll_once(&mut beginning).await.is_pending());
            drop(beginning);
            release.send(()).unwrap();
            // Le fil bloquant prend ses tâches dans l'ordre : celle-ci faite, la création en vol
            // l'est aussi, et ce qu'elle a rendu est lâché.
            tokio::task::spawn_blocking(|| ()).await.unwrap();
            assert!(
                tmp_entries(dir.path()).is_empty(),
                "un temporaire sans envoi reste dans files/tmp : {:?}",
                tmp_entries(dir.path())
            );
        });
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
        let name = upload.temp.path.strip_prefix(root.join(TMP_DIR)).unwrap();
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

    /// `remove` retire un lien posé à la place d'un contenu : le lien lui-même, sans le suivre.
    /// Ce qu'il visait reste (R272).
    #[tokio::test]
    async fn removing_a_link_removes_the_link_itself() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("ailleurs");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precieux.txt"), b"a garder").unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let hash = sha256(b"abc");
        let path = store.dirs.content_path(&hash);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        link_dir(&outside, &path);
        let guard = store.lock().await;
        store.remove(&guard, &hash).await.unwrap();
        assert!(
            std::fs::symlink_metadata(&path).is_err(),
            "le lien est resté à la place du contenu"
        );
        assert_eq!(listing(&outside), ["precieux.txt"]);
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

    /// Un lien au fond d'un dossier laissé dans `tmp/` n'est pas suivi par le vidage du
    /// démarrage : le dossier part, ce que le lien visait reste.
    #[test]
    fn a_link_deep_in_tmp_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("ailleurs");
        std::fs::create_dir_all(outside.join("sous")).unwrap();
        std::fs::write(outside.join("precieux.txt"), b"a garder").unwrap();
        std::fs::write(outside.join("sous").join("aussi.txt"), b"a garder").unwrap();
        FileStore::open(dir.path()).unwrap();
        let tmp = dir.path().join(FILES_DIR).join(TMP_DIR);
        std::fs::create_dir_all(tmp.join("dossier").join("plus-bas")).unwrap();
        link_dir(&outside, &tmp.join("dossier").join("plus-bas").join("lien"));
        FileStore::open(dir.path()).unwrap();
        assert!(tmp_entries(dir.path()).is_empty());
        assert_eq!(listing(&outside), ["precieux.txt", "sous/aussi.txt"]);
    }

    /// Une écriture que tokio a laissée échouer en fond (il rend la main à `write` avant d'avoir
    /// écrit) est dite par `finish`, qui ne pose rien : sans cela, un fichier vide prenait la
    /// place du contenu, sous le hash des octets qu'il n'a pas reçus. L'écriture suivante la dit
    /// aussi, et met l'envoi hors d'usage.
    #[tokio::test]
    async fn a_background_write_that_fails_is_told_and_nothing_is_placed() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();

        let mut upload = store.begin().await.unwrap();
        let failing = fail_writes_of(&mut upload).await;
        // tokio rend la main avant d'avoir écrit : `write` réussit.
        upload.write(b"abc").await.unwrap();
        let guard = store.lock().await;
        let result = upload.finish(&guard).await;
        drop(guard);
        let placed = std::fs::metadata(store.dirs.content_path(&sha256(b"abc")));
        assert!(
            result.is_err(),
            "finish a rendu {result:?} alors que l'écriture a échoué ; à la place de « abc » : \
             {:?} octets",
            placed.map(|meta| meta.len())
        );
        assert!(placed.is_err(), "rien n'est posé");
        drop(failing);
        assert!(
            tmp_entries(dir.path()).is_empty(),
            "le temporaire est parti"
        );

        // La même panne, vue par l'écriture suivante.
        let mut upload = store.begin().await.unwrap();
        let failing = fail_writes_of(&mut upload).await;
        upload.write(b"abc").await.unwrap();
        assert!(upload.write(b"def").await.is_err());
        let guard = store.lock().await;
        assert!(upload.finish(&guard).await.is_err());
        drop(guard);
        drop(failing);

        // Et par `sync`, que l'appelant mène avant de prendre le verrou : l'envoi est alors hors
        // d'usage, `finish` ne pose rien.
        let mut upload = store.begin().await.unwrap();
        let failing = fail_writes_of(&mut upload).await;
        upload.write(b"abc").await.unwrap();
        assert!(upload.sync().await.is_err());
        let guard = store.lock().await;
        let error = upload.finish(&guard).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other, "{error}");
        drop(guard);
        drop(failing);

        // Une écriture faite après un `sync` n'est pas tenue pour vidée : sa panne est dite.
        let mut upload = store.begin().await.unwrap();
        upload.sync().await.unwrap();
        let failing = fail_writes_of(&mut upload).await;
        upload.write(b"abc").await.unwrap();
        let guard = store.lock().await;
        assert!(upload.finish(&guard).await.is_err());
        drop(guard);
        drop(failing);
        assert!(tmp_entries(dir.path()).is_empty());
        assert!(listing(&dir.path().join(FILES_DIR)).is_empty());
    }

    /// `sync` ne rend la main qu'une fois sur le disque tout ce que l'envoi a reçu : `write`,
    /// lui, la rend dès que tokio a pris les octets. L'appelant le mène avant de prendre le
    /// verrou du dépôt, pour que les autres envois n'attendent pas le vidage d'un gros fichier.
    #[test]
    fn sync_waits_for_the_bytes_to_reach_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        one_blocking_thread().block_on(async {
            let mut upload = store.begin().await.unwrap();
            let path = upload.temp.path.clone();
            let release = hold_the_blocking_thread();
            upload.write(b"abc").await.unwrap();
            assert_eq!(
                std::fs::metadata(&path).unwrap().len(),
                0,
                "témoin : rien n'est encore écrit"
            );
            let mut syncing = Box::pin(upload.sync());
            assert!(poll_once(&mut syncing).await.is_pending());
            release.send(()).unwrap();
            syncing.await.unwrap();
            assert_eq!(std::fs::metadata(&path).unwrap().len(), 3);
            // Vidé deux fois, puis posé : le contenu est entier.
            upload.sync().await.unwrap();
            let guard = store.lock().await;
            let stored = upload.finish(&guard).await.unwrap();
            assert_eq!(
                std::fs::read(store.dirs.content_path(&stored.hash)).unwrap(),
                b"abc"
            );
        });
    }

    /// Un envoi déjà vidé sur le disque arrive à son geste sans rien demander d'autre au disque :
    /// sous le verrou du dépôt, `finish` ne fait plus que regarder et renommer. Le seul fil
    /// bloquant est retenu : un `finish` qui viderait encore le fichier attendrait ce vidage,
    /// sans avoir pris le verrou du geste.
    #[test]
    fn a_synced_upload_goes_straight_to_its_gesture() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        one_blocking_thread().block_on(async {
            let mut upload = store.begin().await.unwrap();
            upload.write(b"abc").await.unwrap();
            upload.sync().await.unwrap();
            let release = hold_the_blocking_thread();
            let guard = store.lock().await;
            let mut finishing = Box::pin(upload.finish(&guard));
            assert!(poll_once(&mut finishing).await.is_pending());
            assert!(
                store.dirs.gesture.try_lock().is_err(),
                "le geste n'est pas parti : `finish` attend autre chose du disque"
            );
            release.send(()).unwrap();
            assert!(finishing.await.unwrap().fresh);
            // Témoin : le geste fini, son verrou est rendu.
            assert!(store.dirs.gesture.try_lock().is_ok());
        });
    }

    /// Fait échouer, en fond, les écritures de `upload` tant que la valeur rendue vit. Sous
    /// Windows, un autre descripteur verrouille le fichier (`LockFileEx`) ; sous Unix, où un tel
    /// verrou n'empêche pas d'écrire, le fichier de l'envoi est remplacé par un descripteur en
    /// lecture seule.
    async fn fail_writes_of(upload: &mut Upload) -> Option<std::fs::File> {
        #[cfg(windows)]
        {
            let blocker = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&upload.temp.path)
                .unwrap();
            blocker.lock().unwrap();
            Some(blocker)
        }
        #[cfg(unix)]
        {
            upload.file = tokio::fs::File::open(&upload.temp.path).await.unwrap();
            None
        }
    }

    /// Un `remove` lâché en route (une requête que son client abandonne) a déjà envoyé son geste
    /// à un fil bloquant, où rien ne l'annule, et le garde de son appelant est rendu avec lui. Le
    /// geste tient pourtant le dépôt jusqu'à sa fin : l'envoi du même contenu, qui prend le
    /// verrou juste après, ne regarde ni ne pose rien avant que le retrait soit fait, puis pose
    /// le contenu lui-même. Sans cela, il trouvait le contenu « déjà là » et écrivait son
    /// attache, puis le retrait en vol emportait le fichier : une attache sans contenu.
    #[test]
    fn a_cancelled_remove_lands_before_the_next_gesture() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let (slow, fast) = (one_blocking_thread(), one_blocking_thread());
        let content = b"contenu que le menage croit sans attache";
        let (first, upload) = fast.block_on(async {
            let first = put(&store, content).await;
            let mut upload = store.begin().await.unwrap();
            upload.write(content).await.unwrap();
            upload.sync().await.unwrap();
            (first, upload)
        });
        let path = store.dirs.content_path(&first.hash);

        // Le ménage, dans une requête que son client abandonne : le seul fil bloquant est pris,
        // ce que `remove` y envoie attend dans la file.
        let release = slow.block_on(async {
            let release = hold_the_blocking_thread();
            let guard = store.lock().await;
            let mut removal = Box::pin(store.remove(&guard, &first.hash));
            assert!(poll_once(&mut removal).await.is_pending());
            drop(removal);
            drop(guard);
            release
        });
        assert!(path.exists(), "témoin : le retrait n'est pas encore fait");

        fast.block_on(async {
            // Le verrou est libre : la requête abandonnée l'a rendu.
            let guard = store.lock().await;
            let mut finishing = Box::pin(upload.finish(&guard));
            assert!(
                as_far_as_it_goes(&mut finishing).await.is_none(),
                "l'envoi a regardé le dépôt pendant que le retrait d'une requête abandonnée \
                 était encore en vol"
            );
            assert_eq!(tmp_entries(dir.path()).len(), 1);
            // Le fil bloquant de la requête abandonnée reprend : le retrait se fait.
            release.send(()).unwrap();
            let stored = finishing.await.unwrap();
            // Ici la route écrit l'attache : le contenu est là, et y reste.
            assert!(
                stored.fresh,
                "le retrait est passé d'abord, puis l'envoi a posé le contenu lui-même"
            );
            assert_eq!(std::fs::read(&path).unwrap(), content);
            drop(guard);
        });
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// Un `finish` lâché en route, son geste déjà parti : le geste va au bout, d'un seul tenant.
    /// Son temporaire ne part pas avec l'envoi lâché (le geste le tient), le contenu est posé
    /// entier, et l'envoi suivant du même contenu ne regarde rien avant, puis le trouve.
    #[test]
    fn a_cancelled_finish_lands_whole_before_the_next_gesture() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let (slow, fast) = (one_blocking_thread(), one_blocking_thread());
        let content = b"contenu d'une requete abandonnee pendant sa pose";
        let path = store.dirs.content_path(&sha256(content));
        let (abandoned, upload) = fast.block_on(async {
            let mut uploads = Vec::new();
            for _ in 0..2 {
                let mut upload = store.begin().await.unwrap();
                upload.write(content).await.unwrap();
                upload.sync().await.unwrap();
                uploads.push(upload);
            }
            let upload = uploads.pop().unwrap();
            (uploads.pop().unwrap(), upload)
        });

        let release = slow.block_on(async {
            let release = hold_the_blocking_thread();
            let guard = store.lock().await;
            let mut finishing = Box::pin(abandoned.finish(&guard));
            assert!(poll_once(&mut finishing).await.is_pending());
            drop(finishing);
            drop(guard);
            release
        });
        assert!(!path.exists(), "témoin : rien n'est encore posé");
        assert_eq!(
            tmp_entries(dir.path()).len(),
            2,
            "le temporaire de l'envoi lâché est à son geste, qui n'a pas encore eu lieu"
        );

        fast.block_on(async {
            let guard = store.lock().await;
            let mut finishing = Box::pin(upload.finish(&guard));
            assert!(
                as_far_as_it_goes(&mut finishing).await.is_none(),
                "l'envoi a regardé le dépôt pendant que la pose d'une requête abandonnée était \
                 encore en vol"
            );
            assert!(!path.exists());
            release.send(()).unwrap();
            let stored = finishing.await.unwrap();
            assert!(
                !stored.fresh,
                "la pose abandonnée est passée d'abord : l'envoi trouve le contenu"
            );
            assert_eq!(std::fs::read(&path).unwrap(), content);
            drop(guard);
        });
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// Le verrou du geste est tenu **pendant** le geste, pas seulement jusqu'à son départ (R285) :
    /// c'est le geste lui-même qui regarde son verrou, sans course. Rendu avant le geste, il
    /// laisserait un autre geste regarder le dépôt pendant que celui-ci y agit ; les deux tests
    /// d'annulation, plus haut, ne le voient que selon l'ordre où passent les fils.
    #[tokio::test]
    async fn the_gesture_lock_is_held_until_the_gesture_ends() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let held = store
            .dirs
            .act(|dirs| Ok(dirs.gesture.try_lock().is_err()))
            .await
            .unwrap();
        assert!(held, "le verrou du geste est rendu avant la fin du geste");
        // Témoin : le geste fini, son verrou est rendu.
        assert!(store.dirs.gesture.try_lock().is_ok());
    }

    /// Un geste déjà dans la file d'un runtime qui s'arrête, son appelant lâché : le dépôt n'en
    /// garde rien de travers (R285). Le contenu est à sa place entier, ou n'y est pas ; aucun
    /// temporaire ne reste ; le verrou du geste est rendu. Avec tokio 1.54, le fil bloquant mène
    /// ce qui attendait dans sa file avant de regarder l'arrêt : le geste a lieu, et le contenu
    /// est posé. Le test ne fige pas ce choix de tokio, seulement ce que le dépôt garantit.
    #[test]
    fn a_gesture_queued_on_a_runtime_that_stops_leaves_nothing_halfway() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let slow = one_blocking_thread();
        let content = b"geste dans la file d'un runtime qui s'arrete";
        let path = store.dirs.content_path(&sha256(content));
        let release = slow.block_on(async {
            let mut upload = store.begin().await.unwrap();
            upload.write(content).await.unwrap();
            upload.sync().await.unwrap();
            let release = hold_the_blocking_thread();
            let guard = store.lock().await;
            let mut finishing = Box::pin(upload.finish(&guard));
            assert!(poll_once(&mut finishing).await.is_pending());
            drop(finishing);
            drop(guard);
            release
        });
        // Témoin : le geste attend dans la file, avec son temporaire et son verrou.
        assert_eq!(tmp_entries(dir.path()).len(), 1);
        assert!(store.dirs.gesture.try_lock().is_err());
        slow.shutdown_background();
        drop(release);
        // Le fil bloquant, rendu, finit seul : attendre ce qu'il laisse, borne large.
        let deadline = Instant::now() + Duration::from_secs(20);
        while store.dirs.gesture.try_lock().is_err() || !tmp_entries(dir.path()).is_empty() {
            assert!(
                Instant::now() < deadline,
                "le verrou du geste ou le temporaire est resté : {:?}",
                tmp_entries(dir.path())
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Ok(placed) = std::fs::read(&path) {
            assert_eq!(placed, content, "un contenu posé l'est en entier");
        }
    }

    /// Un geste que le runtime refuse (il s'arrête déjà quand le geste part) : `finish` le dit
    /// par une erreur, rien n'est posé, le temporaire part avec le geste qui ne s'est pas fait,
    /// et le verrou du geste est rendu (R285). Un runtime arrêté ne mène plus rien : le futur
    /// est mené à la main, dans son contexte.
    #[test]
    fn a_gesture_refused_by_a_stopped_runtime_is_told_and_places_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let runtime = one_blocking_thread();
        let content = b"geste refuse par un runtime arrete";
        let upload = runtime.block_on(async {
            let mut upload = store.begin().await.unwrap();
            upload.write(content).await.unwrap();
            upload.sync().await.unwrap();
            upload
        });
        let guard = runtime.block_on(store.lock());
        let handle = runtime.handle().clone();
        runtime.shutdown_background();
        let _context = handle.enter();
        let mut finishing = Box::pin(upload.finish(&guard));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        let deadline = Instant::now() + Duration::from_secs(20);
        let done = loop {
            if let Poll::Ready(done) = finishing.as_mut().poll(&mut cx) {
                break done;
            }
            assert!(Instant::now() < deadline, "finish n'a jamais fini");
            std::thread::sleep(Duration::from_millis(1));
        };
        let error = done.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other, "{error}");
        assert!(
            tmp_entries(dir.path()).is_empty(),
            "le temporaire est resté"
        );
        assert!(store.dirs.gesture.try_lock().is_ok());
        assert!(!store.dirs.content_path(&sha256(content)).exists());
    }

    /// Le hash, la taille et le type sont ceux de tous les octets, quelle que soit la façon dont
    /// ils arrivent : par morceaux d'un octet, de part et d'autre des douze premiers, autour du
    /// tampon de 64 Kio, avec des morceaux vides. Et un fichier de zéro octet se pose et se
    /// relit.
    #[tokio::test]
    async fn hash_size_and_type_hold_however_the_bytes_are_split() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let mut content = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
        let mut next = sequence(0x9E37_79B9);
        while content.len() < 70_007 {
            content.push(next() as u8);
        }
        let expected = sha256(&content);
        // Les premiers morceaux font ces tailles ; ce qui reste arrive d'un coup.
        for sizes in [
            vec![1usize; 24],
            vec![7, 7],
            vec![11, 1],
            vec![12],
            vec![13],
            vec![65_535],
            vec![65_536],
            vec![65_537, 3],
            vec![0, 5, 0, 30_000],
            vec![],
        ] {
            let mut upload = store.begin().await.unwrap();
            let mut rest = &content[..];
            for n in sizes.iter().copied().chain([usize::MAX]) {
                let n = n.min(rest.len());
                upload.write(&rest[..n]).await.unwrap();
                rest = &rest[n..];
            }
            assert!(rest.is_empty());
            assert_eq!(upload.len(), content.len() as u64, "{sizes:?}");
            let guard = store.lock().await;
            let stored = upload.finish(&guard).await.unwrap();
            assert_eq!(hex(&stored.hash), hex(&expected), "hash, découpe {sizes:?}");
            assert_eq!(
                stored.size,
                content.len() as u64,
                "taille, découpe {sizes:?}"
            );
            assert_eq!(stored.mime, "image/png", "type, découpe {sizes:?}");
            assert!(stored.fresh);
            let mut read = Vec::new();
            let mut file = store.open_file(&stored.hash).await.unwrap();
            file.read_to_end(&mut read).await.unwrap();
            drop(file);
            assert!(read == content, "octets relus, découpe {sizes:?}");
            store.remove(&guard, &stored.hash).await.unwrap();
        }
        // Zéro octet : aucun `write`.
        let upload = store.begin().await.unwrap();
        let guard = store.lock().await;
        let empty = upload.finish(&guard).await.unwrap();
        drop(guard);
        assert_eq!(
            hex(&empty.hash),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            (empty.size, empty.mime, empty.fresh),
            (0, OCTET_STREAM, true)
        );
        let mut read = Vec::new();
        let mut file = store.open_file(&empty.hash).await.unwrap();
        file.read_to_end(&mut read).await.unwrap();
        assert!(read.is_empty());
        assert!(!put(&store, b"").await.fresh);
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// Un envoi de `bytes` prêt à être posé : son temporaire, écrit, vidé sur le disque et fermé.
    async fn ready_to_place(store: &FileStore, bytes: &[u8]) -> Temporary {
        let mut upload = store.begin().await.unwrap();
        upload.write(bytes).await.unwrap();
        upload.sync().await.unwrap();
        let Upload { file, temp, .. } = upload;
        drop(file);
        temp
    }

    /// Le geste de `finish` dit dans quels dossiers il vient d'écrire une entrée : ce sont eux
    /// qu'il faut synchroniser pour que le contenu survive à une coupure de courant (R278). Le
    /// dossier de préfixe, où le contenu est entré ; `files/` aussi, quand ce dossier vient d'y
    /// être créé ; aucun pour un contenu trouvé là. (La synchronisation elle-même ne se fait que
    /// sous Unix : ce test ne tient que le choix des dossiers.)
    #[tokio::test]
    async fn the_gesture_says_which_folders_it_wrote_in() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let root = dir.path().join(FILES_DIR);
        // Deux contenus du même dossier de préfixe : le premier octet de leur hash est le même.
        let first = b"contenu 0".to_vec();
        let second = (1u32..)
            .map(|i| format!("contenu {i}").into_bytes())
            .find(|bytes| sha256(bytes)[0] == sha256(&first)[0])
            .unwrap();
        let (hash, other) = (sha256(&first), sha256(&second));
        let prefix = root.join(&hex(&hash)[..2]);
        let dirs = &store.dirs;

        let temp = ready_to_place(&store, &first).await;
        let placed = dirs.place(temp, &hash, first.len() as u64).unwrap();
        assert_eq!(placed, Placed::Fresh { new_prefix: true });
        assert_eq!(dirs.written(&hash, placed), [prefix.clone(), root.clone()]);

        let temp = ready_to_place(&store, &second).await;
        let placed = dirs.place(temp, &other, second.len() as u64).unwrap();
        assert_eq!(placed, Placed::Fresh { new_prefix: false });
        assert_eq!(dirs.written(&other, placed), std::slice::from_ref(&prefix));

        let temp = ready_to_place(&store, &first).await;
        let placed = dirs.place(temp, &hash, first.len() as u64).unwrap();
        assert_eq!(placed, Placed::Found);
        assert!(dirs.written(&hash, placed).is_empty());

        std::fs::write(dirs.content_path(&hash), b"tronque").unwrap();
        let temp = ready_to_place(&store, &first).await;
        let placed = dirs.place(temp, &hash, first.len() as u64).unwrap();
        assert_eq!(placed, Placed::Replaced);
        assert_eq!(dirs.written(&hash, placed), [prefix]);
        assert_eq!(std::fs::read(dirs.content_path(&hash)).unwrap(), first);
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// `missing` compte les contenus que la base connaît et que le dépôt ne peut pas servir : un
    /// fichier parti, jamais venu, ou autre chose à sa place (un dossier, un lien, R272). C'est
    /// ce que laisse une coupure de courant entre une attache validée et un renommage qui
    /// n'était pas encore sur le disque, ou une base restaurée sans son dossier `files/`.
    #[tokio::test]
    async fn missing_counts_what_the_database_knows_and_the_disk_lacks() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("ailleurs");
        std::fs::create_dir_all(&outside).unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let there = put(&store, b"toujours la").await;
        let gone = put(&store, b"parti du disque").await;
        std::fs::remove_file(store.dirs.content_path(&gone.hash)).unwrap();
        let never = sha256(b"jamais venu");
        let folder = sha256(b"un dossier a sa place");
        std::fs::create_dir_all(store.dirs.content_path(&folder)).unwrap();
        let link = sha256(b"un lien a sa place");
        let at = store.dirs.content_path(&link);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        link_dir(&outside, &at);

        assert_eq!(store.missing(&[]), 0);
        assert_eq!(store.missing(&[there.hash]), 0, "témoin");
        assert_eq!(
            store.missing(&[there.hash, gone.hash, never, folder, link]),
            4
        );
        // Rien n'a bougé : le compte ne touche à rien.
        assert!(store.open_file(&there.hash).await.is_ok());
        assert!(store.dirs.content_path(&folder).is_dir());
        assert!(std::fs::symlink_metadata(&at).is_ok());
    }

    /// Ce qui est à la place d'un contenu sans être un fichier ordinaire n'est jamais tenu pour
    /// lui, même s'il pèse autant que lui : l'envoi le remplace par le vrai contenu (R272).
    #[tokio::test]
    async fn something_else_of_the_same_size_is_not_the_content() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("ailleurs");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("precieux.txt"), b"a garder").unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        // Le poids que le système donne à un lien : zéro pour une jonction de Windows, la
        // longueur du chemin visé pour un lien symbolique d'Unix. Le contenu envoyé pèse autant.
        let probe = dir.path().join("sonde");
        link_dir(&outside, &probe);
        let weight = std::fs::symlink_metadata(&probe).unwrap().len();
        let content = vec![b'x'; usize::try_from(weight).unwrap()];
        let hash = sha256(&content);
        let path = store.dirs.content_path(&hash);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        link_dir(&outside, &path);
        assert_eq!(
            std::fs::symlink_metadata(&path).unwrap().len(),
            content.len() as u64,
            "témoin : le lien pèse autant que le contenu"
        );

        let stored = put(&store, &content).await;
        assert_eq!(stored.hash, hash);
        assert!(
            std::fs::symlink_metadata(&path).unwrap().is_file(),
            "le lien est resté à la place du contenu"
        );
        assert_eq!(std::fs::read(&path).unwrap(), content);
        assert_eq!(listing(&outside), ["precieux.txt"]);
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// Un contenu abîmé **à taille égale** est tenu pour le même, sans être relu : il n'est ni
    /// vu ni réparé par un nouvel envoi, et se relit abîmé (limite écrite au registre). Témoin :
    /// d'une autre taille, il est remplacé (`a_truncated_content_is_replaced_by_the_next_upload`).
    #[tokio::test]
    async fn a_damage_of_the_same_size_is_neither_seen_nor_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let content: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let stored = put(&store, &content).await;
        let path = store.dirs.content_path(&stored.hash);
        let mut damaged = content.clone();
        damaged[500] ^= 0xff;
        std::fs::write(&path, &damaged).unwrap();

        let again = put(&store, &content).await;
        assert!(!again.fresh);
        assert_eq!(again.hash, stored.hash);
        let mut read = Vec::new();
        let mut file = store.open_file(&stored.hash).await.unwrap();
        file.read_to_end(&mut read).await.unwrap();
        assert!(read == damaged, "le contenu abîmé est resté tel quel");
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// Un téléchargement en cours ne gêne ni le retrait du contenu, ni l'envoi suivant du même
    /// contenu, ni le remplacement d'un contenu abîmé ; et le lecteur finit sa lecture.
    #[tokio::test]
    async fn a_download_in_progress_blocks_neither_remove_nor_the_next_upload() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let content: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let stored = put(&store, &content).await;
        let path = store.dirs.content_path(&stored.hash);
        let mut reader = store.open_file(&stored.hash).await.unwrap();
        let guard = store.lock().await;
        store.remove(&guard, &stored.hash).await.unwrap();
        drop(guard);
        assert!(!path.exists(), "retiré sous un lecteur");
        let again = put(&store, &content).await;
        assert!(again.fresh, "reposé pendant que l'ancien est encore lu");
        let mut read = Vec::new();
        reader.read_to_end(&mut read).await.unwrap();
        assert!(read == content, "le lecteur finit sa lecture");
        // Un contenu abîmé, remplacé pendant qu'un lecteur le tient.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(3)
            .unwrap();
        let _reader = store.open_file(&stored.hash).await.unwrap();
        let third = put(&store, &content).await;
        assert!(!third.fresh);
        assert!(std::fs::read(&path).unwrap() == content);
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// La forme que la Task S4a donnera à l'envoi : dans une tâche détachée de la requête, le
    /// verrou pris sur un dépôt partagé, `finish`, puis le retrait du contenu qu'on vient de
    /// poser si l'attache échoue. Elle doit compiler (`Send + 'static`), et ne rien laisser.
    #[tokio::test]
    async fn the_upload_of_a_route_runs_in_a_task_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let files = Arc::new(FileStore::open(dir.path()).unwrap());
        let mut upload = files.begin().await.unwrap();
        upload.write(b"abc").await.unwrap();
        let task = tokio::spawn({
            let files = files.clone();
            async move {
                let guard = files.lock().await;
                let stored = upload.finish(&guard).await?;
                if stored.fresh {
                    files.remove(&guard, &stored.hash).await?;
                }
                io::Result::Ok(stored)
            }
        });
        assert!(task.await.unwrap().unwrap().fresh);
        assert!(listing(&dir.path().join(FILES_DIR)).is_empty());
    }

    /// Un fichier à la place d'un dossier de préfixe : rien ne s'y pose, ne s'y lit ni ne s'y
    /// retire, rien ne panique, et l'envoi ne laisse pas son temporaire.
    #[tokio::test]
    async fn a_prefix_that_cannot_be_made_fails_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let root = dir.path().join(FILES_DIR);
        std::fs::write(root.join("ba"), b"pas un dossier").unwrap();
        let mut upload = store.begin().await.unwrap();
        upload.write(b"abc").await.unwrap();
        let guard = store.lock().await;
        assert!(upload.finish(&guard).await.is_err());
        assert!(store.remove(&guard, &sha256(b"abc")).await.is_err());
        drop(guard);
        assert!(tmp_entries(dir.path()).is_empty());
        assert_eq!(listing(&root), ["ba"]);
        assert_eq!(
            store.open_file(&sha256(b"abc")).await.unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(store.strangers(&[]).unwrap(), (0, 0));
    }

    /// Sous Windows, un temporaire qu'un autre programme tient sans partager sa suppression (un
    /// antivirus, un indexeur) : rien ne panique ; un contenu neuf n'est pas posé (une erreur),
    /// un contenu déjà là est rendu ; le temporaire reste, et le démarrage suivant le retire
    /// (R279).
    #[cfg(windows)]
    #[tokio::test]
    async fn a_temporary_held_by_another_program_waits_for_the_next_opening() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        // Lecture et écriture partagées, pas la suppression (FILE_SHARE_READ | FILE_SHARE_WRITE).
        let hold = |path: &Path| {
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .open(path)
                .unwrap()
        };
        // Contenu neuf : le renommage échoue.
        let mut upload = store.begin().await.unwrap();
        upload.write(b"contenu neuf").await.unwrap();
        let held = hold(&upload.temp.path);
        let guard = store.lock().await;
        assert!(upload.finish(&guard).await.is_err());
        drop(guard);
        assert!(store.open_file(&sha256(b"contenu neuf")).await.is_err());
        assert_eq!(tmp_entries(dir.path()).len(), 1, "le temporaire reste");
        drop(held);
        // Contenu déjà là : il est rendu, le temporaire reste.
        let first = put(&store, b"deja la").await;
        let mut upload = store.begin().await.unwrap();
        upload.write(b"deja la").await.unwrap();
        let held = hold(&upload.temp.path);
        let guard = store.lock().await;
        let again = upload.finish(&guard).await.unwrap();
        drop(guard);
        assert_eq!((again.hash, again.fresh), (first.hash, false));
        assert_eq!(
            tmp_entries(dir.path()).len(),
            2,
            "les deux temporaires restent"
        );
        drop(held);
        // Le démarrage suivant les retire.
        drop(store);
        FileStore::open(dir.path()).unwrap();
        assert!(tmp_entries(dir.path()).is_empty());
    }

    /// Tient le dossier `path` ouvert sans le partager, comme le ferait un autre programme : tant
    /// que la valeur rendue vit, Windows refuse de lister ce dossier.
    #[cfg(windows)]
    fn hold_folder(path: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_BACKUP_SEMANTICS : ce qu'il faut pour ouvrir un dossier.
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .custom_flags(0x0200_0000)
            .open(path)
            .unwrap()
    }

    /// Ni le vidage de `tmp/` ni le compte des contenus inconnus n'arrêtent le démarrage pour ce
    /// qu'ils ne peuvent pas lire (R279) : c'est journalisé, et le reste est fait. Sous Windows,
    /// où un test peut rendre un dossier illisible sans changer ses droits.
    #[cfg(windows)]
    #[tokio::test]
    async fn what_cannot_be_read_at_startup_is_logged_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::open(dir.path()).unwrap();
        let first = put(&store, b"abc").await;
        let second = put(&store, b"un contenu d'un autre dossier").await;
        assert_ne!(
            hex(&first.hash)[..2],
            hex(&second.hash)[..2],
            "témoin : deux dossiers de préfixe"
        );
        let tmp = dir.path().join(FILES_DIR).join(TMP_DIR);
        std::fs::write(tmp.join("reste"), b"d'un arret brutal").unwrap();
        let logs = Logs::capture();

        // `tmp/` illisible : le dépôt s'ouvre, le reste attend le démarrage suivant.
        let held = hold_folder(&tmp);
        assert!(
            std::fs::read_dir(&tmp).is_err(),
            "témoin : tmp/ ne se liste plus"
        );
        let store = FileStore::open(dir.path()).unwrap();
        assert!(logs.text().contains("WARN"), "{}", logs.text());
        // Et il sert : un envoi s'y écrit et se pose.
        assert!(put(&store, b"un envoi quand meme").await.fresh);
        drop(held);
        assert_eq!(tmp_entries(dir.path()), ["reste"]);
        let store = FileStore::open(dir.path()).unwrap();
        assert!(tmp_entries(dir.path()).is_empty());

        // Un dossier de préfixe illisible : les contenus des autres sont comptés.
        let known = [sha256(b"un envoi quand meme")];
        let held = hold_folder(&store.dirs.prefix_path(&first.hash));
        assert_eq!(store.strangers(&known).unwrap(), (1, second.size));
        assert!(
            logs.text().contains("n'a pas pu être lue"),
            "{}",
            logs.text()
        );
        drop(held);
        assert_eq!(
            store.strangers(&known).unwrap(),
            (2, first.size + second.size)
        );
    }
}
