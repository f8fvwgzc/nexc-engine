//! OpenTelemetry export of the trace spine (AgentSecOps phase 4, roadmap
//! §4.2): when `NEXC_OTLP_ENDPOINT` is set, a worker follows the spans
//! written to `agent_spans` with a cursor (one row, taken with `SKIP
//! LOCKED`, so instances share the work) and sends them in batches as OTLP
//! spans over gRPC (`opentelemetry-proto`, the wire format
//! opentelemetry-otlp speaks): the trace id is the run id, the span id the
//! tail of the span's UUID v7, parents link, attributes go as recorded, and
//! each workspace is a resource of its own.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::time::Duration;

use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_client::TraceServiceClient;
use opentelemetry_proto::tonic::common::v1::any_value::Value as AnyKind;
use opentelemetry_proto::tonic::common::v1::{
    AnyValue, ArrayValue, InstrumentationScope, KeyValue,
};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::span::SpanKind as OtlpKind;
use opentelemetry_proto::tonic::trace::v1::status::StatusCode;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span as OtlpSpan, Status};
use serde::Serialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tonic::transport::{Channel, Endpoint};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::trace::{Span, SpanKind, SpanStatus};
use crate::repo::otlp_export::{self as cursor, OtlpSpan as Exported};
use crate::rpc::ChannelSettings;

/// How often the cursor is looked at.
pub const TICK: Duration = Duration::from_secs(2);
/// Spans per batch.
pub const BATCH: i64 = 1000;
/// One export call.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The instrumentation scope every span is reported under.
pub const SCOPE: &str = "nexc.trace";

/// What one pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub spans: usize,
    pub batches: usize,
    /// True when the pass stopped on a failed export (left for the next pass).
    pub failed: bool,
    /// True when another instance held the cursor.
    pub skipped: bool,
}

/// Where the export stands, for the operator.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct OtlpStatus {
    pub enabled: bool,
    #[schema(required = true)]
    pub endpoint: Option<String>,
    /// Whether the collector accepted a connection just now.
    pub reachable: bool,
    pub spans_exported: i64,
    /// Spans written that have not gone out yet.
    pub behind: i64,
    #[schema(required = true)]
    pub last_exported_at: Option<chrono::DateTime<chrono::Utc>>,
    #[schema(required = true)]
    pub last_error: Option<String>,
}

/// A lazily connected channel to the collector with the default keepalive
/// and pool.
pub fn channel(endpoint: &str) -> anyhow::Result<Channel> {
    channel_with(endpoint, &ChannelSettings::default())
}

/// A lazily connected channel to the collector, kept as `settings` say
/// (`NEXC_GRPC_*`) for the life of the process: every pass exports over it.
pub fn channel_with(endpoint: &str, settings: &ChannelSettings) -> anyhow::Result<Channel> {
    Ok(crate::rpc::channel(
        endpoint,
        &settings.connect_timeout(CONNECT_TIMEOUT),
    )?)
}

/// Starts the worker when a collector is configured; returns whether it did.
pub fn spawn(state: &AppState, shutdown: CancellationToken) -> bool {
    if state.settings.otlp_endpoint.is_none() {
        return false;
    }
    tokio::spawn(run(state.clone(), shutdown));
    true
}

async fn run(state: AppState, shutdown: CancellationToken) {
    let mut ticker = tokio::time::interval(TICK);
    let mut failing = false;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = ticker.tick() => {
                match pass(&state).await {
                    Ok(progress) => {
                        if progress.failed && !failing {
                            tracing::warn!("OTLP export failed; retrying every {}s", TICK.as_secs());
                        } else if !progress.failed && failing {
                            tracing::info!("OTLP export resumed");
                        }
                        failing = progress.failed;
                        if progress.spans > 0 {
                            tracing::debug!(?progress, "OTLP export pass");
                        }
                    }
                    Err(err) => tracing::warn!(error = %err, "OTLP export pass failed"),
                }
            }
        }
    }
}

