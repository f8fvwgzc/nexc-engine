//! The audit log of a workspace: who changed its members, teams, credentials
//! and rules.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;

string_enum!(
    /// What an audit entry records.
    AuditAction {
        WorkspaceRenamed => "workspace_renamed",
        MemberAdded => "member_added",
        MemberInvited => "member_invited",
        MemberRoleChanged => "member_role_changed",
        MemberRemoved => "member_removed",
        InviteWithdrawn => "invite_withdrawn",
        TeamCreated => "team_created",
        TeamUpdated => "team_updated",
        TeamDeleted => "team_deleted",
        TeamMemberSet => "team_member_set",
        TeamMemberRemoved => "team_member_removed",
        CredentialSet => "credential_set",
        CredentialRemoved => "credential_removed",
        GuardrailsChanged => "guardrails_changed",
        LabelDeleted => "label_deleted",
        KnowledgeChanged => "knowledge_changed",
        WorkspaceTransferred => "workspace_transferred",
        /// A platform administrator made someone an owner, from outside the workspace.
        PlatformOwnerAssigned => "platform_owner_assigned",
        /// A run ended and the final hash of its trace chain was recorded (subject: the run id).
        TraceSealed => "trace_sealed",
        /// An agent's capabilities changed (subject: the agent's principal; detail: the policy version).
        PolicyChanged => "policy_changed",
        /// The runtime's report of an action disagreed with the policy the backend minted
        /// for it (subject: the principal; detail: the span and the rule).
        PolicyMismatch => "policy_mismatch",
        AgentQuarantined => "agent_quarantined",
        AgentReleased => "agent_released",
        /// The workspace's recording setting changed (subject `Recording`; detail: the setting in words).
        RecordingChanged => "recording_changed",
        /// Someone read what a span said (subject: the span id; detail: the run and the kinds).
        PayloadViewed => "payload_viewed",
        /// An operator stopped one sub-agent of a run while the run went on
        /// (subject: the sub-agent's principal; detail: the run, the node, the spawn and the reason).
        AgentCancelled => "agent_cancelled",
        /// A human let a held tool call go ahead (subject: the principal; detail: the tool and run).
        ApprovalGranted => "approval_granted",
        ApprovalDenied => "approval_denied",
        /// The DLP classifier kept an artifact in (subject: the principal; detail: the file and why).
        ArtifactQuarantined => "artifact_quarantined",
        ArtifactReleased => "artifact_released",
        /// An alert was raised (subject: the principal; detail: the title).
        AlertRaised => "alert_raised",
        /// An alert destination was added, changed, tested, removed or switched off
        /// (subject: `alerts:<name>`; detail: what changed).
        AlertDestinationChanged => "alert_destination_changed",
        /// A document, topic or memory was given a classification (subject: what; detail: the label).
        ClassificationChanged => "classification_changed",
        /// An MCP server was registered for the workspace's agents (subject: `mcp:<name>`).
        McpServerAdded => "mcp_server_added",
        McpServerRemoved => "mcp_server_removed",
        /// An API token was issued to an external agent (subject: the principal; detail: the prefix).
        AgentTokenIssued => "agent_token_issued",
        AgentTokenRevoked => "agent_token_revoked",
        /// An agent was published as an A2A agent, or withdrawn (subject: the principal).
        AgentA2aChanged => "agent_a2a_changed",
        /// An A2A counterpart was registered or removed (subject: `a2a:<name>`).
        A2aCounterpartAdded => "a2a_counterpart_added",
        A2aCounterpartRemoved => "a2a_counterpart_removed",
        /// A graph was deleted: archived with its runs' evidence until the retention purge
        /// (subject: the graph's name; detail: its id and the runs kept).
        GraphDeleted => "graph_deleted",
        /// The retention purge removed evidence: recorded payloads, or a deleted graph with
        /// its runs, spans and payloads (subject: what; detail: how much and why).
        EvidencePurged => "evidence_purged",
        /// Someone exported a run's evidence bundle (subject: the run id; detail: the key).
        EvidenceExported => "evidence_exported",
    }
);

/// One entry of a workspace's audit log.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AuditEntry {
    pub id: Uuid,
    pub action: AuditAction,
    /// `null` once the account is gone; `actor_name` stays.
    #[schema(required = true)]
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
    /// What the action was about: a person, a team, a label.
    pub subject: String,
    /// What changed, in words.
    pub detail: String,
    pub created_at: DateTime<Utc>,
}
