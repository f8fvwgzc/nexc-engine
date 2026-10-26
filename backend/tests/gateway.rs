//! The tools gateway (AgentSecOps phase 4): a workspace registers MCP
//! servers, their tools are offered to agents as `mcp:<server>/<tool>`, a
//! call is performed by the backend (over streamable HTTP, behind the SSRF
//! guard; a stdio server is never started) under the policy, the labels,
//! the DLP rules and the agent's pace, with a `tool_call` and an `egress`
//! span on the record; and every refusal is a policy span the model hears
//! about. `make prove-gateway` measures the gateway's own overhead per call.
#![forbid(unsafe_code)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use common::grpc_runtime::fake_grpc_runtime;
use common::{TestApp, collect_until};
use serde_json::{Value, json};
use sqlx::PgPool;

/// What the HTTP server saw: every JSON-RPC method, and the auth headers.
#[derive(Default)]
struct HttpSeen {
    methods: Vec<String>,
    auth: Vec<String>,
}

#[derive(Clone)]
struct HttpState {
    seen: Arc<Mutex<HttpSeen>>,
    /// Answer `tools/call` as an event stream rather than JSON.
    sse: bool,
}

/// An MCP server over streamable HTTP: `echo` says its text back, `big`
/// answers with as many bytes as asked, and `secret` needs the right
/// `Authorization` header to answer.
async fn http_rpc(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: String,
) -> axum::response::Response {
    let message: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let method = message["method"].as_str().unwrap_or("").to_owned();
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    {
        let mut seen = state.seen.lock().unwrap();
        seen.methods.push(method.clone());
        seen.auth.push(auth.clone());
    }
    let id = message["id"].clone();
    if method == "notifications/initialized" {
        return StatusCode::ACCEPTED.into_response();
    }
    if method != "initialize"
        && headers.get("mcp-session-id").and_then(|v| v.to_str().ok()) != Some("session-7")
    {
        return (StatusCode::BAD_REQUEST, "no session").into_response();
    }
    let result = match method.as_str() {
        "initialize" => json!({"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
            "serverInfo": {"name": "http-test", "version": "2.0"}}),
        "tools/list" => json!({"tools": [
            {"name": "echo", "description": "Says the text back.",
             "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}},
            {"name": "big", "description": "Answers with n bytes.",
             "inputSchema": {"type": "object", "properties": {"n": {"type": "integer"}}}},
            {"name": "secret", "description": "Needs the header.", "inputSchema": {"type": "object", "properties": {}}},
        ]}),
        "tools/call" => {
            let name = message["params"]["name"].as_str().unwrap_or("");
            let args = &message["params"]["arguments"];
            match name {
                "echo" => json!({"content": [{"type": "text",
                    "text": format!("echo: {}", args["text"].as_str().unwrap_or(""))}]}),
                "big" => json!({"content": [{"type": "text",
                    "text": "y".repeat(args["n"].as_u64().unwrap_or(0) as usize)}]}),
                "secret" if auth == "Bearer s3cr3t-header" => {
                    json!({"content": [{"type": "text", "text": "the header was right"}]})
                }
                "secret" => {
                    json!({"content": [{"type": "text", "text": "no header"}], "isError": true})
                }
                _ => {
                    json!({"content": [{"type": "text", "text": "unknown tool"}], "isError": true})
                }
            }
        }
        _ => {
            let reply = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}});
            return axum::Json(reply).into_response();
        }
    };
    let reply = json!({"jsonrpc": "2.0", "id": id, "result": result});
    let mut response = if state.sse && method == "tools/call" {
        (
            [("content-type", "text/event-stream")],
            format!("event: message\ndata: {{\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}}\n\ndata: {reply}\n\n"),
        )
            .into_response()
    } else {
        axum::Json(reply).into_response()
    };
    if method == "initialize" {
        response
            .headers_mut()
            .insert("mcp-session-id", "session-7".parse().unwrap());
    }
    response
}

