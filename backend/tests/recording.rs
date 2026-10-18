//! Opt-in recording of prompts, completions and tool calls: off by default
//! (payloads a runtime sends anyway are dropped), on when the guardrails say
//! so (the token tells the runtime, the data_read span notes it, the
//! payloads are kept sealed and read back by the run's owner and admins
//! with an audit entry), restricted passages replaced by their citation and
//! credentials scrubbed before anything is written, and rows past the
//! retention deleted.
#![forbid(unsafe_code)]

mod common;

use axum::Router;
use axum::http::{Method, StatusCode};
use axum::routing::get;
use common::grpc_runtime::fake_grpc_runtime;
use common::{TestApp, collect_until};
use serde_json::{Value, json};
use sqlx::PgPool;

const HANDBOOK: &str = "# Warehouse handbook

## Customs

Invoices from the Rotterdam warehouse need a customs reference before they are released.
The reference is the RT-7731 code printed on the manifest, and the customs desk keeps a copy.

## Parking

Bicycle racks are behind the loading bay.
";

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

async fn agent(app: &TestApp, token: &str, name: &str, role: &str, capabilities: Value) {
    let (status, agent) = call(
        app,
        Method::POST,
        "/agents",
        token,
        Some(json!({
            "name": name, "role": role, "runtime": "python",
            "purpose": format!("Handles {role} work in tests."), "capabilities": capabilities,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
}

async fn grpc_app(pool: PgPool, url: &str) -> TestApp {
    TestApp::new(
        pool,
        &[
            ("NEXC_RUNTIME_PROTOCOL", "grpc"),
            ("NEXC_RUNTIME_GRPC_URL", url),
        ],
    )
    .await
}

/// Sets the workspace's recording; the guardrails as answered.
async fn set_recording(app: &TestApp, token: &str, wid: &str, recording: Value) -> Value {
    let (_, current) = call(
        app,
        Method::GET,
        &format!("/workspaces/{wid}/guardrails"),
        token,
        None,
    )
    .await;
    let mut next = current.clone();
    next["recording"] = recording;
    let (status, saved) = call(
        app,
        Method::PUT,
        &format!("/workspaces/{wid}/guardrails"),
        token,
        Some(next),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    saved
}

/// Runs a one-node graph to its end; `(graph, node, run, events)`.
async fn finished_run(
    app: &TestApp,
    token: &str,
    node: Value,
) -> (String, String, String, Vec<(String, Value)>) {
    let gid = app.graph(token, "Record this").await;
    let node = app.node(token, &gid, node).await;
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
    (gid, node, rid, events)
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

fn span_of<'a>(spans: &'a [Value], kind: &str, name: &str) -> &'a Value {
    spans
        .iter()
        .find(|s| s["kind"] == kind && s["name"] == name)
        .unwrap_or_else(|| panic!("no {kind} span named {name}"))
}

async fn payload(app: &TestApp, token: &str, span_id: &str) -> (StatusCode, Value) {
    call(
        app,
        Method::GET,
        &format!("/spans/{span_id}/payload"),
        token,
        None,
    )
    .await
}

fn kinds(payloads: &Value) -> Vec<String> {
    payloads["payloads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["kind"].as_str().unwrap().to_owned())
        .collect()
}

async fn audit_actions(app: &TestApp, token: &str, wid: &str) -> Vec<(String, String, String)> {
    let (_, audit) = call(
        app,
        Method::GET,
        &format!("/workspaces/{wid}/audit"),
        token,
        None,
    )
    .await;
    audit
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["action"].as_str().unwrap().to_owned(),
                e["subject"].as_str().unwrap().to_owned(),
                e["detail"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn recording_is_off_by_default_and_keeps_what_the_setting_names(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool.clone(), &url).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    agent(&app, &owner, "Scribe", "scribe", json!({})).await;

    // Off: the token says so, and payloads a runtime sends anyway are dropped.
    let (_, _, rid, _) = finished_run(
        &app,
        &owner,
        json!({"title": "LEAKY note", "kind": "task", "executor": "agent", "agent_role": "scribe"}),
    )
    .await;
    let request = seen.requests.lock().unwrap()[0].clone();
    let token = request["token"].as_str().unwrap();
    let claims: Value = {
        use base64::Engine as _;
        let part = token.split('.').nth(1).unwrap();
        serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(part)
                .unwrap(),
        )
        .unwrap()
    };
    assert_eq!(
        claims["recording"],
        json!({"prompts": false, "completions": false, "tool_io": false}),
        "{claims}"
    );
    let spans = spans_of(&app, &owner, &rid).await;
    let turn = span_of(&spans, "llm_call", "claude-test");
    let read = span_of(&spans, "data_read", "context");
    assert_eq!(read["attributes"]["recording"], false);
    let (status, none) = payload(&app, &owner, turn["id"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{none}");
    assert!(none["payloads"].as_array().unwrap().is_empty(), "{none}");
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM span_payloads")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        stored, 0,
        "nothing was kept of the leaky runtime's payloads"
    );
    let (_, session) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/sessions"),
        &owner,
        None,
    )
    .await;
    assert_eq!(session["recording"], false);

    // A bad retention is refused; a change of the setting is audited in words.
    let (status, refused) = call(
        &app,
        Method::PUT,
        &format!("/workspaces/{wid}/guardrails"),
        &owner,
        Some(json!({"recording": {"prompts": true, "retention_days": 0}})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert!(
        refused["errors"]["recording.retention_days"].is_array(),
        "{refused}"
    );
    let saved = set_recording(
        &app,
        &owner,
        &wid,
        json!({"prompts": true, "completions": true, "tool_io": true, "retention_days": 14}),
    )
    .await;
    assert_eq!(saved["recording"]["redact_labels"], json!([]));
    assert_eq!(saved["recording"]["retention_days"], 14);
    let audited = audit_actions(&app, &owner, &wid).await;
    let changed = audited
        .iter()
        .find(|(action, _, _)| action == "recording_changed")
        .expect("the change is audited");
    assert_eq!(changed.1, "Recording");
    assert!(
        changed
            .2
            .contains("recording off -> recording prompts, completions, tool_io; kept 14 day(s)"),
        "{}",
        changed.2
    );

    // On: the token says so, the data_read span notes it, the payloads are kept and read back.
    let (_, node, rid, _) = finished_run(
        &app,
        &owner,
        json!({"title": "Plain note", "kind": "task", "executor": "agent", "agent_role": "scribe"}),
    )
    .await;
    let request = seen.requests.lock().unwrap()[1].clone();
    assert!(request["token"].as_str().unwrap().contains('.'));
    let spans = spans_of(&app, &owner, &rid).await;
    let read = span_of(&spans, "data_read", "context");
    assert_eq!(read["attributes"]["recording"], true);
    assert_eq!(
        read["attributes"]["recording_kinds"],
        json!(["prompts", "completions", "tool_io"])
    );
    let turn = span_of(&spans, "llm_call", "claude-test");
    let (status, turn_payloads) = payload(&app, &owner, turn["id"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{turn_payloads}");
    assert_eq!(turn_payloads["span_id"], turn["id"]);
    assert_eq!(turn_payloads["run_id"], rid);
    assert_eq!(turn_payloads["node_id"], node);
    assert_eq!(turn_payloads["kind"], "llm_call");
    assert_eq!(kinds(&turn_payloads), ["prompt", "completion"]);
    let prompt = &turn_payloads["payloads"][0];
    assert!(
        prompt["content"]
            .as_str()
            .unwrap()
            .starts_with("[system]\n"),
        "{prompt}"
    );
    assert_eq!(prompt["size"], prompt["content"].as_str().unwrap().len());
    assert_eq!(prompt["redactions"], 0);
    assert!(
        turn_payloads["payloads"][1]["content"]
            .as_str()
            .unwrap()
            .contains("[tool call write_file]")
    );
    let tool = span_of(&spans, "tool_call", "write_file");
    let (_, tool_payloads) = payload(&app, &owner, tool["id"].as_str().unwrap()).await;
    assert_eq!(kinds(&tool_payloads), ["tool_input", "tool_output"]);
    assert!(
        tool_payloads["payloads"][0]["content"]
            .as_str()
            .unwrap()
            .contains("deliverable.md")
    );
    assert_eq!(
        tool_payloads["payloads"][1]["content"],
        "wrote deliverable.md (12 bytes)"
    );
    let (_, session) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/sessions"),
        &owner,
        None,
    )
    .await;
    assert_eq!(session["recording"], true);
    // Kept sealed: the rows hold ciphertext, not the words.
    let raw: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT content FROM span_payloads WHERE run_id = $1::uuid")
            .bind(&rid)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(raw.len(), 4);
    assert!(
        raw.iter()
            .all(|bytes| !String::from_utf8_lossy(bytes).contains("deliverable"))
    );
    // Reading is audited once per read that had something to read; a span that recorded
    // nothing is not an audit entry.
    let viewed: Vec<_> = audit_actions(&app, &owner, &wid)
        .await
        .into_iter()
        .filter(|(action, _, _)| action == "payload_viewed")
        .collect();
    assert_eq!(viewed.len(), 2, "{viewed:?}");
    assert_eq!(viewed[0].1, tool["id"]);
    assert!(
        viewed[0].2.contains("tool_input, tool_output"),
        "{}",
        viewed[0].2
    );
    assert!(viewed[1].2.contains(&rid));

    // The run's owner and admins read; another member may not; an outsider finds nothing.
    let (member, _) = app.register("member@example.com").await;
    let (admin, _) = app.register("admin@example.com").await;
    for (email, role) in [
        ("member@example.com", "member"),
        ("admin@example.com", "admin"),
    ] {
        let (status, _) = call(
            &app,
            Method::POST,
            &format!("/workspaces/{wid}/members"),
            &owner,
            Some(json!({"email": email, "role": role})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, _) = payload(&app, &member, turn["id"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = payload(&app, &admin, turn["id"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    let (outsider, _) = app.register("outsider@example.com").await;
    let (status, _) = payload(&app, &outsider, turn["id"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = payload(&app, &owner, &uuid::Uuid::now_v7().to_string()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Only the kinds asked for are kept: prompts alone keeps no completion and no tool io.
    set_recording(&app, &owner, &wid, json!({"prompts": true})).await;
    let (_, _, rid, _) = finished_run(
        &app,
        &owner,
        json!({"title": "LEAKY again", "kind": "task", "executor": "agent", "agent_role": "scribe"}),
    )
    .await;
    let counts = nexc::repo::span_payloads::count_for_run(&pool, rid.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(counts, [("prompt".to_owned(), 1)]);

    // The built-in executor records its one call the same way.
    let (_, _, rid, _) = finished_run(
        &app,
        &owner,
        json!({"title": "Built in", "kind": "task", "executor": "llm"}),
    )
    .await;
    let spans = spans_of(&app, &owner, &rid).await;
    let turn = spans
        .iter()
        .find(|s| s["kind"] == "llm_call")
        .expect("the call");
    let (_, built_in) = payload(&app, &owner, turn["id"].as_str().unwrap()).await;
    assert_eq!(kinds(&built_in), ["prompt"], "{built_in}");
    assert!(
        built_in["payloads"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Built in")
    );
}

/// A small HTTP server the gateway may fetch from.
async fn fetchable() -> String {
    let app = Router::new().route(
        "/status",
        get(|| async { axum::Json(json!({"ok": true, "service": "fetchable"})) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn restricted_passages_are_replaced_by_their_citation_and_secrets_scrubbed(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let site = fetchable().await;
    let app = grpc_app(pool.clone(), &url).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    set_recording(
        &app,
        &owner,
        &wid,
        json!({"prompts": true, "completions": true, "tool_io": true, "redact_labels": ["restricted"]}),
    )
    .await;
    // A restricted handbook, and an agent cleared to read it.
    tokio::fs::create_dir_all(app.state.settings.documents_dir())
        .await
        .unwrap();
    let r = app
        .send_bytes(
            &format!("/api/v1/workspaces/{wid}/documents?name=handbook.md"),
            &owner,
            HANDBOOK.into(),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let did = r.body["id"].as_str().unwrap().to_owned();
    nexc::engine::knowledge::work(&app.state).await.unwrap();
    let (status, body) = call(
        &app,
        Method::PATCH,
        &format!("/workspaces/{wid}/documents/{did}"),
        &owner,
        Some(json!({"label": "restricted"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    agent(
        &app,
        &owner,
        "Clerk",
        "clerk",
        json!({"data": {"labels": ["public", "internal", "confidential", "restricted"]},
            "egress_hosts": ["127.0.0.1"]}),
    )
    .await;
    let (_, _, rid, _) = finished_run(
        &app,
        &owner,
        json!({"title": "RECORD customs", "kind": "research", "executor": "agent", "agent_role": "clerk",
            "content": "Which customs reference do invoices from the Rotterdam warehouse need?"}),
    )
    .await;
    let request = seen.requests.lock().unwrap()[0].clone();
    let documents = request["context"]["documents"].as_array().unwrap().clone();
    assert!(
        !documents.is_empty(),
        "the cleared agent was given the passage: {request}"
    );
    let given = documents[0].as_str().unwrap();
    assert!(given.contains("RT-7731"), "{given}");
    let spans = spans_of(&app, &owner, &rid).await;
    let read = span_of(&spans, "data_read", "context");
    assert_eq!(
        read["attributes"]["recording_redact_labels"],
        json!(["restricted"])
    );
    let chunk = read["attributes"]["document_chunk_ids"][0]
        .as_str()
        .unwrap();
    let turn = span_of(&spans, "llm_call", "claude-test");
    let (status, payloads) = payload(&app, &owner, turn["id"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{payloads}");
    assert_eq!(
        payloads["labels"],
        json!(["restricted"]),
        "the banner's labels"
    );
    let prompt = payloads["payloads"][0]["content"].as_str().unwrap();
    assert!(
        prompt.contains(&format!("[redacted restricted passage {chunk}]")),
        "{prompt}"
    );
    assert!(
        !prompt.contains("RT-7731"),
        "the passage's words are gone: {prompt}"
    );
    assert!(
        !prompt.contains("Rotterdam warehouse need a customs"),
        "{prompt}"
    );
    assert!(
        prompt.contains("ANTHROPIC_API_KEY=[redacted secret]"),
        "{prompt}"
    );
    assert!(!prompt.contains("sk-ant"), "{prompt}");
    assert!(
        prompt.contains("[user]\nWhich customs reference"),
        "the task stays: {prompt}"
    );
    assert!(
        payloads["payloads"][0]["redactions"].as_i64().unwrap() >= 2,
        "{payloads}"
    );

    // A gateway call the backend performed records what went out and what came back.
    let (_, _, rid, _) = finished_run(
        &app,
        &owner,
        json!({"title": "TOOL fetch_url", "kind": "task", "executor": "agent", "agent_role": "clerk",
            "content": json!({"url": format!("{site}/status")}).to_string()}),
    )
    .await;
    let spans = spans_of(&app, &owner, &rid).await;
    let fetched = span_of(&spans, "tool_call", "fetch_url");
    assert_eq!(fetched["attributes"]["gateway"], true);
    let (_, io) = payload(&app, &owner, fetched["id"].as_str().unwrap()).await;
    assert_eq!(kinds(&io), ["tool_input", "tool_output"], "{io}");
    assert!(
        io["payloads"][0]["content"]
            .as_str()
            .unwrap()
            .contains("/status")
    );
    assert!(
        io["payloads"][1]["content"]
            .as_str()
            .unwrap()
            .starts_with("HTTP 200"),
        "{io}"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn payloads_past_the_retention_are_deleted(pool: PgPool) {
    let (url, _) = fake_grpc_runtime().await;
    let app = grpc_app(pool.clone(), &url).await;
    let (owner, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &owner).await;
    set_recording(
        &app,
        &owner,
        &wid,
        json!({"prompts": true, "completions": true, "retention_days": 30}),
    )
    .await;
    agent(&app, &owner, "Scribe", "scribe", json!({})).await;
    let (_, _, rid, _) = finished_run(
        &app,
        &owner,
        json!({"title": "Keep me", "kind": "task", "executor": "agent", "agent_role": "scribe"}),
    )
    .await;
    let run_id: uuid::Uuid = rid.parse().unwrap();
    let count = || async {
        nexc::repo::span_payloads::count_for_run(&pool, run_id)
            .await
            .unwrap()
            .iter()
            .map(|(_, n)| n)
            .sum::<i64>()
    };
    assert_eq!(count().await, 2);
    // Fresh rows stay; a row older than the retention goes; a longer retention keeps it.
    assert_eq!(
        nexc::repo::span_payloads::purge_expired(&pool)
            .await
            .unwrap(),
        0
    );
    sqlx::query("UPDATE span_payloads SET created_at = now() - interval '40 days' WHERE run_id = $1 AND kind = 'prompt'")
        .bind(run_id)
        .execute(&pool)
        .await
        .unwrap();
    set_recording(
        &app,
        &owner,
        &wid,
        json!({"prompts": true, "completions": true, "retention_days": 60}),
    )
    .await;
    assert_eq!(
        nexc::repo::span_payloads::purge_expired(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(count().await, 2);
    set_recording(
        &app,
        &owner,
        &wid,
        json!({"prompts": true, "completions": true, "retention_days": 30}),
    )
    .await;
    assert_eq!(
        nexc::repo::span_payloads::purge_expired(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(count().await, 1);
    // A workspace that never set guardrails keeps payloads thirty days.
    sqlx::query("DELETE FROM workspace_guardrails WHERE workspace_id = $1::uuid")
        .bind(&wid)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE span_payloads SET created_at = now() - interval '31 days' WHERE run_id = $1",
    )
    .bind(run_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        nexc::repo::span_payloads::purge_expired(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(count().await, 0);
}
