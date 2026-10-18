//! The AgentSecOps console (phase 5, `docs/AGENTSECOPS.md` §8): what the top
//! of the page needs in one call, and the rows of the trace explorer.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::agent::AgentStatus;
use super::evidence::EvidenceVerdict;
use super::policy_document::{PolicyScope, Severity};
use super::trace::{SpanKind, SpanStatus};

/// Agents in the workspace, by status.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct AgentCounts {
    pub total: i64,
    pub active: i64,
    pub paused: i64,
    pub over_budget: i64,
    pub quarantined: i64,
}

impl AgentCounts {
    /// Adds `count` agents of `status`.
    pub fn add(&mut self, status: AgentStatus, count: i64) {
        self.total += count;
        match status {
            AgentStatus::Active => self.active += count,
            AgentStatus::Paused => self.paused += count,
            AgentStatus::OverBudget => self.over_budget += count,
            AgentStatus::Quarantined => self.quarantined += count,
        }
    }
}

/// Runs in a window, by outcome.
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct RunCounts {
    pub total: i64,
    pub succeeded: i64,
    pub failed: i64,
    pub cancelled: i64,
    /// Queued or running.
    pub active: i64,
}

impl RunCounts {
    /// Adds `count` runs whose status is `status`.
    pub fn add(&mut self, status: &str, count: i64) {
        self.total += count;
        match status {
            "succeeded" => self.succeeded += count,
            "failed" => self.failed += count,
            "cancelled" => self.cancelled += count,
            _ => self.active += count,
        }
    }
}

/// Outputs by evidence verdict (`docs/AGENTSECOPS.md` §2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, ToSchema)]
pub struct EvidenceCounts {
    pub verified: i64,
    pub partial: i64,
    pub unverified: i64,
}

impl EvidenceCounts {
    /// Adds `count` outputs of `verdict`.
    pub fn add(&mut self, verdict: EvidenceVerdict, count: i64) {
        match verdict {
            EvidenceVerdict::Verified => self.verified += count,
            EvidenceVerdict::Partial => self.partial += count,
            EvidenceVerdict::Unverified => self.unverified += count,
        }
    }

    /// Outputs that are not verified.
    pub fn not_verified(&self) -> i64 {
        self.partial + self.unverified
    }
}

/// How often one rule decided.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RuleCount {
    pub rule: String,
    pub count: i64,
}

/// Unacknowledged alerts of one severity.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SeverityCount {
    pub severity: Severity,
    pub count: i64,
}

/// One of the agents whose latest run strayed furthest from its baseline.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RiskyAgent {
    pub agent_id: Uuid,
    pub name: String,
    pub principal_id: String,
    pub status: AgentStatus,
    pub risk_score: f64,
    pub risk_reasons: Vec<String>,
    #[schema(required = true)]
    pub risk_updated_at: Option<DateTime<Utc>>,
}

/// A policy version in force at one level.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PolicyInForce {
    pub scope: PolicyScope,
    #[schema(required = true)]
    pub scope_id: Option<Uuid>,
    pub version: i32,
    pub created_at: DateTime<Utc>,
}

/// The run whose trace chain was sealed last.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TraceSeal {
    pub run_id: Uuid,
    pub graph_id: Uuid,
    /// Hex SHA-256 of the chain's last span.
    pub trace_hash: String,
    #[schema(required = true)]
    pub sealed_at: Option<DateTime<Utc>>,
}

/// The highest risk score an agent was given on one day.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RiskPoint {
    pub agent_id: Uuid,
    /// The day, `YYYY-MM-DD` (UTC).
    pub day: String,
    pub score: f64,
}

