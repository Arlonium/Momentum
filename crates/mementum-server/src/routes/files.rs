//! Routes des fichiers d'une page (Rulings R244 à R247, Tasks S4a et S4b) : envoyer un fichier
//! dans une page, le lire, recopier son attache dans une autre page ; et le ménage, à la
//! suppression définitive d'une page. Un fichier appartient à la page où il a été envoyé : son
//! adresse la nomme (`/api/v1/pages/{id}/files/{fileId}`, l'identifiant d'une **attache** de la
//! page), et ce sont les droits sur elle, au moment de chaque lecture, qui décident (Ruling R245).
//!
//! | Route | Il faut | Réponse |
//! |---|---|---|
//! | `GET /api/v1/files/limits` | une session | [`FileLimits`] |
//! | `POST /api/v1/pages/{id}/files?name=…` | `edit` | `201`, [`FileInfo`] ; `409 trashed` ; `413 file_too_large` ; droits et session revérifiés une fois le corps reçu |
//! | `POST /api/v1/pages/{id}/files/copies` | `edit` ici, `read` sur la page d'origine (à la corbeille aussi) | `201`, [`FileInfo`] d'une attache neuve de cette page, au même contenu et au même nom ; `409 trashed` ; `422 invalid_source` ; un `GET` à cette adresse répond `405`, pour toute page (Ruling R559) |
//! | `GET /api/v1/pages/{id}/files/{fileId}` | `read` (à la corbeille aussi) | les octets, en téléchargement (en-têtes ci-dessous) ; `304`, `206`, `416` ; un fichier qui n'est pas de la page, inconnu ou mal nommé : la réponse d'une page introuvable |
//!
//! **Recopier** (Ruling R249) : un bloc collé dans une autre page garde l'adresse de sa première
//! page ; le client fait recopier l'attache dans la page où il colle, puis récrit l'adresse. Rien
//! n'est renvoyé : l'attache neuve nomme le même contenu, et vit autant que sa page à elle. Dans
//! cet ordre : `401` (puis le `409 account_changed` d'un en-tête qui nomme un autre compte, R540)
//! ; `404` pour une page qu'on ne voit pas ; `403` sous « Peut modifier » ; `409 trashed` ; puis
//! le corps, [`CopyFile`] : `invalid_json` (`400` illisible, `422` d'une autre forme, un champ de
//! plus compris), puis `422 invalid_source` pour une page d'origine qu'on ne voit pas, inconnue ou
//! mal nommée, et pour un fichier qui n'est pas d'elle, inconnu ou mal nommé : **la même
//! réponse**, jamais `403` ni `404` (Ruling R558). La transaction qui écrit la copie, dans la
//! voie du compte (Ruling R522), relit la page d'ici (supprimée : page introuvable ; à la
//! corbeille : `409 trashed`, Ruling R577) et la source (le fichier est encore de sa page). Les
//! droits, eux, ne sont pas relus : entre leur contrôle et l'écriture, il ne reste que l'attente du
//! tour de la voie du compte, que ses propres écritures en file allongent ; un droit retiré pendant
//! cette attente n'est pas vu (une fenêtre acceptée, conception §5.6).
//!
//! **Envoyer** (Ruling R247), dans cet ordre : `401` ; `409 account_changed` (ou `400`) pour un
//! en-tête `X-Mementum-Account` qui ne nomme pas le compte de la session (Ruling R540) ; `404`
//! pour une page qu'on ne voit pas ; `403` sous « Peut modifier » ; `409 trashed` ; `422
//! invalid_request` pour un paramètre `name` illisible (donné deux fois ; absent, le nom est
//! « fichier ») ; `413 file_too_large` sur `Content-Length`, avant de rien lire. **Un refus fait
//! à un compte** (`400` ou `409` de l'en-tête, `403`, `404`, `409`, `422`), pour un corps annoncé
//! sous la borne, ne part qu'une fois ce corps lu et jeté (au plus la
//! borne, et jamais plus de `upload_idle` sans un octet) : sinon le serveur fermerait sur des
//! octets non lus, le système couperait la connexion, et le navigateur, qui envoie son corps sans
//! attendre, perdrait la réponse (Rulings R505, R545). Le `401` (aucun compte) et le `413` sur
//! `Content-Length` coupent sans lire : le client doit lire la borne avant d'envoyer, et tenir une
//! erreur réseau pendant un envoi pour un refus qu'il n'a pas reçu. Sous `Expect: 100-continue`,
//! rien n'est lu : le refus arrive seul. Tout compte de l'instance peut ainsi faire lire au serveur
//! jusqu'à la borne par requête (débit au lot 7) : une lecture jetée de plus de 1 Mio est
//! journalisée en `info`, avec le compte, la page et les octets. Puis le corps, brut, lu trame par trame et écrit dans le
//! dépôt en calculant son hash, jamais gardé en mémoire : `413` dès qu'il dépasse la borne
//! (`MEMENTUM_MAX_FILE_MB`, 100 Mio par défaut) ; un corps qui ne reçoit plus rien pendant
//! `upload_idle` (30 s) est abandonné, la connexion fermée **sans réponse**, et c'est journalisé
//! (Ruling R391, en HTTP/1, le seul protocole du serveur : R509). Ce délai borne l'inactivité,
//! pas la durée : un envoi qui reçoit un octet de temps en temps tient sa connexion (débit et
//! envois simultanés au lot 7, R247). Le contenu est vidé sur le disque hors de tout verrou
//! (R286) ; puis, la session et les droits sont relus (Rulings R89, R390) : c'est ce second
//! contrôle qui autorise l'écriture, et il ne reste après lui que l'attente des verrous (R507).
//! Puis, sous le verrou du dépôt et dans une tâche que l'abandon de la requête
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
//! au `no-store` du reste de l'API. `If-None-Match`, toutes ses lignes lues (R508), est jugé
//! avant `Range` (RFC 9110, §13.2.2) : s'il nomme l'`ETag`, `304`, quelle que soit la plage ; un
//! `If-Range` qui ne nomme pas l'`ETag` fait ignorer `Range` ; une plage simple est servie
//! (`206`), une plage qui commence après la fin reçoit `416` et `Content-Range: bytes
//! */<taille>`, deux plages (sur une ligne ou sur deux, R508) ou un en-tête illisible le fichier
//! entier (Ruling R392). Le `304` et le `206` portent les mêmes en-têtes que le `200`.
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
use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
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

