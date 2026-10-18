//! Executor backed by the Python agent runtime (contract §8): the
//! `AgentRuntime/Execute` gRPC stream ([`super::runtime_grpc`]), or the
//! NDJSON stream of `POST /v1/execute` kept for one release. The events of
//! both wires mean the same and are handled here the same way.

use std::collections::HashMap;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::super::approvals::{self, Delivery, Granted};
use super::super::artifacts::MAX_ARTIFACT_BYTES;
use super::{ExecContext, ExecError, ExecOutput, NodeExecutor};
use crate::config::RuntimeProtocol;
use crate::domain::mcp::GatewayTool;
use crate::domain::policy::{Action, Effect};
use crate::domain::prompt::UpstreamOutput;
use crate::domain::recording::PayloadKind;
use crate::domain::trace::{self, SPAN_LINE_MAX_BYTES, SpanDraft, SpanKind, SpanStatus};
use crate::engine::recording::{self, Capture};
use crate::realtime::events::LogLevel;
use crate::security::agent_token::{AgentGrant, EXPIRY_SLACK, RecordingClaim};

/// Longest NDJSON line accepted (a base64 artifact of 20 MiB plus envelope).
const MAX_LINE_BYTES: usize = MAX_ARTIFACT_BYTES / 3 * 4 + 64 * 1024;
pub(super) const MAX_TURNS: u32 = 12;
/// Spans kept from one node attempt; a runtime that sends more is not believed.
const MAX_SPANS_PER_ATTEMPT: usize = 20_000;
/// How far a runtime span id's clock may stray from the attempt's.
const SPAN_CLOCK_SLACK_MS: i64 = 5 * 60 * 1000;

/// Executes nodes with `executor: "agent"`.
pub struct AgentExecutor;

impl NodeExecutor for AgentExecutor {
    fn execute<'a>(&'a self, ctx: &'a ExecContext) -> BoxFuture<'a, Result<ExecOutput, ExecError>> {
        Box::pin(run(ctx))
    }
}

#[derive(Debug, Deserialize)]
struct SpawnedAgent {
    name: String,
    role: String,
}

/// One line of the runtime's NDJSON stream.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RuntimeLine {
    Log {
        #[serde(default)]
        level: String,
        message: String,
    },
    Delta {
        text: String,
    },
    Tokens {
        #[serde(default)]
        input: i64,
        #[serde(default)]
        output: i64,
        /// Of `input`, the cache reads.
        #[serde(default)]
        cached: i64,
    },
    Spawn {
        agent: SpawnedAgent,
    },
    Artifact {
        path: String,
        #[serde(default)]
        mime: Option<String>,
        content_b64: String,
    },
    Result {
        output: String,
        #[serde(default)]
        tokens_in: i64,
        #[serde(default)]
        tokens_out: i64,
        #[serde(default)]
        tokens_cached: i64,
    },
    Error {
        message: String,
        #[serde(default)]
        retryable: bool,
    },
    /// One action of an agent (contract §8). Read field by field: a span
    /// that cannot be used is dropped with a warning, never fatal.
    Span(Map<String, Value>),
    /// What a span said, sent only while the token says recording is on.
    Payload {
        span_id: String,
        kind: String,
        content: String,
    },
    #[serde(other)]
    Unknown,
}

/// One event of the runtime, whatever wire it came over.
#[derive(Debug)]
pub(super) enum Event {
    Log {
        level: String,
        message: String,
    },
    Delta(String),
    Tokens {
        input: i64,
        output: i64,
        cached: i64,
    },
    Spawn {
        name: String,
        role: String,
    },
    Artifact {
        path: String,
        mime: Option<String>,
        /// The bytes, or why they could not be read.
        content: Result<Vec<u8>, String>,
    },
    Result {
        output: String,
        tokens_in: i64,
        tokens_out: i64,
        tokens_cached: i64,
    },
    Error {
        message: String,
        retryable: bool,
    },
    /// A span and its size on the wire.
    Span(Map<String, Value>, usize),
    /// A tool call the policy holds (gRPC); the NDJSON wire asks through its `policy` span.
    ApprovalRequest(approvals::Request),
    /// What a span said: the runtime's span id, the kind of payload, the content.
    Payload {
        span_id: String,
        kind: String,
        content: String,
    },
    Unknown,
}

