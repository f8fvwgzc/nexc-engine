//! The gRPC wire to the agent runtime: one `AgentRuntime/Execute` stream per
//! node attempt (contract §8). The backend sends the execute request first,
//! then commands as they arise (acks, approval decisions, tool results, a
//! cancel); the runtime sends events, handled exactly as the NDJSON lines
//! are, plus the two this wire adds: an `approval_request`, answered by a
//! human through the stream, and a `tool_request`, performed by the tools
//! gateway and answered with a `tool_result`.
//!
//! The stream is driven by a task of its own, so that when the attempt is
//! cancelled (the run cancelled, the agent quarantined) the runtime is told
//! with a `cancel` command and given a moment to answer, rather than finding
//! its connection reset.

use std::time::Duration;

use serde_json::{Map, Value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::sync::CancellationToken;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use tonic::{Code, Request, Status};
use uuid::Uuid;

use super::agent::{self, Event, Progress};
use super::{ExecContext, ExecError, ExecOutput};
use crate::domain::mcp::GatewayTool;
use crate::engine::approvals::{self, Delivery};
use crate::engine::artifacts::MAX_ARTIFACT_BYTES;
use crate::gateway;
use crate::ontology::json;
use crate::realtime::events::LogLevel;
use crate::rpc::runtime::agent_runtime_client::AgentRuntimeClient;
use crate::rpc::runtime::{
    self as pb, BackendCommand, RuntimeEvent, backend_command, runtime_event,
};

/// Largest event accepted from the runtime (an artifact plus its envelope).
const MAX_EVENT_BYTES: usize = MAX_ARTIFACT_BYTES + 1024 * 1024;
/// Opening the connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a cancelled attempt waits for the runtime's terminal event.
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// A lazily connected channel to the runtime's gRPC endpoint; a runtime
/// that is down is a failed attempt, never a failed start.
pub fn channel(url: &str) -> anyhow::Result<Channel> {
    Ok(Endpoint::from_shared(url.to_owned())?
        .connect_timeout(CONNECT_TIMEOUT)
        .tcp_nodelay(true)
        .http2_keep_alive_interval(Duration::from_secs(30))
        .keep_alive_timeout(Duration::from_secs(10))
        .keep_alive_while_idle(true)
        .connect_lazy())
}

/// The failure a status stands for: a runtime that is down, busy or
/// resetting is retried; one that refused the request is not.
fn error_of(status: &Status) -> ExecError {
    let message = format!(
        "agent runtime: {} ({})",
        if status.message().is_empty() {
            status.code().description()
        } else {
            status.message()
        },
        status.code()
    );
    match status.code() {
        Code::Unavailable
        | Code::DeadlineExceeded
        | Code::Cancelled
        | Code::Unknown
        | Code::Aborted
        | Code::ResourceExhausted
        | Code::Internal => ExecError::transient(message),
        _ => ExecError::fatal(message),
    }
}

/// The execute request as a message (the same fields as the NDJSON body).
fn execute_request(
    ctx: &ExecContext,
    token: &str,
    gateway_tools: &[GatewayTool],
) -> pb::ExecuteRequest {
    let target = ctx.agent_target();
    let agent = ctx.agent.as_ref();
    let settings = &ctx.state.settings;
    pb::ExecuteRequest {
        run_id: ctx.run_id.to_string(),
        node_id: ctx.node.id.to_string(),
        token: token.to_owned(),
        agent: Some(pb::AgentSpec {
            name: agent.map_or("assistant", |a| a.name.as_str()).to_owned(),
            role: agent.map_or("assistant", |a| a.role.as_str()).to_owned(),
            principal: ctx.principal(),
            system_prompt: ctx.system_prompt(),
            model: target.model.clone(),
            budget_tokens: agent::budget_tokens(ctx),
        }),
        task: Some(pb::Task {
            title: ctx.node.title.clone(),
            content: ctx.node.content.clone(),
            kind: ctx.node.kind.clone(),
            kind_description: ctx
                .node_type
                .as_ref()
                .map_or(String::new(), |t| t.description.clone()),
            produces_artifact: Some(agent::produces_artifact(ctx)),
        }),
        context: Some(pb::Context {
            goal: ctx.goal.clone(),
            upstream: ctx
                .upstream
                .iter()
                .map(|u| pb::Upstream {
                    node_id: u.node_id.to_string(),
                    title: u.title.clone(),
                    output: u.output.clone(),
                })
                .collect(),
            memories: ctx.memories.clone(),
            documents: ctx.documents.clone(),
            labels: ctx
                .labels_given
                .iter()
                .map(|l| l.as_str().to_owned())
                .collect(),
        }),
        llm: Some(pb::Llm {
            provider: target.provider.to_string(),
            api_key: target
                .api_key
                .as_ref()
                .map(|k| k.expose().clone())
                .unwrap_or_default(),
            model: target.model.clone(),
            base_url: target.base_url.clone().unwrap_or_default(),
        }),
        limits: Some(pb::Limits {
            max_turns: agent::MAX_TURNS,
            timeout_s: settings.node_timeout.as_secs().min(u64::from(u32::MAX)) as u32,
            allow_code_exec: agent::allow_code_exec(ctx),
        }),
        gateway_tools: gateway_tools
            .iter()
            .map(|t| pb::GatewayTool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: Some(json::to_struct(&t.input_schema)),
            })
            .collect(),
    }
}

