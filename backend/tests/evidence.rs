//! Evidence-bound outputs (`docs/AGENTSECOPS.md` §2): every node's output is
//! verified against what the node was given, the verdict is stored on the
//! node run and rides on its last `llm_call` span, and the policy's
//! `evidence` rule decides what an unverified output means for the node.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
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

/// A built-in agent of `role`, which the fake model answers for.
async fn analyst(app: &TestApp, token: &str) -> String {
    let (status, agent) = call(
        app,
        Method::POST,
        "/agents",
        token,
        Some(json!({
            "name": "Analyst", "role": "analyst", "runtime": "builtin",
            "purpose": "Answers for the evidence tests.",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    agent["id"].as_str().unwrap().to_owned()
}

/// Sets the agent's evidence rule as its policy's next version.
async fn evidence_policy(app: &TestApp, token: &str, wid: &str, agent_id: &str, rule: Value) {
    let (status, body) = call(
        app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        token,
        Some(json!({"scope": "agent", "scope_id": agent_id, "document": {"evidence": rule}})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// A graph of two analyst nodes, `Research` feeding `Draft`.
async fn graph(app: &TestApp, token: &str) -> (String, String, String) {
    let gid = app.graph(token, "Write a note").await;
    let node = |title: &str| json!({"title": title, "kind": "research", "executor": "llm", "agent_role": "analyst"});
    let research = app.node(token, &gid, node("Research")).await;
    let draft = app.node(token, &gid, node("Draft")).await;
    app.edge(token, &gid, &research, &draft).await;
    (gid, research, draft)
}

async fn run(app: &TestApp, token: &str, gid: &str) -> Value {
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
    let events = collect_until(&mut rx, "run.finished").await;
    events.last().unwrap().1["run"].clone()
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

fn node_run<'a>(run: &'a Value, node: &str) -> &'a Value {
    run["node_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["node_id"] == node)
        .unwrap()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn every_output_gets_a_verdict_and_the_default_policy_only_records_it(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    analyst(&app, &token).await;
    let (gid, research, draft) = graph(&app, &token).await;
    let finished = run(&app, &token, &gid).await;
    assert_eq!(finished["status"], "succeeded", "{finished}");
    // The fake model cites nothing: the first node was given nothing, the second its upstream.
    let first = node_run(&finished, &research);
    assert_eq!(first["evidence"]["verdict"], "unverified", "{first}");
    assert_eq!(first["evidence"]["given"], 0);
    assert_eq!(first["evidence"]["coverage"], 0.0);
    let second = node_run(&finished, &draft);
    assert_eq!(second["evidence"]["verdict"], "unverified", "{second}");
    assert_eq!(second["evidence"]["given"], 1, "the upstream node");
    assert_eq!(second["evidence"]["claims"], 1);
    assert_eq!(second["evidence"]["cited"], json!([]));
    assert_eq!(second["evidence"]["unknown"], json!([]));
    assert_eq!(second["evidence"]["min_coverage"], 0.8);
    assert_eq!(second["error"], Value::Null);
    // The verdict rides on the last model call; no policy decided anything.
    let rid = finished["id"].as_str().unwrap();
    let spans = spans_of(&app, &token, rid).await;
    let calls: Vec<&Value> = spans.iter().filter(|s| s["kind"] == "llm_call").collect();
    assert_eq!(calls.len(), 2);
    for call in &calls {
        assert_eq!(call["attributes"]["evidence"], "unverified", "{call}");
        assert_eq!(call["attributes"]["evidence_coverage"], 0);
        assert!(call["attributes"]["evidence_cited_ids"].is_array());
    }
    assert!(
        !spans.iter().any(|s| s["name"] == "evidence"),
        "nothing required: {spans:?}"
    );
    let read = spans
        .iter()
        .find(|s| s["kind"] == "data_read" && s["node_id"] == draft)
        .unwrap();
    assert_eq!(
        read["attributes"]["citation_ids"],
        json!([format!("node:{research}")])
    );
    // A cached result keeps the verdict it got.
    let mut rx = app.events(&gid);
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &token,
        Some(json!({"force": false})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let again = collect_until(&mut rx, "run.finished")
        .await
        .last()
        .unwrap()
        .1["run"]
        .clone();
    let cached = node_run(&again, &research);
    assert_eq!(cached["cached"], true, "{cached}");
    assert_eq!(cached["evidence"]["verdict"], "unverified");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_reject_policy_fails_the_node_with_the_verdict_as_the_reason(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    let agent_id = analyst(&app, &token).await;
    evidence_policy(
        &app,
        &token,
        &wid,
        &agent_id,
        json!({"required": true, "action": "reject"}),
    )
    .await;
    let (gid, research, draft) = graph(&app, &token).await;
    let finished = run(&app, &token, &gid).await;
    assert_eq!(finished["status"], "failed", "{finished}");
    let first = node_run(&finished, &research);
    assert_eq!(first["status"], "failed");
    let error = first["error"].as_str().unwrap();
    assert!(
        error.starts_with("policy: evidence unverified: no claim rests on what the node was given"),
        "{error}"
    );
    assert_eq!(
        first["evidence"]["verdict"], "unverified",
        "the verdict stays on record"
    );
    assert!(
        first["tokens_in"].as_i64().unwrap() > 0,
        "the call is accounted for"
    );
    assert_eq!(
        node_run(&finished, &draft)["status"],
        "skipped",
        "nothing is built on a rejected output"
    );
    // The rejected output is not the node's output.
    let (_, graph) = call(&app, Method::GET, &format!("/graphs/{gid}"), &token, None).await;
    let node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == research)
        .unwrap();
    assert_eq!(node["output"], Value::Null, "{node}");
    assert_eq!(node["status"], "failed");
    // The decision is a policy span named `evidence`, denied by `evidence.required`.
    let rid = finished["id"].as_str().unwrap();
    let spans = spans_of(&app, &token, rid).await;
    let decision = spans
        .iter()
        .find(|s| s["kind"] == "policy" && s["name"] == "evidence")
        .unwrap_or_else(|| panic!("{spans:?}"));
    let a = &decision["attributes"];
    assert_eq!(a["decision"], "deny");
    assert_eq!(a["rule"], "evidence.required");
    assert_eq!(a["action"], "evidence");
    assert_eq!(a["evidence"], "unverified");
    assert_eq!(a["evidence_action"], "reject");
    assert_eq!(decision["status"], "error");
    let turn = spans
        .iter()
        .find(|s| s["kind"] == "llm_call" && s["node_id"] == research)
        .unwrap();
    assert_eq!(a["turn_span_id"], turn["id"]);
    assert_eq!(turn["attributes"]["evidence"], "unverified");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_mark_policy_records_the_verdict_and_lets_the_node_succeed(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    let agent_id = analyst(&app, &token).await;
    evidence_policy(
        &app,
        &token,
        &wid,
        &agent_id,
        json!({"required": true, "min_coverage": 0.5, "action": "mark"}),
    )
    .await;
    let (gid, research, draft) = graph(&app, &token).await;
    let finished = run(&app, &token, &gid).await;
    assert_eq!(finished["status"], "succeeded", "{finished}");
    for node in [&research, &draft] {
        let nr = node_run(&finished, node);
        assert_eq!(nr["status"], "succeeded");
        assert_eq!(nr["evidence"]["verdict"], "unverified");
        assert_eq!(
            nr["evidence"]["min_coverage"], 0.5,
            "the policy's threshold"
        );
        assert_eq!(nr["error"], Value::Null);
    }
    let rid = finished["id"].as_str().unwrap();
    let spans = spans_of(&app, &token, rid).await;
    let marks: Vec<&Value> = spans
        .iter()
        .filter(|s| s["kind"] == "policy" && s["name"] == "evidence")
        .collect();
    assert_eq!(marks.len(), 2, "{spans:?}");
    for mark in marks {
        assert_eq!(mark["attributes"]["decision"], "alert");
        assert_eq!(mark["attributes"]["severity"], Value::Null);
        assert_eq!(mark["attributes"]["evidence_action"], "mark");
        assert_eq!(mark["status"], "ok");
    }
    // A policy that would weaken the rule below the workspace's is refused.
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        &token,
        Some(json!({"scope": "workspace", "document": {"evidence": {"required": true, "action": "reject"}}})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        &token,
        Some(json!({"scope": "agent", "scope_id": agent_id, "document": {"evidence": {"required": false, "action": "mark"}}})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let text = body.to_string();
    assert!(text.contains("document.evidence.required"), "{text}");
    assert!(text.contains("document.evidence.action"), "{text}");
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        &token,
        Some(json!({"scope": "agent", "scope_id": agent_id, "document": {"evidence": {"min_coverage": 1.5}}})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    // The resolved policy shows the rule and which level set it.
    let (_, resolved) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/policies/resolve?agent_id={agent_id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(resolved["rules"]["evidence"]["required"], true);
    assert_eq!(
        resolved["rules"]["evidence"]["action"], "reject",
        "the workspace's reject is not undone by the agent's mark"
    );
    assert_eq!(resolved["rules"]["evidence"]["min_coverage"], 0.5);
    assert_eq!(resolved["origins"]["evidence.required"], "agent");
    assert_eq!(resolved["origins"]["evidence.min_coverage"], "agent");
}
