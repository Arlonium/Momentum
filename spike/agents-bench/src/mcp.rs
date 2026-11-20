//! Step 1 : un `/mcp` d'un seul outil (`echo`), sans état, réponses JSON, jeton en `Authorization`,
//! qui sert les deux formes de R1236 (2026-07-28 et 2025-11-25), ou l'une seule (`--serve`), et
//! relève chaque requête reçue (méthode HTTP, chemin, en-têtes, corps) et sa réponse.
//!
//!   agents-bench mcp <port> [--serve both|2026|2025] [--token <jeton factice>] [--log <fichier>]
//!
//! N'écoute que sur 127.0.0.1. Jetable : ce n'est pas la crate `mementum-mcp`.

use std::io::Write as _;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use serde_json::{Value, json};

const MODERN: &str = "2026-07-28";
const LEGACY: &str = "2025-11-25";
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPS: &str = "io.modelcontextprotocol/clientCapabilities";
const SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
const SPEC_URI: &str = "mementum://spec/mmd";

#[derive(Clone, Copy, PartialEq)]
enum Serve {
    Both,
    Modern,
    Legacy,
}

struct Bench {
    serve: Serve,
    token: String,
    log: Mutex<std::fs::File>,
    /// `--refuse <méthode>:<401|mismatch>` : refuser cette méthode JSON-RPC ainsi (le jeton
    /// révoqué en cours de session, un en-tête jugé faux), pour voir ce qu'en fait le client.
    refuse: Option<(String, String)>,
}

pub fn run(args: &[String]) {
    let port: u16 = args[0].parse().unwrap();
    let opt = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let serve = match opt("--serve").as_deref() {
        Some("2026") => Serve::Modern,
        Some("2025") => Serve::Legacy,
        _ => Serve::Both,
    };
    let token = opt("--token").unwrap_or_else(|| "mmt_banc_factice".into());
    let log = opt("--log").unwrap_or_else(|| "mcp-log.jsonl".into());
    let refuse = opt("--refuse").and_then(|r| {
        r.split_once(':')
            .map(|(m, k)| (m.to_string(), k.to_string()))
    });
    let bench = Arc::new(Bench {
        serve,
        token,
        refuse,
        log: Mutex::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log)
                .unwrap(),
        ),
    });
    let app = Router::new()
        .route("/mcp", any(endpoint))
        .fallback(fallback)
        .with_state(bench);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        println!("banc /mcp sur http://127.0.0.1:{port}/mcp ; journal {log}");
        axum::serve(listener, app).await.unwrap();
    });
}

fn masked(headers: &HeaderMap) -> Value {
    let mut out = serde_json::Map::new();
    for (name, value) in headers {
        let v = value.to_str().unwrap_or("<non ASCII>").to_string();
        let v = if name == header::AUTHORIZATION {
            // Le jeton du banc est factice ; on ne garde que sa forme.
            match v.split_once(' ') {
                Some((scheme, rest)) => format!("{scheme} <{} caractères, commence par {:?}>", rest.len(), rest.chars().take(6).collect::<String>()),
                None => format!("<{} caractères>", v.len()),
            }
        } else {
            v
        };
        out.insert(name.to_string(), json!(v));
    }
    Value::Object(out)
}

fn record(bench: &Bench, method: &Method, uri: &Uri, headers: &HeaderMap, body: &[u8], status: StatusCode, answer: &str) {
    let body: Value = serde_json::from_slice(body)
        .unwrap_or_else(|_| json!(String::from_utf8_lossy(body).to_string()));
    let line = json!({
        "http": method.as_str(),
        "path": uri.to_string(),
        "headers": masked(headers),
        "body": body,
        "status": status.as_u16(),
        "answer": serde_json::from_str::<Value>(answer).unwrap_or(json!(answer)),
    });
    println!("{line}");
    let mut file = bench.log.lock().unwrap();
    let _ = writeln!(file, "{line}");
}

async fn fallback(State(bench): State<Arc<Bench>>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    record(&bench, &method, &uri, &headers, &body, StatusCode::NOT_FOUND, "");
    StatusCode::NOT_FOUND.into_response()
}

fn reply(status: StatusCode, body: Value) -> (StatusCode, String) {
    (status, body.to_string())
}

fn error(id: &Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut e = json!({ "code": code, "message": message });
    if let Some(d) = data {
        e["data"] = d;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": e })
}

fn result(id: &Value, r: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": r })
}

fn supported(serve: Serve) -> Vec<&'static str> {
    match serve {
        Serve::Both => vec![MODERN, LEGACY],
        Serve::Modern => vec![MODERN],
        Serve::Legacy => vec![LEGACY],
    }
}

