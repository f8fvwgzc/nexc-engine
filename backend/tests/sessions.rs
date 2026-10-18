//! Sessions: a run as a tree of nodes, agents and sub-agents with tokens
//! and cost rolled up at every level (`GET /runs/{id}/sessions`), and the
//! workspace's runs with the same rollups, narrowed and paged
//! (`GET /workspaces/{wid}/sessions`).
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
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

async fn agent(app: &TestApp, token: &str, name: &str, role: &str) -> Value {
    let (status, agent) = call(
        app,
        Method::POST,
        "/agents",
        token,
        Some(json!({
            "name": name, "role": role, "runtime": "python",
            "purpose": format!("Handles {role} work in tests."),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    agent
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

/// Starts a run of a one-node graph and waits for it to finish; the run id.
async fn finished_run(app: &TestApp, token: &str, title: &str, role: &str) -> (String, String) {
    let gid = app.graph(token, "Prove the tree").await;
    app.node(
        token,
        &gid,
        json!({"title": title, "kind": "task", "executor": "agent", "agent_role": role}),
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
    collect_until(&mut rx, "run.finished").await;
    (gid, rid)
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn nested_spawns_become_a_tree_with_rollups_at_every_level(pool: PgPool) {
    let (url, _) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    let analyst = agent(&app, &token, "Analyst", "analyst").await;
    let (gid, rid) = finished_run(&app, &token, "TREE of work", "analyst").await;

    let (status, session) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/sessions"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["run_id"], rid);
    assert_eq!(session["graph_id"], gid);
    assert_eq!(session["status"], "succeeded");
    assert_eq!(session["workspace_id"], wid);
    assert!(session["duration_ms"].as_i64().unwrap() >= 0);
    assert_eq!(session["recording"], false, "off by default");
    let nodes = session["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];
    assert_eq!(node["title"], "TREE of work");
    assert_eq!(node["status"], "succeeded");
    assert_eq!(node["executor"], "agent");
    assert_eq!(
        node["context_span_ids"].as_array().unwrap().len(),
        1,
        "the data_read span"
    );
    // The node: 4 turns, 3 tool calls, 1 denial, the tokens of every turn.
    let rollup = &node["rollup"];
    assert_eq!(rollup["llm_calls"], 4, "{rollup}");
    assert_eq!(rollup["tool_calls"], 3);
    assert_eq!(rollup["denials"], 1);
    assert_eq!(rollup["tokens_in"], 100);
    assert_eq!(rollup["tokens_out"], 10);
    assert_eq!(rollup["tokens_cached"], 10);
    assert!(rollup["cost_usd"].as_f64().unwrap() > 0.0, "{rollup}");
    // The root agent, its researcher, the researcher's qa.
    let root = &node["agents"][0];
    assert_eq!(root["principal"], format!("agent://{wid}/Analyst"));
    assert_eq!(root["agent_id"], analyst["id"]);
    assert_eq!(root["depth"], 0);
    assert!(root["spawn_span_id"].is_null());
    assert_eq!(root["status"], "succeeded");
    assert_eq!(root["own"]["llm_calls"], 2);
    assert_eq!(root["own"]["tool_calls"], 1);
    assert_eq!(root["total"]["llm_calls"], 4);
    assert_eq!(root["total"]["tokens_in"], 100);
    assert_eq!(root["total"]["denials"], 1);
    assert_eq!(root["cancellable"], false);
    let researcher = &root["children"][0];
    assert_eq!(
        researcher["principal"],
        format!("agent://{wid}/Analyst/researcher-1")
    );
    assert_eq!(researcher["role"], "researcher");
    assert_eq!(researcher["depth"], 1);
    assert_eq!(researcher["parent_principal"], root["principal"]);
    assert!(researcher["spawn_span_id"].is_string());
    assert_eq!(researcher["status"], "succeeded");
    assert_eq!(researcher["own"]["llm_calls"], 1);
    assert_eq!(researcher["own"]["denials"], 1);
    assert_eq!(researcher["own"]["tokens_cached"], 10);
    assert_eq!(researcher["total"]["llm_calls"], 2);
    assert_eq!(researcher["total"]["tool_calls"], 2);
    assert_eq!(researcher["total"]["tokens_in"], 70);
    assert!(researcher["duration_ms"].as_i64().unwrap() >= 0);
    let qa = &researcher["children"][0];
    assert_eq!(qa["role"], "qa");
    assert_eq!(qa["depth"], 2);
    assert_eq!(qa["own"]["tokens_in"], 20);
    assert_eq!(qa["own"]["tool_calls"], 1);
    assert_eq!(qa["span_ids"].as_array().unwrap().len(), 2);
    assert!(qa["children"].as_array().unwrap().is_empty());
    // Costs add up: the subtrees sum to the node, the nodes to the run.
    let sum = root["own"]["cost_usd"].as_f64().unwrap()
        + researcher["own"]["cost_usd"].as_f64().unwrap()
        + qa["own"]["cost_usd"].as_f64().unwrap();
    assert!(
        (sum - rollup["cost_usd"].as_f64().unwrap()).abs() < 1e-9,
        "{sum} vs {rollup}"
    );
    assert!((session["rollup"]["cost_usd"].as_f64().unwrap() - sum).abs() < 1e-9);
    let models = session["by_model"].as_array().unwrap();
    assert_eq!(models.len(), 2, "{models:?}");
    assert_eq!(
        models
            .iter()
            .map(|m| m["llm_calls"].as_i64().unwrap())
            .sum::<i64>(),
        4
    );
    assert!(
        models
            .iter()
            .any(|m| m["model"] == "claude-sonnet-5" && m["tokens_cached"] == 10)
    );
    // The span ids open the spans of the trace.
    let (status, trace) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/trace?limit=100"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = trace["spans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    for id in qa["span_ids"].as_array().unwrap() {
        assert!(
            ids.contains(&id.as_str().unwrap()),
            "{id} is a span of the run"
        );
    }
    assert!(ids.contains(&researcher["spawn_span_id"].as_str().unwrap()));

    // An outsider does not see the run; a stranger's id is not found.
    let (outsider, _) = app.register("outsider@example.com").await;
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/sessions"),
        &outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/runs/{}/sessions", uuid::Uuid::now_v7()),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_waiting_approval_shows_on_the_tree_until_it_is_decided(pool: PgPool) {
    let (url, _) = fake_grpc_runtime().await;
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
    agent(&app, &token, "Scribe", "scribe").await;
    let gid = app.graph(&token, "Prove the wait").await;
    app.node(
        &token,
        &gid,
        json!({"title": "APPROVAL report", "kind": "task", "executor": "agent", "agent_role": "scribe"}),
    )
    .await;
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
    let mut seen = collect_until(&mut rx, "node.status").await;
    while !seen
        .iter()
        .any(|(e, d)| e == "node.status" && d["status"] == "waiting_approval")
    {
        seen.extend(collect_until(&mut rx, "node.status").await);
    }
    let (status, session) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/sessions"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["status"], "running");
    let node = &session["nodes"][0];
    assert_eq!(node["status"], "waiting_approval");
    assert_eq!(node["agents"][0]["status"], "waiting_approval", "{node}");
    assert!(node["agents"][0]["ended_at"].is_null());
    assert_eq!(node["agents"][0]["own"]["llm_calls"], 1);
    // The list says the run is live, with the spans so far.
    let (status, page) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/sessions?status=running"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["sessions"][0]["run_id"], rid);
    assert!(page["sessions"][0]["rollup"]["spans"].as_i64().unwrap() >= 2);

    let (_, pending) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/approvals?status=pending"),
        &token,
        None,
    )
    .await;
    let approval_id = pending[0]["id"].as_str().unwrap().to_owned();
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/approvals/{approval_id}/approve"),
        &token,
        Some(json!({"reason": "go ahead"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    collect_until(&mut rx, "run.finished").await;
    let (_, session) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/sessions"),
        &token,
        None,
    )
    .await;
    let agent_row = &session["nodes"][0]["agents"][0];
    assert_eq!(agent_row["status"], "succeeded", "{agent_row}");
    assert!(agent_row["ended_at"].is_string());
    let kinds: usize = agent_row["span_ids"].as_array().unwrap().len();
    assert!(
        kinds >= 4,
        "turn, request, decision and the tool call: {agent_row}"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_list_narrows_pages_and_keeps_its_bounds(pool: PgPool) {
    let (url, _) = fake_grpc_runtime().await;
    let app = grpc_app(pool, &url).await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    let analyst = agent(&app, &token, "Analyst", "analyst").await;
    let scribe = agent(&app, &token, "Scribe", "scribe").await;
    let (_, first) = finished_run(&app, &token, "TREE one", "analyst").await;
    let (_, second) = finished_run(&app, &token, "Plain note", "scribe").await;
    let (_, third) = finished_run(&app, &token, "DROP it", "scribe").await;

    let ws = format!("/workspaces/{wid}/sessions");
    let (status, page) = call(&app, Method::GET, &ws, &token, None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let rows = page["sessions"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| r["run_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [third.as_str(), second.as_str(), first.as_str()],
        "newest first"
    );
    assert!(page["next"].is_null());
    let tree_row = &rows[2];
    assert_eq!(tree_row["status"], "succeeded");
    assert_eq!(tree_row["nodes"], 1);
    assert_eq!(tree_row["agents"][0]["name"], "Analyst");
    assert_eq!(tree_row["agents"][0]["id"], analyst["id"]);
    assert_eq!(tree_row["rollup"]["llm_calls"], 4);
    assert_eq!(tree_row["rollup"]["tool_calls"], 3);
    assert_eq!(tree_row["rollup"]["denials"], 1);
    assert_eq!(tree_row["rollup"]["tokens_in"], 100);
    assert_eq!(tree_row["rollup"]["tokens_cached"], 10);
    assert!(tree_row["rollup"]["cost_usd"].as_f64().unwrap() > 0.0);
    assert!(tree_row["duration_ms"].as_i64().unwrap() >= 0);
    assert_eq!(
        rows[0]["status"], "failed",
        "the dropped stream failed its run"
    );
    assert_eq!(rows[0]["agents"][0]["id"], scribe["id"]);

    // Narrowed by agent, by status and by time.
    let (_, by_agent) = call(
        &app,
        Method::GET,
        &format!("{ws}?agent={}", scribe["id"].as_str().unwrap()),
        &token,
        None,
    )
    .await;
    assert_eq!(by_agent["sessions"].as_array().unwrap().len(), 2);
    let (_, by_status) = call(
        &app,
        Method::GET,
        &format!("{ws}?status=failed"),
        &token,
        None,
    )
    .await;
    assert_eq!(by_status["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(by_status["sessions"][0]["run_id"], third);
    let (_, future) = call(
        &app,
        Method::GET,
        &format!("{ws}?from=2999-01-01T00:00:00Z"),
        &token,
        None,
    )
    .await;
    assert!(future["sessions"].as_array().unwrap().is_empty());
    let (_, past) = call(
        &app,
        Method::GET,
        &format!("{ws}?to=2000-01-01T00:00:00Z"),
        &token,
        None,
    )
    .await;
    assert!(past["sessions"].as_array().unwrap().is_empty());
    // Paged two at a time without repeats.
    let (_, page_one) = call(&app, Method::GET, &format!("{ws}?limit=2"), &token, None).await;
    assert_eq!(page_one["sessions"].as_array().unwrap().len(), 2);
    let next = page_one["next"].as_str().expect("a next cursor").to_owned();
    assert_eq!(next, second);
    let (_, page_two) = call(
        &app,
        Method::GET,
        &format!("{ws}?limit=2&before={next}"),
        &token,
        None,
    )
    .await;
    assert_eq!(page_two["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(page_two["sessions"][0]["run_id"], first);
    assert!(page_two["next"].is_null());
    // The bounds: a clamped limit, a window the wrong way round, unknown states and ids.
    let (status, clamped) = call(
        &app,
        Method::GET,
        &format!("{ws}?limit=100000"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{clamped}");
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{ws}?from=2026-01-02T00:00:00Z&to=2026-01-01T00:00:00Z"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{ws}?status=teleported"),
        &token,
        None,
    )
    .await;
    assert!(status.is_client_error(), "{status}");
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{ws}?agent=not-a-uuid"),
        &token,
        None,
    )
    .await;
    assert!(status.is_client_error(), "{status}");
    // Guests are refused, outsiders find nothing.
    let (guest, _) = app.register("guest@example.com").await;
    call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &token,
        Some(json!({"email": "guest@example.com", "role": "guest"})),
    )
    .await;
    let (status, _) = call(&app, Method::GET, &ws, &guest, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (outsider, _) = app.register("outsider@example.com").await;
    let (status, _) = call(&app, Method::GET, &ws, &outsider, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
