//! Graph CRUD, cycle detection, ETags, ownership isolation, wikilink
//! dependency detection, analysis, templates and HTTP hardening.
#![forbid(unsafe_code)]

mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode, header};
use common::TestApp;
use serde_json::{Value, json};
use sqlx::PgPool;

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn crud_cycles_etag_and_isolation(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("owner@example.com").await;
    let gid = app.graph(&token, "Write a report").await;
    let a = app
        .node(&token, &gid, json!({"title": "A", "kind": "research"}))
        .await;
    let b = app.node(&token, &gid, json!({"title": "B"})).await;
    let c = app
        .node(
            &token,
            &gid,
            json!({"title": "C", "kind": "output", "x": 10.5, "y": -3.0}),
        )
        .await;

    assert_eq!(
        app.edge(&token, &gid, &a, &b).await.status,
        StatusCode::CREATED
    );
    assert_eq!(
        app.edge(&token, &gid, &b, &c).await.status,
        StatusCode::CREATED
    );
    let cycle = app.edge(&token, &gid, &c, &a).await;
    assert_eq!(cycle.status, StatusCode::CONFLICT);
    assert_eq!(
        cycle.headers[header::CONTENT_TYPE],
        "application/problem+json"
    );
    assert_eq!(
        app.edge(&token, &gid, &a, &b).await.status,
        StatusCode::CONFLICT,
        "duplicate edge"
    );
    let related = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/edges"),
            Some(&token),
            Some(json!({"source": c, "target": a, "kind": "relates_to"})),
        )
        .await;
    assert_eq!(
        related.status,
        StatusCode::CREATED,
        "relates_to edges may close loops"
    );

    let g = app
        .request(
            Method::GET,
            &format!("/api/v1/graphs/{gid}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(g.status, StatusCode::OK);
    assert_eq!(g.body["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(g.body["edges"].as_array().unwrap().len(), 3);
    assert_eq!(g.body["nodes"][2]["x"], 10.5);
    let etag = g.headers[header::ETAG].to_str().unwrap().to_owned();
    let cached = app
        .request_with(
            Method::GET,
            &format!("/api/v1/graphs/{gid}"),
            Some(&token),
            None,
            &[("if-none-match", &etag)],
        )
        .await;
    assert_eq!(cached.status, StatusCode::NOT_MODIFIED);

    let patched = app
        .request(
            Method::PATCH,
            &format!("/api/v1/graphs/{gid}/nodes/{b}"),
            Some(&token),
            Some(json!({"title": "B2", "agent_role": null, "tags": ["X", "x", " y "]})),
        )
        .await;
    assert_eq!(patched.status, StatusCode::OK, "{}", patched.body);
    assert_eq!(patched.body["tags"], json!(["x", "y"]));
    let changed = app
        .request_with(
            Method::GET,
            &format!("/api/v1/graphs/{gid}"),
            Some(&token),
            None,
            &[("if-none-match", &etag)],
        )
        .await;
    assert_eq!(
        changed.status,
        StatusCode::OK,
        "the ETag changes with the graph"
    );

    let analysis = app
        .request(
            Method::GET,
            &format!("/api/v1/graphs/{gid}/analysis"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(analysis.body["critical_path"], json!([a, b, c]));
    assert_eq!(analysis.body["levels"], json!([[a], [b], [c]]));
    assert_eq!(analysis.body["cycles"], json!([]));

    // Another user cannot see or touch the graph (404, not 403).
    let (other, _) = app.register("intruder@example.com").await;
    for (method, path) in [
        (Method::GET, format!("/api/v1/graphs/{gid}")),
        (Method::DELETE, format!("/api/v1/graphs/{gid}")),
        (Method::GET, format!("/api/v1/graphs/{gid}/analysis")),
        (Method::DELETE, format!("/api/v1/graphs/{gid}/nodes/{a}")),
    ] {
        let r = app.request(method, &path, Some(&other), None).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
    }
    let list = app
        .request(Method::GET, "/api/v1/graphs", Some(&other), None)
        .await;
    assert_eq!(list.body, json!([]));

    let deleted = app
        .request(
            Method::DELETE,
            &format!("/api/v1/graphs/{gid}/nodes/{a}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let list = app
        .request(Method::GET, "/api/v1/graphs", Some(&token), None)
        .await;
    assert_eq!(
        (
            list.body[0]["node_count"].as_i64(),
            list.body[0]["edge_count"].as_i64()
        ),
        (Some(2), Some(1))
    );
    let gone = app
        .request(
            Method::DELETE,
            &format!("/api/v1/graphs/{gid}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(gone.status, StatusCode::NO_CONTENT);
}

async fn wait_for_edges(app: &TestApp, token: &str, gid: &str, n: usize) -> Vec<Value> {
    for _ in 0..50 {
        let g = app
            .request(
                Method::GET,
                &format!("/api/v1/graphs/{gid}"),
                Some(token),
                None,
            )
            .await;
        let edges = g.body["edges"].as_array().unwrap().clone();
        if edges.len() == n {
            return edges;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("expected {n} edges");
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn wikilinks_create_and_remove_auto_edges(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("wiki@example.com").await;
    let gid = app.graph(&token, "").await;
    let intro = app
        .node(&token, &gid, json!({"title": "Introduction"}))
        .await;
    let body = app
        .node(
            &token,
            &gid,
            json!({"title": "Body", "content": "Builds on [[introduction]]."}),
        )
        .await;
    let edges = wait_for_edges(&app, &token, &gid, 1).await;
    assert_eq!(
        (edges[0]["source"].as_str(), edges[0]["target"].as_str()),
        (Some(intro.as_str()), Some(body.as_str()))
    );
    assert_eq!(edges[0]["origin"], "auto");
    assert_eq!(edges[0]["kind"], "depends_on");

    app.request(
        Method::PATCH,
        &format!("/api/v1/graphs/{gid}/nodes/{body}"),
        Some(&token),
        Some(json!({"content": "No links."})),
    )
    .await;
    wait_for_edges(&app, &token, &gid, 0).await;

    let s = app
        .request(
            Method::GET,
            &format!("/api/v1/graphs/{gid}/suggestions"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(s.status, StatusCode::OK);
    assert!(s.body.is_array());
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn templates_create_ready_graphs(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("tpl@example.com").await;
    let list = app
        .request(Method::GET, "/api/v1/templates", Some(&token), None)
        .await;
    let templates = list.body.as_array().unwrap();
    assert!(templates.len() >= 6);
    for id in [
        "research-report-docx",
        "rest-api-service",
        "market-analysis",
        "blog-series",
        "data-pipeline",
        "product-launch-plan",
    ] {
        assert!(templates.iter().any(|t| t["id"] == id), "{id}");
    }
    let r = app
        .request(
            Method::POST,
            "/api/v1/graphs/from-template",
            Some(&token),
            Some(json!({"template_id": "research-report-docx", "name": "My report"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["name"], "My report");
    assert!((5..=10).contains(&r.body["nodes"].as_array().unwrap().len()));
    assert!(!r.body["edges"].as_array().unwrap().is_empty());
    assert!(!r.body["goal"].as_str().unwrap().is_empty());
    let with_topic = app
        .request(
            Method::POST,
            "/api/v1/graphs/from-template",
            Some(&token),
            Some(json!({"template_id": "research-report-docx", "topic": "Solid-state batteries"})),
        )
        .await;
    assert_eq!(
        with_topic.status,
        StatusCode::CREATED,
        "{}",
        with_topic.body
    );
    assert_eq!(
        with_topic.body["name"],
        "Research report (DOCX) · Solid-state batteries"
    );
    assert!(
        with_topic.body["goal"]
            .as_str()
            .unwrap()
            .starts_with("Topic: Solid-state batteries\n\n"),
        "the topic leads the goal every prompt includes"
    );
    let missing = app
        .request(
            Method::POST,
            "/api/v1/graphs/from-template",
            Some(&token),
            Some(json!({"template_id": "nope"})),
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn http_hardening(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let health = app
        .request(Method::GET, "/api/v1/healthz", None, None)
        .await;
    assert_eq!(health.status, StatusCode::OK);
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        ("cross-origin-opener-policy", "same-origin"),
        ("cache-control", "no-store"),
    ] {
        assert_eq!(health.headers[name], value, "{name}");
    }
    assert!(
        health.headers["content-security-policy"]
            .to_str()
            .unwrap()
            .starts_with("default-src 'none'")
    );
    assert!(health.headers.contains_key("x-request-id"));
    assert_eq!(
        app.request(Method::GET, "/api/v1/readyz", None, None)
            .await
            .status,
        StatusCode::OK
    );

    let missing = app.request(Method::GET, "/api/v1/nope", None, None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(
        missing.headers[header::CONTENT_TYPE],
        "application/problem+json"
    );
    let wrong_method = app
        .request(Method::PUT, "/api/v1/healthz", None, None)
        .await;
    assert_eq!(wrong_method.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(wrong_method.body["status"], 405);

    let (token, _) = app.register("big@example.com").await;
    let huge = json!({"name": "x".repeat(2 * 1024 * 1024)});
    let too_big = app
        .request(Method::POST, "/api/v1/graphs", Some(&token), Some(huge))
        .await;
    assert_eq!(too_big.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(too_big.body["status"], 413);

    let bad_id = app
        .request(Method::GET, "/api/v1/graphs/not-a-uuid", Some(&token), None)
        .await;
    assert_eq!(bad_id.status, StatusCode::BAD_REQUEST);

    let spec = app
        .request(Method::GET, "/api/openapi.json", None, None)
        .await;
    assert_eq!(
        spec.body["openapi"].as_str().map(|v| v.starts_with("3.")),
        Some(true)
    );
    let docs = app.request(Method::GET, "/api/docs", None, None).await;
    assert_eq!(docs.status, StatusCode::OK);
    assert!(
        docs.headers["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("cdn.jsdelivr.net")
    );

    let metrics = app
        .request(Method::GET, "/metrics", Some(&token), None)
        .await;
    assert_eq!(
        metrics.status,
        StatusCode::FORBIDDEN,
        "metrics need an admin"
    );
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn llm_settings_store_encrypted_keys(pool: PgPool) {
    let app = TestApp::new(pool, &[("ANTHROPIC_API_KEY", "")]).await;
    let (token, _) = app.register("keys@example.com").await;
    let get = app
        .request(Method::GET, "/api/v1/settings/llm", Some(&token), None)
        .await;
    assert_eq!(
        (
            get.body["has_api_key"].as_bool(),
            get.body["source"].as_str()
        ),
        (Some(false), Some("none"))
    );

    let put = app
        .request(Method::PUT, "/api/v1/settings/llm", Some(&token), Some(json!({"provider": "anthropic", "model": "claude-opus-5", "api_key": "sk-ant-secret-a1b2"})))
        .await;
    assert_eq!(put.status, StatusCode::OK, "{}", put.body);
    assert_eq!(put.body["key_hint"], "…a1b2");
    assert_eq!(put.body["source"], "user");
    assert!(
        !put.body.to_string().contains("secret"),
        "the key is never returned"
    );
    let stored: Vec<u8> = sqlx::query_scalar("SELECT api_key_enc FROM llm_settings")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&stored).contains("sk-ant"),
        "stored encrypted"
    );

    let keep = app
        .request(
            Method::PUT,
            "/api/v1/settings/llm",
            Some(&token),
            Some(json!({"provider": "anthropic", "model": "claude-sonnet-5"})),
        )
        .await;
    assert_eq!(
        (
            keep.body["has_api_key"].as_bool(),
            keep.body["model"].as_str()
        ),
        (Some(true), Some("claude-sonnet-5"))
    );
    let cleared = app
        .request(
            Method::PUT,
            "/api/v1/settings/llm",
            Some(&token),
            Some(json!({"provider": "anthropic", "model": "claude-opus-5", "api_key": ""})),
        )
        .await;
    assert_eq!(cleared.body["has_api_key"], false);
}
