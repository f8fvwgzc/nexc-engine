//! Evidence-bound outputs (`docs/AGENTSECOPS.md` §2): every node's output is
//! verified against what the node was given, the verdict is stored on the
//! node run and recorded as the node's own `policy` span named `evidence`
//! (the built-in executor's one `llm_call` carries it too), and the policy's
//! `evidence` rule decides what an unverified output means for the node.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::{TestApp, collect_until, fake_runtime};
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
    // At least its upstream; a memory extracted from the first node in the background
    // may already have been recalled too, which the full suite's timing sometimes allows.
    assert!(
        second["evidence"]["given"].as_u64().unwrap_or(0) >= 1,
        "the upstream node: {second}"
    );
    assert_eq!(second["evidence"]["claims"], 1);
    assert_eq!(second["evidence"]["cited"], json!([]));
    assert_eq!(second["evidence"]["unknown"], json!([]));
    assert_eq!(second["evidence"]["min_coverage"], 0.8);
    assert_eq!(second["error"], Value::Null);
    // The verdict is each node's own `evidence` policy span: recorded as an alert
    // since nothing is required, the node untouched. The built-in executor's one
    // call carries it too.
    let rid = finished["id"].as_str().unwrap();
    let spans = spans_of(&app, &token, rid).await;
    let verdicts: Vec<&Value> = spans
        .iter()
        .filter(|s| s["kind"] == "policy" && s["name"] == "evidence")
        .collect();
    assert_eq!(verdicts.len(), 2, "one per node: {spans:?}");
    for verdict in &verdicts {
        let a = &verdict["attributes"];
        assert_eq!(a["decision"], "alert", "{verdict}");
        assert_eq!(a["rule"], "evidence.required");
        assert_eq!(a["action"], "evidence");
        assert_eq!(a["evidence_required"], false);
        assert_eq!(a["evidence_action"], "mark");
        assert_eq!(a["evidence"], "unverified");
        assert_eq!(a["evidence_coverage"], 0);
        assert!(a["evidence_cited_ids"].is_array());
        assert_eq!(verdict["status"], "ok");
    }
    assert!(
        verdicts.iter().any(|s| s["node_id"] == research)
            && verdicts.iter().any(|s| s["node_id"] == draft)
    );
    let calls: Vec<&Value> = spans.iter().filter(|s| s["kind"] == "llm_call").collect();
    assert_eq!(calls.len(), 2);
    for call in &calls {
        assert_eq!(call["attributes"]["evidence"], "unverified", "{call}");
        assert_eq!(call["attributes"]["evidence_coverage"], 0);
    }
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
    // The decision is the one policy span named `evidence`, denied by `evidence.required`.
    let rid = finished["id"].as_str().unwrap();
    let spans = spans_of(&app, &token, rid).await;
    let decisions: Vec<&Value> = spans
        .iter()
        .filter(|s| s["kind"] == "policy" && s["name"] == "evidence")
        .collect();
    assert_eq!(decisions.len(), 1, "one per attempt: {spans:?}");
    let decision = decisions[0];
    let a = &decision["attributes"];
    assert_eq!(a["decision"], "deny");
    assert_eq!(a["rule"], "evidence.required");
    assert_eq!(a["action"], "evidence");
    assert_eq!(a["evidence"], "unverified");
    assert_eq!(a["evidence_required"], true);
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
        assert_eq!(mark["attributes"]["evidence_required"], true);
        assert_eq!(mark["attributes"]["evidence_action"], "mark");
        assert_eq!(mark["attributes"]["evidence"], "unverified");
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