/// One pass: takes the cursor, sends every batch past it, moves the cursor
/// after each one. A failed export leaves the cursor where the last good
/// batch moved it.
pub async fn pass(state: &AppState) -> anyhow::Result<Progress> {
    let Some(channel) = &state.otlp_channel else {
        return Ok(Progress::default());
    };
    let mut progress = Progress::default();
    let mut tx = state.db.begin().await?;
    let Some(mut seq) = cursor::claim(&mut tx).await? else {
        progress.skipped = true;
        return Ok(progress);
    };
    let mut client = TraceServiceClient::new(channel.clone());
    loop {
        let spans = cursor::spans_after(&mut *tx, seq, BATCH).await?;
        let Some(last) = spans.last() else {
            break;
        };
        let last_seq = last.ingested_seq;
        let mut request = tonic::Request::new(to_request(&spans));
        request.set_timeout(EXPORT_TIMEOUT);
        let started = std::time::Instant::now();
        let exported = client.export(request).await;
        state
            .metrics
            .grpc_client("TraceService", "Export", started.elapsed());
        match exported {
            Ok(reply) => {
                let rejected = reply
                    .into_inner()
                    .partial_success
                    .map_or(0, |p| p.rejected_spans);
                if rejected > 0 {
                    tracing::warn!(rejected, "the collector rejected spans of a batch");
                }
            }
            Err(status) => {
                let error = format!("{}: {}", status.code(), status.message());
                cursor::note_error(&mut *tx, &error).await?;
                tx.commit().await?;
                progress.failed = true;
                return Ok(progress);
            }
        }
        cursor::advance(&mut *tx, last_seq, spans.len() as i64).await?;
        progress.spans += spans.len();
        progress.batches += 1;
        seq = last_seq;
        if (spans.len() as i64) < BATCH {
            break;
        }
    }
    tx.commit().await?;
    Ok(progress)
}

/// Whether the collector accepts a connection, and where the export stands.
pub async fn status(state: &AppState) -> OtlpStatus {
    let Some(endpoint) = &state.settings.otlp_endpoint else {
        return OtlpStatus::default();
    };
    let reachable = match Endpoint::from_shared(endpoint.clone()) {
        Ok(endpoint) => tokio::time::timeout(
            CONNECT_TIMEOUT,
            endpoint.connect_timeout(CONNECT_TIMEOUT).connect(),
        )
        .await
        .is_ok_and(|c| c.is_ok()),
        Err(_) => false,
    };
    let standing = cursor::status(&state.db).await.unwrap_or_default();
    OtlpStatus {
        enabled: true,
        endpoint: Some(endpoint.clone()),
        reachable,
        spans_exported: standing.spans_exported,
        behind: standing.behind,
        last_exported_at: standing.last_exported_at,
        last_error: standing.last_error,
    }
}

/// The trace id of a run: the run's UUID, all 16 bytes.
pub fn trace_id(run_id: Uuid) -> Vec<u8> {
    run_id.as_bytes().to_vec()
}

/// The span id of a span: the last 8 bytes of its UUID v7 (the random
/// part; the first 6 are the time, shared by spans of the same millisecond).
pub fn span_id(id: Uuid) -> Vec<u8> {
    id.as_bytes()[8..].to_vec()
}

fn any(value: &Value) -> Option<AnyValue> {
    let kind = match value {
        Value::Null => return None,
        Value::Bool(b) => AnyKind::BoolValue(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => AnyKind::IntValue(i),
            None => AnyKind::DoubleValue(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => AnyKind::StringValue(s.clone()),
        Value::Array(items) => AnyKind::ArrayValue(ArrayValue {
            values: items.iter().filter_map(any).collect(),
        }),
        Value::Object(_) => AnyKind::StringValue(value.to_string()),
    };
    Some(AnyValue { value: Some(kind) })
}

fn attribute(key: &str, value: &Value) -> Option<KeyValue> {
    Some(KeyValue {
        key: key.to_owned(),
        value: any(value),
        ..KeyValue::default()
    })
    .filter(|kv| kv.value.is_some())
}

fn text(key: &str, value: impl Into<String>) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(AnyKind::StringValue(value.into())),
        }),
        ..KeyValue::default()
    }
}

fn nanos(at: chrono::DateTime<chrono::Utc>) -> u64 {
    at.timestamp_nanos_opt().unwrap_or(0).max(0) as u64
}

/// One span of the trace spine as an OTLP span.
pub fn to_otlp(span: &Span) -> OtlpSpan {
    let kind = match span.kind {
        SpanKind::LlmCall | SpanKind::Egress => OtlpKind::Client,
        SpanKind::Approval => OtlpKind::Consumer,
        _ => OtlpKind::Internal,
    };
    let mut attributes: Vec<KeyValue> = vec![
        text("nexc.kind", span.kind.as_str()),
        text("nexc.principal", span.principal.clone()),
        text("nexc.run_id", span.run_id.to_string()),
        text("nexc.node_id", span.node_id.to_string()),
        text("nexc.hash", hex::encode(span.hash)),
        KeyValue {
            key: "nexc.seq".into(),
            value: Some(AnyValue {
                value: Some(AnyKind::IntValue(i64::from(span.seq))),
            }),
            ..KeyValue::default()
        },
    ];
    if let Some(agent) = span.agent_id {
        attributes.push(text("nexc.agent_id", agent.to_string()));
    }
    if let Value::Object(map) = &span.attributes {
        attributes.extend(map.iter().filter_map(|(k, v)| attribute(k, v)));
    }
    let ended = span.ended_at.unwrap_or(span.started_at);
    OtlpSpan {
        trace_id: trace_id(span.run_id),
        span_id: span_id(span.id),
        trace_state: String::new(),
        parent_span_id: span.parent_id.map(span_id).unwrap_or_default(),
        flags: 0,
        name: span.name.clone(),
        kind: kind as i32,
        start_time_unix_nano: nanos(span.started_at),
        end_time_unix_nano: nanos(ended),
        attributes,
        dropped_attributes_count: 0,
        events: Vec::new(),
        dropped_events_count: 0,
        links: Vec::new(),
        dropped_links_count: 0,
        status: Some(Status {
            message: String::new(),
            code: match span.status {
                SpanStatus::Ok => StatusCode::Ok as i32,
                SpanStatus::Error => StatusCode::Error as i32,
            },
        }),
    }
}

