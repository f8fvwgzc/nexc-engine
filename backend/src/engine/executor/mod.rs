//! Node executors. The scheduler picks one per node from `node.executor`:
//! [`llm`] (built-in streaming LLM call), [`agent`] (Python agent runtime)
//! or [`symphony`] (texc-symphony coding agent).

pub mod agent;
pub mod llm;
pub mod runtime_grpc;
pub mod symphony;

use std::sync::atomic::{AtomicI32, Ordering};

use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::agent::{Agent, FALLBACK_SYSTEM_PROMPT};
use crate::domain::audit::AuditAction;
use crate::domain::capabilities::{Capabilities, DataLabel};
use crate::domain::graph::{Executor, GraphNode, NodeStatus};
use crate::domain::guardrails::Recording;
use crate::domain::ontology::NodeType;
use crate::domain::policy::{self, Action, Decision, Rules};
use crate::domain::prompt::UpstreamOutput;
use crate::domain::recording::Redaction;
use crate::domain::run::Artifact;
use crate::domain::settings::LlmProviderKind;
use crate::domain::trace::{SYSTEM_PRINCIPAL, SpanDraft, SpanKind, SpanStatus};
use crate::engine::trace::TraceHandle;
use crate::llm::{LlmError, LlmTarget};
use crate::realtime::events::{LogLevel, NodeStatusEvent, SseEvent};
use crate::repo;
use crate::repo::audit::Subject;

/// Everything an executor needs to run one node attempt.
pub struct ExecContext {
    pub state: AppState,
    pub run_id: Uuid,
    pub graph_id: Uuid,
    /// The workspace the graph belongs to (policy mismatches are audited there).
    pub workspace_id: Option<Uuid>,
    /// Who started the run.
    pub owner: Uuid,
    pub goal: String,
    pub node: GraphNode,
    /// The node's type in the graph's ontology (absent if the type was removed).
    pub node_type: Option<NodeType>,
    /// Whether the workspace's guardrails let agents execute code at all.
    pub code_exec_allowed: bool,
    pub upstream: Vec<UpstreamOutput>,
    pub memories: Vec<String>,
    /// Passages of the workspace's documents, each headed by its citation.
    pub documents: Vec<String>,
    pub agent: Option<Agent>,
    /// What the node's agent may do: its capabilities, or the unrestricted
    /// policy for a node without one. The per-run token carries a copy.
    pub capabilities: Capabilities,
    /// The agent's policy version the token records.
    pub policy_version: i32,
    /// The rules the policies in force resolve to, beside the capabilities.
    pub rules: Rules,
    /// The classifications of the data the node was given (its context and
    /// its upstream nodes'); what it produces is derived from them.
    pub labels_given: Vec<DataLabel>,
    pub target: LlmTarget,
    /// When true, cached LLM responses must not be reused.
    pub force: bool,
    /// The run's trace: every span of this node goes through it.
    pub trace: TraceHandle,
    /// The attempt under way (set by the scheduler before each one).
    pub attempt: AtomicI32,
    /// What the workspace records of what the node says and hears.
    pub recording: Recording,
    /// Whether credentials are scrubbed from what is recorded (the guardrails' `redact_secrets`).
    pub scrub_secrets: bool,
    /// The passages and memories replaced by their citation in everything recorded.
    pub redactions: Vec<Redaction>,
}

impl ExecContext {
    /// A copy for a background task of this node (the approval watcher): the
    /// same run, node, policy and trace.
    pub fn clone_light(&self) -> ExecContext {
        ExecContext {
            state: self.state.clone(),
            run_id: self.run_id,
            graph_id: self.graph_id,
            workspace_id: self.workspace_id,
            owner: self.owner,
            goal: String::new(),
            node: self.node.clone(),
            node_type: None,
            code_exec_allowed: self.code_exec_allowed,
            upstream: Vec::new(),
            memories: Vec::new(),
            documents: Vec::new(),
            agent: self.agent.clone(),
            capabilities: self.capabilities.clone(),
            policy_version: self.policy_version,
            rules: self.rules.clone(),
            labels_given: self.labels_given.clone(),
            target: self.target.clone(),
            force: self.force,
            trace: self.trace.clone(),
            attempt: AtomicI32::new(self.attempt.load(Ordering::Relaxed)),
            recording: self.recording.clone(),
            scrub_secrets: self.scrub_secrets,
            redactions: self.redactions.clone(),
        }
    }

