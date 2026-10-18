//! The OpenTelemetry export of the trace (AgentSecOps phase 4, roadmap
//! §4.2): with `NEXC_OTLP_ENDPOINT` set, a worker follows the spans written
//! and sends them to a collector as OTLP spans: the run as the trace, the
//! span's UUID v7 as its id, parents linked, attributes as recorded, each
//! workspace a resource; the cursor moves only when the collector took the
//! batch; a collector that is down is retried; the status says where it stands.
#![forbid(unsafe_code)]

mod common;

use std::sync::{Arc, Mutex};

use axum::http::{Method, StatusCode};
use common::{TestApp, collect_until};
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::{
    TraceService, TraceServiceServer,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::common::v1::any_value::Value as AnyKind;
use serde_json::{Value, json};
use sqlx::PgPool;
use tonic::{Request, Response, Status};
use uuid::Uuid;

/// A collector that keeps what it was sent, and can be told to refuse.
#[derive(Clone, Default)]
struct Collector {
    requests: Arc<Mutex<Vec<ExportTraceServiceRequest>>>,
    refuse: Arc<Mutex<bool>>,
}

#[tonic::async_trait]
impl TraceService for Collector {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        if *self.refuse.lock().unwrap() {
            return Err(Status::unavailable("the collector is full"));
        }
        self.requests.lock().unwrap().push(request.into_inner());
        Ok(Response::new(ExportTraceServiceResponse {
            partial_success: None,
        }))
    }
}

async fn collector() -> (String, Collector) {
    let collector = Collector::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    let service = collector.clone();
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(TraceServiceServer::new(service))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });
    (url, collector)
}

