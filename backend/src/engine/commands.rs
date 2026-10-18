//! Commands an operator sends into a run while it executes: so far, the
//! cancel of one sub-agent. The handler records the command
//! (`run_commands`) and, when the run executes here, delivers it at once;
//! the run's cancel watcher delivers what was recorded elsewhere within a
//! second. Delivering means sending the command down the node attempt's
//! gRPC stream and putting a `policy` span on the trace.
//!
//! A command is addressed to whoever holds the run's lease: only that
//! instance takes it. When the run changes instances, the new holder takes
//! what the former one never took. A cancel whose sub-agent belonged to an
//! attempt the takeover interrupted is carried to the attempt that replaces
//! it, and delivered when that attempt spawns the sub-agent of the same
//! name ([`spawned`]).

use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::trace::TraceHandle;
use crate::app::AppState;
use crate::domain::session::{CANCEL_ACTION, CANCEL_DECISION};
use crate::domain::trace::{SpanDraft, SpanKind, SpanStatus};
use crate::realtime::events::{LogLevel, SseEvent};
use crate::repo;
use crate::repo::run_commands::RunCommand;
use crate::rpc::runtime::{BackendCommand, backend_command};

/// What an operator's cancel stands for on the trace.
pub const CANCEL_RULE: &str = "operator";

/// A node attempt open on this instance over the gRPC stream: where its
/// commands go and how its spans are recorded.
#[derive(Clone)]
pub struct Attempt {
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub graph_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub policy_version: i32,
    pub commands: mpsc::UnboundedSender<BackendCommand>,
    pub trace: TraceHandle,
}

impl Attempt {
    /// Sends the cancel down the stream and records it; whether the stream
    /// took it. `carried_from` is the spawn the command named when it was
    /// carried over from an interrupted attempt to the sub-agent of the same
    /// name in this one (whose spawn is `command.span_id`).
    fn cancel_agent(
        &self,
        state: &AppState,
        command: &RunCommand,
        carried_from: Option<Uuid>,
    ) -> bool {
        let sent = self
            .commands
            .send(BackendCommand {
                command: Some(backend_command::Command::CancelAgent(
                    crate::rpc::runtime::CancelAgent {
                        spawn_span_id: command.span_id.to_string(),
                        reason: command.reason.clone(),
                    },
                )),
            })
            .is_ok();
        let now = Utc::now();
        let reason = if command.reason.is_empty() {
            "cancelled by an operator".to_owned()
        } else {
            format!("cancelled by an operator: {}", command.reason)
        };
        let mut attributes = json!({
            "decision": CANCEL_DECISION,
            "rule": CANCEL_RULE,
            "reason": reason,
            "action": CANCEL_ACTION,
            "subject": command.subject,
            "spawn_span_id": command.span_id,
            "requested_by": command.requested_by,
            "delivered": sent,
            "policy_version": self.policy_version,
        });
        if let Some(from) = carried_from {
            attributes["carried_from_spawn_span_id"] = json!(from);
        }
        self.trace.emit(SpanDraft {
            id: Uuid::now_v7(),
            node_id: self.node_id,
            agent_id: self.agent_id,
            principal: command.subject.clone(),
            parent_id: Some(command.span_id),
            kind: SpanKind::Policy,
            name: command.subject.clone(),
            started_at: command.created_at,
            ended_at: Some(now),
            status: SpanStatus::Error,
            attributes,
        });
        state.hub.publish(
            self.graph_id,
            SseEvent::NodeLog {
                run_id: self.run_id,
                node_id: self.node_id,
                level: if sent {
                    LogLevel::Warn
                } else {
                    LogLevel::Error
                },
                message: if sent {
                    format!("{} is being stopped by an operator", command.subject)
                } else {
                    format!(
                        "{} could not be stopped: the runtime's stream is closed",
                        command.subject
                    )
                },
            },
        );
        sent
    }
}

/// Takes the run's pending commands, when the run executes here, and
/// delivers each to the attempt that holds its node. A command recorded
/// while another instance held the run is about a sub-agent of an attempt
/// that is gone: it is carried to the node's next attempt here. A command
/// whose attempt is not open on this instance is logged and dropped: the
/// node ended, or it never ran over the stream. Returns how many were
/// delivered.
pub async fn deliver(state: &AppState, run_id: Uuid) -> usize {
    // Only the holder of the run's lease takes its commands; elsewhere they wait for it.
    let Some(lease) = state.engine.lease(run_id) else {
        return 0;
    };
    let Some(epoch) = lease.epoch() else {
        return 0;
    };
    let pending =
        match repo::run_commands::take_pending(&state.db, run_id, lease.instance(), epoch).await {
            Ok(pending) => pending,
            Err(err) => {
                tracing::error!(%run_id, error = %err, "run commands not read");
                return 0;
            }
        };
    let mut delivered = 0;
    for command in pending {
        if command.command != repo::run_commands::CANCEL_AGENT {
            tracing::warn!(%run_id, command = %command.command, "unknown run command dropped");
            continue;
        }
        if command.lease_epoch < epoch {
            tracing::info!(%run_id, node_id = %command.node_id, subject = %command.subject,
                "run command addressed to the run's former holder; carried to the node's next attempt");
            state.engine.carry_command(command);
            continue;
        }
        match state.engine.attempt(run_id, command.node_id) {
            Some(attempt) => {
                if attempt.cancel_agent(state, &command, None) {
                    delivered += 1;
                }
            }
            None => tracing::info!(%run_id, node_id = %command.node_id,
                "run command found no open attempt here: the node ended or runs without the stream"),
        }
    }
    delivered
}

/// A sub-agent named `subject` was spawned (span `spawn_span_id`) in an
/// attempt of `node_id` here: a cancel carried over from the attempt a
/// takeover interrupted, for the sub-agent of that name, is delivered to it
/// now. Returns whether one was.
pub fn spawned(
    state: &AppState,
    run_id: Uuid,
    node_id: Uuid,
    spawn_span_id: Uuid,
    subject: &str,
) -> bool {
    let Some(mut command) = state.engine.take_carried(run_id, node_id, subject) else {
        return false;
    };
    let Some(attempt) = state.engine.attempt(run_id, node_id) else {
        // No stream to send it down (the NDJSON wire): it stays carried, and goes with the run.
        state.engine.carry_command(command);
        return false;
    };
    let from = command.span_id;
    command.span_id = spawn_span_id;
    attempt.cancel_agent(state, &command, Some(from))
}