use crate::access::permit;
use crate::auth::{AuthUser, SessionUser};
use crate::error::ApiError;
use crate::files;
use crate::guard::FileResponse;
use crate::routes::pages::page_id;
use crate::types::{CopyFile, ErrorCode, FileInfo, FileLimits};
use crate::{AppState, now_ms, to_completion};

/// Octets lus sur le disque à chaque morceau d'un fichier servi.
const CHUNK: usize = 64 * 1024;

/// Une lecture jetée de plus de tant d'octets (Ruling R505) est journalisée.
const LOGGED_DISCARD: u64 = 1 << 20;

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
    caller: Result<SessionUser, ApiError>,
    path: Result<Path<String>, PathRejection>,
    query: Result<Query<UploadQuery>, QueryRejection>,
    headers: HeaderMap,
    mut body: Body,
) -> Result<Response, ApiError> {
    // Sans compte (`401`), la réponse part sans rien lire du corps (Ruling R505). Les autres
    // refus de l'extracteur sont faits à un compte (un en-tête qui en nomme un autre, R545) : ils
    // passent par le premier contrôle, comme les suivants.
    let caller = match caller {
        Err(refusal) if refusal.code() == ErrorCode::Unauthenticated => return Err(refusal),
        caller => caller,
    };
    let max = state.max_file_bytes;
    let declared = declared_length(&headers);
    // Le compte et la page nommée, pour le journal d'une lecture jetée : `None` si l'extracteur a
    // refusé, ou si le chemin ne se lit pas.
    let account = caller.as_ref().ok().map(|caller| caller.user.id);
    let named = path
        .as_ref()
        .ok()
        .and_then(|Path(raw)| Uuid::parse_str(raw).ok());
    // Premier contrôle : les refus, avant de rien garder.
    let first = async {
        let SessionUser { user, session } = caller?;
        let id = page_id(path)?;
        state.node_access(&user, id, Action::Edit).await?;
        let Query(query) = query.map_err(|_| ApiError::invalid_file_name())?;
        let name = files::display_name(query.name.as_deref().unwrap_or(""));
        Ok::<_, ApiError>((user, session, id, name))
    };
    let (user, session, id, name) = match first.await {
        Ok(checked) => checked,
        Err(refusal) => {
            // Ruling R505 : un refus fait à un compte (`403`, `404`, `409`, `422`) n'arrive à
            // un client qui pousse son corps que si ce corps est lu ; sinon le serveur ferme sur
            // des octets non lus, le système coupe, et la réponse est perdue.
            if declared.is_none_or(|length| length <= max) && !expects_continue(&headers) {
                let discarded = discard(&mut body, max, state.upload_idle).await;
                // Tout compte de l'instance peut faire lire ainsi au serveur jusqu'à la borne,
                // à chaque requête, avant les plafonds du lot 7 : l'exploitant le voit ici.
                if discarded > LOGGED_DISCARD {
                    tracing::info!(
                        user = ?account,
                        page = ?named,
                        discarded,
                        "corps d'un envoi refusé lu et jeté, pour que le refus arrive au client"
                    );
                }
            }
            return Err(refusal);
        }
    };
    // Annoncé plus gros que la borne : refusé sans rien lire (R247).
    if declared.is_some_and(|length| length > max) {
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

    // Vidé sur le disque avant le second contrôle, et donc avant de prendre le verrou du dépôt,
    // que les autres envois attendent (R286) : entre le contrôle et l'écriture, il ne reste que
    // l'attente des verrous du dépôt et de SQLite (Ruling R507).
    upload.sync().await.map_err(|e| ApiError::internal(&e))?;
    // Second contrôle, le corps reçu (Rulings R89, R390) : c'est lui qui autorise l'écriture.
    state.session_still_live(session).await?;
    state.node_access(&user, id, Action::Edit).await?;
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

/// `POST /api/v1/pages/{id}/files/copies` : recopie dans cette page l'attache d'un fichier d'une
/// autre page (Ruling R249), sans renvoyer un octet.
pub(crate) async fn copy(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<CopyFile>, JsonRejection>,
) -> Result<(StatusCode, Json<FileInfo>), ApiError> {
    let id = page_id(path)?;
    // Les refus de droits d'abord, avant le corps : `404`, `403`, `409 trashed`.
    state.node_access(&user, id, Action::Edit).await?;
    let Json(CopyFile { page_id, file_id }) = body?;
    // Ruling R558 : une source qu'on ne voit pas reçoit la réponse d'une source inconnue, jamais
    // `403` ni `404`. Une page à la corbeille se lit, donc se recopie.
    let (Ok(from), Ok(file)) = (Uuid::parse_str(&page_id), Uuid::parse_str(&file_id)) else {
        return Err(ApiError::invalid_source());
    };
    let Some((_, access, _)) = state.seen_node(&user, from).await? else {
        return Err(ApiError::invalid_source());
    };
    permit(&access, Action::Read).map_err(|_| ApiError::invalid_source())?;
    // Une seule transaction, dans la voie du compte (Ruling R522), qui relit la page d'ici
    // (partie : `NotFound`, la réponse d'une page introuvable ; à la corbeille : `Trashed`, `409
    // trashed`, Ruling R577), puis la source (`None` : le fichier n'est pas, ou plus, de cette
    // page). Les droits ne sont pas relus : entre leur contrôle et l'écriture, il ne reste que
    // l'attente du tour de la voie du compte, que ses propres écritures allongent (R249, R577).
    let copied = state
        .store
        .copy_attachment(from, file, Uuid::now_v7(), id, user.id, now_ms())
        .await?
        .ok_or_else(ApiError::invalid_source)?;
    Ok((StatusCode::CREATED, Json(file_info(&copied))))
}

/// Contenus que le ménage retire au plus sous une même prise du verrou du dépôt (Ruling R556) :
/// entre deux paquets, il rend le verrou, et les envois qui l'attendaient passent avant le
/// paquet suivant.
pub(crate) const SWEEP_BATCH: usize = 64;

/// Le ménage des fichiers, à la suppression définitive d'une page (Rulings R245, R555 à R557) :
/// retire de la base, puis du disque, les contenus qu'aucune attache ne nomme plus, **et eux
/// seuls**. Un contenu que la base ne connaît pas (une base revenue en arrière, un envoi coupé
/// entre la pose et l'attache) n'est jamais regardé ; un contenu encore attaché à une page, même
/// si aucun bloc ne le nomme, reste.
///
/// Deux temps. Les candidats se lisent d'abord, sans verrou ni tour (`unattached_files`, une
/// lecture de tous les contenus de l'instance, R270). Puis, paquet par paquet, sous le verrou du
/// dépôt qui ordonne aussi « poser un contenu puis l'attacher », le dépôt mène le paquet
/// (`FileStore::forget_and_remove`, qui vérifie le garde, R576) : la base retire ceux du paquet
/// qu'aucune attache ne nomme **encore** (dans la voie du dépôt, R522), et le disque, ceux-là. Un
/// envoi qui a posé ou trouvé un contenu sous ce verrou l'a donc attaché avant
/// que le ménage le regarde, ou le pose de nouveau après qu'il est parti : son fichier est
/// toujours là (`upload_of_a_content_being_swept_keeps_its_file`).
///
/// Rien n'y échoue la suppression : une erreur est journalisée, et ce qui reste attend le ménage
/// suivant (une ligne que la base n'a pas pu retirer), ou est compté au démarrage (un contenu que
/// le disque n'a pas pu retirer, inconnu de la base, R285).
pub(crate) async fn sweep(state: &AppState) {
    let candidates = match state.store.unattached_files().await {
        Ok(candidates) => candidates,
        Err(error) => {
            tracing::warn!(
                %error,
                "ménage des fichiers impossible : la base ne se lit pas ; la prochaine \
                 suppression définitive le refera"
            );
            return;
        }
    };
    let mut removed = 0usize;
    for batch in candidates.chunks(SWEEP_BATCH) {
        let guard = state.files.lock().await;
        // Le dépôt mène le paquet, et lui seul retire des lignes `files` (Ruling R576).
        let gone = match state
            .files
            .forget_and_remove(&guard, &state.store, batch)
            .await
        {
            Ok(gone) => gone,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "ménage des fichiers interrompu : la base n'a pas retiré ses contenus sans \
                     attache ; la prochaine suppression définitive le refera"
                );
                break;
            }
        };
        drop(guard);
        for (hash, done) in gone {
            match done {
                Ok(()) => removed += 1,
                // Le chemin du serveur est dans l'erreur : elle va au journal, jamais ailleurs.
                Err(error) => tracing::error!(
                    hash = %files::hex(&hash),
                    %error,
                    "contenu qu'aucune page n'attache plus, impossible à retirer du disque : il y \
                     reste, inconnu de la base, compté au prochain démarrage"
                ),
            }
        }
    }
    if removed > 0 {
        tracing::info!(
            removed,
            "ménage des fichiers : contenus qu'aucune page n'attachait plus, retirés du disque"
        );
    }
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
    // Toutes les lignes de l'en-tête : une liste peut être coupée sur plusieurs (Ruling R508).
    if headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .any(|value| none_match(value, &etag))
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

/// Le client attend une réponse intermédiaire avant d'envoyer son corps (`Expect:
/// 100-continue`) : un refus lui arrive sans qu'on lise rien, et lire le corps ferait d'abord
/// partir cette réponse intermédiaire, qui l'inviterait à l'envoyer.
fn expects_continue(headers: &HeaderMap) -> bool {
    headers
        .get(header::EXPECT)
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"100-continue"))
}