/// Everything the top of the console shows, in one call. Runs, spans and
/// denials count only what the caller may open.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SecopsOverview {
    pub agents: AgentCounts,
    /// Runs created since midnight UTC.
    pub runs_today: RunCounts,
    /// Runs created in the last seven days.
    pub runs_week: RunCounts,
    pub spans_today: i64,
    /// Policy denials since midnight UTC, by the rule that denied.
    pub denials_today: Vec<RuleCount>,
    /// Policy denials over the last fourteen days, by rule.
    pub denials_fortnight: Vec<RuleCount>,
    pub approvals_open: i64,
    /// Unacknowledged alerts by severity (severities with none are left out).
    pub alerts_open: Vec<SeverityCount>,
    /// Artifacts the DLP screen kept in and nobody released.
    pub artifacts_quarantined: i64,
    /// Outputs that answered since midnight UTC, by evidence verdict.
    pub evidence_today: EvidenceCounts,
    /// The five agents scoring highest, highest first; only agents scored at least once.
    pub top_risk: Vec<RiskyAgent>,
    /// The latest version at every level that has one.
    pub policy_versions: Vec<PolicyInForce>,
    #[schema(required = true)]
    pub last_seal: Option<TraceSeal>,
    /// The highest score per agent per day over the last fourteen days, oldest first.
    pub risk_history: Vec<RiskPoint>,
    /// When the overview was computed.
    pub at: DateTime<Utc>,
}

/// A span as the trace explorer lists it: the identifiers and the few
/// attributes the table shows, never the whole attribute document.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SecopsSpan {
    pub id: Uuid,
    pub run_id: Uuid,
    pub graph_id: Uuid,
    pub node_id: Uuid,
    #[schema(required = true)]
    pub agent_id: Option<Uuid>,
    pub principal: String,
    pub kind: SpanKind,
    pub name: String,
    pub started_at: DateTime<Utc>,
    #[schema(required = true)]
    pub ended_at: Option<DateTime<Utc>>,
    pub status: SpanStatus,
    /// `attributes.tool` of a tool call or a policy decision about one.
    #[schema(required = true)]
    pub tool: Option<String>,
    /// `attributes.egress_host`: the host a tool called out to.
    #[schema(required = true)]
    pub host: Option<String>,
    /// `attributes.decision` of a policy or approval span.
    #[schema(required = true)]
    pub decision: Option<String>,
    /// `attributes.rule` of a policy span.
    #[schema(required = true)]
    pub rule: Option<String>,
    /// `attributes.model` of a model call.
    #[schema(required = true)]
    pub model: Option<String>,
    #[schema(required = true)]
    pub tokens_in: Option<i64>,
    #[schema(required = true)]
    pub tokens_out: Option<i64>,
    /// `attributes.inbound`: a tool call that is a task received over A2A.
    pub inbound: bool,
    /// `attributes.evidence`: the verdict on an output, on a node's last model
    /// call and on the policy decision about it.
    #[schema(required = true)]
    pub evidence: Option<String>,
}

/// A page of the trace explorer, newest first.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SecopsSpanPage {
    pub spans: Vec<SecopsSpan>,
    /// Pass as `before` for the next page; `null` on the last.
    #[schema(required = true)]
    pub next: Option<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_add_up_by_status() {
        let mut agents = AgentCounts::default();
        agents.add(AgentStatus::Active, 3);
        agents.add(AgentStatus::Quarantined, 1);
        assert_eq!((agents.total, agents.active, agents.quarantined), (4, 3, 1));
        let mut runs = RunCounts::default();
        runs.add("succeeded", 2);
        runs.add("failed", 1);
        runs.add("running", 1);
        runs.add("queued", 1);
        assert_eq!(
            (runs.total, runs.succeeded, runs.failed, runs.active),
            (5, 2, 1, 2)
        );
        let mut evidence = EvidenceCounts::default();
        evidence.add(EvidenceVerdict::Verified, 3);
        evidence.add(EvidenceVerdict::Partial, 1);
        evidence.add(EvidenceVerdict::Unverified, 2);
        assert_eq!((evidence.verified, evidence.not_verified()), (3, 3));
    }
}