const HANDBOOK: &str = "# Warehouse handbook\n\n## Customs\n\nGoods under 150 EUR are exempt from duty. \
Invoices for Rotterdam name the customs reference on the first page.\n\n## Returns\n\nReturns go back \
within thirty days with the original invoice.\n";

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_console_the_card_and_the_chips_show_the_verdicts(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let (guest, _) = app.register("guest@example.com").await;
    let wid = workspace(&app, &token).await;
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/members"),
        &token,
        Some(json!({"email": "guest@example.com", "role": "guest"})),
    )
    .await;
    assert!(status.is_success(), "{body}");
    let agent_id = analyst(&app, &token).await;
    let (gid, research, draft) = graph(&app, &token).await;
    let finished = run(&app, &token, &gid).await;
    assert_eq!(finished["status"], "succeeded", "{finished}");

    // The overview counts the day's outputs by verdict.
    let (status, overview) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/overview"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    assert_eq!(
        overview["evidence_today"],
        json!({"verified": 0, "partial": 0, "unverified": 2})
    );
    // The explorer narrows to the `evidence` policy spans, one per output, and says the verdict.
    let (status, page) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/spans?decision=evidence"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let spans = page["spans"].as_array().unwrap();
    assert_eq!(spans.len(), 2, "{page}");
    assert!(spans.iter().all(|s| s["kind"] == "policy"
        && s["name"] == "evidence"
        && s["decision"] == "alert"
        && s["evidence"] == "unverified"));
    let (status, page) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/spans?decision=deny"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["spans"].as_array().unwrap().len(), 0);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/secops/spans?decision=evidences"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // The card sums up the agent's last outputs.
    let (status, card) = call(
        &app,
        Method::GET,
        &format!("/agents/{agent_id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{card}");
    let evidence = &card["evidence"];
    assert_eq!(
        (
            evidence["verified"].clone(),
            evidence["partial"].clone(),
            evidence["unverified"].clone()
        ),
        (json!(0), json!(0), json!(2))
    );
    let recent = evidence["recent"].as_array().unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0]["graph_id"], gid);
    assert!(
        recent.iter().any(|o| o["node_id"] == research)
            && recent.iter().any(|o| o["node_id"] == draft)
    );
    assert_eq!(recent[0]["verdict"], "unverified");
    assert_eq!(recent[0]["coverage"], 0.0);
    assert!(recent[0]["finished_at"].is_string());

    // A `[doc:<chunk id>]` chip opens the passage it points at.
    let upload = format!("/api/v1/workspaces/{wid}/documents?name=handbook.md");
    let r = app.send_bytes(&upload, &token, HANDBOOK.into()).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    nexc::engine::knowledge::work(&app.state).await.unwrap();
    let (_, found) = call(
        &app,
        Method::GET,
        &format!("/workspaces/{wid}/knowledge/search?q=customs%20reference%20Rotterdam"),
        &token,
        None,
    )
    .await;
    let chunk = found[0]["chunk_id"].as_str().unwrap().to_owned();
    let passages = format!("/workspaces/{wid}/knowledge/passages");
    let (status, passage) = call(
        &app,
        Method::GET,
        &format!("{passages}/{chunk}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{passage}");
    assert_eq!(passage["chunk_id"], chunk);
    assert_eq!(passage["document_name"], "handbook.md");
    assert!(passage["content"].as_str().unwrap().contains("Rotterdam"));
    assert_eq!(passage["score"], 1.0);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{passages}/{}", uuid::Uuid::now_v7()),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &app,
        Method::GET,
        &format!("{passages}/{chunk}"),
        &guest,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_demo_provider_cites_what_it_was_given(pool: PgPool) {
    let runtime = fake_runtime().await;
    let app = TestApp::with_real_providers(
        pool,
        &[
            ("NEXC_LLM_PROVIDER", "demo"),
            ("ANTHROPIC_API_KEY", ""),
            ("NEXC_RUNTIME_URL", &runtime),
        ],
    )
    .await;
    let (token, _) = app.register("demo@example.com").await;
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
    // Under `reject`, the first node (given nothing) fails and the second never runs.
    let finished = run(&app, &token, &gid).await;
    assert_eq!(finished["status"], "failed", "{finished}");
    assert_eq!(
        node_run(&finished, &research)["evidence"]["verdict"],
        "unverified"
    );
    assert_eq!(node_run(&finished, &draft)["status"], "skipped");
    // Under `mark`, the second node cites its upstream result and is verified.
    evidence_policy(
        &app,
        &token,
        &wid,
        &agent_id,
        json!({"required": true, "action": "mark"}),
    )
    .await;
    let finished = run(&app, &token, &gid).await;
    assert_eq!(finished["status"], "succeeded", "{finished}");
    let first = node_run(&finished, &research);
    assert_eq!(first["evidence"]["verdict"], "unverified");
    assert_eq!(first["evidence"]["coverage"], 0.0);
    assert!(
        first["evidence"]["declared_claims"].as_i64().unwrap() >= 3,
        "{first}"
    );
    let second = node_run(&finished, &draft);
    assert_eq!(second["evidence"]["verdict"], "verified", "{second}");
    assert_eq!(second["evidence"]["coverage"], 1.0);
    assert_eq!(
        second["evidence"]["cited"],
        json!([format!("node:{research}")])
    );
    assert_eq!(second["evidence"]["unknown"], json!([]));
    let preview = second["output_preview"].as_str().unwrap();
    assert!(preview.starts_with("[demo output]"), "{preview}");
    let rid = finished["id"].as_str().unwrap();
    let spans = spans_of(&app, &token, rid).await;
    let turn = spans
        .iter()
        .find(|s| s["kind"] == "llm_call" && s["node_id"] == draft)
        .unwrap();
    assert_eq!(turn["attributes"]["evidence"], "verified");
    assert_eq!(turn["attributes"]["evidence_coverage"], 100);
    assert_eq!(
        turn["attributes"]["evidence_cited_ids"],
        json!([format!("node:{research}")])
    );
    let decisions: Vec<&Value> = spans
        .iter()
        .filter(|s| s["kind"] == "policy" && s["name"] == "evidence")
        .collect();
    assert_eq!(decisions.len(), 2, "{spans:?}");
    let verdict_of = |node: &str| {
        decisions
            .iter()
            .find(|s| s["node_id"] == node)
            .map(|s| s["attributes"].clone())
            .unwrap()
    };
    let marked = verdict_of(&research);
    assert_eq!(marked["decision"], "alert", "marked");
    assert_eq!(marked["evidence"], "unverified");
    let verified = verdict_of(&draft);
    assert_eq!(verified["decision"], "allow", "verified under the rule");
    assert_eq!(verified["evidence"], "verified");
    assert_eq!(verified["evidence_coverage"], 100);
    assert_eq!(
        verified["evidence_cited_ids"],
        json!([format!("node:{research}")])
    );
    // The run page's node table and the agent card see the same verdicts.
    let (_, card) = call(
        &app,
        Method::GET,
        &format!("/agents/{agent_id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(card["evidence"]["verified"], 1);
    assert_eq!(card["evidence"]["unverified"], 2);
}
