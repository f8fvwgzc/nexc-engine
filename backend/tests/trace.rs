//! The trace spine end to end: spans from the built-in executor, the
//! context builder and an agent runtime stand-in are chained per run in
//! order of receipt, fanned out live as `trace.span`, written to the
//! database, sealed into the run and the audit log, served paged, and
//! verified. A changed row is found; hostile span lines are contained.
#![forbid(unsafe_code)]

mod common;

use std::collections::{HashMap, HashSet};

use axum::Router;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::post;
use chrono::Utc;
use common::{RUNTIME_TOKEN, TestApp, collect_until};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Turns of the runtime stand-in's root agent.
const TURNS: usize = 12;
/// Tool calls across those turns.
const TOOL_CALLS: usize = 40;

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

fn span(
    id: Uuid,
    parent: Option<Uuid>,
    kind: &str,
    name: &str,
    agent: &str,
    attrs: Value,
) -> Value {
    json!({
        "type": "span", "id": id, "parent_id": parent, "kind": kind, "name": name, "agent": agent,
        "started_at": now(), "ended_at": now(), "status": "ok", "attributes": attrs,
    })
}

/// The lines a well-behaved runtime sends for a 12-turn run with 40 tool
/// calls and one sub-agent, as the Python runtime does (contract §8).
fn agent_run(agent: &str) -> Vec<Value> {
    let mut lines = Vec::new();
    let mut calls_left = TOOL_CALLS;
    for turn in 1..=TURNS {
        let turn_id = Uuid::now_v7();
        lines.push(span(
            turn_id,
            None,
            "llm_call",
            "claude-test",
            agent,
            json!({
                "provider": "anthropic", "model": "claude-test", "turn": turn,
                "tokens_in": 100, "tokens_out": 20, "tokens_cached": 0, "stop_reason": "tool_use",
            }),
        ));
        lines.push(json!({"type": "tokens", "input": 100, "output": 20}));
        if turn == 2 {
            let spawn = Uuid::now_v7();
            let child = format!("{agent}/researcher-1");
            lines.push(span(
                spawn,
                Some(turn_id),
                "spawn",
                &child,
                agent,
                json!({
                    "child": child, "role": "researcher", "depth": 1, "budget_tokens": 50_000,
                }),
            ));
            lines.push(span(
                Uuid::now_v7(),
                Some(spawn),
                "llm_call",
                "claude-test",
                &child,
                json!({"turn": 1}),
            ));
        }
        let here = calls_left.div_ceil(TURNS + 1 - turn);
        for n in 0..here {
            lines.push(span(Uuid::now_v7(), Some(turn_id), "tool_call", "write_file", agent, json!({
                "tool": "write_file", "args_sha256": format!("{turn:02}{n:02}"), "args_bytes": 80,
                "result_bytes": 24, "is_error": false, "duration_ns": 1_000_000,
            })));
        }
        calls_left -= here;
    }
    assert_eq!(calls_left, 0);
    lines
}