impl From<RuntimeLine> for Event {
    fn from(line: RuntimeLine) -> Self {
        match line {
            RuntimeLine::Log { level, message } => Event::Log { level, message },
            RuntimeLine::Delta { text } => Event::Delta(text),
            RuntimeLine::Tokens {
                input,
                output,
                cached,
            } => Event::Tokens {
                input,
                output,
                cached,
            },
            RuntimeLine::Spawn { agent } => Event::Spawn {
                name: agent.name,
                role: agent.role,
            },
            RuntimeLine::Artifact {
                path,
                mime,
                content_b64,
            } => Event::Artifact {
                path,
                mime,
                content: base64::engine::general_purpose::STANDARD
                    .decode(content_b64.as_bytes())
                    .map_err(|e| format!("invalid base64: {e}")),
            },
            RuntimeLine::Result {
                output,
                tokens_in,
                tokens_out,
                tokens_cached,
            } => Event::Result {
                output,
                tokens_in,
                tokens_out,
                tokens_cached,
            },
            RuntimeLine::Error { message, retryable } => Event::Error { message, retryable },
            RuntimeLine::Span(wire) => Event::Span(wire, 0),
            RuntimeLine::Payload {
                span_id,
                kind,
                content,
            } => Event::Payload {
                span_id,
                kind,
                content,
            },
            RuntimeLine::Unknown => Event::Unknown,
        }
    }
}

/// What the stream of one attempt has said so far.
#[derive(Debug)]
pub struct Progress {
    usage: (i64, i64, i64),
    /// Spans kept so far.
    spans: usize,
    /// Runtime span id -> stored span id (they differ when the runtime's was unusable).
    pub(super) ids: HashMap<Uuid, Uuid>,
    /// When the attempt began (span ids must come from about then).
    began: DateTime<Utc>,
    /// Sub-agents each principal has spawned so far (the fan-out re-check).
    spawns: HashMap<String, u8>,
    /// Approvals granted during the attempt (a held tool may run once with one).
    pub(super) granted: Granted,
    /// Watchers of the approvals asked for; they settle when the attempt ends.
    watchers: Vec<JoinHandle<()>>,
    /// Gateway calls under way; they are dropped when the attempt ends.
    tasks: Vec<JoinHandle<()>>,
    stop: CancellationToken,
    /// How decisions reach the runtime.
    delivery: Delivery,
    /// Whether approvals are asked for by `approval_request` events (gRPC)
    /// rather than by `policy` spans (NDJSON).
    approvals_from_events: bool,
}

impl Progress {
    pub fn new(delivery: Delivery) -> Self {
        let approvals_from_events = matches!(delivery, Delivery::Stream(_));
        Progress {
            usage: (0, 0, 0),
            spans: 0,
            ids: HashMap::new(),
            began: Utc::now(),
            spawns: HashMap::new(),
            granted: Granted::default(),
            watchers: Vec::new(),
            tasks: Vec::new(),
            stop: CancellationToken::new(),
            delivery,
            approvals_from_events,
        }
    }

    /// Keeps a task of the attempt (a gateway call) to end with it.
    pub(super) fn track(&mut self, task: JoinHandle<()>) {
        self.tasks.retain(|t| !t.is_finished());
        self.tasks.push(task);
    }

    /// Ends the attempt: a still-pending approval expires (its watcher
    /// settles first), a gateway call still under way is dropped.
    pub(super) async fn close(self) {
        self.stop.cancel();
        for watcher in self.watchers {
            let _ = watcher.await;
        }
        for task in self.tasks {
            task.abort();
            let _ = task.await;
        }
    }
}

/// Kinds of span the runtime may report; the others are the backend's to record.
const RUNTIME_KINDS: [SpanKind; 4] = [
    SpanKind::LlmCall,
    SpanKind::ToolCall,
    SpanKind::Spawn,
    SpanKind::Policy,
];

/// Whether `reported` is `principal` itself or a sub-agent of it (`{principal}/...`).
fn is_principal_or_descendant(reported: &str, principal: &str) -> bool {
    reported == principal
        || reported
            .strip_prefix(principal)
            .is_some_and(|rest| rest.starts_with('/') && rest.len() > 1)
}

/// Whether a runtime-chosen id is a UUID v7 from about the time of the attempt.
fn plausible_id(id: Uuid, began: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    if id.get_version_num() != 7 {
        return false;
    }
    let Some(ts) = id.get_timestamp() else {
        return false;
    };
    let (secs, nanos) = ts.to_unix();
    let ms = secs as i64 * 1000 + i64::from(nanos / 1_000_000);
    ms >= began.timestamp_millis() - SPAN_CLOCK_SLACK_MS
        && ms <= now.timestamp_millis() + SPAN_CLOCK_SLACK_MS
}

