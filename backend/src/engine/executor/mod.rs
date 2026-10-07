//! Node executors. The scheduler picks one per node from `node.executor`:
//! [`llm`] (built-in streaming LLM call), [`agent`] (Python agent runtime)
//! or [`symphony`] (texc-symphony coding agent).

pub mod agent;
pub mod llm;
pub mod symphony;

use futures::future::BoxFuture;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::agent::{Agent, FALLBACK_SYSTEM_PROMPT};
use crate::domain::graph::{Executor, GraphNode};
use crate::domain::ontology::NodeType;
use crate::domain::prompt::UpstreamOutput;
use crate::domain::run::Artifact;
use crate::domain::settings::LlmProviderKind;
use crate::llm::{LlmError, LlmTarget};
use crate::realtime::events::{LogLevel, SseEvent};

/// Everything an executor needs to run one node attempt.
pub struct ExecContext {
    pub state: AppState,
    pub run_id: Uuid,
    pub graph_id: Uuid,
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
    pub target: LlmTarget,
    /// When true, cached LLM responses must not be reused.
    pub force: bool,
}

impl ExecContext {
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

    /// Stores an artifact produced by this node.
    pub async fn artifact(
        &self,
        path: &str,
        mime: Option<&str>,
        bytes: &[u8],
    ) -> Result<Artifact, String> {
        super::artifacts::store(
            &self.state,
            self.graph_id,
            self.run_id,
            self.node.id,
            path,
            mime,
            bytes,
        )
        .await
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
