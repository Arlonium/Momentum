//! Garde-fous réseau (Rulings R31, R51 et R78). Ils complètent les comptes,
//! ils ne les remplacent pas.
//!
//! - **DNS rebinding** (boucle locale seulement). Écouter sur la boucle locale
//!   ne suffit pas : un site ouvert dans le navigateur peut faire pointer son
//!   propre nom vers `127.0.0.1`, puis parler au serveur comme s'il était de
//!   la même origine. Quand le serveur écoute sur la boucle locale, toute
//!   requête dont l'en-tête `Host` ne nomme pas `localhost`, `127.0.0.1` ou
//!   `[::1]` (port quelconque) reçoit `421 bad_host`. Le mandataire de Vite
//!   garde `Host: localhost:<port>`.
//!
//!   Un seul autre hôte est admis, quand `MEMENTUM_PUBLIC_URL` est réglé :
//!   celui de l'adresse publique (Ruling R78). Derrière un tunnel ou un
//!   mandataire qui garde l'en-tête `Host` publique, le serveur n'écoute que
//!   sur la boucle locale, et sans cela tout répondrait `421`. Cela n'ouvre
//!   rien au DNS rebinding : la page d'un attaquant envoie le `Host` de son
//!   propre nom, pas celui de l'adresse publique.
//! - **Écritures et WebSocket venues d'un autre site.** Toute requête autre
//!   que `GET`, `HEAD` ou `OPTIONS`, et toute connexion à `/sync/{id}` ou à
//!   `/events/{spaceId}` (une WebSocket échappe à CORS ; c'est la route qui
//!   le contrôle, `routes/sync.rs`), est refusée par `403 bad_origin` si elle
//!   porte une en-tête `Origin` qui n'est ni celle d'une page de la boucle
//!   locale, ni celle du serveur lui-même (même hôte et même port que
//!   `Host`), ni celle de l'adresse publique (`MEMENTUM_PUBLIC_URL`). Sans
//!   `Origin` (client hors navigateur), la requête passe : il n'y a pas de
//!   navigateur dont on abuserait.
//!
//! Ce que cela protège, et ce que cela ne protège pas :
//!
//! - le contrôle de l'`Origin` arrête le site tiers ordinaire, qui ferait
//!   écrire le navigateur d'une personne connectée à sa place. Le cookie de
//!   session en `SameSite=Lax` l'arrête déjà dans les navigateurs récents :
//!   c'est une seconde barrière ;
//! - il n'arrête pas le DNS rebinding : il compare l'`Origin` à une en-tête
//!   `Host` que personne n'a vérifiée hors de la boucle locale, et une page
//!   servie sous un nom qui pointe vers le serveur a une `Origin` égale à son
//!   `Host` ;
//! - hors de la boucle locale, le contrôle de `Host` est désactivé (le
//!   démarrage le journalise) : là, **ce sont les comptes qui protègent, pas
//!   ces contrôles**. Un DNS rebinding n'emporte pas le cookie de session,
//!   lié au vrai nom d'hôte : il ne fait donc rien de plus qu'un visiteur
//!   sans session, à qui ni les routes de pages ni `/sync` ne répondent ;
//! - aucun de ces contrôles n'arrête un client hors navigateur, qui écrit les
//!   en-têtes qu'il veut : lui n'a que ce que sa session lui donne.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::error::ApiError;
use crate::types::ErrorCode;

/// `host[:port]` nomme-t-il la boucle locale (`localhost`, `127.0.0.1` ou
/// `[::1]`) ? Le port, s'il est là, n'est fait que de chiffres.
pub(crate) fn is_loopback_authority(authority: &str) -> bool {
    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((host, after)) => match after.strip_prefix(':') {
                Some(port) => (host, Some(port)),
                None if after.is_empty() => (host, None),
                None => return false,
            },
            None => return false,
        },
        None => match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    let port_ok = port.is_none_or(|p| p.bytes().all(|b| b.is_ascii_digit()));
    port_ok && (host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1")
}

/// L'en-tête `Host` (ou, à défaut, l'autorité de l'URI), si elle est lisible.
pub(crate) fn request_host<'a>(
    headers: &'a HeaderMap,
    uri: &'a axum::http::Uri,
) -> Option<&'a str> {
    match headers.get(header::HOST) {
        Some(value) => value.to_str().ok(),
        None => uri.authority().map(|a| a.as_str()),
    }
}

/// L'en-tête `Origin`, si la requête en porte une. Illisible, elle vaut une
/// origine vide, donc refusée.
pub(crate) fn request_origin(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::ORIGIN)
        .map(|value| value.to_str().unwrap_or(""))
}