/// A runtime span line as a draft of this node's chain. The backend decides
/// the node, run and workspace agent; the runtime only says what happened.
pub fn span_from_wire(
    wire: &Map<String, Value>,
    node_id: Uuid,
    agent_id: Option<Uuid>,
    principal: &str,
    line_bytes: usize,
    progress: &mut Progress,
) -> Result<SpanDraft, String> {
    let text = |key: &str| wire.get(key).and_then(Value::as_str);
    let kind_raw = text("kind").unwrap_or_default();
    let kind = kind_raw
        .parse::<SpanKind>()
        .ok()
        .filter(|k| RUNTIME_KINDS.contains(k))
        .ok_or_else(|| {
            format!(
                "span of unknown kind `{}` ignored",
                trace::clean_name(kind_raw)
            )
        })?;
    let uuid = |key: &str| text(key).and_then(|s| s.parse::<Uuid>().ok());
    let time = |key: &str| {
        text(key)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc))
    };
    let now = Utc::now();
    let mut attributes = if line_bytes > SPAN_LINE_MAX_BYTES {
        json!({"attributes_dropped": true, "line_bytes": line_bytes})
    } else {
        trace::clean_attributes(wire.get("attributes").unwrap_or(&Value::Null))
    };
    let reported = uuid("id");
    let id = match reported {
        Some(id) if !progress.ids.contains_key(&id) && plausible_id(id, progress.began, now) => id,
        _ => {
            let fresh = Uuid::now_v7();
            if let (Some(original), Value::Object(map)) = (reported, &mut attributes) {
                map.insert("runtime_id".into(), json!(original));
            }
            fresh
        }
    };
    if let Some(original) = reported {
        progress.ids.entry(original).or_insert(id);
    }
    let parent_id = uuid("parent_id").map(|p| progress.ids.get(&p).copied().unwrap_or(p));
    // The acting principal is the token's, or a sub-agent named under it; anything else
    // is attributed to the token's principal and flagged by the re-check.
    let reported = trace::clean_principal(text("agent").unwrap_or(principal));
    let principal = if is_principal_or_descendant(&reported, principal) {
        reported
    } else {
        if let Value::Object(map) = &mut attributes {
            map.insert("reported_principal".into(), json!(reported));
        }
        principal.to_owned()
    };
    Ok(SpanDraft {
        id,
        node_id,
        agent_id,
        principal,
        parent_id,
        kind,
        name: trace::clean_name(text("name").unwrap_or(kind.as_str())),
        started_at: time("started_at").unwrap_or(now),
        ended_at: time("ended_at"),
        status: match text("status") {
            Some("error") => SpanStatus::Error,
            _ => SpanStatus::Ok,
        },
        attributes,
    })
}

/// What the agent is given to read. Document passages travel apart from
/// memories: each is headed by its citation and the runtime gives them their
/// own section, so they do not compete with memories for room.
fn context(
    goal: &str,
    upstream: &[UpstreamOutput],
    memories: &[String],
    documents: &[String],
    labels: &[crate::domain::capabilities::DataLabel],
) -> serde_json::Value {
    json!({
        "goal": goal,
        "upstream": upstream.iter().map(|u| json!({
            "node_id": u.node_id, "title": u.title, "output": u.output,
        })).collect::<Vec<_>>(),
        "memories": memories,
        "documents": documents,
        "labels": labels,
    })
}

/// The per-run token: the principal, the run and node, the capabilities
/// snapshot and the policy version, valid for the node's timeout plus slack.
pub(super) fn mint_token(ctx: &ExecContext) -> Result<String, ExecError> {
    let principal = ctx.principal();
    let grant = AgentGrant {
        principal: &principal,
        run_id: ctx.run_id,
        node_id: ctx.node.id,
        agent_id: ctx.agent.as_ref().map(|a| a.id),
        workspace_id: ctx.workspace_id,
        policy_version: ctx.policy_version,
        capabilities: &ctx.capabilities,
        rules: &ctx.rules,
        labels: &ctx.labels_given,
        recording: RecordingClaim {
            prompts: ctx.recording.prompts,
            completions: ctx.recording.completions,
            tool_io: ctx.recording.tool_io,
        },
    };
    ctx.state
        .agent_tokens
        .issue(&grant, ctx.state.settings.node_timeout + EXPIRY_SLACK)
        .map_err(|e| ExecError::fatal(format!("cannot mint the agent token: {e}")))
}

