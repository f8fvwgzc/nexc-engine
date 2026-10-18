//! Agent identity and scopes end to end (AgentSecOps phase 1): the runtime
//! receives a signed per-run token it can verify and that carries the
//! agent's capabilities; what it reports is re-checked against that policy
//! and a disagreement is audited and counted on the run; a restricted
//! document never reaches an agent cleared for internal data, and the trace
//! says what was withheld; the model allow-list stops a node before
//! dispatch; quarantine cancels a running node and refuses new ones; the
//! card and the capability editor refuse what does not fit.
#![forbid(unsafe_code)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::post;
use bytes::Bytes;
use chrono::Utc;
use common::{RUNTIME_TOKEN, TestApp, collect_until};
use nexc::domain::capabilities::{Capabilities, DataLabel, DataScopes, Delegation};
use nexc::domain::policy::{Action, decide};
use nexc::security::agent_token::{AgentTokenKeys, derive_key};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::broadcast;
use uuid::Uuid;

const HANDBOOK: &str = "# Warehouse handbook

General rules for everyone on the floor.

## Customs

Invoices from the Rotterdam warehouse need a customs reference before they are released.
The reference is issued by the port authority and starts with NL.

## Plants

Office plants are watered on Mondays.
";

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

fn span(kind: &str, name: &str, agent: &str, attrs: Value) -> Value {
    json!({
        "type": "span", "id": Uuid::now_v7(), "parent_id": null, "kind": kind, "name": name,
        "agent": agent, "started_at": now(), "ended_at": now(), "status": "ok", "attributes": attrs,
    })
}

