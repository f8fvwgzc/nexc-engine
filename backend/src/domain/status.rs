//! Orchestrator status report.

use serde::Serialize;
use utoipa::ToSchema;

/// Health of one backend the engine depends on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, ToSchema)]
pub struct BackendHealth {
    pub enabled: bool,
    pub ok: bool,
    #[schema(required = true)]
    pub url: Option<String>,
    #[schema(required = true)]
    pub detail: Option<String>,
}

/// Health of all backends.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Backends {
    pub llm: BackendHealth,
    pub agent_runtime: BackendHealth,
    pub symphony: BackendHealth,
}

/// Live view of the orchestrator for the current user.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OrchestratorStatus {
    /// True when the effective LLM provider is the offline demo provider.
    pub demo_mode: bool,
    pub queue_depth: i64,
    pub running_nodes: i64,
    pub active_runs: i64,
    pub agents_active: i64,
    pub backends: Backends,
}