/// Tokens the run may spend: what the agent has left, capped by the
/// policy's ceiling per run (0 means no agent budget).
pub(super) fn budget_tokens(ctx: &ExecContext) -> i64 {
    let left = ctx
        .agent
        .as_ref()
        .map_or(0, |a| (a.budget_tokens - a.spent_tokens).max(0));
    match ctx.rules.limits.tokens_per_run {
        Some(ceiling) if left == 0 || ceiling < left => ceiling,
        _ => left,
    }
}

/// Whether the node's type delivers a file and may run code.
pub(super) fn produces_artifact(ctx: &ExecContext) -> bool {
    ctx.node_type.as_ref().is_some_and(|t| t.produces_artifact)
}

pub(super) fn allow_code_exec(ctx: &ExecContext) -> bool {
    ctx.code_exec_allowed && ctx.node_type.as_ref().is_some_and(|t| t.allow_code_exec)
}

fn request_body(
    ctx: &ExecContext,
    token: &str,
    gateway_tools: &[GatewayTool],
) -> serde_json::Value {
    let target = ctx.agent_target();
    let agent = ctx.agent.as_ref();
    let settings = &ctx.state.settings;
    json!({
        "run_id": ctx.run_id,
        "node_id": ctx.node.id,
        "token": token,
        "agent": {
            "name": agent.map_or("assistant", |a| a.name.as_str()),
            "role": agent.map_or("assistant", |a| a.role.as_str()),
            "principal": ctx.principal(),
            "system_prompt": ctx.system_prompt(),
            "model": target.model,
            "budget_tokens": budget_tokens(ctx),
        },
        "task": {
            "title": ctx.node.title,
            "content": ctx.node.content,
            "kind": ctx.node.kind,
            "kind_description": ctx.node_type.as_ref().map_or("", |t| t.description.as_str()),
            "produces_artifact": produces_artifact(ctx),
        },
        "context": context(
            &ctx.goal,
            &ctx.upstream,
            &ctx.memories,
            &ctx.documents,
            &ctx.labels_given,
        ),
        "llm": {
            "provider": target.provider,
            "api_key": target.api_key.as_ref().map(|k| k.expose().as_str()),
            "model": target.model,
            "base_url": target.base_url,
        },
        "limits": {
            "max_turns": MAX_TURNS,
            "timeout_s": settings.node_timeout.as_secs(),
            "allow_code_exec": allow_code_exec(ctx),
        },
        "gateway_tools": gateway_tools,
    })
}

async fn run(ctx: &ExecContext) -> Result<ExecOutput, ExecError> {
    let token = mint_token(ctx)?;
    match ctx.state.settings.runtime_protocol {
        RuntimeProtocol::Grpc => super::runtime_grpc::run(ctx, token).await,
        RuntimeProtocol::Ndjson => run_ndjson(ctx, token).await,
    }
}

/// `POST /v1/execute`: the NDJSON stream. Gateway tools are not offered on
/// this wire (a tool request has no way back down).
async fn run_ndjson(ctx: &ExecContext, token: String) -> Result<ExecOutput, ExecError> {
    let settings = &ctx.state.settings;
    let url = format!("{}/v1/execute", settings.runtime_url.trim_end_matches('/'));
    let resp = ctx
        .state
        .http
        .post(&url)
        .bearer_auth(settings.runtime_token.expose())
        .json(&request_body(ctx, &token, &[]))
        .send()
        .await
        .map_err(|e| {
            ExecError::transient(format!("agent runtime unreachable: {}", e.without_url()))
        })?;
    let status = resp.status();
    if !status.is_success() {
        let message = format!("agent runtime returned HTTP {status}");
        return Err(if status.is_server_error() {
            ExecError::transient(message)
        } else {
            ExecError::fatal(message)
        });
    }
    let mut progress = Progress::new(Delivery::Http);
    let outcome = stream(ctx, resp, &mut progress).await;
    progress.close().await;
    outcome
}

