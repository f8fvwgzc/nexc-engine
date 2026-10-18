//! An in-process stand-in for the Python agent runtime that speaks the gRPC
//! wire of CONTRACT §8 (`AgentRuntime/Execute`). The node title picks its
//! behaviour: `SLOW` keeps the stream open until a `cancel` arrives (or 20 s
//! pass), `APPROVAL` asks for an approval of `make_docx` and waits for the
//! decision, `DROP` ends the stream without a terminal event, `TOOL <name>`
//! asks the gateway to perform `<name>` with the request's arguments and
//! reports what came back, `BURST <n>` sends `n` deltas as fast as it can
//! (for `make prove-gateway`), `TREE` spawns a researcher who spawns a qa
//! (a family of three with turns, tool calls and a denial), `STOPPABLE`
//! spawns a researcher who works until a `cancel_agent` arrives, and
//! anything else is a well-behaved run of one turn, one tool call, one
//! artifact and a result.
#![allow(dead_code)]

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use futures::Stream;
use nexc::ontology::json;
use nexc::rpc::runtime::agent_runtime_server::{AgentRuntime, AgentRuntimeServer};
use nexc::rpc::runtime::{
    self as pb, BackendCommand, RuntimeEvent, backend_command, runtime_event,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tonic::{Request, Response, Status, Streaming};
use uuid::Uuid;

use super::RUNTIME_TOKEN;

/// What the stand-in saw: every execute request (as JSON) and the kinds of
/// the commands that came down, per run.
#[derive(Default)]
pub struct Seen {
    pub requests: Mutex<Vec<Value>>,
    pub commands: Mutex<Vec<String>>,
    /// What the gateway answered to the `TOOL` behaviour, in order: request id, output, is_error.
    pub tool_results: Mutex<Vec<(String, String, bool)>>,
    /// When each delta of a `BURST` was sent, in order.
    pub burst_sent: Mutex<Vec<Instant>>,
    /// The spawn span ids of the `cancel_agent` commands a `STOPPABLE` run received.
    pub cancelled_agents: Mutex<Vec<String>>,
}

#[derive(Clone)]
struct Stub {
    seen: Arc<Seen>,
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

fn event(inner: runtime_event::Event) -> RuntimeEvent {
    RuntimeEvent { event: Some(inner) }
}

pub fn span(kind: &str, name: &str, agent: &str, parent: &str, attrs: Value) -> pb::Span {
    pb::Span {
        id: Uuid::now_v7().to_string(),
        parent_id: parent.to_owned(),
        kind: kind.into(),
        name: name.into(),
        agent: agent.into(),
        started_at: now(),
        ended_at: now(),
        status: "ok".into(),
        attributes: Some(json::to_struct(&attrs)),
    }
}

fn request_json(req: &pb::ExecuteRequest) -> Value {
    let agent = req.agent.clone().unwrap_or_default();
    let task = req.task.clone().unwrap_or_default();
    let context = req.context.clone().unwrap_or_default();
    let llm = req.llm.clone().unwrap_or_default();
    let limits = req.limits.unwrap_or_default();
    json!({
        "run_id": req.run_id,
        "node_id": req.node_id,
        "token": req.token,
        "agent": {"name": agent.name, "role": agent.role, "principal": agent.principal,
            "system_prompt": agent.system_prompt, "model": agent.model, "budget_tokens": agent.budget_tokens},
        "task": {"title": task.title, "content": task.content, "kind": task.kind,
            "kind_description": task.kind_description, "produces_artifact": task.produces_artifact},
        "context": {"goal": context.goal, "labels": context.labels,
            "upstream": context.upstream.iter().map(|u| json!({"node_id": u.node_id, "title": u.title, "output": u.output})).collect::<Vec<_>>(),
            "memories": context.memories, "documents": context.documents},
        "llm": {"provider": llm.provider, "api_key": llm.api_key, "model": llm.model, "base_url": llm.base_url},
        "limits": {"max_turns": limits.max_turns, "timeout_s": limits.timeout_s, "allow_code_exec": limits.allow_code_exec},
        "gateway_tools": req.gateway_tools.iter().map(|t| json!({"name": t.name, "description": t.description,
            "input_schema": t.input_schema.as_ref().map(json::from_struct)})).collect::<Vec<_>>(),
    })
}

type EventStream = Pin<Box<dyn Stream<Item = Result<RuntimeEvent, Status>> + Send>>;

#[tonic::async_trait]
impl AgentRuntime for Stub {
    type ExecuteStream = EventStream;

    async fn health(
        &self,
        _: Request<pb::HealthRequest>,
    ) -> Result<Response<pb::HealthReply>, Status> {
        Ok(Response::new(pb::HealthReply {
            status: "ok".into(),
            version: "stub".into(),
            agents_loaded: 0,
            runs_active: 0,
        }))
    }

    async fn execute(
        &self,
        request: Request<Streaming<BackendCommand>>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        let authorized = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            == Some(&format!("Bearer {RUNTIME_TOKEN}"));
        if !authorized {
            return Err(Status::unauthenticated("invalid or missing bearer token"));
        }
        let mut commands = request.into_inner();
        let first = commands
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("no command"))?;
        let Some(backend_command::Command::Execute(req)) = first.command else {
            return Err(Status::invalid_argument(
                "the first command must be the execute request",
            ));
        };
        self.seen.requests.lock().unwrap().push(request_json(&req));
        let seen = self.seen.clone();
        let (tx, rx) = mpsc::channel::<Result<RuntimeEvent, Status>>(64);
        tokio::spawn(async move {
            let send = |e: runtime_event::Event| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(Ok(event(e))).await;
                }
            };
            let title = req
                .task
                .as_ref()
                .map(|t| t.title.clone())
                .unwrap_or_default();
            let agent = req
                .agent
                .as_ref()
                .map(|a| a.principal.clone())
                .unwrap_or_default();
            let turn = span(
                "llm_call",
                "claude-test",
                &agent,
                "",
                json!({"provider": "anthropic", "model": "claude-test", "turn": 1}),
            );
            let turn_id = turn.id.clone();
            send(runtime_event::Event::Log(pb::Log {
                level: "info".into(),
                message: format!("stub runtime took {title}"),
            }))
            .await;
            send(runtime_event::Event::Span(turn)).await;
            let result = runtime_event::Event::Result(pb::Result {
                output: format!("[runtime] {title}"),
                tokens_in: 10,
                tokens_out: 5,
                tokens_cached: 0,
            });
            // Takes commands until one of `wanted` arrives; records every kind.
            let next_command = |wanted: &'static [&'static str]| {
                let seen = seen.clone();
                async move {
                    loop {
                        match commands.message().await {
                            Ok(Some(command)) => {
                                let kind = match &command.command {
                                    Some(backend_command::Command::Cancel(_)) => "cancel",
                                    Some(backend_command::Command::Ack(_)) => "ack",
                                    Some(backend_command::Command::ApprovalDecision(_)) => {
                                        "approval_decision"
                                    }
                                    Some(backend_command::Command::PolicyUpdate(_)) => {
                                        "policy_update"
                                    }
                                    Some(backend_command::Command::ToolResult(_)) => "tool_result",
                                    Some(backend_command::Command::CancelAgent(_)) => {
                                        "cancel_agent"
                                    }
                                    Some(backend_command::Command::Execute(_)) => "execute",
                                    None => "empty",
                                };
                                seen.commands.lock().unwrap().push(kind.to_owned());
                                if wanted.contains(&kind) {
                                    return Some(command);
                                }
                            }
                            _ => return None,
                        }
                    }
                }
            };
            if title.contains("TREE") {
                // A family: the root spawns a researcher, who spawns a qa; each works a turn.
                let child = format!("{agent}/researcher-1");
                let grandchild = format!("{child}/qa-1");
                let model = |tokens_in: i64, tokens_out: i64, cached: i64| {
                    json!({"provider": "anthropic", "model": "claude-sonnet-5", "turn": 1,
                        "tokens_in": tokens_in, "tokens_out": tokens_out, "tokens_cached": cached})
                };
                let spawn_a = span(
                    "spawn",
                    &child,
                    &agent,
                    &turn_id,
                    json!({"child": child, "role": "researcher", "depth": 1, "budget_tokens": 50_000}),
                );
                let spawn_a_id = spawn_a.id.clone();
                send(runtime_event::Event::Span(spawn_a)).await;
                let a_turn = span(
                    "llm_call",
                    "claude-sonnet-5",
                    &child,
                    &spawn_a_id,
                    model(50, 5, 10),
                );
                let a_turn_id = a_turn.id.clone();
                send(runtime_event::Event::Span(a_turn)).await;
                let spawn_b = span(
                    "spawn",
                    &grandchild,
                    &child,
                    &a_turn_id,
                    json!({"child": grandchild, "role": "qa", "depth": 2, "budget_tokens": 20_000}),
                );
                let spawn_b_id = spawn_b.id.clone();
                send(runtime_event::Event::Span(spawn_b)).await;
                let b_turn = span(
                    "llm_call",
                    "claude-sonnet-5",
                    &grandchild,
                    &spawn_b_id,
                    model(20, 2, 0),
                );
                let b_turn_id = b_turn.id.clone();
                send(runtime_event::Event::Span(b_turn)).await;
                send(runtime_event::Event::Span(span(
                    "tool_call",
                    "write_file",
                    &grandchild,
                    &b_turn_id,
                    json!({"tool": "write_file", "args_sha256": "ab".repeat(32), "args_bytes": 12,
                        "result_bytes": 4, "is_error": false, "duration_ns": 1000}),
                )))
                .await;
                send(runtime_event::Event::Span(span(
                    "tool_call",
                    "spawn_subagent",
                    &child,
                    &a_turn_id,
                    json!({"tool": "spawn_subagent", "args_sha256": "cd".repeat(32), "args_bytes": 40,
                        "result_bytes": 9, "is_error": false, "duration_ns": 5000}),
                )))
                .await;
                let mut denied = span(
                    "policy",
                    "make_docx",
                    &child,
                    &a_turn_id,
                    json!({"decision": "deny", "rule": "tools.allow", "reason": "tool make_docx is not allowed",
                        "action": "tool_call", "subject": "make_docx", "policy_version": 0}),
                );
                denied.status = "error".into();
                send(runtime_event::Event::Span(denied)).await;
                send(runtime_event::Event::Span(span(
                    "tool_call",
                    "spawn_subagent",
                    &agent,
                    &turn_id,
                    json!({"tool": "spawn_subagent", "args_sha256": "ef".repeat(32), "args_bytes": 40,
                        "result_bytes": 12, "is_error": false, "duration_ns": 9000}),
                )))
                .await;
                let mut second = span("llm_call", "claude-sonnet-5", &agent, "", model(30, 3, 0));
                second.attributes = Some(json::to_struct(&json!({"provider": "anthropic",
                    "model": "claude-sonnet-5", "turn": 2, "tokens_in": 30, "tokens_out": 3, "tokens_cached": 0})));
                send(runtime_event::Event::Span(second)).await;
                send(runtime_event::Event::Tokens(pb::Tokens {
                    input: 100,
                    output: 10,
                    cached: 10,
                }))
                .await;
                send(runtime_event::Event::Result(pb::Result {
                    output: format!("[runtime] {title}"),
                    tokens_in: 100,
                    tokens_out: 10,
                    tokens_cached: 10,
                }))
                .await;
                return;
            }
            if title.contains("STOPPABLE") {
                // The root spawns a researcher who works until an operator stops it (or 20 s
                // pass); the root then sees the tool error and finishes the node.
                let child = format!("{agent}/researcher-1");
                let spawn_a = span(
                    "spawn",
                    &child,
                    &agent,
                    &turn_id,
                    json!({"child": child, "role": "researcher", "depth": 1, "budget_tokens": 50_000}),
                );
                let spawn_a_id = spawn_a.id.clone();
                send(runtime_event::Event::Span(spawn_a)).await;
                let a_turn = span(
                    "llm_call",
                    "claude-sonnet-5",
                    &child,
                    &spawn_a_id,
                    json!({"provider": "anthropic", "model": "claude-sonnet-5", "turn": 1,
                        "tokens_in": 50, "tokens_out": 5, "tokens_cached": 0}),
                );
                send(runtime_event::Event::Span(a_turn)).await;
                let stopped =
                    tokio::time::timeout(Duration::from_secs(20), next_command(&["cancel_agent"]))
                        .await;
                let (is_error, output) = match stopped {
                    Ok(Some(BackendCommand {
                        command: Some(backend_command::Command::CancelAgent(cancel)),
                    })) => {
                        seen.cancelled_agents
                            .lock()
                            .unwrap()
                            .push(cancel.spawn_span_id.clone());
                        send(runtime_event::Event::Log(pb::Log {
                            level: "warning".into(),
                            message: format!(
                                "{child} stopped: cancelled by an operator{}",
                                if cancel.reason.is_empty() {
                                    String::new()
                                } else {
                                    format!(": {}", cancel.reason)
                                }
                            ),
                        }))
                        .await;
                        (true, format!("[runtime] went on without {child}"))
                    }
                    _ => (false, format!("[runtime] {child} finished on its own")),
                };
                send(runtime_event::Event::Span(span(
                    "tool_call",
                    "spawn_subagent",
                    &agent,
                    &turn_id,
                    json!({"tool": "spawn_subagent", "args_sha256": "ef".repeat(32), "args_bytes": 40,
                        "result_bytes": 30, "is_error": is_error, "duration_ns": 9000}),
                )))
                .await;
                let mut second = span("llm_call", "claude-sonnet-5", &agent, "", json!({}));
                second.attributes = Some(json::to_struct(&json!({"provider": "anthropic",
                    "model": "claude-sonnet-5", "turn": 2, "tokens_in": 30, "tokens_out": 3, "tokens_cached": 0})));
                send(runtime_event::Event::Span(second)).await;
                send(runtime_event::Event::Result(pb::Result {
                    output,
                    tokens_in: 80,
                    tokens_out: 8,
                    tokens_cached: 0,
                }))
                .await;
                return;
            }
            if title.contains("SLOW") {
                send(runtime_event::Event::Delta(pb::Delta {
                    text: "working".into(),
                }))
                .await;
                let cancelled =
                    tokio::time::timeout(Duration::from_secs(20), next_command(&["cancel"])).await;
                match cancelled {
                    Ok(Some(_)) => {
                        send(runtime_event::Event::Error(pb::Error {
                            message: "run cancelled: the backend asked".into(),
                            retryable: true,
                        }))
                        .await;
                    }
                    _ => send(result).await,
                }
                return;
            }
            if title.contains("DROP") {
                send(runtime_event::Event::Delta(pb::Delta {
                    text: "about to vanish".into(),
                }))
                .await;
                // The stream ends here, without a terminal event.
                return;
            }
            if title.contains("APPROVAL") {
                let approval_id = Uuid::now_v7().to_string();
                let request_span = span(
                    "policy",
                    "make_docx",
                    &agent,
                    &turn_id,
                    json!({"decision": "require_approval", "rule": "approvals.tools",
                        "reason": "tool make_docx needs an approval", "action": "tool_call",
                        "subject": "make_docx", "approval_id": approval_id,
                        "args_sha256": "ab".repeat(32), "policy_version": 0}),
                );
                let request_span_id = request_span.id.clone();
                send(runtime_event::Event::Span(request_span)).await;
                send(runtime_event::Event::ApprovalRequest(pb::ApprovalRequest {
                    approval_id: approval_id.clone(),
                    tool: "make_docx".into(),
                    rule: "approvals.tools".into(),
                    reason: "tool make_docx needs an approval".into(),
                    args_sha256: "ab".repeat(32),
                    span_id: request_span_id,
                    agent: agent.clone(),
                    turn_span_id: turn_id.clone(),
                }))
                .await;
                let decided = tokio::time::timeout(
                    Duration::from_secs(20),
                    next_command(&["approval_decision"]),
                )
                .await;
                match decided {
                    Ok(Some(BackendCommand {
                        command: Some(backend_command::Command::ApprovalDecision(decision)),
                    })) if decision.decision == "approved" => {
                        send(runtime_event::Event::Span(span(
                            "tool_call",
                            "make_docx",
                            &agent,
                            &turn_id,
                            json!({"tool": "make_docx", "approval_id": decision.approval_id}),
                        )))
                        .await;
                        send(runtime_event::Event::Artifact(pb::Artifact {
                            path: "report.docx".into(),
                            mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into(),
                            content: b"PK demo".to_vec(),
                        }))
                        .await;
                        send(result).await;
                    }
                    Ok(Some(BackendCommand {
                        command: Some(backend_command::Command::ApprovalDecision(decision)),
                    })) => {
                        send(runtime_event::Event::Log(pb::Log {
                            level: "warning".into(),
                            message: format!("make_docx denied: {}", decision.reason),
                        }))
                        .await;
                        send(result).await;
                    }
                    _ => {
                        send(runtime_event::Event::Error(pb::Error {
                            message: "approval for make_docx not given within 20s".into(),
                            retryable: false,
                        }))
                        .await;
                    }
                }
                return;
            }
            if title.contains("DOCX") {
                // A document is asked for: one make_docx call, the file, the answer.
                send(runtime_event::Event::Span(span(
                    "tool_call",
                    "make_docx",
                    &agent,
                    &turn_id,
                    json!({"tool": "make_docx", "args_sha256": "cd".repeat(32), "args_bytes": 40,
                        "result_bytes": 7, "is_error": false, "duration_ns": 2000}),
                )))
                .await;
                send(runtime_event::Event::Artifact(pb::Artifact {
                    path: "report.docx".into(),
                    mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                        .into(),
                    content: b"PK demo".to_vec(),
                }))
                .await;
                send(runtime_event::Event::Result(pb::Result {
                    output: format!("[runtime] wrote report.docx for: {title}"),
                    tokens_in: 10,
                    tokens_out: 5,
                    tokens_cached: 0,
                }))
                .await;
                return;
            }
            if let Some(tool) = title.strip_prefix("TOOL ") {
                let tool = tool.trim().to_owned();
                // The arguments are the content as JSON, or the JSON object it ends with.
                let content = req
                    .task
                    .as_ref()
                    .map(|t| t.content.clone())
                    .unwrap_or_default();
                let arguments: Value = serde_json::from_str(&content)
                    .or_else(|_| {
                        serde_json::from_str(content.find('{').map_or("", |at| &content[at..]))
                    })
                    .unwrap_or_else(|_| json!({}));
                let request_id = Uuid::now_v7().to_string();
                send(runtime_event::Event::ToolRequest(pb::ToolRequest {
                    request_id: request_id.clone(),
                    tool: tool.clone(),
                    arguments: Some(json::to_struct(&arguments)),
                    agent: agent.clone(),
                    turn_span_id: turn_id.clone(),
                }))
                .await;
                let answered =
                    tokio::time::timeout(Duration::from_secs(20), next_command(&["tool_result"]))
                        .await;
                let output = match answered {
                    Ok(Some(BackendCommand {
                        command: Some(backend_command::Command::ToolResult(result)),
                    })) => {
                        seen.tool_results.lock().unwrap().push((
                            request_id,
                            result.output.clone(),
                            result.is_error,
                        ));
                        format!(
                            "[runtime] {tool} {}: {}",
                            if result.is_error {
                                "failed"
                            } else {
                                "answered"
                            },
                            result.output
                        )
                    }
                    _ => "[runtime] the gateway did not answer".to_owned(),
                };
                send(runtime_event::Event::Result(pb::Result {
                    output,
                    tokens_in: 10,
                    tokens_out: 5,
                    tokens_cached: 0,
                }))
                .await;
                return;
            }
            if let Some(n) = title
                .strip_prefix("BURST ")
                .and_then(|n| n.trim().parse::<usize>().ok())
            {
                for i in 0..n {
                    // Paced so a subscriber of the graph keeps up with the fan-out.
                    if i % 32 == 31 {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    } else {
                        tokio::task::yield_now().await;
                    }
                    seen.burst_sent.lock().unwrap().push(Instant::now());
                    send(runtime_event::Event::Delta(pb::Delta {
                        text: format!("d{i} "),
                    }))
                    .await;
                }
                send(result).await;
                return;
            }
            send(runtime_event::Event::Delta(pb::Delta {
                text: format!("[runtime] {title}"),
            }))
            .await;
            send(runtime_event::Event::Tokens(pb::Tokens {
                input: 10,
                output: 5,
                cached: 0,
            }))
            .await;
            send(runtime_event::Event::Span(span(
                "tool_call",
                "write_file",
                &agent,
                &turn_id,
                json!({"tool": "write_file", "args_sha256": "ab".repeat(32), "args_bytes": 12,
                    "result_bytes": 4, "is_error": false, "duration_ns": 1000}),
            )))
            .await;
            send(runtime_event::Event::Artifact(pb::Artifact {
                path: "deliverable.md".into(),
                mime: "text/markdown".into(),
                content: format!("# {title}\n").into_bytes(),
            }))
            .await;
            send(result).await;
        });
        Ok(Response::new(Box::pin(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        )))
    }
}

/// Starts the stand-in on a free port; returns its URL (for
/// `NEXC_RUNTIME_GRPC_URL`) and what it sees.
pub async fn fake_grpc_runtime() -> (String, Arc<Seen>) {
    let seen = Arc::new(Seen::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    let stub = Stub { seen: seen.clone() };
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(AgentRuntimeServer::new(stub))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });
    (url, seen)
}
