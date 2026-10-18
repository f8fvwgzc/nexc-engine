//! The AgentSecOps console (phase 5): the overview counts what the member
//! may see and nothing more, the trace explorer narrows and pages across
//! runs, the live feed carries every graph's spans and alerts, and an agent
//! made from a role template arrives with its scopes, its policy and its score.
#![forbid(unsafe_code)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::post;
use chrono::Utc;
use common::{RUNTIME_TOKEN, TestApp, collect_until};
use http_body_util::BodyExt;
use nexc::domain::alert::AlertKind;
use nexc::domain::policy::PolicyVersions;
use nexc::domain::policy_document::Severity;
use nexc::repo;
use nexc::repo::alerts::NewAlert;
use nexc::repo::approvals::ApprovalRequest;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

/// A time as a query parameter (`Z`, not `+00:00`, which a query string would read as a space).
fn iso(at: chrono::DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn span(kind: &str, name: &str, agent: &str, status: &str, attrs: Value) -> Value {
    json!({
        "type": "span", "id": Uuid::now_v7(), "parent_id": null, "kind": kind, "name": name,
        "agent": agent, "started_at": now(), "ended_at": now(), "status": status, "attributes": attrs,
    })
}

/// A stand-in runtime: one turn and one `write_file` call per node; a node
/// whose title says `DENY` also reports a refused tool call, one that says
/// `EGRESS` a call that reached a partner host.
async fn runtime() -> (String, Arc<Mutex<Vec<Value>>>) {
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
            let mut lines = vec![
                span(
                    "llm_call",
                    "claude-test",
                    &agent,
                    "ok",
                    json!({"provider": "anthropic", "model": "claude-test", "turn": 1,
                        "tokens_in": 120, "tokens_out": 40}),
                ),
                span(
                    "tool_call",
                    "write_file",
                    &agent,
                    "ok",
                    json!({"tool": "write_file", "result_bytes": 12}),
                ),
            ];
            if title.contains("DENY") {
                lines.push(span(
                    "policy",
                    "run_python",
                    &agent,
                    "error",
                    json!({"decision": "deny", "rule": "tools.allow", "action": "tool_call",
                        "subject": "run_python", "tool": "run_python",
                        "reason": "tool run_python is not in the allow-list", "policy_version": 0}),
                ));
            }
            if title.contains("EGRESS") {
                lines.push(span(
                    "tool_call",
                    "http_post",
                    &agent,
                    "ok",
                    json!({"tool": "http_post", "egress_host": "api.partner.example",
                        "payload_sha256": "cd".repeat(32), "payload_bytes": 512}),
                ));
            }
            lines.push(json!({"type": "tokens", "input": 10, "output": 5}));
            lines.push(
                json!({"type": "result", "output": format!("[runtime] {title}"),
                "tokens_in": 10, "tokens_out": 5}),
            );
            let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
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

async fn agent(app: &TestApp, token: &str, name: &str, role: &str) -> Value {
    let (status, agent) = call(
        app,
        Method::POST,
        "/agents",
        token,
        Some(json!({"name": name, "role": role, "runtime": "python",
            "purpose": format!("Handles {role} work in tests.")})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    agent
}

/// Runs the graph (`force` skips the result cache) and returns the run id
/// once `run.finished` arrived.
async fn run(app: &TestApp, token: &str, gid: &str, force: bool) -> String {
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
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    rid
}

/// Three nodes for the scribe: a plain one, one that is refused a tool, one that calls out.
async fn graph_of_three(app: &TestApp, token: &str) -> String {
    let gid = app.graph(token, "Report on customs duties").await;
    for title in ["Plain write", "DENY run_python", "EGRESS post"] {
        app.node(
            token,
            &gid,
            json!({"title": title, "kind": "research", "executor": "agent", "agent_role": "scribe"}),
        )
        .await;
    }
    gid
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_overview_counts_what_the_member_may_see(pool: PgPool) {
    let (runtime, _) = runtime().await;
    let app = TestApp::new(pool.clone(), &[("NEXC_RUNTIME_URL", &runtime)]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (member, _) = app.register("member@example.com").await;
    let (guest, _) = app.register("guest@example.com").await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let wid = workspace(&app, &owner).await;
    add_member(&app, &owner, &wid, "member@example.com", "member").await;
    add_member(&app, &owner, &wid, "guest@example.com", "guest").await;
    let scribe = agent(&app, &owner, "Scribe", "scribe").await;
    let scribe_id = scribe["id"].as_str().unwrap().to_owned();
    let scribe_uuid: Uuid = scribe_id.parse().unwrap();
    let scout = agent(&app, &owner, "Scout", "scout").await;
    let scout_id = scout["id"].as_str().unwrap();
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/agents/{scout_id}/quarantine"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        &owner,
        Some(json!({"scope": "workspace", "document": {"alerts": {"denials_per_run": 5}}})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Nothing happened yet: every count is zero and nothing is sealed.
    let (status, empty) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/overview"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["runs_today"]["total"], 0);
    assert_eq!(empty["spans_today"], 0);
    assert!(empty["last_seal"].is_null());
    assert_eq!(empty["top_risk"].as_array().unwrap().len(), 0);

    let gid = graph_of_three(&app, &owner).await;
    let rid = run(&app, &owner, &gid, false).await;
    let run_uuid: Uuid = rid.parse().unwrap();
    let graph_uuid: Uuid = gid.parse().unwrap();
    let wid_uuid: Uuid = wid.parse().unwrap();
    let principal = format!("agent://{wid}/Scribe");

    // What only the engine writes is written here directly: an approval
    // nobody decided, an alert nobody acknowledged, a risk score.
    let versions = PolicyVersions::default();
    repo::approvals::insert(
        &pool,
        &ApprovalRequest {
            id: Uuid::now_v7(),
            workspace_id: wid_uuid,
            run_id: run_uuid,
            node_id: Uuid::now_v7(),
            graph_id: graph_uuid,
            agent_id: Some(scribe_uuid),
            principal: &principal,
            span_id: Uuid::now_v7(),
            tool: "make_docx",
            args_sha256: None,
            rule: "approvals.tools",
            reason: "test",
            approver_ids: &[],
            expires_at: Utc::now() + chrono::Duration::minutes(10),
            policy_versions: &versions,
        },
    )
    .await
    .unwrap();
    repo::alerts::insert(
        &pool,
        &NewAlert {
            workspace_id: wid_uuid,
            agent_id: Some(scribe_uuid),
            principal: &principal,
            run_id: Some(run_uuid),
            node_id: None,
            graph_id: Some(graph_uuid),
            span_id: None,
            kind: AlertKind::Risk,
            severity: Severity::High,
            rule: "alerts.risk_score",
            title: "risk score 7.5 over 5.0",
            detail: "2 hosts it never called",
        },
    )
    .await
    .unwrap();
    repo::agents::set_risk(&pool, scribe_uuid, 7.5, &["2 hosts it never called".into()])
        .await
        .unwrap();

    let (status, overview) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/overview"),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    let agents = &overview["agents"];
    // The default organisation's five plus the two made here, one of them quarantined.
    assert_eq!(agents["total"], 7, "{agents}");
    assert_eq!(agents["quarantined"], 1);
    assert_eq!(agents["active"], 6);
    assert_eq!(overview["runs_today"]["total"], 1);
    assert_eq!(overview["runs_today"]["succeeded"], 1);
    assert_eq!(overview["runs_week"]["total"], 1);
    // Three nodes: a context read, a turn and a write each, plus the denial and the egress.
    assert!(overview["spans_today"].as_i64().unwrap() >= 9, "{overview}");
    assert_eq!(
        overview["denials_today"],
        json!([{"rule": "tools.allow", "count": 1}])
    );
    assert_eq!(overview["denials_fortnight"][0]["rule"], "tools.allow");
    assert_eq!(overview["approvals_open"], 1);
    assert_eq!(
        overview["alerts_open"],
        json!([{"severity": "high", "count": 1}])
    );
    assert_eq!(overview["artifacts_quarantined"], 0);
    let top = overview["top_risk"].as_array().unwrap();
    assert_eq!(top.len(), 1, "{top:?}");
    assert_eq!(top[0]["agent_id"], scribe_id);
    assert_eq!(top[0]["risk_score"], 7.5);
    assert_eq!(top[0]["risk_reasons"][0], "2 hosts it never called");
    assert_eq!(
        overview["policy_versions"][0]["scope"], "workspace",
        "{overview}"
    );
    assert_eq!(overview["policy_versions"][0]["version"], 1);
    assert_eq!(overview["last_seal"]["run_id"], rid);
    assert_eq!(
        overview["last_seal"]["trace_hash"].as_str().unwrap().len(),
        64
    );
    // The trigger kept the score for the chart.
    let history = overview["risk_history"].as_array().unwrap();
    assert_eq!(history.len(), 1, "{history:?}");
    assert_eq!(history[0]["agent_id"], scribe_id);
    assert_eq!(history[0]["score"], 7.5);
    assert_eq!(history[0]["day"].as_str().unwrap().len(), 10);

    // A member sees the same; a guest may not look; an outsider finds nothing.
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/overview"),
        &member,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/overview"),
        &guest,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/overview"),
        &outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{}/secops/overview", Uuid::now_v7()),
        &owner,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_explorer_narrows_and_pages_across_runs(pool: PgPool) {
    let (runtime, _) = runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &runtime)]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (guest, _) = app.register("guest@example.com").await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let wid = workspace(&app, &owner).await;
    add_member(&app, &owner, &wid, "guest@example.com", "guest").await;
    let scribe = agent(&app, &owner, "Scribe", "scribe").await;
    let scribe_id = scribe["id"].as_str().unwrap();
    let gid = graph_of_three(&app, &owner).await;
    let first = run(&app, &owner, &gid, false).await;
    let second = run(&app, &owner, &gid, true).await;
    let search = |query: String, token: String| {
        let app = &app;
        let wid = wid.clone();
        async move {
            call(
                app,
                Method::GET,
                &format!("/workspaces/{wid}/secops/spans?{query}"),
                &token,
                None,
            )
            .await
        }
    };

    // Denials, with the rule and the tool they were about.
    let (status, page) = search("kind=policy&decision=deny".into(), owner.clone()).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let denials = page["spans"].as_array().unwrap();
    assert_eq!(denials.len(), 2, "one per run: {denials:?}");
    assert_eq!(denials[0]["rule"], "tools.allow");
    assert_eq!(denials[0]["tool"], "run_python");
    assert_eq!(denials[0]["decision"], "deny");
    assert_eq!(denials[0]["status"], "error");
    assert_eq!(denials[0]["agent_id"], scribe_id);
    assert_eq!(denials[0]["graph_id"], gid);
    assert!(page["next"].is_null());
    // Newest first: the second run's denial comes before the first run's.
    assert_eq!(denials[0]["run_id"], second);
    assert_eq!(denials[1]["run_id"], first);

    // The host a tool reached, the tool itself, the run, the agent, the time.
    let (_, page) = search("host=api.partner.example".into(), owner.clone()).await;
    let out = page["spans"].as_array().unwrap();
    assert_eq!(out.len(), 2, "{out:?}");
    assert_eq!(out[0]["tool"], "http_post");
    assert_eq!(out[0]["host"], "api.partner.example");
    let (_, page) = search("tool=write_file".into(), owner.clone()).await;
    assert_eq!(page["spans"].as_array().unwrap().len(), 6);
    let (_, page) = search(format!("run={first}"), owner.clone()).await;
    let of_first = page["spans"].as_array().unwrap();
    assert!(of_first.iter().all(|s| s["run_id"] == first));
    assert!(of_first.len() >= 9, "{}", of_first.len());
    let (_, page) = search(format!("agent={scribe_id}&kind=llm_call"), owner.clone()).await;
    let turns = page["spans"].as_array().unwrap();
    assert_eq!(turns.len(), 6);
    assert_eq!(turns[0]["model"], "claude-test");
    assert_eq!(turns[0]["tokens_in"], 120);
    let (_, page) = search(
        format!("from={}", iso(Utc::now() + chrono::Duration::hours(1))),
        owner.clone(),
    )
    .await;
    assert_eq!(page["spans"].as_array().unwrap().len(), 0);
    let (_, page) = search(format!("agent={}", Uuid::now_v7()), owner.clone()).await;
    assert_eq!(page["spans"].as_array().unwrap().len(), 0);

    // Paging: two at a time, no span twice, until `next` is null.
    let (_, all) = search("limit=500".into(), owner.clone()).await;
    let total = all["spans"].as_array().unwrap().len();
    let mut seen = Vec::new();
    let mut before: Option<String> = None;
    for _ in 0..100 {
        let query = match &before {
            Some(id) => format!("limit=2&before={id}"),
            None => "limit=2".into(),
        };
        let (status, page) = search(query, owner.clone()).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let spans = page["spans"].as_array().unwrap();
        assert!(spans.len() <= 2);
        for s in spans {
            let id = s["id"].as_str().unwrap().to_owned();
            assert!(!seen.contains(&id), "span {id} paged twice");
            seen.push(id);
        }
        match page["next"].as_str() {
            Some(next) => before = Some(next.to_owned()),
            None => break,
        }
    }
    assert_eq!(seen.len(), total);

    // Bad input is refused, not served.
    for query in [
        "kind=nonsense",
        "decision=maybe",
        "bogus=1",
        "limit=abc",
        "run=not-a-uuid",
        "from=yesterday",
        "tool=",
    ] {
        let (status, body) = search(query.into(), owner.clone()).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
            "{query}: {status} {body}"
        );
    }
    let later = iso(Utc::now() + chrono::Duration::hours(1));
    let earlier = iso(Utc::now());
    let (status, _) = search(format!("from={later}&to={earlier}"), owner.clone()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = search("limit=100000".into(), owner.clone()).await;
    assert_eq!(status, StatusCode::OK, "an oversized limit is clamped");

    // A guest may not look; an outsider finds nothing.
    let (status, _) = search(String::new(), guest).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = search(String::new(), outsider).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Reads frames from an SSE body until one named `wanted` arrives (or 30 s pass).
async fn read_until(body: &mut Body, wanted: &str) -> Vec<(String, String)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut text = String::new();
    let mut frames = Vec::new();
    loop {
        let frame = tokio::time::timeout_at(deadline, body.frame())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {wanted}; got {frames:?}"))
            .expect("the stream is open")
            .expect("a frame");
        if let Some(data) = frame.data_ref() {
            text.push_str(&String::from_utf8_lossy(data));
        }
        while let Some(end) = text.find("\n\n") {
            let block = text[..end].to_owned();
            text = text[end + 2..].to_owned();
            let mut event = String::from("message");
            let mut data = String::new();
            for line in block.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    event = v.trim().to_owned();
                } else if let Some(v) = line.strip_prefix("data:") {
                    data.push_str(v.trim());
                } else if line.starts_with("retry:") {
                    event = "retry".into();
                }
            }
            let done = event == wanted;
            frames.push((event, data));
            if done {
                return frames;
            }
        }
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_live_feed_carries_every_graphs_spans(pool: PgPool) {
    let (runtime, _) = runtime().await;
    let app = TestApp::new(pool, &[("NEXC_RUNTIME_URL", &runtime)]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (guest, _) = app.register("guest@example.com").await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let wid = workspace(&app, &owner).await;
    add_member(&app, &owner, &wid, "guest@example.com", "guest").await;
    agent(&app, &owner, "Scribe", "scribe").await;
    let gid = graph_of_three(&app, &owner).await;

    let open = |token: &str| {
        Request::builder()
            .method(Method::GET)
            .uri(format!("/api/v1/workspaces/{wid}/secops/events"))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    };
    let response = tower::ServiceExt::oneshot(app.router.clone(), open(&owner))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    let mut body = response.into_body();
    let first = read_until(&mut body, "heartbeat").await;
    assert_eq!(first[0].0, "retry");

    // A run of a graph that existed when the feed opened: its spans arrive as they happen.
    let (status, started) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let frames = read_until(&mut body, "trace.span").await;
    let (event, data) = frames.last().unwrap();
    assert_eq!(event, "trace.span");
    let data: Value = serde_json::from_str(data).unwrap();
    assert_eq!(data["span"]["run_id"], started["id"]);
    assert!(data["span"]["kind"].is_string());
    // Only spans and alerts are forwarded: node status and output never are.
    assert!(
        frames
            .iter()
            .all(|(e, _)| ["retry", "heartbeat", "trace.span", "alert"].contains(&e.as_str())),
        "{frames:?}"
    );
    drop(body);

    // A guest may not listen; an outsider finds nothing.
    let response = tower::ServiceExt::oneshot(app.router.clone(), open(&guest))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = tower::ServiceExt::oneshot(app.router.clone(), open(&outsider))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = tower::ServiceExt::oneshot(
        app.router.clone(),
        Request::builder()
            .method(Method::GET)
            .uri(format!("/api/v1/workspaces/{wid}/secops/events"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn an_agent_made_from_a_template_has_its_scopes_policy_and_score(pool: PgPool) {
    // The offline demo provider: its runtime answers as `nexc-demo` whatever the agent says.
    let app = TestApp::new(pool, &[("NEXC_LLM_PROVIDER", "demo")]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (guest, _) = app.register("guest@example.com").await;
    let (outsider, _) = app.register("outsider@example.com").await;
    let wid = workspace(&app, &owner).await;
    add_member(&app, &owner, &wid, "guest@example.com", "guest").await;

    let (status, templates) = call(&app, Method::GET, "/agent-templates", &owner, None).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = templates
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["analyst", "researcher", "reviewer"]);
    assert_eq!(templates[0]["questions"].as_array().unwrap().len(), 3);

    let (status, card) = call(
        &app,
        Method::POST,
        "/agents/from-template/analyst",
        &owner,
        Some(json!({"workspace_id": wid})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{card}");
    let agent = &card["agent"];
    let aid = agent["id"].as_str().unwrap().to_owned();
    assert_eq!(agent["name"], "Analyst");
    assert_eq!(agent["role"], "analyst");
    assert_eq!(agent["runtime"], "python");
    assert_eq!(agent["principal_id"], format!("agent://{wid}/Analyst"));
    let tools = agent["capabilities"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t == "make_docx"), "{tools:?}");
    assert_eq!(
        agent["capabilities"]["data"]["labels"],
        json!(["public", "internal"])
    );
    assert_eq!(agent["capabilities"]["egress_hosts"], json!([]));
    // The model allow-list is pinned to the model the agent was given, and admits the demo
    // runtime's own name since that is the workspace's provider.
    assert_eq!(
        agent["capabilities"]["models"],
        json!([agent["model"], "nexc-demo"])
    );
    assert_eq!(card["owner"]["name"], "Test");
    assert!(card["recent_spans"].as_array().unwrap().is_empty());
    // The fake model answered the three questions; they are scored and kept.
    let evaluation = &card["evaluation"];
    assert_eq!(evaluation["template"], "analyst");
    assert_eq!(evaluation["total"], 3, "{evaluation}");
    let score = evaluation["score"].as_i64().unwrap();
    assert!((0..=3).contains(&score));
    assert_eq!(evaluation["model"], agent["model"]);
    let questions = evaluation["questions"].as_array().unwrap();
    assert_eq!(questions.len(), 3);
    assert!(
        questions
            .iter()
            .all(|q| q["answer"].as_str().is_some_and(|a| !a.is_empty()))
    );
    assert_eq!(
        questions.iter().filter(|q| q["passed"] == true).count() as i64,
        score
    );
    assert!(app.fake.calls.load(std::sync::atomic::Ordering::SeqCst) >= 3);

    // The card reads the score back; the template's policy is the agent's version 1.
    let (_, read) = call(&app, Method::GET, &format!("/agents/{aid}"), &owner, None).await;
    assert_eq!(read["evaluation"]["total"], 3);
    assert_eq!(read["evaluation"]["score"], score);
    let (_, versions) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/policies?scope=agent&scope_id={aid}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(versions.as_array().unwrap().len(), 1, "{versions}");
    assert_eq!(versions[0]["version"], 1);
    assert_eq!(versions[0]["document"]["alerts"]["denials_per_run"], 1);
    let (_, resolved) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/policies/resolve?agent_id={aid}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(resolved["origins"]["alerts.denials_per_run"], "agent");
    assert_eq!(resolved["origins"]["dlp.secrets"], "agent");
    assert_eq!(resolved["levels"][0]["scope"], "agent");
    let (_, entries) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/audit?limit=50"),
        &owner,
        None,
    )
    .await;
    let changed: Vec<&Value> = entries
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["action"] == "policy_changed")
        .collect();
    assert_eq!(changed.len(), 1, "{entries}");
    assert_eq!(changed[0]["subject"], format!("agent://{wid}/Analyst"));
    assert!(
        changed[0]["detail"]
            .as_str()
            .unwrap()
            .contains("template analyst")
    );
    // The questions were paid for on the record.
    let (_, usage) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/usage?days=1"),
        &owner,
        None,
    )
    .await;
    let evaluated = usage["by_purpose"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == "evaluation")
        .unwrap_or_else(|| panic!("{usage}"));
    assert!(evaluated["tokens_in"].as_i64().unwrap() > 0);

    // The same name again is a conflict; another name is another agent, and
    // nobody is asked anything when told not to.
    let (status, _) = call(
        &app,
        Method::POST,
        "/agents/from-template/analyst",
        &owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, second) = call(
        &app,
        Method::POST,
        "/agents/from-template/Reviewer",
        &owner,
        Some(json!({"name": "Second opinion", "evaluate": false})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert_eq!(second["agent"]["role"], "reviewer");
    assert!(second["evaluation"].is_null());
    // The reviewer's policy denies what its capabilities do not list anyway.
    let (_, resolved) = call(
        &app,
        Method::GET,
        &format!(
            "/workspaces/{wid}/policies/resolve?agent_id={}",
            second["agent"]["id"].as_str().unwrap()
        ),
        &owner,
        None,
    )
    .await;
    assert!(
        resolved["rules"]["tools_denied"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t == "make_docx"),
        "{resolved}"
    );

    // Unknown templates, guests, outsiders and bad bodies.
    let (status, _) = call(
        &app,
        Method::POST,
        "/agents/from-template/plumber",
        &owner,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::POST,
        "/agents/from-template/analyst",
        &guest,
        Some(json!({"workspace_id": wid})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::POST,
        "/agents/from-template/analyst",
        &outsider,
        Some(json!({"workspace_id": wid})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::POST,
        "/agents/from-template/analyst",
        &owner,
        Some(json!({"name": "x".repeat(500)})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(
        &app,
        Method::POST,
        "/agents/from-template/analyst",
        &owner,
        Some(json!({"team_id": Uuid::now_v7(), "name": "Teamed"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
