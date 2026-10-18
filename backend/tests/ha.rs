//! High availability of runs (docs/HA.md): two instances of the app share
//! one database and one agent runtime. The instance executing a run dies
//! abruptly mid-run (no graceful shutdown) and the other takes the run over
//! within a lease and a few seconds, resumes it from what was written and
//! finishes it: no node succeeds twice, no span is missing and the chain
//! verifies. A former holder's late write is refused and recorded; a node
//! waiting for an approval waits on across the takeover and resumes when
//! someone decides; an operator's command the dead instance never took is
//! delivered by the new one; a holder cut off from its lease stops by
//! itself.
#![forbid(unsafe_code)]

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode};
use common::grpc_runtime::{Seen, fake_grpc_runtime};
use common::{TestApp, collect_until};
use nexc::engine::artifacts;
use nexc::engine::lease::{self, Lease, WriteError};
use nexc::engine::trace::{self, Tail};
use nexc::realtime::hub::SseFrame;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::broadcast;
use uuid::Uuid;

/// The lease every instance of these tests holds on a run.
const LEASE_SECS: u64 = 2;

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

/// An instance of the app on `pool` whose runtime is the gRPC stand-in at `url`.
async fn instance(pool: &PgPool, url: &str) -> TestApp {
    instance_with_lease(pool, url, LEASE_SECS).await
}

async fn instance_with_lease(pool: &PgPool, url: &str, lease_secs: u64) -> TestApp {
    TestApp::new(
        pool.clone(),
        &[
            ("NEXC_RUNTIME_PROTOCOL", "grpc"),
            ("NEXC_RUNTIME_GRPC_URL", url),
            ("NEXC_RUN_LEASE_SECS", &lease_secs.to_string()),
        ],
    )
    .await
}

async fn workspace(app: &TestApp, token: &str) -> String {
    let (_, list) = call(app, Method::GET, "/workspaces", token, None).await;
    list[0]["id"].as_str().unwrap().to_owned()
}

