//! Approvals: a tool call a policy holds until a human says yes. The node
//! waits (`waiting_approval`), the approvers hear in their inbox, and the
//! decision is recorded here, in the audit log and as an `approval` span.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::policy::PolicyVersions;
use super::string_enum;

/// Longest reason an approver may give.
pub const REASON_MAX: usize = 2_000;

string_enum!(
    /// Where an approval stands.
    ApprovalStatus {
        Pending => "pending",
        Approved => "approved",
        Denied => "denied",
        /// Nobody decided before the call stopped waiting.
        Expired => "expired",
    }
);

/// Who decided.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Decider {
    /// `null` once the account is gone; the name stays.
    #[schema(required = true)]
    pub id: Option<Uuid>,
    pub name: String,
}

/// One approval, asked for by a tool call under a policy.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Approval {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub graph_id: Uuid,
    #[schema(required = true)]
    pub agent_id: Option<Uuid>,
    /// The acting principal.
    pub principal: String,
    /// The policy span that asked.
    pub span_id: Uuid,
    /// The tool that waits, and a digest of its arguments.
    pub tool: String,
    #[schema(required = true)]
    pub args_sha256: Option<String>,
    /// The rule that asked (`approvals.tools`, `approvals.labels`).
    pub rule: String,
    pub reason: String,
    /// Members who may decide; empty: every admin of the workspace.
    pub approver_ids: Vec<Uuid>,
    pub status: ApprovalStatus,
    pub requested_at: DateTime<Utc>,
    /// When the call stops waiting.
    pub expires_at: DateTime<Utc>,
    #[schema(required = true)]
    pub decided_by: Option<Decider>,
    #[schema(required = true)]
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_reason: String,
    /// The `approval` span that recorded the decision, once there is one.
    #[schema(required = true)]
    pub decision_span_id: Option<Uuid>,
    pub policy_versions: PolicyVersions,
}

impl Approval {
    /// Whether `user` (with `admin` saying if they administer the workspace) may decide.
    pub fn may_decide(&self, user: Uuid, admin: bool) -> bool {
        if self.approver_ids.is_empty() {
            admin
        } else {
            admin || self.approver_ids.contains(&user)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_approvers_and_admins_decide() {
        let (admin, named, other) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let mut approval = Approval {
            id: Uuid::now_v7(),
            workspace_id: Uuid::now_v7(),
            run_id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            graph_id: Uuid::now_v7(),
            agent_id: None,
            principal: "agent://ws/Writer".into(),
            span_id: Uuid::now_v7(),
            tool: "make_docx".into(),
            args_sha256: None,
            rule: "approvals.tools".into(),
            reason: String::new(),
            approver_ids: Vec::new(),
            status: ApprovalStatus::Pending,
            requested_at: Utc::now(),
            expires_at: Utc::now(),
            decided_by: None,
            decided_at: None,
            decision_reason: String::new(),
            decision_span_id: None,
            policy_versions: PolicyVersions::default(),
        };
        assert!(approval.may_decide(admin, true));
        assert!(!approval.may_decide(other, false));
        approval.approver_ids = vec![named];
        assert!(approval.may_decide(named, false));
        assert!(approval.may_decide(admin, true));
        assert!(!approval.may_decide(other, false));
        assert_eq!(ApprovalStatus::Expired.as_str(), "expired");
    }
}