fn text_of(
    attributes: &[opentelemetry_proto::tonic::common::v1::KeyValue],
    key: &str,
) -> Option<String> {
    attributes.iter().find(|a| a.key == key).and_then(|a| {
        match a.value.as_ref().and_then(|v| v.value.as_ref()) {
            Some(AnyKind::StringValue(s)) => Some(s.clone()),
            Some(AnyKind::IntValue(i)) => Some(i.to_string()),
            _ => None,
        }
    })
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

#[sqlx::test(migrator = "nexc::repo::MIGRATOR")]
async fn spans_are_exported_as_otlp_traces_per_workspace(pool: PgPool) {
    let (url, collector) = collector().await;
    let app = TestApp::new(pool.clone(), &[("NEXC_OTLP_ENDPOINT", &url)]).await;
    let (token, _) = app.register("owner@example.com").await;
    let (_, list) = call(&app, Method::GET, "/workspaces", &token, None).await;
    let wid = list[0]["id"].as_str().unwrap().to_owned();
    let gid = app.graph(&token, "Export the trace").await;
    let first = app
        .node(
            &token,
            &gid,
            json!({"title": "Outline", "agent_role": "writer"}),
        )
        .await;
    let second = app
        .node(
            &token,
            &gid,
            json!({"title": "Draft", "agent_role": "writer"}),
        )
        .await;
    app.edge(&token, &gid, &first, &second).await;
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
    let events = collect_until(&mut rx, "run.finished").await;
    assert_eq!(events.last().unwrap().1["run"]["status"], "succeeded");
    let (_, page) = call(
        &app,
        Method::GET,
        &format!("/runs/{rid}/trace?limit=1000"),
        &token,
        None,
    )
    .await;
    let spans = page["spans"].as_array().unwrap().clone();
    assert!(spans.len() >= 4, "{spans:?}");

    // Nothing has gone out yet; one pass sends everything, in one batch for this workspace.
    let before = nexc::otlp::status(&app.state).await;
    assert!(before.enabled && before.reachable);
    assert_eq!(before.spans_exported, 0);
    assert_eq!(before.behind, spans.len() as i64);
    let progress = nexc::otlp::pass(&app.state).await.unwrap();
    assert_eq!(progress.spans, spans.len());
    assert_eq!(progress.batches, 1);
    assert!(!progress.failed && !progress.skipped);
    let requests = collector.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    let resources = &requests[0].resource_spans;
    assert_eq!(resources.len(), 1, "one resource for the workspace");
    let resource = resources[0].resource.as_ref().unwrap();
    assert_eq!(
        text_of(&resource.attributes, "service.name").as_deref(),
        Some("nexc-engine")
    );
    assert_eq!(
        text_of(&resource.attributes, "nexc.workspace_id").as_deref(),
        Some(wid.as_str())
    );
    assert!(text_of(&resource.attributes, "nexc.workspace_name").is_some());
    let scope = &resources[0].scope_spans[0];
    assert_eq!(scope.scope.as_ref().unwrap().name, "nexc.trace");
    assert_eq!(scope.spans.len(), spans.len());
    let run_id: Uuid = rid.parse().unwrap();
    for exported in &scope.spans {
        assert_eq!(
            exported.trace_id,
            run_id.as_bytes().to_vec(),
            "the run is the trace"
        );
        assert_eq!(exported.span_id.len(), 8);
        assert!(exported.end_time_unix_nano >= exported.start_time_unix_nano);
        assert!(text_of(&exported.attributes, "nexc.kind").is_some());
        assert!(text_of(&exported.attributes, "nexc.hash").is_some());
    }
    // Attributes travel as recorded, and a child links to its parent.
    let read = spans.iter().find(|s| s["kind"] == "data_read").unwrap();
    let exported_read = scope
        .spans
        .iter()
        .find(|s| s.span_id == nexc::otlp::span_id(read["id"].as_str().unwrap().parse().unwrap()))
        .unwrap();
    assert_eq!(exported_read.name, "context");
    assert_eq!(
        text_of(&exported_read.attributes, "documents").as_deref(),
        Some("0")
    );
    if let Some(child) = spans.iter().find(|s| s["parent_id"].is_string()) {
        let parent: Uuid = child["parent_id"].as_str().unwrap().parse().unwrap();
        let exported_child = scope
            .spans
            .iter()
            .find(|s| {
                s.span_id == nexc::otlp::span_id(child["id"].as_str().unwrap().parse().unwrap())
            })
            .unwrap();
        assert_eq!(exported_child.parent_span_id, nexc::otlp::span_id(parent));
    }
    let after = nexc::otlp::status(&app.state).await;
    assert_eq!(after.spans_exported, spans.len() as i64);
    assert_eq!(after.behind, 0);
    assert!(after.last_exported_at.is_some() && after.last_error.is_none());

    // Nothing more to send; the cursor holds.
    let again = nexc::otlp::pass(&app.state).await.unwrap();
    assert_eq!(again.spans, 0);
    assert_eq!(collector.requests.lock().unwrap().len(), 1);

    // A collector that refuses leaves the cursor where it was and notes the failure;
    // once it answers again, the spans written since go out.
    *collector.refuse.lock().unwrap() = true;
    let gid = app.graph(&token, "Export more").await;
    app.node(
        &token,
        &gid,
        json!({"title": "Later", "agent_role": "writer"}),
    )
    .await;
    let mut rx = app.events(&gid);
    call(
        &app,
        Method::POST,
        &format!("/graphs/{gid}/runs"),
        &token,
        Some(json!({})),
    )
    .await;
    collect_until(&mut rx, "run.finished").await;
    let failed = nexc::otlp::pass(&app.state).await.unwrap();
    assert!(failed.failed);
    assert_eq!(failed.spans, 0);
    let stuck = nexc::otlp::status(&app.state).await;
    assert!(stuck.behind > 0);
    assert!(
        stuck
            .last_error
            .as_deref()
            .unwrap_or("")
            .contains("the collector is full"),
        "{stuck:?}"
    );
    *collector.refuse.lock().unwrap() = false;
    let resumed = nexc::otlp::pass(&app.state).await.unwrap();
    assert_eq!(resumed.spans as i64, stuck.behind);
    assert_eq!(nexc::otlp::status(&app.state).await.behind, 0);
    assert_eq!(collector.requests.lock().unwrap().len(), 2);

    // Off: no endpoint, nothing goes out, the status says so.
    let off = TestApp::new(pool, &[]).await;
    let status = nexc::otlp::status(&off.state).await;
    assert!(!status.enabled && status.endpoint.is_none());
    assert_eq!(nexc::otlp::pass(&off.state).await.unwrap().spans, 0);
}
