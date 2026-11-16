//! Routes des fichiers d'une page (Rulings R244 à R247, Task S4a) : envoyer un fichier dans une
//! page, et le lire. Un fichier appartient à la page où il a été envoyé : son adresse la nomme
//! (`/api/v1/pages/{id}/files/{fileId}`, l'identifiant d'une **attache** de la page), et ce sont
//! les droits sur elle, au moment de chaque lecture, qui décident (Ruling R245).
//!
//! | Route | Il faut | Réponse |
//! |---|---|---|
//! | `GET /api/v1/files/limits` | une session | [`FileLimits`] |
//! | `POST /api/v1/pages/{id}/files?name=…` | `edit` | `201`, [`FileInfo`] ; `409 trashed` ; `413 file_too_large` ; droits et session revérifiés une fois le corps reçu |
//! | `GET /api/v1/pages/{id}/files/{fileId}` | `read` (à la corbeille aussi) | les octets, en téléchargement (en-têtes ci-dessous) ; `304`, `206`, `416` ; un fichier qui n'est pas de la page, inconnu ou mal nommé : la réponse d'une page introuvable |
//!
//! **Envoyer** (Ruling R247), dans cet ordre : `401` ; `404` pour une page qu'on ne voit pas ;
//! `403` sous « Peut modifier » ; `409 trashed` ; `422 invalid_request` pour un paramètre `name`
//! illisible (donné deux fois ; absent, le nom est « fichier ») ; `413 file_too_large` sur
//! `Content-Length`, avant de rien lire. Puis le corps, brut, lu trame par trame et écrit dans le
//! dépôt en calculant son hash, jamais gardé en mémoire : `413` dès qu'il dépasse la borne
//! (`MEMENTUM_MAX_FILE_MB`, 100 Mio par défaut) ; un corps qui ne reçoit plus rien pendant
//! `upload_idle` (30 s) est abandonné, la connexion fermée **sans réponse**, et c'est journalisé
//! (Ruling R391). Le corps reçu, la session et les droits sont relus (Rulings R89, R390) : c'est
//! ce second contrôle qui autorise l'écriture. Le contenu est vidé sur le disque hors de tout
//! verrou (R286), puis, sous le verrou du dépôt et dans une tâche que l'abandon de la requête
//! n'arrête pas (`to_completion`, R285) : il prend sa place (`finish`), puis son attache est
//! écrite. Si l'attache échoue (la page supprimée entre le second contrôle et l'écriture), un
//! contenu que cet envoi vient de poser lui-même est retiré aussitôt, sous le même verrou ; un
//! contenu qui était déjà là reste (Ruling R395). Rien n'est envoyé « par hash » : qui attache
//! des octets les a envoyés, et la réponse est la même que le contenu soit déjà connu ou non
//! (R244). La limite de 2 Mio des autres corps est levée sur cette route seule.
//!
//! **Lire** : `404` pour une page qu'on ne voit pas, puis le `404` d'un fichier qui n'est pas
//! d'elle (le corps d'une page introuvable, comme une version, R186). La corbeille n'empêche pas
//! de lire. **Toute réponse qui porte un fichier est un téléchargement** (Ruling R246) :
//! `Content-Disposition: attachment` (le nom sous ses deux formes), `X-Content-Type-Options:
//! nosniff`, `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; sandbox`,
//! `Cross-Origin-Resource-Policy: same-origin`, `Accept-Ranges: bytes`, et le type que le serveur
//! a lu dans les premiers octets à l'envoi (jamais celui que le navigateur a déclaré). La page ne
//! montre un fichier « en ligne » que comme sous-ressource d'un bloc image, vidéo ou son. Cache :
//! `Cache-Control: private, no-cache` et `ETag: "<hash>"`, donc une revalidation, et un contrôle
//! des droits, à chaque usage ; ces réponses portent la marque [`FileResponse`], seule à échapper
//! au `no-store` du reste de l'API. `If-None-Match` est jugé avant `Range` (RFC 9110, §13.2.2) :
//! s'il nomme l'`ETag`, `304`, quelle que soit la plage ; un `If-Range` qui ne nomme pas l'`ETag`
//! fait ignorer `Range` ; une plage simple est servie (`206`), une plage qui commence après la
//! fin reçoit `416` et `Content-Range: bytes */<taille>`, deux plages ou un en-tête illisible le
//! fichier entier (Ruling R392). Le `304` et le `206` portent les mêmes en-têtes que le `200`.
//! Un contenu que le dépôt n'a plus, ou dont la taille n'est plus celle que la base garde, est
//! dit « fichier indisponible » (`404`, Ruling R393) : la route n'annonce jamais une taille
//! qu'elle n'a pas lue sur le fichier ouvert. Le fichier est servi par morceaux de 64 Kio, sans
//! être tenu en mémoire ; un téléchargement en cours va au bout, même si le droit tombe pendant
//! qu'il passe (une fenêtre acceptée, conception §5.6).
//!
//! Les erreurs du dépôt portent des chemins du serveur : elles vont au journal, jamais dans une
//! réponse (`ApiError::internal`).

