//! `Idempotency-Key` (CONTRACT §3): a write retried with the same key and
//! body gets the first response, marked `Idempotent-Replayed`, and has its
//! effect once: one run, one issue, one node, one decision. The same key
//! with another body is refused; concurrent retries make one resource; keys
//! belong to an account and a route, and expire after 24 hours.
#![forbid(unsafe_code)]

mod common;

use axum::http::{Method, StatusCode};
use common::grpc_runtime::fake_grpc_runtime;
use common::{Resp, TestApp, collect_until};
use serde_json::{Value, json};
use sqlx::PgPool;

const KEY: &str = "Idempotency-Key";

async fn send(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    body: Value,
    key: Option<&str>,
) -> Resp {
    let headers: Vec<(&str, &str)> = key.map(|k| (KEY, k)).into_iter().collect();
    app.request_with(
        method,
        &format!("/api/v1{path}"),
        Some(token),
        Some(body),
        &headers,
    )
    .await
}

fn replayed(resp: &Resp) -> bool {
    resp.headers
        .get("idempotent-replayed")
        .is_some_and(|v| v == "true")
}

async fn workspace(app: &TestApp, token: &str) -> String {
    let r = app
        .request(Method::GET, "/api/v1/workspaces", Some(token), None)
        .await;
    r.body[0]["id"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_retried_run_start_answers_with_the_first_run(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let gid = app.graph(&token, "Retry safely").await;
    app.node(&token, &gid, json!({"title": "Outline"})).await;
    let runs = format!("/graphs/{gid}/runs");

    let first = send(&app, Method::POST, &runs, &token, json!({}), Some("run-1")).await;
    assert_eq!(first.status, StatusCode::ACCEPTED, "{}", first.body);
    assert!(!replayed(&first));
    let again = send(&app, Method::POST, &runs, &token, json!({}), Some("run-1")).await;
    assert_eq!(again.status, StatusCode::ACCEPTED, "{}", again.body);
    assert!(replayed(&again), "the first response, again");
    assert_eq!(again.body, first.body);
    assert!(
        again.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );

    // The same key with another request is refused; without a key nothing changes.
    let other = send(
        &app,
        Method::POST,
        &runs,
        &token,
        json!({"force": true}),
        Some("run-1"),
    )
    .await;
    assert_eq!(
        other.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        other.body
    );
    assert!(
        other.body["detail"]
            .as_str()
            .unwrap()
            .contains("different request")
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE graph_id = $1::uuid")
        .bind(&gid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "one run for three requests");

    // Keys belong to an account and to a route.
    let (stranger, _) = app.register("stranger@example.com").await;
    let theirs = app.graph(&stranger, "Theirs").await;
    app.node(&stranger, &theirs, json!({"title": "Their node"}))
        .await;
    let fresh = send(
        &app,
        Method::POST,
        &format!("/graphs/{theirs}/runs"),
        &stranger,
        json!({}),
        Some("run-1"),
    )
    .await;
    assert_eq!(fresh.status, StatusCode::ACCEPTED, "{}", fresh.body);
    assert!(!replayed(&fresh));
    let node = send(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/nodes"),
        &token,
        json!({"title": "Draft"}),
        Some("run-1"),
    )
    .await;
    assert_eq!(node.status, StatusCode::CREATED, "{}", node.body);
    assert!(!replayed(&node));
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_retried_issue_or_node_is_created_once(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    let team = send(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/teams"),
        &token,
        json!({"name": "Engineering", "key": "ENG"}),
        None,
    )
    .await;
    assert_eq!(team.status, StatusCode::CREATED, "{}", team.body);
    let issues = format!(
        "/workspaces/{wid}/teams/{}/issues",
        team.body["id"].as_str().unwrap()
    );
    let body = json!({"title": "Fix the login", "priority": 2});
    let first = send(
        &app,
        Method::POST,
        &issues,
        &token,
        body.clone(),
        Some("i-1"),
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.body);
    let again = send(&app, Method::POST, &issues, &token, body, Some("i-1")).await;
    assert_eq!(again.status, StatusCode::CREATED);
    assert!(replayed(&again));
    assert_eq!(again.body["id"], first.body["id"]);
    assert_eq!(again.body["number"], first.body["number"]);
    let next = send(
        &app,
        Method::POST,
        &issues,
        &token,
        json!({"title": "Another"}),
        Some("i-2"),
    )
    .await;
    assert_eq!(
        next.body["number"],
        first.body["number"].as_i64().unwrap() + 1
    );

    // A refusal is an answer too: kept and given again.
    let gid = app.graph(&token, "Nodes").await;
    let nodes = format!("/graphs/{gid}/nodes");
    let invalid = send(
        &app,
        Method::POST,
        &nodes,
        &token,
        json!({"title": ""}),
        Some("n-0"),
    )
    .await;
    assert_eq!(invalid.status, StatusCode::UNPROCESSABLE_ENTITY);
    let invalid_again = send(
        &app,
        Method::POST,
        &nodes,
        &token,
        json!({"title": ""}),
        Some("n-0"),
    )
    .await;
    assert_eq!(invalid_again.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(replayed(&invalid_again));
    assert_eq!(invalid_again.body, invalid.body);

    // Retries that arrive together make one node; the others wait or replay.
    let mut racing = Vec::new();
    for _ in 0..6 {
        racing.push(send(
            &app,
            Method::POST,
            &nodes,
            &token,
            json!({"title": "Once"}),
            Some("n-1"),
        ));
    }
    let answers = futures::future::join_all(racing).await;
    let mut ids = Vec::new();
    for answer in &answers {
        match answer.status {
            StatusCode::CREATED => ids.push(answer.body["id"].clone()),
            StatusCode::CONFLICT => {
                assert_eq!(answer.headers["retry-after"], "1");
            }
            other => panic!("unexpected {other}: {}", answer.body),
        }
    }
    assert!(!ids.is_empty());
    assert!(ids.iter().all(|id| *id == ids[0]), "{ids:?}");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM nodes WHERE graph_id = $1::uuid AND title = 'Once'",
    )
    .bind(&gid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_retried_decision_is_taken_once(pool: PgPool) {
    let (url, _seen) = fake_grpc_runtime().await;
    let app = TestApp::new(
        pool,
        &[
            ("NEXC_RUNTIME_PROTOCOL", "grpc"),
            ("NEXC_RUNTIME_GRPC_URL", &url),
        ],
    )
    .await;
    let (token, _) = app.register("owner@example.com").await;
    let wid = workspace(&app, &token).await;
    let policy = send(
        &app,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        &token,
        json!({"scope": "workspace", "scope_id": null,
            "document": {"require_approval": {"tools": ["make_docx"], "timeout_s": 60}}}),
        None,
    )
    .await;
    assert_eq!(policy.status, StatusCode::CREATED, "{}", policy.body);
    let agent = send(
        &app,
        Method::POST,
        "/agents",
        &token,
        json!({"name": "Scribe", "role": "scribe", "runtime": "python",
            "purpose": "Handles scribe work in tests.", "capabilities": {}}),
        None,
    )
    .await;
    assert_eq!(agent.status, StatusCode::CREATED, "{}", agent.body);
    let gid = app.graph(&token, "Decide once").await;
    app.node(
        &token,
        &gid,
        json!({"title": "APPROVAL report", "kind": "task", "executor": "agent", "agent_role": "scribe"}),
    )
    .await;
    let mut rx = app.events(&gid);
    let run = send(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &token,
        json!({}),
        None,
    )
    .await;
    assert_eq!(run.status, StatusCode::ACCEPTED, "{}", run.body);
    loop {
        let events = collect_until(&mut rx, "node.status").await;
        if events
            .last()
            .is_some_and(|(_, d)| d["status"] == "waiting_approval")
        {
            break;
        }
    }
    let pending = app
        .request(
            Method::GET,
            &format!("/api/v1/workspaces/{wid}/approvals?status=pending"),
            Some(&token),
            None,
        )
        .await;
    let id = pending.body[0]["id"].as_str().unwrap().to_owned();
    let approve = format!("/approvals/{id}/approve");
    let body = json!({"reason": "go ahead"});

    let first = send(
        &app,
        Method::POST,
        &approve,
        &token,
        body.clone(),
        Some("d-1"),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    let again = send(
        &app,
        Method::POST,
        &approve,
        &token,
        body.clone(),
        Some("d-1"),
    )
    .await;
    assert_eq!(
        again.status,
        StatusCode::OK,
        "a retry is answered, not refused"
    );
    assert!(replayed(&again));
    assert_eq!(again.body, first.body);
    // A second decision is still a conflict, with or without another key.
    let twice = send(&app, Method::POST, &approve, &token, body, Some("d-2")).await;
    assert_eq!(twice.status, StatusCode::CONFLICT);
    let deny = send(
        &app,
        Method::POST,
        &format!("/approvals/{id}/deny"),
        &token,
        json!({"reason": "no"}),
        Some("d-1"),
    )
    .await;
    assert_eq!(
        deny.status,
        StatusCode::CONFLICT,
        "another route, another key"
    );
    let finished = collect_until(&mut rx, "run.finished").await;
    assert_eq!(finished.last().unwrap().1["run"]["status"], "succeeded");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn keys_are_checked_and_expire_after_a_day(pool: PgPool) {
    let app = TestApp::new(pool.clone(), &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let gid = app.graph(&token, "Expire").await;
    let nodes = format!("/graphs/{gid}/nodes");
    for bad in ["has space", &"k".repeat(256)] {
        let r = send(
            &app,
            Method::POST,
            &nodes,
            &token,
            json!({"title": "X"}),
            Some(bad),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    }
    // No session: the handler answers, the key is not looked at.
    let anonymous = app
        .request_with(
            Method::POST,
            &format!("/api/v1{nodes}"),
            None,
            Some(json!({"title": "X"})),
            &[(KEY, "k-1")],
        )
        .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    // A route that does not take keys ignores one.
    let node = app.node(&token, &gid, json!({"title": "Plain"})).await;
    let patched = send(
        &app,
        Method::PATCH,
        &format!("{nodes}/{node}"),
        &token,
        json!({"title": "Renamed"}),
        Some("k-1"),
    )
    .await;
    assert_eq!(patched.status, StatusCode::OK);
    assert!(!replayed(&patched));

    let first = send(
        &app,
        Method::POST,
        &nodes,
        &token,
        json!({"title": "Daily"}),
        Some("k-1"),
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED);
    // A day later the key is free again, and the housekeeping job deletes it.
    sqlx::query("UPDATE idempotency_keys SET created_at = now() - interval '25 hours'")
        .execute(&pool)
        .await
        .unwrap();
    let later = send(
        &app,
        Method::POST,
        &nodes,
        &token,
        json!({"title": "Daily"}),
        Some("k-1"),
    )
    .await;
    assert_eq!(later.status, StatusCode::CREATED);
    assert!(!replayed(&later));
    assert_ne!(later.body["id"], first.body["id"]);
    sqlx::query("UPDATE idempotency_keys SET created_at = now() - interval '25 hours'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        nexc::repo::idempotency::purge_expired(&pool).await.unwrap(),
        1
    );
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM idempotency_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_keyed_request_counts_once_against_the_rate_limit_and_so_does_a_replay(pool: PgPool) {
    // 50 requests a minute per account: a burst of 10.
    let app = TestApp::new(pool, &[("NEXC_API_CALLS_PER_MINUTE", "50")]).await;
    let (token, _) = app.register("owner@example.com").await;
    let gid = app.graph(&token, "Limited").await;
    let nodes = format!("/graphs/{gid}/nodes");
    // Each keyed request is charged once, not once by the key's check and again by the handler.
    for n in 0..8 {
        let created = send(
            &app,
            Method::POST,
            &nodes,
            &token,
            json!({"title": format!("Node {n}")}),
            Some(&format!("node-{n}")),
        )
        .await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    }
    // Replays are answered from the record, but each is a request like any other.
    let mut statuses = Vec::new();
    for _ in 0..20 {
        let again = send(
            &app,
            Method::POST,
            &nodes,
            &token,
            json!({"title": "Node 0"}),
            Some("node-0"),
        )
        .await;
        statuses.push(again.status);
        if again.status == StatusCode::TOO_MANY_REQUESTS {
            assert!(!replayed(&again));
            break;
        }
        assert!(replayed(&again), "{}", again.body);
    }
    assert_eq!(
        statuses.last(),
        Some(&StatusCode::TOO_MANY_REQUESTS),
        "{statuses:?}"
    );
}