fn command(inner: backend_command::Command) -> BackendCommand {
    BackendCommand {
        command: Some(inner),
    }
}

/// A span message as the map the NDJSON reader works on.
fn span_to_wire(span: pb::Span) -> Map<String, Value> {
    let mut map = Map::new();
    let text = |value: String| {
        if value.is_empty() {
            Value::Null
        } else {
            Value::String(value)
        }
    };
    map.insert("id".into(), text(span.id));
    map.insert("parent_id".into(), text(span.parent_id));
    map.insert("kind".into(), Value::String(span.kind));
    map.insert("name".into(), Value::String(span.name));
    map.insert("agent".into(), text(span.agent));
    map.insert("started_at".into(), text(span.started_at));
    map.insert("ended_at".into(), text(span.ended_at));
    map.insert("status".into(), Value::String(span.status));
    map.insert(
        "attributes".into(),
        span.attributes
            .as_ref()
            .map(json::from_struct)
            .unwrap_or(Value::Null),
    );
    map
}

/// Ends the stream's driver when the attempt's future is dropped.
struct StopOnDrop(CancellationToken);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Runs the node over the gRPC stream.
pub async fn run(ctx: &ExecContext, token: String) -> Result<ExecOutput, ExecError> {
    let state = &ctx.state;
    let Some(channel) = state.runtime_grpc.clone() else {
        return Err(ExecError::fatal(
            "the runtime's gRPC endpoint is not configured (NEXC_RUNTIME_GRPC_URL)",
        ));
    };
    let gateway_tools = gateway::tools_for(
        state,
        ctx.workspace_id,
        ctx.agent.as_ref().map(|a| a.id),
        &ctx.capabilities,
        &ctx.rules,
        &ctx.labels_given,
    )
    .await;
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<BackendCommand>();
    let _ = cmd_tx.send(command(backend_command::Command::Execute(execute_request(
        ctx,
        &token,
        &gateway_tools,
    ))));
    let mut request = Request::new(UnboundedReceiverStream::new(cmd_rx));
    let bearer: MetadataValue<_> = format!("Bearer {}", state.settings.runtime_token.expose())
        .parse()
        .map_err(|_| ExecError::fatal("the runtime token is not ASCII"))?;
    request.metadata_mut().insert("authorization", bearer);
    let mut client = AgentRuntimeClient::new(channel).max_decoding_message_size(MAX_EVENT_BYTES);
    let mut events = client
        .execute(request)
        .await
        .map_err(|s| error_of(&s))?
        .into_inner();

    // The driver owns the stream: it forwards events, and on cancellation sends
    // `cancel` and waits a moment for the runtime's terminal event.
    let stop = CancellationToken::new();
    let _guard = StopOnDrop(stop.clone());
    let (ev_tx, mut ev_rx) = mpsc::channel::<Result<RuntimeEvent, Status>>(64);
    let driver_tx = cmd_tx.clone();
    let driver_stop = stop.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = driver_stop.cancelled() => {
                    let _ = driver_tx.send(command(backend_command::Command::Cancel(pb::Cancel {
                        reason: "the node attempt ended".into(),
                    })));
                    let _ = tokio::time::timeout(CANCEL_GRACE, async {
                        while let Ok(Some(event)) = events.message().await {
                            if matches!(
                                event.event,
                                Some(runtime_event::Event::Result(_) | runtime_event::Event::Error(_))
                            ) {
                                break;
                            }
                        }
                    })
                    .await;
                    return;
                }
                next = events.message() => {
                    match next {
                        Ok(Some(event)) => {
                            if ev_tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                        Ok(None) => return,
                        Err(status) => {
                            let _ = ev_tx.send(Err(status)).await;
                            return;
                        }
                    }
                }
            }
        }
    });

    let mut progress = Progress::new(Delivery::Stream(cmd_tx.clone()));
    let outcome = loop {
        let Some(next) = ev_rx.recv().await else {
            break Err(ExecError::transient(
                "agent runtime stream ended without a result",
            ));
        };
        let event = match next {
            Ok(event) => event,
            Err(status) => break Err(error_of(&status)),
        };
        let bytes = prost::Message::encoded_len(&event);
        match event.event {
            Some(runtime_event::Event::ToolRequest(request)) => {
                tool_request(ctx, request, &mut progress, &cmd_tx);
            }
            Some(runtime_event::Event::ApprovalRequest(request)) => {
                let _ = cmd_tx.send(command(backend_command::Command::Ack(pb::Ack {
                    request_id: request.approval_id.clone(),
                })));
                let Some(approval_id) = request.approval_id.parse::<Uuid>().ok() else {
                    ctx.log(
                        LogLevel::Warn,
                        "an approval request without a usable id was ignored",
                    );
                    continue;
                };
                let event = Event::ApprovalRequest(approvals::Request {
                    approval_id,
                    span_id: request.span_id.parse().unwrap_or_else(|_| Uuid::now_v7()),
                    parent_id: request.turn_span_id.parse().ok(),
                    tool: request.tool,
                    rule: if request.rule.is_empty() {
                        "approvals.tools".into()
                    } else {
                        request.rule
                    },
                    reason: request.reason,
                    args_sha256: (!request.args_sha256.is_empty()).then_some(request.args_sha256),
                });
                match agent::handle_event(ctx, event, &mut progress).await {
                    Ok(None) => {}
                    Ok(Some(done)) => break Ok(done),
                    Err(err) => break Err(err),
                }
            }
            other => {
                let event = match other {
                    Some(runtime_event::Event::Log(log)) => Event::Log {
                        level: log.level,
                        message: log.message,
                    },
                    Some(runtime_event::Event::Delta(delta)) => Event::Delta(delta.text),
                    Some(runtime_event::Event::Tokens(tokens)) => Event::Tokens {
                        input: tokens.input,
                        output: tokens.output,
                        cached: tokens.cached,
                    },
                    Some(runtime_event::Event::Spawn(spawn)) => Event::Spawn {
                        name: spawn.name,
                        role: spawn.role,
                    },
                    Some(runtime_event::Event::Span(span)) => {
                        Event::Span(span_to_wire(span), bytes)
                    }
                    Some(runtime_event::Event::Artifact(artifact)) => Event::Artifact {
                        path: artifact.path,
                        mime: (!artifact.mime.is_empty()).then_some(artifact.mime),
                        content: Ok(artifact.content),
                    },
                    Some(runtime_event::Event::Result(result)) => Event::Result {
                        output: result.output,
                        tokens_in: result.tokens_in,
                        tokens_out: result.tokens_out,
                        tokens_cached: result.tokens_cached,
                    },
                    Some(runtime_event::Event::Error(error)) => Event::Error {
                        message: error.message,
                        retryable: error.retryable,
                    },
                    Some(
                        runtime_event::Event::ToolRequest(_)
                        | runtime_event::Event::ApprovalRequest(_),
                    )
                    | None => Event::Unknown,
                };
                match agent::handle_event(ctx, event, &mut progress).await {
                    Ok(None) => {}
                    Ok(Some(done)) => break Ok(done),
                    Err(err) => break Err(err),
                }
            }
        }
    };
    progress.close().await;
    outcome
}

