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