async fn scribe(app: &TestApp, token: &str) {
    let (status, agent) = call(
        app,
        Method::POST,
        "/agents",
        token,
        Some(json!({
            "name": "Scribe", "role": "scribe", "runtime": "python",
            "purpose": "Handles scribe work in tests.", "capabilities": {},
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
}

/// A graph of agent nodes run by the scribe, chained in the order given;
/// returns its id and the node ids by title.
async fn chain_of(app: &TestApp, token: &str, titles: &[&str]) -> (String, Vec<String>) {
    let gid = app.graph(token, "Survive a crash").await;
    let mut nodes = Vec::new();
    for title in titles {
        nodes.push(
            app.node(
                token,
                &gid,
                json!({"title": title, "kind": "task", "executor": "agent", "agent_role": "scribe"}),
            )
            .await,
        );
    }
    for pair in nodes.windows(2) {
        let r = app.edge(token, &gid, &pair[0], &pair[1]).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    }
    (gid, nodes)
}

async fn start_run(app: &TestApp, token: &str, gid: &str) -> String {
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        token,
        Some(json!({"force": true})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    body["id"].as_str().unwrap().to_owned()
}

/// Collects `node.status` events until `node` reaches `status`.
async fn until_status(
    rx: &mut broadcast::Receiver<SseFrame>,
    node: &str,
    status: &str,
) -> Vec<(String, Value)> {
    let mut seen = Vec::new();
    loop {
        let events = collect_until(rx, "node.status").await;
        let reached = events
            .last()
            .is_some_and(|(_, d)| d["node_id"] == node && d["status"] == status);
        seen.extend(events);
        if reached {
            return seen;
        }
    }
}

/// Every event already received, without waiting.
fn drain(rx: &mut broadcast::Receiver<SseFrame>) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    while let Ok(frame) = rx.try_recv() {
        out.push((
            frame.event.to_string(),
            serde_json::from_str(&frame.data).unwrap(),
        ));
    }
    out
}

/// Waits until `instance` holds the run; returns how long that took.
async fn until_held_by(pool: &PgPool, rid: &str, instance: Uuid, limit: Duration) -> Duration {
    let started = Instant::now();
    let rid: Uuid = rid.parse().unwrap();
    loop {
        let holder: Option<Uuid> = sqlx::query_scalar("SELECT claimed_by FROM runs WHERE id = $1")
            .bind(rid)
            .fetch_one(pool)
            .await
            .unwrap();
        if holder == Some(instance) {
            return started.elapsed();
        }
        assert!(
            started.elapsed() < limit,
            "the run was not taken over within {limit:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Polls the run until it is no longer running; returns it.
async fn until_finished(app: &TestApp, token: &str, rid: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let (status, run) = call(app, Method::GET, &format!("/runs/{rid}"), token, None).await;
        assert_eq!(status, StatusCode::OK, "{run}");
        if !matches!(run["status"].as_str(), Some("queued" | "running")) {
            return run;
        }
        assert!(Instant::now() < deadline, "the run did not finish: {run}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
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

/// The lease events of a run, oldest first: `(kind, instance, epoch, previous, detail)`.
async fn lease_events(pool: &PgPool, rid: &str) -> Vec<(String, Uuid, i64, Option<Uuid>, String)> {
    nexc::repo::run_leases::of_run(pool, rid.parse().unwrap())
        .await
        .unwrap()
        .into_iter()
        .map(|e| (e.kind, e.instance, e.epoch, e.previous, e.detail))
        .collect()
}

/// How many execute requests the runtime received per node title.
fn executed(seen: &Arc<Seen>) -> HashMap<String, usize> {
    let mut count = HashMap::new();
    for request in seen.requests.lock().unwrap().iter() {
        *count
            .entry(request["task"]["title"].as_str().unwrap().to_owned())
            .or_default() += 1;
    }
    count
}

/// Usage ledger entries per node of a run.
async fn usage_per_node(pool: &PgPool, rid: &str) -> HashMap<Uuid, i64> {
    sqlx::query_as::<_, (Uuid, i64)>(
        "SELECT node_id, count(*) FROM llm_usage WHERE run_id = $1 AND purpose = 'node' GROUP BY node_id",
    )
    .bind(rid.parse::<Uuid>().unwrap())
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .collect()
}

/// How many files are under `dir`, at any depth.
fn files_under(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| {
                    if e.path().is_dir() {
                        files_under(&e.path())
                    } else {
                        1
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

fn succeeded_events(events: &[(String, Value)], node: &str) -> usize {
    events
        .iter()
        .filter(|(e, d)| e == "node.status" && d["node_id"] == node && d["status"] == "succeeded")
        .count()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_run_survives_the_crash_of_the_replica_executing_it(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let a = instance(&pool, &url).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["Outline", "NAP 2500 Draft", "Final"]).await;
    let (outline, draft, last) = (&nodes[0], &nodes[1], &nodes[2]);
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    // A finished the outline and is in the middle of the draft: the runtime is working on it.
    let mut seen_on_a = until_status(&mut on_a, draft, "running").await;
    seen_on_a.extend(collect_until(&mut on_a, "node.output").await);
    assert_eq!(succeeded_events(&seen_on_a, outline), 1);

    let b = instance(&pool, &url).await;
    let mut on_b = b.events(&gid);
    a.crash();
    let limit = Duration::from_secs(LEASE_SECS + 5);
    let took = until_held_by(&pool, &rid, b.state.engine.instance(), limit).await;
    assert!(took <= limit, "taken over after {took:?}");

    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    let by_node: HashMap<&str, &Value> = run["node_runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| (n["node_id"].as_str().unwrap(), n))
        .collect();
    for node in &nodes {
        assert_eq!(by_node[node.as_str()]["status"], "succeeded", "{run}");
    }
    assert_eq!(
        by_node[outline.as_str()]["attempt"],
        1,
        "kept, not run again"
    );
    assert_eq!(
        by_node[draft.as_str()]["attempt"],
        2,
        "the interrupted attempt is followed by the next"
    );
    assert_eq!(by_node[last.as_str()]["attempt"], 1);

    // Every node reached success once: on the wire of both instances, and in the ledger.
    let events_b = collect_until(&mut on_b, "run.finished").await;
    seen_on_a.extend(drain(&mut on_a));
    for node in &nodes {
        let successes = succeeded_events(&seen_on_a, node) + succeeded_events(&events_b, node);
        assert_eq!(successes, 1, "node {node} succeeded {successes} times");
    }
    let usage = usage_per_node(&pool, &rid).await;
    assert_eq!(usage.len(), 3, "{usage:?}");
    assert!(usage.values().all(|&n| n == 1), "{usage:?}");
    let ran = executed(&seen);
    assert_eq!(ran["Outline"], 1);
    assert_eq!(ran["NAP 2500 Draft"], 2, "run again after the crash");
    assert_eq!(ran["Final"], 1);

    // The chain continued from the last span A wrote: it verifies, and no span is missing.
    let verification = verified(&b, &token, &rid).await;
    assert_eq!(verification["ok"], true, "{verification}");
    let spans = spans_of(&b, &token, &rid).await;
    assert_eq!(verification["spans"], spans.len());
    let mut seqs: Vec<i64> = spans.iter().map(|s| s["seq"].as_i64().unwrap()).collect();
    seqs.sort_unstable();
    assert_eq!(seqs, (0..spans.len() as i64).collect::<Vec<_>>());
    for node in &nodes {
        assert!(
            spans
                .iter()
                .any(|s| s["node_id"] == node.as_str() && s["kind"] == "data_read"),
            "node {node} has its context on the record"
        );
    }
    let interrupted = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["decision"] == "interrupted")
        .expect("the interrupted attempt is on the trace");
    assert_eq!(interrupted["node_id"], draft.as_str());
    assert_eq!(interrupted["attributes"]["rule"], "lease");
    assert_eq!(
        interrupted["attributes"]["previous_instance"],
        a.state.engine.instance().to_string()
    );
    assert_eq!(interrupted["attributes"]["attempt"], 1);
    assert_eq!(interrupted["attributes"]["next_attempt"], 2);
    assert_eq!(interrupted["attributes"]["lease_epoch"], 2);

    // The lease's history: claimed by A, taken over by B from A.
    let events = lease_events(&pool, &rid).await;
    let (a_id, b_id) = (a.state.engine.instance(), b.state.engine.instance());
    assert!(
        events.contains(&("claimed".into(), a_id, 1, None, String::new())),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| e.0 == "taken_over" && e.1 == b_id && e.2 == 2 && e.3 == Some(a_id)),
        "{events:?}"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_former_holder_cannot_write_once_the_run_was_taken_over(pool: PgPool) {
    let (url, _seen) = fake_grpc_runtime().await;
    let a = instance(&pool, &url).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["NAP 3000 Draft"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, &nodes[0], "running").await;
    let a_id = a.state.engine.instance();
    a.crash();
    let b = instance(&pool, &url).await;
    until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(LEASE_SECS + 5),
    )
    .await;

    // A comes back, still believing it holds the run at epoch 1, and writes.
    let run_id: Uuid = rid.parse().unwrap();
    let resurrected = Lease::held(run_id, 1, a_id);
    let refused = lease::begin(&a.state, &resurrected, "node outcome").await;
    assert!(
        matches!(refused, Err(WriteError::Fenced)),
        "the late write is refused"
    );
    assert!(resurrected.is_lost(), "and the former holder knows it now");
    // Its spans, continuing the chain as it last saw it, are refused too.
    let graph_id: Uuid = gid.parse().unwrap();
    let tail = Tail::of_run(&a.state, run_id).await.unwrap();
    let sink = trace::start(
        a.state.clone(),
        run_id,
        graph_id,
        Lease::held(run_id, 1, a_id),
        tail,
    );
    let late = Uuid::now_v7();
    sink.handle().emit(nexc::domain::trace::SpanDraft {
        id: late,
        node_id: nodes[0].parse().unwrap(),
        agent_id: None,
        principal: "system".into(),
        parent_id: None,
        kind: nexc::domain::trace::SpanKind::Policy,
        name: "late".into(),
        started_at: chrono::Utc::now(),
        ended_at: None,
        status: nexc::domain::trace::SpanStatus::Ok,
        attributes: json!({}),
    });
    sink.seal().await;

    // Recorded once per lease, with what it tried and where the run is now.
    let events = lease_events(&pool, &rid).await;
    let fenced: Vec<_> = events.iter().filter(|e| e.0 == "fenced").collect();
    assert_eq!(fenced.len(), 2, "{events:?}");
    assert!(fenced.iter().all(|e| e.1 == a_id && e.2 == 1));
    assert!(
        fenced.iter().any(|e| e
            .4
            .starts_with("node outcome refused: the run is at epoch 2")),
        "{fenced:?}"
    );
    assert!(
        fenced
            .iter()
            .any(|e| e.4.starts_with("trace spans refused")),
        "{fenced:?}"
    );
    assert!(
        a.state
            .metrics
            .render()
            .contains("nexc_run_lease_events_total{event=\"fenced\"} 2"),
        "counted on the instance that tried"
    );

    // B stores an artifact of the node; A's late artifact of the same path replaces neither
    // its record nor its bytes, and leaves no file behind.
    let node_id: Uuid = nodes[0].parse().unwrap();
    let b_lease = b.state.engine.lease(run_id).expect("B executes the run");
    let kept = artifacts::store(
        &b.state,
        &b_lease,
        graph_id,
        run_id,
        node_id,
        "report.md",
        None,
        b"from the holder",
    )
    .await
    .expect("the holder's artifact is kept");
    let refused_artifact = artifacts::store(
        &a.state,
        &Lease::held(run_id, 1, a_id),
        graph_id,
        run_id,
        node_id,
        "report.md",
        None,
        b"from a former holder",
    )
    .await;
    assert!(refused_artifact.is_err(), "the late artifact is refused");
    let stored = nexc::repo::artifacts::list_stored(&pool, run_id)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].storage_path, kept.storage_path);
    let on_disk = artifacts::resolve(&b.state, &stored[0].storage_path).unwrap();
    assert_eq!(std::fs::read(on_disk).unwrap(), b"from the holder");
    // (Each instance of these tests has a data directory of its own.)
    let a_run_dir = a.state.settings.artifacts_dir().join(run_id.to_string());
    assert_eq!(files_under(&a_run_dir), 0, "the refused write left no file");

    // B finishes the run as if A had never come back.
    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    assert_eq!(run["node_runs"][0]["attempt"], 2);
    assert_eq!(verified(&b, &token, &rid).await["ok"], true);
    let spans = spans_of(&b, &token, &rid).await;
    assert!(
        spans.iter().all(|s| s["id"] != late.to_string()),
        "the late span is not on the record"
    );
    let usage = usage_per_node(&pool, &rid).await;
    assert_eq!(usage.values().sum::<i64>(), 1, "{usage:?}");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_node_waiting_for_an_approval_resumes_after_a_takeover(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let a = instance(&pool, &url).await;
    let (token, _) = a.register("owner@example.com").await;
    let wid = workspace(&a, &token).await;
    let (status, policy) = call(
        &a,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        &token,
        Some(json!({"scope": "workspace", "scope_id": null,
            "document": {"require_approval": {"tools": ["make_docx"], "timeout_s": 60}}})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{policy}");
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["APPROVAL report", "Final"]).await;
    let (report, last) = (&nodes[0], &nodes[1]);
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, report, "waiting_approval").await;
    let (_, pending) = call(
        &a,
        Method::GET,
        &format!("/workspaces/{wid}/approvals?status=pending"),
        &token,
        None,
    )
    .await;
    let original = pending[0]["id"].as_str().unwrap().to_owned();

    let b = instance(&pool, &url).await;
    a.crash();
    until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(LEASE_SECS + 5),
    )
    .await;
    // The node waits on, on the new holder.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let waiting: String =
        sqlx::query_scalar("SELECT status FROM node_runs WHERE run_id = $1 AND node_id = $2")
            .bind(rid.parse::<Uuid>().unwrap())
            .bind(report.parse::<Uuid>().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(waiting, "waiting_approval");

    // Someone decides, on whichever instance answers; the node resumes.
    let (status, decided) = call(
        &b,
        Method::POST,
        &format!("/approvals/{original}/approve"),
        &token,
        Some(json!({"reason": "go ahead"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    let report_run = run["node_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["node_id"] == report.as_str())
        .unwrap();
    assert_eq!(report_run["attempt"], 2);
    assert_eq!(
        run["policy_mismatches"], 0,
        "the held tool ran with permission"
    );
    assert_eq!(executed(&seen)["APPROVAL report"], 2);
    assert!(
        seen.commands
            .lock()
            .unwrap()
            .contains(&"approval_decision".to_owned())
    );

    // The new attempt asked again and was answered with the decision already taken.
    let (_, approvals) = call(
        &b,
        Method::GET,
        &format!("/workspaces/{wid}/approvals"),
        &token,
        None,
    )
    .await;
    let approvals = approvals.as_array().unwrap();
    assert_eq!(approvals.len(), 2, "{approvals:?}");
    assert!(approvals.iter().all(|a| a["status"] == "approved"));
    assert!(
        approvals.iter().all(|a| a["decided_by"]["name"] == "Test"),
        "decided once, by the same person: {approvals:?}"
    );
    let carried = approvals
        .iter()
        .find(|a| a["id"] != original.as_str())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let spans = spans_of(&b, &token, &rid).await;
    assert!(
        spans
            .iter()
            .any(|s| s["kind"] == "policy" && s["attributes"]["decision"] == "reattached"),
        "the node was re-attached"
    );
    let decision = spans
        .iter()
        .find(|s| s["kind"] == "approval" && s["attributes"]["approval_id"] == original.as_str())
        .expect("the decision on the original request");
    assert_eq!(decision["attributes"]["decision"], "approved");
    assert_eq!(decision["attributes"]["reattached"], true);
    let answered = spans
        .iter()
        .find(|s| s["kind"] == "approval" && s["attributes"]["approval_id"] == carried.as_str())
        .expect("the decision carried to the new request");
    assert_eq!(answered["attributes"]["carried_from"], original.as_str());
    let ran = spans
        .iter()
        .find(|s| s["kind"] == "tool_call" && s["name"] == "make_docx")
        .expect("the held tool ran");
    assert_eq!(ran["attributes"]["approval_id"], carried.as_str());
    assert!(
        run["node_runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["node_id"] == last.as_str() && n["status"] == "succeeded")
    );
    assert_eq!(verified(&b, &token, &rid).await["ok"], true);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_command_the_dead_instance_never_took_is_delivered_by_the_new_one(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let a = instance(&pool, &url).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, _) = chain_of(&a, &token, &["STOPPABLE research"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    // The researcher was spawned on A.
    let spawn = loop {
        let events = collect_until(&mut on_a, "trace.span").await;
        if let Some((_, d)) = events.last()
            && d["span"]["kind"] == "spawn"
        {
            break d["span"]["id"].as_str().unwrap().to_owned();
        }
    };
    // Give the persister a moment to write the spawn span, then A dies.
    tokio::time::sleep(Duration::from_millis(300)).await;
    a.crash();
    let b = instance(&pool, &url).await;
    // The operator stops the researcher while nobody executes the run: recorded, not delivered.
    let (status, cancelled) = call(
        &b,
        Method::POST,
        &format!("/runs/{rid}/agents/{spawn}/cancel"),
        &token,
        Some(json!({"reason": "off topic"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{cancelled}");
    assert_eq!(cancelled["delivered"], false);
    until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(LEASE_SECS + 5),
    )
    .await;
    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    // B took the command and delivered it to the researcher of the attempt that replaced A's.
    let stopped = seen.cancelled_agents.lock().unwrap().clone();
    assert_eq!(stopped.len(), 1, "{stopped:?}");
    assert_ne!(stopped[0], spawn, "the sub-agent of the new attempt");
    let spans = spans_of(&b, &token, &rid).await;
    let delivered = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["attributes"]["action"] == "cancel_agent")
        .expect("the cancel is on the trace");
    assert_eq!(delivered["attributes"]["delivered"], true);
    assert_eq!(
        delivered["attributes"]["spawn_span_id"],
        stopped[0].as_str()
    );
    assert_eq!(
        delivered["attributes"]["carried_from_spawn_span_id"],
        spawn.as_str()
    );
    assert_eq!(verified(&b, &token, &rid).await["ok"], true);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_run_cancelled_while_nobody_holds_it_runs_nothing_more_on_takeover(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let a = instance(&pool, &url).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["NAP 8000 Draft", "Final"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, &nodes[0], "running").await;
    collect_until(&mut on_a, "node.output").await;
    a.crash();
    let b = instance(&pool, &url).await;
    // The owner cancels while nobody executes the run: only recorded, nothing runs here to stop.
    let (status, cancelled) = call(
        &b,
        Method::POST,
        &format!("/runs/{rid}/cancel"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["status"], "running", "{cancelled}");
    until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(LEASE_SECS + 5),
    )
    .await;
    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "cancelled", "{run}");
    for node_run in run["node_runs"].as_array().unwrap() {
        assert_eq!(node_run["status"], "cancelled", "{run}");
    }
    // The new holder sent no attempt to the runtime: the cancel took effect before any node.
    let ran = executed(&seen);
    assert_eq!(ran["NAP 8000 Draft"], 1, "{ran:?}");
    assert!(!ran.contains_key("Final"), "{ran:?}");
    let spans = spans_of(&b, &token, &rid).await;
    let interrupted = spans
        .iter()
        .find(|s| s["attributes"]["decision"] == "interrupted")
        .expect("the interrupted attempt is on the trace");
    assert_eq!(interrupted["attributes"]["next_attempt"], Value::Null);
    assert_eq!(verified(&b, &token, &rid).await["ok"], true);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_run_claimed_by_a_replica_of_the_earlier_version_is_taken_over_once_it_is_silent(
    pool: PgPool,
) {
    let (url, seen) = fake_grpc_runtime().await;
    let a = instance(&pool, &url).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["NAP 3000 Draft"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, &nodes[0], "running").await;
    collect_until(&mut on_a, "node.output").await;
    a.crash();
    // As a replica of the version before leases leaves a run it claimed: a holder and a
    // heartbeat, no lease.
    let run_id: Uuid = rid.parse().unwrap();
    let heartbeat = |pool: PgPool| async move {
        sqlx::query("UPDATE runs SET lease_expires_at = NULL, heartbeat_at = now() WHERE id = $1")
            .bind(run_id)
            .execute(&pool)
            .await
            .unwrap();
    };
    heartbeat(pool.clone()).await;
    let b = instance(&pool, &url).await;
    let a_id = a.state.engine.instance();
    // While it sends heartbeats it keeps the run, for longer than a lease.
    for _ in 0..8 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        heartbeat(pool.clone()).await;
        let holder: Option<Uuid> = sqlx::query_scalar("SELECT claimed_by FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            holder,
            Some(a_id),
            "taken from a holder that sends heartbeats"
        );
    }
    // Silent for a lease: taken over and finished.
    until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(LEASE_SECS + 5),
    )
    .await;
    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    assert_eq!(run["node_runs"][0]["attempt"], 2);
    assert_eq!(executed(&seen)["NAP 3000 Draft"], 2);
    let events = lease_events(&pool, &rid).await;
    assert!(
        events
            .iter()
            .any(|e| e.0 == "taken_over" && e.3 == Some(a_id) && e.4.contains("stopped renewing")),
        "{events:?}"
    );
    assert_eq!(verified(&b, &token, &rid).await["ok"], true);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_holder_that_vanished_in_the_middle_of_a_write_does_not_keep_its_run(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let a = instance(&pool, &url).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["NAP 3000 Draft"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, &nodes[0], "running").await;
    collect_until(&mut on_a, "node.output").await;
    // A's host vanishes between two statements of a write: as far as the database knows, its
    // transaction is open and holds the run row.
    let run_id: Uuid = rid.parse().unwrap();
    let held = a.state.engine.lease(run_id).expect("A executes the run");
    let open = lease::begin(&a.state, &held, "node outcome")
        .await
        .expect("A still holds the run");
    a.crash();
    let b = instance(&pool, &url).await;
    // The database ends the idle transaction after a lease, so the run is taken over as usual.
    until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(LEASE_SECS + 5),
    )
    .await;
    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    assert_eq!(run["node_runs"][0]["attempt"], 2);
    assert_eq!(executed(&seen)["NAP 3000 Draft"], 2);
    assert_eq!(verified(&b, &token, &rid).await["ok"], true);
    drop(open);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_holder_cut_off_from_its_lease_stops_by_itself(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let a = instance(&pool, &url).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    // Long enough that A is still working on it when B has taken the run over.
    let (gid, nodes) = chain_of(&a, &token, &["NAP 8000 Draft"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, &nodes[0], "running").await;
    collect_until(&mut on_a, "node.output").await;
    // A stops renewing (cut off from the database) but goes on executing.
    a.stop_renewing();
    let b = instance(&pool, &url).await;
    until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(LEASE_SECS + 5),
    )
    .await;
    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    assert_eq!(run["node_runs"][0]["attempt"], 2);
    // A noticed and stopped its copy without recording anything for it.
    let events = lease_events(&pool, &rid).await;
    let a_id = a.state.engine.instance();
    assert!(
        events
            .iter()
            .any(|e| e.0 == "lost" && e.1 == a_id && e.2 == 1),
        "{events:?}"
    );
    assert_eq!(a.state.engine.active_count(), 0, "A no longer executes it");
    let usage = usage_per_node(&pool, &rid).await;
    assert_eq!(usage.values().sum::<i64>(), 1, "{usage:?}");
    assert_eq!(executed(&seen)["NAP 8000 Draft"], 2);
    assert!(
        drain(&mut on_a)
            .iter()
            .all(|(e, d)| !(e == "node.status" && d["status"] == "succeeded")),
        "A announced no success"
    );
    // The chain verifies with no span missing: what A chained before it stopped, then B's.
    let verification = verified(&b, &token, &rid).await;
    assert_eq!(verification["ok"], true, "{verification}");
    let spans = spans_of(&b, &token, &rid).await;
    let mut seqs: Vec<i64> = spans.iter().map(|s| s["seq"].as_i64().unwrap()).collect();
    seqs.sort_unstable();
    assert_eq!(seqs, (0..spans.len() as i64).collect::<Vec<_>>());
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_holder_the_database_stops_answering_gives_its_run_up_before_its_lease_runs_out(
    pool: PgPool,
) {
    // Long enough that the third of a lease A keeps in hand is not eaten by a busy machine.
    const LEASE: u64 = 6;
    let (url, _seen) = fake_grpc_runtime().await;
    let a = instance_with_lease(&pool, &url, LEASE).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["NAP 20000 Draft"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, &nodes[0], "running").await;
    collect_until(&mut on_a, "node.output").await;
    let run_id: Uuid = rid.parse().unwrap();
    let held = a.state.engine.lease(run_id).expect("A executes the run");
    // The database stops answering A about the run: its renewals wait on the row until they
    // time out. The lease runs out a lease after the last renewal that went through.
    let mut clock = pool.acquire().await.unwrap();
    let mut cut = pool.begin().await.unwrap();
    let expires: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT lease_expires_at FROM runs WHERE id = $1 FOR UPDATE")
            .bind(run_id)
            .fetch_one(&mut *cut)
            .await
            .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2 * LEASE);
    while !held.is_lost() {
        assert!(Instant::now() < deadline, "A never gave the run up");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // A stopped its copy before a peer could have taken the run over.
    let now: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *clock)
        .await
        .unwrap();
    assert!(
        now < expires,
        "given up at {now}, after the lease ran out at {expires}"
    );
    assert!(held.is_stopped());
    cut.rollback().await.unwrap();
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_replica_that_shuts_down_hands_its_runs_to_a_peer_at_once(pool: PgPool) {
    // A long lease: the peer takes the run over because it was released, not because it ran out.
    let (url, seen) = fake_grpc_runtime().await;
    let a = instance_with_lease(&pool, &url, 30).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["Outline", "NAP 2500 Draft"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, &nodes[1], "running").await;
    collect_until(&mut on_a, "node.output").await;
    let b = instance_with_lease(&pool, &url, 30).await;
    let ready = a.request(Method::GET, "/api/v1/readyz", None, None).await;
    assert_eq!(ready.status, StatusCode::OK);

    // SIGTERM: A drains.
    let handed = nexc::engine::scheduler::hand_off(&a.state, Duration::from_secs(10)).await;
    assert_eq!(handed, 1);
    assert_eq!(a.state.engine.active_count(), 0);
    let ready = a.request(Method::GET, "/api/v1/readyz", None, None).await;
    assert_eq!(
        ready.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        ready.body
    );
    assert!(ready.body["detail"].as_str().unwrap().contains("draining"));
    let took = until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(5),
    )
    .await;
    assert!(took < Duration::from_secs(5), "taken over after {took:?}");

    // A draining replica claims nothing new: the next run goes to B.
    let next = start_run(&a, &token, &gid).await;
    until_held_by(
        &pool,
        &next,
        b.state.engine.instance(),
        Duration::from_secs(5),
    )
    .await;

    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    assert_eq!(run["node_runs"][0]["attempt"], 1, "the outline is kept");
    assert_eq!(run["node_runs"][1]["attempt"], 2);
    let outlines = seen
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r["run_id"] == rid.as_str() && r["task"]["title"] == "Outline")
        .count();
    assert_eq!(outlines, 1, "the outline ran once in this run");
    let events = lease_events(&pool, &rid).await;
    let (a_id, b_id) = (a.state.engine.instance(), b.state.engine.instance());
    assert!(
        events
            .iter()
            .any(|e| e.0 == "released" && e.1 == a_id && e.2 == 1),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| e.0 == "taken_over" && e.1 == b_id && e.2 == 3 && e.3 == Some(a_id)),
        "released at the next epoch, taken over at the one after: {events:?}"
    );
    // Every span A chained before it stopped was written: the chain goes on without a gap.
    let verification = verified(&b, &token, &rid).await;
    assert_eq!(verification["ok"], true, "{verification}");
    let spans = spans_of(&b, &token, &rid).await;
    let interrupted = spans
        .iter()
        .find(|s| s["attributes"]["decision"] == "interrupted")
        .expect("the handed-off attempt is on the trace");
    assert_eq!(interrupted["attributes"]["released"], true);
    assert_eq!(
        interrupted["attributes"]["previous_instance"],
        a_id.to_string()
    );
    until_finished(&b, &token, &next).await;
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn hand_offs_on_shutdown_do_not_use_up_a_nodes_attempts(pool: PgPool) {
    // Two attempts per node (the harness's NEXC_MAX_ATTEMPTS) and two hand-offs in a row, as a
    // rolling update over three replicas does: the node still gets its third attempt.
    let (url, seen) = fake_grpc_runtime().await;
    let a = instance_with_lease(&pool, &url, 30).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["NAP 4000 Draft"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, &nodes[0], "running").await;
    collect_until(&mut on_a, "node.output").await;

    let b = instance_with_lease(&pool, &url, 30).await;
    let mut on_b = b.events(&gid);
    assert_eq!(
        nexc::engine::scheduler::hand_off(&a.state, Duration::from_secs(10)).await,
        1
    );
    until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(5),
    )
    .await;
    until_status(&mut on_b, &nodes[0], "running").await;
    collect_until(&mut on_b, "node.output").await;

    let c = instance_with_lease(&pool, &url, 30).await;
    assert_eq!(
        nexc::engine::scheduler::hand_off(&b.state, Duration::from_secs(10)).await,
        1
    );
    until_held_by(
        &pool,
        &rid,
        c.state.engine.instance(),
        Duration::from_secs(5),
    )
    .await;
    let run = until_finished(&c, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    assert_eq!(run["node_runs"][0]["attempt"], 3);
    assert_eq!(executed(&seen)["NAP 4000 Draft"], 3);
    let spans = spans_of(&c, &token, &rid).await;
    let interrupted: Vec<&Value> = spans
        .iter()
        .filter(|s| s["attributes"]["decision"] == "interrupted")
        .collect();
    assert_eq!(interrupted.len(), 2, "{interrupted:?}");
    assert!(
        interrupted
            .iter()
            .all(|s| s["attributes"]["released"] == true)
    );
    let last = interrupted
        .iter()
        .find(|s| s["attributes"]["attempt"] == 2)
        .expect("the second hand-off");
    assert_eq!(last["attributes"]["next_attempt"], 3);
    assert_eq!(last["attributes"]["max_attempts"], 4);
    assert_eq!(last["attributes"]["handoffs"], 2);
    assert_eq!(verified(&c, &token, &rid).await["ok"], true);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_node_queued_again_by_a_holder_that_died_resumes_at_its_next_attempt(pool: PgPool) {
    let (url, seen) = fake_grpc_runtime().await;
    let a = instance(&pool, &url).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["NAP 3000 Draft"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, &nodes[0], "running").await;
    collect_until(&mut on_a, "node.output").await;
    a.crash();
    // Another instance took the run over, queued the interrupted node again, and died before it
    // started its next attempt.
    let run_id: Uuid = rid.parse().unwrap();
    sqlx::query("UPDATE node_runs SET status = 'queued' WHERE run_id = $1")
        .bind(run_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE runs SET claimed_by = $2, lease_epoch = lease_epoch + 1, lease_expires_at = now()
         WHERE id = $1",
    )
    .bind(run_id)
    .bind(Uuid::now_v7())
    .execute(&pool)
    .await
    .unwrap();
    let b = instance(&pool, &url).await;
    until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(LEASE_SECS + 5),
    )
    .await;
    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    assert_eq!(run["node_runs"][0]["attempt"], 2, "attempt 1 stays counted");
    assert_eq!(executed(&seen)["NAP 3000 Draft"], 2);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_peer_with_a_longer_lease_takes_a_run_over_a_lease_of_its_holder_after_it_died(
    pool: PgPool,
) {
    let (url, _seen) = fake_grpc_runtime().await;
    let a = instance(&pool, &url).await;
    let (token, _) = a.register("owner@example.com").await;
    scribe(&a, &token).await;
    let (gid, nodes) = chain_of(&a, &token, &["NAP 3000 Draft"]).await;
    let mut on_a = a.events(&gid);
    let rid = start_run(&a, &token, &gid).await;
    until_status(&mut on_a, &nodes[0], "running").await;
    let held: Option<f64> = sqlx::query_scalar("SELECT lease_secs FROM runs WHERE id = $1")
        .bind(rid.parse::<Uuid>().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        held,
        Some(LEASE_SECS as f64),
        "the run records its holder's lease"
    );
    a.crash();
    // B was configured with a lease of 30 s: it still waits for A's, not its own.
    let b = instance_with_lease(&pool, &url, 30).await;
    until_held_by(
        &pool,
        &rid,
        b.state.engine.instance(),
        Duration::from_secs(LEASE_SECS + 5),
    )
    .await;
    let run = until_finished(&b, &token, &rid).await;
    assert_eq!(run["status"], "succeeded", "{run}");
}
