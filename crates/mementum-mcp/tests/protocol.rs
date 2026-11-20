//! Le protocole MCP, dans ses deux révisions (Rulings R1236, R1251) : la table de cas
//! `spec/mcp/messages.json`, puis ce que la table ne dit pas d'un seul cas.

#[path = "../../mementum-md/tests/common/linear.rs"]
mod linear;

use std::future::Future;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use linear::{Answer as Size, Form, Limits, grows_like_input};
use mementum_mcp::protocol::{
    self, Answer, Endpoint, Era, Headers, SUPPORTED, TTL_MS, V2025_11_25, V2026_07_28,
};
use mementum_mcp::text::{self, SpaceLine, TokenLine, Workspace};
use mementum_mcp::tools::{self, ToolCall};
use mementum_mcp::{Host, Reply, ToolOutput, serve, spec};
use serde_json::{Value, json};

/// Mène un futur qui ne s'endort jamais (les hôtes de ces tests répondent tout de suite).
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

/// Un serveur de test : `workspace_info` réussit, tout autre outil est refusé ; il compte ses
/// appels.
#[derive(Default)]
struct FakeHost {
    calls: AtomicUsize,
}

impl Host for FakeHost {
    fn call(&self, call: ToolCall) -> impl Future<Output = ToolOutput> + Send {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let output = match call {
            ToolCall::WorkspaceInfo(_) => text::workspace(&Workspace {
                account: "Alex".into(),
                token: TokenLine {
                    name: "Claude Code".into(),
                    level: "edit".into(),
                    scope: "space".into(),
                    expires_at: None,
                    can_trash: false,
                },
                spaces: vec![SpaceLine {
                    id: "0193a1b2-0000-7000-8000-000000000001".into(),
                    name: "Pro".into(),
                    role: "owner".into(),
                }],
                space: None,
            }),
            _ => text::error(
                "not_found",
                "Page introuvable : vérifie son identifiant.",
                None,
            ),
        };
        async move { output }
    }
}

/// Les valeurs d'un en-tête d'un cas : une valeur, ou une liste.
fn header_values(case: &Value, name: &str) -> Vec<Vec<u8>> {
    let Some(headers) = case.get("headers").and_then(Value::as_object) else {
        return Vec::new();
    };
    headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .flat_map(|(_, value)| match value {
            Value::String(one) => vec![one.as_bytes().to_vec()],
            Value::Array(many) => many
                .iter()
                .map(|v| v.as_str().unwrap().as_bytes().to_vec())
                .collect(),
            other => panic!("en-tête {name} illisible : {other}"),
        })
        .collect()
}

/// Une requête : en-têtes, corps, adresse.
struct Call {
    version: Vec<Vec<u8>>,
    method: Vec<Vec<u8>>,
    name: Vec<Vec<u8>>,
    body: Vec<u8>,
    endpoint: Endpoint,
}

impl Call {
    fn from_case(case: &Value) -> Call {
        let body = match (case.get("raw"), case.get("body")) {
            (Some(raw), _) => raw.as_str().unwrap().as_bytes().to_vec(),
            (None, Some(body)) => serde_json::to_vec(body).unwrap(),
            (None, None) => panic!("cas sans corps"),
        };
        Call {
            version: header_values(case, "MCP-Protocol-Version"),
            method: header_values(case, "Mcp-Method"),
            name: header_values(case, "Mcp-Name"),
            body,
            endpoint: match case.get("endpoint").and_then(Value::as_str) {
                Some("readonly") => Endpoint::ReadOnly,
                None => Endpoint::Full,
                Some(other) => panic!("adresse inconnue : {other}"),
            },
        }
    }

