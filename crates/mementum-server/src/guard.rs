//! Garde-fous réseau (Ruling R31) : le lot 1 n'a pas d'authentification.
//!
//! - **DNS rebinding.** Écouter sur la boucle locale ne suffit pas : un site
//!   ouvert dans le navigateur peut faire pointer son propre nom vers
//!   `127.0.0.1`, puis lire l'arbre et écrire par `PUT mmd`. Quand le serveur
//!   écoute sur la boucle locale, toute requête dont l'en-tête `Host` ne nomme
//!   pas `localhost`, `127.0.0.1` ou `[::1]` (port quelconque) reçoit
//!   `421 bad_host`. Le mandataire de Vite garde `Host: localhost:<port>`.
//! - **Détournement de WebSocket.** Une WebSocket échappe à CORS : à
//!   `/sync/{id}`, une en-tête `Origin` présente doit nommer l'un de ces trois
//!   hôtes, ou être l'origine du serveur lui-même (même hôte et même port que
//!   `Host`) ; sinon `403 bad_origin`. Sans `Origin` (client hors navigateur),
//!   la connexion passe.
//!
//! Ces deux protections ne valent que sur la boucle locale. Hors d'elle, le
//! contrôle de `Host` est désactivé (le démarrage le journalise) : le DNS
//! rebinding n'est plus refusé. Celui de l'`Origin` reste, mais il ne protège
//! plus du DNS rebinding : il compare l'`Origin` à une en-tête `Host` que
//! personne n'a vérifiée, et une page servie sous un nom qui pointe vers le
//! serveur a une `Origin` égale à son `Host`. Il n'arrête alors que le site
//! tiers ordinaire, dont l'`Origin` diffère du `Host`. Sans authentification
//! au lot 1, n'écouter que sur la boucle locale.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
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

/// Couche de toutes les routes : `421 bad_host` si le serveur écoute sur la
/// boucle locale et que `Host` ne la nomme pas.
pub(crate) async fn check_host(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if state.check_host
        && !request_host(request.headers(), request.uri()).is_some_and(is_loopback_authority)
    {
        return ApiError::new(
            StatusCode::MISDIRECTED_REQUEST,
            ErrorCode::BadHost,
            "en-tête Host refusée : ce serveur ne répond qu'à localhost, 127.0.0.1 ou [::1]",
        )
        .into_response();
    }
    next.run(request).await
}

/// L'`Origin` d'une connexion `/sync` est-elle acceptée ? Absente : oui.
/// Présente : seulement `http(s)://` vers la boucle locale, ou vers l'hôte
/// même du serveur (`host`, l'en-tête `Host` de la requête).
pub(crate) fn origin_allowed(origin: Option<&str>, host: Option<&str>) -> bool {
    let Some(origin) = origin else { return true };
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
        "en-tête Origin refusée : la synchronisation n'accepte que les pages servies par \
         localhost, 127.0.0.1, [::1] ou par ce serveur",
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
        assert!(origin_allowed(None, host));
        assert!(origin_allowed(Some("http://localhost:5173"), host));
        assert!(origin_allowed(Some("https://[::1]"), host));
        assert!(origin_allowed(Some("http://192.168.1.10:8080"), host));
        assert!(!origin_allowed(Some("http://192.168.1.10:9090"), host));
        assert!(!origin_allowed(Some("http://192.168.1.10:8080"), None));
        for refused in [
            "http://evil.example",
            "null",
            "file://",
            "ws://localhost",
            "http://",
            "http://localhost/chemin",
        ] {
            assert!(!origin_allowed(Some(refused), host), "{refused}");
        }
    }
}