/// Reads the runtime's stream to its terminal line.
async fn stream(
    ctx: &ExecContext,
    resp: reqwest::Response,
    progress: &mut Progress,
) -> Result<ExecOutput, ExecError> {
    let mut bytes = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = bytes.next().await {
        let chunk = chunk.map_err(|e| {
            ExecError::transient(format!("agent runtime stream failed: {}", e.without_url()))
        })?;
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            if let Some(done) = handle_line(ctx, &line, progress).await? {
                return Ok(done);
            }
        }
        if buf.len() > MAX_LINE_BYTES {
            return Err(ExecError::fatal("agent runtime sent an oversized line"));
        }
    }
    if let Some(done) = handle_line(ctx, &buf, progress).await? {
        return Ok(done);
    }
    Err(ExecError::transient(
        "agent runtime stream ended without a result",
    ))
}

/// Handles one NDJSON line; returns the output once the `result` line arrives.
async fn handle_line(
    ctx: &ExecContext,
    line: &[u8],
    progress: &mut Progress,
) -> Result<Option<ExecOutput>, ExecError> {
    let text = String::from_utf8_lossy(line);
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let parsed: RuntimeLine = serde_json::from_str(text)
        .map_err(|e| ExecError::fatal(format!("agent runtime sent invalid JSON: {e}")))?;
    let event = match parsed {
        RuntimeLine::Span(wire) => Event::Span(wire, line.len()),
        other => Event::from(other),
    };
    handle_event(ctx, event, progress).await
}

/// Handles one event of either wire; returns the output once the terminal
/// `result` arrives, an error on the terminal `error`.
pub(super) async fn handle_event(
    ctx: &ExecContext,
    event: Event,
    progress: &mut Progress,
) -> Result<Option<ExecOutput>, ExecError> {
    let usage = &mut progress.usage;
    match event {
        Event::Log { level, message } => ctx.log(parse_level(&level), message),
        Event::Delta(text) => ctx.output(&text),
        Event::Tokens {
            input,
            output,
            cached,
        } => {
            usage.0 += input.max(0);
            usage.1 += output.max(0);
            usage.2 += cached.max(0);
            ctx.tokens(usage.0, usage.1);
        }
        Event::Spawn { name, role } => {
            ctx.log(LogLevel::Info, format!("spawned sub-agent {name} ({role})"));
        }
        Event::Artifact {
            path,
            mime,
            content,
        } => match content {
            Ok(bytes) => {
                if let Err(err) = ctx.artifact(&path, mime.as_deref(), &bytes).await {
                    ctx.log(LogLevel::Warn, format!("artifact `{path}` rejected: {err}"));
                }
            }
            Err(err) => ctx.log(LogLevel::Warn, format!("artifact `{path}` rejected: {err}")),
        },
        Event::Result {
            output,
            tokens_in,
            tokens_out,
            tokens_cached,
        } => {
            let (tokens_in, tokens_out) = (tokens_in.max(usage.0), tokens_out.max(usage.1));
            let tokens_cached = tokens_cached.max(usage.2);
            ctx.tokens(tokens_in, tokens_out);
            return Ok(Some(ExecOutput {
                output,
                tokens_in,
                tokens_out,
                tokens_cached,
                span_id: None,
            }));
        }
        Event::Error { message, retryable } => {
            return Err(ExecError {
                message: format!("agent runtime: {message}"),
                retryable,
            });
        }
        Event::Span(wire, line_bytes) => {
            if progress.spans >= MAX_SPANS_PER_ATTEMPT {
                if progress.spans == MAX_SPANS_PER_ATTEMPT {
                    ctx.log(
                        LogLevel::Warn,
                        format!("more than {MAX_SPANS_PER_ATTEMPT} spans from one attempt; the rest are ignored"),
                    );
                    progress.spans += 1;
                }
                return Ok(None);
            }
            let agent_id = ctx.agent.as_ref().map(|a| a.id);
            match span_from_wire(
                &wire,
                ctx.node.id,
                agent_id,
                &ctx.principal(),
                line_bytes,
                progress,
            ) {
                Ok(span) => {
                    progress.spans += 1;
                    let mismatch = recheck(ctx, progress, &span);
                    let span_id = span.id;
                    let request = if progress.approvals_from_events {
                        None
                    } else {
                        approvals::Request::from_span(&span)
                    };
                    let egress = (span.kind == SpanKind::ToolCall)
                        .then(|| span.attributes.get("egress_host").and_then(Value::as_str))
                        .flatten()
                        .map(str::to_owned);
                    let injection = (span.kind == SpanKind::Policy
                        && span.attributes.get("rule").and_then(Value::as_str)
                            == Some("injection")
                        && span.attributes.get("severity").and_then(Value::as_str) == Some("high"))
                    .then(|| {
                        (
                            span.name.clone(),
                            span.attributes
                                .get("reason")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                        )
                    });
                    ctx.record(span);
                    if let Some((tool, reason)) = injection {
                        ctx.alert(&crate::engine::defend::Finding {
                            span_id: Some(span_id),
                            kind: crate::domain::alert::AlertKind::Injection,
                            severity: crate::domain::policy_document::Severity::High,
                            rule: "injection",
                            title: &format!("instructions found in the result of {tool}"),
                            detail: &reason,
                        })
                        .await;
                    }
                    if let Some(host) = egress {
                        // The call already happened; what left is on the record either way.
                        ctx.screen_egress(&host, span_id).await;
                    }
                    if let Some((rule, detail)) = mismatch {
                        ctx.policy_mismatch(span_id, rule, detail).await;
                    } else if let Some(request) = request {
                        open_approval(ctx, request, progress).await;
                    }
                }
                Err(warning) => ctx.log(LogLevel::Warn, warning),
            }
        }
        Event::ApprovalRequest(request) => {
            // The request names the policy span the runtime recorded; its stored id.
            let mut request = request;
            request.span_id = progress
                .ids
                .get(&request.span_id)
                .copied()
                .unwrap_or(request.span_id);
            request.parent_id = request
                .parent_id
                .map(|p| progress.ids.get(&p).copied().unwrap_or(p));
            open_approval(ctx, request, progress).await;
        }
        Event::Payload {
            span_id,
            kind,
            content,
        } => {
            // Only what the token asked for is kept; a runtime that sends more is ignored.
            let Ok(kind) = kind.parse::<PayloadKind>() else {
                return Ok(None);
            };
            if !ctx.recording.wants(kind) {
                return Ok(None);
            }
            let Ok(reported) = span_id.parse::<Uuid>() else {
                return Ok(None);
            };
            let span_id = progress.ids.get(&reported).copied().unwrap_or(reported);
            recording::store(
                ctx,
                Capture {
                    span_id,
                    kind,
                    content,
                },
            )
            .await;
        }
        Event::Unknown => {}
    }
    Ok(None)
}