    /// Publishes the node's status for the attempt under way (`node.status`).
    pub fn publish_status(&self, status: NodeStatus) {
        self.state.hub.publish(
            self.graph_id,
            SseEvent::NodeStatus(NodeStatusEvent {
                run_id: self.run_id,
                node_id: self.node.id,
                status,
                attempt: self.attempt.load(Ordering::Relaxed),
                error: None,
                cached: false,
            }),
        );
    }

    /// Who this node's actions are attributed to: its agent's principal
    /// (`agent://{workspace}/{name}`), or the system.
    pub fn principal(&self) -> String {
        self.agent
            .as_ref()
            .map_or_else(|| SYSTEM_PRINCIPAL.to_owned(), |a| a.principal_id.clone())
    }

    /// Decides `action` under the node's capabilities and rules (in-process, microseconds).
    pub fn decide(&self, action: &Action<'_>) -> Decision {
        policy::decide_with(&self.capabilities, &self.rules, &self.labels_given, action)
    }

    /// The strictest classification among the data the node was given.
    pub fn strictest_label(&self) -> DataLabel {
        self.labels_given
            .iter()
            .copied()
            .fold(DataLabel::Public, DataLabel::stricter)
    }

    /// Records a policy decision about `subject` as a `policy` span, with
    /// status `error` for a denial; `extra` adds attributes. Returns its id.
    pub fn policy_span(&self, subject: &str, decision: &Decision, extra: Value) -> Uuid {
        let now = Utc::now();
        let mut span = self.draft(SpanKind::Policy, subject, now);
        span.ended_at = Some(now);
        span.status = if decision.allowed {
            SpanStatus::Ok
        } else {
            SpanStatus::Error
        };
        let mut attributes = json!({
            "decision": decision.verdict(),
            "rule": decision.rule,
            "reason": decision.reason,
            "policy_version": self.policy_version,
            "policy_versions": self.rules.versions,
        });
        if let (Value::Object(into), Value::Object(from)) = (&mut attributes, extra) {
            into.extend(from);
        }
        span.attributes = attributes;
        let id = span.id;
        self.record(span);
        id
    }

    /// The runtime reported something the policy minted for this node does
    /// not allow: on the record as a `policy` span, counted on the run, and
    /// in the workspace's audit log as `policy_mismatch`.
    pub async fn policy_mismatch(&self, span_id: Uuid, rule: &'static str, detail: String) {
        let principal = self.principal();
        self.log(
            LogLevel::Warn,
            format!("policy mismatch ({rule}): {detail}"),
        );
        self.policy_span(
            "policy_mismatch",
            &Decision::deny(rule, detail.clone()),
            json!({
                "decision": "mismatch",
                "action": "recheck",
                "span_id": span_id,
                "principal": principal,
            }),
        );
        self.state.metrics.policy_mismatches.add(1);
        if let Err(err) = repo::runs::add_policy_mismatch(&self.state.db, self.run_id).await {
            tracing::error!(run_id = %self.run_id, error = %err, "policy mismatch not counted on the run");
        }
        if let Some(workspace) = self.workspace_id
            && let Err(err) = repo::audit::record(
                &self.state.db,
                workspace,
                self.owner,
                AuditAction::PolicyMismatch,
                Subject::Text(&principal),
                &format!("run {} span {span_id}: {detail} ({rule})", self.run_id),
            )
            .await
        {
            tracing::error!(run_id = %self.run_id, error = %err, "policy mismatch not audited");
        }
    }

    /// A span of this node that started at `started_at`, with a fresh id, no
    /// parent, status `ok` and no attributes; fill it in and [`ExecContext::record`] it.
    pub fn draft(
        &self,
        kind: SpanKind,
        name: impl Into<String>,
        started_at: DateTime<Utc>,
    ) -> SpanDraft {
        SpanDraft {
            id: Uuid::now_v7(),
            node_id: self.node.id,
            agent_id: self.agent.as_ref().map(|a| a.id),
            principal: self.principal(),
            parent_id: None,
            kind,
            name: name.into(),
            started_at,
            ended_at: None,
            status: SpanStatus::Ok,
            attributes: json!({}),
        }
    }

    /// Appends a span to the run's trace (`trace.span`).
    pub fn record(&self, span: SpanDraft) {
        self.trace.emit(span);
    }

