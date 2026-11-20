//! Serveur MCP de Mementum, sans réseau (conception §6.5 ; plan du lot 5, Task M1).
//!
//! Le protocole est écrit à la main (Ruling R1236) : JSON-RPC 2.0, un message par `POST`, une
//! réponse JSON, sans état, sans flux, sans session. Deux révisions sont servies sur la même
//! adresse, l'ère d'une requête se lisant à son corps : la 2026-07-28 d'abord (la version et les
//! capacités du client dans le `_meta` de chaque requête, redites par les en-têtes
//! `MCP-Protocol-Version`, `Mcp-Method` et `Mcp-Name`), la 2025-11-25 pour un client plus ancien
//! (`initialize`, puis l'en-tête `MCP-Protocol-Version: 2025-11-25`).
//!
//! - [`protocol`] lit une requête ([`protocol::read`]) et met en forme une réponse
//!   ([`protocol::write`]), selon l'ère ;
//! - [`tools`] décrit les six outils (schémas d'entrée dérivés des types, plafonds, ordre fixe) et
//!   lit les arguments d'un appel ([`tools::parse`]) ;
//! - [`text`] met en texte ce qu'un outil rend : borné, et le contenu des pages entre deux lignes
//!   de borne à nombre tiré au hasard (Ruling R1239) ;
//! - [`spec`] porte la ressource `mementum://spec/mmd`.
//!
//! La crate ne fait ni réseau ni opération : le serveur (`mementum-server`, route `/mcp`) lui
//! passe les en-têtes et le corps de la requête ([`serve`]), et fait chaque outil par la fonction
//! d'`agent/` que sa route REST appelle aussi, derrière le trait [`Host`].

use std::future::Future;

pub mod protocol;
pub mod spec;
pub mod text;
pub mod tools;

pub use protocol::{Answer, Endpoint, Era, Headers, Id, Incoming, Reply, Request};
pub use text::ToolOutput;
pub use tools::ToolCall;

/// Ce que le serveur fait pour chaque outil. Il reçoit un appel dont les arguments ont été lus
/// et bornés ([`tools::parse`]), et rend sa sortie mise en texte par une fonction de [`text`]
/// (une sortie ne se bâtit pas autrement : toutes sont bornées).
///
/// Implémenté par la route `/mcp` (Task M2), pour l'appelant de la requête : un outil n'a aucun
/// droit à lui, il appelle la même fonction que la route REST de la même opération.
pub trait Host {
    fn call(&self, call: ToolCall) -> impl Future<Output = ToolOutput> + Send;
}

/// Répond à une requête de bout en bout : la lit ([`protocol::read`]), fait l'appel d'outil par
/// `host`, et met la réponse en forme pour l'ère de la requête ([`protocol::write`]).
///
/// `headers` porte les valeurs reçues de `MCP-Protocol-Version`, `Mcp-Method` et `Mcp-Name` ;
/// `body`, le corps du `POST` ; `endpoint`, l'adresse appelée (`/mcp` ou `/mcp/readonly`). La
/// méthode HTTP (`405` pour `GET` et `DELETE`), le jeton, l'`Origin` et les en-têtes de la
/// réponse sont l'affaire de la route.
pub async fn serve<H: Host + Sync>(
    headers: &Headers<'_>,
    body: &[u8],
    endpoint: Endpoint,
    host: &H,
) -> Reply {
    let incoming = match protocol::read(headers, body, endpoint) {
        Ok(incoming) => incoming,
        Err(reply) => return reply,
    };
    let Some(id) = incoming.id else {
        return Reply::accepted();
    };
    let answer = match incoming.request {
        Request::Discover => Answer::Discover,
        Request::Initialize => Answer::Initialize,
        Request::Ping => Answer::Ping,
        Request::ToolsList => Answer::ToolsList(endpoint),
        Request::ToolsCall { name, arguments } => match tools::parse(&name, arguments) {
            Ok(call) => Answer::ToolResult(host.call(call).await),
            Err(refused) => Answer::ToolResult(refused),
        },
        Request::ResourcesList => Answer::ResourcesList,
        Request::ResourcesRead { uri } => Answer::ResourceRead(uri),
        // Une notification n'a pas d'`id` : traitée plus haut.
        Request::Initialized | Request::Notification => return Reply::accepted(),
    };
    protocol::write(incoming.era, &id, answer)
}