    /// Une requête de la 2026-07-28, en-têtes justes.
    fn modern(method: &str, name: Option<&str>, params: Value) -> Call {
        let mut params = params;
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": V2026_07_28,
            "io.modelcontextprotocol/clientCapabilities": {},
        });
        Call {
            version: vec![V2026_07_28.as_bytes().to_vec()],
            method: vec![method.as_bytes().to_vec()],
            name: name
                .map(|n| vec![n.as_bytes().to_vec()])
                .unwrap_or_default(),
            body: serde_json::to_vec(&json!({
                "jsonrpc": "2.0", "id": 1, "method": method, "params": params,
            }))
            .unwrap(),
            endpoint: Endpoint::Full,
        }
    }

    fn serve(&self, host: &FakeHost) -> Reply {
        let version: Vec<&[u8]> = self.version.iter().map(Vec::as_slice).collect();
        let method: Vec<&[u8]> = self.method.iter().map(Vec::as_slice).collect();
        let name: Vec<&[u8]> = self.name.iter().map(Vec::as_slice).collect();
        let headers = Headers {
            protocol_version: &version,
            method: &method,
            name: &name,
        };
        block_on(serve(&headers, &self.body, self.endpoint, host))
    }
}

/// `expected` est-il un sous-ensemble d'`actual` (voir la description de la table) ?
fn contains(actual: &Value, expected: &Value, at: &str) -> Result<(), String> {
    match (actual, expected) {
        (_, Value::String(any)) if any == "<any>" => Ok(()),
        (Value::Object(actual), Value::Object(expected)) => {
            for (key, value) in expected {
                let path = format!("{at}/{key}");
                match actual.get(key) {
                    Some(found) => contains(found, value, &path)?,
                    None => return Err(format!("{path} manque")),
                }
            }
            Ok(())
        }
        (Value::Array(actual), Value::Array(expected)) => {
            if actual.len() != expected.len() {
                return Err(format!(
                    "{at} : {} éléments au lieu de {}",
                    actual.len(),
                    expected.len()
                ));
            }
            for (i, (found, value)) in actual.iter().zip(expected).enumerate() {
                contains(found, value, &format!("{at}/{i}"))?;
            }
            Ok(())
        }
        (actual, expected) if actual == expected => Ok(()),
        (actual, expected) => Err(format!("{at} : {actual} au lieu de {expected}")),
    }
}