use std::io::{self, SeekFrom};
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use axum::Json;
use axum::body::Body;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use http_body::{Frame, SizeHint};
use http_body_util::BodyExt;
use mementum_acl::Action;
use mementum_store::{Attachment, NewAttachment};
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncSeekExt, ReadBuf};
use uuid::Uuid;

use crate::auth::{AuthUser, SessionUser};
use crate::error::ApiError;
use crate::files;
use crate::guard::FileResponse;
use crate::routes::pages::page_id;
use crate::types::{FileInfo, FileLimits};
use crate::{AppState, now_ms, to_completion};

/// Octets lus sur le disque à chaque morceau d'un fichier servi.
const CHUNK: usize = 64 * 1024;

/// La politique de contenu de tout fichier servi : rien ne s'y charge ni ne s'y exécute, et un
/// document ouvert l'est dans un bac à sable (Ruling R246).
const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

/// `Cross-Origin-Resource-Policy`, que la crate `http` ne nomme pas.
const CORP: HeaderName = HeaderName::from_static("cross-origin-resource-policy");

/// La requête d'un envoi : `?name=<nom du fichier>`, un libellé.
#[derive(Debug, Deserialize)]
pub(crate) struct UploadQuery {
    name: Option<String>,
}

/// `GET /api/v1/files/limits` : les bornes d'un envoi.
pub(crate) async fn limits(State(state): State<AppState>, _user: AuthUser) -> Json<FileLimits> {
    Json(FileLimits {
        max_file_bytes: state.max_file_bytes,
    })
}

