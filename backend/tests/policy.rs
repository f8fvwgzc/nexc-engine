//! The policy engine and the Defend function end to end (AgentSecOps phase
//! 2): policies are versioned per level and only narrow downwards; the
//! resolved policy is compiled into the per-run token and its versions are
//! recorded on the run, the node run and every policy span; the backend gates
//! dispatch on working hours, the day's spend and the pace; a policy that
//! requires an approval pauses a run until an admin decides from the inbox.
#![forbid(unsafe_code)]

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::post;
use base64::Engine as _;
use bytes::Bytes;
use chrono::Utc;
use common::{RUNTIME_TOKEN, TestApp, collect_until};
use nexc::security::agent_token::{AgentTokenKeys, derive_key};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

fn span(kind: &str, name: &str, agent: &str, attrs: Value) -> Value {
    json!({
        "type": "span", "id": Uuid::now_v7(), "parent_id": null, "kind": kind, "name": name,
        "agent": agent, "started_at": now(), "ended_at": now(), "status": "ok", "attributes": attrs,
    })
}

/// Decisions the stand-in runtime received: approval id -> the body of `POST /v1/approvals/{id}`.
type Decisions = Arc<Mutex<HashMap<String, Value>>>;

/// A stand-in for the agent runtime that keeps every request it was sent and
/// answers one turn and one `write_file` call, then a result. A node whose
/// title says `APPROVAL` first asks for an approval of `make_docx` and waits
/// for the backend to deliver the decision (`POST /v1/approvals/{id}`), then
/// runs the tool or reports the refusal, as the real runtime does.
async fn policy_runtime() -> (String, Arc<Mutex<Vec<Value>>>, Decisions) {
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let decisions: Decisions = Arc::default();
    let waiting: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<Value>>>> = Arc::default();
    let recorder = seen.clone();
    let waiters = waiting.clone();
    let execute = move |headers: HeaderMap, axum::Json(req): axum::Json<Value>| {
        let seen = recorder.clone();
        let waiting = waiters.clone();
        async move {
            let authorized = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                == Some(&format!("Bearer {RUNTIME_TOKEN}"));
            if !authorized {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            seen.lock().unwrap().push(req.clone());
            let title = req["task"]["title"].as_str().unwrap_or("node").to_owned();
            let agent = req["agent"]["principal"]
                .as_str()
                .unwrap_or("agent")
                .to_owned();
            let turn = span(
                "llm_call",
                "claude-test",
                &agent,
                json!({"provider": "anthropic", "model": "claude-test", "turn": 1}),
            );
            let line = |v: &Value| format!("{v}\n");
            let result = json!({"type": "result", "output": format!("[runtime] {title}"),
                "tokens_in": 10, "tokens_out": 5});
            if title.contains("APPROVAL") {
                let approval_id = Uuid::now_v7().to_string();
                let (tx, rx) = tokio::sync::oneshot::channel();
                waiting.lock().unwrap().insert(approval_id.clone(), tx);
                let request = line(&json!({
                    "type": "span", "id": Uuid::now_v7(), "parent_id": turn["id"], "kind": "policy",
                    "name": "make_docx", "agent": agent, "started_at": now(), "ended_at": now(),
                    "status": "ok",
                    "attributes": {"decision": "require_approval", "rule": "approvals.tools",
                        "reason": "tool make_docx needs an approval", "action": "tool_call",
                        "subject": "make_docx", "approval_id": approval_id,
                        "args_sha256": "ab".repeat(32), "policy_version": 0},
                }));
                let first = line(&turn) + &request;
                let stream = async_stream::stream! {
                    yield Ok::<_, std::io::Error>(Bytes::from(first));
                    let decision = tokio::time::timeout(Duration::from_secs(20), rx).await;
                    let mut lines = String::new();
                    match decision {
                        Ok(Ok(body)) if body["decision"] == "approved" => {
                            lines += &line(&span("tool_call", "make_docx", &agent,
                                json!({"tool": "make_docx", "approval_id": approval_id})));
                            lines += &line(&json!({"type": "artifact", "path": "report.docx",
                                "mime": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                                "content_b64": base64::engine::general_purpose::STANDARD.encode(b"PK demo")}));
                            lines += &line(&result);
                        }
                        Ok(Ok(body)) => {
                            lines += &line(&json!({"type": "log", "level": "warning",
                                "message": format!("make_docx denied: {}", body["reason"])}));
                            lines += &line(&result);
                        }
                        _ => {
                            lines += &line(&json!({"type": "error", "retryable": false,
                                "message": "approval for make_docx not given within 20s"}));
                        }
                    }
                    yield Ok(Bytes::from(lines));
                };
                return (
                    [(header::CONTENT_TYPE, "application/x-ndjson")],
                    Body::from_stream(stream),
                )
                    .into_response();
            }
            let lines = [
                turn,
                span(
                    "tool_call",
                    "write_file",
                    &agent,
                    json!({"tool": "write_file"}),
                ),
                json!({"type": "tokens", "input": 10, "output": 5}),
                result,
            ];
            let body: String = lines.iter().map(line).collect();
            ([(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response()
        }
    };
    let delivered = decisions.clone();
    let decide = move |axum::extract::Path(id): axum::extract::Path<String>,
                       headers: HeaderMap,
                       axum::Json(body): axum::Json<Value>| {
        let (waiting, delivered) = (waiting.clone(), delivered.clone());
        async move {
            let authorized = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                == Some(&format!("Bearer {RUNTIME_TOKEN}"));
            if !authorized {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            delivered.lock().unwrap().insert(id.clone(), body.clone());
            match waiting.lock().unwrap().remove(&id) {
                Some(tx) => {
                    let _ = tx.send(body);
                    axum::Json(json!({"status": "delivered"})).into_response()
                }
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
    };
    let app = Router::new()
        .route("/v1/execute", post(execute))
        .route("/v1/approvals/{id}", post(decide));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen, decisions)
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

async fn add_member(app: &TestApp, owner: &str, wid: &str, email: &str, role: &str) {
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        owner,
        Some(json!({"email": email, "role": role})),
    )
    .await;
    assert!(status.is_success(), "{body}");
}

/// Adds a python-runtime agent of `role` with `capabilities` and returns it.
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

/// Adds a policy version at a level and returns the response.
async fn policy(
    app: &TestApp,
    token: &str,
    wid: &str,
    scope: &str,
    scope_id: Option<&str>,
    document: Value,
) -> (StatusCode, Value) {
    let mut body = json!({"scope": scope, "document": document});
    if let Some(id) = scope_id {
        body["scope_id"] = json!(id);
    }
    call(
        app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        token,
        Some(body),
    )
    .await
}

/// Runs the graph and returns the run id and the events up to `run.finished`.
async fn run(app: &TestApp, token: &str, gid: &str, force: bool) -> (String, Vec<(String, Value)>) {
    let mut rx = app.events(gid);
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        token,
        Some(json!({"force": force})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let rid = body["id"].as_str().unwrap().to_owned();
    let events = collect_until(&mut rx, "run.finished").await;
    (rid, events)
}

async fn spans_of(app: &TestApp, token: &str, rid: &str) -> Vec<Value> {
    let (status, page) = call(
        app,
        Method::GET,
        &format!("/runs/{rid}/trace?limit=1000"),
        token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    page["spans"].as_array().unwrap().clone()
}

async fn audit_actions(app: &TestApp, token: &str, wid: &str, action: &str) -> Vec<Value> {
    let (_, entries) = call(
        app,
        Method::GET,
        &format!("/workspaces/{wid}/audit?limit=200"),
        token,
        None,
    )
    .await;
    entries
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["action"] == action)
        .cloned()
        .collect()
}

fn keys() -> AgentTokenKeys {
    AgentTokenKeys::new(&derive_key(RUNTIME_TOKEN))
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn policies_are_versioned_only_narrow_and_govern_the_run(pool: PgPool) {
    let (runtime, seen, _) = policy_runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &runtime)]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (member, _) = app.register("member@example.com").await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let wid = workspace(&app, &owner).await;
    add_member(&app, &owner, &wid, "member@example.com", "member").await;
    let analyst = agent(
        &app,
        &owner,
        "Analyst",
        "analyst",
        json!({"tools": ["write_file", "read_file", "make_docx", "finish", "spawn_subagent"]}),
    )
    .await;
    let aid = analyst["id"].as_str().unwrap().to_owned();
    let principal = format!("agent://{wid}/Analyst");
    let policies = format!("/workspaces/{wid}/policies");

    // Admins only, on both sides; an outsider finds nothing.
    let (status, _) = call(&app, Method::GET, &policies, &member, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = policy(&app, &member, &wid, "workspace", None, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::GET, &policies, &outsider, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = call(&app, Method::GET, &policies, &owner, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    // A document that does not fit is refused field by field.
    let (status, body) = policy(
        &app,
        &owner,
        &wid,
        "workspace",
        None,
        json!({"tools": {"allow": ["write file"]}, "limits": {"tokens_per_run": 0}, "surprise": 1}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = policy(
        &app,
        &owner,
        &wid,
        "workspace",
        None,
        json!({"tools": {"allow": ["write file"]}, "limits": {"tokens_per_run": 0}}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["document.tools.allow"].is_array(), "{body}");
    assert!(body["errors"]["document.limits.tokens_per_run"].is_array());
    let (status, body) = policy(&app, &owner, &wid, "workspace", Some(&aid), json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["scope_id"].is_array());
    let (status, body) = policy(&app, &owner, &wid, "team", None, json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = policy(
        &app,
        &owner,
        &wid,
        "team",
        Some(&Uuid::now_v7().to_string()),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["errors"]["scope_id"].is_array());

    // The workspace level, twice: versions count up and each change is audited with its diff.
    let (status, v1) = policy(
        &app,
        &owner,
        &wid,
        "workspace",
        None,
        json!({
            "tools": {"allow": ["write_file", "read_file", "make_docx", "finish"]},
            "limits": {"tokens_per_run": 50000},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{v1}");
    assert_eq!(
        (v1["scope"].as_str(), v1["version"].as_i64()),
        (Some("workspace"), Some(1))
    );
    assert_eq!(v1["scope_id"], Value::Null);
    assert_eq!(v1["author_name"], "Test");
    let (status, v2) = policy(
        &app,
        &owner,
        &wid,
        "workspace",
        None,
        json!({
            "tools": {"allow": ["write_file", "read_file", "make_docx", "finish"], "deny": ["run_python"]},
            "limits": {"tokens_per_run": 50000},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{v2}");
    assert_eq!(v2["version"], 2);
    let changed = audit_actions(&app, &owner, &wid, "policy_changed").await;
    assert_eq!(changed.len(), 2, "{changed:?}");
    assert_eq!(changed[0]["subject"], "the workspace");
    let detail = changed[0]["detail"].as_str().unwrap();
    assert!(
        detail.starts_with("workspace policy version 2: "),
        "{detail}"
    );
    assert!(
        detail.contains("tools.deny: [] -> [\"run_python\"]"),
        "{detail}"
    );
    assert!(
        !detail.contains("tokens_per_run"),
        "unchanged rules are not in the diff: {detail}"
    );

    // An agent-level policy may not widen the workspace's; narrowing is welcome.
    let (status, body) = policy(
        &app,
        &owner,
        &wid,
        "agent",
        Some(&aid),
        json!({"tools": {"allow": ["write_file", "spawn_subagent"]}, "limits": {"tokens_per_run": 60000}}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["errors"]["document.tools.allow"][0]
            .as_str()
            .unwrap()
            .contains("would widen"),
        "{body}"
    );
    assert!(body["errors"]["document.limits.tokens_per_run"].is_array());
    let (status, agent_v1) = policy(
        &app,
        &owner,
        &wid,
        "agent",
        Some(&aid),
        json!({"limits": {"tokens_per_run": 20000}, "require_approval": {"tools": ["make_docx"]}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent_v1}");
    assert_eq!(agent_v1["version"], 1);
    assert_eq!(agent_v1["scope_id"], aid);
    let changed = audit_actions(&app, &owner, &wid, "policy_changed").await;
    assert_eq!(changed[0]["subject"], principal);

    // The history, whole and per level.
    let (_, all) = call(&app, Method::GET, &policies, &owner, None).await;
    assert_eq!(all.as_array().unwrap().len(), 3);
    let (_, agent_only) = call(
        &app,
        Method::GET,
        &format!("{policies}?scope=agent&scope_id={aid}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(agent_only.as_array().unwrap().len(), 1);
    let (_, workspace_only) = call(
        &app,
        Method::GET,
        &format!("{policies}?scope=workspace"),
        &owner,
        None,
    )
    .await;
    assert_eq!(workspace_only.as_array().unwrap().len(), 2);
    assert_eq!(workspace_only[0]["version"], 2, "newest first");

    // The resolve view: the effective policy and which level set each rule.
    let (status, resolved) = call(
        &app,
        Method::GET,
        &format!("{policies}/resolve?agent_id={aid}"),
        &member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resolved}");
    assert_eq!(
        resolved["capabilities"]["tools"],
        json!(["write_file", "read_file", "make_docx", "finish"]),
        "the agent's own spawn_subagent is gone: the workspace never allowed it"
    );
    assert_eq!(resolved["rules"]["tools_denied"], json!(["run_python"]));
    assert_eq!(resolved["rules"]["limits"]["tokens_per_run"], 20000);
    assert_eq!(
        resolved["rules"]["require_approval"]["tools"],
        json!(["make_docx"])
    );
    assert_eq!(resolved["rules"]["require_approval"]["timeout_s"], 600);
    assert_eq!(
        resolved["rules"]["versions"],
        json!({"workspace": 2, "team": null, "agent": 1})
    );
    assert_eq!(resolved["origins"]["tools.allow"], "workspace");
    assert_eq!(resolved["origins"]["tools.deny"], "workspace");
    assert_eq!(resolved["origins"]["limits.tokens_per_run"], "agent");
    assert_eq!(resolved["origins"]["require_approval.tools"], "agent");
    assert_eq!(resolved["levels"].as_array().unwrap().len(), 2);
    assert_eq!(resolved["levels"][1]["scope"], "agent");
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{policies}/resolve?agent_id={}", Uuid::now_v7()),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A run: the token carries the compiled policy, the versions are on the run, the node run
    // and the spans, and the ceiling per run caps the budget sent.
    let gid = app.graph(&owner, "Analyse the quarter").await;
    let node = app
        .node(
            &owner,
            &gid,
            json!({"title": "Analysis", "kind": "research", "executor": "agent", "agent_role": "analyst"}),
        )
        .await;
    let (rid, events) = run(&app, &owner, &gid, false).await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(
        finished["policy_versions"],
        json!({"workspace": 2, "team": null, "agent": null})
    );
    let node_run = &finished["node_runs"][0];
    assert_eq!(
        node_run["policy_versions"],
        json!({"workspace": 2, "team": null, "agent": 1})
    );
    assert_eq!(node_run["context_labels"], json!([]));
    let request = seen.lock().unwrap().last().unwrap().clone();
    let claims = keys().verify(request["token"].as_str().unwrap()).unwrap();
    assert_eq!(
        claims.capabilities.tools,
        Some(vec![
            "write_file".into(),
            "read_file".into(),
            "make_docx".into(),
            "finish".into()
        ])
    );
    assert_eq!(claims.rules.tools_denied, vec!["run_python"]);
    assert_eq!(claims.rules.limits.tokens_per_run, Some(20000));
    assert_eq!(claims.rules.require_approval.tools, vec!["make_docx"]);
    assert_eq!(
        (claims.rules.versions.workspace, claims.rules.versions.agent),
        (Some(2), Some(1))
    );
    assert_eq!(claims.node_id.to_string(), node);
    assert_eq!(
        request["agent"]["budget_tokens"], 20000,
        "the ceiling per run caps the budget"
    );
    assert_eq!(request["context"]["labels"], json!([]));
    let spans = spans_of(&app, &owner, &rid).await;
    let read = spans.iter().find(|s| s["kind"] == "data_read").unwrap();
    assert_eq!(read["attributes"]["policy_versions"]["workspace"], 2);
    assert_eq!(read["attributes"]["policy_versions"]["agent"], 1);
    let (_, loaded) = call(&app, Method::GET, &format!("/runs/{rid}"), &owner, None).await;
    assert_eq!(loaded["policy_versions"]["workspace"], 2);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_backend_gates_dispatch_on_hours_spend_and_pace(pool: PgPool) {
    let (runtime, seen, _) = policy_runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &runtime)]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let clerk = agent(&app, &owner, "Clerk", "clerk", json!({})).await;
    let aid = clerk["id"].as_str().unwrap().to_owned();
    let principal = format!("agent://{wid}/Clerk");
    let gid = app.graph(&owner, "Keep the books").await;
    app.node(
        &owner,
        &gid,
        json!({"title": "Books", "kind": "research", "executor": "agent", "agent_role": "clerk"}),
    )
    .await;

    // No window at all: nothing may start.
    let (status, _) = policy(
        &app,
        &owner,
        &wid,
        "agent",
        Some(&aid),
        json!({"time_windows": []}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (rid, events) = run(&app, &owner, &gid, false).await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "failed", "{finished}");
    let error = finished["node_runs"][0]["error"].as_str().unwrap();
    assert!(
        error.contains("outside the agent's working hours"),
        "{error}"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "the runtime was not called"
    );
    let spans = spans_of(&app, &owner, &rid).await;
    let refused = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "time_windows")
        .expect("the refusal is a span");
    assert_eq!(refused["principal"], principal);
    assert_eq!(refused["attributes"]["decision"], "deny");
    assert_eq!(refused["attributes"]["policy_versions"]["agent"], 1);

    // Round the clock, one start per hour and a token a day: the first run goes, the next is paced.
    let (status, body) = policy(
        &app,
        &owner,
        &wid,
        "agent",
        Some(&aid),
        json!({
            "time_windows": [
                {"days": [1, 2, 3, 4, 5, 6, 7], "start": "00:00", "end": "12:00"},
                {"days": [1, 2, 3, 4, 5, 6, 7], "start": "12:00", "end": "00:00"},
            ],
            "limits": {"runs_per_hour": 1, "tokens_per_day": 12},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["version"], 2);
    let (_, events) = run(&app, &owner, &gid, true).await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    assert_eq!(seen.lock().unwrap().len(), 1);
    let (rid, events) = run(&app, &owner, &gid, true).await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "failed", "{finished}");
    let error = finished["node_runs"][0]["error"].as_str().unwrap();
    // The 15 tokens of the first run are over the day's 12 before the pace is even asked.
    assert!(error.contains("tokens for today"), "{error}");
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "the runtime was not called again"
    );
    let spans = spans_of(&app, &owner, &rid).await;
    assert!(
        spans
            .iter()
            .any(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "limits.tokens_per_day"),
        "{spans:?}"
    );
    // Lift the day's ceiling: the pace is what stops the third run.
    let (status, _) = policy(
        &app,
        &owner,
        &wid,
        "agent",
        Some(&aid),
        json!({
            "time_windows": [
                {"days": [1, 2, 3, 4, 5, 6, 7], "start": "00:00", "end": "12:00"},
                {"days": [1, 2, 3, 4, 5, 6, 7], "start": "12:00", "end": "00:00"},
            ],
            "limits": {"runs_per_hour": 1},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, events) = run(&app, &owner, &gid, true).await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "failed", "{finished}");
    let error = finished["node_runs"][0]["error"].as_str().unwrap();
    assert!(error.contains("this hour"), "{error}");
    assert_eq!(seen.lock().unwrap().len(), 1);
    let _ = (Duration::ZERO, Instant::now());
}

/// Waits until `node` reports `status` on the graph's stream.
async fn wait_for_status(
    rx: &mut tokio::sync::broadcast::Receiver<nexc::realtime::hub::SseFrame>,
    node: &str,
    status: &str,
) -> Vec<(String, Value)> {
    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let frame = tokio::time::timeout_at(deadline.into(), rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {status}; got {seen:?}"))
            .expect("hub open");
        let data: Value = serde_json::from_str(&frame.data).unwrap();
        let done =
            &*frame.event == "node.status" && data["node_id"] == node && data["status"] == status;
        seen.push((frame.event.to_string(), data));
        if done {
            return seen;
        }
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_policy_that_requires_an_approval_pauses_the_run_until_an_admin_decides(pool: PgPool) {
    let (runtime, seen, decisions) = policy_runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &runtime)]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (member, _) = app.register("member@example.com").await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let wid = workspace(&app, &owner).await;
    add_member(&app, &owner, &wid, "member@example.com", "member").await;
    let (_, me) = call(&app, Method::GET, "/auth/me", &owner, None).await;
    let owner_id = me["id"].as_str().unwrap().to_owned();
    let writer = agent(&app, &owner, "Scribe", "scribe", json!({})).await;
    let aid = writer["id"].as_str().unwrap().to_owned();
    let principal = format!("agent://{wid}/Scribe");
    let (status, body) = policy(
        &app,
        &owner,
        &wid,
        "workspace",
        None,
        json!({"require_approval": {"tools": ["make_docx"], "timeout_s": 60}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let gid = app.graph(&owner, "Write the report").await;
    let node = app
        .node(
            &owner,
            &gid,
            json!({"title": "APPROVAL report", "kind": "document", "executor": "agent", "agent_role": "scribe"}),
        )
        .await;

    // The run pauses: the node waits, the run goes on, the admin's inbox has the request.
    let mut rx = app.events(&gid);
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let rid = body["id"].as_str().unwrap().to_owned();
    wait_for_status(&mut rx, &node, "waiting_approval").await;
    let (_, run_now) = call(&app, Method::GET, &format!("/runs/{rid}"), &owner, None).await;
    assert_eq!(run_now["status"], "running", "{run_now}");
    assert_eq!(run_now["node_runs"][0]["status"], "waiting_approval");
    let (_, inbox) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/inbox"),
        &owner,
        None,
    )
    .await;
    let entries = inbox.as_array().unwrap();
    assert_eq!(entries.len(), 1, "{inbox}");
    let entry = &entries[0];
    assert_eq!(entry["kind"], "approval");
    assert_eq!(entry["issue"], Value::Null);
    assert_eq!(entry["read_at"], Value::Null);
    let request = &entry["approval"];
    assert_eq!(request["tool"], "make_docx");
    assert_eq!(request["principal"], principal);
    assert_eq!(request["status"], "pending");
    assert_eq!(request["run_id"], rid);
    assert_eq!(request["node_id"], node);
    let approval_id = request["id"].as_str().unwrap().to_owned();
    // The member was not asked (no approvers named: every admin was).
    let (_, members_inbox) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/inbox"),
        &member,
        None,
    )
    .await;
    assert_eq!(members_inbox, json!([]));

    // Only an admin (or a named approver) decides; denying needs a reason; outsiders see nothing.
    let approve = format!("/approvals/{approval_id}/approve");
    let deny = format!("/approvals/{approval_id}/deny");
    let (status, _) = call(&app, Method::POST, &approve, &member, Some(json!({}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::POST, &approve, &outsider, Some(json!({}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/approvals/{approval_id}"),
        &outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = call(
        &app,
        Method::POST,
        &deny,
        &owner,
        Some(json!({"reason": "  "})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, shown) = call(
        &app,
        Method::GET,
        &format!("/approvals/{approval_id}"),
        &member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "members may look: {shown}");
    assert_eq!(shown["status"], "pending");
    assert_eq!(shown["rule"], "approvals.tools");
    assert_eq!(shown["policy_versions"]["workspace"], 1);
    let (_, pending) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/approvals?status=pending"),
        &owner,
        None,
    )
    .await;
    assert_eq!(pending.as_array().unwrap().len(), 1);

    // Approved: the decision reaches the runtime, the node resumes and the run finishes.
    let (status, decided) = call(
        &app,
        Method::POST,
        &approve,
        &owner,
        Some(json!({"reason": "the board asked for it"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    assert_eq!(decided["status"], "approved");
    assert_eq!(decided["decided_by"]["id"], owner_id);
    assert_eq!(decided["decided_by"]["name"], "Test");
    assert_eq!(decided["decision_reason"], "the board asked for it");
    let (status, _) = call(&app, Method::POST, &approve, &owner, Some(json!({}))).await;
    assert_eq!(status, StatusCode::CONFLICT, "decided once");
    let events = collect_until(&mut rx, "run.finished").await;
    assert!(
        events
            .iter()
            .any(|(e, d)| e == "node.status" && d["status"] == "running" && d["node_id"] == node),
        "the node resumed: {events:?}"
    );
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(finished["node_runs"][0]["status"], "succeeded");
    let delivered = decisions
        .lock()
        .unwrap()
        .get(&approval_id)
        .cloned()
        .unwrap();
    assert_eq!(delivered["decision"], "approved");
    assert_eq!(delivered["reason"], "the board asked for it");
    assert_eq!(delivered["decided_by"], "Test");
    // On the record: the request, the decision with who decided, the tool call that ran with it.
    let spans = spans_of(&app, &owner, &rid).await;
    let request_span = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["decision"] == "require_approval")
        .expect("the request");
    let decision_span = spans
        .iter()
        .find(|s| s["kind"] == "approval")
        .expect("the decision is an approval span");
    assert_eq!(decision_span["name"], "make_docx");
    assert_eq!(decision_span["status"], "ok");
    assert_eq!(decision_span["principal"], principal);
    assert_eq!(decision_span["parent_id"], request_span["parent_id"]);
    let attrs = &decision_span["attributes"];
    assert_eq!(attrs["decision"], "approved");
    assert_eq!(attrs["approval_id"], approval_id);
    assert_eq!(attrs["decided_by"], owner_id);
    assert_eq!(attrs["decided_by_name"], "Test");
    assert_eq!(attrs["reason"], "the board asked for it");
    assert_eq!(attrs["request_span_id"], request_span["id"]);
    assert!(attrs["waited_ms"].as_i64().unwrap() >= 0);
    let ran = spans
        .iter()
        .find(|s| s["kind"] == "tool_call" && s["name"] == "make_docx")
        .expect("the tool ran");
    assert_eq!(ran["attributes"]["approval_id"], approval_id);
    assert!(
        !spans.iter().any(|s| s["name"] == "policy_mismatch"),
        "an approved call is no mismatch: {spans:?}"
    );
    let (_, after) = call(
        &app,
        Method::GET,
        &format!("/approvals/{approval_id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(after["decision_span_id"], decision_span["id"]);
    let granted = audit_actions(&app, &owner, &wid, "approval_granted").await;
    assert_eq!(granted.len(), 1);
    assert_eq!(granted[0]["subject"], principal);
    assert!(
        granted[0]["detail"]
            .as_str()
            .unwrap()
            .contains("make_docx in run"),
        "{granted:?}"
    );
    let (_, inbox) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/inbox"),
        &owner,
        None,
    )
    .await;
    assert!(inbox[0]["read_at"].is_string(), "settled: {inbox}");
    assert_eq!(inbox[0]["approval"]["status"], "approved");
    assert_eq!(
        finished["node_runs"][0]["status"],
        "succeeded",
        "{}",
        seen.lock().unwrap().len()
    );

    // Denied: the model sees the refusal, the run still finishes; it is audited as denied.
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
    wait_for_status(&mut rx, &node, "waiting_approval").await;
    let (_, pending) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/approvals?status=pending"),
        &owner,
        None,
    )
    .await;
    let approval_id = pending[0]["id"].as_str().unwrap().to_owned();
    let (status, denied) = call(
        &app,
        Method::POST,
        &format!("/approvals/{approval_id}/deny"),
        &owner,
        Some(json!({"reason": "not without legal"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{denied}");
    assert_eq!(denied["status"], "denied");
    let events = collect_until(&mut rx, "run.finished").await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "succeeded", "{finished}");
    let spans = spans_of(&app, &owner, &rid).await;
    let decision_span = spans.iter().find(|s| s["kind"] == "approval").unwrap();
    assert_eq!(decision_span["status"], "error");
    assert_eq!(decision_span["attributes"]["decision"], "denied");
    assert_eq!(decision_span["attributes"]["reason"], "not without legal");
    assert!(
        !spans
            .iter()
            .any(|s| s["kind"] == "tool_call" && s["name"] == "make_docx"),
        "the tool never ran"
    );
    assert_eq!(
        audit_actions(&app, &owner, &wid, "approval_denied")
            .await
            .len(),
        1
    );
    let _ = aid;
}