fn table() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/mcp/messages.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_message_table_holds_in_both_eras() {
    let table = table();
    let cases = table["cases"].as_array().unwrap();
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        assert!(
            case["proven_by"].is_string(),
            "{name} : « proven_by » manque"
        );
        let host = FakeHost::default();
        let reply = Call::from_case(case).serve(&host);
        let status = case["status"].as_u64().unwrap() as u16;
        if reply.status != status {
            failures.push(format!(
                "{name} : statut {} au lieu de {status} ({:?})",
                reply.status, reply.body
            ));
            continue;
        }
        match (&reply.body, &case["expect"]) {
            (None, Value::Null) => {}
            (Some(body), Value::Null) => failures.push(format!("{name} : corps inattendu {body}")),
            (None, _) => failures.push(format!("{name} : pas de corps")),
            (Some(body), expected) => {
                if let Err(e) = contains(body, expected, "") {
                    failures.push(format!("{name} : {e} ; réponse {body}"));
                }
                for pointer in case["absent"].as_array().into_iter().flatten() {
                    let pointer = pointer.as_str().unwrap();
                    if body.pointer(pointer).is_some() {
                        failures.push(format!("{name} : {pointer} ne devrait pas y être"));
                    }
                }
            }
        }
        if let Some(called) = case["called"].as_bool() {
            let calls = host.calls.load(Ordering::SeqCst);
            if called != (calls == 1) {
                failures.push(format!("{name} : {calls} appels de l'outil"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // La table couvre les deux ères, et les requêtes réelles de Claude Code dans chacune.
    for era in [V2026_07_28, V2025_11_25] {
        let of_era = |case: &&Value| case["era"] == era;
        assert!(cases.iter().filter(of_era).count() >= 15, "{era}");
        assert!(
            cases
                .iter()
                .filter(of_era)
                .any(|c| c["name"].as_str().unwrap().contains("Claude Code 2.1.329")),
            "{era}"
        );
    }
}

/// Toute réponse de la 2026-07-28 qui porte un résultat porte `resultType: "complete"` et
/// l'identité du serveur ; aucune de la 2025-11-25.
#[test]
fn every_2026_result_carries_its_result_type() {
    let id = protocol::read(
        &Headers {
            protocol_version: &[V2026_07_28.as_bytes()],
            method: &[b"tools/list"],
            name: &[],
        },
        &Call::modern("tools/list", None, json!({})).body,
        Endpoint::Full,
    )
    .unwrap()
    .id
    .unwrap();
    let answers = [
        Answer::Discover,
        Answer::Initialize,
        Answer::Ping,
        Answer::ToolsList(Endpoint::Full),
        Answer::ToolsList(Endpoint::ReadOnly),
        Answer::ToolResult(text::error("not_found", "Introuvable.", None)),
        Answer::ResourcesList,
        Answer::ResourceRead(spec::MMD_URI.to_string()),
    ];
    for answer in answers {
        let modern = protocol::write(Era::V2026_07_28, &id, answer.clone());
        let body = modern.body.unwrap();
        assert_eq!(body["result"]["resultType"], "complete", "{answer:?}");
        assert_eq!(
            body["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "mementum"
        );
        let legacy = protocol::write(Era::V2025_11_25, &id, answer.clone());
        let body = legacy.body.unwrap();
        assert!(body["result"].get("resultType").is_none(), "{answer:?}");
        assert!(body["result"].get("_meta").is_none(), "{answer:?}");
    }
    // Dans la table aussi : chaque résultat d'une requête de la 2026-07-28.
    let table = table();
    for case in table["cases"].as_array().unwrap() {
        if case["era"] != V2026_07_28 {
            continue;
        }
        let reply = Call::from_case(case).serve(&FakeHost::default());
        if let Some(result) = reply.body.as_ref().and_then(|b| b.get("result")) {
            assert_eq!(result["resultType"], "complete", "{}", case["name"]);
        }
    }
}

/// `server/discover`, les listes et la lecture d'une ressource portent `ttlMs` et `cacheScope:
/// "private"` en 2026-07-28 (Ruling R1251) ; un appel d'outil n'en porte pas.
#[test]
fn discover_and_lists_carry_ttl_and_a_private_cache_scope() {
    let host = FakeHost::default();
    for (method, name, params) in [
        ("server/discover", None, json!({})),
        ("tools/list", None, json!({})),
        ("resources/list", None, json!({})),
        (
            "resources/read",
            Some(spec::MMD_URI),
            json!({ "uri": spec::MMD_URI }),
        ),
    ] {
        let reply = Call::modern(method, name, params).serve(&host);
        assert_eq!(reply.status, 200, "{method}");
        let body = reply.body.unwrap();
        assert_eq!(body["result"]["ttlMs"], json!(TTL_MS), "{method}");
        assert_eq!(body["result"]["cacheScope"], "private", "{method}");
    }
    let reply = Call::modern(
        "tools/call",
        Some("workspace_info"),
        json!({ "name": "workspace_info", "arguments": {} }),
    )
    .serve(&host);
    let body = reply.body.unwrap();
    assert!(body["result"].get("ttlMs").is_none());
    assert!(body["result"].get("cacheScope").is_none());
}

/// Une même sortie d'outil, mise en forme pour chaque révision, porte le même contenu.
#[test]
fn one_function_serves_both_eras() {
    let id = protocol::read(
        &Headers::default(),
        br#"{"jsonrpc":"2.0","id":"x","method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
        Endpoint::Full,
    )
    .unwrap()
    .id
    .unwrap();
    for output in [
        text::error("token_busy", "Trop d'appels.", Some(12)),
        text::created(&[text::Created {
            id: "0193a1b2-0000-7000-8000-000000000002".into(),
            title: "Compte rendu".into(),
            url: "/p/0193a1b2-0000-7000-8000-000000000002".into(),
        }]),
    ] {
        let modern = protocol::write(Era::V2026_07_28, &id, Answer::ToolResult(output.clone()));
        let legacy = protocol::write(Era::V2025_11_25, &id, Answer::ToolResult(output.clone()));
        let (modern, legacy) = (modern.body.unwrap(), legacy.body.unwrap());
        for key in ["content", "isError", "structuredContent"] {
            assert_eq!(
                modern["result"].get(key),
                legacy["result"].get(key),
                "{key}"
            );
        }
        assert_eq!(modern["result"]["content"][0]["text"], output.text());
        assert_eq!(modern["result"]["isError"], output.is_error());
    }
}

/// Un message mal formé reçoit son erreur, et coûte en proportion de sa taille, sans jamais
/// recopier l'entrée dans la réponse (Rulings R380, R434). La méthode est celle des tests de
/// linéarité (`mementum-md/tests/common/linear.rs`).
#[test]
fn malformed_messages_get_an_error_and_cost_nothing() {
    let meta = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}"#;
    let batch = |n: usize| {
        let one = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"},"#;
        format!("[{}]", one.repeat(n).trim_end_matches(','))
    };
    let unterminated =
        |n: usize| format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{}"#, "a".repeat(n));
    let deep = |n: usize| "[".repeat(n);
    let long_method = move |n: usize| {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"{}","params":{{{meta}}}}}"#,
            "m".repeat(n)
        )
    };
    let long_version = |n: usize| {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"{}"}}}}}}"#,
            "9".repeat(n)
        )
    };
    let long_id = |n: usize| {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"x"}}"#,
            "i".repeat(n)
        )
    };
    let long_tool = move |n: usize| {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{}","arguments":{{}},{meta}}}}}"#,
            "t".repeat(n)
        )
    };
    let long_uri = move |n: usize| {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{{"uri":"{}",{meta}}}}}"#,
            "u".repeat(n)
        )
    };
    let unknown_field = move |n: usize| {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"search","arguments":{{"query":"q","{}":1}},{meta}}}}}"#,
            "f".repeat(n)
        )
    };
    let unknown_variant = move |n: usize| {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"manage_pages","arguments":{{"action":"{}","page":"p"}},{meta}}}}}"#,
            "v".repeat(n)
        )
    };
    // Un `_meta` aux mille clés, sans `clientCapabilities`.
    let many_keys = |n: usize| {
        let keys: String = (0..n).map(|i| format!(r#","k{i}":0"#)).collect();
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28"{keys}}}}}}}"#
        )
    };
    type Build = Box<dyn Fn(usize) -> String>;
    type MakeCall = Box<dyn Fn(usize) -> Call>;
    let builds: Vec<(&str, Build)> = vec![
        ("un lot", Box::new(batch)),
        ("un JSON coupé", Box::new(unterminated)),
        ("des crochets imbriqués", Box::new(deep)),
        ("une méthode inconnue et longue", Box::new(long_method)),
        ("une version longue", Box::new(long_version)),
        ("un id long", Box::new(long_id)),
        ("un nom d'outil long", Box::new(long_tool)),
        ("une adresse de ressource longue", Box::new(long_uri)),
        ("un champ inconnu et long", Box::new(unknown_field)),
        ("un geste inconnu et long", Box::new(unknown_variant)),
        ("des params aux mille clés", Box::new(many_keys)),
    ];
    // Chaque requête porte des en-têtes qui disent ce que dit son corps, lus à sa construction
    // (hors de la mesure), pour qu'elle aille aussi loin que le protocole la laisse aller.
    let calls: Vec<(&str, MakeCall)> = builds
        .into_iter()
        .map(|(what, build)| {
            let call: MakeCall = Box::new(move |n| {
                let body = build(n).into_bytes();
                let parsed: Option<Value> = serde_json::from_slice(&body).ok();
                let said = |pointer: &str| {
                    parsed
                        .as_ref()
                        .and_then(|v| v.pointer(pointer))
                        .and_then(Value::as_str)
                        .map(|s| s.as_bytes().to_vec())
                };
                Call {
                    version: vec![V2026_07_28.as_bytes().to_vec()],
                    method: said("/method").into_iter().collect(),
                    name: said("/params/name")
                        .or(said("/params/uri"))
                        .into_iter()
                        .collect(),
                    body,
                    endpoint: Endpoint::Full,
                }
            });
            (what, call)
        })
        .collect();
    let forms: Vec<Form<'_, Call>> = calls
        .iter()
        .map(|(what, build)| Form {
            what,
            build: build.as_ref(),
        })
        .collect();
    let host = FakeHost::default();
    let failures = grows_like_input(
        &forms,
        |call: &Call| call.body.len(),
        |call: &Call| call.serve(&host),
        |reply: &Reply| reply.to_bytes().len(),
        Limits {
            peak_per_byte: 64,
            answer: Size::Absolute(4096),
        },
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        host.calls.load(Ordering::SeqCst),
        0,
        "un outil a été appelé"
    );
    // Chaque forme reçoit bien une erreur (et pas un résultat), à 2 Mio aussi.
    for form in &forms {
        let mut n = 1;
        while (form.build)(n).body.len() < 2 << 20 {
            n *= 2;
        }
        let call = (form.build)(n);
        let reply = call.serve(&host);
        assert!(reply.to_bytes().len() <= 4096, "{}", form.what);
        let body = reply.body.unwrap_or_default();
        let refused = body.get("error").is_some() || body["result"]["isError"] == true;
        assert!(refused, "{} : {body}", form.what);
    }
}