/// `host`, l'en-tête `Host` d'une requête, est-il celui de l'adresse publique
/// (`public`, son origine : `schéma://hôte[:port]`, sans le port par défaut
/// du schéma) ? L'hôte et le port doivent être les siens ; le port par défaut
/// du schéma peut être écrit en toutes lettres (`notes.exemple.fr:443`).
pub(crate) fn is_public_host(host: &str, public: Option<&str>) -> bool {
    let Some((scheme, authority)) = public.and_then(|origin| origin.split_once("://")) else {
        return false;
    };
    if host.eq_ignore_ascii_case(authority) {
        return true;
    }
    // L'adresse publique porte-t-elle un port (elle ne garde que celui qui
    // n'est pas le port par défaut) ? Alors c'est ce port-là, rien d'autre.
    let has_port = authority
        .rsplit_once(':')
        .is_some_and(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()));
    let default_port = if scheme == "https" { ":443" } else { ":80" };
    !has_port
        && host
            .strip_suffix(default_port)
            .is_some_and(|name| name.eq_ignore_ascii_case(authority))
}

/// Couche de toutes les routes, fallbacks compris :
/// - `421 bad_host` si le serveur écoute sur la boucle locale et que `Host`
///   ne nomme ni elle, ni l'hôte de l'adresse publique ([`is_public_host`]) ;
/// - `403 bad_origin` si la requête écrit (tout sauf `GET`, `HEAD` et
///   `OPTIONS`) et porte une `Origin` que [`origin_allowed`] refuse.
pub(crate) async fn check(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let host = request_host(request.headers(), request.uri());
    let public = state.public_origin.as_deref();
    if state.check_host
        && !host.is_some_and(|host| is_loopback_authority(host) || is_public_host(host, public))
    {
        return ApiError::new(
            StatusCode::MISDIRECTED_REQUEST,
            ErrorCode::BadHost,
            "en-tête Host refusée : ce serveur ne répond qu'à localhost, 127.0.0.1 ou [::1]. \
             Derrière un mandataire ou un tunnel, régler MEMENTUM_PUBLIC_URL sur l'adresse \
             publique : son hôte est alors admis",
        )
        .into_response();
    }
    let writes = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if writes && !origin_allowed(request_origin(request.headers()), host, public) {
        return bad_origin().into_response();
    }
    next.run(request).await
}

/// Couche la plus extérieure : toute réponse de l'API (`/api/…`), réussite ou refus, ceux de
/// [`check`] compris, porte `Cache-Control: no-store` et `X-Content-Type-Options: nosniff`.
/// Défense en profondeur : une réponse peut porter un jeton d'invitation, des adresses ou le
/// contenu d'une page, qu'aucun cache ne doit garder, et dont aucun navigateur ne doit deviner
/// un autre type que celui annoncé.
pub(crate) async fn api_headers(request: Request, next: Next) -> Response {
    let api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    if api {
        let headers = response.headers_mut();
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
    }
    response
}

/// L'`Origin` d'une écriture ou d'une connexion `/sync` ou `/events` est-elle acceptée ?
/// Absente : oui. Présente : seulement `http(s)://` vers la boucle locale ou
/// vers l'hôte même du serveur (`host`, l'en-tête `Host` de la requête), ou
/// exactement `public`, l'origine de l'adresse publique, schéma compris.
pub(crate) fn origin_allowed(
    origin: Option<&str>,
    host: Option<&str>,
    public: Option<&str>,
) -> bool {
    let Some(origin) = origin else { return true };
    if public.is_some_and(|public| public.eq_ignore_ascii_case(origin)) {
        return true;
    }
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    if authority.is_empty() || authority.contains('/') {
        return false;
    }
    is_loopback_authority(authority) || host.is_some_and(|h| h.eq_ignore_ascii_case(authority))
}

