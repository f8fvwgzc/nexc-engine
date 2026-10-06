//! The platform console: what whoever runs this installation sees and does
//! across all workspaces. It shows who registered and which workspaces
//! exist, with counts, never a workspace's content. Platform administrators
//! are the accounts whose role is `admin`; everyone else is a `user`,
//! whatever they are inside their own workspaces.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;
use super::user::Role;
use super::workspace::WorkspaceRole;

/// Longest reason kept with a suspension.
pub const REASON_MAX: usize = 300;

string_enum!(
    /// What a platform administrator did.
    PlatformAction {
        RoleChanged => "role_changed",
        AccountSuspended => "account_suspended",
        AccountReactivated => "account_reactivated",
        OwnerAssigned => "owner_assigned",
        WorkspaceDeleted => "workspace_deleted",
        AccountErased => "account_erased",
    }
);

/// A workspace as the platform sees it: who owns it and how big it is.
#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct PlatformWorkspace {
    pub id: Uuid,
    pub name: String,
    /// Its first owner, by name and e-mail; `null` when it has none.
    #[schema(required = true)]
    pub owner_name: Option<String>,
    #[schema(required = true)]
    pub owner_email: Option<String>,
    pub member_count: i64,
    pub team_count: i64,
    pub issue_count: i64,
    pub graph_count: i64,
    pub created_at: DateTime<Utc>,
}

/// A member of a workspace, as the platform sees them.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlatformMember {
    pub user_id: Uuid,
    pub name: String,
    pub email: String,
    /// Their role inside the workspace.
    pub role: WorkspaceRole,
    /// Whether the account can use the workspace at all: a suspended account
    /// and a platform administrator cannot.
    pub suspended: bool,
    pub platform_admin: bool,
    pub joined_at: DateTime<Utc>,
}

/// How much a workspace holds, in counts.
#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct WorkspaceFootprint {
    pub project_count: i64,
    pub document_count: i64,
    pub document_bytes: i64,
    pub memory_count: i64,
    pub run_count: i64,
}

/// One workspace with its members and its size.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlatformWorkspaceDetail {
    #[serde(flatten)]
    pub workspace: PlatformWorkspace,
    pub members: Vec<PlatformMember>,
    pub footprint: WorkspaceFootprint,
}

/// An account as the platform sees it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlatformUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    /// `admin` administers the platform; `user` is everyone else.
    pub role: Role,
    /// Workspaces the account belongs to, and how many of them it owns.
    pub workspace_count: i64,
    pub owned_count: i64,
    /// Whether sign-in is locked right now (too many failed attempts).
    pub locked: bool,
    /// Whether a platform administrator suspended the account, and why.
    pub suspended: bool,
    pub suspended_reason: String,
    pub created_at: DateTime<Utc>,
}

/// One entry of the platform's activity log.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlatformEvent {
    pub id: Uuid,
    pub action: PlatformAction,
    /// `null` once the administrator's account is gone; `actor_name` stays.
    #[schema(required = true)]
    pub actor_id: Option<Uuid>,
    pub actor_name: String,
    /// The account or workspace the action was about.
    pub subject: String,
    /// What changed, in words.
    pub detail: String,
    pub created_at: DateTime<Utc>,
}