#[test]
fn the_readonly_list_has_three_tools() {
    let names = |endpoint| -> Vec<&str> { tools::list(endpoint).iter().map(|t| t.name).collect() };
    assert_eq!(
        names(Endpoint::ReadOnly),
        ["search", "fetch", "workspace_info"]
    );
    assert!(tools::list(Endpoint::ReadOnly).iter().all(|t| t.read_only));
    for tool in tools::list(Endpoint::Full) {
        let served = tools::find(tool.name, Endpoint::ReadOnly).is_some();
        assert_eq!(served, tool.read_only, "{}", tool.name);
    }
}

#[test]
fn the_tools_come_in_a_fixed_order() {
    let names: Vec<&str> = tools::list(Endpoint::Full).iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        [
            "search",
            "fetch",
            "create_pages",
            "update_page",
            "manage_pages",
            "workspace_info"
        ]
    );
    // Deux listes de suite sont les mêmes, octet pour octet (un client les garde en cache).
    let a: Vec<Value> = tools::list(Endpoint::Full)
        .iter()
        .map(|t| t.to_json())
        .collect();
    let b: Vec<Value> = tools::list(Endpoint::Full)
        .iter()
        .map(|t| t.to_json())
        .collect();
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

/// `spec/mcp/tools.json` est à jour (Ruling R1238) ; `UPDATE_TOOLS=1` le récrit.
#[test]
fn tool_schemas_are_the_committed_ones() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/mcp/tools.json");
    let mut expected = serde_json::to_string_pretty(&tools::catalog()).unwrap();
    expected.push('\n');
    if std::env::var_os("UPDATE_TOOLS").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} illisible ({e}) : lancer avec UPDATE_TOOLS=1",
            path.display()
        )
    });
    assert_eq!(
        committed, expected,
        "spec/mcp/tools.json périmé : lancer avec UPDATE_TOOLS=1"
    );
}