/// A tool call waits for a human: the node pauses until the decision.
async fn open_approval(ctx: &ExecContext, request: approvals::Request, progress: &mut Progress) {
    let watcher = approvals::open(
        ctx,
        request,
        progress.stop.clone(),
        progress.granted.clone(),
        progress.delivery.clone(),
    )
    .await;
    progress.watchers.extend(watcher);
}

/// Re-evaluates what the runtime reported against the capabilities the
/// backend minted for the node: the rule and the detail of a disagreement.
fn recheck(
    ctx: &ExecContext,
    progress: &mut Progress,
    span: &SpanDraft,
) -> Option<(&'static str, String)> {
    recheck_with(
        &|action| ctx.decide(action),
        &ctx.principal(),
        progress,
        span,
    )
}

/// The re-check with `decide` as the policy and `principal` as the token's
/// (the Agent Firewall API re-checks what external agents report the same way).
pub fn recheck_with(
    decide: &dyn Fn(&Action<'_>) -> crate::domain::policy::Decision,
    principal: &str,
    progress: &mut Progress,
    span: &SpanDraft,
) -> Option<(&'static str, String)> {
    let attrs = &span.attributes;
    let text = |key: &str| attrs.get(key).and_then(Value::as_str);
    let small = |key: &str| {
        attrs
            .get(key)
            .and_then(Value::as_u64)
            .map(|n| n.min(255) as u8)
    };
    if let Some(reported) = text("reported_principal") {
        return Some((
            "principal",
            format!(
                "the runtime acted as `{reported}`, which is not {principal} or a sub-agent of it"
            ),
        ));
    }
    let denied = |action: Action<'_>, what: String| {
        let decision = decide(&action);
        (!decision.allowed).then(|| (decision.rule, format!("{what}; {}", decision.reason)))
    };
    match span.kind {
        SpanKind::ToolCall => {
            let tool = text("tool").unwrap_or(&span.name);
            let decision = decide(&Action::Tool(tool));
            match decision.effect {
                // A held tool may run once a human said yes; the span names the approval.
                Effect::RequireApproval { .. } => {
                    let approved = text("approval_id")
                        .and_then(|id| id.parse::<Uuid>().ok())
                        .is_some_and(|id| {
                            progress.granted.lock().expect("granted set").contains(&id)
                        });
                    (!approved).then(|| {
                        (
                            decision.rule,
                            format!(
                                "the runtime ran tool {tool} without an approval; {}",
                                decision.reason
                            ),
                        )
                    })
                }
                _ => denied(Action::Tool(tool), format!("the runtime ran tool {tool}")),
            }
        }
        SpanKind::Spawn => {
            let role = text("role").unwrap_or_default();
            let depth = small("depth").unwrap_or(1);
            let children = progress.spawns.entry(span.principal.clone()).or_default();
            let action = Action::Spawn {
                role,
                depth,
                children: *children,
            };
            *children = children.saturating_add(1);
            denied(
                action,
                format!("the runtime spawned a {role} sub-agent at depth {depth}"),
            )
        }
        SpanKind::LlmCall => text("model").and_then(|model| {
            denied(
                Action::Model(model),
                format!("the runtime called model {model}"),
            )
        }),
        SpanKind::Policy => {
            // The runtime's own decision must be the one the backend would take.
            let subject = text("subject").unwrap_or(&span.name);
            let action = match text("action") {
                Some("tool_call") => Action::Tool(subject),
                Some("spawn") => Action::Spawn {
                    role: subject,
                    depth: small("depth").unwrap_or(1),
                    children: small("children").unwrap_or(0),
                },
                Some("model") => Action::Model(subject),
                Some("egress") => Action::Egress(subject),
                _ => return None,
            };
            let ours = decide(&action);
            let theirs = text("decision").unwrap_or("deny");
            (ours.verdict() != theirs).then(|| {
                (
                    ours.rule,
                    format!(
                        "the runtime decided `{theirs}` on {} {subject}; the policy says `{}` ({})",
                        action.kind(),
                        ours.verdict(),
                        ours.reason
                    ),
                )
            })
        }
        SpanKind::DataRead | SpanKind::Egress | SpanKind::Approval => None,
    }
}