/// Spans grouped by workspace, each workspace a resource.
pub fn to_request(spans: &[Exported]) -> ExportTraceServiceRequest {
    let mut by_workspace: BTreeMap<Option<Uuid>, (String, Vec<OtlpSpan>)> = BTreeMap::new();
    for exported in spans {
        let entry = by_workspace
            .entry(exported.workspace_id)
            .or_insert_with(|| (exported.workspace_name.clone(), Vec::new()));
        entry.1.push(to_otlp(&exported.span));
    }
    ExportTraceServiceRequest {
        resource_spans: by_workspace
            .into_iter()
            .map(|(workspace_id, (name, spans))| {
                let mut attributes = vec![
                    text("service.name", "nexc-engine"),
                    text("service.version", env!("CARGO_PKG_VERSION")),
                ];
                if let Some(id) = workspace_id {
                    attributes.push(text("nexc.workspace_id", id.to_string()));
                    attributes.push(text("nexc.workspace_name", name));
                }
                ResourceSpans {
                    resource: Some(Resource {
                        attributes,
                        dropped_attributes_count: 0,
                        entity_refs: Vec::new(),
                    }),
                    scope_spans: vec![ScopeSpans {
                        scope: Some(InstrumentationScope {
                            name: SCOPE.into(),
                            version: env!("CARGO_PKG_VERSION").into(),
                            attributes: Vec::new(),
                            dropped_attributes_count: 0,
                        }),
                        spans,
                        schema_url: String::new(),
                    }],
                    schema_url: String::new(),
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::domain::trace::{GENESIS, SpanDraft, link};

    #[tokio::test]
    async fn spans_map_to_otlp_with_the_run_as_trace() {
        let run = Uuid::now_v7();
        let parent = Uuid::now_v7();
        let draft = SpanDraft {
            id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            agent_id: Some(Uuid::now_v7()),
            principal: "agent://w/a".into(),
            parent_id: Some(parent),
            kind: SpanKind::Egress,
            name: "api.example.com".into(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            status: SpanStatus::Error,
            attributes: json!({"host": "api.example.com", "bytes_in": 12, "ratio": "1.5",
                "ids": ["a", "b"], "flag": true, "none": null}),
        };
        let span = link(draft, run, 3, GENESIS);
        let otlp = to_otlp(&span);
        assert_eq!(otlp.trace_id, run.as_bytes().to_vec());
        assert_eq!(otlp.span_id.len(), 8);
        assert_eq!(otlp.parent_span_id, span_id(parent));
        assert_eq!(otlp.kind, OtlpKind::Client as i32);
        assert_eq!(otlp.status.unwrap().code, StatusCode::Error as i32);
        let keys: Vec<&str> = otlp.attributes.iter().map(|a| a.key.as_str()).collect();
        assert!(keys.contains(&"host") && keys.contains(&"nexc.seq") && keys.contains(&"ids"));
        assert!(!keys.contains(&"none"), "null attributes are dropped");
        let request = to_request(&[
            Exported {
                span: span.clone(),
                ingested_seq: 1,
                workspace_id: Some(Uuid::nil()),
                workspace_name: "Acme".into(),
            },
            Exported {
                span,
                ingested_seq: 2,
                workspace_id: None,
                workspace_name: String::new(),
            },
        ]);
        assert_eq!(
            request.resource_spans.len(),
            2,
            "one resource per workspace"
        );
        let named = request
            .resource_spans
            .iter()
            .find(|r| {
                r.resource.as_ref().is_some_and(|res| {
                    res.attributes
                        .iter()
                        .any(|a| a.key == "nexc.workspace_name")
                })
            })
            .unwrap();
        assert_eq!(named.scope_spans[0].scope.as_ref().unwrap().name, SCOPE);
        assert!(channel("http://127.0.0.1:4317").is_ok());
    }
}