/// Chaque descripteur a la forme de `tools/list` : un schéma d'entrée objet, fermé, et des
/// annotations qui disent ce que fait l'outil.
#[test]
fn each_descriptor_has_a_closed_object_schema_and_annotations() {
    for tool in tools::list(Endpoint::Full) {
        let json = tool.to_json();
        assert_eq!(json["name"], tool.name);
        assert!(!json["description"].as_str().unwrap().is_empty());
        let schema = &json["inputSchema"];
        assert_eq!(schema["type"], "object", "{}", tool.name);
        assert_eq!(schema["additionalProperties"], false, "{}", tool.name);
        assert!(
            schema.get("$ref").is_none() && schema.get("$defs").is_none(),
            "{}",
            tool.name
        );
        assert_eq!(
            json["annotations"]["readOnlyHint"], tool.read_only,
            "{}",
            tool.name
        );
        assert_eq!(json["annotations"]["openWorldHint"], false, "{}", tool.name);
    }
}

/// Aucun schéma ne déclare d'en-tête `Mcp-Param-*` (`x-mcp-header`, 2026-07-28) : la route n'en
/// lit aucun, et un client qui en enverrait un pour un argument déclaré attendrait qu'on le
/// compare au corps.
#[test]
fn no_schema_declares_an_mcp_param_header() {
    let catalog = serde_json::to_string(&tools::catalog()).unwrap();
    assert!(!catalog.contains("x-mcp-header"));
    assert!(!catalog.to_ascii_lowercase().contains("mcp-param"));
}