/// `403 bad_origin`.
pub(crate) fn bad_origin() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        ErrorCode::BadOrigin,
        "en-tête Origin refusée : seules les pages servies par localhost, 127.0.0.1, [::1], par \
         ce serveur ou par son adresse publique peuvent écrire et se synchroniser. Derrière un \
         mandataire ou un tunnel, régler MEMENTUM_PUBLIC_URL sur l'adresse publique",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_authorities() {
        for ok in [
            "localhost",
            "LOCALHOST:80",
            "127.0.0.1",
            "127.0.0.1:8080",
            "[::1]",
            "[::1]:5173",
            "localhost:",
        ] {
            assert!(is_loopback_authority(ok), "{ok}");
        }
        for refused in [
            "",
            "evil.example",
            "localhost.evil.example",
            "127.0.0.1.nip.io",
            "127.0.0.2",
            "::1",
            "[::1",
            "[::1]x",
            "[::2]",
            "localhost:80:80",
            "localhost:http",
            "localhost/",
        ] {
            assert!(!is_loopback_authority(refused), "{refused}");
        }
    }

    #[test]
    fn origins() {
        let host = Some("192.168.1.10:8080");
        let allowed = |origin, host| origin_allowed(origin, host, None);
        assert!(allowed(None, host));
        assert!(allowed(Some("http://localhost:5173"), host));
        assert!(allowed(Some("https://[::1]"), host));
        assert!(allowed(Some("http://192.168.1.10:8080"), host));
        assert!(!allowed(Some("http://192.168.1.10:9090"), host));
        assert!(!allowed(Some("http://192.168.1.10:8080"), None));
        for refused in [
            "",
            "http://evil.example",
            "null",
            "file://",
            "ws://localhost",
            "http://",
            "http://localhost/chemin",
        ] {
            assert!(!allowed(Some(refused), host), "{refused}");
        }
    }

    /// L'origine de l'adresse publique est admise telle quelle (même schéma,
    /// même hôte, même port), quel que soit l'en-tête `Host`.
    #[test]
    fn public_origin() {
        let public = Some("https://notes.exemple.fr");
        for host in [None, Some("127.0.0.1:8080"), Some("notes.exemple.fr")] {
            assert!(origin_allowed(
                Some("https://notes.exemple.fr"),
                host,
                public
            ));
            assert!(origin_allowed(
                Some("HTTPS://Notes.Exemple.FR"),
                host,
                public
            ));
            assert!(origin_allowed(None, host, public));
        }
        let host = Some("10.0.0.5:8080");
        for refused in [
            "http://notes.exemple.fr",
            "https://notes.exemple.fr:8443",
            "https://notes.exemple.fr/",
            "https://evil.notes.exemple.fr",
            "https://notes.exemple.fr.evil.example",
            "https://evil.example",
            "null",
            "",
        ] {
            assert!(!origin_allowed(Some(refused), host, public), "{refused}");
        }
        // L'hôte même du serveur et la boucle locale restent admis.
        assert!(origin_allowed(Some("http://10.0.0.5:8080"), host, public));
        assert!(origin_allowed(Some("http://localhost:5173"), host, public));
    }

    /// Ruling R78 : l'hôte de l'adresse publique, et lui seul, avec son port.
    #[test]
    fn public_hosts() {
        let public = Some("https://notes.exemple.fr");
        for ok in [
            "notes.exemple.fr",
            "NOTES.exemple.FR",
            "notes.exemple.fr:443",
        ] {
            assert!(is_public_host(ok, public), "{ok}");
        }
        for refused in [
            "",
            "evil.example",
            "exemple.fr",
            "sub.notes.exemple.fr",
            "notes.exemple.fr.evil.example",
            "notes.exemple.fr:80",
            "notes.exemple.fr:8443",
            "notes.exemple.fr:",
            "notes.exemple.fr:443:443",
            "alex@notes.exemple.fr",
            "https://notes.exemple.fr",
            "localhost",
        ] {
            assert!(!is_public_host(refused, public), "{refused}");
        }
        // Sans adresse publique, personne.
        assert!(!is_public_host("notes.exemple.fr", None));

        // En `http`, le port par défaut est 80.
        let public = Some("http://notes.exemple.fr");
        assert!(is_public_host("notes.exemple.fr:80", public));
        assert!(!is_public_host("notes.exemple.fr:443", public));

        // Une adresse publique qui porte un port : ce port-là, et rien d'autre.
        let public = Some("https://notes.exemple.fr:8443");
        assert!(is_public_host("notes.exemple.fr:8443", public));
        for refused in [
            "notes.exemple.fr",
            "notes.exemple.fr:443",
            "notes.exemple.fr:8443:443",
        ] {
            assert!(!is_public_host(refused, public), "{refused}");
        }
        // Une adresse IPv6, sans port puis avec.
        let public = Some("http://[2001:db8::1]");
        assert!(is_public_host("[2001:db8::1]", public));
        assert!(is_public_host("[2001:db8::1]:80", public));
        assert!(!is_public_host("[2001:db8::2]", public));
        let public = Some("http://[2001:db8::1]:8080");
        assert!(is_public_host("[2001:db8::1]:8080", public));
        assert!(!is_public_host("[2001:db8::1]", public));
        assert!(!is_public_host("[2001:db8::1]:8080:80", public));
    }
}