/// `POST /api/v1/pages/{id}/files?name=<nom>` : le corps est le fichier, brut.
pub(crate) async fn upload(
    State(state): State<AppState>,
    caller: SessionUser,
    path: Result<Path<String>, PathRejection>,
    query: Result<Query<UploadQuery>, QueryRejection>,
    headers: HeaderMap,
    mut body: Body,
) -> Result<Response, ApiError> {
    let SessionUser { user, session } = caller;
    let id = page_id(path)?;
    // Premier contrôle : les refus, avant de rien lire.
    state.node_access(&user, id, Action::Edit).await?;
    let Query(query) = query.map_err(|_| ApiError::invalid_file_name())?;
    let name = files::display_name(query.name.as_deref().unwrap_or(""));
    let max = state.max_file_bytes;
    if declared_length(&headers).is_some_and(|length| length > max) {
        return Err(ApiError::file_too_large(max));
    }

    let mut upload = state
        .files
        .begin()
        .await
        .map_err(|e| ApiError::internal(&e))?;
    loop {
        // Le délai entoure la lecture d'une trame, jamais l'écriture (R273) : un futur de
        // `write` lâché en route mettrait l'envoi hors d'usage.
        let frame = match tokio::time::timeout(state.upload_idle, body.frame()).await {
            Err(_) => {
                tracing::warn!(
                    page = %id,
                    user = %user.id,
                    received = upload.len(),
                    idle = ?state.upload_idle,
                    "envoi abandonné : aucun octet reçu pendant le délai ; la connexion est \
                     fermée sans réponse, et rien n'est gardé"
                );
                return Ok(abandoned());
            }
            Ok(None) => break,
            Ok(Some(Err(error))) => {
                tracing::info!(
                    page = %id,
                    user = %user.id,
                    received = upload.len(),
                    %error,
                    "envoi interrompu avant la fin du fichier : rien n'est gardé"
                );
                return Err(ApiError::interrupted_upload());
            }
            Ok(Some(Ok(frame))) => frame,
        };
        // Des en-têtes de fin, s'il y en a, ne sont pas du fichier.
        let Ok(data) = frame.into_data() else {
            continue;
        };
        if upload.len() + data.len() as u64 > max {
            return Err(ApiError::file_too_large(max));
        }
        upload
            .write(&data)
            .await
            .map_err(|e| ApiError::internal(&e))?;
    }

    // Second contrôle, le corps reçu (Rulings R89, R390) : c'est lui qui autorise l'écriture.
    state.session_still_live(session).await?;
    state.node_access(&user, id, Action::Edit).await?;
    // Vidé sur le disque avant de prendre le verrou du dépôt, que les autres envois attendent
    // (R286).
    upload.sync().await.map_err(|e| ApiError::internal(&e))?;
    let (store, files) = (state.store.clone(), state.files.clone());
    let attachment = to_completion(async move {
        let guard = files.lock().await;
        let stored = upload
            .finish(&guard)
            .await
            .map_err(|e| ApiError::internal(&e))?;
        let new = NewAttachment {
            id: Uuid::now_v7(),
            node_id: id,
            hash: stored.hash,
            size: stored.size as i64,
            mime: stored.mime,
            name: &name,
            created_by: user.id,
        };
        match store.attach_file(new, now_ms()).await {
            Ok(attachment) => Ok(attachment),
            Err(error) => {
                // Ruling R395 : un contenu posé par cet envoi, et que rien n'attache, repart ;
                // un contenu qui était déjà là reste. Sous le même garde : aucun autre envoi ne
                // l'a trouvé « déjà là » entre-temps.
                if stored.fresh
                    && let Err(removal) = files.remove(&guard, &stored.hash).await
                {
                    tracing::error!(
                        %removal,
                        "contenu posé par un envoi dont l'attache a échoué, impossible à \
                         retirer : il reste sur le disque, compté au prochain démarrage"
                    );
                }
                Err(ApiError::from(error))
            }
        }
    })
    .await?;
    Ok((StatusCode::CREATED, Json(file_info(&attachment))).into_response())
}

/// `GET /api/v1/pages/{id}/files/{fileId}` : le fichier, en téléchargement.
pub(crate) async fn download(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (id, raw) = file_path(path)?;
    state.node_access(&user, id, Action::Read).await?;
    // Un fichier qui n'est pas de cette page, inconnu ou mal nommé : la page est introuvable.
    let file_id = Uuid::parse_str(&raw).map_err(|_| ApiError::page_not_found())?;
    let attachment = state
        .store
        .attachment(id, file_id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    let etag = format!("\"{}\"", files::hex(&attachment.hash));
    if headers
        .get(header::IF_NONE_MATCH)
        .is_some_and(|value| none_match(value, &etag))
    {
        return Ok(served(
            StatusCode::NOT_MODIFIED,
            &attachment,
            &etag,
            Body::empty(),
        ));
    }

    let mut file = match state.files.open_file(&attachment.hash).await {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(unavailable(&attachment, "absent du dépôt"));
        }
        Err(error) => return Err(ApiError::internal(&error)),
    };
    let size = file
        .metadata()
        .await
        .map_err(|e| ApiError::internal(&e))?
        .len();
    if i64::try_from(size).ok() != Some(attachment.size) {
        return Err(unavailable(
            &attachment,
            "d'une autre taille que celle de la base",
        ));
    }
    match wanted(&headers, &etag, size) {
        Wanted::Whole => {
            let mut response = served(StatusCode::OK, &attachment, &etag, file_body(file, size));
            response
                .headers_mut()
                .insert(header::CONTENT_LENGTH, HeaderValue::from(size));
            Ok(response)
        }
        Wanted::Part { start, end } => {
            file.seek(SeekFrom::Start(start))
                .await
                .map_err(|e| ApiError::internal(&e))?;
            let length = end - start + 1;
            let mut response = served(
                StatusCode::PARTIAL_CONTENT,
                &attachment,
                &etag,
                file_body(file, length),
            );
            let headers = response.headers_mut();
            headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
            let range = format!("bytes {start}-{end}/{size}");
            headers.insert(
                header::CONTENT_RANGE,
                HeaderValue::try_from(range).map_err(|e| ApiError::internal(&e))?,
            );
            Ok(response)
        }
        Wanted::Unsatisfiable => {
            let range = HeaderValue::try_from(format!("bytes */{size}"))
                .map_err(|e| ApiError::internal(&e))?;
            Ok((
                [(header::CONTENT_RANGE, range)],
                ApiError::range_not_satisfiable(),
            )
                .into_response())
        }
    }
}

