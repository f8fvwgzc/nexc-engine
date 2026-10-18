//! Tasks an exposed agent receives: admitted by the firewall (the counterpart
//! list, the delegation depth across hops, no loops), turned into a one-node
//! run of the agent in its A2A graph, executed by this instance with the
//! caller, the decision and the task on the run's trace before anything
//! else, and answered as A2A tasks whose artifacts are the run's output and
//! files.

use std::convert::Infallible;
use std::time::{Duration, Instant};

use axum::response::sse::Event;
use base64::Engine as _;
use chrono::{SecondsFormat, Utc};
use futures::Stream;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use uuid::Uuid;

use super::{Caller, internal};
use crate::app::AppState;
use crate::domain::a2a::{
    E_TASK_NOT_CANCELABLE, E_TASK_NOT_FOUND, Hops, INLINE_ARTIFACT_MAX, InboundMessage, Part,
    RpcError, TOOL_PREFIX, TaskState,
};
use crate::domain::agent::{Agent, AgentRuntime};
use crate::domain::graph::{Executor, NodeDraft, NodeOrigin, NodeStatus};
use crate::domain::policy::{Action, Decision, decide_with};
use crate::domain::run::RunStatus;
use crate::domain::trace::{SpanDraft, SpanKind, SpanStatus};
use crate::engine::{artifacts, knowledge, scheduler};
use crate::realtime::hub::SseFrame;
use crate::repo;
use crate::repo::a2a::{NewTask, TaskRow};
use crate::repo::artifacts::StoredArtifact;

/// How long `tasks/send` waits for the run before answering with the task
/// still working (the request itself times out at a minute).
pub const SEND_WAIT: Duration = Duration::from_secs(25);
/// How often a waiting `tasks/send` looks at the run.
const POLL: Duration = Duration::from_millis(200);

/// A task as received, read and checked.
#[derive(Debug, Clone)]
pub struct Inbound {
    pub task_id: Uuid,
    pub session_id: Option<String>,
    pub message: InboundMessage,
    pub hops: Hops,
}

impl Inbound {
    /// Reads `tasks/send` params: the id (a UUID v7, minted when absent), the
    /// session, the message and the hops in the metadata.
    pub fn parse(params: &Value) -> Result<Self, RpcError> {
        let object = params
            .as_object()
            .ok_or_else(|| RpcError::invalid_params("`params` must be an object"))?;
        let task_id = match object.get("id") {
            None | Some(Value::Null) => Uuid::now_v7(),
            Some(value) => value
                .as_str()
                .and_then(|s| s.parse::<Uuid>().ok())
                .filter(|id| id.get_version_num() == 7)
                .ok_or_else(|| RpcError::invalid_params("`id` must be a UUID v7"))?,
        };
        let session_id = object
            .get("sessionId")
            .or_else(|| object.get("session_id"))
            .and_then(Value::as_str)
            .map(|s| s.chars().take(200).collect::<String>())
            .filter(|s| !s.is_empty());
        if object.get("pushNotification").is_some_and(|p| !p.is_null()) {
            return Err(RpcError::new(
                crate::domain::a2a::E_PUSH_UNSUPPORTED,
                "push notifications are not supported; poll tasks/get or use tasks/sendSubscribe",
            ));
        }
        let message = InboundMessage::parse(
            object
                .get("message")
                .ok_or_else(|| RpcError::invalid_params("`message` is required"))?,
        )?;
        let hops = Hops::parse(object.get("metadata"))?;
        Ok(Inbound {
            task_id,
            session_id,
            message,
            hops,
        })
    }
}

/// A task taken on: its row, the graph its run is in, and the graph's
/// events when the caller subscribed before the run started.
pub struct Accepted {
    pub row: TaskRow,
    pub graph_id: Uuid,
    pub events: Option<broadcast::Receiver<SseFrame>>,
}

/// The message as kept: file bytes replaced by the documents they became.
fn stored_message(message: &InboundMessage, documents: &[(String, Option<Uuid>)]) -> Value {
    let mut files = documents.iter();
    let parts: Vec<Value> = message
        .parts
        .iter()
        .map(|part| match part {
            Part::Text(text) => json!({"type": "text", "text": text}),
            Part::Data(data) => json!({"type": "data", "data": data}),
            Part::File {
                name, mime, uri, ..
            } => {
                let document = files.next().and_then(|(_, id)| *id);
                json!({"type": "file", "file": {"name": name, "mimeType": mime, "uri": uri, "documentId": document}})
            }
        })
        .collect();
    json!({"role": message.role, "parts": parts})
}