/// Aucune description d'outil ne porte la spécification du MMD : elle est dans la ressource
/// (§6.2 ; Review Focus de M1).
#[test]
fn no_tool_description_carries_the_mmd_spec() {
    let catalog = serde_json::to_string(&tools::catalog()).unwrap();
    for mark in ["<callout", "<toggle", "header-rows", "&#32;", "<columns>"] {
        assert!(!catalog.contains(mark), "{mark}");
    }
    assert!(catalog.contains(spec::MMD_URI));
}

#[test]
fn the_spec_resource_is_the_committed_text() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/mmd/SPEC.md");
    let committed = std::fs::read_to_string(path).unwrap();
    assert_eq!(spec::MMD, committed);
    let resource = spec::find(spec::MMD_URI).unwrap();
    assert_eq!(resource.text, committed);
    assert_eq!(resource.mime_type, "text/markdown");
    let reply = Call::modern(
        "resources/read",
        Some(spec::MMD_URI),
        json!({ "uri": spec::MMD_URI }),
    )
    .serve(&FakeHost::default());
    assert_eq!(
        reply.body.unwrap()["result"]["contents"][0]["text"],
        committed
    );
}

/// Les exemples des outils et des refus sont des arguments que l'outil accepte.
#[test]
fn every_example_is_accepted() {
    for tool in tools::list(Endpoint::Full) {
        let arguments = tool.example.as_object().unwrap().clone();
        assert!(tools::parse(tool.name, arguments).is_ok(), "{}", tool.name);
    }
    for (code, hint) in text::hints() {
        if let Some((tool, example)) = hint.example {
            let arguments: Value = serde_json::from_str(example).unwrap();
            let parsed = tools::parse(tool, arguments.as_object().unwrap().clone());
            assert!(parsed.is_ok(), "exemple de {code} : {parsed:?}");
        }
    }
}