/// Performs a gateway tool call in a task of its own and answers with a
/// `tool_result`; the request is acknowledged at once.
fn tool_request(
    ctx: &ExecContext,
    request: pb::ToolRequest,
    progress: &mut Progress,
    cmd_tx: &mpsc::UnboundedSender<BackendCommand>,
) {
    let _ = cmd_tx.send(command(backend_command::Command::Ack(pb::Ack {
        request_id: request.request_id.clone(),
    })));
    let parent = request
        .turn_span_id
        .parse::<Uuid>()
        .ok()
        .map(|id| progress.ids.get(&id).copied().unwrap_or(id));
    let arguments = request
        .arguments
        .as_ref()
        .map(json::from_struct)
        .unwrap_or_else(|| Value::Object(Map::new()));
    let approval_id = arguments
        .get("__approval_id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok());
    let mut arguments = arguments;
    if let Value::Object(map) = &mut arguments {
        map.remove("__approval_id");
    }
    let req = gateway::ToolRequest {
        request_id: request.request_id,
        tool: request.tool,
        arguments,
        agent: request.agent,
        parent,
        approval_id,
    };
    let light = ctx.clone_light();
    let granted = progress.granted.clone();
    let tx = cmd_tx.clone();
    progress.track(tokio::spawn(async move {
        let outcome = gateway::perform(&light, &req, &granted).await;
        let _ = tx.send(command(backend_command::Command::ToolResult(
            pb::ToolResult {
                request_id: req.request_id,
                output: outcome.output,
                is_error: outcome.is_error,
            },
        )));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn statuses_decide_whether_to_retry() {
        assert!(error_of(&Status::unavailable("down")).retryable);
        assert!(error_of(&Status::resource_exhausted("full")).retryable);
        assert!(error_of(&Status::internal("boom")).retryable);
        assert!(!error_of(&Status::unauthenticated("bad token")).retryable);
        assert!(!error_of(&Status::invalid_argument("bad request")).retryable);
        assert!(error_of(&Status::unknown("")).message.contains("Unknown"));
        assert!(channel("http://127.0.0.1:1").is_ok());
        assert!(channel("not a url").is_err());
    }

    #[test]
    fn span_messages_become_the_wire_map() {
        let wire = span_to_wire(pb::Span {
            id: "0199".into(),
            parent_id: String::new(),
            kind: "tool_call".into(),
            name: "write_file".into(),
            agent: "agent://w/a".into(),
            started_at: "2026-10-17T12:00:00Z".into(),
            ended_at: String::new(),
            status: "ok".into(),
            attributes: Some(json::to_struct(
                &serde_json::json!({"tool": "write_file", "n": 2}),
            )),
        });
        assert_eq!(wire["id"], "0199");
        assert!(wire["parent_id"].is_null() && wire["ended_at"].is_null());
        assert_eq!(wire["attributes"]["n"], 2);
    }
}
