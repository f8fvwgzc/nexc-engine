//! Runs (one execution of a graph), per-node results and artifacts.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::graph::{Executor, NodeStatus};
use super::string_enum;

/// Characters of node output included in [`NodeRun::output_preview`].
pub const OUTPUT_PREVIEW_CHARS: usize = 400;

string_enum!(
    /// Lifecycle of a run.
    RunStatus {
        Queued => "queued",
        Running => "running",
        Succeeded => "succeeded",
        Failed => "failed",
        Cancelled => "cancelled",
    }
);

impl RunStatus {
    /// True once the run can no longer change.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunStatus::Succeeded | RunStatus::Failed | RunStatus::Cancelled
        )
    }
}

/// Result of one node within a run.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeRun {
    pub node_id: Uuid,
    pub status: NodeStatus,
    pub attempt: i32,
    pub executor: Executor,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cached: bool,
    #[schema(required = true)]
    pub error: Option<String>,
    #[schema(required = true)]
    pub started_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub finished_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub output_preview: Option<String>,
}

/// One execution of (part of) a graph.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Run {
    pub id: Uuid,
    pub graph_id: Uuid,
    pub status: RunStatus,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd: f64,
    #[schema(required = true)]
    pub started_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub node_runs: Vec<NodeRun>,
}

/// A file produced by a node during a run.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Artifact {
    pub id: Uuid,
    pub run_id: Uuid,
    pub node_id: Uuid,
    /// Relative path inside the run (also the download file name).
    pub path: String,
    pub size: i64,
    pub mime: String,
    pub created_at: DateTime<Utc>,
}

/// First [`OUTPUT_PREVIEW_CHARS`] characters of an output.
pub fn preview(output: &str) -> String {
    output.chars().take(OUTPUT_PREVIEW_CHARS).collect()
}

/// Final status of a run given its node results and whether it was cancelled.
pub fn final_status(node_statuses: &[NodeStatus], cancelled: bool) -> RunStatus {
    if cancelled {
        RunStatus::Cancelled
    } else if node_statuses.iter().all(|s| *s == NodeStatus::Succeeded) {
        RunStatus::Succeeded
    } else {
        RunStatus::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_status_rules() {
        use NodeStatus::*;
        assert_eq!(
            final_status(&[Succeeded, Succeeded], false),
            RunStatus::Succeeded
        );
        assert_eq!(
            final_status(&[Succeeded, Skipped], false),
            RunStatus::Failed
        );
        assert_eq!(final_status(&[Succeeded], true), RunStatus::Cancelled);
        assert_eq!(final_status(&[], false), RunStatus::Succeeded);
        assert!(RunStatus::Failed.is_terminal() && !RunStatus::Running.is_terminal());
    }

    #[test]
    fn preview_is_char_bounded() {
        assert_eq!(
            preview(&"é".repeat(1000)).chars().count(),
            OUTPUT_PREVIEW_CHARS
        );
    }
}