async fn http_server(sse: bool) -> (String, Arc<Mutex<HttpSeen>>) {
    let seen: Arc<Mutex<HttpSeen>> = Arc::default();
    let app = Router::new()
        .route("/mcp", post(http_rpc))
        .with_state(HttpState {
            seen: seen.clone(),
            sse,
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

async fn call(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let r = app
        .request(method, &format!("/api/v1{path}"), Some(token), body)
        .await;
    (r.status, r.body)
}

async fn workspace(app: &TestApp, token: &str) -> String {
    let (_, list) = call(app, Method::GET, "/workspaces", token, None).await;
    list[0]["id"].as_str().unwrap().to_owned()
}

async fn agent(app: &TestApp, token: &str, name: &str, role: &str, capabilities: Value) -> Value {
    let (status, agent) = call(
        app,
        Method::POST,
        "/agents",
        token,
        Some(json!({
            "name": name, "role": role, "runtime": "python",
            "purpose": format!("Handles {role} work in tests."),
            "capabilities": capabilities,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    agent
}

/// Registers a server; returns the answer.
async fn register(app: &TestApp, token: &str, wid: &str, body: Value) -> (StatusCode, Value) {
    call(
        app,
        Method::POST,
        &format!("/workspaces/{wid}/mcp-servers"),
        token,
        Some(body),
    )
    .await
}

fn http_body(name: &str, url: &str) -> Value {
    json!({"name": name, "transport": "http", "url": url})
}

/// Runs a graph of one `TOOL <tool>` node with `arguments`; returns the run
/// id, the final run and the spans.
async fn tool_run(
    app: &TestApp,
    token: &str,
    role: &str,
    tool: &str,
    arguments: Value,
) -> (String, Value, Vec<Value>) {
    let gid = app.graph(token, "Use a tool").await;
    app.node(
        token,
        &gid,
        json!({"title": format!("TOOL {tool}"), "kind": "task", "executor": "agent", "agent_role": role,
            "content": arguments.to_string()}),
    )
    .await;
    let mut rx = app.events(&gid);
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        token,
        Some(json!({"force": true})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let rid = body["id"].as_str().unwrap().to_owned();
    let events = collect_until(&mut rx, "run.finished").await;
    let finished = events.last().unwrap().1["run"].clone();
    let (status, page) = call(
        app,
        Method::GET,
        &format!("/runs/{rid}/trace?limit=1000"),
        token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    (rid, finished, page["spans"].as_array().unwrap().clone())
}

/// The gateway's answer to the last `TOOL` request of the stand-in.
fn last_answer(seen: &common::grpc_runtime::Seen) -> (String, bool) {
    let answers = seen.tool_results.lock().unwrap();
    let (_, output, is_error) = answers.last().cloned().expect("the gateway answered");
    (output, is_error)
}

async fn grpc_app(pool: PgPool, url: &str, extra: &[(&str, &str)]) -> TestApp {
    let mut vars = vec![
        ("NEXC_RUNTIME_PROTOCOL", "grpc"),
        ("NEXC_RUNTIME_GRPC_URL", url),
    ];
    vars.extend_from_slice(extra);
    TestApp::new(pool, &vars).await
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn admins_register_servers_and_the_gateway_lists_their_tools(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (member, _) = app.register("member@example.com").await;
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &owner,
        Some(json!({"email": "member@example.com", "role": "member"})),
    )
    .await;
    assert!(status.is_success(), "{body}");
    let (outsider, _) = app.register("outsider@example.com").await;
    let (server_url, _) = http_server(false).await;

    // Registering connects at once: the tools are listed and kept.
    let (status, server) = register(&app, &owner, &wid, http_body("files", &server_url)).await;
    assert_eq!(status, StatusCode::CREATED, "{server}");
    assert_eq!(server["transport"], "http");
    assert_eq!(server["last_check_ok"], true, "{server}");
    assert!(
        server["last_check_detail"]
            .as_str()
            .unwrap()
            .contains("3 tool(s) listed"),
        "{server}"
    );
    let tools: Vec<&str> = server["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(tools, ["echo", "big", "secret"]);
    assert_eq!(server["tools"][0]["input_schema"]["required"][0], "text");
    assert_eq!(server["max_label"], "internal");
    assert_eq!(server["has_auth_header"], false);
    let sid = server["id"].as_str().unwrap().to_owned();

    // Members see the list; only admins change it; outsiders see nothing.
    let (status, list) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/mcp-servers"),
        &member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    let (status, _) = register(&app, &member, &wid, http_body("theirs", &server_url)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/mcp-servers"),
        &outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/mcp-servers/{sid}"),
        &outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A second server of the same name, and inputs that do not fit.
    let (status, _) = register(&app, &owner, &wid, http_body("files", &server_url)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    for bad in [
        json!({"name": "Bad Name", "transport": "http", "url": server_url}),
        json!({"name": "nourl", "transport": "http"}),
        json!({"name": "ftp", "transport": "http", "url": "ftp://x"}),
        json!({"name": "mixed", "transport": "http", "url": "http://x", "command": "x"}),
        json!({"name": "header", "transport": "http", "url": server_url, "auth_header": "a\nb"}),
        json!({"name": "label", "transport": "http", "url": server_url, "max_label": "secret"}),
        json!({"name": "x", "transport": "http", "url": server_url, "unknown": 1}),
        // The backend never starts a command, whoever asks.
        json!({"name": "shell", "transport": "stdio", "command": "sh", "args": ["-c", "id"]}),
        // Nor connects to the cloud's metadata service.
        json!({"name": "meta", "transport": "http", "url": "http://169.254.169.254/latest/meta-data/"}),
    ] {
        let (status, body) = register(&app, &owner, &wid, bad.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {body}");
    }

    // A server that does not answer is registered, with the failure on it.
    let (status, broken) = register(
        &app,
        &owner,
        &wid,
        // Nothing listens on port 9 (discard).
        json!({"name": "broken", "transport": "http", "url": "http://127.0.0.1:9/mcp"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{broken}");
    assert_eq!(broken["last_check_ok"], false);
    assert!(
        broken["last_check_detail"]
            .as_str()
            .unwrap()
            .contains("not reachable"),
        "{broken}"
    );
    assert_eq!(broken["tools"].as_array().unwrap().len(), 0);
    let bid = broken["id"].as_str().unwrap().to_owned();
    let (status, check) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/mcp-servers/{bid}/check"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(check["ok"], false);

    // The check of the working one, and its removal, are audited.
    let (status, check) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/mcp-servers/{sid}/check"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{check}");
    assert_eq!(check["ok"], true);
    assert_eq!(check["tools"].as_array().unwrap().len(), 3);
    assert!(check["latency_ms"].as_u64().unwrap() < 15_000);
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/workspaces/{wid}/mcp-servers/{sid}"),
        &member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/workspaces/{wid}/mcp-servers/{sid}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/workspaces/{wid}/mcp-servers/{sid}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, entries) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/audit?limit=50"),
        &owner,
        None,
    )
    .await;
    let actions: Vec<&str> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"mcp_server_added") && actions.contains(&"mcp_server_removed"));
    assert!(
        entries
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["subject"] == "mcp:files"),
        "{entries}"
    );
}

/// The remote code execution a stdio server was: whoever may register a
/// server (any workspace admin, and with sign-ups on anyone owns a
/// workspace) chose a command the backend started. Registering one is
/// refused, and one registered before is listed but never started: its
/// check fails, its tools are not offered, and a call to one is refused.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_stdio_server_is_never_started_by_the_backend(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool.clone(), &url, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    // What the command would leave behind, if it ever ran.
    let marker = std::env::temp_dir().join(format!("nexc-stdio-{}", uuid::Uuid::now_v7()));
    let command = format!("touch {}", marker.display());
    let args = vec!["-c".to_owned(), command.clone()];

    let (status, body) = register(
        &app,
        &owner,
        &wid,
        json!({"name": "files", "transport": "stdio", "command": "sh", "args": args}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["errors"]["transport"][0]
            .as_str()
            .unwrap()
            .contains("not started by the backend"),
        "{body}"
    );

    // A server registered before, with the tools it listed then.
    let owner_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind("owner@example.com")
        .fetch_one(&pool)
        .await
        .unwrap();
    let sid = uuid::Uuid::now_v7();
    nexc::repo::mcp_servers::insert(
        &pool,
        sid,
        &nexc::repo::mcp_servers::NewMcpServer {
            workspace_id: wid.parse().unwrap(),
            name: "files",
            transport: nexc::domain::mcp::McpTransport::Stdio,
            command: Some("sh"),
            args: &args,
            url: None,
            auth_header: None,
            allowed_tools: &[],
            max_label: nexc::domain::capabilities::DataLabel::Internal,
            created_by: owner_id,
        },
    )
    .await
    .unwrap();
    let echo = nexc::domain::mcp::McpTool {
        name: "echo".into(),
        description: "Says the text back.".into(),
        input_schema: json!({"type": "object", "properties": {}}),
    };
    nexc::repo::mcp_servers::set_check(&pool, sid, true, "listed before", &[echo])
        .await
        .unwrap();
    let (status, list) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/mcp-servers"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list[0]["transport"], "stdio", "still listed, to be removed");

    let (status, check) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/mcp-servers/{sid}/check"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{check}");
    assert_eq!(check["ok"], false);
    assert!(
        check["detail"]
            .as_str()
            .unwrap()
            .contains("not started by the backend"),
        "{check}"
    );

    agent(
        &app,
        &owner,
        "Clerk",
        "clerk",
        json!({"tools": ["mcp:files/*", "finish"]}),
    )
    .await;
    let (_, finished, spans) = tool_run(
        &app,
        &owner,
        "clerk",
        "mcp:files/echo",
        json!({"text": "hello"}),
    )
    .await;
    assert_eq!(finished["status"], "succeeded", "{finished}");
    let (answer, is_error) = last_answer(&seen);
    assert!(is_error && answer.contains("mcp.transport"), "{answer}");
    let request = seen.requests.lock().unwrap()[0].clone();
    assert!(
        !request["gateway_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"].as_str().unwrap().starts_with("mcp:files/")),
        "not offered: {request}"
    );
    assert!(
        spans
            .iter()
            .any(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "mcp.transport")
    );
    assert!(!spans.iter().any(|s| s["kind"] == "egress"));
    assert!(!marker.exists(), "the command never ran");
}

/// The SSRF guard on MCP servers: an address on this machine or a private
/// network is refused at registration unless the operator opened its range
/// (`NEXC_EGRESS_ALLOW_PRIVATE`), and nothing connects to it.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_mcp_server_on_a_private_address_is_refused(pool: PgPool) {
    let app = TestApp::new(pool, &[("NEXC_EGRESS_ALLOW_PRIVATE", "")]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (server_url, seen) = http_server(false).await;
    let by_name = server_url.replace("127.0.0.1", "localhost");
    for (name, url) in [
        ("loopback", server_url.as_str()),
        ("named", by_name.as_str()),
    ] {
        let (status, body) = register(&app, &owner, &wid, http_body(name, url)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{url}: {body}");
        assert!(
            body["errors"]["url"][0]
                .as_str()
                .unwrap()
                .contains("NEXC_EGRESS_ALLOW_PRIVATE"),
            "{body}"
        );
    }
    assert!(seen.lock().unwrap().methods.is_empty(), "nothing connected");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_agent_calls_an_mcp_tool_over_http_through_the_gateway(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    for (sse, name) in [(false, "json"), (true, "sse")] {
        let (server_url, http_seen) = http_server(sse).await;
        let (status, server) = register(
            &app,
            &owner,
            &wid,
            json!({"name": name, "transport": "http", "url": server_url,
                "auth_header": "Bearer s3cr3t-header", "allowed_tools": ["echo", "secret"]}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{server}");
        assert_eq!(server["has_auth_header"], true);
        assert_eq!(server["last_check_ok"], true, "{server}");
        assert!(server.get("auth_header").is_none(), "never shown");
        agent(
            &app,
            &owner,
            &format!("Caller-{name}"),
            &format!("caller-{name}"),
            json!({"egress_hosts": ["127.0.0.1"]}),
        )
        .await;
        let (_, finished, spans) = tool_run(
            &app,
            &owner,
            &format!("caller-{name}"),
            &format!("mcp:{name}/secret"),
            json!({}),
        )
        .await;
        assert_eq!(finished["status"], "succeeded", "{finished}");
        let (answer, is_error) = last_answer(&seen);
        assert!(!is_error, "{answer}");
        assert_eq!(answer, "the header was right");
        let methods = http_seen.lock().unwrap().methods.clone();
        assert!(
            methods.contains(&"initialize".into())
                && methods.contains(&"notifications/initialized".into())
                && methods.contains(&"tools/call".into()),
            "{methods:?}"
        );
        assert!(
            http_seen
                .lock()
                .unwrap()
                .auth
                .iter()
                .all(|a| a == "Bearer s3cr3t-header")
        );
        let egress = spans.iter().find(|s| s["kind"] == "egress").unwrap();
        assert_eq!(egress["name"], "127.0.0.1");
        assert_eq!(egress["attributes"]["transport"], "http");
        assert!(
            egress["attributes"]["url"]
                .as_str()
                .unwrap()
                .ends_with("/mcp")
        );
        // The request only offered the allowed tools.
        let request = seen.requests.lock().unwrap().last().unwrap().clone();
        let offered: Vec<&str> = request["gateway_tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .filter(|t| t.starts_with("mcp:"))
            .collect();
        assert!(
            offered.contains(&format!("mcp:{name}/secret").as_str()),
            "{offered:?}"
        );
        assert!(
            !offered.contains(&format!("mcp:{name}/big").as_str()),
            "{offered:?}"
        );
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_gateway_refuses_outside_the_allow_lists_and_limits(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(
        pool,
        &url,
        &[
            ("NEXC_GATEWAY_MAX_RESPONSE_BYTES", "4096"),
            // Twelve a minute: a burst of three, then one every five seconds.
            ("NEXC_GATEWAY_CALLS_PER_MINUTE", "12"),
        ],
    )
    .await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (server_url, _) = http_server(false).await;
    let (status, _) = register(
        &app,
        &owner,
        &wid,
        json!({"name": "web", "transport": "http", "url": server_url, "allowed_tools": ["echo", "big"]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    agent(
        &app,
        &owner,
        "Clerk",
        "clerk",
        json!({"egress_hosts": ["127.0.0.1"]}),
    )
    .await;

    // A tool outside the server's allow-list.
    let (_, finished, spans) = tool_run(&app, &owner, "clerk", "mcp:web/secret", json!({})).await;
    assert_eq!(finished["status"], "succeeded");
    let (answer, is_error) = last_answer(&seen);
    assert!(is_error && answer.contains("mcp.allowed_tools"), "{answer}");
    let denial = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "mcp.allowed_tools")
        .expect("the refusal is a policy span");
    assert_eq!(denial["attributes"]["decision"], "deny");
    assert_eq!(denial["attributes"]["server"], "web");
    assert!(!spans.iter().any(|s| s["kind"] == "egress"));

    // An answer larger than the gateway passes on.
    let (_, _, spans) = tool_run(&app, &owner, "clerk", "mcp:web/big", json!({"n": 10_000})).await;
    let (answer, is_error) = last_answer(&seen);
    assert!(
        is_error && answer.contains("larger than 4096 bytes"),
        "{answer}"
    );
    let tool = spans.iter().find(|s| s["kind"] == "tool_call").unwrap();
    assert_eq!(tool["attributes"]["is_error"], true);
    let (_, _, _) = tool_run(&app, &owner, "clerk", "mcp:web/big", json!({"n": 5_000})).await;
    let (answer, is_error) = last_answer(&seen);
    assert!(
        is_error && answer.contains("larger than 4096 bytes"),
        "{answer}"
    );

    // The agent's pace: the three calls above were its burst; the fourth waits.
    let (_, _, spans) = tool_run(&app, &owner, "clerk", "mcp:web/echo", json!({"text": "x"})).await;
    let (answer, is_error) = last_answer(&seen);
    assert!(
        is_error && answer.contains("limits.gateway_calls_per_minute"),
        "{answer}"
    );
    assert!(
        spans.iter().any(|s| s["kind"] == "policy"
            && s["attributes"]["rule"] == "limits.gateway_calls_per_minute")
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn labels_deny_lists_and_personal_data_keep_arguments_in(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (server_url, http_seen) = http_server(false).await;
    let (status, _) = register(
        &app,
        &owner,
        &wid,
        json!({"name": "web", "transport": "http", "url": server_url, "max_label": "internal"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    // The same server by another name of the host, which the deny-list below does not name.
    let (status, _) = register(
        &app,
        &owner,
        &wid,
        http_body("files", &server_url.replace("127.0.0.1", "localhost")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // A node that read restricted data: the server is cleared for internal at most, and even a
    // server cleared for restricted may not receive it (the default DLP rules).
    tokio::fs::create_dir_all(app.state.settings.documents_dir())
        .await
        .unwrap();
    let r = app
        .send_bytes(
            &format!("/api/v1/workspaces/{wid}/documents?name=handbook.md"),
            &owner,
            b"# Handbook\n\nInvoices from the Rotterdam warehouse need a customs reference.\n"
                .to_vec(),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let did = r.body["id"].as_str().unwrap().to_owned();
    nexc::engine::knowledge::work(&app.state).await.unwrap();
    let (status, _) = call(
        &app,
        Method::PATCH,
        &format!("/workspaces/{wid}/documents/{did}"),
        &owner,
        Some(json!({"label": "restricted"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    agent(
        &app,
        &owner,
        "Reader",
        "reader",
        json!({"data": {"labels": ["public", "internal", "confidential", "restricted"]},
            "egress_hosts": ["127.0.0.1"]}),
    )
    .await;
    let gid = app
        .graph(&owner, "Invoices from the Rotterdam warehouse")
        .await;
    app.node(
        &owner,
        &gid,
        json!({"title": "TOOL mcp:web/echo", "kind": "task", "executor": "agent", "agent_role": "reader",
            "content": json!({"text": "Rotterdam customs reference"}).to_string()}),
    )
    .await;
    let mut rx = app.events(&gid);
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &owner,
        Some(json!({"force": true})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let rid = body["id"].as_str().unwrap().to_owned();
    let events = collect_until(&mut rx, "run.finished").await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    let request = seen.requests.lock().unwrap().last().unwrap().clone();
    assert_eq!(request["context"]["labels"], json!(["restricted"]));
    let (answer, is_error) = last_answer(&seen);
    assert!(is_error && answer.contains("mcp.max_label"), "{answer}");
    assert!(
        !http_seen
            .lock()
            .unwrap()
            .methods
            .contains(&"tools/call".to_owned()),
        "nothing reached the server"
    );
    let (_, page) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/trace?limit=1000"),
        &owner,
        None,
    )
    .await;
    let spans = page["spans"].as_array().unwrap();
    let denial = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "mcp.max_label")
        .unwrap();
    assert_eq!(denial["attributes"]["action"], "egress");
    assert_eq!(denial["name"], "127.0.0.1");

    // Same node, a server cleared for restricted: the DLP rules still keep restricted data in.
    let (status, _) = register(
        &app,
        &owner,
        &wid,
        json!({"name": "vault", "transport": "http", "url": server_url, "max_label": "restricted"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let gid = app
        .graph(&owner, "Invoices from the Rotterdam warehouse")
        .await;
    app.node(
        &owner,
        &gid,
        json!({"title": "TOOL mcp:vault/echo", "kind": "task", "executor": "agent", "agent_role": "reader",
            "content": json!({"text": "Rotterdam customs reference"}).to_string()}),
    )
    .await;
    let mut rx = app.events(&gid);
    call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &owner,
        Some(json!({"force": true})),
    )
    .await;
    collect_until(&mut rx, "run.finished").await;
    let (answer, is_error) = last_answer(&seen);
    assert!(is_error && answer.contains("dlp.labels"), "{answer}");
    let (_, alerts) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/alerts"),
        &owner,
        None,
    )
    .await;
    assert!(
        alerts
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["rule"] == "dlp.labels" && a["kind"] == "dlp"),
        "{alerts}"
    );

    // A host on the policy's deny-list, and personal data in the arguments under `pii: deny`.
    let (status, policy) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        &owner,
        Some(json!({"scope": "workspace", "scope_id": null,
            "document": {"egress": {"deny": ["127.0.0.1"]}, "dlp": {"pii": {"action": "deny"}}}})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{policy}");
    agent(
        &app,
        &owner,
        "Clerk",
        "clerk",
        json!({"egress_hosts": ["127.0.0.1", "localhost"]}),
    )
    .await;
    let (_, _, spans) =
        tool_run(&app, &owner, "clerk", "mcp:web/echo", json!({"text": "hi"})).await;
    let (answer, is_error) = last_answer(&seen);
    assert!(is_error && answer.contains("egress.deny"), "{answer}");
    assert!(
        spans
            .iter()
            .any(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "egress.deny")
    );
    let (_, _, spans) = tool_run(
        &app,
        &owner,
        "clerk",
        "mcp:files/echo",
        json!({"text": "write to alice@example.com about it"}),
    )
    .await;
    let (answer, is_error) = last_answer(&seen);
    assert!(is_error && answer.contains("dlp.pii"), "{answer}");
    let denial = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "dlp.pii")
        .unwrap();
    assert_eq!(denial["attributes"]["action"], "egress");
    assert!(
        !spans.iter().any(|s| s["kind"] == "egress"),
        "the arguments never left"
    );
    // Plain arguments still go through to the server under the same policy.
    let (_, _, _) = tool_run(
        &app,
        &owner,
        "clerk",
        "mcp:files/echo",
        json!({"text": "plain"}),
    )
    .await;
    let (answer, is_error) = last_answer(&seen);
    assert!(!is_error, "{answer}");
    assert_eq!(answer, "echo: plain");
}

/// Percentile of sorted samples.
fn pct(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

/// `make prove-gateway`, first half: the gateway's own overhead per call
/// (the pace, the decisions, the DLP screen, the spans), the tool itself
/// excluded, over 500 calls of an MCP tool over HTTP and 500 of
/// `fetch_url`, through `gateway::perform` on a real node of a real run.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "a measurement: run with `make prove-gateway`"]
async fn prove_gateway(pool: PgPool) {
    use nexc::domain::capabilities::Capabilities;
    use nexc::domain::policy::Rules;
    use nexc::domain::settings::LlmProviderKind;
    use nexc::engine::approvals::Granted;
    use nexc::engine::executor::ExecContext;
    use nexc::gateway::{self, ToolRequest};
    use nexc::llm::LlmTarget;

    // Within the pace's burst (a quarter of the minute's allowance, 1,000 at most).
    const CALLS: usize = 500;
    let app = TestApp::new(pool.clone(), &[("NEXC_GATEWAY_CALLS_PER_MINUTE", "10000")]).await;
    let (owner, _) = app.register("prover@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (server_url, _) = http_server(false).await;
    let (status, _) = register(
        &app,
        &owner,
        &wid,
        json!({"name": "web", "transport": "http", "url": server_url}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let site = {
        let app = Router::new().route(
            "/status",
            axum::routing::get(|| async { axum::Json(json!({"ok": true})) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/status", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        url
    };
    let created = agent(
        &app,
        &owner,
        "Prover",
        "prover",
        json!({"egress_hosts": ["127.0.0.1"]}),
    )
    .await;
    let agent_id: uuid::Uuid = created["id"].as_str().unwrap().parse().unwrap();
    let workspace_id: uuid::Uuid = wid.parse().unwrap();
    let gid = app.graph(&owner, "Prove the gateway").await;
    let node_id = app
        .node(&owner, &gid, json!({"title": "Work", "executor": "agent"}))
        .await;
    let graph_id: uuid::Uuid = gid.parse().unwrap();
    let node = nexc::repo::nodes::find(&pool, graph_id, node_id.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    let agent = nexc::repo::agents::find(&pool, workspace_id, agent_id)
        .await
        .unwrap()
        .unwrap();
    let owner_id: uuid::Uuid = sqlx::query_scalar("SELECT owner_id FROM graphs WHERE id = $1")
        .bind(graph_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    // A run that is already running, so no dispatcher picks it up; its trace is real.
    let run_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runs (id, graph_id, owner_id, status, max_concurrency, started_at)
         VALUES ($1, $2, $3, 'running', 1, now())",
    )
    .bind(run_id)
    .bind(graph_id)
    .bind(owner_id)
    .execute(&pool)
    .await
    .unwrap();
    let lease = nexc::engine::lease::Lease::unfenced(run_id);
    let sink = nexc::engine::trace::start(
        app.state.clone(),
        run_id,
        graph_id,
        lease.clone(),
        nexc::engine::trace::Tail::GENESIS,
    );
    let ctx = ExecContext {
        state: app.state.clone(),
        run_id,
        graph_id,
        workspace_id: Some(workspace_id),
        owner: owner_id,
        owner_role: Some(nexc::domain::workspace::WorkspaceRole::Owner),
        goal: String::new(),
        node,
        node_type: None,
        code_exec_allowed: false,
        upstream: Vec::new(),
        memories: Vec::new(),
        documents: Vec::new(),
        given: Default::default(),
        agent: Some(agent.clone()),
        capabilities: agent.capabilities.clone(),
        policy_version: 0,
        rules: Rules::default(),
        labels_given: Vec::new(),
        target: LlmTarget {
            provider: LlmProviderKind::Demo,
            model: "demo".into(),
            base_url: None,
            api_key: None,
        },
        force: false,
        trace: sink.handle(),
        attempt: std::sync::atomic::AtomicI32::new(1),
        recording: Default::default(),
        scrub_secrets: true,
        redactions: Vec::new(),
        lease,
        carried: Default::default(),
    };
    let _ = Capabilities::default();
    let granted = Granted::default();
    let mut report = Vec::new();
    for (name, tool, arguments) in [
        (
            "mcp tool over http",
            "mcp:web/echo".to_owned(),
            json!({"text": "measure me"}),
        ),
        ("fetch_url", "fetch_url".to_owned(), json!({"url": site})),
    ] {
        let mut overhead = Vec::with_capacity(CALLS);
        let mut total = Vec::with_capacity(CALLS);
        for i in 0..CALLS {
            let request = ToolRequest {
                request_id: format!("r{i}"),
                tool: tool.clone(),
                arguments: arguments.clone(),
                agent: agent.principal_id.clone(),
                parent: None,
                approval_id: None,
            };
            let started = Instant::now();
            let outcome = gateway::perform(&ctx, &request, &granted).await;
            total.push(started.elapsed().as_secs_f64() * 1e6);
            assert!(!outcome.is_error, "{}", outcome.output);
            overhead.push(outcome.overhead_ns as f64 / 1e3);
        }
        overhead.sort_by(f64::total_cmp);
        total.sort_by(f64::total_cmp);
        report.push((name, overhead, total));
    }
    let sealed = sink.seal().await;
    assert_eq!(sealed.spans, 4 * CALLS as i64, "two spans per call");
    println!(
        "prove-gateway: gateway overhead per call (pace, decisions, DLP screen, tool_call + egress spans; the tool itself excluded), {CALLS} calls each, Apple-silicon release build:"
    );
    for (name, overhead, total) in report {
        println!(
            "prove-gateway: {name:<20} overhead p50 {:.0} µs, p99 {:.0} µs, max {:.0} µs; whole call p50 {:.0} µs, p99 {:.0} µs",
            pct(&overhead, 50.0),
            pct(&overhead, 99.0),
            overhead.last().copied().unwrap_or(0.0),
            pct(&total, 50.0),
            pct(&total, 99.0),
        );
    }
}