    /// Streams output text to the UI (`node.output`).
    pub fn output(&self, delta: &str) {
        let (run_id, node_id) = (self.run_id, self.node.id);
        self.state.hub.publish(
            self.graph_id,
            SseEvent::NodeOutput {
                run_id,
                node_id,
                delta: delta.to_owned(),
            },
        );
    }

    /// Emits a `node.log` line.
    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        let (run_id, node_id) = (self.run_id, self.node.id);
        self.state.hub.publish(
            self.graph_id,
            SseEvent::NodeLog {
                run_id,
                node_id,
                level,
                message: message.into(),
            },
        );
    }

    /// Reports token usage so far (`node.tokens`).
    pub fn tokens(&self, tokens_in: i64, tokens_out: i64) {
        let (run_id, node_id) = (self.run_id, self.node.id);
        self.state.hub.publish(
            self.graph_id,
            SseEvent::NodeTokens {
                run_id,
                node_id,
                tokens_in,
                tokens_out,
            },
        );
    }

    /// Stores an artifact produced by this node and screens it on the way
    /// out: a DLP denial keeps it in until an admin releases it.
    pub async fn artifact(
        &self,
        path: &str,
        mime: Option<&str>,
        bytes: &[u8],
    ) -> Result<Artifact, String> {
        let artifact = super::artifacts::store(
            &self.state,
            self.graph_id,
            self.run_id,
            self.node.id,
            path,
            mime,
            bytes,
        )
        .await?;
        self.screen_artifact(&artifact, bytes).await;
        Ok(artifact)
    }

    /// System prompt: the assigned agent's, or a generic one.
    pub fn system_prompt(&self) -> String {
        self.agent
            .as_ref()
            .map(|a| a.system_prompt.trim())
            .filter(|p| !p.is_empty())
            .unwrap_or(FALLBACK_SYSTEM_PROMPT)
            .to_owned()
    }

    /// The target with the agent's model when it fits the provider.
    pub fn agent_target(&self) -> LlmTarget {
        let mut target = self.target.clone();
        if let Some(agent) = &self.agent
            && target.provider == LlmProviderKind::Anthropic
            && agent.model.starts_with("claude-")
        {
            target.model.clone_from(&agent.model);
        }
        target
    }
}

/// A successful node attempt.
#[derive(Debug, Clone, Default)]
pub struct ExecOutput {
    pub output: String,
    pub tokens_in: i64,
    pub tokens_out: i64,
    /// Of `tokens_in`, the ones read from the provider's prompt cache.
    pub tokens_cached: i64,
    /// The `llm_call` span of the node, when it made exactly one call (the
    /// built-in executor); usage is booked to it.
    pub span_id: Option<Uuid>,
}

/// A failed node attempt.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ExecError {
    pub message: String,
    /// Whether the scheduler may retry the node.
    pub retryable: bool,
}

impl ExecError {
    /// A failure that retrying cannot fix.
    pub fn fatal(message: impl Into<String>) -> Self {
        ExecError {
            message: message.into(),
            retryable: false,
        }
    }

    /// A transient failure.
    pub fn transient(message: impl Into<String>) -> Self {
        ExecError {
            message: message.into(),
            retryable: true,
        }
    }
}

impl From<LlmError> for ExecError {
    fn from(err: LlmError) -> Self {
        ExecError {
            retryable: err.is_retryable(),
            message: err.to_string(),
        }
    }
}

/// Runs one attempt of a node.
pub trait NodeExecutor: Send + Sync {
    /// Executes the node described by `ctx`.
    fn execute<'a>(&'a self, ctx: &'a ExecContext) -> BoxFuture<'a, Result<ExecOutput, ExecError>>;
}

/// The executor for `kind`.
pub fn for_kind(kind: Executor) -> &'static dyn NodeExecutor {
    match kind {
        Executor::Llm => &llm::LlmExecutor,
        Executor::Agent => &agent::AgentExecutor,
        Executor::Symphony => &symphony::SymphonyExecutor,
    }
}

/// A file-name friendly slug of `title` (`output` when empty).
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 60 {
            break;
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "output".into()
    } else {
        out.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Final Report (DOCX)!"), "final-report-docx");
        assert_eq!(slug("ÄÖÜ"), "output");
        assert!(slug(&"word ".repeat(40)).len() <= 60);
    }
}
