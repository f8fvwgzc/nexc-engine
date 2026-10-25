//! The hot-read caches: an answer served from memory is the same as the
//! database's, a write drops it here and on every other replica through the
//! realtime fan-out, and the firewall decides from the dropped-and-reloaded
//! policy at once.
#![forbid(unsafe_code)]

mod common;

use std::time::{Duration, Instant};

use axum::http::{Method, StatusCode};
use common::TestApp;
use nexc::app::caches::Key;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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

/// Two servers on one database, each with its fan-out running, as behind a load balancer.
async fn replicas(pool: &PgPool) -> (TestApp, TestApp, CancellationToken) {
    let here = TestApp::new(pool.clone(), &[]).await;
    let there = TestApp::new(pool.clone(), &[]).await;
    let shutdown = CancellationToken::new();
    for app in [&here, &there] {
        nexc::realtime::fanout::spawn(
            app.state.hub.clone(),
            app.state.caches.clone(),
            pool.clone(),
            shutdown.clone(),
        );
    }
    // The listeners are connected once a notification of our own comes back.
    tokio::time::sleep(Duration::from_millis(300)).await;
    (here, there, shutdown)
}

/// Waits until `check` holds, a few seconds at most.
async fn eventually(check: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    check()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_write_drops_the_entry_on_every_replica(pool: PgPool) {
    let (here, there, shutdown) = replicas(&pool).await;
    let (owner, _) = here.register("owner@example.com").await;
    let (_, list) = call(&here, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let workspace: Uuid = wid.parse().unwrap();
    let guardrails = format!("/workspaces/{wid}/guardrails");

    // Both replicas serve the guardrails from memory after one read each.
    for app in [&here, &there] {
        let (status, body) = call(app, Method::GET, &guardrails, &owner, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body["allow_code_exec"].is_boolean(), "{body}");
        assert!(app.state.caches.has_guardrails(workspace));
    }
    let before = there.state.metrics.render();
    let (_, again) = call(&there, Method::GET, &guardrails, &owner, None).await;
    assert!(again["allow_code_exec"].is_boolean());
    let after = there.state.metrics.render();
    assert!(
        hits(&after, "guardrails") > hits(&before, "guardrails"),
        "the second read was a hit:\n{after}"
    );

    // Written here: dropped here at once, and there as soon as the notification lands.
    let (_, current) = call(&here, Method::GET, &guardrails, &owner, None).await;
    let mut changed = current.clone();
    changed["monthly_token_budget"] = json!(12345);
    let (status, body) = call(&here, Method::PUT, &guardrails, &owner, Some(changed)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!here.state.caches.has_guardrails(workspace));
    assert!(
        eventually(|| !there.state.caches.has_guardrails(workspace)).await,
        "the other replica dropped the entry"
    );
    let (_, seen) = call(&there, Method::GET, &guardrails, &owner, None).await;
    assert_eq!(seen["monthly_token_budget"], 12345, "{seen}");

    // An agent's card and its resolved policy go the same way.
    let (status, agent) = call(
        &here,
        Method::POST,
        "/agents",
        &owner,
        Some(
            json!({"name": "Clerk", "role": "clerk", "workspace_id": wid,
            "capabilities": {"tools": ["read_file", "finish"]}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    let agent_id = agent["id"].as_str().unwrap().to_owned();
    let card = format!("/agents/{agent_id}");
    let resolve = format!("/workspaces/{wid}/policies/resolve?agent_id={agent_id}");
    for app in [&here, &there] {
        let (status, body) = call(app, Method::GET, &card, &owner, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, resolved) = call(app, Method::GET, &resolve, &owner, None).await;
        assert_eq!(status, StatusCode::OK, "{resolved}");
        assert_eq!(
            resolved["capabilities"]["tools"],
            json!(["read_file", "finish"])
        );
        assert!(app.state.caches.has_card(agent_id.parse().unwrap()));
    }
    let (status, _) = call(
        &there,
        Method::POST,
        &format!("/workspaces/{wid}/policies"),
        &owner,
        Some(json!({"scope": "workspace", "scope_id": null,
            "document": {"tools": {"deny": ["read_file"]}}})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = call(
        &there,
        Method::PATCH,
        &card,
        &owner,
        Some(json!({"title": "Senior clerk"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let agent_uuid: Uuid = agent_id.parse().unwrap();
    assert!(
        eventually(|| !here.state.caches.has_card(agent_uuid)).await,
        "the card went on the other replica"
    );
    let (_, resolved) = call(&here, Method::GET, &resolve, &owner, None).await;
    assert_eq!(
        resolved["rules"]["tools_denied"],
        json!(["read_file"]),
        "the new policy applies on the replica that did not write it: {resolved}"
    );
    assert_eq!(resolved["levels"][0]["version"], 1, "{resolved}");
    let (_, card) = call(&here, Method::GET, &card, &owner, None).await;
    assert_eq!(card["agent"]["title"], "Senior clerk");
    shutdown.cancel();
}

/// A replica whose LISTEN connection dropped may have missed what the others
/// invalidated meanwhile: it drops everything it kept once it listens again,
/// and hears the next write as before.
#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn a_lost_listen_connection_drops_every_entry(pool: PgPool) {
    let (here, there, shutdown) = replicas(&pool).await;
    let (owner, _) = here.register("owner@example.com").await;
    let (_, list) = call(&here, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let workspace: Uuid = wid.parse().unwrap();
    let guardrails = format!("/workspaces/{wid}/guardrails");
    for app in [&here, &there] {
        let (status, _) = call(app, Method::GET, &guardrails, &owner, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(app.state.caches.has_guardrails(workspace));
    }

    // The database ends both LISTEN connections, as a failover would.
    let ended: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE datname = current_database() AND query LIKE 'LISTEN %') ended",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ended, 2, "one LISTEN connection per replica");
    for app in [&here, &there] {
        assert!(
            eventually(|| !app.state.caches.has_guardrails(workspace)).await,
            "the entry went once the replica listened again"
        );
    }

    // Listening again: a write on one replica still drops the entry on the other.
    let (_, current) = call(&there, Method::GET, &guardrails, &owner, None).await;
    assert!(there.state.caches.has_guardrails(workspace));
    let mut changed = current.clone();
    changed["monthly_token_budget"] = json!(777);
    let (status, body) = call(&here, Method::PUT, &guardrails, &owner, Some(changed)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        eventually(|| !there.state.caches.has_guardrails(workspace)).await,
        "the other replica heard the write"
    );
    shutdown.cancel();
}

fn hits(rendered: &str, cache: &str) -> u64 {
    rendered
        .lines()
        .find(|l| {
            l.starts_with(&format!(
                "nexc_cache_lookups_total{{cache=\"{cache}\",result=\"hit\"}}"
            ))
        })
        .and_then(|l| l.rsplit(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn the_caches_can_be_turned_off(pool: PgPool) {
    let app = TestApp::new(pool, &[("NEXC_CACHE_TTL_SECS", "0")]).await;
    let (owner, _) = app.register("owner@example.com").await;
    let (_, list) = call(&app, Method::GET, "/workspaces", &owner, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let workspace: Uuid = wid.parse().unwrap();
    for _ in 0..2 {
        let (status, _) = call(
            &app,
            Method::GET,
            &format!("/workspaces/{wid}/guardrails"),
            &owner,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    assert!(!app.state.caches.has_guardrails(workspace));
    assert_eq!(hits(&app.state.metrics.render(), "guardrails"), 0);
    assert!(app.state.caches.sizes().iter().all(|(_, n)| *n == 0));
    app.state.caches.invalidate(Key::Guardrails(workspace));
}