/// `413` sur `Content-Length` : la longueur annoncée, si elle se lit.
fn declared_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// La page et l'identifiant (encore à lire) du fichier d'un chemin `…/files/{fileId}`. Un
/// chemin illisible : la page n'existe pas.
fn file_path(
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<(Uuid, String), ApiError> {
    let Path((page, file)) = path.map_err(|_| ApiError::page_not_found())?;
    let page = Uuid::parse_str(&page).map_err(|_| ApiError::page_not_found())?;
    Ok((page, file))
}

/// Ce que l'API dit d'une attache : son adresse se lit avec « Peut lire » sur sa page.
fn file_info(attachment: &Attachment) -> FileInfo {
    FileInfo {
        id: attachment.id.to_string(),
        url: format!(
            "/api/v1/pages/{}/files/{}",
            attachment.node_id, attachment.id
        ),
        name: attachment.name.clone(),
        size: attachment.size,
        mime: attachment.mime.clone(),
    }
}

/// « Fichier indisponible » (Ruling R393) : le journal dit lequel, et pourquoi ; la réponse ne
/// nomme rien du serveur.
fn unavailable(attachment: &Attachment, why: &str) -> ApiError {
    tracing::warn!(
        page = %attachment.node_id,
        file = %attachment.id,
        hash = %files::hex(&attachment.hash),
        "fichier indisponible : contenu {why} (base restaurée sans son dossier files/, coupure \
         de courant juste après l'envoi, ou contenu abîmé)"
    );
    ApiError::file_unavailable()
}

/// Une réponse de fichier servi, de statut `status`, avec les en-têtes de Ruling R246 et la
/// marque [`FileResponse`]. `Content-Length` et `Content-Range` sont à la charge de l'appelant.
fn served(status: StatusCode, attachment: &Attachment, etag: &str, body: Body) -> Response {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    let mime = HeaderValue::try_from(attachment.mime.as_str())
        .unwrap_or(HeaderValue::from_static(files::OCTET_STREAM));
    let etag = HeaderValue::try_from(etag).unwrap_or(HeaderValue::from_static("\"\""));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, mime);
    headers.insert(
        header::CONTENT_DISPOSITION,
        files::content_disposition(&attachment.name),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    headers.insert(CORP, HeaderValue::from_static("same-origin"));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-cache"),
    );
    headers.insert(header::ETAG, etag);
    response.extensions_mut().insert(FileResponse);
    response
}

/// `If-None-Match` nomme-t-il l'`ETag` du fichier ? `*`, ou l'un des `ETag` de la liste, en
/// comparaison faible (RFC 9110, §13.1.2 : `W/"x"` vaut `"x"`).
fn none_match(value: &HeaderValue, etag: &str) -> bool {
    let Ok(value) = value.to_str() else {
        return false;
    };
    value
        .split(',')
        .map(str::trim)
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag)
}