/// Des arguments refusés font un échec de l'outil (`isError`) qui nomme le champ en cause, en
/// français, et montre un exemple ; jamais une longue part de l'entrée.
#[test]
fn refused_arguments_name_the_field_and_show_an_example() {
    let cases = [
        ("workspace_info", json!({ "space": 3 }), "space"),
        ("fetch", json!({}), "id"),
        ("fetch", json!({ "id": "p", "lien": true }), "lien"),
        ("fetch", json!({ "id": "p", "limit": 0 }), "limit"),
        ("fetch", json!({ "id": "p", "limit": 65537 }), "limit"),
        ("search", json!({ "query": "" }), "query"),
        ("search", json!({ "query": "q".repeat(201) }), "query"),
        ("create_pages", json!({ "pages": [] }), "pages"),
        (
            "create_pages",
            json!({ "pages": vec![json!({ "title": "t" }); 21] }),
            "pages",
        ),
        (
            "create_pages",
            json!({ "pages": [{ "title": "t".repeat(501) }] }),
            "title",
        ),
        (
            "update_page",
            json!({ "page": "p", "operations": [] }),
            "operations",
        ),
        (
            "update_page",
            json!({ "page": "p", "operations": vec![json!({ "op": "delete_block", "anchor": "b" }); 101] }),
            "operations",
        ),
        (
            "update_page",
            json!({ "page": "p", "operations": [{ "op": "str_replace", "old": "o".repeat(2001), "new": "" }] }),
            "old",
        ),
        (
            "update_page",
            json!({ "page": "p", "operations": [{ "op": "insert", "markdown": "x" }] }),
            "after",
        ),
        (
            "update_page",
            json!({ "page": "p", "operations": [{ "op": "insert", "after": "a", "before": "b", "markdown": "x" }] }),
            "after",
        ),
        (
            "update_page",
            json!({ "page": "p", "operations": [{ "op": "explode" }] }),
            "explode",
        ),
        (
            "update_page",
            json!({ "page": "p", "operations": [{ "op": "str_replace", "old": "a", "new": "b", "zzz": 1 }] }),
            "zzz",
        ),
        (
            "update_page",
            json!({ "page": "p", "operations": [{ "old": "a", "new": "b" }] }),
            "op",
        ),
        (
            "manage_pages",
            json!({ "action": "rename", "page": "p" }),
            "title",
        ),
        (
            "manage_pages",
            json!({ "action": "trash", "page": "p", "title": "t" }),
            "title",
        ),
        (
            "manage_pages",
            json!({ "action": "trash", "page": "p", "after": "q" }),
            "after",
        ),
    ];
    for (tool, arguments, field) in cases {
        let refused = tools::parse(tool, arguments.as_object().unwrap().clone()).unwrap_err();
        assert!(refused.is_error(), "{tool} {arguments}");
        let text = refused.text();
        assert!(
            text.contains("invalid_arguments"),
            "{tool} {arguments} : {text}"
        );
        assert!(
            text.contains(field),
            "{tool} {arguments} : « {field} » manque à {text}"
        );
        assert!(text.contains("Exemple"), "{tool} : {text}");
        assert!(text.len() < 2_048, "{tool} : {} octets", text.len());
        let structured = refused.structured().unwrap();
        assert_eq!(structured["code"], "invalid_arguments");
    }
}

/// Une requête que le protocole refuse n'appelle jamais l'outil ; une requête sans `id` non plus
/// (personne ne lirait le résultat d'une écriture).
#[test]
fn a_refused_request_never_reaches_the_tool() {
    let host = FakeHost::default();
    let mut call = Call::modern(
        "tools/call",
        Some("workspace_info"),
        json!({ "name": "workspace_info", "arguments": {} }),
    );
    let mut body: Value = serde_json::from_slice(&call.body).unwrap();
    body.as_object_mut().unwrap().remove("id");
    call.body = serde_json::to_vec(&body).unwrap();
    assert_eq!(call.serve(&host).status, 400);
    call.name = vec![b"search".to_vec()];
    body["id"] = json!(1);
    call.body = serde_json::to_vec(&body).unwrap();
    assert_eq!(call.serve(&host).status, 400);
    assert_eq!(host.calls.load(Ordering::SeqCst), 0);
}

/// Les révisions servies sont annoncées dans l'ordre, la plus récente d'abord.
#[test]
fn the_newest_revision_comes_first() {
    assert_eq!(SUPPORTED, [V2026_07_28, V2025_11_25]);
}