fn echo_tool() -> Value {
    json!({
        "name": "echo",
        "title": "Écho",
        "description": "Rend le texte reçu, tel quel (outil du banc de mesure).",
        "inputSchema": {
            "type": "object",
            "properties": { "text": { "type": "string", "description": "Le texte à rendre." } },
            "required": ["text"],
            "additionalProperties": false
        }
    })
}

/// `=?base64?…?=` décodé, sinon la valeur telle quelle.
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(name)?.to_str().ok()?.to_string();
    if let Some(inner) = raw.strip_prefix("=?base64?").and_then(|r| r.strip_suffix("?=")) {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(inner).ok()?;
        return String::from_utf8(bytes).ok();
    }
    Some(raw)
}

async fn endpoint(State(bench): State<Arc<Bench>>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let (status, answer) = answer(&bench, &method, &headers, &body);
    record(&bench, &method, &uri, &headers, &body, status, &answer);
    let mut response = (status, answer.clone()).into_response();
    let h = response.headers_mut();
    if !answer.is_empty() {
        h.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    }
    if status == StatusCode::UNAUTHORIZED {
        h.insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap());
    }
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

fn answer(bench: &Bench, method: &Method, headers: &HeaderMap, body: &[u8]) -> (StatusCode, String) {
    let expected = format!("Bearer {}", bench.token);
    let auth = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    if auth != Some(expected.as_str()) {
        return reply(
            StatusCode::UNAUTHORIZED,
            json!({ "error": "invalid_token", "message": "Jeton révoqué, échu ou inconnu." }),
        );
    }
    if method != Method::POST {
        return (StatusCode::METHOD_NOT_ALLOWED, String::new());
    }
    let message: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return reply(StatusCode::BAD_REQUEST, error(&Value::Null, -32700, "Parse error", None)),
    };
    if message.is_array() {
        return reply(StatusCode::BAD_REQUEST, error(&Value::Null, -32600, "Batches are not supported", None));
    }
    let id = message.get("id").cloned();
    let rpc = message.get("method").and_then(Value::as_str).unwrap_or("");
    if let Some((refused, kind)) = &bench.refuse
        && refused == rpc
    {
        let rid = id.clone().unwrap_or(Value::Null);
        return match kind.as_str() {
            "401" => reply(
                StatusCode::UNAUTHORIZED,
                json!({ "error": "invalid_token", "message": "Jeton révoqué, échu ou inconnu : crée-en un autre dans ton compte." }),
            ),
            _ => reply(
                StatusCode::BAD_REQUEST,
                error(&rid, -32020, "Header mismatch: Mcp-Name header value does not match body value", None),
            ),
        };
    }
    let params = message.get("params").cloned().unwrap_or(json!({}));
    let meta_version = params
        .get("_meta")
        .and_then(|m| m.get(META_VERSION))
        .and_then(Value::as_str)
        .map(str::to_string);
    let header_version = header_value(headers, "mcp-protocol-version");

    // L'ère se lit au corps : une version dans `_meta` (2026-07-28), sinon `initialize` ou
    // l'en-tête d'une session ancienne (2025-11-25).
    if let Some(version) = meta_version {
        if bench.serve == Serve::Legacy {
            // Un serveur 2025-11-25 ne connaît pas ces méthodes.
            return match id {
                Some(id) => reply(StatusCode::OK, error(&id, -32601, "Method not found", None)),
                None => (StatusCode::ACCEPTED, String::new()),
            };
        }
        return modern(bench, headers, id, rpc, &params, &version, header_version);
    }
    if rpc == "initialize" || header_version.as_deref() == Some(LEGACY) {
        if bench.serve == Serve::Modern {
            let requested = params.get("protocolVersion").cloned().unwrap_or(Value::Null);
            return reply(
                StatusCode::BAD_REQUEST,
                error(
                    &id.unwrap_or(Value::Null),
                    -32022,
                    "Unsupported protocol version",
                    Some(json!({ "supported": supported(bench.serve), "requested": requested })),
                ),
            );
        }
        return legacy(id, rpc, &params);
    }
    reply(
        StatusCode::BAD_REQUEST,
        error(
            &id.unwrap_or(Value::Null),
            -32022,
            "Unsupported protocol version",
            Some(json!({ "supported": supported(bench.serve), "requested": header_version })),
        ),
    )
}