/// Lit et jette le corps d'un envoi refusé (Ruling R505), pour que la réponse arrive : au plus
/// `max` octets, et jamais plus de `idle` sans en recevoir. Au-delà, ou sur une erreur, la
/// lecture s'arrête, et le refus part quand même (il peut alors être perdu, comme avant). Rend les
/// octets lus.
async fn discard(body: &mut Body, max: u64, idle: Duration) -> u64 {
    let mut read = 0u64;
    while let Ok(Some(Ok(frame))) = tokio::time::timeout(idle, body.frame()).await {
        if let Ok(data) = frame.into_data() {
            read += data.len() as u64;
            if read > max {
                break;
            }
        }
    }
    read
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
    let mut ranges = headers.get_all(header::RANGE).iter();
    let Some(range) = ranges.next() else {
        return Wanted::Whole;
    };
    // `Range` n'est pas une liste : sur deux lignes, c'est deux plages, et le fichier entier
    // (Ruling R508).
    if ranges.next().is_some() {
        return Wanted::Whole;
    }
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

    // -----------------------------------------------------------------------------------------
    // Le ménage et les envois, sous le verrou du dépôt (Task S4b).

    use std::time::Instant;

    use axum::Router;
    use axum::http::Request;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;

    use crate::access::tests::state;

    /// Délai maximal d'une attente de condition.
    const WAIT: Duration = Duration::from_secs(10);

    /// Un serveur sur un état neuf (celui des tests de `access.rs`), monté par le vrai routeur,
    /// et la session d'un compte qui y a son espace.
    struct Bench {
        state: AppState,
        router: Router,
        cookie: String,
        user: Uuid,
        space: String,
        dir: tempfile::TempDir,
    }

    impl Bench {
        async fn new() -> Bench {
            let dir = tempfile::tempdir().unwrap();
            let state = state(&dir).await;
            let router = crate::router(state.clone());
            let signup = Request::post("/api/v1/auth/signup")
                .header(header::HOST, "localhost")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "email": "mona@exemple.fr",
                        "name": "Mona",
                        "password": "mot-de-passe-de-test",
                    })
                    .to_string(),
                ))
                .unwrap();
            let response = router.clone().oneshot(signup).await.unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
            let cookie = response
                .headers()
                .get(header::SET_COOKIE)
                .unwrap()
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_string();
            let me: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            Bench {
                state,
                router,
                cookie,
                user: Uuid::parse_str(me["user"]["id"].as_str().unwrap()).unwrap(),
                space: me["spaces"][0]["id"].as_str().unwrap().to_string(),
                dir,
            }
        }

        /// Une requête de Mona, menée dans une tâche à elle : son statut et son corps.
        fn spawn(
            &self,
            method: &str,
            uri: String,
            body: Vec<u8>,
        ) -> tokio::task::JoinHandle<(StatusCode, Bytes)> {
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .header(header::HOST, "localhost")
                .header(header::COOKIE, &self.cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap();
            let router = self.router.clone();
            tokio::spawn(async move {
                let response = router.oneshot(request).await.unwrap();
                let status = response.status();
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                (status, body)
            })
        }

        async fn send(&self, method: &str, uri: String, body: Vec<u8>) -> (StatusCode, Bytes) {
            self.spawn(method, uri, body).await.unwrap()
        }

        /// Une page neuve, à la racine de l'espace de Mona.
        async fn page(&self, title: &str) -> Uuid {
            let body = json!({ "spaceId": self.space, "title": title }).to_string();
            let (status, body) = self.send("POST", "/api/v1/pages".into(), body.into()).await;
            assert_eq!(status, StatusCode::CREATED);
            let node: Value = serde_json::from_slice(&body).unwrap();
            Uuid::parse_str(node["id"].as_str().unwrap()).unwrap()
        }

        fn upload(
            &self,
            page: Uuid,
            name: &str,
            bytes: &[u8],
        ) -> tokio::task::JoinHandle<(StatusCode, Bytes)> {
            let uri = format!("/api/v1/pages/{page}/files?name={name}");
            self.spawn("POST", uri, bytes.to_vec())
        }

        async fn trash(&self, page: Uuid) {
            let uri = format!("/api/v1/pages/{page}/trash");
            assert_eq!(self.send("POST", uri, Vec::new()).await.0, StatusCode::OK);
        }

        fn delete(&self, page: Uuid) -> tokio::task::JoinHandle<(StatusCode, Bytes)> {
            self.spawn("DELETE", format!("/api/v1/pages/{page}"), Vec::new())
        }

        /// Le chemin du contenu `bytes` dans le dépôt.
        fn content_path(&self, bytes: &[u8]) -> std::path::PathBuf {
            let hash: [u8; 32] = Sha256::digest(bytes).into();
            let hex = files::hex(&hash);
            self.dir
                .path()
                .join(files::FILES_DIR)
                .join(&hex[..2])
                .join(&hex[2..])
        }

        /// Les attaches de la page `page`, lues par une connexion à côté du serveur.
        async fn attachments_of(&self, page: Uuid) -> i64 {
            let mut side = self.side().await;
            let count = sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE node_id = ?")
                .bind(page.to_string())
                .fetch_one(&mut side)
                .await
                .unwrap();
            sqlx::Connection::close(side).await.unwrap();
            count
        }

        /// Une connexion à la base, à côté du serveur : à fermer avant [`Bench::finish`].
        async fn side(&self) -> sqlx::SqliteConnection {
            let url = format!("sqlite:{}", self.dir.path().join("m.db").display());
            <sqlx::SqliteConnection as sqlx::Connection>::connect(&url)
                .await
                .unwrap()
        }

        /// Ferme la base, puis lâche le dossier (Ruling R344) : sous Windows, une base ouverte
        /// ne se supprime pas, et le dossier d'un banc lâché sans cela restait dans le dossier
        /// temporaire du système (constat m6 de la relecture).
        async fn finish(self) {
            self.state.store.close().await;
        }

        /// Attend que `n` tâches attendent le verrou du dépôt ([`WAIT`] au plus).
        async fn waiting_for_the_lock(&self, n: usize) {
            let deadline = Instant::now() + WAIT;
            while self.state.files.waiting() != n {
                assert!(
                    Instant::now() < deadline,
                    "{} tâches attendent le verrou du dépôt, {n} attendues",
                    self.state.files.waiting()
                );
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
    }

    /// Preuve 3 du plan, la course (Ruling R555) : la suppression de la seule page qui porte un
    /// contenu, et l'envoi du même contenu vers une autre page. Le test tient le verrou du dépôt ;
    /// il fait attendre au verrou l'un, puis l'autre (le verrou les sert dans cet ordre), puis le
    /// rend. Dans les deux ordres, l'attache neuve se lit, avec les bons octets : l'envoi d'abord,
    /// il trouve le contenu et l'attache, et le ménage, qui a lu ce contenu sans attache avant le
    /// verrou, le revoit attaché et le laisse ; le ménage d'abord, le contenu quitte le disque, et
    /// l'envoi le pose de nouveau. Témoin : sans le second envoi, le fichier quitte le disque.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn upload_of_a_content_being_swept_keeps_its_file() {
        const CONTENT: &[u8] = b"un contenu en sursis";
        for upload_first in [true, false] {
            let what = if upload_first {
                "l'envoi d'abord"
            } else {
                "le ménage d'abord"
            };
            let bench = Bench::new().await;
            let (doomed, kept) = (bench.page("Supprimée").await, bench.page("Gardée").await);
            let (status, _) = bench.upload(doomed, "x.txt", CONTENT).await.unwrap();
            assert_eq!(status, StatusCode::CREATED);
            bench.trash(doomed).await;

            let guard = bench.state.files.lock().await;
            let (upload, delete) = if upload_first {
                let upload = bench.upload(kept, "y.txt", CONTENT);
                bench.waiting_for_the_lock(1).await;
                let delete = bench.delete(doomed);
                bench.waiting_for_the_lock(2).await;
                (upload, delete)
            } else {
                let delete = bench.delete(doomed);
                bench.waiting_for_the_lock(1).await;
                let upload = bench.upload(kept, "y.txt", CONTENT);
                bench.waiting_for_the_lock(2).await;
                (upload, delete)
            };
            drop(guard);
            assert_eq!(delete.await.unwrap().0, StatusCode::NO_CONTENT, "{what}");
            let (status, body) = upload.await.unwrap();
            assert_eq!(status, StatusCode::CREATED, "{what}");
            let info: Value = serde_json::from_slice(&body).unwrap();
            let (status, served) = bench
                .send("GET", info["url"].as_str().unwrap().into(), Vec::new())
                .await;
            assert_eq!(status, StatusCode::OK, "{what} : {served:?}");
            assert_eq!(served.as_ref(), CONTENT, "{what}");
            bench.finish().await;
        }

        // Témoin : seule, la suppression attend au verrou, puis retire le contenu du disque.
        let bench = Bench::new().await;
        let doomed = bench.page("Supprimée").await;
        assert_eq!(
            bench.upload(doomed, "x.txt", CONTENT).await.unwrap().0,
            StatusCode::CREATED
        );
        bench.trash(doomed).await;
        assert!(bench.content_path(CONTENT).exists());
        let guard = bench.state.files.lock().await;
        let delete = bench.delete(doomed);
        bench.waiting_for_the_lock(1).await;
        drop(guard);
        assert_eq!(delete.await.unwrap().0, StatusCode::NO_CONTENT);
        assert!(!bench.content_path(CONTENT).exists());
        bench.finish().await;
    }

    /// Un client qui coupe ne lâche pas le ménage en route (Ruling R557 ; sonde 1 de la
    /// relecture) : la requête de suppression est abandonnée (sa tâche annulée) pendant que son
    /// ménage attend le verrou du dépôt, que le test tient ; le verrou rendu, le contenu quitte
    /// quand même le disque, parce que le ménage est mené dans la tâche de la suppression
    /// (`to_completion`), pas dans celle de la requête.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_sweep_goes_on_when_its_deletion_is_abandoned() {
        const CONTENT: &[u8] = b"un contenu dont la suppression est abandonnee";
        let bench = Bench::new().await;
        let doomed = bench.page("Supprimée").await;
        let (status, _) = bench.upload(doomed, "x.txt", CONTENT).await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        bench.trash(doomed).await;
        let guard = bench.state.files.lock().await;
        let delete = bench.delete(doomed);
        bench.waiting_for_the_lock(1).await;
        delete.abort();
        assert!(delete.await.unwrap_err().is_cancelled());
        drop(guard);
        let deadline = Instant::now() + WAIT;
        while bench.content_path(CONTENT).exists() {
            assert!(
                Instant::now() < deadline,
                "le contenu est resté sur le disque : le ménage a été lâché avec la requête"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        bench.finish().await;
    }

    /// Le ménage n'affame pas les envois (Ruling R556) : il retire au plus [`SWEEP_BATCH`]
    /// contenus sous une même prise du verrou du dépôt, et un envoi qui attend le verrou passe
    /// entre deux paquets. La page supprimée porte deux paquets et un contenu ; un envoi attend
    /// le verrou derrière le ménage : un déclencheur témoin numérote, dans l'ordre de leur
    /// validation, chaque contenu retiré de la base et l'attache de l'envoi. Exactement un paquet
    /// passe avant elle ; d'un seul tenant, le ménage les ferait tous passer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_large_sweep_lets_uploads_through_between_batches() {
        const CONTENTS: usize = 2 * SWEEP_BATCH + 1;
        let bench = Bench::new().await;
        let (doomed, other) = (bench.page("Supprimée").await, bench.page("Autre").await);
        // Des contenus attachés par le store, sans fichier sur le disque : le ménage retire de la
        // base ce qu'il trouve, et un contenu absent du disque se retire sans erreur (R275).
        for i in 0..CONTENTS {
            let mut hash = [0u8; 32];
            hash[..8].copy_from_slice(&(i as u64).to_le_bytes());
            let new = NewAttachment {
                id: Uuid::now_v7(),
                node_id: doomed,
                hash,
                size: 1,
                mime: "text/plain",
                name: "petit.txt",
                created_by: bench.user,
            };
            bench.state.store.attach_file(new, now_ms()).await.unwrap();
        }
        let mut side = bench.side().await;
        sqlx::raw_sql(
            "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
             CREATE TRIGGER witness_sweep AFTER DELETE ON files \
             BEGIN INSERT INTO witness (what) VALUES ('ménage'); END; \
             CREATE TRIGGER witness_upload AFTER INSERT ON attachments \
             WHEN NEW.name = 'envoi.txt' \
             BEGIN INSERT INTO witness (what) VALUES ('envoi'); END;",
        )
        .execute(&mut side)
        .await
        .unwrap();
        bench.trash(doomed).await;

        let guard = bench.state.files.lock().await;
        let delete = bench.delete(doomed);
        bench.waiting_for_the_lock(1).await;
        let upload = bench.upload(other, "envoi.txt", b"un envoi qui attend");
        bench.waiting_for_the_lock(2).await;
        drop(guard);
        assert_eq!(delete.await.unwrap().0, StatusCode::NO_CONTENT);
        assert_eq!(upload.await.unwrap().0, StatusCode::CREATED);

        let order: Vec<String> = sqlx::query_scalar("SELECT what FROM witness ORDER BY n")
            .fetch_all(&mut side)
            .await
            .unwrap();
        assert_eq!(order.len(), CONTENTS + 1, "{order:?}");
        let before = order.iter().position(|what| what == "envoi").unwrap();
        assert_eq!(
            before, SWEEP_BATCH,
            "contenus retirés avant l'envoi : {before}"
        );
        sqlx::Connection::close(side).await.unwrap();
        bench.finish().await;
    }
    /// Le ménage d'une suppression n'attend pas la rafale de qui supprime (Ruling R522 ; constat
    /// C de la re-relecture 1 de la robustesse), sur le modèle de
    /// `the_file_store_lock_never_waits_for_an_account_burst` : il tient le verrou du dépôt, que
    /// tous les envois de l'instance attendent, pendant qu'il attend son tour. Mona a vingt
    /// écritures en file quand le ménage de sa suppression, verrou pris, demande le sien : il passe
    /// le premier. Dans la voie de Mona, que la requête nomme (`to_completion` garde la portée),
    /// il passerait après les vingt, verrou tenu.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_sweep_of_a_deletion_never_waits_for_the_burst_of_who_deletes() {
        const BURST: usize = 20;
        let bench = Bench::new().await;
        let doomed = bench.page("Supprimée").await;
        let (status, _) = bench.upload(doomed, "x.txt", b"orphelin").await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        bench.trash(doomed).await;
        let mut side = bench.side().await;
        sqlx::raw_sql(
            "CREATE TABLE witness (n INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT NOT NULL); \
             CREATE TRIGGER witness_burst AFTER INSERT ON spaces \
             BEGIN INSERT INTO witness (what) VALUES ('rafale'); END; \
             CREATE TRIGGER witness_sweep AFTER DELETE ON files \
             BEGIN INSERT INTO witness (what) VALUES ('ménage'); END;",
        )
        .execute(&mut side)
        .await
        .unwrap();
        let store = bench.state.store.clone();
        let queued = async |n: usize| {
            let deadline = Instant::now() + WAIT;
            while store.queued_writes() != n {
                assert!(
                    Instant::now() < deadline,
                    "{} écritures en file, {n} attendues",
                    store.queued_writes()
                );
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        };

        // La suppression passe, et son ménage attend le verrou du dépôt, que le test tient.
        let guard = bench.state.files.lock().await;
        let delete = bench.delete(doomed);
        bench.waiting_for_the_lock(1).await;
        // La rafale de Mona, en file.
        let held = store.hold_writes().await;
        let mut burst = Vec::new();
        for i in 0..BURST {
            let store = store.clone();
            let name = format!("Rafale {i:02}");
            burst.push(tokio::spawn(mementum_store::writes_by(
                Some(bench.user),
                async move { store.create_space(&name, None).await },
            )));
            queued(i + 1).await;
        }
        // Le ménage prend le verrou du dépôt, et demande son tour.
        drop(guard);
        queued(BURST + 1).await;
        drop(held);
        assert_eq!(delete.await.unwrap().0, StatusCode::NO_CONTENT);
        for write in burst {
            write.await.unwrap().unwrap();
        }
        let order: Vec<String> = sqlx::query_scalar("SELECT what FROM witness ORDER BY n")
            .fetch_all(&mut side)
            .await
            .unwrap();
        assert_eq!(order.len(), BURST + 1, "{order:?}");
        assert_eq!(order[0], "ménage", "{order:?}");
        sqlx::Connection::close(side).await.unwrap();
        bench.finish().await;
    }

    /// La fenêtre d'une recopie est la file d'écriture de son compte (Ruling R577 ; sonde 6 de la
    /// relecture) : Mona a trois écritures en file quand sa recopie, ses contrôles passés,
    /// demande son tour ; pendant qu'elle attend, la page cible est mise à la corbeille par une
    /// autre voie. La recopie relit la corbeille dans sa transaction : `409 trashed`, et la page à
    /// la corbeille ne prend aucune attache. Témoin : la page restaurée, la même recopie passe.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_page_trashed_while_its_copy_waits_its_turn_takes_nothing() {
        const CONTENT: &[u8] = b"un fichier a recopier";
        let bench = Bench::new().await;
        let (source, target) = (bench.page("Source").await, bench.page("Cible").await);
        let (status, body) = bench.upload(source, "f.txt", CONTENT).await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        let info: Value = serde_json::from_slice(&body).unwrap();
        let file = info["id"].as_str().unwrap().to_string();
        let copy_body = json!({ "pageId": source.to_string(), "fileId": file })
            .to_string()
            .into_bytes();
        let copies = format!("/api/v1/pages/{target}/files/copies");
        let store = bench.state.store.clone();
        let queued = async |n: usize| {
            let deadline = Instant::now() + WAIT;
            while store.queued_writes() != n {
                assert!(
                    Instant::now() < deadline,
                    "{} écritures en file, {n} attendues",
                    store.queued_writes()
                );
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        };
        let held = store.hold_writes().await;
        let mut burst = Vec::new();
        for i in 0..3 {
            let store = store.clone();
            let name = format!("Rafale {i}");
            burst.push(tokio::spawn(mementum_store::writes_by(
                Some(bench.user),
                async move { store.create_space(&name, None).await },
            )));
            queued(i + 1).await;
        }
        let copy = bench.spawn("POST", copies.clone(), copy_body.clone());
        queued(4).await;
        let trash = tokio::spawn(mementum_store::writes_by(None, {
            let store = store.clone();
            let by = bench.user;
            async move { store.trash_node(target, by).await }
        }));
        queued(5).await;
        drop(held);
        trash.await.unwrap().unwrap();
        for write in burst {
            write.await.unwrap().unwrap();
        }
        let (status, body) = copy.await.unwrap();
        assert_eq!(status, StatusCode::CONFLICT, "{body:?}");
        let refusal: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(refusal["error"], "trashed");
        assert_eq!(bench.attachments_of(target).await, 0, "rien d'attaché");

        // Témoin.
        let restore = format!("/api/v1/pages/{target}/restore");
        assert_eq!(
            bench.send("POST", restore, Vec::new()).await.0,
            StatusCode::OK
        );
        assert_eq!(
            bench.send("POST", copies, copy_body).await.0,
            StatusCode::CREATED
        );
        assert_eq!(bench.attachments_of(target).await, 1);
        bench.finish().await;
    }
}