/// Takes a task on for `agent` from `caller`: decides it, records it, starts
/// its run (or fails it at once when refused) and returns it.
pub async fn send(
    state: &AppState,
    caller: &Caller,
    agent: &Agent,
    workspace: Uuid,
    inbound: Inbound,
    subscribe: bool,
) -> Result<Accepted, RpcError> {
    // The same task sent twice is the same task (a client retrying), as long as it is the caller's.
    if let Some(existing) = repo::a2a::find_task(&state.db, inbound.task_id)
        .await
        .map_err(internal)?
    {
        if existing.task.caller_agent_id != Some(caller.agent.id) {
            return Err(RpcError::invalid_params("that task id is already in use"));
        }
        let run = repo::runs::find_row(&state.db, existing.task.run_id)
            .await
            .map_err(internal)?
            .ok_or_else(|| RpcError::new(E_TASK_NOT_FOUND, "the task's run is gone"))?;
        let events = subscribe.then(|| state.hub.subscribe_sse(run.graph_id));
        return Ok(Accepted {
            row: existing,
            graph_id: run.graph_id,
            events,
        });
    }
    let counterpart = &caller.counterpart.counterpart;
    let effective = state
        .caches
        .resolved(&state.db, workspace, agent)
        .await
        .map_err(internal)?;
    let versions = effective.versions.clone();
    let resolved = &effective.resolved;
    // The firewall's decision: no loop, the counterpart on the list, the hop within the depth.
    let decision = if inbound.hops.contains(&agent.principal_id) {
        Decision::deny(
            "delegation.loop",
            format!(
                "the task already passed through {}: a loop between agents",
                agent.principal_id
            ),
        )
    } else {
        decide_with(
            &resolved.capabilities,
            &resolved.rules,
            &[],
            &Action::Counterpart {
                name: &counterpart.name,
                depth: inbound.hops.hop,
            },
        )
    };
    let refused = !decision.allowed;

    let mut tx = state.db.begin().await.map_err(internal)?;
    let (graph_id, owner) = repo::a2a::ensure_graph(&mut tx, agent.id, workspace, &agent.name)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    // Files become documents of the workspace (never for a refused task).
    let mut documents: Vec<(String, Option<Uuid>)> = Vec::new();
    if !refused {
        for part in &inbound.message.parts {
            if let Part::File {
                name,
                bytes: Some(bytes),
                ..
            } = part
            {
                let stored = knowledge::store_document(state, workspace, name, bytes, owner)
                    .await
                    .map_err(|err| RpcError::invalid_params(format!("file {name}: {err}")))?;
                documents.push((name.clone(), Some(stored.0.id)));
            } else if let Part::File { name, .. } = part {
                documents.push((name.clone(), None));
            }
        }
    }
    let message = stored_message(&inbound.message, &documents);
    let mut content = inbound.message.text();
    for (part, (name, document)) in inbound
        .message
        .parts
        .iter()
        .filter(|p| matches!(p, Part::File { .. }))
        .zip(&documents)
    {
        let Part::File { mime, uri, .. } = part else {
            continue;
        };
        content.push_str(&format!(
            "\n\nAttached: {name} ({mime}){}{}",
            document.map_or(String::new(), |id| {
                format!(", in the knowledge base as document {id}")
            }),
            uri.as_deref()
                .map_or(String::new(), |uri| format!(", at {uri}")),
        ));
    }
    let node_draft = NodeDraft {
        title: inbound.message.title(),
        content,
        kind: "document".into(),
        tags: vec!["a2a".into()],
        x: 0.0,
        y: 0.0,
        agent_role: Some(agent.role.clone()),
        executor: if agent.runtime == AgentRuntime::Python {
            Executor::Agent
        } else {
            Executor::Llm
        },
        origin: NodeOrigin::User,
    };

    let mut tx = state.db.begin().await.map_err(internal)?;
    let node_id = Uuid::now_v7();
    repo::nodes::create(&mut *tx, node_id, graph_id, &node_draft)
        .await
        .map_err(internal)?;
    let run_id = repo::runs::create(
        &mut tx,
        graph_id,
        owner,
        1,
        true,
        &[(node_id, node_draft.executor)],
    )
    .await
    .map_err(internal)?;
    let row = repo::runs::claim(
        &mut *tx,
        state.engine.instance(),
        run_id,
        state.settings.run_lease.as_secs_f64(),
    )
    .await
    .map_err(internal)?
    .ok_or_else(|| internal("the inbound run could not be claimed"))?;
    repo::a2a::insert_task(
        &mut *tx,
        &NewTask {
            id: inbound.task_id,
            workspace_id: workspace,
            agent_id: agent.id,
            caller_agent_id: Some(caller.agent.id),
            caller_principal: &caller.agent.principal_id,
            run_id,
            node_id,
            session_id: inbound.session_id.as_deref(),
            hops: &inbound.hops,
            refused: refused.then_some((decision.rule, decision.reason.as_str())),
            message: &message,
        },
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    // What is on the record before the agent starts: who called, and the decision.
    let now = Utc::now();
    let canonical = serde_json::to_string(&message).unwrap_or_default();
    let document_ids: Vec<Uuid> = documents.iter().filter_map(|(_, id)| *id).collect();
    let files = inbound
        .message
        .parts
        .iter()
        .filter(|p| matches!(p, Part::File { .. }))
        .count();
    let call = SpanDraft {
        id: Uuid::now_v7(),
        node_id,
        agent_id: Some(agent.id),
        principal: agent.principal_id.clone(),
        parent_id: None,
        kind: SpanKind::ToolCall,
        name: format!("{TOOL_PREFIX}{}", counterpart.name),
        started_at: now,
        ended_at: Some(now),
        status: if refused {
            SpanStatus::Error
        } else {
            SpanStatus::Ok
        },
        attributes: json!({
            "tool": format!("{TOOL_PREFIX}{}", counterpart.name),
            "a2a": true,
            "inbound": true,
            "direction": "inbound",
            "task_id": inbound.task_id,
            "session_id": inbound.session_id,
            "counterpart": caller.agent.principal_id,
            "counterpart_name": counterpart.name,
            "hop": inbound.hops.hop,
            "chain": inbound.hops.chain,
            "args_sha256": crate::kernel::sha256_hex(canonical.as_bytes()),
            "args_bytes": canonical.len(),
            "parts": inbound.message.parts.len(),
            "files": files,
            "document_ids": document_ids,
            "is_error": refused,
            "policy_version": agent.policy_version,
            "policy_versions": versions,
        }),
    };
    let admission = SpanDraft {
        id: Uuid::now_v7(),
        node_id,
        agent_id: Some(agent.id),
        principal: agent.principal_id.clone(),
        parent_id: Some(call.id),
        kind: SpanKind::Policy,
        name: counterpart.name.clone(),
        started_at: now,
        ended_at: Some(now),
        status: if refused {
            SpanStatus::Error
        } else {
            SpanStatus::Ok
        },
        attributes: json!({
            "decision": decision.verdict(),
            "rule": decision.rule,
            "reason": decision.reason,
            "action": "counterpart",
            "subject": counterpart.name,
            "a2a": true,
            "inbound": true,
            "firewall": true,
            "task_id": inbound.task_id,
            "counterpart": caller.agent.principal_id,
            "hop": inbound.hops.hop,
            "chain": inbound.hops.chain,
            "policy_version": agent.policy_version,
            "policy_versions": versions,
        }),
    };
    state.metrics.a2a_tasks.add(
        &[
            ("direction", "inbound"),
            ("outcome", if refused { "refused" } else { "accepted" }),
        ],
        1,
    );
    let events = subscribe.then(|| state.hub.subscribe_sse(graph_id));
    let refusal = refused.then(|| format!("policy: {} ({})", decision.reason, decision.rule));
    tokio::spawn(scheduler::execute_claimed(
        state.clone(),
        row,
        vec![call, admission],
        refusal,
    ));
    let stored = repo::a2a::find_task(&state.db, inbound.task_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("the task was not recorded"))?;
    Ok(Accepted {
        row: stored,
        graph_id,
        events,
    })
}

/// The task of `id` as the caller may see it: theirs, else not found.
pub async fn task_of(state: &AppState, caller: &Caller, id: Uuid) -> Result<TaskRow, RpcError> {
    repo::a2a::find_task(&state.db, id)
        .await
        .map_err(internal)?
        .filter(|row| row.task.caller_agent_id == Some(caller.agent.id))
        .ok_or_else(|| RpcError::new(E_TASK_NOT_FOUND, "no such task"))
}

/// Waits until the task's run ended, or `limit` passed.
pub async fn wait(state: &AppState, run_id: Uuid, limit: Duration) {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        match repo::runs::find_row(&state.db, run_id).await {
            Ok(Some(row)) if !row.status.is_terminal() => tokio::time::sleep(POLL).await,
            _ => return,
        }
    }
}