/// A stand-in for the agent runtime that keeps every request it was sent.
/// The node title picks its behaviour: `OVERSTEP` reports what its policy
/// forbids, `SLOW` keeps the stream open for a while, anything else is a
/// well-behaved run of one turn and one tool call.
async fn identity_runtime() -> (String, Arc<Mutex<Vec<Value>>>) {
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let recorder = seen.clone();
    let execute = move |headers: HeaderMap, axum::Json(req): axum::Json<Value>| {
        let seen = recorder.clone();
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
            let result = json!({"type": "result", "output": format!("[runtime] {title}"),
                "tokens_in": 10, "tokens_out": 5});
            let line = |v: &Value| format!("{v}\n");
            if title.contains("SLOW") {
                let first = line(&json!({"type": "delta", "text": "working"}));
                let last = line(&result);
                let stream = async_stream::stream! {
                    yield Ok::<_, std::io::Error>(Bytes::from(first));
                    tokio::time::sleep(Duration::from_secs(20)).await;
                    yield Ok(Bytes::from(last));
                };
                return (
                    [(header::CONTENT_TYPE, "application/x-ndjson")],
                    Body::from_stream(stream),
                )
                    .into_response();
            }
            let mut lines = vec![turn];
            if title.contains("OVERSTEP") {
                // A tool outside the allow-list, a sub-agent of a role that is no counterpart,
                // a decision the backend would not take, and a span acting as someone else.
                lines.push(span(
                    "tool_call",
                    "make_docx",
                    &agent,
                    json!({"tool": "make_docx"}),
                ));
                lines.push(span(
                    "spawn",
                    &format!("{agent}/editor-1"),
                    &agent,
                    json!({"child": format!("{agent}/editor-1"), "role": "editor", "depth": 1}),
                ));
                lines.push(span("policy", "run_python", &agent, json!({
                    "decision": "allow", "rule": "tools.allow", "action": "tool_call", "subject": "run_python",
                })));
                lines.push(span(
                    "tool_call",
                    "write_file",
                    "agent://elsewhere/Admin",
                    json!({"tool": "write_file"}),
                ));
            } else {
                lines.push(span(
                    "tool_call",
                    "write_file",
                    &agent,
                    json!({"tool": "write_file"}),
                ));
            }
            lines.push(result);
            let body: String = lines.iter().map(line).collect();
            ([(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response()
        }
    };
    let app = Router::new().route("/v1/execute", post(execute));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
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

/// The caller's workspace: its id and the `agent://` prefix of its agents.
async fn workspace(app: &TestApp, token: &str) -> String {
    let (_, list) = call(app, Method::GET, "/workspaces", token, None).await;
    list[0]["id"].as_str().unwrap().to_owned()
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
async fn the_runtime_receives_a_token_that_carries_the_agents_capabilities(pool: PgPool) {
    let (runtime, seen) = identity_runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &runtime)]).await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    let capabilities = json!({
        "tools": ["write_file", "read_file", "finish"],
        "counterparts": ["researcher"],
        "delegation": {"max_depth": 1, "max_fan_out": 2},
    });
    let analyst = agent(&app, &token, "Analyst", "analyst", capabilities).await;
    let principal = format!("agent://{wid}/Analyst");
    assert_eq!(analyst["principal_id"], principal);
    assert_eq!(analyst["policy_version"], 0);
    assert_eq!(
        analyst["capabilities"]["tools"],
        json!(["write_file", "read_file", "finish"])
    );
    assert_eq!(
        analyst["capabilities"]["data"]["labels"],
        json!(["public", "internal"])
    );
    assert_eq!(analyst["capabilities"]["models"], Value::Null);
    assert_eq!(analyst["status"], "active");
    assert_eq!(analyst["quarantined_at"], Value::Null);
    let aid = analyst["id"].as_str().unwrap().to_owned();

    let gid = app.graph(&token, "Analyse the quarter").await;
    let node = app
        .node(
            &token,
            &gid,
            json!({"title": "ECHO analysis", "kind": "research", "executor": "agent", "agent_role": "analyst"}),
        )
        .await;
    let (rid, events) = run(&app, &token, &gid, false).await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");

    // The request carried a token this runtime can verify, minted for this node.
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request["agent"]["principal"], principal);
    let claims = keys()
        .verify(request["token"].as_str().unwrap())
        .expect("the token verifies with the shared key");
    assert_eq!(claims.sub, principal);
    assert_eq!(
        (claims.run_id.to_string(), claims.node_id.to_string()),
        (rid.clone(), node)
    );
    assert_eq!(claims.agent_id.map(|id| id.to_string()), Some(aid.clone()));
    assert_eq!(
        claims.workspace_id.map(|id| id.to_string()),
        Some(wid.clone())
    );
    assert_eq!(claims.policy_version, 0);
    assert_eq!(claims.depth, 0);
    assert_eq!(
        claims.capabilities.tools,
        Some(vec![
            "write_file".into(),
            "read_file".into(),
            "finish".into()
        ])
    );
    assert_eq!(claims.capabilities.delegation.max_depth, 1);
    // The node's timeout plus a minute of slack: no longer.
    assert_eq!(claims.exp - claims.iat, 600 + 60);
    // The same key, derived from the runtime's bearer secret, refuses a token signed with another.
    let other = AgentTokenKeys::new(b"not-the-key-not-the-key-not-the-key!");
    assert!(other.verify(request["token"].as_str().unwrap()).is_none());

    // Every span of the node is attributed to the principal, the backend's own included.
    let spans = spans_of(&app, &token, &rid).await;
    assert!(spans.len() >= 3, "{spans:?}");
    assert!(
        spans.iter().all(|s| s["principal"] == principal),
        "{spans:?}"
    );
    assert!(
        !spans.iter().any(|s| s["kind"] == "policy"),
        "nothing was refused"
    );

    // The card: who it is, who answers for it, what it has left, what it did last.
    let (status, card) = call(&app, Method::GET, &format!("/agents/{aid}"), &token, None).await;
    assert_eq!(status, StatusCode::OK, "{card}");
    assert_eq!(card["agent"]["principal_id"], principal);
    assert_eq!(card["owner"]["name"], "Test");
    assert_eq!(card["team"], Value::Null);
    assert_eq!(card["budget_left"], 1_000_000 - 15);
    let recent = card["recent_spans"].as_array().unwrap();
    assert!(!recent.is_empty() && recent.len() <= 20);
    assert!(recent.iter().all(|s| s["agent_id"] == aid.as_str()));

    // Editing the capabilities raises the policy version and is audited; the same document does not.
    let same = json!({"capabilities": analyst["capabilities"]});
    let (status, edited) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{aid}"),
        &token,
        Some(same),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["policy_version"], 0);
    let narrower = json!({"capabilities": {"tools": ["finish"], "models": ["claude-opus-5"]}});
    let (status, edited) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{aid}"),
        &token,
        Some(narrower),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["policy_version"], 1);
    assert_eq!(edited["capabilities"]["tools"], json!(["finish"]));
    assert_eq!(
        edited["capabilities"]["data"]["labels"],
        json!(["public", "internal"])
    );
    let changed = audit_actions(&app, &token, &wid, "policy_changed").await;
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert_eq!(changed[0]["subject"], principal);
    assert_eq!(changed[0]["detail"], "policy version 1");
    // A rename moves the principal with the name.
    let (_, renamed) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{aid}"),
        &token,
        Some(json!({"name": "Analyst Two"})),
    )
    .await;
    assert_eq!(
        renamed["principal_id"],
        format!("agent://{wid}/Analyst Two")
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn what_the_runtime_reports_is_rechecked_against_the_token(pool: PgPool) {
    let (runtime, _) = identity_runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &runtime)]).await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    let capabilities =
        json!({"tools": ["write_file", "read_file", "finish"], "counterparts": ["researcher"]});
    agent(&app, &token, "Analyst", "analyst", capabilities).await;
    let principal = format!("agent://{wid}/Analyst");
    let gid = app.graph(&token, "Overstep").await;
    app.node(
        &token,
        &gid,
        json!({"title": "OVERSTEP", "kind": "research", "executor": "agent", "agent_role": "analyst"}),
    )
    .await;
    let (rid, events) = run(&app, &token, &gid, false).await;
    // The run still finishes: a mismatch is an incident on the record, not a failure of the work.
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "succeeded");
    assert_eq!(finished["policy_mismatches"], 4, "{finished}");

    let spans = spans_of(&app, &token, &rid).await;
    let mismatches: Vec<&Value> = spans
        .iter()
        .filter(|s| s["kind"] == "policy" && s["name"] == "policy_mismatch")
        .collect();
    let rules: Vec<&str> = mismatches
        .iter()
        .map(|s| s["attributes"]["rule"].as_str().unwrap())
        .collect();
    assert_eq!(
        rules,
        ["tools.allow", "counterparts", "tools.allow", "principal"],
        "{spans:?}"
    );
    assert!(mismatches.iter().all(|s| s["status"] == "error"));
    assert!(
        mismatches
            .iter()
            .all(|s| s["attributes"]["decision"] == "mismatch")
    );
    assert!(
        mismatches
            .iter()
            .all(|s| s["attributes"]["principal"] == principal)
    );
    let reasons: Vec<&str> = mismatches
        .iter()
        .map(|s| s["attributes"]["reason"].as_str().unwrap())
        .collect();
    assert!(reasons[0].contains("ran tool make_docx"), "{reasons:?}");
    assert!(
        reasons[1].contains("spawned a editor") || reasons[1].contains("editor"),
        "{reasons:?}"
    );
    assert!(
        reasons[2].contains("decided `allow` on tool_call run_python"),
        "{reasons:?}"
    );
    assert!(
        reasons[3].contains("agent://elsewhere/Admin"),
        "{reasons:?}"
    );
    // Each mismatch names the span it is about, and that span is on the record under the
    // token's principal (the intruder's name is kept as an attribute).
    for mismatch in &mismatches {
        let about = mismatch["attributes"]["span_id"].as_str().unwrap();
        assert!(
            spans.iter().any(|s| s["id"] == about),
            "{about} is in the trace"
        );
    }
    let intruder = spans
        .iter()
        .find(|s| s["attributes"]["reported_principal"] == "agent://elsewhere/Admin")
        .expect("the span acting as someone else");
    assert_eq!(intruder["principal"], principal);
    assert!(
        spans.iter().all(|s| s["principal"] == principal),
        "{spans:?}"
    );

    // And the workspace's audit log has one entry per disagreement.
    let audited = audit_actions(&app, &token, &wid, "policy_mismatch").await;
    assert_eq!(audited.len(), 4, "{audited:?}");
    assert!(audited.iter().all(|e| e["subject"] == principal));
    assert!(
        audited
            .iter()
            .all(|e| e["detail"].as_str().unwrap().contains(&rid))
    );
    let (_, loaded) = call(&app, Method::GET, &format!("/runs/{rid}"), &token, None).await;
    assert_eq!(loaded["policy_mismatches"], 4);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_restricted_document_never_reaches_an_internal_agent(pool: PgPool) {
    let (runtime, seen) = identity_runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &runtime)]).await;
    tokio::fs::create_dir_all(app.state.settings.documents_dir())
        .await
        .unwrap();
    let (owner, _) = app.register("owner@example.com").await;
    let (member, _) = app.register("member@example.com").await;
    let wid = workspace(&app, &owner).await;
    call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &owner,
        Some(json!({"email": "member@example.com", "role": "member"})),
    )
    .await;
    let r = app
        .send_bytes(
            &format!("/api/v1/workspaces/{wid}/documents?name=handbook.md"),
            &owner,
            HANDBOOK.into(),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let did = r.body["id"].as_str().unwrap().to_owned();
    assert_eq!(r.body["label"], "internal", "the default classification");
    nexc::engine::knowledge::work(&app.state).await.unwrap();

    // Only an admin classifies; it is audited.
    let doc = format!("/workspaces/{wid}/documents/{did}");
    let (status, _) = call(
        &app,
        Method::PATCH,
        &doc,
        &member,
        Some(json!({"label": "restricted"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = call(
        &app,
        Method::PATCH,
        &doc,
        &owner,
        Some(json!({"label": "top secret"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = call(
        &app,
        Method::PATCH,
        &doc,
        &owner,
        Some(json!({"label": "restricted"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["label"].as_str(), body["status"].as_str()),
        (Some("restricted"), Some("ready"))
    );
    let classified = audit_actions(&app, &owner, &wid, "classification_changed").await;
    assert_eq!(classified.len(), 1);
    assert_eq!(classified[0]["detail"], "restricted");

    // An agent cleared for internal data asks about exactly what the document says.
    let clerk = agent(&app, &owner, "Clerk", "clerk", json!({})).await;
    let cid = clerk["id"].as_str().unwrap().to_owned();
    let principal = format!("agent://{wid}/Clerk");
    let gid = app.graph(&owner, "Release the Rotterdam invoices").await;
    let node = app
        .node(
            &owner,
            &gid,
            json!({"title": "ECHO customs", "kind": "research", "executor": "agent", "agent_role": "clerk",
                "content": "Which customs reference do invoices from the Rotterdam warehouse need?"}),
        )
        .await;
    let (rid, events) = run(&app, &owner, &gid, false).await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    let request = seen.lock().unwrap().last().unwrap().clone();
    assert_eq!(
        request["context"]["documents"],
        json!([]),
        "nothing of the document reached the agent"
    );
    let spans = spans_of(&app, &owner, &rid).await;
    let read = spans.iter().find(|s| s["kind"] == "data_read").unwrap();
    assert_eq!(read["attributes"]["document_chunk_ids"], json!([]));
    assert_eq!(read["principal"], principal);
    let withheld = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["name"] == "context")
        .expect("what was withheld is on the record");
    assert_eq!(withheld["status"], "error");
    assert_eq!(withheld["attributes"]["rule"], "data.labels");
    assert_eq!(withheld["attributes"]["decision"], "deny");
    let ids = withheld["attributes"]["withheld_document_chunk_ids"]
        .as_array()
        .unwrap();
    assert!(!ids.is_empty(), "{withheld}");
    assert_eq!(withheld["attributes"]["withheld_by_labels"], ids.len());
    assert_eq!(withheld["attributes"]["withheld_memory_ids"], json!([]));
    // Ids only: not a word of the document is in the trace.
    let trace_text = serde_json::to_string(&spans).unwrap();
    assert!(!trace_text.contains("Rotterdam") && !trace_text.contains("customs reference"));
    assert_eq!(node, read["node_id"]);

    // Cleared for restricted data, the same agent is given the passage.
    let (status, cleared) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{cid}"),
        &owner,
        Some(json!({"capabilities": {"data": {"labels": ["public", "internal", "restricted"]}}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    let (rid, events) = run(&app, &owner, &gid, true).await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    let request = seen.lock().unwrap().last().unwrap().clone();
    let documents = request["context"]["documents"].as_array().unwrap();
    assert!(!documents.is_empty(), "{request}");
    // Headed by its citation id and its source: `[doc:<chunk id>] handbook.md …`.
    let head = documents[0].as_str().unwrap();
    assert!(head.starts_with("[doc:"), "{head}");
    assert!(head.contains("] handbook.md"), "{head}");
    let spans = spans_of(&app, &owner, &rid).await;
    assert!(!spans.iter().any(|s| s["kind"] == "policy"), "{spans:?}");
    let read = spans.iter().find(|s| s["kind"] == "data_read").unwrap();
    let chunk_ids = read["attributes"]["document_chunk_ids"].as_array().unwrap();
    assert_eq!(chunk_ids.len(), documents.len());
    // The ids the model was told to cite are the ids it was given (documents first).
    let cited: Vec<&str> = read["attributes"]["citation_ids"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .filter(|id| id.starts_with("doc:"))
        .collect();
    assert_eq!(cited.len(), chunk_ids.len());
    assert_eq!(cited[0], format!("doc:{}", chunk_ids[0].as_str().unwrap()));
    assert!(head.starts_with(&format!("[{}] ", cited[0])), "{head}");
    // Back to internal, with the agent's clearance back to the default: withheld again.
    call(
        &app,
        Method::PATCH,
        &format!("/agents/{cid}"),
        &owner,
        Some(json!({"capabilities": {}})),
    )
    .await;
    let (_, events) = run(&app, &owner, &gid, true).await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    assert_eq!(
        seen.lock().unwrap().last().unwrap()["context"]["documents"],
        json!([])
    );
    let (_, body) = call(
        &app,
        Method::PATCH,
        &doc,
        &owner,
        Some(json!({"label": "internal"})),
    )
    .await;
    assert_eq!(body["label"], "internal");
    let (_, events) = run(&app, &owner, &gid, true).await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    assert!(
        !seen.lock().unwrap().last().unwrap()["context"]["documents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

/// Waits until `node` reports `status`.
async fn wait_for_status(
    rx: &mut broadcast::Receiver<nexc::realtime::hub::SseFrame>,
    node: &str,
    status: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let frame = tokio::time::timeout_at(deadline.into(), rx.recv())
            .await
            .expect("the node reports its status in time")
            .expect("hub open");
        if &*frame.event != "node.status" {
            continue;
        }
        let data: Value = serde_json::from_str(&frame.data).unwrap();
        if data["node_id"] == node && data["status"] == status {
            return;
        }
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn quarantine_cancels_a_running_node_and_refuses_new_ones(pool: PgPool) {
    let (runtime, seen) = identity_runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &runtime)]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (member, _) = app.register("member@example.com").await;
    let (guest, _) = app.register("guest@example.com").await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let wid = workspace(&app, &owner).await;
    for (email, role) in [
        ("member@example.com", "member"),
        ("guest@example.com", "guest"),
    ] {
        call(
            &app,
            Method::POST,
            &format!("/workspaces/{wid}/members"),
            &owner,
            Some(json!({"email": email, "role": role})),
        )
        .await;
    }
    let slow = agent(&app, &owner, "Slowpoke", "slow", json!({})).await;
    let aid = slow["id"].as_str().unwrap().to_owned();
    let principal = format!("agent://{wid}/Slowpoke");
    let quarantine = format!("/agents/{aid}/quarantine");
    let release = format!("/agents/{aid}/release");

    // Only a workspace admin may; an agent of another workspace is as good as missing.
    for (token, expected) in [
        (&member, StatusCode::FORBIDDEN),
        (&guest, StatusCode::FORBIDDEN),
        (&outsider, StatusCode::NOT_FOUND),
    ] {
        let (status, _) = call(&app, Method::POST, &quarantine, token, None).await;
        assert_eq!(status, expected);
        let (status, _) = call(&app, Method::POST, &release, token, None).await;
        assert_eq!(status, expected);
    }
    let (status, _) = call(&app, Method::POST, &release, &owner, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "not quarantined yet");

    let gid = app.graph(&owner, "Take your time").await;
    let node = app
        .node(
            &owner,
            &gid,
            json!({"title": "SLOW work", "kind": "research", "executor": "agent", "agent_role": "slow"}),
        )
        .await;
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
    wait_for_status(&mut rx, &node, "running").await;
    // Give the runtime stand-in a moment to be mid-stream.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let started = Instant::now();
    let (status, quarantined) = call(&app, Method::POST, &quarantine, &owner, None).await;
    assert_eq!(status, StatusCode::OK, "{quarantined}");
    assert_eq!(quarantined["status"], "quarantined");
    assert!(quarantined["quarantined_at"].is_string());
    assert_eq!(quarantined["policy_version"], 1);
    let events = collect_until(&mut rx, "run.finished").await;
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the node did not wait for the runtime"
    );
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "failed", "{finished}");
    let node_run = &finished["node_runs"][0];
    assert_eq!(node_run["status"], "failed");
    assert!(
        node_run["error"].as_str().unwrap().contains("quarantined"),
        "{node_run}"
    );
    let spans = spans_of(&app, &owner, &rid).await;
    let decision = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["rule"] == "quarantine")
        .expect("the quarantine is on the record");
    assert_eq!(decision["principal"], principal);
    assert_eq!(decision["status"], "error");
    let (status, _) = call(&app, Method::POST, &quarantine, &owner, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "already quarantined");
    let audited = audit_actions(&app, &owner, &wid, "agent_quarantined").await;
    assert_eq!(audited.len(), 1);
    assert_eq!(audited[0]["subject"], principal);
    assert!(
        audited[0]["detail"]
            .as_str()
            .unwrap()
            .contains("1 running node(s) cancelled")
    );

    // While quarantined: new dispatches are refused at once, and the status cannot be
    // changed around the quarantine.
    let before = seen.lock().unwrap().len();
    let (_, events) = run(&app, &owner, &gid, true).await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "failed");
    assert!(
        finished["node_runs"][0]["error"]
            .as_str()
            .unwrap()
            .contains("is quarantined")
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        before,
        "the runtime was not called"
    );
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{aid}"),
        &owner,
        Some(json!({"status": "active"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/agents/{aid}"),
        &owner,
        Some(json!({"status": "quarantined"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, card) = call(&app, Method::GET, &format!("/agents/{aid}"), &member, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card["agent"]["status"], "quarantined");

    // Released: back in service, and the policy version moved on once more.
    let (status, released) = call(&app, Method::POST, &release, &owner, None).await;
    assert_eq!(status, StatusCode::OK, "{released}");
    assert_eq!(
        (
            released["status"].as_str(),
            released["policy_version"].as_i64()
        ),
        (Some("active"), Some(2))
    );
    assert_eq!(released["quarantined_at"], Value::Null);
    assert_eq!(
        audit_actions(&app, &owner, &wid, "agent_released")
            .await
            .len(),
        1
    );
    call(
        &app,
        Method::PATCH,
        &format!("/graphs/{gid}/nodes/{node}"),
        &owner,
        Some(json!({"title": "ECHO work"})),
    )
    .await;
    let (_, events) = run(&app, &owner, &gid, true).await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_model_allow_list_is_enforced_before_dispatch(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (status, scribe) = call(
        &app,
        Method::POST,
        "/agents",
        &owner,
        Some(json!({
            "name": "Scribe", "role": "scribe", "runtime": "builtin", "model": "claude-haiku-5-5",
            "capabilities": {"models": ["claude-opus-5"]},
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{scribe}");
    let gid = app.graph(&owner, "Write it down").await;
    app.node(
        &owner,
        &gid,
        json!({"title": "Notes", "kind": "task", "agent_role": "scribe"}),
    )
    .await;
    let calls_before = app.fake.calls.load(std::sync::atomic::Ordering::SeqCst);
    let (rid, events) = run(&app, &owner, &gid, false).await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "failed", "{finished}");
    let error = finished["node_runs"][0]["error"].as_str().unwrap();
    assert!(
        error.contains("policy: model claude-haiku-5-5 is not allowed"),
        "{error}"
    );
    assert_eq!(
        app.fake.calls.load(std::sync::atomic::Ordering::SeqCst),
        calls_before,
        "no model was called"
    );
    let spans = spans_of(&app, &owner, &rid).await;
    let refused = spans
        .iter()
        .find(|s| s["kind"] == "policy")
        .expect("the refusal is a span");
    assert_eq!(refused["name"], "claude-haiku-5-5");
    assert_eq!(refused["attributes"]["rule"], "models.allow");
    assert_eq!(refused["principal"], format!("agent://{wid}/Scribe"));
    assert!(!spans.iter().any(|s| s["kind"] == "llm_call"));
    // Allowed, the same node runs.
    let aid = scribe["id"].as_str().unwrap();
    call(
        &app,
        Method::PATCH,
        &format!("/agents/{aid}"),
        &owner,
        Some(json!({"capabilities": {"models": ["claude-haiku-5-5"]}})),
    )
    .await;
    let (_, events) = run(&app, &owner, &gid, true).await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn capabilities_that_do_not_fit_are_refused(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (stranger, _) = app.register("stranger@example.com").await;
    let wid = workspace(&app, &owner).await;
    let (_, me) = call(&app, Method::GET, "/auth/me", &stranger, None).await;
    let stranger_id = me["id"].as_str().unwrap();
    let base = json!({"name": "Bad", "role": "bad", "runtime": "python"});
    let with = |extra: Value| {
        let mut body = base.clone();
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };
    let cases = [
        (
            json!({"capabilities": {"tools": (0..65).map(|i| format!("t{i}")).collect::<Vec<_>>()}}),
            "capabilities.tools",
        ),
        (
            json!({"capabilities": {"tools": ["write file"]}}),
            "capabilities.tools",
        ),
        (
            json!({"capabilities": {"egress_hosts": ["Not A Host"]}}),
            "capabilities.egress_hosts",
        ),
        (
            json!({"capabilities": {"counterparts": ["QA Lead"]}}),
            "capabilities.counterparts",
        ),
        (
            json!({"capabilities": {"delegation": {"max_depth": 9}}}),
            "capabilities.delegation.max_depth",
        ),
        (
            json!({"capabilities": {"delegation": {"max_fan_out": 33}}}),
            "capabilities.delegation.max_fan_out",
        ),
        (
            json!({"capabilities": {"data": {"labels": ["secret"]}}}),
            "body",
        ),
        (json!({"capabilities": {"surprise": true}}), "body"),
        (json!({"team_id": Uuid::now_v7()}), "team_id"),
        (json!({"owner_user_id": stranger_id}), "owner_user_id"),
        (json!({"purpose": "p".repeat(2_001)}), "purpose"),
    ];
    for (extra, field) in cases {
        let (status, body) = call(
            &app,
            Method::POST,
            "/agents",
            &owner,
            Some(with(extra.clone())),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{extra}: {body}");
        if field != "body" {
            assert!(body["errors"][field].is_array(), "{extra}: {body}");
        }
    }
    // A team of the workspace and a member as owner are fine; the card shows them.
    let (_, team) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/teams"),
        &owner,
        Some(json!({"name": "Ops", "key": "OPS"})),
    )
    .await;
    let (_, me) = call(&app, Method::GET, "/auth/me", &owner, None).await;
    let good = with(
        json!({"team_id": team["id"], "owner_user_id": me["id"], "purpose": "Keeps the lights on."}),
    );
    let (status, made) = call(&app, Method::POST, "/agents", &owner, Some(good)).await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert_eq!(
        (made["team_id"].as_str(), made["owner_user_id"].as_str()),
        (team["id"].as_str(), me["id"].as_str())
    );
    let (_, card) = call(
        &app,
        Method::GET,
        &format!("/agents/{}", made["id"].as_str().unwrap()),
        &owner,
        None,
    )
    .await;
    assert_eq!(card["team"]["key"], "OPS");
    assert_eq!(card["agent"]["purpose"], "Keeps the lights on.");
    // The whole default organisation came with a principal and the default policy.
    let (_, agents) = call(&app, Method::GET, "/agents", &owner, None).await;
    for agent in agents.as_array().unwrap() {
        assert_eq!(
            agent["principal_id"],
            format!("agent://{wid}/{}", agent["name"].as_str().unwrap())
        );
        assert_eq!(agent["capabilities"]["tools"], Value::Null);
        assert!(agent["purpose"].as_str().is_some_and(|p| !p.is_empty()));
    }
}

/// `make prove-identity`: the latency of in-process decisions, the budget
/// of every enforcement point (target p99 < 100 µs).
#[test]
#[ignore = "a measurement: run with `make prove-identity`"]
fn prove_identity() {
    let caps = Capabilities {
        tools: Some(
            [
                "write_file",
                "read_file",
                "list_files",
                "make_docx",
                "finish",
            ]
            .map(String::from)
            .to_vec(),
        ),
        data: DataScopes {
            topics: Some((0..8).map(|_| Uuid::now_v7()).collect()),
            labels: vec![DataLabel::Public, DataLabel::Internal],
            memory_namespaces: None,
        },
        egress_hosts: Some(vec!["api.example.com".into(), "*.internal.example".into()]),
        counterparts: Some(vec!["researcher".into(), "editor".into()]),
        delegation: Delegation {
            max_depth: 2,
            max_fan_out: 4,
        },
        models: Some(vec!["claude-opus-5".into()]),
    };
    let topic = caps.data.topics.as_ref().unwrap()[3];
    let actions = [
        Action::Tool("make_docx"),
        Action::Tool("run_python"),
        Action::Spawn {
            role: "researcher",
            depth: 1,
            children: 2,
        },
        Action::Model("claude-opus-5"),
        Action::Egress("db.internal.example"),
        Action::Read {
            label: DataLabel::Internal,
            topic: Some(topic),
            namespace: None,
        },
        Action::Read {
            label: DataLabel::Restricted,
            topic: None,
            namespace: None,
        },
    ];
    const DECISIONS: usize = 100_000;
    let mut samples = Vec::with_capacity(DECISIONS);
    let mut denied = 0;
    for i in 0..DECISIONS {
        let action = &actions[i % actions.len()];
        let start = Instant::now();
        let decision = decide(&caps, action);
        samples.push(start.elapsed());
        denied += usize::from(!decision.allowed);
    }
    samples.sort();
    let at = |q: f64| samples[((samples.len() as f64 * q) as usize).min(samples.len() - 1)];
    println!(
        "prove-identity (backend, Rust): {DECISIONS} decisions ({denied} denied): p50 {:.2} µs, p99 {:.2} µs, max {:.2} µs",
        at(0.5).as_secs_f64() * 1e6,
        at(0.99).as_secs_f64() * 1e6,
        samples.last().unwrap().as_secs_f64() * 1e6
    );
    let budget: f64 = std::env::var("NEXC_PROVE_P99_US")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100.0);
    assert!(
        at(0.99).as_secs_f64() * 1e6 < budget,
        "p99 is over the {budget} µs target"
    );
}