fn modern(
    bench: &Bench,
    headers: &HeaderMap,
    id: Option<Value>,
    rpc: &str,
    params: &Value,
    version: &str,
    header_version: Option<String>,
) -> (StatusCode, String) {
    let rid = id.clone().unwrap_or(Value::Null);
    if version != MODERN {
        return reply(
            StatusCode::BAD_REQUEST,
            error(
                &rid,
                -32022,
                "Unsupported protocol version",
                Some(json!({ "supported": supported(bench.serve), "requested": version })),
            ),
        );
    }
    let mismatch = |what: String| reply(StatusCode::BAD_REQUEST, error(&rid, -32020, &format!("Header mismatch: {what}"), None));
    if header_version.as_deref() != Some(version) {
        return mismatch(format!("MCP-Protocol-Version {header_version:?} ≠ {version}"));
    }
    let header_method = header_value(headers, "mcp-method");
    if header_method.as_deref() != Some(rpc) {
        return mismatch(format!("Mcp-Method {header_method:?} ≠ {rpc}"));
    }
    let named = match rpc {
        "tools/call" => params.get("name").and_then(Value::as_str),
        "resources/read" => params.get("uri").and_then(Value::as_str),
        _ => None,
    };
    if let Some(name) = named {
        let header_name = header_value(headers, "mcp-name");
        if header_name.as_deref() != Some(name) {
            return mismatch(format!("Mcp-Name {header_name:?} ≠ {name}"));
        }
    }
    if params.get("_meta").and_then(|m| m.get(META_CAPS)).is_none() {
        return reply(StatusCode::BAD_REQUEST, error(&rid, -32602, "Missing client capabilities", None));
    }
    let Some(id) = id else {
        return (StatusCode::ACCEPTED, String::new());
    };
    let info = json!({ SERVER_INFO: { "name": "agents-bench", "version": "0.0.0" } });
    let r = match rpc {
        "server/discover" => json!({
            "resultType": "complete",
            "supportedVersions": supported(bench.serve),
            "capabilities": { "tools": {}, "resources": {} },
            "instructions": "Banc de mesure : un seul outil, echo.",
            "ttlMs": 60000,
            "cacheScope": "private",
            "_meta": info,
        }),
        "tools/list" => json!({
            "resultType": "complete",
            "tools": [echo_tool()],
            "ttlMs": 60000,
            "cacheScope": "private",
            "_meta": info,
        }),
        "tools/call" => match params.get("name").and_then(Value::as_str) {
            Some("echo") => json!({
                "resultType": "complete",
                "content": [{ "type": "text", "text": params["arguments"]["text"].as_str().unwrap_or("") }],
                "isError": false,
                "_meta": info,
            }),
            other => return reply(StatusCode::OK, error(&id, -32602, &format!("Unknown tool: {other:?}"), None)),
        },
        "resources/list" => json!({
            "resultType": "complete",
            "resources": [{ "uri": SPEC_URI, "name": "Format MMD", "mimeType": "text/markdown" }],
            "ttlMs": 60000,
            "cacheScope": "private",
            "_meta": info,
        }),
        "resources/read" => match params.get("uri").and_then(Value::as_str) {
            Some(SPEC_URI) => json!({
                "resultType": "complete",
                "contents": [{ "uri": SPEC_URI, "mimeType": "text/markdown", "text": "# MMD (banc)" }],
                "ttlMs": 60000,
                "cacheScope": "private",
                "_meta": info,
            }),
            other => return reply(StatusCode::OK, error(&id, -32602, "Resource not found", Some(json!({ "uri": other })))),
        },
        _ => return reply(StatusCode::NOT_FOUND, error(&id, -32601, "Method not found", None)),
    };
    reply(StatusCode::OK, result(&id, r))
}

fn legacy(id: Option<Value>, rpc: &str, params: &Value) -> (StatusCode, String) {
    let Some(id) = id else {
        // `notifications/initialized` et toute notification.
        return (StatusCode::ACCEPTED, String::new());
    };
    let r = match rpc {
        "initialize" => json!({
            "protocolVersion": LEGACY,
            "capabilities": { "tools": {}, "resources": {} },
            "serverInfo": { "name": "agents-bench", "version": "0.0.0" },
            "instructions": "Banc de mesure : un seul outil, echo.",
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": [echo_tool()] }),
        "tools/call" => match params.get("name").and_then(Value::as_str) {
            Some("echo") => json!({
                "content": [{ "type": "text", "text": params["arguments"]["text"].as_str().unwrap_or("") }],
                "isError": false,
            }),
            other => return reply(StatusCode::OK, error(&id, -32602, &format!("Unknown tool: {other:?}"), None)),
        },
        "resources/list" => json!({ "resources": [{ "uri": SPEC_URI, "name": "Format MMD", "mimeType": "text/markdown" }] }),
        "resources/read" => match params.get("uri").and_then(Value::as_str) {
            Some(SPEC_URI) => json!({ "contents": [{ "uri": SPEC_URI, "mimeType": "text/markdown", "text": "# MMD (banc)" }] }),
            other => return reply(StatusCode::OK, error(&id, -32002, "Resource not found", Some(json!({ "uri": other })))),
        },
        _ => return reply(StatusCode::OK, error(&id, -32601, "Method not found", None)),
    };
    reply(StatusCode::OK, result(&id, r))
}
