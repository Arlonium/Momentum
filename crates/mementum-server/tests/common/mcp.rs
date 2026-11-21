//! Un client MCP de test (lot 5, Task M2) : un message JSON-RPC, une réponse, dans l'une ou l'autre
//! révision que sert `/mcp` (Ruling R1236).
//!
//! - **2026-07-28** : chaque requête porte la version et les capacités du client dans son `_meta`,
//!   et les en-têtes `MCP-Protocol-Version`, `Mcp-Method` et `Mcp-Name` qui les redisent, comme
//!   Claude Code 2.1.329 les écrit (relevé par la Task S0) ;
//! - **2025-11-25** : `initialize`, puis l'en-tête `MCP-Protocol-Version: 2025-11-25` sur chaque
//!   requête, sans `_meta` de version.
//!
//! Le jeton va dans `Authorization: Bearer`, comme un client le pose (jamais un vrai jeton : il est
//! tiré par le test, [`super::api_token`]).

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use http_body::Frame;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Reply, TestApp, with_bearer};

/// La révision courante de MCP.
pub const V2026: &str = "2026-07-28";
/// La révision d'avant, servie à un client plus ancien.
pub const V2025: &str = "2025-11-25";

/// La révision que parle un client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Era {
    V2026,
    V2025,
}

/// Les deux révisions, la courante d'abord.
pub const ERAS: [Era; 2] = [Era::V2026, Era::V2025];

/// Un client MCP de test, sur `/mcp` ou `/mcp/readonly`.
pub struct Mcp<'a> {
    t: &'a TestApp,
    token: Option<String>,
    era: Era,
    path: &'static str,
    next_id: AtomicU64,
}

/// Ce qu'un outil a rendu : son texte, et s'il dit un échec (`isError`).
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

impl<'a> Mcp<'a> {
    /// Un client de cette révision, sur `/mcp`, avec ce jeton.
    pub fn new(t: &'a TestApp, token: &str, era: Era) -> Mcp<'a> {
        Mcp {
            t,
            token: Some(token.to_string()),
            era,
            path: "/mcp",
            next_id: AtomicU64::new(1),
        }
    }

    /// Le même client, sans jeton.
    pub fn without_token(t: &'a TestApp, era: Era) -> Mcp<'a> {
        Mcp {
            token: None,
            ..Mcp::new(t, "", era)
        }
    }

    /// Le même client, sur `/mcp/readonly`.
    pub fn readonly(mut self) -> Mcp<'a> {
        self.path = "/mcp/readonly";
        self
    }

    pub fn era(&self) -> Era {
        self.era
    }

    /// La requête HTTP d'un message JSON-RPC de cette révision : `method`, ses `params` (un objet),
    /// un `id` neuf.
    pub fn request(&self, method: &str, params: Value) -> Request<Body> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut params = match params {
            Value::Object(map) => map,
            other => panic!("des params en objet : {other}"),
        };
        let mut builder = Request::post(self.path)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream");
        match self.era {
            Era::V2026 => {
                let name = match method {
                    "tools/call" => params.get("name").and_then(Value::as_str),
                    "resources/read" => params.get("uri").and_then(Value::as_str),
                    _ => None,
                }
                .map(str::to_string);
                params.insert(
                    "_meta".into(),
                    json!({
                        "io.modelcontextprotocol/protocolVersion": V2026,
                        "io.modelcontextprotocol/clientCapabilities": {},
                    }),
                );
                builder = builder
                    .header("MCP-Protocol-Version", V2026)
                    .header("Mcp-Method", method);
                if let Some(name) = name {
                    builder = builder.header("Mcp-Name", name);
                }
            }
            Era::V2025 => {
                if method != "initialize" {
                    builder = builder.header("MCP-Protocol-Version", V2025);
                }
            }
        }
        let body = serde_json::to_vec(
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )
        .unwrap();
        // Comme un client sur le réseau : la longueur annoncée (la porte en juge le débit).
        let request = builder
            .header(header::CONTENT_LENGTH, body.len())
            .body(Body::from(body))
            .unwrap();
        match &self.token {
            Some(token) => with_bearer(request, token),
            None => request,
        }
    }

    /// Envoie un message et rend la réponse HTTP telle quelle.
    pub async fn send(&self, method: &str, params: Value) -> Reply {
        self.t.send(self.request(method, params)).await
    }

    /// Envoie un message et rend son `result` : la réponse doit être un `200` sans erreur JSON-RPC.
    pub async fn rpc(&self, method: &str, params: Value) -> Value {
        let reply = self.send(method, params).await;
        reply.assert_status(StatusCode::OK);
        let body = reply.json();
        assert!(body.get("error").is_none(), "erreur JSON-RPC : {body}");
        body["result"].clone()
    }