/// Ce que la requête demande du fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wanted {
    /// Le fichier entier : pas de `Range`, un `Range` illisible ou de plusieurs plages, ou un
    /// `If-Range` qui ne nomme pas l'`ETag`.
    Whole,
    /// Les octets `start` à `end`, compris.
    Part { start: u64, end: u64 },
    /// Une plage qui commence après la fin du fichier, ou de longueur nulle (`bytes=-0`).
    Unsatisfiable,
}

/// La plage demandée d'un fichier de `size` octets (Ruling R392) : une seule, `bytes=a-b`,
/// `bytes=a-` ou `bytes=-n`, sans espace ; sa fin est ramenée à celle du fichier. Un fichier vide
/// n'a aucun octet à donner : `Range` y est ignoré (RFC 9110, §14.2 : un serveur peut l'ignorer).
fn wanted(headers: &HeaderMap, etag: &str, size: u64) -> Wanted {
    let Some(range) = headers.get(header::RANGE) else {
        return Wanted::Whole;
    };
    if size == 0 {
        return Wanted::Whole;
    }
    // Un `If-Range` se compare fortement : un `ETag` faible, ou une date, ne nomme pas ce
    // contenu-ci (RFC 9110, §13.1.5).
    if headers
        .get(header::IF_RANGE)
        .is_some_and(|value| value.as_bytes() != etag.as_bytes())
    {
        return Wanted::Whole;
    }
    let Some((unit, spec)) = range.to_str().ok().and_then(|r| r.split_once('=')) else {
        return Wanted::Whole;
    };
    if !unit.eq_ignore_ascii_case("bytes") || spec.contains(',') {
        return Wanted::Whole;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return Wanted::Whole;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    // Des chiffres, déjà vérifiés : seul un nombre trop grand ne se lit pas.
    let number = |s: &str| s.parse::<u64>().unwrap_or(u64::MAX);
    let last_byte = size - 1;
    match (digits(first), digits(last)) {
        (false, true) if first.is_empty() => match number(last) {
            0 => Wanted::Unsatisfiable,
            suffix => Wanted::Part {
                start: size.saturating_sub(suffix),
                end: last_byte,
            },
        },
        (true, false) if last.is_empty() => match number(first) {
            start if start > last_byte => Wanted::Unsatisfiable,
            start => Wanted::Part {
                start,
                end: last_byte,
            },
        },
        (true, true) => {
            let (start, end) = (number(first), number(last));
            if end < start {
                Wanted::Whole
            } else if start > last_byte {
                Wanted::Unsatisfiable
            } else {
                Wanted::Part {
                    start,
                    end: end.min(last_byte),
                }
            }
        }
        _ => Wanted::Whole,
    }
}

/// Le corps d'une réponse qui sert `length` octets de `file`, depuis sa position courante.
fn file_body(file: tokio::fs::File, length: u64) -> Body {
    let buffer = usize::try_from(length).unwrap_or(usize::MAX).min(CHUNK);
    Body::new(FileBody {
        file,
        remaining: length,
        buffer: vec![0; buffer].into_boxed_slice(),
    })
}

/// Un fichier servi par morceaux de [`CHUNK`] octets au plus : seul un morceau est en mémoire à
/// la fois, en plus du tampon de tokio (Ruling R247).
struct FileBody {
    file: tokio::fs::File,
    remaining: u64,
    buffer: Box<[u8]>,
}

impl http_body::Body for FileBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        let this = self.get_mut();
        if this.remaining == 0 {
            return Poll::Ready(None);
        }
        let wanted = usize::try_from(this.remaining)
            .unwrap_or(usize::MAX)
            .min(this.buffer.len());
        let mut read = ReadBuf::new(&mut this.buffer[..wanted]);
        ready!(Pin::new(&mut this.file).poll_read(cx, &mut read))?;
        let filled = read.filled();
        if filled.is_empty() {
            // Raccourci pendant qu'on le servait : la réponse s'arrête, la connexion avec elle.
            return Poll::Ready(Some(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "le contenu a raccourci pendant qu'on le servait",
            ))));
        }
        this.remaining -= filled.len() as u64;
        Poll::Ready(Some(Ok(Frame::data(Bytes::copy_from_slice(filled)))))
    }

    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.remaining)
    }
}