/// Cancels the task's run; a task that already ended cannot be.
pub async fn cancel(state: &AppState, row: &TaskRow) -> Result<(), RpcError> {
    let run = repo::runs::find_row(&state.db, row.task.run_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| RpcError::new(E_TASK_NOT_FOUND, "the task's run is gone"))?;
    if run.status.is_terminal() {
        return Err(RpcError::new(
            E_TASK_NOT_CANCELABLE,
            format!(
                "the task is already {}",
                TaskState::of_run(run.status).as_str()
            ),
        ));
    }
    scheduler::cancel_run(state, run.owner_id, run.id)
        .await
        .map_err(internal)?;
    Ok(())
}

fn timestamp(at: chrono::DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// An artifact of the run as an A2A artifact: a file part with the bytes
/// inline when small, else a URI the caller fetches with its token.
pub async fn artifact_json(
    state: &AppState,
    task_id: Uuid,
    stored: &StoredArtifact,
    index: usize,
) -> Value {
    let artifact = &stored.artifact;
    let mut file = json!({"name": artifact.path, "mimeType": artifact.mime});
    let inline = artifact.size >= 0 && artifact.size as usize <= INLINE_ARTIFACT_MAX;
    let bytes = if inline {
        match artifacts::resolve(state, &stored.storage_path) {
            Ok(path) => tokio::fs::read(&path).await.ok(),
            Err(_) => None,
        }
    } else {
        None
    };
    match bytes {
        Some(bytes) => {
            file["bytes"] = json!(base64::engine::general_purpose::STANDARD.encode(bytes));
        }
        None => {
            file["uri"] = json!(format!(
                "{}/a2a/tasks/{task_id}/artifacts/{}",
                state.settings.public_url, artifact.id
            ));
        }
    }
    json!({
        "name": artifact.path,
        "index": index,
        "parts": [{"type": "file", "file": file}],
        "lastChunk": true,
        "metadata": {"nexc.artifact_id": artifact.id, "nexc.size": artifact.size, "nexc.created_at": timestamp(artifact.created_at)},
    })
}

/// Whether a stored artifact may leave: not kept in by the DLP classifier.
fn releasable(stored: &StoredArtifact) -> bool {
    stored.artifact.quarantined_at.is_none() || stored.artifact.released_at.is_some()
}

/// The task as A2A describes it: its state from the run, its artifacts
/// from the run's output and files, its history when asked for.
pub async fn task_json(
    state: &AppState,
    row: &TaskRow,
    history_length: Option<u64>,
) -> Result<Value, RpcError> {
    let run = repo::runs::find_row(&state.db, row.task.run_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| RpcError::new(E_TASK_NOT_FOUND, "the task's run is gone"))?;
    let node = repo::nodes::find(&state.db, run.graph_id, row.task.node_id)
        .await
        .map_err(internal)?;
    let node_runs = repo::runs::node_runs(&state.db, run.id)
        .await
        .map_err(internal)?;
    let state_of = if row.task.refused_rule.is_some() {
        TaskState::Failed
    } else {
        TaskState::of_run(run.status)
    };
    let error = row.task.refused_reason.clone().or_else(|| {
        node_runs
            .iter()
            .find(|n| n.node_id == row.task.node_id)
            .and_then(|n| n.error.clone())
    });
    let mut artifacts_json = Vec::new();
    if state_of.is_final() {
        if let Some(output) = node
            .as_ref()
            .and_then(|n| n.output.as_deref())
            .filter(|o| !o.trim().is_empty())
        {
            artifacts_json.push(json!({
                "name": "answer",
                "index": 0,
                "parts": [{"type": "text", "text": output}],
                "lastChunk": true,
            }));
        }
        let stored = repo::artifacts::list_stored(&state.db, run.id)
            .await
            .map_err(internal)?;
        for artifact in stored.iter().filter(|s| releasable(s)) {
            let index = artifacts_json.len();
            artifacts_json.push(artifact_json(state, row.task.id, artifact, index).await);
        }
    }
    let status_message = match state_of {
        TaskState::Failed => {
            error.map(|e| json!({"role": "agent", "parts": [{"type": "text", "text": e}]}))
        }
        _ => None,
    };
    let history = match history_length {
        Some(0) => Vec::new(),
        _ => vec![row.message.clone()],
    };
    Ok(json!({
        "id": row.task.id,
        "sessionId": row.task.session_id,
        "status": {
            "state": state_of.as_str(),
            "message": status_message,
            "timestamp": timestamp(run.finished_at.or(run.started_at).unwrap_or(run.created_at)),
        },
        "artifacts": artifacts_json,
        "history": history,
        "metadata": {
            "nexc.agent": repo_principal(state, row).await,
            "nexc.run_id": run.id,
            "nexc.node_id": row.task.node_id,
            "nexc.hop": row.task.hop,
            "nexc.chain": row.task.chain,
            "nexc.refused_rule": row.task.refused_rule,
        },
    }))
}

async fn repo_principal(state: &AppState, row: &TaskRow) -> Option<String> {
    repo::agents::find(&state.db, row.task.workspace_id, row.task.agent_id)
        .await
        .ok()
        .flatten()
        .map(|a| a.principal_id)
}

/// The JSON-RPC response carrying `result`.
fn rpc_result(rpc_id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": rpc_id, "result": result})
}

/// The events of `tasks/sendSubscribe`: a status update when the run takes
/// the task on, an artifact update per file as it is stored, the answer and
/// a final status update when the run ends; each a JSON-RPC response.
pub fn stream(
    state: AppState,
    accepted: Accepted,
    rpc_id: Value,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let limit = state.settings.node_timeout * 2 + Duration::from_secs(60);
    async_stream::stream! {
        let task_id = accepted.row.task.id;
        let run_id = accepted.row.task.run_id;
        let event = |value: Value| Event::default().data(value.to_string());
        let status = |state: TaskState, message: Option<Value>, last: bool| {
            json!({
                "id": task_id,
                "status": {"state": state.as_str(), "message": message, "timestamp": timestamp(Utc::now())},
                "final": last,
            })
        };
        yield Ok(event(rpc_result(&rpc_id, status(TaskState::Submitted, None, false))));
        let Some(mut events) = accepted.events else {
            // A task sent before: say where it stands and stop.
            if let Ok(task) = task_json(&state, &accepted.row, Some(0)).await {
                let state_now = TaskState::parse(task["status"]["state"].as_str().unwrap_or("working")).unwrap_or(TaskState::Working);
                for artifact in task["artifacts"].as_array().into_iter().flatten() {
                    yield Ok(event(rpc_result(&rpc_id, json!({"id": task_id, "artifact": artifact}))));
                }
                yield Ok(event(rpc_result(&rpc_id, status(state_now, Some(task["status"]["message"].clone()), state_now.is_final()))));
            }
            return;
        };
        let deadline = Instant::now() + limit;
        let mut working = false;
        let mut index = 1usize;
        loop {
            let frame = match tokio::time::timeout_at(deadline.into(), events.recv()).await {
                Ok(Ok(frame)) => frame,
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => break,
            };
            let data: Value = serde_json::from_str(&frame.data).unwrap_or(Value::Null);
            match &*frame.event {
                "node.status" if data["run_id"].as_str() == Some(&run_id.to_string()) => {
                    if !working && data["status"] == NodeStatus::Running.as_str() {
                        working = true;
                        yield Ok(event(rpc_result(&rpc_id, status(TaskState::Working, None, false))));
                    }
                }
                "artifact.created" if data["artifact"]["run_id"].as_str() == Some(&run_id.to_string()) => {
                    // Quarantine is decided right after storing: read the row as it stands now.
                    let id = data["artifact"]["id"].as_str().and_then(|s| s.parse::<Uuid>().ok());
                    let stored = match id {
                        Some(id) => repo::artifacts::list_stored(&state.db, run_id).await.ok()
                            .and_then(|all| all.into_iter().find(|s| s.artifact.id == id)),
                        None => None,
                    };
                    if let Some(stored) = stored.filter(releasable) {
                        let artifact = artifact_json(&state, task_id, &stored, index).await;
                        index += 1;
                        yield Ok(event(rpc_result(&rpc_id, json!({"id": task_id, "artifact": artifact}))));
                    }
                }
                "run.finished" if data["run"]["id"].as_str() == Some(&run_id.to_string()) => {
                    let (final_state, message) = match task_json(&state, &accepted.row, Some(0)).await {
                        Ok(task) => {
                            if let Some(answer) = task["artifacts"].as_array().and_then(|a| a.iter().find(|x| x["name"] == "answer")) {
                                yield Ok(event(rpc_result(&rpc_id, json!({"id": task_id, "artifact": answer}))));
                            }
                            (
                                TaskState::parse(task["status"]["state"].as_str().unwrap_or("failed")).unwrap_or(TaskState::Failed),
                                task["status"]["message"].clone(),
                            )
                        }
                        Err(_) => (TaskState::Failed, Value::Null),
                    };
                    yield Ok(event(rpc_result(&rpc_id, status(final_state, Some(message), true))));
                    break;
                }
                _ => {}
            }
        }
        if Instant::now() >= deadline {
            yield Ok(event(rpc_result(&rpc_id, status(TaskState::Working, Some(json!({"role": "agent", "parts": [{"type": "text", "text": "the stream ended; poll tasks/get"}]})), false))));
        }
    }
}

/// The state the console summarises a task by.
pub fn state_of(run: RunStatus, refused: bool) -> TaskState {
    if refused {
        TaskState::Failed
    } else {
        TaskState::of_run(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::a2a::E_INVALID_PARAMS;

    #[test]
    fn params_are_read_strictly() {
        let good = Inbound::parse(&json!({
            "id": Uuid::now_v7().to_string(),
            "sessionId": "s1",
            "message": {"role": "user", "parts": [{"type": "text", "text": "Hello"}]},
            "metadata": {"nexc.hop": 1, "nexc.chain": ["agent://w/A"]}
        }))
        .unwrap();
        assert_eq!(good.session_id.as_deref(), Some("s1"));
        assert_eq!(good.hops.hop, 1);
        let minted =
            Inbound::parse(&json!({"message": {"parts": [{"type": "text", "text": "x"}]}}))
                .unwrap();
        assert_eq!(minted.task_id.get_version_num(), 7);
        assert_eq!(
            Inbound::parse(&json!({"id": Uuid::new_v4().to_string(), "message": {"parts": [{"type": "text", "text": "x"}]}}))
                .unwrap_err()
                .code,
            E_INVALID_PARAMS
        );
        assert_eq!(
            Inbound::parse(&json!([])).unwrap_err().code,
            E_INVALID_PARAMS
        );
        assert_eq!(
            Inbound::parse(&json!({})).unwrap_err().code,
            E_INVALID_PARAMS
        );
        assert_eq!(
            Inbound::parse(&json!({"message": {"parts": [{"type": "text", "text": "x"}]}, "pushNotification": {"url": "x"}}))
                .unwrap_err()
                .code,
            crate::domain::a2a::E_PUSH_UNSUPPORTED
        );
        let message = InboundMessage::parse(&json!({"parts": [
            {"type": "text", "text": "hi"},
            {"type": "file", "file": {"name": "a.csv", "mimeType": "text/csv", "bytes": "YQ=="}}
        ]}))
        .unwrap();
        let kept = stored_message(&message, &[("a.csv".into(), Some(Uuid::nil()))]);
        assert!(kept["parts"][1]["file"].get("bytes").is_none());
        assert_eq!(
            kept["parts"][1]["file"]["documentId"],
            Uuid::nil().to_string()
        );
        assert_eq!(state_of(RunStatus::Succeeded, true), TaskState::Failed);
        assert_eq!(state_of(RunStatus::Succeeded, false), TaskState::Completed);
    }
}
