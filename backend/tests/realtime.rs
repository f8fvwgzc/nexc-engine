//! Realtime: single-use tickets, the SSE wire format and the WebSocket
//! protocol (node moves, presence, ping, broadcast of REST mutations).
#![forbid(unsafe_code)]

mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::TestApp;
use futures::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

async fn ticket(app: &TestApp, token: &str, gid: &str) -> String {
    let r = app
        .request(
            Method::POST,
            "/api/v1/realtime/tickets",
            Some(token),
            Some(json!({"graph_id": gid})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["expires_in"], 30);
    r.body["ticket"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn sse_stream_uses_tickets_and_the_contract_framing(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("sse@example.com").await;
    let gid = app.graph(&token, "").await;
    let (other, _) = app.register("sse-other@example.com").await;
    let denied = app
        .request(
            Method::POST,
            "/api/v1/realtime/tickets",
            Some(&other),
            Some(json!({"graph_id": gid})),
        )
        .await;
    assert_eq!(denied.status, StatusCode::NOT_FOUND);

    let t = ticket(&app, &token, &gid).await;
    let req = Request::get(format!("/api/v1/graphs/{gid}/events?ticket={t}"))
        .body(Body::empty())
        .unwrap();
    let resp = tower::ServiceExt::oneshot(app.router.clone(), req)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "text/event-stream");
    let mut body = resp.into_body();

    app.node(&token, &gid, json!({"title": "trigger"})).await;
    let plan = app
        .request(
            Method::POST,
            &format!("/api/v1/graphs/{gid}/plan"),
            Some(&token),
            Some(json!({})),
        )
        .await;
    assert_eq!(plan.status, StatusCode::ACCEPTED);

    let mut text = String::new();
    while !text.contains("event: plan.ready") {
        let frame = tokio::time::timeout(Duration::from_secs(10), body.frame())
            .await
            .expect("stream stalled");
        let frame = frame.expect("stream open").unwrap();
        if let Ok(data) = frame.into_data() {
            text.push_str(&String::from_utf8_lossy(&data));
        }
    }
    assert!(text.starts_with("retry: 3000\n"), "{text}");
    assert!(text.contains("event: heartbeat\ndata: {\"at\":"));
    assert!(text.contains("event: plan.started\ndata: {\"plan_id\":"));
    let ids: Vec<u64> = text
        .lines()
        .filter_map(|l| l.strip_prefix("id: ")?.parse().ok())
        .collect();
    assert!(
        ids.len() >= 4 && ids.windows(2).all(|w| w[0] < w[1]),
        "ids increase: {ids:?}"
    );

    let reused = app
        .request(
            Method::GET,
            &format!("/api/v1/graphs/{gid}/events?ticket={t}"),
            None,
            None,
        )
        .await;
    assert_eq!(
        reused.status,
        StatusCode::UNAUTHORIZED,
        "tickets are single use"
    );
    let t2 = ticket(&app, &token, &gid).await;
    let other_gid = app.graph(&token, "").await;
    let wrong = app
        .request(
            Method::GET,
            &format!("/api/v1/graphs/{other_gid}/events?ticket={t2}"),
            None,
            None,
        )
        .await;
    assert_eq!(
        wrong.status,
        StatusCode::UNAUTHORIZED,
        "tickets are bound to one graph"
    );
}

async fn next_json(
    ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    kind: &str,
) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("socket stalled")
            .unwrap()
            .unwrap();
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["type"] == kind {
                return v;
            }
        }
    }
}

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn websocket_collaboration(pool: PgPool) {
    let app = TestApp::new(pool, &[]).await;
    let (token, _) = app.register("ws@example.com").await;
    let gid = app.graph(&token, "").await;
    let nid = app.node(&token, &gid, json!({"title": "Movable"})).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = app.router.clone();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let t = ticket(&app, &token, &gid).await;
    let mut bad_origin = format!("ws://{addr}/api/v1/graphs/{gid}/ws?ticket={t}")
        .into_client_request()
        .unwrap();
    bad_origin
        .headers_mut()
        .insert("origin", "https://evil.example".parse().unwrap());
    assert!(
        tokio_tungstenite::connect_async(bad_origin).await.is_err(),
        "foreign origins are rejected"
    );

    let t = ticket(&app, &token, &gid).await;
    let mut req = format!("ws://{addr}/api/v1/graphs/{gid}/ws?ticket={t}")
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("origin", "http://localhost:5173".parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();

    ws.send(Message::Text(json!({"type": "ping"}).to_string().into()))
        .await
        .unwrap();
    assert_eq!(next_json(&mut ws, "pong").await, json!({"type": "pong"}));

    ws.send(Message::Text(
        json!({"type": "presence", "cursor": {"x": 1.0, "y": 2.0}})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let presence = next_json(&mut ws, "presence").await;
    assert_eq!(presence["cursor"], json!({"x": 1.0, "y": 2.0}));
    assert_eq!(presence["name"], "Test");

    ws.send(Message::Text(
        json!({"type": "node.move", "node_id": nid, "x": 42.0, "y": 7.0})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let moved = next_json(&mut ws, "node.upserted").await;
    assert_eq!(
        (moved["node"]["x"].as_f64(), moved["node"]["y"].as_f64()),
        (Some(42.0), Some(7.0))
    );
    let g = app
        .request(
            Method::GET,
            &format!("/api/v1/graphs/{gid}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(g.body["nodes"][0]["x"], 42.0, "moves are persisted");

    app.node(
        &token,
        &gid,
        json!({"title": "Second", "content": "see [[Movable]]"}),
    )
    .await;
    let created = next_json(&mut ws, "node.upserted").await;
    assert_eq!(created["node"]["title"], "Second");
    let edge = next_json(&mut ws, "edge.upserted").await;
    assert_eq!(edge["edge"]["origin"], "auto");
    let suggestions = next_json(&mut ws, "suggestions").await;
    assert!(suggestions["items"].is_array());
    ws.close(None).await.unwrap();
}