/// La réponse d'un envoi abandonné, que le serveur n'écrit jamais (Ruling R391) : son corps
/// échoue avant son premier octet, et hyper ferme alors la connexion sans rien envoyer, pas
/// même la ligne d'état (sa tête de réponse est encore dans son tampon). Le statut, `408`, ne
/// sert qu'à qui la verrait sans passer par hyper (un test en mémoire).
fn abandoned() -> Response {
    let mut response = Response::new(Body::new(Abandoned));
    *response.status_mut() = StatusCode::REQUEST_TIMEOUT;
    response
}

/// Le corps de [`abandoned`] : une erreur, tout de suite.
struct Abandoned;

impl http_body::Body for Abandoned {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        Poll::Ready(Some(Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "envoi abandonné : la connexion est fermée sans réponse",
        ))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(pairs: &[(HeaderName, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(name, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    /// Les formes d'une plage, et ce qu'elles demandent d'un fichier de dix octets.
    #[test]
    fn ranges_are_read_as_ruling_r392_says() {
        let etag = "\"abc\"";
        let range = |value: &str| wanted(&with(&[(header::RANGE, value)]), etag, 10);
        let part = |start, end| Wanted::Part { start, end };
        assert_eq!(range("bytes=0-"), part(0, 9));
        assert_eq!(range("BYTES=0-0"), part(0, 0));
        assert_eq!(range("bytes=-3"), part(7, 9));
        assert_eq!(range("bytes=-99999999999999999999999"), part(0, 9));
        assert_eq!(range("bytes=3-99999999999999999999999"), part(3, 9));
        assert_eq!(range("bytes=10-"), Wanted::Unsatisfiable);
        assert_eq!(
            range("bytes=99999999999999999999999-"),
            Wanted::Unsatisfiable
        );
        assert_eq!(range("bytes=10-12"), Wanted::Unsatisfiable);
        assert_eq!(range("bytes=-0"), Wanted::Unsatisfiable);
        for whole in [
            "bytes=0-1, 3-4",
            "bytes=3-2",
            "bytes=x-",
            "bytes",
            "items=0-1",
            "bytes=+1-2",
        ] {
            assert_eq!(range(whole), Wanted::Whole, "{whole}");
        }
        assert_eq!(wanted(&with(&[]), etag, 10), Wanted::Whole);
        // Un fichier vide : la plage est ignorée.
        assert_eq!(
            wanted(&with(&[(header::RANGE, "bytes=-5")]), etag, 0),
            Wanted::Whole
        );
        // `If-Range` : seul l'`ETag` exact garde la plage.
        let if_range = |value: &str| {
            wanted(
                &with(&[(header::RANGE, "bytes=0-0"), (header::IF_RANGE, value)]),
                etag,
                10,
            )
        };
        assert_eq!(if_range("\"abc\""), part(0, 0));
        assert_eq!(if_range("W/\"abc\""), Wanted::Whole);
        assert_eq!(if_range("\"autre\""), Wanted::Whole);
    }

    /// `If-None-Match` : `*`, ou l'`ETag` dans la liste, fort ou faible.
    #[test]
    fn if_none_match_compares_weakly() {
        let matches = |value: &str| none_match(&HeaderValue::from_str(value).unwrap(), "\"abc\"");
        for yes in [
            "\"abc\"",
            "W/\"abc\"",
            "*",
            "\"x\", \"abc\"",
            " \"x\" ,W/\"abc\" ",
        ] {
            assert!(matches(yes), "{yes}");
        }
        for no in ["\"abcd\"", "abc", "\"ab\"c", "", "W/abc"] {
            assert!(!matches(no), "{no}");
        }
    }
}