/// An agent runtime stand-in. The node title picks its behaviour:
/// `HOSTILE` adds an oversized span, a span of an unknown kind, a span of a
/// kind only the backend may record, a repeated id and a span after the
/// result; anything else is a well-behaved 12-turn run.
async fn tracing_runtime() -> String {
    async fn execute(
        headers: HeaderMap,
        axum::Json(req): axum::Json<Value>,
    ) -> axum::response::Response {
        let authorized = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            == Some(&format!("Bearer {RUNTIME_TOKEN}"));
        if !authorized {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        let title = req["task"]["title"].as_str().unwrap_or("node").to_owned();
        // A well-behaved runtime acts as the principal its token names.
        let agent = req["agent"]["principal"]
            .as_str()
            .unwrap_or("agent")
            .to_owned();
        let mut lines = vec![json!({"type": "delta", "text": format!("[runtime] {title}")})];
        if title.contains("HOSTILE") {
            let repeated = Uuid::now_v7();
            lines.push(span(
                repeated,
                None,
                "llm_call",
                "claude-test",
                &agent,
                json!({"turn": 1}),
            ));
            lines.push(span(
                repeated,
                None,
                "tool_call",
                "read_file",
                &agent,
                json!({}),
            ));
            lines.push(span(
                Uuid::now_v7(),
                None,
                "tool_call",
                "OVERSIZED",
                &agent,
                json!({"padding": "x".repeat(100 * 1024)}),
            ));
            lines.push(span(
                Uuid::now_v7(),
                None,
                "teleport",
                "UNKNOWN",
                &agent,
                json!({}),
            ));
            lines.push(span(
                Uuid::now_v7(),
                None,
                "approval",
                "FORGED",
                &agent,
                json!({"decision": "allow"}),
            ));
            lines.push(json!({"type": "span", "kind": "tool_call", "name": "BARE"}));
        } else {
            lines.extend(agent_run(&agent));
        }
        lines.push(
            json!({"type": "result", "output": format!("[runtime] {title}"),
            "tokens_in": 1200, "tokens_out": 240}),
        );
        lines.push(span(
            Uuid::now_v7(),
            None,
            "tool_call",
            "AFTER",
            &agent,
            json!({}),
        ));
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        ([(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response()
    }
    let app = Router::new().route("/v1/execute", post(execute));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

async fn get(app: &TestApp, token: &str, path: &str) -> (StatusCode, Value) {
    let r = app
        .request(Method::GET, &format!("/api/v1{path}"), Some(token), None)
        .await;
    (r.status, r.body)
}

/// Every span of a run through the paged endpoint, `page` at a time.
async fn all_spans(app: &TestApp, token: &str, rid: &str, page: usize) -> Vec<Value> {
    let mut out = Vec::new();
    let mut after = String::new();
    loop {
        let (status, body) = get(
            app,
            token,
            &format!("/runs/{rid}/trace?limit={page}{after}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let spans = body["spans"].as_array().unwrap();
        assert!(spans.len() <= page);
        out.extend(spans.iter().cloned());
        match body["next"].as_str() {
            Some(next) => after = format!("&after={next}"),
            None => return out,
        }
    }
}

struct Traced {
    app: TestApp,
    token: String,
    gid: String,
    rid: String,
    /// The node run by the built-in executor, and the one run by the runtime.
    llm_node: String,
    agent_node: String,
    events: Vec<(String, Value)>,
}

async fn traced_run(pool: PgPool, agent_title: &str) -> Traced {
    let runtime = tracing_runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &runtime)]).await;
    let (token, _) = app.register("tracer@example.com").await;
    let gid = app.graph(&token, "Ship the quarterly report").await;
    let llm_node = app
        .node(
            &token,
            &gid,
            json!({"title": "Outline", "kind": "task", "agent_role": "writer"}),
        )
        .await;
    let agent_node = app
        .node(
            &token,
            &gid,
            json!({"title": agent_title, "kind": "research",
            "executor": "agent", "agent_role": "researcher"}),
        )
        .await;
    app.edge(&token, &gid, &llm_node, &agent_node).await;
    let mut rx = app.events(&gid);
    let r = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/runs"),
            Some(&token),
            Some(json!({})),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    let rid = r.body["id"].as_str().unwrap().to_owned();
    let events = collect_until(&mut rx, "run.finished").await;
    assert_eq!(
        events.last().unwrap().1["run"]["status"],
        "succeeded",
        "{:?}",
        events.last()
    );
    Traced {
        app,
        token,
        gid,
        rid,
        llm_node,
        agent_node,
        events,
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_twelve_turn_agent_run_produces_a_verifiable_chain(pool: PgPool) {
    let t = traced_run(pool.clone(), "Research").await;
    let (app, token, rid) = (&t.app, t.token.as_str(), t.rid.as_str());

    // Live: every span was fanned out before the run finished, in chain order.
    let live: Vec<&Value> = t
        .events
        .iter()
        .filter(|(e, _)| e == "trace.span")
        .map(|(_, d)| &d["span"])
        .collect();
    let seqs: Vec<i64> = live.iter().map(|s| s["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs, (0..live.len() as i64).collect::<Vec<_>>());
    assert_eq!(
        live[0]["prev_hash"],
        "0".repeat(64),
        "the chain starts from 32 zero bytes"
    );
    let finished = t
        .events
        .iter()
        .position(|(e, _)| e == "run.finished")
        .unwrap();
    let first_span = t
        .events
        .iter()
        .position(|(e, _)| e == "trace.span")
        .unwrap();
    assert!(first_span < finished);

    // Stored: the same spans, paged, in id order.
    let stored = all_spans(app, token, rid, 7).await;
    assert_eq!(stored.len(), live.len());
    let ids: Vec<&str> = stored.iter().map(|s| s["id"].as_str().unwrap()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "pages come in id order");
    assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len());
    let by_id: HashMap<&str, &Value> = stored
        .iter()
        .map(|s| (s["id"].as_str().unwrap(), s))
        .collect();
    for s in &live {
        assert_eq!(
            by_id[s["id"].as_str().unwrap()]["hash"],
            s["hash"],
            "stored as fanned out"
        );
    }

    // What each executor recorded.
    let of = |node: &str, kind: &str| -> Vec<&Value> {
        stored
            .iter()
            .filter(|s| s["node_id"] == node && s["kind"] == kind)
            .collect()
    };
    let runtime_turns: Vec<&Value> = of(&t.agent_node, "llm_call");
    assert_eq!(
        runtime_turns.len(),
        TURNS + 1,
        "12 turns and the sub-agent's one"
    );
    assert_eq!(of(&t.agent_node, "tool_call").len(), TOOL_CALLS);
    let (spawn,) = <[&Value; 1]>::try_from(of(&t.agent_node, "spawn"))
        .map(|[s]| (s,))
        .unwrap();
    for call in of(&t.agent_node, "tool_call") {
        let parent = by_id[call["parent_id"].as_str().unwrap()];
        assert_eq!(
            parent["kind"], "llm_call",
            "a tool call hangs under its turn"
        );
    }
    assert_eq!(
        by_id[spawn["parent_id"].as_str().unwrap()]["kind"],
        "llm_call"
    );
    let child = runtime_turns
        .iter()
        .find(|s| {
            s["principal"].as_str().is_some_and(|p| {
                p.starts_with("agent://") && p.ends_with("/Researcher/researcher-1")
            })
        })
        .unwrap();
    assert_eq!(
        child["parent_id"], spawn["id"],
        "the child's turn hangs under its spawn"
    );
    let researcher = runtime_turns[0]["agent_id"].as_str().unwrap().to_owned();
    assert!(
        runtime_turns
            .iter()
            .all(|s| s["agent_id"] == researcher.as_str() && s["run_id"] == rid)
    );
    assert!(
        stored.iter().all(|s| s["name"] != "AFTER"),
        "nothing after the result is read"
    );

    let builtin = of(&t.llm_node, "llm_call");
    assert_eq!(
        builtin.len(),
        1,
        "the built-in executor makes one call: one span"
    );
    assert_eq!(builtin[0]["attributes"]["tokens_in"], 100);
    assert_eq!(builtin[0]["attributes"]["stop_reason"], "end_turn");
    assert!(
        builtin[0]["principal"]
            .as_str()
            .is_some_and(|p| p.starts_with("agent://") && p.ends_with("/Writer")),
        "the built-in executor records the agent's principal: {}",
        builtin[0]["principal"]
    );
    for node in [&t.llm_node, &t.agent_node] {
        let reads = of(node, "data_read");
        assert_eq!(
            reads.len(),
            1,
            "the context given to each node is on record"
        );
        assert!(reads[0]["attributes"]["document_chunk_ids"].is_array());
        assert!(reads[0]["attributes"]["memory_ids"].is_array());
    }
    assert_eq!(
        of(&t.agent_node, "data_read")[0]["attributes"]["upstream_node_ids"],
        json!([t.llm_node])
    );

    // Verified and sealed: on the run and in the workspace's audit log.
    let (status, verified) = get(app, token, &format!("/runs/{rid}/trace/verify")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(verified["ok"], true, "{verified}");
    assert_eq!(verified["spans"], stored.len());
    let (_, run) = get(app, token, &format!("/runs/{rid}")).await;
    assert_eq!(run["trace_hash"], verified["hash"]);
    assert_eq!(verified["sealed_hash"], verified["hash"]);
    let last = live.last().unwrap();
    assert_eq!(
        run["trace_hash"], last["hash"],
        "the seal is the last span's hash"
    );
    let (_, workspaces) = get(app, token, "/workspaces").await;
    let wid = workspaces[0]["id"].as_str().unwrap();
    let (_, audit) = get(app, token, &format!("/workspaces/{wid}/audit")).await;
    let seal = audit
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["action"] == "trace_sealed")
        .expect("sealed");
    assert_eq!(seal["subject"], rid);
    assert!(
        seal["detail"]
            .as_str()
            .unwrap()
            .contains(run["trace_hash"].as_str().unwrap())
    );

    // Usage joins the trace.
    let rows: Vec<(Option<Uuid>, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT node_id, agent_id, span_id FROM llm_usage WHERE run_id = $1 AND purpose = 'node' ORDER BY node_id",
    )
    .bind(rid.parse::<Uuid>().unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    let llm_row = rows
        .iter()
        .find(|r| r.0 == Some(t.llm_node.parse().unwrap()))
        .unwrap();
    assert_eq!(
        llm_row.2.map(|s| s.to_string()),
        builtin[0]["id"].as_str().map(str::to_owned)
    );
    assert!(llm_row.1.is_some());
    let agent_row = rows
        .iter()
        .find(|r| r.0 == Some(t.agent_node.parse().unwrap()))
        .unwrap();
    assert_eq!(agent_row.1.map(|a| a.to_string()), Some(researcher.clone()));

    // The agent's activity, newest first.
    let (status, activity) = get(
        app,
        token,
        &format!("/agents/{researcher}/activity?limit=5"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{activity}");
    let activity = activity.as_array().unwrap();
    assert_eq!(activity.len(), 5);
    let started: Vec<&str> = activity
        .iter()
        .map(|s| s["started_at"].as_str().unwrap())
        .collect();
    let mut newest_first = started.clone();
    newest_first.sort_by(|a, b| b.cmp(a));
    assert_eq!(started, newest_first);
    assert!(
        activity
            .iter()
            .all(|s| s["agent_id"] == researcher.as_str())
    );
    let _ = &t.gid;
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_changed_row_is_named_by_verify(pool: PgPool) {
    let t = traced_run(pool.clone(), "Research").await;
    let (app, token, rid) = (&t.app, t.token.as_str(), t.rid.as_str());
    let stored = all_spans(app, token, rid, 1000).await;
    let victim = stored.iter().find(|s| s["name"] == "write_file").unwrap();
    let victim_id: Uuid = victim["id"].as_str().unwrap().parse().unwrap();

    // Flip one byte of the stored span: one character of its name.
    sqlx::query("UPDATE agent_spans SET name = 'write_filf' WHERE id = $1")
        .bind(victim_id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, verified) = get(app, token, &format!("/runs/{rid}/trace/verify")).await;
    assert_eq!(verified["ok"], false);
    assert_eq!(verified["broken"]["span_id"], victim["id"]);
    assert_eq!(verified["broken"]["seq"], victim["seq"]);
    assert!(
        verified["broken"]["reason"]
            .as_str()
            .unwrap()
            .contains("content")
    );

    // Put it back; then change an attribute instead, then forge a matching hash.
    sqlx::query("UPDATE agent_spans SET name = 'write_file' WHERE id = $1")
        .bind(victim_id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, verified) = get(app, token, &format!("/runs/{rid}/trace/verify")).await;
    assert_eq!(verified["ok"], true, "restored: {verified}");
    sqlx::query("UPDATE agent_spans SET attributes = jsonb_set(attributes, '{result_bytes}', '25') WHERE id = $1")
        .bind(victim_id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, verified) = get(app, token, &format!("/runs/{rid}/trace/verify")).await;
    assert_eq!(verified["broken"]["span_id"], victim["id"]);

    // Removing the last span leaves a chain that holds but no longer ends at the seal.
    sqlx::query("UPDATE agent_spans SET attributes = jsonb_set(attributes, '{result_bytes}', '24') WHERE id = $1")
        .bind(victim_id)
        .execute(&pool)
        .await
        .unwrap();
    let last = stored.iter().max_by_key(|s| s["seq"].as_i64()).unwrap();
    sqlx::query("DELETE FROM agent_spans WHERE id = $1")
        .bind(last["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let (_, verified) = get(app, token, &format!("/runs/{rid}/trace/verify")).await;
    assert_eq!(verified["ok"], false);
    assert!(
        verified["broken"]["reason"]
            .as_str()
            .unwrap()
            .contains("sealed"),
        "{verified}"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn hostile_span_lines_are_contained(pool: PgPool) {
    let t = traced_run(pool, "HOSTILE research").await;
    let (app, token, rid) = (&t.app, t.token.as_str(), t.rid.as_str());
    let stored = all_spans(app, token, rid, 1000).await;
    let mut of_node: Vec<&Value> = stored
        .iter()
        .filter(|s| s["node_id"] == t.agent_node)
        .collect();
    // In chain order: the order the backend received them.
    of_node.sort_by_key(|s| s["seq"].as_i64());
    // The backend's own spans frame what the runtime reported: what the node was
    // given first, the verdict on its output last, once the result arrived.
    let is_verdict = |s: &&Value| s["kind"] == "policy" && s["name"] == "evidence";
    assert!(is_verdict(of_node.last().unwrap()), "{of_node:?}");
    let runtime: Vec<&Value> = of_node
        .iter()
        .copied()
        .filter(|s| s["kind"] != "data_read" && !is_verdict(s))
        .collect();
    let names: Vec<&str> = runtime
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["claude-test", "read_file", "OVERSIZED", "BARE"],
        "{names:?}"
    );

    // A repeated id gets a fresh one and keeps the reported one on record.
    assert_ne!(runtime[0]["id"], runtime[1]["id"]);
    assert_eq!(runtime[1]["attributes"]["runtime_id"], runtime[0]["id"]);
    // An oversized line keeps its span but not its attributes.
    assert_eq!(runtime[2]["attributes"]["attributes_dropped"], true);
    assert!(runtime[2]["attributes"].get("padding").is_none());
    // A span without an id, times or agent still gets all of them from the backend: the
    // agent's principal, `agent://{workspace}/{name}`.
    assert!(
        runtime[3]["principal"]
            .as_str()
            .is_some_and(|p| p.starts_with("agent://") && p.ends_with("/Researcher")),
        "{}",
        runtime[3]["principal"]
    );
    assert!(runtime[3]["started_at"].is_string());
    // Unknown kinds and kinds only the backend may record are refused with a warning.
    let warnings: Vec<&str> = t
        .events
        .iter()
        .filter(|(e, d)| e == "node.log" && d["level"] == "warn")
        .map(|(_, d)| d["message"].as_str().unwrap())
        .collect();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("unknown kind `teleport`")),
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("unknown kind `approval`")),
        "{warnings:?}"
    );
    assert!(
        stored
            .iter()
            .all(|s| s["name"] != "AFTER" && s["name"] != "FORGED" && s["name"] != "UNKNOWN")
    );

    let (_, verified) = get(app, token, &format!("/runs/{rid}/trace/verify")).await;
    assert_eq!(verified["ok"], true, "{verified}");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_trace_is_private_and_bounded(pool: PgPool) {
    let t = traced_run(pool, "Research").await;
    let (app, token, rid) = (&t.app, t.token.as_str(), t.rid.as_str());
    let (outsider, _) = app.register("outsider@example.com").await;
    let researcher = all_spans(app, token, rid, 1000)
        .await
        .iter()
        .find_map(|s| s["agent_id"].as_str().map(str::to_owned))
        .unwrap();
    for path in [
        format!("/runs/{rid}/trace"),
        format!("/runs/{rid}/trace/verify"),
        format!("/agents/{researcher}/activity"),
        format!("/runs/{}/trace", Uuid::now_v7()),
        format!("/agents/{}/activity", Uuid::now_v7()),
    ] {
        let (status, body) = get(app, &outsider, &path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
    }
    // Limits are clamped, and a bad cursor or limit is refused, not a failure.
    let (_, body) = get(app, token, &format!("/runs/{rid}/trace?limit=0")).await;
    assert_eq!(body["spans"].as_array().unwrap().len(), 1);
    let (_, body) = get(app, token, &format!("/runs/{rid}/trace?limit=999999")).await;
    assert!(body["next"].is_null());
    for bad in ["after=not-a-uuid", "limit=abc", "unknown=1"] {
        let (status, _) = get(app, token, &format!("/runs/{rid}/trace?{bad}")).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
            "{bad}: {status}"
        );
    }
    let (_, body) = get(
        app,
        token,
        &format!("/agents/{researcher}/activity?limit=100000"),
    )
    .await;
    assert!(body.as_array().unwrap().len() <= 200);
}

/// Percentile of sorted microsecond samples.
fn pct(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

/// `make prove-trace`: 10,000 spans through one run's chain. Prints, for each
/// span, emit → `trace.span` fan-out as a subscriber of the graph sees it;
/// the time until the last span is durable in PostgreSQL; and how long
/// `GET /runs/{rid}/trace/verify` takes, and the recomputation alone.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
#[ignore = "a measurement: run with `make prove-trace`"]
async fn prove_trace(pool: PgPool) {
    use std::time::{Duration, Instant};

    use nexc::domain::trace::{SpanDraft, SpanKind, SpanStatus};
    use nexc::engine::lease::Lease;
    use nexc::engine::trace;

    const SPANS: usize = 10_000;
    let app = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = app.register("prover@example.com").await;
    let gid = app.graph(&token, "Prove the trace spine").await;
    let node = app.node(&token, &gid, json!({"title": "Work"})).await;
    let graph_id: Uuid = gid.parse().unwrap();
    let owner: Uuid = sqlx::query_scalar("SELECT owner_id FROM graphs WHERE id = $1")
        .bind(graph_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    // A run that is already running, so no dispatcher picks it up.
    let run_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runs (id, graph_id, owner_id, status, max_concurrency, started_at)
         VALUES ($1, $2, $3, 'running', 1, now())",
    )
    .bind(run_id)
    .bind(graph_id)
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();

    let mut rx = app.events(&gid);
    let sink = trace::start(
        app.state.clone(),
        run_id,
        graph_id,
        Lease::unfenced(run_id),
        trace::Tail::GENESIS,
    );
    let handle = sink.handle();
    let node_id: Uuid = node.parse().unwrap();
    let ids: Vec<Uuid> = (0..SPANS).map(|_| Uuid::now_v7()).collect();
    let index: HashMap<String, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.to_string(), i))
        .collect();
    let mut emitted = vec![Instant::now(); SPANS];
    let listener = tokio::spawn(async move {
        let mut seen = vec![None; SPANS];
        let mut count = 0;
        while count < SPANS {
            let frame = rx.recv().await.expect("no frame is lost");
            if &*frame.event != "trace.span" {
                continue;
            }
            let at = Instant::now();
            let start = frame.data.find("\"id\":\"").unwrap() + 6;
            let id = &frame.data[start..start + 36];
            seen[index[id]] = Some(at);
            count += 1;
        }
        seen
    });
    let began = Instant::now();
    for (i, id) in ids.iter().enumerate() {
        let now = Utc::now();
        emitted[i] = Instant::now();
        handle.emit(SpanDraft {
            id: *id,
            node_id,
            agent_id: None,
            principal: "prover".into(),
            parent_id: (i % 4 != 0).then(|| ids[i - i % 4]),
            kind: if i % 4 == 0 {
                SpanKind::LlmCall
            } else {
                SpanKind::ToolCall
            },
            name: if i % 4 == 0 {
                "claude-test".into()
            } else {
                "write_file".into()
            },
            started_at: now,
            ended_at: Some(now),
            status: SpanStatus::Ok,
            attributes: json!({"args_sha256": "0".repeat(64), "args_bytes": 812, "result_bytes": 24,
                "is_error": false, "duration_ns": 1_000_000}),
        });
        // Spans arrive one by one as agents work, not in one burst.
        tokio::task::yield_now().await;
    }
    let emitting = began.elapsed();
    let seen = tokio::time::timeout(Duration::from_secs(60), listener)
        .await
        .unwrap()
        .unwrap();
    drop(handle);
    let sealing = Instant::now();
    let sealed = sink.seal().await;
    let durable = sealing.elapsed();
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_spans WHERE run_id = $1")
        .bind(run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, SPANS as i64);
    assert_eq!(sealed.spans, SPANS as i64);
    sqlx::query(
        "UPDATE runs SET status = 'succeeded', finished_at = now(), trace_hash = $2 WHERE id = $1",
    )
    .bind(run_id)
    .bind(sealed.hash.unwrap().to_vec())
    .execute(&pool)
    .await
    .unwrap();

    let mut fanout: Vec<f64> = seen
        .iter()
        .zip(&emitted)
        .map(|(at, sent)| at.unwrap().duration_since(*sent).as_secs_f64() * 1e6)
        .collect();
    fanout.sort_by(f64::total_cmp);

    // Verify over HTTP (load 10,000 rows and recompute), five times; and the recomputation alone.
    let mut http = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        let (status, verified) = get(&app, &token, &format!("/runs/{run_id}/trace/verify")).await;
        http.push(started.elapsed().as_secs_f64() * 1e3);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(verified["ok"], true, "{verified}");
        assert_eq!(verified["spans"], SPANS);
    }
    http.sort_by(f64::total_cmp);
    let spans = nexc::repo::spans::chain(&pool, run_id).await.unwrap();
    let started = Instant::now();
    let result = nexc::domain::trace::verify(&spans, sealed.hash);
    let recompute = started.elapsed().as_secs_f64() * 1e3;
    assert!(result.ok);

    println!("\nprove-trace: {SPANS} spans in one run's chain");
    println!("| measure                                         | value |");
    println!("| ----------------------------------------------- | ----- |");
    println!(
        "| emit -> trace.span fan-out, p50                 | {:.0} µs |",
        pct(&fanout, 50.0)
    );
    println!(
        "| emit -> trace.span fan-out, p99                 | {:.0} µs |",
        pct(&fanout, 99.0)
    );
    println!(
        "| emit -> trace.span fan-out, max                 | {:.0} µs |",
        pct(&fanout, 100.0)
    );
    println!(
        "| emitting all spans (paced, one per task yield)  | {:.1} ms |",
        emitting.as_secs_f64() * 1e3
    );
    println!(
        "| last emit -> all spans durable in PostgreSQL    | {:.1} ms |",
        durable.as_secs_f64() * 1e3
    );
    println!(
        "| GET /runs/{{id}}/trace/verify, median of 5        | {:.1} ms |",
        pct(&http, 50.0)
    );
    println!(
        "| chain recomputation alone (C SHA-256 kernel)    | {:.1} ms |",
        recompute
    );
}

/// A run is sealed when it ends, even while a background task (memory
/// extraction) still holds a handle; what it emits afterwards is not chained.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn sealing_does_not_wait_for_handles_still_held(pool: PgPool) {
    use std::time::Duration;

    use nexc::domain::trace::{SpanDraft, SpanKind, SpanStatus};
    use nexc::engine::lease::Lease;
    use nexc::engine::trace;

    let app = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = app.register("sealer@example.com").await;
    let gid = app.graph(&token, "Seal").await;
    let graph_id: Uuid = gid.parse().unwrap();
    let owner: Uuid = sqlx::query_scalar("SELECT owner_id FROM graphs WHERE id = $1")
        .bind(graph_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let run_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runs (id, graph_id, owner_id, status, max_concurrency) VALUES ($1, $2, $3, 'running', 1)",
    )
    .bind(run_id)
    .bind(graph_id)
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();
    let draft = || SpanDraft {
        id: Uuid::now_v7(),
        node_id: Uuid::now_v7(),
        agent_id: None,
        principal: "system".into(),
        parent_id: None,
        kind: SpanKind::DataRead,
        name: "context".into(),
        started_at: Utc::now(),
        ended_at: None,
        status: SpanStatus::Ok,
        attributes: json!({}),
    };
    let sink = trace::start(
        app.state.clone(),
        run_id,
        graph_id,
        Lease::unfenced(run_id),
        trace::Tail::GENESIS,
    );
    let lingering = sink.handle();
    for _ in 0..3 {
        lingering.emit(draft());
    }
    let sealed = tokio::time::timeout(Duration::from_secs(5), sink.seal())
        .await
        .expect("sealed without waiting for the lingering handle");
    assert_eq!(sealed.spans, 3);
    lingering.emit(draft());
    tokio::time::sleep(Duration::from_millis(100)).await;
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_spans WHERE run_id = $1")
        .bind(run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 3, "nothing is chained after the seal");
}

/// A workspace transfer takes the trace with it as it was chained: on the
/// owner's database the run's spans still verify against its seal.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_transferred_trace_still_verifies(pool: PgPool) {
    let t = traced_run(pool.clone(), "Research").await;
    let (_, workspaces) = get(&t.app, &t.token, "/workspaces").await;
    let workspace: Uuid = workspaces[0]["id"].as_str().unwrap().parse().unwrap();
    let name = format!("nexc_trace_{}", Uuid::now_v7().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&pool)
        .await
        .unwrap();
    let base = std::env::var("DATABASE_URL").unwrap();
    let url = format!("{}/{name}", base.rsplit_once('/').unwrap().0);
    let report = nexc::engine::transfer::copy_workspace(&pool, &url, workspace)
        .await
        .unwrap();
    let spans = report.iter().find(|r| r.table == "agent_spans").unwrap();
    assert!(
        spans.read > 50 && spans.written == spans.read,
        "{} of {}",
        spans.written,
        spans.read
    );

    let target = PgPool::connect(&url).await.unwrap();
    let run: Uuid = t.rid.parse().unwrap();
    let sealed: Vec<u8> = sqlx::query_scalar("SELECT trace_hash FROM runs WHERE id = $1")
        .bind(run)
        .fetch_one(&target)
        .await
        .unwrap();
    let chain = nexc::repo::spans::chain(&target, run).await.unwrap();
    let verified = nexc::domain::trace::verify(&chain, Some(sealed.try_into().unwrap()));
    assert!(verified.ok, "{:?}", verified.broken);
    assert_eq!(verified.spans, spans.read);
    target.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {name}")))
        .execute(&pool)
        .await
        .unwrap();
}