    /// `initialize` puis `notifications/initialized` pour un client de la 2025-11-25 ; rien pour
    /// un client de la 2026-07-28, sans session (il sonde par `server/discover`).
    pub async fn open(&self) {
        match self.era {
            Era::V2026 => {
                let found = self.rpc("server/discover", json!({})).await;
                assert_eq!(found["supportedVersions"][0], V2026, "{found}");
            }
            Era::V2025 => {
                let opened = self
                    .rpc(
                        "initialize",
                        json!({
                            "protocolVersion": V2025,
                            "capabilities": {},
                            "clientInfo": { "name": "test", "version": "1" },
                        }),
                    )
                    .await;
                assert_eq!(opened["protocolVersion"], V2025, "{opened}");
                let reply = self
                    .t
                    .send({
                        let mut request = Request::post(self.path)
                            .header(header::CONTENT_TYPE, "application/json")
                            .header("MCP-Protocol-Version", V2025)
                            .body(Body::from(
                                serde_json::to_vec(&json!({
                                    "jsonrpc": "2.0",
                                    "method": "notifications/initialized",
                                }))
                                .unwrap(),
                            ))
                            .unwrap();
                        if let Some(token) = &self.token {
                            request = with_bearer(request, token);
                        }
                        request
                    })
                    .await;
                reply.assert_status(StatusCode::ACCEPTED);
            }
        }
    }

    /// Appelle un outil et rend ce qu'il a rendu : la réponse doit être un résultat, pas une erreur
    /// JSON-RPC.
    pub async fn call(&self, tool: &str, arguments: Value) -> ToolResult {
        let result = self
            .rpc(
                "tools/call",
                json!({ "name": tool, "arguments": arguments }),
            )
            .await;
        let content = result["content"].as_array().expect("un contenu");
        assert_eq!(content.len(), 1, "{result}");
        assert_eq!(content[0]["type"], "text", "{result}");
        assert!(
            result.get("structuredContent").is_none(),
            "que du texte : {result}"
        );
        ToolResult {
            text: content[0]["text"].as_str().unwrap().to_string(),
            is_error: result["isError"].as_bool().unwrap(),
        }
    }
}

impl ToolResult {
    /// Le contenu borné : ce qui est entre la ligne `<<<contenu N` et la ligne `>>>N`.
    pub fn content(&self) -> Option<&str> {
        let start = self.text.find("\n<<<contenu ")? + "\n<<<contenu ".len();
        let nonce_end = start + self.text[start..].find('\n')?;
        let nonce = &self.text[start..nonce_end];
        let end = self.text.rfind(&format!("\n>>>{nonce}"))?;
        Some(&self.text[nonce_end + 1..end])
    }

    /// Les lignes du serveur, avant le contenu borné.
    pub fn head(&self) -> &str {
        match self.text.find("\nCe qui suit est") {
            Some(end) => &self.text[..end],
            None => match self.text.find("\n<<<contenu ") {
                Some(end) => &self.text[..end],
                None => &self.text,
            },
        }
    }

    /// Le code d'un refus (`Erreur <code> : …`, la première ligne).
    pub fn code(&self) -> Option<&str> {
        let rest = self.text.strip_prefix("Erreur ")?;
        rest.split_once(" : ").map(|(code, _)| code)
    }

    /// L'outil a réussi.
    #[track_caller]
    pub fn ok(&self) -> &ToolResult {
        assert!(!self.is_error, "refus inattendu : {}", self.text);
        self
    }

    /// L'outil a refusé, sous ce code.
    #[track_caller]
    pub fn refused(&self, code: &str) -> &ToolResult {
        assert!(self.is_error, "réussite inattendue : {}", self.text);
        assert_eq!(self.code(), Some(code), "{}", self.text);
        self
    }

    /// La révision que dit la sortie (`révision r_…`).
    pub fn revision(&self) -> String {
        let at = self
            .head()
            .find("révision ")
            .unwrap_or_else(|| panic!("aucune révision : {}", self.text))
            + "révision ".len();
        self.head()[at..]
            .split([' ', ',', '.'])
            .next()
            .unwrap()
            .to_string()
    }

    /// Les identifiants (UUID) que porte un texte, dans l'ordre.
    pub fn uuids(text: &str) -> Vec<Uuid> {
        text.split(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
            .filter(|word| word.len() == 36)
            .filter_map(|word| Uuid::try_parse(word).ok())
            .collect()
    }
}

/// Un corps qui dit s'il a été lu : la preuve qu'un refus part sans lire le corps (R505).
pub struct Watched {
    read: Arc<AtomicBool>,
    data: Option<Bytes>,
}

impl Watched {
    /// Le corps `data`, et ce qui dira s'il a été lu.
    pub fn body(data: Vec<u8>) -> (Body, Arc<AtomicBool>) {
        let read = Arc::new(AtomicBool::new(false));
        let body = Body::new(Watched {
            read: Arc::clone(&read),
            data: Some(Bytes::from(data)),
        });
        (body, read)
    }
}

impl http_body::Body for Watched {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        self.read.store(true, Ordering::SeqCst);
        Poll::Ready(self.data.take().map(|data| Ok(Frame::data(data))))
    }
}
