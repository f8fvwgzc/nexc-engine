//! Approvals: what waits for a human, and the decision.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::workspaces::{audit, member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::approval::{Approval, ApprovalStatus, REASON_MAX};
use crate::domain::audit::AuditAction;
use crate::domain::validation::{FieldErrors, Validate, check_max_len};
use crate::domain::workspace::WorkspaceAction;
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo;
use crate::repo::audit::Subject;

/// Most approvals listed at once.
pub const LIST_MAX: i64 = 200;

/// `POST /approvals/{id}/approve` and `/deny` body.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Decide {
    /// Why; required to deny.
    #[serde(default)]
    pub reason: String,
}

impl Validate for Decide {
    fn validate(&self, errors: &mut FieldErrors) {
        check_max_len(errors, "reason", &self.reason, REASON_MAX);
    }
}

/// Query of `GET /workspaces/{wid}/approvals`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct ApprovalQuery {
    /// Only approvals in this state.
    pub status: Option<ApprovalStatus>,
    /// Approvals to return, 1–200 (default 50).
    pub limit: Option<i64>,
}

/// The approval, if the caller belongs to its workspace (others: 404).
async fn approval_of(
    state: &AppState,
    auth: AuthUser,
    id: Uuid,
) -> Result<(Approval, bool), AppError> {
    let approval = repo::approvals::find(&state.db, id)
        .await?
        .ok_or(AppError::NotFound("approval"))?;
    let role = repo::workspaces::role_of(&state.db, approval.workspace_id, auth.id)
        .await?
        .ok_or(AppError::NotFound("approval"))?;
    Ok((approval, role.is_admin()))
}

/// One approval (members of its workspace).
#[utoipa::path(get, path = "/approvals/{id}", tag = "approvals", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Approval id")),
    responses((status = 200, body = Approval), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Approval>, AppError> {
    let (approval, _) = approval_of(&state, auth, id).await?;
    Ok(Json(approval))
}

/// The approvals of a workspace, newest first (admins).
#[utoipa::path(get, path = "/workspaces/{wid}/approvals", tag = "approvals", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ApprovalQuery),
    responses((status = 200, body = [Approval]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<ApprovalQuery>,
) -> Result<Json<Vec<Approval>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let limit = query.limit.unwrap_or(50).clamp(1, LIST_MAX);
    Ok(Json(
        repo::approvals::list(&state.db, wid, query.status, limit).await?,
    ))
}

async fn decide(
    state: &AppState,
    auth: AuthUser,
    id: Uuid,
    status: ApprovalStatus,
    reason: &str,
) -> Result<Json<Approval>, AppError> {
    let (approval, admin) = approval_of(state, auth, id).await?;
    if !approval.may_decide(auth.id, admin) {
        return Err(AppError::Forbidden(
            "only an approver named by the policy or a workspace admin decides".into(),
        ));
    }
    if status == ApprovalStatus::Denied && reason.trim().is_empty() {
        return Err(AppError::field("reason", "say why the call is refused"));
    }
    let decided = repo::approvals::decide(&state.db, id, status, auth.id, reason.trim())
        .await?
        .ok_or_else(|| {
            AppError::Conflict(format!("the approval is already {}", approval.status))
        })?;
    // The approvers' inbox entries are settled.
    repo::issues::mark_read_about_approval(&state.db, id).await?;
    let action = match status {
        ApprovalStatus::Approved => AuditAction::ApprovalGranted,
        _ => AuditAction::ApprovalDenied,
    };
    let detail = format!(
        "{} in run {}{}",
        decided.tool,
        decided.run_id,
        if reason.trim().is_empty() {
            String::new()
        } else {
            format!(": {}", reason.trim())
        }
    );
    audit(
        state,
        decided.workspace_id,
        auth.id,
        action,
        Subject::Text(&decided.principal),
        &detail,
    )
    .await;
    Ok(Json(decided))
}

/// Lets the call go ahead (an approver named by the policy, or a workspace admin).
#[utoipa::path(post, path = "/approvals/{id}/approve", tag = "approvals", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Approval id")), request_body = Decide,
    responses((status = 200, body = Approval), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Already decided", body = Problem), (status = 422, body = Problem)))]
pub async fn approve(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<Decide>,
) -> Result<Json<Approval>, AppError> {
    decide(&state, auth, id, ApprovalStatus::Approved, &req.reason).await
}

/// Refuses the call, with a reason the model sees (an approver named by the
/// policy, or a workspace admin).
#[utoipa::path(post, path = "/approvals/{id}/deny", tag = "approvals", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Approval id")), request_body = Decide,
    responses((status = 200, body = Approval), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Already decided", body = Problem), (status = 422, body = Problem)))]
pub async fn deny(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<Decide>,
) -> Result<Json<Approval>, AppError> {
    decide(&state, auth, id, ApprovalStatus::Denied, &req.reason).await
}
