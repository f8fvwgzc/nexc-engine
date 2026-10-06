//! Planning and execution with a fake LLM: SSE event ordering, caching,
//! retries, failure propagation, artifacts, the missing-key 422 and demo mode.
#![forbid(unsafe_code)]

mod common;

use std::sync::atomic::Ordering;

use axum::http::{Method, StatusCode, header};
use common::{TestApp, collect_until, fake_runtime};
use serde_json::{Value, json};
use sqlx::PgPool;

fn names(events: &[(String, Value)]) -> Vec<&str> {
    events.iter().map(|(e, _)| e.as_str()).collect()
}

/// Index of the first `node.status` event of `node` with `status`.
fn status_at(events: &[(String, Value)], node: &str, status: &str) -> usize {
    events
        .iter()
        .position(|(e, d)| e == "node.status" && d["node_id"] == node && d["status"] == status)
        .unwrap_or_else(|| panic!("no {status} event for {node}"))
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn run_executes_the_dag_in_order(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("runner@example.com").await;
    let gid = app.graph(&token, "Write a short report").await;
    let research = app
        .node(
            &token,
            &gid,
            json!({"title": "Research", "kind": "research"}),
        )
        .await;
    let draft = app
        .node(&token, &gid, json!({"title": "Draft", "kind": "document"}))
        .await;
    let review = app
        .node(&token, &gid, json!({"title": "Review", "kind": "task"}))
        .await;
    app.edge(&token, &gid, &research, &draft).await;
    app.edge(&token, &gid, &draft, &review).await;

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
    assert_eq!(r.body["status"], "queued");
    assert_eq!(r.body["node_runs"].as_array().unwrap().len(), 3);
    let rid = r.body["id"].as_str().unwrap().to_owned();

    let events = collect_until(&mut rx, "run.finished").await;
    let n = names(&events);
    assert_eq!(n[0], "run.started");
    assert_eq!(n.last(), Some(&"run.finished"));
    for node in [&research, &draft, &review] {
        assert!(status_at(&events, node, "queued") < status_at(&events, node, "running"));
        assert!(status_at(&events, node, "running") < status_at(&events, node, "succeeded"));
    }
    assert!(status_at(&events, &research, "succeeded") < status_at(&events, &draft, "running"));
    assert!(status_at(&events, &draft, "succeeded") < status_at(&events, &review, "running"));
    assert!(
        n.contains(&"node.output") && n.contains(&"node.tokens") && n.contains(&"artifact.created")
    );
    let output: String = events
        .iter()
        .filter(|(e, d)| e == "node.output" && d["node_id"] == draft)
        .map(|(_, d)| d["delta"].as_str().unwrap())
        .collect();
    assert_eq!(
        output, "Result of Draft (after: Research)",
        "upstream output is in the prompt"
    );

    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "succeeded");
    assert_eq!(finished["tokens_in"], 300);
    assert!(finished["cost_usd"].as_f64().unwrap() > 0.0);
    assert!(
        finished["node_runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["cached"] == false && n["attempt"] == 1)
    );

    let artifacts = app
        .request(
            Method::GET,
            &format!("/api/v1/runs/{rid}/artifacts"),
            Some(&token),
            None,
        )
        .await;
    let list = artifacts.body.as_array().unwrap();
    assert_eq!(list.len(), 1, "document nodes produce a markdown artifact");
    assert_eq!(list[0]["path"], "draft.md");
    let aid = list[0]["id"].as_str().unwrap();
    let file = app
        .request(
            Method::GET,
            &format!("/api/v1/artifacts/{aid}/download"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(file.status, StatusCode::OK);
    assert!(
        file.headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    assert_eq!(file.headers[header::CONTENT_TYPE], "text/markdown");
    let zip = app
        .request(
            Method::GET,
            &format!("/api/v1/runs/{rid}/artifacts.zip"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(
        (
            zip.status,
            zip.headers[header::CONTENT_TYPE].to_str().unwrap()
        ),
        (StatusCode::OK, "application/zip")
    );

    let (other, _) = app.register("other@example.com").await;
    for path in [
        format!("/api/v1/runs/{rid}"),
        format!("/api/v1/artifacts/{aid}/download"),
        format!("/api/v1/runs/{rid}/artifacts.zip"),
    ] {
        assert_eq!(
            app.request(Method::GET, &path, Some(&other), None)
                .await
                .status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }

    // Unchanged nodes are served from the cache; `force` bypasses it.
    let calls = app.fake.calls.load(Ordering::SeqCst);
    let mut rx = app.events(&gid);
    app.request(
        Method::POST,
        &format!("/api/v1/graphs/{gid}/runs"),
        Some(&token),
        Some(json!({})),
    )
    .await;
    let events = collect_until(&mut rx, "run.finished").await;
    let run = &events.last().unwrap().1["run"];
    assert_eq!(run["status"], "succeeded");
    assert!(
        run["node_runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["cached"] == true)
    );
    assert!(
        events
            .iter()
            .any(|(e, d)| e == "node.status" && d["status"] == "succeeded" && d["cached"] == true)
    );
    assert!(
        events.iter().any(|(e, _)| e == "artifact.created"),
        "cached artifacts are copied"
    );
    assert_eq!(
        app.fake.calls.load(Ordering::SeqCst),
        calls,
        "no LLM calls for cached nodes"
    );

    let mut rx = app.events(&gid);
    let forced = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/runs"),
            Some(&token),
            Some(json!({"force": true, "node_ids": [review]})),
        )
        .await;
    assert_eq!(forced.body["node_runs"].as_array().unwrap().len(), 1);
    let events = collect_until(&mut rx, "run.finished").await;
    let run = &events.last().unwrap().1["run"];
    assert_eq!(run["node_runs"][0]["cached"], false);

    let runs = app
        .request(
            Method::GET,
            &format!("/api/v1/graphs/{gid}/runs"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(runs.body.as_array().unwrap().len(), 3);
    assert_eq!(runs.body[0]["id"], run["id"], "newest first");

    // Deleting the graph removes its runs' files from disk, not only their rows.
    let stored = app.state.settings.artifacts_dir().join(&rid);
    assert!(
        stored.exists(),
        "the artifact is on disk while the graph lives"
    );
    let deleted = app
        .request(
            Method::DELETE,
            &format!("/api/v1/graphs/{gid}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert!(!stored.exists(), "and gone with it");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn failures_skip_descendants_and_retries_recover(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("fail@example.com").await;
    let gid = app.graph(&token, "").await;
    let bad = app.node(&token, &gid, json!({"title": "FAIL here"})).await;
    let child = app.node(&token, &gid, json!({"title": "Child"})).await;
    let flaky = app.node(&token, &gid, json!({"title": "FLAKY step"})).await;
    app.edge(&token, &gid, &bad, &child).await;

    let mut rx = app.events(&gid);
    app.request(
        Method::POST,
        &format!("/api/v1/graphs/{gid}/runs"),
        Some(&token),
        Some(json!({"max_concurrency": 1})),
    )
    .await;
    let events = collect_until(&mut rx, "run.finished").await;
    let run = &events.last().unwrap().1["run"];
    assert_eq!(run["status"], "failed");
    let by_node = |id: &str| {
        run["node_runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["node_id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(by_node(&bad)["status"], "failed");
    assert_eq!(
        by_node(&bad)["attempt"],
        1,
        "non-retryable errors are not retried"
    );
    assert_eq!(by_node(&child)["status"], "skipped");
    assert_eq!(by_node(&flaky)["status"], "succeeded");
    assert_eq!(
        by_node(&flaky)["attempt"],
        2,
        "retryable errors are retried"
    );
    assert!(
        events
            .iter()
            .any(|(e, d)| e == "node.log" && d["level"] == "warn")
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn plan_and_run_need_an_llm_key(pool: PgPool) {
    let app = TestApp::new(pool, &[("ANTHROPIC_API_KEY", "")]).await;
    let (token, _) = app.register("nokey@example.com").await;
    let gid = app.graph(&token, "").await;
    app.node(&token, &gid, json!({"title": "Only node"})).await;
    for path in [
        format!("/api/v1/graphs/{gid}/runs"),
        format!("/api/v1/graphs/{gid}/plan"),
    ] {
        let r = app
            .request(Method::POST, &path, Some(&token), Some(json!({})))
            .await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{path}");
        let detail = r.body["detail"].as_str().unwrap();
        assert!(
            detail.contains("No LLM API key configured") && detail.contains("demo"),
            "{detail}"
        );
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn plan_streams_nodes_and_applies(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("planner@example.com").await;
    let gid = app.graph(&token, "Write a report").await;
    let existing = app
        .node(
            &token,
            &gid,
            json!({"title": "Idea", "x": 500.0, "y": 300.0}),
        )
        .await;

    let mut rx = app.events(&gid);
    let r = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/plan"),
            Some(&token),
            Some(json!({"instructions": "be brief"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.body);
    assert_eq!(r.body["status"], "streaming");
    let pid = r.body["id"].as_str().unwrap().to_owned();
    let events = collect_until(&mut rx, "plan.ready").await;
    let n = names(&events);
    assert_eq!(n[0], "plan.started");
    assert_eq!(
        n.iter().filter(|e| **e == "plan.node").count(),
        3,
        "each node is streamed"
    );
    assert_eq!(
        n.iter().filter(|e| **e == "plan.edge").count(),
        2,
        "the cycle-closing edge was dropped"
    );
    let first_edge = n.iter().position(|e| *e == "plan.edge").unwrap();
    assert!(n.iter().rposition(|e| *e == "plan.node").unwrap() < first_edge);
    let plan = &events.last().unwrap().1["plan"];
    assert_eq!(plan["status"], "ready");
    assert_eq!(plan["nodes"][0]["existing_id"], existing.as_str());
    assert_eq!(
        plan["nodes"][2]["existing_id"],
        Value::Null,
        "invalid ids are read as null"
    );

    let get = app
        .request(
            Method::GET,
            &format!("/api/v1/graphs/{gid}/plans/{pid}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(get.body["status"], "ready");

    let applied = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/plans/{pid}/apply"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(applied.status, StatusCode::OK, "{}", applied.body);
    let nodes = applied.body["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);
    let refined = nodes.iter().find(|n| n["id"] == existing.as_str()).unwrap();
    assert_eq!(
        (refined["title"].as_str(), refined["origin"].as_str()),
        (Some("Outline"), Some("user"))
    );
    assert_eq!(refined["x"], 500.0, "existing nodes keep their position");
    let new_nodes: Vec<&Value> = nodes.iter().filter(|n| n["origin"] == "plan").collect();
    assert_eq!(new_nodes.len(), 2);
    assert!(
        new_nodes.iter().all(|n| n["y"].as_f64().unwrap() > 300.0),
        "new nodes are laid out below"
    );
    let edges = applied.body["edges"].as_array().unwrap();
    assert_eq!(edges.iter().filter(|e| e["origin"] == "plan").count(), 2);

    let again = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/plans/{pid}/apply"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn demo_mode_plans_and_runs_without_a_key(pool: PgPool) {
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
    let status = app
        .request(
            Method::GET,
            "/api/v1/orchestrator/status",
            Some(&token),
            None,
        )
        .await;
    assert_eq!(status.body["demo_mode"], true);
    assert_eq!(
        status.body["agents_active"], 5,
        "the default organisation is seeded"
    );

    let gid = app.graph(&token, "Write a blog post").await;
    app.node(&token, &gid, json!({"title": "Research", "content": "- audience\n- angle\n- sources\n- examples", "kind": "research"})).await;

    let mut rx = app.events(&gid);
    let plan = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/plan"),
            Some(&token),
            Some(json!({})),
        )
        .await;
    assert_eq!(plan.status, StatusCode::ACCEPTED);
    let events = collect_until(&mut rx, "plan.ready").await;
    let ready = &events.last().unwrap().1["plan"];
    assert!(ready["summary"].as_str().unwrap().starts_with("[demo]"));
    let pid = ready["id"].as_str().unwrap();
    let applied = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/plans/{pid}/apply"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(applied.status, StatusCode::OK);
    assert_eq!(
        applied.body["nodes"].as_array().unwrap().len(),
        5,
        "3 sub-tasks and a final node were added"
    );

    let mut rx = app.events(&gid);
    let run = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/runs"),
            Some(&token),
            Some(json!({"max_concurrency": 8})),
        )
        .await;
    assert_eq!(run.status, StatusCode::ACCEPTED, "{}", run.body);
    let events = collect_until(&mut rx, "run.finished").await;
    let finished = &events.last().unwrap().1["run"];
    assert_eq!(finished["status"], "succeeded");
    assert!(finished["tokens_out"].as_i64().unwrap() > 0);
    assert_eq!(finished["cost_usd"], 0.0);
    let preview = finished["node_runs"][0]["output_preview"].as_str().unwrap();
    assert!(preview.starts_with("[demo output]"), "{preview}");
    assert!(
        events
            .iter()
            .filter(|(e, _)| e == "artifact.created")
            .count()
            >= 5,
        "demo nodes produce artifacts"
    );
    let final_run = finished["node_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["executor"] == "agent")
        .expect("the final output node runs on the agent runtime");
    assert_eq!(final_run["status"], "succeeded");
    assert_eq!(
        final_run["tokens_in"], 10,
        "runtime token increments are summed"
    );
    assert!(
        events.iter().any(|(e, d)| e == "artifact.created"
            && d["artifact"]["path"]
                .as_str()
                .unwrap()
                .ends_with("deliverable.md")),
        "runtime artifacts are stored"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn memories_are_extracted_and_searchable(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("mem@example.com").await;
    let gid = app.graph(&token, "").await;
    app.node(&token, &gid, json!({"title": "Write intro"}))
        .await;
    let mut rx = app.events(&gid);
    app.request(
        Method::POST,
        &format!("/api/v1/graphs/{gid}/runs"),
        Some(&token),
        Some(json!({})),
    )
    .await;
    collect_until(&mut rx, "run.finished").await;

    let mut found = Value::Null;
    for _ in 0..50 {
        let r = app
            .request(
                Method::GET,
                &format!("/api/v1/memories?graph_id={gid}&q=citation%20style"),
                Some(&token),
                None,
            )
            .await;
        if r.body.as_array().is_some_and(|a| !a.is_empty()) {
            found = r.body;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(found[0]["content"], "Reports use APA style.");
    assert_eq!(found[0]["kind"], "fact");
    assert!(found[0]["score"].as_f64().unwrap() > 0.0);
    let id = found[0]["id"].as_str().unwrap();
    let plain = app
        .request(Method::GET, "/api/v1/memories", Some(&token), None)
        .await;
    assert_eq!(plain.body[0]["score"], Value::Null);
    let (other, _) = app.register("other-mem@example.com").await;
    assert_eq!(
        app.request(
            Method::DELETE,
            &format!("/api/v1/memories/{id}"),
            Some(&other),
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.request(
            Method::DELETE,
            &format!("/api/v1/memories/{id}"),
            Some(&token),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
}
