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

pub use protocol::{
    Answer, CallOutcome, Endpoint, Era, Headers, Id, Incoming, Refused, Reply, Request,
};
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

/// Ce que [`serve`] rend : la réponse, et l'issue d'un `tools/call` (aucune pour une autre
/// méthode), que la route écrit au journal d'activité (Rulings R1244, R1560).
#[derive(Clone, Debug, PartialEq)]
pub struct Served {
    pub reply: Reply,
    pub call: Option<CallOutcome>,
    /// La méthode d'un message lu qui n'est pas un `tools/call` (`server/discover`, `tools/list`,
    /// `resources/read`…) : un nom de [`method_name`], jamais un texte du client. `None` pour un
    /// appel d'outil, une notification inconnue et un message refusé avant d'être lu. L'écran
    /// « Activité » la dit en mots (recette du lot 5, E2, Ruling R1551).
    pub method: Option<&'static str>,
}

/// La méthode d'une requête lue qui n'est pas un appel d'outil, telle que le protocole l'écrit.
pub fn method_name(request: &Request) -> Option<&'static str> {
    Some(match request {
        Request::Discover => "server/discover",
        Request::Initialize => "initialize",
        Request::Initialized => "notifications/initialized",
        Request::Ping => "ping",
        Request::ToolsList => "tools/list",
        Request::ResourcesList => "resources/list",
        Request::ResourceTemplatesList => "resources/templates/list",
        Request::ResourcesRead { .. } => "resources/read",
        Request::Notification | Request::ToolsCall { .. } => return None,
    })
}

/// Répond à une requête de bout en bout : la lit ([`protocol::read`]), fait l'appel d'outil par
/// `host`, et met la réponse en forme pour l'ère de la requête ([`protocol::write`]). Rend aussi
/// l'issue de chaque `tools/call` : réussi, refusé par l'outil, refusé par [`tools::parse`], ou
/// refusé par le protocole avant (outil inconnu ou d'écriture sur `/mcp/readonly`, en-tête qui ne
/// dit pas le corps, requête sans `id`…).
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
) -> Served {
    let incoming = match protocol::read(headers, body, endpoint) {
        Ok(incoming) => incoming,
        Err(refused) => {
            let Refused { reply, call } = *refused;
            return Served {
                reply,
                call,
                method: None,
            };
        }
    };
    let method = method_name(&incoming.request);
    let Some(id) = incoming.id else {
        return Served {
            reply: Reply::accepted(),
            call: None,
            method,
        };
    };
    let mut call = None;
    let answer = match incoming.request {
        Request::Discover => Answer::Discover(endpoint),
        Request::Initialize => Answer::Initialize(endpoint),
        Request::Ping => Answer::Ping,
        Request::ToolsList => Answer::ToolsList(endpoint),
        Request::ToolsCall { name, arguments } => {
            let output = match tools::parse(&name, arguments) {
                Ok(tool_call) => host.call(tool_call).await,
                Err(refused) => refused,
            };
            call = Some(CallOutcome {
                tool: name,
                code: output.code().map(str::to_string),
            });
            Answer::ToolResult(output)
        }
        Request::ResourcesList => Answer::ResourcesList,
        Request::ResourceTemplatesList => Answer::ResourceTemplatesList,
        Request::ResourcesRead { uri } => Answer::ResourceRead(uri),
        // Une notification n'a pas d'`id` : traitée plus haut.
        Request::Initialized | Request::Notification => {
            return Served {
                reply: Reply::accepted(),
                call: None,
                method,
            };
        }
    };
    Served {
        reply: protocol::write(incoming.era, &id, answer),
        call,
        method,
    }
}