fn parse_level(level: &str) -> LogLevel {
    match level {
        "debug" => LogLevel::Debug,
        "warn" | "warning" => LogLevel::Warn,
        "error" => LogLevel::Error,
        _ => LogLevel::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_lines() {
        let lines = [
            r#"{"type":"log","level":"info","message":"m"}"#,
            r#"{"type":"delta","text":"t"}"#,
            r#"{"type":"tokens","input":1,"output":2}"#,
            r#"{"type":"spawn","agent":{"name":"n","role":"r"}}"#,
            r#"{"type":"artifact","path":"a.md","mime":"text/markdown","content_b64":"aGk="}"#,
            r#"{"type":"result","output":"o","tokens_in":0,"tokens_out":0}"#,
            r#"{"type":"error","message":"boom","retryable":true}"#,
            r#"{"type":"future_thing","x":1}"#,
            r#"{"type":"span","kind":"tool_call","name":"write_file"}"#,
        ];
        let parsed: Vec<RuntimeLine> = lines
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(matches!(parsed[5], RuntimeLine::Result { .. }));
        assert!(matches!(
            parsed[6],
            RuntimeLine::Error {
                retryable: true,
                ..
            }
        ));
        assert!(matches!(parsed[7], RuntimeLine::Unknown));
        assert!(matches!(&parsed[8], RuntimeLine::Span(m) if m["name"] == "write_file"));
        assert_eq!(parse_level("warning"), LogLevel::Warn);
        let artifact: RuntimeLine = serde_json::from_str(lines[4]).unwrap();
        assert!(matches!(
            Event::from(artifact),
            Event::Artifact { content: Ok(bytes), .. } if bytes == b"hi"
        ));
    }

    #[test]
    fn runtime_spans_are_read_field_by_field() {
        let node = Uuid::now_v7();
        let agent = Some(Uuid::now_v7());
        let mut progress = Progress::new(Delivery::Http);
        let turn = Uuid::now_v7();
        let wire = |v: Value| v.as_object().unwrap().clone();
        let first = span_from_wire(
            &wire(json!({"id": turn.to_string(), "kind": "llm_call", "name": "m", "agent": "agent://ws/writer",
                "started_at": "2026-10-17T12:00:00.123456789Z", "status": "ok", "attributes": {"tokens_in": 3}})),
            node, agent, "agent://ws/writer", 100, &mut progress,
        )
        .unwrap();
        assert_eq!(
            (first.id, first.node_id, first.agent_id),
            (turn, node, agent)
        );
        assert_eq!(first.principal, "agent://ws/writer");
        assert_eq!(first.attributes["tokens_in"], 3);
        // A repeated id, a foreign one and garbage all get fresh ids; children follow the first.
        let again = span_from_wire(
            &wire(
                json!({"id": turn.to_string(), "kind": "tool_call", "parent_id": turn.to_string()}),
            ),
            node,
            agent,
            "system",
            100,
            &mut progress,
        )
        .unwrap();
        assert_ne!(again.id, turn);
        assert_eq!(again.parent_id, Some(turn));
        assert_eq!(again.attributes["runtime_id"], turn.to_string());
        let v4 = Uuid::from_u128(0x1234_5678_9abc_4def_8123_4567_89ab_cdef);
        let old = span_from_wire(
            &wire(json!({"id": v4.to_string(), "kind": "spawn"})),
            node,
            agent,
            "system",
            100,
            &mut progress,
        )
        .unwrap();
        assert_ne!(old.id, v4);
        assert_eq!(old.name, "spawn");
        assert_eq!(old.principal, "system");
        let child = span_from_wire(
            &wire(json!({"kind": "llm_call", "parent_id": v4.to_string()})),
            node,
            agent,
            "system",
            100,
            &mut progress,
        )
        .unwrap();
        assert_eq!(
            child.parent_id,
            Some(old.id),
            "a parent's replaced id is followed"
        );
        for kind in ["teleport", "approval", "data_read", ""] {
            let refused = span_from_wire(
                &wire(json!({"kind": kind})),
                node,
                agent,
                "system",
                100,
                &mut progress,
            );
            assert!(refused.is_err(), "{kind} is not the runtime's to report");
        }
        let big = span_from_wire(
            &wire(json!({"kind": "tool_call", "attributes": {"x": 1}})),
            node,
            agent,
            "system",
            SPAN_LINE_MAX_BYTES + 1,
            &mut progress,
        )
        .unwrap();
        assert_eq!(big.attributes["attributes_dropped"], true);
        assert!(big.attributes.get("x").is_none());
    }

    #[test]
    fn spans_are_attributed_to_the_tokens_principal_or_its_sub_agents() {
        let principal = "agent://ws/Writer";
        assert!(is_principal_or_descendant(principal, principal));
        assert!(is_principal_or_descendant(
            "agent://ws/Writer/researcher-1",
            principal
        ));
        assert!(!is_principal_or_descendant("agent://ws/Writer/", principal));
        assert!(!is_principal_or_descendant("agent://ws/Writers", principal));
        assert!(!is_principal_or_descendant(
            "agent://other/Writer",
            principal
        ));
        let (node, mut progress) = (Uuid::now_v7(), Progress::new(Delivery::Http));
        let wire = |v: Value| v.as_object().unwrap().clone();
        let child = span_from_wire(
            &wire(json!({"kind": "llm_call", "agent": "agent://ws/Writer/researcher-1"})),
            node,
            None,
            principal,
            100,
            &mut progress,
        )
        .unwrap();
        assert_eq!(child.principal, "agent://ws/Writer/researcher-1");
        assert!(child.attributes.get("reported_principal").is_none());
        let intruder = span_from_wire(
            &wire(json!({"kind": "tool_call", "name": "write_file", "agent": "agent://ws/Admin"})),
            node,
            None,
            principal,
            100,
            &mut progress,
        )
        .unwrap();
        assert_eq!(
            intruder.principal, principal,
            "attributed to the token's principal"
        );
        assert_eq!(
            intruder.attributes["reported_principal"],
            "agent://ws/Admin"
        );
        let policy = span_from_wire(
            &wire(
                json!({"kind": "policy", "name": "make_docx", "agent": principal,
                "attributes": {"decision": "deny", "rule": "tools.allow"}}),
            ),
            node,
            None,
            principal,
            100,
            &mut progress,
        )
        .unwrap();
        assert_eq!(
            policy.kind,
            SpanKind::Policy,
            "the runtime reports its denials"
        );
    }

    #[test]
    fn document_passages_travel_apart_from_memories() {
        let memories = ["The team writes in British English.".to_owned()];
        let documents = ["[customs.pdf, p. 17]\nGoods under 150 EUR are exempt.".to_owned()];
        let sent = context(
            "Ship the handbook",
            &[],
            &memories,
            &documents,
            &[crate::domain::capabilities::DataLabel::Internal],
        );
        assert_eq!(sent["memories"], json!(memories));
        assert_eq!(sent["documents"], json!(documents));
        assert_eq!(sent["goal"], "Ship the handbook");
        assert_eq!(sent["labels"], json!(["internal"]));
    }
}
